use rust_i18n::t;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::TryRecvError;

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
use tracing::info;
use tracing::trace;
use tracing::warn;

use serde::Deserialize;
use serde::Serialize;

use tokio::runtime::Runtime;
use wows_replays::analyzer::battle_controller::GameMessage;

use crate::icons;
use crate::tab_state::TabState;
use crate::task;
use crate::task::BackgroundTaskCompletion;
use crate::task::BackgroundTaskKind;
use crate::task::NetworkJob;
use crate::task::NetworkResult;
use crate::task::ReplayBackgroundParserThreadMessage;
use crate::ui::file_unpacker::UNPACKER_STOP;
use crate::util::error::ToolkitError;

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
    fn title(&self) -> String {
        use rust_i18n::t;
        let (icon, key) = match self {
            Tab::Unpacker => (icons::ARCHIVE, "ui.tabs.unpacker"),
            Tab::Settings => (icons::GEAR_FINE, "ui.tabs.settings"),
            Tab::ReplayParser => (icons::MAGNIFYING_GLASS, "ui.tabs.replay_parser"),
            Tab::PlayerTracker => (icons::DETECTIVE, "ui.tabs.player_tracker"),
            Tab::ModManager => (icons::WRENCH, "ui.tabs.mod_manager"),
            Tab::ArmorViewer => (icons::SHIELD, "ui.tabs.armor_viewer"),
            Tab::Stats => (icons::CHART_BAR, "ui.tabs.stats"),
        };
        wt_translations::icon_t(icon, &t!(key))
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
    manual_update_requested: bool,
    #[serde(skip)]
    update_window_open: bool,
    #[serde(skip)]
    panic_window_open: bool,
    #[serde(skip)]
    panic_info: Option<String>,
    #[serde(skip)]
    build_consent_window_open: bool,
    #[serde(skip)]
    language_selection_open: bool,
    #[serde(skip)]
    latest_release: Option<Release>,
    #[serde(skip)]
    show_about_window: bool,
    #[serde(skip)]
    show_error_window: bool,
    #[serde(skip)]
    error_to_show: Option<String>,

    #[serde(skip)]
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
    realtime_armor_viewers: Vec<Arc<parking_lot::Mutex<crate::replay::realtime_armor_viewer::RealtimeArmorViewer>>>,

    /// SQLite connection pool for persisting app state.
    #[serde(skip)]
    db_pool: Option<sqlx::SqlitePool>,

    /// Last observed `PersistedState::generation`, used to detect changes
    /// and notify the background save task.
    #[serde(skip)]
    last_persisted_generation: u64,

    /// Shutdown signal for the background save task. Dropping or sending
    /// triggers a final save before the task exits.
    #[serde(skip)]
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,

    /// Join handle for the background save task, used to await completion on exit.
    #[serde(skip)]
    save_task_handle: Option<tokio::task::JoinHandle<()>>,

    /// Constants data fetched from the network before game data was loaded.
    /// Flushed to disk once we know the build number (in `DataLoaded`).
    #[serde(skip)]
    pending_constants_data: Option<Vec<u8>>,
}

impl Default for WowsToolkitApp {
    fn default() -> Self {
        Self {
            checked_for_updates: false,
            manual_update_requested: false,
            update_window_open: false,
            panic_info: None,
            panic_window_open: false,
            build_consent_window_open: false,
            language_selection_open: false,
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
            last_persisted_generation: 0,
            db_pool: None,
            shutdown_tx: None,
            save_task_handle: None,
            pending_constants_data: None,
        }
    }
}

impl WowsToolkitApp {
    /// Called once before the first frame.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // This is also where you can customize the look and feel of egui using
        // `cc.egui_ctx.set_visuals` and `cc.egui_ctx.set_fonts`.

        // Ensure the app data directory exists before anything tries to write to it.
        if let Some(dir) = crate::storage_dir() {
            let _ = std::fs::create_dir_all(&dir);
        }

        // Install the ring crypto provider for rustls before any networking happens.
        let _ = rustls::crypto::ring::default_provider().install_default();

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

        // Add system font fallbacks for CJK/Thai characters that egui's default
        // fonts don't cover.
        add_system_font_fallbacks(&mut fonts);

        // Register "GameFont" as a proportional fallback so game_font() never panics.
        // Upgraded to real game fonts once WoWs data is loaded.
        crate::replay::minimap_view::shapes::register_game_fonts(&mut fonts, None);

        cc.egui_ctx.set_fonts(fonts);
        cc.egui_ctx.set_theme(egui::Theme::Dark);

        // Open SQLite database for persisting app state.
        let default_state: Self = Default::default();
        let db_pool = match default_state.runtime.block_on(crate::db::open_db()) {
            Ok(pool) => Some(pool),
            Err(e) => {
                error!("Failed to open database: {e}");
                None
            }
        };

        // Load previous app state.
        //
        // Priority:
        // 1. SQLite (if migration has been completed)
        // 2. app.ron via eframe (legacy) — then migrate to SQLite
        // 3. Fresh defaults
        let mut had_saved_state = false;
        let mut state = if let Some(ref pool) = db_pool
            && default_state.runtime.block_on(crate::db::is_migrated(pool))
        {
            // Load from SQLite.
            info!("Loading app state from SQLite");
            let mut saved_state: Self = Default::default();
            if let Err(e) =
                saved_state.runtime.block_on(crate::db::load::load_tab_state_from_db(pool, &mut saved_state.tab_state))
            {
                error!("Failed to load state from SQLite: {e}");
            } else {
                had_saved_state = true;
            }
            saved_state
        } else if let Some(legacy_app) = load_from_app_ron() {
            // Legacy: loaded from app.ron on disk — convert to new structure.
            had_saved_state = true;

            let (persisted, player_tracker, sent_replays, replay_sort) = legacy_app.into_new_state();
            let mut saved_state: Self = Default::default();
            *saved_state.tab_state.persisted.write() = persisted;
            saved_state.tab_state.player_tracker = player_tracker;
            saved_state.tab_state.sent_replays = sent_replays;
            saved_state.tab_state.replay_sort = replay_sort;

            // Migrate converted data to SQLite.
            if let Some(ref pool) = db_pool {
                info!("Migrating app.ron data to SQLite...");
                if let Err(e) = saved_state
                    .runtime
                    .block_on(crate::db::migrate_ron::migrate_tab_state_to_db(pool, &saved_state.tab_state))
                {
                    error!("Failed to migrate app.ron to SQLite: {e}");
                }
            }

            // Rename app.ron → app.ron.migrated as a backup.
            if let Some(dir) = crate::storage_dir() {
                let ron_path = dir.join("app.ron");
                let migrated_path = dir.join("app.ron.migrated");
                if ron_path.exists() && !migrated_path.exists() {
                    if let Err(e) = std::fs::rename(&ron_path, &migrated_path) {
                        warn!("Failed to rename app.ron to app.ron.migrated: {e}");
                    } else {
                        info!("Renamed app.ron to app.ron.migrated");
                    }
                }
            }

            saved_state
        } else {
            warn!("Creating new default app settings");
            Default::default()
        };

        // Store the DB pool in the app state.
        state.db_pool = db_pool;

        if had_saved_state {
            {
                let mut p = state.tab_state.persisted.write();
                if !p.settings.game.has_052_game_params_fix {
                    p.settings.game.has_052_game_params_fix = true;
                    crate::util::game_params::clear_all_game_params_caches();
                }

                // Apply persisted armor viewer defaults to the initial pane
                // (ArmorViewerState is #[serde(skip)] so it gets Default on load)
                state.tab_state.armor_viewer.apply_defaults(&p.armor_viewer_defaults);

                // Sync the GPU encoder warning flag from persisted settings
                state
                    .tab_state
                    .suppress_gpu_encoder_warning
                    .store(p.settings.app.suppress_gpu_encoder_warning, std::sync::atomic::Ordering::Relaxed);

                // Ensure session stats are sorted correctly (backfills sort_key for legacy data)
                p.session_stats.sort_games();
            }

            let wows_dir = state.tab_state.persisted.read().settings.game.wows_dir.clone();
            if !wows_dir.is_empty() {
                let task = Some(state.tab_state.load_game_data(PathBuf::from(wows_dir)));
                update_background_task!(state.tab_state.background_tasks, task);
            }
        }

        if !had_saved_state {
            let detected = sys_locale::get_locale()
                .and_then(|sys| wt_translations::system_locale_to_wows(&sys).map(String::from))
                .unwrap_or_else(|| "en".into());
            state.tab_state.persisted.write().settings.app.locale = Some(detected);

            let default_wows_dir = "C:\\Games\\World_of_Warships";
            let default_wows_path = Path::new(default_wows_dir);
            if default_wows_path.exists() {
                state.tab_state.persisted.write().settings.game.wows_dir = default_wows_dir.to_string();

                let task = state.tab_state.load_game_data(default_wows_path.to_path_buf());
                update_background_task!(state.tab_state.background_tasks, Some(task));
            }
        }

        // Restore zoom factor from persisted settings.
        cc.egui_ctx.set_zoom_factor(state.tab_state.persisted.read().settings.app.zoom_factor);

