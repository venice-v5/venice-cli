pub const VENDOR_ID: u32 = 0x11235813;
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

use build::build;
use errors::CliError;
use manifest::{find_manifest, parse_manifest, prompt_for_slot, prompt_for_entrypoint, update_missing_config};
use run::run;
use terminal::terminal;
use upload::{open_connection, upload};
use runtime::RuntimeSource;

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
    /// Path to a raw runtime binary (dev builds only)
    #[cfg(debug_assertions)]
    #[arg(long = "raw-binary")]
    raw_binary: Option<PathBuf>,
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
}

fn clean(dir: Option<PathBuf>) -> miette::Result<()> {
    let manifest_dir = match dir {
        Some(dir) => dir,
        None => find_manifest(None)?.parent().unwrap().to_path_buf(),
    };

    std::fs::remove_dir_all(manifest_dir.join(BUILD_DIR)).map_err(CliError::Io)?;

    Ok(())
}

async fn ensure_project_config(dir: Option<PathBuf>) -> Result<(PathBuf, PathBuf), CliError> {
    let manifest_path = find_manifest(dir.as_deref())?;
    let project_dir = dir
        .as_deref()
        .unwrap_or_else(|| manifest_path.parent().unwrap());
    
    // Parse current manifest
    let project = parse_manifest(&manifest_path).await?;
    
    let mut needs_update = false;
    let mut slot_to_add = None;
    let mut entrypoint_to_add = None;
    
    // Check if slot is missing
    if project.slot.is_none() {
        slot_to_add = Some(prompt_for_slot()?);
        needs_update = true;
    }
    
    // Check if entrypoint is missing
    if project.entrypoint.is_none() {
        entrypoint_to_add = Some(prompt_for_entrypoint(project_dir)?);
        needs_update = true;
    }
    
    // Update manifest if needed
    if needs_update {
        update_missing_config(&manifest_path, slot_to_add, entrypoint_to_add).await?;
    }
    
    Ok((manifest_path.to_path_buf(), project_dir.to_path_buf()))
}

#[pyfunction]
#[pyo3(signature = (args, binary_path=None, version=None))]
fn call(args: Vec<String>, binary_path: Option<String>, version: Option<String>) -> PyResult<()> {
    let rt = Runtime::new().unwrap();
    let result: miette::Result<()> = rt.block_on(async {
        let cmd = Venice::try_parse_from(args);
        let cmd = match cmd {
            Ok(cmd) => cmd,
            Err(err) => {
                err.exit();
            }
        };

        // Determine the runtime source
        #[cfg(debug_assertions)]
        let runtime_source: Option<RuntimeSource> = if let Some(raw_binary) = cmd.raw_binary.clone() {
            // For raw binary mode, use version 0.1.0
            Some(RuntimeSource::new(raw_binary, semver::Version::new(0, 1, 0)))
        } else if let (Some(path), Some(ver)) = (binary_path, version) {
            ver.parse::<semver::Version>()
                .ok()
                .map(|v| RuntimeSource::new(PathBuf::from(path), v))
        } else {
            None
        };

        #[cfg(not(debug_assertions))]
        let runtime_source: Option<RuntimeSource> = if let (Some(path), Some(ver)) = (binary_path, version) {
            ver.parse::<semver::Version>()
                .ok()
                .map(|v| RuntimeSource::new(PathBuf::from(path), v))
        } else {
            None
        };

        let dir = cmd.dir;
        match cmd.subcmd {
            Subcommand::Build => {
                let _ = ensure_project_config(dir.clone()).await?;
                let _ = build(dir).await?;
            }
            Subcommand::Clean => clean(dir)?,
            Subcommand::Upload { after_upload } => {
                let _ = ensure_project_config(dir.clone()).await?;
                let _ = upload(dir, after_upload.map(|a| a.into()), runtime_source).await?;
            }
            Subcommand::Terminal => terminal(&mut open_connection().await?).await?,
            Subcommand::Run => {
                let _ = ensure_project_config(dir.clone()).await?;
                let _ = run(dir, runtime_source).await?;
            }
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
