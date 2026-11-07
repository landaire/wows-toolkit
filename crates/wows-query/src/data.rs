use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::num::NonZeroU32;
use std::path::Path;
use std::path::PathBuf;
use std::sync::OnceLock;

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

use crate::armor_assessment::ComparisonCache;
use crate::commanders::Commander;
use crate::consumables::consumable_names;
use crate::error::LoadError;
use crate::gameparams::RawParamsSlot;
use crate::names::Alias;
use crate::names::AliasError;
use crate::names::AliasTarget;
use crate::names::NameIndex;
use crate::raw_params::COMPLETE_MARKER;
use crate::raw_params::LAST_USED_MARKER;
use crate::raw_params::touch;
use crate::shells::RicochetStandards;
use crate::ships::Ship;
use crate::ships::ship_names;
use crate::skills::skill_names;
use crate::upgrades::Upgrade;
use crate::upgrades::upgrade_names;

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

    /// The standard captain skill table, which most crews share. Unique
    /// commanders carry their own table with improved entries.
    pub(crate) skills: Vec<CrewSkill>,

    /// Unique commanders with improved skills or talents.
    pub(crate) commanders: Vec<Commander>,

    /// Upgrades that occupy a slot.
    pub(crate) upgrades: Vec<Upgrade>,

    /// Ship param name to the param names of the ships that research it.
    pub(crate) research_predecessors: HashMap<String, Vec<String>>,

    /// Names that resolve to indexes into `ships`, `skills` and `upgrades`,
    /// and to consumable types.
    pub(crate) ship_names: NameIndex<usize>,
    pub(crate) skill_names: NameIndex<usize>,
    pub(crate) upgrade_names: NameIndex<usize>,
    pub(crate) consumable_names: NameIndex<String>,
    pub(crate) commander_names: NameIndex<usize>,

    /// Raw GameParams, opened or built on first use.
    pub(crate) raw_params: RawParamsSlot,

    /// Peer tables of armor assessments, traced on first use.
    pub(crate) armor_comparisons: ComparisonCache,

    /// Ricochet angles most shells of each ammo type share, counted on first use.
    pub(crate) ricochet_standards: OnceLock<RicochetStandards>,
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
        let (version, build) = dump_version(dump_dir)?;
        let dump = Dump::open(dump_dir);
        let params = load_params(&dump, dump_dir, params_cache(cache_root, build).as_deref())?;
        let provider = GameMetadataProvider::from_params_no_specs(params)
            .map_err(|source| report!(LoadError::GameParams { dir: dump_dir.to_path_buf(), source }))?;
        load_english(&provider, &dump, dump_dir)?;

        let raw_params = RawParamsSlot::new(dump_dir, cache_root, build);
        let armor_comparisons = ComparisonCache::new(cache_root, build);

        Ok(Self::index(version, provider, dump.vfs(), raw_params, armor_comparisons))
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

    /// Teaches the resolvers extra names, such as former skill names. Each
    /// alias names its target by GameParams name, index, skill internal name,
    /// consumable type or English name. Aliases whose target is not in this
    /// game data are returned in the error; the others are registered.
    pub fn add_aliases(&mut self, aliases: impl IntoIterator<Item = Alias>) -> Result<(), AliasError> {
        let mut unknown = Vec::new();
        for alias in aliases {
            let added = match &alias.target {
                AliasTarget::Ship(target) => self.ship_names.add_alias(&alias.name, target, alias.method),
                AliasTarget::Skill(target) => self.skill_names.add_alias(&alias.name, target, alias.method),
                AliasTarget::Upgrade(target) => self.upgrade_names.add_alias(&alias.name, target, alias.method),
                AliasTarget::Consumable(target) => self.consumable_names.add_alias(&alias.name, target, alias.method),
            };
            if !added {
                unknown.push(alias);
            }
        }

        if unknown.is_empty() { Ok(()) } else { Err(AliasError { unknown }) }
    }

    fn index(
        version: Version,
        provider: GameMetadataProvider,
        vfs: VfsPath,
        raw_params: RawParamsSlot,
        armor_comparisons: ComparisonCache,
    ) -> Self {
        let mut ships: Vec<Ship> =
            provider.params().iter().filter_map(|param| Ship::from_param(&provider, param)).collect();
        Ship::mark_test_copies(&mut ships);
        Ship::mark_replicas(&mut ships);

        let skills = standard_skills(&provider);

        let upgrades: Vec<Upgrade> =
            provider.params().iter().filter_map(|param| Upgrade::from_param(&provider, param)).collect();

        let mut research_predecessors: HashMap<String, Vec<String>> = HashMap::new();
        for ship in &ships {
            let Some(vehicle) = ship.param.vehicle() else {
                continue;
            };
            for next in vehicle.research().next_ships() {
                research_predecessors.entry(next.clone()).or_default().push(ship.param.name().to_owned());
            }
        }

        let ship_names = ship_names(&ships);
        let skill_names = skill_names(&provider, &version, &skills);
        let upgrade_names = upgrade_names(&upgrades);
        let consumable_names = consumable_names(&provider);
        let (commanders, commander_names) = Commander::index(&provider, &skills);

        Self {
            version,
            provider,
            vfs,
            ships,
            skills,
            commanders,
            upgrades,
            research_predecessors,
            ship_names,
            skill_names,
            upgrade_names,
            consumable_names,
            commander_names,
            raw_params,
            armor_comparisons,
            ricochet_standards: OnceLock::new(),
        }
    }
}