        // Apply locale to rust-i18n
        if let Some(locale) = &state.tab_state.persisted.read().settings.app.locale {
            rust_i18n::set_locale(locale);
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

        {
            let p = state.tab_state.persisted.read();
            if !p.settings.app.build_consent_window_shown {
                state.build_consent_window_open = true;
            }

            // Show language selection dialog on first launch if a non-English locale was detected
            if !p.settings.app.language_selection_shown {
                let locale = p.settings.app.locale.as_deref().unwrap_or("en");
                if locale != "en" {
                    state.language_selection_open = true;
                } else {
                    drop(p);
                    // English detected or default — no need to ask
                    state.tab_state.persisted.write().settings.app.language_selection_shown = true;
                }
            }
        }

        // Initialize logging if the feature is enabled and the user hasn't disabled it
        #[cfg(feature = "logging")]
        if state.tab_state.persisted.read().settings.app.enable_logging {
            state._log_guard = Self::init_logging();
        }

        // Capture wgpu render state for 3D viewport rendering
        state.tab_state.wgpu_render_state = cc.wgpu_render_state.clone();

        // Share the tokio runtime and DB pool with tab_state for collab sessions and persistence.
        state.tab_state.tokio_runtime = Some(Arc::clone(&state.runtime));
        state.tab_state.db_pool = state.db_pool.clone();

        // Main window geometry is now restored via the ViewportBuilder in main.rs,
        // which is the only way to set window position. Size, fullscreen, and
        // maximized state are also applied there.

        // Load persisted cap layout cache.
        {
            let mut loaded = false;

            // Try SQLite first.
            if let Some(ref pool) = state.db_pool {
                let mut db = state.runtime.block_on(crate::data::cap_layout::CapLayoutDb::load_from_db(pool));
                if !db.is_empty() {
                    let removed = db.dedup();
                    if removed > 0 {
                        tracing::info!("removed {removed} duplicate cap layouts from SQLite");
                        let pool = pool.clone();
                        let _ = state.runtime.block_on(db.save_to_db(&pool));
                    }
                    *state.tab_state.cap_layout_db.lock() = db;
                    loaded = true;
                }
            }

            // Fall back to cap_layouts.bin file.
            if !loaded
                && let Some(cache_path) = crate::data::cap_layout::cache_path()
                && let Some(mut db) = crate::data::cap_layout::CapLayoutDb::load(&cache_path)
            {
                let removed = db.dedup();
                if removed > 0 {
                    tracing::info!("removed {removed} duplicate cap layouts from cache");
                    let _ = db.save(&cache_path);
                }
                tracing::info!("loaded {} cap layouts from cache", db.len());

                // Migrate file-based cap layouts to SQLite.
                if let Some(ref pool) = state.db_pool {
                    let pool = pool.clone();
                    if let Err(e) = state.runtime.block_on(db.save_to_db(&pool)) {
                        error!("Failed to migrate cap layouts to SQLite: {e}");
                    }
                }

                *state.tab_state.cap_layout_db.lock() = db;
            }
        }

        state.tab_state.revalidate_wows_dir();

        // Spawn the background save task (runs on a 30s timer, independent of painting).
        if let Some(ref pool) = state.db_pool {
            let save_ctx = crate::db::save::SaveContext {
                persisted: state.tab_state.persisted.clone(),
                player_tracker: state.tab_state.player_tracker.clone(),
                sent_replays: state.tab_state.sent_replays.clone(),
                replay_sort: state.tab_state.replay_sort.clone(),
                window_settings: state.tab_state.window_settings.clone(),
                active_viewports: state.tab_state.active_viewports.clone(),
                save_notify: state.tab_state.save_notify.clone(),
            };
            let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
            let handle = crate::db::save::spawn_save_task(
                &state.runtime,
                pool.clone(),
                save_ctx,
                cc.egui_ctx.clone(),
                shutdown_rx,
            );
            state.shutdown_tx = Some(shutdown_tx);
            state.save_task_handle = Some(handle);
        }

        let (tx, rx) = tokio::sync::mpsc::channel(1);
        state.tab_state.twitch_update_sender = Some(tx);
        state.begin_startup_tasks(rx);

        state
    }

    #[tracing::instrument(skip_all)]
    fn begin_startup_tasks(&mut self, token_rx: tokio::sync::mpsc::Receiver<crate::twitch::TwitchUpdate>) {
        use std::sync::Arc;

        // Start the networking thread
        let (network_job_tx, network_result_rx) = task::start_networking_thread();
        self.tab_state.network_job_tx = Some(network_job_tx);
        self.network_result_rx = Some(network_result_rx);

        let (twitch_channel, twitch_token) = {
            let p = self.tab_state.persisted.read();
            (p.settings.integrations.twitch_monitored_channel.clone(), p.settings.integrations.twitch_token.clone())
        };
        task::start_twitch_task(
            &self.runtime,
            Arc::clone(&self.tab_state.twitch_state),
            twitch_channel,
            twitch_token,
            token_rx,
        );

        #[cfg(feature = "mod_manager")]
        update_background_task!(self.tab_state.background_tasks, Some(crate::mod_manager::load_mods_db()));

        // Load PR expected values from disk if available
        let pr_path = crate::util::personal_rating::get_expected_values_path();
        if pr_path.exists() {
            if let Ok(pr_data) = std::fs::read(&pr_path) {
                update_background_task!(
                    self.tab_state.background_tasks,
                    Some(task::load_personal_rating_data(pr_data))
                );
            } else {
                tracing::error!("failed to read PR expected values file");
            }
        }
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

        if self.tab_state.persisted.read().settings.app.debug_mode {
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
                            ui.label(t!("ui.labels.loading_replays", count = pending_replay_count));
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

                        self.handle_task_completion(ui.ctx(), result);
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
                if ui.button(t!("ui.buttons.stop")).clicked() {
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
            current_commit: self.tab_state.persisted.read().settings.game.constants_file_commit.clone(),
        });
        if crate::util::personal_rating::needs_update() {
            self.tab_state.send_network_job(NetworkJob::FetchPersonalRatingData);
        }
        self.checked_for_updates = true;
    }

