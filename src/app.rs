use std::env;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::TryRecvError;
use std::time::Duration;
use std::time::Instant;

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
use egui_dock::TabViewer;

use http_body_util::BodyExt;
use octocrab::models::repos::Release;
use octocrab::params::repos::Reference;
use rootcause::Report;
use rootcause::hooks::builtin_hooks::report_formatter::DefaultReportFormatter;
use rootcause::prelude::ResultExt;
use tracing::trace;

use serde::Deserialize;
use serde::Serialize;

use tokio::runtime::Runtime;
use wows_replays::analyzer::battle_controller::GameMessage;

use crate::error::ToolkitError;
use crate::game_params::game_params_bin_path;
use crate::icons;
use crate::tab_state::TabState;
use crate::task::BackgroundTaskCompletion;
use crate::task::BackgroundTaskKind;
use crate::task::ReplayBackgroundParserThreadMessage;
use crate::task::{self};
use crate::ui::file_unpacker::UNPACKER_STOP;
use crate::wows_data::parse_replay;

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
}

impl Tab {
    fn title(&self) -> String {
        match self {
            Tab::Unpacker => format!("{} Resource Unpacker", icons::ARCHIVE),
            Tab::Settings => format!("{} Settings", icons::GEAR_FINE),
            Tab::ReplayParser => format!("{} Replay Inspector", icons::MAGNIFYING_GLASS),
            Tab::PlayerTracker => format!("{} Player Tracker", icons::DETECTIVE),
            Tab::ModManager => format!("{} Mod Manager", icons::WRENCH),
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
        }
    }
}

#[derive(Default)]
pub struct ReplayParserTabState {
    pub game_chat: Vec<GameMessage>,
}

#[derive(Clone)]
pub struct TimedMessage {
    pub message: String,
    pub expiration: Instant,
}

impl TimedMessage {
    pub fn new(message: String) -> Self {
        TimedMessage { message, expiration: Instant::now() + Duration::from_secs(10) }
    }

