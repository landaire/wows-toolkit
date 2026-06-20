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
    ///
    /// The friendly version is normalized to match the replay header
    /// (`clientVersionFromExe`), which is the source of truth: for the 0.7.8-0.11.7
    /// era WG's `Account.def` wrote the version with the leading `0.` stripped and
    /// shifted (`release_11_0_0` for 0.11.0, `release_7_11_0` for 0.7.11), while the
    /// exe and replays report `0.X.Y`. See [`Self::friendly_from_account_def_parts`].
    #[cfg(feature = "parsing")]
    pub fn from_account_def(xml: &str) -> Option<Version> {
        let doc = roxmltree::Document::parse(xml).ok()?;
        for node in doc.descendants() {
            if let Some(rest) = node.tag_name().name().strip_prefix("curVersion_") {
                // Strip optional "Release_" or "release_" prefix (used in older versions)
                let rest = rest.strip_prefix("Release_").or_else(|| rest.strip_prefix("release_")).unwrap_or(rest);
                let parts: Vec<&str> = rest.split('_').collect();
                // Modern: major_minor_patch_build. Legacy: major_minor_patch_subpatch_build.
                let build_idx = match parts.len() {
                    4 => 3,
                    5 => 4,
                    _ => continue,
                };
                let (major, minor, patch) = Self::friendly_from_account_def_parts(
                    parts[0].parse().ok()?,
                    parts[1].parse().ok()?,
                    parts[2].parse().ok()?,
                );
                return Some(Version { major, minor, patch, build: NonZeroU32::new(parts[build_idx].parse().ok()?) });
            }
        }
        None
    }

    /// Map an `Account.def` `curVersion` triple to the friendly version the replay
    /// header reports. A `major` of 1..=11 never existed as a real WoWS version
    /// (the game was `0.x` until 12.0.0), so it is unambiguously the stripped form
    /// WG wrote for the 0.7.8-0.11.7 builds; the friendly version is `0.{major}.{minor}`
    /// (the stripped patch is a hotfix marker the friendly version does not carry).
    /// `major` of 0 (older `0.x`) and >= 12 (the post-rename scheme) are already friendly.
    #[cfg(feature = "parsing")]
    fn friendly_from_account_def_parts(major: u32, minor: u32, patch: u32) -> (u32, u32, u32) {
        if (1..=11).contains(&major) { (0, major, minor) } else { (major, minor, patch) }
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

    /// Whether the base `major.minor.patch` versions are equal, ignoring the build
    /// entirely. Unlike [`Self::matches`], two *different* concrete builds of the same
    /// base version compare equal here. Pairs with [`Self::base`]; use it to test
    /// friendly-version equality when the builds may legitimately differ.
    pub fn base_eq(&self, other: &Version) -> bool {
        (self.major, self.minor, self.patch) == (other.major, other.minor, other.patch)
    }

    /// Whether this version matches `other` for relaxed version-gating, where the build
    /// is an optional refinement of the friendly `major.minor.patch`. The friendly parts
    /// must be equal; the build narrows the match only when BOTH sides specify one:
    /// `15.4.0` build N matches `15.4.0` with no build (and vice versa), but two different
    /// concrete builds of the same friendly version do not match. Not transitive - use `==`
    /// for strict equality.
    pub fn matches(&self, other: &Version) -> bool {
        self.base_eq(other)
            && match (self.build, other.build) {
                (Some(a), Some(b)) => a == b,
                _ => true,
            }
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

    #[test]
    fn matches_build_optional_refinement() {
        let no_build = Version::base(15, 4, 0);
        let build_a = Version { major: 15, minor: 4, patch: 0, build: NonZeroU32::new(100) };
        let build_b = Version { major: 15, minor: 4, patch: 0, build: NonZeroU32::new(200) };

        // Same friendly + one side has no build -> matches (both directions).
        assert!(no_build.matches(&build_a));
        assert!(build_a.matches(&no_build));

        // Same friendly + both builds equal -> matches.
        assert!(build_a.matches(&build_a));

        // Same friendly + both builds differ -> does NOT match.
        assert!(!build_a.matches(&build_b));
        assert!(!build_b.matches(&build_a));

        // Different friendly -> does NOT match regardless of build.
        let other_friendly = Version { major: 15, minor: 3, patch: 0, build: NonZeroU32::new(100) };
        assert!(!build_a.matches(&other_friendly));
        assert!(!other_friendly.matches(&build_a));
        assert!(!no_build.matches(&Version::base(15, 3, 0)));
    }

    #[test]
    fn base_eq_ignores_build() {
        let build_a = Version { major: 15, minor: 4, patch: 0, build: NonZeroU32::new(100) };
        let build_b = Version { major: 15, minor: 4, patch: 0, build: NonZeroU32::new(200) };
        // Same friendly, different builds -> base_eq true (where matches would be false).
        assert!(build_a.base_eq(&build_b));
        assert!(!build_a.matches(&build_b));
        // Same friendly, one build None -> still equal.
        assert!(build_a.base_eq(&Version::base(15, 4, 0)));
        // Different friendly -> not equal.
        assert!(!build_a.base_eq(&Version::base(15, 3, 0)));
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
    fn from_account_def_release_prefix_normalizes_stripped_version() {
        // WG's Account.def wrote `release_11_4_0` for friendly version 0.11.4
        // (build 5624555); the exe and replay clientVersionFromExe both report
        // `0,11,4`, so the detector must normalize to match.
        let xml = r#"<root><Properties><curVersion_release_11_4_0_5624555></curVersion_release_11_4_0_5624555></Properties></root>"#;
        let v = Version::from_account_def(xml).unwrap();
        assert_eq!((v.major, v.minor, v.patch), (0, 11, 4));
        assert_eq!(v.build, NonZeroU32::new(5624555));
    }

    #[cfg(feature = "parsing")]
    #[test]
    fn from_account_def_stripped_versions_match_replay_header() {
        // Each case is (Account.def stripped triple, friendly version the replay reports).
        for (tag, build, want) in [
            ("curVersion_release_11_0_0_5045210", 5045210u32, (0, 11, 0)),
            ("curVersion_release_7_11_0_1167524", 1167524, (0, 7, 11)),
            ("curVersion_release_10_0_0_3343484", 3343484, (0, 10, 0)),
        ] {
            let xml = format!("<root><{tag}></{tag}></root>");
            let v = Version::from_account_def(&xml).unwrap();
            assert_eq!((v.major, v.minor, v.patch), want, "tag {tag}");
            assert_eq!(v.build, NonZeroU32::new(build), "tag {tag}");
        }
    }

    #[cfg(feature = "parsing")]
    #[test]
    fn from_account_def_friendly_versions_untouched() {
        // major 0 (older 0.x, here the 5-part `0_11_8_0` format) and major >= 12
        // (post-rename scheme) are already friendly and must not be shifted.
        let xml = r#"<root><curVersion_0_11_8_0_6223574></curVersion_0_11_8_0_6223574></root>"#;
        let v = Version::from_account_def(xml).unwrap();
        assert_eq!((v.major, v.minor, v.patch), (0, 11, 8));
        let xml = r#"<root><curVersion_12_0_0_6775398></curVersion_12_0_0_6775398></root>"#;
        let v = Version::from_account_def(xml).unwrap();
        assert_eq!((v.major, v.minor, v.patch), (12, 0, 0));
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
