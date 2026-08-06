pub mod build_request;
pub mod game_data_download;
pub mod live_match_stats;
pub mod networking;
pub mod replay_upload;
pub mod replays;
pub mod scan;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc;
use std::sync::mpsc::TryRecvError;

use parking_lot::RwLock;
use rootcause::Report;
use rust_i18n::t;

use crate::data::wows_data::WorldOfWarshipsData;
#[cfg(feature = "mod_manager")]
use crate::mod_manager::ModTaskCompletion;
use crate::ui::plaintext_viewer::PlaintextFileViewer;
use crate::ui::replay_parser::Replay;
use crate::util::error::ToolkitError;

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
    /// Opened from the search results table. The caller has already put the
    /// replay in a sub-tab of the workspace that owns its directory, so this
    /// opens no tab of its own. It tracks no session stats either: a match
    /// found by searching history is history, not part of this session.
    SearchOpen,
}

// Re-export everything so `use crate::task::*` still works
pub use build_request::BuildRequest;
pub use game_data_download::GameDataFollowUp;
pub use game_data_download::PlanTicket;
pub use game_data_download::start_game_data_download_task;
pub use game_data_download::start_game_data_plan_task;
pub use game_data_download::start_game_data_update_check_task;
pub use game_data_download::start_game_data_validation_task;
pub use live_match_stats::FlushState;
pub use networking::NetworkJob;
pub use networking::NetworkResult;
pub use networking::load_personal_rating_data;
pub use networking::load_versioned_constants_from_disk_with_fallback;
#[cfg(target_os = "windows")]
pub use networking::start_download_update_task;
pub use networking::start_networking_thread;
pub use networking::start_twitch_task;
pub use replay_upload::ReplayCount;
pub use replay_upload::SendAllReplaysProgress;
pub use replay_upload::SendReplayCachePolicy;
pub use replays::BackgroundParserThread;
pub use replays::DataExportSettings;
pub use replays::ReplayBackgroundParserThreadMessage;
pub use replays::ReplayExportFormat;
pub use replays::SourceSelector;
pub use replays::build_game_constants;
pub use replays::load_nation_flag;
pub use replays::load_ribbon_icons;
pub use replays::load_ship_icons;
pub use replays::load_wows_data_for_build;
pub use replays::load_wows_files;
pub use replays::start_background_parsing_thread;
pub use replays::start_load_row_summaries;
pub use replays::start_populating_player_inspector;
pub use replays::start_read_directory;
pub use replays::start_reconcile_index;
#[allow(unused_imports)]
pub use replays::start_send_all_replays_to_shipbuilds;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DownloadProgress {
    pub downloaded: u64,
    pub total: u64,
}

/// Progress for the on-demand "Index all replays" reconciliation pass.
pub struct IndexProgress {
    pub done: u64,
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
    LoadingPersonalRatingData,
    DownloadingGameData {
        rx: mpsc::Receiver<DownloadProgress>,
        last_progress: Option<DownloadProgress>,
        /// What is waiting on this build: a replay to reopen or a directory to
        /// walk again. `None` for downloads nothing is waiting on.
        follow_up: Option<GameDataFollowUp>,
    },
    /// Resolving a selection of builds against the remote repository to report
    /// what each one's availability is and how much the selection would fetch.
    /// `ticket` identifies this run against the offer that started it.
    PlanningGameDataDownload {
        ticket: PlanTicket,
    },
    CheckingGameDataUpdates,
    ValidatingGameData {
        rx: mpsc::Receiver<DownloadProgress>,
        last_progress: Option<DownloadProgress>,
    },
    #[cfg(feature = "mod_manager")]
    ModTask(Box<crate::mod_manager::ModTaskInfo>),
    UpdateTimedMessage(ToastMessage),
    OpenFileViewer(PlaintextFileViewer),
    BatchVideoExport {
        progress: Arc<parking_lot::Mutex<BatchVideoExportProgress>>,
    },
    ReconcilingIndex {
        rx: mpsc::Receiver<IndexProgress>,
        last_progress: Option<IndexProgress>,
    },
    SendingReplaysToShipBuilds {
        rx: mpsc::Receiver<SendAllReplaysProgress>,
        last_progress: Option<SendAllReplaysProgress>,
    },
    LoadingRowSummaries {
        workspace: crate::db::index::rows::WorkspaceId,
    },
    /// Walking a picked directory and building a `Replay` per file it holds,
    /// for the workspace that directory was opened as. `rx` carries the
    /// replays the walk has read so far and how far it has got, so the listing
    /// fills as the walk runs rather than when it ends.
    IngestingDirectory {
        workspace: crate::db::index::rows::WorkspaceId,
        rx: mpsc::Receiver<replays::IngestUpdate>,
    },
    /// Reading the header of every replay under a picked directory, to count
    /// them and group them by build before any is read in full.
    ScanningDirectory {
        workspace: crate::db::index::rows::WorkspaceId,
        rx: mpsc::Receiver<replays::IngestUpdate>,
    },
}

