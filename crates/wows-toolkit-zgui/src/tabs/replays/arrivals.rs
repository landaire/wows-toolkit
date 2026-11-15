//! What happens when the game writes a replay: it is exported where the reader asked, contributed
//! as the data-sharing setting asks, opened when "Autoload Latest Replay" is on, and announced.
//! Replays written while the app was closed are contributed by a sweep once game data is ready.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use futures::channel::oneshot;
use landaire_ui::Notice;
use landaire_ui::ToastKey;
use rust_i18n::t;
use wows_toolkit_config::ReplayExportFormat;
use wows_toolkit_config::ReplaySettings;
use wows_toolkit_replay_data::load::parse_replay;
use wows_toolkit_replay_data::replay_folder::ReplayFolder;
use wows_toolkit_services::upload::LedgerUse;
use wows_toolkit_services::upload::Outcome;
use wows_toolkit_viewmodel::upload::RawUploadDeadline;
use zgui::reactive::LocalStorage;
use zgui::reactive::StoredValue;
use zgui::reactive::Task;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::UpdateValue;
use zgui::reactive::prelude::WithValue;
use zgui::task::blocking;
use zgui::task::spawn_local;
use zgui::view::Timers;

use crate::long_job::long_job;
use crate::tabs::replays::panel::ReparseCause;
use crate::tabs::replays::panel::export_extension;
use crate::tabs::replays::panel::write_export;
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

/// How long a replay must go unchanged before it is exported. The game writes a replay in bursts
/// and each write is a file event.
const AUTO_EXPORT_QUIET: Duration = Duration::from_millis(500);

/// Where the battles that land are exported to, when the reader asked for that.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AutoExport {
    Off,
    To { directory: PathBuf, format: ReplayExportFormat },
}

impl AutoExport {
    /// Reads the setting, refusing a directory that is not one.
    pub fn from_settings(settings: &ReplaySettings) -> Self {
        if !settings.auto_export_data {
            return Self::Off;
        }
        let directory = PathBuf::from(&settings.auto_export_path);
        if !directory.is_dir() {
            return Self::Off;
        }
        Self::To { directory, format: settings.auto_export_format }
    }
}

/// The exports waiting for their replay to go quiet. A newer event for the same replay replaces,
/// and so cancels, the older one, so a burst of writes is read once.
#[derive(Clone, Copy)]
pub struct Arrivals {
    exports: StoredValue<HashMap<PathBuf, Task>, LocalStorage>,
    /// Battles held for their results, each read again when its grace lapses. Any newer read of
    /// the replay replaces the hold.
    held: StoredValue<HashMap<PathBuf, Task>, LocalStorage>,
    /// The replays being read for sharing, by these arrivals, the sweep or the bulk send.
    reads: SharingReads,
    /// Whether this session's sweep of the install's replays has started.
    swept: StoredValue<bool, LocalStorage>,
}

impl Arrivals {
    /// A replay asked for while `reads` held it is read again here once that read ends.
    pub fn new(workspace: ReplayWorkspace, reads: SharingReads) -> Self {
        let arrivals = Self {
            exports: StoredValue::new_local(HashMap::new()),
            held: StoredValue::new_local(HashMap::new()),
            reads,
            swept: StoredValue::new_local(false),
        };
        reads.on_again(move |path| arrivals.contribute(workspace, &path));
        arrivals
    }

