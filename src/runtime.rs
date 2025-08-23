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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct RtVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct RtBin {
    pub version: RtVersion,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RtVersionParseError {
    #[error("version string ended abruptly")]
    TooShort,

    #[error("expected end of version string")]
    TooLong,

    #[error("version string malformed")]
    Malformed,

    #[error(transparent)]
    InvalidNumber(<u32 as FromStr>::Err),
}

impl FromStr for RtVersion {
    type Err = RtVersionParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // if the first character isn't 'v'
        if !s
            .chars()
            .nth(0)
            .map(|c| c == 'v')
            .ok_or(RtVersionParseError::TooShort)?
        {
            return Err(RtVersionParseError::Malformed);
        }

        let mut split = s[1..].split('.');

        let major = split
            .next()
            .ok_or(RtVersionParseError::TooShort)?
            .parse()
            .map_err(RtVersionParseError::InvalidNumber)?;

        let minor = split
            .next()
            .ok_or(RtVersionParseError::TooShort)?
            .parse()
            .map_err(RtVersionParseError::InvalidNumber)?;

        let patch = split
            .next()
            .ok_or(RtVersionParseError::TooShort)?
            .parse()
            .map_err(RtVersionParseError::InvalidNumber)?;

        if split.next().is_some() {
            return Err(RtVersionParseError::TooLong);
        }

        Ok(RtVersion {
            major,
            minor,
            patch,
        })
    }
}

impl RtVersion {
    pub const fn as_rt(self) -> RtBin {
        RtBin { version: self }
    }
}

impl Display for RtVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "v{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RtBinParseError {
    #[error(transparent)]
    VersionError(RtVersionParseError),

    #[error("runtime name did not start with 'venice-'")]
    BadPrefix,

    #[error("runtime name did not end with '.bin'")]
    BadExtension,
}

impl FromStr for RtBin {
    type Err = RtBinParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        const PREFIX: &str = "venice-";
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

pub async fn installed_versions(dir: &Path) -> Result<Vec<RtVersion>, std::io::Error> {
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

pub async fn version_exists(version: RtVersion, dir: &Path) -> Result<bool, std::io::Error> {
    tokio::fs::try_exists(dir.join(format!("venice-{version}.bin"))).await
}

const USER_AGENT: &str = concat!("venice-cli/", env!("CARGO_PKG_VERSION"));

#[derive(Deserialize)]
struct Release {
    tag_name: String,
}

pub async fn latest_version(client: &Client) -> miette::Result<RtVersion> {
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

pub async fn download(version: RtVersion, dir: &Path) -> Result<PathBuf, CliError> {
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

#[cfg(test)]
mod tests {
    use super::{RtBin, RtBinParseError, RtVersion, RtVersionParseError};

    #[test]
    fn version_parse() {
        assert_eq!(
            "v1.2.3".parse(),
            Ok(RtVersion {
                major: 1,
                minor: 2,
                patch: 3,
            }),
        );

        assert_eq!(
            "1.2.3".parse::<RtVersion>(),
            Err(RtVersionParseError::Malformed)
        );
        assert!(matches!(
            "v1a.2.3".parse::<RtVersion>(),
            Err(RtVersionParseError::InvalidNumber(_))
        ));
    }

    #[test]
    fn bin_parse() {
        assert_eq!(
            "venice-v1.2.3.bin".parse(),
            Ok(RtBin {
                version: RtVersion {
                    major: 1,
                    minor: 2,
                    patch: 3,
                }
            }),
        );

        assert_eq!(
            "v1.2.3.bin".parse::<RtBin>(),
            Err(RtBinParseError::BadPrefix)
        );
        assert_eq!(
            "venice-v1.2.3".parse::<RtBin>(),
            Err(RtBinParseError::BadExtension)
        );
        assert!(matches!(
            "venice-.bin".parse::<RtBin>(),
            Err(RtBinParseError::VersionError(_))
        ));
    }
}
