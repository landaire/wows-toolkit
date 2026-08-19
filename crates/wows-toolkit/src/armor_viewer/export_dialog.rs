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
use wowsunpack::export::ship::ShipModelContext;
use wowsunpack::export::size_estimate::ExportSizeModel;
use wowsunpack::export::size_estimate::SizeEstimate;
use wowsunpack::export::size_estimate::TextureLadder;
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

/// The subset of `ExportDraft` that changes `SizeEstimate`, plus the
/// `ExportDialog::load_generation` a `size_model` belongs to (a fresh load
/// invalidates any estimate computed against the previous one, even if every
/// draft field happens to still match). `estimate()` measures in the low
/// microseconds (~5us on Smaland, measured against real game data), but
/// recomputing it every frame regardless of whether anything changed is still
/// wasted UI-thread work for no visible benefit.
#[derive(Clone, PartialEq, Eq)]
struct EstimateKey {
    load_generation: u64,
    contents: ExportContents,
    lod: usize,
    texture_res: TextureResolution,
    camos: BTreeSet<CamoSchemeId>,
}

impl EstimateKey {
    fn from_draft(load_generation: u64, draft: &ExportDraft) -> Self {
        Self {
            load_generation,
            contents: draft.contents,
            lod: draft.lod,
            texture_res: draft.texture_res,
            camos: draft.camos.clone(),
        }
    }
}

/// Stage 1 of a background load: the ship itself and its camo scheme list.
/// Fast -- no geometry counting, no texture ladder reads -- so the LoD slider
/// and camo list only wait on this, not on stage 2. `ctx` is kept alive so a
/// camo the user selects afterward can be priced with `ShipModelContext::
/// scheme_ladders` without reloading the ship.
pub struct ExportMeta {
    pub hull_lod_count: usize,
    pub camo_schemes: Vec<CamoSchemeInfo>,
    ctx: Arc<ShipModelContext>,
}

pub enum ExportMetaState {
    Loading(egui_inbox::UiInbox<Result<ExportMeta, String>>),
    Loaded(ExportMeta),
    Failed(String),
}

/// Stage 2 of a background load: the size model. Slower than stage 1 (it
/// still reads the base albedo ladders), so it is tracked independently and
/// must never gate the hull/contents/resolution/LoD/camo controls -- only the
/// size line itself waits on it.
enum SizeModelState {
    Loading(egui_inbox::UiInbox<Option<ExportSizeModel>>),
    /// `None` means the model could not be built: the dialog shows no size
    /// line rather than a fabricated number, and the export still works.
    Loaded(Option<ExportSizeModel>),
}

/// One camo scheme's resolved `(path, ladder)` pairs, as
/// `ExportSizeModel::insert_scheme_ladders` takes them.
type SchemeLadders = Vec<(String, TextureLadder)>;

/// A background scheme-ladder fetch's result: one entry per requested id.
type SchemeLadderBatch = Vec<(CamoSchemeId, SchemeLadders)>;

/// A background request for one or more camo schemes' texture ladders,
/// spawned when the draft selects a scheme `size_model` was not built with
/// (it only prices the schemes selected at load time, typically none).
struct PendingSchemeFetch {
    ids: BTreeSet<CamoSchemeId>,
    inbox: egui_inbox::UiInbox<SchemeLadderBatch>,
}

/// Whether the dialog is being edited or is waiting on a background export.
/// Exporting keeps the dialog open (with a spinner and disabled controls)
/// rather than closing it and leaving the app looking idle for however long
/// the write takes -- minutes, on a full-resolution ship.
enum ExportPhase {
    Editing,
    Exporting(egui_inbox::UiInbox<()>),
}

