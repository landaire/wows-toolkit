use std::{
    collections::HashMap,
    env,
    error::Error,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        atomic::Ordering,
        mpsc::{self, TryRecvError},
        Arc,
    },
    time::{Duration, Instant},
};

use egui::{
    mutex::{Mutex, RwLock},
    OpenUrl, Ui, WidgetText,
};
use egui_dock::{DockArea, DockState, Style, TabViewer};
use egui_extras::{Size, StripBuilder};
use gettext::Catalog;

use notify::{
    event::{ModifyKind, RenameMode},
    EventKind, RecommendedWatcher, RecursiveMode, Watcher,
};
use octocrab::models::repos::Release;
use tracing::debug;

use serde::{Deserialize, Serialize};
use sys_locale::get_locale;
use tokio::runtime::Runtime;
use wows_replays::{analyzer::battle_controller::GameMessage, game_params::Species, ReplayFile};
use wowsunpack::{idx::FileNode, pkg::PkgFileLoader};

use crate::{
    error::ToolkitError,
    file_unpacker::{UnpackerProgress, UNPACKER_STOP},
    game_params::GameMetadataProvider,
    icons,
    plaintext_viewer::PlaintextFileViewer,
    replay_parser::{Replay, SharedReplayParserTabState},
    task::{self, BackgroundTask, BackgroundTaskCompletion, BackgroundTaskKind},
    wows_data::WorldOfWarshipsData,
};

#[derive(Clone)]
pub enum Tab {
    Unpacker,
    ReplayParser,
    Settings,
}

impl Tab {
    fn title(&self) -> String {
        match self {
            Tab::Unpacker => format!("{} Resource Unpacker", icons::ARCHIVE),
            Tab::Settings => format!("{} Settings", icons::GEAR_FINE),
            Tab::ReplayParser => format!("{} Replay Inspector", icons::MAGNIFYING_GLASS),
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
            });
            ui.label("World of Warships Settings");
            ui.group(|ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        StripBuilder::new(ui).size(Size::remainder()).size(Size::exact(50.0)).horizontal(|mut strip| {
                            strip.cell(|ui| {
                                ui.add_sized(
                                    ui.available_size(),
                                    egui::TextEdit::singleline(&mut self.tab_state.settings.wows_dir)
                                        .interactive(self.tab_state.can_change_wows_dir)
                                        .hint_text("World of Warships Directory"),
                                );
                            });
                            strip.cell(|ui| {
                                if ui.add_enabled(self.tab_state.can_change_wows_dir, egui::Button::new("Open...")).clicked() {
                                    let folder = rfd::FileDialog::new().pick_folder();
                                    if let Some(folder) = folder {
                                        self.tab_state.prevent_changing_wows_dir();
                                        self.tab_state.load_game_data(folder);
                                    }
                                }
                            });
                        });
                    });
                })
            });
            ui.label("Replay Settings");
            ui.group(|ui| {
                ui.checkbox(&mut self.tab_state.settings.replay_settings.show_game_chat, "Show Game Chat");
                ui.checkbox(&mut self.tab_state.settings.replay_settings.show_entity_id, "Show Entity ID Column");
                ui.checkbox(&mut self.tab_state.settings.replay_settings.show_observed_damage, "Show Observed Damage Column");
            })
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
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct ReplaySettings {
    pub show_game_chat: bool,
    pub show_entity_id: bool,
    pub show_observed_damage: bool,
}

impl Default for ReplaySettings {
    fn default() -> Self {
        Self {
            show_game_chat: true,
            show_entity_id: false,
            show_observed_damage: true,
        }
    }
}

pub const fn default_bool<const V: bool>() -> bool {
    V
}

#[derive(Default, Serialize, Deserialize)]
pub struct Settings {
    pub current_replay_path: PathBuf,
    pub wows_dir: String,
    pub locale: Option<String>,
    #[serde(default)]
    pub replay_settings: ReplaySettings,
    #[serde(default = "default_bool::<true>")]
    pub check_for_updates: bool,
}

