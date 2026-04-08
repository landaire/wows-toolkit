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

    // Try exact match
    if available.contains(&target_build)
        && let Some(data) = fetch_build(target_build).await
    {
        return Ok((data, target_build));
    }

    // Fall back to nearest older build
    for &build in available.iter().rev() {
        if build >= target_build {
            continue;
        }
        if let Some(data) = fetch_build(build).await {
            return Ok((data, build));
        }
    }

    bail!("No constants found for build {target_build} or any older build")
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
