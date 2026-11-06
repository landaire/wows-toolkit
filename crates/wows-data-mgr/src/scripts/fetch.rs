//! Fetch published scripts from the `scripts` release of wows-replay-data.
//!
//! Nothing else in this crate downloads scripts: a consumer that wants them
//! asks here, per build.
//!
//! reqwest's rustls backend is enabled without a crypto provider. Install a
//! process default before building the `reqwest::Client` passed here.

use std::path::Path;
use std::path::PathBuf;

use sha2::Digest;
use sha2::Sha256;
use tokio::io::AsyncWriteExt;

use crate::error::ScriptsError;
use crate::scripts::INDEX_FILE;
use crate::scripts::ScriptsEntry;
use crate::scripts::ScriptsIndex;
use crate::scripts::Sha256Digest;

/// Download URL prefix of the `scripts` release assets.
pub const RELEASE_URL: &str = "https://github.com/landaire/wows-replay-data/releases/download/scripts";

/// Fetch the published index and replace the local copy in `output_base` with
/// it once it parses.
pub async fn update_index(client: &reqwest::Client, output_base: &Path) -> Result<ScriptsIndex, ScriptsError> {
    let url = format!("{RELEASE_URL}/{INDEX_FILE}");
    let response = get(client, &url).await?;
    let text = response.text().await.map_err(|source| ScriptsError::Http { url: url.clone(), source })?;

    let index = ScriptsIndex::parse(&url, &text)?;
    index.save(output_base)?;

    Ok(index)
}

/// `build`'s scripts.zip in the store of `output_base`, downloaded and checked
/// against `entry` when the store does not hold it yet.
///
/// Concurrent calls for one build each download their own copy and the last
/// rename wins; callers that share a store serialize them.
pub async fn download(
    client: &reqwest::Client,
    output_base: &Path,
    build: u32,
    entry: &ScriptsEntry,
) -> Result<PathBuf, ScriptsError> {
    if let Some(path) = super::local_zip(output_base, build) {
        return Ok(path);
    }

    let path = super::zip_path(output_base, build);
    let temporary = super::temporary_path(&path)?;
    let written = download_verified(client, build, entry, &temporary).await;
    super::finish_temporary(written, &temporary, &path)?;

    Ok(path)
}

async fn download_verified(
    client: &reqwest::Client,
    build: u32,
    entry: &ScriptsEntry,
    temporary: &Path,
) -> Result<(), ScriptsError> {
    let url = format!("{RELEASE_URL}/{build}.zip");
    let io = |source| ScriptsError::Io { path: temporary.to_owned(), source };

    let mut response = get(client, &url).await?;
    let mut file = tokio::fs::File::create_new(temporary).await.map_err(io)?;
    let mut hasher = Sha256::new();
    while let Some(chunk) = response.chunk().await.map_err(|source| ScriptsError::Http { url: url.clone(), source })? {
        hasher.update(&chunk);
        file.write_all(&chunk).await.map_err(io)?;
    }
    file.sync_all().await.map_err(io)?;

    let actual = Sha256Digest::from_hasher(hasher);
    if actual != entry.sha256 {
        return Err(ScriptsError::Checksum { build, expected: entry.sha256.clone(), actual });
    }

    Ok(())
}

async fn get(client: &reqwest::Client, url: &str) -> Result<reqwest::Response, ScriptsError> {
    client
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|source| ScriptsError::Http { url: url.to_owned(), source })
}
