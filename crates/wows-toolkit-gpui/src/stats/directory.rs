//! Stats over every replay in one directory, rather than over the recorded
//! session.
//!
//! The replays are read here and folded through
//! `wows_toolkit_viewmodel::stats::replay_set`, shared with the egui app.
//! [`StatsView`] renders them in its directory scope; nothing here touches the
//! session rows or the replay index.

use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::ops::ControlFlow;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;

use futures::StreamExt as _;
use futures::channel::mpsc::UnboundedSender;
use gpui_kit::App;
use gpui_kit::Context;
use gpui_kit::Entity;
use gpui_kit::EventEmitter;
use gpui_kit::InteractiveElement as _;
use gpui_kit::IntoElement;
use gpui_kit::ParentElement as _;
use gpui_kit::StatefulInteractiveElement as _;
use gpui_kit::Styled as _;
use gpui_kit::Task;
use gpui_kit::Window;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::Button;
use gpui_kit::component::h_flex;
use gpui_kit::component::progress::Progress;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::v_flex;
use gpui_kit::div;
use gpui_kit::prelude::FluentBuilder as _;
use rust_i18n::t;
use wows_toolkit_viewmodel::stats::PerGameStat;
use wows_toolkit_viewmodel::stats::replay_set::ReplaySetGames;
use wows_toolkit_viewmodel::stats::replay_set::ReplaySetProgress;
use wows_toolkit_viewmodel::stats::replay_set::ReplaySetSkips;
use wowsunpack::data::Version;

use crate::replay_inspector::load::GameDataCache;
use crate::replay_inspector::load::ReplayLoadError;
use crate::replay_inspector::view::ReplayInspectorView;

use super::view::StatsScope;
use super::view::StatsView;

/// The listing asks for a stats tab over the directory it is showing.
pub(crate) struct DirectoryStatsRequested(pub PathBuf);
impl EventEmitter<DirectoryStatsRequested> for ReplayInspectorView {}

/// Where the read of a directory has got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadState {
    /// Listing the directory; no replay found yet.
    Walking,
    /// Reading each replay's header to group them by build.
    Scanning(ReplaySetProgress),
    Reading(ReplaySetProgress),
    Finished,
}

/// The games of a stats view over one directory, and the read filling them.
pub struct DirectoryScope {
    root: PathBuf,
    game_data: GameDataCache,
    games: ReplaySetGames,
    read: ReadState,
    skips: ReplaySetSkips,
    /// Holds the receiving end of the reader's channel: dropping it, with the
    /// view or on a reload, is what stops the reader.
    read_task: Option<Task<()>>,
}

impl DirectoryScope {
    fn new(root: PathBuf, game_data: GameDataCache) -> Self {
        Self {
            root,
            game_data,
            games: ReplaySetGames::new(),
            read: ReadState::Walking,
            skips: ReplaySetSkips::default(),
            read_task: None,
        }
    }

    pub fn games(&self) -> &[PerGameStat] {
        self.games.games()
    }
}

/// What the reader sends the view it is filling.
enum DirectoryStatsUpdate {
    /// One more header read while finding and grouping the replays.
    Scanning(ReplaySetProgress),
    /// One more replay visited by the read itself.
    Read {
        game: Option<Box<PerGameStat>>,
        progress: ReplaySetProgress,
        /// Every replay skipped so far, not only this one.
        skips: ReplaySetSkips,
    },
}

/// What reading one replay produced.
enum GameRead {
    Game(PerGameStat),
    NoResults,
    MissingBuild,
    Unreadable,
}

/// The replays of one build, and the version their headers name.
struct BuildGroup {
    version: String,
    paths: Vec<PathBuf>,
}

impl StatsView {
    /// A stats view over every replay listed in `root`, read against
    /// `game_data`. Starts reading at once.
    pub fn new_for_directory(
        root: PathBuf,
        game_data: GameDataCache,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let scope = DirectoryScope::new(root, game_data);
        let mut view = Self::with_scope(StatsScope::Directory(Box::new(scope)), window, cx);
        view.start_directory_read(cx);
        view
    }

    /// Reads the directory again from the start, picking up replays added or
    /// removed since. Does nothing while a read is running.
    pub fn reload_directory(&mut self, cx: &mut Context<Self>) {
        let Some(scope) = self.directory_scope() else { return };
        if scope.read != ReadState::Finished {
            return;
        }

        self.start_directory_read(cx);
    }

