//! Fetch versioned game constants from the padtrack/wows-constants GitHub repo.
//!
//! This module is gated behind the `constants` feature to avoid pulling in
//! octocrab/tokio when not needed.

use rootcause::prelude::*;

/// Fetch versioned constants for a specific build from GitHub.
///
/// Tries exact build match first, then falls back to the nearest older build.
/// Returns `(json_data, actual_build_fetched)`.
pub fn fetch_versioned_constants_blocking(build: u32) -> Result<(serde_json::Value, u32), rootcause::Report> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .attach_with(|| "Failed to create tokio runtime")?;

    runtime.block_on(fetch_versioned_constants(build))
}

/// Async version of [`fetch_versioned_constants_blocking`].
/// Use this when you already have a tokio runtime (e.g. from wows-toolkit's networking thread).
pub async fn fetch_versioned_constants(target_build: u32) -> Result<(serde_json::Value, u32), rootcause::Report> {
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

/// Stateful fetcher that caches the list of upstream-available builds so the
/// listing request runs once per process even when constants are fetched for
/// many builds in a row (e.g. backfilling via `wows-data-mgr refresh-derived`).
pub struct ConstantsFetcher {
    runtime: tokio::runtime::Runtime,
    available: Vec<u32>,
}

impl ConstantsFetcher {
    /// Create a fetcher and pre-load the list of available builds.
    pub fn new() -> Result<Self, rootcause::Report> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .attach_with(|| "Failed to create tokio runtime")?;
        let available = runtime.block_on(list_available_builds())?;
        Ok(Self { runtime, available })
    }

    /// Returns `(json_data, actual_build_fetched)` if upstream has constants for
    /// `target_build` (exact match) or any older build (fallback).
    pub fn fetch(&self, target_build: u32) -> Option<(serde_json::Value, u32)> {
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
