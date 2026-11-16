//! The WGC layer that runs a Steam install the way Game Center runs its own install. The Steam
//! and WGC copies of a build differ only in WG's `locale` part: the Steam copy carries the Steam
//! build of `platform64.dll`, which alone picks the storefront, plus `steam_api64.dll`. The layer
//! holds WG's WGC files from that part, a generated `game_info.xml`, and whiteouts that hide the
//! Steam API and any Steam-mode realm file. It is fetched from WG's CDN per `locale` version,
//! cached under the app data, and laid over the install as a `wgc_launch` delta.

use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::Ordering;

use serde::Deserialize;
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;
use wgc_catalog::models::PartName;
use wgc_catalog::update::PartDownload;
use wgc_catalog::update::PartStatus;

use crate::error::WgcLayerError;
use crate::game::GameBuild;

/// The WoWs game id whose `locale` part the layer comes from. Steam's copy is the worldwide
/// build.
const GAME_ID: &str = "WOWS.WW.PRODUCTION";

/// Recorded beside a finished layer. Written last, so a layer without it is incomplete.
const MANIFEST_FILE: &str = "layer.json";
const DELTA_DIR: &str = "delta";
/// Holds each `locale` version's fetch lock and staging directory, apart from the layers.
const WORK_DIR: &str = "work";

/// Every `.dspkg` of a part names it last: `wows.ww_<version>_locale.dspkg`.
const LOCALE_PACKAGE_SUFFIX: &str = "_locale.dspkg";

/// A finished layer on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WgcLayer {
    dir: PathBuf,
    manifest: LayerManifest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct LayerManifest {
    locale_version: String,
    /// Every part's version in the catalog chain, which `game_info.xml` reports.
    parts: Vec<LayerPart>,
    /// The `bin/<build>` directories the layer replaces files in.
    builds: Vec<u32>,

    /// The files taken from the package, checked before the layer is used.
    files: Vec<LayerFile>,
    /// Delta-relative whiteouts written with the layer. Those written for an install are not
    /// recorded.
    whiteouts: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct LayerPart {
    name: String,
    version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct LayerFile {
    /// Relative to the delta directory.
    path: PathBuf,
    size: u64,
    sha256: String,
}

impl LayerFile {
    fn read(delta: &Path, rel: &Path) -> Result<Self, WgcLayerError> {
        let path = delta.join(rel);
        let size = std::fs::metadata(&path).map_err(WgcLayerError::io(&path))?.len();

        Ok(Self { path: rel.to_path_buf(), size, sha256: sha256_hex(&path)? })
    }

    /// Whether the file under `delta` is a regular file of the recorded size and hash.
    fn is_intact(&self, delta: &Path) -> Result<bool, WgcLayerError> {
        let path = delta.join(&self.path);
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(source) => return Err(WgcLayerError::Io { path, source }),
        };
        if !metadata.is_file() || metadata.len() != self.size {
            return Ok(false);
        }

        Ok(sha256_hex(&path)? == self.sha256)
    }
}

impl WgcLayer {
    /// The directory laid over the install.
    pub fn delta_dir(&self) -> PathBuf {
        self.dir.join(DELTA_DIR)
    }

    /// The `locale` part version the layer was built from.
    pub fn locale_version(&self) -> &str {
        &self.manifest.locale_version
    }

    pub fn covers(&self, build: GameBuild) -> bool {
        self.manifest.builds.contains(&build.get())
    }

    /// Writes what depends on the install: `game_info.xml` in the game language picked in
    /// Steam, and whiteouts for every `currentrealm.txt` the install has. Call under the session
    /// lock, while no game runs from the layer.
    pub(crate) fn prepare_for(&self, install: &Path, steam_language: Option<&str>) -> Result<(), WgcLayerError> {
        let delta = self.delta_dir();
        let newest = newest_build(install).map_err(WgcLayerError::io(install))?;
        let language = newest
            .and_then(|build| content_language(&install.join("bin").join(build.to_string()), steam_language))
            .unwrap_or_else(|| {
                // WGC's own default, which every WoWs realm ships.
                "en".to_owned()
            });
        let game_info = game_info_xml(&self.manifest.parts, &language);
        let game_info_path = delta.join("game_info.xml");
        if std::fs::read_to_string(&game_info_path).ok().as_deref() != Some(game_info.as_str()) {
            std::fs::write(&game_info_path, game_info).map_err(WgcLayerError::io(&game_info_path))?;
        }

        for rel in realm_files(install)? {
            let target = delta.join(&rel);
            if target.symlink_metadata().is_ok() {
                continue;
            }
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(WgcLayerError::io(parent))?;
            }
            wgc_launch::create_whiteout(&target).map_err(WgcLayerError::io(&target))?;
        }

        Ok(())
    }

    /// The finished layer at `dir`. A layer whose record, files or whiteouts do not match what it
    /// was built with is removed and reported absent, so it is fetched again.
    fn open(dir: PathBuf) -> Result<Option<Self>, WgcLayerError> {
        let path = dir.join(MANIFEST_FILE);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(WgcLayerError::Io { path, source }),
        };

        match serde_json::from_str::<LayerManifest>(&text) {
            Ok(manifest) => {
                let layer = Self { dir, manifest };
                if layer.is_intact()? {
                    return Ok(Some(layer));
                }
                tracing::warn!("removing the damaged WGC layer at {}", layer.dir.display());
                remove_dir_if_present(&layer.dir)?;
            }
            Err(err) => {
                tracing::warn!("removing the WGC layer at {} with an unreadable record: {err}", dir.display());
                remove_dir_if_present(&dir)?;
            }
        }

        Ok(None)
    }

    /// Whether every file and whiteout the layer was built with is in place and unchanged.
    fn is_intact(&self) -> Result<bool, WgcLayerError> {
        let delta = self.delta_dir();
        for file in &self.manifest.files {
            if !file.is_intact(&delta)? {
                return Ok(false);
            }
        }

        Ok(self.manifest.whiteouts.iter().all(|rel| wgc_launch::is_char_whiteout(&delta.join(rel))))
    }

    /// A layer at `dir` covering `build`, without files.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn for_test(dir: PathBuf, build: GameBuild) -> Self {
        let manifest = LayerManifest {
            locale_version: "test".to_owned(),
            parts: Vec::new(),
            builds: vec![build.get()],
            files: Vec::new(),
            whiteouts: Vec::new(),
        };
        Self { dir, manifest }
    }
}

