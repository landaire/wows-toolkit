use core::f32;
use std::collections::HashMap;
use std::collections::HashSet;
use std::env;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::Sender;
use std::sync::mpsc::TryRecvError;
use std::sync::mpsc::{
    self,
};
use std::time::Duration;
use std::time::Instant;

use clipboard::ClipboardContext;
use clipboard::ClipboardProvider;
use eframe::APP_KEY;
use egui::Color32;
use egui::Context;
use egui::KeyboardShortcut;
use egui::Modifiers;
use egui::OpenUrl;
use egui::RichText;
use egui::ScrollArea;
use egui::Slider;
use egui::TextStyle;
use egui::Ui;
use egui::UiKind;
use egui::WidgetText;
use egui::mutex::Mutex;
use egui_commonmark::CommonMarkCache;
use egui_commonmark::CommonMarkViewer;
use egui_dock::DockArea;
use egui_dock::DockState;
use egui_dock::Style;
use egui_dock::TabViewer;
use gettext::Catalog;

use http_body_util::BodyExt;
use notify::EventKind;
use notify::RecommendedWatcher;
use notify::RecursiveMode;
use notify::Watcher;
use notify::event::ModifyKind;
use notify::event::RenameMode;
use octocrab::models::repos::Release;
use octocrab::params::repos::Reference;
use parking_lot::RwLock;
use rootcause::Report;
use rootcause::hooks::builtin_hooks::report_formatter::DefaultReportFormatter;
use rootcause::prelude::ResultExt;
use tracing::debug;
use tracing::trace;

use serde::Deserialize;
use serde::Serialize;

use tokio::runtime::Runtime;
use wows_replays::ReplayFile;
use wows_replays::analyzer::battle_controller::GameMessage;
use wowsunpack::data::idx::FileNode;

use crate::error::ToolkitError;
use crate::game_params::game_params_bin_path;
use crate::icons;
use crate::plaintext_viewer::PlaintextFileViewer;
use crate::task::BackgroundParserThread;
use crate::task::BackgroundTask;
use crate::task::BackgroundTaskCompletion;
use crate::task::BackgroundTaskKind;
use crate::task::DataExportSettings;
use crate::task::ReplayBackgroundParserThreadMessage;
use crate::task::ReplayExportFormat;
use crate::task::{
    self,
};
use crate::twitch::Token;
use crate::twitch::TwitchState;
use crate::ui::file_unpacker::UNPACKER_STOP;
use crate::ui::file_unpacker::UnpackerProgress;
use crate::ui::mod_manager::ModInfo;
use crate::ui::mod_manager::ModManagerInfo;
use crate::ui::player_tracker::PlayerTracker;
use crate::ui::replay_parser::Replay;
use crate::ui::replay_parser::SharedReplayParserTabState;
use crate::ui::replay_parser::{
    self,
};
use crate::wows_data::WorldOfWarshipsData;
use crate::wows_data::load_replay;
use crate::wows_data::parse_replay;

const DEFAULT_ZOOM_FACTOR: f32 = 1.15;

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

