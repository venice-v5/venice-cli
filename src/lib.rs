pub const VENDOR_ID: u32 = 0x11235813;
pub const SRC_DIR: &str = "src";
pub const BUILD_DIR: &str = "build";
pub const TABLE_FILE: &str = "out.vpt";

pub mod build;
pub mod errors;
pub mod manifest;
pub mod runtime;
pub mod terminal;
pub mod upload;

use clap::Parser;
use pyo3::prelude::*;
use tokio::runtime::Runtime;

use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

use build::build;
use errors::CliError;
use manifest::{get_project, prompt_for_slot, resolve_project_dir, update_missing_config, MANIFEST_NAME};
use runtime::RuntimeSource;
use terminal::terminal;
use upload::{open_connection, upload};

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
    Upload {
        after_upload: Option<AfterUpload>,
        #[arg(long, short, action = clap::ArgAction::SetTrue)]
        cold: bool,
    },
    Terminal,
    Run {
        cold: bool,
    },
}

fn clean() -> miette::Result<()> {
    std::fs::remove_dir_all(project_dir()?.join(BUILD_DIR)).map_err(CliError::Io)?;

    Ok(())
}

async fn ensure_project_config() -> Result<(PathBuf, PathBuf), CliError> {
    let project_dir = project_dir()?;
    let manifest_path = project_dir.join(MANIFEST_NAME);

    // Parse current manifest
    let project = get_project().await?;

    let mut needs_update = false;
    let mut slot_to_add = None;

    // Check if slot is missing
    if project.slot.is_none() {
        slot_to_add = Some(prompt_for_slot()?);
        needs_update = true;
    }

    // Update manifest if needed
    if needs_update {
        update_missing_config(&manifest_path, slot_to_add).await?;
    }

    Ok((manifest_path.to_path_buf(), project_dir.to_path_buf()))
}

static PROJECT_DIR: OnceLock<PathBuf> = OnceLock::new();
static MPY_CROSS_PATH: OnceLock<String> = OnceLock::new();

pub fn project_dir() -> Result<&'static Path, CliError> {
    PROJECT_DIR
        .get()
        .map(PathBuf::as_path)
        .ok_or(CliError::NoManifest)
}

#[pyfunction]
#[pyo3(signature = (args, binary_path, version, mpy_cross))]
fn call(
    args: Vec<String>,
    binary_path: Option<String>,
    version: Option<String>,
    mpy_cross: Option<String>,
) -> PyResult<()> {
    let rt = Runtime::new().unwrap();
    let result: miette::Result<()> = rt.block_on(async {
        MPY_CROSS_PATH
            .set(mpy_cross.unwrap_or_else(|| "mpy-cross".to_string()))
            .unwrap();

        let cmd = Venice::try_parse_from(args);
        let cmd = match cmd {
            Ok(cmd) => cmd,
            Err(err) => {
                err.exit();
            }
        };

        let start_dir = match cmd.dir.clone() {
            Some(dir) => dir,
            None => std::env::current_dir().map_err(CliError::Io)?,
        };
        if let Ok(project_dir) = resolve_project_dir(&start_dir) {
            PROJECT_DIR.set(project_dir).unwrap();
        }

        // Determine the runtime source
        #[cfg(debug_assertions)]
        let runtime_source: Option<RuntimeSource> = if let Some(raw_binary) = cmd.raw_binary.clone()
        {
            Some(RuntimeSource::new(
                raw_binary,
                semver::Version::new(0, 1, 0),
            ))
        } else if let (Some(path), Some(ver)) = (binary_path, version) {
            ver.parse::<semver::Version>()
                .ok()
                .map(|v| RuntimeSource::new(PathBuf::from(path), v))
        } else {
            None
        };

        #[cfg(not(debug_assertions))]
        let runtime_source: Option<RuntimeSource> =
            if let (Some(path), Some(ver)) = (binary_path, version) {
                ver.parse::<semver::Version>()
                    .ok()
                    .map(|v| RuntimeSource::new(PathBuf::from(path), v))
            } else {
                None
            };

        match cmd.subcmd {
            Subcommand::Build => {
                let _ = ensure_project_config().await?;
                let _ = build().await?;
            }
            Subcommand::Clean => clean()?,
            Subcommand::Upload { after_upload, cold } => {
                let _ = ensure_project_config().await?;
                let _ = upload(after_upload.map(|a| a.into()), runtime_source, cold).await?;
            }
            Subcommand::Terminal => terminal(&mut open_connection().await?).await?,
            Subcommand::Run { cold } => {
                let _ = ensure_project_config().await?;
                let mut conn = upload(Some(FileExitAction::RunProgram), runtime_source, cold).await?;
                terminal(&mut conn).await?;
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
