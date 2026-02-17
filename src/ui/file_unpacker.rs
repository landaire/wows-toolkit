use crate::icon_str;
use std::collections::HashSet;
use std::fs::File;
use std::fs::{self};
use std::io::BufWriter;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc;

use crate::icons;
use egui::CollapsingHeader;
use egui::Label;
use egui::Response;
use egui::Sense;
use egui::Ui;
use egui::UiKind;
use egui::mutex::Mutex;
use egui_extras::Size;
use egui_extras::StripBuilder;
use pickled::HashableValue;
use serde::Serialize;
use serde_cbor::ser::IoWrite;
use tracing::debug;
use wowsunpack::game_params::convert::game_params_to_pickle;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::vfs::VfsPath;

use crate::app::ToolkitTabViewer;
use crate::plaintext_viewer::FileType;
use crate::plaintext_viewer::{self};
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
    fn add_view_file_menu(&self, file_label: &Response, node: &VfsPath) {
        let filename = node.filename();
        let is_plaintext_file = PLAINTEXT_FILE_TYPES.iter().find(|extension| filename.ends_with(**extension));
        let is_image_file = IMAGE_FILE_TYPES.iter().find(|extension| filename.ends_with(**extension));

        if is_plaintext_file.is_some() || is_image_file.is_some() {
            file_label.context_menu(|ui| {
                if ui.button("View Contents").clicked() {
                    let mut file_contents = Vec::new();
                    if let Ok(mut reader) = node.open_file() {
                        let _ = reader.read_to_end(&mut file_contents);
                    }

                    let file_type = match (is_plaintext_file, is_image_file) {
                        (Some(ext), None) => String::from_utf8(file_contents)
                            .ok()
                            .map(|contents| FileType::PlainTextFile { ext: ext.to_string(), contents }),
                        (None, Some(_ext)) => Some(FileType::Image { contents: file_contents }),
                        (None, None) => None,
                        _ => unreachable!("this should be impossible"),
                    };

                    if let Some(file_type) = file_type {
                        let path_str = node.as_str().trim_start_matches('/');
                        let viewer = plaintext_viewer::PlaintextFileViewer {
                            title: Arc::new(format!("res/{path_str}")),
                            file_info: Arc::new(Mutex::new(file_type)),
                            open: Arc::new(AtomicBool::new(true)),
                        };

                        self.tab_state.file_viewer.lock().push(viewer);
                    }

                    ui.close_kind(UiKind::Menu);
                }
            });
        }
    }
    /// Builds a resource tree node from a [VfsPath]
    fn build_resource_tree_node(&self, ui: &mut egui::Ui, file_tree: &VfsPath) {
        let label = if file_tree.is_root() { "res".to_string() } else { file_tree.filename() };
        let header = CollapsingHeader::new(label).default_open(file_tree.is_root()).show(ui, |ui| {
            if let Ok(entries) = file_tree.read_dir() {
                let mut entries: Vec<VfsPath> = entries.collect();
                entries.sort_by(|a, b| {
                    let a_is_dir = a.is_dir().unwrap_or(false);
                    let b_is_dir = b.is_dir().unwrap_or(false);
                    b_is_dir.cmp(&a_is_dir).then_with(|| a.filename().cmp(&b.filename()))
                });
                for node in &entries {
                    if node.is_file().unwrap_or(false) {
                        let file_label = ui.add(Label::new(node.filename()).sense(Sense::click()));
                        self.add_view_file_menu(&file_label, node);
                        if file_label.double_clicked() {
                            self.tab_state.items_to_extract.lock().push(node.clone());
                        }
                    } else {
                        self.build_resource_tree_node(ui, node);
                    }
                }
            }
        });

        if header.header_response.double_clicked() {
            self.tab_state.items_to_extract.lock().push(file_tree.clone());
        }
    }

    /// Builds a flat list of resource files from a VfsPath iterator.
    fn build_file_list_from_array<'i, I>(&self, ui: &mut egui::Ui, files: I)
    where
        I: IntoIterator<Item = &'i (Arc<PathBuf>, VfsPath)>,
    {
        egui::Grid::new("filtered_files_grid").num_columns(1).striped(true).show(ui, |ui| {
            let files = files.into_iter();
            for file in files {
                let display_path = format!("res/{}", file.0.display());
                let label = ui.add(Label::new(display_path).sense(Sense::click()));
                self.add_view_file_menu(&label, &file.1);

                let text = if file.1.is_file().unwrap_or(false) {
                    let size = file.1.metadata().map(|m| m.len).unwrap_or(0);
                    format!("File ({})", humansize::format_size(size, humansize::DECIMAL))
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

    fn extract_files(&mut self, output_dir: &Path, items_to_unpack: &[VfsPath]) {
        let (tx, rx) = mpsc::channel();

        self.tab_state.unpacker_progress = Some(rx);
        UNPACKER_STOP.store(false, Ordering::Relaxed);

        if !items_to_unpack.is_empty() {
            let output_dir = output_dir.to_owned();
            let mut file_queue = items_to_unpack.to_vec();
            let _unpacker_thread = Some(std::thread::spawn(move || {
                let mut files_to_extract: Vec<VfsPath> = Vec::new();
                let mut folders_created: HashSet<PathBuf> = HashSet::default();
                while let Some(file) = file_queue.pop() {
                    if file.is_file().unwrap_or(false) {
                        files_to_extract.push(file);
                    } else if let Ok(entries) = file.read_dir() {
                        for child in entries {
                            file_queue.push(child);
                        }
                    }
                }
                let file_count = files_to_extract.len();

                for (files_written, file) in files_to_extract.iter().enumerate() {
                    if UNPACKER_STOP.load(Ordering::Relaxed) {
                        break;
                    }

                    let vfs_path_str = file.as_str().trim_start_matches('/');
                    let parent_path = Path::new(vfs_path_str).parent().unwrap_or(Path::new(""));
                    let path = output_dir.join(parent_path);
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

                    if let Ok(mut reader) = file.open_file() {
                        std::io::copy(&mut reader, &mut out_file).expect("Failed to extract file");
                    }
                }
            }));
        }
    }

    fn extract_files_clicked(&mut self, _ui: &mut Ui) {
        let items_to_unpack = self.tab_state.items_to_extract.lock().clone();
        let output_dir = Path::new(self.tab_state.output_dir.as_str()).join("res");

        self.extract_files(output_dir.as_ref(), items_to_unpack.as_slice());
    }

    fn dump_game_params(&mut self, file_path: PathBuf, format: GameParamsFormat, base_params: bool) {
        let (tx, rx) = mpsc::channel();

        self.tab_state.unpacker_progress = Some(rx);
        UNPACKER_STOP.store(false, Ordering::Relaxed);

        if let Some(wows_data) = self.tab_state.world_of_warships_data.as_ref() {
            let wows_data = wows_data.read();
            let metadata_provider = { wows_data.game_metadata.clone() };
            let vfs = wows_data.vfs.clone();

            let game_params_path = vfs.join("content/GameParams.data");
            if let Ok(game_params_vfs) = game_params_path {
                let _unpacker_thread = Some(std::thread::spawn(move || {
                    let mut game_params_data = Vec::new();

                    game_params_vfs
                        .open_file()
                        .and_then(|mut f| Ok(f.read_to_end(&mut game_params_data)?))
                        .expect("failed to read GameParams");

                    tx.send(UnpackerProgress { file_name: file_path.to_string_lossy().into(), progress: 0.0 }).unwrap();

                    let pickle = game_params_to_pickle(game_params_data).expect("failed to deserialize GameParams");
                    let params_dict = if base_params {
                        match pickle {
                            pickled::Value::Dict(params_dict) => params_dict
                                .inner()
                                .get(&HashableValue::String("".to_string().into()))
                                .expect("Could not find base game params with empty key")
                                .clone(),
                            pickled::Value::List(params_list) => params_list.inner_mut().remove(0),
                            _other => {
                                panic!("Unexpected GameParams root element type");
                            }
                        }
                    } else {
                        pickle
                    };

                    let file = BufWriter::new(File::create(&file_path).expect("failed to create GameParams.json file"));
                    match format {
                        GameParamsFormat::Json => {
                            let mut serializer = serde_json::Serializer::pretty(file);
                            params_dict.serialize(&mut serializer).expect("failed to write GameParams data");
                        }
                        GameParamsFormat::Cbor => {
                            let file = IoWrite::new(file);
                            let mut serializer = serde_cbor::Serializer::new(file);
                            params_dict.serialize(&mut serializer).expect("failed to write GameParams data");
                        }
                        GameParamsFormat::MinimalJson => {
                            if let Some(metadata_provider) = metadata_provider {
                                serde_json::to_writer(file, &metadata_provider.params())
                                    .expect("failed to write CBOR data");
                            }
                        }
                        GameParamsFormat::MinimalCbor => {
                            if let Some(metadata_provider) = metadata_provider {
                                serde_cbor::to_writer(file, &metadata_provider.params())
                                    .expect("failed to write CBOR data");
                            }
                        }
                    }

                    tx.send(UnpackerProgress { file_name: file_path.to_string_lossy().into(), progress: 1.0 }).unwrap();
                }));
            }
        }
    }

    /// Returns the SharedWoWsData for the currently selected browser build.
    fn selected_browser_data(&self) -> Option<crate::wows_data::SharedWoWsData> {
        let map = self.tab_state.wows_data_map.as_ref()?;
        map.get(self.tab_state.selected_browser_build)
    }

    /// Builds the file unpacker tab
    pub fn build_unpacker_tab(&mut self, ui: &mut egui::Ui) {
        egui::SidePanel::left("left").show_inside(ui, |ui| {
            ui.vertical(|ui| {
                // Version selector dropdown (only shown when multiple builds are available)
                if self.tab_state.available_builds.len() > 1
                    && let Some(map) = &self.tab_state.wows_data_map
                {
                    let mut builds = self.tab_state.available_builds.clone();
                    builds.sort();
                    builds.reverse();

                    let selected_label = format!("{}", self.tab_state.selected_browser_build);

                    ui.horizontal(|ui| {
                        ui.label("Version:");
                        egui::ComboBox::from_id_salt("browser_version_select").selected_text(&selected_label).show_ui(
                            ui,
                            |ui| {
                                for &build in &builds {
                                    let is_loaded = map.get(build).is_some();
                                    let label = format!("{}", build);
                                    if ui
                                        .selectable_value(&mut self.tab_state.selected_browser_build, build, &label)
                                        .changed()
                                    {
                                        self.tab_state.filtered_file_list = None;
                                        self.tab_state.used_filter = None;

                                        // Load build data on-demand if not already loaded
                                        if !is_loaded {
                                            let map = map.clone();
                                            let (tx, rx) = std::sync::mpsc::channel();
                                            std::thread::spawn(move || match map.resolve_build(build) {
                                                Some(_) => {
                                                    let _ = tx.send(Ok(
                                                        crate::task::BackgroundTaskCompletion::BuildDataLoaded {
                                                            build,
                                                        },
                                                    ));
                                                }
                                                None => {
                                                    let report: rootcause::Report =
                                                        crate::error::ToolkitError::ReplayBuildUnavailable {
                                                            build,
                                                            version: format!("{}", build),
                                                        }
                                                        .into();
                                                    let _ = tx
                                                        .send(Err(report
                                                            .attach("game data could not be loaded for this build")));
                                                }
                                            });
                                            let _ = self.tab_state.background_task_sender.send(
                                                crate::task::BackgroundTask {
                                                    receiver: Some(rx),
                                                    kind: crate::task::BackgroundTaskKind::LoadingBuildData(build),
                                                },
                                            );
                                        }
                                    }
                                }
                            },
                        );
                    });
                    ui.separator();
                }

                if self.tab_state.used_filter.is_none()
                    || self.tab_state.filter.as_str() != self.tab_state.used_filter.as_ref().unwrap().as_str()
                {
                    debug!("Filtering file listing again");
                    let filter_list = if let Some(wows_data) =
                        self.selected_browser_data().or_else(|| self.tab_state.world_of_warships_data.clone())
                    {
                        let wows_data = wows_data.read();
                        let files = &wows_data.filtered_files;
                        if self.tab_state.filter.len() >= 3 {
                            let glob = glob::Pattern::new(self.tab_state.filter.as_str());
                            if self.tab_state.filter.contains('*')
                                && let Ok(glob) = glob
                            {
                                let leafs: Vec<_> =
                                    files.iter().filter(|(path, _node)| glob.matches_path(path)).cloned().collect();

                                Some(leafs)
                            } else {
                                let leafs = files
                                    .iter()
                                    .filter(|(path, _node)| {
                                        path.to_str()
                                            .map(|path| path.contains(self.tab_state.filter.as_str()))
                                            .unwrap_or(false)
                                    })
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
                                if let Some(filter_list) = &filter_list
                                    && ui.button("Add All").clicked()
                                {
                                    let mut items_to_extract = self.tab_state.items_to_extract.lock();
                                    for file in filter_list.iter() {
                                        items_to_extract.push(file.1.clone());
                                    }
                                }
                            });
                        });
                    });
                    strip.cell(|ui| {
                        egui::ScrollArea::both().id_salt("file_tree_scroll_area").show(ui, |ui| {
                            if let Some(wows_data) =
                                self.selected_browser_data().or_else(|| self.tab_state.world_of_warships_data.clone())
                            {
                                let wows_data = wows_data.read();
                                let vfs = &wows_data.vfs;
                                if let Some(filtered_files) = &filter_list {
                                    self.build_file_list_from_array(ui, filtered_files.iter());
                                } else {
                                    self.build_resource_tree_node(ui, vfs);
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
                                let path_str = item.as_str().trim_start_matches('/');
                                if ui.add(Label::new(format!("res/{path_str}")).sense(Sense::click())).double_clicked()
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

                strip.cell(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.menu_button(icon_str!(icons::FLOPPY_DISK, "Dump GameParams"), |ui| {
                            if ui.small_button("Base As JSON").clicked() {
                                if let Some(path) = rfd::FileDialog::new().set_file_name("GameParams.json").save_file()
                                {
                                    self.dump_game_params(path, GameParamsFormat::Json, true);
                                }
                                ui.close_kind(UiKind::Menu);
                            }
                            if ui.small_button("As JSON").clicked() {
                                if let Some(path) = rfd::FileDialog::new().set_file_name("GameParams.json").save_file()
                                {
                                    self.dump_game_params(path, GameParamsFormat::Json, false);
                                }
                                ui.close_kind(UiKind::Menu);
                            }
                            if ui.small_button("As CBOR").clicked() {
                                if let Some(path) = rfd::FileDialog::new().set_file_name("GameParams.cbor").save_file()
                                {
                                    self.dump_game_params(path, GameParamsFormat::Cbor, false);
                                }
                                ui.close_kind(UiKind::Menu);
                            }
                            if ui.small_button("As JSON (Minimal / Transformed)").clicked() {
                                if let Some(path) =
                                    rfd::FileDialog::new().set_file_name("MinGameParams.json").save_file()
                                {
                                    self.dump_game_params(path, GameParamsFormat::MinimalJson, false);
                                }
                                ui.close_kind(UiKind::Menu);
                            }
                            if ui.small_button("As CBOR (Minimal / Transformed)").clicked() {
                                if let Some(path) =
                                    rfd::FileDialog::new().set_file_name("MinGameParams.cbor").save_file()
                                {
                                    self.dump_game_params(path, GameParamsFormat::MinimalCbor, false);
                                }
                                ui.close_kind(UiKind::Menu);
                            }
                        });

                        if ui.button("Extract").clicked() {
                            self.extract_files_clicked(ui);
                        }

                        if ui.button("Choose...").clicked() {
                            let folder = rfd::FileDialog::new().pick_folder();
                            if let Some(folder) = folder {
                                self.tab_state.output_dir = folder.to_string_lossy().into_owned();
                            }
                        }

                        ui.add_sized(
                            ui.available_size(),
                            egui::TextEdit::singleline(&mut self.tab_state.output_dir).hint_text("Output Path"),
                        );
                    });
                });
            });
        });
    }
}
