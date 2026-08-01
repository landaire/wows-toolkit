use std::num::NonZeroU32;

use wowsunpack::data::Version;

/// A build whose game data can actually be requested.
///
/// `Version::build` is optional because a base `major.minor.patch` carries no
/// build, and nothing can be requested for a version that has none. Wrapping
/// the version resolves that `Option` once, at construction, so no downstream
/// step re-asks a question already answered.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BuildRequest(Version);

impl BuildRequest {
    /// `None` when `version` carries no build.
    pub fn new(version: Version) -> Option<Self> {
        version.build?;
        Some(Self(version))
    }

    pub fn build(&self) -> NonZeroU32 {
        self.0.build.expect("constructed only from a version carrying a build")
    }

    /// The build as the raw `u32` the download and resolution APIs still take.
    pub fn build_u32(&self) -> u32 {
        self.build().get()
    }

    pub fn version(&self) -> Version {
        self.0
    }

    /// `major.minor.patch`, the key the game-data repository resolves on when
    /// no exact build is published.
    pub fn friendly_version(&self) -> String {
        format!("{}.{}.{}", self.0.major, self.0.minor, self.0.patch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(major: u32, minor: u32, patch: u32, build: u32) -> Version {
        Version { major, minor, patch, build: std::num::NonZeroU32::new(build) }
    }

    /// A pre-0.10 replay reports a build field of 0, which `Version` already
    /// stores as absent. Nothing can be requested for it.
    #[test]
    fn a_version_with_no_build_is_not_requestable() {
        assert!(BuildRequest::new(version(0, 6, 13, 0)).is_none());
    }

    #[test]
    fn a_version_with_a_build_is_requestable() {
        let request = BuildRequest::new(version(15, 0, 0, 11791718)).expect("build 11791718 is present");
        assert_eq!(request.build_u32(), 11791718);
    }

    /// The repository resolves on major.minor.patch when no exact build is
    /// published, so the build must not leak into the friendly version.
    #[test]
    fn the_friendly_version_drops_the_build() {
        let request = BuildRequest::new(version(15, 0, 0, 11791718)).expect("build 11791718 is present");
        assert_eq!(request.friendly_version(), "15.0.0");
    }

    /// Ordering is what makes a `BTreeMap` of requests deterministic, and the
    /// build is what distinguishes two requests of the same friendly version.
    #[test]
    fn requests_order_by_build() {
        let older = BuildRequest::new(version(14, 11, 0, 11189791)).expect("build present");
        let newer = BuildRequest::new(version(15, 0, 0, 11791718)).expect("build present");
        assert!(older < newer);
    }
}
