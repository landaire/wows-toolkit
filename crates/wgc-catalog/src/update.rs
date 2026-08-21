//! Compare a WGC install's installed part versions against WGUS's newest
//! catalog chain and report what, if anything, is out of date.
//!
//! [`diff_parts`] and [`latest_build_from_responses`] are pure (no network),
//! so update-detection is unit-testable against fixture XML;
//! [`CatalogClient::check_updates`] wraps them with the two fetches.

use crate::client::CatalogClient;
use crate::client::CurrentPartVersions;
use crate::error::CatalogError;
use crate::game_info::GameInfo;
use crate::models::BuildVersion;
use crate::models::ChainType;
use crate::models::InfoHash;
use crate::models::MetadataResponse;
use crate::models::PartName;
use crate::models::Patch;
use crate::models::PatchesChain;
use crate::models::PatchesChainResponse;

/// `chain_id` WGCheck sends on the bootstrap `metadata` call, before it has
/// learned a real chain id (report A / ledger's validated capture command).
const BOOTSTRAP_CHAIN_ID: &str = "unknown";

/// A part's status relative to the newest chain WGUS offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PartStatus {
    UpToDate,
    UpdateAvailable { from: BuildVersion, to: BuildVersion },
    NotInstalled { to: BuildVersion },
}

/// One file within a part's build, as WGUS lists it in `patches_chain/patch/files`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub name: String,
    pub size: u64,
    pub unpacked_size: u64,
}

/// Everything needed to download one part's newest build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartDownload {
    pub part: PartName,
    pub version_to: BuildVersion,
    pub info_hash: InfoHash,
    pub torrent_urls: Vec<String>,
    /// The catalog response's first `web_seeds/url` base, if any. WGUS
    /// advertises web seeds per response, not per part, so every part in a
    /// response shares the same base.
    pub web_seed_base: Option<String>,
    pub files: Vec<FileEntry>,
}

/// The newest build WGUS offers for an install, plus how it compares to
/// what is installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatestBuild {
    /// The chain this build was taken from (`install` or `preinstall`).
    pub chain_type: ChainType,
    pub parts: Vec<(PartName, PartStatus)>,
    pub downloads: Vec<PartDownload>,
    /// WGUS's suggested webseed concurrency (`web_seeds/url/@threads`), if
    /// the response carried a `web_seeds` advisory at all. The downloader
    /// uses this (clamped to a sane range) as its download parallelism,
    /// same as WGC's own client would.
    pub web_seed_threads: Option<u32>,
}

/// Part names in first-seen order from `chain`'s patch list, deduplicated.
fn part_names_in_chain(chain: &PatchesChain) -> Vec<PartName> {
    let mut seen = Vec::new();
    for patch in &chain.patch {
        if !seen.contains(&patch.part) {
            seen.push(patch.part.clone());
        }
    }
    seen
}

/// The terminal (newest) patch for `part` within `chain`.
///
/// WGCheck's own algorithm (report A sec 3-4, "How WGCheck derives 'latest
/// version'"): within a chain, for each part, take the patch whose
/// `version_to` sorts last by (string length, then lexicographic) - i.e. the
/// longest `version_to`, ties broken lexicographically. This is a value
/// comparison, not list position: WGUS's response order per part is not
/// documented and must not be relied on.
fn terminal_patch<'a>(chain: &'a PatchesChain, part: &PartName) -> Option<&'a Patch> {
    chain.patch.iter().filter(|p| &p.part == part).max_by_key(|p| version_to_sort_key(p))
}

fn version_to_sort_key(patch: &Patch) -> (usize, &str) {
    let v = patch.version_to.as_str();
    (v.len(), v)
}

/// Every part's terminal patch within `chain`, computed once and shared by
/// [`diff_parts`] and download collection so a `LatestBuild` does not scan
/// `chain.patch` twice per part.
fn terminal_patches(chain: &PatchesChain) -> Vec<(PartName, &Patch)> {
    part_names_in_chain(chain).into_iter().filter_map(|part| terminal_patch(chain, &part).map(|p| (part, p))).collect()
}

