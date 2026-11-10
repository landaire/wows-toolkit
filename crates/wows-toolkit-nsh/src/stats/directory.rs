//! Stats views over every replay in one directory, as GPUI opens them from a
//! replay listing: read on a thread of their own, folded into the view's
//! games in batches, and never saved.

use std::ops::ControlFlow;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use nsh_core::prelude::*;
use wows_toolkit_replay_data::directory_stats::DirectoryStatsUpdate;
use wows_toolkit_replay_data::directory_stats::read_directory;
use wows_toolkit_replay_data::load::GameDataCache;
use wows_toolkit_viewmodel::stats::chart_model::ChartId;
use wows_toolkit_viewmodel::stats::chart_model::ChartSettings;
use wows_toolkit_viewmodel::stats::replay_set::ReplaySetGames;
use wows_toolkit_viewmodel::stats::replay_set::ReplaySetProgress;
use wows_toolkit_viewmodel::stats::replay_set::ReplaySetSkips;

use super::ScopeView;
use super::scope::Scope;
use crate::text::text;
use crate::text::text_with;

/// How often a running read folds what it has into the view. Each fold
/// re-sorts the set and refilters every panel, so a fast read is folded in
/// batches, as the GPUI view does.
const FOLD_INTERVAL: Duration = Duration::from_millis(250);

/// Names one open directory view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct DirectoryId(pub(crate) u32);

impl DirectoryId {
    pub(crate) fn scope_name(self) -> String {
        format!("directory-{}", self.0)
    }

    pub(crate) fn parse(scope: &str) -> Option<Self> {
        scope.strip_prefix("directory-")?.parse().ok().map(Self)
    }
}

/// Where the read of a directory has got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReadState {
    /// Listing the directory; no replay found yet.
    Walking,
    /// Reading each replay's header to group them by build.
    Scanning(ReplaySetProgress),
    Reading(ReplaySetProgress),
    Finished,
}

/// A directory view's read, beside its scope.
pub(crate) struct DirectoryRead {
    root: PathBuf,
    state: ReadState,
    skips: ReplaySetSkips,
    games: ReplaySetGames,
    /// Set to stop the running read: the view closed or is reading again.
    cancel: Arc<AtomicBool>,
    revision: tokio::sync::watch::Sender<u32>,
}

impl DirectoryRead {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self {
            root,
            state: ReadState::Walking,
            skips: ReplaySetSkips::default(),
            games: ReplaySetGames::new(),
            cancel: Arc::new(AtomicBool::new(false)),
            revision: tokio::sync::watch::channel(0).0,
        }
    }

    pub(crate) fn root(&self) -> &PathBuf {
        &self.root
    }

    pub(crate) fn is_finished(&self) -> bool {
        self.state == ReadState::Finished
    }

    pub(crate) fn revisions(&self) -> tokio::sync::watch::Receiver<u32> {
        self.revision.subscribe()
    }

    /// Stops the running read, if any.
    pub(crate) fn stop(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Starts over from an empty set, stopping a read still running.
    fn restart(&mut self) -> Arc<AtomicBool> {
        self.stop();
        self.state = ReadState::Walking;
        self.skips = ReplaySetSkips::default();
        self.games = ReplaySetGames::new();
        self.cancel = Arc::new(AtomicBool::new(false));
        self.bump();
        Arc::clone(&self.cancel)
    }

    fn bump(&self) {
        self.revision.send_modify(|revision| *revision = revision.wrapping_add(1));
    }

    /// Folds `updates` in. Returns whether games were added.
    fn apply(&mut self, updates: Vec<DirectoryStatsUpdate>) -> bool {
        let mut games = Vec::new();
        for update in updates {
            match update {
                DirectoryStatsUpdate::Scanning(progress) => self.state = ReadState::Scanning(progress),
                DirectoryStatsUpdate::Read { game, progress, skips } => {
                    self.state = ReadState::Reading(progress);
                    self.skips = skips;
                    games.extend(game.map(|game| *game));
                }
            }
        }
        let added = !games.is_empty();
        self.games.extend(games);
        added
    }

    pub(crate) fn status(&self, view: ScopeView) -> DirectoryStatus {
        let progress = match self.state {
            ReadState::Scanning(progress) | ReadState::Reading(progress) => Some(f64::from(progress.fraction())),
            ReadState::Walking | ReadState::Finished => None,
        };
        let count = |progress: ReplaySetProgress| [("done", progress.done.to_string()), ("total", progress.total.to_string())];
        let status = match self.state {
            ReadState::Walking => Some(text("ui.stats.directory_walking")),
            ReadState::Scanning(progress) => {
                let [done, total] = count(progress);
                Some(text_with("ui.stats.directory_scanning", &[(done.0, &done.1), (total.0, &total.1)]))
            }
            ReadState::Reading(progress) => {
                let [done, total] = count(progress);
                Some(text_with("ui.stats.directory_reading", &[(done.0, &done.1), (total.0, &total.1)]))
            }
            ReadState::Finished => None,
        };
        let skips = self.skips;
        let skipped = (skips.total() > 0).then(|| {
            text_with("ui.stats.directory_skipped", &[("count", &skips.total().to_string())])
        });
        let skipped_detail = (skips.total() > 0).then(|| {
            text_with(
                "ui.stats.directory_skipped_detail",
                &[
                    ("unreadable", &skips.unreadable.to_string()),
                    ("missing_build", &skips.missing_build.to_string()),
                    ("no_results", &skips.no_results.to_string()),
                ],
            )
        });

        DirectoryStatus {
            revision: *self.revision.borrow(),
            root: self.root.display().to_string(),
            status,
            progress,
            finished: self.is_finished(),
            skipped,
            skipped_detail,
            no_games: (self.is_finished() && self.games.is_empty()).then(|| text("ui.stats.directory_no_games")),
            view,
        }
    }
}

