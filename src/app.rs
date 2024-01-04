use std::{
    collections::HashMap,
    fs::{read_dir, File},
    io::Cursor,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        atomic::Ordering,
        mpsc::{self, TryRecvError},
        Arc,
    },
};

use egui::{mutex::Mutex, OpenUrl, Ui, WidgetText};
use egui_dock::{DockArea, DockState, Style, TabViewer};
use egui_extras::{Size, StripBuilder};
use gettext::Catalog;
use language_tags::LanguageTag;
use notify::{
    event::{ModifyKind, RenameMode},
    EventKind, RecommendedWatcher, RecursiveMode, Watcher,
};
use octocrab::models::repos::Release;

use serde::{Deserialize, Serialize};
use sys_locale::get_locale;
use wows_replays::{analyzer::battle_controller::GameMessage, game_params::Species};
use wowsunpack::{
    idx::{self, FileNode},
    pkg::PkgFileLoader,
};

use crate::{
    file_unpacker::{UnpackerProgress, UNPACKER_STOP},
    game_params::GameMetadataProvider,
    plaintext_viewer::PlaintextFileViewer,
    replay_parser::{Replay, SharedReplayParserTabState},
};

#[derive(Clone)]
pub enum Tab {
    Unpacker,
    ReplayParser,
    Settings,
}

