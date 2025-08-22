mod build;
mod errors;
mod manifest;
mod runtime;
mod upload;

use std::path::{Path, PathBuf};

use clap::Parser;
use directories::ProjectDirs;
use miette::miette;
use reqwest::Client;

use crate::{build::build, errors::CliError, manifest::find_manifest, upload::upload};

const VENDOR_ID: u32 = 0x11235813;

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
    Update,
}

const SRC_DIR: &str = "src";
const BUILD_DIR: &str = "build";
const TABLE_FILE: &str = "out.vpt";

fn clean(dir: Option<PathBuf>) -> miette::Result<()> {
    let manifest_dir = match dir {
        Some(dir) => dir,
        None => find_manifest(None)?.parent().unwrap().to_path_buf(),
    };

    std::fs::remove_dir_all(manifest_dir.join(BUILD_DIR)).map_err(CliError::Io)?;

    Ok(())
}

fn project_dirs() -> miette::Result<ProjectDirs> {
    directories::ProjectDirs::from("org", "venice", "venice-cli")
        .ok_or(miette!("couldn't get data dir"))
}

async fn data_dir(project_dirs: &ProjectDirs) -> Result<&Path, std::io::Error> {
    let data_dir = project_dirs.data_dir();

    tokio::fs::create_dir_all(&data_dir).await?;

    Ok(data_dir)
}

async fn update() -> miette::Result<(bool, runtime::Version)> {
    let project_dirs = project_dirs()?;
    let data_dir = data_dir(&project_dirs).await.map_err(CliError::Io)?;

    let client = Client::new();
    let latest_version = runtime::latest_version(&client).await?;

    if !runtime::version_exists(latest_version, data_dir)
        .await
        .map_err(CliError::Io)?
    {
        runtime::download(latest_version, data_dir).await?;
        Ok((true, latest_version))
    } else {
        Ok((false, latest_version))
    }
}

#[tokio::main]
async fn main() -> miette::Result<()> {
    let cmd = Venice::parse();
    let _ = runtime::latest_version(&reqwest::Client::new()).await;

    match cmd {
        Venice::Build { dir } => build(dir),
        Venice::Clean { dir } => clean(dir),
        Venice::Update => {
            let (updated, latest_version) = update().await?;
            if updated {
                println!("updated to Venice {latest_version}");
            } else {
                println!("already up to date ({latest_version})");
            }
            Ok(())
        }
        Venice::Upload { dir } => upload(dir).await,
    }
}
