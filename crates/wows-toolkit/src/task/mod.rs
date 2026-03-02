pub mod networking;
pub mod replays;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc;
use std::sync::mpsc::TryRecvError;

use parking_lot::RwLock;
use rootcause::Report;
use tracing::error;

use tracing::instrument;

use crate::WowsToolkitApp;
use crate::error::ToolkitError;
#[cfg(feature = "mod_manager")]
use crate::mod_manager::ModTaskCompletion;
#[cfg(feature = "mod_manager")]
use crate::mod_manager::load_mods_db;
use crate::plaintext_viewer::PlaintextFileViewer;
use crate::twitch::TwitchUpdate;
use crate::ui::replay_parser::Replay;
use crate::update_background_task;
use crate::wows_data::WorldOfWarshipsData;

/// Describes where a replay load request originated from.
/// This determines what UI actions to take when the replay finishes loading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplaySource {
    /// Opened from the file listing (tab already managed by the listing handler).
    /// Tracks session stats but does NOT open a tab.
    FileListing,
    /// Drag-and-drop or manual "Open" button.
    /// Opens in focused tab but does NOT track session stats.
    ManualOpen,
    /// Auto-loaded from file watcher (new/modified replay detected).
    /// Opens in focused tab and tracks session stats.
    AutoLoad,
    /// Re-loading the focused replay after constants changed.
    /// Opens in focused tab and tracks session stats.
    Reload,
    /// Background batch loading for session stats only.
    /// No UI update, only tracks session stats.
    SessionStatsOnly,
}

// Re-export everything so `use crate::task::*` still works
pub use networking::NetworkJob;
pub use networking::NetworkResult;
pub use networking::load_constants;
pub use networking::load_personal_rating_data;
pub use networking::load_versioned_constants_from_disk_with_fallback;
#[cfg(target_os = "windows")]
pub use networking::start_download_update_task;
pub use networking::start_networking_thread;
pub use networking::start_twitch_task;
pub use replays::BackgroundParserThread;
pub use replays::DataExportSettings;
pub use replays::ReplayBackgroundParserThreadMessage;
pub use replays::ReplayExportFormat;
pub use replays::build_game_constants;
pub use replays::load_nation_flag;
pub use replays::load_ribbon_icons;
pub use replays::load_ship_icons;
pub use replays::load_wows_data_for_build;
pub use replays::load_wows_files;
pub use replays::start_background_parsing_thread;
pub use replays::start_populating_player_inspector;

pub struct DownloadProgress {
    pub downloaded: u64,
    pub total: u64,
}

#[derive(Clone)]
#[allow(dead_code)]
pub enum ToastLevel {
    Success,
    Info,
    Warning,
    Error,
}

#[derive(Clone)]
pub struct ToastMessage {
    pub message: String,
    pub level: ToastLevel,
}

#[allow(dead_code)]
impl ToastMessage {
    pub fn success(message: impl Into<String>) -> Self {
        Self { message: message.into(), level: ToastLevel::Success }
    }

    pub fn info(message: impl Into<String>) -> Self {
        Self { message: message.into(), level: ToastLevel::Info }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self { message: message.into(), level: ToastLevel::Error }
    }
}

pub struct BackgroundTask {
    pub receiver: Option<mpsc::Receiver<Result<BackgroundTaskCompletion, Report>>>,
    pub kind: BackgroundTaskKind,
}

pub enum BackgroundTaskKind {
    LoadingData,
    LoadingBuildData(u32),
    LoadingReplay,
    // Updates only occur on Windows
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    Updating {
        rx: mpsc::Receiver<DownloadProgress>,
        last_progress: Option<DownloadProgress>,
    },
    PopulatePlayerInspectorFromReplays,
    LoadingConstants,
    LoadingPersonalRatingData,
    #[cfg(feature = "mod_manager")]
    ModTask(Box<crate::mod_manager::ModTaskInfo>),
    UpdateTimedMessage(ToastMessage),
    OpenFileViewer(PlaintextFileViewer),
}

#[cfg(feature = "mod_manager")]
impl From<crate::mod_manager::ModTaskInfo> for BackgroundTaskKind {
    fn from(info: crate::mod_manager::ModTaskInfo) -> Self {
        Self::ModTask(Box::new(info))
    }
}

