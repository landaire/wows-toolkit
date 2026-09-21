//! Derived replay-listing state cached across frames.
//!
//! The listing draws from three layers, each rebuilt only when its inputs
//! move rather than every frame:
//!
//! - rows: the path-descending sorted listing, keyed on the files revision.
//! - groups: ship/date group structure and tree node-id maps, keyed on the
//!   files revision plus (for ship grouping) the metadata provider and locale
//!   that ship names resolve through.
//! - stats: per-group win-rate counts and prebuilt dir-node labels from the
//!   index summaries, keyed on the files and summaries revisions. Hydrated
//!   replays whose parsed outcome overrides the index are patched at draw
//!   time instead of invalidating this layer.
//!
//! [`ReplayWorkspace`] owns one [`ListingCacheSlot`]; the get-or-build
//! methods here hand `Arc`s to the draw code so nothing borrows the
//! workspace across the frame.

use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;
use wowsunpack::game_params::provider::GameMetadataProvider;

use super::GroupedListing;
use super::listing_row;
use super::listing_row::ListedReplay;
use crate::db::index::rows::MatchOutcome;
use crate::db::index::rows::RowSummary;
use crate::db::index::rows::WorkspaceId;
use crate::ui::replay_parser::workspace::ReplayWorkspace;
use crate::ui::replay_parser::workspace::workspace_group_salt;
use crate::ui::replay_parser::workspace::workspace_leaf_salt;

pub(crate) type ListedRow = (PathBuf, Arc<ListedReplay>);

/// Index of a row in the cache's sorted listing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RowIdx(pub u32);

impl RowIdx {
    pub fn get(self, rows: &[ListedRow]) -> &ListedRow {
        &rows[self.0 as usize]
    }
}

/// One dir node of a grouped listing.
pub(crate) struct CachedGroup {
    pub name: String,
    /// The dir node id `workspace_group_salt` derives for this group.
    pub id: egui::Id,
    /// Members in listing order.
    pub rows: Vec<RowIdx>,
}

/// Node-id lookups for tree actions: context menus, selection expansion,
/// double-click activation.
pub(crate) struct TreeIdMaps {
    /// Leaf node id -> file path.
    pub leaf_paths: HashMap<egui::Id, PathBuf>,
    /// Group node id -> its leaves' node ids, in listing order.
    pub group_child_ids: HashMap<egui::Id, Vec<egui::Id>>,
}

impl TreeIdMaps {
    /// The member paths of the group node `id`, or `None` when `id` is not a
    /// group. Built on demand: menus and selection expansion want it rarely,
    /// so no per-group path list is kept resident.
    pub fn group_paths(&self, id: &egui::Id) -> Option<Vec<PathBuf>> {
        let child_ids = self.group_child_ids.get(id)?;
        Some(child_ids.iter().filter_map(|child| self.leaf_paths.get(child).cloned()).collect())
    }

    /// Collect paths from a set of selected node IDs, deduplicating leaf nodes
    /// that are already covered by a selected group.
    pub fn collect_selected(&self, selected_ids: &[egui::Id]) -> Vec<PathBuf> {
        let mut covered_by_group: HashSet<egui::Id> = HashSet::new();
        let mut paths: Vec<PathBuf> = Vec::new();
        for id in selected_ids {
            if let Some(child_ids) = self.group_child_ids.get(id) {
                paths.extend(child_ids.iter().filter_map(|child| self.leaf_paths.get(child).cloned()));
                covered_by_group.extend(child_ids.iter().copied());
            }
            if !covered_by_group.contains(id)
                && let Some(path) = self.leaf_paths.get(id)
            {
                paths.push(path.clone());
            }
        }
        paths
    }
}

/// Group structure for one grouping mode.
pub(crate) struct GroupsCache {
    pub groups: Vec<CachedGroup>,
    /// Group name -> index in `groups`, for locating the group a hydrated
    /// replay's outcome patch lands on.
    pub by_name: HashMap<String, u32>,
    pub tree: Arc<TreeIdMaps>,
}

