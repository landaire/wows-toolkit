//! What happens when the game writes a replay: it is read once for its tab's index, the export
//! the reader asked for, the data-sharing contribution and the session, opened when "Autoload
//! Latest Replay" is on, and announced. Replays written while the app was closed are contributed
//! by a sweep once game data is ready.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use futures::channel::oneshot;
use landaire_ui::Notice;
use landaire_ui::ToastKey;
use landaire_ui::Toasts;
use rust_i18n::t;
use wows_toolkit_config::ReplayExportFormat;
use wows_toolkit_config::ReplaySettings;
use wows_toolkit_replay_data::directory_stats::GameRead;
use wows_toolkit_replay_data::error::ReplayLoadError;
use wows_toolkit_replay_data::load::GameDataCache;
use wows_toolkit_replay_data::load::ParsedReplay;
use wows_toolkit_replay_data::load::parse_replay;
use wows_toolkit_replay_data::replay_folder::ReplayFolder;
use wows_toolkit_replay_data::replay_index::IndexOneError;
use wows_toolkit_services::upload::LedgerUse;
use wows_toolkit_services::upload::Outcome;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingData;
use wows_toolkit_viewmodel::replay_export::Match as ExportedMatch;
use wows_toolkit_viewmodel::upload::RawUploadDeadline;
use wows_toolkit_viewmodel::upload::Shareable;
use zgui::reactive::LocalStorage;
use zgui::reactive::StoredValue;
use zgui::reactive::Task;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::UpdateValue;
use zgui::reactive::prelude::WithValue;
use zgui::task::blocking;
use zgui::task::spawn_local;
use zgui::view::Timers;

use crate::long_job::long_job;
use crate::tabs::replays::browser::FolderId;
use crate::tabs::replays::browser::LandingIndex;
use crate::tabs::replays::browser::landed_indexed;
use crate::tabs::replays::browser::landing_index;
use crate::tabs::replays::panel::ExportError;
use crate::tabs::replays::panel::ReparseCause;
use crate::tabs::replays::panel::export_extension;
use crate::tabs::replays::panel::write_export;
use crate::tabs::replays::session_stats::SessionLanding;
use crate::tabs::replays::session_stats::SessionReads;
use crate::tabs::replays::sharing::ContributeError;
use crate::tabs::replays::sharing::Contributor;
use crate::tabs::replays::sharing::SharingReads;
use crate::tabs::replays::workspace::OpenTarget;
use crate::tabs::replays::workspace::ReplayWorkspace;

/// Keys the notice that an export failed, so a directory of battles that all fail says so once.
const AUTO_EXPORT_FAILED: &str = "replay-auto-export-failed";
/// Keys the notice that a battle was exported, so a burst of battles says only the latest.
const AUTO_EXPORTED: &str = "replay-auto-exported";
/// Keys the notice that a battle landed, for the same reason.
const NEW_BATTLE: &str = "replay-new-battle";

/// How long a replay must go unchanged before it is read. The game writes a replay in bursts and
/// each write is a file event.
const LANDING_QUIET: Duration = Duration::from_millis(500);

/// How many times a battle that landed is read before it is left for a later write of the file,
/// and the wait between reads: the game may still be writing it.
const LANDING_READ_ATTEMPTS: u32 = 3;
const LANDING_READ_RETRY: Duration = Duration::from_secs(5);

/// Where every battle the game writes is exported to, when the reader asked for that.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutoExport {
    directory: PathBuf,
    format: ReplayExportFormat,
}

impl AutoExport {
    /// Reads the setting. `None` while it is off or its directory is not one.
    pub fn from_settings(settings: &ReplaySettings) -> Option<Self> {
        if !settings.auto_export_data {
            return None;
        }
        let directory = PathBuf::from(&settings.auto_export_path);
        if !directory.is_dir() {
            return None;
        }

        Some(Self { directory, format: settings.auto_export_format })
    }

    /// Writes `export`, the battle named `stem`, returning the file written. Outside debug mode
    /// what an ordinary export leaves out is stripped first.
    pub fn write(&self, export: ExportedMatch, stem: &str, debug: bool) -> Result<PathBuf, ExportError> {
        let out = self.directory.join(stem).with_extension(export_extension(self.format));
        let export = if debug { export } else { export.stripped() };
        write_export(&export, &out, self.format)?;

        Ok(out)
    }

    /// Writes the battle `parsed` holds, when it has its results, returning the file written. One
    /// with no results yet is left: they arrive later as a rewrite of the file.
    pub fn write_read(&self, parsed: &ParsedReplay, debug: bool) -> Result<Option<PathBuf>, ExportError> {
        if parsed.raw_results_json.is_none() {
            return Ok(None);
        }

        self.write(parsed.export.clone(), &parsed.export_stem, debug).map(Some)
    }
}

