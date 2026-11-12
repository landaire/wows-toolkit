//! Every landaire-ui component, one section per file under `gallery_parts`.
//!
//! `cargo run -p landaire-ui --features test-support --example gallery` opens a window that
//! follows the desktop and shows every section. `-- --section <name>` shows one section. With
//! `-- --png <directory>` it writes `gallery-<name>-dark.png` and `gallery-<name>-light.png` for
//! each section, or for the one `--section` names, instead.

use std::path::Path;
use std::path::PathBuf;
use std::process::ExitCode;

use landaire_ui::Scheme;
use landaire_ui::ThemeChoice;
use landaire_ui::ThemeRootProps;
use landaire_ui::WorkspaceInsetProps;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::WindowSize;
use landaire_ui::test_support::render_png;
use zgui::component;
use zgui::css;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;

#[path = "gallery_parts/chrome.rs"]
mod chrome;
#[path = "gallery_parts/dock.rs"]
mod dock;
#[path = "gallery_parts/grid.rs"]
mod grid;
#[path = "gallery_parts/inputs.rs"]
mod inputs;
#[path = "gallery_parts/schemes.rs"]
mod schemes;
#[path = "gallery_parts/surfaces.rs"]
mod surfaces;
#[path = "gallery_parts/title_bar.rs"]
mod title_bar;
#[path = "gallery_parts/tree.rs"]
mod tree;

/// The sections in the order the window shows them.
const SECTIONS: [Section; 8] = [
    schemes::SECTION,
    surfaces::SECTION,
    chrome::SECTION,
    title_bar::SECTION,
    tree::SECTION,
    grid::SECTION,
    inputs::SECTION,
    dock::SECTION,
];

const WINDOW: WindowSize = WindowSize { width: 1280.0, height: 860.0 };

const SHEET: &str = css!(
    ".gallery { display: flex; flex-direction: column; gap: 16px; padding: 12px; overflow-y: auto;
                font-size: var(--lui-font-size); font-family: var(--lui-font-family); }
     .gallery__heading { flex: none; font-size: 14px; font-weight: 600; color: var(--lui-color-text-bright); }
     .gallery__part { display: flex; flex-direction: column; flex: none; gap: 8px; }
     .gallery__section { font-size: 12px; font-weight: 600; color: var(--lui-color-text-dim); }"
);

/// One part of the gallery: what `--section` calls it, its heading, the page its PNG is drawn
/// on, and what it shows.
#[derive(Clone, Copy)]
struct Section {
    name: &'static str,
    title: &'static str,
    page: WindowSize,
    view: fn() -> AnyView,
}

/// What the command line asked for.
struct Request {
    /// Only this section, rather than all of them.
    section: Option<Section>,
    /// Where to write PNGs, rather than opening a window.
    png: Option<PathBuf>,
}

impl Request {
    fn parse(mut arguments: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut request = Self { section: None, png: None };
        while let Some(flag) = arguments.next() {
            match (flag.as_str(), arguments.next()) {
                ("--png", Some(directory)) => request.png = Some(PathBuf::from(directory)),
                ("--section", Some(name)) => {
                    let Some(section) = SECTIONS.into_iter().find(|section| section.name == name) else {
                        let names: Vec<&str> = SECTIONS.iter().map(|section| section.name).collect();
                        return Err(format!("no section is called {name}; the sections are {}", names.join(", ")));
                    };
                    request.section = Some(section);
                }
                _ => return Err("usage: gallery [--section <name>] [--png <directory>]".to_owned()),
            }
        }

        Ok(request)
    }

    fn sections(&self) -> Vec<Section> {
        match self.section {
            Some(section) => vec![section],
            None => SECTIONS.to_vec(),
        }
    }
}

#[component]
fn Gallery(choice: ThemeChoice, sections: Vec<Section>) -> impl IntoView {
    install_stylesheet("landaire-ui-gallery", SHEET);

    let parts: Vec<AnyView> = sections
        .into_iter()
        .map(|section| {
            let content = (section.view)();
            AnyView::new(view! {
                column(class = "gallery__part") {
                    text(class = "gallery__section") {{section.title}}
                    {content}
                }
            })
        })
        .collect();

    view! {
        ThemeRoot(choice = choice) {
            WorkspaceInset {
                column(class = "gallery") {
                    text(class = "gallery__heading") {{format!("landaire-ui: {}", choice.label())}}
                    {parts}
                }
            }
        }
    }
}

/// Writes each of `sections` in each scheme to `directory`, one page per section.
fn write_pngs(directory: &Path, sections: Vec<Section>) -> Result<(), String> {
    for section in sections {
        let options = HeadlessOptions::new(section.page);
        for (scheme, suffix) in [(Scheme::Dark, "dark"), (Scheme::Light, "light")] {
            let path = directory.join(format!("gallery-{}-{suffix}.png", section.name));
            let view = move || view! { Gallery(choice = ThemeChoice::Fixed(scheme), sections = vec![section]) };
            render_png(options, view, &path).map_err(|error| format!("{}: {error}", path.display()))?;
            println!("wrote {}", path.display());
        }
    }

    Ok(())
}

fn main() -> ExitCode {
    let request = match Request::parse(std::env::args().skip(1)) {
        Ok(request) => request,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::FAILURE;
        }
    };

    let sections = request.sections();
    let outcome = match request.png {
        Some(directory) => write_pngs(&directory, sections),
        None => zgui::app()
            .with_title("landaire-ui gallery")
            .with_size(WINDOW.width, WINDOW.height)
            .run(move || view! { Gallery(choice = ThemeChoice::System, sections = sections.clone()) })
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use landaire_ui::test_support::HeadlessWindow;

    use super::*;

    #[test]
    fn every_section_is_drawn_whole_on_its_page() {
        for section in SECTIONS {
            for scheme in [Scheme::Dark, Scheme::Light] {
                let view = move || view! { Gallery(choice = ThemeChoice::Fixed(scheme), sections = vec![section]) };
                let mut window = HeadlessWindow::open(HeadlessOptions::new(section.page), view)
                    .unwrap_or_else(|error| panic!("the {} section opens: {error}", section.name));
                window.advance(Duration::from_millis(500));

                let part = window.by_class("gallery__part").expect("the section is drawn");
                let bounds = window.bounds(part).expect("the section has a box");
                let bottom = bounds.origin.y.0 + bounds.size.height.0;
                assert!(bounds.size.height.0 > 40.0, "the {} section shows its content", section.name);
                assert!(
                    bottom <= section.page.height,
                    "the {} section ends at {bottom}, inside its {} page",
                    section.name,
                    section.page.height
                );
            }
        }
    }

    #[test]
    fn a_section_is_chosen_by_name() {
        let arguments = ["--section", "grid"].map(str::to_owned);
        let request = Request::parse(arguments.into_iter()).expect("grid is a section");

        let names: Vec<&str> = request.sections().iter().map(|section| section.name).collect();
        assert_eq!(names, ["grid"]);
        assert!(Request::parse(["--section", "nothing"].map(str::to_owned).into_iter()).is_err());
    }
}
