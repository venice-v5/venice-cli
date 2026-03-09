use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    process::Stdio,
    time::SystemTime,
};

use venice_program_table::{ProgramBuilder, ProgramFlags, VptBuilder};

use crate::{
    BUILD_DIR, MPY_CROSS_PATH, TABLE_FILE, VENDOR_ID, errors::CliError, project_dir,
};

pub const SRC_EXT: &str = "py";
pub const BUILD_EXT: &str = "mpy";

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

    pub fn python_name(&self) -> Result<Vec<u8>, CliError> {
        let mut python_name = self.name.clone().into_encoded_bytes();

        for c in python_name.iter_mut() {
            if *c as char == std::path::MAIN_SEPARATOR {
                *c = b'.';
            }
        }

        if python_name == b"__init__" {
            return Err(CliError::TopLevelInit);
        } else if python_name.ends_with(b".__init__") {
            python_name.truncate(python_name.len() - b".__init__".len());
        }

        Ok(python_name)
    }

    pub fn module_flags(&self) -> ProgramFlags {
        if self.name.as_encoded_bytes().ends_with(b"__init__") {
            ProgramFlags::IS_PACKAGE
        } else {
            ProgramFlags::empty()
        }
    }

    pub fn src_path(&self, src_dir: &Path) -> PathBuf {
        src_dir.join(&self.name).with_extension(SRC_EXT)
    }

    pub fn build_path(&self, build_dir: &Path) -> PathBuf {
        build_dir.join(&self.name).with_extension(BUILD_EXT)
    }

    pub async fn needs_rebuild(
        &self,
        src_dir: &Path,
        build_dir: &Path,
    ) -> Result<bool, std::io::Error> {
        let src_modified = tokio::fs::metadata(self.src_path(src_dir))
            .await?
            .modified()
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let build_modified = tokio::fs::metadata(self.build_path(build_dir))
            .await
            .and_then(|metadata| metadata.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        Ok(src_modified >= build_modified)
    }
}

/// Find modules starting from entrypoint directory.
/// For root: looks for main.py first, then __init__.py.
/// For subdirectories: only __init__.py marks a package.
async fn find_modules_inner(
    src_dir: &Path,
    dir: &Path,
    modules: &mut Vec<SrcModule>,
    is_root: bool,
) -> Result<(), CliError> {
    let has_init = tokio::fs::try_exists(dir.join("__init__.py")).await.map_err(CliError::Io)?;
    let has_main = tokio::fs::try_exists(dir.join("main.py")).await.map_err(CliError::Io)?;
    if is_root && !has_main {
        // For root, we need main.py
        return Err(CliError::NoEntrypoint(dir.to_path_buf()));
    }
    else if !is_root && !has_init {
        // For subdirs, we need __init__.py to be a package
        return Ok(());
    }

    let mut read_dir = tokio::fs::read_dir(dir).await.map_err(CliError::Io)?;
    while let Some(entry) = read_dir.next_entry().await.map_err(CliError::Io)? {
        let path = entry.path();

        let file_type = entry.file_type().await.map_err(CliError::Io)?;
        if file_type.is_dir() {
            Box::pin(find_modules_inner(src_dir, &path, modules, false)).await?;
        } else if path.extension() == Some(OsStr::new(SRC_EXT)) {
            let filename = path.file_stem().and_then(|s| s.to_str());

            if !is_root && filename == Some("main") {
                continue;
            }

            modules.push(SrcModule::from_path(&path, src_dir));
        }
    }

    Ok(())
}

pub async fn find_modules(src_dir: &Path) -> Result<Vec<SrcModule>, CliError> {
    let mut modules = Vec::new();
    find_modules_inner(src_dir, src_dir, &mut modules, true).await?;
    Ok(modules)
}

pub async fn build_modules(
    src_dir: &Path,
    build_dir: &Path,
    modules: &[SrcModule],
) -> Result<(), CliError> {
    for module in modules.iter() {
        let src_path = module.src_path(src_dir);

        let build_path = module.build_path(build_dir);
        tokio::fs::create_dir_all(build_path.parent().unwrap()).await?;
        let mut name = module.name.clone();
        name.push(".py");
        let output = std::process::Command::new(MPY_CROSS_PATH.get().unwrap())
            .arg(&src_path)
            .arg("-o")
            .arg(build_path)
            .arg("-s")
            .arg(name)
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

    Ok(())
}

pub async fn build() -> Result<Vec<u8>, CliError> {
    let manifest_dir = project_dir()?;

    let src_dir = manifest_dir;
    let build_dir = manifest_dir.join(BUILD_DIR);

    let modules = find_modules(&src_dir).await?;

    if !tokio::fs::try_exists(&build_dir).await? {
        tokio::fs::create_dir(&build_dir).await?;
    }

    let table_path = build_dir.join(TABLE_FILE);
    build_modules(&src_dir, &build_dir, &modules).await?;

    let mut vpt_builder = VptBuilder::new(VENDOR_ID);

    for module in modules.iter() {
        let build_path = module.build_path(&build_dir);
        let bytecode = tokio::fs::read(&build_path).await?;
        let module_name = String::from_utf8_lossy(&module.python_name()?).into_owned();

        vpt_builder.add_program(ProgramBuilder {
            name: module_name.into_bytes(),
            payload: bytecode,
            flags: module.module_flags()
        });
    }

    let vpt = vpt_builder.build();

    tokio::fs::write(&table_path, &vpt).await?;
    Ok(vpt)
}