/// Says that a battle was exported to `out`, replacing any notice that an export failed.
pub fn say_exported(toasts: Toasts, out: &Path) {
    let file = out.file_name().unwrap_or(out.as_os_str()).to_string_lossy().into_owned();
    toasts.withdraw(&ToastKey::new(AUTO_EXPORT_FAILED));
    toasts
        .show(Notice::ok(t!("ui.replay.auto_exported", file = file).into_owned()).keyed(ToastKey::new(AUTO_EXPORTED)));
}

/// Says that an export failed. Kept up: the reader asked for a file per battle and is not getting
/// one.
pub fn say_export_failed(toasts: Toasts, reason: String) {
    toasts.show(
        Notice::failed(t!("ui.replay.auto_export_failed", error = reason).into_owned())
            .keyed(ToastKey::new(AUTO_EXPORT_FAILED))
            .sticky(),
    );
}

/// The reads of landed battles waiting for their replay to go quiet, or for another try. A newer
/// event for the same replay replaces, and so cancels, a landing still waiting, so a burst of
/// writes is read once. One that arrives during a read is read after it.
#[derive(Clone, Copy)]
pub struct Arrivals {
    landings: StoredValue<HashMap<PathBuf, Landing>, LocalStorage>,
    /// Battles held for their results, each read again when its grace lapses. Any newer read of
    /// the replay replaces the hold.
    held: StoredValue<HashMap<PathBuf, Task>, LocalStorage>,
    /// The replays being read for sharing, by these arrivals, the sweep or the bulk send.
    reads: SharingReads,
    /// The game directory whose replays this session's sweep has read. Another directory set in
    /// the settings is swept in turn.
    swept: StoredValue<Option<String>, LocalStorage>,
    session: SessionReads,
}

/// A landed battle's read: the task waiting for the replay to go quiet and reading it.
struct Landing {
    task: Task,
    /// Whether the read is under way, which is let finish rather than stopped.
    reading: bool,
    /// The landing that arrived during the read, to be read once it ends.
    again: Option<LandingAsk>,
}

/// What a landing reads its battle for.
#[derive(Clone, Copy)]
struct LandingAsk {
    /// The directory tab whose index the battle is written to.
    folder: FolderId,
    session: Option<SessionLanding>,
}

impl Arrivals {
    /// A replay asked for while `reads` held it is read again here once that read ends. The
    /// battles the game writes to the install's directory are handed to `session`.
    pub fn new(workspace: ReplayWorkspace, reads: SharingReads, session: SessionReads) -> Self {
        let arrivals = Self {
            landings: StoredValue::new_local(HashMap::new()),
            held: StoredValue::new_local(HashMap::new()),
            reads,
            swept: StoredValue::new_local(None),
            session,
        };
        reads.on_again(move |path| arrivals.contribute(workspace, &path));
        arrivals
    }

    /// A watched directory gained `path`, in the directory tab `folder`. `battle` is its
    /// "Ship - Map"; `followed` says whether it landed in the install's directory or in the folder
    /// on screen, which is what autoloading and the announcement follow.
    pub fn appeared(self, workspace: ReplayWorkspace, path: PathBuf, battle: &str, followed: bool, folder: FolderId) {
        // Indexed, written and contributed whether or not it is opened, as the egui app does from
        // its background parser.
        self.landed(workspace, &path, folder);
        if !followed {
            return;
        }
        let autoload =
            workspace.services.settings.read_untracked(|settings| settings.auto_load_latest_replay).unwrap_or(false);
        let opened = autoload && workspace.open(path, OpenTarget::ShowingTab).is_some();
        let said = if opened {
            t!("ui.replay.opened_new_battle", battle = battle)
        } else {
            t!("ui.replay.new_battle_listed", battle = battle)
        };
        workspace.shell.toasts().show(Notice::info(said.into_owned()).keyed(ToastKey::new(NEW_BATTLE)));
    }

    /// A listed replay of the directory tab `folder` was written again, which may be the results
    /// arriving for a battle the client left early: it is read again for the index, the export
    /// and sharing, and an open tab reads it again.
    pub fn changed(self, workspace: ReplayWorkspace, path: PathBuf, folder: FolderId) {
        self.landed(workspace, &path, folder);
        workspace.reparse(&path, ReparseCause::FileRewritten);
    }

