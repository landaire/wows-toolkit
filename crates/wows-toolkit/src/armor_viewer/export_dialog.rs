//! The ship model export dialog: what goes into the GLB, and what it costs.

use std::collections::BTreeSet;
use std::sync::Arc;

use rust_i18n::t;
use wowsunpack::export::camo_textures::CamoSchemeId;
use wowsunpack::export::camo_textures::CamoSchemeInfo;
use wowsunpack::export::gltf_export::CamoOrigin;
use wowsunpack::export::ship::CamoSelection;
use wowsunpack::export::ship::ExportContents;
use wowsunpack::export::ship::ShipExportOptions;
use wowsunpack::export::size_estimate::ExportSizeModel;
use wowsunpack::export::texture::MaxEdge;
use wowsunpack::export::texture::TextureLod;

/// The texture detail levels the dialog offers. A cap, not a tier count, because
/// the user thinks in pixels and the authored ladders differ per texture.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum TextureResolution {
    #[default]
    Full,
    Px2048,
    Px1024,
    Px512,
}

impl TextureResolution {
    pub const ALL: [Self; 4] = [Self::Full, Self::Px2048, Self::Px1024, Self::Px512];

    pub fn to_texture_lod(self) -> TextureLod {
        match self {
            // Full detail is the absence of a cap, not a very large one.
            Self::Full => TextureLod::Full,
            Self::Px2048 => TextureLod::Capped(MaxEdge::new(2048).expect("2048 is a valid edge")),
            Self::Px1024 => TextureLod::Capped(MaxEdge::new(1024).expect("1024 is a valid edge")),
            Self::Px512 => TextureLod::Capped(MaxEdge::new(512).expect("512 is a valid edge")),
        }
    }

    pub const fn label_key(self) -> &'static str {
        match self {
            Self::Full => "ui.armor.export.res_full",
            Self::Px2048 => "ui.armor.export.res_2048",
            Self::Px1024 => "ui.armor.export.res_1024",
            Self::Px512 => "ui.armor.export.res_512",
        }
    }
}

pub const fn contents_label_key(contents: ExportContents) -> &'static str {
    match contents {
        ExportContents::Mesh => "ui.armor.export.contents_mesh",
        ExportContents::Armor => "ui.armor.export.contents_armor",
        ExportContents::MeshAndArmor => "ui.armor.export.contents_both",
    }
}

/// The user's in-progress choices.
pub struct ExportDraft {
    pub contents: ExportContents,
    pub hull: Option<String>,
    pub lod: usize,
    pub texture_res: TextureResolution,
    pub camos: BTreeSet<CamoSchemeId>,
}

/// The options a draft describes. One ticked camo bakes: it is the smallest
/// output and needs no variants extension, so it imports anywhere.
pub fn draft_to_options(draft: &ExportDraft) -> ShipExportOptions {
    let camos = if draft.contents.includes_mesh() {
        let mut ids: Vec<CamoSchemeId> = draft.camos.iter().copied().collect();
        match ids.len() {
            0 => CamoSelection::BaseOnly,
            1 => CamoSelection::Baked(ids.remove(0)),
            _ => CamoSelection::Variants(ids),
        }
    } else {
        CamoSelection::BaseOnly
    };

    ShipExportOptions {
        lod: draft.lod,
        hull: draft.hull.clone(),
        // Armor is untextured, so an armor-only export never reads a DDS.
        textures: draft.contents.includes_mesh(),
        damaged: false,
        contents: draft.contents,
        texture_lod: draft.texture_res.to_texture_lod(),
        camos,
        module_overrides: Default::default(),
    }
}

/// The scheme the dialog pre-ticks. Matched on the raw camo name as well as the
/// label, so a locale that translates "Default" does not hide it. A ship with no
/// such scheme starts with nothing ticked, which exports its stock appearance.
pub fn default_camo(schemes: &[CamoSchemeInfo]) -> Option<CamoSchemeId> {
    schemes
        .iter()
        .find(|s| s.raw_name.eq_ignore_ascii_case("default") || s.display_name.eq_ignore_ascii_case("default"))
        .map(|s| s.id)
}

/// Everything the dialog needs about one ship at one hull selection.
pub struct ExportMeta {
    pub hull_upgrades: Vec<String>,
    pub hull_lod_count: usize,
    pub camo_schemes: Vec<CamoSchemeInfo>,
    /// `None` when the model could not be built. The dialog then shows no size
    /// line rather than a fabricated number, and the export still works.
    pub size_model: Option<Arc<ExportSizeModel>>,
}

