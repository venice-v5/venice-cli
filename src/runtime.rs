use std::{
    fmt::Display,
    path::{Path, PathBuf},
    str::FromStr,
};

use miette::miette;
use reqwest::Client;
use serde::Deserialize;
use thiserror::Error;

use crate::errors::CliError;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RtBin {
    pub version: semver::Version,
}

impl RtBin {
    pub const fn from_version(version: semver::Version) -> Self {
        Self { version }
    }
}

impl Display for RtBin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "venice-v{}.bin", self.version)
    }
}

#[derive(Debug, Error)]
pub enum RtBinParseError {
    #[error(transparent)]
    VersionError(semver::Error),

    #[error("runtime name did not start with 'venice-'")]
    BadPrefix,

    #[error("runtime name did not end with '.bin'")]
    BadExtension,
}

impl FromStr for RtBin {
    type Err = RtBinParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        const PREFIX: &str = "venice-v";
        const EXT: &str = ".bin";

        if !s.starts_with(PREFIX) {
            return Err(RtBinParseError::BadPrefix);
        } else if !s.ends_with(EXT) {
            return Err(RtBinParseError::BadExtension);
        }

        let version = &s[PREFIX.len()..s.len() - EXT.len()];
        version
            .parse()
            .map(|version| Self { version })
            .map_err(RtBinParseError::VersionError)
    }
}

pub async fn installed_bins(dir: &Path) -> Result<Vec<RtBin>, std::io::Error> {
    let mut entries = tokio::fs::read_dir(dir).await?;
    let mut versions = Vec::new();

    loop {
        let entry = match entries.next_entry().await? {
            Some(entry) => entry,
            None => break,
        };

        let name = entry.file_name();
        let name = match name.to_str() {
            Some(name) => name,
            // runtime names can only contain "venice-<version>.bin" which is always valid UTF-8
            None => continue,
        };

        if let Ok(version) = name.parse() {
            versions.push(version);
        }
    }

    Ok(versions)
}

pub async fn bin_exists(bin: &RtBin, dir: &Path) -> Result<bool, std::io::Error> {
    tokio::fs::try_exists(dir.join(format!("{bin}"))).await
}

const USER_AGENT: &str = concat!("venice-cli/", env!("CARGO_PKG_VERSION"));

#[derive(Deserialize)]
struct Release {
    tag_name: String,
}

pub async fn latest_version(client: &Client) -> miette::Result<semver::Version> {
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
    Ok(release.tag_name.parse().unwrap())
}

pub async fn download(bin: &RtBin, dir: &Path) -> Result<PathBuf, CliError> {
    let client = reqwest::Client::new();

    let bytes = client
        .get(format!(
            "https://github.com/venice-v5/venice/releases/download/{version}/{bin}",
            version = bin.version
        ))
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .map_err(CliError::Network)?
        .bytes()
        .await
        .map_err(CliError::Network)?;

    let path = dir.join(format!("{bin}"));
    tokio::fs::write(&path, bytes).await.map_err(CliError::Io)?;

    Ok(path)
}

#[cfg(test)]
mod tests {
    use semver::BuildMetadata;

    use super::{RtBin, RtBinParseError};

    #[test]
    fn bin_parse() {
        assert_eq!(
            "venice-v1.2.3.bin".parse::<RtBin>().unwrap(),
            RtBin {
                version: semver::Version::new(1, 2, 3),
            }
        );

        assert!(matches!(
            "1.2.3.bin".parse::<RtBin>().unwrap_err(),
            RtBinParseError::BadPrefix
        ));
        assert!(matches!(
            "venice-v1.2.3".parse::<RtBin>().unwrap_err(),
            RtBinParseError::BadExtension
        ));
        assert!(matches!(
            "venice-v.bin".parse::<RtBin>(),
            Err(RtBinParseError::VersionError(_))
        ));
    }

    #[test]
    fn bin_format() {
        let bin = RtBin {
            version: semver::Version::new(1, 2, 3),
        };
        assert_eq!(format!("{bin}"), "venice-v1.2.3.bin");
    }
}
