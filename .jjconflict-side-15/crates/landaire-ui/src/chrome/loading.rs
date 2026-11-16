//! What a view draws in place of content it is still waiting for.

use zgui::component;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::Signal;
use zgui::reactive::prelude::Get;
use zgui::view;
use zgui::view::A11yBinding;
use zgui::view::AnyView;
use zgui::view::Attrs;
use zgui::view::Classes;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;
use zgui::vocab::Role;
use zgui_ui::ProgressProps;
use zgui_ui::SkeletonProps;
use zgui_ui::SpinnerProps;

use crate::chrome::status_bar::Fraction;
use crate::chrome::status_bar::RunningBarProps;

const SHEET: &str = "landaire-ui-loading";

const CSS: &str = css!(
    ".lui-loading {
        display: flex;
        flex-direction: column;
        min-width: 0;
        font-family: var(--lui-font-family);
    }
    .lui-loading[data-placement=\"centered\"] {
        flex: 1 1 auto;
        width: 100%;
        height: 100%;
        align-items: center;
        justify-content: center;
        padding: 16px;
    }
    .lui-loading[data-placement=\"top\"] {
        width: 100%;
        gap: 8px;
        padding: 8px;
        font-size: 13px;
        color: var(--lui-color-text-dim);
    }
    .lui-loading__card {
        display: flex;
        flex-direction: column;
        gap: 12px;
        width: 280px;
        max-width: 100%;
        padding: 20px 24px;
        border: 1px solid var(--lui-color-border);
        border-radius: var(--lui-radius);
        background-color: var(--lui-color-card);
        box-shadow: var(--lui-shadow-overlay);
        color: var(--lui-color-text);
    }
    .lui-loading__label {
        font-size: 13px;
        text-align: center;
    }
    .lui-loading__line {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 8px;
        min-width: 0;
    }
    .lui-loading .lui-running-bar, .lui-loading .zui-progress {
        width: 100%;
        height: 8px;
    }
    .lui-loading-line {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 4px;
        min-width: 0;
        font-size: 12px;
        color: var(--lui-color-text-dim);
    }
    .lui-loading-line .zui-spinner {
        width: 12px;
        height: 12px;
        flex: none;
    }
    .lui-loading__line .zui-spinner {
        width: 16px;
        height: 16px;
        flex: none;
    }
    .lui-table-skeleton {
        position: relative;
        display: flex;
        flex-direction: column;
        flex: 1 1 auto;
        width: 100%;
        height: 100%;
        min-height: 0;
        overflow: hidden;
    }
    .lui-table-skeleton__header, .lui-table-skeleton__row {
        display: flex;
        flex-direction: row;
        flex: none;
        align-items: center;
        gap: 12px;
        padding: 0 8px;
    }
    .lui-table-skeleton__header {
        gap: 8px;
        padding: 4px 8px;
    }
    .lui-table-skeleton__context {
        display: flex;
        flex: none;
        padding: 0 8px 4px 8px;
    }
    .lui-table-skeleton__row { height: 26px; }
    .lui-table-skeleton__columns { border-bottom: 1px solid var(--lui-color-border); }
    .lui-table-skeleton__name {
        display: flex;
        flex-direction: row;
        flex: none;
        align-items: center;
        gap: 8px;
        width: 200px;
    }
    .lui-table-skeleton__grow { flex: 1 1 auto; }
    .lui-table-skeleton .zui-skeleton {
        flex: none;
        height: 10px;
        border-radius: var(--lui-radius);
        background-color: var(--lui-color-border);
    }
    .lui-table-skeleton__faint { opacity: 0.6; }
    .lui-table-skeleton__overlay {
        position: absolute;
        top: 0;
        right: 0;
        bottom: 0;
        left: 0;
        display: flex;
        align-items: center;
        justify-content: center;
        padding: 16px;
    }"
);

/// Where a wait sits in the space its content would fill.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LoadingPlacement {
    /// Centred in an open content area, as a card.
    #[default]
    Centered,
    /// At the head of a narrow list, where its first rows would be.
    Top,
}