/// Decided-outcome tallies behind a group header's win-rate suffix.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct OutcomeCounts {
    pub wins: u32,
    pub losses: u32,
}

impl OutcomeCounts {
    pub fn add(&mut self, outcome: MatchOutcome) {
        match outcome {
            MatchOutcome::Win => self.wins += 1,
            MatchOutcome::Loss => self.losses += 1,
            MatchOutcome::Draw | MatchOutcome::Unknown => {}
        }
    }

    /// Remove a previously counted outcome. Saturating: a patch that removes
    /// an outcome never counted (a summary that changed under us) must not
    /// wrap the tally.
    pub fn remove(&mut self, outcome: MatchOutcome) {
        match outcome {
            MatchOutcome::Win => self.wins = self.wins.saturating_sub(1),
            MatchOutcome::Loss => self.losses = self.losses.saturating_sub(1),
            MatchOutcome::Draw | MatchOutcome::Unknown => {}
        }
    }

    /// The ` - xW/yL (%)` suffix a group header carries, empty when no member
    /// has a decided result.
    pub fn suffix(self) -> String {
        let total = self.wins + self.losses;
        if total > 0 {
            format!(" - {}W/{}L ({:.0}%)", self.wins, self.losses, (f64::from(self.wins) / f64::from(total)) * 100.0)
        } else {
            String::new()
        }
    }
}

/// Summary-derived layer for one grouping mode, parallel to
/// `GroupsCache::groups`.
pub(crate) struct StatsCache {
    pub counts: Vec<OutcomeCounts>,
    /// Fully formatted dir-node labels: `Name (n) - xW/yL (%)`.
    pub labels: Vec<String>,
}

struct RowsEntry {
    files_rev: u64,
    rows: Arc<Vec<ListedRow>>,
}

struct GroupsEntry {
    files_rev: u64,
    /// The provider ship names resolved through; `None` for date grouping,
    /// which never consults it. Held as a `Weak`: it keeps the allocation
    /// alive, so a later provider can never reuse the address and pointer
    /// equality cannot false-positive (no ABA).
    provider: Option<std::sync::Weak<GameMetadataProvider>>,
    /// The provider's catalog-swap count when the names were resolved, so an
    /// in-place `set_translations` invalidates them.
    translation_epoch: u64,
    locale: Option<String>,
    groups: Arc<GroupsCache>,
}

struct StatsEntry {
    files_rev: u64,
    summaries_rev: u64,
    stats: Arc<StatsCache>,
}

#[derive(Default)]
pub(crate) struct ListingCacheSlot {
    rows: Option<RowsEntry>,
    ship: Option<GroupsEntry>,
    date: Option<GroupsEntry>,
    ship_stats: Option<StatsEntry>,
    date_stats: Option<StatsEntry>,
    artifacts: Arc<RowArtifactCache>,
}

/// The built widgets of one listing row: the two-line label and its tooltip
/// text. Everything here is a pure function of the inputs an
/// [`ArtifactEpoch`] captures plus the row's path and selection state.
pub(crate) struct RowArtifacts {
    pub label: egui::text::LayoutJob,
    pub hover: String,
}

/// The inputs every cached row artifact was built against. Any change means
/// every entry is stale at once, so a mismatch clears the cache instead of
/// tagging entries individually.
///
/// Not captured because it cannot change today: the body font (zoom scales
/// `pixels_per_point`, not point size, and no text-size setting exists) and
/// the grouping mode (only the ungrouped listing uses this cache; grouped
/// rows would need the grouping in the key or epoch before sharing it).
#[derive(Debug, Clone)]
pub(crate) struct ArtifactEpoch {
    pub files_rev: u64,
    pub summaries_rev: u64,
    /// The provider row identities resolve through, plus its catalog-swap
    /// count. Held as a `Weak`: it keeps the allocation alive, so a later
    /// provider can never reuse the address and the pointer comparison in
    /// `PartialEq` cannot false-positive (no ABA).
    pub provider: std::sync::Weak<GameMetadataProvider>,
    pub translation_epoch: u64,
    pub locale: Option<String>,
    /// Rows color by theme, so a theme flip rebuilds them.
    pub dark_mode: bool,
}

