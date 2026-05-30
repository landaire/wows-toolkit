/// VFS abstraction for reading files from an assets.bin PrototypeDatabase
#[cfg(feature = "vfs")]
pub mod assets_bin_vfs;
/// Main logic for parsing the game's resource index files
pub mod idx;
/// VFS abstraction for reading files from IDX/PKG archives
#[cfg(feature = "vfs")]
pub mod idx_vfs;
/// Shared winnow parsing utilities
pub mod parser_utils;
/// Utilities for helping load and maintain `.pkg` files (memmap-backed)
#[cfg(feature = "vfs-mmap")]
pub mod pkg;
/// Ship configuration (loadout) binary parser
pub mod ship_config;
// File tree serialization utilities
pub mod serialization;
/// Wrapper types for VFS data sources
#[cfg(feature = "vfs")]
pub mod wrappers;

use std::borrow::Cow;

use crate::Rc;
use crate::error::GameDataError;
use crate::game_params::types::Param;
use crate::game_types::GameParamId;
use crate::rpc::entitydefs::EntitySpec;

pub trait ResourceLoader {
    fn localized_name_from_param(&self, param: &Param) -> Option<String>;
    fn localized_name_from_id(&self, id: &str) -> Option<String>;
    fn game_param_by_id(&self, id: GameParamId) -> Option<Rc<Param>>;
    fn entity_specs(&self) -> &[EntitySpec];
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub build: u32,
}

impl Version {
    pub fn from_client_exe(version: &str) -> Version {
        let parts: Vec<_> = version.split(",").collect();
        assert!(parts.len() == 4);
        Version {
            major: parts[0].trim().parse::<u32>().unwrap(),
            minor: parts[1].trim().parse::<u32>().unwrap(),
            patch: parts[2].trim().parse::<u32>().unwrap(),
            build: parts[3].trim().parse::<u32>().unwrap(),
        }
    }

    /// Fallible variant of [`Version::from_client_exe`] that returns `None`
    /// instead of panicking when the version string is malformed (e.g. read
    /// from a corrupt or truncated replay).
    pub fn try_from_client_exe(version: &str) -> Option<Version> {
        let parts: Vec<_> = version.split(',').collect();
        if parts.len() != 4 {
            return None;
        }
        Some(Version {
            major: parts[0].trim().parse::<u32>().ok()?,
            minor: parts[1].trim().parse::<u32>().ok()?,
            patch: parts[2].trim().parse::<u32>().ok()?,
            build: parts[3].trim().parse::<u32>().ok()?,
        })
    }

    /// Extract the game version from the `Account.def` entity definition XML.
    ///
    /// The file contains a node like `<curVersion_15_1_0_11965230></curVersion_15_1_0_11965230>`
    /// whose tag name encodes the version as `curVersion_{major}_{minor}_{patch}_{build}`.
    ///
    /// Older versions use different formats:
    /// - `curVersion_release_{major}_{minor}_{patch}_{build}` (~v11-v12)
    /// - `curVersion_Release_{major}_{minor}_{patch}_{subpatch}_{build}` (v0.x era)
    pub fn from_account_def(xml: &str) -> Option<Version> {
        let doc = roxmltree::Document::parse(xml).ok()?;
        for node in doc.descendants() {
            if let Some(rest) = node.tag_name().name().strip_prefix("curVersion_") {
                // Strip optional "Release_" or "release_" prefix (used in older versions)
                let rest = rest.strip_prefix("Release_").or_else(|| rest.strip_prefix("release_")).unwrap_or(rest);
                let parts: Vec<&str> = rest.split('_').collect();
                match parts.len() {
                    // Modern: major_minor_patch_build
                    4 => {
                        return Some(Version {
                            major: parts[0].parse().ok()?,
                            minor: parts[1].parse().ok()?,
                            patch: parts[2].parse().ok()?,
                            build: parts[3].parse().ok()?,
                        });
                    }
                    // Legacy: major_minor_patch_subpatch_build (subpatch folded into patch)
                    5 => {
                        return Some(Version {
                            major: parts[0].parse().ok()?,
                            minor: parts[1].parse().ok()?,
                            patch: parts[2].parse().ok()?,
                            build: parts[4].parse().ok()?,
                        });
                    }
                    _ => {}
                }
            }
        }
        None
    }

