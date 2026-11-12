//! The card a hover preview is shown in, beside the hovered row.

use rust_i18n::t;
use zgui::component;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::Memo;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetValue;
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
        display: flex;
        flex-direction: column;
        gap: 4px;
        padding: 4px;
        max-width: 392px;
    }
    .wt-preview-card--instant.zui-surface { animation: none; }
    .wt-preview-map {
        position: relative;
        display: flex;
        align-items: center;
        justify-content: center;
        flex: none;
        width: 384px;
        height: 384px;
        overflow: hidden;
        background-color: #06272e;
        color: rgba(255, 255, 255, 0.9);
    }
    .wt-preview-map__frames {
        position: absolute;
        left: 0;
        top: 0;
        width: 384px;
        height: 384px;
    }
    .wt-preview-map .zui-spinner {
        position: relative;
        width: 32px;
        height: 32px;
        border-width: 3px;
    }
    .wt-preview-map__failure {
        padding: 8px;
        font-size: 12px;
        text-align: center;
        color: rgba(255, 255, 255, 0.7);
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
    children: ChildrenFn,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let shown = Memo::new(move |_| preview.placement());
    let facts = StoredValue::new_local(children);

    move || {
        shown.get().map(|(anchor, replaced)| {
            view! {
                PreviewCard(preview = preview, anchor = anchor, replaced = replaced, placement = placement) {
                    {facts.get_value().view()}
                }
            }
            .into_view()
        })
    }
}

/// One row's card. Built again for each row, since a popper is placed against one anchor.
#[component]
fn PreviewCard(
    preview: HoverPreview,
    anchor: NodeRef,
    replaced: bool,
    placement: Placement,
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

    view! {
        AnchoredSurface(
            state = state,
            placement = placement,
            dismiss_on_outside_press = false,
            class = class,
            attr:data-testid = "hover-preview"
        ) {
            PreviewMap(preview = preview)
            box(class = "wt-preview-facts") {{facts.get_value().view()}}
        }
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
            MapArea::Holding { .. } => Self::Holding,
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
    let spinning = move || matches!(area.get(), MapArea::Holding { baking: true } | MapArea::Frames { waiting: true });
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
                    Show(when = spinning) {
                        Spinner(label = loading.clone())
                    }
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
