use std::{
    collections::HashSet,
    fs::{self, File},
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};

use egui::{CollapsingHeader, Label, Sense, Ui};
use egui_extras::{Size, StripBuilder};
use wowsunpack::idx::FileNode;

use crate::app::ToolkitTabViewer;
pub static UNPACKER_STOP: AtomicBool = AtomicBool::new(false);

pub struct UnpackerProgress {
    pub file_name: String,
    pub progress: f32,
}

impl ToolkitTabViewer<'_> {
    /// Builds a resource tree node from a [FileNode]
    fn build_resource_tree_node(&self, ui: &mut egui::Ui, file_tree: &FileNode) {
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
                    self.build_resource_tree_node(ui, node);
                }
            }
        });

        if header.header_response.double_clicked() {
            self.parent.items_to_extract.lock().push(file_tree.clone());
        }
    }

    /// Builds a flat list of resource files from a [FileNode] iterator.
    fn build_file_list_from_array<'i, I>(&self, ui: &mut egui::Ui, files: I)
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

    fn extract_files_clicked(&mut self, _ui: &mut Ui) {
        {
            let items_to_unpack = self.parent.items_to_extract.lock().clone();
            let output_dir = Path::new(self.parent.output_dir.as_str()).join("res");
            if let Some(pkg_loader) = self.parent.pkg_loader.clone() {
                let (tx, rx) = mpsc::channel();

                self.parent.unpacker_progress = Some(rx);
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
                                for (_, child) in file.children() {
                                    file_queue.push(child.clone());
                                }
                            }
                        }
                        let file_count = files_to_extract.len();
                        let mut files_written = 0;

                        for file in files_to_extract {
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
                                fs::create_dir_all(&path);
                                folders_created.insert(path.clone());
                            }

                            let mut out_file =
                                File::create(file_path).expect("failed to create output file");

                            file.read_file(&*pkg_loader, &mut out_file);
                            files_written += 1;
                        }
                    }));
                }
            }
        }
    }

    /// Builds the file unpacker tab
    pub fn build_unpacker_tab(&mut self, ui: &mut egui::Ui) {
        egui::SidePanel::left("left").show_inside(ui, |ui| {
            ui.vertical(|ui| {
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
                                    self.build_file_list_from_array(ui, leafs);
                                } else {
                                    let leafs = files.iter().filter(|(path, _node)| {
                                        path.to_str()
                                            .map(|path| path.contains(self.parent.filter.as_str()))
                                            .unwrap_or(false)
                                    });
                                    self.build_file_list_from_array(ui, leafs);
                                }
                            } else {
                                self.build_resource_tree_node(ui, file_tree);
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
                                        ui.heading("Selected Files");
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
                                        self.extract_files_clicked(ui);
                                    }
                                });
                            });
                    });
                });
        });
    }
}
