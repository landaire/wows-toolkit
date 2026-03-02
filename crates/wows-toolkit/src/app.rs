use crate::icon_str;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::TryRecvError;

use eframe::APP_KEY;
use egui::Color32;
use egui::Context;
use egui::KeyboardShortcut;
use egui::Modifiers;
use egui::OpenUrl;
use egui::RichText;
use egui::ScrollArea;
use egui::TextStyle;
use egui::Ui;
use egui::UiKind;
use egui::WidgetText;
use egui_commonmark::CommonMarkViewer;
use egui_dock::DockArea;
use egui_dock::DockState;
use egui_dock::Style;
use egui_dock::TabStyle;
use egui_dock::TabViewer;

use octocrab::models::repos::Release;
use rootcause::Report;
use rootcause::hooks::builtin_hooks::report_formatter::DefaultReportFormatter;
use rootcause::prelude::ResultExt;
use tracing::debug;
use tracing::error;
use tracing::trace;
use tracing::warn;

use serde::Deserialize;
use serde::Serialize;

use tokio::runtime::Runtime;
use wows_replays::analyzer::battle_controller::GameMessage;

use crate::error::ToolkitError;
use crate::icons;
use crate::tab_state::TabState;
use crate::task;
use crate::task::BackgroundTaskCompletion;
use crate::task::BackgroundTaskKind;
use crate::task::NetworkJob;
use crate::task::NetworkResult;
use crate::task::ReplayBackgroundParserThreadMessage;
use crate::ui::file_unpacker::UNPACKER_STOP;

#[macro_export]
macro_rules! update_background_task {
    ($saved_tasks:expr, $background_task:expr) => {
        let task = $background_task;
        if let Some(task) = task {
            $saved_tasks.push(task);
        }
    };
}

#[allow(dead_code)]
#[derive(Clone)]
pub enum Tab {
    Unpacker,
    ReplayParser,
    Settings,
    PlayerTracker,
    ModManager,
    ArmorViewer,
    Stats,
}

impl Tab {
    fn title(&self) -> &'static str {
        match self {
            Tab::Unpacker => icon_str!(icons::ARCHIVE, "Resource Unpacker"),
            Tab::Settings => icon_str!(icons::GEAR_FINE, "Settings"),
            Tab::ReplayParser => icon_str!(icons::MAGNIFYING_GLASS, "Replay Inspector"),
            Tab::PlayerTracker => icon_str!(icons::DETECTIVE, "Player Tracker"),
            Tab::ModManager => icon_str!(icons::WRENCH, "Mod Manager"),
            Tab::ArmorViewer => icon_str!(icons::SHIELD, "Armor Viewer"),
            Tab::Stats => icon_str!(icons::CHART_BAR, "Stats"),
        }
    }
}

pub struct ToolkitTabViewer<'a> {
    pub tab_state: &'a mut TabState,
}

impl TabViewer for ToolkitTabViewer<'_> {
    // This associated type is used to attach some data to each tab.
    type Tab = Tab;

    // Returns the current `tab`'s title.
    fn title(&mut self, tab: &mut Self::Tab) -> WidgetText {
        tab.title().into()
    }

    // Defines the contents of a given `tab`.
    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        match tab {
            Tab::Unpacker => self.build_unpacker_tab(ui),
            Tab::Settings => self.build_settings_tab(ui),
            Tab::ReplayParser => self.build_replay_parser_tab(ui),
            Tab::PlayerTracker => self.build_player_tracker_tab(ui),
            Tab::ModManager => self.build_mod_manager_tab(ui),
            Tab::ArmorViewer => self.build_armor_viewer_tab(ui),
            Tab::Stats => self.build_stats_tab(ui),
        }
    }

    fn tab_style_override(&self, tab: &Self::Tab, global_style: &TabStyle) -> Option<TabStyle> {
        if matches!(tab, Tab::Settings) && self.tab_state.settings_needs_attention {
            let mut style = global_style.clone();
            let red = egui::Color32::from_rgb(255, 80, 80);
            style.active.text_color = red;
            style.inactive.text_color = red;
            style.focused.text_color = red;
            style.hovered.text_color = red;
            style.active_with_kb_focus.text_color = red;
            style.inactive_with_kb_focus.text_color = red;
            style.focused_with_kb_focus.text_color = red;
            Some(style)
        } else {
            None
        }
    }
}

#[derive(Default)]
pub struct ReplayParserTabState {
    pub game_chat: Vec<GameMessage>,
}

/// We derive Deserialize/Serialize so we can persist app state on shutdown.
#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct WowsToolkitApp {
    #[serde(skip)]
    checked_for_updates: bool,
    #[serde(skip)]
    update_window_open: bool,
    #[serde(skip)]
    panic_window_open: bool,
    #[serde(skip)]
    panic_info: Option<String>,
    #[serde(skip)]
    build_consent_window_open: bool,
    #[serde(skip)]
    latest_release: Option<Release>,
    #[serde(skip)]
    show_about_window: bool,
    #[serde(skip)]
    show_error_window: bool,
    #[serde(skip)]
    error_to_show: Option<String>,

    pub(crate) tab_state: TabState,
    #[serde(skip)]
    dock_state: DockState<Tab>,

    #[serde(skip)]
    pub(crate) runtime: Arc<Runtime>,

    /// Whether a constants/game version mismatch has been detected.
    #[serde(skip)]
    constants_version_mismatch: bool,
    /// Whether we've already shown a network error for constants updates
    /// (to avoid spamming the user on repeated failures).
    #[serde(skip)]
    constants_update_error_shown: bool,

    /// Whether we've already shown a toast for an invalid twitch token.
    #[serde(skip)]
    shown_twitch_token_error: bool,

    /// Receiver for results from the background networking thread.
    #[serde(skip)]
    pub(crate) network_result_rx: Option<std::sync::mpsc::Receiver<NetworkResult>>,

    /// Guard for the non-blocking log writer. Dropping this flushes remaining logs.
    #[cfg(feature = "logging")]
    #[serde(skip)]
    _log_guard: Option<tracing_appender::non_blocking::WorkerGuard>,

    /// Active realtime armor viewer windows spawned from replay renderers.
    #[serde(skip)]
    realtime_armor_viewers: Vec<Arc<egui::mutex::Mutex<crate::realtime_armor_viewer::RealtimeArmorViewer>>>,
}

