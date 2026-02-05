use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::Sender;
use std::sync::mpsc::{
    self,
};
use std::time::Duration;

use egui::mutex::Mutex;
use notify::EventKind;
use notify::RecommendedWatcher;
use notify::RecursiveMode;
use notify::Watcher;
use notify::event::ModifyKind;
use notify::event::RenameMode;
use parking_lot::RwLock;
use serde::Deserialize;
use serde::Serialize;
use tracing::debug;
use wows_replays::ReplayFile;
use wowsunpack::data::idx::FileNode;

use crate::personal_rating::PersonalRatingData;
use crate::plaintext_viewer::PlaintextFileViewer;
use crate::session_stats::SessionStats;
use crate::settings::Settings;
use crate::settings::default_bool;
use crate::task::BackgroundParserThread;
use crate::task::BackgroundTask;
use crate::task::BackgroundTaskKind;
use crate::task::DataExportSettings;
use crate::task::ReplayBackgroundParserThreadMessage;
use crate::twitch::TwitchState;
use crate::ui::file_unpacker::UnpackerProgress;
use crate::ui::mod_manager::ModInfo;
use crate::ui::mod_manager::ModManagerInfo;
use crate::ui::replay_parser::Replay;
use crate::ui::replay_parser::SharedReplayParserTabState;
use crate::ui::replay_parser::SortOrder;
use crate::update_background_task;
use crate::wows_data::ReplayDependencies;
use crate::wows_data::ReplayLoader;
use crate::wows_data::WorldOfWarshipsData;

/// Available statistics for charting
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChartableStat {
    #[default]
    Damage,
    SpottingDamage,
    Frags,
    RawXp,
    BaseXp,
    WinRate,
    PersonalRating,
}

impl ChartableStat {
    pub fn name(&self) -> &'static str {
        match self {
            ChartableStat::Damage => "Damage",
            ChartableStat::SpottingDamage => "Spotting Damage",
            ChartableStat::Frags => "Frags",
            ChartableStat::RawXp => "Raw XP",
            ChartableStat::BaseXp => "Base XP",
            ChartableStat::WinRate => "Win Rate",
            ChartableStat::PersonalRating => "Personal Rating",
        }
    }

    pub fn all() -> &'static [ChartableStat] {
        &[
            ChartableStat::BaseXp,
            ChartableStat::Damage,
            ChartableStat::Frags,
            ChartableStat::PersonalRating,
            ChartableStat::RawXp,
            ChartableStat::SpottingDamage,
            ChartableStat::WinRate,
        ]
    }
}

/// Chart display mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChartMode {
    /// Line chart showing stat over each game played
    #[default]
    Line,
    /// Bar chart showing cumulative stat comparison between ships
    Bar,
}

/// Configuration for the session stats chart
#[derive(Default)]
pub struct SessionStatsChartConfig {
    /// Selected stat to display
    pub selected_stat: ChartableStat,
    /// Chart display mode (line or bar)
    pub mode: ChartMode,
    /// Selected ships to show (empty = all ships)
    pub selected_ships: Vec<String>,
    pub selected_ships_manually_changed: bool,
    /// Whether to show rolling average instead of per-game values (line chart only)
    pub rolling_average: bool,
    /// Whether to show value labels on data points
    pub show_labels: bool,
    /// Whether a screenshot has been requested (waiting for the event)
    pub screenshot_requested: bool,
    /// The plot rectangle from the last frame (used to crop the screenshot)
    pub plot_rect: Option<egui::Rect>,
}

/// File system events for replay monitoring
#[derive(Debug)]
pub enum NotifyFileEvent {
    Added(PathBuf),
    Modified(PathBuf),
    Removed(PathBuf),
    PreferencesChanged,
    TempArenaInfoCreated(PathBuf),
}

pub type PathFileNodePair = (Arc<PathBuf>, FileNode);