/// What WGUS offers for a layer: the `locale` part of its newest build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerOffer {
    locale: PartDownload,
    parts: Vec<LayerPart>,
    /// The `bin/<build>` of the client part in the same chain.
    client_build: GameBuild,
}

impl LayerOffer {
    /// The bytes a fetch downloads.
    pub fn size(&self) -> u64 {
        self.locale.files.iter().map(|file| file.size).sum()
    }
}

/// Where a layer for a build stands before any download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayerProbe {
    Cached(WgcLayer),
    Fetchable(LayerOffer),
}

/// The layers kept under the app data: one directory per `locale` version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LayerStore {
    root: PathBuf,
}

impl LayerStore {
    pub(crate) fn new(dirs: &wgc_launch::RuntimeDirs) -> Self {
        Self { root: dirs.root().join("wgc-layers") }
    }

    /// A cached layer that covers `build`, else what WGUS offers when its newest build is
    /// `build`.
    pub(crate) async fn probe(&self, build: GameBuild, proxy_url: Option<&str>) -> Result<LayerProbe, WgcLayerError> {
        // Opening a layer hashes its files.
        let store = self.clone();
        let cached = tokio::task::spawn_blocking(move || store.cached(build)).await.map_err(WgcLayerError::Task)??;
        if let Some(layer) = cached {
            return Ok(LayerProbe::Cached(layer));
        }

        let offer = offer(proxy_url).await?;
        if offer.client_build != build {
            return Err(WgcLayerError::NoLayerForBuild { build, offered: offer.client_build });
        }

        Ok(LayerProbe::Fetchable(offer))
    }