/// Progress state for a batch video export, shared between the background thread and the UI.
///
/// A batch reads each replay off disk only when it reaches it, so the frame
/// counts it can report are the current replay's, not the whole batch's.
pub struct BatchVideoExportProgress {
    /// Frames rendered so far in the replay currently being rendered.
    pub current_frames: u64,
    /// Frames the replay currently being rendered is expected to produce.
    /// `None` until it has been read off disk and its duration is known.
    pub current_total_frames: Option<u64>,
    /// Index of the replay currently being rendered (0-based).
    pub current_index: usize,
    /// Total number of replays to render.
    pub total_replays: usize,
    /// Name of the replay currently being rendered.
    pub current_name: String,
}

impl BatchVideoExportProgress {
    /// The starting state for a batch of `total_replays` replays.
    pub fn for_batch(total_replays: usize) -> Self {
        Self {
            current_frames: 0,
            current_total_frames: None,
            current_index: 0,
            total_replays,
            current_name: String::new(),
        }
    }

    /// How far the whole batch has got, in `0.0..=1.0`. Every replay is one
    /// equal slice of the bar, and the one being rendered fills its own slice by
    /// frame count once its length is known.
    pub fn fraction(&self) -> f32 {
        if self.total_replays == 0 {
            return 0.0;
        }
        let within_current = match self.current_total_frames {
            Some(total) if total > 0 => (self.current_frames as f32 / total as f32).clamp(0.0, 1.0),
            _ => 0.0,
        };
        ((self.current_index as f32 + within_current) / self.total_replays as f32).clamp(0.0, 1.0)
    }
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
                    BackgroundTaskKind::DownloadingGameData { rx, last_progress, .. } => {
                        match rx.try_recv() {
                            Ok(progress) => *last_progress = Some(progress),
                            Err(TryRecvError::Empty) => {}
                            Err(TryRecvError::Disconnected) => {}
                        }
                        match last_progress {
                            Some(progress) if progress.total > 0 => {
                                ui.add(
                                    egui::ProgressBar::new(progress.downloaded as f32 / progress.total as f32)
                                        .text(t!("ui.messages.downloading_game_data")),
                                );
                            }
                            _ => {
                                ui.spinner();
                                ui.label(t!("ui.messages.downloading_game_data"));
                            }
                        }
                    }
                    BackgroundTaskKind::CheckingGameDataUpdates => {
                        ui.spinner();
                        ui.label(t!("ui.messages.checking_game_data_updates"));
                    }
                    BackgroundTaskKind::PlanningGameDataDownload { .. } => {
                        // The dialog that asked for this plan shows its own
                        // pending footer, so the task bar stays quiet.
                    }
                    BackgroundTaskKind::ValidatingGameData { rx, last_progress } => {
                        match rx.try_recv() {
                            Ok(progress) => *last_progress = Some(progress),
                            Err(TryRecvError::Empty) => {}
                            Err(TryRecvError::Disconnected) => {}
                        }
                        match last_progress {
                            Some(progress) if progress.total > 0 => {
                                ui.add(
                                    egui::ProgressBar::new(progress.downloaded as f32 / progress.total as f32)
                                        .text(t!("ui.messages.validating_game_data")),
                                );
                            }
                            _ => {
                                ui.spinner();
                                ui.label(t!("ui.messages.validating_game_data"));
                            }
                        }
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
                    BackgroundTaskKind::BatchVideoExport { progress } => {
                        let p = progress.lock();
                        ui.add(egui::ProgressBar::new(p.fraction()).text(t!(
                            "ui.task.batch_render_progress",
                            current = p.current_index + 1,
                            total = p.total_replays,
                            name = &p.current_name,
                        )));
                    }
                    BackgroundTaskKind::ReconcilingIndex { rx, last_progress } => {
                        match rx.try_recv() {
                            Ok(progress) => *last_progress = Some(progress),
                            Err(TryRecvError::Empty) => {}
                            Err(TryRecvError::Disconnected) => {}
                        }
                        match last_progress {
                            Some(progress) if progress.total > 0 => {
                                ui.add(egui::ProgressBar::new(progress.done as f32 / progress.total as f32).text(t!(
                                    "ui.messages.indexing_replays_progress",
                                    done = progress.done,
                                    total = progress.total
                                )));
                            }
                            _ => {
                                ui.spinner();
                                ui.label(t!("ui.messages.indexing_replays"));
                            }
                        }
                    }
                    BackgroundTaskKind::SendingReplaysToShipBuilds { .. } => {}
                    BackgroundTaskKind::LoadingRowSummaries { .. } => {
                        ui.spinner();
                        ui.label(t!("ui.messages.loading_row_summaries"));
                    }
                    // Both stages of a directory open read the same directory,
                    // and the listing itself reports which one is running.
                    BackgroundTaskKind::IngestingDirectory { .. } | BackgroundTaskKind::ScanningDirectory { .. } => {
                        ui.spinner();
                        ui.label(t!("ui.messages.reading_replay_directory"));
                    }
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
        replays: Option<HashMap<PathBuf, Arc<crate::ui::replay_parser::ListedReplay>>>,
        available_builds: Vec<u32>,
    },
    BuildDataLoaded {
        build: u32,
    },
    GameDataDownloaded {
        /// Each requested build paired with the build actually downloaded for
        /// it, which differs when a version fallback served it.
        downloaded: Vec<(u32, u32)>,
        /// Builds whose download failed. Their replays stay unread; the rest of
        /// the selection is unaffected.
        failures: Vec<u32>,
    },
    /// A selection of builds was resolved against the remote repository. Each
    /// requested build's availability is reported alongside the deduplicated
    /// count of CAS objects the whole selection would fetch.
    GameDataDownloadPlanned {
        /// The planner run this answers, so an offer only ever applies the plan
        /// it asked for.
        ticket: PlanTicket,
        plan: wows_data_mgr::download_repo::DownloadPlan,
    },
    GameDataUpdatesChecked {
        tip: String,
        updates: Vec<wows_data_mgr::download_repo::BuildUpdateStatus>,
    },
    GameDataValidated {
        tip: String,
        builds: Vec<wows_data_mgr::download_repo::BuildValidation>,
    },
    ReplayLoaded {
        replay: Arc<RwLock<Replay>>,
        source: ReplaySource,
    },
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    UpdateDownloaded(PathBuf),
    PopulatePlayerInspectorFromReplays,
    PersonalRatingDataLoaded(crate::util::personal_rating::ExpectedValuesData),
    /// On-demand "Index all replays" reconciliation pass finished.
    /// `indexed` counts replays newly parsed and written to the index this pass;
    /// `total` is the number of replay files scanned.
    ReconcileIndexComplete {
        indexed: usize,
        total: usize,
    },
    ReplaysSentToShipBuilds {
        attempted: ReplayCount,
        sent: ReplayCount,
        total: ReplayCount,
    },
    /// The replay-listing row summaries finished loading for `generation`, for
    /// the listing identified by `workspace`.
    RowSummariesLoaded {
        summaries: HashMap<PathBuf, crate::db::index::rows::RowSummary>,
        generation: u64,
        workspace: crate::db::index::rows::WorkspaceId,
    },
    /// A picked directory finished being walked. The replays themselves arrive
    /// as [`replays::IngestBatch`]es while the walk runs; this reports what the
    /// walk could not do, keeping the builds no game data is installed for
    /// apart from the files nothing can be done about and from the replays that
    /// are listed but did not index. `workspace` is carried so the result lands
    /// on the workspace that asked for it even if the inspector has since moved
    /// on, and is dropped if that workspace was closed while the walk was
    /// running.
    DirectoryIngested {
        workspace: crate::db::index::rows::WorkspaceId,
        source: crate::db::index::rows::SourceId,
        failures: crate::task::replays::IngestFailures,
    },
    /// A picked directory finished being scanned: every replay under it has had
    /// its header read, and nothing else. What the scan found is what the
    /// download offer is built from and what the read stage then consumes, so
    /// the caller retains it rather than walking the directory again.
    DirectoryScanned {
        workspace: crate::db::index::rows::WorkspaceId,
        /// Boxed: a scan of a large directory holds a path per replay, and this
        /// enum is moved through the task channel by value.
        scan: Box<crate::task::scan::DirectoryScan>,
    },
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
            Self::GameDataDownloaded { downloaded, failures } => f
                .debug_struct("GameDataDownloaded")
                .field("downloaded", downloaded)
                .field("failures", failures)
                .finish(),
            Self::GameDataDownloadPlanned { ticket, plan } => f
                .debug_struct("GameDataDownloadPlanned")
                .field("ticket", ticket)
                .field("unique_missing_objects", &plan.unique_missing_objects)
                .field("resolved", &plan.resolved.len())
                .finish(),
            Self::GameDataUpdatesChecked { tip, updates } => {
                f.debug_struct("GameDataUpdatesChecked").field("tip", tip).field("updates", updates).finish()
            }
            Self::GameDataValidated { tip, builds } => {
                f.debug_struct("GameDataValidated").field("tip", tip).field("builds", builds).finish()
            }
            Self::ReplayLoaded { replay: _, source } => {
                f.debug_struct("ReplayLoaded").field("replay", &"<...>").field("source", source).finish()
            }
            Self::UpdateDownloaded(arg0) => f.debug_tuple("UpdateDownloaded").field(arg0).finish(),
            Self::PopulatePlayerInspectorFromReplays => f.write_str("PopulatePlayerInspectorFromReplays"),
            Self::PersonalRatingDataLoaded(_) => f.write_str("PersonalRatingDataLoaded(_)"),
            Self::ReconcileIndexComplete { indexed, total } => {
                f.debug_struct("ReconcileIndexComplete").field("indexed", indexed).field("total", total).finish()
            }
            Self::ReplaysSentToShipBuilds { attempted, sent, total } => f
                .debug_struct("ReplaysSentToShipBuilds")
                .field("attempted", attempted)
                .field("sent", sent)
                .field("total", total)
                .finish(),
            Self::RowSummariesLoaded { summaries, generation, workspace } => f
                .debug_struct("RowSummariesLoaded")
                .field("summaries", &summaries.len())
                .field("generation", generation)
                .field("workspace", workspace)
                .finish(),
            Self::DirectoryIngested { workspace, source, failures } => f
                .debug_struct("DirectoryIngested")
                .field("workspace", workspace)
                .field("source", source)
                .field("failures", failures)
                .finish(),
            Self::DirectoryScanned { workspace, scan } => {
                f.debug_struct("DirectoryScanned").field("workspace", workspace).field("total", &scan.total).finish()
            }
            #[cfg(feature = "mod_manager")]
            Self::ModManager(mod_manager_completion) => {
                f.write_fmt(format_args!("ModManager({:?})", mod_manager_completion))
            }
            Self::NoReceiver => f.debug_struct("NoReceiver").finish(),
        }
    }
}