/// Main application state container
#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct TabState {
    #[serde(skip)]
    pub world_of_warships_data: Option<Arc<RwLock<WorldOfWarshipsData>>>,

    pub filter: String,

    #[serde(skip)]
    pub used_filter: Option<String>,
    #[serde(skip)]
    pub filtered_file_list: Option<Arc<Vec<PathFileNodePair>>>,

    #[serde(skip)]
    pub items_to_extract: Mutex<Vec<FileNode>>,

    pub settings: Settings,

    #[serde(skip)]
    pub translations: Option<gettext::Catalog>,

    pub output_dir: String,

    #[serde(skip)]
    pub unpacker_progress: Option<mpsc::Receiver<UnpackerProgress>>,

    #[serde(skip)]
    pub last_progress: Option<UnpackerProgress>,

    #[serde(skip)]
    pub replay_parser_tab: SharedReplayParserTabState,

    #[serde(skip)]
    pub file_viewer: Mutex<Vec<PlaintextFileViewer>>,

    #[serde(skip)]
    pub file_watcher: Option<RecommendedWatcher>,

    #[serde(skip)]
    pub file_receiver: Option<mpsc::Receiver<NotifyFileEvent>>,

    #[serde(skip)]
    pub replay_files: Option<HashMap<PathBuf, Arc<RwLock<Replay>>>>,

    #[serde(skip)]
    pub background_tasks: Vec<BackgroundTask>,

    #[serde(skip)]
    pub timed_message: RwLock<Option<crate::app::TimedMessage>>,

    #[serde(skip)]
    pub can_change_wows_dir: bool,

    #[serde(skip)]
    pub current_replay: Option<Arc<RwLock<Replay>>>,

    #[serde(default = "default_bool::<true>")]
    pub auto_load_latest_replay: bool,

    #[serde(skip)]
    pub twitch_update_sender: Option<tokio::sync::mpsc::Sender<crate::twitch::TwitchUpdate>>,

    #[serde(skip)]
    pub twitch_state: Arc<RwLock<TwitchState>>,

    #[serde(skip)]
    pub markdown_cache: egui_commonmark::CommonMarkCache,

    #[serde(default)]
    pub replay_sort: Arc<parking_lot::Mutex<SortOrder>>,

    #[serde(skip)]
    pub game_constants: Arc<RwLock<serde_json::Value>>,

    #[serde(default)]
    pub mod_manager_info: ModManagerInfo,

    #[serde(skip)]
    pub mod_action_sender: Sender<ModInfo>,

    #[serde(skip)]
    /// Used temporarily to store the mod action receiver until the mod manager thread is started
    pub mod_action_receiver: Option<Receiver<ModInfo>>,

    #[serde(skip)]
    pub background_task_receiver: Receiver<BackgroundTask>,
    #[serde(skip)]
    pub background_task_sender: Sender<BackgroundTask>,
    #[serde(skip)]
    pub background_parser_tx: Option<Sender<ReplayBackgroundParserThreadMessage>>,
    #[serde(skip)]
    pub parser_lock: Arc<parking_lot::Mutex<()>>,

    #[serde(skip)]
    pub show_session_stats: bool,
    #[serde(skip)]
    pub show_session_stats_chart: bool,
    #[serde(skip)]
    pub session_stats: SessionStats,
    #[serde(skip)]
    pub session_stats_chart_config: SessionStatsChartConfig,
    #[serde(skip)]
    pub personal_rating_data: Arc<RwLock<PersonalRatingData>>,

    /// Replays selected for resetting session stats. When Some, they will be
    /// processed and added to session stats, clearing the old ones first.
    /// Uses Weak references to avoid retaining stale replays if they're removed from the listing.
    #[serde(skip)]
    pub replays_for_session_reset: Option<Vec<std::sync::Weak<RwLock<Replay>>>>,
}