    /// Downloads the offer's package, checks it against the catalog's hashes, and builds the
    /// layer from it. `progress` gets the bytes downloaded and the total. Waits while another
    /// toolkit process fetches the same version, then uses its layer.
    pub(crate) async fn fetch(
        &self,
        offer: &LayerOffer,
        proxy_url: Option<&str>,
        control: &wgc_download::DownloadControl,
        progress: impl Fn(u64, u64),
    ) -> Result<WgcLayer, WgcLayerError> {
        let version = offer.locale.version_to.as_str().to_owned();
        let store = self.clone();
        let locked = tokio::task::spawn_blocking(move || {
            let lock = store.lock(&version)?;
            let layer = WgcLayer::open(store.root.join(&version))?;
            Ok::<_, WgcLayerError>((lock, layer))
        });
        let (_lock, layer) = locked.await.map_err(WgcLayerError::Task)??;
        if let Some(layer) = layer {
            return Ok(layer);
        }

        // Only the holder of the version's lock touches its staging directory, so a download a
        // failed fetch left there is resumed.
        let staging = self.root.join(WORK_DIR).join(offer.locale.version_to.as_str());
        let client = crate::download::http_client(proxy_url).map_err(WgcLayerError::Client)?;
        let web_seed_base = offer.locale.web_seed_base.clone().ok_or(WgcLayerError::NoWebSeed)?;
        let torrent_url = offer.locale.torrent_urls.first().ok_or(WgcLayerError::NoTorrent)?;

        let mut torrent =
            wgc_download::fetch_verified_torrent(&client, torrent_url, offer.locale.info_hash.as_str()).await?;
        // A torrent may list other parts; the layer needs only the catalog's locale package.
        torrent.files.retain(|file| {
            let name = file.path.last().map(String::as_str).unwrap_or_default();
            name.ends_with(LOCALE_PACKAGE_SUFFIX) && offers_file(&offer.locale.files, name, file.length)
        });
        if torrent.files.is_empty() {
            return Err(WgcLayerError::NoLocalePackage);
        }
        torrent.total_length = torrent.files.iter().map(|file| file.length).sum();

        let request = wgc_download::DownloadRequest {
            staging_dir: staging.join("download"),
            torrent: torrent.clone(),
            web_seed_base,
            is_steam_install: false,
        };
        wgc_download::download(&client, &request, control, |update| {
            progress(update.downloaded_bytes, update.total_bytes)
        })
        .await?;
        if control.cancel.load(Ordering::SeqCst) {
            return Err(WgcLayerError::Download(wgc_download::DownloadError::Cancelled));
        }

        let store = self.clone();
        let offer = offer.clone();
        tokio::task::spawn_blocking(move || store.build(&offer, &torrent, &staging))
            .await
            .map_err(WgcLayerError::Task)?
    }

    /// Waits for and takes the lock on fetching `version`, which toolkit processes share. The OS
    /// releases it when the holder exits.
    fn lock(&self, version: &str) -> Result<std::fs::File, WgcLayerError> {
        let work = self.root.join(WORK_DIR);
        std::fs::create_dir_all(&work).map_err(WgcLayerError::io(&work))?;

        let path = work.join(format!("{version}.lock"));
        let file = std::fs::File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(WgcLayerError::io(&path))?;
        file.lock().map_err(WgcLayerError::io(&path))?;

        Ok(file)
    }

    fn cached(&self, build: GameBuild) -> Result<Option<WgcLayer>, WgcLayerError> {
        let entries = match std::fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(WgcLayerError::Io { path: self.root.clone(), source }),
        };

        let mut found: Option<WgcLayer> = None;
        for entry in entries {
            let entry = entry.map_err(WgcLayerError::io(&self.root))?;
            if entry.file_name() == WORK_DIR || !entry.file_type().map_err(WgcLayerError::io(&self.root))?.is_dir() {
                continue;
            }
            let Some(layer) = WgcLayer::open(entry.path())? else {
                continue;
            };
            if !layer.covers(build) {
                continue;
            }
            // Of two layers for the build, the newer locale version carries WG's fixes.
            let newer = found
                .as_ref()
                .is_none_or(|current| version_key(layer.locale_version()) > version_key(current.locale_version()));
            if newer {
                found = Some(layer);
            }
        }

