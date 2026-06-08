//! Fetch versioned game constants from the padtrack/wows-constants GitHub repo.
//!
//! This module is gated behind the `constants` feature to avoid pulling in
//! octocrab/tokio when not needed.

use rootcause::prelude::*;

/// One entry in the repo's root `manifest.json`: the friendly version a build
/// maps to. `version` is `major.minor`; `patch` is the third component.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ConstantsVersion {
    pub version: String,
    #[serde(default)]
    pub patch: f64,
}

impl ConstantsVersion {
    /// Reconstruct the full friendly version, e.g. version "15.4" + patch 0.0 -> "15.4.0".
    pub fn friendly_version(&self) -> String {
        format!("{}.{}", self.version, self.patch as i64)
    }
}

/// Fetch the repo's root manifest.json mapping build number -> friendly version.
pub async fn fetch_constants_manifest() -> Option<std::collections::BTreeMap<u32, ConstantsVersion>> {
    use http_body_util::BodyExt;
    use octocrab::params::repos::Reference;

    let response = octocrab::instance()
        .repos("padtrack", "wows-constants")
        .raw_file(Reference::Branch("main".to_string()), "manifest.json")
        .await
        .ok()?;

    let mut body = response.into_body();
    let mut result = Vec::new();

    while let Some(frame) = body.frame().await {
        match frame {
            Ok(frame) => {
                if let Some(data) = frame.data_ref() {
                    result.extend_from_slice(data);
                }
            }
            Err(_) => return None,
        }
    }

    // Manifest keys are build numbers as strings.
    let raw: std::collections::BTreeMap<String, ConstantsVersion> = serde_json::from_slice(&result).ok()?;
    Some(raw.into_iter().filter_map(|(k, v)| k.parse::<u32>().ok().map(|b| (b, v))).collect())
}

/// Resolve which build's constants to fetch for a replay's (build, friendly_version),
/// given the repo manifest. Exact build wins; else the highest build whose friendly
/// version matches; else None.
pub fn resolve_manifest_build(
    target_build: u32,
    target_version: Option<&str>,
    manifest: &std::collections::BTreeMap<u32, ConstantsVersion>,
) -> Option<u32> {
    if manifest.contains_key(&target_build) {
        return Some(target_build);
    }
    let want = target_version?;
    manifest.iter().filter(|(_, v)| v.friendly_version() == want).map(|(b, _)| *b).max()
}

/// Fetch versioned constants for a specific build from GitHub.
///
/// Resolves the build to fetch via the repo manifest (friendly-version match,
/// so cross-region replays find the matching build), then falls back to exact
/// build match or the nearest older build. Returns `(json_data, actual_build_fetched)`.
pub fn fetch_versioned_constants_blocking(
    build: u32,
    target_version: Option<&str>,
) -> Result<(serde_json::Value, u32), rootcause::Report> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .attach_with(|| "Failed to create tokio runtime")?;

    runtime.block_on(fetch_versioned_constants(build, target_version))
}

/// Async version of [`fetch_versioned_constants_blocking`].
/// Use this when you already have a tokio runtime (e.g. from wows-toolkit's networking thread).
pub async fn fetch_versioned_constants(
    target_build: u32,
    target_version: Option<&str>,
) -> Result<(serde_json::Value, u32), rootcause::Report> {
    if let Some(manifest) = fetch_constants_manifest().await
        && let Some(resolved) = resolve_manifest_build(target_build, target_version, &manifest)
        && let Some(data) = fetch_build(resolved).await
    {
        return Ok((data, resolved));
    }
    // Fallback: version-blind exact-then-nearest-older.
    let available = list_available_builds().await?;
    pick_constants(target_build, &available)
        .await
        .ok_or_else(|| report!("No constants found for build {target_build} or any older build"))
}

/// Select constants for `target_build` given a pre-fetched `available` list:
/// exact match first, otherwise nearest older build. Returns `None` only when
/// nothing usable is published upstream.
async fn pick_constants(target_build: u32, available: &[u32]) -> Option<(serde_json::Value, u32)> {
    if available.contains(&target_build)
        && let Some(data) = fetch_build(target_build).await
    {
        return Some((data, target_build));
    }

    for &build in available.iter().rev() {
        if build >= target_build {
            continue;
        }
        if let Some(data) = fetch_build(build).await {
            return Some((data, build));
        }
    }
    None
}