    fn start_directory_read(&mut self, cx: &mut Context<Self>) {
        let Some(scope) = self.directory_scope_mut() else { return };
        scope.games = ReplaySetGames::new();
        scope.read = ReadState::Walking;
        scope.skips = ReplaySetSkips::default();
        scope.read_task = None;

        let (tx, mut rx) = futures::channel::mpsc::unbounded();
        let root = scope.root.clone();
        let game_data = scope.game_data.clone();
        let spawned = std::thread::Builder::new()
            .name("directory-stats".to_owned())
            .spawn(move || read_directory(&root, &game_data, &tx));
        if let Err(err) = spawned {
            tracing::warn!("directory stats: no thread to read on: {err}");
            scope.read = ReadState::Finished;
            self.directory_games_changed(cx);
            return;
        }

        scope.read_task = Some(cx.spawn(async move |this, cx| {
            while let Some(first) = rx.next().await {
                let mut updates = vec![first];
                while let Ok(update) = rx.try_recv() {
                    updates.push(update);
                }
                if this.update(cx, |this, cx| this.apply_directory_updates(updates, cx)).is_err() {
                    return;
                }
                // Each fold re-sorts the set and refilters every panel, so a
                // fast read is folded in batches.
                cx.background_executor().timer(Duration::from_millis(250)).await;
            }
            let _ = this.update(cx, |this, cx| {
                if let Some(scope) = this.directory_scope_mut() {
                    scope.read = ReadState::Finished;
                }
                cx.notify();
            });
        }));
        self.directory_games_changed(cx);
    }

    fn apply_directory_updates(&mut self, updates: Vec<DirectoryStatsUpdate>, cx: &mut Context<Self>) {
        let Some(scope) = self.directory_scope_mut() else { return };

        let mut games = Vec::new();
        for update in updates {
            match update {
                DirectoryStatsUpdate::Scanning(progress) => scope.read = ReadState::Scanning(progress),
                DirectoryStatsUpdate::Read { game, progress, skips } => {
                    scope.read = ReadState::Reading(progress);
                    scope.skips = skips;
                    games.extend(game.map(|game| *game));
                }
            }
        }
        if games.is_empty() {
            cx.notify();
            return;
        }

        scope.games.extend(games);
        self.directory_games_changed(cx);
    }
}

/// The line above a directory view's filter bar: the directory, how far the
/// read has got, and what it skipped.
pub(super) fn directory_header(scope: &DirectoryScope, view: Entity<StatsView>, cx: &App) -> impl IntoElement {
    let progress = match scope.read {
        ReadState::Scanning(progress) | ReadState::Reading(progress) => Some(progress),
        ReadState::Walking | ReadState::Finished => None,
    };
    let status = match scope.read {
        ReadState::Walking => Some(t!("ui.stats.directory_walking").into_owned()),
        ReadState::Scanning(progress) => {
            Some(t!("ui.stats.directory_scanning", done = progress.done, total = progress.total).into_owned())
        }
        ReadState::Reading(progress) => {
            Some(t!("ui.stats.directory_reading", done = progress.done, total = progress.total).into_owned())
        }
        ReadState::Finished => None,
    };
    let skips = scope.skips;
    let skipped = (skips.total() > 0).then(|| {
        let detail = t!(
            "ui.stats.directory_skipped_detail",
            unreadable = skips.unreadable,
            missing_build = skips.missing_build,
            no_results = skips.no_results
        )
        .into_owned();
        div()
            .id("directory-stats-skipped")
            .text_xs()
            .text_color(crate::theme::text_dim())
            .child(t!("ui.stats.directory_skipped", count = skips.total()).into_owned())
            .tooltip(move |window, cx| Tooltip::new(detail.clone()).build(window, cx))
    });

    let row = crate::ui::toolbar(cx)
        .child(
            h_flex()
                .gap_1()
                .items_center()
                .min_w_0()
                .child(crate::icons::icon(crate::icons::Glyph::FolderOpen))
                .child(div().text_sm().truncate().child(scope.root.display().to_string())),
        )
        .child(crate::ui::rule_v(cx))
        .when_some(status.clone(), |row, status| {
            row.child(h_flex().gap_2().items_center().text_xs().child(Spinner::new().xsmall()).child(status))
        })
        .when(scope.read == ReadState::Finished, |row| {
            row.child(
                Button::new("directory-stats-reload")
                    .icon(crate::icons::Glyph::RotateCw)
                    .label(t!("ui.stats.directory_reload").into_owned())
                    .compact()
                    .tooltip(t!("ui.stats.directory_reload_tooltip").into_owned())
                    .on_click(move |_event, _window, cx: &mut App| {
                        view.update(cx, |this, cx| this.reload_directory(cx));
                    }),
            )
        })
        .when_some(skipped, |row, skipped| row.child(crate::ui::rule_v(cx)).child(skipped));

    v_flex().flex_none().child(row).when_some(progress, |column, progress| {
        column.child(
            Progress::new("directory-stats-progress")
                .accessibility_label(status.unwrap_or_default())
                .color(cx.theme().primary)
                .value(progress.fraction() * 100.),
        )
    })
}

