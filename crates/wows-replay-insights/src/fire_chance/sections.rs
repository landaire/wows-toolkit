//! Where fire-section geometry comes from, and the effective-fire-chance
//! result built on it.
//!
//! Resolution is the expensive half of the statistic: it means parsing a
//! ~178 MB `assets.bin` out of the game install. Both front ends want the
//! same answer for the same build, so the source, the on-disk cache, the
//! record of what did not resolve, and the analysis call that ties them
//! together all live here rather than in either app.

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::sync::LazyLock;
use std::sync::RwLock;

use wows_battle_world::report::BattleReport;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::models::fire_nodes::FireSectionGeometry;
use wowsunpack::vfs::VfsPath;

use crate::fire_chance::analysis::EffectiveFireChance;
use crate::fire_chance::analysis::VictimContext;

/// Why fire-section geometry could not be read from a game install.
#[derive(Debug, thiserror::Error)]
pub enum FireSectionSourceError {
    #[error("could not resolve assets.bin's path in the game vfs: {0}")]
    Path(String),
    #[error("could not open assets.bin: {0}")]
    Open(String),
    #[error("could not read assets.bin: {0}")]
    Read(#[from] std::io::Error),
    #[error("could not parse assets.bin: {0}")]
    Parse(String),
}

/// Where fire-section geometry comes from for one game build.
///
/// The whole batch of hulls is resolved in one call because opening the
/// source means parsing that `assets.bin`, which is the cost worth paying
/// once.
pub trait FireSectionSource {
    /// Geometry for the hulls in `wanted`, keyed by hull model path. A hull
    /// the source has no geometry for is absent from the map; `Err` means the
    /// source itself could not be read, which is true of builds shipping no
    /// `assets.bin` at all.
    fn resolve_all(
        &self,
        wanted: &[(&str, usize)],
    ) -> Result<HashMap<String, FireSectionGeometry>, FireSectionSourceError>;
}

/// The real source: `content/assets.bin` out of a build's game VFS.
pub struct VfsFireSections<'a> {
    pub vfs: &'a VfsPath,
}

impl FireSectionSource for VfsFireSections<'_> {
    fn resolve_all(
        &self,
        wanted: &[(&str, usize)],
    ) -> Result<HashMap<String, FireSectionGeometry>, FireSectionSourceError> {
        let bytes = read_assets_bin(self.vfs)?;
        let db = wowsunpack::models::assets_bin::parse_assets_bin(&bytes)
            .map_err(|error| FireSectionSourceError::Parse(error.to_string()))?;
        let self_id_index = db.build_self_id_index();
        let mut resolved = HashMap::new();
        for &(path, nodes) in wanted {
            match wowsunpack::models::fire_nodes::resolve_fire_sections(&db, &self_id_index, path, nodes) {
                Ok(geom) => {
                    resolved.insert(path.to_string(), geom);
                }
                Err(error) => tracing::debug!(hull = path, %error, "fire-section geometry unresolved"),
            }
        }
        Ok(resolved)
    }
}

fn read_assets_bin(vfs: &VfsPath) -> Result<Vec<u8>, FireSectionSourceError> {
    let assets_path = vfs.join("content/assets.bin").map_err(|e| FireSectionSourceError::Path(e.to_string()))?;
    let mut file = assets_path.open_file().map_err(|e| FireSectionSourceError::Open(e.to_string()))?;
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut file, &mut bytes)?;
    Ok(bytes)
}

/// Fire-section lookups that came back empty for a reason that holds until
/// the build's game data changes. The on-disk
/// [`FireSectionCache`](wowsunpack::models::fire_nodes_cache::FireSectionCache)
/// stores what resolved; without this, what did not resolve is retried for
/// every replay.
#[derive(Default)]
pub struct FireSectionFailures {
    /// Builds whose `assets.bin` could not be read or parsed. Builds older
    /// than the file itself ship none at all.
    no_source: HashSet<u32>,
    /// Hulls a readable `assets.bin` held no geometry for, per build.
    unresolvable_hulls: HashMap<u32, HashSet<String>>,
}

impl FireSectionFailures {
    pub fn has_no_source(&self, build: u32) -> bool {
        self.no_source.contains(&build)
    }

    pub fn note_no_source(&mut self, build: u32) {
        self.no_source.insert(build);
    }

    pub fn hull_is_unresolvable(&self, build: u32, hull_model_path: &str) -> bool {
        self.unresolvable_hulls.get(&build).is_some_and(|hulls| hulls.contains(hull_model_path))
    }

