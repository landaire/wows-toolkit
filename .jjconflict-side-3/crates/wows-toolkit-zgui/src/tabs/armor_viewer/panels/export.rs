//! The GLB export dialog: what goes into an exported ship or model, and what it costs, then the
//! export it confirms.
//!
//! The choices are the shared ones (`wows_toolkit_viewmodel::armor::export`), so this collects the
//! same contents, hull, level of detail, texture cap and camo set as the other front ends and
//! remembers the same answers for the next ship. A single model has no hull or camo to choose, so
//! those rows are left out.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use rootcause::Report;
use rust_i18n::t;
use wows_toolkit_armor::assets::ModelAssets;
use wows_toolkit_armor::load_model::ModelLoadRequest;
use wows_toolkit_armor::load_model::building_has_armor;
use wows_toolkit_armor::load_model::export_model_glb;
use wows_toolkit_armor::load_model::thickness_known;
use wows_toolkit_armor::load_ship::ExportGlbError;
use wows_toolkit_armor::load_ship::build_hull_upgrade_names;
use wows_toolkit_armor::load_ship::default_export_filename;
use wows_toolkit_armor::load_ship::export_ship_glb;
use wows_toolkit_viewmodel::armor::export::ExportDefaults;
use wows_toolkit_viewmodel::armor::export::ExportDraft;
use wows_toolkit_viewmodel::armor::export::TextureResolution;
use wows_toolkit_viewmodel::armor::export::contents_label_key;
use wows_toolkit_viewmodel::armor::export::default_camo;
use wows_toolkit_viewmodel::armor::export::draft_to_options;
use wows_toolkit_viewmodel::formatting::byte_size;
use wowsunpack::export::camo_textures::CamoSchemeId;
use wowsunpack::export::camo_textures::CamoSchemeInfo;
use wowsunpack::export::gltf_export::CamoOrigin;
use wowsunpack::export::model_catalog::ModelPath;
use wowsunpack::export::model_library::ModelLoadError;
use wowsunpack::export::ship::ExportContents;
use wowsunpack::export::ship::ShipAssets;
use wowsunpack::export::ship::ShipExportOptions;
use wowsunpack::export::ship::ShipModelContext;
use wowsunpack::export::size_estimate::ExportSizeModel;
use wowsunpack::export::size_estimate::SchemeLadderStatus;
use wowsunpack::export::size_estimate::SchemeLadders;
use wowsunpack::export::size_estimate::SizeEstimate;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::GameParamProvider;
use zgui::component;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::task::blocking;
use zgui::task::spawn_detached;
use zgui::task::spawn_local;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::CheckboxProps;
use zgui_ui::Checked;
use zgui_ui::DialogContentProps;
use zgui_ui::DialogFooterProps;
use zgui_ui::DialogHeaderProps;
use zgui_ui::DialogTitleProps;
use zgui_ui::SpinnerProps;
use zgui_ui::ToggleProps;
use zgui_ui::ToggleSize;
use zgui_ui_primitives::Binding;

use crate::services::Job;
use crate::services::Services;
use crate::shell::DialogHandle;
use crate::shell::HostedDialogProps;
use crate::shell::Shell;

const SHEET: &str = "wt-armor-export";

const CSS: &str = css!(
    ".wt-armor-export { display: flex; flex-direction: column; gap: 8px; width: 460px; }
    .wt-armor-export__row { display: flex; align-items: flex-start; gap: 8px; }
    .wt-armor-export__label { flex: none; width: 96px; font-size: 12px; }
    .wt-armor-export__control { flex: 1; display: flex; flex-wrap: wrap; gap: 4px; }
    .wt-armor-export__hint { padding-left: 104px; font-size: 12px; color: var(--lui-color-text-dim); }
    .wt-armor-export__dim { font-size: 12px; color: var(--lui-color-text-dim); }
    .wt-armor-export__strong { font-size: 12px; color: var(--wt-color-text-strong); }
    .wt-armor-export__error { font-size: 12px; color: var(--wt-color-error); }
    .wt-armor-export__warn { font-size: 12px; color: var(--wt-color-warn); }
    .wt-armor-export__text { font-size: 12px; }
    .wt-armor-export__camos { flex: 1; display: flex; flex-direction: column; gap: 4px; max-height: 240px;
        overflow-y: auto; }
    .wt-armor-export__group { padding-top: 4px; font-size: 12px; font-weight: 700; }
    .wt-armor-export__check { display: flex; align-items: center; gap: 6px; font-size: 12px; }
    .wt-armor-export__line { display: flex; align-items: center; gap: 8px; }"
);

/// What an export dialog opens on.
pub enum ExportRequest {
    Ship {
        param_index: String,
        display_name: String,
        /// The hull the pane is showing, where there is one to seed from.
        seed_hull: Option<String>,
    },
    Model {
        request: ModelLoadRequest,
        models: Arc<ModelAssets>,
    },
}