/// Reads every replay listed in `root` into a game, sending each result to
/// `tx`. Returns early once `tx` has no receiver.
///
/// Replays are read one build at a time, in ascending build order, so a
/// directory spanning many builds loads their game data one after another
/// rather than all at once.
fn read_directory(root: &Path, game_data: &GameDataCache, tx: &UnboundedSender<DirectoryStatsUpdate>) {
    let paths = replays_beneath(root);
    let total = paths.len();
    let workers = worker_count();

    let mut versions: Vec<Option<Version>> = vec![None; total];
    let mut scanned = 0;
    let scan = for_each_path(&paths, workers, read_header_version, |index, version| {
        scanned += 1;
        // A header read that panicked leaves the slot empty, which counts
        // the file unreadable.
        versions[index] = version.flatten();
        let progress = ReplaySetProgress { done: scanned, total };
        to_flow(tx.unbounded_send(DirectoryStatsUpdate::Scanning(progress)))
    });
    if scan.is_break() {
        return;
    }

    let mut groups: BTreeMap<u32, BuildGroup> = BTreeMap::new();
    let mut skips = ReplaySetSkips::default();
    for (path, version) in paths.into_iter().zip(versions) {
        let Some((build, version)) = version.and_then(|version| Some((version.build_number()?, version))) else {
            skips.unreadable += 1;
            continue;
        };
        groups
            .entry(build)
            .or_insert_with(|| BuildGroup {
                version: format!("{}.{}.{}", version.major, version.minor, version.patch),
                paths: Vec::new(),
            })
            .paths
            .push(path);
    }

    let mut progress = ReplaySetProgress { done: skips.unreadable, total };
    if tx.unbounded_send(DirectoryStatsUpdate::Read { game: None, progress, skips }).is_err() {
        return;
    }

    for (build, group) in groups {
        // Nothing is downloaded here: the directory listing offers that, and
        // a stats view only reports what it could not read.
        if !game_data.can_read_build(build, Some(&group.version)) {
            skips.missing_build += group.paths.len();
            progress.done += group.paths.len();
            if tx.unbounded_send(DirectoryStatsUpdate::Read { game: None, progress, skips }).is_err() {
                return;
            }
            continue;
        }

        let flow = for_each_path(
            &group.paths,
            workers,
            |path| read_game(path, game_data),
            |_index, read| {
                progress.done += 1;
                let game = match read {
                    Some(GameRead::Game(game)) => Some(Box::new(game)),
                    Some(GameRead::NoResults) => {
                        skips.no_results += 1;
                        None
                    }
                    Some(GameRead::MissingBuild) => {
                        skips.missing_build += 1;
                        None
                    }
                    Some(GameRead::Unreadable) | None => {
                        skips.unreadable += 1;
                        None
                    }
                };
                to_flow(tx.unbounded_send(DirectoryStatsUpdate::Read { game, progress, skips }))
            },
        );
        if flow.is_break() {
            return;
        }
    }
}

/// Every replay at or below `root`, matching the egui app's directory stats:
/// symlinks are not followed, and the live recording is left out because it
/// has no results yet.
fn replays_beneath(root: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(walkdir::DirEntry::into_path)
        .filter(|path| {
            path.extension().is_some_and(|extension| extension == "wowsreplay")
                && path.file_name().is_some_and(|name| name != "temp.wowsreplay")
        })
        .collect();
    paths.sort();
    paths
}

/// Half the cores, at most four: a full parse holds a battle's packet stream
/// and report.
fn worker_count() -> NonZeroUsize {
    let cores = std::thread::available_parallelism().map_or(1, NonZeroUsize::get);
    NonZeroUsize::new((cores / 2).clamp(1, 4)).unwrap_or(NonZeroUsize::MIN)
}

fn to_flow<E>(sent: Result<(), E>) -> ControlFlow<()> {
    match sent {
        Ok(()) => ControlFlow::Continue(()),
        Err(_) => ControlFlow::Break(()),
    }
}

