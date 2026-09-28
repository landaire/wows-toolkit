//! What goes into an exported ship model, and what it costs.
//!
//! The choices are the shared ones (`wows_toolkit_viewmodel::armor::export`), so
//! this collects the same contents, hull, level of detail, texture cap and camo
//! set the egui dialog does and remembers the same answers for the next ship.

use std::collections::BTreeSet;
use std::sync::Arc;

use gpui_kit::component::Disableable;
use gpui_kit::component::Selectable;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dialog::Dialog;
use gpui_kit::component::h_flex;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rust_i18n::t;
use wows_toolkit_viewmodel::armor::export::ExportDefaults;
use wows_toolkit_viewmodel::armor::export::ExportDraft;
use wows_toolkit_viewmodel::armor::export::TextureResolution;
use wows_toolkit_viewmodel::armor::export::contents_label_key;
use wows_toolkit_viewmodel::armor::export::default_camo;
use wows_toolkit_viewmodel::armor::export::draft_to_options;
use wowsunpack::export::camo_textures::CamoSchemeId;
use wowsunpack::export::camo_textures::CamoSchemeInfo;
use wowsunpack::export::gltf_export::CamoOrigin;
use wowsunpack::export::ship::ExportContents;
use wowsunpack::export::ship::ShipExportOptions;
use wowsunpack::export::ship::ShipModelContext;
use wowsunpack::export::size_estimate::ExportSizeModel;
use wowsunpack::export::size_estimate::SchemeLadderStatus;
use wowsunpack::export::size_estimate::SizeEstimate;

/// The ship the dialog has read, enough to offer its levels of detail and its
/// camo schemes.
///
/// `context` is kept so a camo ticked afterwards can be priced without reading
/// the ship again.
struct ShipMeta {
    hull_lod_count: usize,
    camo_schemes: Vec<CamoSchemeInfo>,
    context: Arc<ShipModelContext>,
}

/// How far the read of the ship has got.
enum MetaState {
    Loading,
    Loaded(ShipMeta),
    Failed(String),
}

/// How far the read of the ship's texture ladders has got.
///
/// The size model is slower to build than the ship itself, so it is tracked on
/// its own: only the size line waits on it. `Loaded(None)` is a ship that could
/// not be priced, which costs the size line and nothing else.
enum SizeState {
    Loading,
    Loaded(Option<ExportSizeModel>),
}

/// What the size line has to say.
enum SizeLine {
    /// Still being read, or waiting on a camo's ladders.
    Waiting,
    /// Nothing to say: this ship could not be priced at all.
    Silent,
    Known(SizeEstimate),
    /// Some of the ticked camos could not be priced, and can be asked for again.
    Incomplete(Vec<CamoSchemeId>),
}

/// The dialog's state moved.
///
/// A dialog is not part of any view's tree, so nothing redraws the window when
/// a read reports into this. The pane that opened it listens for this and asks
/// for the redraw.
pub(crate) struct Changed;

/// A pending ship model export, and the choices behind it.
pub(crate) struct ExportDialogState {
    param_index: String,
    display_name: String,
    /// `(raw upgrade key, label)`, in the hull vocabulary the pane's own picker
    /// uses, so one ship is described the same way in both places.
    hull_upgrades: Vec<(String, String)>,
    meta: MetaState,
    size: SizeState,
    /// The camo ladders being read, so the same set is not asked for twice while
    /// one read is in flight.
    pricing: BTreeSet<CamoSchemeId>,
    /// Bumped on every read of the ship. A read reports only if it is still the
    /// one the dialog is waiting for: two hull changes in quick succession leave
    /// two reads in flight, and the older landing last would describe a hull the
    /// draft has moved off.
    generation: u64,
    draft: ExportDraft,
    assets: Arc<super::assets::ArmorAssetsBundle>,
}

