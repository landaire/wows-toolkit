//! Game version: a `major.minor.patch` triple plus an optional `build` number.

use std::num::NonZeroU32;

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    /// The monotonic build number, if known. `None` for a base version that
    /// only carries `major.minor.patch`.
    pub build: Option<NonZeroU32>,
}

impl Version {
    pub fn from_client_exe(version: &str) -> Version {
        let parts: Vec<_> = version.split(",").collect();
        assert!(parts.len() == 4);
        Version {
            major: parts[0].trim().parse::<u32>().unwrap(),
            minor: parts[1].trim().parse::<u32>().unwrap(),
            patch: parts[2].trim().parse::<u32>().unwrap(),
            build: NonZeroU32::new(parts[3].trim().parse::<u32>().unwrap()),
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
            build: NonZeroU32::new(parts[3].trim().parse::<u32>().ok()?),
        })
    }

    /// The numeric build, if known.
    pub fn build_number(&self) -> Option<u32> {
        self.build.map(NonZeroU32::get)
    }

    /// Extract the game version from the `Account.def` entity definition XML.
    ///
    /// The file contains a node like `<curVersion_15_1_0_11965230></curVersion_15_1_0_11965230>`
    /// whose tag name encodes the version as `curVersion_{major}_{minor}_{patch}_{build}`.
    ///
    /// Older versions use different formats:
    /// - `curVersion_release_{major}_{minor}_{patch}_{build}` (~v11-v12)
    /// - `curVersion_Release_{major}_{minor}_{patch}_{subpatch}_{build}` (v0.x era)
    #[cfg(feature = "parsing")]
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
                            build: NonZeroU32::new(parts[3].parse().ok()?),
                        });
                    }
                    // Legacy: major_minor_patch_subpatch_build (subpatch folded into patch)
                    5 => {
                        return Some(Version {
                            major: parts[0].parse().ok()?,
                            minor: parts[1].parse().ok()?,
                            patch: parts[2].parse().ok()?,
                            build: NonZeroU32::new(parts[4].parse().ok()?),
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

    /// A base version `(major, minor, patch)` with no build component (`build`
    /// is `None`). Useful for keying version-gated tables, where entries take
    /// effect at a friendly version regardless of build. Compare against a full
    /// version with [`Self::is_at_least`], which ignores the build field.
    pub const fn base(major: u32, minor: u32, patch: u32) -> Version {
        Version { major, minor, patch, build: None }
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

    #[cfg(feature = "parsing")]
    #[test]
    fn from_account_def_parses_version() {
        let xml = r#"<root><Properties><curVersion_15_1_0_11965230></curVersion_15_1_0_11965230></Properties></root>"#;
        let v = Version::from_account_def(xml).unwrap();
        assert_eq!(v.major, 15);
        assert_eq!(v.minor, 1);
        assert_eq!(v.patch, 0);
        assert_eq!(v.build, NonZeroU32::new(11965230));
    }

    #[cfg(feature = "parsing")]
    #[test]
    fn from_account_def_with_release_prefix() {
        let xml = r#"<root><Properties><curVersion_release_11_4_0_5624555></curVersion_release_11_4_0_5624555></Properties></root>"#;
        let v = Version::from_account_def(xml).unwrap();
        assert_eq!(v.major, 11);
        assert_eq!(v.minor, 4);
        assert_eq!(v.patch, 0);
        assert_eq!(v.build, NonZeroU32::new(5624555));
    }

    #[cfg(feature = "parsing")]
    #[test]
    fn from_account_def_legacy_5part_version() {
        let xml = r#"<root><Properties><curVersion_Release_0_6_13_0_296659></curVersion_Release_0_6_13_0_296659></Properties></root>"#;
        let v = Version::from_account_def(xml).unwrap();
        assert_eq!(v.major, 0);
        assert_eq!(v.minor, 6);
        assert_eq!(v.patch, 13);
        assert_eq!(v.build, NonZeroU32::new(296659));
    }

    #[cfg(feature = "parsing")]
    #[test]
    fn from_account_def_returns_none_on_missing() {
        let xml = r#"<root><Properties><someOtherNode/></Properties></root>"#;
        assert!(Version::from_account_def(xml).is_none());
    }
}
