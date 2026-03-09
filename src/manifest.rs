use std::path::{Path, PathBuf};

use inquire::CustomType;
use inquire::validator::Validation;
use serde::Deserialize;

use crate::{errors::CliError, project_dir};

pub const MANIFEST_NAME: &str = "pyproject.toml";

/// The parsed pyproject.toml structure
#[derive(Deserialize, Debug)]
pub struct PyProjectToml {
    project: Option<PyProject>,
    tool: Option<Tool>,
}

/// Standard [project] section
#[derive(Deserialize, Debug)]
pub struct PyProject {
    name: Option<String>,
    description: Option<String>,
}

/// [tool] section containing venice config
#[derive(Deserialize, Debug)]
pub struct Tool {
    venice: Option<VeniceConfig>,
}

/// [tool.venice] section
#[derive(Deserialize, Debug)]
pub struct VeniceConfig {
    pub slot: Option<u8>,
    pub name: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub icon: ProgramIcon,
}

/// The resolved project configuration (after merging [project] and [tool.venice])
#[derive(Debug)]
pub struct Project {
    pub name: String,
    pub slot: Option<u8>,
    pub description: Option<String>,
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

pub fn resolve_project_dir(start_dir: &Path) -> Result<PathBuf, CliError> {
    let mut search_dir = start_dir.to_path_buf();

    loop {
        let manifest_path = search_dir.join(MANIFEST_NAME);
        if std::fs::exists(&manifest_path).map_err(CliError::Io)? {
            return Ok(search_dir);
        }

        if !search_dir.pop() {
            return Err(CliError::NoManifest);
        }
    }
}

pub async fn get_project() -> Result<Project, CliError> {
    let manifest_path = project_dir()?.join(MANIFEST_NAME);
    let file_string = tokio::fs::read_to_string(manifest_path).await?;
    let pyproject: PyProjectToml = toml::from_str(&file_string).map_err(CliError::Manifest)?;

    let venice_config = pyproject.tool.and_then(|t| t.venice);

    let project_name = pyproject.project.as_ref().and_then(|p| p.name.clone());
    let name = venice_config
        .as_ref()
        .and_then(|v| v.name.clone())
        .or(project_name)
        .ok_or(CliError::NoProjectName)?;

    let project_description = pyproject
        .project
        .as_ref()
        .and_then(|p| p.description.clone());
    let description = venice_config
        .as_ref()
        .and_then(|v| v.description.clone())
        .or(project_description);

    Ok(Project {
        name,
        slot: venice_config.as_ref().and_then(|v| v.slot),
        description,
        icon: venice_config.as_ref().map(|v| v.icon).unwrap_or_default(),
    })
}

pub fn prompt_for_slot() -> Result<u8, CliError> {
    println!("\nYou haven't yet configured a slot for your program in pyproject.toml.");
    let slot = CustomType::<u8>::new("Choose a slot for your program (1-8):")
        .with_validator(|&input: &u8| {
            if (1..=8).contains(&input) {
                Ok(Validation::Valid)
            } else {
                // Inquire handles the pretty printing of this error message
                Ok(Validation::Invalid(
                    "❌ Slot must be between 1 and 8".into(),
                ))
            }
        })
        .with_error_message("Please enter a valid number")
        .prompt()
        .map_err(|e| CliError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

    println!("✓ Using slot {}", slot);
    Ok(slot)
}

/// update pyproject.toml with missing slot
pub async fn update_missing_config(manifest_path: &Path, slot: Option<u8>) -> Result<(), CliError> {
    if slot.is_none() {
        return Ok(()); // Nothing to update
    }

    // Read existing pyproject.toml
    let content = tokio::fs::read_to_string(manifest_path)
        .await
        .map_err(CliError::Io)?;
    // Parse with toml_edit and convert any errors to our error type
    let mut doc = content
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| CliError::ManifestEdit(e.to_string()))?;

    // Ensure [tool] exists
    if !doc.contains_key("tool") {
        doc.insert("tool", toml_edit::Item::Table(toml_edit::Table::new()));
    }

    // Ensure [tool.venice] exists
    if let Some(tool) = doc.get_mut("tool") {
        if let Some(tool_table) = tool.as_table_mut() {
            if !tool_table.contains_key("venice") {
                tool_table.insert("venice", toml_edit::Item::Table(toml_edit::Table::new()));
            }

            // Update slot if provided
            if let Some(slot) = slot {
                if let Some(venice) = tool_table.get_mut("venice") {
                    if let Some(venice_table) = venice.as_table_mut() {
                        venice_table.insert("slot", toml_edit::value(i64::from(slot)));
                    }
                }
            }
        }
    }

    // Write back
    tokio::fs::write(manifest_path, doc.to_string())
        .await
        .map_err(CliError::Io)?;
    Ok(())
}
