//! The GLB export dialog: what goes into the model and what it will weigh.
//!
//! The ship is read twice off-thread: once without textures or armor for its
//! levels of detail and camo list, then for the texture ladders the size line
//! prices. A read reports only while it is still the one the dialog waits for.

use std::collections::BTreeSet;
use std::sync::Arc;

use nsh_core::prelude::JsObject;
use nsh_core::prelude::PropEnum;
use rust_i18n::t;
use wows_toolkit_viewmodel::armor::export::ExportDefaults;
use wows_toolkit_viewmodel::armor::export::ExportDraft;
use wows_toolkit_viewmodel::armor::export::TextureResolution;
use wows_toolkit_viewmodel::armor::export::default_camo;
use wows_toolkit_viewmodel::armor::export::draft_to_options;
use wows_toolkit_viewmodel::formatting::byte_size;
use wowsunpack::export::camo_textures::CamoSchemeId;
use wowsunpack::export::camo_textures::CamoSchemeInfo;
use wowsunpack::export::ship::ExportContents;
use wowsunpack::export::ship::ShipAssets;
use wowsunpack::export::ship::ShipExportOptions;
use wowsunpack::export::ship::ShipModelContext;
use wowsunpack::export::size_estimate::ExportSizeModel;
use wowsunpack::export::size_estimate::SchemeLadderStatus;

use super::state::Bundle;
use super::state::ShipSource;
use super::state::State;
use super::state::spawn_named;
use super::state::update;
use super::view::CamoGroupView;
use super::view::CamoView;
use super::view::ChoiceView;
use super::view::camo_groups;

/// What the model contains.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PropEnum)]
pub enum Contents {
    Mesh,
    Armor,
    #[default]
    MeshAndArmor,
}

impl From<Contents> for ExportContents {
    fn from(contents: Contents) -> Self {
        match contents {
            Contents::Mesh => Self::Mesh,
            Contents::Armor => Self::Armor,
            Contents::MeshAndArmor => Self::MeshAndArmor,
        }
    }
}

impl From<ExportContents> for Contents {
    fn from(contents: ExportContents) -> Self {
        match contents {
            ExportContents::Mesh => Self::Mesh,
            ExportContents::Armor => Self::Armor,
            ExportContents::MeshAndArmor => Self::MeshAndArmor,
        }
    }
}

/// The texture cap.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PropEnum)]
pub enum Resolution {
    #[default]
    Full,
    Px2048,
    Px1024,
    Px512,
}

impl From<Resolution> for TextureResolution {
    fn from(resolution: Resolution) -> Self {
        match resolution {
            Resolution::Full => Self::Full,
            Resolution::Px2048 => Self::Px2048,
            Resolution::Px1024 => Self::Px1024,
            Resolution::Px512 => Self::Px512,
        }
    }
}

impl From<TextureResolution> for Resolution {
    fn from(resolution: TextureResolution) -> Self {
        match resolution {
            TextureResolution::Full => Self::Full,
            TextureResolution::Px2048 => Self::Px2048,
            TextureResolution::Px1024 => Self::Px1024,
            TextureResolution::Px512 => Self::Px512,
        }
    }
}

/// What the size line says.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PropEnum)]
pub enum SizeKind {
    /// Still reading, or waiting on a camo's ladders.
    #[default]
    Waiting,
    /// This ship could not be priced at all.
    Silent,
    Known,
    /// Some ticked camos could not be priced; they can be asked for again.
    Incomplete,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct SizeView {
    pub kind: SizeKind,
    pub estimate: Option<String>,
    pub breakdown: Option<String>,
    pub incomplete: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct ExportCamosView {
    pub none_ticked: bool,
    /// `selected` is ticked.
    pub own: Vec<CamoView>,
    pub groups: Vec<CamoGroupView>,
}

#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct ExportDialogView {
    pub ship: String,
    pub contents: Contents,
    pub hulls: Vec<ChoiceView>,
    /// The ship is being read, and a hull switch would throw that read away.
    pub hulls_disabled: bool,
    pub lod_count: u32,
    pub lod: u32,
    /// The contents include the mesh, which is all resolution, camo and
    /// detail apply to.
    pub textured: bool,
    pub resolution: Resolution,
    pub camos: ExportCamosView,
    pub ready: bool,
    pub error: Option<String>,
    pub size: SizeView,
}

/// A written model.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct ExportResult {
    pub ship: String,
    pub path: String,
    pub bytes: Option<u64>,
    /// The size, formatted.
    pub size: Option<String>,
}