impl ToolkitTabViewer<'_> {
    fn build_settings_tab(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            ui.label("Application Settings");
            ui.group(|ui| {
                ui.checkbox(&mut self.tab_state.settings.check_for_updates, "Check for Updates on Startup");
                if ui.checkbox(&mut self.tab_state.settings.send_replay_data, "Send Builds from Ranked and Random Battles Replays to ShipBuilds.com").changed() {
                    self.tab_state.send_replay_consent_changed();
                }
                ui.horizontal(|ui| {
                    let mut zoom = ui.ctx().zoom_factor();
                    if ui.add(Slider::new(&mut zoom, 0.5..=2.0).text("Zoom Factor (Ctrl + and Ctrl - also changes this)")).changed() {
                        ui.ctx().set_zoom_factor(zoom);
                    }
                    if ui.button("Reset").clicked() {
                        ui.ctx().set_zoom_factor(DEFAULT_ZOOM_FACTOR);
                    }
                });
            });
            ui.label("World of Warships Settings");
            ui.group(|ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add_enabled(self.tab_state.can_change_wows_dir, egui::Button::new("Choose...")).clicked() {
                                let folder = rfd::FileDialog::new().pick_folder();
                                if let Some(folder) = folder {
                                    self.tab_state.prevent_changing_wows_dir();
                                    crate::update_background_task!(self.tab_state.background_tasks, Some(self.tab_state.load_game_data(folder)));
                                }
                            }

                            let show_text_error = {
                                let path = Path::new(&self.tab_state.settings.wows_dir);
                                !(path.exists() && path.join("bin").exists())
                            };

                            let response = ui.add_sized(
                                ui.available_size(),
                                egui::TextEdit::singleline(&mut self.tab_state.settings.wows_dir)
                                    .interactive(self.tab_state.can_change_wows_dir)
                                    .hint_text("World of Warships Directory")
                                    .text_color_opt(show_text_error.then_some(Color32::LIGHT_RED)),
                            );

                            // If someone pastes a path in, let's do some basic validation to see if this
                            // can be a WoWs path. If so, reload game data.
                            if response.changed() {
                                let path = Path::new(&self.tab_state.settings.wows_dir).to_owned();
                                if path.exists() && path.join("bin").exists() {
                                    self.tab_state.prevent_changing_wows_dir();
                                    crate::update_background_task!(self.tab_state.background_tasks, Some(self.tab_state.load_game_data(path)));
                                }
                            }
                        });
                    });
                })
            });
            ui.label("Replay Settings");
            ui.group(|ui| {
                ui.checkbox(&mut self.tab_state.settings.replay_settings.show_game_chat, "Show Game Chat");
                ui.checkbox(&mut self.tab_state.settings.replay_settings.show_raw_xp, "Show Raw XP Column");
                ui.checkbox(&mut self.tab_state.settings.replay_settings.show_entity_id, "Show Entity ID Column");
                ui.checkbox(&mut self.tab_state.settings.replay_settings.show_observed_damage, "Show Observed Damage Column");
                ui.checkbox(&mut self.tab_state.settings.replay_settings.show_fires, "Show Fires Column");
                ui.checkbox(&mut self.tab_state.settings.replay_settings.show_floods, "Show Floods Column");
                ui.checkbox(&mut self.tab_state.settings.replay_settings.show_citadels, "Show Citadels Column");
                ui.checkbox(&mut self.tab_state.settings.replay_settings.show_crits, "Show Critical Module Hits Column");
                ui.horizontal(|ui| {
                    let mut alert_data_export_change = false;
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Choose...").clicked() {
                            let folder = rfd::FileDialog::new().pick_folder();
                            if let Some(folder) = folder {
                                self.tab_state.settings.replay_settings.auto_export_path = folder.to_string_lossy().to_string();
                                alert_data_export_change = true;
                            }
                        }

                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            if ui.checkbox(&mut self.tab_state.settings.replay_settings.auto_export_data, "Auto-Export Data").changed() {
                                alert_data_export_change = true;
                            }

                            let selected_format = &mut self.tab_state.settings.replay_settings.auto_export_format;
                            let previously_selected_format = *selected_format;
                            egui::ComboBox::from_id_salt("auto_export_format_combobox").selected_text(selected_format.as_str()).show_ui(ui, |ui| {
                                ui.selectable_value(selected_format, ReplayExportFormat::Json, "JSON");
                                ui.selectable_value(selected_format, ReplayExportFormat::Csv, "CSV");
                                ui.selectable_value(selected_format, ReplayExportFormat::Cbor, "CBOR");
                            });
                            if previously_selected_format != *selected_format {
                                alert_data_export_change = true;
                            }
                            let path = Path::new(&self.tab_state.settings.replay_settings.auto_export_path);
                            let path_is_valid = path.exists() && path.is_dir();
                            let response = ui.add_sized(
                                ui.available_size(),
                                egui::TextEdit::singleline(&mut self.tab_state.settings.replay_settings.auto_export_path)
                                    .hint_text("Data auto-export directory")
                                    .text_color_opt((!path_is_valid).then_some(Color32::LIGHT_RED)),
                            );

                            if response.lost_focus() {
                                let path = Path::new(&self.tab_state.settings.replay_settings.auto_export_path);
                                if path.exists() && path.is_dir() {
                                    alert_data_export_change = true;
                                }
                            }
                        });
                    });

                    if alert_data_export_change {
                        let _ = self.tab_state.background_parser_tx.as_ref().map(|tx| {
                            tx.send(ReplayBackgroundParserThreadMessage::DataAutoExportSettingChange(DataExportSettings {
                                should_auto_export: self.tab_state.settings.replay_settings.auto_export_data,
                                export_path: PathBuf::from(self.tab_state.settings.replay_settings.auto_export_path.clone()),
                                export_format: self.tab_state.settings.replay_settings.auto_export_format,
                            }))
                        });
                    }
                });
            });
            ui.label("Twitch Settings");
            ui.group(|ui| {
                if ui
                    .button(format!("{} Get Login Token", icons::BROWSER))
                    .on_hover_text(
                        "We use Chatterino's login page as it provides a token with the \
                        necessary permissions (basically a moderator token with chat permissions), \
                        and it removes the need for the WoWs Toolkit developer to host their own login page website which would have the same result.",
                    )
                    .clicked()
                {
                    ui.ctx().open_url(OpenUrl::new_tab("https://chatterino.com/client_login"));
                }

                let text = if self.tab_state.twitch_state.read().token_is_valid() {
                    format!("{} Paste Token (Current Token is Valid {})", icons::CLIPBOARD_TEXT, icons::CHECK_CIRCLE)
                } else {
                    format!("{} Paste Token (No Current Token / Invalid Token {})", icons::CLIPBOARD_TEXT, icons::X_CIRCLE)
                };
                if ui.button(text).clicked() {
                    let mut ctx: ClipboardContext = ClipboardProvider::new().unwrap();
                    if let Ok(contents) = ctx.get_contents() {
                        let token: Result<Token, _> = contents.parse();
                        if let Ok(token) = token
                            && let Some(tx) = self.tab_state.twitch_update_sender.as_ref()
                        {
                            self.tab_state.settings.twitch_token = Some(token.clone());
                            let _ = tx.blocking_send(crate::twitch::TwitchUpdate::Token(token));
                        }
                    }
                }
                ui.label("Monitored Channel (Default to Self)");
                let response = ui.text_edit_singleline(&mut self.tab_state.settings.twitch_monitored_channel);
                if response.lost_focus()
                    && let Some(tx) = self.tab_state.twitch_update_sender.as_ref()
                {
                    let _ = tx.blocking_send(crate::twitch::TwitchUpdate::User(self.tab_state.settings.twitch_monitored_channel.clone()));
                }
            });
        });
    }
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReplayGrouping {
    #[default]
    Date,
    Ship,
    None,
}