    /// Poll the networking thread for results and handle them.
    fn poll_network_results(&mut self) {
        let mut check_version_mismatch = false;

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
                    self.tab_state.toasts.lock().success(t!("ui.messages.app_up_to_date"));
                }
                NetworkResult::AppUpdateCheckFailed(msg) => {
                    warn!("App update check failed: {}", msg);
                    self.tab_state.toasts.lock().error(t!("ui.messages.update_check_failed"));
                }
                NetworkResult::ConstantsFetched { data, commit } => {
                    // Save under the current build number so the versioned system finds it.
                    // If game data hasn't loaded yet, stash for later (DataLoaded will flush it).
                    if let Some(wows_data) = &self.tab_state.world_of_warships_data {
                        let build = wows_data.read().build_number;
                        if let Some(storage_dir) = crate::storage_dir() {
                            let path = storage_dir.join(format!("constants_{build}.json"));
                            let _ = std::fs::write(path, data.as_slice());
                        }
                        // Rebuild loaded data with the new constants from disk.
                        if wows_data.write().rebuild_with_new_constants()
                            && let Some(replay_files) = &self.tab_state.replay_files
                        {
                            for replay in replay_files.values() {
                                replay.write().ui_report = None;
                            }
                        }
                    } else {
                        self.pending_constants_data = Some(data);
                    }
                    self.tab_state.persisted.write().settings.game.constants_file_commit = commit;
                    check_version_mismatch = true;
                }
                NetworkResult::ConstantsUpToDate => {}
                NetworkResult::ConstantsFetchFailed(msg) => {
                    warn!("Constants fetch failed: {}", msg);
                    if !self.constants_update_error_shown {
                        self.constants_update_error_shown = true;
                        self.tab_state.toasts.lock().error(t!("ui.messages.constants_fetch_failed")).duration(None);
                    }
                }
                NetworkResult::PersonalRatingDataFetched(data) => {
                    if crate::util::personal_rating::save_expected_values(&data).is_ok() {
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

        if check_version_mismatch {
            self.check_constants_version_mismatch();
        }
    }

    /// Handle a completed background task result.
    fn handle_task_completion(&mut self, ctx: &egui::Context, result: Result<BackgroundTaskCompletion, Report>) {
        match result {
            Ok(data) => match data {
                BackgroundTaskCompletion::NoReceiver => {}
                BackgroundTaskCompletion::DataLoaded { new_dir, wows_data, replays, available_builds } => {
                    let replays_dir = wows_data.replays_dir.clone();
                    let build_number = wows_data.build_number;

                    // Detect if the WoWs directory changed
                    let dir_changed =
                        self.tab_state.persisted.read().settings.game.wows_dir != new_dir.to_str().unwrap_or_default();

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

                    // Register real game fonts from VFS now that data is available.
                    {
                        let wdata = self.tab_state.world_of_warships_data.as_ref().unwrap().read();
                        let gf = self.tab_state.renderer_asset_cache.lock().get_or_load_game_fonts(&wdata.vfs);
                        let mut font_defs = ctx.fonts(|r| r.definitions().clone());
                        crate::replay::minimap_view::shapes::register_game_fonts(&mut font_defs, Some(&gf));
                        ctx.set_fonts(font_defs);
                    }

                    // Initialize or update the version data map.
                    // Always create a new map when the directory changed
                    // (reset_game_state sets wows_data_map to None).
                    let wows_data_ref = self.tab_state.world_of_warships_data.as_ref().unwrap();
                    if let Some(map) = &self.tab_state.wows_data_map {
                        map.insert(build_number, Arc::clone(wows_data_ref));
                    } else {
                        let mut map = crate::data::wows_data::WoWsDataMap::new(
                            PathBuf::from(&new_dir),
                            self.tab_state
                                .persisted
                                .read()
                                .settings
                                .app
                                .locale
                                .clone()
                                .unwrap_or_else(|| "en".to_string()),
                        );
                        if let Some(tx) = self.tab_state.network_job_tx.clone() {
                            map.set_network_job_tx(tx);
                        }
                        map.insert(build_number, Arc::clone(wows_data_ref));
                        self.tab_state.wows_data_map = Some(map);
                    }

                    // If the initial build used fallback constants, request the correct version
                    if !wows_data_ref.read().replay_constants_exact_match {
                        self.tab_state.send_network_job(NetworkJob::FetchVersionedConstants { build: build_number });
                    }

                    // Flush any constants data that arrived from the network before
                    // we knew the build number.
                    if let Some(data) = self.pending_constants_data.take()
                        && let Some(storage_dir) = crate::storage_dir()
                    {
                        let path = storage_dir.join(format!("constants_{build_number}.json"));
                        let _ = std::fs::write(path, &data);
                    }

                    self.tab_state.available_builds = available_builds;
                    self.tab_state.selected_browser_build = build_number;

                    self.tab_state.update_wows_dir(&new_dir, &replays_dir);
                    let no_replays = replays.as_ref().is_none_or(|r| r.is_empty());
                    self.tab_state.replay_files = replays;
                    self.tab_state.browser_state.reset_filters();

                    self.tab_state.toasts.lock().success(t!("ui.messages.game_data_loaded"));

                    if no_replays {
                        self.tab_state.toasts.lock().warning(t!("ui.messages.no_replays_detected"));
                    }

                    self.check_constants_version_mismatch();
                }
                BackgroundTaskCompletion::BuildDataLoaded { build } => {
                    self.tab_state.selected_browser_build = build;
                    self.tab_state.browser_state.reset_filters();
                    self.tab_state.toasts.lock().success(t!("ui.messages.build_loaded", build = build));
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
                    let open_tab =
                        matches!(source, ReplaySource::ManualOpen | ReplaySource::AutoLoad | ReplaySource::Reload);

                    if track_session_stats {
                        let replay_guard = replay.read();
                        if let Some(stat) = crate::data::session_stats::PerGameStat::from_replay(
                            &replay_guard,
                            &replay_guard.resource_loader,
                        ) {
                            self.tab_state.persisted.write().session_stats.add_game(stat);
                        }
                        drop(replay_guard);
                    }
                    if update_ui {
                        self.tab_state.replay_parser_tab.lock().game_chat.clear();
                        self.tab_state.player_tracker.write().update_from_replay(&replay.read());
                        if open_tab {
                            self.tab_state.open_replay_in_focused_tab(replay);
                        }
                        self.tab_state.toasts.lock().success(t!("ui.messages.replay_loaded"));
                        self.try_update_constants();
                    }
                }
                BackgroundTaskCompletion::UpdateDownloaded(new_exe) => {
                    let current_process = std::env::current_exe().expect("current process has no path?");
                    let mut current_process_new_path = current_process.as_os_str().to_owned();
                    current_process_new_path.push(".old");
                    let current_process_new_path = PathBuf::from(current_process_new_path);
                    let rename_process = move || {
                        std::fs::rename(current_process.clone(), &current_process_new_path)
                            .context("failed to rename current process")?;
                        std::fs::rename(new_exe, &current_process).context("failed to rename new process")?;

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
                    self.tab_state.player_tracker.write().filter_time_period =
                        crate::ui::player_tracker::TimePeriod::AllTime;
                }
                BackgroundTaskCompletion::PersonalRatingDataLoaded(pr_data) => {
                    self.tab_state.personal_rating_data.write().load(pr_data);
                }
                #[cfg(feature = "mod_manager")]
                BackgroundTaskCompletion::ModManager(mod_manager_info) => match *mod_manager_info {
                    crate::mod_manager::ModTaskCompletion::DatabaseLoaded(index) => {
                        self.tab_state.persisted.write().mod_manager_info.update_index("test".to_string(), index);
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
    }

    /// Draw replay renderer viewports, auto-wire collab sessions, and clean up closed renderers.
    fn sync_replay_renderers(&mut self, ctx: &egui::Context) {
        let mut replay_renderers = self.tab_state.replay_renderers.lock();
        let mut remove_renderers = Vec::new();
        for (idx, renderer) in replay_renderers.iter().enumerate() {
            if !renderer.open.load(Ordering::Relaxed) {
                // Keep hidden client renderers alive so they can be reopened
                // from the session popover without showing a loading spinner.
                let is_hidden_client = renderer.shared_state().lock().collab_replay_id.is_some()
                    && self.tab_state.client_session.is_some();
                if is_hidden_client {
                    continue; // Skip draw + settings sync for hidden viewers.
                }
                remove_renderers.push(idx);
                continue;
            }
            renderer.draw(ctx);
            // Check if renderer wants to save default options
            if let Some(saved) = renderer.pending_defaults_save.lock().take() {
                self.tab_state.persisted.write().settings.renderer = saved;
            }
            // Sync GPU warning suppress flag back to settings
            let suppress = renderer.suppress_gpu_warning.load(Ordering::Relaxed);
            if suppress != self.tab_state.persisted.read().settings.app.suppress_gpu_encoder_warning {
                self.tab_state.persisted.write().settings.app.suppress_gpu_encoder_warning = suppress;
            }

            // Auto-wire renderer to host session if active.
            if let Some(ref host_handle) = self.tab_state.host_session {
                let mut state = renderer.shared_state().lock();
                // Assign replay_id if not yet assigned.
                if state.collab_replay_id.is_none() {
                    let id = self.tab_state.next_replay_id;
                    self.tab_state.next_replay_id += 1;
                    state.collab_replay_id = Some(id);
                    state.session_frame_tx = Some(host_handle.frame_tx.clone());
                    state.collab_session_state = Some(std::sync::Arc::clone(&self.tab_state.session_state));
                    state.collab_local_tx = Some(host_handle.local_tx.clone());
                    state.collab_command_tx = Some(host_handle.command_tx.clone());
                    // Send the current frame (if any) so clients get it immediately.
                    if let Some(ref frame) = state.frame {
                        tracing::debug!("Auto-wire: first frame already available, broadcasting (replay_id={id})");
                        let _ = host_handle.frame_tx.try_send(crate::collab::peer::FrameBroadcast {
                            replay_id: id,
                            clock: frame.clock.0,
                            frame_index: frame.frame_index as u32,
                            total_frames: frame.total_frames as u32,
                            game_duration: frame.game_duration,
                            commands: frame.commands.clone(),
                        });
                    }
                }
                // ReplayOpened is normally sent by the background thread once
                // assets load. But if assets loaded before auto-wire set
                // collab_command_tx, the background thread missed its chance.
                // Handle that race here.
                if !state.session_announced
                    && state.assets.is_some()
                    && let Some(replay_id) = state.collab_replay_id
                {
                    let map_png = state
                        .assets
                        .as_ref()
                        .and_then(|a| {
                            a.map_image.as_ref().map(|img| {
                                let mut buf = Vec::new();
                                if let Some(image) = image::RgbaImage::from_raw(img.width, img.height, img.data.clone())
                                {
                                    let mut cursor = std::io::Cursor::new(&mut buf);
                                    let _ = image.write_to(&mut cursor, image::ImageFormat::Png);
                                }
                                buf
                            })
                        })
                        .unwrap_or_default();
                    let game_version = state.game_version.clone().unwrap_or_default();
                    let replay_name = state.collab_replay_name.clone().unwrap_or_else(|| {
                        renderer.title.strip_prefix("Replay Renderer - ").unwrap_or(&renderer.title).to_string()
                    });
                    let collab_map_name = state.collab_map_name.clone().unwrap_or_default();
                    let display_name =
                        translate_map_display_name(&collab_map_name, &self.tab_state.world_of_warships_data);
                    let _ = host_handle.command_tx.send(crate::collab::SessionCommand::ReplayOpened {
                        replay_id,
                        replay_name,
                        map_image_png: map_png,
                        game_version,
                        map_name: collab_map_name,
                        display_name,
                    });
                    state.session_announced = true;
                }
            }
        }

        // Send ReplayClosed for renderers being removed while a host session is active.
        // Also poison session_announced + collab_command_tx so the background playback
        // thread can't send a late ReplayOpened after the renderer is already gone.
        for &idx in &remove_renderers {
            let mut state = replay_renderers[idx].shared_state().lock();
            state.session_announced = true;
            state.collab_command_tx = None;
            if let Some(replay_id) = state.collab_replay_id
                && let Some(ref handle) = self.tab_state.host_session
            {
                let _ = handle.command_tx.send(crate::collab::SessionCommand::ReplayClosed { replay_id });
            }
        }

        *replay_renderers = replay_renderers
            .drain(..)
            .enumerate()
            .filter_map(|(idx, r)| if !remove_renderers.contains(&idx) { Some(r) } else { None })
            .collect();
    }

    fn sync_tactics_boards(&mut self, ctx: &egui::Context) {
        let is_host = self.tab_state.host_session.is_some();
        let is_client = self.tab_state.client_session.is_some();
        let mut boards = self.tab_state.tactics_boards.lock();

        // Auto-wire existing tactics boards to session when one starts.
        let session_handle = self.tab_state.host_session.as_ref().or(self.tab_state.client_session.as_ref());
        if let Some(handle) = session_handle {
            for board in boards.iter_mut() {
                if board.collab_local_tx.is_none() {
                    board.collab_local_tx = Some(handle.local_tx.clone());
                    board.collab_session_state = Some(std::sync::Arc::clone(&self.tab_state.session_state));
                    board.collab_command_tx = Some(handle.command_tx.clone());
                    if is_host {
                        board.is_session_board = true;
                        // Send current map + caps + annotations to peers so they can catch up.
                        let state = board.state_arc().lock();
                        if let Some((map_id, map_name)) = state.selected_map() {
                            let map_name = map_name.to_string();
                            let map_image_png = state
                                .map_image_raw()
                                .map(|img| {
                                    let mut buf = Vec::new();
                                    if let Some(image) =
                                        image::RgbaImage::from_raw(img.width, img.height, img.data.clone())
                                    {
                                        let mut cursor = std::io::Cursor::new(&mut buf);
                                        let _ = image.write_to(&mut cursor, image::ImageFormat::Png);
                                    }
                                    buf
                                })
                                .unwrap_or_default();
                            let map_info = state.map_info().cloned();
                            let wire_caps: Vec<crate::collab::protocol::WireCapPoint> = state
                                .cap_points()
                                .iter()
                                .map(|c| crate::collab::protocol::WireCapPoint {
                                    id: c.id,
                                    index: c.index as u32,
                                    world_x: c.world_x,
                                    world_z: c.world_z,
                                    radius: c.radius,
                                    team_id: c.team_id,
                                    frozen: c.frozen,
                                })
                                .collect();
                            drop(state);
                            let display_name =
                                translate_map_display_name(&map_name, &self.tab_state.world_of_warships_data);
                            let _ = handle.local_tx.send(crate::collab::peer::LocalEvent::TacticsMapOpened {
                                board_id: board.board_id,
                                owner_user_id: board.owner_user_id,
                                map_name,
                                display_name,
                                map_id,
                                map_image_png,
                                map_info,
                            });
                            let _ = handle.command_tx.send(crate::collab::SessionCommand::SyncCapPoints {
                                board_id: board.board_id,
                                cap_points: wire_caps,
                            });
                            // Push pre-existing annotations into the session.
                            let ann = board.annotation_state_arc().lock();
                            if !ann.annotations.is_empty() {
                                crate::replay::minimap_view::send_annotation_full_sync(
                                    &Some(handle.command_tx.clone()),
                                    &ann,
                                    Some(board.board_id),
                                );
                            }
                        }
                    }
                }
            }
        }

        // Promotion: when a peer becomes co-host, flip their local boards to session boards
        // and announce them so they become visible to everyone.
        if let Some(handle) = session_handle {
            let is_authority = {
                let s = self.tab_state.session_state.lock();
                s.role.is_host() || s.role.is_co_host()
            };
            if is_authority {
                for board in boards.iter_mut() {
                    if !board.is_session_board && board.collab_local_tx.is_some() {
                        board.is_session_board = true;
                        // Announce this board to the session.
                        let state = board.state_arc().lock();
                        if let Some((map_id, map_name)) = state.selected_map() {
                            let map_name = map_name.to_string();
                            let map_image_png = state
                                .map_image_raw()
                                .map(|img| {
                                    let mut buf = Vec::new();
                                    if let Some(image) =
                                        image::RgbaImage::from_raw(img.width, img.height, img.data.clone())
                                    {
                                        let mut cursor = std::io::Cursor::new(&mut buf);
                                        let _ = image.write_to(&mut cursor, image::ImageFormat::Png);
                                    }
                                    buf
                                })
                                .unwrap_or_default();
                            let map_info = state.map_info().cloned();
                            let wire_caps: Vec<crate::collab::protocol::WireCapPoint> = state
                                .cap_points()
                                .iter()
                                .map(|c| crate::collab::protocol::WireCapPoint {
                                    id: c.id,
                                    index: c.index as u32,
                                    world_x: c.world_x,
                                    world_z: c.world_z,
                                    radius: c.radius,
                                    team_id: c.team_id,
                                    frozen: c.frozen,
                                })
                                .collect();
                            drop(state);
                            let display_name =
                                translate_map_display_name(&map_name, &self.tab_state.world_of_warships_data);
                            let _ = handle.local_tx.send(crate::collab::peer::LocalEvent::TacticsMapOpened {
                                board_id: board.board_id,
                                owner_user_id: board.owner_user_id,
                                map_name,
                                display_name,
                                map_id,
                                map_image_png,
                                map_info,
                            });
                            let _ = handle.command_tx.send(crate::collab::SessionCommand::SyncCapPoints {
                                board_id: board.board_id,
                                cap_points: wire_caps,
                            });
                            let ann = board.annotation_state_arc().lock();
                            if !ann.annotations.is_empty() {
                                crate::replay::minimap_view::send_annotation_full_sync(
                                    &Some(handle.command_tx.clone()),
                                    &ann,
                                    Some(board.board_id),
                                );
                            }
                        }
                    }
                }
            }
        }

        // Peer-only: auto-open tactics boards that appear in session state but aren't
        // open locally.  Each board_id is tracked in `tactics_auto_opened_board_ids`
        // so we don't re-open after the user closes one.
        if is_client
            && !self.tab_state.persisted.read().settings.collab.disable_auto_open_session_windows
            && let Some(handle) = self.tab_state.client_session.as_ref()
            && let Some(ref wows_data) = self.tab_state.world_of_warships_data
        {
            let ss = self.tab_state.session_state.lock();
            let new_boards: Vec<(u64, u64)> = ss
                .tactics_boards
                .iter()
                .filter(|(bid, _)| {
                    !boards.iter().any(|b| b.board_id == **bid)
                        && !self.tab_state.tactics_auto_opened_board_ids.contains(bid)
                })
                .map(|(&bid, bs)| (bid, bs.owner_user_id))
                .collect();
            drop(ss);
            for (bid, owner) in new_boards {
                if boards.len() >= crate::collab::protocol::MAX_TACTICS_BOARDS {
                    break;
                }
                self.tab_state.tactics_auto_opened_board_ids.insert(bid);
                let mut board = crate::replay::minimap_view::tactics::TacticsBoardViewer::new(
                    bid,
                    owner,
                    std::sync::Arc::clone(&self.tab_state.cap_layout_db),
                    std::sync::Arc::clone(&self.tab_state.renderer_asset_cache),
                    std::sync::Arc::clone(wows_data),
                    self.tab_state.db_pool.clone(),
                    self.tab_state.tokio_runtime.clone(),
                    self.tab_state.window_settings.clone(),
                    self.tab_state.save_notify.clone(),
                );
                board.is_session_board = true;
                board.collab_local_tx = Some(handle.local_tx.clone());
                board.collab_session_state = Some(std::sync::Arc::clone(&self.tab_state.session_state));
                board.collab_command_tx = Some(handle.command_tx.clone());
                boards.push(board);
            }
        }

        // Drain force_open_window_ids — the host asked everyone to open these windows.
        // For tactics boards, force-open even if the user previously closed them.
        if let Some(handle) = self.tab_state.host_session.as_ref().or(self.tab_state.client_session.as_ref())
            && let Some(ref wows_data) = self.tab_state.world_of_warships_data
        {
            let mut ss = self.tab_state.session_state.lock();
            let force_ids: Vec<u64> = ss.force_open_window_ids.drain().collect();
            // Collect board info while we have the lock.
            let force_boards: Vec<(u64, u64)> = force_ids
                .iter()
                .filter_map(|id| ss.tactics_boards.get(id).map(|bs| (*id, bs.owner_user_id)))
                .filter(|(bid, _)| !boards.iter().any(|b| b.board_id == *bid))
                .collect();
            drop(ss);
            for (bid, owner) in force_boards {
                if boards.len() >= crate::collab::protocol::MAX_TACTICS_BOARDS {
                    break;
                }
                self.tab_state.tactics_auto_opened_board_ids.insert(bid);
                let mut board = crate::replay::minimap_view::tactics::TacticsBoardViewer::new(
                    bid,
                    owner,
                    std::sync::Arc::clone(&self.tab_state.cap_layout_db),
                    std::sync::Arc::clone(&self.tab_state.renderer_asset_cache),
                    std::sync::Arc::clone(wows_data),
                    self.tab_state.db_pool.clone(),
                    self.tab_state.tokio_runtime.clone(),
                    self.tab_state.window_settings.clone(),
                    self.tab_state.save_notify.clone(),
                );
                board.is_session_board = true;
                board.collab_local_tx = Some(handle.local_tx.clone());
                board.collab_session_state = Some(std::sync::Arc::clone(&self.tab_state.session_state));
                board.collab_command_tx = Some(handle.command_tx.clone());
                boards.push(board);
            }
        }

        // Peer-only: close local session boards whose board_id is no longer in session state.
        if is_client && !boards.is_empty() {
            let session = self.tab_state.session_state.lock();
            for board in boards.iter() {
                if board.is_session_board && !session.tactics_boards.contains_key(&board.board_id) {
                    board.open.store(false, Ordering::Relaxed);
                }
            }
        }

        let mut remove = Vec::new();
        for (idx, board) in boards.iter().enumerate() {
            if !board.open.load(Ordering::Relaxed) {
                remove.push(idx);
            } else {
                board.draw(ctx);
            }
        }
        if !remove.is_empty() {
            // Host/co-host closing a session board — clear annotations and notify peers per board.
            let close_handle = self.tab_state.host_session.as_ref().or(self.tab_state.client_session.as_ref());
            if let Some(handle) = close_handle {
                for &idx in &remove {
                    if boards[idx].is_session_board && boards[idx].collab_local_tx.is_some() {
                        let bid = boards[idx].board_id;
                        let _ = handle.local_tx.send(crate::collab::peer::LocalEvent::Annotation(
                            crate::collab::peer::LocalAnnotationEvent::Clear { board_id: Some(bid) },
                        ));
                        let _ =
                            handle.local_tx.send(crate::collab::peer::LocalEvent::TacticsMapClosed { board_id: bid });
                    }
                }
            }
            *boards = boards
                .drain(..)
                .enumerate()
                .filter_map(|(idx, b)| if !remove.contains(&idx) { Some(b) } else { None })
                .collect();
        }
    }

    /// Poll pending armor viewer requests from replay renderers and spawn viewers.
    fn poll_armor_viewer_requests(&mut self) {
        // Poll ship assets loading (so it works without the Armor Viewer tab open)
        if let crate::armor_viewer::state::ShipAssetsState::Loading(ref rx) = self.tab_state.armor_viewer.ship_assets
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
                                if !self.tab_state.armor_viewer.nation_flag_textures.contains_key(&nation_group.nation)
                                    && let Some(asset) = crate::task::load_nation_flag(&wd.vfs, &nation_group.nation)
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
                    self.tab_state.armor_viewer.ship_assets = crate::armor_viewer::state::ShipAssetsState::Failed(e);
                }
            }
        }

        let replay_renderers = self.tab_state.replay_renderers.lock();
        for renderer in replay_renderers.iter() {
            let mut state = renderer.shared_state().lock();
            let requests: Vec<crate::replay::renderer::ArmorViewerRequest> =
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
                        let viewer = crate::replay::realtime_armor_viewer::RealtimeArmorViewer::new(
                            player,
                            request.bridge.clone(),
                            ship_assets,
                            gpu_pipeline,
                            render_state,
                            Some(request.command_tx.clone()),
                            self.tab_state.window_settings.clone(),
                            self.tab_state.save_notify.clone(),
                        );
                        drop(bridge);
                        self.realtime_armor_viewers.push(Arc::new(parking_lot::Mutex::new(viewer)));
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
                        crate::util::thread::spawn_logged("load-ship-assets", move || {
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
                    TextStyle::Heading.resolve(&ctx.global_style()),
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
            self.tab_state.persisted.write().settings.game.current_replay_path = path.clone();
            update_background_task!(
                self.tab_state.background_tasks,
                deps.parse_replay_from_path(path.clone(), crate::task::ReplaySource::ManualOpen)
            );
        }
    }

    fn update_impl(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if mitigate_wgpu_mem_leak(ctx) {
            return;
        }

        // Update active viewport list for the background save task's window geometry capture.
        {
            use crate::tab_state::WindowKind;
            let mut viewports: Vec<(WindowKind, egui::ViewportId)> = Vec::new();

            for r in self.tab_state.replay_renderers.lock().iter() {
                viewports.push((WindowKind::ReplayRenderer, r.viewport_id()));
            }
            for t in self.tab_state.tactics_boards.lock().iter() {
                viewports.push((WindowKind::TacticsBoard, t.viewport_id()));
            }
            for v in &self.realtime_armor_viewers {
                viewports.push((WindowKind::ArmorViewer, v.lock().viewport_id()));
            }

            *self.tab_state.active_viewports.lock() = viewports;
        }

        // Register main window context so the peer task can wake us.
        {
            let mut s = self.tab_state.session_state.lock();
            if s.egui_ctx.is_none() {
                s.egui_ctx = Some(ctx.clone());
            }
        }
        // Draw realtime armor viewer windows
        self.realtime_armor_viewers.retain(|v| v.lock().open.load(Ordering::Relaxed));
        for viewer in &self.realtime_armor_viewers {
            crate::replay::realtime_armor_viewer::draw_realtime_armor_viewer(viewer, ctx);
        }

        if ctx
            .input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::CTRL | Modifiers::SHIFT, egui::Key::D)))
        {
            {
                let mut p = self.tab_state.persisted.write();
                p.settings.app.debug_mode = !p.settings.app.debug_mode;
            }
            let debug_mode = self.tab_state.persisted.read().settings.app.debug_mode;
            if let Some(sender) = self.tab_state.background_parser_tx.as_ref() {
                let _ = sender.send(ReplayBackgroundParserThreadMessage::DebugStateChange(debug_mode));
            }
        }

        self.tab_state.try_update_replays();

        // Pick up "Add to Session Stats" requests (no confirmation needed)
        if let Some(replays) = ctx.data_mut(|data| {
            data.remove_temp::<Vec<std::sync::Weak<parking_lot::RwLock<crate::ui::replay_parser::Replay>>>>(
                egui::Id::new("add_to_session_stats_request"),
            )
        }) {
            self.tab_state.clear_before_session_reset = false;
            self.tab_state.replays_for_session_reset = Some(replays);
        }

        self.tab_state.process_session_stats_reset();

        if self.manual_update_requested
            || (!self.checked_for_updates && self.tab_state.persisted.read().settings.app.check_for_updates)
        {
            self.manual_update_requested = false;
            self.request_update_checks();
        }

        self.poll_network_results();

        // Update settings_needs_attention based on cached WoWs directory validity and twitch token state
        {
            let twitch_token_failed = self.tab_state.persisted.read().settings.integrations.twitch_token.is_some()
                && self.tab_state.twitch_state.read().token_validation_failed;

            if twitch_token_failed && !self.shown_twitch_token_error {
                self.shown_twitch_token_error = true;
                error!("Twitch token is invalid or expired");
                self.tab_state.toasts.lock().error(t!("ui.messages.twitch_token_invalid"));
            } else if !twitch_token_failed {
                self.shown_twitch_token_error = false;
            }

            self.tab_state.settings_needs_attention = self.tab_state.wows_dir_invalid || twitch_token_failed;
        }

        if self.build_consent_window_open {
            egui::Window::new(t!("ui.windows.build_consent")).collapsible(false).show(ctx, |ui| {
                ui.label(t!("ui.dialogs.build_consent_message"));
                ui.horizontal(|ui| {
                    if ui.button(t!("ui.buttons.yes")).clicked() {
                        self.build_consent_window_open = false;
                        let mut p = self.tab_state.persisted.write();
                        p.settings.app.build_consent_window_shown = true;
                        p.settings.integrations.send_replay_data = true;
                        drop(p);
                        self.tab_state.send_replay_consent_changed();
                    }
                    if ui.button(t!("ui.buttons.no")).clicked() {
                        self.build_consent_window_open = false;
                        let mut p = self.tab_state.persisted.write();
                        p.settings.app.build_consent_window_shown = true;
                        p.settings.integrations.send_replay_data = false;
                        drop(p);
                        self.tab_state.send_replay_consent_changed();
                    }
                });
            });
        }

        if self.language_selection_open {
            let detected_locale =
                self.tab_state.persisted.read().settings.app.locale.clone().unwrap_or_else(|| "en".into());
            let native_name = wt_translations::language_name(&detected_locale).unwrap_or("English");

            egui::Window::new(t!("dialog.select_language"))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(t!("dialog.machine_translation_warning"));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        // "Continue in English" button
                        if ui.button(t!("dialog.continue_in_english")).clicked() {
                            let mut p = self.tab_state.persisted.write();
                            p.settings.app.locale = Some("en".into());
                            rust_i18n::set_locale("en");
                            p.settings.app.language_selection_shown = true;
                            drop(p);
                            self.language_selection_open = false;
                        }
                        // "Continue in <detected language>" button
                        let continue_label = t!("dialog.continue_in_language");
                        // For English TOML the label is the same, but for translated TOMLs
                        // it will be in the detected language. Show native name as fallback.
                        let label = if continue_label == "Continue in English" {
                            format!("Continue in {}", native_name)
                        } else {
                            continue_label.into()
                        };
                        if ui.button(label).clicked() {
                            // Keep the detected locale
                            self.tab_state.persisted.write().settings.app.language_selection_shown = true;
                            self.language_selection_open = false;
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
                egui::Window::new(t!("ui.windows.error")).open(&mut self.show_error_window).show(ctx, |ui| {
                    build_error_window(ui, error);
                });
            } else {
                self.error_to_show = None;
            }
        }

        if self.show_about_window {
            egui::Window::new(t!("ui.windows.about")).open(&mut self.show_about_window).show(ctx, |ui| {
                build_about_window(ui);
            });
        }

        // Panels are drawn in ui() via draw_panels()

        self.show_confirmation_dialog(ctx);
        self.show_ip_warning_dialog(ctx);
        if self.tab_state.pending_join && !self.tab_state.show_ip_warning {
            self.tab_state.pending_join = false;
            self.do_join_session();
        }
        if self.tab_state.pending_host && !self.tab_state.show_ip_warning {
            self.tab_state.pending_host = false;
            self.do_host_session();
        }
        self.poll_host_session_events();
        self.poll_client_session_events(ctx);

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

        self.sync_replay_renderers(ctx);
        self.sync_tactics_boards(ctx);

        self.poll_armor_viewer_requests();

        self.ui_file_drag_and_drop(ctx);

        self.tab_state.toasts.lock().show(ctx);

        // If persisted state was written to this frame, wake the background save task.
        {
            let current_gen = self.tab_state.persisted.generation();
            if current_gen != self.last_persisted_generation {
                self.last_persisted_generation = current_gen;
                self.tab_state.request_save();
            }
        }

        // When any replay renderer is playing locally, repaint continuously so
        // deferred viewports stay in sync. Client sessions are event-driven:
        // the peer task repaints registered viewports when state changes.
        let any_playing = self.tab_state.replay_renderers.lock().iter().any(|r| r.shared_state().lock().playing);
        if any_playing || !self.realtime_armor_viewers.is_empty() {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after_secs(1.0);
        }
    }

    fn show_panic_window(&mut self, ctx: &Context) {
        if let Some(panic_info) = self.panic_info.as_mut() {
            egui::Window::new(t!("ui.windows.crash_detected")).open(&mut self.panic_window_open).show(ctx, |ui| {
                ui.vertical(|ui| {
                    ui.label(t!("ui.dialogs.crash_message"));
                    ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                        ui.scope(|ui| {
                            let style = ui.style_mut();
                            style.override_text_style = Some(TextStyle::Monospace);
                            let widget = egui::TextEdit::multiline(panic_info).desired_width(f32::INFINITY);
                            ui.add_enabled(false, widget);
                        });
                    });
                    ui.horizontal(|ui| {
                        if ui.button(t!("ui.buttons.copy")).clicked() {
                            Context::copy_text(ctx, panic_info.clone());
                        }
                        if ui.button(wt_translations::icon_t(icons::GITHUB_LOGO, &t!("ui.buttons.github"))).clicked() {
                            ui.ctx().open_url(OpenUrl::new_tab(
                                "https://github.com/landaire/wows-toolkit/issues/new/choose",
                            ));
                        }
                        if ui.button(wt_translations::icon_t(icons::DISCORD_LOGO, &t!("ui.buttons.discord"))).clicked()
                        {
                            ui.ctx().open_url(OpenUrl::new_tab("https://discord.gg/SpmXzfSdux"));
                        }
                    });
                    ui.collapsing(t!("ui.buttons.more_options"), |ui| {
                        ui.label(t!("ui.dialogs.crash_clear_settings"));
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

                            if ui.button(t!("ui.buttons.clear_settings")).clicked() {
                                *self.tab_state.persisted.write() = Default::default();
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
                egui::Window::new(t!("ui.windows.update_available")).open(&mut self.update_window_open).show(
                    ctx,
                    |ui| {
                        ui.vertical(|ui| {
                            ui.label(t!("ui.dialogs.update_message", tag = tag));
                            if let Some(notes) = notes.as_mut() {
                                ScrollArea::vertical().max_height(500.0).show(ui, |ui| {
                                    CommonMarkViewer::new().show(ui, &mut self.tab_state.markdown_cache, notes);
                                });
                            }
                            ui.horizontal(|ui| {
                                #[cfg(target_os = "windows")]
                                {
                                    if ui.button(t!("ui.buttons.install_update")).clicked() {
                                        let task = Some(crate::task::start_download_update_task(&self.runtime, asset));
                                        update_background_task!(self.tab_state.background_tasks, task);
                                    }
                                }
                                #[cfg(not(target_os = "windows"))]
                                {
                                    let _ = asset;
                                    ui.label(t!("ui.dialogs.update_windows_only"));
                                }
                                if ui.button(t!("ui.buttons.view_release")).clicked() {
                                    ui.ctx().open_url(OpenUrl::new_tab(url));
                                }
                            });
                        });
                    },
                );
            } else {
                self.update_window_open = false;
            }
        }
    }

    pub fn panic_log_path() -> PathBuf {
        let mut panic_log_path = PathBuf::from("wows_toolkit_panic.log");
        if let Some(storage_dir) = crate::storage_dir() {
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
            current_commit: self.tab_state.persisted.read().settings.game.constants_file_commit.clone(),
        });
    }

    fn check_constants_version_mismatch(&mut self) {
        // Determine mismatch status under locks, then drop them before acting.
        // Read the version from the loaded WorldOfWarshipsData's replay constants
        // rather than a separate copy.
        let mismatch_status = {
            let Some(wows_data) = &self.tab_state.world_of_warships_data else { return };
            let wows_data = wows_data.read();
            let Some(full_version) = &wows_data.full_version else { return };

            let replay_constants = wows_data.replay_constants.read();
            let constants_version =
                replay_constants.get("VERSION").and_then(|v| v.get("VERSION")).and_then(|v| v.as_str());
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
                self.tab_state.toasts.lock().warning(t!("ui.messages.constants_version_mismatch")).duration(None);

                // The on-disk constants file is stale — delete it so the versioned
                // system doesn't treat it as an exact match, then request a fresh fetch.
                if let Some(wows_data) = &self.tab_state.world_of_warships_data {
                    let build = wows_data.read().build_number;
                    if let Some(storage_dir) = crate::storage_dir() {
                        let path = storage_dir.join(format!("constants_{build}.json"));
                        let _ = std::fs::remove_file(path);
                    }
                    // Mark as inexact so the fetch/rebuild path works
                    wows_data.write().replay_constants_exact_match = false;
                    self.tab_state.send_network_job(NetworkJob::FetchVersionedConstants { build });
                }
                // Also clear the saved commit so FetchLatestConstants re-downloads
                self.tab_state.persisted.write().settings.game.constants_file_commit = None;
                self.tab_state.send_network_job(NetworkJob::FetchLatestConstants { current_commit: None });
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

                    self.tab_state.toasts.lock().success(t!("ui.messages.constants_updated"));
                } else if !self.constants_update_error_shown {
                    self.constants_update_error_shown = true;
                    warn!("Failed to fetch versioned constants during rebuild");
                    self.tab_state
                        .toasts
                        .lock()
                        .error(t!("ui.messages.versioned_constants_rebuild_failed"))
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

    fn pick_up_confirmation_request(&mut self, ctx: &egui::Context) {
        if self.tab_state.pending_confirmation.is_none() {
            let request: Option<Option<crate::tab_state::ConfirmableAction>> =
                ctx.data_mut(|data| data.remove_temp(egui::Id::new("pending_confirmation_request")));
            if let Some(Some(action)) = request {
                self.tab_state.pending_confirmation = Some(action);
            }
        }
    }

    fn show_confirmation_dialog(&mut self, ctx: &egui::Context) {
        self.pick_up_confirmation_request(ctx);

        let Some(action) = self.tab_state.pending_confirmation.clone() else {
            return;
        };

        let message = action.confirmation_message();

        let mut confirmed = false;
        let mut dismissed = false;

        egui::Window::new(t!("ui.windows.confirm"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(message);
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button(t!("ui.buttons.yes")).clicked() {
                        confirmed = true;
                    }
                    if ui.button(t!("ui.buttons.no")).clicked() {
                        dismissed = true;
                    }
                });
            });

        if confirmed {
            let action = self.tab_state.pending_confirmation.take().unwrap();
            self.execute_confirmed_action(action, ctx);
        } else if dismissed {
            self.tab_state.pending_confirmation = None;
        }
    }

    fn execute_confirmed_action(&mut self, action: crate::tab_state::ConfirmableAction, ctx: &egui::Context) {
        match action {
            crate::tab_state::ConfirmableAction::OpenInGame { replay_path } => {
                let wows_dir = self.tab_state.persisted.read().settings.game.wows_dir.clone();
                let exe = std::path::Path::new(&wows_dir).join("WorldOfWarships.exe");
                let _ = std::process::Command::new(exe).arg(&replay_path).spawn();
                // Signal the replay parser to open the controls window
                ctx.data_mut(|data| {
                    data.insert_temp(egui::Id::new("open_replay_controls_window"), true);
                });
            }
            crate::tab_state::ConfirmableAction::ClearSessionStats => {
                self.tab_state.persisted.write().session_stats.clear();
            }
            crate::tab_state::ConfirmableAction::ClearShipSessionStats { ship_id } => {
                self.tab_state.persisted.write().session_stats.clear_ship(ship_id);
            }
            crate::tab_state::ConfirmableAction::SetAsSessionStats { replays } => {
                self.tab_state.clear_before_session_reset = true;
                self.tab_state.replays_for_session_reset = Some(replays);
            }
        }
    }

    fn show_ip_warning_dialog(&mut self, ctx: &egui::Context) {
        if !self.tab_state.show_ip_warning {
            return;
        }

        let mut proceed = false;
        let mut cancel = false;

        egui::Window::new(t!("ui.windows.network_warning"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(t!("ui.dialogs.p2p_warning"));
                ui.add_space(4.0);
                ui.hyperlink_to(t!("ui.labels.more_info"), "https://landaire.github.io/wows-toolkit/networking");
                ui.add_space(8.0);
                {
                    let mut p = self.tab_state.persisted.write();
                    ui.checkbox(&mut p.settings.collab.suppress_p2p_ip_warning, t!("ui.labels.suppress_warning"));
                }
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button(t!("ui.buttons.continue_")).clicked() {
                        proceed = true;
                    }
                    if ui.button(t!("ui.buttons.cancel")).clicked() {
                        cancel = true;
                    }
                });
            });

        if proceed {
            self.tab_state.show_ip_warning = false;
            // pending_join / pending_host were set before showing the dialog;
            // they will execute on the next frame now that the gate is lifted.
        }
        if cancel {
            self.tab_state.show_ip_warning = false;
            self.tab_state.pending_join = false;
            self.tab_state.pending_host = false;
        }
    }

    fn do_join_session(&mut self) {
        let params = crate::collab::peer::JoinParams {
            token: self.tab_state.join_session_token.trim().to_string(),
            display_name: self.tab_state.persisted.read().settings.collab.display_name.trim().to_string(),
            toolkit_version: env!("CARGO_PKG_VERSION").to_string(),
        };

        let state = Arc::clone(&self.tab_state.session_state);
        let handle = crate::collab::peer::start_peer_session(
            Arc::clone(&self.runtime),
            crate::collab::peer::PeerMode::Join(params),
            state,
        );

        self.tab_state.client_session = Some(handle);
        self.tab_state.join_session_token.clear();
    }

    fn do_host_session(&mut self) {
        let web_asset_bundle = Arc::new(parking_lot::Mutex::new(self.build_web_asset_bundle()));
        self.tab_state.web_asset_bundle = Some(Arc::clone(&web_asset_bundle));

        let params = crate::collab::peer::HostParams {
            toolkit_version: env!("CARGO_PKG_VERSION").to_string(),
            display_name: self.tab_state.persisted.read().settings.collab.display_name.clone(),
            initial_render_options: crate::collab::protocol::collab_render_options_from_saved(
                &crate::data::settings::SavedRenderOptions::default(),
            ),
            web_asset_bundle,
        };

        let session_state = Arc::clone(&self.tab_state.session_state);
        let handle = crate::collab::peer::start_peer_session(
            Arc::clone(&self.runtime),
            crate::collab::peer::PeerMode::Host(params),
            session_state,
        );

        self.tab_state.host_session = Some(handle);
    }

    /// Build a pre-serialized `PeerMessage::AssetBundle` for web clients.
    /// Returns `None` if game data isn't loaded yet.
    fn build_web_asset_bundle(&self) -> Option<Vec<u8>> {
        use crate::collab::protocol::GameFontsWire;
        use crate::collab::protocol::PeerMessage;
        use crate::collab::protocol::RgbaAssetWire;
        use crate::collab::protocol::frame_peer_message;

        let wows_data = self.tab_state.world_of_warships_data.as_ref()?;
        let wd = wows_data.read();
        let mut cache = self.tab_state.renderer_asset_cache.lock();

        let convert_icons = |icons: &std::collections::HashMap<String, crate::replay::renderer::RgbaAsset>| -> Vec<(String, RgbaAssetWire)> {
            icons.iter().map(|(k, a)| {
                (k.clone(), RgbaAssetWire { data: a.data.clone(), width: a.width, height: a.height })
            }).collect()
        };

        let ship_icons = convert_icons(&cache.get_or_load_ship_icons(&wd.vfs));
        let plane_icons = convert_icons(&cache.get_or_load_plane_icons(&wd.vfs));
        let consumable_icons = convert_icons(&cache.get_or_load_consumable_icons(&wd.vfs));
        let death_cause_icons = convert_icons(&cache.get_or_load_death_cause_icons(&wd.vfs));
        let powerup_icons = convert_icons(&cache.get_or_load_powerup_icons(&wd.vfs));

        let fonts = cache.get_or_load_game_fonts(&wd.vfs);
        let game_fonts = Some(GameFontsWire {
            primary: fonts.primary_bytes.clone(),
            fallback_ko: fonts.fallback_bytes.first().cloned(),
            fallback_ja: fonts.fallback_bytes.get(1).cloned(),
            fallback_zh: fonts.fallback_bytes.get(2).cloned(),
        });

        let msg = PeerMessage::AssetBundle {
            ship_icons,
            plane_icons,
            consumable_icons,
            death_cause_icons,
            powerup_icons,
            game_fonts,
        };

        match frame_peer_message(&msg) {
            Ok(bytes) => Some(bytes),
            Err(e) => {
                tracing::warn!("Failed to serialize AssetBundle: {e}");
                None
            }
        }
    }

    fn poll_host_session_events(&mut self) {
        let Some(ref session) = self.tab_state.host_session else {
            return;
        };

        // Lazily build the asset bundle once game data becomes available.
        if let Some(ref bundle_slot) = self.tab_state.web_asset_bundle
            && bundle_slot.lock().is_none()
            && let Some(bundle) = self.build_web_asset_bundle()
        {
            *bundle_slot.lock() = Some(bundle);
        }

        let mut session_ended = false;
        while let Ok(event) = session.event_rx.try_recv() {
            match event {
                crate::collab::SessionEvent::Started => {
                    self.tab_state.toasts.lock().info(t!("ui.messages.session_started"));
                }
                crate::collab::SessionEvent::UserJoined(user) => {
                    self.tab_state.toasts.lock().info(t!("ui.messages.user_joined", name = &user.name));
                }
                crate::collab::SessionEvent::UserLeft { name, timed_out, .. } => {
                    if timed_out {
                        self.tab_state.toasts.lock().warning(t!("ui.messages.user_timeout", name = name));
                    } else {
                        self.tab_state.toasts.lock().info(t!("ui.messages.user_left", name = name));
                    }
                }
                crate::collab::SessionEvent::Ended => {
                    self.tab_state.toasts.lock().info(t!("ui.messages.session_ended"));
                    session_ended = true;
                }
                crate::collab::SessionEvent::Error(msg) => {
                    self.tab_state.toasts.lock().error(t!("ui.messages.session_error", msg = msg));
                    session_ended = true;
                }
                _ => {}
            }
        }

        if session_ended {
            // Unwire all renderers and reset their applied sync versions.
            for r in self.tab_state.replay_renderers.lock().iter() {
                let mut s = r.shared_state().lock();
                s.session_frame_tx = None;
                s.collab_replay_id = None;
                s.session_announced = false;
                s.collab_session_state = None;
                s.collab_local_tx = None;
                s.applied_render_options_version = 0;
                s.applied_annotation_sync_version = 0;
                s.applied_range_override_version = 0;
                s.applied_trail_override_version = 0;
            }
            // Unwire all tactics boards and reset their applied sync versions.
            for b in self.tab_state.tactics_boards.lock().iter_mut() {
                b.collab_local_tx = None;
                b.collab_session_state = None;
                b.collab_command_tx = None;
                b.state_arc().lock().reset_applied_sync_versions();
            }
            self.tab_state.host_session = None;
            self.tab_state.web_asset_bundle = None;
            self.tab_state.session_state.lock().clear_session_data();
        }
    }

    fn cleanup_client_session(&mut self) {
        // Remove hidden client renderers (kept alive for quick reopen)
        // and unwire visible ones.
        let mut renderers = self.tab_state.replay_renderers.lock();
        renderers.retain(|r| {
            let is_hidden_client =
                !r.open.load(Ordering::Relaxed) && r.shared_state().lock().collab_replay_id.is_some();
            !is_hidden_client
        });
        for r in renderers.iter() {
            let mut s = r.shared_state().lock();
            s.session_frame_tx = None;
            s.collab_replay_id = None;
            s.session_announced = false;
            s.collab_session_state = None;
            s.collab_local_tx = None;
            s.applied_render_options_version = 0;
            s.applied_annotation_sync_version = 0;
            s.applied_range_override_version = 0;
            s.applied_trail_override_version = 0;
        }
        drop(renderers);
        // Unwire tactics boards and reset applied sync versions.
        for b in self.tab_state.tactics_boards.lock().iter_mut() {
            b.collab_local_tx = None;
            b.collab_session_state = None;
            b.collab_command_tx = None;
            b.state_arc().lock().reset_applied_sync_versions();
        }
        self.tab_state.client_session = None;
        self.tab_state.session_state.lock().clear_session_data();
    }

    fn poll_client_session_events(&mut self, ctx: &egui::Context) {
        let Some(ref session) = self.tab_state.client_session else {
            return;
        };

        // Poll events
        while let Ok(event) = session.event_rx.try_recv() {
            match event {
                crate::collab::SessionEvent::Started => {
                    self.tab_state.toasts.lock().info(t!("ui.messages.connected_to_session"));
                }
                crate::collab::SessionEvent::SessionInfoReceived { open_replays } => {
                    tracing::debug!("SessionInfoReceived: {} open replay(s)", open_replays.len());
                    // Launch client viewer windows for each open replay (up to 2).
                    let saved_options = self.tab_state.persisted.read().settings.renderer.clone();
                    let suppress = Arc::clone(&self.tab_state.suppress_gpu_encoder_warning);
                    for replay in open_replays.into_iter().take(2) {
                        self.tab_state.toasts.lock().info(t!("ui.messages.joined_session", name = &replay.replay_name));
                        let viewer = crate::replay::renderer::launch_client_renderer(
                            replay.replay_name,
                            replay.map_image_png,
                            replay.game_version,
                            &saved_options,
                            Arc::clone(&suppress),
                            self.tab_state.world_of_warships_data.as_ref(),
                            &self.tab_state.renderer_asset_cache,
                            self.tab_state.window_settings.clone(),
                            self.tab_state.save_notify.clone(),
                        );
                        if let Some(ref client_handle) = self.tab_state.client_session {
                            let (frame_tx, frame_rx) = std::sync::mpsc::sync_channel(2);
                            let viewport_id = egui::ViewportId::from_hash_of(&*viewer.title);
                            let mut state = viewer.shared_state().lock();
                            state.collab_replay_id = Some(replay.replay_id);
                            state.collab_session_state = Some(std::sync::Arc::clone(&self.tab_state.session_state));
                            state.collab_local_tx = Some(client_handle.local_tx.clone());
                            state.collab_frame_rx = Some(frame_rx);
                            self.tab_state.session_state.lock().register_viewport_sink(
                                replay.replay_id,
                                crate::collab::ViewportSink { frame_tx: Some(frame_tx), viewport_id },
                            );
                        }
                        self.tab_state.replay_renderers.lock().push(viewer);
                    }
                }
                crate::collab::SessionEvent::ReplayOpened {
                    replay_id,
                    replay_name,
                    map_image_png,
                    game_version,
                    ..
                } => {
                    // Spam protection: track timestamps of ReplayOpened events.
                    let now = std::time::Instant::now();
                    self.tab_state.replay_open_timestamps.push_back(now);
                    while self
                        .tab_state
                        .replay_open_timestamps
                        .front()
                        .is_some_and(|t| now.duration_since(*t).as_secs() >= 10)
                    {
                        self.tab_state.replay_open_timestamps.pop_front();
                    }
                    if self.tab_state.replay_open_timestamps.len() >= 5 {
                        self.tab_state.toasts.lock().error(t!("ui.messages.replay_spam_protection"));
                        if let Some(ref handle) = self.tab_state.client_session {
                            let _ = handle.command_tx.send(crate::collab::SessionCommand::Stop);
                        }
                        self.tab_state.client_session = None;
                        self.tab_state.replay_open_timestamps.clear();
                        return;
                    }

                    // Cap at 2 client viewer windows — close oldest if needed.
                    let mut renderers = self.tab_state.replay_renderers.lock();
                    let client_count =
                        renderers.iter().filter(|r| r.shared_state().lock().collab_replay_id.is_some()).count();
                    if client_count >= 2 {
                        // Close the oldest client viewer.
                        if let Some(pos) =
                            renderers.iter().position(|r| r.shared_state().lock().collab_replay_id.is_some())
                        {
                            renderers[pos].open.store(false, Ordering::Relaxed);
                            renderers.remove(pos);
                        }
                    }
                    drop(renderers);

                    let saved_options = self.tab_state.persisted.read().settings.renderer.clone();
                    let suppress = Arc::clone(&self.tab_state.suppress_gpu_encoder_warning);
                    self.tab_state.toasts.lock().info(t!("ui.messages.host_opened_replay", name = replay_name));
                    let viewer = crate::replay::renderer::launch_client_renderer(
                        replay_name,
                        map_image_png,
                        game_version,
                        &saved_options,
                        suppress,
                        self.tab_state.world_of_warships_data.as_ref(),
                        &self.tab_state.renderer_asset_cache,
                        self.tab_state.window_settings.clone(),
                        self.tab_state.save_notify.clone(),
                    );
                    // Wire the client viewer to the session.
                    if let Some(ref client_handle) = self.tab_state.client_session {
                        let (frame_tx, frame_rx) = std::sync::mpsc::sync_channel(2);
                        let viewport_id = egui::ViewportId::from_hash_of(&*viewer.title);
                        let mut state = viewer.shared_state().lock();
                        state.collab_replay_id = Some(replay_id);
                        state.collab_session_state = Some(std::sync::Arc::clone(&self.tab_state.session_state));
                        state.collab_local_tx = Some(client_handle.local_tx.clone());
                        state.collab_frame_rx = Some(frame_rx);
                        self.tab_state.session_state.lock().register_viewport_sink(
                            replay_id,
                            crate::collab::ViewportSink { frame_tx: Some(frame_tx), viewport_id },
                        );
                    }
                    self.tab_state.replay_renderers.lock().push(viewer);
                }
                crate::collab::SessionEvent::ReplayClosed { replay_id } => {
                    // Close the matching client viewer.
                    let mut renderers = self.tab_state.replay_renderers.lock();
                    if let Some(pos) =
                        renderers.iter().position(|r| r.shared_state().lock().collab_replay_id == Some(replay_id))
                    {
                        renderers[pos].open.store(false, Ordering::Relaxed);
                        renderers.remove(pos);
                    }
                    self.tab_state.session_state.lock().viewport_sinks.remove(&replay_id);
                    self.tab_state.toasts.lock().info(t!("ui.messages.host_closed_replay"));
                }
                crate::collab::SessionEvent::Error(msg) => {
                    self.tab_state.toasts.lock().error(t!("ui.messages.session_error_generic", msg = msg));
                    self.cleanup_client_session();
                    return;
                }
                crate::collab::SessionEvent::Rejected(reason) => {
                    self.tab_state.toasts.lock().error(t!("ui.messages.session_rejected", reason = reason));
                    self.tab_state.client_session = None;
                    return;
                }
                crate::collab::SessionEvent::Ended => {
                    self.tab_state.toasts.lock().info(t!("ui.messages.session_ended"));
                    self.cleanup_client_session();
                    return;
                }
                _ => {}
            }
        }

        // Request repaint while session is active to keep polling events
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
}

impl eframe::App for WowsToolkitApp {
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.update_impl(ctx, frame);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        egui::Panel::top("top_panel").show_inside(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                let is_web = cfg!(target_arch = "wasm32");
                if !is_web {
                    ui.menu_button(t!("ui.menu.file"), |ui| {
                        if ui.button(t!("ui.menu.check_updates")).clicked() {
                            self.manual_update_requested = true;
                            ui.close_kind(UiKind::Menu);
                        }
                        if ui.button(t!("ui.menu.about")).clicked() {
                            self.show_about_window = true;
                            ui.close_kind(UiKind::Menu);
                        }
                        if ui.button(t!("ui.menu.quit")).clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                    ui.add_space(16.0);
                }

                if ui.button(wt_translations::icon_t(icons::BUG, &t!("ui.buttons.create_issue"))).clicked() {
                    ui.ctx().open_url(OpenUrl::new_tab("https://github.com/landaire/wows-toolkit/issues/new/choose"));
                }

                if ui.button(wt_translations::icon_t(icons::DISCORD_LOGO, &t!("ui.buttons.discord"))).clicked() {
                    ui.ctx().open_url(OpenUrl::new_tab("https://discord.gg/SpmXzfSdux"));
                }
            });
        });

        egui::Panel::bottom("status_panel").show_inside(ui, |ui| {
            self.build_bottom_panel(ui);
        });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            DockArea::new(&mut self.dock_state)
                .style(Style::from_egui(ui.style().as_ref()))
                .allowed_splits(egui_dock::AllowedSplits::None)
                .show_leaf_collapse_buttons(false)
                .show_leaf_close_all_buttons(false)
                .show_close_buttons(false)
                .show_inside(ui, &mut ToolkitTabViewer { tab_state: &mut self.tab_state });
        });
    }

