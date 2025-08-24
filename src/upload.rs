use std::{path::PathBuf, time::Duration};

use directories::ProjectDirs;
use miette::IntoDiagnostic;
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
        system::{GetSystemVersionReplyPacket, ProductType},
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

use vex_v5_serial::packets::{
    radio::{
        GetRadioStatusPacket, GetRadioStatusReplyPacket, RadioChannel, SelectRadioChannelPacket,
        SelectRadioChannelPayload, SelectRadioChannelReplyPacket,
    },
    system::{
        GetSystemFlagsPacket, GetSystemFlagsReplyPacket, GetSystemVersionPacket,
        GetSystemVersionReplyPayload,
    },
};

async fn is_connection_wireless(connection: &mut SerialConnection) -> Result<bool, CliError> {
    let version = connection
        .packet_handshake::<GetSystemVersionReplyPacket>(
            Duration::from_millis(500),
            1,
            GetSystemVersionPacket::new(()),
        )
        .await?;
    let system_flags = connection
        .packet_handshake::<GetSystemFlagsReplyPacket>(
            Duration::from_millis(500),
            1,
            GetSystemFlagsPacket::new(()),
        )
        .await?
        .try_into_inner()?;
    let controller = matches!(version.payload.product_type, ProductType::Controller);

    let tethered = system_flags.flags & (1 << 8) != 0;
    Ok(!tethered && controller)
}

async fn switch_radio_channel(
    conn: &mut SerialConnection,
    channel: RadioChannel,
) -> Result<(), CliError> {
    let radio_status = conn
        .packet_handshake::<GetRadioStatusReplyPacket>(
            Duration::from_secs(2),
            3,
            GetRadioStatusPacket::new(()),
        )
        .await?
        .try_into_inner()?;

    // Return early if already in download channel.
    // TODO: Make this also detect the bluetooth radio channel
    if (radio_status.channel == 5 && channel == RadioChannel::Download)
        || (radio_status.channel == 31 && channel == RadioChannel::Pit)
        || (radio_status.channel == -11)
    {
        return Ok(());
    }

    if is_connection_wireless(conn).await? {
        let channel_str = match channel {
            RadioChannel::Download => "download",
            RadioChannel::Pit => "pit",
        };

        // Tell the controller to switch to the download channel.
        conn.packet_handshake::<SelectRadioChannelReplyPacket>(
            Duration::from_secs(2),
            3,
            SelectRadioChannelPacket::new(SelectRadioChannelPayload { channel }),
        )
        .await?
        .try_into_inner()?;

        // Wait for the controller to disconnect by spamming it with a packet and waiting until that packet
        // doesn't go through. This indicates that the radio has actually started to switch channels.
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(8)) => {
                return Err(CliError::RadioChannelDisconnectTimeout)
            }
            _ = async {
                while conn
                    .packet_handshake::<GetRadioStatusReplyPacket>(
                        Duration::from_millis(250),
                        1,
                        GetRadioStatusPacket::new(())
                    )
                    .await
                    .is_ok()
                {
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
            } => {}
        }

        // Poll the connection of the controller to ensure the radio has switched channels by sending
        // test packets every 250ms for 8 seconds until we get a successful reply, indicating that the
        // controller has reconnected.
        //
        // If the controller doesn't a reply within 8 seconds, it hasn't reconnected correctly.
        conn.packet_handshake::<GetRadioStatusReplyPacket>(
            Duration::from_millis(250),
            32,
            GetRadioStatusPacket::new(()),
        )
        .await
        .map_err(|err| match err {
            SerialError::Timeout => CliError::RadioChannelReconnectTimeout,
            other => CliError::Serial(other),
        })?;
    }

    Ok(())
}

// I swear this wasn't vibe coded. I only added the superfluous amount of comments to make sure all
// the logic was correct.
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
    let rtbin_name = FixedString::new(format!("{rtbin}")).unwrap();
    let ini_name = FixedString::new(format!("slot_{}.ini", manifest.slot)).unwrap();

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

    // Build the package's VPT.
    let vpt = build(dir).await?;

    // Prepare the contents of the slot's INI configuration
    let ini_data = serde_ini::to_vec(&ProgramIniConfig {
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
    })
    .unwrap();

    // Join the connection task and get a connection so we can start interacting with the brain.
    let mut conn = conn_task.await.unwrap()?;

    switch_radio_channel(&mut conn, RadioChannel::Download).await?;

    // Other than the VPT, we need to upload two things:
    // - The Venice runtime
    // - The slot's INI configuration (slot_{n}.ini)
    let needs_ini_upload = brain_file_metadata(&mut conn, ini_name.clone())
        .await
        .map_err(CliError::Serial)?
        .is_none();
    let needs_rt_upload = brain_file_metadata(&mut conn, rtbin_name.clone())
        .await
        .map_err(CliError::Serial)?
        .is_none();

    let bin_string = FixedString::new(String::from("bin")).unwrap();

    if needs_ini_upload {
        // Upload the INI we prepared
        conn.execute_command(UploadFile {
            // Must be "slot_{n}.ini"
            filename: ini_name,
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
            data: ini_data,
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

    if needs_rt_upload {
        // Join the runtime fetch task and obtain the binary.
        let bin = bin_fetch_task.await.unwrap()?;

        // Start uploading the runtime which user programs will link to.
        conn.execute_command(UploadFile {
            filename: rtbin_name,
            metadata: FileMetadata {
                extension: bin_string.clone(),
                extension_type: ExtensionType::Binary,
                timestamp: j2000_timestamp(),
                version: vex_v5_serial::version::Version {
                    major: rtbin.version.major as u8,
                    minor: rtbin.version.minor as u8,
                    build: 0,
                    beta: 0,
                },
            },
            vendor: Some(FileVendor::User),
            data: bin,
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
        filename: FixedString::new(format!("slot_{}.bin", manifest.slot)).unwrap(),
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

    // The INI, runtime, and VPT have all been uploaded. Operation complete.
    Ok(())
}
