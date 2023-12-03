use std::{
    fs::{self, read_dir, File},
    io::Cursor,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, TryRecvError},
        Arc,
    },
    thread::JoinHandle,
};

use egui::{ahash::HashSet, mutex::Mutex, CollapsingHeader, Label, Sense, Separator, WidgetText};
use egui_dock::{DockArea, DockState, Style, TabViewer};
use egui_extras::{Size, StripBuilder};
use serde::{Deserialize, Serialize};
use wowsunpack::{
    idx::{self, FileNode, IdxFile},
    pkg::PkgFileLoader,
};

#[derive(Clone)]
enum Tab {
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

struct ToolkitTabViewer<'a> {
    parent: &'a mut TabState,
}

impl ToolkitTabViewer<'_> {
    fn build_tree_node(&self, ui: &mut egui::Ui, file_tree: &FileNode) {
        let header = CollapsingHeader::new(if file_tree.is_root() {
            "res"
        } else {
            file_tree.filename()
        })
        .default_open(file_tree.is_root())
        .show(ui, |ui| {
            for (name, node) in file_tree.children() {
                if node.children().is_empty() {
                    if ui
                        .add(Label::new(name).sense(Sense::click()))
                        .double_clicked()
                    {
                        self.parent.items_to_extract.lock().push(node.clone());
                    }
                } else {
                    self.build_tree_node(ui, node);
                }
            }
        });

        if header.header_response.double_clicked() {
            self.parent.items_to_extract.lock().push(file_tree.clone());
        }
    }

    fn build_tree_node_from_array<'i, I>(&self, ui: &mut egui::Ui, files: I)
    where
        I: IntoIterator<Item = &'i (Rc<PathBuf>, FileNode)>,
    {
        egui::Grid::new("filtered_files_grid")
            .num_columns(1)
            .striped(true)
            .show(ui, |ui| {
                let files = files.into_iter();
                for file in files {
                    let label = ui.add(
                        Label::new(Path::new("res").join(&*file.0).to_string_lossy().to_owned())
                            .sense(Sense::click()),
                    );

                    let text = if file.1.is_file() {
                        format!(
                            "File ({})",
                            humansize::format_size(
                                file.1.file_info().unwrap().size,
                                humansize::DECIMAL
                            )
                        )
                    } else {
                        format!("Folder")
                    };

                    let label = label.on_hover_text(text);

                    if label.double_clicked() {
                        self.parent.items_to_extract.lock().push(file.1.clone());
                    }
                    ui.end_row();
                }
            });
    }
    fn build_unpacker_tab(&mut self, ui: &mut egui::Ui) {
        egui::SidePanel::left("left").show_inside(ui, |ui| {
            // })
            // ui.with_layout(egui::Layout::left_to_right(egui::Align::LEFT), |ui| {
            ui.vertical(|ui| {
                //     ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                //     });

                ui.add(egui::TextEdit::singleline(&mut self.parent.filter).hint_text("Filter"));
                egui::ScrollArea::both()
                    .id_source("file_tree_scroll_area")
                    .show(ui, |ui| {
                        if let (Some(file_tree), Some(files)) =
                            (&self.parent.file_tree, &self.parent.files)
                        {
                            if self.parent.filter.len() > 3 {
                                let glob = glob::Pattern::new(self.parent.filter.as_str());
                                if self.parent.filter.contains("*") && glob.is_ok() {
                                    let glob = glob.unwrap();
                                    let leafs = files
                                        .iter()
                                        .filter(|(path, _node)| glob.matches_path(&*path));
                                    self.build_tree_node_from_array(ui, leafs);
                                } else {
                                    let leafs = files.iter().filter(|(path, node)| {
                                        path.to_str()
                                            .map(|path| path.contains(self.parent.filter.as_str()))
                                            .unwrap_or(false)
                                    });
                                    self.build_tree_node_from_array(ui, leafs);
                                }
                            } else {
                                self.build_tree_node(ui, file_tree);
                            }
                        }
                    });
            });
        });
        egui::CentralPanel::default().show_inside(ui, |ui| {
            StripBuilder::new(ui)
                .size(Size::remainder())
                .size(Size::exact(20.0))
                .vertical(|mut strip| {
                    strip.cell(|ui| {
                        ui.vertical(|ui| {
                            egui::ScrollArea::both()
                                .id_source("selected_files_scroll_area")
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        // strip.cell(|ui| {
                                        ui.heading("Selected Files");
                                        // });
                                        // strip.cell(|ui| {
                                        ui.button("Add Glob");
                                        // });
                                    });

                                    ui.separator();

                                    let mut items = self.parent.items_to_extract.lock();
                                    let mut remove_idx = None;
                                    for (i, item) in items.iter().enumerate() {
                                        if ui
                                            .add(
                                                Label::new(
                                                    Path::new("res")
                                                        .join(item.path().unwrap())
                                                        .to_string_lossy()
                                                        .to_owned(),
                                                )
                                                .sense(Sense::click()),
                                            )
                                            .double_clicked()
                                        {
                                            remove_idx = Some(i);
                                        }
                                    }

                                    if let Some(remove_idx) = remove_idx {
                                        items.remove(remove_idx);
                                    }
                                });
                        });
                    });

                    strip.strip(|builder| {
                        builder
                            .size(Size::remainder())
                            .size(Size::exact(60.0))
                            .size(Size::exact(60.0))
                            .horizontal(|mut strip| {
                                strip.cell(|ui| {
                                    ui.add_sized(
                                        ui.available_size(),
                                        egui::TextEdit::singleline(&mut self.parent.output_dir)
                                            .hint_text("Output Path"),
                                    );
                                });
                                strip.cell(|ui| {
                                    if ui.button("Choose...").clicked() {
                                        let folder = rfd::FileDialog::new().pick_folder();
                                        if let Some(folder) = folder {
                                            self.parent.output_dir =
                                                folder.to_string_lossy().into_owned();
                                        }
                                    }
                                });
                                strip.cell(|ui| {
                                    if ui.button("Extract").clicked() {
                                        let items_to_unpack =
                                            self.parent.items_to_extract.lock().clone();
                                        let output_dir =
                                            Path::new(self.parent.output_dir.as_str()).join("res");
                                        if let Some(pkg_loader) = self.parent.pkg_loader.clone() {
                                            let (tx, rx) = mpsc::channel();

                                            self.parent.unpacker_progress = Some(rx);
                                            UNPACKER_STOP.store(false, Ordering::Relaxed);

                                            if !items_to_unpack.is_empty() {
                                                let unpacker_thread =
                                                    Some(std::thread::spawn(move || {
                                                        let mut file_queue =
                                                            items_to_unpack.clone();
                                                        let mut files_to_extract =
                                                            HashSet::default();
                                                        let mut folders_created =
                                                            HashSet::default();
                                                        while let Some(file) = file_queue.pop() {
                                                            if file.is_file() {
                                                                files_to_extract.insert(file);
                                                            } else {
                                                                for (_, child) in file.children() {
                                                                    file_queue.push(child.clone());
                                                                }
                                                            }
                                                        }
                                                        let file_count = files_to_extract.len();
                                                        let mut files_written = 0;

                                                        for file in files_to_extract {
                                                            if UNPACKER_STOP.load(Ordering::Relaxed)
                                                            {
                                                                break;
                                                            }

                                                            let path = output_dir.join(
                                                                file.parent()
                                                                    .unwrap()
                                                                    .path()
                                                                    .unwrap(),
                                                            );
                                                            let mut file_path =
                                                                path.join(file.filename());
                                                            tx.send(UnpackerProgress {
                                                                file_name: file_path
                                                                    .to_string_lossy()
                                                                    .into(),
                                                                progress: (files_written as f32)
                                                                    / (file_count as f32),
                                                            })
                                                            .unwrap();
                                                            if !folders_created.contains(&path) {
                                                                fs::create_dir_all(&path);
                                                                folders_created
                                                                    .insert(path.clone());
                                                            }

                                                            let mut out_file = File::create(
                                                                file_path,
                                                            )
                                                            .expect("failed to create output file");

                                                            file.read_file(
                                                                &*pkg_loader,
                                                                &mut out_file,
                                                            );
                                                            files_written += 1;
                                                        }
                                                    }));
                                            }
                                        }
                                    }
                                });
                            });
                    });
                });
        });
    }

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
                                            &mut self.parent.settings.wows_dir,
                                        )
                                        .hint_text("World of Warships Directory"),
                                    );
                                });
                                strip.cell(|ui| {
                                    if ui.button("Open...").clicked() {
                                        let folder = rfd::FileDialog::new().pick_folder();
                                        if let Some(folder) = folder {
                                            self.parent.settings.wows_dir =
                                                folder.to_string_lossy().into_owned();
                                            self.parent.load_wows_files();
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
            Tab::ReplayParser => todo!(),
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
struct Settings {
    wows_dir: String,
}

static UNPACKER_STOP: AtomicBool = AtomicBool::new(false);

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct TabState {
    #[serde(skip)]
    file_tree: Option<FileNode>,

    #[serde(skip)]
    pkg_loader: Option<Arc<PkgFileLoader>>,

    #[serde(skip)]
    files: Option<Vec<(Rc<PathBuf>, FileNode)>>,

    filter: String,

    #[serde(skip)]
    items_to_extract: Mutex<Vec<FileNode>>,

    settings: Settings,

    output_dir: String,

    #[serde(skip)]
    unpacker_progress: Option<mpsc::Receiver<UnpackerProgress>>,

    #[serde(skip)]
    last_progress: Option<UnpackerProgress>,
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
            output_dir: Default::default(),
            unpacker_progress: Default::default(),
            last_progress: Default::default(),
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

                self.file_tree = Some(file_tree);
                self.files = Some(files);
                self.pkg_loader = Some(pkg_loader);
            }
        }

        Ok(())
    }
}

struct UnpackerProgress {
    file_name: String,
    progress: f32,
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
            tab_state: TabState {
                file_tree: None,
                files: None,
                pkg_loader: None,
                filter: Default::default(),
                items_to_extract: Default::default(),
                output_dir: String::new(),
                settings: Settings::default(),
                unpacker_progress: None,
                last_progress: None,
            },
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
                        parent: &mut self.tab_state,
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