    fn on_exit(&mut self) {
        // Signal the background save task to do a final save, then await completion.
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.save_task_handle.take() {
            self.runtime.block_on(async {
                match tokio::time::timeout(std::time::Duration::from_secs(5), handle).await {
                    Ok(Ok(())) => info!("Final save completed"),
                    Ok(Err(e)) => error!("Save task panicked: {e}"),
                    Err(_) => error!("Final save timed out after 5 seconds"),
                }
            });
        }
    }
}

/// Load app state from the legacy `app.ron` file on disk.
///
/// The file is a RON-serialized `HashMap<String, String>` (eframe's key-value
/// storage). The app state lives under the `"app"` key as a nested RON string.
///
/// Returns a `LegacyWowsToolkitApp` which must be converted via
/// [`into_new_state()`](crate::data::legacy_settings::LegacyWowsToolkitApp::into_new_state).
fn load_from_app_ron() -> Option<crate::data::legacy_settings::LegacyWowsToolkitApp> {
    let dir = crate::storage_dir()?;
    let ron_path = dir.join("app.ron");
    let contents = std::fs::read_to_string(&ron_path).ok()?;
    let kv: std::collections::HashMap<String, String> = ron::from_str(&contents).ok()?;
    let app_str = kv.get("app")?;
    if app_str.is_empty() {
        return None;
    }
    match ron::from_str::<crate::data::legacy_settings::LegacyWowsToolkitApp>(app_str) {
        Ok(app) => {
            info!("Loaded legacy app state from {}", ron_path.display());
            Some(app)
        }
        Err(e) => {
            error!("Failed to deserialize app.ron: {e}");
            None
        }
    }
}