impl ExportDialogState {
    /// Opens on a ship, seeding the hull from whatever the caller has on screen.
    pub(crate) fn new(
        param_index: String,
        display_name: String,
        seed_hull: Option<String>,
        defaults: ExportDefaults,
        assets: Arc<super::assets::ArmorAssetsBundle>,
        cx: &mut Context<Self>,
    ) -> Self {
        let hull_upgrades = hull_upgrades_for(&assets.assets, &param_index);
        let draft = ExportDraft {
            contents: defaults.contents,
            hull: seed_hull,
            lod: defaults.lod,
            texture_res: defaults.texture_res,
            camos: BTreeSet::new(),
        };
        let mut state = Self {
            param_index,
            display_name,
            hull_upgrades,
            meta: MetaState::Loading,
            size: SizeState::Loading,
            pricing: BTreeSet::new(),
            generation: 0,
            draft,
            assets,
        };
        state.read_ship(cx);
        state
    }

    /// Reads the ship and then prices it, on the background executor.
    ///
    /// Two steps rather than one call: the levels of detail and the camo list
    /// are what the controls need, and they are ready long before the texture
    /// ladders the size line waits on.
    fn read_ship(&mut self, cx: &mut Context<Self>) {
        self.meta = MetaState::Loading;
        self.size = SizeState::Loading;
        self.pricing.clear();
        self.generation += 1;
        let reading = self.generation;

        let assets = Arc::clone(&self.assets);
        let param_index = self.param_index.clone();
        let hull = self.draft.hull.clone();
        cx.spawn(async move |this, cx| {
            let read = cx.background_spawn(async move { read_ship_meta(&assets.assets, &param_index, hull) }).await;
            let context = match read {
                Ok((meta, context)) => {
                    let landed = this.update(cx, |this, cx| {
                        if this.generation != reading {
                            return false;
                        }
                        this.adopt_meta(meta);
                        cx.emit(Changed);
                        true
                    });
                    if !matches!(landed, Ok(true)) {
                        return;
                    }
                    context
                }
                Err(why) => {
                    let _ = this.update(cx, |this, cx| {
                        if this.generation != reading {
                            return;
                        }
                        this.meta = MetaState::Failed(why);
                        this.size = SizeState::Loaded(None);
                        cx.emit(Changed);
                    });
                    return;
                }
            };

            let priced = cx
                .background_spawn(async move {
                    context.size_model().map_err(|report| format!("{report:?}")).inspect_err(|why| {
                        tracing::warn!("armor viewer: the export size could not be modelled: {why}");
                    })
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.generation != reading {
                    return;
                }
                this.size = SizeState::Loaded(priced.ok());
                cx.emit(Changed);
            });
        })
        .detach();
    }

    /// Takes the ship a read turned up, and the draft it implies.
    ///
    /// The level of detail is held to what this hull has, and the scheme named
    /// "default" is ticked where there is one: a ship that has one exports with
    /// it in the egui dialog, and an export set up in either has to bake the
    /// same appearance.
    fn adopt_meta(&mut self, meta: ShipMeta) {
        self.draft.lod = self.draft.lod.min(meta.hull_lod_count.saturating_sub(1));
        self.draft.camos.clear();
        if let Some(scheme) = default_camo(&meta.camo_schemes) {
            self.draft.camos.insert(scheme);
        }
        self.meta = MetaState::Loaded(meta);
    }

