use std::{
    collections::HashSet,
    fs::{self, File},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};

use egui::{CollapsingHeader, Label, Response, Sense, Ui, mutex::Mutex};
use egui_extras::{Size, StripBuilder};
use egui_phosphor::regular as icons;
use tracing::debug;
use wowsunpack::{
    data::{idx::FileNode, pkg::PkgFileLoader},
    game_params::{
        convert::{game_params_to_pickle, pickle_to_cbor, pickle_to_json},
        types::GameParamProvider,
    },
};

use crate::{
    app::ToolkitTabViewer,
    plaintext_viewer::{self, FileType},
};
pub static UNPACKER_STOP: AtomicBool = AtomicBool::new(false);

pub struct UnpackerProgress {
    pub file_name: String,
    pub progress: f32,
}

const IMAGE_FILE_TYPES: [&str; 3] = [".jpg", ".png", ".svg"];
const PLAINTEXT_FILE_TYPES: [&str; 3] = [".xml", ".json", ".txt"];

#[derive(Eq, PartialEq)]
enum GameParamsFormat {
    Json,
    Cbor,
    MinimalJson,
    MinimalCbor,
}

impl ToolkitTabViewer<'_> {
    fn pkg_loader(&self) -> Option<Arc<PkgFileLoader>> {
        self.tab_state.world_of_warships_data.as_ref().map(|wows_data| wows_data.read().pkg_loader.clone())
    }

    fn add_view_file_menu(&self, file_label: &Response, node: &FileNode) {
        let is_plaintext_file = PLAINTEXT_FILE_TYPES.iter().find(|extension| node.filename().ends_with(**extension));
        let is_image_file = IMAGE_FILE_TYPES.iter().find(|extension| node.filename().ends_with(**extension));

        if is_plaintext_file.is_some() || is_image_file.is_some() {
            file_label.context_menu(|ui| {
                if let Some(pkg_loader) = self.pkg_loader() {
                    if ui.button("View Contents").clicked() {
                        let mut file_contents: Vec<u8> = Vec::with_capacity(node.file_info().unwrap().unpacked_size as usize);

                        node.read_file(&pkg_loader, &mut file_contents).expect("failed to read file");

                        let file_type = match (is_plaintext_file, is_image_file) {
                            (Some(ext), None) => String::from_utf8(file_contents)
                                .ok()
                                .map(|contents| FileType::PlainTextFile { ext: ext.to_string(), contents }),
                            (None, Some(ext)) => Some(FileType::Image {
                                ext: ext.to_string(),
                                contents: file_contents,
                            }),
                            (None, None) => None,
                            _ => unreachable!("this should be impossible"),
                        };

                        if let Some(file_type) = file_type {
                            let viewer = plaintext_viewer::PlaintextFileViewer {
                                title: Arc::new(Path::new("res").join(node.path().unwrap()).to_str().unwrap().to_string()),
                                file_info: Arc::new(Mutex::new(file_type)),
                                open: Arc::new(AtomicBool::new(true)),
                            };

                            self.tab_state.file_viewer.lock().push(viewer);
                        }

                        ui.close_menu();
                    }
                }
            });
        }
    }
    /// Builds a resource tree node from a [FileNode]
    fn build_resource_tree_node(&self, ui: &mut egui::Ui, file_tree: &FileNode) {
        let header = CollapsingHeader::new(if file_tree.is_root() { "res" } else { file_tree.filename() })
            .default_open(file_tree.is_root())
            .show(ui, |ui| {
                for (name, node) in file_tree.children() {
                    if node.is_file() {
                        let file_label = ui.add(Label::new(name).sense(Sense::click()));
                        self.add_view_file_menu(&file_label, node);
                        if file_label.double_clicked() {
                            self.tab_state.items_to_extract.lock().push(node.clone());
                        }
                    } else {
                        self.build_resource_tree_node(ui, node);
                    }
                }
            });

        if header.header_response.double_clicked() {
            self.tab_state.items_to_extract.lock().push(file_tree.clone());
        }
    }

    /// Builds a flat list of resource files from a [FileNode] iterator.
    fn build_file_list_from_array<'i, I>(&self, ui: &mut egui::Ui, files: I)
    where
        I: IntoIterator<Item = &'i (wowsunpack::Rc<PathBuf>, FileNode)>,
    {
        egui::Grid::new("filtered_files_grid").num_columns(1).striped(true).show(ui, |ui| {
            let files = files.into_iter();
            for file in files {
                let label = ui.add(Label::new(Path::new("res").join(&*file.0).to_string_lossy().into_owned()).sense(Sense::click()));
                self.add_view_file_menu(&label, &file.1);

                let text = if file.1.is_file() {
                    format!("File ({})", humansize::format_size(file.1.file_info().unwrap().size, humansize::DECIMAL))
                } else {
                    "Folder".to_string()
                };

                let label = label.on_hover_text(text);

                if label.double_clicked() {
                    self.tab_state.items_to_extract.lock().push(file.1.clone());
                }
                ui.end_row();
            }
        });
    }

    fn extract_files(&mut self, output_dir: &Path, items_to_unpack: &[FileNode]) {
        if let Some(pkg_loader) = self.pkg_loader() {
            let (tx, rx) = mpsc::channel();

            self.tab_state.unpacker_progress = Some(rx);
            UNPACKER_STOP.store(false, Ordering::Relaxed);

            if !items_to_unpack.is_empty() {
                let output_dir = output_dir.to_owned();
                let mut file_queue = items_to_unpack.to_vec();
                let _unpacker_thread = Some(std::thread::spawn(move || {
                    let mut files_to_extract: HashSet<FileNode> = HashSet::default();
                    let mut folders_created: HashSet<PathBuf> = HashSet::default();
                    while let Some(file) = file_queue.pop() {
                        if file.is_file() {
                            files_to_extract.insert(file);
                        } else {
                            for child in file.children().values() {
                                file_queue.push(child.clone());
                            }
                        }
                    }
                    let file_count = files_to_extract.len();

                    for (files_written, file) in files_to_extract.iter().enumerate() {
                        if UNPACKER_STOP.load(Ordering::Relaxed) {
                            break;
                        }

                        let path = output_dir.join(file.parent().unwrap().path().unwrap());
                        let file_path = path.join(file.filename());
                        tx.send(UnpackerProgress {
                            file_name: file_path.to_string_lossy().into(),
                            progress: (files_written as f32) / (file_count as f32),
                        })
                        .unwrap();
                        if !folders_created.contains(&path) {
                            fs::create_dir_all(&path).expect("failed to create folder");
                            folders_created.insert(path.clone());
                        }

                        let mut out_file = File::create(file_path).expect("failed to create output file");

                        file.read_file(&pkg_loader, &mut out_file).expect("Failed to read file");
                    }
                }));
            }
        }
    }

    fn extract_files_clicked(&mut self, _ui: &mut Ui) {
        let items_to_unpack = self.tab_state.items_to_extract.lock().clone();
        let output_dir = Path::new(self.tab_state.output_dir.as_str()).join("res");

        self.extract_files(output_dir.as_ref(), items_to_unpack.as_slice());
    }

    fn dump_game_params(&mut self, file_path: PathBuf, format: GameParamsFormat) {
        if let Some(pkg_loader) = self.pkg_loader() {
            let (tx, rx) = mpsc::channel();

            self.tab_state.unpacker_progress = Some(rx);
            UNPACKER_STOP.store(false, Ordering::Relaxed);

            // Find GameParams.json
            if let Some(wows_data) = self.tab_state.world_of_warships_data.as_ref() {
                let wows_data = wows_data.read();
                let metadata_provider = { wows_data.game_metadata.clone() };

                if let Ok(game_params_file) = wows_data.file_tree.find("content/GameParams.data") {
                    let mut game_params_data: Vec<u8> = Vec::with_capacity(game_params_file.file_info().unwrap().unpacked_size as usize);

                    game_params_file.read_file(&pkg_loader, &mut game_params_data).expect("failed to read GameParams");
                    let _unpacker_thread = Some(std::thread::spawn(move || {
                        tx.send(UnpackerProgress {
                            file_name: file_path.to_string_lossy().into(),
                            progress: 0.0,
                        })
                        .unwrap();

                        let pickle = game_params_to_pickle(game_params_data).expect("failed to deserialize GameParams");

                        let mut file = File::create(&file_path).expect("failed to create GameParams.json file");
                        match format {
                            GameParamsFormat::Json => {
                                let json = pickle_to_json(pickle);
                                serde_json::to_writer_pretty(&mut file, &json).expect("failed to write JSON data");
                            }
                            GameParamsFormat::Cbor => {
                                let cbor = pickle_to_cbor(pickle);
                                serde_cbor::to_writer(file, &cbor).expect("failed to write CBOR data");
                            }
                            GameParamsFormat::MinimalJson => {
                                if let Some(metadata_provider) = metadata_provider {
                                    serde_json::to_writer(file, &metadata_provider.params()).expect("failed to write CBOR data");
                                }
                            }
                            GameParamsFormat::MinimalCbor => {
                                if let Some(metadata_provider) = metadata_provider {
                                    serde_cbor::to_writer(file, &metadata_provider.params()).expect("failed to write CBOR data");
                                }
                            }
                        }

                        tx.send(UnpackerProgress {
                            file_name: file_path.to_string_lossy().into(),
                            progress: 1.0,
                        })
                        .unwrap();
                    }));
                }
            }
        }
    }

    /// Builds the file unpacker tab
    pub fn build_unpacker_tab(&mut self, ui: &mut egui::Ui) {
        egui::SidePanel::left("left").show_inside(ui, |ui| {
            ui.vertical(|ui| {
                if self.tab_state.used_filter.is_none() || self.tab_state.filter.as_str() != self.tab_state.used_filter.as_ref().unwrap().as_str() {
                    debug!("Filtering file listing again");
                    let filter_list = if let Some(wows_data) = self.tab_state.world_of_warships_data.as_ref() {
                        let wows_data = wows_data.read();
                        let files = &wows_data.filtered_files;
                        if self.tab_state.filter.len() >= 3 {
                            let glob = glob::Pattern::new(self.tab_state.filter.as_str());
                            if self.tab_state.filter.contains('*') && glob.is_ok() {
                                let glob = glob.unwrap();
                                let leafs: Vec<_> = files.iter().filter(|(path, _node)| glob.matches_path(path)).cloned().collect();

                                Some(leafs)
                            } else {
                                let leafs = files
                                    .iter()
                                    .filter(|(path, _node)| path.to_str().map(|path| path.contains(self.tab_state.filter.as_str())).unwrap_or(false))
                                    .cloned()
                                    .collect();

                                Some(leafs)
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    self.tab_state.filtered_file_list = filter_list.map(Arc::new);
                    self.tab_state.used_filter = Some(self.tab_state.filter.clone());
                }

                let filter_list = self.tab_state.filtered_file_list.clone();

                StripBuilder::new(ui).size(Size::exact(25.0)).size(Size::remainder()).vertical(|mut strip| {
                    strip.strip(|builder| {
                        builder.size(Size::remainder()).size(Size::exact(50.0)).horizontal(|mut strip| {
                            strip.cell(|ui| {
                                ui.add(egui::TextEdit::singleline(&mut self.tab_state.filter).hint_text("Filter"));
                            });
                            strip.cell(|ui| {
                                if let Some(filter_list) = &filter_list {
                                    if ui.button("Add All").clicked() {
                                        let mut items_to_extract = self.tab_state.items_to_extract.lock();
                                        for file in filter_list.iter() {
                                            items_to_extract.push(file.1.clone());
                                        }
                                    }
                                }
                            });
                        });
                    });
                    strip.cell(|ui| {
                        egui::ScrollArea::both().id_salt("file_tree_scroll_area").show(ui, |ui| {
                            if let Some(wows_data) = self.tab_state.world_of_warships_data.as_ref() {
                                let wows_data = wows_data.read();
                                let file_tree = &wows_data.file_tree;
                                if let Some(filtered_files) = &filter_list {
                                    self.build_file_list_from_array(ui, filtered_files.iter());
                                } else {
                                    self.build_resource_tree_node(ui, file_tree);
                                }
                            }
                        });
                    });
                });
            });
        });
        egui::CentralPanel::default().show_inside(ui, |ui| {
            StripBuilder::new(ui).size(Size::remainder()).size(Size::exact(20.0)).vertical(|mut strip| {
                strip.cell(|ui| {
                    ui.vertical(|ui| {
                        egui::ScrollArea::both().id_salt("selected_files_scroll_area").show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.heading("Selected Files");
                            });

                            ui.separator();

                            let mut items = self.tab_state.items_to_extract.lock();
                            let mut remove_idx = None;
                            for (i, item) in items.iter().enumerate() {
                                if ui
                                    .add(Label::new(Path::new("res").join(item.path().unwrap()).to_string_lossy().into_owned()).sense(Sense::click()))
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
                        .size(Size::exact(150.0))
                        .size(Size::exact(150.0))
                        .horizontal(|mut strip| {
                            strip.cell(|ui| {
                                ui.add_sized(
                                    ui.available_size(),
                                    egui::TextEdit::singleline(&mut self.tab_state.output_dir).hint_text("Output Path"),
                                );
                            });
                            strip.cell(|ui| {
                                if ui.button("Choose...").clicked() {
                                    let folder = rfd::FileDialog::new().pick_folder();
                                    if let Some(folder) = folder {
                                        self.tab_state.output_dir = folder.to_string_lossy().into_owned();
                                    }
                                }
                            });
                            strip.cell(|ui| {
                                if ui.button("Extract").clicked() {
                                    self.extract_files_clicked(ui);
                                }
                            });
                            strip.cell(|ui| {
                                ui.menu_button(format!("{} Dump GameParams", icons::FLOPPY_DISK), |ui| {
                                    if ui.small_button("As JSON").clicked() {
                                        if let Some(path) = rfd::FileDialog::new().set_file_name("GameParams.json").save_file() {
                                            self.dump_game_params(path, GameParamsFormat::Json);
                                        }
                                        ui.close_menu();
                                    }
                                    if ui.small_button("As CBOR").clicked() {
                                        if let Some(path) = rfd::FileDialog::new().set_file_name("GameParams.cbor").save_file() {
                                            self.dump_game_params(path, GameParamsFormat::Cbor);
                                        }
                                        ui.close_menu();
                                    }
                                    if ui.small_button("As JSON (Minimal / Transformed)").clicked() {
                                        if let Some(path) = rfd::FileDialog::new().set_file_name("MinGameParams.json").save_file() {
                                            self.dump_game_params(path, GameParamsFormat::MinimalJson);
                                        }
                                        ui.close_menu();
                                    }
                                    if ui.small_button("As CBOR (Minimal / Transformed)").clicked() {
                                        if let Some(path) = rfd::FileDialog::new().set_file_name("MinGameParams.cbor").save_file() {
                                            self.dump_game_params(path, GameParamsFormat::MinimalCbor);
                                        }
                                        ui.close_menu();
                                    }
                                });
                            });
                        });
                });
            });
        });
    }
}
