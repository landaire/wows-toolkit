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

use crate::error::LoadError;
use crate::ships::Ship;
use crate::upgrades::Upgrade;

const GAME_PARAMS_CACHE: &str = "game_params.rkyv";
const ENGLISH_CATALOG: &str = "translations/en/LC_MESSAGES/global.mo";

/// GameParams and English translations for one game build, indexed for the
/// queries in this crate. Load once and share; every query borrows it.
pub struct GameData {
    version: Version,
    provider: GameMetadataProvider,
    pub(crate) ships: Vec<Ship>,

    /// The captain skill table. Every crew in a build shares it.
    pub(crate) skills: Vec<CrewSkill>,

    /// Upgrades that occupy a slot.
    pub(crate) upgrades: Vec<Upgrade>,

    /// Ship param name to the param names of the ships that research it.
    pub(crate) research_predecessors: HashMap<String, Vec<String>>,
}

impl GameData {
    /// Loads one build directory of a wows-replay-data dump (the directory
    /// holding `metadata.toml`). Parsing `GameParams.data` takes several
    /// seconds when the dump's `game_params.rkyv` was written by another
    /// wowsunpack; `cache_dir`, when given, receives a rebuilt cache that later
    /// loads read first.
    pub fn load_dump(dump_dir: &Path, cache_dir: Option<&Path>) -> Result<Self, Report<LoadError>> {
        let Some(metadata) = BuildMetadata::load(&dump_dir.join("metadata.toml")) else {
            return Err(report!(LoadError::NotADump { dir: dump_dir.to_path_buf() }));
        };
        let version = parse_version(&metadata.version, metadata.build).ok_or_else(|| {
            report!(LoadError::Version { dir: dump_dir.to_path_buf(), version: metadata.version.clone() })
        })?;

        let mut dump = Dump::open(dump_dir);
        // Keyed by build: the cache header carries no build, so a shared
        // directory would serve one build's params for another.
        if let Some(cache_dir) = cache_dir {
            dump.set_override_root(cache_dir.join(metadata.build.to_string()));
        }

        let provider = load_provider(&dump, dump_dir)?;
        let Some(catalog_path) = dump.derived_path(ENGLISH_CATALOG) else {
            return Err(report!(LoadError::MissingFile { dir: dump_dir.to_path_buf(), file: ENGLISH_CATALOG }));
        };
        let file = File::open(&catalog_path)
            .map_err(|source| report!(LoadError::Io { path: catalog_path.clone(), source }))?;
        let catalog = Catalog::parse(file)
            .map_err(|source| report!(LoadError::Translations { path: catalog_path.clone(), source }))?;
        provider.set_translations(catalog);

        Ok(Self::index(version, provider))
    }

    /// Loads the newest build listed in a wows-replay-data dump root (the
    /// directory holding `builds.toml`).
    pub fn load_latest(root: &Path, cache_dir: Option<&Path>) -> Result<Self, Report<LoadError>> {
        let dir = latest_dump_dir(root)?;

        Self::load_dump(&dir, cache_dir)
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

    fn index(version: Version, provider: GameMetadataProvider) -> Self {
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

        Self { version, provider, ships, skills, upgrades, research_predecessors }
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

fn load_provider(dump: &Dump, dump_dir: &Path) -> Result<GameMetadataProvider, Report<LoadError>> {
    let params_error = |source| report!(LoadError::GameParams { dir: dump_dir.to_path_buf(), source });

    let cached = dump.derived_path(GAME_PARAMS_CACHE).and_then(|path| cache::load(&path));
    let params = match cached {
        Some(params) => params,
        None => {
            tracing::info!(dir = %dump_dir.display(), "game_params.rkyv unusable; parsing GameParams.data");
            let bytes = read_vfs_file(dump, dump_dir, "content/GameParams.data")?;
            let params = GameMetadataProvider::params_from_data(bytes).map_err(params_error)?;

            // A cache write failure only costs the next load the same parse.
            if let Some(path) = dump.derived_write_path(GAME_PARAMS_CACHE)
                && let Err(error) = cache::save(&path, &params)
            {
                tracing::warn!(path = %path.display(), %error, "failed to write game_params.rkyv");
            }
            params
        }
    };

    GameMetadataProvider::from_params_no_specs(params).map_err(params_error)
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
}