    /// Adopts a change to the draft, reading the ship again when the change is
    /// one the read depends on.
    fn edit(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut ExportDraft)) {
        let hull_before = self.draft.hull.clone();
        change(&mut self.draft);
        if self.draft.hull == hull_before {
            cx.emit(Changed);
            return;
        }
        // A different hull is a different model: its levels of detail, its camo
        // schemes and its size are all read from it, and the read holds the
        // draft to what the new hull has.
        self.read_ship(cx);
        cx.emit(Changed);
    }

    /// What the size line has to say, asking for any camo ladders it is missing.
    fn size_line(&mut self, cx: &mut Context<Self>) -> SizeLine {
        let SizeState::Loaded(model) = &self.size else { return SizeLine::Waiting };
        let Some(model) = model else { return SizeLine::Silent };

        let options = draft_to_options(&self.draft);
        match model.priced_estimate(&options) {
            Ok(estimate) => SizeLine::Known(estimate),
            Err(unpriced) => {
                let pending: Vec<CamoSchemeId> = unpriced
                    .iter()
                    .filter(|(_, status)| *status == SchemeLadderStatus::Pending)
                    .map(|(id, _)| *id)
                    .collect();
                let failed: Vec<CamoSchemeId> = unpriced
                    .iter()
                    .filter(|(_, status)| *status == SchemeLadderStatus::Failed)
                    .map(|(id, _)| *id)
                    .collect();
                if !pending.is_empty() {
                    self.price_schemes(pending, cx);
                }
                // A read that failed never asks again on its own, so without
                // this the size line would simply disappear.
                if failed.is_empty() { SizeLine::Waiting } else { SizeLine::Incomplete(failed) }
            }
        }
    }

    /// Reads the ladders of camos the size model was not built with.
    ///
    /// Asked for in one batch: the assets.bin re-parse behind it is shared
    /// across ids, and a read per ticked camo would pay it once each.
    fn price_schemes(&mut self, wanted: Vec<CamoSchemeId>, cx: &mut Context<Self>) {
        let asking: Vec<CamoSchemeId> = wanted.into_iter().filter(|id| !self.pricing.contains(id)).collect();
        if asking.is_empty() {
            return;
        }
        let MetaState::Loaded(meta) = &self.meta else { return };
        let context = Arc::clone(&meta.context);
        self.pricing.extend(asking.iter().copied());

        cx.spawn(async move |this, cx| {
            let ids = asking.clone();
            let read = cx
                .background_spawn(async move {
                    match context.scheme_ladders_batch(&ids) {
                        Ok(batch) => batch.into_iter().map(|(id, ladders)| (id, Some(ladders))).collect(),
                        // Best effort like the rest of the pricing, but a read
                        // that failed is recorded as having failed rather than
                        // merged in as a camo that costs nothing.
                        Err(why) => {
                            tracing::warn!("armor viewer: {} camo scheme(s) could not be priced: {why:?}", ids.len());
                            ids.into_iter().map(|id| (id, None)).collect::<Vec<_>>()
                        }
                    }
                })
                .await;

            let _ = this.update(cx, |this, cx| {
                if let SizeState::Loaded(Some(model)) = &mut this.size {
                    for (id, ladders) in read {
                        match ladders {
                            Some(ladders) => model.insert_scheme_ladders(id, ladders),
                            None => model.mark_scheme_ladders_failed(id),
                        }
                    }
                }
                for id in asking {
                    this.pricing.remove(&id);
                }
                cx.emit(Changed);
            });
        })
        .detach();
    }

    /// Asks for the ladders of camos whose last read failed.
    fn price_again(&mut self, failed: Vec<CamoSchemeId>, cx: &mut Context<Self>) {
        if let SizeState::Loaded(Some(model)) = &mut self.size {
            for id in &failed {
                model.clear_failed_scheme_ladders(*id);
            }
        }
        self.price_schemes(failed, cx);
    }

    /// Puts the camo selection back to the one the ship opens with.
    pub(crate) fn reset_camos(&mut self, cx: &mut Context<Self>) {
        self.edit(cx, |_| {});
        self.draft.camos.clear();
        if let MetaState::Loaded(meta) = &self.meta
            && let Some(scheme) = default_camo(&meta.camo_schemes)
        {
            self.draft.camos.insert(scheme);
        }
        cx.emit(Changed);
    }

    /// The options this draft describes, and what to remember from it.
    pub(crate) fn confirmed(&self) -> (ShipExportOptions, ExportDefaults) {
        (draft_to_options(&self.draft), ExportDefaults::from_draft(&self.draft))
    }

    pub(crate) fn param_index(&self) -> &str {
        &self.param_index
    }

    pub(crate) fn display_name(&self) -> &str {
        &self.display_name
    }

    /// Whether the ship has been read, which is what the choices describe.
    ///
    /// Exporting before it lands would write a model at a level of detail the
    /// hull may not have and with a camo set nobody has seen.
    pub(crate) fn is_ready(&self) -> bool {
        matches!(self.meta, MetaState::Loaded(_))
    }
}

impl EventEmitter<Changed> for ExportDialogState {}