        Ok(found)
    }

    /// Unpacks the staged package and moves the finished layer into place. Call under the
    /// version's lock.
    fn build(
        &self,
        offer: &LayerOffer,
        torrent: &wgc_download::TorrentMeta,
        staging: &Path,
    ) -> Result<WgcLayer, WgcLayerError> {
        let unpacked = staging.join("unpacked");
        remove_dir_if_present(&unpacked)?;
        for file in &torrent.files {
            let package = file.path.iter().fold(staging.join("download"), |path, part| path.join(part));
            wgc_download::unpack_dspkg(&package, &unpacked)?;
        }

        let layer = self.assemble(offer, &unpacked, &staging.join("layer"))?;
        remove_dir_if_present(staging)?;

        Ok(layer)
    }

    /// Builds the layer from the unpacked package at `unpacked` in `partial`, then moves it into
    /// place. A valid layer already in place is kept and returned instead.
    fn assemble(&self, offer: &LayerOffer, unpacked: &Path, partial: &Path) -> Result<WgcLayer, WgcLayerError> {
        remove_dir_if_present(partial)?;
        let composed = compose(unpacked, &partial.join(DELTA_DIR))?;
        if !composed.builds.contains(&offer.client_build) {
            return Err(WgcLayerError::PackageMissesBuild { offered: offer.client_build, composed: composed.builds });
        }

        let manifest = LayerManifest {
            locale_version: offer.locale.version_to.as_str().to_owned(),
            parts: offer.parts.clone(),
            builds: composed.builds.iter().map(|build| build.get()).collect(),
            files: composed.files,
            whiteouts: composed.whiteouts,
        };
        let manifest_path = partial.join(MANIFEST_FILE);
        let text = serde_json::to_string_pretty(&manifest).expect("a layer manifest serializes");
        std::fs::write(&manifest_path, text).map_err(WgcLayerError::io(&manifest_path))?;

        let dir = self.root.join(&manifest.locale_version);
        if let Some(layer) = WgcLayer::open(dir.clone())? {
            remove_dir_if_present(partial)?;
            return Ok(layer);
        }
        // Without a record, what is there is an incomplete layer that no launch selected.
        remove_dir_if_present(&dir)?;
        std::fs::rename(partial, &dir).map_err(WgcLayerError::io(&dir))?;

        Ok(WgcLayer { dir, manifest })
    }
}

/// What [`compose`] wrote into a delta.
#[derive(Debug)]
struct Composed {
    builds: Vec<GameBuild>,
    files: Vec<LayerFile>,
    whiteouts: Vec<PathBuf>,
}

/// A Steam install's game build: its newest `bin/<build>`.
pub fn newest_build(install: &Path) -> std::io::Result<Option<GameBuild>> {
    Ok(builds_in(&install.join("bin"))?.into_iter().max())
}

/// Asks WGUS for the newest chain of the worldwide build.
async fn offer(proxy_url: Option<&str>) -> Result<LayerOffer, WgcLayerError> {
    let known = wgc_catalog::known_games::known_games()
        .iter()
        .find(|known| known.game_id == GAME_ID)
        .expect("the worldwide WoWs build is a known game");
    let game_info = known.to_fresh_game_info().map_err(WgcLayerError::Catalog)?;
    let client = wgc_catalog::CatalogClient::with_proxy(proxy_url).map_err(WgcLayerError::Catalog)?;
    let latest = client.check_updates(&game_info).await.map_err(WgcLayerError::Catalog)?;

    let parts = latest
        .parts
        .iter()
        .map(|(name, status)| {
            let version = match status {
                PartStatus::UpToDate => None,
                PartStatus::UpdateAvailable { to, .. } | PartStatus::NotInstalled { to } => Some(to),
            };
            version.map(|version| LayerPart { name: name.to_string(), version: version.as_str().to_owned() })
        })
        .collect::<Option<Vec<_>>>()
        .ok_or(WgcLayerError::NoLocalePart)?;
    let locale = latest
        .downloads
        .iter()
        .find(|download| download.part == PartName::Locale)
        .cloned()
        .ok_or(WgcLayerError::NoLocalePart)?;
    let client_build = latest
        .downloads
        .iter()
        .find(|download| download.part == PartName::Client)
        .and_then(|download| build_of_version(download.version_to.as_str()))
        .ok_or(WgcLayerError::NoClientPart)?;

    Ok(LayerOffer { locale, parts, client_build })
}

/// Fills `delta` from the unpacked `locale` package at `package`.
/// Whether the update service offers a file named `name` of `length` bytes. The service names a
/// file under its part's directory; a torrent names it bare.
fn offers_file(offered: &[wgc_catalog::update::FileEntry], name: &str, length: u64) -> bool {
    offered.iter().any(|entry| {
        let entry_name = entry.name.rsplit('/').next().unwrap_or(&entry.name);
        entry_name == name && entry.size == length
    })
}

