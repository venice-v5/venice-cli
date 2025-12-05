use std::path::{Path, PathBuf};
use std::io::{self, Write};

use serde::Deserialize;

use crate::errors::CliError;

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
    pub entrypoint: Option<PathBuf>,
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
    pub entrypoint: Option<PathBuf>,
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

pub fn find_manifest(dir: Option<&Path>) -> Result<PathBuf, CliError> {
    if let Some(dir) = dir {
        let manifest_path = dir.join(MANIFEST_NAME);
        return if std::fs::exists(&manifest_path).map_err(CliError::Io)? {
            Ok(manifest_path)
        } else {
            Err(CliError::NoManifest)
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
            return Err(CliError::NoManifest);
        }
    }
}

pub async fn parse_manifest(path: &Path) -> Result<Project, CliError> {
    let file_string = tokio::fs::read_to_string(path).await?;
    let pyproject: PyProjectToml = toml::from_str(&file_string).map_err(CliError::Manifest)?;

    // Get [tool.venice] section if it exists
    let venice_config = pyproject
        .tool
        .and_then(|t| t.venice);

    // Merge names: [tool.venice].name overrides [project].name
    let project_name = pyproject.project.as_ref().and_then(|p| p.name.clone());
    let name = venice_config
        .as_ref()
        .and_then(|v| v.name.clone())
        .or(project_name)
        .ok_or(CliError::NoProjectName)?;

    // Merge descriptions: [tool.venice].description overrides [project].description
    let project_description = pyproject.project.as_ref().and_then(|p| p.description.clone());
    let description = venice_config
        .as_ref()
        .and_then(|v| v.description.clone())
        .or(project_description);

    Ok(Project {
        name,
        slot: venice_config.as_ref().and_then(|v| v.slot),
        entrypoint: venice_config.as_ref().and_then(|v| v.entrypoint.clone()),
        description,
        icon: venice_config.as_ref().map(|v| v.icon).unwrap_or_default(),
    })
}

/// Resolve the actual Python file from an entrypoint directory.
/// Looks for main.py first, then __init__.py.
/// For subdirectories (during recursive compilation), only __init__.py is used.
pub fn resolve_entrypoint(entrypoint: &Path, is_root: bool) -> Result<PathBuf, CliError> {
    if is_root {
        // For root entrypoint, check main.py first
        let main_py = entrypoint.join("main.py");
        if main_py.exists() {
            return Ok(main_py);
        }
    }

    // Fall back to __init__.py
    let init_py = entrypoint.join("__init__.py");
    if init_py.exists() {
        return Ok(init_py);
    }

    Err(CliError::NoEntrypoint(entrypoint.to_path_buf()))
}

/// Find all main.py files in a directory tree and return them sorted by path depth (shortest first)
pub fn find_main_py_files(dir: &Path) -> Result<Vec<PathBuf>, CliError> {
    let mut main_files = Vec::new();
    find_main_py_recursive(dir, &mut main_files)?;
    
    // Sort by path component count (shortest paths first)
    main_files.sort_by_key(|p| p.components().count());
    
    Ok(main_files)
}

fn find_main_py_recursive(dir: &Path, results: &mut Vec<PathBuf>) -> Result<(), CliError> {
    let entries = std::fs::read_dir(dir).map_err(CliError::Io)?;
    
    for entry in entries {
        let entry = entry.map_err(CliError::Io)?;
        let path = entry.path();
        
        if path.is_dir() {
            // Skip hidden directories and common non-source directories
            let dir_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !dir_name.starts_with('.') && dir_name != "__pycache__" && dir_name != "node_modules" && dir_name != ".venv" && dir_name != "venv" {
                find_main_py_recursive(&path, results)?;
            }
        } else if path.file_name().and_then(|n| n.to_str()) == Some("main.py") {
            results.push(path);
        }
    }
    
    Ok(())
}

/// Prompt user for slot number with explanation
pub fn prompt_for_slot() -> Result<u8, CliError> {
    println!();
    println!("📍 Slot Configuration");
    println!("The VEX V5 Brain can store up to 8 programs simultaneously.");
    println!("Each program occupies a numbered slot (1-8) on the brain.");
    println!();
    println!("Choose a slot for your program:");
    
    loop {
        print!("Enter slot number (1-8): ");
        io::stdout().flush().map_err(CliError::Io)?;
        
        let mut input = String::new();
        io::stdin().read_line(&mut input).map_err(CliError::Io)?;
        let input = input.trim();
        
        match input.parse::<u8>() {
            Ok(slot) if (1..=8).contains(&slot) => {
                println!("✓ Using slot {}", slot);
                return Ok(slot);
            }
            Ok(_) => {
                println!("❌ Slot must be between 1 and 8. Please try again.");
            }
            Err(_) => {
                println!("❌ Please enter a valid number between 1 and 8.");
            }
        }
    }
}

