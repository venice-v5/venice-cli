use std::path::PathBuf;

use clap::Parser;
use shared::{
    BUILD_DIR,
    build::build,
    errors::CliError,
    manifest::find_manifest,
    runtime,
    terminal::terminal,
    upload::{open_connection, upload},
};

#[derive(clap::Parser)]
#[command(version)]
enum Venice {
    Build {
        #[arg(long = "directory", short = 'C')]
        dir: Option<PathBuf>,
    },
    Clean {
        #[arg(long = "directory", short = 'C')]
        dir: Option<PathBuf>,
    },
    Upload {
        #[arg(long = "directory", short = 'C')]
        dir: Option<PathBuf>,
    },
    Terminal,
    Update,
}

fn clean(dir: Option<PathBuf>) -> miette::Result<()> {
    let manifest_dir = match dir {
        Some(dir) => dir,
        None => find_manifest(None)?.parent().unwrap().to_path_buf(),
    };

    std::fs::remove_dir_all(manifest_dir.join(BUILD_DIR)).map_err(CliError::Io)?;

    Ok(())
}

#[tokio::main]
async fn main() -> miette::Result<()> {
    let cmd = Venice::parse();
    let _ = runtime::latest_version(&reqwest::Client::new()).await;

    match cmd {
        Venice::Build { dir } => {
            let _ = build(dir).await?;
        }
        Venice::Clean { dir } => clean(dir)?,
        Venice::Upload { dir } => upload(dir).await?,
        Venice::Terminal => terminal(&mut open_connection().await?).await?,
        Venice::Update => todo!(),
    };

    Ok(())
}
