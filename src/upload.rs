use std::path::PathBuf;

use crate::{
    build::build,
    errors::CliError,
    manifest::{Manifest, find_manifest},
};

pub async fn upload(dir: Option<PathBuf>) -> miette::Result<()> {
    let manifest_path = find_manifest(dir.as_deref())?;
    let _manifest =
        toml::from_str::<Manifest>(&std::fs::read_to_string(&manifest_path).map_err(CliError::Io)?)
            .map_err(CliError::Manifest)?;

    build(dir)?;

    todo!();
}
