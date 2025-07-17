mod build;
mod errors;
mod manifest;
mod runtime;

use std::path::{Path, PathBuf};

use clap::Parser;
use directories::ProjectDirs;
use miette::{Context, IntoDiagnostic, miette};
use reqwest::Client;

use crate::{
    build::{Table, build_modules, find_modules},
    errors::CliError,
    manifest::{Manifest, find_manifest},
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
    Update,
}

const SRC_DIR: &str = "src";
const BUILD_DIR: &str = "build";
const TABLE_FILE: &str = "out.vbt";

fn build(dir: Option<PathBuf>) -> miette::Result<()> {
    let manifest_path = find_manifest(dir.as_deref())?;
    let manifest_dir = dir
        .as_deref()
        .unwrap_or_else(|| manifest_path.parent().unwrap());

    let src_dir = manifest_dir.join(SRC_DIR);
    let build_dir = manifest_dir.join(BUILD_DIR);

    let modules = find_modules(&src_dir)
        .map_err(CliError::Io)
        .wrap_err("couldn't find source modules")?;

    if !std::fs::exists(&build_dir).into_diagnostic()? {
        std::fs::create_dir(&build_dir).into_diagnostic()?;
    }

    let rebuild_table =
        build_modules(&src_dir, &build_dir, &modules).wrap_err("couldn't build source modules")?;

    if rebuild_table {
        let table_path = build_dir.join(TABLE_FILE);

        Table::generate(&build_dir, &modules)
            .into_diagnostic()
            .wrap_err("couldn't generate bytecode table")?
            .write_to_file(&table_path)
            .into_diagnostic()
            .wrap_err("couldn't write bytecode table to file")?;
    }

    Ok(())
}

fn clean(dir: Option<PathBuf>) -> miette::Result<()> {
    let manifest_dir = match dir {
        Some(dir) => dir,
        None => find_manifest(None)?.parent().unwrap().to_path_buf(),
    };

    std::fs::remove_dir_all(manifest_dir.join("build")).map_err(CliError::Io)?;

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

async fn upload(dir: Option<PathBuf>) -> miette::Result<()> {
    let manifest_path = find_manifest(dir.as_deref())?;
    let _manifest =
        toml::from_str::<Manifest>(&std::fs::read_to_string(&manifest_path).map_err(CliError::Io)?)
            .map_err(CliError::Manifest)?;

    build(dir)?;

    todo!();
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
