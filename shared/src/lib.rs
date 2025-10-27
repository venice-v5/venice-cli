pub const VENDOR_ID: u32 = 0x11235813;
pub const SRC_DIR: &str = "src";
pub const BUILD_DIR: &str = "build";
pub const TABLE_FILE: &str = "out.vpt";

pub mod build;
pub mod errors;
pub mod manifest;
pub mod run;
pub mod runtime;
pub mod terminal;
pub mod upload;