/// Prompt user for entrypoint with explanation and suggestions
pub fn prompt_for_entrypoint(project_dir: &Path) -> Result<PathBuf, CliError> {
    println!();
    println!("📁 Entrypoint Configuration");
    println!("The entrypoint is the folder containing your main Python code.");
    println!("Venice will look for main.py first, then __init__.py in this folder.");
    println!();
    
    // Find main.py files to suggest
    match find_main_py_files(project_dir) {
        Ok(main_files) if !main_files.is_empty() => {
            println!("Found main.py files in your project:");
            for (i, file) in main_files.iter().enumerate() {
                let relative_path = file.strip_prefix(project_dir).unwrap_or(file);
                let parent = relative_path.parent().unwrap_or(Path::new("."));
                println!("  {}. {}", i + 1, parent.display());
            }
            println!();
            
            loop {
                print!("Choose an entrypoint (1-{}, or enter custom path): ", main_files.len());
                io::stdout().flush().map_err(CliError::Io)?;
                
                let mut input = String::new();
                io::stdin().read_line(&mut input).map_err(CliError::Io)?;
                let input = input.trim();
                
                if input.is_empty() {
                    println!("❌ Please make a selection.");
                    continue;
                }
                
                // Try to parse as number selection
                if let Ok(choice) = input.parse::<usize>() {
                    if (1..=main_files.len()).contains(&choice) {
                        let selected_file = &main_files[choice - 1];
                        let entrypoint = selected_file.parent().unwrap_or(selected_file);
                        let relative_path = entrypoint.strip_prefix(project_dir).unwrap_or(entrypoint);
                        let entrypoint_str = if relative_path.as_os_str().is_empty() { "." } else { &relative_path.to_string_lossy() };
                        println!("✓ Using entrypoint: {}", entrypoint_str);
                        return Ok(PathBuf::from(entrypoint_str));
                    } else {
                        println!("❌ Please enter a number between 1 and {}.", main_files.len());
                        continue;
                    }
                }
                
                // Treat as custom path
                let custom_path = PathBuf::from(input);
                let full_path = project_dir.join(&custom_path);
                
                if !full_path.exists() {
                    println!("❌ Path '{}' does not exist.", custom_path.display());
                    continue;
                }
                
                // Check if it contains main.py or __init__.py
                let has_main = full_path.join("main.py").exists();
                let has_init = full_path.join("__init__.py").exists();
                
                if !has_main && !has_init {
                    println!("❌ No main.py or __init__.py found in '{}'.", custom_path.display());
                    continue;
                }
                
                println!("✓ Using custom entrypoint: {}", custom_path.display());
                return Ok(custom_path);
            }
        }
        _ => {
            println!("No main.py files found in your project.");
            println!("You'll need to specify a custom path.");
            
            loop {
                print!("Enter entrypoint path (relative to project root): ");
                io::stdout().flush().map_err(CliError::Io)?;
                
                let mut input = String::new();
                io::stdin().read_line(&mut input).map_err(CliError::Io)?;
                let input = input.trim();
                
                if input.is_empty() {
                    println!("❌ Please enter a path.");
                    continue;
                }
                
                let custom_path = PathBuf::from(input);
                let full_path = project_dir.join(&custom_path);
                
                if !full_path.exists() {
                    println!("❌ Path '{}' does not exist.", custom_path.display());
                    continue;
                }
                
                let has_main = full_path.join("main.py").exists();
                let has_init = full_path.join("__init__.py").exists();
                
                if !has_main && !has_init {
                    println!("❌ No main.py or __init__.py found in '{}'.", custom_path.display());
                    continue;
                }
                
                println!("✓ Using entrypoint: {}", custom_path.display());
                return Ok(custom_path);
            }
        }
    }
}

/// Update pyproject.toml with missing slot and/or entrypoint
pub async fn update_missing_config(manifest_path: &Path, slot: Option<u8>, entrypoint: Option<PathBuf>) -> Result<(), CliError> {
    if slot.is_none() && entrypoint.is_none() {
        return Ok(()); // Nothing to update
    }
    
    // Read existing pyproject.toml
    let content = tokio::fs::read_to_string(manifest_path).await.map_err(CliError::Io)?;
    // Parse with toml_edit and convert any errors to our error type
    let mut doc = content.parse::<toml_edit::DocumentMut>().map_err(|e| {
        CliError::ManifestEdit(e.to_string())
    })?;
    
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
            
            // Update entrypoint if provided
            if let Some(entrypoint) = entrypoint {
                if let Some(venice) = tool_table.get_mut("venice") {
                    if let Some(venice_table) = venice.as_table_mut() {
                        let entrypoint_str = entrypoint.to_string_lossy();
                        venice_table.insert("entrypoint", toml_edit::value(entrypoint_str.as_ref()));
                    }
                }
            }
        }
    }
    
    // Write back
    tokio::fs::write(manifest_path, doc.to_string()).await.map_err(CliError::Io)?;
    Ok(())
}
