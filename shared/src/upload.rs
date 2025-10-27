use std::{path::PathBuf, time::Duration};

use directories::ProjectDirs;
use tokio::task::spawn_blocking;
use vex_v5_serial::{
    Connection,
    commands::file::{LinkedFile, USER_PROGRAM_LOAD_ADDR, UploadFile, j2000_timestamp},
    protocol::{
        FixedString, VEX_CRC32, Version,
        cdc2::{
            Cdc2Ack,
            file::{
                ExtensionType, FileExitAction, FileMetadata, FileMetadataPacket,
                FileMetadataPayload, FileMetadataReplyPacket, FileMetadataReplyPayload,
                FileTransferTarget, FileVendor,
            },
        },
    },
    serial::{self, SerialConnection, SerialError},
};

use crate::{
    build::build,
    errors::CliError,
    manifest::{Manifest, find_manifest},
    runtime::{RtBin, VPT_LOAD_ADDR},
};

pub async fn open_connection() -> Result<SerialConnection, CliError> {
    let devices = serial::find_devices()?;

    spawn_blocking(move || {
        Ok(devices
            .first()
            .ok_or(CliError::NoDevice)?
            .connect(Duration::from_secs(5))?)
    })
    .await
    .unwrap()
}

/// # Errors
///
/// - Returns Err(e) if a serial error occurred.
/// - Returns Ok(None) if there is no metadata associated with the file.
pub async fn brain_file_metadata(
    conn: &mut SerialConnection,
    name: FixedString<23>,
) -> Result<Option<FileMetadataReplyPayload>, SerialError> {
    let reply = conn
        .handshake::<FileMetadataReplyPacket>(
            Duration::from_secs(1),
            2,
            FileMetadataPacket::new(FileMetadataPayload {
                file_name: name,
                vendor: FileVendor::User,
                reserved: 0,
            }),
        )
        .await?;

    match reply.ack() {
        Cdc2Ack::Ack => reply.payload.map_err(SerialError::Nack),
        Cdc2Ack::NackProgramFile => Ok(None),
        nack => Err(SerialError::Nack(nack)),
    }
}

fn ini_config(name: &str, slot: u8, icon: u16, description: &str) -> String {
    format!(
        "[project]\
    \r\nide=Venice\
    \r\n[program]\
    \r\nname={name}\
    \r\nslot={slot}\
    \r\nicon=USER{icon:03}x.bmp\
    \r\niconalt=\
    \r\ndescription={description}\r\n",
    )
}

// I swear this wasn't vibe coded. I only added the superfluous amount of comments to make sure all
// the logic was correct.
// I believe you -- aadish 2025-08-23
pub async fn upload(
    dir: Option<PathBuf>,
    after_upload: Option<FileExitAction>,
) -> Result<SerialConnection, CliError> {
    let bin_string = FixedString::new(String::from("bin")).unwrap();

    // background opening a serial conn
    let conn_task = tokio::spawn(open_connection());

    // read and parse manifest
    let manifest_path = find_manifest(dir.as_deref())?;
    let manifest = toml::from_str::<Manifest>(&tokio::fs::read_to_string(&manifest_path).await?)?;

    if !(1..=8).contains(&manifest.slot) {
        return Err(CliError::SlotOutOfRange);
    }

    let rtbin = RtBin::from_version(manifest.venice_version.parse::<semver::Version>()?);

    let config = ini_config(
        &manifest.name,
        manifest.slot - 1,
        manifest.icon as u16,
        manifest.description.as_deref().unwrap_or("Made in Heaven!"),
    );

    let mut conn = conn_task.await.unwrap()?;
    let ini_name = FixedString::new(format!("slot_{}.ini", manifest.slot)).unwrap();
    let metadata = brain_file_metadata(&mut conn, ini_name.clone()).await?;

    let reupload_ini = match metadata {
        None => true,
        Some(data) => data.crc32 != VEX_CRC32.checksum(config.as_bytes()),
    };

    if reupload_ini {
        conn.execute_command(UploadFile {
            // Must be "slot_{n}.ini"
            file_name: ini_name,
            metadata: FileMetadata {
                extension: FixedString::new(String::from("ini")).unwrap(),
                extension_type: ExtensionType::Binary,
                timestamp: j2000_timestamp(),
                version: Version {
                    major: 0,
                    minor: 1,
                    build: 0,
                    beta: 0,
                },
            },
            // Third party vendors like Venice use FileVendor::User
            vendor: FileVendor::User,
            data: config.as_bytes(),
            target: FileTransferTarget::Qspi,
            load_address: USER_PROGRAM_LOAD_ADDR,
            linked_file: None,
            after_upload: FileExitAction::DoNothing,
            // TODO?: add progress indicator
            progress_callback: Some(Box::new(|f| println!("Uploading ini {}", f))),
        })
        .await?;
    }

    // Four-stage process to determine whether the rt should be uploaded:
    // 1. check if rt is available by trying to fetch it from brain
    // 2. if it is not available, check if it is available on user's system
    // 3. if it isn't, download it from github
    // 4. upload the rt if its not on the brain
    let rtbin_name = FixedString::new(format!("{rtbin}")).unwrap();
    let rt_metadata = brain_file_metadata(&mut conn, rtbin_name.clone()).await?;

    let reupload_rt = rt_metadata.is_none();

    if reupload_rt {
        let project_dir =
            ProjectDirs::from("org", "venice", "venice-cli").ok_or(CliError::HomeDirNotFound)?;
        let cache_dir = project_dir.cache_dir();
        let contents = rtbin.fetch(cache_dir).await?;
        conn.execute_command(UploadFile {
            file_name: rtbin_name.clone(),
            metadata: FileMetadata {
                extension: bin_string.clone(),
                extension_type: ExtensionType::Binary,
                timestamp: j2000_timestamp(),
                version: Version {
                    major: rtbin.version.major as u8,
                    minor: rtbin.version.minor as u8,
                    build: 0,
                    beta: 0,
                },
            },
            vendor: FileVendor::User,
            data: &contents,
            target: FileTransferTarget::Qspi,
            // This is the main load address for V5 programs.
            load_address: USER_PROGRAM_LOAD_ADDR,
            linked_file: None,
            after_upload: FileExitAction::DoNothing,
            progress_callback: Some(Box::new(|f| println!("Uploading runtime {}", f))),
        })
        .await?;
    }

    let vpt = build(dir).await?;
    conn.execute_command(UploadFile {
        // It's not technically a binary, but I believe it must still be named this way.
        file_name: FixedString::new(format!("slot_{}.bin", manifest.slot)).unwrap(),
        metadata: FileMetadata {
            extension: bin_string.clone(),
            extension_type: ExtensionType::Binary,
            timestamp: j2000_timestamp(),
            version: Version {
                major: 0,
                minor: 1,
                build: 0,
                beta: 0,
            },
        },
        vendor: FileVendor::User,
        data: &vpt,
        linked_file: Some(LinkedFile {
            file_name: rtbin_name.clone(),
            vendor: FileVendor::User,
        }),
        load_address: VPT_LOAD_ADDR,
        target: FileTransferTarget::Qspi,
        after_upload: after_upload.unwrap_or(FileExitAction::ShowRunScreen),
        progress_callback: Some(Box::new(|f| println!("Uploading VPT {}", f))),
    })
    .await?;
    Ok(conn)
}