/// How far a directory view's read has got, and what the view shows.
#[derive(Debug, Clone, PartialEq, JsObject)]
#[non_exhaustive]
pub struct DirectoryStatus {
    pub revision: u32,
    pub root: String,
    /// What the read is doing, while it runs.
    pub status: Option<String>,
    /// How far it has got, in `0..=1`, while it reads.
    pub progress: Option<f64>,
    pub finished: bool,
    pub skipped: Option<String>,
    /// Why each skipped replay was skipped, by reason.
    pub skipped_detail: Option<String>,
    /// Said once the read found no game at all.
    pub no_games: Option<String>,
    pub view: ScopeView,
}

/// A new directory scope, with the one damage chart every view opens with.
pub(crate) fn new_scope(id: DirectoryId) -> Scope {
    let mut scope = Scope::new(id.scope_name(), false);
    scope.add_chart(ChartSettings { id: Some(ChartId(0)), ..ChartSettings::default() });
    scope
}

/// Reads `id`'s directory from the start on a thread of its own.
pub(crate) fn start(id: DirectoryId, game_data: GameDataCache) {
    let begun = super::with_directory(id, |scope, read| {
        let cancel = read.restart();
        scope.set_games(Vec::new());
        Ok((read.root().clone(), cancel))
    });
    let Ok((root, cancel)) = begun else { return };

    let spawned = std::thread::Builder::new().name(format!("directory-stats-{}", id.0)).spawn(move || {
        let mut pending = Vec::new();
        let mut last_fold = Instant::now();
        read_directory(&root, &game_data, &cancel, &mut |update| {
            if cancel.load(Ordering::Relaxed) {
                return ControlFlow::Break(());
            }
            pending.push(update);
            if last_fold.elapsed() >= FOLD_INTERVAL {
                last_fold = Instant::now();
                fold(id, std::mem::take(&mut pending), &cancel);
            }
            ControlFlow::Continue(())
        });
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        fold(id, pending, &cancel);
        let _ = super::with_directory(id, |_, read| {
            read.state = ReadState::Finished;
            read.bump();
            Ok(())
        });
    });
    if let Err(error) = spawned {
        log::warn!("directory stats: no thread to read on: {error}");
        let _ = super::with_directory(id, |_, read| {
            read.state = ReadState::Finished;
            read.bump();
            Ok(())
        });
    }
}

fn fold(id: DirectoryId, updates: Vec<DirectoryStatsUpdate>, cancel: &AtomicBool) {
    let _ = super::with_directory(id, |scope, read| {
        // A read stopped while this batch was gathered is not this view's.
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        if read.apply(updates) {
            scope.set_games(read.games.games().to_vec());
        }
        read.bump();
        Ok(())
    });
}
