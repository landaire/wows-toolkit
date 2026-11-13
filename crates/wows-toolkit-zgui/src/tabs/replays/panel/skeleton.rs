//! What the panel draws while its replay is read: the outline of the header and the player table,
//! pulsing, under a card naming the wait.

use landaire_ui::OverlaySurfaceProps;
use zgui::component;
use zgui::css;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;
use zgui_ui::ProgressProps;

const SHEET: &str = "wt-table-skeleton";

// The blocks take the border tone, since the widget tone barely differs from the panel. The pulse
// is one animation over the whole outline rather than one per block.
const CSS: &str = css!(
    ".wt-skeleton {
        position: relative;
        display: flex;
        flex: 1 1 auto;
        min-width: 0;
        min-height: 0;
        overflow: hidden;
    }
    .wt-skeleton__outline {
        display: flex;
        flex-direction: column;
        flex: 1 1 auto;
        min-width: 0;
        animation: wt-skeleton-pulse 2000ms cubic-bezier(0.42, 0, 0.58, 1) infinite;
    }
    [reduced-motion] .wt-skeleton__outline { animation: none; }
    @keyframes wt-skeleton-pulse {
        0% { opacity: 1; }
        50% { opacity: 0.6; }
        100% { opacity: 1; }
    }
    .wt-skeleton__header {
        display: flex;
        flex: none;
        align-items: center;
        gap: 8px;
        padding: 4px 8px;
    }
    .wt-skeleton__context { display: flex; flex: none; padding: 0 8px 4px 8px; }
    .wt-skeleton__row {
        display: flex;
        flex: none;
        align-items: center;
        gap: 12px;
        height: 26px;
        padding: 0 8px;
    }
    .wt-skeleton__columns { border-bottom: 1px solid var(--lui-color-border); }
    .wt-skeleton__name { display: flex; flex: none; align-items: center; gap: 8px; width: 200px; }
    .wt-skeleton__fill { flex: 1 1 auto; }
    .wt-skeleton__block {
        flex: none;
        border-radius: var(--lui-radius);
        background-color: var(--lui-color-border);
    }
    .wt-skeleton__block--faint { opacity: 0.6; }
    .wt-skeleton__overlay {
        position: absolute;
        top: 0;
        right: 0;
        bottom: 0;
        left: 0;
        display: flex;
        align-items: center;
        justify-content: center;
        padding: 16px;
    }
    .wt-skeleton__card {
        display: flex;
        flex-direction: column;
        gap: 12px;
        width: 280px;
        max-width: 100%;
        padding: 20px 24px;
    }
    .wt-skeleton__label { font-size: 14px; text-align: center; }
    [reduced-motion] .wt-skeleton__card .zui-progress__fill { animation: none; transform: translateX(75%); }"
);

/// A block of the outline, in CSS pixels.
#[derive(Clone, Copy)]
struct Block {
    width: f32,
    height: f32,
}

impl Block {
    const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }

    fn view(self, faint: bool) -> AnyView {
        let class = if faint { "wt-skeleton__block wt-skeleton__block--faint" } else { "wt-skeleton__block" };
        let width = format!("{}px", self.width);
        let height = format!("{}px", self.height);
        AnyView::new(view! { box(class = class, style:width = width, style:height = height) })
    }
}

/// The outline of a header over a table of players, labelled `label`, which is also its
/// accessible name.
#[component]
pub fn TableSkeleton(label: String) -> impl IntoView {
    // Two full teams; the panel clips whatever does not fit.
    const ROWS: usize = 24;
    const TEXT_HEIGHT: f32 = 10.0;
    const FIGURE_WIDTHS: [f32; 7] = [44.0, 56.0, 40.0, 64.0, 48.0, 40.0, 56.0];

    install_stylesheet(SHEET, CSS);
    let figures = |faint: bool| -> Vec<AnyView> {
        FIGURE_WIDTHS.into_iter().map(|width| Block::new(width, TEXT_HEIGHT).view(faint)).collect()
    };
    let rows: Vec<AnyView> = (0..ROWS)
        .map(|index| {
            // Names differ in length; equal bars would read as a drawing rather than as rows
            // about to fill.
            let name_width = 96.0 + (index * 37 % 72) as f32;
            AnyView::new(view! {
                row(class = "wt-skeleton__row") {
                    row(class = "wt-skeleton__name") {
                        {Block::new(16.0, 16.0).view(false)}
                        {Block::new(name_width, TEXT_HEIGHT).view(false)}
                    }
                    {figures(false)}
                }
            })
        })
        .collect();

    view! {
        box(class = "wt-skeleton", a11y:label = label.clone(), attr:data-testid = "replay-panel-loading") {
            column(class = "wt-skeleton__outline", a11y:hidden = true) {
                row(class = "wt-skeleton__header") {
                    {Block::new(56.0, 18.0).view(false)}
                    {Block::new(40.0, 18.0).view(false)}
                    {Block::new(160.0, TEXT_HEIGHT).view(false)}
                    box(class = "wt-skeleton__fill") {}
                    {Block::new(72.0, 22.0).view(false)}
                    {Block::new(22.0, 22.0).view(false)}
                }
                row(class = "wt-skeleton__context") {
                    {Block::new(360.0, TEXT_HEIGHT).view(true)}
                }
                row(class = "wt-skeleton__row wt-skeleton__columns") {
                    row(class = "wt-skeleton__name") {
                        {Block::new(64.0, TEXT_HEIGHT).view(true)}
                    }
                    {figures(true)}
                }
                {rows}
            }
            box(class = "wt-skeleton__overlay") {
                OverlaySurface(class = "wt-skeleton__card") {
                    text(class = "wt-skeleton__label") {{label.clone()}}
                    Progress()
                }
            }
        }
    }
}
