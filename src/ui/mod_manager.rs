#![allow(dead_code)]
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use crate::icons;
use egui::CollapsingHeader;
use egui::ImageSource;
use egui::Label;
use egui::Widget;
use egui_commonmark::CommonMarkViewer;
use egui_extras::Column;
use egui_extras::TableBuilder;
use parking_lot::Mutex;
use serde::Deserialize;
use serde::Serialize;

use crate::app::ToolkitTabViewer;

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
#[serde(default)]
/// Basically a copy
pub struct IndexModInfo {
    unique_id: String,
    category: String,
    author: String,
    name: String,
    description: String,
    repo_url: String,
    preview_images: Vec<String>,
    discord_approval_url: Option<String>,
    commit: String,
    /// Paths that should be pulled from the repo to install the mod
    paths: Vec<String>,
    /// Mod unique IDs that this mod depends on
    dependencies: Vec<String>,
}

// TODO: remove if we ever complete this feature.
impl IndexModInfo {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn repo_url(&self) -> &str {
        &self.repo_url
    }

    pub fn preview_images(&self) -> &[String] {
        &self.preview_images
    }

    pub fn discord_approval_url(&self) -> Option<&String> {
        self.discord_approval_url.as_ref()
    }

    pub fn unique_id(&self) -> &str {
        &self.unique_id
    }

    pub fn category(&self) -> &str {
        &self.category
    }

    pub fn author(&self) -> &str {
        &self.author
    }

    pub fn commit(&self) -> &str {
        &self.commit
    }

    pub fn paths(&self) -> &[String] {
        &self.paths
    }

    pub fn dependencies(&self) -> &[String] {
        &self.dependencies
    }
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct ModInfo {
    /// Wether a mod is enabled
    pub enabled: bool,
    /// Paths that were written for installing the mod. This is an arc/mutex
    /// since we will want to be able to be able to install / uninstall the mod without locking
    /// the whole structure.
    pub mod_paths: Arc<Mutex<Vec<PathBuf>>>,
    pub meta: IndexModInfo,
}

impl ModInfo {
    pub fn update_meta(&mut self, other: &ModInfo) {
        self.meta = other.meta.clone();
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ModManagerIndex {
    mods: Vec<IndexModInfo>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct ModManagerCategory {
    name: String,
    enabled: bool,
    mods: Vec<Rc<RefCell<ModInfo>>>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ModManagerInfo {
    index_hash: String,

    categories: Vec<(String, Vec<ModManagerCategory>)>,

    #[serde(skip)]
    selected_mod: Option<Rc<RefCell<ModInfo>>>,

    filter_text: String,
}

impl ModManagerInfo {
    /// Look up a mod by its unique ID
    pub fn mod_by_id(&self, id: &str) -> Option<Rc<RefCell<ModInfo>>> {
        self.categories
            .iter()
            .flat_map(|(_macro_category, categories)| categories.iter())
            .flat_map(|category| category.mods.iter())
            .find(|mod_info| mod_info.borrow().meta.unique_id == id)
            .map(Rc::clone)
    }

    pub fn update_index(&mut self, index_hash: String, index: ModManagerIndex) {
        println!("{index:#?}");
        self.index_hash = index_hash;
        self.update_categories(index.mods);
    }

    fn update_categories(&mut self, mods: Vec<IndexModInfo>) {
        let mut unique_categories: HashMap<String, HashMap<String, ModManagerCategory>> = HashMap::new();

        // Build a list of known previous mods
        let mut old_mods: HashMap<String, Rc<RefCell<ModInfo>>> = HashMap::new();
        for (_macro_group, categories) in &self.categories {
            for category in categories {
                for modi in &category.mods {
                    old_mods.insert(modi.borrow().meta.unique_id.clone(), Rc::clone(modi));
                }
            }
        }

        // Build a list of categories
        let mut category_status = HashMap::new();
        for (_macro_gropu, categories) in &self.categories {
            for category in categories {
                category_status.insert(category.name.clone(), category.enabled);
            }
        }

        // Build the new category list
        for modi in mods {
            let category_parts = modi.category.split('/').collect::<Vec<_>>();
            match (category_parts.first(), category_parts.get(1)) {
                (Some(macro_group), Some(micro_group)) => {
                    let previous_mod = old_mods.remove(&modi.unique_id);
                    let (enabled, mod_paths) = if let Some(previous) = previous_mod {
                        let previous = previous.borrow();
                        (previous.enabled, previous.mod_paths.clone())
                    } else {
                        Default::default()
                    };

                    let category_enabled = category_status.remove(&modi.category).unwrap_or(true);

                    unique_categories
                        .entry(macro_group.to_string())
                        .or_default()
                        .entry(micro_group.to_string())
                        .or_insert_with(|| ModManagerCategory { name: micro_group.to_string(), enabled: category_enabled, mods: Vec::new() })
                        .mods
                        .push(Rc::new(RefCell::new(ModInfo { meta: modi, enabled, mod_paths })));
                }
                _ => {
                    continue;
                }
            }
        }

        // We build an entirely new list from the source of truth. If we end up changing categories in the future,
        // I want to make sure we don't have any stale data in the UI.
        let mut mapped_categories: Vec<(String, Vec<ModManagerCategory>)> = Vec::new();
        for (macro_group, mut micro_groups) in unique_categories.drain() {
            if let Some((_macro_group, categories)) = mapped_categories.iter_mut().find(|(name, _)| name == &macro_group) {
                let mut new_categories: Vec<ModManagerCategory> = micro_groups.drain().map(|(_k, v)| v).collect();
                categories.append(&mut new_categories);
            } else {
                mapped_categories.push((macro_group, micro_groups.drain().map(|(_k, v)| v).collect()));
            }
        }

        mapped_categories.sort_by(|a, b| a.0.cmp(&b.0));
        for (_macro_group, categories) in &mut mapped_categories {
            categories.sort_by(|a, b| a.name.cmp(&b.name));
        }

        self.categories = mapped_categories;
    }
}

impl ToolkitTabViewer<'_> {
    pub fn build_mod_manager_tab(&mut self, ui: &mut egui::Ui) {
        let mod_manager_info = &mut self.tab_state.mod_manager_info;
        egui::SidePanel::left("mod_manager_left").show_inside(ui, |ui| {
            ui.heading("Category Filters");

            for (macro_group, categories) in &mut mod_manager_info.categories {
                CollapsingHeader::new(&*macro_group).default_open(true).show(ui, |ui| {
                    for category in categories {
                        ui.checkbox(&mut category.enabled, &category.name).changed();
                    }
                });
            }
        });

        egui::SidePanel::right("mod_manager_right").min_width(400.0).show_inside(ui, |ui| {
            if let Some(selected_mod_arc) = &mod_manager_info.selected_mod {
                let mut selected_mod = selected_mod_arc.borrow_mut();

                ui.vertical(|ui| {
                    ui.heading(&selected_mod.meta.name);
                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut selected_mod.enabled, "Enabled").changed() {
                            self.tab_state.mod_action_sender.send(selected_mod.clone()).expect("failed to send selected mod on mod_action_sender");
                        }
                        ui.hyperlink_to(format!("{} Mod Home Page", icons::BROWSER), &selected_mod.meta.repo_url);
                        if let Some(discord_url) = &selected_mod.meta.discord_approval_url {
                            ui.hyperlink_to(format!("{} Discord Approval Thread", icons::DISCORD_LOGO), discord_url);
                        }
                    });
                    CommonMarkViewer::new().show(ui, &mut self.tab_state.markdown_cache, &selected_mod.meta.description);
                    for image in selected_mod.meta.preview_images.iter() {
                        ui.add(egui::Image::new(ImageSource::Uri(image.into())).max_width(512.0).max_height(512.0));
                    }
                });
                ui.allocate_space(ui.available_size_before_wrap());
            }
        });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.button(format!("{} Save Mod Config", icons::FLOPPY_DISK)).clicked();
                ui.button(format!("{} Load Mod Config", icons::FOLDER)).clicked();
                ui.add(egui::TextEdit::singleline(&mut mod_manager_info.filter_text).hint_text("Filter"));
            });

