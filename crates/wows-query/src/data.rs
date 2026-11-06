use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::num::NonZeroU32;
use std::path::Path;
use std::path::PathBuf;

use gettext::Catalog;
use rootcause::prelude::*;
use wows_core::Version;
use wows_data_mgr::Dump;
use wows_data_mgr::builds::BuildMetadata;
use wows_data_mgr::builds::BuildsIndex;
use wowsunpack::game_params::cache;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::CrewSkill;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::vfs::VfsPath;

use crate::error::LoadError;
use crate::gameparams::RawParamsSlot;
use crate::raw_params::COMPLETE_MARKER;
use crate::raw_params::LAST_USED_MARKER;
use crate::raw_params::touch;
use crate::ships::Ship;
use crate::upgrades::Upgrade;

/// Cache kind directory of rebuilt `game_params.rkyv` files under a cache
/// root, laid out `<kind>/<build>/` like [`crate::raw_params::RAW_PARAMS_KIND`].
/// The suffix is [`cache::FORMAT_VERSION`], so programs built with different
/// formats sharing a cache root do not replace each other's files.
pub fn game_params_cache_kind() -> String {
    format!("game-params-v{}", cache::FORMAT_VERSION)
}

const GAME_PARAMS_CACHE: &str = "game_params.rkyv";
const ENGLISH_CATALOG: &str = "translations/en/LC_MESSAGES/global.mo";
pub(crate) const GAME_PARAMS_DATA: &str = "content/GameParams.data";

/// GameParams and English translations for one game build, indexed for the
/// queries in this crate. Load once and share; every query borrows it.
pub struct GameData {
    version: Version,
    provider: GameMetadataProvider,

    /// The dump's files, for data read on demand such as ship armor.
    pub(crate) vfs: VfsPath,

    pub(crate) ships: Vec<Ship>,

    /// The captain skill table. Every crew in a build shares it.
    pub(crate) skills: Vec<CrewSkill>,

    /// Upgrades that occupy a slot.
    pub(crate) upgrades: Vec<Upgrade>,

    /// Ship param name to the param names of the ships that research it.
    pub(crate) research_predecessors: HashMap<String, Vec<String>>,

    /// Raw GameParams, opened or built on first use.
    pub(crate) raw_params: RawParamsSlot,
}

impl GameData {
    /// Loads one build directory of a wows-replay-data dump (the directory
    /// holding `metadata.toml`). Parsing `GameParams.data` takes several
    /// seconds when the dump's `game_params.rkyv` was written by another
    /// wowsunpack. `cache_root`, when given, holds per-build artifacts in
    /// `<cache_root>/<kind>/<build>/` for [`game_params_cache_kind`] and
    /// [`crate::raw_params::RAW_PARAMS_KIND`], each marked with `.complete`
    /// and `.last-used`.
    pub fn load_dump(dump_dir: &Path, cache_root: Option<&Path>) -> Result<Self, Report<LoadError>> {
        let Some(metadata) = BuildMetadata::load(&dump_dir.join("metadata.toml")) else {
            return Err(report!(LoadError::NotADump { dir: dump_dir.to_path_buf() }));
        };
        let version = parse_version(&metadata.version, metadata.build).ok_or_else(|| {
            report!(LoadError::Version { dir: dump_dir.to_path_buf(), version: metadata.version.clone() })
        })?;

        let dump = Dump::open(dump_dir);
        // Keyed by build: the cache header carries no build, so a shared
        // directory would serve one build's params for another.
        let params_cache = cache_root.map(|root| root.join(game_params_cache_kind()).join(metadata.build.to_string()));

        let provider = load_provider(&dump, dump_dir, params_cache.as_deref())?;
        let Some(catalog_path) = dump.derived_path(ENGLISH_CATALOG) else {
            return Err(report!(LoadError::MissingFile { dir: dump_dir.to_path_buf(), file: ENGLISH_CATALOG }));
        };
        let file = File::open(&catalog_path)
            .map_err(|source| report!(LoadError::Io { path: catalog_path.clone(), source }))?;
        let catalog = Catalog::parse(file)
            .map_err(|source| report!(LoadError::Translations { path: catalog_path.clone(), source }))?;
        provider.set_translations(catalog);

        let raw_params = RawParamsSlot::new(dump_dir, cache_root, metadata.build);

        Ok(Self::index(version, provider, dump.vfs(), raw_params))
    }

