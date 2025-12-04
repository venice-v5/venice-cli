use std::path::PathBuf;

use vex_v5_serial::protocol::cdc2::file::FileExitAction;

use crate::{errors::CliError, runtime::RuntimeSource, terminal::terminal, upload::upload};

pub async fn run(dir: Option<PathBuf>, runtime_source: Option<RuntimeSource>) -> Result<(), CliError> {
    let mut conn = upload(dir, Some(FileExitAction::RunProgram), runtime_source).await?;
    terminal(&mut conn).await?;
    Ok(())
}