impl PartialEq for ArtifactEpoch {
    fn eq(&self, other: &Self) -> bool {
        self.files_rev == other.files_rev
            && self.summaries_rev == other.summaries_rev
            && std::sync::Weak::ptr_eq(&self.provider, &other.provider)
            && self.translation_epoch == other.translation_epoch
            && self.locale == other.locale
            && self.dark_mode == other.dark_mode
    }
}

impl Eq for ArtifactEpoch {}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ArtifactKey {
    path: PathBuf,
    selected: bool,
}

struct ArtifactEntry {
    artifacts: Arc<RowArtifacts>,
    tick: u64,
}

#[derive(Default)]
struct ArtifactInner {
    epoch: Option<ArtifactEpoch>,
    map: HashMap<ArtifactKey, ArtifactEntry>,
    /// Recency queue of (key, tick-at-push). An entry whose recorded tick is
    /// older than the map's is stale and skipped on eviction, which keeps
    /// both push and evict amortized O(1).
    queue: VecDeque<(ArtifactKey, u64)>,
    tick: u64,
    capacity: usize,
}

/// LRU cache of built row widgets, bounded to what the viewport can show.
///
/// Interior mutability because rows are built inside the scroll area's draw
/// closure, where the workspace is already borrowed shared.
#[derive(Default)]
pub(crate) struct RowArtifactCache {
    inner: Mutex<ArtifactInner>,
}

/// The smallest multiple of 64 that covers the viewport three times over: the
/// visible rows plus a screen of scroll headroom in each direction.
pub(crate) fn artifact_capacity(visible_rows: usize) -> usize {
    visible_rows.saturating_mul(3).checked_next_multiple_of(64).unwrap_or(usize::MAX).max(64)
}

impl RowArtifactCache {
    /// Sets the epoch and capacity for the coming frame. A changed epoch
    /// empties the cache; a shrunken capacity evicts down to fit.
    pub fn begin_frame(&self, epoch: ArtifactEpoch, capacity: usize) {
        let mut inner = self.inner.lock();
        if inner.epoch.as_ref() != Some(&epoch) {
            inner.map.clear();
            inner.queue.clear();
            inner.epoch = Some(epoch);
        }
        inner.capacity = capacity;
        while inner.map.len() > inner.capacity {
            Self::evict_one(&mut inner);
        }
    }

    pub fn get_or_insert_with(
        &self,
        path: &Path,
        selected: bool,
        build: impl FnOnce() -> RowArtifacts,
    ) -> Arc<RowArtifacts> {
        let mut inner = self.inner.lock();
        inner.tick += 1;
        let tick = inner.tick;
        let key = ArtifactKey { path: path.to_path_buf(), selected };
        if let Some(entry) = inner.map.get_mut(&key) {
            entry.tick = tick;
            let artifacts = Arc::clone(&entry.artifacts);
            inner.queue.push_back((key, tick));
            Self::compact(&mut inner);
            return artifacts;
        }
        let artifacts = Arc::new(build());
        inner.map.insert(key.clone(), ArtifactEntry { artifacts: Arc::clone(&artifacts), tick });
        inner.queue.push_back((key, tick));
        while inner.map.len() > inner.capacity {
            Self::evict_one(&mut inner);
        }
        Self::compact(&mut inner);
        artifacts
    }

    /// Drop superseded queue records once they outnumber the live ones. Hits
    /// push a record without evicting anything, so without this a steady
    /// cache (every visible row hitting, no misses) grows the queue by one
    /// path clone per row per frame, without bound.
    fn compact(inner: &mut ArtifactInner) {
        if inner.queue.len() <= 2 * inner.map.len().max(inner.capacity) {
            return;
        }
        let ArtifactInner { map, queue, .. } = inner;
        queue.retain(|(key, queued_tick)| map.get(key).is_some_and(|entry| entry.tick == *queued_tick));
    }