pub enum ExportMetaState {
    Loading(egui_inbox::UiInbox<Result<ExportMeta, String>>),
    Loaded(ExportMeta),
    Failed(String),
}

/// A pending ship model export.
pub struct ExportDialog {
    pub param_index: String,
    pub display_name: String,
    pub meta: ExportMetaState,
    pub draft: ExportDraft,
}

/// The export choices that carry across ships. A `CamoSchemeId` indexes one
/// ship's ordered scheme list and a hull names one ship's upgrade, so neither is
/// meaningful for the next ship and neither belongs here. Task 10 adds the
/// persistence; this task needs the type because `open` takes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExportDefaults {
    pub contents: ExportContents,
    pub lod: usize,
    pub texture_res: TextureResolution,
}

impl Default for ExportDefaults {
    fn default() -> Self {
        // What the armor viewer's export button produced before the dialog existed.
        Self { contents: ExportContents::MeshAndArmor, lod: 0, texture_res: TextureResolution::Full }
    }
}

impl ExportDialog {
    /// Open for a ship, seeding the hull from whatever the caller has on screen.
    /// The tree's context menu has no loaded ship, so it passes `None`.
    pub fn open(
        param_index: String,
        display_name: String,
        seed_hull: Option<String>,
        defaults: ExportDefaults,
        assets: Arc<wowsunpack::export::ship::ShipAssets>,
    ) -> Self {
        let draft = ExportDraft {
            contents: defaults.contents,
            hull: seed_hull,
            lod: defaults.lod,
            texture_res: defaults.texture_res,
            camos: BTreeSet::new(),
        };
        // `start_load` sets `meta`, so it is built here rather than seeded with a
        // placeholder state that would never be observed.
        let (sender, inbox) = egui_inbox::UiInbox::channel();
        let dialog = Self { param_index, display_name, meta: ExportMetaState::Loading(inbox), draft };
        dialog.spawn_load(assets, sender);
        dialog
    }

    /// Reload after a hull change: the hull decides which parts exist, and so the
    /// camo list, the LoD depth and the size model.
    pub fn start_load(&mut self, assets: Arc<wowsunpack::export::ship::ShipAssets>) {
        let (sender, inbox) = egui_inbox::UiInbox::channel();
        self.meta = ExportMetaState::Loading(inbox);
        self.spawn_load(assets, sender);
    }

    /// Load the ship's geometry only: the controls need the hull list, the LoD
    /// depth, the camo list and the size model, none of which need textures.
    fn spawn_load(
        &self,
        assets: Arc<wowsunpack::export::ship::ShipAssets>,
        sender: egui_inbox::UiInboxSender<Result<ExportMeta, String>>,
    ) {
        let param_index = self.param_index.clone();
        let hull = self.draft.hull.clone();
        crate::util::thread::spawn_logged("export-dialog-load", move || {
            let result = (|| -> Result<ExportMeta, String> {
                use wowsunpack::game_params::types::GameParamProvider;
                let param = assets.metadata().game_param_by_index(&param_index);
                let vehicle =
                    param.as_ref().and_then(|p| p.vehicle().cloned()).ok_or_else(|| "Vehicle not found".to_string())?;
                let probe = ShipExportOptions {
                    hull: hull.clone(),
                    textures: false,
                    contents: ExportContents::Mesh,
                    ..Default::default()
                };
                let ctx = assets.load_ship_from_vehicle(&vehicle, &probe).map_err(|e| format!("{e:?}"))?;
                let source = ctx.camo_texture_source().map_err(|e| format!("{e:?}"))?;
                // A ship that cannot be priced can still be exported, so a failed
                // model costs the estimate line and nothing else.
                let size_model = match ctx.size_model() {
                    Ok(m) => Some(Arc::new(m)),
                    Err(e) => {
                        tracing::warn!("could not build the export size model for {param_index}: {e:?}");
                        None
                    }
                };
                // A ship with no listed upgrades (older game versions) simply
                // offers no hull choice.
                let hull_upgrades = assets
                    .list_hull_upgrades(&param_index)
                    .map(|ups| ups.into_iter().map(|u| u.name).collect())
                    .unwrap_or_default();
                Ok(ExportMeta {
                    hull_upgrades,
                    hull_lod_count: ctx.hull_lod_count(),
                    camo_schemes: source.scheme_infos(),
                    size_model,
                })
            })();
            let _ = sender.send(result);
        });
    }
}