/// What an export dialog writes.
#[derive(Clone)]
enum ExportTarget {
    Ship {
        param_index: String,
        /// `(raw upgrade key, label)`, in the hull vocabulary the viewer's own picker uses.
        hull_upgrades: Vec<(String, String)>,
    },
    Model {
        /// The model and the mount its armor thickness is read from.
        request: ModelLoadRequest,
        models: Arc<ModelAssets>,
    },
}

/// What the dialog has read of its target, enough to offer its levels of detail and its camos.
///
/// `context` is a ship's, kept so a camo ticked afterwards can be priced without reading the ship
/// again. A model has none and is not priced.
struct ShipMeta {
    hull_lod_count: usize,
    camo_schemes: Vec<CamoSchemeInfo>,
    context: Option<Arc<ShipModelContext>>,
    /// Whether there is armor to export: a model may carry none, or plates no ship gives a
    /// thickness.
    has_armor: bool,
}

/// How far the read of the target has got.
enum MetaState {
    Loading,
    Loaded(ShipMeta),
    Failed(String),
}

/// How far the read of the ship's texture ladders has got.
///
/// Tracked apart from the ship: the size model is slower to build, and only the size line waits on
/// it. `Loaded(None)` is a ship that could not be priced, which costs the size line and nothing
/// else.
enum SizeState {
    Loading,
    Loaded(Option<ExportSizeModel>),
}

/// What the size line has to say.
#[derive(Debug, PartialEq)]
enum SizeLine {
    /// Still being read, or waiting on a camo's ladders.
    Waiting,
    /// Nothing to say: this target could not be priced at all.
    Silent,
    Known(SizeEstimate),
    /// Some of the ticked camos could not be priced, and can be asked for again.
    Incomplete(Vec<CamoSchemeId>),
}

/// A pending export and the choices behind it.
struct ExportModel {
    target: ExportTarget,
    display_name: String,
    meta: MetaState,
    size: SizeState,
    /// The camo ladders being read, so the same set is not asked for twice while one read is in
    /// flight.
    pricing: BTreeSet<CamoSchemeId>,
    /// Bumped on every read of the target. A read reports only if it is still the one the dialog
    /// waits for: two hull changes in quick succession leave two reads in flight, and the older one
    /// landing last would describe a hull the draft has moved off.
    generation: u64,
    draft: ExportDraft,
}

impl ExportModel {
    fn new(target: ExportTarget, display_name: String, seed_hull: Option<String>, defaults: ExportDefaults) -> Self {
        let draft = ExportDraft {
            contents: defaults.contents,
            hull: seed_hull,
            lod: defaults.lod,
            texture_res: defaults.texture_res,
            camos: BTreeSet::new(),
        };

        Self {
            target,
            display_name,
            meta: MetaState::Loading,
            size: SizeState::Loading,
            pricing: BTreeSet::new(),
            generation: 0,
            draft,
        }
    }

    /// Starts a new read of the target, and answers its generation.
    fn begin_read(&mut self) -> u64 {
        self.meta = MetaState::Loading;
        self.size = SizeState::Loading;
        self.pricing.clear();
        self.generation += 1;
        self.generation
    }

    /// Takes the target a read turned up, and the draft it implies.
    ///
    /// The level of detail is held to what this hull has, and the camo named "default" is ticked
    /// where there is one, so an export set up in any front end bakes the same appearance.
    fn adopt_meta(&mut self, meta: ShipMeta) {
        self.draft.lod = self.draft.lod.min(meta.hull_lod_count.saturating_sub(1));
        if !meta.has_armor {
            self.draft.contents = ExportContents::Mesh;
        }
        self.draft.camos.clear();
        if let Some(scheme) = default_camo(&meta.camo_schemes) {
            self.draft.camos.insert(scheme);
        }
        self.meta = MetaState::Loaded(meta);
    }

    /// What the size line has to say, and the camos whose ladders it still waits on.
    fn size_line(&self) -> (SizeLine, Vec<CamoSchemeId>) {
        let SizeState::Loaded(model) = &self.size else { return (SizeLine::Waiting, Vec::new()) };
        let Some(model) = model else { return (SizeLine::Silent, Vec::new()) };

        match model.priced_estimate(&draft_to_options(&self.draft)) {
            Ok(estimate) => (SizeLine::Known(estimate), Vec::new()),
            Err(unpriced) => {
                let of = |wanted: SchemeLadderStatus| -> Vec<CamoSchemeId> {
                    unpriced.iter().filter(|(_, status)| *status == wanted).map(|(id, _)| *id).collect()
                };
                let (pending, failed) = (of(SchemeLadderStatus::Pending), of(SchemeLadderStatus::Failed));
                // A read that failed never asks again on its own, so without this the size line
                // would simply disappear.
                let line = if failed.is_empty() { SizeLine::Waiting } else { SizeLine::Incomplete(failed) };
                (line, pending)
            }
        }
    }