impl Default for TabState {
    fn default() -> Self {
        let default_constants = serde_json::from_str(include_str!("../embedded_resources/constants.json"))
            .expect("failed to parse constants JSON");
        let (mod_action_sender, mod_action_receiver) = mpsc::channel();
        let (background_task_sender, background_task_receiver) = mpsc::channel();
        Self {
            world_of_warships_data: None,
            filter: Default::default(),
            items_to_extract: Default::default(),
            settings: Default::default(),
            translations: Default::default(),
            output_dir: Default::default(),
            unpacker_progress: Default::default(),
            last_progress: Default::default(),
            replay_parser_tab: Default::default(),
            file_viewer: Default::default(),
            file_watcher: None,
            replay_files: None,
            file_receiver: None,
            background_tasks: Vec::new(),
            can_change_wows_dir: true,
            timed_message: RwLock::new(None),
            current_replay: None,
            used_filter: None,
            filtered_file_list: None,
            auto_load_latest_replay: true,
            twitch_update_sender: Default::default(),
            twitch_state: Default::default(),
            markdown_cache: Default::default(),
            replay_sort: Default::default(),
            game_constants: Arc::new(parking_lot::RwLock::new(default_constants)),
            mod_manager_info: Default::default(),
            mod_action_sender,
            mod_action_receiver: Some(mod_action_receiver),
            background_task_receiver,
            background_task_sender,
            background_parser_tx: None,
            parser_lock: Arc::new(parking_lot::Mutex::new(())),
            show_session_stats: false,
            show_session_stats_chart: false,
            session_stats: Default::default(),
            session_stats_chart_config: Default::default(),
            personal_rating_data: Arc::new(RwLock::new(PersonalRatingData::new())),
            replays_for_session_reset: None,
        }
    }
}

impl TabState {
    /// Returns the shared dependencies needed for loading replays, if wows_data is available.
    pub fn replay_dependencies(&self) -> Option<ReplayDependencies> {
        let wows_data = self.world_of_warships_data.as_ref()?;
        Some(ReplayDependencies {
            game_constants: Arc::clone(&self.game_constants),
            wows_data: Arc::clone(wows_data),
            twitch_state: Arc::clone(&self.twitch_state),
            replay_sort: Arc::clone(&self.replay_sort),
            background_task_sender: self.background_task_sender.clone(),
            is_debug_mode: self.settings.debug_mode,
        })
    }

    pub(crate) fn send_replay_consent_changed(&self) {
        let _ = self.background_parser_tx.as_ref().map(|tx| {
            tx.send(ReplayBackgroundParserThreadMessage::ShouldSendReplaysToServer(self.settings.send_replay_data))
        });
    }