impl Default for WowsToolkitApp {
    fn default() -> Self {
        Self {
            checked_for_updates: false,
            update_window_open: false,
            panic_info: None,
            panic_window_open: false,
            build_consent_window_open: false,
            latest_release: None,
            show_about_window: false,
            tab_state: Default::default(),
            dock_state: DockState::new(
                [Tab::ReplayParser, Tab::Stats, Tab::PlayerTracker, Tab::ArmorViewer, Tab::Unpacker, Tab::Settings]
                    .to_vec(),
            ),
            show_error_window: false,
            error_to_show: None,
            constants_version_mismatch: false,
            constants_update_error_shown: false,
            shown_twitch_token_error: false,
            network_result_rx: None,
            runtime: Arc::new(Runtime::new().expect("failed to create tokio runtime")),
            #[cfg(feature = "logging")]
            _log_guard: None,
            realtime_armor_viewers: Vec::new(),
        }
    }
}

impl WowsToolkitApp {
    /// Called once before the first frame.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // This is also where you can customize the look and feel of egui using
        // `cc.egui_ctx.set_visuals` and `cc.egui_ctx.set_fonts`.

        // Include phosphor icons
        let mut fonts = egui::FontDefinitions::default();
        egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
        egui_extras::install_image_loaders(&cc.egui_ctx);

        // TODO: Maybe at some point I want to use Berkeley Mono?
        // fonts.font_data.insert("bm".into(), egui::FontData::from_static(include_bytes!("../assets/BerkeleyMono-Regular.otf")).into());

        // if let Some(font_keys) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        //     font_keys.insert(0, "bm".into());
        // }
        // if let Some(font_keys) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
        //     font_keys.insert(0, "bm".into());
        // }

        // fonts.add_font(FontInsert::new(
        //     "bm",
        //     egui::FontData::from_static(include_bytes!("")),
        //     vec![
        //         InsertFontFamily { family: egui::FontFamily::Proportional, priority: egui::epaint::text::FontPriority::Highest },
        //         InsertFontFamily { family: egui::FontFamily::Monospace, priority: egui::epaint::text::FontPriority::Lowest },
        //     ],
        // ));

        cc.egui_ctx.set_fonts(fonts);
        cc.egui_ctx.set_theme(egui::Theme::Dark);

        // Load previous app state (if any).
        // Note that you must enable the `persistence` feature for this to work.
        let mut had_saved_state = false;
        let mut state = if let Some(storage) = cc.storage {
            let mut saved_state: Self = if storage.get_string(APP_KEY).is_some() {
                // if the app key is present and we get no result back, that means deserialization
                // failed and we should panic because this is an app bug -- likely caused by
                // not setting a default value for a persisted field
                match eframe::get_value(storage, eframe::APP_KEY) {
                    Some(app) => {
                        had_saved_state = true;
                        app
                    }
                    None => {
                        if cfg!(debug_assertions) {
                            panic!("could not deserialize app state")
                        } else {
                            error!("could not deserialize app state -- using default");
                            Default::default()
                        }
                    }
                }
            } else {
                warn!("Creating new default app settings");
                Default::default()
            };

            if !saved_state.tab_state.settings.has_052_game_params_fix {
                saved_state.tab_state.settings.has_052_game_params_fix = true;
                crate::game_params::clear_all_game_params_caches();
            }

            // Apply persisted armor viewer defaults to the initial pane
            // (ArmorViewerState is #[serde(skip)] so it gets Default on load)
            saved_state.tab_state.armor_viewer.apply_defaults(&saved_state.tab_state.armor_viewer_defaults);

            // Sync the GPU encoder warning flag from persisted settings
            saved_state.tab_state.suppress_gpu_encoder_warning.store(
                saved_state.tab_state.settings.suppress_gpu_encoder_warning,
                std::sync::atomic::Ordering::Relaxed,
            );

            // Ensure session stats are sorted correctly (backfills sort_key for legacy data)
            saved_state.tab_state.settings.session_stats.sort_games();

            if !saved_state.tab_state.settings.wows_dir.is_empty() {
                let task = Some(
                    saved_state
                        .tab_state
                        .load_game_data(PathBuf::from(saved_state.tab_state.settings.wows_dir.clone())),
                );
                update_background_task!(saved_state.tab_state.background_tasks, task);
            }

            saved_state
        } else {
            Default::default()
        };

        const DEFAULT_ZOOM_FACTOR: f32 = 1.15;

        if !had_saved_state {
            let mut this: Self = Default::default();
            // this.tab_state.settings.locale = Some(get_locale().unwrap_or_else(|| String::from("en")));
            this.tab_state.settings.locale = Some("en".to_string());

            let default_wows_dir = "C:\\Games\\World_of_Warships";
            let default_wows_path = Path::new(default_wows_dir);
            if default_wows_path.exists() {
                this.tab_state.settings.wows_dir = default_wows_dir.to_string();

                let task = this.tab_state.load_game_data(default_wows_path.to_path_buf());
                update_background_task!(this.tab_state.background_tasks, Some(task));
            }

            // By default set the zoom factor. We don't persist this value because it's
            // persisted with the application window instead.
            cc.egui_ctx.set_zoom_factor(DEFAULT_ZOOM_FACTOR);

            state = this;
        }

        // Check if the application panicked
        let panic_log_path = Self::panic_log_path();
        if panic_log_path.exists() {
            let mut file = File::open(panic_log_path).expect("failed to open panic log");
            let mut contents = String::new();
            file.read_to_string(&mut contents).expect("failed to read panic log");
            state.panic_info = Some(contents);
            state.panic_window_open = true;
        }

        if !state.tab_state.settings.build_consent_window_shown {
            state.build_consent_window_open = true;
        }

        // Initialize logging if the feature is enabled and the user hasn't disabled it
        #[cfg(feature = "logging")]
        if state.tab_state.settings.enable_logging {
            state._log_guard = Self::init_logging();
        }

        // Capture wgpu render state for 3D viewport rendering
        state.tab_state.wgpu_render_state = cc.wgpu_render_state.clone();

        let (tx, rx) = tokio::sync::mpsc::channel(1);
        state.tab_state.twitch_update_sender = Some(tx);
        task::begin_startup_tasks(&mut state, rx);

