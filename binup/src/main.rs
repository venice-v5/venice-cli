use ::serenity::all::EditMessage;
use anyhow::{Result, anyhow};
use poise::serenity_prelude as serenity;
use shared::{errors::CliError, runtime::VPT_LOAD_ADDR};
use std::io::Read;
use std::time::Duration;
use std::time::Instant;
use tokio::{select, task::spawn_blocking, time::sleep};
use vex_v5_serial::{
    Connection as _,
    commands::file::{LinkedFile, USER_PROGRAM_LOAD_ADDR, UploadFile, j2000_timestamp},
    protocol::{
        FixedString, Version,
        cdc2::{
            Cdc2Ack,
            file::{
                ExtensionType, FileExitAction, FileMetadata, FileMetadataPacket,
                FileMetadataPayload, FileMetadataReplyPacket, FileTransferTarget, FileVendor,
            },
        },
    },
    serial::{self, SerialConnection},
};
struct Data {} // User data, which is stored and accessible in all command invocations
// type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Data, anyhow::Error>;

async fn open_connection() -> Result<SerialConnection, CliError> {
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

/*
 * General idea for how it should work:
 * Venice runtime binary & vpt are optional
 * Otherwise, the most recently uploaded (venice_v0.1.0.bin or wtv and
 *      slot_1.bin) are used by default
 * INI and VPT file is reuploaded at all times just to be safe
 * This does require running `venice upload` to set sensible defaults before
 *  starting the bot
 */
async fn upload_and_test(
    binary: Option<serenity::Attachment>,
    vpt: Option<serenity::Attachment>,
    ctx: &Context<'_>,
) -> Result<()> {
    let rtbin_name = FixedString::new("venice-v0.1.0.bin").unwrap();
    let bin_string = FixedString::new("bin").unwrap();
    let mut connection = open_connection().await?;
    // upload runtime bin
    if binary.is_some() {
        let rt_bytes = reqwest::get(&binary.expect("if this is logging then rust skibidi").url)
            .await?
            .bytes()
            .await?
            .to_vec();
        ctx.reply("uploading runtime... (est. 15 seconds)").await?;
        connection
            .execute_command(UploadFile {
                file_name: rtbin_name.clone(),
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
                data: &rt_bytes,
                target: FileTransferTarget::Qspi,
                load_address: USER_PROGRAM_LOAD_ADDR,
                linked_file: None,
                after_upload: FileExitAction::DoNothing,
                progress_callback: None,
            })
            .await?;
    } else {
        // make sure there is a runtime alr uploaded
        let reply = connection
            .handshake::<FileMetadataReplyPacket>(
                Duration::from_secs(1),
                2,
                FileMetadataPacket::new(FileMetadataPayload {
                    file_name: rtbin_name.clone(),
                    vendor: FileVendor::User,
                    reserved: 0,
                }),
            )
            .await?;

        match reply.ack() {
            Cdc2Ack::Ack => reply.payload.map_err(|_| anyhow!("NACK")),
            Cdc2Ack::NackProgramFile => Err(anyhow!(
                "there is no existing runtime binary on the brain. please rerun this command with a runtime binary"
            )),
            _nack => Err(anyhow!("NACK")),
        }?;
    }

    // upload INI & VPT
    {
        ctx.reply("uploading ini... (est. -1 seconds)").await?;
        let ini = "[project]
ide=Venice
[program]
name=Binup program
slot=1
icon=USER002x.bmp
iconalt=
description=Made in heaven!";
        connection
            .execute_command(UploadFile {
                file_name: FixedString::new("slot_1.ini").unwrap(),
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
                vendor: FileVendor::User,
                data: ini.as_bytes(),
                target: FileTransferTarget::Qspi,
                load_address: USER_PROGRAM_LOAD_ADDR,
                linked_file: None,
                after_upload: FileExitAction::DoNothing,
                progress_callback: None,
            })
            .await?;
    }
    {
        ctx.reply(if vpt.is_some() {
            "uploading vpt... (est. 1 second)"
        } else {
            "uploading vpt... (est. 1 second) (using stress test vpt)"
        })
        .await?;
        let vpt = if vpt.is_some() {
            reqwest::get(&vpt.expect("if this is logging then rust skibidi").url)
                .await?
                .bytes()
                .await?
                .to_vec()
        } else {
            // use stress test
            tokio::fs::read("out.vpt").await?
        };
        connection
            .execute_command(UploadFile {
                file_name: FixedString::new("slot_1.bin").unwrap(),
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
                after_upload: FileExitAction::RunProgram,
                progress_callback: None,
            })
            .await?;
    }
    let mut handle = ctx
        .channel_id()
        .say(&ctx.http(), "running, monitoring terminal output...")
        .await?;
    let time = Instant::now();
    let mut output = "".to_string();
    while time.elapsed().as_millis() < 10000 {
        // 10s
        let mut program_output = [0; 1024];
        select! {
            read = connection.read_user(&mut program_output) => {
                if let Ok(size) = read {
                    let mut bytes = &program_output[..size];
                    let mut s: String = "".to_string();
                    bytes.read_to_string(&mut s)?;
                    // because I'm not bothered to learn how to properly concat
                    // strings in rust
                    output = format!("{}{}", output, s);
                    handle.edit(&ctx.http(), EditMessage::default().content(format!("terminal output captured:\n```\n{output}```"))).await?;
                }
            }
            _ = sleep(Duration::from_millis(50)) => {/* timeout */}
        };
    }
    ctx.reply("done!").await?;
    Ok(())
}

/// Test a program given runtime and vpt
#[poise::command(slash_command, prefix_command)]
async fn test(
    ctx: Context<'_>,
    #[description = "Runtime (.bin)"] _binary: Option<serenity::Attachment>,
    #[description = "Program (.vpt)"] _vpt: Option<serenity::Attachment>,
) -> Result<()> {
    if !["blood_bagel".to_string(), "fibonacci161803".to_string()].contains(&ctx.author().name) {
        ctx.say("SUCKER").await?;
    } else {
        upload_and_test(_binary, _vpt, &ctx).await?;
        // // Download the attachment using async reqwest
        // // ctx.defer();
        // ctx.defer().await?;
        // ctx.channel_id().say(&ctx.http(), "downloading file [est. 3s]...").await?;
        // let runtime = reqwest::get(&binary.url).await?.bytes().await?.to_vec();
        // let table = reqwest::get(&vpt.url).await?.bytes().await?.to_vec();
        // ctx.channel_id().say(&ctx.http(), "connecting to brain...").await?;
        // let mut conn = serial::find_devices()?
        //     .first()
        //     .ok_or(anyhow!("no brain connected"))?
        //     .connect(Duration::from_secs(5))?;
        // ctx.channel_id().say(&ctx.http(), "uploading program to brain [est. 15s]...").await?;
        // let ini_name = FixedString::new(format!("slot_{}.ini", 1)).unwrap();
        // let rtbin_version = semver::Version::new(0, 1, 0);
        // let rtbin_name = FixedString::new(format!("{}", RtBin::from_version(rtbin_version.clone()))).unwrap();
        // shared::upload::upload_inner(
        //     // conn
        //     &mut conn,
        //     // config
        //     Some((ini_name, &ProgramIniConfig {
        //         program: Program {
        //             name: "binup".to_string(),
        //             description: String::from("Made in Heaven!"),
        //             icon: format!("USER{:03}x.bmp", ProgramIcon::Alien as u16),
        //             iconalt: String::new(),
        //             slot: 0,
        //         },
        //         project: Project {
        //             ide: String::from("Venice"),
        //         },
        //     })),
        //     // runtime
        //     Some((
        //         rtbin_name,
        //         runtime,
        //         rtbin_version
        //     )),
        //     // vpt
        //     table,
        //     // slot
        //     1,
        //     Some(FileExitAction::RunProgram)
        // ).await?;
        // todo!();
        // ctx.channel_id().say(&ctx.http(), "running program & monitoring terminal...").await?;
        // ctx.say("running program & monitoring terminal...").await?;
        // loop { // 10s
        //     let mut program_output = [0; 1024];
        //     select! {
        //         read = conn.read_user(&mut program_output) => {
        //             if let Ok(size) = read {
        //                 let mut bytes = &program_output[..size];
        //                 let mut s: String = "".to_string();
        //                 bytes.read_to_string(&mut s)?;
        //                 if s.contains('\n') {
        //                     ctx.say(format!("```\n{s}```")).await?;
        //                 } else {
        //                     ctx.say(format!("`{s}`")).await?;
        //                 }
        //             }
        //         }
        //         _ = sleep(Duration::from_millis(50)) => {/* timeout */}
        //     };
        // }
    };
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv()?;

    let token = std::env::var("DISCORD_TOKEN").expect("missing DISCORD_TOKEN");
    let intents = serenity::GatewayIntents::non_privileged();

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![test()],
            ..Default::default()
        })
        .setup(|ctx, _ready, framework| {
            Box::pin(async move {
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                Ok(Data {})
            })
        })
        .build();

    let client = serenity::ClientBuilder::new(token, intents)
        .framework(framework)
        .await;
    client.unwrap().start().await?;
    Ok(())
}