fn status_for(installed_parts: &[crate::game_info::PartVersion], part: &PartName, patch: &Patch) -> PartStatus {
    match installed_parts.iter().find(|p| &p.name == part) {
        None => PartStatus::NotInstalled { to: patch.version_to.clone() },
        Some(installed) if installed.installed == patch.version_to => PartStatus::UpToDate,
        Some(installed) => {
            PartStatus::UpdateAvailable { from: installed.installed.clone(), to: patch.version_to.clone() }
        }
    }
}

/// Compare `game_info`'s installed part versions against `chain`'s terminal
/// version per part. Only reports on parts that appear in `chain`.
pub fn diff_parts(game_info: &GameInfo, chain: &PatchesChain) -> Vec<(PartName, PartStatus)> {
    terminal_patches(chain)
        .into_iter()
        .map(|(part, patch)| {
            let status = status_for(&game_info.installed_parts, &part, patch);
            (part, status)
        })
        .collect()
}

/// Choose which chain (`preinstall` or `install`) to treat as "the newest
/// build" (report A sec 3-4, `GetPatchChainType`): `preinstall` iff it
/// exists and its terminal client patch already matches the installed
/// client version (meaning the preinstalled content is ready to switch to);
/// otherwise `install`.
fn choose_chain<'a>(
    response: &'a PatchesChainResponse,
    game_info: &GameInfo,
) -> Result<&'a PatchesChain, CatalogError> {
    let installed_client = game_info.installed_parts.iter().find(|p| p.name == PartName::Client);

    if let Some(installed_client) = installed_client
        && let Some(preinstall) = response.patches_chain.iter().find(|c| c.chain_type == ChainType::Preinstall)
        && let Some(client_patch) = terminal_patch(preinstall, &PartName::Client)
        && client_patch.version_to == installed_client.installed
    {
        return Ok(preinstall);
    }

    response
        .patches_chain
        .iter()
        .find(|c| c.chain_type == ChainType::Install)
        .ok_or_else(|| CatalogError::NoChain { game_id: game_info.game_id.clone(), chain_type: "install" })
}

fn build_downloads(terminal: &[(PartName, &Patch)], web_seeds: &crate::models::WebSeeds) -> Vec<PartDownload> {
    // WGC's own client only ever reads a single web_seeds/url child (report A
    // sec 4.2); take the first one WGUS returns, per response (not per part).
    let web_seed_base = web_seeds.url.first().map(|w| w.base.clone());
    terminal
        .iter()
        .map(|(part, patch)| PartDownload {
            part: part.clone(),
            version_to: patch.version_to.clone(),
            info_hash: patch.torrent.hash.clone(),
            torrent_urls: patch.torrent.urls.url.clone(),
            web_seed_base: web_seed_base.clone(),
            files: patch
                .files
                .file
                .iter()
                .map(|f| FileEntry { name: f.name.clone(), size: f.size, unpacked_size: f.unpacked_size })
                .collect(),
        })
        .collect()
}

/// Pure core of [`CatalogClient::check_updates`]: derive a [`LatestBuild`]
/// from already-fetched responses. Split out so update-detection is
/// unit-testable without network access.
pub fn latest_build_from_responses(
    game_info: &GameInfo,
    patches: &PatchesChainResponse,
) -> Result<LatestBuild, CatalogError> {
    let chosen = choose_chain(patches, game_info)?;
    let terminal = terminal_patches(chosen);
    let parts = terminal
        .iter()
        .map(|(part, patch)| (part.clone(), status_for(&game_info.installed_parts, part, patch)))
        .collect();
    Ok(LatestBuild {
        chain_type: chosen.chain_type.clone(),
        parts,
        downloads: build_downloads(&terminal, &patches.web_seeds),
        web_seed_threads: patches.web_seeds.url.first().map(|w| w.threads),
    })
}

impl CatalogClient {
    /// Fetch metadata and the full newest patches chain for `game_info`,
    /// then diff installed part versions against it.
    pub async fn check_updates(&self, game_info: &GameInfo) -> Result<LatestBuild, CatalogError> {
        let metadata: MetadataResponse =
            self.fetch_metadata(&game_info.update_base_url, &game_info.game_id, BOOTSTRAP_CHAIN_ID).await?;
        let patches = self
            .fetch_patches_chain(
                &game_info.update_base_url,
                &game_info.game_id,
                metadata.chain_id(),
                metadata.metadata_version(),
                Some(&game_info.localization),
                &CurrentPartVersions::full(),
            )
            .await?;
        latest_build_from_responses(game_info, &patches)
    }
}
