//! Newtypes for WGUS wire values that could otherwise be confused with
//! plain strings from other fields (build strings, hex hashes, part ids).

use std::fmt;
use std::str::FromStr;

use serde::Deserialize;

/// A build/version string as returned in `version_to`, e.g. `15.8.0.0.13090112`.
///
/// Kept as an opaque dotted string (WG's own format, not semver) rather than
/// parsed into components: nothing in this crate needs to compare or sort
/// builds, only to carry the identifier through unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize)]
pub struct BuildVersion(String);

impl BuildVersion {
    pub fn new(value: String) -> BuildVersion {
        BuildVersion(value)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for BuildVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A BitTorrent info-hash as returned in `torrent/hash`, hex-encoded.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize)]
pub struct InfoHash(String);

impl InfoHash {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for InfoHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A downloadable game part, as named in `part_versions`, `parts_info`, and
/// per-patch `part` elements.
///
/// Variants cover the parts seen across WG titles (report A sec 2.2, 4.1);
/// `Other` preserves any part name introduced server-side that this crate
/// does not yet special-case, rather than failing to parse.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PartName {
    Client,
    Locale,
    Sdcontent,
    Hotfix,
    Hdcontent,
    Udsound,
    Tm,
    Wwlocale,
    Other(String),
}

impl PartName {
    pub fn parse(s: &str) -> PartName {
        match s {
            "client" => PartName::Client,
            "locale" => PartName::Locale,
            "sdcontent" => PartName::Sdcontent,
            "hotfix" => PartName::Hotfix,
            "hdcontent" => PartName::Hdcontent,
            "udsound" => PartName::Udsound,
            "tm" => PartName::Tm,
            "wwlocale" => PartName::Wwlocale,
            other => PartName::Other(other.to_string()),
        }
    }
}

impl FromStr for PartName {
    // Every input has a valid representation (falls back to `Other`).
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(PartName::parse(s))
    }
}

impl fmt::Display for PartName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            PartName::Client => "client",
            PartName::Locale => "locale",
            PartName::Sdcontent => "sdcontent",
            PartName::Hotfix => "hotfix",
            PartName::Hdcontent => "hdcontent",
            PartName::Udsound => "udsound",
            PartName::Tm => "tm",
            PartName::Wwlocale => "wwlocale",
            PartName::Other(s) => s,
        };
        f.write_str(s)
    }
}

impl<'de> Deserialize<'de> for PartName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(PartName::parse(&s))
    }
}

/// A `patches_chain/@type` value (report A sec 3-4: observed values
/// `install`, `preinstall`). `Other` preserves any value this crate does not
/// yet special-case rather than failing to parse.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ChainType {
    Install,
    Preinstall,
    Other(String),
}

impl ChainType {
    pub fn parse(s: &str) -> ChainType {
        match s {
            "install" => ChainType::Install,
            "preinstall" => ChainType::Preinstall,
            other => ChainType::Other(other.to_string()),
        }
    }
}

impl fmt::Display for ChainType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            ChainType::Install => "install",
            ChainType::Preinstall => "preinstall",
            ChainType::Other(s) => s,
        };
        f.write_str(s)
    }
}

impl<'de> Deserialize<'de> for ChainType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(ChainType::parse(&s))
    }
}
