//! The gallery's surfaces section: each surface once, in the theme around it.

use landaire_ui::CardSurfaceProps;
use landaire_ui::CollapseButtonProps;
use landaire_ui::ExpandRailProps;
use landaire_ui::IndentGuidesProps;
use landaire_ui::IndentWidth;
use landaire_ui::OverlaySurfaceProps;
use landaire_ui::PanelSide;
use landaire_ui::StripedRowsProps;
use landaire_ui::TABULAR_CLASS;
use landaire_ui::TreeDepth;
use landaire_ui::test_support::WindowSize;
use zgui::css;
use zgui::reactive::RwSignal;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::install_stylesheet;
use zgui_ui::BadgeProps;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonVariant;
use zgui_ui::SwitchProps;

use crate::Section;

pub const SECTION: Section =
    Section { name: "surfaces", title: "Surfaces", page: WindowSize { width: 1280.0, height: 260.0 }, view };

const SHEET: &str = css!(
    ".gallery-surfaces { display: flex; flex-direction: row; gap: 12px; align-items: flex-start; }
     .gallery-surfaces__column { display: flex; flex-direction: column; gap: 8px; }
     .gallery-surfaces__padded { padding: 10px; display: flex; flex-direction: column; gap: 6px; }
     .gallery-surfaces__rows { width: 220px; border: 1px solid var(--lui-color-border); }
     .gallery-surfaces__row { display: flex; flex-direction: row; justify-content: space-between;
                              padding: 2px 8px; }
     .gallery-surfaces__tree { width: 200px; border: 1px solid var(--lui-color-border); }
     .gallery-surfaces__node { display: flex; flex-direction: row; height: 20px; align-items: center; }
     .gallery-surfaces__node-label { padding-left: 4px; }
     .gallery-surfaces__dim { color: var(--lui-color-text-dim); }
     .gallery-surfaces__controls { display: flex; flex-direction: row; gap: 8px; align-items: center; }"
);

fn view() -> AnyView {
    install_stylesheet("landaire-ui-gallery-surfaces", SHEET);

    let rows: Vec<AnyView> =
        [("Alpha", 1204.5), ("Bravo", 87.25), ("Charlie", 15.0), ("Delta", 3300.75), ("Echo", 9.5)]
            .into_iter()
            .map(|(name, figure)| {
                AnyView::new(view! {
                    row(class = "gallery-surfaces__row") {
                        text {{name}}
                        text(class = TABULAR_CLASS) {{format!("{figure:.2}")}}
                    }
                })
            })
            .collect();
    let tree: Vec<AnyView> = [(0, "Root"), (1, "Group"), (2, "Item"), (2, "Item"), (1, "Group"), (2, "Item")]
        .into_iter()
        .map(|(depth, label)| {
            AnyView::new(view! {
                row(class = "gallery-surfaces__node") {
                    IndentGuides(depth = TreeDepth(depth), indent = IndentWidth(14.0))
                    text(class = "gallery-surfaces__node-label") {{label}}
                }
            })
        })
        .collect();
    let on = RwSignal::new_local(true);

    AnyView::new(view! {
        row(class = "gallery-surfaces") {
            column(class = "gallery-surfaces__column") {
                CardSurface(class = "gallery-surfaces__padded") {
                    text {"Card"}
                    text(class = "gallery-surfaces__dim") {"One step above the panel"}
                }
                OverlaySurface(class = "gallery-surfaces__padded") {
                    text {"Overlay"}
                    text(class = "gallery-surfaces__dim") {"A popover the application draws"}
                }
            }
            StripedRows(class = "gallery-surfaces__rows") {{rows}}
            column(class = "gallery-surfaces__tree") {{tree}}
            column(class = "gallery-surfaces__column") {
                row(class = "gallery-surfaces__controls") {
                    CollapseButton(side = PanelSide::Left, label = "Collapse panel")
                    ExpandRail(side = PanelSide::Right, label = "Expand panel")
                    CollapseButton(side = PanelSide::Bottom, label = "Collapse bottom panel")
                }
                row(class = "gallery-surfaces__controls") {
                    Button {"Primary"}
                    Button(variant = ButtonVariant::Secondary) {"Secondary"}
                    Button(variant = ButtonVariant::Outline) {"Outline"}
                }
                row(class = "gallery-surfaces__controls") {
                    Switch(checked = on, a11y:label = "Enabled")
                    Badge {"Badge"}
                }
            }
        }
    })
}
