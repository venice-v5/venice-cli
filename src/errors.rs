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

    #[error("no entrypoint found at `{0}` - expected main.py or __init__.py")]
    NoEntrypoint(PathBuf),

    #[error("no runtime source provided - ensure the 'venice' package is installed")]
    NoRuntimeSource,
}
