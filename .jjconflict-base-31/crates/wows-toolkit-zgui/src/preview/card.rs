//! The card a hover preview is shown in, beside the hovered row.

use rust_i18n::t;
use zgui::component;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::Memo;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::With;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::ChildrenFn;
use zgui::view::IntoView;
use zgui::view::NodeRef;
use zgui::view::ShowProps;
use zgui::view::install_stylesheet;
use zgui_ui::AnchoredSurfaceProps;
use zgui_ui::OverlayState;
use zgui_ui::SpinnerProps;
use zgui_ui_primitives::Align;
use zgui_ui_primitives::Placement;
use zgui_ui_primitives::Side;

use crate::frames::FrameViewProps;
use crate::preview::HoverPreview;
use crate::preview::MapArea;

const SHEET: &str = "wt-hover-preview";

// The water colour is Ocean's, the one map that is nothing but water: every pixel of
// `spaces/00_CO_ocean` is this colour. A card that holds open water and then shows the real map
// does not jump the way one that holds an empty box does. The spinner is light in every theme
// because the water is dark in every theme.
const CSS: &str = css!(
    ".wt-preview-card {
        padding: 4px;
    }
    .wt-preview-card__body {
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 4px;
        width: 384px;
    }
    .wt-preview-facts {
        align-self: stretch;
    }
    .wt-preview-card--instant.zui-surface { animation: none; }
    .wt-preview-map {
        position: relative;
        display: flex;
        align-items: center;
        justify-content: center;
        flex: none;
        width: min(384px, calc(var(--zui-popper-available-height, 400px) - 16px - var(--wt-preview-facts-height, 0px)));
        height: min(384px, calc(var(--zui-popper-available-height, 400px) - 16px - var(--wt-preview-facts-height, 0px)));
        overflow: hidden;
        background-color: #06272e;
        color: rgba(255, 255, 255, 0.9);
    }
    .wt-preview-map__frames {
        position: absolute;
        left: 0;
        top: 0;
        width: 100%;
        height: 100%;
    }
    .wt-preview-map .zui-spinner {
        position: relative;
        width: 32px;
        height: 32px;
        border-width: 3px;
    }
    .wt-preview-map__failure {
        flex: 1 1 0;
        min-width: 0;
        padding: 8px;
        font-size: 12px;
        overflow-wrap: anywhere;
        color: var(--wt-color-text-dim);
    }"
);

/// The card for `preview`: beside the watched row, the map where it plays, and `children` under
/// it. The children are the row's facts, read from [`HoverPreview::watched`].
///
/// Up for as long as a row is watched. Escape takes it down and ends the preview.
#[component]
pub fn HoverPreviewCard(
    preview: HoverPreview,
    /// Where the card goes against the row's anchor. A card must not cover the row it is for:
    /// the pointer would leave the row for the card and take the card down.
    #[prop(default = Placement::new(Side::Right, Align::Start))]
    placement: Placement,
    /// For a card placed right of its row, the element whose right edge the card clears, so the
    /// card does not cover that element's scrollbar.
    #[prop(optional)]
    beyond: Option<NodeRef>,
    children: ChildrenFn,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let shown = Memo::new(move |_| preview.placement());
    let facts = StoredValue::new_local(children);

    move || {
        shown.get().map(|(anchor, replaced)| {
            let offset = offset_past(anchor, beyond);
            view! {
                PreviewCard(preview = preview, anchor = anchor, replaced = replaced, placement = placement, offset = offset) {
                    {facts.get_value().view()}
                }
            }
            .into_view()
        })
    }
}

/// How far from its anchor the card goes: the surface's usual gap, or a wider one past the right
/// edge of `beyond`.
fn offset_past(anchor: NodeRef, beyond: Option<NodeRef>) -> f32 {
    const GAP: f32 = 4.0;
    const CLEAR_GAP: f32 = 8.0;

    let clearance = beyond.and_then(|beyond| {
        let row = anchor.window_bounds()?;
        let area = beyond.window_bounds()?;
        let past = area.origin.x.0 + area.size.width.0 - (row.origin.x.0 + row.size.width.0);
        Some(past.max(0.0) / anchor.scale())
    });

    clearance.map_or(GAP, |clearance| clearance + CLEAR_GAP)
}

