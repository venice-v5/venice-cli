use std::path::PathBuf;

use vex_v5_serial::protocol::cdc2::file::FileExitAction;

use crate::{errors::CliError, terminal::terminal, upload::upload};

pub async fn run(dir: Option<PathBuf>) -> Result<(), CliError> {
    let mut conn = upload(dir, Some(FileExitAction::RunProgram)).await?;
    terminal(&mut conn).await?;
    Ok(())
}