fn compose(package: &Path, delta: &Path) -> Result<Composed, WgcLayerError> {
    const ROOT_FILES: [&str; 2] = ["platform64.dll", "wgc_api.dll"];
    const BUILD_FILES: [&str; 4] =
        ["bin64/platform64.dll", "bin64/wgc_api.dll", "res/scripts_config.xml", "res/engine_config.xml"];

    let builds = builds_in(&package.join("bin")).map_err(WgcLayerError::io(package))?;
    if builds.is_empty() {
        return Err(WgcLayerError::MissingPackageFile { path: PathBuf::from("bin") });
    }

    let mut copies: Vec<PathBuf> = ROOT_FILES.iter().map(PathBuf::from).collect();
    let mut hidden = vec![PathBuf::from("steam_api64.dll"), PathBuf::from("currentrealm.txt")];
    for build in &builds {
        let dir = Path::new("bin").join(build.to_string());
        copies.extend(BUILD_FILES.iter().map(|rel| dir.join(rel)));
        hidden.push(dir.join("bin64/steam_api64.dll"));
    }

    let mut files = Vec::with_capacity(copies.len());
    for rel in &copies {
        let source = package.join(rel);
        if !source.is_file() {
            return Err(WgcLayerError::MissingPackageFile { path: rel.clone() });
        }
        let target = delta.join(rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(WgcLayerError::io(parent))?;
        }
        std::fs::copy(&source, &target).map_err(WgcLayerError::io(&target))?;
        files.push(LayerFile::read(delta, rel)?);
    }

    for rel in &hidden {
        let target = delta.join(rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(WgcLayerError::io(parent))?;
        }
        wgc_launch::create_whiteout(&target).map_err(WgcLayerError::io(&target))?;
    }

    Ok(Composed { builds, files, whiteouts: hidden })
}

fn sha256_hex(path: &Path) -> Result<String, WgcLayerError> {
    let mut file = std::fs::File::open(path).map_err(WgcLayerError::io(path))?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).map_err(WgcLayerError::io(path))?;

    Ok(hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
}

/// The numbered directories under `bin`.
fn builds_in(bin: &Path) -> std::io::Result<Vec<GameBuild>> {
    let entries = match std::fs::read_dir(bin) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    let mut builds = Vec::new();
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        if let Some(build) = entry.file_name().to_str().and_then(|name| name.parse().ok()) {
            builds.push(GameBuild::new(build));
        }
    }
    builds.sort();

    Ok(builds)
}

/// `15.10.0.0.13533378` names build `13533378`.
fn build_of_version(version: &str) -> Option<GameBuild> {
    version.rsplit('.').next()?.parse().ok().map(GameBuild::new)
}

fn version_key(version: &str) -> Vec<u64> {
    version.split('.').map(|part| part.parse().unwrap_or(0)).collect()
}

/// Install-relative paths of every `currentrealm.txt` in `install`. The Steam-mode client writes
/// it; it must not compete with the realm WGC reports.
fn realm_files(install: &Path) -> Result<Vec<PathBuf>, WgcLayerError> {
    const REALM_FILE: &str = "currentrealm.txt";
    // Game content, which holds no realm file and most of the install's entries.
    const SKIPPED: [&str; 1] = ["res_packages"];

    let mut found = Vec::new();
    let mut pending = vec![PathBuf::new()];
    while let Some(rel) = pending.pop() {
        let dir = install.join(&rel);
        let entries = std::fs::read_dir(&dir).map_err(WgcLayerError::io(&dir))?;
        for entry in entries {
            let entry = entry.map_err(WgcLayerError::io(&dir))?;
            let name = entry.file_name();
            let file_type = entry.file_type().map_err(WgcLayerError::io(&dir))?;
            if file_type.is_dir() {
                if !SKIPPED.iter().any(|skipped| name == *skipped) {
                    pending.push(rel.join(&name));
                }
                continue;
            }
            if name.to_str().is_some_and(|name| name.eq_ignore_ascii_case(REALM_FILE)) {
                found.push(rel.join(&name));
            }
        }
    }
    found.sort();

    Ok(found)
}