/// Translate a map name to a human-readable display name using game metadata.
///
/// Falls back to a prettified version of the raw name if game data is unavailable.
fn translate_map_display_name(map_name: &str, wows_data: &Option<crate::data::wows_data::SharedWoWsData>) -> String {
    if let Some(wd) = wows_data {
        let wd = wd.read();
        if let Some(ref gm) = wd.game_metadata {
            return wowsunpack::game_params::translations::translate_map_name(map_name, gm.as_ref());
        }
    }
    // Fallback: strip "spaces/" prefix and leading number prefix, replace underscores.
    let bare = map_name.strip_prefix("spaces/").unwrap_or(map_name);
    let stripped = bare.find('_').map(|i| &bare[i + 1..]).unwrap_or(bare);
    stripped.replace('_', " ")
}

fn build_about_window(ui: &mut egui::Ui) {
    ui.vertical(|ui| {
        ui.label(t!("ui.labels.made_by"));
        ui.label(t!("ui.labels.credits"));
        ui.horizontal(|ui| {
            ui.label(t!("ui.labels.pr_credits"));
            ui.hyperlink_to(t!("ui.labels.more_info"), "https://wows-numbers.com/personal/rating");
        });
        if ui.button(t!("ui.buttons.view_github")).clicked() {
            ui.ctx().open_url(OpenUrl::new_tab("https://github.com/landaire/wows-toolkit"));
        }

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.label(t!("ui.labels.powered_by"));
            ui.hyperlink_to("egui", "https://github.com/emilk/egui");
            ui.label(t!("ui.labels.and"));
            ui.hyperlink_to("eframe", "https://github.com/emilk/egui/tree/master/crates/eframe");
            ui.label(".");
        });
    });
}

