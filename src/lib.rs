pub const VENDOR_ID: u32 = 0x11235813;
pub const SRC_DIR: &str = "src";
pub const BUILD_DIR: &str = "build";
pub const TABLE_FILE: &str = "out.vpt";

pub mod build;
pub mod errors;
pub mod manifest;
pub mod run;
pub mod runtime;
pub mod terminal;
pub mod upload;

use clap::Parser;
use pyo3::prelude::*;
use tokio::runtime::Runtime;

use std::path::PathBuf;

use miette::IntoDiagnostic;

use build::build;
use errors::CliError;
use manifest::{MANIFEST_NAME, find_manifest};
use run::run;
use runtime::latest_version;
use terminal::terminal;
use upload::{open_connection, upload};

use toml_edit::{DocumentMut, Formatted, Item, Value};
use vex_v5_serial::protocol::cdc2::file::FileExitAction;

#[derive(Debug, Clone, clap::ValueEnum)]
enum AfterUpload {
    Halt,
    DoNothing,
    ShowRunScreen,
    RunProgram,
}

impl From<AfterUpload> for FileExitAction {
    fn from(value: AfterUpload) -> Self {
        match value {
            AfterUpload::Halt => Self::Halt,
            AfterUpload::DoNothing => Self::DoNothing,
            AfterUpload::ShowRunScreen => Self::ShowRunScreen,
            AfterUpload::RunProgram => Self::RunProgram,
        }
    }
}

#[derive(clap::Parser)]
#[command(version)]
struct Venice {
    #[arg(long = "directory", short = 'C')]
    dir: Option<PathBuf>,
    #[command(subcommand)]
    subcmd: Subcommand,
}

#[derive(Clone, clap::Subcommand)]
enum Subcommand {
    Build,
    Clean,
    Upload { after_upload: Option<AfterUpload> },
    Terminal,
    Run,
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

async fn update(dir: Option<PathBuf>) -> miette::Result<()> {
    let manifest_path = match dir {
        Some(dir) => dir.join(MANIFEST_NAME),
        None => find_manifest(None)?,
    };

    let manifest_src = tokio::fs::read_to_string(&manifest_path)
        .await
        .map_err(CliError::Io)?;
    let mut doc = manifest_src.parse::<DocumentMut>().into_diagnostic()?;
    let latest_version = latest_version(&reqwest::Client::new()).await?;
    let version_string = latest_version.to_string();

    doc["project"]["venice-version"] = Item::Value(Value::String(Formatted::new(version_string)));

    tokio::fs::write(manifest_path, doc.to_string())
        .await
        .map_err(CliError::Io)?;

    Ok(())
}

pub async fn run_cli() -> miette::Result<()> {
    let cmd = Venice::parse();
    let _ = runtime::latest_version(&reqwest::Client::new()).await;

    let dir = cmd.dir;
    match cmd.subcmd {
        Subcommand::Build => {
            let _ = build(dir).await?;
        }
        Subcommand::Clean => clean(dir)?,
        Subcommand::Upload { after_upload } => {
            let _ = upload(dir, after_upload.map(|a| a.into())).await?;
        }
        Subcommand::Terminal => terminal(&mut open_connection().await?).await?,
        Subcommand::Run => run(dir).await?,
        Subcommand::Update => update(dir).await?,
    };

    Ok(())
}

#[pyfunction]
fn call(args: Vec<String>) -> PyResult<()> {
    let rt = Runtime::new().unwrap();
    let result: miette::Result<()> = rt.block_on(async {
        let cmd = Venice::try_parse_from(args);
        let cmd = match cmd {
            Ok(cmd) => cmd,
            Err(err) => {
                err.exit();
            }
        };

        let _ = runtime::latest_version(&reqwest::Client::new()).await;

        let dir = cmd.dir;
        match cmd.subcmd {
            Subcommand::Build => {
                let _ = build(dir).await?;
            }
            Subcommand::Clean => clean(dir)?,
            Subcommand::Upload { after_upload } => {
                let _ = upload(dir, after_upload.map(|a| a.into())).await?;
            }
            Subcommand::Terminal => terminal(&mut open_connection().await?).await?,
            Subcommand::Run => run(dir).await?,
            Subcommand::Update => update(dir).await?,
        };
        Ok(())
    });
    let _ = result.map_err(|e| {
        eprint!("{:?}", e);
        std::process::exit(1);
    });
    Ok(())
}

/// This function defines the Python module.
/// The name `_core` must match the last part of the
/// `module-name` you set in pyproject.toml.
#[pymodule]
fn _core(_py: Python, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(call, m)?)?;
    Ok(())
}