impl ReplayGrouping {
    pub fn label(&self) -> &'static str {
        match self {
            ReplayGrouping::Date => "Date",
            ReplayGrouping::Ship => "Ship",
            ReplayGrouping::None => "None",
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct ReplaySettings {
    pub show_game_chat: bool,
    pub show_entity_id: bool,
    pub show_observed_damage: bool,
    #[serde(default)]
    pub show_raw_xp: bool,
    #[serde(default = "default_bool::<true>")]
    pub show_fires: bool,
    #[serde(default = "default_bool::<true>")]
    pub show_floods: bool,
    #[serde(default = "default_bool::<true>")]
    pub show_citadels: bool,
    #[serde(default = "default_bool::<false>")]
    pub show_crits: bool,
    #[serde(default = "default_bool::<true>")]
    pub show_received_damage: bool,
    #[serde(default = "default_bool::<true>")]
    pub show_distance_traveled: bool,
    #[serde(default = "default_bool::<false>")]
    pub auto_export_data: bool,
    #[serde(default)]
    pub auto_export_path: String,
    #[serde(default)]
    pub auto_export_format: ReplayExportFormat,
    #[serde(default)]
    pub grouping: ReplayGrouping,
}

impl Default for ReplaySettings {
    fn default() -> Self {
        Self {
            show_game_chat: true,
            show_entity_id: false,
            show_observed_damage: false,
            show_raw_xp: false,
            show_fires: true,
            show_floods: true,
            show_citadels: true,
            show_crits: false,
            show_received_damage: true,
            show_distance_traveled: true,
            auto_export_data: false,
            auto_export_path: String::new(),
            auto_export_format: ReplayExportFormat::default(),
            grouping: ReplayGrouping::default(),
        }
    }
}

pub const fn default_bool<const V: bool>() -> bool {
    V
}

pub fn default_sent_replays() -> Arc<RwLock<HashSet<String>>> {
    Default::default()
}

#[derive(Serialize, Deserialize)]
pub struct Settings {
    pub current_replay_path: PathBuf,
    pub wows_dir: String,
    #[serde(skip)]
    pub replays_dir: Option<PathBuf>,
    pub locale: Option<String>,
    #[serde(default)]
    pub replay_settings: ReplaySettings,
    #[serde(default = "default_bool::<true>")]
    pub check_for_updates: bool,
    #[serde(default = "default_bool::<false>")]
    pub send_replay_data: bool,
    #[serde(default = "default_bool::<false>")]
    pub has_default_value_fix_015: bool,
    #[serde(default = "default_sent_replays")]
    pub sent_replays: Arc<RwLock<HashSet<String>>>,
    #[serde(default = "default_bool::<false>")]
    pub has_019_game_params_update: bool,
    #[serde(default = "default_bool::<false>")]
    pub has_037_crew_skills_fix: bool,
    #[serde(default = "default_bool::<false>")]
    pub has_038_game_params_fix: bool,
    #[serde(default)]
    pub player_tracker: Arc<RwLock<PlayerTracker>>,
    #[serde(default)]
    pub twitch_token: Option<Token>,
    #[serde(default)]
    pub twitch_monitored_channel: String,
    #[serde(default)]
    pub constants_file_commit: Option<String>,
    #[serde(default)]
    pub debug_mode: bool,
    #[serde(default)]
    pub build_consent_window_shown: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            current_replay_path: Default::default(),
            wows_dir: "C:\\Games\\World_of_Warships".to_string(),
            replays_dir: Default::default(),
            locale: Some("en".to_string()),
            replay_settings: Default::default(),
            check_for_updates: true,
            send_replay_data: false,
            has_default_value_fix_015: true,
            sent_replays: Default::default(),
            has_019_game_params_update: true,
            player_tracker: Default::default(),
            twitch_token: Default::default(),
            twitch_monitored_channel: Default::default(),
            constants_file_commit: None,
            debug_mode: false,
            build_consent_window_shown: false,
            has_037_crew_skills_fix: true,
            has_038_game_params_fix: true,
        }
    }
}

