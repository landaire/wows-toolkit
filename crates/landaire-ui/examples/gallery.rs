//! Every scheme's palette, and the surfaces in one of them.
//!
//! `cargo run -p landaire-ui --features test-support --example gallery` opens a window that
//! follows the desktop. With `-- --png <directory>` it writes `gallery-dark.png` and
//! `gallery-light.png` there instead.

use std::path::Path;
use std::path::PathBuf;
use std::process::ExitCode;

use landaire_ui::CardSurfaceProps;
use landaire_ui::CollapseButtonProps;
use landaire_ui::ExpandRailProps;
use landaire_ui::IndentGuidesProps;
use landaire_ui::IndentWidth;
use landaire_ui::OverlaySurfaceProps;
use landaire_ui::PanelSide;
use landaire_ui::Scheme;
use landaire_ui::StripedRowsProps;
use landaire_ui::TABULAR_CLASS;
use landaire_ui::ThemeChoice;
use landaire_ui::ThemeRootProps;
use landaire_ui::Tier;
use landaire_ui::TreeDepth;
use landaire_ui::WorkspaceInsetProps;
use landaire_ui::test_support::DisplayScale;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::WindowSize;
use landaire_ui::test_support::render_png;
use zgui::component;
use zgui::css;
use zgui::reactive::RwSignal;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;
use zgui_ui::BadgeProps;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonVariant;
use zgui_ui::SwitchProps;

const SHEET: &str = css!(
    ".gallery { display: flex; flex-direction: column; gap: 12px; padding: 12px; overflow: hidden;
                font-size: var(--lui-font-size); font-family: var(--lui-font-family); }
     .gallery__heading { font-size: 14px; font-weight: 600; color: var(--lui-color-text-bright); }
     .gallery__section { font-size: 12px; font-weight: 600; color: var(--lui-color-text-dim); }
     .gallery__schemes { display: grid; grid-template-columns: repeat(4, 1fr); gap: 8px; }
     .gallery__scheme { display: flex; flex-direction: column; gap: 6px; padding: 8px; }
     .gallery__sample { display: flex; flex-direction: row; gap: 8px; padding: 6px 8px;
                        border-radius: var(--lui-radius); }
     .gallery__tiers { display: flex; flex-direction: row; gap: 2px; }
     .gallery__tier { width: 18px; height: 18px; border-radius: var(--lui-radius);
                      border: 1px solid rgb(127 127 127 / 0.35); }
     .gallery__surfaces { display: flex; flex-direction: row; gap: 12px; align-items: flex-start; }
     .gallery__column { display: flex; flex-direction: column; gap: 8px; }
     .gallery__padded { padding: 10px; display: flex; flex-direction: column; gap: 6px; }
     .gallery__rows { width: 220px; border: 1px solid var(--lui-color-border); }
     .gallery__row { display: flex; flex-direction: row; justify-content: space-between;
                     padding: 2px 8px; }
     .gallery__tree { width: 200px; border: 1px solid var(--lui-color-border); }
     .gallery__node { display: flex; flex-direction: row; height: 20px; align-items: center; }
     .gallery__node-label { padding-left: 4px; }
     .gallery__dim { color: var(--lui-color-text-dim); }
     .gallery__controls { display: flex; flex-direction: row; gap: 8px; align-items: center; }"
);