/// Runs `work` over `paths` on `workers` threads, handing each result to
/// `on_result` on the calling thread as it lands, with the path's index. A
/// result is `None` when `work` panicked on that path. `on_result` breaks to
/// abandon the rest.
fn for_each_path<T: Send>(
    paths: &[PathBuf],
    workers: NonZeroUsize,
    work: impl Fn(&Path) -> T + Sync,
    mut on_result: impl FnMut(usize, Option<T>) -> ControlFlow<()>,
) -> ControlFlow<()> {
    let next = AtomicUsize::new(0);
    let abort = AtomicBool::new(false);

    std::thread::scope(|scope| {
        let (tx, rx) = std::sync::mpsc::channel::<(usize, Option<T>)>();

        for _ in 0..workers.get().min(paths.len()) {
            let tx = tx.clone();
            let (next, abort, work) = (&next, &abort, &work);
            scope.spawn(move || {
                while !abort.load(Ordering::Relaxed) {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(path) = paths.get(index) else { break };

                    // One malformed replay must cost only itself.
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(path)));
                    if result.is_err() {
                        tracing::warn!("directory stats: reading {} panicked", path.display());
                    }
                    if tx.send((index, result.ok())).is_err() {
                        break;
                    }
                }
            });
        }
        // The coordinator holding a sender would keep its own recv loop alive
        // after every worker exits.
        drop(tx);

        while let Ok((index, result)) = rx.recv() {
            if on_result(index, result).is_break() {
                abort.store(true, Ordering::Relaxed);
                return ControlFlow::Break(());
            }
        }

        ControlFlow::Continue(())
    })
}

/// The client version a replay's header names. Only the plaintext header is
/// read.
fn read_header_version(path: &Path) -> Option<Version> {
    let blob = wows_replays::ReplayFile::read_meta_blob(path).ok()?;
    let meta = wows_replays::ReplayMetaRef::from_slice(&blob).ok()?;
    Version::try_from_client_exe(&meta.clientVersionFromExe)
}

/// Parses one replay and reads the recording player's game off it.
fn read_game(path: &Path, game_data: &GameDataCache) -> GameRead {
    let parsed = match crate::replay_inspector::load::parse_replay(path, game_data, None) {
        Ok(parsed) => parsed,
        Err(ReplayLoadError::UnsupportedVersion { .. }) => return GameRead::MissingBuild,
        Err(err) => {
            tracing::warn!("directory stats: skipping {}: {err}", path.display());
            return GameRead::Unreadable;
        }
    };
    // A battle whose results never arrived is one the session does not
    // record either.
    if parsed.indexable.results_pending {
        return GameRead::NoResults;
    }

    match parsed.session_stat {
        Some(game) => GameRead::Game(game),
        None => GameRead::NoResults,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain(rx: &mut futures::channel::mpsc::UnboundedReceiver<DirectoryStatsUpdate>) -> Vec<DirectoryStatsUpdate> {
        let mut updates = Vec::new();
        while let Ok(update) = rx.try_recv() {
            updates.push(update);
        }
        updates
    }

    /// A file that is not a replay costs only itself and is counted as
    /// unreadable; the live recording is not part of the set at all.
    #[test]
    fn a_broken_replay_is_counted_unreadable_and_the_live_recording_is_left_out() {
        let dir = tempfile::tempdir().expect("a temporary directory can be made");
        std::fs::write(dir.path().join("broken.wowsreplay"), b"not a replay").expect("the file can be written");
        std::fs::write(dir.path().join("temp.wowsreplay"), b"in progress").expect("the file can be written");
        std::fs::write(dir.path().join("notes.txt"), b"unrelated").expect("the file can be written");

        let (tx, mut rx) = futures::channel::mpsc::unbounded();
        read_directory(dir.path(), &GameDataCache::without_install(), &tx);

        let updates = drain(&mut rx);
        let Some(DirectoryStatsUpdate::Read { game, progress, skips }) = updates.last() else {
            panic!("the read ends on a read update");
        };
        assert!(game.is_none());
        assert_eq!(*progress, ReplaySetProgress { done: 1, total: 1 }, "only the broken replay is in the set");
        assert_eq!(*skips, ReplaySetSkips { unreadable: 1, ..ReplaySetSkips::default() });
    }

    /// Replays in subdirectories are part of the set; a live recording there
    /// is still left out.
    #[test]
    fn replays_in_subdirectories_are_read() {
        let dir = tempfile::tempdir().expect("a temporary directory can be made");
        let nested = dir.path().join("2026").join("archive");
        std::fs::create_dir_all(&nested).expect("the directory can be made");
        std::fs::write(dir.path().join("top.wowsreplay"), b"not a replay").expect("the file can be written");
        std::fs::write(nested.join("nested.wowsreplay"), b"not a replay").expect("the file can be written");
        std::fs::write(nested.join("temp.wowsreplay"), b"in progress").expect("the file can be written");

        let (tx, mut rx) = futures::channel::mpsc::unbounded();
        read_directory(dir.path(), &GameDataCache::without_install(), &tx);

        let updates = drain(&mut rx);
        let Some(DirectoryStatsUpdate::Read { progress, skips, .. }) = updates.last() else {
            panic!("the read ends on a read update");
        };
        assert_eq!(*progress, ReplaySetProgress { done: 2, total: 2 });
        assert_eq!(*skips, ReplaySetSkips { unreadable: 2, ..ReplaySetSkips::default() });
    }
}