/// A label standing in for content that is not ready, with a bar showing how much of the work is
/// finished when `done` is given, or sweeping while that is not known. The label is also the
/// accessible name.
#[component]
pub fn Loading(
    #[prop(into)] label: Signal<String, LocalStorage>,
    #[prop(optional)] placement: LoadingPlacement,
    #[prop(into, optional)] done: Option<Signal<Fraction, LocalStorage>>,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);

    let own = status(label);
    // The root names the wait, so the indicators inside it say nothing of their own.
    let bar = move || match done {
        Some(done) => AnyView::new(view! { RunningBar(done = done, label = label.get(), a11y:hidden = true) }),
        None => AnyView::new(view! { Progress(a11y:hidden = true) }),
    };

    match placement {
        LoadingPlacement::Centered => AnyView::new(view! {
            box(class = "lui-loading", attr:data-placement = "centered", {..own}, {..attrs}, class = class) {
                column(class = "lui-loading__card") {
                    text(class = "lui-loading__label") {{move || label.get()}}
                    {bar()}
                }
            }
        }),
        LoadingPlacement::Top => AnyView::new(view! {
            box(class = "lui-loading", attr:data-placement = "top", {..own}, {..attrs}, class = class) {
                row(class = "lui-loading__line") {
                    Spinner(a11y:hidden = true)
                    text {{move || label.get()}}
                }
                {done.map(|done| AnyView::new(view! { RunningBar(done = done, label = label.get(), a11y:hidden = true) }))}
            }
        }),
    }
}

/// A small spinner beside a label, for a wait reported within a line of other content rather than
/// in place of it.
#[component]
pub fn LoadingLine(
    #[prop(into)] label: Signal<String, LocalStorage>,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);

    let own = status(label);

    view! {
        row(class = "lui-loading-line", {..own}, {..attrs}, class = class) {
            Spinner(a11y:hidden = true)
            text {{move || label.get()}}
        }
    }
}

/// A busy status named `label`, for the root of a wait.
fn status(label: Signal<String, LocalStorage>) -> Attrs {
    Attrs::new().a11y_from(A11yBinding::new(Role::Status).label(move || label.get()).busy(true))
}

/// The outline of a header over a table of players, pulsing where the table will be drawn once it
/// is read, under a centred card naming what it waits for.
#[component]
pub fn TableSkeleton(
    #[prop(into)] label: Signal<String, LocalStorage>,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
) -> impl IntoView {
    // Two full teams; the area clips whatever does not fit.
    const ROWS: usize = 24;
    const FIGURE_WIDTHS: [u32; 7] = [44, 56, 40, 64, 48, 40, 56];

    install_stylesheet(SHEET, CSS);

    let block = |width: u32, height: u32| {
        AnyView::new(view! { Skeleton(style:width = format!("{width}px"), style:height = format!("{height}px")) })
    };
    let figures = move || FIGURE_WIDTHS.map(|width| block(width, 10)).into_iter().collect::<Vec<_>>();
    // Names differ in length; equal bars would read as a drawing rather than as rows about to fill.
    let rows = (0..ROWS)
        .map(|index| {
            let name_width = 96 + (index * 37 % 72) as u32;
            AnyView::new(view! {
                row(class = "lui-table-skeleton__row") {
                    row(class = "lui-table-skeleton__name") {
                        {block(16, 16)}
                        {block(name_width, 10)}
                    }
                    {figures()}
                }
            })
        })
        .collect::<Vec<_>>();

    view! {
        box(class = "lui-table-skeleton", {..attrs}, class = class) {
            row(class = "lui-table-skeleton__header") {
                {block(56, 18)}
                {block(40, 18)}
                {block(160, 10)}
                box(class = "lui-table-skeleton__grow") {}
                {block(72, 22)}
                {block(22, 22)}
            }
            row(class = "lui-table-skeleton__context lui-table-skeleton__faint") {
                {block(360, 10)}
            }
            row(class = "lui-table-skeleton__row lui-table-skeleton__columns lui-table-skeleton__faint") {
                row(class = "lui-table-skeleton__name") {
                    {block(64, 10)}
                }
                {figures()}
            }
            {rows}
            box(class = "lui-table-skeleton__overlay") {
                Loading(label = label)
            }
        }
    }
}
