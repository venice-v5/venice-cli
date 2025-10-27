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

    #[error("invalid version of venice in {MANIFEST_NAME}: {0}")]
    InvalidVersion(#[from] semver::Error),

    #[error("unreleased version of venice in {MANIFEST_NAME}")]
    UnreleasedVersion,

    #[error("couldn't parse {MANIFEST_NAME}")]
    Manifest(#[from] toml::de::Error),

    #[error("couldn't build `{file}` with `mpy-cross`: {stderr}")]
    Compiler { file: PathBuf, stderr: String },

    #[error(transparent)]
    Network(#[from] reqwest::Error),

    #[error("home directory could not be found")]
    HomeDirNotFound,

    #[error("couldn't find {MANIFEST_NAME} in current directory or any parent directories")]
    NoManifest,
}