    /// Takes the camo ladders read for `asking` under `generation`. A read of an earlier target is
    /// dropped: its ladders are another hull's, and the newer read keeps its own pricing set.
    fn adopt_ladders(
        &mut self,
        generation: u64,
        asking: Vec<CamoSchemeId>,
        read: Vec<(CamoSchemeId, Option<SchemeLadders>)>,
    ) {
        if generation != self.generation {
            return;
        }

        if let SizeState::Loaded(Some(sizes)) = &mut self.size {
            for (id, ladders) in read {
                match ladders {
                    Some(ladders) => sizes.insert_scheme_ladders(id, ladders),
                    None => sizes.mark_scheme_ladders_failed(id),
                }
            }
        }
        for id in asking {
            self.pricing.remove(&id);
        }
    }

    /// Puts the camo selection back to the one the target opens with.
    fn reset_camos(&mut self) {
        self.draft.camos.clear();
        if let MetaState::Loaded(meta) = &self.meta
            && let Some(scheme) = default_camo(&meta.camo_schemes)
        {
            self.draft.camos.insert(scheme);
        }
    }

    /// Whether the target has been read, which is what the choices describe. Exporting before it
    /// lands would write a level of detail the hull may not have and a camo set nobody has seen.
    fn is_ready(&self) -> bool {
        matches!(self.meta, MetaState::Loaded(_))
    }

    fn has_armor(&self) -> bool {
        match &self.meta {
            MetaState::Loaded(meta) => meta.has_armor,
            MetaState::Loading | MetaState::Failed(_) => true,
        }
    }

    fn lod_count(&self) -> usize {
        match &self.meta {
            MetaState::Loaded(meta) => meta.hull_lod_count,
            MetaState::Loading | MetaState::Failed(_) => 0,
        }
    }

    fn schemes(&self) -> &[CamoSchemeInfo] {
        match &self.meta {
            MetaState::Loaded(meta) => &meta.camo_schemes,
            MetaState::Loading | MetaState::Failed(_) => &[],
        }
    }

    fn hulls(&self) -> &[(String, String)] {
        match &self.target {
            ExportTarget::Ship { hull_upgrades, .. } => hull_upgrades,
            ExportTarget::Model { .. } => &[],
        }
    }

    /// No hull chosen means the first one, which is what the exporter takes.
    fn hull_chosen(&self, key: &str, index: usize) -> bool {
        match self.draft.hull.as_deref() {
            Some(chosen) => chosen == key,
            None => index == 0,
        }
    }

    fn title(&self) -> String {
        let heading = match &self.target {
            ExportTarget::Ship { .. } => t!("ui.armor.export_model"),
            ExportTarget::Model { .. } => t!("ui.model_view.export_title"),
        };
        format!("{heading} - {}", self.display_name)
    }
}

/// The open dialog: its model and the assets its reads run against.
#[derive(Clone, Copy)]
struct ExportDialog {
    model: RwSignal<ExportModel, LocalStorage>,
    assets: StoredValue<Arc<ShipAssets>>,
}

impl ExportDialog {
    /// Reads the target, then prices it, off the UI thread.
    ///
    /// Two steps rather than one: the levels of detail and the camo list are what the controls
    /// need, and they are ready long before the texture ladders the size line waits on.
    fn read_target(self) {
        let mut reading = 0;
        let mut asked = None;
        self.model.update(|model| {
            reading = model.begin_read();
            asked = Some((model.target.clone(), model.draft.hull.clone()));
        });
        let Some((target, hull)) = asked else { return };
        let assets = self.assets.get_value();
        let model = self.model;

        spawn_local(async move {
            let read = blocking(move || match &target {
                ExportTarget::Ship { param_index, .. } => read_ship_meta(&assets, param_index, hull),
                ExportTarget::Model { request, models } => {
                    read_model_meta(models, assets.metadata(), request).map_err(|error| error.to_string())
                }
            })
            .await;
            let current = move || model.with_untracked(|model| model.generation == reading);
            if !current() {
                return;
            }
            let context = match read {
                Ok(meta) => {
                    let context = meta.context.clone();
                    model.update(|model| model.adopt_meta(meta));
                    context
                }
                Err(why) => {
                    model.update(|model| {
                        model.meta = MetaState::Failed(why);
                        model.size = SizeState::Loaded(None);
                    });
                    return;
                }
            };
            // Only a ship's texture ladders are modelled; a model's size line stays silent.
            let Some(context) = context else {
                model.update(|model| model.size = SizeState::Loaded(None));
                return;
            };

            let priced = blocking(move || {
                context.size_model().map_err(|report| format!("{report:?}")).inspect_err(|why| {
                    tracing::warn!("armor viewer: the export size could not be modelled: {why}");
                })
            })
            .await;
            if current() {
                model.update(|model| model.size = SizeState::Loaded(priced.ok()));
            }
        });
    }

    /// Adopts a change to the draft, reading the target again when the hull changed: a different
    /// hull is a different model, with its own levels of detail, camos and size.
    fn edit(self, change: impl FnOnce(&mut ExportDraft)) {
        let mut hull_changed = false;
        self.model.update(|model| {
            let before = model.draft.hull.clone();
            change(&mut model.draft);
            hull_changed = model.draft.hull != before;
        });
        if hull_changed {
            self.read_target();
        }
    }