/// One row's card. Built again for each row, since a popper is placed against one anchor.
#[component]
fn PreviewCard(
    preview: HoverPreview,
    anchor: NodeRef,
    replaced: bool,
    placement: Placement,
    offset: f32,
    children: ChildrenFn,
) -> impl IntoView {
    let state = OverlayState::uncontrolled(
        true,
        Some(UnsyncCallback::new(move |open: bool| {
            if !open {
                preview.leave();
            }
        })),
    )
    .anchored_to(anchor);
    let class = if replaced { "wt-preview-card wt-preview-card--instant" } else { "wt-preview-card" };
    let placement: Signal<Placement, LocalStorage> = Signal::stored_local(placement);
    let facts = StoredValue::new_local(children);
    // The map gives up height so a card with no room either side of its row still fits the
    // window, and the row stays uncovered.
    let facts_box = NodeRef::new();
    let facts_size = facts_box.observe_border_size();
    let facts_height = RwSignal::new_local(None);
    let following = RenderEffect::new(move |_| {
        let Some(size) = facts_size.get() else { return };
        let shown = facts_height.get_untracked();
        let next = settled_facts_height(shown, size.height.0, size.width.0);
        if next != shown {
            facts_height.set(next);
        }
    });
    on_cleanup_local(move || drop(following));
    let facts_height = move || facts_height.get().map(|height: f32| format!("{height}px"));

    view! {
        AnchoredSurface(
            state = state,
            placement = placement,
            offset = offset,
            dismiss_on_outside_press = false,
            class = class,
            attr:data-testid = "hover-preview"
        ) {
            box(class = "wt-preview-card__body", var:--wt-preview-facts-height = facts_height) {
                PreviewMap(preview = preview)
                box(class = "wt-preview-facts", node_ref = facts_box) {{facts.get_value().view()}}
            }
        }
    }
}

/// The facts' height in CSS pixels, from their measured box at any scale or transform: the facts
/// are as wide as the card's body, so the ratio of the two sides gives the height. A change of a
/// pixel or two is the text settling at a new position as the card moves, and is not followed,
/// since the card moving is what following it would cause.
fn settled_facts_height(shown: Option<f32>, measured_height: f32, measured_width: f32) -> Option<f32> {
    const BODY_WIDTH: f32 = 384.0;
    const SETTLED: f32 = 2.0;

    if measured_width <= 0.0 {
        return shown;
    }
    let height = (measured_height / measured_width * BODY_WIDTH).round();

    match shown {
        Some(shown) if (shown - height).abs() <= SETTLED => Some(shown),
        _ => Some(height),
    }
}

/// Which branch of [`MapArea`] is up, so a change within one does not build it again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AreaKind {
    Absent,
    Holding,
    Frames,
    Failed,
}

impl AreaKind {
    fn of(area: &MapArea) -> Self {
        match area {
            MapArea::Absent => Self::Absent,
            MapArea::Holding => Self::Holding,
            MapArea::Frames { .. } => Self::Frames,
            MapArea::Failed(_) => Self::Failed,
        }
    }
}

/// The map's square: frames, open water while they are coming, or why there are none.
#[component]
fn PreviewMap(preview: HoverPreview) -> impl IntoView {
    let area = Memo::new(move |_| preview.map_area());
    let kind = Memo::new(move |_| area.with(AreaKind::of));
    let spinning = move || matches!(area.get(), MapArea::Frames { waiting: true });
    let failure = move || match area.get() {
        MapArea::Failed(reason) => reason,
        _ => String::new(),
    };
    let loading = t!("ui.replay.preview_loading").into_owned();

    move || {
        let loading = loading.clone();
        match kind.get() {
            AreaKind::Absent => None,
            AreaKind::Holding => Some(AnyView::new(view! {
                box(class = "wt-preview-map", attr:data-testid = "hover-preview-water") {
                    Spinner(label = loading.clone())
                }
            })),
            AreaKind::Frames => {
                let sink = preview.sink()?;
                Some(AnyView::new(view! {
                    box(class = "wt-preview-map", attr:data-testid = "hover-preview-map") {
                        FrameView(sink = sink, class = "wt-preview-map__frames")
                        Show(when = spinning) {
                            Spinner(label = loading.clone())
                        }
                    }
                }))
            }
            AreaKind::Failed => Some(AnyView::new(view! {
                box(class = "wt-preview-map", attr:data-testid = "hover-preview-failed", a11y:label = failure) {
                    text(class = "wt-preview-map__failure") {{failure}}
                }
            })),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_facts_height_ignores_scale_and_small_wobble() {
        // At a display scale of 1.15 and mid-entrance, 98.5% of full size.
        let scale = 1.15 * 0.985;
        let measured = settled_facts_height(None, 120.0 * scale, 384.0 * scale);
        assert_eq!(measured, Some(120.0));

        assert_eq!(settled_facts_height(measured, 121.4, 384.0), Some(120.0), "a pixel of wobble is not followed");
        assert_eq!(settled_facts_height(measured, 140.0, 384.0), Some(140.0), "a new row's facts are");
        assert_eq!(settled_facts_height(measured, 10.0, 0.0), measured, "an unmeasured box changes nothing");
    }
}