    fn evict_one(inner: &mut ArtifactInner) {
        while let Some((key, queued_tick)) = inner.queue.pop_front() {
            let Some(entry) = inner.map.get(&key) else { continue };
            // A newer queue entry supersedes this one; the entry is not
            // actually the least recently used.
            if entry.tick != queued_tick {
                continue;
            }
            inner.map.remove(&key);
            return;
        }
        // The queue emptied without finding an evictable entry, which cannot
        // happen while the map is non-empty; clear defensively so the loop in
        // the callers terminates either way.
        inner.map.clear();
    }
}

impl ReplayWorkspace {
    /// The sorted listing, rebuilt when the files revision moved. `None` when
    /// no directory has been listed, mirroring `replay_files()`.
    pub(crate) fn listing_rows(&mut self) -> Option<Arc<Vec<ListedRow>>> {
        let (files_rev, _) = self.listing_revisions();
        if let Some(entry) = &self.listing_cache.rows
            && entry.files_rev == files_rev
        {
            return Some(Arc::clone(&entry.rows));
        }
        let rows = Arc::new(build_rows(self.replay_files()?));
        self.listing_cache.rows = Some(RowsEntry { files_rev, rows: Arc::clone(&rows) });
        // Structures downstream of the rows are stale with certainty, so drop
        // them now rather than letting their own key checks find out.
        self.listing_cache.ship = None;
        self.listing_cache.date = None;
        self.listing_cache.ship_stats = None;
        self.listing_cache.date_stats = None;
        Some(rows)
    }

    /// Group structure for `mode`, rebuilt when the files revision moved or
    /// (ship grouping) ship names would resolve differently.
    pub(crate) fn listing_groups(
        &mut self,
        ws_id: WorkspaceId,
        mode: GroupedListing,
        provider: &Arc<GameMetadataProvider>,
        locale: Option<&str>,
    ) -> Option<Arc<GroupsCache>> {
        let rows = self.listing_rows()?;
        let (files_rev, _) = self.listing_revisions();
        // Date grouping reads nothing localized, so its cache ignores the
        // provider, its catalog and the app locale.
        let uses_provider = match mode {
            GroupedListing::Ship => true,
            GroupedListing::Date => false,
        };
        let translation_epoch = if uses_provider { provider.translation_epoch() } else { 0 };
        let locale_key = if uses_provider { locale } else { None };
        let slot = match mode {
            GroupedListing::Ship => &mut self.listing_cache.ship,
            GroupedListing::Date => &mut self.listing_cache.date,
        };
        if let Some(entry) = slot
            && entry.files_rev == files_rev
            && entry.translation_epoch == translation_epoch
            && entry.locale.as_deref() == locale_key
            && match (&entry.provider, uses_provider) {
                (None, false) => true,
                (Some(cached), true) => std::sync::Weak::ptr_eq(cached, &Arc::downgrade(provider)),
                _ => false,
            }
        {
            return Some(Arc::clone(&entry.groups));
        }
        let groups = Arc::new(build_groups(ws_id, mode, &rows, provider));
        *slot = Some(GroupsEntry {
            files_rev,
            provider: uses_provider.then(|| Arc::downgrade(provider)),
            translation_epoch,
            locale: locale_key.map(str::to_owned),
            groups: Arc::clone(&groups),
        });
        // Stats are parallel to the group list they were built from.
        match mode {
            GroupedListing::Ship => self.listing_cache.ship_stats = None,
            GroupedListing::Date => self.listing_cache.date_stats = None,
        }
        Some(groups)
    }

    /// The per-row widget cache. Shared out as an `Arc` so rows can populate
    /// it from inside draw closures that hold the workspace shared.
    pub(crate) fn row_artifacts(&self) -> Arc<RowArtifactCache> {
        Arc::clone(&self.listing_cache.artifacts)
    }

