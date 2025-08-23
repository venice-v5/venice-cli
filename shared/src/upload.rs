use std::{path::PathBuf, time::Duration};

use directories::ProjectDirs;
use miette::IntoDiagnostic;
use semver::Version;
use tokio::task::spawn_blocking;
use vex_v5_serial::{
    commands::file::{Program, ProgramIniConfig, Project, USER_PROGRAM_LOAD_ADDR, UploadFile},
    connection::{
        Connection,
        serial::{self, SerialConnection, SerialError},
    },
    packets::{
        cdc2::Cdc2Ack,
        file::{
            ExtensionType, FileExitAction, FileMetadata, FileVendor, GetDirectoryEntryPacket,
            GetDirectoryEntryPayload, GetDirectoryEntryReplyPacket, GetDirectoryFileCountPacket,
            GetDirectoryFileCountPayload, GetDirectoryFileCountReplyPacket, GetFileMetadataPacket,
            GetFileMetadataPayload, GetFileMetadataReplyPacket, GetFileMetadataReplyPayload,
        },
    },
    string::FixedString,
    timestamp::j2000_timestamp,
};

use crate::{
    build::build,
    errors::CliError,
    manifest::{Manifest, find_manifest},
    runtime::{RtBin, VPT_LOAD_ADDR, fetch},
};

pub async fn open_connection() -> miette::Result<SerialConnection> {
    let devices = serial::find_devices().map_err(CliError::Serial)?;

    spawn_blocking(move || {
        Ok(devices
            .first()
            .ok_or(CliError::NoDevice)?
            .connect(Duration::from_secs(5))
            .map_err(CliError::Serial)?)
    })
    .await
    .unwrap()
}

pub async fn brain_file_metadata(
    conn: &mut SerialConnection,
    name: FixedString<23>,
) -> Result<Option<GetFileMetadataReplyPayload>, SerialError> {
    let reply = conn
        .packet_handshake::<GetFileMetadataReplyPacket>(
            Duration::from_secs(1),
            2,
            GetFileMetadataPacket::new(GetFileMetadataPayload {
                file_name: name,
                vendor: FileVendor::User,
                option: 0,
            }),
        )
        .await?;

    match reply.ack {
        Cdc2Ack::Ack => Ok(reply.payload),
        Cdc2Ack::NackProgramFile => Ok(None),
        nack => Err(SerialError::Nack(nack)),
    }
}

// TODO: add list command
pub async fn uploaded_rts(conn: &mut SerialConnection) -> Result<Vec<RtBin>, SerialError> {
    let count = conn
        .packet_handshake::<GetDirectoryFileCountReplyPacket>(
            Duration::from_millis(200),
            2,
            GetDirectoryFileCountPacket::new(GetDirectoryFileCountPayload {
                vendor: FileVendor::User,
                option: 0,
            }),
        )
        .await?;

    let mut rts = Vec::new();
    for i in 0..count.payload {
        let result = conn
            .packet_handshake::<GetDirectoryEntryReplyPacket>(
                Duration::from_millis(200),
                2,
                GetDirectoryEntryPacket::new(GetDirectoryEntryPayload {
                    file_index: i as u8,
                    unknown: 0,
                }),
            )
            .await?;

        if let Some(payload) = result.payload
            && let Ok(bin) = payload.file_name.parse()
        {
            rts.push(bin);
        }
    }

    Ok(rts)
}

// I swear this wasn't vibe coded. I only added the superfluous amount of comments to make sure all
// the logic was correct.
// I believe you -- aadish 2025-08-23
pub async fn upload(dir: Option<PathBuf>) -> miette::Result<()> {
    // Open a serial connection in the background while we build and prepare for uploading.
    let conn_task = tokio::spawn(open_connection());

    // Search for the program's manifest in this directory and any higher directories if it wasn't
    // provided explicity.
    let manifest_path = find_manifest(dir.as_deref())?;
    // Read the manifest and parse it, propogating any errors.
    let manifest = toml::from_str::<Manifest>(
        &tokio::fs::read_to_string(&manifest_path)
            .await
            .map_err(CliError::Io)?,
    )
    .map_err(CliError::Manifest)?;

    if !(1..=8).contains(&manifest.slot) {
        return Err(CliError::SlotOutOfRange.into());
    }

    // Parse the Venice semver version specified in the manifest.
    let venice_version = manifest
        .venice_version
        .parse::<semver::Version>()
        .into_diagnostic()?;

    // Strings needed for uploading
    let rtbin = RtBin::from_version(venice_version);

    // Start downloading the specified version of Venice in the background, or grab it from the
    // runtime cache.
    // TODO: Maybe it's better to refrain from potentially needless downloading, at the expense of
    // concurrency?
    let rtbin_clone = rtbin.clone();
    let bin_fetch_task = tokio::spawn(async move {
        // TODO: propogate error upwards instead of unwrapping
        let project_dirs = ProjectDirs::from("org", "venice", "venice-cli").unwrap();
        fetch(&rtbin_clone, project_dirs.cache_dir()).await
    });

    // Other than the VPT, we need to upload two things:
    // - The Venice runtime
    // - The slot's INI configuration (slot_{n}.ini)
    let mut conn = conn_task.await.unwrap()?;
    let ini_name = FixedString::new(format!("slot_{}.ini", manifest.slot)).unwrap();
    let rtbin_name = FixedString::new(format!("{rtbin}")).unwrap();
    let needs_ini_upload = brain_file_metadata(&mut conn, ini_name.clone())
        .await
        .map_err(CliError::Serial)?
        .is_none();
    let needs_rt_upload = brain_file_metadata(&mut conn, rtbin_name.clone())
        .await
        .map_err(CliError::Serial)?
        .is_none();
    let config = &ProgramIniConfig {
        program: Program {
            name: manifest.name,
            // TODO: add description from Venice.toml
            description: String::from("Made in Heaven!"),
            icon: format!("USER{:03}x.bmp", manifest.icon as u16),
            iconalt: String::new(),
            slot: manifest.slot - 1,
        },
        project: Project {
            ide: String::from("Venice"),
        },
    };
    upload_inner(
        // conn
        &mut conn,
        // config
        if needs_ini_upload { Some(
            (
                ini_name,
                config
            )
        ) } else {
            None
        },
        // runtime
        if needs_rt_upload { Some((
            rtbin_name,
            bin_fetch_task.await.unwrap()?,
            rtbin.version.clone(),
        )) } else {
            None
        },
        // vpt
        build(dir).await?,
        // slot
        manifest.slot,
    ).await
}