    /// A watched directory gained `path`. `battle` is its "Ship - Map"; `followed` says whether it
    /// landed in the install's directory or in the folder on screen, which is what autoloading and
    /// the announcement follow.
    pub fn appeared(self, workspace: ReplayWorkspace, path: PathBuf, battle: &str, followed: bool) {
        // Written and contributed whether or not it is opened, as the egui app does from its
        // background parser.
        self.export(workspace, &path);
        self.contribute(workspace, &path);
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

    /// A listed replay was written again, which may be the results arriving for a battle the
    /// client left early: the export is tried again and an open tab reads it again.
    pub fn changed(self, workspace: ReplayWorkspace, path: PathBuf) {
        self.export(workspace, &path);
        // A battle held back for its results is sent once they are in the file.
        self.contribute(workspace, &path);
        workspace.reparse(&path, ReparseCause::FileRewritten);
    }

    /// Exports `path` once it has gone quiet, if the reader asked for every battle to be. A
    /// replay with no battle results is skipped: its results arrive later as a change.
    fn export(self, workspace: ReplayWorkspace, path: &Path) {
        let Some(replay) = workspace.services.settings.read_untracked(|settings| settings.replay.clone()) else {
            return;
        };
        let AutoExport::To { directory, format } = AutoExport::from_settings(&replay) else { return };
        let Some(game_data) = workspace.services.game_data.cache_untracked() else { return };
        let Some(timers) = workspace.under_tab(Timers::current) else { return };
        let debug = workspace.shell.debug_mode.get_untracked();
        let personal_rating = workspace.personal_rating.get_untracked();
        let toasts = workspace.shell.toasts();
        let named = path.to_path_buf();

        let task = workspace.under_tab(|| {
            spawn_local(async move {
                let (done, waited) = oneshot::channel();
                let _timer = timers.set_timeout(AUTO_EXPORT_QUIET, move || {
                    let _ = done.send(());
                });
                if waited.await.is_err() {
                    return;
                }

                let reading = named.clone();
                let written = long_job(move || {
                    let parsed = parse_replay(&reading, &game_data, personal_rating.as_deref())
                        .map_err(|err| ExportFailure::Parse(err.to_string()))?;
                    if parsed.raw_results_json.is_none() {
                        return Ok(None);
                    }
                    let out = directory.join(&parsed.export_stem).with_extension(export_extension(format));
                    let export = if debug { parsed.export } else { parsed.export.stripped() };
                    write_export(&export, &out, format).map_err(|err| ExportFailure::Write(err.to_string()))?;
                    Ok(Some(out))
                })
                .await;
                match written {
                    Ok(Some(out)) => {
                        tracing::info!(path = %named.display(), "auto-export: the battle was written");
                        let file = out.file_name().unwrap_or(out.as_os_str()).to_string_lossy().into_owned();
                        toasts.withdraw(&ToastKey::new(AUTO_EXPORT_FAILED));
                        toasts.show(
                            Notice::ok(t!("ui.replay.auto_exported", file = file).into_owned())
                                .keyed(ToastKey::new(AUTO_EXPORTED)),
                        );
                    }
                    Ok(None) => {
                        tracing::debug!(path = %named.display(), "auto-export: no battle results, so nothing is written");
                    }
                    Err(ExportFailure::Parse(reason)) => {
                        tracing::warn!(path = %named.display(), %reason, "auto-export: the replay did not parse");
                    }
                    // Kept up: the reader asked for a file per battle and is not getting one.
                    Err(ExportFailure::Write(reason)) => {
                        tracing::warn!(path = %named.display(), %reason, "auto-export failed");
                        toasts.show(
                            Notice::failed(t!("ui.replay.auto_export_failed", error = reason).into_owned())
                                .keyed(ToastKey::new(AUTO_EXPORT_FAILED))
                                .sticky(),
                        );
                    }
                }
            })
        });
        self.exports.update_value(|exports| {
            exports.retain(|_, task| !task.is_finished());
            if let Some(older) = exports.insert(path.to_path_buf(), task) {
                older.cancel();
            }
        });
    }
}

impl Arrivals {
    /// Contributes the replays the install's directory holds that were never contributed, once a
    /// session, as the egui app does when its background parser starts. Waits for the settings
    /// and game data; sharing that is off when they are ready skips this session's sweep.
    pub fn sweep(self, workspace: ReplayWorkspace) {
        if self.swept.get_value() {
            return;
        }
        let services = workspace.services;
        let Some(wows_dir) = services.settings.read_untracked(|settings| settings.wows_dir.clone()) else { return };
        if services.game_data.cache_untracked().is_none() {
            return;
        }
        self.swept.set_value(true);
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
        // Released first, so only a battle that is still waiting is held again.
        if let Some(held) = self.held.try_update_value(|held| held.remove(path)).flatten() {
            held.cancel();
        }
        self.start(workspace, path.to_path_buf());
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
}

/// Why an auto-export wrote nothing.
#[derive(Debug)]
enum ExportFailure {
    Parse(String),
    Write(String),
}