    /// Reads `path` once it has gone quiet, once for everything a landed battle feeds: its tab's
    /// index, the export the reader asked for, the data-sharing contribution and the session. A
    /// read the file failed is tried again after a wait, as the game may still be writing it.
    ///
    /// A read under way is not stopped, as its worker would write on behind a newer read. The
    /// landing is read once it ends instead, and only that newer read's results are handed on.
    fn landed(self, workspace: ReplayWorkspace, path: &Path, folder: FolderId) {
        // A battle held back for its results is sent once they are in the file.
        self.release(path);
        let ask = LandingAsk { folder, session: self.session.landed(path.to_path_buf()) };
        let queued = self.landings.try_update_value(|landings| match landings.get_mut(path) {
            Some(landing) if landing.reading => {
                landing.again = Some(ask);
                true
            }
            Some(_) | None => false,
        });
        if queued == Some(false) {
            self.read_when_quiet(workspace, path.to_path_buf(), ask);
        }
    }

    /// Reads `path` for `ask` once it has gone quiet, in place of a landing of it still waiting.
    fn read_when_quiet(self, workspace: ReplayWorkspace, path: PathBuf, ask: LandingAsk) {
        let Some(timers) = workspace.under_tab(Timers::current) else { return };
        let named = path.clone();

        let task = workspace.under_tab(|| {
            spawn_local(async move {
                for attempt in 1..=LANDING_READ_ATTEMPTS {
                    let wait = if attempt == 1 { LANDING_QUIET } else { LANDING_READ_RETRY };
                    let (done, waited) = oneshot::channel();
                    let _timer = timers.set_timeout(wait, move || {
                        let _ = done.send(());
                    });
                    if waited.await.is_err() {
                        return;
                    }

                    self.mark_reading(&named);
                    let read = self.read_landed(workspace, &named, ask).await;
                    if let Some(again) = self.end_reading(&named) {
                        // The file landed again during the read, so the newer read's results win.
                        self.read_when_quiet(workspace, named, again);
                        return;
                    }
                    let err = match read {
                        Ok(Some(read)) => {
                            self.hand_on(workspace, &named, ask, read);
                            return;
                        }
                        Ok(None) => return,
                        Err(err) => err,
                    };
                    if err.is_unreadable_file() && attempt < LANDING_READ_ATTEMPTS {
                        tracing::debug!(path = %named.display(), %err, attempt, "the battle that landed is read again");
                        continue;
                    }
                    tracing::warn!(path = %named.display(), %err, "the battle that landed could not be read");
                    if let Some(landing) = ask.session {
                        self.session.landed_read(&named, landing, GameRead::of_failure(&err));
                    }
                    return;
                }
            })
        });
        self.landings.update_value(|landings| {
            landings.retain(|_, landing| !landing.task.is_finished());
            // Only a landing still waiting can be here: one reading takes newer landings in turn.
            if let Some(older) = landings.insert(path, Landing { task, reading: false, again: None }) {
                older.task.cancel();
            }
        });
    }

    fn mark_reading(self, path: &Path) {
        let _ = self.landings.try_update_value(|landings| {
            if let Some(landing) = landings.get_mut(path) {
                landing.reading = true;
            }
        });
    }

    /// Ends the read of `path`. Answers the landing that arrived during it, if any, whose read
    /// then takes the place of this one.
    fn end_reading(self, path: &Path) -> Option<LandingAsk> {
        self.landings
            .try_update_value(|landings| {
                let landing = landings.get_mut(path)?;
                landing.reading = false;
                let again = landing.again.take()?;
                landings.remove(path);
                Some(again)
            })
            .flatten()
    }

    /// Reads the battle at `path` for what wants it now. `Ok(None)` when nothing does, and `Err`
    /// when the replay did not read, for the caller to try again or give up on.
    async fn read_landed(
        self,
        workspace: ReplayWorkspace,
        path: &Path,
        ask: LandingAsk,
    ) -> Result<Option<LandingRead>, ReplayLoadError> {
        let services = workspace.services;
        let Some(game_data) = services.game_data.cache_untracked() else { return Ok(None) };
        let index = landing_index(workspace.browser(), ask.folder, path);
        let export = services.settings.read_untracked(|settings| AutoExport::from_settings(&settings.replay)).flatten();
        let sharing = Contributor::new(services).is_some();
        if index.is_none() && export.is_none() && !sharing && ask.session.is_none() {
            return Ok(None);
        }
        let debug = workspace.shell.debug_mode.get_untracked();
        let personal_rating = workspace.personal_rating.get_untracked();
        let reading = path.to_path_buf();

        let read = long_job(move || {
            let wanted = Wanted { index: index.as_ref(), export: export.as_ref(), debug };
            read_landing(&reading, &game_data, personal_rating.as_deref(), wanted)
        })
        .await?;

        Ok(Some(read))
    }

