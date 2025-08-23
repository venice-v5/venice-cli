use std::path::{Path, PathBuf};

use miette::miette;
use serde::Deserialize;

use crate::errors::CliError;

pub const MANIFEST_NAME: &str = "Venice.toml";

#[derive(Deserialize, Debug)]
#[serde(rename_all = "kebab-case")]
pub struct Manifest {
    pub name: String,
    pub slot: u8,
    pub venice_version: String,
    #[serde(default)]
    pub icon: ProgramIcon,
}

#[derive(Deserialize, Default, Debug, Clone, Copy, Eq, PartialEq)]
#[repr(u16)]
pub enum ProgramIcon {
    VexCodingStudio = 0,
    CoolX = 1,
    // This is the icon that appears when you provide a missing icon name.
    // 2 is one such icon that doesn't exist.
    #[default]
    QuestionMark = 2,
    Pizza = 3,
    Clawbot = 10,
    Robot = 11,
    PowerButton = 12,
    Planets = 13,
    Alien = 27,
    AlienInUfo = 29,
    CupInField = 50,
    CupAndBall = 51,
    Matlab = 901,
    Pros = 902,
    RobotMesh = 903,
    RobotMeshCpp = 911,
    RobotMeshBlockly = 912,
    RobotMeshFlowol = 913,
    RobotMeshJS = 914,
    RobotMeshPy = 915,
    // This icon is duplicated several times and has many file names.
    CodeFile = 920,
    VexcodeBrackets = 921,
    VexcodeBlocks = 922,
    VexcodePython = 925,
    VexcodeCpp = 926,
}

pub fn find_manifest(dir: Option<&Path>) -> miette::Result<PathBuf> {
    if let Some(dir) = dir {
        let manifest_path = dir.join(MANIFEST_NAME);
        return if std::fs::exists(&manifest_path).map_err(CliError::Io)? {
            Ok(manifest_path)
        } else {
            Err(miette!(
                "couldn't find `{MANIFEST_NAME}` in `{}`",
                dir.display()
            ))
        };
    }

    let current_dir = std::env::current_dir().map_err(CliError::Io)?;
    let mut search_dir = current_dir.clone();

    loop {
        let manifest_path = search_dir.join(MANIFEST_NAME);
        if std::fs::exists(&manifest_path).map_err(CliError::Io)? {
            return Ok(manifest_path);
        }

        if !search_dir.pop() {
            return Err(miette!(
                "couldn't find `{MANIFEST_NAME}` in `{}` or any parent directory",
                current_dir.display()
            ));
        }
    }
}

pub fn parse_manifest(path: &Path) -> miette::Result<Manifest> {
    let file_string = std::fs::read_to_string(path).map_err(CliError::Io)?;
    Ok(toml::from_str(&file_string).map_err(CliError::Manifest)?)
}