            ui.vertical(|ui| {
                let table = TableBuilder::new(ui)
                    .striped(true)
                    .resizable(true)
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    .column(Column::auto().clip(false))
                    .column(Column::auto().clip(true))
                    .column(Column::auto().clip(true))
                    .column(Column::remainder())
                    .sense(egui::Sense::click());

                table
                    .header(20.0, |mut header| {
                        header.col(|ui| {
                            ui.strong("Enabled");
                        });
                        header.col(|ui| {
                            ui.strong("Name");
                        });
                        header.col(|ui| {
                            ui.strong("Author");
                        });
                        header.col(|ui| {
                            ui.strong("Category");
                        });
                    })
                    .body(|mut body| {
                        for (_macro_category, categories) in &mod_manager_info.categories {
                            for category in categories.iter().filter(|category| category.enabled) {
                                for mod_info_arc in &category.mods {
                                    let mut mod_info = mod_info_arc.borrow_mut();
                                    // Apply the filter
                                    // TODO: maybe just build a raw list of mods to show so we aren't doing work in the UI thread
                                    if !mod_manager_info.filter_text.is_empty()
                                        && (!mod_info.meta.name.to_lowercase().contains(&mod_manager_info.filter_text.to_lowercase())
                                            || mod_info.meta.author.to_lowercase().contains(&mod_manager_info.filter_text.to_lowercase()))
                                    {
                                        continue;
                                    }

                                    body.row(30.0, |mut row| {
                                        row.col(|ui| {
                                            if ui.checkbox(&mut mod_info.enabled, "").changed() {
                                                self.tab_state.mod_action_sender.send(mod_info.clone()).expect("failed to send mod info on mod_action_sender");
                                            }
                                        });
                                        row.col(|ui| {
                                            Label::new(&mod_info.meta.name).selectable(false).ui(ui);
                                        });
                                        row.col(|ui| {
                                            Label::new(&mod_info.meta.author).selectable(false).ui(ui);
                                        });
                                        row.col(|ui| {
                                            Label::new(&mod_info.meta.category).selectable(false).ui(ui);
                                        });

                                        if row.response().clicked() {
                                            mod_manager_info.selected_mod = Some(Rc::clone(mod_info_arc));
                                        }
                                    });
                                }
                            }
                        }
                    });
            })
        });
    }
}
