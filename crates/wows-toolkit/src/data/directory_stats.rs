use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::PathBuf;

use wows_toolkit_viewmodel::stats::replay_set::ReplaySetGames;
use wows_toolkit_viewmodel::stats::replay_set::ReplaySetProgress;
use wows_toolkit_viewmodel::stats::replay_set::ReplaySetSkips;

use crate::data::session_stats::PerGameStat;
use crate::data::session_stats::SessionStats;
use crate::data::settings::StatsFilterSettings;
use crate::tab_state::SessionStatsChartConfig;
use crate::tab_state::StatsSubTab;
use crate::tab_state::TabState;
use crate::task::BackgroundTaskKind;
use crate::task::directory_stats::DirectoryStatsUpdate;

/// Runtime handle for one directory stats pane. Valid only within a single
/// run: the panes are not saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DirectoryStatsId(pub u64);

/// Where a pane's read of its directory has got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadState {
    /// Listing the directory; no replay found yet.
    Walking,
    /// Reading each replay's header to group them by build.
    Scanning(ReplaySetProgress),
    Reading(ReplaySetProgress),
    Finished,
}

/// A stats pane over every replay under one directory. Holds the state the
/// session stats tab keeps in the persisted settings, so the same rendering
/// serves both.
pub struct DirectoryStatsPane {
    pub root: PathBuf,

    games: ReplaySetGames,
    /// `games` as the stats rendering reads it. Rebuilt when `games` changes,
    /// never per frame.
    pub session: SessionStats,
    pub read: ReadState,
    pub skips: ReplaySetSkips,

    pub filters: StatsFilterSettings,
    pub dock_state: egui_dock::DockState<StatsSubTab>,
    pub chart_configs: HashMap<u64, SessionStatsChartConfig>,
    /// `default_stats_dock_state` opens chart 0, so ids start after it.
    pub next_chart_tab_id: u64,
}

impl DirectoryStatsPane {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            games: ReplaySetGames::new(),
            session: SessionStats::default(),
            read: ReadState::Walking,
            skips: ReplaySetSkips::default(),
            // A new pane shows every game in its directory.
            filters: StatsFilterSettings::default(),
            dock_state: crate::tab_state::default_stats_dock_state(),
            chart_configs: HashMap::new(),
            next_chart_tab_id: 1,
        }
    }

    fn restart(&mut self) {
        self.games = ReplaySetGames::new();
        self.session.games.clear();
        self.read = ReadState::Walking;
        self.skips = ReplaySetSkips::default();
    }

    fn rebuild_session(&mut self) {
        self.session.games = self.games.games().iter().map(PerGameStat::from_shared).collect();
    }
}

impl TabState {
    /// Open a stats pane over every replay under `root`, or return the pane
    /// already open for it. `None` while game data has not loaded, since no
    /// replay can be parsed before then.
    pub fn open_directory_stats(&mut self, root: PathBuf) -> Option<DirectoryStatsId> {
        if let Some((&id, _)) = self.directory_stats.iter().find(|(_, pane)| pane.root == root) {
            return Some(id);
        }

        let deps = self.replay_dependencies()?;
        let id = DirectoryStatsId(self.next_directory_stats_id);
        self.next_directory_stats_id += 1;

        let task = crate::task::directory_stats::start_directory_stats(deps, id, root.clone());
        self.background_tasks.push(task);
        self.directory_stats.insert(id, DirectoryStatsPane::new(root));

        Some(id)
    }

    /// Read the pane's directory again from the start, picking up replays
    /// added or removed since. Does nothing while a read is running.
    pub fn reload_directory_stats(&mut self, id: DirectoryStatsId) {
        let Some(deps) = self.replay_dependencies() else {
            return;
        };
        let Some(pane) = self.directory_stats.get_mut(&id) else {
            return;
        };
        if pane.read != ReadState::Finished {
            return;
        }

        pane.restart();
        let task = crate::task::directory_stats::start_directory_stats(deps, id, pane.root.clone());
        self.background_tasks.push(task);
    }

    /// Fold what running reads sent into the panes they belong to. An update
    /// for a closed pane is dropped: it belongs to nothing else.
    pub fn apply_directory_stats_updates(&mut self, updates: Vec<DirectoryStatsUpdate>) {
        // Folded in once per pane: a frame can drain many replays, and each
        // fold re-sorts the pane's games and rebuilds what the tab renders.
        let mut new_games: BTreeMap<DirectoryStatsId, Vec<wows_toolkit_viewmodel::stats::PerGameStat>> =
            BTreeMap::new();
        for update in updates {
            let Some(pane) = self.directory_stats.get_mut(&update.pane()) else {
                continue;
            };

            match update {
                DirectoryStatsUpdate::Scanning { progress, .. } => pane.read = ReadState::Scanning(progress),
                DirectoryStatsUpdate::Read { pane: id, game, progress, skips } => {
                    pane.read = ReadState::Reading(progress);
                    pane.skips = skips;
                    new_games.entry(id).or_default().extend(game.map(|game| *game));
                }
            }
        }

        for (id, games) in new_games {
            if games.is_empty() {
                continue;
            }
            if let Some(pane) = self.directory_stats.get_mut(&id) {
                pane.games.extend(games);
                pane.rebuild_session();
            }
        }
    }

    /// Mark the pane's read as over, however it ended.
    pub fn finish_directory_stats(&mut self, id: DirectoryStatsId) {
        if let Some(pane) = self.directory_stats.get_mut(&id) {
            pane.read = ReadState::Finished;
        }
    }

    /// Drop the pane and stop its read. Dropping the task drops the inbox the
    /// read sends to, which is what stops it. Tolerates an id already closed:
    /// egui_dock's context-menu close path calls this twice.
    pub fn close_directory_stats(&mut self, id: DirectoryStatsId) {
        self.directory_stats.remove(&id);
        self.background_tasks
            .retain(|task| !matches!(task.kind, BackgroundTaskKind::ReadingDirectoryStats { pane, .. } if pane == id));
    }
}