        state
    }

    /// Initialize the tracing subscriber with file logging.
    /// Only logs from `wows_toolkit`, `wows_replays`, and `wows_minimap_renderer` are captured.
    #[cfg(feature = "logging")]
    fn init_logging() -> Option<tracing_appender::non_blocking::WorkerGuard> {
        use tracing_appender::rolling::Rotation;
        use tracing_subscriber::Layer;
        use tracing_subscriber::fmt;
        use tracing_subscriber::fmt::time::LocalTime;
        use tracing_subscriber::layer::SubscriberExt;

        let log_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| ".".into());
        let file_appender = tracing_appender::rolling::Builder::new()
            .rotation(Rotation::HOURLY)
            .max_log_files(3)
            .filename_prefix("wows_toolkit.log")
            .build(&log_dir)
            .ok()?;
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

        let target_filter =
            tracing_subscriber::filter::Targets::new().with_target("wows_toolkit", tracing::Level::DEBUG);

        let subscriber = tracing_subscriber::registry().with(
            fmt::Layer::new()
                .with_writer(non_blocking)
                .with_timer(LocalTime::rfc_3339())
                .with_ansi(false)
                .with_target(true)
                .with_filter(target_filter),
        );

        // In debug builds, also log to the console
        #[cfg(debug_assertions)]
        let subscriber = {
            let console_filter =
                tracing_subscriber::filter::Targets::new().with_target("wows_toolkit", tracing::Level::DEBUG);

            subscriber.with(fmt::Layer::new().with_ansi(true).with_target(true).with_filter(console_filter))
        };

        let _ = tracing::subscriber::set_global_default(subscriber);

        Some(guard)
    }

    pub fn build_bottom_panel(&mut self, ui: &mut Ui) {
        // Try to update mod update tasks
        if let Ok(new_task) = self.tab_state.background_task_receiver.try_recv() {
            self.tab_state.background_tasks.push(new_task);
        }

        if self.tab_state.settings.debug_mode {
            ui.label(RichText::new("⚠ Debug build ⚠").heading().color(ui.visuals().warn_fg_color));
        }

        ui.horizontal(|ui| {
            let mut remove_tasks = Vec::new();

            // Count pending LoadingReplay tasks so we can show a single consolidated indicator
            let pending_replay_count = self
                .tab_state
                .background_tasks
                .iter()
                .filter(|t| matches!(t.kind, BackgroundTaskKind::LoadingReplay) && t.receiver.is_some())
                .count();
            let mut shown_replay_spinner = false;

            for i in 0..self.tab_state.background_tasks.len() {
                let task = &mut self.tab_state.background_tasks[i];

                let remove_task = {
                    // For LoadingReplay tasks, show one consolidated spinner instead of many
                    let desc = if matches!(task.kind, BackgroundTaskKind::LoadingReplay) && pending_replay_count > 1 {
                        if !shown_replay_spinner {
                            shown_replay_spinner = true;
                            ui.spinner();
                            ui.label(format!("Loading {} replays...", pending_replay_count));
                        }
                        task.check_completion()
                    } else {
                        task.build_description(ui)
                    };
                    trace!("Task description: {:?}", desc);
                    if let Some(result) = desc {
                        match &task.kind {
                            BackgroundTaskKind::LoadingData => {
                                self.tab_state.allow_changing_wows_dir();
                            }
                            BackgroundTaskKind::LoadingBuildData(_) => {}
                            BackgroundTaskKind::LoadingReplay => {}
                            BackgroundTaskKind::Updating { rx: _rx, last_progress: _last_progress } => {}
                            BackgroundTaskKind::PopulatePlayerInspectorFromReplays => {}
                            BackgroundTaskKind::LoadingConstants => {}
                            #[cfg(feature = "mod_manager")]
                            BackgroundTaskKind::ModTask(_task_info) => {}
                            BackgroundTaskKind::LoadingPersonalRatingData => {}
                            BackgroundTaskKind::UpdateTimedMessage(toast) => {
                                let mut toasts = self.tab_state.toasts.lock();
                                match &toast.level {
                                    task::ToastLevel::Success => {
                                        toasts.success(toast.message.clone());
                                    }
                                    task::ToastLevel::Info => {
                                        toasts.info(toast.message.clone());
                                    }
                                    task::ToastLevel::Warning => {
                                        toasts.warning(toast.message.clone());
                                    }
                                    task::ToastLevel::Error => {
                                        toasts.error(toast.message.clone());
                                    }
                                };
                            }
                            BackgroundTaskKind::OpenFileViewer(plaintext_file_viewer) => {
                                self.tab_state.file_viewer.lock().push(plaintext_file_viewer.clone());
                            }
                        }

                        match result {
                            Ok(data) => match data {
                                BackgroundTaskCompletion::NoReceiver => {}
                                BackgroundTaskCompletion::DataLoaded {
                                    new_dir,
                                    wows_data,
                                    replays,
                                    available_builds,
                                } => {
                                    let replays_dir = wows_data.replays_dir.clone();
                                    let build_number = wows_data.build_number;

                                    // Detect if the WoWs directory changed
                                    let dir_changed =
                                        self.tab_state.settings.wows_dir != new_dir.to_str().unwrap_or_default();

                                    // Clear all stale game state when directory changes
                                    if dir_changed {
                                        self.tab_state.reset_game_state();
                                    }

                                    if let Some(old_wows_data) = &self.tab_state.world_of_warships_data {
                                        *old_wows_data.write() = wows_data;
                                    } else {
                                        let wows_data = Arc::new(parking_lot::RwLock::new(wows_data));
                                        self.tab_state.world_of_warships_data = Some(Arc::clone(&wows_data));

                                        #[cfg(feature = "mod_manager")]
                                        crate::mod_manager::start_mod_manager_thread(
                                            Arc::clone(&self.runtime),
                                            wows_data,
                                            self.tab_state.mod_action_receiver.take().unwrap(),
                                            self.tab_state.background_task_sender.clone(),
                                        );
                                    }

                                    // Initialize or update the version data map.
                                    // Always create a new map when the directory changed
                                    // (reset_game_state sets wows_data_map to None).
                                    let wows_data_ref = self.tab_state.world_of_warships_data.as_ref().unwrap();
                                    if let Some(map) = &self.tab_state.wows_data_map {
                                        map.insert(build_number, Arc::clone(wows_data_ref));
                                    } else {
                                        let mut map = crate::wows_data::WoWsDataMap::new(
                                            PathBuf::from(&new_dir),
                                            self.tab_state.settings.locale.clone().unwrap_or_else(|| "en".to_string()),
                                        );
                                        if let Some(tx) = self.tab_state.network_job_tx.clone() {
                                            map.set_network_job_tx(tx);
                                        }
                                        map.insert(build_number, Arc::clone(wows_data_ref));
                                        self.tab_state.wows_data_map = Some(map);
                                    }

                                    // If the initial build used fallback constants, request the correct version
                                    if !wows_data_ref.read().replay_constants_exact_match {
                                        self.tab_state.send_network_job(NetworkJob::FetchVersionedConstants {
                                            build: build_number,
                                        });
                                    }

                                    self.tab_state.available_builds = available_builds;
                                    self.tab_state.selected_browser_build = build_number;

                                    self.tab_state.update_wows_dir(&new_dir, &replays_dir);
                                    let no_replays = replays.as_ref().is_none_or(|r| r.is_empty());
                                    self.tab_state.replay_files = replays;
                                    self.tab_state.browser_state.reset_filters();

                                    self.tab_state.toasts.lock().success("Successfully loaded game data");

                                    if no_replays {
                                        self.tab_state.toasts.lock().warning(
                                            "No replays detected \u{2014} is your WoWs directory properly configured?",
                                        );
                                    }

                                    self.check_constants_version_mismatch();
                                }
                                BackgroundTaskCompletion::BuildDataLoaded { build } => {
                                    self.tab_state.selected_browser_build = build;
                                    self.tab_state.browser_state.reset_filters();
                                    self.tab_state.toasts.lock().success(format!("Loaded build {build}"));
                                }
                                BackgroundTaskCompletion::ReplayLoaded { replay, source } => {
                                    use crate::task::ReplaySource;

                                    let track_session_stats = matches!(
                                        source,
                                        ReplaySource::FileListing
                                            | ReplaySource::AutoLoad
                                            | ReplaySource::Reload
                                            | ReplaySource::SessionStatsOnly
                                    );
                                    let update_ui = !matches!(source, ReplaySource::SessionStatsOnly);
                                    let open_tab = matches!(
                                        source,
                                        ReplaySource::ManualOpen | ReplaySource::AutoLoad | ReplaySource::Reload
                                    );

                                    if track_session_stats {
                                        let replay_guard = replay.read();
                                        if let Some(stat) = crate::session_stats::PerGameStat::from_replay(
                                            &replay_guard,
                                            &replay_guard.resource_loader,
                                        ) {
                                            self.tab_state.settings.session_stats.add_game(stat);
                                        }
                                        drop(replay_guard);
                                    }
                                    if update_ui {
                                        self.tab_state.replay_parser_tab.lock().game_chat.clear();
                                        self.tab_state
                                            .settings
                                            .player_tracker
                                            .write()
                                            .update_from_replay(&replay.read());
                                        if open_tab {
                                            self.tab_state.open_replay_in_focused_tab(replay);
                                        }
                                        self.tab_state.toasts.lock().success("Successfully loaded replay");
                                        self.try_update_constants();
                                    }
                                }
                                BackgroundTaskCompletion::UpdateDownloaded(new_exe) => {
                                    let current_process =
                                        std::env::current_exe().expect("current process has no path?");
                                    let mut current_process_new_path = current_process.as_os_str().to_owned();
                                    current_process_new_path.push(".old");
                                    let current_process_new_path = PathBuf::from(current_process_new_path);
                                    let rename_process = move || {
                                        std::fs::rename(current_process.clone(), &current_process_new_path)
                                            .context("failed to rename current process")?;
                                        std::fs::rename(new_exe, &current_process)
                                            .context("failed to rename new process")?;

                                        std::process::Command::new(current_process)
                                            .arg(current_process_new_path)
                                            .spawn()
                                            .context("failed to execute updated process")
                                    };

                                    match rename_process() {
                                        Ok(_) => {
                                            std::process::exit(0);
                                        }
                                        Err(e) => {
                                            error!("Update rename failed: {e:?}");
                                            self.show_err_window(e.into());
                                        }
                                    }
                                }
                                BackgroundTaskCompletion::PopulatePlayerInspectorFromReplays => {
                                    // Switch to "All Time" so historical data is visible
                                    self.tab_state.settings.player_tracker.write().filter_time_period =
                                        crate::ui::player_tracker::TimePeriod::AllTime;
                                }
                                BackgroundTaskCompletion::ConstantsLoaded(constants) => {
                                    *self.tab_state.game_constants.write() = constants;
                                    self.check_constants_version_mismatch();
                                }
                                BackgroundTaskCompletion::PersonalRatingDataLoaded(pr_data) => {
                                    self.tab_state.personal_rating_data.write().load(pr_data);
                                }
                                #[cfg(feature = "mod_manager")]
                                BackgroundTaskCompletion::ModManager(mod_manager_info) => match *mod_manager_info {
                                    crate::mod_manager::ModTaskCompletion::DatabaseLoaded(index) => {
                                        self.tab_state.mod_manager_info.update_index("test".to_string(), index);
                                    }
                                    crate::mod_manager::ModTaskCompletion::ModInstalled(mod_info) => {
                                        self.tab_state
                                            .toasts
                                            .lock()
                                            .success(format!("Successfully installed mod: {}", mod_info.meta.name()));
                                    }
                                    crate::mod_manager::ModTaskCompletion::ModUninstalled(mod_info) => {
                                        self.tab_state
                                            .toasts
                                            .lock()
                                            .success(format!("Successfully uninstalled mod: {}", mod_info.meta.name()));
                                    }
                                    crate::mod_manager::ModTaskCompletion::ModDownloaded(_) => {}
                                },
                            },
                            Err(e)
                                if e.downcast_current_context::<ToolkitError>()
                                    .is_some_and(|e| matches!(e, ToolkitError::BackgroundTaskCompleted)) => {}
                            Err(e) => {
                                error!("Background task error: {e:?}");

                                if e.downcast_current_context::<ToolkitError>()
                                    .is_some_and(|e| matches!(e, ToolkitError::InvalidWowsDirectory(_)))
                                {
                                    self.tab_state.settings_needs_attention = true;
                                }

                                self.tab_state.toasts.lock().error(format!("{e}"));
                            }
                        }
                        true
                    } else {
                        false
                    }
                };

                if remove_task {
                    remove_tasks.push(i);
                }
            }

            // Remove whatever background tasks have yielded a result
            self.tab_state.background_tasks = self
                .tab_state
                .background_tasks
                .drain(..)
                .enumerate()
                .filter_map(|(i, task)| if remove_tasks.contains(&i) { None } else { Some(task) })
                .collect();

            if let Some(rx) = &self.tab_state.unpacker_progress {
                if ui.button("Stop").clicked() {
                    UNPACKER_STOP.store(true, Ordering::Relaxed);
                }
                let mut done = false;
                loop {
                    match rx.try_recv() {
                        Ok(progress) => {
                            self.tab_state.last_progress = Some(progress);
                        }
                        Err(TryRecvError::Empty) => {
                            if let Some(last_progress) = self.tab_state.last_progress.as_ref() {
                                ui.add(
                                    egui::ProgressBar::new(last_progress.progress)
                                        .text(last_progress.file_name.as_str()),
                                );
                            }
                            break;
                        }
                        Err(TryRecvError::Disconnected) => {
                            done = true;
                            break;
                        }
                    }
                }

                if done {
                    self.tab_state.unpacker_progress.take();
                    self.tab_state.last_progress.take();
                }
            }
        });
    }

    /// Send all startup network checks to the background networking thread (non-blocking).
    fn request_update_checks(&mut self) {
        self.tab_state.send_network_job(NetworkJob::CheckForAppUpdates);
        self.tab_state.send_network_job(NetworkJob::FetchLatestConstants {
            current_commit: self.tab_state.settings.constants_file_commit.clone(),
        });
        if crate::personal_rating::needs_update() {
            self.tab_state.send_network_job(NetworkJob::FetchPersonalRatingData);
        }
        self.checked_for_updates = true;
    }

    /// Poll the networking thread for results and handle them.
    fn poll_network_results(&mut self) {
        let Some(rx) = &self.network_result_rx else {
            return;
        };
        while let Ok(result) = rx.try_recv() {
            match result {
                NetworkResult::AppUpdateAvailable(release) => {
                    self.update_window_open = true;
                    self.latest_release = Some(*release);
                }
                NetworkResult::AppUpToDate => {
                    self.tab_state.toasts.lock().success("Application up-to-date");
                }
                NetworkResult::AppUpdateCheckFailed(msg) => {
                    warn!("App update check failed: {}", msg);
                    self.tab_state.toasts.lock().error("Failed to check for app updates");
                }
                NetworkResult::ConstantsFetched { data, commit } => {
                    let mut constants_path = PathBuf::from("constants.json");
                    if let Some(storage_dir) = eframe::storage_dir(crate::APP_NAME) {
                        constants_path = storage_dir.join(constants_path);
                    }

                    if std::fs::write(&constants_path, data.as_slice()).is_ok() {
                        self.tab_state.settings.constants_file_commit = commit;
                        update_background_task!(self.tab_state.background_tasks, Some(task::load_constants(data)));
                    }
                }
                NetworkResult::ConstantsUpToDate => {}
                NetworkResult::ConstantsFetchFailed(msg) => {
                    warn!("Constants fetch failed: {}", msg);
                    if !self.constants_update_error_shown {
                        self.constants_update_error_shown = true;
                        self.tab_state
                            .toasts
                            .lock()
                            .error("Failed to fetch updated replay data mapping. Will retry later.")
                            .duration(None);
                    }
                }
                NetworkResult::PersonalRatingDataFetched(data) => {
                    if crate::personal_rating::save_expected_values(&data).is_ok() {
                        update_background_task!(
                            self.tab_state.background_tasks,
                            Some(task::load_personal_rating_data(data))
                        );
                    }
                }
                NetworkResult::PersonalRatingDataFetchFailed(msg) => {
                    warn!("PR data fetch failed: {}", msg);
                }
                NetworkResult::VersionedConstantsFetched { build } => {
                    // Versioned constants were downloaded and saved to disk.
                    // If we have this build loaded with inexact constants, rebuild it.
                    if let Some(wows_data_map) = self.tab_state.wows_data_map.as_ref()
                        && let Some(data) = wows_data_map.get(build)
                        && !data.read().replay_constants_exact_match
                    {
                        debug!("Rebuilding build {} with newly fetched versioned constants", build);
                        if data.write().rebuild_with_new_constants() {
                            // Invalidate cached reports so they rebuild with correct constants
                            if let Some(replay_files) = &self.tab_state.replay_files {
                                for replay in replay_files.values() {
                                    replay.write().ui_report = None;
                                }
                            }
                        }
                    }
                }
                NetworkResult::VersionedConstantsFetchFailed { build, msg } => {
                    warn!("Versioned constants fetch failed for build {}: {}", build, msg);
                }
            }
        }
    }

    fn ui_file_drag_and_drop(&mut self, ctx: &Context) {
        use egui::Align2;
        use egui::Color32;
        use egui::Id;
        use egui::LayerId;
        use egui::Order;
        use egui::TextStyle;

        // Preview hovering files:
        if !ctx.input(|i| i.raw.hovered_files.is_empty()) {
            let text = ctx.input(|i| {
                if i.raw.hovered_files.len() > 1 {
                    return Some("Only one file at a time, please.".to_owned());
                }

                if let Some(file) = i.raw.hovered_files.first()
                    && let Some(path) = &file.path
                    && path.is_file()
                {
                    return Some(format!("Drop to load\n{}", path.file_name()?.to_str()?));
                }

                None
            });

            if let Some(text) = text {
                let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("file_drop_target")));

                let screen_rect = ctx.content_rect();
                painter.rect_filled(screen_rect, 0.0, Color32::from_black_alpha(192));
                painter.text(
                    screen_rect.center(),
                    Align2::CENTER_CENTER,
                    text,
                    TextStyle::Heading.resolve(&ctx.style()),
                    Color32::WHITE,
                );
            }
        }

        let mut dropped_files = Vec::new();

        ctx.input(|i| {
            if !i.raw.dropped_files.is_empty() {
                dropped_files.clone_from(&i.raw.dropped_files);
            }
        });

        if dropped_files.len() == 1
            && let Some(path) = &dropped_files[0].path
            && let Some(deps) = self.tab_state.replay_dependencies()
        {
            self.tab_state.settings.current_replay_path = path.clone();
            update_background_task!(
                self.tab_state.background_tasks,
                deps.parse_replay_from_path(
                    self.tab_state.settings.current_replay_path.clone(),
                    crate::task::ReplaySource::ManualOpen
                )
            );
        }
    }

    fn update_impl(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if mitigate_wgpu_mem_leak(ctx) {
            return;
        }

        if ctx
            .input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::CTRL | Modifiers::SHIFT, egui::Key::D)))
        {
            self.tab_state.settings.debug_mode = !self.tab_state.settings.debug_mode;
            if let Some(sender) = self.tab_state.background_parser_tx.as_ref() {
                let _ = sender
                    .send(ReplayBackgroundParserThreadMessage::DebugStateChange(self.tab_state.settings.debug_mode));
            }
        }

        self.tab_state.try_update_replays();
        self.tab_state.process_session_stats_reset();

        if !self.checked_for_updates && self.tab_state.settings.check_for_updates {
            self.request_update_checks();
        }

        self.poll_network_results();

        // Update settings_needs_attention based on WoWs directory validity and twitch token state
        {
            let wows_dir = Path::new(&self.tab_state.settings.wows_dir);
            let wows_dir_invalid = if self.tab_state.settings.wows_dir.is_empty() {
                false
            } else if !wows_dir.exists() {
                true
            } else {
                // Must have at least one of: WorldOfWarships.exe, bin/, replays/
                let has_exe = wows_dir.join("WorldOfWarships.exe").exists();
                let has_bin = wows_dir.join("bin").exists();
                let has_replays = wows_dir.join("replays").exists();
                !has_exe && !has_bin && !has_replays
            };

            let twitch_token_failed = self.tab_state.settings.twitch_token.is_some()
                && self.tab_state.twitch_state.read().token_validation_failed;

            if twitch_token_failed && !self.shown_twitch_token_error {
                self.shown_twitch_token_error = true;
                error!("Twitch token is invalid or expired");
                self.tab_state.toasts.lock().error("Twitch token is invalid or expired. Please update it in Settings.");
            } else if !twitch_token_failed {
                self.shown_twitch_token_error = false;
            }

            self.tab_state.settings_needs_attention = wows_dir_invalid || twitch_token_failed;
        }

        if self.build_consent_window_open {
            egui::Window::new("Build Collection Consent").collapsible(false).show(ctx, |ui| {
                ui.label("Would you like to send player build information information from ranked and random battles to the developer? This data collection helps the community bulk analyze player builds. You may opt out at any time in the settings.");
                ui.horizontal(|ui| {
                    if ui.button("Yes").clicked() {
                        self.build_consent_window_open = false;
                        self.tab_state.settings.build_consent_window_shown = true;
                        self.tab_state.settings.send_replay_data = true;
                        self.tab_state.send_replay_consent_changed();
                    }
                    if ui.button("No").clicked() {
                        self.build_consent_window_open = false;
                        self.tab_state.settings.build_consent_window_shown = true;
                        self.tab_state.settings.send_replay_data = false;
                        self.tab_state.send_replay_consent_changed();
                    }
                });
            });
        }

        if self.panic_window_open {
            self.show_panic_window(ctx);
        }

        if self.update_window_open {
            self.show_update_window(ctx);
        }

        if let Some(error) = self.error_to_show.as_ref() {
            if self.show_error_window {
                egui::Window::new("Error").open(&mut self.show_error_window).show(ctx, |ui| {
                    build_error_window(ui, error);
                });
            } else {
                self.error_to_show = None;
            }
        }

        if self.show_about_window {
            egui::Window::new("About").open(&mut self.show_about_window).show(ctx, |ui| {
                build_about_window(ui);
            });
        }

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                let is_web = cfg!(target_arch = "wasm32");
                if !is_web {
                    ui.menu_button("File", |ui| {
                        if ui.button("Check for Updates").clicked() {
                            self.checked_for_updates = false;
                            ui.close_kind(UiKind::Menu);
                        }
                        if ui.button("About").clicked() {
                            self.show_about_window = true;
                            ui.close_kind(UiKind::Menu);
                        }
                        if ui.button("Quit").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                    ui.add_space(16.0);
                }

                if ui.button(icon_str!(icons::BUG, "Create Issue")).clicked() {
                    ui.ctx().open_url(OpenUrl::new_tab("https://github.com/landaire/wows-toolkit/issues/new/choose"));
                }

                if ui.button(icon_str!(icons::DISCORD_LOGO, "Discord")).clicked() {
                    ui.ctx().open_url(OpenUrl::new_tab("https://discord.gg/SpmXzfSdux"));
                }
            });
        });

        egui::TopBottomPanel::bottom("status_panel").show(ctx, |ui| {
            self.build_bottom_panel(ui);
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            DockArea::new(&mut self.dock_state)
                .style(Style::from_egui(ui.style().as_ref()))
                .allowed_splits(egui_dock::AllowedSplits::None)
                .show_leaf_collapse_buttons(false)
                .show_leaf_close_all_buttons(false)
                .show_close_buttons(false)
                .show_inside(ui, &mut ToolkitTabViewer { tab_state: &mut self.tab_state });
        });

        // Pop open something to view the clicked file from the unpacker tab
        let mut file_viewer = self.tab_state.file_viewer.lock();
        let mut remove_viewers = Vec::new();
        for (idx, file_viewer) in file_viewer.iter_mut().enumerate() {
            file_viewer.draw(ctx);
            if !file_viewer.open.load(Ordering::Relaxed) {
                remove_viewers.push(idx);
            }
        }

        *file_viewer = file_viewer
            .drain(..)
            .enumerate()
            .filter_map(|(idx, viewer)| if !remove_viewers.contains(&idx) { Some(viewer) } else { None })
            .collect();
        drop(file_viewer);

        // Draw replay renderer viewports
        {
            let mut replay_renderers = self.tab_state.replay_renderers.lock();
            let mut remove_renderers = Vec::new();
            for (idx, renderer) in replay_renderers.iter().enumerate() {
                renderer.draw(ctx);
                if !renderer.open.load(Ordering::Relaxed) {
                    remove_renderers.push(idx);
                }
                // Check if renderer wants to save default options
                if let Some(saved) = renderer.pending_defaults_save.lock().take() {
                    self.tab_state.settings.renderer_options = saved;
                }
                // Sync GPU warning suppress flag back to settings
                let suppress = renderer.suppress_gpu_warning.load(Ordering::Relaxed);
                if suppress != self.tab_state.settings.suppress_gpu_encoder_warning {
                    self.tab_state.settings.suppress_gpu_encoder_warning = suppress;
                }
            }

            *replay_renderers = replay_renderers
                .drain(..)
                .enumerate()
                .filter_map(|(idx, r)| if !remove_renderers.contains(&idx) { Some(r) } else { None })
                .collect();
        }

        // Poll pending armor viewer requests from replay renderers and spawn viewers
        {
            // Poll ship assets loading (so it works without the Armor Viewer tab open)
            if let crate::armor_viewer::state::ShipAssetsState::Loading(ref rx) =
                self.tab_state.armor_viewer.ship_assets
                && let Ok(result) = rx.try_recv()
            {
                match result {
                    Ok(assets) => {
                        // Build ship catalog if not already built (same logic as build_armor_viewer_tab)
                        if self.tab_state.armor_viewer.ship_catalog.is_none()
                            && let Some(ref wows_data) = self.tab_state.world_of_warships_data
                        {
                            let wd = wows_data.read();
                            if let Some(metadata) = wd.game_metadata.as_ref() {
                                let catalog = crate::armor_viewer::ship_selector::ShipCatalog::build(metadata);
                                for nation_group in &catalog.nations {
                                    if !self
                                        .tab_state
                                        .armor_viewer
                                        .nation_flag_textures
                                        .contains_key(&nation_group.nation)
                                        && let Some(asset) =
                                            crate::task::load_nation_flag(&wd.vfs, &nation_group.nation)
                                    {
                                        self.tab_state
                                            .armor_viewer
                                            .nation_flag_textures
                                            .insert(nation_group.nation.clone(), asset);
                                    }
                                }
                                self.tab_state.armor_viewer.ship_catalog = Some(std::sync::Arc::new(catalog));
                            }
                        }
                        self.tab_state.armor_viewer.ship_assets =
                            crate::armor_viewer::state::ShipAssetsState::Loaded(assets);
                    }
                    Err(e) => {
                        tracing::error!("Failed to load ship assets: {e}");
                        self.tab_state.armor_viewer.ship_assets =
                            crate::armor_viewer::state::ShipAssetsState::Failed(e);
                    }
                }
            }

            let replay_renderers = self.tab_state.replay_renderers.lock();
            for renderer in replay_renderers.iter() {
                let mut state = renderer.shared_state().lock();
                let requests: Vec<crate::replay_renderer::ArmorViewerRequest> =
                    state.pending_armor_viewers.drain(..).collect();
                drop(state);

                for request in requests {
                    // Ensure ship assets and GPU pipeline are available
                    let ship_assets = match &self.tab_state.armor_viewer.ship_assets {
                        crate::armor_viewer::state::ShipAssetsState::Loaded(assets) => Some(assets.clone()),
                        _ => None,
                    };
                    let gpu_pipeline = self.tab_state.armor_viewer.gpu_pipeline.clone();
                    let render_state = self.tab_state.wgpu_render_state.clone();

                    if let (Some(ship_assets), Some(gpu_pipeline), Some(render_state)) =
                        (ship_assets, gpu_pipeline, render_state)
                    {
                        // Find the target player info from the bridge
                        let bridge = request.bridge.lock();
                        let target_player = bridge.players.iter().find(|p| p.entity_id == request.target_entity_id);
                        if let Some(player) = target_player {
                            let viewer = crate::realtime_armor_viewer::RealtimeArmorViewer::new(
                                player,
                                request.bridge.clone(),
                                ship_assets,
                                gpu_pipeline,
                                render_state,
                                Some(request.command_tx.clone()),
                            );
                            drop(bridge);
                            self.realtime_armor_viewers.push(Arc::new(egui::mutex::Mutex::new(viewer)));
                        } else {
                            // Bridge players not populated yet — re-queue for next frame
                            drop(bridge);
                            let mut state = renderer.shared_state().lock();
                            state.pending_armor_viewers.push(request);
                        }
                    } else {
                        // Assets not ready — trigger loading if needed
                        if matches!(
                            &self.tab_state.armor_viewer.ship_assets,
                            crate::armor_viewer::state::ShipAssetsState::NotLoaded
                        ) && let Some(ref wows_data) = self.tab_state.world_of_warships_data
                        {
                            let wd = wows_data.read();
                            let vfs = wd.vfs.clone();
                            let game_metadata = wd.game_metadata.clone();
                            drop(wd);
                            let (tx, rx) = std::sync::mpsc::channel();
                            std::thread::spawn(move || {
                                let result = (|| -> Result<Arc<wowsunpack::export::ship::ShipAssets>, String> {
                                    let metadata =
                                        game_metadata.ok_or_else(|| "GameMetadataProvider not loaded".to_string())?;
                                    let assets =
                                        wowsunpack::export::ship::ShipAssets::from_vfs_with_metadata(&vfs, metadata)
                                            .map_err(|e| format!("{e:?}"))?;
                                    Ok(Arc::new(assets))
                                })();
                                let _ = tx.send(result);
                            });
                            self.tab_state.armor_viewer.ship_assets =
                                crate::armor_viewer::state::ShipAssetsState::Loading(rx);
                        }
                        if self.tab_state.armor_viewer.gpu_pipeline.is_none()
                            && let Some(ref rs) = self.tab_state.wgpu_render_state
                        {
                            self.tab_state.armor_viewer.gpu_pipeline =
                                Some(Arc::new(crate::viewport_3d::GpuPipeline::new(&rs.device, &rs.queue)));
                        }
                        // Re-queue the request for next frame
                        let mut state = renderer.shared_state().lock();
                        state.pending_armor_viewers.push(request);
                    }
                }
            }
            drop(replay_renderers);
        }

        // Draw realtime armor viewer windows
        self.realtime_armor_viewers.retain(|v| v.lock().open.load(Ordering::Relaxed));
        for viewer in &self.realtime_armor_viewers {
            crate::realtime_armor_viewer::draw_realtime_armor_viewer(viewer, ctx);
        }

        self.ui_file_drag_and_drop(ctx);

        self.tab_state.toasts.lock().show(ctx);

        // When any replay renderer is playing, repaint continuously so all
        // deferred viewports (replay renderer + armor viewer) stay in sync
        // regardless of which window currently has focus.
        let any_playing = self.tab_state.replay_renderers.lock().iter().any(|r| r.shared_state().lock().playing);
        if any_playing || !self.realtime_armor_viewers.is_empty() {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after_secs(1.0);
        }
    }

    fn show_panic_window(&mut self, ctx: &Context) {
        if let Some(panic_info) = self.panic_info.as_mut() {
            egui::Window::new("Application Crash Detected").open(&mut self.panic_window_open).show(ctx, |ui| {
                ui.vertical(|ui| {
                    ui.label(
                        "It looks like WoWs Toolkit crashed the last time it ran. \
                    If you would like to report this issue, please either post an issue on \
                    GitHub or join the Discord server and provide the below information.",
                    );
                    ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                        ui.scope(|ui| {
                            let style = ui.style_mut();
                            style.override_text_style = Some(TextStyle::Monospace);
                            let widget = egui::TextEdit::multiline(panic_info).desired_width(f32::INFINITY);
                            ui.add_enabled(false, widget);
                        });
                    });
                    ui.horizontal(|ui| {
                        if ui.button("Copy").clicked() {
                            Context::copy_text(ctx, panic_info.clone());
                        }
                        if ui.button(icon_str!(icons::GITHUB_LOGO, "GitHub")).clicked() {
                            ui.ctx().open_url(OpenUrl::new_tab(
                                "https://github.com/landaire/wows-toolkit/issues/new/choose",
                            ));
                        }
                        if ui.button(icon_str!(icons::DISCORD_LOGO, "Discord")).clicked() {
                            ui.ctx().open_url(OpenUrl::new_tab("https://discord.gg/SpmXzfSdux"));
                        }
                    });
                    ui.collapsing("More Options", |ui| {
                        ui.label(
                            "If for some reason data that the application persists may \
                        be responsible, you can try clearing settings by pressing the button below. \
                        This will clear all settings, including tracked players. Your replays and any \
                        WoWs data will be safe.",
                        );
                        ui.scope(|ui| {
                            let visuals = &mut ui.style_mut().visuals;

                            visuals.widgets.inactive.bg_fill = Color32::from_rgb(200, 50, 50);
                            visuals.widgets.hovered.bg_fill = Color32::from_rgb(220, 70, 70);
                            visuals.widgets.active.bg_fill = Color32::from_rgb(160, 30, 30);

                            visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(200, 50, 50);
                            visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(220, 70, 70);
                            visuals.widgets.active.weak_bg_fill = Color32::from_rgb(160, 30, 30);

                            visuals.widgets.inactive.fg_stroke.color = Color32::WHITE;
                            visuals.widgets.hovered.fg_stroke.color = Color32::WHITE;
                            visuals.widgets.active.fg_stroke.color = Color32::WHITE;

                            if ui.button("Clear Settings").clicked() {
                                self.tab_state.settings = Default::default();
                            }
                        });
                    });
                });
            });
        }

        if !self.panic_window_open {
            let _ = std::fs::remove_file(Self::panic_log_path());
            self.panic_info = None;
        }
    }

    fn show_update_window(&mut self, ctx: &Context) {
        if let Some(latest_release) = self.latest_release.as_ref() {
            let url = latest_release.html_url.clone();
            let mut notes = latest_release.body.clone();
            let tag = latest_release.tag_name.clone();
            let asset = latest_release
                .assets
                .iter()
                .find(|asset| asset.name.contains("windows") && asset.name.ends_with(".zip"));
            if let Some(asset) = asset {
                egui::Window::new("Update Available").open(&mut self.update_window_open).show(ctx, |ui| {
                    ui.vertical(|ui| {
                        ui.label(format!("Version {tag} of WoWs Toolkit is available"));
                        if let Some(notes) = notes.as_mut() {
                            ScrollArea::vertical().max_height(500.0).show(ui, |ui| {
                                CommonMarkViewer::new().show(ui, &mut self.tab_state.markdown_cache, notes);
                            });
                        }
                        ui.horizontal(|ui| {
                            #[cfg(target_os = "windows")]
                            {
                                if ui.button("Install Update").clicked() {
                                    let task = Some(crate::task::start_download_update_task(&self.runtime, asset));
                                    update_background_task!(self.tab_state.background_tasks, task);
                                }
                            }
                            #[cfg(not(target_os = "windows"))]
                            {
                                let _ = asset;
                                ui.label("Update available, but only Windows is supported at this time.");
                            }
                            if ui.button("View Release").clicked() {
                                ui.ctx().open_url(OpenUrl::new_tab(url));
                            }
                        });
                    });
                });
            } else {
                self.update_window_open = false;
            }
        }
    }

    pub fn panic_log_path() -> PathBuf {
        let mut panic_log_path = PathBuf::from("wows_toolkit_panic.log");
        if let Some(storage_dir) = eframe::storage_dir(crate::APP_NAME) {
            panic_log_path = storage_dir.join(panic_log_path)
        }
        panic_log_path
    }

    /// If a constants/game version mismatch was detected, request updated
    /// constants from the networking thread. The thread handles throttling internally.
    fn try_update_constants(&mut self) {
        if !self.constants_version_mismatch {
            return;
        }

        self.tab_state.send_network_job(NetworkJob::FetchLatestConstants {
            current_commit: self.tab_state.settings.constants_file_commit.clone(),
        });
    }

    fn check_constants_version_mismatch(&mut self) {
        // Determine mismatch status under locks, then drop them before acting
        let mismatch_status = {
            let constants = self.tab_state.game_constants.read();
            let Some(wows_data) = &self.tab_state.world_of_warships_data else { return };
            let wows_data = wows_data.read();
            let Some(full_version) = &wows_data.full_version else { return };

            let constants_version = constants.get("VERSION").and_then(|v| v.get("VERSION")).and_then(|v| v.as_str());
            let Some(constants_version) = constants_version else { return };
            let game_version = format!("{}.{}", full_version.major, full_version.minor);

            if constants_version != game_version {
                Some(true) // mismatch
            } else if self.constants_version_mismatch {
                Some(false) // mismatch just resolved
            } else {
                None // no change
            }
        };

        match mismatch_status {
            Some(true) => {
                self.constants_version_mismatch = true;
                self.tab_state.toasts.lock()
                    .warning("Replay data mapping file version does not match game version.\nPost-battle results may not be accurate. Please be patient while project maintainers update the mapping on the server.".to_string())
                    .duration(None);
            }
            Some(false) => {
                self.constants_version_mismatch = false;
                self.tab_state.toasts.lock().dismiss_all_toasts();

                // Rebuild all loaded WorldOfWarshipsData with fresh constants
                let rebuild_ok = self
                    .tab_state
                    .wows_data_map
                    .as_ref()
                    .map(|map| map.rebuild_all_with_new_constants())
                    .unwrap_or(true);

                if rebuild_ok {
                    self.constants_update_error_shown = false;

                    // Invalidate ui_report on all loaded replays so they re-build
                    // with the new constants on next access
                    if let Some(replay_files) = &self.tab_state.replay_files {
                        for replay in replay_files.values() {
                            replay.write().ui_report = None;
                        }
                    }

                    // Re-load the focused replay to rebuild its ui_report
                    if let Some(focused) = self.tab_state.focused_replay()
                        && let Some(deps) = self.tab_state.replay_dependencies()
                    {
                        update_background_task!(
                            self.tab_state.background_tasks,
                            deps.load_replay(focused, crate::task::ReplaySource::Reload)
                        );
                    }

                    self.tab_state.toasts.lock().success("Replay data mapping file updated successfully");
                } else if !self.constants_update_error_shown {
                    self.constants_update_error_shown = true;
                    warn!("Failed to fetch versioned constants during rebuild");
                    self.tab_state
                        .toasts
                        .lock()
                        .error("Failed to fetch versioned constants during rebuild. Will retry later.")
                        .duration(None);
                }
            }
            None => {}
        }
    }

    fn show_err_window(&mut self, err: Report) {
        self.show_error_window = true;
        let formatted = err.format_with(&DefaultReportFormatter::ASCII);
        self.error_to_show = Some(format!("{formatted}"));
    }
}