/// A pending ship model export.
pub struct ExportDialog {
    pub param_index: String,
    pub display_name: String,
    /// Resolved synchronously in `open`/`start_load`: it comes from
    /// `vehicle.hull_upgrades()`, already in hand from the metadata provider
    /// before any background work starts, so the hull combo needs no spinner.
    pub hull_upgrades: Vec<String>,
    pub meta: ExportMetaState,
    size_model: SizeModelState,
    pending_scheme_fetch: Option<PendingSchemeFetch>,
    pub draft: ExportDraft,
    phase: ExportPhase,
    /// Bumped on every `spawn_load`, so a `SizeEstimate` cached against the
    /// model one load produced is never mistaken for one that matches the next.
    load_generation: u64,
    estimate_cache: Option<(EstimateKey, SizeEstimate)>,
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
        // Already resolved in `assets`'s metadata provider (no assets.bin scan,
        // no VFS read), so this needs no background thread: the hull combo can
        // render on the very first frame.
        let hull_upgrades = hull_upgrades_for(&assets, &param_index);
        // `start_load` sets `meta`/`size_model`, so they are built here rather
        // than seeded with a placeholder state that would never be observed.
        let (stage1_sender, stage1_inbox) = egui_inbox::UiInbox::channel();
        let (stage2_sender, stage2_inbox) = egui_inbox::UiInbox::channel();
        let dialog = Self {
            param_index,
            display_name,
            hull_upgrades,
            meta: ExportMetaState::Loading(stage1_inbox),
            size_model: SizeModelState::Loading(stage2_inbox),
            pending_scheme_fetch: None,
            draft,
            phase: ExportPhase::Editing,
            load_generation: 0,
            estimate_cache: None,
        };
        dialog.spawn_load(assets, stage1_sender, stage2_sender);
        dialog
    }

    /// Reload after a hull change: the hull decides which parts exist, and so the
    /// camo list, the LoD depth and the size model. `hull_upgrades` itself does
    /// not depend on the hull selection, so it is left as-is.
    pub fn start_load(&mut self, assets: Arc<wowsunpack::export::ship::ShipAssets>) {
        let (stage1_sender, stage1_inbox) = egui_inbox::UiInbox::channel();
        let (stage2_sender, stage2_inbox) = egui_inbox::UiInbox::channel();
        self.meta = ExportMetaState::Loading(stage1_inbox);
        self.size_model = SizeModelState::Loading(stage2_inbox);
        // Tied to the model this generation is about to replace; any fetch
        // still in flight against the old one would merge into a size model
        // that no longer describes the current hull.
        self.pending_scheme_fetch = None;
        self.estimate_cache = None;
        self.load_generation += 1;
        self.spawn_load(assets, stage1_sender, stage2_sender);
    }

    /// Loads the ship in two stages on one background thread. Stage 1 (the ship
    /// load and its camo scheme list) unblocks the LoD slider and camo list;
    /// stage 2 (the size model) is sent afterward, on the same thread, so it can
    /// never land before stage 1 does. Neither stage embeds textures.
    fn spawn_load(
        &self,
        assets: Arc<wowsunpack::export::ship::ShipAssets>,
        stage1_sender: egui_inbox::UiInboxSender<Result<ExportMeta, String>>,
        stage2_sender: egui_inbox::UiInboxSender<Option<ExportSizeModel>>,
    ) {
        let param_index = self.param_index.clone();
        let hull = self.draft.hull.clone();
        crate::util::thread::spawn_logged("export-dialog-load", move || {
            let stage1 = (|| -> Result<(ExportMeta, Arc<ShipModelContext>), String> {
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
                let ctx = Arc::new(ctx);
                let source = ctx.camo_texture_source().map_err(|e| format!("{e:?}"))?;
                let meta = ExportMeta {
                    hull_lod_count: ctx.hull_lod_count(),
                    camo_schemes: source.scheme_infos(),
                    ctx: ctx.clone(),
                };
                Ok((meta, ctx))
            })();

            let ctx = match stage1 {
                Ok((meta, ctx)) => {
                    let _ = stage1_sender.send(Ok(meta));
                    ctx
                }
                Err(e) => {
                    let _ = stage1_sender.send(Err(e));
                    return;
                }
            };

            // A ship that cannot be priced can still be exported, so a failed
            // model costs the estimate line and nothing else.
            let size_model = match ctx.size_model() {
                Ok(m) => Some(m),
                Err(e) => {
                    tracing::warn!("could not build the export size model for {param_index}: {e:?}");
                    None
                }
            };
            let _ = stage2_sender.send(size_model);
        });
    }

    /// Whether a background export is in flight for this dialog. The entry
    /// points that would replace this dialog with a fresh one for a
    /// different ship check this first, so starting a second export from a
    /// stray click can never happen while this one is still running.
    pub fn is_exporting(&self) -> bool {
        matches!(self.phase, ExportPhase::Exporting(_))
    }
}