pub enum DialogOutcome {
    /// Still open, nothing to do.
    Idle,
    /// The hull changed; reload the metadata.
    ReloadRequested,
    /// The user confirmed. Carries the resolved options.
    Export(Box<ShipExportOptions>),
    Cancelled,
}

pub fn show(dialog: &mut ExportDialog, ctx: &egui::Context) -> DialogOutcome {
    let mut outcome = DialogOutcome::Idle;
    egui::Window::new(t!("ui.armor.export_model").as_ref())
        .collapsible(false)
        .resizable(true)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            if let ExportMetaState::Loading(inbox) = &mut dialog.meta
                && let Some(result) = inbox.read(ui).last()
            {
                dialog.meta = match result {
                    Ok(meta) => {
                        dialog.draft.camos.clear();
                        if let Some(id) = default_camo(&meta.camo_schemes) {
                            dialog.draft.camos.insert(id);
                        }
                        dialog.draft.lod = dialog.draft.lod.min(meta.hull_lod_count.saturating_sub(1));
                        ExportMetaState::Loaded(meta)
                    }
                    Err(e) => ExportMetaState::Failed(e),
                };
            }

            match &dialog.meta {
                ExportMetaState::Loading(_) => {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().size(14.0));
                        ui.label(t!("ui.armor.loading_ship").as_ref());
                    });
                }
                ExportMetaState::Failed(e) => {
                    ui.colored_label(ui.visuals().error_fg_color, t!("ui.armor.load_failed", error = e).as_ref());
                }
                ExportMetaState::Loaded(meta) => {
                    outcome = controls(ui, &mut dialog.draft, meta);
                }
            }

            ui.add_space(8.0);
            ui.label(t!("ui.armor.export_disclaimer").as_ref());
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let ready = matches!(dialog.meta, ExportMetaState::Loaded(_));
                if ui.add_enabled(ready, egui::Button::new(t!("ui.armor.export_button").as_ref())).clicked() {
                    outcome = DialogOutcome::Export(Box::new(draft_to_options(&dialog.draft)));
                }
                if ui.button(t!("ui.buttons.cancel").as_ref()).clicked() {
                    outcome = DialogOutcome::Cancelled;
                }
            });
        });
    outcome
}

