use poise::serenity_prelude as serenity;

struct Data {} // User data, which is stored and accessible in all command invocations
type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Data, Error>;

/// Test a binary file
#[poise::command(slash_command, prefix_command)]
async fn test(
    ctx: Context<'_>,
    #[description = "Binary file (.bin)"] _binary: serenity::Attachment,
    #[description = "Program (.vpt)"] _vpt: serenity::Attachment,
) -> Result<(), Error> {
    if !["blood_bagel".to_string(), "fibonacci161803".to_string()].contains(&ctx.author().name) {
        ctx.say("SUCKER").await?;
    } else {
        todo!();
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
async fn main() {
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
    client.unwrap().start().await.unwrap();
}
