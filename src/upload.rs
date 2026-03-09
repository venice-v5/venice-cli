use std::io::Write;
use std::time::Duration;

use flate2::{Compression, GzBuilder};
use indicatif::{ProgressBar, ProgressStyle};
use tokio::task::spawn_blocking;
use vex_v5_serial::{
    Connection,
    commands::file::{LinkedFile, USER_PROGRAM_LOAD_ADDR, UploadFile, j2000_timestamp},
    protocol::{
        FixedString, Version,
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
    manifest::get_project,
    runtime::{RuntimeSource, VPT_LOAD_ADDR},
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

fn gzip_compress(data: &mut Vec<u8>) {
    let mut encoder = GzBuilder::new().write(Vec::new(), Compression::best());
    encoder.write_all(data).unwrap();
    *data = encoder.finish().unwrap();
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

fn create_upload_progress_bar(message: &str) -> ProgressBar {
    let pb = ProgressBar::new(100);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("[{elapsed_precise}] {bar:40.cyan/blue} {pos:>3}% {msg}")
            .unwrap()
            .progress_chars("##-"),
    );
    pb.set_message(message.to_string());
    pb
}

// I swear this wasn't vibe coded. I only added the superfluous amount of comments to make sure all
// the logic was correct.
// I believe you -- aadish 2025-08-23
pub async fn upload(
    after_upload: Option<FileExitAction>,
    runtime_source: Option<RuntimeSource>,
    _force_reupload_runtime: bool,
) -> Result<SerialConnection, CliError> {
    let bin_string = FixedString::new(String::from("bin")).unwrap();

    // background opening a serial conn
    let conn_task = tokio::spawn(open_connection());

    // read and parse manifest
    let manifest = get_project().await?;

    if let Some(slot) = manifest.slot {
        if !(1..=8).contains(&slot) {
            return Err(CliError::SlotOutOfRange);
        }
    } else {
        return Err(CliError::SlotOutOfRange); // This shouldn't happen if ensure_project_config worked
    }

    // Get the runtime source or error if none provided
    let runtime_source = runtime_source.ok_or(CliError::NoRuntimeSource)?;
    let rtbin = runtime_source.as_rtbin();
    let mut runtime_contents = runtime_source.read_binary().await?;
    // <https://media1.tenor.com/m/cjSTJh8J3QcAAAAd/cat-cat-sink.gif>
    gzip_compress(&mut runtime_contents);

    let config = ini_config(
        &manifest.name,
        manifest.slot.unwrap_or(1), // Default to 1 if somehow missing
        manifest.icon as u16,
        manifest.description.as_deref().unwrap_or("Made in Heaven!"),
    );

    let mut conn = conn_task.await.unwrap()?;
    let ini_name = FixedString::new(format!("slot_{}.ini", manifest.slot.unwrap_or(1))).unwrap();

    let ini_pb = create_upload_progress_bar("Uploading ini");
    let ini_pb_clone = ini_pb.clone();
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
        progress_callback: Some(Box::new(move |progress| {
            ini_pb_clone.set_position(progress as u64);
        })),
    })
    .await?;
    ini_pb.finish_with_message("Uploading ini - done");

    // Check if the runtime is already on the brain; if not, upload it
    let rtbin_name = FixedString::new(format!("{rtbin}")).unwrap();
    let rt_metadata = brain_file_metadata(&mut conn, rtbin_name.clone()).await?;

    let reupload_rt = rt_metadata.is_none();

    if reupload_rt {
        let rt_pb = create_upload_progress_bar("Uploading runtime");
        let rt_pb_clone = rt_pb.clone();
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
            data: &runtime_contents,
            target: FileTransferTarget::Qspi,
            // This is the main load address for V5 programs.
            load_address: USER_PROGRAM_LOAD_ADDR,
            linked_file: None,
            after_upload: FileExitAction::DoNothing,
            progress_callback: Some(Box::new(move |progress| {
                rt_pb_clone.set_position(progress as u64);
            })),
        })
        .await?;
        rt_pb.finish_with_message("Uploading runtime - done");
    }

    let vpt = build().await?;

    let vpt_pb = create_upload_progress_bar("Uploading VPT");
    let vpt_pb_clone = vpt_pb.clone();
    conn.execute_command(UploadFile {
        // It's not technically a binary, but I believe it must still be named this way.
        file_name: FixedString::new(format!("slot_{}.bin", manifest.slot.unwrap_or(1))).unwrap(),
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
        progress_callback: Some(Box::new(move |progress| {
            vpt_pb_clone.set_position(progress as u64);
        })),
    })
    .await?;
    vpt_pb.finish_with_message("Uploading VPT - done");
    Ok(conn)
}