/// Renders hull, contents, LoD, texture resolution and camo controls. Returns
/// `ReloadRequested` when the hull selection changed, since the hull decides
/// which parts, camos and LoDs even exist.
fn controls(ui: &mut egui::Ui, draft: &mut ExportDraft, meta: &ExportMeta) -> DialogOutcome {
    let mut outcome = DialogOutcome::Idle;

    if !meta.hull_upgrades.is_empty() {
        let selected_label = draft
            .hull
            .as_ref()
            .and_then(|h| meta.hull_upgrades.iter().find(|u| *u == h))
            .cloned()
            .unwrap_or_else(|| t!("ui.armor.export.hull_stock").to_string());
        ui.horizontal(|ui| {
            ui.label(t!("ui.armor.export.hull").as_ref());
            egui::ComboBox::from_id_salt("export_hull_combo").selected_text(selected_label).show_ui(ui, |ui| {
                if ui.selectable_label(draft.hull.is_none(), t!("ui.armor.export.hull_stock").as_ref()).clicked()
                    && draft.hull.is_some()
                {
                    draft.hull = None;
                    outcome = DialogOutcome::ReloadRequested;
                }
                for name in &meta.hull_upgrades {
                    let is_selected = draft.hull.as_ref() == Some(name);
                    if ui.selectable_label(is_selected, name).clicked() && !is_selected {
                        draft.hull = Some(name.clone());
                        outcome = DialogOutcome::ReloadRequested;
                    }
                }
            });
        });
    }

    ui.horizontal(|ui| {
        ui.label(t!("ui.armor.export.contents").as_ref());
        for contents in [ExportContents::Mesh, ExportContents::Armor, ExportContents::MeshAndArmor] {
            ui.radio_value(&mut draft.contents, contents, t!(contents_label_key(contents)).as_ref());
        }
    });

    if meta.hull_lod_count > 1 {
        ui.add_enabled(
            draft.contents.includes_mesh(),
            egui::Slider::new(&mut draft.lod, 0..=meta.hull_lod_count.saturating_sub(1))
                .text(t!("ui.armor.export.lod")),
        )
        .on_hover_text(t!("ui.armor.export.lod_tooltip").as_ref());
    }

    ui.add_enabled_ui(draft.contents.includes_mesh(), |ui| {
        ui.horizontal(|ui| {
            ui.label(t!("ui.armor.export.res").as_ref());
            egui::ComboBox::from_id_salt("export_texture_res_combo")
                .selected_text(t!(draft.texture_res.label_key()).as_ref())
                .show_ui(ui, |ui| {
                    for res in TextureResolution::ALL {
                        ui.selectable_value(&mut draft.texture_res, res, t!(res.label_key()).as_ref());
                    }
                });
        });
    });

    if !meta.camo_schemes.is_empty() {
        ui.add_enabled_ui(draft.contents.includes_mesh(), |ui| {
            ui.label(t!("ui.armor.export.camos").as_ref());
            ui.horizontal(|ui| {
                if ui.button(t!("ui.armor.export.camo_select_none").as_ref()).clicked() {
                    draft.camos.clear();
                }
                if ui.button(t!("ui.armor.export.camo_reset").as_ref()).clicked() {
                    draft.camos.clear();
                    if let Some(id) = default_camo(&meta.camo_schemes) {
                        draft.camos.insert(id);
                    }
                }
            });

            let mut ship_infos: Vec<&CamoSchemeInfo> =
                meta.camo_schemes.iter().filter(|i| i.origin == CamoOrigin::ShipSpecific).collect();
            ship_infos.sort_by(|a, b| a.display_name.to_lowercase().cmp(&b.display_name.to_lowercase()));
            for info in &ship_infos {
                camo_checkbox(ui, draft, info);
            }

            for (origin, key) in [
                (CamoOrigin::Universal, "ui.armor.camo_group_universal"),
                (CamoOrigin::Expendable, "ui.armor.camo_group_expendable"),
                (CamoOrigin::LegacyScan, "ui.armor.camo_group_other"),
            ] {
                let mut group: Vec<&CamoSchemeInfo> = meta.camo_schemes.iter().filter(|i| i.origin == origin).collect();
                if group.is_empty() {
                    continue;
                }
                group.sort_by(|a, b| a.display_name.to_lowercase().cmp(&b.display_name.to_lowercase()));
                let id = ui.make_persistent_id(("export_camo_group", key));
                egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false)
                    .show_header(ui, |ui| {
                        ui.label(t!(key).as_ref());
                    })
                    .body(|ui| {
                        egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
                            for info in &group {
                                camo_checkbox(ui, draft, info);
                            }
                        });
                    });
            }
        });
    }

    // Recomputed from the live draft on every frame: cheap arithmetic over
    // metadata already loaded, not a re-decode.
    if let Some(model) = &meta.size_model {
        let estimate = model.estimate(&draft_to_options(draft));
        let size = humansize::format_size(estimate.total(), humansize::BINARY);
        ui.label(t!("ui.armor.export.size_estimate", size = size).as_ref());
    }

    outcome
}