/// `vehicle.hull_upgrades()` is already resolved (no assets.bin scan, unlike
/// `ShipAssets::list_hull_upgrades`, whose model-name-based lookup strategy
/// never matches a param index and always runs to exhaustion). `None` from
/// `hull_upgrades()` genuinely means this ship has no listed upgrades (older
/// game versions offer no hull choice); a missing param or vehicle is treated
/// the same way here since `spawn_load` reports that failure properly.
fn hull_upgrades_for(assets: &wowsunpack::export::ship::ShipAssets, param_index: &str) -> Vec<String> {
    use wowsunpack::game_params::types::GameParamProvider;
    let Some(param) = assets.metadata().game_param_by_index(param_index) else {
        return Vec::new();
    };
    let Some(vehicle) = param.vehicle() else {
        return Vec::new();
    };
    match vehicle.hull_upgrades() {
        Some(upgrades) => {
            let mut names: Vec<String> = upgrades.keys().cloned().collect();
            names.sort();
            names
        }
        None => Vec::new(),
    }
}

pub enum DialogOutcome {
    /// Still open, nothing to do.
    Idle,
    /// The hull changed; reload the metadata.
    ReloadRequested,
    /// The user confirmed. The dialog has already moved itself into
    /// `ExportPhase::Exporting`; the caller spawns the worker with `sender`.
    /// The worker owns reporting the result (toast, from any thread) --
    /// `sender` only tells this dialog, if it is still open to hear it, to
    /// leave the exporting phase.
    Export { options: Box<ShipExportOptions>, sender: egui_inbox::UiInboxSender<()> },
    /// The user dismissed the dialog. A worker started by `Export` keeps
    /// running and reports its own result independently of this dialog's
    /// lifetime.
    Cancelled,
}

pub fn show(dialog: &mut ExportDialog, ctx: &egui::Context) -> DialogOutcome {
    let mut outcome = DialogOutcome::Idle;

    // Polled here, ahead of the window body, since it decides this frame's
    // enabled/disabled state for everything the window renders below. The
    // signal carries no payload: the worker reports its own result (toast)
    // independently, from any thread, regardless of whether this dialog is
    // even still open to see it. This is purely "stop showing the spinner."
    if let ExportPhase::Exporting(inbox) = &mut dialog.phase
        && inbox.read(ctx).last().is_some()
    {
        dialog.phase = ExportPhase::Editing;
    }

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

            if let SizeModelState::Loading(inbox) = &mut dialog.size_model
                && let Some(result) = inbox.read(ui).last()
            {
                dialog.size_model = SizeModelState::Loaded(result);
                // The number, if any, cached against the loading model would
                // have been priced with zero schemes -- stale the moment a
                // real model (or its absence) lands.
                dialog.estimate_cache = None;
            }

            let exporting = matches!(dialog.phase, ExportPhase::Exporting(_));

            // The hull combo, contents radios and texture-resolution combo
            // need nothing loaded, so `controls` renders on every frame this
            // dialog is open -- including while stage 1 is still loading, in
            // which case it shows its own small spinners for the LoD slider
            // and camo list instead of gating the whole panel on them.
            if let ExportMetaState::Failed(e) = &dialog.meta {
                ui.colored_label(ui.visuals().error_fg_color, t!("ui.armor.load_failed", error = e).as_ref());
            } else {
                let meta = match &dialog.meta {
                    ExportMetaState::Loaded(meta) => Some(meta),
                    _ => None,
                };
                ui.add_enabled_ui(!exporting, |ui| {
                    outcome = controls(
                        ui,
                        &mut dialog.draft,
                        &dialog.hull_upgrades,
                        meta,
                        &mut dialog.size_model,
                        &mut dialog.pending_scheme_fetch,
                        dialog.load_generation,
                        &mut dialog.estimate_cache,
                    );
                });
            }

            if exporting {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(14.0));
                    ui.label(t!("ui.armor.export.exporting", ship = dialog.display_name.as_str()).as_ref());
                });
            }

            ui.add_space(8.0);
            ui.label(t!("ui.armor.export_disclaimer").as_ref());
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let ready = matches!(dialog.meta, ExportMetaState::Loaded(_)) && !exporting;
                if ui.add_enabled(ready, egui::Button::new(t!("ui.armor.export_button").as_ref())).clicked() {
                    let (sender, inbox) = egui_inbox::UiInbox::channel();
                    dialog.phase = ExportPhase::Exporting(inbox);
                    outcome = DialogOutcome::Export { options: Box::new(draft_to_options(&dialog.draft)), sender };
                }
                // Always enabled, even mid-export: the worker is detached and
                // reports its own result, so dismissing the dialog here costs
                // nothing and is the only way to get rid of one whose worker
                // panicked (which sends nothing back, ever).
                let close_label = if exporting { t!("ui.armor.export.close_button") } else { t!("ui.buttons.cancel") };
                if ui.button(close_label.as_ref()).clicked() {
                    outcome = DialogOutcome::Cancelled;
                }
            });
        });
    outcome
}

