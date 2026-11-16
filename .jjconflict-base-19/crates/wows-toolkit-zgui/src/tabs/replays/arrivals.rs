//! What happens when the game writes a replay: it is exported where the reader asked, contributed
//! as the data-sharing setting asks, opened when "Autoload Latest Replay" is on, and announced.

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
use wows_toolkit_services::upload;
use wows_toolkit_services::upload::Candidate;
use wows_toolkit_services::upload::LedgerUse;
use wows_toolkit_services::upload::Outcome;
use wows_toolkit_services::upload::Sender;
use wows_toolkit_viewmodel::settings::DataSharingMode;
use zgui::reactive::LocalStorage;
use zgui::reactive::StoredValue;
use zgui::reactive::Task;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::UpdateValue;
use zgui::task::blocking;
use zgui::task::spawn_local;
use zgui::view::Timers;

use crate::tabs::replays::browser::proxy_url;
use crate::tabs::replays::panel::ReparseCause;
use crate::tabs::replays::panel::export_extension;
use crate::tabs::replays::panel::write_export;
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
}

impl Arrivals {
    pub fn new() -> Self {
        Self { exports: StoredValue::new_local(HashMap::new()) }
    }

    /// A watched directory gained `path`. `battle` is its "Ship - Map"; `install` says whether it
    /// landed in the install's directory, which is what autoloading follows.
    pub fn appeared(self, workspace: ReplayWorkspace, path: PathBuf, battle: &str, install: bool) {
        // Written and contributed whether or not it is opened, as the egui app does from its
        // background parser.
        self.export(workspace, &path);
        contribute(workspace, &path);
        if !install {
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
        contribute(workspace, &path);
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
                let written = blocking(move || {
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

impl Default for Arrivals {
    fn default() -> Self {
        Self::new()
    }
}

/// Why an auto-export wrote nothing.
#[derive(Debug)]
enum ExportFailure {
    Parse(String),
    Write(String),
}

/// Contributes a battle that has just landed, as the data-sharing setting asks. The ledger is
/// shared with the egui app, so a battle is contributed once however it was read.
fn contribute(workspace: ReplayWorkspace, path: &Path) {
    let services = workspace.services;
    let Some(mode) = services.settings.read_untracked(|settings| settings.data_sharing) else { return };
    if mode == DataSharingMode::Off {
        return;
    }
    let Some(game_data) = services.game_data.cache_untracked() else { return };
    let Some(pool) = services.settings.pool() else { return };
    let proxy = proxy_url(services);
    let named = path.to_path_buf();

    workspace.under_tab(|| {
        spawn_local(async move {
            let reading = named.clone();
            let parsed = match blocking(move || parse_replay(&reading, &game_data, None)).await {
                Ok(parsed) => parsed,
                Err(err) => {
                    tracing::warn!(path = %named.display(), %err, "sharing: the replay did not parse");
                    return;
                }
            };
            // The anchor both apps keep, recorded on the first sight of this replay: the grace
            // window for its results runs from there.
            let (asked, seen) = (pool.clone(), named.clone());
            let first_seen = services.on_tokio(async move { upload::first_seen(&asked, &seen).await }).await;
            let first_seen = first_seen.unwrap_or_else(|_| jiff::Timestamp::now());
            let candidate = Candidate { path: named.clone(), shareable: parsed.shareable, first_seen };
            let outcome = services
                .on_tokio(async move {
                    let sender = Sender { pool: &pool, agent: crate::http::USER_AGENT, proxy: &proxy };
                    upload::send(candidate, mode, LedgerUse::Consult, sender).await
                })
                .await;
            match outcome {
                Ok(Outcome::Sent) => tracing::info!(path = %named.display(), "sharing: the battle was contributed"),
                Ok(Outcome::Failed(reason)) => {
                    tracing::warn!(path = %named.display(), %reason, "sharing: the battle was not contributed")
                }
                Ok(other) => tracing::debug!(path = %named.display(), ?other, "sharing: nothing sent"),
                Err(err) => tracing::warn!(path = %named.display(), %err, "sharing: the send did not complete"),
            }
        });
    });
}