/// The ship as the dialog needs it.
struct ShipMeta {
    hull_lod_count: usize,
    camo_schemes: Vec<CamoSchemeInfo>,
    /// Kept so a camo ticked later is priced without reading the ship again.
    context: Arc<ShipModelContext>,
}

enum MetaState {
    Loading,
    Loaded(ShipMeta),
    Failed(String),
}

/// `Loaded(None)` is a ship that could not be priced.
enum SizeState {
    Loading,
    Loaded(Option<ExportSizeModel>),
}

pub(super) struct ExportDialog {
    source: ShipSource,
    hull_upgrades: Vec<(String, String)>,
    meta: MetaState,
    size: SizeState,
    /// Camo ladders being read, so none is asked for twice.
    pricing: BTreeSet<CamoSchemeId>,
    generation: u64,
    draft: ExportDraft,
    /// What the size line says, settled after every change.
    size_view: SizeView,
}

impl ExportDialog {
    pub fn view(&self) -> ExportDialogView {
        let (lod_count, schemes, error) = match &self.meta {
            MetaState::Loaded(meta) => (meta.hull_lod_count, meta.camo_schemes.as_slice(), None),
            MetaState::Loading => (0, [].as_slice(), None),
            MetaState::Failed(why) => {
                (0, [].as_slice(), Some(t!("ui.armor.load_failed", error = why.clone()).into_owned()))
            }
        };
        let (own, groups) = camo_groups(schemes, |id| self.draft.camos.contains(&id));
        let hulls = self
            .hull_upgrades
            .iter()
            .enumerate()
            .map(|(index, (key, label))| ChoiceView {
                key: key.clone(),
                label: label.clone(),
                // No hull chosen is the first, which is what the exporter takes.
                selected: self.draft.hull.as_ref().map_or(index == 0, |chosen| chosen == key),
            })
            .collect();

        ExportDialogView {
            ship: self.source.display_name.clone(),
            contents: self.draft.contents.into(),
            hulls,
            hulls_disabled: matches!(self.meta, MetaState::Loading),
            lod_count: lod_count as u32,
            lod: self.draft.lod as u32,
            textured: self.draft.contents.includes_mesh(),
            resolution: self.draft.texture_res.into(),
            camos: ExportCamosView { none_ticked: self.draft.camos.is_empty(), own, groups },
            ready: self.is_ready(),
            error,
            size: self.size_view.clone(),
        }
    }

    fn is_ready(&self) -> bool {
        matches!(self.meta, MetaState::Loaded(_))
    }

    /// Holds the level of detail to what this hull has and ticks the scheme
    /// named "default", as the egui dialog does.
    fn adopt_meta(&mut self, meta: ShipMeta) {
        self.draft.lod = self.draft.lod.min(meta.hull_lod_count.saturating_sub(1));
        self.draft.camos.clear();
        if let Some(scheme) = default_camo(&meta.camo_schemes) {
            self.draft.camos.insert(scheme);
        }
        self.meta = MetaState::Loaded(meta);
    }

    /// The options the draft describes and what to remember from it.
    fn confirmed(&self) -> (ShipExportOptions, ExportDefaults) {
        (draft_to_options(&self.draft), ExportDefaults::from_draft(&self.draft))
    }
}

/// What the dialog confirmed: where it came from and what to write.
pub(super) struct ConfirmedExport {
    pub bundle: Arc<Bundle>,
    pub source: ShipSource,
    pub options: ShipExportOptions,
    pub defaults: ExportDefaults,
}

impl State {
    /// Opens the dialog on `source`, seeded from the active pane's hull and
    /// the remembered choices.
    pub fn open_export_dialog(
        &mut self,
        source: ShipSource,
        defaults: ExportDefaults,
    ) -> Result<(), super::error::ArmorError> {
        let bundle = self.bundle().cloned().ok_or(super::error::ArmorError::CatalogNotReady)?;
        let seed_hull = self.active_pane().scene.hull_selection().hull;
        let hull_upgrades = hull_upgrades_for(&bundle.assets, &source.param_index);
        self.export = Some(ExportDialog {
            source,
            hull_upgrades,
            meta: MetaState::Loading,
            size: SizeState::Loading,
            pricing: BTreeSet::new(),
            generation: 0,
            draft: ExportDraft {
                contents: defaults.contents,
                hull: seed_hull,
                lod: defaults.lod,
                texture_res: defaults.texture_res,
                camos: BTreeSet::new(),
            },
            size_view: SizeView::default(),
        });
        self.read_export_ship(bundle);

        Ok(())
    }