/// The skill table most crews share. Crews that share a table share one
/// allocation (`share_crew_skills`), so tables are counted by address.
fn standard_skills(provider: &GameMetadataProvider) -> Vec<CrewSkill> {
    let mut tables: Vec<(&[CrewSkill], usize)> = Vec::new();
    for skills in provider.params().iter().filter_map(|param| param.crew().and_then(|crew| crew.skills())) {
        match tables.iter_mut().find(|(table, _)| std::ptr::eq(*table, skills)) {
            Some((_, count)) => *count += 1,
            None => tables.push((skills, 1)),
        }
    }

    tables.into_iter().rev().max_by_key(|(_, count)| *count).map(|(table, _)| table.to_vec()).unwrap_or_default()
}

/// The directory of the newest build listed in a dump root's `builds.toml`.
pub fn latest_dump_dir(root: &Path) -> Result<PathBuf, Report<LoadError>> {
    let index = BuildsIndex::load(&root.join("builds.toml"));
    let Some(entry) = index.builds.iter().max_by_key(|entry| entry.build) else {
        return Err(report!(LoadError::NoBuilds { root: root.to_path_buf() }));
    };

    Ok(root.join(&entry.dir))
}

/// One dump build's GameParams, entity definitions and English names: what
/// parsing a replay's packets needs.
pub(crate) struct ReplayGameData {
    pub version: Version,
    pub provider: GameMetadataProvider,
    pub vfs: VfsPath,
}

impl ReplayGameData {
    /// Loads `dump_dir` like [`GameData::load_dump`], with the entity
    /// definitions the packet parser needs.
    pub(crate) fn load(dump_dir: &Path, cache_root: Option<&Path>) -> Result<Self, Report<LoadError>> {
        let (version, build) = dump_version(dump_dir)?;
        let dump = Dump::open(dump_dir);
        let params = load_params(&dump, dump_dir, params_cache(cache_root, build).as_deref())?;
        let vfs = dump.vfs();
        let provider = GameMetadataProvider::from_params_with_vfs(params, &vfs)
            .map_err(|source| report!(LoadError::GameParams { dir: dump_dir.to_path_buf(), source }))?;
        load_english(&provider, &dump, dump_dir)?;

        Ok(Self { version, provider, vfs })
    }
}

/// A game build number, such as `13357625`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct BuildNumber(u32);

impl BuildNumber {
    pub(crate) fn new(build: u32) -> Self {
        Self(build)
    }

    pub(crate) fn raw(self) -> u32 {
        self.0
    }
}

impl std::fmt::Display for BuildNumber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// The version and build in a dump directory's `metadata.toml`.
fn dump_version(dump_dir: &Path) -> Result<(Version, BuildNumber), Report<LoadError>> {
    let Some(metadata) = BuildMetadata::load(&dump_dir.join("metadata.toml")) else {
        return Err(report!(LoadError::NotADump { dir: dump_dir.to_path_buf() }));
    };
    let version = parse_version(&metadata.version, metadata.build).ok_or_else(|| {
        report!(LoadError::Version { dir: dump_dir.to_path_buf(), version: metadata.version.clone() })
    })?;

    Ok((version, BuildNumber::new(metadata.build)))
}

/// This build's [`game_params_cache_kind`] directory under `cache_root`.
/// Keyed by build: the cache header carries no build, so a shared directory
/// would serve one build's params for another.
fn params_cache(cache_root: Option<&Path>, build: BuildNumber) -> Option<PathBuf> {
    cache_root.map(|root| root.join(game_params_cache_kind()).join(build.to_string()))
}