    /// Win-rate counts and dir labels for `mode`, rebuilt when either
    /// revision moved. Callers pass the `groups` they are about to draw so
    /// the two are always parallel.
    pub(crate) fn listing_group_stats(
        &mut self,
        mode: GroupedListing,
        groups: &GroupsCache,
        rows: &[ListedRow],
    ) -> Arc<StatsCache> {
        let (files_rev, summaries_rev) = self.listing_revisions();
        let slot = match mode {
            GroupedListing::Ship => &mut self.listing_cache.ship_stats,
            GroupedListing::Date => &mut self.listing_cache.date_stats,
        };
        if let Some(entry) = slot
            && entry.files_rev == files_rev
            && entry.summaries_rev == summaries_rev
        {
            return Arc::clone(&entry.stats);
        }
        let stats = Arc::new(build_group_stats(groups, rows, self.row_summaries()));
        let slot = match mode {
            GroupedListing::Ship => &mut self.listing_cache.ship_stats,
            GroupedListing::Date => &mut self.listing_cache.date_stats,
        };
        *slot = Some(StatsEntry { files_rev, summaries_rev, stats: Arc::clone(&stats) });
        stats
    }
}

fn build_rows(files: &HashMap<PathBuf, Arc<ListedReplay>>) -> Vec<ListedRow> {
    let mut rows: Vec<ListedRow> = files.iter().map(|(path, listed)| (path.clone(), Arc::clone(listed))).collect();
    // Descending: replay filenames embed their timestamp, so this is
    // newest-first.
    rows.sort_unstable_by(|a, b| b.0.cmp(&a.0));
    rows
}

/// The group a row belongs to under `mode`. Also used at draw time to locate
/// the group a hydrated replay's outcome patch applies to.
pub(crate) fn group_key(mode: GroupedListing, listed: &ListedReplay, provider: &GameMetadataProvider) -> String {
    match mode {
        GroupedListing::Ship => listing_row::listed_ship_name(listed, provider),
        GroupedListing::Date => listed.date_time.split(' ').next().unwrap_or(&listed.date_time).to_string(),
    }
}

fn group_id_salt(mode: GroupedListing) -> &'static str {
    match mode {
        GroupedListing::Ship => "ship_group",
        GroupedListing::Date => "date_group",
    }
}

fn build_groups(
    ws_id: WorkspaceId,
    mode: GroupedListing,
    rows: &[ListedRow],
    provider: &GameMetadataProvider,
) -> GroupsCache {
    debug_assert!(rows.len() <= u32::MAX as usize, "RowIdx is u32");
    let mut groups: Vec<CachedGroup> = Vec::new();
    let mut by_name: HashMap<String, u32> = HashMap::new();
    // Rows are newest-first, so first-seen order puts each group at the
    // position of its most recent member: ships in most-recently-played
    // order, dates descending.
    for (idx, (_, listed)) in rows.iter().enumerate() {
        let name = group_key(mode, listed, provider);
        let group_idx = *by_name.entry(name.clone()).or_insert_with(|| {
            groups.push(CachedGroup {
                id: workspace_group_salt(ws_id, group_id_salt(mode), &name),
                name,
                rows: Vec::new(),
            });
            (groups.len() - 1) as u32
        });
        groups[group_idx as usize].rows.push(RowIdx(idx as u32));
    }

    let mut leaf_paths: HashMap<egui::Id, PathBuf> = HashMap::with_capacity(rows.len());
    let mut group_child_ids: HashMap<egui::Id, Vec<egui::Id>> = HashMap::with_capacity(groups.len());
    for group in &groups {
        let mut child_ids = Vec::with_capacity(group.rows.len());
        for &row in &group.rows {
            let (path, _) = row.get(rows);
            let id = workspace_leaf_salt(ws_id, path);
            leaf_paths.insert(id, path.clone());
            child_ids.push(id);
        }
        group_child_ids.insert(group.id, child_ids);
    }

    GroupsCache { groups, by_name, tree: Arc::new(TreeIdMaps { leaf_paths, group_child_ids }) }
}