    /// Hands what a landing's read found on to the tab, the reader, sharing and the session.
    fn hand_on(self, workspace: ReplayWorkspace, path: &Path, ask: LandingAsk, read: LandingRead) {
        match read.indexed {
            Some(Ok(rated)) => landed_indexed(workspace.browser(), ask.folder, rated),
            Some(Err(err)) => tracing::warn!(path = %path.display(), %err, "the battle that landed was not indexed"),
            None => {}
        }
        match read.exported {
            Some(Ok(Some(out))) => {
                tracing::info!(path = %path.display(), "auto-export: the battle was written");
                say_exported(workspace.shell.toasts(), &out);
            }
            Some(Ok(None)) => {
                tracing::debug!(path = %path.display(), "auto-export: no battle results, so nothing is written");
            }
            Some(Err(err)) => {
                tracing::warn!(path = %path.display(), %err, "auto-export failed");
                say_export_failed(workspace.shell.toasts(), err.to_string());
            }
            None => {}
        }
        // Asked as the read ends, as sharing may have been turned off during it.
        if let Some(contributor) = Contributor::new(workspace.services) {
            self.contribute_read(workspace, contributor, path.to_path_buf(), read.shareable);
        }
        if let Some(landing) = ask.session {
            self.session.landed_read(path, landing, read.game);
        }
    }
}

impl Arrivals {
    /// Contributes the replays the install's directory holds that were never contributed, once a
    /// session for each game directory set, as the egui app does when its background parser
    /// starts. Waits for the settings and game data; sharing that is off when they are ready skips
    /// the directory's sweep.
    pub fn sweep(self, workspace: ReplayWorkspace) {
        let services = workspace.services;
        let Some(wows_dir) = services.settings.read_untracked(|settings| settings.wows_dir.clone()) else { return };
        if self.swept.with_value(|swept| swept.as_ref() == Some(&wows_dir)) {
            return;
        }
        if services.game_data.cache_untracked().is_none() {
            return;
        }
        self.swept.set_value(Some(wows_dir.clone()));
        if wows_dir.is_empty() {
            return;
        }
        let Some(contributor) = Contributor::new(services) else { return };

        workspace.under_tab(|| {
            spawn_local(async move {
                let folder = blocking(move || ReplayFolder::from_app_install(Path::new(&wows_dir))).await;
                let unsent = contributor.unsent_in(folder).await;
                tracing::info!(count = unsent.len(), "sharing: contributing the replays not yet sent");
                let mut ineligible = Vec::new();
                for path in unsent {
                    if contributor.mode().is_none() {
                        tracing::info!("sharing: turned off, so the sweep stops");
                        break;
                    }
                    // Another read has it, or a landing holds it for its results.
                    if self.held.with_value(|held| held.contains_key(&path)) || !self.reads.begin(&path) {
                        continue;
                    }
                    match contributor.contribute(path.clone(), LedgerUse::Consult).await {
                        Ok(Outcome::Ineligible) => ineligible.push(path.clone()),
                        outcome => self.settle(workspace, &contributor, &path, outcome).await,
                    }
                    self.reads.finish(&path);
                }
                contributor.remember_ineligible(ineligible).await;
            });
        });
    }

    /// Contributes `path` as the data-sharing setting asks. The ledger is shared with the egui app,
    /// so a battle is contributed once however it was read.
    fn contribute(self, workspace: ReplayWorkspace, path: &Path) {
        self.release(path);
        self.start(workspace, path.to_path_buf());
    }

    /// Drops the hold on `path`, so only a battle that is still waiting is held again.
    fn release(self, path: &Path) {
        if let Some(held) = self.held.try_update_value(|held| held.remove(path)).flatten() {
            held.cancel();
        }
    }

    fn start(self, workspace: ReplayWorkspace, path: PathBuf) {
        let Some(contributor) = Contributor::new(workspace.services) else { return };
        if !self.reads.begin(&path) {
            return;
        }
        workspace.under_tab(|| {
            spawn_local(async move {
                let outcome = contributor.contribute(path.clone(), LedgerUse::Consult).await;
                self.settle(workspace, &contributor, &path, outcome).await;
                self.reads.finish(&path);
            });
        });
    }