impl eframe::App for WowsToolkitApp {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }

    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.update_impl(ctx, frame);
    }
}

fn build_about_window(ui: &mut egui::Ui) {
    ui.vertical(|ui| {
        ui.label("Made by landaire.");
        ui.label("Thanks to Trackpad, TTaro, lkolbly for their contributions.");
        ui.horizontal(|ui| {
            ui.label("Personal rating (PR) calculation data and formula provided by WoWs Numbers.");
            ui.hyperlink_to("More Info.", "https://wows-numbers.com/personal/rating");
        });
        if ui.button("View Project on GitHub").clicked() {
            ui.ctx().open_url(OpenUrl::new_tab("https://github.com/landaire/wows-toolkit"));
        }

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.label("Powered by ");
            ui.hyperlink_to("egui", "https://github.com/emilk/egui");
            ui.label(" and ");
            ui.hyperlink_to("eframe", "https://github.com/emilk/egui/tree/master/crates/eframe");
            ui.label(".");
        });
    });
}

fn build_error_window(ui: &mut egui::Ui, error: &str) {
    ui.vertical(|ui| {
        ui.label(icon_str!(icons::WARNING, "An error occurred:"));
        ui.label(error);
    });
}

/// Helper function to mitigate https://github.com/emilk/egui/issues/7434.
///
/// If this returns true, the app should early return in the `update()` function
/// or call `wgpu::Device::poll()`
pub fn mitigate_wgpu_mem_leak(ctx: &egui::Context) -> bool {
    let mut is_minimized = false;
    ctx.input(|reader| {
        is_minimized = reader.viewport().minimized.unwrap_or_default();
    });

    is_minimized
}