/// The ship as the dialog needs it, read off the background executor.
fn read_ship_meta(
    assets: &wowsunpack::export::ship::ShipAssets,
    param_index: &str,
    hull: Option<String>,
) -> Result<(ShipMeta, Arc<ShipModelContext>), String> {
    use wowsunpack::game_params::types::GameParamProvider;

    let param = assets.metadata().game_param_by_index(param_index);
    let vehicle = param
        .as_ref()
        .and_then(|param| param.vehicle().cloned())
        .ok_or_else(|| t!("ui.armor.export.vehicle_not_found").into_owned())?;
    // No textures and no armor: this read is for the levels of detail and the
    // camo list, and reading either would cost far more than they do.
    let probe =
        ShipExportOptions { hull, textures: false, contents: ExportContents::Mesh, ..ShipExportOptions::default() };
    let context = assets.load_ship_from_vehicle(&vehicle, &probe).map_err(|report| format!("{report:?}"))?;
    let context = Arc::new(context);
    let source = context.camo_texture_source().map_err(|report| format!("{report:?}"))?;
    let meta = ShipMeta {
        hull_lod_count: context.hull_lod_count(),
        camo_schemes: source.scheme_infos(),
        context: Arc::clone(&context),
    };
    Ok((meta, context))
}

/// The hull upgrades this ship offers, in the pane's own vocabulary.
///
/// An empty list genuinely means the ship offers no hull choice, which older
/// game versions do; a ship that could not be resolved at all is reported by the
/// read itself rather than here.
fn hull_upgrades_for(assets: &wowsunpack::export::ship::ShipAssets, param_index: &str) -> Vec<(String, String)> {
    use wowsunpack::game_params::types::GameParamProvider;

    let Some(param) = assets.metadata().game_param_by_index(param_index) else { return Vec::new() };
    let Some(vehicle) = param.vehicle() else { return Vec::new() };
    super::load_ship::build_hull_upgrade_names(vehicle)
}