    /// Reads the ladders of camos the size model was not built with, in one batch: the assets.bin
    /// parse behind it is shared across ids.
    fn price_schemes(self, wanted: Vec<CamoSchemeId>) {
        let mut asked = None;
        self.model.update(|model| {
            let asking: Vec<CamoSchemeId> = wanted.into_iter().filter(|id| !model.pricing.contains(id)).collect();
            if asking.is_empty() {
                return;
            }
            let MetaState::Loaded(meta) = &model.meta else { return };
            let Some(context) = meta.context.as_ref().map(Arc::clone) else { return };
            model.pricing.extend(asking.iter().copied());
            asked = Some((asking, context));
        });
        let Some((asking, context)) = asked else { return };
        let generation = self.model.with_untracked(|model| model.generation);
        let model = self.model;

        spawn_local(async move {
            let ids = asking.clone();
            let read = blocking(move || match context.scheme_ladders_batch(&ids) {
                Ok(batch) => batch.into_iter().map(|(id, ladders)| (id, Some(ladders))).collect(),
                // A read that failed is recorded as failed rather than merged in as a camo that
                // costs nothing.
                Err(why) => {
                    tracing::warn!("armor viewer: {} camo scheme(s) could not be priced: {why:?}", ids.len());
                    ids.into_iter().map(|id| (id, None)).collect::<Vec<_>>()
                }
            })
            .await;

            model.update(|model| model.adopt_ladders(generation, asking, read));
        });
    }

    /// Asks again for the ladders of camos whose last read failed.
    fn price_again(self, failed: Vec<CamoSchemeId>) {
        self.model.update(|model| {
            if let SizeState::Loaded(Some(sizes)) = &mut model.size {
                for id in &failed {
                    sizes.clear_failed_scheme_ladders(*id);
                }
            }
        });
        self.price_schemes(failed);
    }
}

/// Opens the export dialog on `request`, seeded with the remembered choices, and writes the model
/// it confirms.
pub fn open_export_dialog(shell: Shell, request: ExportRequest, assets: Arc<ShipAssets>) {
    let services = shell.services();
    let pool = services.settings.pool();
    let loading = services.on_tokio(async move {
        match pool {
            Some(pool) => ExportDefaults::load(&pool).await,
            None => ExportDefaults::default(),
        }
    });

    spawn_local(async move {
        let defaults = loading.await.unwrap_or_else(|err| {
            tracing::warn!("armor viewer: the export defaults could not be read: {err}");
            ExportDefaults::default()
        });
        let (target, display_name, seed_hull) = match request {
            ExportRequest::Ship { param_index, display_name, seed_hull } => {
                let hull_upgrades = hull_upgrades_for(&assets, &param_index);
                (ExportTarget::Ship { param_index, hull_upgrades }, display_name, seed_hull)
            }
            ExportRequest::Model { request, models } => {
                let display_name = request.path.stem().to_string();
                (ExportTarget::Model { request, models }, display_name, None)
            }
        };
        shell.dialogs.open(move |handle| {
            let model = ExportModel::new(target.clone(), display_name.clone(), seed_hull.clone(), defaults);
            let dialog =
                ExportDialog { model: RwSignal::new_local(model), assets: StoredValue::new(Arc::clone(&assets)) };
            AnyView::new(view! { ExportDialogView(dialog = dialog, handle = handle, shell = shell) })
        });
    });
}