    pub fn note_unresolvable_hull(&mut self, build: u32, hull_model_path: &str) {
        self.unresolvable_hulls.entry(build).or_default().insert(hull_model_path.to_string());
    }

    /// Downloading a build's data can supply the `assets.bin` that was
    /// missing, so nothing recorded against it survives the download.
    pub fn forget_build(&mut self, build: u32) {
        self.no_source.remove(&build);
        self.unresolvable_hulls.remove(&build);
    }

    pub fn clear(&mut self) {
        self.no_source.clear();
        self.unresolvable_hulls.clear();
    }
}

/// Process-wide: these failures are facts about the game data on disk, not
/// about any one replay or report.
static FAILURES: LazyLock<RwLock<FireSectionFailures>> = LazyLock::new(RwLock::default);

/// Let `build` be probed for fire-section geometry again, after its game data
/// was downloaded.
pub fn forget_fire_section_failures(build: u32) {
    if let Ok(mut failures) = FAILURES.write() {
        failures.forget_build(build);
    }
}

/// Let every build be probed again, after the game directory or the game data
/// cache directory changed.
pub fn clear_fire_section_failures() {
    if let Ok(mut failures) = FAILURES.write() {
        failures.clear();
    }
}

/// Geometry for every hull among `victims`, backed by a per-build on-disk
/// cache so `assets.bin` is parsed once per hull per game build rather than
/// once per replay.
///
/// A hull that fails to resolve, or an unreadable `assets.bin`, simply has no
/// entry in the returned map: `analyze` treats a missing geometry as
/// `NoSectionGeometry` for that victim rather than failing the whole
/// statistic. `cache_dir` being `None` is the same story: resolution still
/// runs, it just is not persisted.
pub fn resolve_fire_section_geometry(
    vfs: &VfsPath,
    cache_dir: Option<&Path>,
    build_number: u32,
    victims: &HashMap<wows_replays::types::EntityId, VictimContext>,
) -> HashMap<String, FireSectionGeometry> {
    let mut expected_nodes: HashMap<&str, usize> = HashMap::new();
    for victim in victims.values() {
        expected_nodes.entry(victim.hull_model_path.as_str()).or_insert_with(|| victim.hull_section_count());
    }
    resolve_fire_section_geometry_from(&VfsFireSections { vfs }, &FAILURES, cache_dir, build_number, &expected_nodes)
}

/// [`resolve_fire_section_geometry`] against a given source and failure
/// record, with the hulls already reduced to one entry per hull model path.
///
/// `failures` is a parameter rather than the process-wide record so a test
/// can watch one run's effect on it without the previous test's leaking in.
pub fn resolve_fire_section_geometry_from(
    source: &dyn FireSectionSource,
    failures: &RwLock<FireSectionFailures>,
    cache_dir: Option<&Path>,
    build_number: u32,
    expected_nodes: &HashMap<&str, usize>,
) -> HashMap<String, FireSectionGeometry> {
    let mut cache =
        cache_dir.map(|dir| wowsunpack::models::fire_nodes_cache::FireSectionCache::load(dir, build_number));
    let mut resolved = HashMap::new();
    let mut misses = Vec::new();
    for (&path, &nodes) in expected_nodes {
        match cache.as_ref().and_then(|cache| cache.get(path, nodes)) {
            Some(geom) => {
                resolved.insert(path.to_string(), geom);
            }
            None => misses.push((path, nodes)),
        }
    }

    {
        let Ok(record) = failures.read() else { return resolved };
        misses.retain(|(path, _)| !record.hull_is_unresolvable(build_number, path));
        if misses.is_empty() || record.has_no_source(build_number) {
            return resolved;
        }
    }

    let mut stored = false;
    match source.resolve_all(&misses) {
        Ok(mut geometries) => {
            for (path, nodes) in misses {
                let Some(geom) = geometries.remove(path) else {
                    if let Ok(mut record) = failures.write() {
                        record.note_unresolvable_hull(build_number, path);
                    }
                    continue;
                };
                if let Some(cache) = cache.as_mut() {
                    match cache.insert(path, nodes, &geom) {
                        Ok(()) => stored = true,
                        Err(error) => tracing::warn!(
                            hull = path,
                            %error,
                            "freshly resolved fire-section geometry disagreed with the cache"
                        ),
                    }
                }
                resolved.insert(path.to_string(), geom);
            }
        }
        Err(error) => {
            match error {
                FireSectionSourceError::Parse(_) => {
                    tracing::warn!(%error, "could not parse assets.bin for fire-section geometry")
                }
                _ => tracing::debug!(%error, "assets.bin unavailable for fire-section geometry"),
            }
            if let Ok(mut record) = failures.write() {
                record.note_no_source(build_number);
            }
        }
    }

    if stored
        && let (Some(cache), Some(dir)) = (&cache, cache_dir)
        && let Err(error) = cache.save(dir)
    {
        tracing::warn!(%error, "could not save fire-section cache");
    }

    resolved
}