/// `params_cache` is this build's [`game_params_cache_kind`] directory.
fn load_params(
    dump: &Dump,
    dump_dir: &Path,
    params_cache: Option<&Path>,
) -> Result<Vec<wowsunpack::game_params::types::Param>, Report<LoadError>> {
    let cache_path = params_cache.map(|dir| dir.join(GAME_PARAMS_CACHE));
    if let Some(params) = load_cached_params(cache_path.as_deref(), dump.derived_path(GAME_PARAMS_CACHE).as_deref()) {
        return Ok(params);
    }

    tracing::info!(dir = %dump_dir.display(), "game_params.rkyv unusable; parsing GameParams.data");
    let bytes = read_vfs_file(dump, dump_dir, GAME_PARAMS_DATA)?;
    let params = GameMetadataProvider::params_from_data(bytes)
        .map_err(|source| report!(LoadError::GameParams { dir: dump_dir.to_path_buf(), source }))?;
    if let Some(path) = &cache_path {
        save_params_cache(path, &params);
    }

    Ok(params)
}

/// Installs the dump's English translation catalog in `provider`.
fn load_english(provider: &GameMetadataProvider, dump: &Dump, dump_dir: &Path) -> Result<(), Report<LoadError>> {
    let Some(catalog_path) = dump.derived_path(ENGLISH_CATALOG) else {
        return Err(report!(LoadError::MissingFile { dir: dump_dir.to_path_buf(), file: ENGLISH_CATALOG }));
    };
    let file =
        File::open(&catalog_path).map_err(|source| report!(LoadError::Io { path: catalog_path.clone(), source }))?;
    let catalog = Catalog::parse(file)
        .map_err(|source| report!(LoadError::Translations { path: catalog_path.clone(), source }))?;
    provider.set_translations(catalog);

    Ok(())
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
    use serde::Serialize;

    use super::*;
    use crate::consumables::CONSUMABLE_ALIASES;
    use crate::names::Lookup;
    use crate::names::ResolutionMethod;
    use crate::ships::SHIP_ALIASES;
    use crate::skills::SKILL_ALIASES;
    use crate::upgrades::UPGRADE_ALIASES;

    #[derive(Serialize)]
    struct NameResolution {
        name: &'static str,
        resolved: String,
        method: Option<ResolutionMethod>,
    }

    /// How `name` resolves in `names`, after checking that it reaches a
    /// target `is_target` accepts by `method`, `None` for an exact name or
    /// identifier.
    fn resolve_name<T>(
        names: &NameIndex<T>,
        name: &'static str,
        method: Option<ResolutionMethod>,
        is_target: impl Fn(&T) -> bool,
    ) -> NameResolution
    where
        T: Clone + PartialEq,
    {
        let Lookup::Found(resolved, targets) = names.lookup(name) else {
            panic!("{name} does not resolve to one name");
        };
        assert!(targets.iter().any(is_target), "{name} resolves to {}, not its target", resolved.name);
        assert_eq!(resolved.method, method, "{name} resolves to {} by {:?}", resolved.name, resolved.method);

        NameResolution { name, resolved: resolved.name.to_owned(), method }
    }

    #[test]
    #[cfg_attr(not(has_build_13533378), ignore)]
    fn alias_tables_resolve_to_their_targets() {
        let root = PathBuf::from(std::env::var_os("WOWS_GAME_DATA").expect("WOWS_GAME_DATA names the dump root"));
        let index = BuildsIndex::load(&root.join("builds.toml"));
        let entry = index.find_by_build(13533378).expect("dump root lists build 13533378");
        let cache = std::env::temp_dir().join("wows-query-test-cache");
        let game = GameData::load_dump(&root.join(&entry.dir), Some(&cache)).expect("dump loads");

        let alias = Some(ResolutionMethod::ExactAlias);
        let mut resolutions = Vec::new();
        for (name, ship) in SHIP_ALIASES {
            resolutions.push(resolve_name(&game.ship_names, name, alias, |index| game.ships[*index].name == *ship));
        }
        for (name, internal_name, method) in SKILL_ALIASES {
            resolutions.push(resolve_name(&game.skill_names, name, Some(*method), |index| {
                game.skills[*index].internal_name().as_str() == *internal_name
            }));
        }
        for (name, param) in UPGRADE_ALIASES {
            resolutions.push(resolve_name(&game.upgrade_names, name, alias, |index| {
                game.upgrades[*index].param.name() == *param
            }));
        }
        for (name, kind) in CONSUMABLE_ALIASES {
            resolutions.push(resolve_name(&game.consumable_names, name, alias, |target| target == kind));
        }

        // Names the tables leave out because they resolve without an entry.
        let initialism = Some(ResolutionMethod::Initialism);
        resolutions.push(resolve_name(&game.upgrade_names, "MBM3", initialism, |index| {
            game.upgrades[*index].name == "Main Battery Modification 3"
        }));
        resolutions.push(resolve_name(&game.consumable_names, "DCP", initialism, |target| target == "crashCrew"));
        resolutions.push(resolve_name(&game.consumable_names, "Fighter", None, |target| target == "fighter"));

        insta::assert_yaml_snapshot!(resolutions);
    }

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
