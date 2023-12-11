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
    thread::LocalKey,
};

use egui::{mutex::Mutex, Ui, WidgetText};
use egui_dock::{DockArea, DockState, Style, TabViewer};
use egui_extras::{Size, StripBuilder};
use gettext::Catalog;
use language_tags::LanguageTag;
use serde::{Deserialize, Serialize};
use sys_locale::get_locale;
use wows_replays::ReplayFile;
use wowsunpack::{
    game_params,
    idx::{self, FileNode},
    pkg::PkgFileLoader,
};

use crate::{
    file_unpacker::{UnpackerProgress, UNPACKER_STOP},
    game_params::GameParams,
    plaintext_viewer::PlaintextFileViewer,
    replay_parser::{ChatChannel, SharedReplayParserTabState, ShipLoadout},
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
            ui.group(|ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        StripBuilder::new(ui)
                            .size(Size::remainder())
                            .size(Size::exact(50.0))
                            .horizontal(|mut strip| {
                                strip.cell(|ui| {
                                    ui.add_sized(
                                        ui.available_size(),
                                        egui::TextEdit::singleline(
                                            &mut self.tab_state.settings.wows_dir,
                                        )
                                        .hint_text("World of Warships Directory"),
                                    );
                                });
                                strip.cell(|ui| {
                                    if ui.button("Open...").clicked() {
                                        let folder = rfd::FileDialog::new().pick_folder();
                                        if let Some(folder) = folder {
                                            self.tab_state.settings.wows_dir =
                                                folder.to_string_lossy().into_owned();
                                            self.tab_state.load_wows_files();
                                        }
                                    }
                                });
                            });
                    });
                })
            });
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

#[derive(Default, Serialize, Deserialize)]
pub struct Settings {
    #[serde(skip)]
    pub current_replay: Option<ReplayFile>,
    pub current_replay_path: PathBuf,
    wows_dir: String,
}

#[derive(Serialize, Deserialize)]
pub struct GameMessage {
    pub sender_relation: u32,
    pub sender_name: String,
    pub channel: ChatChannel,
    pub message: String,
}

#[derive(Default)]
pub struct ReplayParserTabState {
    pub game_chat: Vec<GameMessage>,
    pub ship_configs: HashMap<u32, ShipLoadout>,
    pub vehicle_id_to_entity_id: HashMap<u32, u32>,
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct TabState {
    #[serde(skip)]
    pub file_tree: Option<FileNode>,

    #[serde(skip)]
    pub pkg_loader: Option<Arc<PkgFileLoader>>,

    #[serde(skip)]
    pub files: Option<Vec<(Rc<PathBuf>, FileNode)>>,

    pub filter: String,

    #[serde(skip)]
    pub items_to_extract: Mutex<Vec<FileNode>>,

    pub settings: Settings,

    #[serde(skip)]
    pub translations: Option<Catalog>,

    #[serde(skip)]
    pub game_params: Option<GameParams>,

    pub output_dir: String,

    #[serde(skip)]
    pub unpacker_progress: Option<mpsc::Receiver<UnpackerProgress>>,

    #[serde(skip)]
    pub last_progress: Option<UnpackerProgress>,

    #[serde(skip)]
    pub replay_parser_tab: SharedReplayParserTabState,

    #[serde(skip)]
    pub file_viewer: Mutex<Vec<PlaintextFileViewer>>,
}

impl Default for TabState {
    fn default() -> Self {
        Self {
            file_tree: Default::default(),
            pkg_loader: Default::default(),
            files: Default::default(),
            filter: Default::default(),
            items_to_extract: Default::default(),
            settings: Default::default(),
            translations: Default::default(),
            game_params: Default::default(),
            output_dir: Default::default(),
            unpacker_progress: Default::default(),
            last_progress: Default::default(),
            replay_parser_tab: Default::default(),
            file_viewer: Default::default(),
        }
    }
}

impl TabState {
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

                    if let Some(build_num) = file
                        .file_name()
                        .to_str()
                        .and_then(|name| usize::from_str_radix(name, 10).ok())
                    {
                        if highest_number.is_none()
                            || highest_number
                                .clone()
                                .map(|number| number < build_num)
                                .unwrap_or(false)
                        {
                            highest_number = Some(build_num)
                        }
                    }
                }
            }

            if let Some(number) = highest_number {
                let locale = get_locale().unwrap_or_else(|| String::from("en"));
                let language_tag: LanguageTag = locale.parse().unwrap();
                let attempted_dirs = [locale.as_str(), language_tag.primary_language(), "en"];
                for dir in attempted_dirs {
                    let localization_path = wows_directory.join(format!(
                        "bin/{}/res/texts/{}/LC_MESSAGES/global.mo",
                        number, dir
                    ));
                    if !localization_path.exists() {
                        continue;
                    }
                    let global =
                        File::open(localization_path).expect("failed to open localization file");
                    let catalog = Catalog::parse(global).expect("could not parse catalog");
                    self.translations = Some(catalog);
                }
                for file in read_dir(
                    wows_directory
                        .join("bin")
                        .join(format!("{}", number))
                        .join("idx"),
                )? {
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

                // Try loading GameParams.data
                self.game_params = GameParams::from_pkg(&file_tree, &pkg_loader).ok();

                self.file_tree = Some(file_tree);
                self.files = Some(files);
                self.pkg_loader = Some(pkg_loader);
            }
        }

        Ok(())
    }
}

/// We derive Deserialize/Serialize so we can persist app state on shutdown.
#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct WowsToolkitApp {
    label: String,

    value: f32,

    tab_state: TabState,
    #[serde(skip)]
    dock_state: DockState<Tab>,
}

impl Default for WowsToolkitApp {
    fn default() -> Self {
        Self {
            // Example stuff:
            label: "Hello World!".to_owned(),
            value: 2.7,
            tab_state: Default::default(),
            dock_state: DockState::new([Tab::Unpacker, Tab::ReplayParser, Tab::Settings].to_vec()),
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
            let mut saved_state: Self =
                eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default();
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

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            // The top panel is often a good place for a menu bar:

            egui::menu::bar(ui, |ui| {
                // NOTE: no File->Quit on web pages!
                let is_web = cfg!(target_arch = "wasm32");
                if !is_web {
                    ui.menu_button("File", |ui| {
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
                .show_inside(
                    ui,
                    &mut ToolkitTabViewer {
                        tab_state: &mut self.tab_state,
                    },
                );

            // ui.vertical(|ui| {

            //     ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
            //     });
            // });

            // ui.horizontal(|ui| {
            //     ui.label("Write something: ");
            //     ui.text_edit_singleline(&mut self.label);
            // });

            // ui.add(egui::Slider::new(&mut self.value, 0.0..=10.0).text("value"));
            // if ui.button("Increment").clicked() {
            //     self.value += 1.0;
            // }

            // ui.separator();

            // ui.add(egui::github_link_file!(
            //     "https://github.com/emilk/eframe_template/blob/master/",
            //     "Source code."
            // ));

            // ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
            //     powered_by_egui_and_eframe(ui);
            //     egui::warn_if_debug_build(ui);
            // });
        });

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
            .filter_map(|(idx, viewer)| {
                if !remove_viewers.contains(&idx) {
                    Some(viewer)
                } else {
                    None
                }
            })
            .collect();
    }
}

fn powered_by_egui_and_eframe(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.label("Powered by ");
        ui.hyperlink_to("egui", "https://github.com/emilk/egui");
        ui.label(" and ");
        ui.hyperlink_to(
            "eframe",
            "https://github.com/emilk/egui/tree/master/crates/eframe",
        );
        ui.label(".");
    });
}
