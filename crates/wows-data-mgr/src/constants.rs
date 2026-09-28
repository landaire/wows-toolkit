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

/// GitHub API host every fetch in this module talks to, for log context.
const GITHUB_HOST: &str = "api.github.com";

/// Why a constants fetch did not produce data.
#[derive(Debug, thiserror::Error)]
pub enum ConstantsFetchError {
    #[error("GitHub API rate limit is spent for this IP address; it resets hourly")]
    RateLimited,
    #[error("GitHub returned HTTP {status}")]
    Http { status: u16 },
    #[error("could not reach GitHub: {message}")]
    Transport { message: String },
    #[error("GitHub returned data that is not valid JSON: {source}")]
    Malformed {
        #[source]
        source: serde_json::Error,
    },
}

impl ConstantsFetchError {
    /// Classify a GitHub response. A 403 with `x-ratelimit-remaining: 0` is the
    /// shape a spent unauthenticated quota takes, which is worth naming since it
    /// is shared per public IP address and resolves on its own.
    pub fn from_status(status: u16, rate_limit_remaining: Option<&str>) -> Self {
        if status == 403 && rate_limit_remaining == Some("0") {
            return Self::RateLimited;
        }
        Self::Http { status }
    }
}

/// Render `err` and its full `source()` chain on one line.
fn error_chain(err: &dyn std::error::Error) -> String {
    // Cap the walk so a pathological self-referencing source() can't loop forever.
    const MAX_DEPTH: usize = 16;
    let mut message = err.to_string();
    let mut source = err.source();
    let mut depth = 0;
    while let Some(cause) = source {
        depth += 1;
        if depth > MAX_DEPTH {
            message.push_str(": ...");
            break;
        }
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    message
}

/// Fetch the repo's root manifest.json mapping build number -> friendly version.
pub async fn fetch_constants_manifest() -> Result<std::collections::BTreeMap<u32, ConstantsVersion>, ConstantsFetchError>
{
    use http_body_util::BodyExt;
    use octocrab::params::repos::Reference;

    const PATH: &str = "manifest.json";

    let response = octocrab::instance()
        .repos("padtrack", "wows-constants")
        .raw_file(Reference::Branch("main".to_string()), PATH)
        .await
        .map_err(|e| {
            let err = ConstantsFetchError::Transport { message: error_chain(&e) };
            tracing::warn!(host = GITHUB_HOST, path = PATH, %err, "fetching constants manifest");
            err
        })?;

    let status = response.status();
    if !status.is_success() {
        let rate_limit_remaining = response.headers().get("x-ratelimit-remaining").and_then(|v| v.to_str().ok());
        let err = ConstantsFetchError::from_status(status.as_u16(), rate_limit_remaining);
        tracing::warn!(host = GITHUB_HOST, path = PATH, %err, "fetching constants manifest");
        return Err(err);
    }

    let mut body = response.into_body();
    let mut result = Vec::new();

    while let Some(frame) = body.frame().await {
        match frame {
            Ok(frame) => {
                if let Some(data) = frame.data_ref() {
                    result.extend_from_slice(data);
                }
            }
            Err(e) => {
                let err = ConstantsFetchError::Transport { message: error_chain(&e) };
                tracing::warn!(host = GITHUB_HOST, path = PATH, %err, "reading constants manifest body");
                return Err(err);
            }
        }
    }

    // Manifest keys are build numbers as strings.
    let raw: std::collections::BTreeMap<String, ConstantsVersion> =
        serde_json::from_slice(&result).map_err(|source| {
            let err = ConstantsFetchError::Malformed { source };
            tracing::warn!(host = GITHUB_HOST, path = PATH, %err, "parsing constants manifest");
            err
        })?;
    Ok(raw.into_iter().filter_map(|(k, v)| k.parse::<u32>().ok().map(|b| (b, v))).collect())
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
    let manifest = match fetch_constants_manifest().await {
        Ok(manifest) => Some(manifest),
        Err(e) => {
            tracing::warn!(
                build = target_build,
                "constants manifest unavailable, falling back to version-blind lookup: {e}"
            );
            None
        }
    };

    if let Some(manifest) = manifest
        && let Some(resolved) = resolve_manifest_build(target_build, target_version, &manifest)
    {
        match fetch_build(resolved).await {
            Ok(data) => return Ok((data, resolved)),
            Err(e) => {
                tracing::warn!(
                    build = resolved,
                    "constants build fetch failed, falling back to version-blind lookup: {e}"
                );
            }
        }
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
    if available.contains(&target_build) {
        match fetch_build(target_build).await {
            Ok(data) => return Some((data, target_build)),
            Err(e) => tracing::warn!(build = target_build, "constants build fetch failed: {e}"),
        }
    }

    for &build in available.iter().rev() {
        if build >= target_build {
            continue;
        }
        match fetch_build(build).await {
            Ok(data) => return Some((data, build)),
            Err(e) => tracing::warn!(build, "constants build fetch failed: {e}"),
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
        let manifest = match runtime.block_on(fetch_constants_manifest()) {
            Ok(manifest) => Some(manifest),
            Err(e) => {
                tracing::warn!("constants manifest unavailable, will use version-blind lookup: {e}");
                None
            }
        };
        let available = runtime.block_on(list_available_builds())?;
        Ok(Self { runtime, manifest, available })
    }

    /// Returns `(json_data, actual_build_fetched)` resolving the build via the
    /// cached manifest (friendly-version match for `target_version`), falling
    /// back to exact match or the nearest older build.
    pub fn fetch(&self, target_build: u32, target_version: Option<&str>) -> Option<(serde_json::Value, u32)> {
        if let Some(manifest) = self.manifest.as_ref()
            && let Some(resolved) = resolve_manifest_build(target_build, target_version, manifest)
        {
            match self.runtime.block_on(fetch_build(resolved)) {
                Ok(data) => return Some((data, resolved)),
                Err(e) => {
                    tracing::warn!(
                        build = resolved,
                        "constants build fetch failed, falling back to version-blind lookup: {e}"
                    );
                }
            }
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

/// The newest published constants, and the commit they were published in.
pub struct LatestConstants {
    /// The file as published, to be written out as it is.
    pub data: Vec<u8>,
    /// The commit it was read at, which is what the next check compares against.
    pub commit: String,
}

/// Fetches `data/latest.json` when the repository has moved since
/// `known_commit`.
///
/// `Ok(None)` means what the caller already has is the newest there is. The
/// commit is asked for first because that is one small request, where the file
/// itself is the whole mapping.
pub async fn fetch_latest_constants(
    known_commit: Option<&str>,
) -> Result<Option<LatestConstants>, ConstantsFetchError> {
    use http_body_util::BodyExt;
    use octocrab::params::repos::Reference;

    const PATH: &str = "data/latest.json";

    // Asked for first because it is one small request, where the file itself is
    // the whole mapping. A repository that cannot be asked is an error rather than
    // an answer: reporting it as "up to date" would tell a caller its year-old
    // mapping is the newest there is.
    let latest_commit = octocrab::instance()
        .repos("padtrack", "wows-constants")
        .list_commits()
        .per_page(1)
        .send()
        .await
        .map_err(|e| {
            let err = ConstantsFetchError::Transport { message: error_chain(&e) };
            tracing::warn!(host = GITHUB_HOST, %err, "asking which commit the constants are at");
            err
        })?
        .take_items()
        .pop()
        .map(|commit| commit.sha)
        .ok_or_else(|| {
            let err = ConstantsFetchError::Transport { message: "the repository listed no commits".to_owned() };
            tracing::warn!(host = GITHUB_HOST, %err, "asking which commit the constants are at");
            err
        })?;

    // Nothing to do when the repository is where the caller last saw it.
    if known_commit == Some(latest_commit.as_str()) {
        return Ok(None);
    }

    let response = octocrab::instance()
        .repos("padtrack", "wows-constants")
        .raw_file(Reference::Branch("main".to_string()), PATH)
        .await
        .map_err(|e| {
            let err = ConstantsFetchError::Transport { message: error_chain(&e) };
            tracing::warn!(host = GITHUB_HOST, path = PATH, %err, "fetching the latest constants");
            err
        })?;

    let status = response.status();
    if !status.is_success() {
        let rate_limit_remaining = response.headers().get("x-ratelimit-remaining").and_then(|v| v.to_str().ok());
        let err = ConstantsFetchError::from_status(status.as_u16(), rate_limit_remaining);
        tracing::warn!(host = GITHUB_HOST, path = PATH, %err, "fetching the latest constants");
        return Err(err);
    }

    let mut body = response.into_body();
    let mut data = Vec::new();
    while let Some(frame) = body.frame().await {
        match frame {
            Ok(frame) => {
                if let Some(chunk) = frame.data_ref() {
                    data.extend_from_slice(chunk);
                }
            }
            Err(e) => {
                let err = ConstantsFetchError::Transport { message: error_chain(&e) };
                tracing::warn!(host = GITHUB_HOST, path = PATH, %err, "reading the latest constants body");
                return Err(err);
            }
        }
    }

    Ok(Some(LatestConstants { data, commit: latest_commit }))
}

/// Fetch constants JSON for a specific build number.
pub async fn fetch_build(build: u32) -> Result<serde_json::Value, ConstantsFetchError> {
    use http_body_util::BodyExt;
    use octocrab::params::repos::Reference;

    let path = format!("data/versions/{build}.json");
    let response = octocrab::instance()
        .repos("padtrack", "wows-constants")
        .raw_file(Reference::Branch("main".to_string()), &path)
        .await
        .map_err(|e| {
            let err = ConstantsFetchError::Transport { message: error_chain(&e) };
            tracing::warn!(host = GITHUB_HOST, path, %err, "fetching constants build");
            err
        })?;

    let status = response.status();
    if !status.is_success() {
        let rate_limit_remaining = response.headers().get("x-ratelimit-remaining").and_then(|v| v.to_str().ok());
        let err = ConstantsFetchError::from_status(status.as_u16(), rate_limit_remaining);
        tracing::warn!(host = GITHUB_HOST, path, %err, "fetching constants build");
        return Err(err);
    }

    let mut body = response.into_body();
    let mut result = Vec::new();

    while let Some(frame) = body.frame().await {
        match frame {
            Ok(frame) => {
                if let Some(data) = frame.data_ref() {
                    result.extend_from_slice(data);
                }
            }
            Err(e) => {
                let err = ConstantsFetchError::Transport { message: error_chain(&e) };
                tracing::warn!(host = GITHUB_HOST, path, %err, "reading constants build body");
                return Err(err);
            }
        }
    }

    serde_json::from_slice(&result).map_err(|source| {
        let err = ConstantsFetchError::Malformed { source };
        tracing::warn!(host = GITHUB_HOST, path, %err, "parsing constants build");
        err
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spent_rate_limit_is_named_as_one() {
        let e = ConstantsFetchError::from_status(403, Some("0"));
        assert!(matches!(e, ConstantsFetchError::RateLimited));
        assert!(e.to_string().contains("rate limit"));
    }

    #[test]
    fn a_forbidden_response_with_quota_left_is_not_a_rate_limit() {
        assert!(matches!(ConstantsFetchError::from_status(403, Some("57")), ConstantsFetchError::Http { status: 403 }));
        assert!(matches!(ConstantsFetchError::from_status(403, None), ConstantsFetchError::Http { status: 403 }));
    }

    #[test]
    fn other_statuses_carry_their_code() {
        assert!(matches!(ConstantsFetchError::from_status(404, None), ConstantsFetchError::Http { status: 404 }));
    }
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
