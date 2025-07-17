use std::{
    ffi::{OsStr, OsString},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Stdio,
    time::SystemTime,
};

use crate::errors::CliError;

#[derive(Debug, PartialEq, Eq)]
pub struct SrcModule {
    name: OsString,
}

impl SrcModule {
    pub fn src_path(&self, src_dir: &Path) -> PathBuf {
        src_dir.join(&self.name).with_extension("py")
    }

    pub fn build_path(&self, build_dir: &Path) -> PathBuf {
        build_dir.join(&self.name).with_extension("mpy")
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

fn find_modules_inner(dir: &Path, modules: &mut Vec<SrcModule>) -> Result<(), std::io::Error> {
    let read_dir = std::fs::read_dir(dir)?;
    for entry in read_dir.filter_map(|entry| entry.ok()) {
        let path = entry.path();

        if path.extension() == Some(OsStr::new("py")) {
            modules.push(SrcModule {
                name: path.file_stem().unwrap().to_os_string(),
            });
        }
    }

    Ok(())
}

pub fn find_modules(src_dir: &Path) -> Result<Vec<SrcModule>, std::io::Error> {
    let mut modules = Vec::new();
    find_modules_inner(src_dir, &mut modules)?;
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
        let output = std::process::Command::new("mpy-cross")
            .arg(&src_path)
            .arg("-o")
            .arg(module.build_path(build_dir))
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

#[derive(bytemuck::NoUninit, Clone, Copy)]
#[repr(C)]
struct TableHeader {
    magic: u32,
    name_pool: u32,
    bytecode_pool: u32,
    module_count: u32,
}

#[derive(bytemuck::NoUninit, Clone, Copy)]
#[repr(C)]
struct ModulePtr {
    name_len: u32,
    bytecode_len: u32,
}

pub struct Table {
    header: TableHeader,
    module_ptrs: Vec<ModulePtr>,
    name_pool: Vec<u8>,
    bytecode_pool: Vec<u8>,
}

impl Table {
    pub fn generate(build_dir: &Path, modules: &[SrcModule]) -> Result<Self, CliError> {
        let mut module_ptrs = Vec::new();
        let mut bytecode_pool = Vec::new();
        let mut name_pool = Vec::new();

        for module in modules.iter() {
            let build_path = module.build_path(build_dir);
            let len = std::fs::OpenOptions::new()
                .read(true)
                .open(&build_path)
                .and_then(|mut f| f.read_to_end(&mut bytecode_pool))
                .map_err(CliError::Io)?;

            let name_bytes = module.name.as_encoded_bytes();
            name_pool.extend_from_slice(name_bytes);
            module_ptrs.push(ModulePtr {
                name_len: name_bytes.len() as u32,
                bytecode_len: len as u32,
            });
        }

        let name_pool_offset =
            size_of::<TableHeader>() + module_ptrs.len() * size_of::<ModulePtr>();
        let bytecode_pool_offset = name_pool_offset + name_pool.len();

        const BYTECODE_TABLE_MAGIC: u32 = 0x675c3ed9;

        Ok(Self {
            header: TableHeader {
                magic: BYTECODE_TABLE_MAGIC,
                name_pool: name_pool_offset as u32,
                bytecode_pool: bytecode_pool_offset as u32,
                module_count: module_ptrs.len() as u32,
            },
            module_ptrs,
            name_pool,
            bytecode_pool,
        })
    }

    pub fn write_to_file(&self, path: &Path) -> Result<(), std::io::Error> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;

        file.write_all(bytemuck::bytes_of(&self.header))?;
        file.write_all(bytemuck::cast_slice(&self.module_ptrs))?;
        file.write_all(&self.name_pool)?;
        file.write_all(&self.bytecode_pool)?;

        Ok(())
    }
}