/// The dialog's content.
#[component]
fn ExportDialogView(dialog: ExportDialog, handle: DialogHandle, shell: Shell) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let model = dialog.model;
    dialog.read_target();

    // Asks for the ladders of ticked camos the size model lacks, as the draft changes.
    let pricing = RenderEffect::new(move |_| {
        let (_, pending) = model.with(ExportModel::size_line);
        if !pending.is_empty() {
            dialog.price_schemes(pending);
        }
    });
    on_cleanup_local(move || drop(pricing));

    let title = move || model.with(ExportModel::title);
    let textured = move || model.with(|model| model.draft.contents.includes_mesh());

    let contents = move || {
        let (has_armor, chosen) = model.with(|model| (model.has_armor(), model.draft.contents));
        offered_contents(has_armor)
            .enumerate()
            .map(|(index, contents)| {
                let label = t!(contents_label_key(contents)).into_owned();
                choice(format!("armor-export-contents-{index}"), label, contents == chosen, false, move || {
                    dialog.edit(|draft| draft.contents = contents)
                })
            })
            .collect::<Vec<_>>()
    };

    let hulls = move || {
        let (hulls, loading) = model.with(|model| {
            let hulls: Vec<(String, String, bool)> = model
                .hulls()
                .iter()
                .enumerate()
                .map(|(index, (key, label))| (key.clone(), label.clone(), model.hull_chosen(key, index)))
                .collect();
            (hulls, matches!(model.meta, MetaState::Loading))
        });
        if hulls.is_empty() {
            return AnyView::new(());
        }
        let buttons: Vec<AnyView> = hulls
            .into_iter()
            .enumerate()
            .map(|(index, (key, label, chosen))| {
                // Off while the ship is read: a read is per hull, and switching mid-read spends a
                // whole ship load on a result that is then thrown away.
                choice(format!("armor-export-hull-{index}"), label, chosen, loading, move || {
                    let key = key.clone();
                    dialog.edit(|draft| draft.hull = Some(key));
                })
            })
            .collect();
        AnyView::new(view! {
            row(class = "wt-armor-export__row") {
                text(class = "wt-armor-export__label") {{t!("ui.armor.export.hull").into_owned()}}
                row(class = "wt-armor-export__control") {
                    {buttons}
                }
            }
        })
    };

    let lods = move || {
        let (count, chosen, textured) =
            model.with(|model| (model.lod_count(), model.draft.lod, model.draft.contents.includes_mesh()));
        if count <= 1 {
            return AnyView::new(());
        }
        let buttons: Vec<AnyView> = (0..count)
            .map(|lod| {
                choice(format!("armor-export-lod-{lod}"), lod.to_string(), lod == chosen, !textured, move || {
                    dialog.edit(|draft| draft.lod = lod)
                })
            })
            .collect();
        AnyView::new(view! {
            column {
                row(class = "wt-armor-export__row") {
                    text(class = "wt-armor-export__label") {{t!("ui.armor.export.lod").into_owned()}}
                    row(class = "wt-armor-export__control") {
                        {buttons}
                    }
                }
                text(class = "wt-armor-export__hint") {{t!("ui.armor.export.lod_tooltip").into_owned()}}
            }
        })
    };

    // Greyed rather than taken away: a dialog that changed height with the contents would move the
    // button out from under the pointer.
    let resolutions = move || {
        let (chosen, textured) = model.with(|model| (model.draft.texture_res, model.draft.contents.includes_mesh()));
        TextureResolution::ALL
            .into_iter()
            .enumerate()
            .map(|(index, res)| {
                choice(
                    format!("armor-export-res-{index}"),
                    t!(res.label_key()).into_owned(),
                    res == chosen,
                    !textured,
                    move || dialog.edit(|draft| draft.texture_res = res),
                )
            })
            .collect::<Vec<_>>()
    };

    let camos = move || {
        let empty = model.with(|model| model.schemes().is_empty());
        if empty {
            return AnyView::new(());
        }
        AnyView::new(view! {
            row(class = "wt-armor-export__row") {
                text(class = "wt-armor-export__label") {{t!("ui.armor.export.camos").into_owned()}}
                CamoList(dialog = dialog)
            }
        })
    };

    let failed = move || {
        let why = model.with(|model| match &model.meta {
            MetaState::Failed(why) => Some(why.clone()),
            MetaState::Loading | MetaState::Loaded(_) => None,
        })?;
        Some(AnyView::new(view! {
            text(class = "wt-armor-export__error", attr:data-testid = "armor-export-failed") {
                {t!("ui.armor.load_failed", error = why).into_owned()}
            }
        }))
    };

    let size = move || match model.with(|model| model.size_line().0) {
        SizeLine::Silent => AnyView::new(()),
        SizeLine::Waiting => AnyView::new(view! {
            row(class = "wt-armor-export__line", attr:data-testid = "armor-export-size-waiting") {
                Spinner(label = t!("ui.armor.export.loading_details").into_owned())
                text(class = "wt-armor-export__dim") {{t!("ui.armor.export.loading_details").into_owned()}}
            }
        }),
        SizeLine::Known(estimate) => {
            let total = humansize::format_size(estimate.total(), humansize::BINARY);
            let geometry = humansize::format_size(estimate.geometry_bytes, humansize::BINARY);
            let textures = humansize::format_size(estimate.texture_bytes, humansize::BINARY);
            AnyView::new(view! {
                column(attr:data-testid = "armor-export-size") {
                    text(class = "wt-armor-export__text") {{t!("ui.armor.export.size_estimate", size = total).into_owned()}}
                    text(class = "wt-armor-export__dim") {
                        {t!("ui.armor.export.size_breakdown", geometry = geometry, textures = textures).into_owned()}
                    }
                }
            })
        }
        SizeLine::Incomplete(failed) => {
            let count = failed.len();
            let failed = StoredValue::new_local(failed);
            AnyView::new(view! {
                row(class = "wt-armor-export__line") {
                    text(class = "wt-armor-export__warn") {{t!("ui.armor.export.size_incomplete", count = count).into_owned()}}
                    Button(
                        variant = ButtonVariant::Outline,
                        size = ButtonSize::Sm,
                        attr:data-testid = "armor-export-size-retry",
                        on:click = move |_| dialog.price_again(failed.get_value())
                    ) {
                        {t!("ui.buttons.retry").into_owned()}
                    }
                }
            })
        }
    };

    let not_ready = Signal::derive_local(move || !model.with(ExportModel::is_ready));

    view! {
        HostedDialog(handle = handle) {
            DialogContent {
                DialogHeader {
                    DialogTitle {{title}}
                }
                column(class = "wt-armor-export", attr:data-testid = "armor-export-dialog") {
                    row(class = "wt-armor-export__row") {
                        text(class = "wt-armor-export__label") {{t!("ui.armor.export.contents").into_owned()}}
                        row(class = "wt-armor-export__control") {
                            {contents}
                        }
                    }
                    {hulls}
                    {lods}
                    row(class = "wt-armor-export__row", attr:data-textured = move || textured().then(|| "true".to_owned())) {
                        text(class = "wt-armor-export__label") {{t!("ui.armor.export.res").into_owned()}}
                        row(class = "wt-armor-export__control") {
                            {resolutions}
                        }
                    }
                    {camos}
                    {failed}
                    {size}
                    // Where the models come from, beside a button that writes one to disk.
                    text(class = "wt-armor-export__dim") {{t!("ui.armor.export_disclaimer").into_owned()}}
                }
                DialogFooter {
                    Button(
                        variant = ButtonVariant::Outline,
                        size = ButtonSize::Sm,
                        attr:data-testid = "armor-export-cancel",
                        on:click = move |_| handle.close()
                    ) {
                        {t!("ui.buttons.cancel").into_owned()}
                    }
                    Button(
                        size = ButtonSize::Sm,
                        disabled = not_ready,
                        attr:data-testid = "armor-export-confirm",
                        on:click = move |_| confirm(shell, dialog, handle)
                    ) {
                        {t!("ui.armor.export_button").into_owned()}
                    }
                }
            }
        }
    }
}