impl Tab {
    fn tab_name(&self) -> &'static str {
        match self {
            Tab::Unpacker => "Resource Unpacker",
            Tab::Settings => "Settings",
            Tab::ReplayParser => "Replay Inspector",
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
                                    egui::TextEdit::singleline(&mut self.tab_state.settings.wows_dir).hint_text("World of Warships Directory"),
                                );
                            });
                            strip.cell(|ui| {
                                if ui.button("Open...").clicked() {
                                    let folder = rfd::FileDialog::new().pick_folder();
                                    if let Some(folder) = folder {
                                        self.tab_state.settings.wows_dir = folder.to_string_lossy().into_owned();
                                        // TODO: Handle loading error
                                        let _ = self.tab_state.load_wows_files();
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
        tab.tab_name().into()
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

pub struct WorldOfWarshipsData {
    pub file_tree: Option<FileNode>,

    pub pkg_loader: Option<Arc<PkgFileLoader>>,

    pub files: Option<Vec<(Rc<PathBuf>, FileNode)>>,

    pub game_metadata: Option<Rc<GameMetadataProvider>>,

    pub current_replay: Option<Replay>,

    pub ship_icons: Option<HashMap<Species, (String, Vec<u8>)>>,

    pub game_version: Option<usize>,
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

pub enum NotifyFileEvent {
    Added(PathBuf),
    Removed(PathBuf),
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct TabState {
    #[serde(skip)]
    pub world_of_warships_data: WorldOfWarshipsData,

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
    pub replays_dir: Option<PathBuf>,

    #[serde(skip)]
    pub file_receiver: Option<mpsc::Receiver<NotifyFileEvent>>,

    #[serde(skip)]
    pub replay_files: Option<Vec<PathBuf>>,
}

impl Default for TabState {
    fn default() -> Self {
        Self {
            world_of_warships_data: WorldOfWarshipsData {
                file_tree: None,
                pkg_loader: None,
                files: None,
                game_metadata: None,
                current_replay: Default::default(),
                ship_icons: None,
                game_version: None,
            },
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
            replays_dir: None,
            replay_files: None,
            file_receiver: None,
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
                            replay_files.insert(0, new_file);
                        }
                        NotifyFileEvent::Removed(old_file) => {
                            if let Some(pos) = replay_files.iter().position(|file_path| file_path == &old_file) {
                                replay_files.remove(pos);
                            }
                        }
                    }
                }
            }
        }
    }

    fn reload_replays(&mut self) {
        if let Some(watcher) = self.file_watcher.as_mut() {
            let _ = watcher.unwatch(self.replays_dir.as_ref().unwrap());
        }
        let replay_dir = Path::new(self.settings.wows_dir.as_str()).join("replays");
        let mut files = Vec::new();
        self.replay_files = None;

        if replay_dir.exists() {
            for file in std::fs::read_dir(&replay_dir).expect("failed to read replay dir").flatten() {
                if !file.file_type().expect("failed to get file type").is_file() {
                    continue;
                }

                let file_path = file.path();

                if let Some("wowsreplay") = file_path.extension().map(|s| s.to_str().expect("failed to convert extension to str")) {
                    if file.file_name() != "temp.wowsreplay" {
                        files.push(file_path);
                    }
                }
            }
        }
        if !files.is_empty() {
            files.sort_by_key(|a| a.metadata().unwrap().created().unwrap());
            files.reverse();
            self.replay_files = Some(files);
        }

        if self.replay_files.is_some() && self.file_watcher.is_none() && replay_dir.exists() {
            eprintln!("creating filesystem watcher");
            let (tx, rx) = mpsc::channel();
            // Automatically select the best implementation for your platform.
            let watcher = if let Some(watcher) = self.file_watcher.as_mut() {
                watcher
            } else {
                let watcher = notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| match res {
                    Ok(event) => {
                        // TODO: maybe properly handle moves?
                        println!("{:?}", event);
                        match event.kind {
                            EventKind::Modify(ModifyKind::Name(RenameMode::To)) | EventKind::Create(_) => {
                                for path in event.paths {
                                    if path.is_file()
                                        && path.extension().map(|ext| ext == "wowsreplay").unwrap_or(false)
                                        && path.file_name().unwrap() != "temp.wowsreplay"
                                    {
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
                    Err(e) => println!("watch error: {:?}", e),
                })
                .expect("failed to create fs watcher for replays dir");

                self.file_watcher = Some(watcher);
                self.file_watcher.as_mut().unwrap()
            };

            // Add a path to be watched. All files and directories at that path and
            // below will be monitored for changes.
            watcher.watch(replay_dir.as_ref(), RecursiveMode::NonRecursive).expect("failed to watch directory");

            self.file_receiver = Some(rx);
            self.replays_dir = Some(replay_dir);
        }
    }
    pub fn load_wows_files(&mut self) -> std::io::Result<()> {
        let mut idx_files = Vec::new();
        let wows_directory = Path::new(self.settings.wows_dir.as_str());
        if wows_directory.exists() {
            let mut highest_number = None;
            for file in read_dir(wows_directory.join("bin"))? {
                if file.is_err() {
                    continue;
                }

                let file = file.unwrap();
                if let Ok(ty) = file.file_type() {
                    if ty.is_file() {
                        continue;
                    }

                    if let Some(build_num) = file.file_name().to_str().and_then(|name| name.parse::<usize>().ok()) {
                        if highest_number.is_none() || highest_number.map(|number| number < build_num).unwrap_or(false) {
                            highest_number = Some(build_num)
                        }
                    }
                }
            }

            if let Some(number) = highest_number {
                for file in read_dir(wows_directory.join("bin").join(format!("{}", number)).join("idx"))? {
                    let file = file.unwrap();
                    if file.file_type().unwrap().is_file() {
                        let file_data = std::fs::read(file.path()).unwrap();
                        let mut file = Cursor::new(file_data.as_slice());
                        idx_files.push(idx::parse(&mut file).unwrap());
                    }
                }

                let pkgs_path = wows_directory.join("res_packages");
                if !pkgs_path.exists() {
                    return Ok(());
                }

                let pkg_loader = Arc::new(PkgFileLoader::new(pkgs_path));

                let file_tree = idx::build_file_tree(idx_files.as_slice());
                let files = file_tree.paths();

                let locale = get_locale().unwrap_or_else(|| String::from("en"));
                let language_tag: LanguageTag = locale.parse().unwrap();
                let attempted_dirs = [locale.as_str(), language_tag.primary_language(), "en"];
                let mut found_catalog = None;
                for dir in attempted_dirs {
                    let localization_path = wows_directory.join(format!("bin/{}/res/texts/{}/LC_MESSAGES/global.mo", number, dir));
                    if !localization_path.exists() {
                        continue;
                    }
                    let global = File::open(localization_path).expect("failed to open localization file");
                    let catalog = Catalog::parse(global).expect("could not parse catalog");
                    found_catalog = Some(catalog);
                    break;
                }

                self.settings.locale = Some(locale.clone());

                // Try loading GameParams.data
                let metadata_provider = GameMetadataProvider::from_pkg(&file_tree, &pkg_loader, number).ok().map(|mut metadata_provider| {
                    if let Some(catalog) = found_catalog {
                        metadata_provider.set_translations(catalog)
                    }

                    Rc::new(metadata_provider)
                });

                // Try loading ship icons
                let species = [
                    Species::AirCarrier,
                    Species::Battleship,
                    Species::Cruiser,
                    Species::Destroyer,
                    Species::Submarine,
                    Species::Auxiliary,
                ];

                let icons: HashMap<Species, (String, Vec<u8>)> = HashMap::from_iter(species.iter().map(|species| {
                    let path = format!("gui/fla/minimap/ship_icons/minimap_{}.svg", <&'static str>::from(species).to_ascii_lowercase());
                    let icon_node = file_tree.find(&path).expect("failed to find file");

                    let mut icon_data = Vec::with_capacity(icon_node.file_info().unwrap().unpacked_size as usize);
                    icon_node.read_file(&pkg_loader, &mut icon_data).expect("failed to read ship icon");

                    (species.clone(), (path, icon_data))
                }));

                let data = WorldOfWarshipsData {
                    game_metadata: metadata_provider,
                    file_tree: Some(file_tree),
                    pkg_loader: Some(pkg_loader),
                    files: Some(files),
                    current_replay: Default::default(),
                    game_version: Some(number),
                    ship_icons: Some(icons),
                };

                self.world_of_warships_data = data;

                self.reload_replays();
            }
        }

        Ok(())
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

    tab_state: TabState,
    #[serde(skip)]
    dock_state: DockState<Tab>,
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
        }
    }
}

impl WowsToolkitApp {
    /// Called once before the first frame.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // This is also where you can customize the look and feel of egui using
        // `cc.egui_ctx.set_visuals` and `cc.egui_ctx.set_fonts`.

        // Load previous app state (if any).
        // Note that you must enable the `persistence` feature for this to work.
        if let Some(storage) = cc.storage {
            let mut saved_state: Self = eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default();
            if !saved_state.tab_state.settings.wows_dir.is_empty() {
                match saved_state.tab_state.load_wows_files() {
                    Ok(_) => {
                        // do nothing
                    }
                    Err(_) => {
                        // TODO: handle errors
                    }
                }
            }

            return saved_state;
        }

        Default::default()
    }

    pub fn build_bottom_panel(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
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
            }
        });
    }

    fn check_for_updates(&mut self) {
        let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
        let result = rt.block_on(async {
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
                        if ui.button("View Release").clicked() {
                            ui.ctx().open_url(OpenUrl::new_tab(url));
                        }
                    });
                });
            }
        }

        if self.show_about_window {
            egui::Window::new("About").open(&mut self.show_about_window).show(ctx, |ui| {
                show_about_window(ui);
            });
        }

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            // The top panel is often a good place for a menu bar:

            egui::menu::bar(ui, |ui| {
                // NOTE: no File->Quit on web pages!
                let is_web = cfg!(target_arch = "wasm32");
                if !is_web {
                    ui.menu_button("File", |ui| {
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

fn show_about_window(ui: &mut egui::Ui) {
    ui.vertical(|ui| {
        ui.label("Made by landaire.");
        ui.label("Thanks to Trackpad, TTaro, lkolby for their contributions.");
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
