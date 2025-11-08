use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    process::Stdio,
    time::SystemTime,
};

use venice_program_table::{ProgramBuilder, VptBuilder};

use crate::{
    BUILD_DIR, SRC_DIR, TABLE_FILE, VENDOR_ID,
    errors::CliError,
    manifest::{find_manifest, parse_manifest},
};

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

// TODO: refactor
async fn find_modules_inner(
    src_dir: &Path,
    dir: &Path,
    modules: &mut Vec<SrcModule>,
) -> Result<(), std::io::Error> {
    if !tokio::fs::try_exists(dir.join("__init__.py")).await? {
        return Ok(());
    }

    let mut read_dir = tokio::fs::read_dir(dir).await?;
    while let Some(entry) = read_dir.next_entry().await? {
        let path = entry.path();

        let file_type = entry.file_type().await?;
        if file_type.is_dir() {
            Box::pin(find_modules_inner(src_dir, &path, modules)).await?;
        } else if path.extension() == Some(OsStr::new(SRC_EXT)) {
            modules.push(SrcModule::from_path(&path, src_dir));
        }
    }

    Ok(())
}

pub async fn find_modules(src_dir: &Path) -> Result<Vec<SrcModule>, std::io::Error> {
    let mut modules = Vec::new();
    find_modules_inner(src_dir, src_dir, &mut modules).await?;
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

    Ok(())
}

const VENICE_PACKAGE_NAME_PROGRAM: &[u8] = b"__venice__package_name__";

pub async fn build(dir: Option<PathBuf>) -> Result<Vec<u8>, CliError> {
    let manifest_path = find_manifest(dir.as_deref())?;
    let manifest = parse_manifest(&manifest_path).await?;
    let manifest_dir = dir
        .as_deref()
        .unwrap_or_else(|| manifest_path.parent().unwrap());

    let src_dir = manifest_dir.join(SRC_DIR);
    let build_dir = manifest_dir.join(BUILD_DIR);

    let modules = find_modules(&src_dir).await?;

    if !tokio::fs::try_exists(&build_dir).await? {
        tokio::fs::create_dir(&build_dir).await?;
    }

    let table_path = build_dir.join(TABLE_FILE);
    build_modules(&src_dir, &build_dir, &modules).await?;

    let mut vpt_builder = VptBuilder::new(VENDOR_ID);

    let package_name = manifest.name.as_bytes();

    let mut package_name_payload = vec![0];
    package_name_payload.extend_from_slice(package_name);
    vpt_builder.add_program(ProgramBuilder {
        name: VENICE_PACKAGE_NAME_PROGRAM.to_vec(),
        payload: package_name_payload,
    });

    for module in modules.iter() {
        let build_path = module.build_path(&build_dir);

        let mut payload = tokio::fs::read(&build_path).await?;
        payload.insert(0, module.module_flags());

        vpt_builder.add_program(ProgramBuilder {
            name: module.python_name(package_name),
            payload,
        });
    }

    let bytes = vpt_builder.build();
    tokio::fs::write(&table_path, &bytes).await?;
    Ok(bytes)
}