fn build_error_window(ui: &mut egui::Ui, error: &str) {
    ui.vertical(|ui| {
        ui.label(wt_translations::icon_t(icons::WARNING, &t!("ui.labels.error_occurred")));
        ui.label(error);
    });
}

/// Helper function to mitigate https://github.com/emilk/egui/issues/7434.
///
/// Load system fonts that cover scripts egui's built-in fonts lack (CJK, Thai,
/// Cyrillic, etc.) and append them as low-priority fallbacks in the Proportional
/// family. Fonts that don't exist on the current system are silently skipped.
#[cfg(not(target_arch = "wasm32"))]
fn add_system_font_fallbacks(fonts: &mut egui::FontDefinitions) {
    // (logical name, file path) — tried in order per platform.
    #[cfg(target_os = "windows")]
    let candidates: &[(&str, &str)] = &[
        ("sys_cjk_sc", r"C:\Windows\Fonts\msyh.ttc"), // Microsoft YaHei — Simplified Chinese + Latin
        ("sys_cjk_tc", r"C:\Windows\Fonts\msjh.ttc"), // Microsoft JhengHei — Traditional Chinese
        ("sys_cjk_jp", r"C:\Windows\Fonts\YuGothR.ttc"), // Yu Gothic — Japanese
        ("sys_cjk_kr", r"C:\Windows\Fonts\malgun.ttf"), // Malgun Gothic — Korean
        ("sys_thai", r"C:\Windows\Fonts\leelawui.ttf"), // Leelawadee UI — Thai
    ];

    #[cfg(target_os = "macos")]
    let candidates: &[(&str, &str)] = &[
        ("sys_cjk_sc", "/System/Library/Fonts/PingFang.ttc"), // PingFang — CJK
        ("sys_cjk_jp", "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc"), // Hiragino Sans
        ("sys_thai", "/System/Library/Fonts/Supplemental/Ayuthaya.ttf"), // Thai
    ];

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let candidates: &[(&str, &str)] = &[
        ("sys_cjk", "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc"),
        ("sys_cjk", "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc"),
        ("sys_cjk", "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc"),
        ("sys_thai", "/usr/share/fonts/truetype/noto/NotoSansThai-Regular.ttf"),
        ("sys_thai", "/usr/share/fonts/noto/NotoSansThai-Regular.ttf"),
    ];

    for (name, path) in candidates {
        // Skip if we already loaded a font under this logical name (e.g. multiple
        // candidate paths for the same script on Linux).
        if fonts.font_data.contains_key(*name) {
            continue;
        }
        if let Ok(data) = std::fs::read(path) {
            fonts.font_data.insert(name.to_string(), egui::FontData::from_owned(data).into());
            if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                family.push(name.to_string());
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn add_system_font_fallbacks(_fonts: &mut egui::FontDefinitions) {
    // No filesystem access on WASM — nothing to load.
}

/// If this returns true, the app should early return in the `update()` function
/// or call `wgpu::Device::poll()`
pub fn mitigate_wgpu_mem_leak(ctx: &egui::Context) -> bool {
    let mut is_minimized = false;
    ctx.input(|reader| {
        is_minimized = reader.viewport().minimized.unwrap_or_default();
    });

    is_minimized
}