/// Renders hull, contents, LoD, texture resolution and camo controls, plus the
/// size line. Returns `ReloadRequested` when the hull selection changed, since
/// the hull decides which parts, camos and LoDs even exist.
///
/// `meta` is `None` while stage 1 is still loading: the hull combo, contents
/// radios and texture-resolution combo render regardless (they need nothing
/// from it), while the LoD slider and camo list each show a small spinner in
/// its place. The size line is driven by `size_model` independently, and
/// never waits on `meta` either.
#[allow(clippy::too_many_arguments)]
fn controls(
    ui: &mut egui::Ui,
    draft: &mut ExportDraft,
    hull_upgrades: &[String],
    meta: Option<&ExportMeta>,
    size_model: &mut SizeModelState,
    pending_scheme_fetch: &mut Option<PendingSchemeFetch>,
    load_generation: u64,
    estimate_cache: &mut Option<(EstimateKey, SizeEstimate)>,
) -> DialogOutcome {
    let mut outcome = DialogOutcome::Idle;

    if !hull_upgrades.is_empty() {
        let selected_label = draft
            .hull
            .as_ref()
            .and_then(|h| hull_upgrades.iter().find(|u| *u == h))
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
                for name in hull_upgrades {
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

    match meta {
        Some(meta) if meta.hull_lod_count > 1 => {
            ui.add_enabled(
                draft.contents.includes_mesh(),
                egui::Slider::new(&mut draft.lod, 0..=meta.hull_lod_count.saturating_sub(1))
                    .text(t!("ui.armor.export.lod")),
            )
            .on_hover_text(t!("ui.armor.export.lod_tooltip").as_ref());
        }
        Some(_) => {}
        None => loading_placeholder(ui),
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

    match meta {
        Some(meta) if !meta.camo_schemes.is_empty() => {
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
                    let mut group: Vec<&CamoSchemeInfo> =
                        meta.camo_schemes.iter().filter(|i| i.origin == origin).collect();
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
        Some(_) => {}
        None => loading_placeholder(ui),
    }

    size_line(ui, draft, meta, size_model, pending_scheme_fetch, load_generation, estimate_cache);

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

/// A small inline spinner for a control section still waiting on a
/// background stage. Never blocks the rest of the dialog.
fn loading_placeholder(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.add(egui::Spinner::new().size(12.0));
        ui.label(t!("ui.armor.export.loading_details").as_ref());
    });
}

/// Renders the size estimate, or a spinner while it is unavailable. The size
/// model prices only the schemes it was built with (typically none); when the
/// draft selects one it lacks, this requests that scheme's ladders in the
/// background (never on this thread -- a ladder read is VFS IO) and shows a
/// spinner instead of a number computed as if that scheme cost nothing.
#[allow(clippy::too_many_arguments)]
fn size_line(
    ui: &mut egui::Ui,
    draft: &ExportDraft,
    meta: Option<&ExportMeta>,
    size_model: &mut SizeModelState,
    pending_scheme_fetch: &mut Option<PendingSchemeFetch>,
    load_generation: u64,
    estimate_cache: &mut Option<(EstimateKey, SizeEstimate)>,
) {
    let SizeModelState::Loaded(model) = size_model else {
        loading_placeholder(ui);
        return;
    };
    let Some(model) = model else {
        // The ship could not be priced; the estimate line is the only thing
        // this costs, and the export itself still works.
        return;
    };

    if let Some(fetch) = pending_scheme_fetch
        && let Some(results) = fetch.inbox.read(ui).last()
    {
        for (id, ladders) in results {
            model.insert_scheme_ladders(id, ladders);
        }
        *estimate_cache = None;
        *pending_scheme_fetch = None;
    }

    let options = draft_to_options(draft);
    let missing = model.missing_scheme_ladders(&options);
    if !missing.is_empty() {
        if let Some(meta) = meta {
            ensure_scheme_fetch(pending_scheme_fetch, &meta.ctx, &missing);
        }
        loading_placeholder(ui);
        return;
    }

    let key = EstimateKey::from_draft(load_generation, draft);
    let estimate = match estimate_cache {
        Some((cached_key, cached)) if *cached_key == key => *cached,
        _ => {
            let e = model.estimate(&options);
            *estimate_cache = Some((key, e));
            e
        }
    };
    let size = humansize::format_size(estimate.total(), humansize::BINARY);
    ui.label(t!("ui.armor.export.size_estimate", size = size).as_ref());
}

/// Spawns a background fetch for `missing`'s ladders, unless one already in
/// flight covers exactly this set. A different set (the user changed the
/// selection again before the first fetch landed) replaces it; the replaced
/// fetch's result, if it still lands, merges harmlessly (`insert_scheme_ladders`
/// is idempotent per id) and is simply ignored by `size_line` on arrival since
/// `pending_scheme_fetch` no longer points at its inbox.
fn ensure_scheme_fetch(
    pending: &mut Option<PendingSchemeFetch>,
    ctx: &Arc<ShipModelContext>,
    missing: &[CamoSchemeId],
) {
    let missing_ids: BTreeSet<CamoSchemeId> = missing.iter().copied().collect();
    if pending.as_ref().is_some_and(|p| p.ids == missing_ids) {
        return;
    }

    let (sender, inbox) = egui_inbox::UiInbox::channel();
    let ctx = ctx.clone();
    let ids: Vec<CamoSchemeId> = missing_ids.iter().copied().collect();
    crate::util::thread::spawn_logged("export-dialog-scheme-ladders", move || {
        let mut results = Vec::with_capacity(ids.len());
        for id in ids {
            let ladders = match ctx.scheme_ladders(id) {
                Ok(l) => l,
                // Best-effort, like the rest of the size model: one scheme that
                // cannot be priced must not deny the user a number for the
                // others, so it prices as zero extra bytes rather than staying
                // pending forever.
                Err(e) => {
                    tracing::warn!("could not price camo scheme {id:?}: {e:?}");
                    Vec::new()
                }
            };
            results.push((id, ladders));
        }
        let _ = sender.send(results);
    });
    *pending = Some(PendingSchemeFetch { ids: missing_ids, inbox });
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
    fn a_scheme_named_default_only_in_its_display_name_still_matches() {
        // raw_name is deliberately not "default" here, so this guards the
        // display_name arm on its own: deleting that arm would fail this test
        // even though it would not fail `the_default_camo_is_the_one_named_default`.
        let schemes = vec![scheme(0, "Default", "PCEC001")];
        assert_eq!(default_camo(&schemes), Some(CamoSchemeId(0)));
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
    /// against real game data, and measures the three numbers Task 11 asks
    /// for: `open`'s own wall-clock (the controls are interactive the instant
    /// it returns, since it does only synchronous, already-resolved metadata
    /// lookups), stage 1's wall-clock (LoD slider + camo list), and stage 2's
    /// wall-clock (the size line). Requires a game install; point `WOWS_DIR`
    /// at one to run it.
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
        let open_started = std::time::Instant::now();
        let mut dialog =
            ExportDialog::open(param_index, "Smaland".to_string(), None, ExportDefaults::default(), assets);
        let open_elapsed = open_started.elapsed();
        eprintln!("[measured] ExportDialog::open wall-clock (controls interactive): {open_elapsed:?}");
        // `open` resolves this synchronously; it is the proof the hull combo
        // needed no background work at all.
        assert!(!dialog.hull_upgrades.is_empty(), "Smaland has hull upgrades, resolved synchronously by open()");

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);

        while let ExportMetaState::Loading(inbox) = &mut dialog.meta {
            if let Some(result) = inbox.read_without_ctx().last() {
                dialog.meta = match result {
                    Ok(meta) => ExportMetaState::Loaded(meta),
                    Err(e) => ExportMetaState::Failed(e),
                };
                break;
            }
            assert!(std::time::Instant::now() < deadline, "stage 1 did not finish in time");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        eprintln!(
            "[measured] Smaland stage 1 (ship load + camo list) wall-clock since open: {:.3}s",
            open_started.elapsed().as_secs_f64()
        );

        while let SizeModelState::Loading(inbox) = &mut dialog.size_model {
            if let Some(result) = inbox.read_without_ctx().last() {
                dialog.size_model = SizeModelState::Loaded(result);
                break;
            }
            assert!(std::time::Instant::now() < deadline, "stage 2 did not finish in time");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        eprintln!(
            "[measured] Smaland stage 2 (size model / size line) wall-clock since open: {:.3}s",
            open_started.elapsed().as_secs_f64()
        );

        match &dialog.meta {
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

        match &dialog.size_model {
            SizeModelState::Loaded(Some(model)) => {
                let draft = ExportDraft {
                    contents: ExportContents::MeshAndArmor,
                    hull: None,
                    lod: 0,
                    texture_res: TextureResolution::Full,
                    camos: BTreeSet::new(),
                };
                let options = draft_to_options(&draft);
                assert!(model.missing_scheme_ladders(&options).is_empty(), "no camo selected needs no scheme ladders");
                // Warm up (page faults, branch predictor) before timing.
                for _ in 0..10 {
                    std::hint::black_box(model.estimate(&options));
                }
                let iters = 10_000;
                let started = std::time::Instant::now();
                for _ in 0..iters {
                    std::hint::black_box(model.estimate(&options));
                }
                let per_call = started.elapsed() / iters;
                eprintln!("[measured] SizeEstimate::estimate() cost: {per_call:?} per call ({iters} iterations)");
            }
            SizeModelState::Loaded(None) => eprintln!("size model could not be built for Smaland"),
            SizeModelState::Loading(_) => unreachable!("loop only exits once Loading is resolved"),
        }
    }

    /// The dialog's headline slow case: a ship with roughly 100 camo schemes
    /// (Yamato). `size_model` now only prices the schemes selected at load
    /// time (typically none), not every scheme the ship offers, so this
    /// stage should land in a small fraction of the ~14s the eager path
    /// cost. Requires a game install; point `WOWS_DIR` at one to run it.
    #[test]
    #[ignore = "requires a game install"]
    fn stage_two_lands_quickly_on_a_ship_with_many_schemes() {
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
            .find(|p| {
                p.vehicle().and_then(|v| v.model_path()).map(|mp| mp.contains("JSB039_Yamato_1945")).unwrap_or(false)
            })
            .expect("Yamato present in this game data");
        let param_index = param.index().to_string();

        let open_started = std::time::Instant::now();
        let mut dialog = ExportDialog::open(param_index, "Yamato".to_string(), None, ExportDefaults::default(), assets);
        eprintln!("[measured] ExportDialog::open wall-clock (controls interactive): {:?}", open_started.elapsed());

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(180);

        while let ExportMetaState::Loading(inbox) = &mut dialog.meta {
            if let Some(result) = inbox.read_without_ctx().last() {
                dialog.meta = match result {
                    Ok(meta) => ExportMetaState::Loaded(meta),
                    Err(e) => ExportMetaState::Failed(e),
                };
                break;
            }
            assert!(std::time::Instant::now() < deadline, "stage 1 did not finish in time");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let scheme_count = match &dialog.meta {
            ExportMetaState::Loaded(meta) => meta.camo_schemes.len(),
            ExportMetaState::Failed(e) => panic!("stage 1 failed for Yamato: {e}"),
            ExportMetaState::Loading(_) => unreachable!("loop only exits once Loading is resolved"),
        };
        eprintln!(
            "[measured] Yamato stage 1 wall-clock since open: {:.3}s ({scheme_count} camo schemes)",
            open_started.elapsed().as_secs_f64()
        );
        assert!(scheme_count > 50, "Yamato should offer roughly 100 schemes, got {scheme_count}");

        while let SizeModelState::Loading(inbox) = &mut dialog.size_model {
            if let Some(result) = inbox.read_without_ctx().last() {
                dialog.size_model = SizeModelState::Loaded(result);
                break;
            }
            assert!(std::time::Instant::now() < deadline, "stage 2 did not finish in time");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        eprintln!(
            "[measured] Yamato stage 2 (size model / size line) wall-clock since open: {:.3}s",
            open_started.elapsed().as_secs_f64()
        );

        match &dialog.size_model {
            SizeModelState::Loaded(Some(model)) => {
                let draft = ExportDraft {
                    contents: ExportContents::MeshAndArmor,
                    hull: None,
                    lod: 0,
                    texture_res: TextureResolution::Full,
                    camos: BTreeSet::new(),
                };
                let options = draft_to_options(&draft);
                assert!(
                    model.missing_scheme_ladders(&options).is_empty(),
                    "no camo selected on open needs no scheme ladders, even with ~100 on offer"
                );
            }
            SizeModelState::Loaded(None) => eprintln!("size model could not be built for Yamato"),
            SizeModelState::Loading(_) => unreachable!("loop only exits once Loading is resolved"),
        }
    }
}