/// One choice of a row, pressed while it is the one chosen.
fn choice(test_id: String, label: String, chosen: bool, disabled: bool, choose: impl Fn() + 'static) -> AnyView {
    AnyView::new(view! {
        Toggle(
            size = ToggleSize::Sm,
            pressed = Binding::controlled(Signal::stored_local(chosen), move |_: bool| choose()),
            disabled = disabled,
            label = label.clone(),
            attr:data-testid = test_id
        ) {
            {label.clone()}
        }
    })
}

/// The camos this ship can be exported wearing.
///
/// Grouped and sorted as the viewer's own picker groups them: the ship's own schemes first, then
/// the universal, expendable and legacy ones, each by name. A stock line at the top says what an
/// export with nothing ticked bakes.
#[component]
fn CamoList(dialog: ExportDialog) -> impl IntoView {
    let model = dialog.model;
    let textured = move || model.with(|model| model.draft.contents.includes_mesh());
    let none_ticked = move || model.with(|model| model.draft.camos.is_empty());

    let groups = move || {
        let schemes: Vec<CamoSchemeInfo> = model.with(|model| model.schemes().to_vec());
        let mut index = 0usize;
        let mut views = Vec::new();
        for group in camo_groups(&schemes) {
            if let Some(heading) = group.heading {
                views.push(AnyView::new(view! {
                    text(class = "wt-armor-export__group") {{t!(heading).into_owned()}}
                }));
            }
            for scheme in group.schemes {
                let id = scheme.id;
                let name = scheme.display_name.clone();
                let ticked = Signal::derive_local(move || {
                    if model.with(|model| model.draft.camos.contains(&id)) { Checked::Yes } else { Checked::No }
                });
                let tick = move |checked: Checked| {
                    dialog.edit(|draft| {
                        if checked == Checked::Yes {
                            draft.camos.insert(id);
                        } else {
                            draft.camos.remove(&id);
                        }
                    })
                };
                views.push(AnyView::new(view! {
                    row(class = "wt-armor-export__check") {
                        Checkbox(
                            checked = Binding::controlled(ticked, tick),
                            disabled = Signal::derive_local(move || !textured()),
                            attr:data-testid = format!("armor-export-camo-{index}"),
                            a11y:label = name.clone()
                        )
                        text {{name.clone()}}
                    }
                }));
                index += 1;
            }
        }
        views
    };

    view! {
        column(class = "wt-armor-export__camos", attr:data-testid = "armor-export-camos") {
            row(class = "wt-armor-export__line") {
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Sm,
                    disabled = Signal::derive_local(move || !textured()),
                    attr:data-testid = "armor-export-camo-none",
                    on:click = move |_| dialog.edit(|draft| draft.camos.clear())
                ) {
                    {t!("ui.armor.export.camo_select_none").into_owned()}
                }
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Sm,
                    disabled = Signal::derive_local(move || !textured()),
                    attr:data-testid = "armor-export-camo-reset",
                    on:click = move |_| model.update(ExportModel::reset_camos)
                ) {
                    {t!("ui.armor.export.camo_reset").into_owned()}
                }
            }
            // Not a control: nothing ticked is what it names.
            text(
                class = "wt-armor-export__dim",
                class:wt-armor-export__strong = none_ticked,
                attr:data-testid = "armor-export-camo-stock"
            ) {
                {t!("ui.armor.camo_none").into_owned()}
            }
            {groups}
        }
    }
}