/// The content language for Steam's `language` (such as `german`), by the `full` and `short`
/// names in the build's `res/locale_config.xml`.
fn content_language(build_dir: &Path, steam_language: Option<&str>) -> Option<String> {
    let steam_language = steam_language?;
    let config = std::fs::read_to_string(build_dir.join("res/locale_config.xml")).ok()?;
    let full = format!("full=\"{steam_language}\"");

    let line = config.lines().find(|line| line.contains("<lang ") && line.contains(&full))?;
    let start = line.find("short=\"")? + "short=\"".len();
    let end = line[start..].find('"')? + start;

    Some(line[start..end].to_owned()).filter(|short| !short.is_empty())
}

/// A `game_info.xml` like the one WGC writes for an install of these parts.
fn game_info_xml(parts: &[LayerPart], language: &str) -> String {
    let versions: String = parts
        .iter()
        .map(|part| {
            format!(
                "            <version name=\"{name}\" installed=\"{version}\" available=\"{version}\"/>\n",
                name = part.name,
                version = part.version
            )
        })
        .collect();

    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<protocol name="game_info" version="2.16" wgc_publisher_id="wargaming">
    <game>
        <id>{GAME_ID}</id>
        <index>0</index>
        <installed>true</installed>
        <localization>EN</localization>
        <content_localizations>
            <content_localization realm="asia">{language}</content_localization>
            <content_localization realm="eu">{language}</content_localization>
            <content_localization realm="na">{language}</content_localization>
        </content_localizations>
        <client_type>high</client_type>
        <update_urls>
            <value>https://wgus-eu.wargaming.net/</value>
        </update_urls>
        <part_versions>
{versions}        </part_versions>
    </game>
</protocol>
"#
    )
}