impl BackgroundTask {
    /// Check if the task has completed without rendering any UI.
    pub fn check_completion(&mut self) -> Option<Result<BackgroundTaskCompletion, Report>> {
        if self.receiver.is_none() {
            return Some(Ok(BackgroundTaskCompletion::NoReceiver));
        }

        match self.receiver.as_ref().unwrap().try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.receiver = None;
                Some(Ok(BackgroundTaskCompletion::NoReceiver))
            }
        }
    }

    /// TODO: has a bug currently where if multiple tasks are running at the same time, the message looks a bit wonky
    pub fn build_description(&mut self, ui: &mut egui::Ui) -> Option<Result<BackgroundTaskCompletion, Report>> {
        if self.receiver.is_none() {
            return Some(Ok(BackgroundTaskCompletion::NoReceiver));
        }

        match self.receiver.as_ref().unwrap().try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => {
                match &mut self.kind {
                    BackgroundTaskKind::LoadingData => {
                        ui.spinner();
                        ui.label("Loading game data...");
                    }
                    BackgroundTaskKind::LoadingBuildData(build) => {
                        ui.spinner();
                        ui.label(format!("Loading build {build}..."));
                    }
                    BackgroundTaskKind::LoadingReplay => {
                        ui.spinner();
                        ui.label("Loading replay...");
                    }
                    BackgroundTaskKind::Updating { rx, last_progress } => {
                        match rx.try_recv() {
                            Ok(progress) => {
                                *last_progress = Some(progress);
                            }
                            Err(TryRecvError::Empty) => {}
                            Err(TryRecvError::Disconnected) => {}
                        }

                        if let Some(progress) = last_progress {
                            ui.add(
                                egui::ProgressBar::new(progress.downloaded as f32 / progress.total as f32)
                                    .text("Downloading Update"),
                            );
                        }
                    }
                    BackgroundTaskKind::PopulatePlayerInspectorFromReplays => {
                        ui.spinner();
                        ui.label("Populating player inspector from historical replays...");
                    }
                    BackgroundTaskKind::LoadingConstants => {
                        ui.spinner();
                        ui.label("Loading data constants...");
                    }
                    #[cfg(feature = "mod_manager")]
                    BackgroundTaskKind::ModTask(mod_task) => match mod_task.as_mut() {
                        crate::mod_manager::ModTaskInfo::LoadingModDatabase => {
                            ui.spinner();
                            ui.label("Loading mod database...");
                        }
                        crate::mod_manager::ModTaskInfo::DownloadingMod { mod_info, rx, last_progress } => {
                            match rx.try_recv() {
                                Ok(progress) => {
                                    *last_progress = Some(progress);
                                }
                                Err(TryRecvError::Empty) => {}
                                Err(TryRecvError::Disconnected) => {}
                            }

                            if let Some(progress) = last_progress {
                                ui.add(
                                    egui::ProgressBar::new(progress.downloaded as f32 / progress.total as f32)
                                        .text(format!("Downloading {}", mod_info.meta.name())),
                                );
                            }
                        }
                        crate::mod_manager::ModTaskInfo::InstallingMod { mod_info, rx, last_progress } => {
                            match rx.try_recv() {
                                Ok(progress) => {
                                    *last_progress = Some(progress);
                                }
                                Err(TryRecvError::Empty) => {}
                                Err(TryRecvError::Disconnected) => {}
                            }

                            if let Some(progress) = last_progress {
                                ui.add(
                                    egui::ProgressBar::new(progress.downloaded as f32 / progress.total as f32)
                                        .text(format!("Installing {}", mod_info.meta.name())),
                                );
                            }
                        }
                        crate::mod_manager::ModTaskInfo::UninstallingMod { mod_info, rx, last_progress } => {
                            match rx.try_recv() {
                                Ok(progress) => *last_progress = Some(progress),
                                Err(TryRecvError::Empty) => {}
                                Err(TryRecvError::Disconnected) => {}
                            }

                            if let Some(progress) = last_progress {
                                ui.add(
                                    egui::ProgressBar::new(progress.downloaded as f32 / progress.total as f32)
                                        .text(format!("Uninstalling {}", mod_info.meta.name())),
                                );
                            }
                        }
                    },
                    BackgroundTaskKind::LoadingPersonalRatingData
                    | BackgroundTaskKind::UpdateTimedMessage(_)
                    | BackgroundTaskKind::OpenFileViewer(_) => {
                        // do nothing
                    }
                }
                None
            }
            Err(TryRecvError::Disconnected) => Some(Err(ToolkitError::BackgroundTaskCompleted.into())),
        }
    }
}

