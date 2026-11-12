//! The gallery's schemes section: every scheme's tiers and body text.

use landaire_ui::CardSurfaceProps;
use landaire_ui::Scheme;
use landaire_ui::Tier;
use landaire_ui::test_support::WindowSize;
use zgui::component;
use zgui::css;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;

use crate::Section;

pub const SECTION: Section =
    Section { name: "schemes", title: "Schemes", page: WindowSize { width: 1280.0, height: 420.0 }, view };

const SHEET: &str = css!(
    ".gallery-schemes { display: grid; grid-template-columns: repeat(4, 1fr); gap: 8px; }
     .gallery-schemes__card { display: flex; flex-direction: column; gap: 6px; padding: 8px; }
     .gallery-schemes__sample { display: flex; flex-direction: row; gap: 8px; padding: 6px 8px;
                                border-radius: var(--lui-radius); }
     .gallery-schemes__tiers { display: flex; flex-direction: row; gap: 2px; }
     .gallery-schemes__tier { width: 18px; height: 18px; border-radius: var(--lui-radius);
                              border: 1px solid rgb(127 127 127 / 0.35); }"
);

/// One scheme's tiers in order, and its body text on its panel.
#[component]
fn SchemeCard(scheme: Scheme) -> impl IntoView {
    let palette = scheme.palette();
    let tiers: Vec<AnyView> = Tier::ALL
        .into_iter()
        .map(|tier| {
            let color = palette.tier(tier).to_string();
            AnyView::new(view! { box(class = "gallery-schemes__tier", style:background-color = color) })
        })
        .collect();
    let sample = format!("background-color: {}; color: {}", palette.panel, palette.text);
    let accent = format!("color: {}", palette.accent);
    let dim = format!("color: {}", palette.text_dim);

    view! {
        CardSurface(class = "gallery-schemes__card") {
            text {{scheme.label()}}
            row(class = "gallery-schemes__tiers") {{tiers}}
            row(class = "gallery-schemes__sample", style = sample) {
                text {"Body 1,234.5"}
                text(style = dim) {"dim"}
                text(style = accent) {"accent"}
            }
        }
    }
}

fn view() -> AnyView {
    install_stylesheet("landaire-ui-gallery-schemes", SHEET);

    let cards: Vec<AnyView> =
        Scheme::ALL.into_iter().map(|scheme| AnyView::new(view! { SchemeCard(scheme = scheme) })).collect();

    AnyView::new(view! { box(class = "gallery-schemes") {{cards}} })
}