/// Fills `dialog` with the export choices, reading them off `state`.
pub(crate) fn render(state: &Entity<ExportDialogState>, dialog: Dialog, _window: &mut Window, cx: &mut App) -> Dialog {
    let size = state.update(cx, |state, cx| state.size_line(cx));
    let view = state.read(cx);
    let lod_count = match &view.meta {
        MetaState::Loaded(meta) => meta.hull_lod_count,
        _ => 0,
    };
    let schemes: Vec<CamoSchemeInfo> = match &view.meta {
        MetaState::Loaded(meta) => meta.camo_schemes.clone(),
        _ => Vec::new(),
    };
    let failed_read = match &view.meta {
        MetaState::Failed(why) => Some(why.clone()),
        _ => None,
    };
    let draft_contents = view.draft.contents;
    let draft_hull = view.draft.hull.clone();
    let draft_lod = view.draft.lod;
    let draft_res = view.draft.texture_res;
    let ticked = view.draft.camos.clone();
    let hulls = view.hull_upgrades.clone();
    let title = format!("{} - {}", t!("ui.armor.export_model"), view.display_name);

    let loading = matches!(view.meta, MetaState::Loading);
    // No hull chosen means the first one, which is what the exporter takes and
    // what the egui dialog shows selected.
    let hull_chosen = |key: &str, index: usize| match draft_hull.as_deref() {
        Some(chosen) => chosen == key,
        None => index == 0,
    };
    // Armor is untextured, so an armor-only export has no resolution, no camo
    // and no level of detail to pick.
    let textured = draft_contents.includes_mesh();

    let body = v_flex()
        .id("armor-export-dialog")
        .gap_2()
        .w(px(460.))
        .child(row(t!("ui.armor.export.contents").into_owned(), {
            h_flex().gap_1().children(
                [ExportContents::Mesh, ExportContents::Armor, ExportContents::MeshAndArmor]
                    .into_iter()
                    .enumerate()
                    .map(|(ix, contents)| {
                        let state = state.clone();
                        let chosen = contents == draft_contents;
                        crate::ui::selectable(
                            ("armor-export-contents", ix),
                            chosen,
                            Button::new(("armor-export-contents-button", ix))
                                .label(t!(contents_label_key(contents)).into_owned())
                                .compact()
                                .selected(chosen)
                                .on_click(move |_event, _window, cx: &mut App| {
                                    state.update(cx, |state, cx| state.edit(cx, |draft| draft.contents = contents));
                                }),
                        )
                    }),
            )
        }))
        .when(!hulls.is_empty(), |this| {
            this.child(row(t!("ui.armor.export.hull").into_owned(), {
                h_flex().flex_wrap().gap_1().children(hulls.into_iter().enumerate().map(|(ix, (key, label))| {
                    let state = state.clone();
                    let chosen = hull_chosen(&key, ix);
                    crate::ui::selectable(
                        ("armor-export-hull", ix),
                        chosen,
                        Button::new(("armor-export-hull-button", ix))
                            .label(label)
                            .compact()
                            .selected(chosen)
                            // Off while the ship is being read: a read is per
                            // hull, and switching mid-read spends a whole ship
                            // load on a result that is then thrown away.
                            .disabled(loading)
                            .on_click(move |_event, _window, cx: &mut App| {
                                let key = key.clone();
                                state.update(cx, |state, cx| state.edit(cx, |draft| draft.hull = Some(key)));
                            }),
                    )
                }))
            }))
        })
        .when(lod_count > 1, |this| {
            this.child(row(t!("ui.armor.export.lod").into_owned(), {
                h_flex().flex_wrap().gap_1().children((0..lod_count).map(|lod| {
                    let state = state.clone();
                    let chosen = lod == draft_lod;
                    crate::ui::selectable(
                        ("armor-export-lod", lod),
                        chosen,
                        Button::new(("armor-export-lod-button", lod))
                            .label(lod.to_string())
                            .compact()
                            .selected(chosen)
                            .disabled(!textured)
                            .on_click(move |_event, _window, cx: &mut App| {
                                state.update(cx, |state, cx| state.edit(cx, |draft| draft.lod = lod));
                            }),
                    )
                }))
            }))
            .child(
                div()
                    .pl(px(104.))
                    .text_xs()
                    .text_color(crate::theme::text_dim())
                    .child(t!("ui.armor.export.lod_tooltip").to_string()),
            )
        })
        // Greyed rather than taken away: a dialog that changed height as the
        // contents changed would move the button out from under the pointer.
        .child(row(t!("ui.armor.export.res").into_owned(), {
            h_flex().gap_1().children(TextureResolution::ALL.into_iter().enumerate().map(|(ix, res)| {
                let state = state.clone();
                let chosen = res == draft_res;
                crate::ui::selectable(
                    ("armor-export-res", ix),
                    chosen,
                    Button::new(("armor-export-res-button", ix))
                        .label(t!(res.label_key()).into_owned())
                        .compact()
                        .selected(chosen)
                        .disabled(!textured)
                        .on_click(move |_event, _window, cx: &mut App| {
                            state.update(cx, |state, cx| state.edit(cx, |draft| draft.texture_res = res));
                        }),
                )
            }))
        }))
        .when(!schemes.is_empty(), |this| {
            this.child(row(t!("ui.armor.export.camos").into_owned(), render_camos(state, &schemes, &ticked, textured)))
        })
        .when_some(failed_read, |this, why| {
            this.child(
                div()
                    .text_xs()
                    .text_color(rgb(crate::theme::semantic().error))
                    .child(t!("ui.armor.load_failed", error = why).into_owned()),
            )
        })
        .child(render_size_line(state, size))
        // Where the models come from, which is worth saying beside a button
        // that writes one to disk.
        .child(
            div().text_xs().text_color(crate::theme::text_dim()).child(t!("ui.armor.export_disclaimer").to_string()),
        );

    dialog.title(title).child(body)
}

/// What the export is expected to weigh, or why that is not known yet.
fn render_size_line(state: &Entity<ExportDialogState>, size: SizeLine) -> impl IntoElement + use<> {
    match size {
        SizeLine::Silent => div().into_any_element(),
        SizeLine::Waiting => div()
            .text_xs()
            .text_color(crate::theme::text_dim())
            .child(t!("ui.armor.export.loading_details").into_owned())
            .into_any_element(),
        SizeLine::Known(estimate) => {
            let total = humansize::format_size(estimate.total(), humansize::BINARY);
            let geometry = humansize::format_size(estimate.geometry_bytes, humansize::BINARY);
            let textures = humansize::format_size(estimate.texture_bytes, humansize::BINARY);
            v_flex()
                .gap_0p5()
                .child(div().text_xs().child(t!("ui.armor.export.size_estimate", size = total).into_owned()))
                .child(
                    div().text_xs().text_color(crate::theme::text_dim()).child(
                        t!("ui.armor.export.size_breakdown", geometry = geometry, textures = textures).into_owned(),
                    ),
                )
                .into_any_element()
        }
        SizeLine::Incomplete(failed) => {
            let state = state.clone();
            let count = failed.len();
            h_flex()
                .gap_2()
                .items_center()
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(crate::theme::semantic().warn))
                        .child(t!("ui.armor.export.size_incomplete", count = count).into_owned()),
                )
                .child(
                    Button::new("armor-export-size-retry")
                        .label(t!("ui.buttons.retry").into_owned())
                        .compact()
                        .on_click(move |_event, _window, cx: &mut App| {
                            let failed = failed.clone();
                            state.update(cx, |state, cx| state.price_again(failed, cx));
                        }),
                )
                .into_any_element()
        }
    }
}