pub enum BackgroundTaskCompletion {
    DataLoaded {
        new_dir: PathBuf,
        wows_data: Box<WorldOfWarshipsData>,
        replays: Option<HashMap<PathBuf, Arc<RwLock<Replay>>>>,
        available_builds: Vec<u32>,
    },
    BuildDataLoaded {
        build: u32,
    },
    ReplayLoaded {
        replay: Arc<RwLock<Replay>>,
        source: ReplaySource,
    },
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    UpdateDownloaded(PathBuf),
    PopulatePlayerInspectorFromReplays,
    ConstantsLoaded(serde_json::Value),
    PersonalRatingDataLoaded(crate::personal_rating::ExpectedValuesData),
    #[cfg(feature = "mod_manager")]
    ModManager(Box<crate::mod_manager::ModTaskCompletion>),
    NoReceiver,
}

#[cfg(feature = "mod_manager")]
impl From<ModTaskCompletion> for BackgroundTaskCompletion {
    fn from(completion: ModTaskCompletion) -> Self {
        Self::ModManager(Box::new(completion))
    }
}

impl std::fmt::Debug for BackgroundTaskCompletion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DataLoaded { new_dir, wows_data: _, replays: _, available_builds } => f
                .debug_struct("DataLoaded")
                .field("new_dir", new_dir)
                .field("wows_data", &"<...>")
                .field("replays", &"<...>")
                .field("available_builds", available_builds)
                .finish(),
            Self::BuildDataLoaded { build } => f.debug_struct("BuildDataLoaded").field("build", build).finish(),
            Self::ReplayLoaded { replay: _, source } => {
                f.debug_struct("ReplayLoaded").field("replay", &"<...>").field("source", source).finish()
            }
            Self::UpdateDownloaded(arg0) => f.debug_tuple("UpdateDownloaded").field(arg0).finish(),
            Self::PopulatePlayerInspectorFromReplays => f.write_str("PopulatePlayerInspectorFromReplays"),
            Self::ConstantsLoaded(_) => f.write_str("ConstantsLoaded(_)"),
            Self::PersonalRatingDataLoaded(_) => f.write_str("PersonalRatingDataLoaded(_)"),
            #[cfg(feature = "mod_manager")]
            Self::ModManager(mod_manager_completion) => {
                f.write_fmt(format_args!("ModManager({:?})", mod_manager_completion))
            }
            Self::NoReceiver => f.debug_struct("NoReceiver").finish(),
        }
    }
}

#[instrument(skip_all)]
pub fn begin_startup_tasks(toolkit: &mut WowsToolkitApp, token_rx: tokio::sync::mpsc::Receiver<TwitchUpdate>) {
    // Start the networking thread
    let (network_job_tx, network_result_rx) = start_networking_thread();
    toolkit.tab_state.network_job_tx = Some(network_job_tx);
    toolkit.network_result_rx = Some(network_result_rx);

    start_twitch_task(
        &toolkit.runtime,
        Arc::clone(&toolkit.tab_state.twitch_state),
        toolkit.tab_state.settings.twitch_monitored_channel.clone(),
        toolkit.tab_state.settings.twitch_token.clone(),
        token_rx,
    );

    #[cfg(feature = "mod_manager")]
    update_background_task!(toolkit.tab_state.background_tasks, Some(load_mods_db()));

    let mut constants_path = PathBuf::from("constants.json");
    if let Some(storage_dir) = eframe::storage_dir(crate::APP_NAME) {
        constants_path = storage_dir.join(constants_path)
    }

    if constants_path.exists() {
        if let Ok(constants_data) = std::fs::read(&constants_path) {
            update_background_task!(toolkit.tab_state.background_tasks, Some(load_constants(constants_data)));
        } else {
            error!("failed to read constants file");
        }
    }

    // Load PR expected values from disk if available
    let pr_path = crate::personal_rating::get_expected_values_path();
    if pr_path.exists() {
        if let Ok(pr_data) = std::fs::read(&pr_path) {
            update_background_task!(toolkit.tab_state.background_tasks, Some(load_personal_rating_data(pr_data)));
        } else {
            error!("failed to read PR expected values file");
        }
    }
}