#[derive(Default)]
pub struct ReplayParserTabState {
    pub game_chat: Vec<GameMessage>,
}

#[derive(Debug)]
pub enum NotifyFileEvent {
    Added(PathBuf),
    Removed(PathBuf),
}

pub struct TimedMessage {
    pub message: String,
    pub expiration: Instant,
}

impl TimedMessage {
    pub fn new(message: String) -> Self {
        TimedMessage {
            message,
            expiration: Instant::now() + Duration::from_secs(10),
        }
    }

    pub fn is_expired(&self) -> bool {
        self.expiration < Instant::now()
    }
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct TabState {
    #[serde(skip)]
    pub world_of_warships_data: Option<Arc<WorldOfWarshipsData>>,

    pub filter: String,

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
    pub background_task: Option<BackgroundTask>,

    #[serde(skip)]
    pub timed_message: Option<TimedMessage>,

    #[serde(skip)]
    pub can_change_wows_dir: bool,

    #[serde(skip)]
    pub current_replay: Option<Arc<RwLock<Replay>>>,
}

impl Default for TabState {
    fn default() -> Self {
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
            background_task: None,
            can_change_wows_dir: true,
            timed_message: None,
            current_replay: None,
        }
    }
}

impl TabState {
    fn try_update_replays(&mut self) {
        if let Some(replay_files) = &mut self.replay_files {
            if let Some(file) = self.file_receiver.as_ref() {
                while let Ok(file_event) = file.try_recv() {
                    match file_event {
                        NotifyFileEvent::Added(new_file) => {
                            if let Some(wows_data) = self.world_of_warships_data.as_ref() {
                                if let Some(game_metadata) = wows_data.game_metadata.as_ref() {
                                    let replay_file: ReplayFile = ReplayFile::from_file(&new_file).unwrap();
                                    let replay = Replay::new(replay_file, game_metadata.clone());
                                    let replay = Arc::new(RwLock::new(replay));

                                    replay_files.insert(new_file, replay);
                                }
                            }
                        }
                        NotifyFileEvent::Removed(old_file) => {
                            replay_files.remove(&old_file);
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
            let old_replays_dir = Path::new(self.settings.wows_dir.as_str()).join("replays");
            let _ = watcher.unwatch(&old_replays_dir);
            watcher
        } else {
            debug!("creating filesystem watcher");
            let (tx, rx) = mpsc::channel();
            let watcher = notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| match res {
                Ok(event) => {
                    // TODO: maybe properly handle moves?
                    debug!("filesytem event: {:?}", event);
                    match event.kind {
                        EventKind::Modify(ModifyKind::Name(RenameMode::To)) | EventKind::Create(_) => {
                            for path in event.paths {
                                if path.is_file() && path.extension().map(|ext| ext == "wowsreplay").unwrap_or(false) && path.file_name().unwrap() != "temp.wowsreplay" {
                                    tx.send(NotifyFileEvent::Added(path)).expect("failed to send file creation event");
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
    }

    pub fn load_game_data(&mut self, wows_directory: PathBuf) {
        let (tx, rx) = mpsc::channel();
        let locale = self.settings.locale.clone().unwrap();
        let _join_handle = std::thread::spawn(move || {
            let _ = tx.send(task::load_wows_files(wows_directory, locale.as_str()));
        });

        self.background_task = Some(BackgroundTask {
            receiver: rx,
            kind: BackgroundTaskKind::LoadingData,
        });
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
    latest_release: Option<Release>,
    #[serde(skip)]
    show_about_window: bool,
    #[serde(skip)]
    show_error_window: bool,
    #[serde(skip)]
    error_to_show: Option<Box<dyn Error>>,

    tab_state: TabState,
    #[serde(skip)]
    dock_state: DockState<Tab>,

    #[serde(skip)]
    runtime: Runtime,
}

impl Default for WowsToolkitApp {
    fn default() -> Self {
        Self {
            checked_for_updates: false,
            update_window_open: false,
            latest_release: None,
            show_about_window: false,
            tab_state: Default::default(),
            dock_state: DockState::new([Tab::ReplayParser, Tab::Unpacker, Tab::Settings].to_vec()),
            show_error_window: false,
            error_to_show: None,
            runtime: Runtime::new().expect("failed to create tokio runtime"),
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

        cc.egui_ctx.set_fonts(fonts);

        // Load previous app state (if any).
        // Note that you must enable the `persistence` feature for this to work.
        if let Some(storage) = cc.storage {
            let mut saved_state: Self = eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default();
            if !saved_state.tab_state.settings.wows_dir.is_empty() {
                saved_state.tab_state.load_game_data(PathBuf::from(saved_state.tab_state.settings.wows_dir.clone()));
            }

            saved_state.tab_state.settings.locale = Some("en".to_string());

            return saved_state;
        }

        let mut this: Self = Default::default();
        // this.tab_state.settings.locale = Some(get_locale().unwrap_or_else(|| String::from("en")));
        this.tab_state.settings.locale = Some("en".to_string());

        let default_wows_dir = "C:\\Games\\World_of_Warships";
        let default_wows_path = Path::new(default_wows_dir);
        if default_wows_path.exists() {
            this.tab_state.settings.wows_dir = default_wows_dir.to_string();
        }

        this
    }

    pub fn build_bottom_panel(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            // TODO: Merge these channels
            if let Some(task) = &mut self.tab_state.background_task {
                if let Some(result) = task.build_description(ui) {
                    match &task.kind {
                        BackgroundTaskKind::LoadingData => {
                            self.tab_state.allow_changing_wows_dir();
                        }
                        BackgroundTaskKind::LoadingReplay => {
                            // nothing to do
                        }
                        BackgroundTaskKind::Updating {
                            rx: _rx,
                            last_progress: _last_progress,
                        } => {
                            // do nothing
                        }
                    }

                    match result {
                        Ok(data) => match data {
                            BackgroundTaskCompletion::DataLoaded { new_dir, wows_data, replays } => {
                                self.tab_state.update_wows_dir(&new_dir, &wows_data.replays_dir);
                                self.tab_state.world_of_warships_data = Some(Arc::new(wows_data));
                                self.tab_state.replay_files = replays;

                                self.tab_state.timed_message = Some(TimedMessage::new(format!("{} Successfully loaded game data", icons::CHECK_CIRCLE)))
                            }
                            BackgroundTaskCompletion::ReplayLoaded { replay } => {
                                {
                                    self.tab_state.replay_parser_tab.lock().game_chat.clear();
                                }
                                self.tab_state.current_replay = Some(replay);
                                self.tab_state.timed_message = Some(TimedMessage::new(format!("{} Successfully loaded replay", icons::CHECK_CIRCLE)))
                            }
                            BackgroundTaskCompletion::UpdateDownloaded(new_exe) => {
                                let current_process = env::args().next().expect("current process has no path?");
                                let current_process_new_path = format!("{}.old", current_process);
                                // Rename this process
                                std::fs::rename(current_process.clone(), &current_process_new_path).expect("failed to rename current process");
                                // Rename the new exe
                                std::fs::rename(new_exe, &current_process).expect("failed to rename new process");

                                Command::new(current_process)
                                    .arg(current_process_new_path)
                                    .spawn()
                                    .expect("failed to execute updated process");

                                std::process::exit(0);
                            }
                        },
                        Err(ToolkitError::BackgroundTaskCompleted) => {
                            self.tab_state.background_task = None;
                        }
                        Err(e) => {
                            self.show_error_window = true;
                            self.error_to_show = Some(Box::new(e));
                        }
                    }
                }
            } else if let Some(rx) = &self.tab_state.unpacker_progress {
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
            } else if let Some(timed_message) = &self.tab_state.timed_message {
                if !timed_message.is_expired() {
                    ui.label(timed_message.message.as_str());
                } else {
                    self.tab_state.timed_message = None;
                }
            }
        });
    }

    fn check_for_updates(&mut self) {
        let result = self.runtime.block_on(async {
            octocrab::instance()
                .repos("landaire", "wows-toolkit")
                .releases()
                .list()
                // Optional Parameters
                .per_page(1)
                // Send the request
                .send()
                .await
        });

        if let Ok(result) = result {
            if !result.items.is_empty() {
                let latest_release = result.items[0].clone();
                if let Ok(version) = semver::Version::parse(&latest_release.tag_name[1..]) {
                    let app_version = semver::Version::parse(env!("CARGO_PKG_VERSION")).unwrap();
                    if app_version < version {
                        self.update_window_open = true;
                        self.latest_release = Some(latest_release);
                    } else {
                        self.tab_state.timed_message = Some(TimedMessage::new(format!("{} Application up-to-date", icons::CHECK_CIRCLE)));
                    }
                }
            }
        }
        self.checked_for_updates = true;
    }
}

impl eframe::App for WowsToolkitApp {
    /// Called by the frame work to save state before shutdown.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }

    /// Called each time the UI needs repainting, which may be many times per second.
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Put your widgets into a `SidePanel`, `TopPanel`, `CentralPanel`, `Window` or `Area`.
        // For inspiration and more examples, go to https://emilk.github.io/egui

        egui_extras::install_image_loaders(ctx);

        self.tab_state.try_update_replays();

        if !self.checked_for_updates && self.tab_state.settings.check_for_updates {
            self.check_for_updates();
        }

        if self.update_window_open {
            if let Some(latest_release) = self.latest_release.as_ref() {
                let url = latest_release.html_url.clone();
                let mut notes = latest_release.body.clone();
                let tag = latest_release.tag_name.clone();
                egui::Window::new("Update Available").open(&mut self.update_window_open).show(ctx, |ui| {
                    ui.vertical(|ui| {
                        ui.label(format!("Version {} of WoWs Toolkit is available", tag));
                        if let Some(notes) = notes.as_mut() {
                            ui.text_edit_multiline(notes);
                        }
                        ui.horizontal(|ui| {
                            #[cfg(target_os = "windows")]
                            {
                                let asset = latest_release
                                    .assets
                                    .iter()
                                    .find(|asset| asset.name.contains("windows") && asset.name.ends_with(".zip"));
                                if let Some(asset) = asset {
                                    if ui.button("Install Update").clicked() {
                                        self.tab_state.background_task = Some(crate::task::start_download_update_task(&self.runtime, asset));
                                    }
                                }
                            }
                            if ui.button("View Release").clicked() {
                                ui.ctx().open_url(OpenUrl::new_tab(url));
                            }
                        });
                    });
                });
            }
        }

        if let Some(error) = self.error_to_show.as_ref() {
            if self.show_error_window {
                egui::Window::new("Error").open(&mut self.show_error_window).show(ctx, |ui| {
                    build_error_window(ui, error.as_ref());
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

            egui::menu::bar(ui, |ui| {
                // NOTE: no File->Quit on web pages!
                let is_web = cfg!(target_arch = "wasm32");
                if !is_web {
                    ui.menu_button("File", |ui| {
                        if ui.button("Check for Updates").clicked() {
                            self.checked_for_updates = false;
                            ui.close_menu();
                        }
                        if ui.button("About").clicked() {
                            self.show_about_window = true;
                            ui.close_menu();
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

                egui::widgets::global_dark_light_mode_buttons(ui);
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

fn build_error_window(ui: &mut egui::Ui, error: &dyn Error) {
    ui.vertical(|ui| {
        ui.label(format!("{} An error occurred:", icons::WARNING));
        ui.label(error.to_string());
    });
}

#[macro_export]
macro_rules! update_background_task {
    ($saved_task:expr, $background_task:expr) => {
        let task = $background_task;
        if task.is_some() {
            $saved_task = task;
        }
    };
}
