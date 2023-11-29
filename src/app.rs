use std::{fs::read_dir, io::Cursor, path::PathBuf};

use egui::{mutex::Mutex, CollapsingHeader, Label, Sense, Separator, WidgetText};
use egui_dock::{DockArea, DockState, Style, TabViewer};
use egui_extras::{Size, StripBuilder};
use wowsunpack::idx::{self, FileNode, IdxFile};

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
            Tab::ReplayParser => "Replay Parser",
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
    fn build_unpacker_tab(&mut self, ui: &mut egui::Ui) {
        ui.with_layout(egui::Layout::left_to_right(egui::Align::LEFT), |ui| {
            egui::ScrollArea::both()
                .id_source("file_tree_scroll_area")
                .show(ui, |ui| {
                    self.build_tree_node(ui, &self.parent.file_tree);
                });

            ui.add(Separator::default().vertical());

            StripBuilder::new(ui)
                .size(Size::remainder())
                .size(Size::exact(20.0))
                .vertical(|mut strip| {
                    strip.cell(|ui| {
                        ui.vertical(|ui| {
                            egui::ScrollArea::both()
                                .id_source("selected_files_scroll_area")
                                .show(ui, |ui| {
                                    ui.heading("Selected Files");

                                    ui.separator();

                                    let items = self.parent.items_to_extract.lock();
                                    for item in &*items {
                                        ui.label(
                                            PathBuf::from("res")
                                                .join(item.path().unwrap())
                                                .to_string_lossy()
                                                .to_owned(),
                                        );
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
                                    ui.button("Extract");
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

#[derive(Default)]
struct Settings {
    wows_dir: String,
}

struct TabState {
    file_tree: FileNode,

    items_to_extract: Mutex<Vec<FileNode>>,
    settings: Settings,

    output_dir: String,
}

/// We derive Deserialize/Serialize so we can persist app state on shutdown.
pub struct WowsToolkitApp {
    label: String,

    value: f32,

    tab_state: TabState,
    dock_state: DockState<Tab>,
}

impl Default for WowsToolkitApp {
    fn default() -> Self {
        let mut idx_files = Vec::new();
        for file in
            read_dir("/Users/lander/Downloads/depots/552993/12603293/bin/7708495/idx").unwrap()
        {
            let file = file.unwrap();
            if file.file_type().unwrap().is_file() {
                let file_data = std::fs::read(file.path()).unwrap();
                let mut file = Cursor::new(file_data.as_slice());
                idx_files.push(idx::parse(&mut file).unwrap());
            }
        }

        let file_tree = idx::build_file_tree(idx_files.as_slice());

        Self {
            // Example stuff:
            label: "Hello World!".to_owned(),
            value: 2.7,
            tab_state: TabState {
                file_tree,
                items_to_extract: Default::default(),
                output_dir: String::new(),
                settings: Settings::default(),
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

        Default::default()
    }
}

impl eframe::App for WowsToolkitApp {
    /// Called by the frame work to save state before shutdown.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {}

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

        egui::CentralPanel::default().show(ctx, |ui| {
            // The central panel the region left after adding TopPanel's and SidePanel's
            ui.heading("WoWs Toolkit");

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