    pub fn is_expired(&self) -> bool {
        self.expiration < Instant::now()
    }
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
            dock_state: DockState::new([Tab::ReplayParser, Tab::PlayerTracker, Tab::Unpacker, Tab::Settings].to_vec()),
            show_error_window: false,
            error_to_show: None,
            runtime: Arc::new(Runtime::new().expect("failed to create tokio runtime")),
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
                had_saved_state = true;
                // if the app key is present and we get no result back, that means deserialization
                // failed and we should panic because this is an app bug -- likely caused by
                // not setting a default value for a persisted field
                eframe::get_value(storage, eframe::APP_KEY).expect("could not deserialize app state")
            } else {
                Default::default()
            };

            if !saved_state.tab_state.settings.has_default_value_fix_015 {
                saved_state.tab_state.settings.check_for_updates = true;
                saved_state.tab_state.settings.send_replay_data = false;
                saved_state.tab_state.settings.has_default_value_fix_015 = true;
            }

            if !saved_state.tab_state.settings.has_019_game_params_update {
                saved_state.tab_state.settings.has_019_game_params_update = true;

                // Remove the old game params
                let _ = std::fs::remove_file(game_params_bin_path());
            }

            if !saved_state.tab_state.settings.has_037_crew_skills_fix {
                saved_state.tab_state.settings.has_037_crew_skills_fix = true;

                // Remove the old game params
                let _ = std::fs::remove_file(game_params_bin_path());
            }

            if !saved_state.tab_state.settings.has_038_game_params_fix {
                saved_state.tab_state.settings.has_038_game_params_fix = true;

                // Remove the old game params
                let _ = std::fs::remove_file(game_params_bin_path());
            }

            // Added the Achievements to GameParams
            if !saved_state.tab_state.settings.has_041_game_params_fix {
                saved_state.tab_state.settings.has_041_game_params_fix = true;

                // Remove the old game params
                let _ = std::fs::remove_file(game_params_bin_path());
            }

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

        let (tx, rx) = tokio::sync::mpsc::channel(1);
        state.tab_state.twitch_update_sender = Some(tx);
        task::begin_startup_tasks(&mut state, rx);

        state
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

            for i in 0..self.tab_state.background_tasks.len() {
                let task = &mut self.tab_state.background_tasks[i];

                let remove_task = {
                    let desc = task.build_description(ui);
                    trace!("Task description: {:?}", desc);
                    if let Some(result) = desc {
                        match &task.kind {
                            BackgroundTaskKind::LoadingData => {
                                self.tab_state.allow_changing_wows_dir();
                            }
                            BackgroundTaskKind::LoadingReplay => {}
                            BackgroundTaskKind::Updating { rx: _rx, last_progress: _last_progress } => {}
                            BackgroundTaskKind::PopulatePlayerInspectorFromReplays => {}
                            BackgroundTaskKind::LoadingConstants => {}
                            #[cfg(feature = "mod_manager")]
                            BackgroundTaskKind::ModTask(_task_info) => {}
                            BackgroundTaskKind::LoadingPersonalRatingData => {}
                            BackgroundTaskKind::UpdateTimedMessage(timed_message) => {
                                self.tab_state.timed_message.write().replace(timed_message.clone());
                            }
                            BackgroundTaskKind::OpenFileViewer(plaintext_file_viewer) => {
                                self.tab_state.file_viewer.lock().push(plaintext_file_viewer.clone());
                            }
                        }

                        match result {
                            Ok(data) => match data {
                                BackgroundTaskCompletion::NoReceiver => {}
                                BackgroundTaskCompletion::DataLoaded { new_dir, wows_data, replays } => {
                                    let replays_dir = wows_data.replays_dir.clone();
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
                                    self.tab_state.update_wows_dir(&new_dir, &replays_dir);
                                    self.tab_state.replay_files = replays;
                                    self.tab_state.filtered_file_list = None;
                                    self.tab_state.used_filter = None;

                                    *self.tab_state.timed_message.write() = Some(TimedMessage::new(format!(
                                        "{} Successfully loaded game data",
                                        icons::CHECK_CIRCLE
                                    )));
                                }
                                BackgroundTaskCompletion::ReplayLoaded { replay, skip_ui_update } => {
                                    if !skip_ui_update {
                                        {
                                            self.tab_state.replay_parser_tab.lock().game_chat.clear();
                                        }
                                        {
                                            self.tab_state
                                                .settings
                                                .player_tracker
                                                .write()
                                                .update_from_replay(&replay.read());
                                        }
                                        self.tab_state.session_stats.add_replay(replay.clone());
                                        self.tab_state.current_replay = Some(replay);
                                        *self.tab_state.timed_message.write() = Some(TimedMessage::new(format!(
                                            "{} Successfully loaded replay",
                                            icons::CHECK_CIRCLE
                                        )));
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
                                            self.show_err_window(e.into());
                                        }
                                    }
                                }
                                BackgroundTaskCompletion::PopulatePlayerInspectorFromReplays => {}
                                BackgroundTaskCompletion::ConstantsLoaded(constants) => {
                                    *self.tab_state.game_constants.write() = constants;
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
                                        *self.tab_state.timed_message.write() = Some(TimedMessage::new(format!(
                                            "{} Successfully installed mod: {}",
                                            icons::CHECK_CIRCLE,
                                            mod_info.meta.name()
                                        )));
                                    }
                                    crate::mod_manager::ModTaskCompletion::ModUninstalled(mod_info) => {
                                        *self.tab_state.timed_message.write() = Some(TimedMessage::new(format!(
                                            "{} Successfully uninstalled mod: {}",
                                            icons::CHECK_CIRCLE,
                                            mod_info.meta.name()
                                        )));
                                    }
                                    crate::mod_manager::ModTaskCompletion::ModDownloaded(_) => {}
                                },
                            },
                            Err(e)
                                if e.downcast_current_context::<ToolkitError>()
                                    .is_some_and(|e| matches!(e, ToolkitError::BackgroundTaskCompleted)) => {}
                            Err(e) => {
                                eprintln!("Background task error: {e:?}");
                                self.show_err_window(e);
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
            } else {
                let reset_message = if let Some(timed_message) = &*self.tab_state.timed_message.read() {
                    if !timed_message.is_expired() {
                        ui.label(timed_message.message.as_str());
                        false
                    } else {
                        true
                    }
                } else {
                    false
                };

                if reset_message {
                    *self.tab_state.timed_message.write() = None;
                }
            }
        });
    }

    fn check_for_updates(&mut self) {
        use http_body::Body;

        let current_constants_commit = &self.tab_state.settings.constants_file_commit;

        let (app_updates, constants_updates) = self.runtime.block_on(async {
            let octocrab = octocrab::instance();
            let app_updates = octocrab.repos("landaire", "wows-toolkit").releases().get_latest().await;

            let latest_commit = octocrab
                .repos("padtrack", "wows-constants")
                .list_commits()
                .per_page(1)
                .send()
                .await
                .ok()
                .and_then(|mut list| list.take_items().pop())
                .map(|commit| commit.sha);

            if current_constants_commit == &latest_commit || latest_commit.is_none() {
                return (app_updates, None);
            }

            if let Ok(constants_updates) = octocrab
                .repos("padtrack", "wows-constants")
                .raw_file(Reference::Branch("main".to_string()), "data/latest.json")
                .await
            {
                let mut body = constants_updates.into_body();
                let mut result = Vec::with_capacity(body.size_hint().exact().unwrap_or_default() as usize);

                while let Some(frame) = body.frame().await {
                    match frame {
                        Ok(frame) => {
                            if let Some(data) = frame.data_ref() {
                                result.extend_from_slice(data);
                            }
                        }
                        Err(_) => return (app_updates, None),
                    }
                }

                (app_updates, Some((result, latest_commit)))
            } else {
                (app_updates, None)
            }
        });

        if let Ok(latest_release) = app_updates
            && let Ok(version) = semver::Version::parse(&latest_release.tag_name[1..])
        {
            let app_version = semver::Version::parse(env!("CARGO_PKG_VERSION")).unwrap();
            if app_version < version {
                self.update_window_open = true;
                self.latest_release = Some(latest_release);
            } else {
                *self.tab_state.timed_message.write() =
                    Some(TimedMessage::new(format!("{} Application up-to-date", icons::CHECK_CIRCLE)));
            }
        }

        if let Some((constants_updates, latest_commit)) = constants_updates {
            let mut constants_path = PathBuf::from("constants.json");
            if let Some(storage_dir) = eframe::storage_dir(crate::APP_NAME) {
                constants_path = storage_dir.join(constants_path)
            }

            if std::fs::write(constants_path, constants_updates.as_slice()).is_ok() {
                self.tab_state.settings.constants_file_commit = latest_commit;
                update_background_task!(self.tab_state.background_tasks, Some(task::load_constants(constants_updates)));
            }
        }

        // Check and update PR expected values
        if crate::personal_rating::needs_update() {
            if let Ok(pr_data) = self.runtime.block_on(crate::personal_rating::fetch_expected_values()) {
                if crate::personal_rating::save_expected_values(&pr_data).is_ok() {
                    update_background_task!(
                        self.tab_state.background_tasks,
                        Some(task::load_personal_rating_data(pr_data))
                    );
                }
            }
        }

        self.checked_for_updates = true;
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
            && let Some(wows_data) = self.tab_state.world_of_warships_data.as_ref()
        {
            self.tab_state.settings.current_replay_path = path.clone();
            update_background_task!(
                self.tab_state.background_tasks,
                parse_replay(
                    Arc::clone(&self.tab_state.game_constants),
                    Arc::clone(wows_data),
                    self.tab_state.settings.current_replay_path.clone(),
                    Arc::clone(&self.tab_state.replay_sort),
                    self.tab_state.background_task_sender.clone(),
                    self.tab_state.settings.debug_mode
                )
            );
        }
    }

    fn update_impl(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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
            self.check_for_updates();
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

                if ui.button(format!("{} Create Issue", icons::BUG)).clicked() {
                    ui.ctx().open_url(OpenUrl::new_tab("https://github.com/landaire/wows-toolkit/issues/new/choose"));
                }

                if ui.button(format!("{} Discord", icons::DISCORD_LOGO)).clicked() {
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

        self.ui_file_drag_and_drop(ctx);

        ctx.request_repaint_after_secs(1.0);
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
                        if ui.button(format!("{} GitHub", icons::GITHUB_LOGO)).clicked() {
                            ui.ctx().open_url(OpenUrl::new_tab(
                                "https://github.com/landaire/wows-toolkit/issues/new/choose",
                            ));
                        }
                        if ui.button(format!("{} Discord", icons::DISCORD_LOGO)).clicked() {
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
        ui.label(format!("{} An error occurred:", icons::WARNING));
        ui.label(error);
    });
}