fn remove_dir_if_present(dir: &Path) -> Result<(), WgcLayerError> {
    match std::fs::remove_dir_all(dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(WgcLayerError::Io { path: dir.to_path_buf(), source }),
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use std::os::unix::fs::FileTypeExt;

    use super::*;

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// WG's update service names the locale package under its part's directory, and the torrent
    /// names it bare, as on 15.10.0.0.13567617.
    #[test]
    fn a_torrent_file_matches_the_offer_under_its_part_directory() {
        let offered = [wgc_catalog::update::FileEntry {
            name: "wows_15.10.0.0.13567617_ww/wows.ww_15.10.0.0.13567617_locale.dspkg".to_owned(),
            size: 81_666_471,
            unpacked_size: 0,
        }];

        assert!(offers_file(&offered, "wows.ww_15.10.0.0.13567617_locale.dspkg", 81_666_471));
        assert!(!offers_file(&offered, "wows.ww_15.10.0.0.13567617_locale.dspkg", 1));
        assert!(!offers_file(&offered, "wows.ww_15.10.0.0.13567617_client.dspkg", 81_666_471));
    }

    /// The members of WG's `locale` package the layer reads, with stand-in contents.
    fn package(root: &Path) {
        for rel in [
            "platform64.dll",
            "wgc_api.dll",
            "WGCheck/WGCheck_config.xml",
            "_service/service.xml",
            "bin/13357625/bin64/platform64.dll",
            "bin/13357625/bin64/wgc_api.dll",
            "bin/13357625/res/scripts_config.xml",
            "bin/13357625/res/engine_config.xml",
            "bin/13357625/res/gui_urls.xml",
            "bin/13357625/res/texts/de/LC_MESSAGES/global.mo",
            "bin/13533378/bin64/platform64.dll",
            "bin/13533378/bin64/wgc_api.dll",
            "bin/13533378/res/scripts_config.xml",
            "bin/13533378/res/engine_config.xml",
            "bin/13533378/res/gui_urls.xml",
        ] {
            write(&root.join(rel), rel);
        }
        for build in ["13357625", "13533378"] {
            write(&root.join(format!("bin/{build}/res/locale_config.xml")), LOCALE_CONFIG);
        }
    }

    const LOCALE_CONFIG: &str = r#"<locale_config>
    <lang_mapping>
        <lang acceptLang="en" egs="en-US" fonts="EU" full="english" gog="english" short="en"/>
        <lang acceptLang="de,en" egs="de" fonts="EU" full="german" gog="german" short="de"/>
        <lang acceptLang="zh-Tw,en" egs="zh-Hant" fonts="CN" full="tchinese" short="zh_tw"/>
    </lang_mapping>
</locale_config>"#;

    fn steam_install(root: &Path) {
        for rel in
            ["WorldOfWarships.exe", "currentrealm.txt", "bin/13533378/bin64/CurrentRealm.txt", "res_packages/a.pkg"]
        {
            write(&root.join(rel), "steam");
        }
        write(&root.join("bin/13533378/res/locale_config.xml"), LOCALE_CONFIG);
    }

    /// Every entry under `dir`, as added files and whiteouts.
    fn entries(dir: &Path) -> Vec<String> {
        let mut out = Vec::new();
        let mut pending = vec![dir.to_path_buf()];
        while let Some(current) = pending.pop() {
            for entry in std::fs::read_dir(&current).unwrap() {
                let entry = entry.unwrap();
                let rel = entry.path().strip_prefix(dir).unwrap().display().to_string();
                let file_type = entry.file_type().unwrap();
                if file_type.is_dir() {
                    pending.push(entry.path());
                } else if file_type.is_char_device() {
                    out.push(format!("hidden {rel}"));
                } else {
                    out.push(format!("added {rel}"));
                }
            }
        }
        out.sort();
        out
    }

    fn parts() -> Vec<LayerPart> {
        [
            ("client", "15.10.0.0.13533378"),
            ("hotfix", "15.10.0.0.13533378"),
            ("locale", "15.10.0.0.13567617"),
            ("sdcontent", "15.10.0.0.13533378"),
        ]
        .into_iter()
        .map(|(name, version)| LayerPart { name: name.to_owned(), version: version.to_owned() })
        .collect()
    }

    fn layer(dir: &Path, builds: Vec<u32>) -> WgcLayer {
        WgcLayer {
            dir: dir.to_path_buf(),
            manifest: LayerManifest {
                locale_version: "15.10.0.0.13567617".to_owned(),
                parts: parts(),
                builds,
                files: Vec::new(),
                whiteouts: Vec::new(),
            },
        }
    }

    /// WGUS's offer of the `locale` part of version 15.10.0.0.13567617 for `client_build`.
    fn locale_offer(client_build: u32) -> LayerOffer {
        let locale = PartDownload {
            part: PartName::Locale,
            version_to: wgc_catalog::models::BuildVersion::new("15.10.0.0.13567617".to_owned()),
            info_hash: serde_json::from_str("\"00\"").unwrap(),
            torrent_urls: Vec::new(),
            web_seed_base: None,
            files: Vec::new(),
        };
        LayerOffer { locale, parts: parts(), client_build: GameBuild::new(client_build) }
    }

    /// A store under `root` holding the layer built from [`package`] for build 13533378. The
    /// fetch lock has made the store's directory.
    fn built_store(root: &Path) -> LayerStore {
        package(&root.join("package"));
        let store = LayerStore { root: root.join("layers") };
        std::fs::create_dir_all(&store.root).unwrap();
        store.assemble(&locale_offer(13533378), &root.join("package"), &root.join("partial")).unwrap();
        store
    }

    #[test]
    fn the_layer_adds_wgc_files_and_hides_the_steam_api() {
        let tmp = tempfile::tempdir().unwrap();
        package(&tmp.path().join("package"));

        let composed = compose(&tmp.path().join("package"), &tmp.path().join("delta")).unwrap();

        assert_eq!(composed.builds, vec![GameBuild::new(13357625), GameBuild::new(13533378)]);
        insta::assert_yaml_snapshot!(entries(&tmp.path().join("delta")));
    }

    #[test]
    fn a_built_layer_records_its_files_and_whiteouts() {
        let tmp = tempfile::tempdir().unwrap();
        let store = built_store(tmp.path());
        let dir = store.root.join("15.10.0.0.13567617");

        insta::assert_snapshot!(std::fs::read_to_string(dir.join(MANIFEST_FILE)).unwrap());
        assert!(WgcLayer::open(dir).unwrap().is_some());
    }

    #[test]
    fn a_package_without_the_offered_build_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        package(&tmp.path().join("package"));
        let store = LayerStore { root: tmp.path().join("layers") };

        let err = store
            .assemble(&locale_offer(13700000), &tmp.path().join("package"), &tmp.path().join("partial"))
            .unwrap_err();

        assert!(
            matches!(err, WgcLayerError::PackageMissesBuild { offered, .. } if offered == GameBuild::new(13700000))
        );
        assert!(!store.root.join("15.10.0.0.13567617").exists());
    }

    #[test]
    fn a_damaged_layer_is_removed() {
        type Damage = fn(&Path);
        let damages: [(&str, Damage); 4] = [
            ("changed file", |delta| {
                write(&delta.join("bin/13533378/bin64/platform64.dll"), "bin/13533378/bin64/platform64.DLL");
            }),
            ("missing file", |delta| std::fs::remove_file(delta.join("wgc_api.dll")).unwrap()),
            ("whiteout replaced", |delta| {
                std::fs::remove_file(delta.join("steam_api64.dll")).unwrap();
                write(&delta.join("steam_api64.dll"), "");
            }),
            ("unreadable record", |delta| write(&delta.parent().unwrap().join(MANIFEST_FILE), "{")),
        ];
        for (name, damage) in damages {
            let tmp = tempfile::tempdir().unwrap();
            let store = built_store(tmp.path());
            let dir = store.root.join("15.10.0.0.13567617");
            damage(&dir.join(DELTA_DIR));

            assert!(store.cached(GameBuild::new(13533378)).unwrap().is_none(), "{name}");
            assert!(!dir.exists(), "{name}");
        }
    }

    #[test]
    fn a_package_without_a_build_file_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        package(&tmp.path().join("package"));
        std::fs::remove_file(tmp.path().join("package/bin/13533378/bin64/wgc_api.dll")).unwrap();

        let err = compose(&tmp.path().join("package"), &tmp.path().join("delta")).unwrap_err();

        assert!(
            matches!(err, WgcLayerError::MissingPackageFile { path } if path == Path::new("bin/13533378/bin64/wgc_api.dll"))
        );
    }

    #[test]
    fn preparing_for_an_install_writes_game_info_and_hides_its_realm_files() {
        let tmp = tempfile::tempdir().unwrap();
        package(&tmp.path().join("package"));
        let dir = tmp.path().join("layer");
        compose(&tmp.path().join("package"), &dir.join(DELTA_DIR)).unwrap();
        let install = tmp.path().join("install");
        steam_install(&install);

        layer(&dir, vec![13357625, 13533378]).prepare_for(&install, Some("german")).unwrap();

        insta::assert_yaml_snapshot!(entries(&dir.join(DELTA_DIR)));
        insta::assert_snapshot!(std::fs::read_to_string(dir.join("delta/game_info.xml")).unwrap());
    }

    #[test]
    fn an_unknown_steam_language_reports_english() {
        let tmp = tempfile::tempdir().unwrap();
        package(&tmp.path().join("package"));
        let dir = tmp.path().join("layer");
        compose(&tmp.path().join("package"), &dir.join(DELTA_DIR)).unwrap();
        let install = tmp.path().join("install");
        steam_install(&install);

        layer(&dir, vec![13357625, 13533378]).prepare_for(&install, Some("klingon")).unwrap();

        let game_info = std::fs::read_to_string(dir.join("delta/game_info.xml")).unwrap();
        assert!(game_info.contains(r#"<content_localization realm="eu">en</content_localization>"#));
    }

    #[test]
    fn a_cached_layer_is_found_for_its_builds_only() {
        let tmp = tempfile::tempdir().unwrap();
        let store = LayerStore { root: tmp.path().to_path_buf() };
        let manifest = |dir: &str, locale: &str, builds: &[u32]| {
            let manifest = LayerManifest {
                locale_version: locale.to_owned(),
                parts: Vec::new(),
                builds: builds.to_vec(),
                files: Vec::new(),
                whiteouts: Vec::new(),
            };
            write(&tmp.path().join(dir).join(MANIFEST_FILE), &serde_json::to_string(&manifest).unwrap());
        };
        manifest("15.9.0.0.13300000", "15.9.0.0.13300000", &[13200000, 13357625]);
        manifest("15.10.0.0.13567617", "15.10.0.0.13567617", &[13357625, 13533378]);
        std::fs::create_dir_all(tmp.path().join("work/15.11.0.0.13700000/layer/delta")).unwrap();

        let found = |build| store.cached(GameBuild::new(build)).unwrap().map(|layer| layer.locale_version().to_owned());

        assert_eq!(found(13357625).as_deref(), Some("15.10.0.0.13567617"));
        assert_eq!(found(13200000).as_deref(), Some("15.9.0.0.13300000"));
        assert_eq!(found(13700000), None);
    }
}