    pub(crate) fn try_update_replays(&mut self) {
        // Sometimes we parse the replay too early. Let's try to parse it a couple times
        let parser_lock = self.parser_lock.try_lock();
        if parser_lock.is_none() {
            // don't make the UI hang
            return;
        }

        if let Some(file) = self.file_receiver.as_ref() {
            while let Ok(file_event) = file.try_recv() {
                match file_event {
                    NotifyFileEvent::Added(new_file) => {
                        if let Some(wows_data) = self.world_of_warships_data.as_ref() {
                            let wows_data = wows_data.read();
                            if let Some(game_metadata) = wows_data.game_metadata.as_ref() {
                                for _ in 0..3 {
                                    if let Ok(replay_file) = ReplayFile::from_file(&new_file) {
                                        let replay = Replay::new(replay_file, game_metadata.clone());
                                        let replay = Arc::new(RwLock::new(replay));

                                        if let Some(replay_files) = &mut self.replay_files {
                                            replay_files.insert(new_file.clone(), Arc::clone(&replay));
                                        }

                                        if let Some(deps) = self.replay_dependencies() {
                                            update_background_task!(
                                                self.background_tasks,
                                                deps.load_replay(replay, self.auto_load_latest_replay)
                                            );
                                        }

                                        break;
                                    } else {
                                        // oops our framerate
                                        std::thread::sleep(Duration::from_secs(1));
                                    }
                                }
                            }
                        }
                    }
                    NotifyFileEvent::Modified(modified_file) => {
                        // Invalidate cached data when file is modified
                        if let Some(replay_files) = &self.replay_files
                            && let Some(replay) = replay_files.get(&modified_file)
                        {
                            let mut replay_inner = replay.write();
                            replay_inner.battle_report = None;
                            replay_inner.ui_report = None;
                            drop(replay_inner);

                            if let Some(deps) = self.replay_dependencies() {
                                update_background_task!(
                                    self.background_tasks,
                                    deps.load_replay(Arc::clone(replay), self.auto_load_latest_replay)
                                );
                            }
                        }
                    }
                    NotifyFileEvent::Removed(old_file) => {
                        if let Some(replay_files) = &mut self.replay_files {
                            replay_files.remove(&old_file);
                        }
                    }
                    NotifyFileEvent::PreferencesChanged => {
                        // debug!("Preferences file changed -- reloading game data");
                        // self.background_task = Some(self.load_game_data(self.settings.wows_dir.clone().into()));
                    }
                    NotifyFileEvent::TempArenaInfoCreated(path) => {
                        // Parse the metadata
                        let meta_data = std::fs::read(path);

                        if meta_data.is_err() {
                            return;
                        }

                        if let Ok(replay_file) =
                            ReplayFile::from_decrypted_parts(meta_data.unwrap(), Vec::with_capacity(0))
                        {
                            self.settings.player_tracker.write().update_from_live_arena_info(&replay_file.meta);
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn prevent_changing_wows_dir(&mut self) {
        self.can_change_wows_dir = false;
    }

    pub(crate) fn allow_changing_wows_dir(&mut self) {
        self.can_change_wows_dir = true;
    }

    /// Process replays selected for session stats reset.
    /// Clears the current session stats and populates with the selected replays.
    /// If any replays haven't been parsed yet, they will be queued for parsing.
    pub(crate) fn process_session_stats_reset(&mut self) {
        let Some(weak_replays) = self.replays_for_session_reset.take() else {
            return;
        };

        // Clear current session stats
        self.session_stats.clear();

        // Upgrade weak references and add to session stats
        for weak_replay in weak_replays {
            if let Some(replay) = weak_replay.upgrade() {
                // Check if the replay needs parsing (no ui_report means not parsed)
                let needs_parsing = replay.read().ui_report.is_none();

                if needs_parsing {
                    // Queue the replay for parsing (skip UI update since this is batch loading)
                    if let Some(deps) = self.replay_dependencies() {
                        update_background_task!(
                            self.background_tasks,
                            ReplayLoader::new(deps, replay.clone()).skip_ui_update().load()
                        );
                    }
                }

                // Add the replay to session stats (it will be updated when parsing completes)
                self.session_stats.add_replay(replay);
            }
        }

        // Show session stats window automatically
        self.show_session_stats = true;
    }

    pub(crate) fn update_wows_dir(&mut self, wows_dir: &Path, replay_dir: &Path) {
        let watcher = if let Some(watcher) = self.file_watcher.as_mut() {
            let old_replays_dir =
                self.settings.replays_dir.as_ref().expect("watcher was created but replay dir was not assigned?");
            let _ = watcher.unwatch(old_replays_dir);
            watcher
        } else {
            debug!("creating filesystem watcher");
            let (tx, rx) = mpsc::channel();
            let (background_tx, background_rx) = mpsc::channel();

            self.background_parser_tx = Some(background_tx.clone());

            if let Some(wows_data) = self.world_of_warships_data.clone() {
                let background_thread_data = BackgroundParserThread {
                    rx: background_rx,
                    sent_replays: Arc::clone(&self.settings.sent_replays),
                    wows_data,
                    twitch_state: Arc::clone(&self.twitch_state),
                    should_send_replays: self.settings.send_replay_data,
                    data_export_settings: DataExportSettings {
                        should_auto_export: self.settings.replay_settings.auto_export_data,
                        export_path: PathBuf::from(self.settings.replay_settings.auto_export_path.clone()),
                        export_format: self.settings.replay_settings.auto_export_format,
                    },

                    constants_file_data: Arc::clone(&self.game_constants),
                    player_tracker: Arc::clone(&self.settings.player_tracker),
                    is_debug: self.settings.debug_mode,
                    parser_lock: Arc::clone(&self.parser_lock),
                };
                crate::task::start_background_parsing_thread(background_thread_data);
            }

            let watcher = notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| match res {
                Ok(event) => {
                    // TODO: maybe properly handle moves?
                    debug!("filesytem event: {:?}", event);
                    match event.kind {
                        EventKind::Modify(ModifyKind::Name(RenameMode::To)) | EventKind::Create(_) => {
                            for path in event.paths {
                                if path.is_file() {
                                    if path.extension().map(|ext| ext == "wowsreplay").unwrap_or(false)
                                        && path.file_name().expect("path has no filename") != "temp.wowsreplay"
                                    {
                                        tx.send(NotifyFileEvent::Added(path.clone()))
                                            .expect("failed to send file creation event");
                                        // Send this path to the thread watching for replays in background
                                        let _ = background_tx
                                            .send(crate::task::ReplayBackgroundParserThreadMessage::NewReplay(path));
                                    } else if path.file_name().expect("path has no file name") == "tempArenaInfo.json" {
                                        tx.send(NotifyFileEvent::TempArenaInfoCreated(path.clone()))
                                            .expect("failed to send file creation event");
                                    }
                                }
                            }
                        }
                        EventKind::Modify(ModifyKind::Data(_)) => {
                            for path in event.paths {
                                if let Some(filename) = path.file_name()
                                    && filename == "preferences.xml"
                                {
                                    debug!("Sending preferences changed event");
                                    tx.send(NotifyFileEvent::PreferencesChanged)
                                        .expect("failed to send file creation event");
                                }
                                if path.extension().map(|ext| ext == "wowsreplay").unwrap_or(false) {
                                    tx.send(NotifyFileEvent::Modified(path.clone()))
                                        .expect("failed to send file modification event");
                                    let _ = background_tx
                                        .send(crate::task::ReplayBackgroundParserThreadMessage::ModifiedReplay(path));
                                }
                            }
                        }
                        EventKind::Remove(_) => {
                            for path in event.paths {
                                tx.send(NotifyFileEvent::Removed(path)).expect("failed to send file removal event");
                            }
                        }
                        _ => {
                            // TODO: handle RenameMode::From for proper file moves
                        }
                    }
                }
                Err(e) => debug!("watch error: {:?}", e),
            })
            .expect("failed to create fs watcher for replays dir");
            self.file_watcher = Some(watcher);
            self.file_receiver = Some(rx);
            self.file_watcher.as_mut().unwrap()
        };

        // Add a path to be watched. All files and directories at that path and
        // below will be monitored for changes.
        watcher.watch(replay_dir, RecursiveMode::NonRecursive).expect("failed to watch directory");

        self.settings.wows_dir = wows_dir.to_str().unwrap().to_string();
        self.settings.replays_dir = Some(replay_dir.to_owned())
    }

    #[must_use]
    pub fn load_game_data(&self, wows_directory: PathBuf) -> BackgroundTask {
        let (tx, rx) = mpsc::channel();
        let locale = self.settings.locale.clone().unwrap();
        let _join_handle = std::thread::spawn(move || {
            let _ = tx.send(crate::task::load_wows_files(wows_directory, locale.as_str()));
        });

        BackgroundTask { receiver: Some(rx), kind: BackgroundTaskKind::LoadingData }
    }
}
