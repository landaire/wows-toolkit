//! The gallery's dock section: a split center, a left region and a bottom region.

use landaire_ui::DockProps;
use landaire_ui::DockRegion;
use landaire_ui::DockState;
use landaire_ui::DockTarget;
use landaire_ui::Edge;
use landaire_ui::PanelEntry;
use landaire_ui::PanelFactory;
use landaire_ui::PanelId;
use landaire_ui::PanelKind;
use landaire_ui::PanelRegistry;
use landaire_ui::test_support::WindowSize;
use zgui::css;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::install_stylesheet;

use crate::Section;

pub const SECTION: Section =
    Section { name: "dock", title: "Dock", page: WindowSize { width: 1280.0, height: 340.0 }, view };

const SHEET: &str = css!(
    ".gallery-dock { display: flex; width: 860px; height: 230px; border: 1px solid var(--lui-color-border); }
     .gallery-dock__panel { padding: 4px 12px; color: var(--lui-color-text); }"
);

fn view() -> AnyView {
    install_stylesheet("landaire-ui-gallery-dock", SHEET);

    let dock = sample_dock();

    AnyView::new(view! {
        box(class = "gallery-dock") {
            Dock(state = dock)
        }
    })
}

fn sample_dock() -> DockState {
    let kind = PanelKind::new("text");
    let mut registry = PanelRegistry::new();
    registry.register(
        kind.clone(),
        PanelFactory::new(
            |id: &PanelId| id.as_str().to_owned(),
            |id: &PanelId| {
                let line = format!("{} content", id.as_str());
                AnyView::new(view! { box(class = "gallery-dock__panel") {{line}} })
            },
        ),
    );
    let dock = DockState::new(registry);
    let entry = |id: &str| PanelEntry::new(PanelId::new(id), kind.clone());

    let added = (|| {
        dock.add_panel(DockTarget::Region(DockRegion::Left), PanelEntry { closable: false, ..entry("Files") })?;
        let main = dock
            .add_panel(DockTarget::Region(DockRegion::Center), PanelEntry { closable: false, ..entry("Overview") })?;
        dock.add_panel(DockTarget::Group(main), entry("Table"))?;
        dock.add_panel(DockTarget::Group(main), entry("Notes"))?;
        dock.activate(&PanelId::new("Overview"))?;
        dock.add_panel(DockTarget::Beside { group: main, edge: Edge::Right }, entry("Preview"))?;
        dock.add_panel(DockTarget::Region(DockRegion::Bottom), entry("Log"))
    })();
    added.expect("every sample panel has a registered kind and its own id");

    dock
}
