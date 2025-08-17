use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    process::Stdio,
    time::SystemTime,
};

use crate::errors::CliError;

pub const SRC_EXT: &str = "py";
pub const BUILD_EXT: &str = "mpy";

pub const PACKAGE_INIT_NAME: &[u8] = b"__init__";
pub const PYTHON_MOD_SEP: u8 = b'.';

#[derive(Debug, PartialEq, Eq)]
pub struct SrcModule {
    name: OsString,
}

impl SrcModule {
    fn from_path(path: &Path, src_dir: &Path) -> Self {
        let dir_stripped = path.strip_prefix(src_dir).unwrap();
        let ext_stripped = dir_stripped
            .with_file_name(dir_stripped.file_stem().unwrap())
            .into_os_string();

        Self { name: ext_stripped }
    }

    pub fn python_name(&self, package_name: &[u8]) -> Vec<u8> {
        let mut python_name = Vec::new();
        python_name.extend_from_slice(package_name);

        let mut name_bytes = self.name.as_encoded_bytes();
        python_name.push(b'.');

        if name_bytes.ends_with(PACKAGE_INIT_NAME) {
            name_bytes = &name_bytes[..name_bytes.len() - PACKAGE_INIT_NAME.len()];
        }

        python_name.extend_from_slice(name_bytes);

        for c in python_name.iter_mut() {
            if *c as char == std::path::MAIN_SEPARATOR {
                *c = b'.';
            }
        }

        if python_name.ends_with(&[PYTHON_MOD_SEP]) {
            python_name.pop();
        }

        python_name
    }

    pub fn module_flags(&self) -> u8 {
        0b01 | if self.name.as_encoded_bytes().ends_with(PACKAGE_INIT_NAME) {
            0b10
        } else {
            0b00
        }
    }

    pub fn src_path(&self, src_dir: &Path) -> PathBuf {
        src_dir.join(&self.name).with_extension(SRC_EXT)
    }

    pub fn build_path(&self, build_dir: &Path) -> PathBuf {
        build_dir.join(&self.name).with_extension(BUILD_EXT)
    }

    pub fn needs_rebuild(&self, src_dir: &Path, build_dir: &Path) -> bool {
        let src_modified = std::fs::metadata(self.src_path(src_dir))
            .and_then(|metadata| metadata.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let build_modified = std::fs::metadata(self.build_path(build_dir))
            .and_then(|metadata| metadata.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        src_modified >= build_modified
    }
}

fn find_modules_inner(
    src_dir: &Path,
    dir: &Path,
    modules: &mut Vec<SrcModule>,
) -> Result<(), std::io::Error> {
    if !std::fs::exists(dir.join("__init__.py"))? {
        return Ok(());
    }

    let read_dir = std::fs::read_dir(dir)?;
    for entry in read_dir.flatten() {
        let path = entry.path();

        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            find_modules_inner(src_dir, &path, modules)?;
        } else if path.extension() == Some(OsStr::new(SRC_EXT)) {
            modules.push(SrcModule::from_path(&path, src_dir));
        }
    }

    Ok(())
}

pub fn find_modules(src_dir: &Path) -> Result<Vec<SrcModule>, std::io::Error> {
    let mut modules = Vec::new();
    find_modules_inner(src_dir, src_dir, &mut modules)?;
    Ok(modules)
}

pub fn build_modules(
    src_dir: &Path,
    build_dir: &Path,
    modules: &[SrcModule],
) -> Result<bool, CliError> {
    let mut rebuild_table = false;

    for module in modules.iter() {
        if !module.needs_rebuild(src_dir, build_dir) {
            continue;
        }

        rebuild_table = true;
        let src_path = module.src_path(src_dir);

        let build_path = module.build_path(build_dir);
        std::fs::create_dir_all(build_path.parent().unwrap()).map_err(CliError::Io)?;

        let output = std::process::Command::new("mpy-cross")
            .arg(&src_path)
            .arg("-o")
            .arg(build_path)
            .stdin(Stdio::null())
            .output();

        if let Ok(output) = output
            && !output.status.success()
        {
            return Err(CliError::Compiler {
                file: src_path,
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            });
        }
    }

    Ok(rebuild_table)
}
