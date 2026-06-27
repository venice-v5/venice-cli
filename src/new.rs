use std::path::{Path, PathBuf};
use std::process::Command;

use crate::errors::CliError;
use crate::uv_path;

/// The venice runtime version paired with this CLI release.
/// Bump in tandem with CARGO_PKG_VERSION when cutting a release.
const VENICE_VERSION: &str = "0.1.0";

const PYPROJECT_TEMPLATE: &str = r#"[project]
name = "{name}"
version = "0.1.0"
requires-python = ">=3.14"

[tool.venice]
slot = 1
"#;

const MAIN_TEMPLATE: &str = r#"from venice import *
import vasyncio

async def main():
    print("Hello, Venice!")

vasyncio.run(main())
"#;

pub fn new(name: &str, venice_wheel: Option<&Path>, cli_wheel: Option<&Path>) -> miette::Result<()> {
    let uv = uv_path()?;

    let project_dir = PathBuf::from(name);
    if project_dir.exists() {
        return Err(CliError::ProjectExists(project_dir).into());
    }

    std::fs::create_dir(&project_dir).map_err(CliError::Io)?;

    let pyproject = PYPROJECT_TEMPLATE.replace("{name}", name);
    std::fs::write(project_dir.join("pyproject.toml"), pyproject).map_err(CliError::Io)?;
    std::fs::write(project_dir.join("main.py"), MAIN_TEMPLATE).map_err(CliError::Io)?;

    let venice_spec = match venice_wheel {
        Some(path) => path.to_string_lossy().into_owned(),
        None => format!("venice=={VENICE_VERSION}"),
    };
    let cli_spec = match cli_wheel {
        Some(path) => path.to_string_lossy().into_owned(),
        None => format!("venice-cli=={}", env!("CARGO_PKG_VERSION")),
    };

    run_uv(uv, &project_dir, &["add", &venice_spec])?;
    run_uv(uv, &project_dir, &["add", "--dev", &cli_spec])?;

    println!(
        "\nCreated project `{name}`. To get started:\n\n  cd {name}\n  uv run venice-cli build\n"
    );

    Ok(())
}

fn run_uv(uv: &str, dir: &Path, args: &[&str]) -> miette::Result<()> {
    let output = Command::new(uv)
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(CliError::Io)?;

    if !output.status.success() {
        return Err(CliError::UvFailed {
            status: output.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
        .into());
    }

    Ok(())
}
