use std::path::Path;
use std::path::PathBuf;

use rootcause::prelude::*;
use wows_data_mgr::download_repo;

use crate::error::LoadError;

/// Downloads the newest published game data build into the dump root `root`
/// and returns the build's directory, for [`crate::GameData::load_dump`].
/// Objects already in the root's shared store are not fetched again.
///
/// `client` must be built after a rustls crypto provider is installed.
pub async fn download_latest(client: &reqwest::Client, root: &Path) -> Result<PathBuf, Report<LoadError>> {
    let index = download_repo::fetch_builds_index(client, download_repo::DEFAULT_REPO_BASE_URL)
        .await
        .context(LoadError::Download)?;
    let Some(latest) = index.builds.iter().max_by_key(|entry| entry.build) else {
        return Err(report!(LoadError::NoRemoteBuilds));
    };

    std::fs::create_dir_all(root).map_err(|source| report!(LoadError::Io { path: root.to_path_buf(), source }))?;
    download_repo::download_build(
        client,
        download_repo::DEFAULT_REPO_BASE_URL,
        root,
        latest.build,
        Some(&latest.version),
        false,
        &|_, _| {},
    )
    .await
    .context(LoadError::Download)?;

    Ok(root.join(&latest.dir))
}