/// One labelled row of the dialog.
fn row(label: String, control: impl IntoElement) -> impl IntoElement {
    h_flex()
        .gap_2()
        .items_start()
        .child(div().w(px(96.)).text_xs().flex_shrink_0().child(label))
        .child(div().flex_1().child(control))
}

/// The camo schemes this ship can be exported wearing.
///
/// Grouped and sorted as the viewer's own picker groups them: the ship's own
/// schemes first, then the universal, expendable and legacy ones, each sorted by
/// name. A stock row sits at the top saying what an export with nothing ticked
/// bakes, which is most ships, since few name a scheme "default".
fn render_camos(
    state: &Entity<ExportDialogState>,
    schemes: &[CamoSchemeInfo],
    ticked: &BTreeSet<CamoSchemeId>,
    enabled: bool,
) -> impl IntoElement + use<> {
    let mut column = v_flex().id("armor-export-camos").gap_1().max_h(CAMO_LIST_MAX_HEIGHT).overflow_y_scroll();

    column = column.child(
        h_flex()
            .gap_2()
            .child({
                let state = state.clone();
                Button::new("armor-export-camo-none")
                    .label(t!("ui.armor.export.camo_select_none").into_owned())
                    .compact()
                    .disabled(!enabled)
                    .on_click(move |_event, _window, cx: &mut App| {
                        state.update(cx, |state, cx| state.edit(cx, |draft| draft.camos.clear()));
                    })
            })
            .child({
                let state = state.clone();
                Button::new("armor-export-camo-reset")
                    .label(t!("ui.armor.export.camo_reset").into_owned())
                    .compact()
                    .disabled(!enabled)
                    .on_click(move |_event, _window, cx: &mut App| {
                        state.update(cx, |state, cx| state.reset_camos(cx));
                    })
            }),
    );

    // Not a control: nothing ticked is what it names, and the only way to reach
    // it is to untick everything.
    column = column.child(
        div()
            .text_xs()
            .text_color(crate::theme::text_dim())
            .when(ticked.is_empty(), |this| this.text_color(rgb(crate::theme::semantic().text_strong)))
            .child(t!("ui.armor.camo_none").to_string()),
    );

    let mut index = 0usize;
    for group in camo_groups(schemes) {
        if let Some(heading) = group.heading {
            column = column.child(div().pt_1().text_xs().font_weight(FontWeight::BOLD).child(t!(heading).into_owned()));
        }
        for scheme in group.schemes {
            let state = state.clone();
            let id = scheme.id;
            column = column.child(
                Checkbox::new(("armor-export-camo", index))
                    .label(scheme.display_name.clone())
                    .checked(ticked.contains(&id))
                    .disabled(!enabled)
                    .on_click(move |checked, _window, cx: &mut App| {
                        let checked = *checked;
                        state.update(cx, |state, cx| {
                            state.edit(cx, |draft| {
                                if checked {
                                    draft.camos.insert(id);
                                } else {
                                    draft.camos.remove(&id);
                                }
                            })
                        });
                    }),
            );
            index += 1;
        }
    }
    column
}

/// How far the camo list runs before it scrolls.
const CAMO_LIST_MAX_HEIGHT: Pixels = px(240.);

/// One heading's worth of camo schemes.
struct CamoGroup<'a> {
    /// `None` for the ship's own schemes, which need no heading: they are the
    /// ones the reader came for.
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