#[derive(Default)]
pub struct ReplayParserTabState {
    pub game_chat: Vec<GameMessage>,
}

#[derive(Debug)]
pub enum NotifyFileEvent {
    Added(PathBuf),
    Modified(PathBuf),
    Removed(PathBuf),
    PreferencesChanged,
    TempArenaInfoCreated(PathBuf),
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

type PathFileNodePair = (Arc<PathBuf>, FileNode);

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
    pub translations: Option<Catalog>,

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
    pub timed_message: RwLock<Option<TimedMessage>>,

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
    pub markdown_cache: CommonMarkCache,

    #[serde(default)]
    pub replay_sort: Arc<parking_lot::Mutex<replay_parser::SortOrder>>,

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
}

impl Default for TabState {
    fn default() -> Self {
        let default_constants = serde_json::from_str(include_str!("../embedded_resources/constants.json")).expect("failed to parse constants JSON");
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
        }
    }
}

impl TabState {
    fn send_replay_consent_changed(&self) {
        let _ = self.background_parser_tx.as_ref().map(|tx| tx.send(ReplayBackgroundParserThreadMessage::ShouldSendReplaysToServer(self.settings.send_replay_data)));
    }
    fn try_update_replays(&mut self) {
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

                                        if self.auto_load_latest_replay
                                            && let Some(wows_data) = self.world_of_warships_data.as_ref()
                                        {
                                            update_background_task!(
                                                self.background_tasks,
                                                load_replay(
                                                    Arc::clone(&self.game_constants),
                                                    Arc::clone(wows_data),
                                                    replay,
                                                    Arc::clone(&self.replay_sort),
                                                    self.background_task_sender.clone(),
                                                    self.settings.debug_mode,
                                                )
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
                            let mut replay = replay.write();
                            replay.battle_report = None;
                            replay.ui_report = None;
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

                        if let Ok(replay_file) = ReplayFile::from_decrypted_parts(meta_data.unwrap(), Vec::with_capacity(0)) {
                            self.settings.player_tracker.write().update_from_live_arena_info(&replay_file.meta);
                        }
                    }
                }
            }
        }
    }

    fn prevent_changing_wows_dir(&mut self) {
        self.can_change_wows_dir = false;
    }

    fn allow_changing_wows_dir(&mut self) {
        self.can_change_wows_dir = true;
    }

    fn update_wows_dir(&mut self, wows_dir: &Path, replay_dir: &Path) {
        let watcher = if let Some(watcher) = self.file_watcher.as_mut() {
            let old_replays_dir = self.settings.replays_dir.as_ref().expect("watcher was created but replay dir was not assigned?");
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
                task::start_background_parsing_thread(background_thread_data);
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
                                        tx.send(NotifyFileEvent::Added(path.clone())).expect("failed to send file creation event");
                                        // Send this path to the thread watching for replays in background
                                        let _ = background_tx.send(task::ReplayBackgroundParserThreadMessage::NewReplay(path));
                                    } else if path.file_name().expect("path has no file name") == "tempArenaInfo.json" {
                                        tx.send(NotifyFileEvent::TempArenaInfoCreated(path.clone())).expect("failed to send file creation event");
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
                                    tx.send(NotifyFileEvent::PreferencesChanged).expect("failed to send file creation event");
                                }
                                if path.extension().map(|ext| ext == "wowsreplay").unwrap_or(false) {
                                    tx.send(NotifyFileEvent::Modified(path.clone())).expect("failed to send file modification event");
                                    let _ = background_tx.send(task::ReplayBackgroundParserThreadMessage::ModifiedReplay(path));
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
            let _ = tx.send(task::load_wows_files(wows_directory, locale.as_str()));
        });

        BackgroundTask { receiver: Some(rx), kind: BackgroundTaskKind::LoadingData }
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
            dock_state: DockState::new([Tab::ReplayParser, Tab::PlayerTracker, Tab::Unpacker, Tab::Settings].to_vec()), //, Tab::ModManager, Tab::Settings].to_vec()),
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

            if !saved_state.tab_state.settings.wows_dir.is_empty() {
                let task = Some(saved_state.tab_state.load_game_data(PathBuf::from(saved_state.tab_state.settings.wows_dir.clone())));
                update_background_task!(saved_state.tab_state.background_tasks, task);
            }

            saved_state
        } else {
            Default::default()
        };

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
            // TODO: Merge these channels
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
                            BackgroundTaskKind::LoadingReplay => {
                                // nothing to do
                            }
                            BackgroundTaskKind::Updating { rx: _rx, last_progress: _last_progress } => {
                                // do nothing
                            }
                            BackgroundTaskKind::PopulatePlayerInspectorFromReplays => {
                                // do nothing
                            }
                            BackgroundTaskKind::LoadingConstants => {
                                // do nothing
                            }
                            #[cfg(feature = "mod_manager")]
                            BackgroundTaskKind::ModTask(_task_info) => {
                                // do nothing
                            }
                            BackgroundTaskKind::UpdateTimedMessage(timed_message) => {
                                self.tab_state.timed_message.write().replace(timed_message.clone());
                            }
                            BackgroundTaskKind::OpenFileViewer(plaintext_file_viewer) => {
                                self.tab_state.file_viewer.lock().push(plaintext_file_viewer.clone());
                            }
                        }

                        match result {
                            Ok(data) => match data {
                                BackgroundTaskCompletion::NoReceiver => {
                                    // do nothing
                                }
                                BackgroundTaskCompletion::DataLoaded { new_dir, wows_data, replays } => {
                                    let replays_dir = wows_data.replays_dir.clone();
                                    if let Some(old_wows_data) = &self.tab_state.world_of_warships_data {
                                        *old_wows_data.write() = wows_data;
                                    } else {
                                        let wows_data = Arc::new(RwLock::new(wows_data));
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

                                    *self.tab_state.timed_message.write() = Some(TimedMessage::new(format!("{} Successfully loaded game data", icons::CHECK_CIRCLE)));
                                }
                                BackgroundTaskCompletion::ReplayLoaded { replay } => {
                                    {
                                        self.tab_state.replay_parser_tab.lock().game_chat.clear();
                                    }
                                    {
                                        self.tab_state.settings.player_tracker.write().update_from_replay(&replay.read());
                                    }
                                    self.tab_state.current_replay = Some(replay);
                                    *self.tab_state.timed_message.write() = Some(TimedMessage::new(format!("{} Successfully loaded replay", icons::CHECK_CIRCLE)));
                                }
                                BackgroundTaskCompletion::UpdateDownloaded(new_exe) => {
                                    let current_process = env::args().next().expect("current process has no path?");
                                    let current_process_new_path = format!("{current_process}.old");
                                    // Rename this process
                                    let rename_process = move || {
                                        std::fs::rename(current_process.clone(), &current_process_new_path).context("failed to rename current process")?;
                                        // Rename the new exe
                                        std::fs::rename(new_exe, &current_process).context("failed to rename new process")?;

                                        Command::new(current_process).arg(current_process_new_path).spawn().context("failed to execute updated process")
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
                                BackgroundTaskCompletion::PopulatePlayerInspectorFromReplays => {
                                    // do nothing
                                }
                                BackgroundTaskCompletion::ConstantsLoaded(constants) => {
                                    *self.tab_state.game_constants.write() = constants;
                                }
                                #[cfg(feature = "mod_manager")]
                                BackgroundTaskCompletion::ModManager(mod_manager_info) => {
                                    match *mod_manager_info {
                                        crate::mod_manager::ModTaskCompletion::DatabaseLoaded(index) => {
                                            self.tab_state.mod_manager_info.update_index("test".to_string(), index);
                                        }
                                        crate::mod_manager::ModTaskCompletion::ModInstalled(mod_info) => {
                                            *self.tab_state.timed_message.write() =
                                                Some(TimedMessage::new(format!("{} Successfully installed mod: {}", icons::CHECK_CIRCLE, mod_info.meta.name())));
                                        }
                                        crate::mod_manager::ModTaskCompletion::ModUninstalled(mod_info) => {
                                            *self.tab_state.timed_message.write() =
                                                Some(TimedMessage::new(format!("{} Successfully uninstalled mod: {}", icons::CHECK_CIRCLE, mod_info.meta.name())));
                                        }
                                        crate::mod_manager::ModTaskCompletion::ModDownloaded(_) => {
                                            // Do nothing when the mod is downloaded.
                                        }
                                    }
                                }
                            },
                            Err(e) if e.downcast_current_context::<ToolkitError>().is_some_and(|e| matches!(e, ToolkitError::BackgroundTaskCompleted)) => {}
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
            self.tab_state.background_tasks =
                self.tab_state.background_tasks.drain(..).enumerate().filter_map(|(i, task)| if remove_tasks.contains(&i) { None } else { Some(task) }).collect();

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
                                ui.add(egui::ProgressBar::new(last_progress.progress).text(last_progress.file_name.as_str()));
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

            if let Ok(constants_updates) = octocrab.repos("padtrack", "wows-constants").raw_file(Reference::Branch("main".to_string()), "data/latest.json").await {
                let mut body = constants_updates.into_body();
                let mut result = Vec::with_capacity(body.size_hint().exact().unwrap_or_default() as usize);

                // Iterate through all data chunks in the body
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
                *self.tab_state.timed_message.write() = Some(TimedMessage::new(format!("{} Application up-to-date", icons::CHECK_CIRCLE)));
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
                painter.text(screen_rect.center(), Align2::CENTER_CENTER, text, TextStyle::Heading.resolve(&ctx.style()), Color32::WHITE);
            }
        }

        let mut dropped_files = Vec::new();

        // Collect dropped files:
        ctx.input(|i| {
            if !i.raw.dropped_files.is_empty() {
                dropped_files.clone_from(&i.raw.dropped_files);
            }
        });

        // Only perform operations if we have one file
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
        // Put your widgets into a `SidePanel`, `TopPanel`, `CentralPanel`, `Window` or `Area`.
        // For inspiration and more examples, go to https://emilk.github.io/egui

        if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::CTRL | Modifiers::SHIFT, egui::Key::D))) {
            self.tab_state.settings.debug_mode = !self.tab_state.settings.debug_mode;
            if let Some(sender) = self.tab_state.background_parser_tx.as_ref() {
                let _ = sender.send(ReplayBackgroundParserThreadMessage::DebugStateChange(self.tab_state.settings.debug_mode));
            }
        }

        self.tab_state.try_update_replays();

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
            // The top panel is often a good place for a menu bar:

            egui::MenuBar::new().ui(ui, |ui| {
                // NOTE: no File->Quit on web pages!
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
            // The central panel the region left after adding TopPanel's and SidePanel's
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

        *file_viewer = file_viewer.drain(..).enumerate().filter_map(|(idx, viewer)| if !remove_viewers.contains(&idx) { Some(viewer) } else { None }).collect();
        drop(file_viewer);

        // Handle replay drag and drop events
        self.ui_file_drag_and_drop(ctx);

        // Ensure we update at least every second
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
                            ui.ctx().open_url(OpenUrl::new_tab("https://github.com/landaire/wows-toolkit/issues/new/choose"));
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

                            visuals.widgets.inactive.bg_fill = Color32::from_rgb(200, 50, 50); // Base red
                            visuals.widgets.hovered.bg_fill = Color32::from_rgb(220, 70, 70); // Lighter on hover
                            visuals.widgets.active.bg_fill = Color32::from_rgb(160, 30, 30); // Darker on click

                            visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(200, 50, 50); // Base red
                            visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(220, 70, 70); // Lighter on hover
                            visuals.widgets.active.weak_bg_fill = Color32::from_rgb(160, 30, 30); // Darker on click

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
            // Remove the panic log so we don't show it again
            let _ = std::fs::remove_file(Self::panic_log_path());
            self.panic_info = None;
        }
    }

    fn show_update_window(&mut self, ctx: &Context) {
        if let Some(latest_release) = self.latest_release.as_ref() {
            let url = latest_release.html_url.clone();
            let mut notes = latest_release.body.clone();
            let tag = latest_release.tag_name.clone();
            let asset = latest_release.assets.iter().find(|asset| asset.name.contains("windows") && asset.name.ends_with(".zip"));
            // Only show the update window if we have a valid artifact to download
            if let Some(asset) = asset {
                egui::Window::new("Update Available").open(&mut self.update_window_open).show(ctx, |ui| {
                    ui.vertical(|ui| {
                        ui.label(format!("Version {tag} of WoWs Toolkit is available"));
                        if let Some(notes) = notes.as_mut() {
                            CommonMarkViewer::new().show(ui, &mut self.tab_state.markdown_cache, notes);
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
    /// Called by the frame work to save state before shutdown.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }

    /// Called each time the UI needs repainting, which may be many times per second.
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.update_impl(ctx, frame);
    }
}

fn build_about_window(ui: &mut egui::Ui) {
    ui.vertical(|ui| {
        ui.label("Made by landaire.");
        ui.label("Thanks to Trackpad, TTaro, lkolbly for their contributions.");
        if ui.button("View on GitHub").clicked() {
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