#[cfg(test)]
mod batch_progress_tests {
    use super::BatchVideoExportProgress;

    #[test]
    fn a_batch_that_has_started_nothing_reports_no_progress() {
        assert_eq!(BatchVideoExportProgress::for_batch(4).fraction(), 0.0);
    }

    #[test]
    fn an_empty_batch_does_not_divide_by_zero() {
        assert_eq!(BatchVideoExportProgress::for_batch(0).fraction(), 0.0);
    }

    #[test]
    fn each_replay_is_one_equal_slice_of_the_bar() {
        let mut progress = BatchVideoExportProgress::for_batch(4);
        progress.current_index = 2;
        assert_eq!(progress.fraction(), 0.5, "two of four replays are behind us");

        progress.current_total_frames = Some(100);
        progress.current_frames = 50;
        assert_eq!(progress.fraction(), 0.625, "half way through the third of four");
    }

    /// A replay is only read off disk when the batch reaches it, so its length
    /// is unknown for as long as that read takes. The bar must sit at that
    /// replay's own boundary meanwhile, neither stalling behind it nor guessing
    /// past it.
    #[test]
    fn a_replay_whose_length_is_not_known_yet_contributes_only_its_slices_start() {
        let mut progress = BatchVideoExportProgress::for_batch(2);
        progress.current_index = 1;
        progress.current_frames = 999;
        assert_eq!(progress.fraction(), 0.5);
    }

    /// The per-replay frame count is an estimate from the replay's duration, so
    /// a render can overrun it. That must not spill into the next replay's slice.
    #[test]
    fn overshooting_the_frame_estimate_stays_inside_the_replays_own_slice() {
        let mut progress = BatchVideoExportProgress::for_batch(2);
        progress.current_total_frames = Some(100);
        progress.current_frames = 400;
        assert_eq!(progress.fraction(), 0.5, "the first of two replays cannot fill more than half the bar");
    }
}
