use std::path::PathBuf;

use miette::Diagnostic;
use thiserror::Error;

use crate::manifest::MANIFEST_NAME;

#[derive(Debug, Error, Diagnostic)]
pub enum CliError {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("slot must be between 1 and 8")]
    SlotOutOfRange,

    #[error(transparent)]
    Serial(#[from] vex_v5_serial::serial::SerialError),

    #[error("no devices found")]
    NoDevice,

    #[error("radio channel disconnect timeout")]
    RadioChannelDisconnectTimeout,

    #[error("radio channel reconnect timeout")]
    RadioChannelReconnectTimeout,

    #[error("invalid version: {0}")]
    InvalidVersion(#[from] semver::Error),

    #[error("couldn't parse {MANIFEST_NAME}")]
    Manifest(#[from] toml::de::Error),

    #[error("couldn't parse {MANIFEST_NAME}: {0}")]
    ManifestEdit(String),

    #[error("couldn't build `{file}` with `mpy-cross`: {stderr}")]
    Compiler { file: PathBuf, stderr: String },

    #[error("couldn't find {MANIFEST_NAME} in current directory or any parent directories")]
    NoManifest,

    #[error("no project name found - set [project].name or [tool.venice].name in {MANIFEST_NAME}")]
    NoProjectName,

    #[error("no entrypoint found in `{0}` - expected main.py")]
    NoEntrypoint(PathBuf),

    #[error("found top-level __init__.py in source root. the device root is not a package, so this file will never execute; please move initialization code to main.py")]
    TopLevelInit,

    #[error("no runtime source provided - ensure the 'venice' package is installed")]
    NoRuntimeSource,

    #[error("uv not found - ensure the 'uv' package is installed in the same environment as venice-cli")]
    NoUv,

    #[error("directory `{0}` already exists")]
    ProjectExists(PathBuf),

    #[error("uv exited with status {status}:\n{stderr}")]
    UvFailed { status: i32, stderr: String },
}