fn build_group_stats(groups: &GroupsCache, rows: &[ListedRow], summaries: &HashMap<PathBuf, RowSummary>) -> StatsCache {
    let mut counts = Vec::with_capacity(groups.groups.len());
    let mut labels = Vec::with_capacity(groups.groups.len());
    for group in &groups.groups {
        let mut tally = OutcomeCounts::default();
        for &row in &group.rows {
            let (path, _) = row.get(rows);
            if let Some(summary) = summaries.get(path) {
                tally.add(summary.outcome);
            }
        }
        labels.push(format!("{} ({}){}", group.name, group.rows.len(), tally.suffix()));
        counts.push(tally);
    }
    StatsCache { counts, labels }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::replay_parser::workspace::ReplayWorkspace;

    fn listed(date_time: &str) -> Arc<ListedReplay> {
        Arc::new(ListedReplay {
            ship_id: None,
            map_name: "map".to_string(),
            game_type: "RandomBattle".to_string(),
            scenario: "Domination".to_string(),
            date_time: date_time.to_string(),
            build: None,
        })
    }

    fn summary(outcome: MatchOutcome) -> RowSummary {
        RowSummary {
            outcome,
            self_damage: None,
            self_kills: None,
            self_survived: None,
            self_pr: None,
            division_id: None,
            division_mates: Vec::new(),
            results_available: true,
            file_mtime: None,
        }
    }

    fn provider() -> Arc<GameMetadataProvider> {
        Arc::new(GameMetadataProvider::from_params_no_specs(Vec::new()).expect("an empty param list is always valid"))
    }

    #[test]
    fn win_rate_suffix_counts_only_decided_outcomes() {
        use MatchOutcome::Draw;
        use MatchOutcome::Loss;
        use MatchOutcome::Unknown;
        use MatchOutcome::Win;

        let tally = |outcomes: &[MatchOutcome]| {
            let mut counts = OutcomeCounts::default();
            for &outcome in outcomes {
                counts.add(outcome);
            }
            counts.suffix()
        };
        assert_eq!(tally(&[Win, Win, Win, Loss]), " - 3W/1L (75%)");
        // A draw and an unindexed row are neither a win nor a loss, and must
        // not enter the denominator either.
        assert_eq!(tally(&[Win, Loss, Draw, Unknown]), " - 1W/1L (50%)");
        assert_eq!(tally(&[Draw, Unknown]), "", "no decided result means no label at all");
        assert_eq!(tally(&[]), "");
    }

    #[test]
    fn removing_an_outcome_never_counted_saturates() {
        let mut counts = OutcomeCounts::default();
        counts.remove(MatchOutcome::Win);
        counts.add(MatchOutcome::Loss);
        assert_eq!(counts, OutcomeCounts { wins: 0, losses: 1 });
    }

    #[test]
    fn date_groups_merge_same_date_and_order_newest_first() {
        let mut ws = ReplayWorkspace::new(None);
        ws.set_replay_files(Some(HashMap::from([
            (PathBuf::from("c.wowsreplay"), listed("02.08.2026 10:00:00")),
            (PathBuf::from("a.wowsreplay"), listed("01.08.2026 09:00:00")),
            (PathBuf::from("b.wowsreplay"), listed("02.08.2026 08:00:00")),
        ])));
        let ws_id = crate::db::index::rows::WorkspaceId(7);
        let groups =
            ws.listing_groups(ws_id, GroupedListing::Date, &provider(), None).expect("files were listed above");

        let names: Vec<&str> = groups.groups.iter().map(|group| group.name.as_str()).collect();
        assert_eq!(names, ["02.08.2026", "01.08.2026"], "one group per date, newest first");
        assert_eq!(groups.groups[0].rows.len(), 2, "both replays of the date share its group");
        assert_eq!(groups.tree.leaf_paths.len(), 3);
        assert_eq!(
            groups.tree.group_child_ids.get(&groups.groups[0].id).map(Vec::len),
            Some(2),
            "the tree map mirrors group membership"
        );
    }

    #[test]
    fn caches_rebuild_only_when_their_revision_moves() {
        let mut ws = ReplayWorkspace::new(None);
        ws.set_replay_files(Some(HashMap::from([(PathBuf::from("a.wowsreplay"), listed("01.08.2026 09:00:00"))])));

        let first = ws.listing_rows().expect("files were listed above");
        let again = ws.listing_rows().expect("files were listed above");
        assert!(Arc::ptr_eq(&first, &again), "an unchanged listing must reuse the cached rows");

        ws.replay_files_mut()
            .get_or_insert_with(HashMap::new)
            .insert(PathBuf::from("b.wowsreplay"), listed("02.08.2026 10:00:00"));
        let rebuilt = ws.listing_rows().expect("files are still listed");
        assert!(!Arc::ptr_eq(&first, &rebuilt), "a mutated listing must rebuild");
        assert_eq!(rebuilt.len(), 2);
        assert_eq!(rebuilt[0].0, PathBuf::from("b.wowsreplay"), "rows sort newest (path-descending) first");
    }

    #[test]
    fn artifact_capacity_is_a_viewport_sized_multiple_of_64() {
        assert_eq!(artifact_capacity(0), 64);
        assert_eq!(artifact_capacity(10), 64, "3x10 rounds up to one block");
        assert_eq!(artifact_capacity(40), 128, "3x40 rounds up to the next multiple");
        assert_eq!(artifact_capacity(64), 192, "an exact multiple is kept");
    }

    #[test]
    fn row_artifacts_evict_least_recent_and_clear_on_epoch_change() {
        let epoch = |files_rev| ArtifactEpoch {
            files_rev,
            summaries_rev: 0,
            provider: std::sync::Weak::new(),
            translation_epoch: 0,
            locale: None,
            dark_mode: false,
        };
        let build = || RowArtifacts { label: egui::text::LayoutJob::default(), hover: String::new() };

        let cache = RowArtifactCache::default();
        cache.begin_frame(epoch(0), 64);
        let first = cache.get_or_insert_with(Path::new("a"), false, build);
        let again = cache.get_or_insert_with(Path::new("a"), false, build);
        assert!(Arc::ptr_eq(&first, &again), "a cached row must be reused");

        // 32 fillers, a touch of "a", then enough fillers to force evictions:
        // the pre-touch fillers are older than "a" and must evict first.
        for i in 0..32 {
            cache.get_or_insert_with(Path::new(&format!("early-{i}")), false, build);
        }
        cache.get_or_insert_with(Path::new("a"), false, build);
        for i in 0..62 {
            cache.get_or_insert_with(Path::new(&format!("late-{i}")), false, build);
        }
        let survived = cache.get_or_insert_with(Path::new("a"), false, build);
        assert!(Arc::ptr_eq(&first, &survived), "a recently used row must survive eviction");

        cache.begin_frame(epoch(1), 64);
        let rebuilt = cache.get_or_insert_with(Path::new("a"), false, build);
        assert!(!Arc::ptr_eq(&first, &rebuilt), "an epoch change must drop every cached row");
    }

    #[test]
    fn group_stats_rebuild_when_summaries_move() {
        let ws_id = crate::db::index::rows::WorkspaceId(7);
        let mut ws = ReplayWorkspace::new(None);
        ws.set_replay_files(Some(HashMap::from([(PathBuf::from("a.wowsreplay"), listed("01.08.2026 09:00:00"))])));

        let rows = ws.listing_rows().expect("files were listed above");
        let groups =
            ws.listing_groups(ws_id, GroupedListing::Date, &provider(), None).expect("files were listed above");
        let stats = ws.listing_group_stats(GroupedListing::Date, &groups, &rows);
        assert_eq!(stats.labels[0], "01.08.2026 (1)", "no summary, no win-rate suffix");
        let again = ws.listing_group_stats(GroupedListing::Date, &groups, &rows);
        assert!(Arc::ptr_eq(&stats, &again), "unchanged summaries must reuse the cached stats");

        ws.set_row_summaries(HashMap::from([(PathBuf::from("a.wowsreplay"), summary(MatchOutcome::Win))]));
        let rebuilt = ws.listing_group_stats(GroupedListing::Date, &groups, &rows);
        assert_eq!(rebuilt.labels[0], "01.08.2026 (1) - 1W/0L (100%)");
        assert_eq!(rebuilt.counts[0], OutcomeCounts { wins: 1, losses: 0 });
    }
}