/// Remembers the choices, closes the dialog, asks where to write the file and writes it.
fn confirm(shell: Shell, dialog: ExportDialog, handle: DialogHandle) {
    let confirmed = dialog.model.with_untracked(|model| {
        model.is_ready().then(|| {
            (
                draft_to_options(&model.draft),
                ExportDefaults::from_draft(&model.draft),
                model.target.clone(),
                model.display_name.clone(),
            )
        })
    });
    let Some((options, remember, target, display_name)) = confirmed else { return };
    let assets = dialog.assets.get_value();
    handle.close();
    // Remembered when the reader says yes, not when they pick a file: the choices are theirs either
    // way.
    remember_export_defaults(shell.services(), remember);

    let write = move |path: PathBuf| -> Result<(), ExportGlbError> {
        match target {
            ExportTarget::Ship { param_index, .. } => export_ship_glb(&assets, &param_index, &options, &path),
            ExportTarget::Model { request, models } => {
                export_model_glb(&models, assets.metadata(), &request, &options, &path)
            }
        }
    };
    write_glb(shell, display_name, write);
}

/// Asks where to write `display_name`'s GLB, then writes it with `write` off the UI thread under a
/// status bar job, and reports the outcome. Shared with the viewport's own export of what it shows.
pub fn write_glb(
    shell: Shell,
    display_name: String,
    write: impl FnOnce(PathBuf) -> Result<(), ExportGlbError> + Send + 'static,
) {
    let asked = wows_toolkit_services::dialog::save_file(
        None,
        &default_export_filename(&display_name),
        Some(wows_toolkit_services::dialog::GLB),
    );
    let services = shell.services();

    // Detached: the export outlives the dialog that asked for it.
    spawn_detached(async move {
        let Some(path) = shell.chosen_path(asked).await else { return };
        let _job = services.jobs.start(Job {
            name: t!("ui.armor.export.exporting", ship = display_name.as_str()).into_owned(),
            progress: None,
        });
        let written = blocking(move || {
            write(path.clone())?;
            // The size is what the toast reports beside the ship, and it is only knowable once the
            // file is on disk.
            let size = std::fs::metadata(&path).map(|meta| meta.len()).ok();
            Ok((path, size))
        })
        .await;
        report_export(shell, written, &display_name);
    });
}

/// Reports a finished export: what was written and how large it is, or why it was not. The
/// destination is logged rather than shown: the reader chose it in the save dialog.
fn report_export(shell: Shell, written: Result<(PathBuf, Option<u64>), ExportGlbError>, ship: &str) {
    match written {
        Ok((path, size)) => {
            tracing::info!("armor viewer: exported {ship} to {}", path.display());
            let message = match size {
                Some(bytes) => {
                    t!("ui.armor.export.exported_with_size", ship = ship, size = byte_size(bytes)).into_owned()
                }
                None => t!("ui.armor.export.exported", ship = ship).into_owned(),
            };
            shell.toasts().ok(message);
        }
        Err(err) => {
            tracing::error!("armor viewer: failed to export {ship}: {err}");
            shell.toasts().failed(t!("ui.armor.export.export_failed", error = err.to_string()).into_owned());
        }
    }
}

/// Saves what an export was asked to contain, for the next one. The row is the one the other front
/// ends write, so an export set up in any of them is what the others offer next.
fn remember_export_defaults(services: Services, defaults: ExportDefaults) {
    let Some(pool) = services.settings.pool() else { return };
    let saving = services.on_tokio(async move { defaults.save(&pool).await });
    spawn_detached(async move {
        match saving.await {
            Ok(Ok(())) => {}
            Ok(Err(err)) => tracing::warn!("armor viewer: the export defaults were not saved: {err}"),
            Err(err) => tracing::warn!("armor viewer: the export defaults write did not complete: {err}"),
        }
    });
}

/// The ship as the dialog needs it, read off the UI thread.
fn read_ship_meta(assets: &ShipAssets, param_index: &str, hull: Option<String>) -> Result<ShipMeta, String> {
    let param = assets.metadata().game_param_by_index(param_index);
    let vehicle = param
        .as_ref()
        .and_then(|param| param.vehicle().cloned())
        .ok_or_else(|| t!("ui.armor.export.vehicle_not_found").into_owned())?;
    // No textures and no armor: this read is for the levels of detail and the camo list, and
    // reading either would cost far more than they do.
    let probe =
        ShipExportOptions { hull, textures: false, contents: ExportContents::Mesh, ..ShipExportOptions::default() };
    let context = assets.load_ship_from_vehicle(&vehicle, &probe).map_err(|report| format!("{report:?}"))?;
    let context = Arc::new(context);
    let source = context.camo_texture_source().map_err(|report| format!("{report:?}"))?;

    Ok(ShipMeta {
        hull_lod_count: context.hull_lod_count(),
        camo_schemes: source.scheme_infos(),
        context: Some(context),
        has_armor: true,
    })
}

/// Why a model's export choices could not be read.
#[derive(Debug, thiserror::Error)]
#[error("failed to read model {path}: {report}")]
struct ModelMetaError {
    path: ModelPath,
    report: Report<ModelLoadError>,
}

