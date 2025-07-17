use std::path::PathBuf;

use miette::Diagnostic;
use thiserror::Error;

use crate::manifest::MANIFEST_NAME;

#[derive(Debug, Error, Diagnostic)]
pub enum CliError {
    #[error(transparent)]
    Io(std::io::Error),

    #[error(transparent)]
    Serial(vex_v5_serial::connection::serial::SerialError),

    #[error("couldn't parse {MANIFEST_NAME}")]
    Manifest(toml::de::Error),

    #[error("couldn't build `{file}` with `mpy-cross`: {stderr}")]
    Compiler { file: PathBuf, stderr: String },

    #[error(transparent)]
    Network(reqwest::Error),
}