    /// Reads the ship for the dialog, then prices it.
    fn read_export_ship(&mut self, bundle: Arc<Bundle>) {
        let Some(dialog) = self.export.as_mut() else { return };
        dialog.meta = MetaState::Loading;
        dialog.size = SizeState::Loading;
        dialog.pricing.clear();
        dialog.generation += 1;
        dialog.size_view = SizeView::default();
        let reading = dialog.generation;
        let param_index = dialog.source.param_index.clone();
        let hull = dialog.draft.hull.clone();

        spawn_named("armor-export-read", move || {
            let read = read_ship_meta(&bundle.assets, &param_index, hull);
            let context = match read {
                Ok(meta) => {
                    let context = Arc::clone(&meta.context);
                    let landed = update(|state| {
                        let Some(dialog) = state.current_export(reading) else { return false };
                        dialog.adopt_meta(meta);
                        state.settle_export_size();
                        true
                    });
                    if !landed {
                        return;
                    }
                    context
                }
                Err(why) => {
                    update(|state| {
                        let Some(dialog) = state.current_export(reading) else { return };
                        dialog.meta = MetaState::Failed(why);
                        dialog.size = SizeState::Loaded(None);
                        state.settle_export_size();
                    });
                    return;
                }
            };

            let priced = context.size_model().inspect_err(|report| {
                tracing::warn!("armor viewer: the export size could not be modelled: {report:?}");
            });
            update(|state| {
                let Some(dialog) = state.current_export(reading) else { return };
                dialog.size = SizeState::Loaded(priced.ok());
                state.settle_export_size();
            });
        });
    }

    /// The dialog, while it is still the read `generation` belongs to.
    fn current_export(&mut self, generation: u64) -> Option<&mut ExportDialog> {
        self.export.as_mut().filter(|dialog| dialog.generation == generation)
    }

    /// Changes the draft, reading the ship again when the hull changed.
    pub fn edit_export(&mut self, change: impl FnOnce(&mut ExportDraft)) {
        let Some(dialog) = self.export.as_mut() else { return };
        let hull_before = dialog.draft.hull.clone();
        change(&mut dialog.draft);
        if dialog.draft.hull != hull_before
            && let Some(bundle) = self.bundle().cloned()
        {
            self.read_export_ship(bundle);
            return;
        }
        self.settle_export_size();
    }

    /// Puts the camo selection back to what the ship opens with.
    pub fn reset_export_camos(&mut self) {
        self.edit_export(|_| {});
        let Some(dialog) = self.export.as_mut() else { return };
        dialog.draft.camos.clear();
        if let MetaState::Loaded(meta) = &dialog.meta
            && let Some(scheme) = default_camo(&meta.camo_schemes)
        {
            dialog.draft.camos.insert(scheme);
        }
        self.settle_export_size();
    }

    /// Asks again for the camo ladders whose read failed.
    pub fn retry_export_pricing(&mut self) {
        let Some(dialog) = self.export.as_mut() else { return };
        let SizeState::Loaded(Some(model)) = &mut dialog.size else { return };
        let failed = failed_schemes(model, &draft_to_options(&dialog.draft));
        for id in &failed {
            model.clear_failed_scheme_ladders(*id);
        }
        self.settle_export_size();
    }