    /// Contributes what a landing's read of `path` found. A task of its own rather than part of
    /// the landing's, which a newer write of the file cancels: the read must end to release
    /// `path`.
    fn contribute_read(
        self,
        workspace: ReplayWorkspace,
        contributor: Contributor,
        path: PathBuf,
        shareable: Shareable,
    ) {
        if !self.reads.begin(&path) {
            return;
        }
        workspace.under_tab(|| {
            spawn_local(async move {
                let outcome = contributor.contribute_read(path.clone(), shareable, LedgerUse::Consult).await;
                self.settle(workspace, &contributor, &path, outcome).await;
                self.reads.finish(&path);
            });
        });
    }

    async fn settle(
        self,
        workspace: ReplayWorkspace,
        contributor: &Contributor,
        path: &Path,
        outcome: Result<Outcome, ContributeError>,
    ) {
        match outcome {
            Ok(Outcome::Sent) => tracing::info!(path = %path.display(), "sharing: the battle was contributed"),
            Ok(Outcome::Waiting { deadline }) => self.hold(workspace, path.to_path_buf(), deadline),
            Ok(Outcome::Ineligible) => contributor.remember_ineligible(vec![path.to_path_buf()]).await,
            Ok(Outcome::Failed(reason)) => {
                tracing::warn!(path = %path.display(), %reason, "sharing: the battle was not contributed")
            }
            Ok(other @ (Outcome::Skipped | Outcome::AlreadySent)) => {
                tracing::debug!(path = %path.display(), ?other, "sharing: nothing sent")
            }
            Err(err) => tracing::warn!(path = %path.display(), %err, "sharing: the battle was not contributed"),
        }
    }

    /// Reads `path` again at `deadline`, when its raw upload goes whether or not the results have
    /// arrived.
    pub(super) fn hold(self, workspace: ReplayWorkspace, path: PathBuf, deadline: RawUploadDeadline) {
        let Some(timers) = workspace.under_tab(Timers::current) else { return };
        // A deadline already past is due now.
        let wait = Duration::try_from(deadline.0.duration_since(jiff::Timestamp::now())).unwrap_or(Duration::ZERO);
        let due = path.clone();
        let task = workspace.under_tab(|| {
            spawn_local(async move {
                let (done, waited) = oneshot::channel();
                let _timer = timers.set_timeout(wait, move || {
                    let _ = done.send(());
                });
                if waited.await.is_err() {
                    return;
                }
                tracing::debug!(path = %due.display(), "sharing: the grace for the battle's results lapsed");
                self.start(workspace, due);
            })
        });
        self.held.update_value(|held| {
            held.retain(|_, task| !task.is_finished());
            if let Some(older) = held.insert(path, task) {
                older.cancel();
            }
        });
    }

    #[cfg(test)]
    pub fn holds(self, path: &Path) -> bool {
        self.held.with_value(|held| held.get(path).is_some_and(|task| !task.is_finished()))
    }

    #[cfg(test)]
    pub fn swept(self) -> Option<String> {
        self.swept.with_value(Clone::clone)
    }
}

/// What a landing's read writes besides reading the battle.
struct Wanted<'a> {
    /// The tab's index, when it has one to write to.
    index: Option<&'a LandingIndex>,
    /// Where the battle is exported, when the reader asked for every battle to be.
    export: Option<&'a AutoExport>,
    debug: bool,
}

/// What a landing's one read of a battle wrote, and what it read for the UI thread to hand on.
struct LandingRead {
    /// Whether the rows were written and rated. `None` when the tab has no index to write to.
    indexed: Option<Result<bool, IndexOneError>>,
    /// The file the battle was exported to, `Ok(None)` for one with no results yet. `None` when
    /// nothing is exported.
    exported: Option<Result<Option<PathBuf>, ExportError>>,
    shareable: Shareable,
    game: GameRead,
}

/// Reads the battle at `path` once and writes what `wanted` asks for from it. Blocking.
fn read_landing(
    path: &Path,
    game_data: &GameDataCache,
    personal_rating: Option<&PersonalRatingData>,
    wanted: Wanted<'_>,
) -> Result<LandingRead, ReplayLoadError> {
    // Before the read, so a battle that will not read still roots its tab's source.
    let source = wanted.index.map(|index| (index, index.source()));
    let ParsedReplay { indexable, export, shareable, session_stat, export_stem, raw_results_json, .. } =
        parse_replay(path, game_data, personal_rating)?;

    let game = GameRead::of_battle(indexable.results_pending, session_stat);
    let exported = wanted.export.map(|target| match raw_results_json {
        Some(_) => target.write(export, &export_stem, wanted.debug).map(Some),
        // Its results arrive later as a rewrite of the file.
        None => Ok(None),
    });
    let indexed = source.map(|(index, source)| {
        let source = source.map_err(IndexOneError::from)?;
        index.write(source, path, indexable)
    });

    Ok(LandingRead { indexed, exported, shareable, game })
}
