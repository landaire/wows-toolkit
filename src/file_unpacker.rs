use std::{
    collections::HashSet,
    fs::{self, File},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
};

use egui::{mutex::Mutex, CollapsingHeader, Label, Response, Sense, Ui};
use egui_extras::{Size, StripBuilder};
use wowsunpack::{idx::FileNode, pkg::PkgFileLoader};

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

impl ToolkitTabViewer<'_> {
    fn pkg_loader(&self) -> Option<Arc<PkgFileLoader>> {
        self.tab_state.world_of_warships_data.as_ref().map(|wows_data| wows_data.pkg_loader.clone())
    }

    fn add_view_file_menu(&self, file_label: Response, node: &FileNode) -> Response {
        let is_plaintext_file = PLAINTEXT_FILE_TYPES.iter().find(|extension| node.filename().ends_with(**extension));
        let is_image_file = IMAGE_FILE_TYPES.iter().find(|extension| node.filename().ends_with(**extension));

        let file_label = if is_plaintext_file.is_some() || is_image_file.is_some() {
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
            })
        } else {
            file_label
        };

        file_label
    }
    /// Builds a resource tree node from a [FileNode]
    fn build_resource_tree_node(&self, ui: &mut egui::Ui, file_tree: &FileNode) {
        let header = CollapsingHeader::new(if file_tree.is_root() { "res" } else { file_tree.filename() })
            .default_open(file_tree.is_root())
            .show(ui, |ui| {
                for (name, node) in file_tree.children() {
                    if node.is_file() {
                        let file_label = ui.add(Label::new(name).sense(Sense::click()));
                        let file_label = self.add_view_file_menu(file_label, node);
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
                let label = self.add_view_file_menu(label, &file.1);

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

    fn extract_files_clicked(&mut self, _ui: &mut Ui) {
        let items_to_unpack = self.tab_state.items_to_extract.lock().clone();
        let output_dir = Path::new(self.tab_state.output_dir.as_str()).join("res");
        if let Some(pkg_loader) = self.pkg_loader() {
            let (tx, rx) = mpsc::channel();

            self.tab_state.unpacker_progress = Some(rx);
            UNPACKER_STOP.store(false, Ordering::Relaxed);

            if !items_to_unpack.is_empty() {
                let _unpacker_thread = Some(std::thread::spawn(move || {
                    let mut file_queue = items_to_unpack.clone();
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

    /// Builds the file unpacker tab
    pub fn build_unpacker_tab(&mut self, ui: &mut egui::Ui) {
        egui::SidePanel::left("left").show_inside(ui, |ui| {
            ui.vertical(|ui| {
                let filter_list = if let Some(wows_data) = self.tab_state.world_of_warships_data.as_ref() {
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
                                        for file in filter_list {
                                            items_to_extract.push(file.1.clone());
                                        }
                                    }
                                }
                            });
                        });
                    });
                    strip.cell(|ui| {
                        egui::ScrollArea::both().id_source("file_tree_scroll_area").show(ui, |ui| {
                            if let Some(wows_data) = self.tab_state.world_of_warships_data.as_ref() {
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
                        egui::ScrollArea::both().id_source("selected_files_scroll_area").show(ui, |ui| {
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
                        });
                });
            });
        });
    }
}