/// The model as the dialog needs it: its levels of detail and whether it has armor to export.
/// Reads headers only, no mesh or texture.
fn read_model_meta(
    models: &ModelAssets,
    metadata: &GameMetadataProvider,
    request: &ModelLoadRequest,
) -> Result<ShipMeta, ModelMetaError> {
    let summary = models
        .library
        .model_summary(&request.path)
        .map_err(|report| ModelMetaError { path: request.path.clone(), report })?;
    let thickness_known = thickness_known(models, metadata, request);
    let has_armor = building_has_armor(models, request).unwrap_or(summary.has_armor);

    Ok(ShipMeta {
        hull_lod_count: summary.lod_count,
        camo_schemes: Vec::new(),
        context: None,
        has_armor: model_exports_armor(has_armor, thickness_known),
    })
}

/// Whether a model's armor is offered. Plates no ship gives a thickness would be written as 0 mm,
/// which the viewer shows as unknown.
fn model_exports_armor(has_armor: bool, thickness_known: bool) -> bool {
    has_armor && thickness_known
}

/// The hull upgrades this ship offers, in the viewer's own vocabulary. An empty list means the ship
/// offers no hull choice; a ship that cannot be resolved is reported by the read itself.
fn hull_upgrades_for(assets: &ShipAssets, param_index: &str) -> Vec<(String, String)> {
    let Some(param) = assets.metadata().game_param_by_index(param_index) else { return Vec::new() };
    let Some(vehicle) = param.vehicle() else { return Vec::new() };
    build_hull_upgrade_names(vehicle)
}

/// One heading's worth of camos.
struct CamoGroup<'a> {
    /// `None` for the ship's own schemes, which need no heading.
    heading: Option<&'static str>,
    schemes: Vec<&'a CamoSchemeInfo>,
}

/// The schemes in the order the picker lists them.
fn camo_groups(schemes: &[CamoSchemeInfo]) -> Vec<CamoGroup<'_>> {
    let mut groups = Vec::new();
    for (origin, heading) in [
        (CamoOrigin::ShipSpecific, None),
        (CamoOrigin::Universal, Some("ui.armor.camo_group_universal")),
        (CamoOrigin::Expendable, Some("ui.armor.camo_group_expendable")),
        (CamoOrigin::LegacyScan, Some("ui.armor.camo_group_other")),
    ] {
        let mut group: Vec<&CamoSchemeInfo> = schemes.iter().filter(|scheme| scheme.origin == origin).collect();
        if group.is_empty() {
            continue;
        }
        group.sort_by_key(|scheme| scheme.display_name.to_lowercase());
        groups.push(CamoGroup { heading, schemes: group });
    }
    groups
}

/// What an export can be asked to contain: a model without armor has only its meshes to write.
fn offered_contents(has_armor: bool) -> impl Iterator<Item = ExportContents> {
    [ExportContents::Mesh, ExportContents::Armor, ExportContents::MeshAndArmor]
        .into_iter()
        .filter(move |contents| has_armor || !contents.includes_armor())
}

#[cfg(test)]
mod tests {
    use wows_toolkit_viewmodel::armor::export::ExportDefaults;
    use wowsunpack::export::camo_textures::CamoSchemeId;
    use wowsunpack::export::ship::ExportContents;

    use super::ExportModel;
    use super::ExportTarget;
    use super::model_exports_armor;
    use super::offered_contents;

    /// A camo read asked for before a hull change lands after the new hull's read asked for the
    /// same camo. It must not take the camo off the new read's pricing set, which would ask for
    /// it a second time.
    #[test]
    fn camo_pricing_from_an_earlier_read_is_dropped() {
        let target = ExportTarget::Ship { param_index: "PASB018".to_owned(), hull_upgrades: Vec::new() };
        let mut model = ExportModel::new(target, "Iowa".to_owned(), None, ExportDefaults::default());
        let camo = CamoSchemeId(3);
        let earlier = model.begin_read();
        model.pricing.insert(camo);

        model.begin_read();
        model.pricing.insert(camo);
        model.adopt_ladders(earlier, vec![camo], vec![(camo, None)]);

        assert!(model.pricing.contains(&camo), "the newer read is still pricing it");

        model.adopt_ladders(model.generation, vec![camo], vec![(camo, None)]);
        assert!(!model.pricing.contains(&camo), "its own read lands");
    }

    #[test]
    fn a_model_without_armor_is_offered_only_its_meshes() {
        assert_eq!(offered_contents(false).collect::<Vec<_>>(), vec![ExportContents::Mesh]);
        assert_eq!(offered_contents(true).count(), 3);
    }

    #[test]
    fn armor_of_unknown_thickness_is_not_offered() {
        let unknown = offered_contents(model_exports_armor(true, false)).collect::<Vec<_>>();
        assert_eq!(unknown, vec![ExportContents::Mesh]);

        let known = offered_contents(model_exports_armor(true, true)).collect::<Vec<_>>();
        assert_eq!(known, vec![ExportContents::Mesh, ExportContents::Armor, ExportContents::MeshAndArmor]);

        let absent = offered_contents(model_exports_armor(false, true)).collect::<Vec<_>>();
        assert_eq!(absent, vec![ExportContents::Mesh]);
    }
}