    /// Loads the newest build listed in a wows-replay-data dump root (the
    /// directory holding `builds.toml`).
    pub fn load_latest(root: &Path, cache_root: Option<&Path>) -> Result<Self, Report<LoadError>> {
        let dir = latest_dump_dir(root)?;

        Self::load_dump(&dir, cache_root)
    }

    /// The game version this data belongs to.
    pub fn version(&self) -> Version {
        self.version
    }

    /// The underlying provider, for callers that need wowsunpack directly.
    pub fn provider(&self) -> &GameMetadataProvider {
        &self.provider
    }

    pub fn ship_count(&self) -> usize {
        self.ships.len()
    }

    fn index(version: Version, provider: GameMetadataProvider, vfs: VfsPath, raw_params: RawParamsSlot) -> Self {
        let ships: Vec<Ship> =
            provider.params().iter().filter_map(|param| Ship::from_param(&provider, param)).collect();

        let skills = provider
            .params()
            .iter()
            .find_map(|param| param.crew().and_then(|crew| crew.skills()).map(<[CrewSkill]>::to_vec))
            .unwrap_or_default();

        let upgrades = provider.params().iter().filter_map(|param| Upgrade::from_param(&provider, param)).collect();

        let mut research_predecessors: HashMap<String, Vec<String>> = HashMap::new();
        for ship in &ships {
            let Some(vehicle) = ship.param.vehicle() else {
                continue;
            };
            for next in vehicle.research().next_ships() {
                research_predecessors.entry(next.clone()).or_default().push(ship.param.name().to_owned());
            }
        }

        Self { version, provider, vfs, ships, skills, upgrades, research_predecessors, raw_params }
    }
}

/// The directory of the newest build listed in a dump root's `builds.toml`.
pub fn latest_dump_dir(root: &Path) -> Result<PathBuf, Report<LoadError>> {
    let index = BuildsIndex::load(&root.join("builds.toml"));
    let Some(entry) = index.builds.iter().max_by_key(|entry| entry.build) else {
        return Err(report!(LoadError::NoBuilds { root: root.to_path_buf() }));
    };

    Ok(root.join(&entry.dir))
}

/// `params_cache` is this build's [`game_params_cache_kind`] directory.
fn load_provider(
    dump: &Dump,
    dump_dir: &Path,
    params_cache: Option<&Path>,
) -> Result<GameMetadataProvider, Report<LoadError>> {
    let params_error = |source| report!(LoadError::GameParams { dir: dump_dir.to_path_buf(), source });
    let cache_path = params_cache.map(|dir| dir.join(GAME_PARAMS_CACHE));

    let params = match load_cached_params(cache_path.as_deref(), dump.derived_path(GAME_PARAMS_CACHE).as_deref()) {
        Some(params) => params,
        None => {
            tracing::info!(dir = %dump_dir.display(), "game_params.rkyv unusable; parsing GameParams.data");
            let bytes = read_vfs_file(dump, dump_dir, GAME_PARAMS_DATA)?;
            let params = GameMetadataProvider::params_from_data(bytes).map_err(params_error)?;
            if let Some(path) = &cache_path {
                save_params_cache(path, &params);
            }
            params
        }
    };

    GameMetadataProvider::from_params_no_specs(params).map_err(params_error)
}

/// The cache directory's copy at `cache_path` when it loads, else the dump's
/// own copy at `dump_path`.
fn load_cached_params(
    cache_path: Option<&Path>,
    dump_path: Option<&Path>,
) -> Option<Vec<wowsunpack::game_params::types::Param>> {
    if let Some(path) = cache_path
        && path.with_file_name(COMPLETE_MARKER).is_file()
        && let Some(params) = cache::load(path)
    {
        touch(&path.with_file_name(LAST_USED_MARKER));
        return Some(params);
    }

    dump_path.and_then(cache::load)
}