pub async fn upload_inner(
    conn: &mut SerialConnection,
    // metadata ini -- is optionally uploaded
    config: Option<(FixedString<23>, &ProgramIniConfig)>,
    // runtime bin -- is optionally uploaded
    // bad things happen if you don't upload this at the right time!
    runtime: Option<(FixedString<23>, Vec<u8>, Version)>,
    // vpt bytes
    vpt: Vec<u8>,
    // slot #,
    slot: u8,
) -> miette::Result<()> {
    let bin_string = FixedString::new(String::from("bin")).unwrap();

    if let Some((name, config)) = config {
        // Upload the INI we prepared
        conn.execute_command(UploadFile {
            // Must be "slot_{n}.ini"
            filename: name,
            metadata: FileMetadata {
                extension: FixedString::new(String::from("ini")).unwrap(),
                // ExtensionType::EncryptedBinary if we were encrypting.
                extension_type: ExtensionType::Binary,
                // VEX uses J2000 (Jan. 2000) timestamps.
                timestamp: j2000_timestamp(),
                // TODO: add version from manifest
                version: vex_v5_serial::version::Version {
                    major: 0,
                    minor: 1,
                    build: 0,
                    beta: 0,
                },
            },
            // Third party vendors like Venice use FileVendor::User
            vendor: Some(FileVendor::User),
            data: serde_ini::to_vec(config).unwrap(),
            target: None,
            // Don't know why this would be significant for INIs.
            load_addr: USER_PROGRAM_LOAD_ADDR,
            linked_file: None,
            after_upload: FileExitAction::DoNothing,
            // TODO?: add progress indicator
            progress_callback: None,
        })
        .await
        .map_err(CliError::Serial)?;
    }

    if let Some((rtbin_name, rtbin, version)) = runtime {
        // Start uploading the runtime which user programs will link to.
        conn.execute_command(UploadFile {
            filename: rtbin_name,
            metadata: FileMetadata {
                extension: bin_string.clone(),
                extension_type: ExtensionType::Binary,
                timestamp: j2000_timestamp(),
                version: vex_v5_serial::version::Version {
                    major: version.major as u8,
                    minor: version.minor as u8,
                    build: 0,
                    beta: 0,
                },
            },
            vendor: Some(FileVendor::User),
            data: rtbin,
            target: None,
            // This is the main load address for V5 programs.
            load_addr: USER_PROGRAM_LOAD_ADDR,
            linked_file: None,
            after_upload: FileExitAction::ShowRunScreen,
            progress_callback: None,
        })
        .await
        .map_err(CliError::Serial)?;
    }

    // Upload the VPT.
    conn.execute_command(UploadFile {
        // It's not technically a binary, but I believe it must still be named this way.
        filename: FixedString::new(format!("slot_{}.bin", slot)).unwrap(),
        metadata: FileMetadata {
            extension: bin_string,
            extension_type: ExtensionType::Binary,
            timestamp: j2000_timestamp(),
            // TODO: add version from manifest
            version: vex_v5_serial::version::Version {
                major: 0,
                minor: 1,
                build: 0,
                beta: 0,
            },
        },
        vendor: Some(FileVendor::User),
        data: vpt,
        target: None,
        load_addr: VPT_LOAD_ADDR,
        linked_file: None,
        // Show the slot's run screen after uploading.
        // TODO: add CLI option to choose after upload behavior instead of hard-coding it
        after_upload: FileExitAction::ShowRunScreen,
        progress_callback: None,
    })
    .await
    .map_err(CliError::Serial)?;

    Ok(())
}