/// Effective fire chance for the recording player of `report`.
///
/// `None` whenever the underlying facts do not resolve: no self vehicle, no
/// resolvable build, an unresolved secondary battery, or no victim hull with
/// fire-section geometry. A result with no eligible hits is still `Some`, and
/// the render path shows it as an unknown rate rather than a zero one. Never
/// panics: an unreadable `assets.bin` degrades to a geometry lookup that
/// always misses, which `analyze` reports as no result rather than an
/// approximation.
pub fn compute_fire_chance(
    report: &BattleReport,
    params: &GameMetadataProvider,
    vfs: &VfsPath,
    build_number: u32,
    cache_dir: Option<&Path>,
) -> Option<EffectiveFireChance> {
    let resolved = match crate::fire_chance::resolve::resolve_fire_chance_input(report, params) {
        Ok(resolved) => resolved,
        Err(error) => {
            tracing::debug!(%error, "fire chance input did not resolve");
            return None;
        }
    };

    let geometry_map = resolve_fire_section_geometry(vfs, cache_dir, build_number, resolved.victims());
    let geometry = |path: &str| geometry_map.get(path).cloned();

    let input = resolved.input(report, params, &geometry);
    let result = crate::fire_chance::analysis::analyze(&input);
    if let Some(result) = &result {
        tracing::debug!(
            eligible_hits = result.eligible_hits,
            set_fire_ribbons = result.set_fire_ribbons,
            fires = result.fires,
            unattributed_fires = result.unattributed_fires,
            unattributed_reasons = ?result.unattributed_reasons,
            "computed effective fire chance"
        );
    }
    result
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::cell::RefCell;

    use wowsunpack::game_params::types::Meters;
    use wowsunpack::models::fire_nodes::FireSectionGeometry;

    use std::collections::HashMap;
    use std::sync::RwLock;

    use super::FireSectionFailures;
    use super::FireSectionSource;
    use super::FireSectionSourceError;
    use super::resolve_fire_section_geometry_from;

    /// A stand-in for one build's `assets.bin` that records what was asked of
    /// it. `hulls` is what it has geometry for; `None` is a build shipping no
    /// `assets.bin` at all, which is the case that fills the user's log.
    struct CountingSource {
        hulls: Option<Vec<&'static str>>,
        opens: Cell<u32>,
        asked: RefCell<Vec<String>>,
    }

    impl CountingSource {
        fn without_assets_bin() -> CountingSource {
            CountingSource { hulls: None, opens: Cell::new(0), asked: RefCell::new(Vec::new()) }
        }

        fn with_hulls(hulls: &[&'static str]) -> CountingSource {
            CountingSource { hulls: Some(hulls.to_vec()), opens: Cell::new(0), asked: RefCell::new(Vec::new()) }
        }

        fn opens(&self) -> u32 {
            self.opens.get()
        }

        fn times_asked_for(&self, hull: &str) -> usize {
            self.asked.borrow().iter().filter(|asked| *asked == hull).count()
        }
    }

    impl FireSectionSource for CountingSource {
        fn resolve_all(
            &self,
            wanted: &[(&str, usize)],
        ) -> Result<HashMap<String, FireSectionGeometry>, FireSectionSourceError> {
            self.opens.set(self.opens.get() + 1);
            let Some(hulls) = &self.hulls else {
                return Err(FireSectionSourceError::Open("no such file".to_string()));
            };
            let mut resolved = HashMap::new();
            for &(path, nodes) in wanted {
                self.asked.borrow_mut().push(path.to_string());
                if hulls.contains(&path) {
                    resolved.insert(path.to_string(), geometry(nodes));
                }
            }
            Ok(resolved)
        }
    }

    fn geometry(nodes: usize) -> FireSectionGeometry {
        FireSectionGeometry::from_longitudinal((0..nodes).map(|i| Meters::from(-(i as f32))).collect())
            .expect("a small node count is a valid geometry")
    }

    fn hulls(entries: &[(&'static str, usize)]) -> HashMap<&'static str, usize> {
        entries.iter().copied().collect()
    }

    /// The on-disk cache only holds what resolved, so a build with no
    /// `assets.bin` leaves every hull a miss and re-opens the file for every
    /// replay unless the failure itself is remembered.
    #[test]
    fn a_build_without_assets_bin_is_only_probed_once() {
        let source = CountingSource::without_assets_bin();
        let failures = RwLock::new(FireSectionFailures::default());
        let wanted = hulls(&[("iowa.model", 4)]);

        assert!(resolve_fire_section_geometry_from(&source, &failures, None, 111, &wanted).is_empty());
        assert!(resolve_fire_section_geometry_from(&source, &failures, None, 111, &wanted).is_empty());

        assert_eq!(source.opens(), 1, "the second replay of this build must not re-open assets.bin");
    }

    /// One build's missing `assets.bin` says nothing about another build's.
    /// A record that is not per build silently drops fire chance for every
    /// build the user does have data for.
    #[test]
    fn a_build_with_assets_bin_is_still_probed() {
        let missing = CountingSource::without_assets_bin();
        let present = CountingSource::with_hulls(&["iowa.model"]);
        let failures = RwLock::new(FireSectionFailures::default());
        let wanted = hulls(&[("iowa.model", 4)]);

        assert!(resolve_fire_section_geometry_from(&missing, &failures, None, 111, &wanted).is_empty());

        let resolved = resolve_fire_section_geometry_from(&present, &failures, None, 222, &wanted);
        assert!(resolved.contains_key("iowa.model"), "a build that has assets.bin still resolves");
        assert_eq!(present.opens(), 1);
    }

    /// A hull a readable `assets.bin` has no geometry for is permanently
    /// unresolvable for that build, and every replay featuring that ship would
    /// otherwise walk the path store for it again.
    #[test]
    fn an_unresolvable_hull_is_not_retried_for_every_replay() {
        let source = CountingSource::with_hulls(&["iowa.model"]);
        let failures = RwLock::new(FireSectionFailures::default());
        let wanted = hulls(&[("iowa.model", 4), ("ghost.model", 3)]);

        let first = resolve_fire_section_geometry_from(&source, &failures, None, 111, &wanted);
        assert!(first.contains_key("iowa.model"));
        assert!(!first.contains_key("ghost.model"));

        let second = resolve_fire_section_geometry_from(&source, &failures, None, 111, &wanted);
        assert!(second.contains_key("iowa.model"), "the resolvable hull is still resolved");

        assert_eq!(source.times_asked_for("ghost.model"), 1, "the unresolvable hull is asked for once");
        assert_eq!(source.times_asked_for("iowa.model"), 2, "the resolvable hull is not swept up in the record");
    }

    /// The app tells the user to download the build's data and then downloads
    /// it. A record surviving that leaves fire chance permanently off for a
    /// build whose `assets.bin` is now on disk.
    #[test]
    fn downloading_a_builds_data_lets_it_be_probed_again() {
        let source = CountingSource::without_assets_bin();
        let failures = RwLock::new(FireSectionFailures::default());
        let wanted = hulls(&[("iowa.model", 4)]);

        assert!(resolve_fire_section_geometry_from(&source, &failures, None, 111, &wanted).is_empty());
        assert_eq!(source.opens(), 1);

        failures.write().expect("the record is not poisoned").forget_build(111);

        assert!(resolve_fire_section_geometry_from(&source, &failures, None, 111, &wanted).is_empty());
        assert_eq!(source.opens(), 2, "the download must buy the build a second probe");
    }

    /// Pointing the app at different game data invalidates every record, hull
    /// records included.
    #[test]
    fn clearing_the_record_lets_an_unresolvable_hull_be_probed_again() {
        let source = CountingSource::with_hulls(&[]);
        let failures = RwLock::new(FireSectionFailures::default());
        let wanted = hulls(&[("ghost.model", 3)]);

        resolve_fire_section_geometry_from(&source, &failures, None, 111, &wanted);
        failures.write().expect("the record is not poisoned").clear();
        resolve_fire_section_geometry_from(&source, &failures, None, 111, &wanted);

        assert_eq!(source.times_asked_for("ghost.model"), 2);
    }
}