    pub fn to_path(&self) -> String {
        format!("{}.{}.{}", self.major, self.minor, self.patch)
    }

    pub fn is_at_least(&self, other: &Version) -> bool {
        if self.major > other.major {
            true
        } else if self.major < other.major {
            false
        } else if self.minor > other.minor {
            true
        } else if self.minor < other.minor {
            false
        } else {
            self.patch >= other.patch
        }
    }
}

pub trait DataFileLoader {
    fn get(&self, path: &str) -> Result<Cow<'static, [u8]>, GameDataError>;
}

pub struct DataFileWithCallback<F> {
    callback: F,
}

impl<F> DataFileWithCallback<F>
where
    F: Fn(&str) -> Result<Cow<'static, [u8]>, GameDataError>,
{
    pub fn new(callback: F) -> Self {
        Self { callback }
    }
}

impl<F> DataFileLoader for DataFileWithCallback<F>
where
    F: Fn(&str) -> Result<Cow<'static, [u8]>, GameDataError>,
{
    fn get(&self, path: &str) -> Result<Cow<'static, [u8]>, GameDataError> {
        (self.callback)(path)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn assert_older_newer(older: Version, newer: Version) {
        assert!(newer.is_at_least(&older));
        assert!(newer.is_at_least(&newer));
        assert!(!older.is_at_least(&newer));
    }

    #[test]
    fn different_patch() {
        let older = Version::from_client_exe("0,10,9,0");
        let newer = Version::from_client_exe("0,10,10,0");
        assert_older_newer(older, newer);
    }

    #[test]
    fn different_minor() {
        let older = Version::from_client_exe("0,10,9,0");
        let newer = Version::from_client_exe("0,11,0,0");
        assert_older_newer(older, newer);
    }

    #[test]
    fn different_major() {
        let older = Version::from_client_exe("0,11,5,0");
        let newer = Version::from_client_exe("1,0,0,0");
        assert_older_newer(older, newer);
    }

    #[test]
    fn from_account_def_parses_version() {
        let xml = r#"<root><Properties><curVersion_15_1_0_11965230></curVersion_15_1_0_11965230></Properties></root>"#;
        let v = Version::from_account_def(xml).unwrap();
        assert_eq!(v.major, 15);
        assert_eq!(v.minor, 1);
        assert_eq!(v.patch, 0);
        assert_eq!(v.build, 11965230);
    }

    #[test]
    fn from_account_def_with_release_prefix() {
        let xml = r#"<root><Properties><curVersion_release_11_4_0_5624555></curVersion_release_11_4_0_5624555></Properties></root>"#;
        let v = Version::from_account_def(xml).unwrap();
        assert_eq!(v.major, 11);
        assert_eq!(v.minor, 4);
        assert_eq!(v.patch, 0);
        assert_eq!(v.build, 5624555);
    }

    #[test]
    fn from_account_def_legacy_5part_version() {
        let xml = r#"<root><Properties><curVersion_Release_0_6_13_0_296659></curVersion_Release_0_6_13_0_296659></Properties></root>"#;
        let v = Version::from_account_def(xml).unwrap();
        assert_eq!(v.major, 0);
        assert_eq!(v.minor, 6);
        assert_eq!(v.patch, 13);
        assert_eq!(v.build, 296659);
    }

    #[test]
    fn from_account_def_returns_none_on_missing() {
        let xml = r#"<root><Properties><someOtherNode/></Properties></root>"#;
        assert!(Version::from_account_def(xml).is_none());
    }
}
