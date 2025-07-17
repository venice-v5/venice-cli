use std::{
    fmt::Display,
    path::{Path, PathBuf},
};

use miette::miette;
use regex::Regex;
use reqwest::Client;
use serde::Deserialize;

use crate::errors::CliError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

const RUNTIME_RE: &str = r"^venice-v(\d+)\.(\d+)\.(\d+)\.bin$";
const VERSION_RE: &str = r"^v(\d+)\.(\d+)\.(\d+)$";

impl Version {
    fn from_str(str: &str, re: &Regex) -> Option<Self> {
        re.captures(str).map(|caps| Version {
            major: caps[1].parse().unwrap(),
            minor: caps[2].parse().unwrap(),
            patch: caps[3].parse().unwrap(),
        })
    }
}

impl Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "v{}.{}.{}", self.major, self.minor, self.patch)
    }
}

pub async fn latest_installed_version(dir: &Path) -> Result<Option<Version>, std::io::Error> {
    let mut entries = tokio::fs::read_dir(dir).await?;
    let mut latest_version = None;
    let re = Regex::new(RUNTIME_RE).unwrap();

    loop {
        let entry = match entries.next_entry().await? {
            Some(entry) => entry,
            None => break,
        };

        let name = entry.file_name();
        let name = match name.to_str() {
            Some(name) => name,
            // runtime names can only contain "venice-<version>.bin"
            None => continue,
        };

        if let Some(version) = Version::from_str(name, &re) {
            latest_version = Some(
                latest_version
                    .map(|latest| if version > latest { version } else { latest })
                    .unwrap_or(version),
            );
        }
    }

    Ok(latest_version)
}

pub async fn version_exists(version: Version, dir: &Path) -> Result<bool, std::io::Error> {
    tokio::fs::try_exists(dir.join(format!("venice-{version}.bin"))).await
}

const USER_AGENT: &str = concat!("venice-cli/", env!("CARGO_PKG_VERSION"));

#[derive(Deserialize)]
struct Release {
    tag_name: String,
}

pub async fn latest_version(client: &Client) -> miette::Result<Version> {
    let text = client
        .get("https://api.github.com/repos/vexide/vexide/releases/latest")
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .map_err(CliError::Network)?
        .text()
        .await
        .map_err(CliError::Network)?;
    let release = serde_json::from_str::<Release>(&text)
        .map_err(|e| miette!("couldn't parse json response: {e}"))?;
    Ok(Version::from_str(&release.tag_name, &Regex::new(VERSION_RE).unwrap()).unwrap())
}

pub async fn download(version: Version, dir: &Path) -> Result<PathBuf, CliError> {
    let client = reqwest::Client::new();

    let bytes = client
        .get(format!(
            "https://github.com/venice-v5/venice/releases/download/{version}/venice-{version}.bin",
        ))
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .map_err(CliError::Network)?
        .bytes()
        .await
        .map_err(CliError::Network)?;

    let path = dir.join(format!("venice-{version}.bin"));
    tokio::fs::write(&path, bytes).await.map_err(CliError::Io)?;

    Ok(path)
}