/// Writes `game_params.rkyv` and its markers through a temporary file in
/// the same directory, so readers never see a partial file. A failure only
/// costs the next load the same parse.
fn save_params_cache(path: &Path, params: &[wowsunpack::game_params::types::Param]) {
    let complete = path.with_file_name(COMPLETE_MARKER);
    // The file is about to be replaced; it is not complete until saved.
    let _ = std::fs::remove_file(&complete);

    let temp = path.with_file_name(format!("{GAME_PARAMS_CACHE}.tmp-{}", std::process::id()));
    let written = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| cache::encode(params))
        .and_then(|bytes| std::fs::write(&temp, bytes))
        .and_then(|()| std::fs::rename(&temp, path));
    if let Err(error) = written {
        let _ = std::fs::remove_file(&temp);
        tracing::warn!(path = %path.display(), %error, "failed to write game_params.rkyv");
        return;
    }

    touch(&path.with_file_name(LAST_USED_MARKER));
    if let Err(error) = File::create(&complete) {
        tracing::warn!(path = %complete.display(), %error, "failed to mark game_params.rkyv complete");
    }
}

fn read_vfs_file(dump: &Dump, dump_dir: &Path, path: &'static str) -> Result<Vec<u8>, Report<LoadError>> {
    let missing = || report!(LoadError::MissingFile { dir: dump_dir.to_path_buf(), file: path });

    let mut file = dump.vfs().join(path).and_then(|entry| entry.open_file()).map_err(|_| missing())?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|source| report!(LoadError::Io { path: dump_dir.join(path), source }))?;

    Ok(bytes)
}

fn parse_version(version: &str, build: u32) -> Option<Version> {
    let mut parts = version.split('.').map(|part| part.parse::<u32>());
    let (Some(Ok(major)), Some(Ok(minor)), Some(Ok(patch)), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return None;
    };

    Some(Version { major, minor, patch, build: NonZeroU32::new(build) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_version_reads_three_components() {
        let version = parse_version("15.10.0", 13533378).expect("valid version");
        assert_eq!((version.major, version.minor, version.patch, version.build_number()), (15, 10, 0, Some(13533378)));

        assert!(parse_version("15.10", 1).is_none());
        assert!(parse_version("15.10.0.1", 1).is_none());
    }

    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wows-query-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create scratch directory");
        dir
    }

    #[test]
    fn cache_kind_names_the_format_version() {
        assert_eq!(game_params_cache_kind(), format!("game-params-v{}", cache::FORMAT_VERSION));
    }

    #[test]
    fn a_rejected_cache_copy_falls_back_to_the_dump_copy() {
        let dir = scratch_dir("params-fallback");
        let cache_path = dir.join("cache").join(GAME_PARAMS_CACHE);
        let dump_path = dir.join(GAME_PARAMS_CACHE);
        std::fs::create_dir_all(cache_path.parent().unwrap()).unwrap();
        std::fs::write(&cache_path, b"WUGP\0\0\0\0stale").unwrap();
        File::create(cache_path.with_file_name(COMPLETE_MARKER)).unwrap();
        std::fs::write(&dump_path, cache::encode(&[]).unwrap()).unwrap();

        assert!(load_cached_params(Some(&cache_path), Some(&dump_path)).is_some(), "the dump copy loads");
        assert!(load_cached_params(Some(&cache_path), None).is_none(), "the stale copy is refused");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_saved_cache_copy_is_complete_and_loads() {
        let dir = scratch_dir("params-save");
        let cache_path = dir.join("build").join(GAME_PARAMS_CACHE);
        save_params_cache(&cache_path, &[]);

        assert!(cache_path.with_file_name(COMPLETE_MARKER).is_file());
        assert!(cache_path.with_file_name(LAST_USED_MARKER).is_file());
        let leftovers: Vec<_> = std::fs::read_dir(cache_path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .filter(|name| name.to_string_lossy().contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty(), "no temporary file remains: {leftovers:?}");
        assert!(load_cached_params(Some(&cache_path), None).is_some());

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
