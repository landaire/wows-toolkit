//! Version and build detection for an install.

use std::path::Path;

use crate::GameInstall;

/// Largest numeric build directory under `<path>/bin`, if any.
fn latest_build(path: &Path) -> Option<u32> {
    let bin = path.join("bin");
    let mut builds: Vec<u32> = std::fs::read_dir(&bin)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter_map(|e| e.file_name().to_str().and_then(|n| n.parse::<u32>().ok()))
        .collect();
    builds.sort_unstable();
    builds.pop()
}

/// True when the path looks like a WoWs install (has bin/<build>/).
pub fn is_probably_wows(path: &Path) -> bool {
    latest_build(path).is_some()
}

/// Fill `build` and `version`. Leaves them `None` on failure: an install whose
/// version cannot be read is still launchable, so this is a genuinely-correct
/// absence rather than a papered-over error.
pub fn detect(install: &mut GameInstall) {
    let Some(build) = latest_build(&install.path) else {
        return;
    };
    install.build = Some(build);
    match wows_data_mgr::detect::detect_version_at_path(&install.path, build) {
        Ok(version_str) => {
            // `wowsunpack::data::Version` has no string constructor; split the
            // "major.minor.patch" text the same way `build_requests_from_updates`
            // (wows-toolkit/src/tab_state.rs) does. Version stays None if major
            // does not parse; build is still recorded either way.
            let mut parts = version_str.split('.').filter_map(|p| p.trim().parse::<u32>().ok());
            install.version = parts.next().map(|major| wowsunpack::data::Version {
                major,
                minor: parts.next().unwrap_or(0),
                patch: parts.next().unwrap_or(0),
                build: std::num::NonZeroU32::new(build),
            });
        }
        Err(e) => tracing::debug!("version detect failed at {}: {e}", install.path.display()),
    }
}