/// Stateful fetcher that caches the upstream manifest and available-build list
/// so the listing requests run once per process even when constants are fetched
/// for many builds in a row (e.g. backfilling via `wows-data-mgr refresh-derived`).
pub struct ConstantsFetcher {
    runtime: tokio::runtime::Runtime,
    manifest: Option<std::collections::BTreeMap<u32, ConstantsVersion>>,
    available: Vec<u32>,
}

impl ConstantsFetcher {
    /// Create a fetcher and pre-load the manifest and list of available builds.
    pub fn new() -> Result<Self, rootcause::Report> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .attach_with(|| "Failed to create tokio runtime")?;
        let manifest = runtime.block_on(fetch_constants_manifest());
        let available = runtime.block_on(list_available_builds())?;
        Ok(Self { runtime, manifest, available })
    }

    /// Returns `(json_data, actual_build_fetched)` resolving the build via the
    /// cached manifest (friendly-version match for `target_version`), falling
    /// back to exact match or the nearest older build.
    pub fn fetch(&self, target_build: u32, target_version: Option<&str>) -> Option<(serde_json::Value, u32)> {
        if let Some(manifest) = self.manifest.as_ref()
            && let Some(resolved) = resolve_manifest_build(target_build, target_version, manifest)
            && let Some(data) = self.runtime.block_on(fetch_build(resolved))
        {
            return Some((data, resolved));
        }
        self.runtime.block_on(pick_constants(target_build, &self.available))
    }
}

/// List all available build numbers from the padtrack/wows-constants repo.
pub async fn list_available_builds() -> Result<Vec<u32>, rootcause::Report> {
    let items = octocrab::instance()
        .repos("padtrack", "wows-constants")
        .get_content()
        .path("data/versions")
        .r#ref("main")
        .send()
        .await
        .attach_with(|| "Failed to list constants builds from GitHub")?;

    let mut builds: Vec<u32> =
        items.items.iter().filter_map(|item| item.name.strip_suffix(".json")?.parse::<u32>().ok()).collect();
    builds.sort();
    Ok(builds)
}

/// Fetch constants JSON for a specific build number. Returns None if not found.
pub async fn fetch_build(build: u32) -> Option<serde_json::Value> {
    use http_body_util::BodyExt;
    use octocrab::params::repos::Reference;

    let path = format!("data/versions/{build}.json");
    let response = octocrab::instance()
        .repos("padtrack", "wows-constants")
        .raw_file(Reference::Branch("main".to_string()), &path)
        .await
        .ok()?;

    let mut body = response.into_body();
    let mut result = Vec::new();

    while let Some(frame) = body.frame().await {
        match frame {
            Ok(frame) => {
                if let Some(data) = frame.data_ref() {
                    result.extend_from_slice(data);
                }
            }
            Err(_) => return None,
        }
    }

    serde_json::from_slice(&result).ok()
}

#[cfg(test)]
mod manifest_tests {
    use std::collections::BTreeMap;

    use super::*;
    fn m() -> BTreeMap<u32, ConstantsVersion> {
        let mut m = BTreeMap::new();
        m.insert(11965230, ConstantsVersion { version: "15.1".into(), patch: 0.0 });
        m.insert(12506899, ConstantsVersion { version: "15.4".into(), patch: 0.0 });
        m
    }
    #[test]
    fn friendly_version_reconstructs() {
        assert_eq!(ConstantsVersion { version: "15.4".into(), patch: 0.0 }.friendly_version(), "15.4.0");
    }
    #[test]
    fn exact_build_wins() {
        assert_eq!(resolve_manifest_build(12506899, Some("15.4.0"), &m()), Some(12506899));
    }
    #[test]
    fn cross_region_resolves_by_version() {
        // CN build not in manifest, same friendly version -> RoW build.
        assert_eq!(resolve_manifest_build(99999999, Some("15.1.0"), &m()), Some(11965230));
    }
    #[test]
    fn no_match_is_none() {
        assert_eq!(resolve_manifest_build(99999999, Some("9.9.9"), &m()), None);
        assert_eq!(resolve_manifest_build(99999999, None, &m()), None);
    }
}