fn main() -> ExitCode {
    let mut arguments = std::env::args().skip(1);
    let directory = match (arguments.next().as_deref(), arguments.next()) {
        (None, _) => None,
        (Some("--png"), Some(directory)) => Some(PathBuf::from(directory)),
        _ => {
            eprintln!("usage: gallery [--png <directory>]");
            return ExitCode::FAILURE;
        }
    };

    let outcome = match directory {
        Some(directory) => write_pngs(&directory),
        None => zgui::app()
            .with_title("landaire-ui gallery")
            .with_size(1280.0, 860.0)
            .run(|| view! { Gallery(choice = ThemeChoice::System) })
            .map_err(|error| error.to_string()),
    };

    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn write_pngs(directory: &Path) -> Result<(), String> {
    let options = HeadlessOptions {
        scale: DisplayScale(1.0),
        ..HeadlessOptions::new(WindowSize { width: 1280.0, height: 860.0 })
    };

    for (scheme, name) in [(Scheme::Dark, "gallery-dark.png"), (Scheme::Light, "gallery-light.png")] {
        let path = directory.join(name);
        render_png(options, move || view! { Gallery(choice = ThemeChoice::Fixed(scheme)) }, &path)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        println!("wrote {}", path.display());
    }

    Ok(())
}

#[component]
fn Gallery(choice: ThemeChoice) -> impl IntoView {
    install_stylesheet("landaire-ui-gallery", SHEET);

    let schemes: Vec<AnyView> =
        Scheme::ALL.into_iter().map(|scheme| AnyView::new(view! { SchemeCard(scheme = scheme) })).collect();

    view! {
        ThemeRoot(choice = choice) {
            WorkspaceInset {
                column(class = "gallery") {
                    text(class = "gallery__heading") {{format!("landaire-ui: {}", choice.label())}}
                    text(class = "gallery__section") {"Schemes"}
                    box(class = "gallery__schemes") {{schemes}}
                    text(class = "gallery__section") {"Surfaces"}
                    Surfaces()
                }
            }
        }
    }
}

/// One scheme's tiers in order, and its body text on its panel.
#[component]
fn SchemeCard(scheme: Scheme) -> impl IntoView {
    let palette = scheme.palette();
    let tiers: Vec<AnyView> = Tier::ALL
        .into_iter()
        .map(|tier| {
            let color = palette.tier(tier).to_string();
            AnyView::new(view! { box(class = "gallery__tier", style:background-color = color) })
        })
        .collect();
    let sample = format!("background-color: {}; color: {}", palette.panel, palette.text);
    let accent = format!("color: {}", palette.accent);
    let dim = format!("color: {}", palette.text_dim);

    view! {
        CardSurface(class = "gallery__scheme") {
            text {{scheme.label()}}
            row(class = "gallery__tiers") {{tiers}}
            row(class = "gallery__sample", style = sample) {
                text {"Body 1,234.5"}
                text(style = dim) {"dim"}
                text(style = accent) {"accent"}
            }
        }
    }
}

/// Each surface once, in the theme around it.
#[component]
fn Surfaces() -> impl IntoView {
    let rows: Vec<AnyView> =
        [("Alpha", 1204.5), ("Bravo", 87.25), ("Charlie", 15.0), ("Delta", 3300.75), ("Echo", 9.5)]
            .into_iter()
            .map(|(name, figure)| {
                AnyView::new(view! {
                    row(class = "gallery__row") {
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
                row(class = "gallery__node") {
                    IndentGuides(depth = TreeDepth(depth), indent = IndentWidth(14.0))
                    text(class = "gallery__node-label") {{label}}
                }
            })
        })
        .collect();
    let on = RwSignal::new_local(true);

    view! {
        row(class = "gallery__surfaces") {
            column(class = "gallery__column") {
                CardSurface(class = "gallery__padded") {
                    text {"Card"}
                    text(class = "gallery__dim") {"One step above the panel"}
                }
                OverlaySurface(class = "gallery__padded") {
                    text {"Overlay"}
                    text(class = "gallery__dim") {"A popover the application draws"}
                }
            }
            StripedRows(class = "gallery__rows") {{rows}}
            column(class = "gallery__tree") {{tree}}
            column(class = "gallery__column") {
                row(class = "gallery__controls") {
                    CollapseButton(side = PanelSide::Left, label = "Collapse panel")
                    ExpandRail(side = PanelSide::Right, label = "Expand panel")
                    CollapseButton(side = PanelSide::Bottom, label = "Collapse bottom panel")
                }
                row(class = "gallery__controls") {
                    Button {"Primary"}
                    Button(variant = ButtonVariant::Secondary) {"Secondary"}
                    Button(variant = ButtonVariant::Outline) {"Outline"}
                }
                row(class = "gallery__controls") {
                    Switch(checked = on, a11y:label = "Enabled")
                    Badge {"Badge"}
                }
            }
        }
    }
}