    /// Works out the size line, asking for any camo ladders it is missing.
    fn settle_export_size(&mut self) {
        let Some(dialog) = self.export.as_mut() else { return };
        let SizeState::Loaded(model) = &dialog.size else {
            dialog.size_view = SizeView::default();
            return;
        };
        let Some(model) = model else {
            dialog.size_view = SizeView { kind: SizeKind::Silent, ..SizeView::default() };
            return;
        };

        let options = draft_to_options(&dialog.draft);
        let unpriced = match model.priced_estimate(&options) {
            Ok(estimate) => {
                let total = byte_size(estimate.total());
                let geometry = byte_size(estimate.geometry_bytes);
                let textures = byte_size(estimate.texture_bytes);
                dialog.size_view = SizeView {
                    kind: SizeKind::Known,
                    estimate: Some(t!("ui.armor.export.size_estimate", size = total).into_owned()),
                    breakdown: Some(
                        t!("ui.armor.export.size_breakdown", geometry = geometry, textures = textures).into_owned(),
                    ),
                    incomplete: None,
                };
                return;
            }
            Err(unpriced) => unpriced,
        };

        let pending: Vec<CamoSchemeId> = unpriced
            .iter()
            .filter(|(id, status)| *status == SchemeLadderStatus::Pending && !dialog.pricing.contains(id))
            .map(|(id, _)| *id)
            .collect();
        let failed = unpriced.iter().filter(|(_, status)| *status == SchemeLadderStatus::Failed).count();
        // A read that failed never asks again on its own; without the retry
        // the size line would simply disappear.
        dialog.size_view = if failed == 0 {
            SizeView::default()
        } else {
            SizeView {
                kind: SizeKind::Incomplete,
                incomplete: Some(t!("ui.armor.export.size_incomplete", count = failed).into_owned()),
                ..SizeView::default()
            }
        };
        if pending.is_empty() {
            return;
        }
        let MetaState::Loaded(meta) = &dialog.meta else { return };
        let context = Arc::clone(&meta.context);
        dialog.pricing.extend(pending.iter().copied());
        let generation = dialog.generation;

        // One batch: the assets.bin parse behind it is shared across schemes.
        spawn_named("armor-export-price", move || {
            let read: Vec<_> = match context.scheme_ladders_batch(&pending) {
                Ok(batch) => batch.into_iter().map(|(id, ladders)| (id, Some(ladders))).collect(),
                Err(why) => {
                    tracing::warn!("armor viewer: {} camo scheme(s) could not be priced: {why:?}", pending.len());
                    pending.iter().map(|id| (*id, None)).collect()
                }
            };
            update(|state| {
                let Some(dialog) = state.current_export(generation) else { return };
                if let SizeState::Loaded(Some(model)) = &mut dialog.size {
                    for (id, ladders) in read {
                        match ladders {
                            Some(ladders) => model.insert_scheme_ladders(id, ladders),
                            None => model.mark_scheme_ladders_failed(id),
                        }
                    }
                }
                for id in &pending {
                    dialog.pricing.remove(id);
                }
                state.settle_export_size();
            });
        });
    }

    /// Closes the dialog with what it describes, once the ship has been read.
    pub fn confirm_export_dialog(&mut self) -> Result<ConfirmedExport, super::error::ArmorError> {
        let bundle = self.bundle().cloned().ok_or(super::error::ArmorError::CatalogNotReady)?;
        let dialog = self.export.as_ref().ok_or(super::error::ArmorError::NoExportDialog)?;
        if !dialog.is_ready() {
            return Err(super::error::ArmorError::ExportNotReady);
        }
        let (options, defaults) = dialog.confirmed();
        let source = dialog.source.clone();
        self.export = None;

        Ok(ConfirmedExport { bundle, source, options, defaults })
    }
}

fn failed_schemes(model: &ExportSizeModel, options: &ShipExportOptions) -> Vec<CamoSchemeId> {
    match model.priced_estimate(options) {
        Ok(_) => Vec::new(),
        Err(unpriced) => {
            unpriced.into_iter().filter(|(_, status)| *status == SchemeLadderStatus::Failed).map(|(id, _)| id).collect()
        }
    }
}

/// The ship read without textures or armor: those cost far more than the
/// levels of detail and the camo list this is for.
fn read_ship_meta(assets: &ShipAssets, param_index: &str, hull: Option<String>) -> Result<ShipMeta, String> {
    use wowsunpack::game_params::types::GameParamProvider;

    let param = assets.metadata().game_param_by_index(param_index);
    let vehicle = param
        .as_ref()
        .and_then(|param| param.vehicle().cloned())
        .ok_or_else(|| t!("ui.armor.export.vehicle_not_found").into_owned())?;
    let probe =
        ShipExportOptions { hull, textures: false, contents: ExportContents::Mesh, ..ShipExportOptions::default() };
    let context = assets.load_ship_from_vehicle(&vehicle, &probe).map_err(|report| format!("{report:?}"))?;
    let source = context.camo_texture_source().map_err(|report| format!("{report:?}"))?;

    Ok(ShipMeta {
        hull_lod_count: context.hull_lod_count(),
        camo_schemes: source.scheme_infos(),
        context: Arc::new(context),
    })
}

/// The hull upgrades the ship offers, in the pane's own vocabulary.
fn hull_upgrades_for(assets: &ShipAssets, param_index: &str) -> Vec<(String, String)> {
    use wowsunpack::game_params::types::GameParamProvider;

    let Some(param) = assets.metadata().game_param_by_index(param_index) else { return Vec::new() };
    let Some(vehicle) = param.vehicle() else { return Vec::new() };
    wows_toolkit_armor::load_ship::build_hull_upgrade_names(vehicle)
}