fn camo_checkbox(ui: &mut egui::Ui, draft: &mut ExportDraft, info: &CamoSchemeInfo) {
    let mut checked = draft.camos.contains(&info.id);
    if ui.checkbox(&mut checked, info.display_name.as_str()).changed() {
        if checked {
            draft.camos.insert(info.id);
        } else {
            draft.camos.remove(&info.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use wowsunpack::export::camo_textures::CamoSchemeId;
    use wowsunpack::export::camo_textures::CamoSchemeInfo;
    use wowsunpack::export::gltf_export::CamoOrigin;
    use wowsunpack::export::ship::CamoSelection;
    use wowsunpack::export::ship::ExportContents;
    use wowsunpack::export::texture::MaxEdge;
    use wowsunpack::export::texture::TextureLod;

    use super::*;

    fn draft() -> ExportDraft {
        ExportDraft {
            contents: ExportContents::MeshAndArmor,
            hull: None,
            lod: 0,
            texture_res: TextureResolution::Full,
            camos: BTreeSet::new(),
        }
    }

    fn scheme(id: usize, display_name: &str, raw_name: &str) -> CamoSchemeInfo {
        CamoSchemeInfo {
            id: CamoSchemeId(id),
            display_name: display_name.to_string(),
            raw_name: raw_name.to_string(),
            origin: CamoOrigin::ShipSpecific,
            use_color_scheme: false,
            uv_transforms: Default::default(),
        }
    }

    #[test]
    fn no_camo_ticked_exports_the_stock_appearance() {
        assert_eq!(draft_to_options(&draft()).camos, CamoSelection::BaseOnly);
    }

    #[test]
    fn one_camo_ticked_bakes() {
        let mut d = draft();
        d.camos.insert(CamoSchemeId(3));
        assert_eq!(draft_to_options(&d).camos, CamoSelection::Baked(CamoSchemeId(3)));
    }

    #[test]
    fn several_camos_ticked_become_variants() {
        let mut d = draft();
        d.camos.insert(CamoSchemeId(3));
        d.camos.insert(CamoSchemeId(7));
        assert_eq!(draft_to_options(&d).camos, CamoSelection::Variants(vec![CamoSchemeId(3), CamoSchemeId(7)]));
    }

    #[test]
    fn armor_only_forces_textures_off_and_drops_the_camo_selection() {
        let mut d = draft();
        d.contents = ExportContents::Armor;
        d.camos.insert(CamoSchemeId(3));
        let options = draft_to_options(&d);
        assert!(!options.textures, "armor is untextured");
        assert_eq!(options.camos, CamoSelection::BaseOnly, "a camo cannot apply to armor");
    }

    #[test]
    fn the_texture_resolution_maps_onto_the_lod_cap() {
        assert_eq!(TextureResolution::Full.to_texture_lod(), TextureLod::Full);
        assert_eq!(TextureResolution::Px1024.to_texture_lod(), TextureLod::Capped(MaxEdge::new(1024).unwrap()));
    }

    #[test]
    fn the_default_camo_is_the_one_named_default() {
        let schemes = vec![scheme(0, "Ocean Soul", "PCEC001"), scheme(1, "Default", "default")];
        assert_eq!(default_camo(&schemes), Some(CamoSchemeId(1)));
    }

    #[test]
    fn a_translated_label_does_not_hide_the_default_scheme() {
        let schemes = vec![scheme(0, "Standard", "default")];
        assert_eq!(default_camo(&schemes), Some(CamoSchemeId(0)), "the raw name still names it");
    }

    #[test]
    fn a_ship_with_no_default_scheme_starts_with_nothing_ticked() {
        let schemes = vec![scheme(0, "Ocean Soul", "PCEC001")];
        assert_eq!(default_camo(&schemes), None);
    }

    /// Exercises the tree context menu's exact call shape (`seed_hull: None`)
    /// against real game data, standing in for the interactive check that a
    /// dialog with no loaded pane still reaches `Loaded`. Requires a game
    /// install; point `WOWS_DIR` at one to run it.
    #[test]
    #[ignore = "requires a game install"]
    fn open_with_no_seeded_hull_loads_real_metadata() {
        let game_dir = std::env::var_os("WOWS_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(r"E:\WoWs\World_of_Warships"));
        let assets = Arc::new(
            wowsunpack::export::ship::ShipAssets::from_game_dir(&game_dir).expect("load ship assets from game dir"),
        );

        use wowsunpack::game_params::types::GameParamProvider;
        let param = assets
            .metadata()
            .params()
            .iter()
            .find(|p| p.vehicle().and_then(|v| v.model_path()).map(|mp| mp.contains("WSD011_Smaland")).unwrap_or(false))
            .expect("Smaland present in this game data");
        let param_index = param.index().to_string();

        // The tree context menu path: no loaded pane, so no seed hull.
        let mut dialog =
            ExportDialog::open(param_index, "Smaland".to_string(), None, ExportDefaults::default(), assets);

        // spawn_load's own metadata load re-parses assets.bin more than once
        // (camo source, size model, hull upgrades); generous on a cold cache.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        while let ExportMetaState::Loading(inbox) = &mut dialog.meta {
            if let Some(result) = inbox.read_without_ctx().last() {
                dialog.meta = match result {
                    Ok(meta) => ExportMetaState::Loaded(meta),
                    Err(e) => ExportMetaState::Failed(e),
                };
                break;
            }
            assert!(std::time::Instant::now() < deadline, "export metadata load did not finish in time");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        match dialog.meta {
            ExportMetaState::Loaded(meta) => {
                assert!(meta.hull_lod_count >= 1);
                assert!(!meta.camo_schemes.is_empty(), "Smaland has camo schemes");
                // No translation catalog is loaded here, so display names come back
                // as raw ids ("camo_permanent_1") rather than "Default"; the
                // default-scheme match itself is covered by the unit tests above
                // against synthetic display names.
                eprintln!("default_camo resolved to: {:?}", default_camo(&meta.camo_schemes));
            }
            ExportMetaState::Failed(e) => panic!("metadata load failed: {e}"),
            ExportMetaState::Loading(_) => unreachable!("loop only exits once Loading is resolved"),
        }
    }
}
