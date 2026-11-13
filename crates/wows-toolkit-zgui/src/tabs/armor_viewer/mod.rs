//! The Armor Viewer tab: the ships list beside the viewport dock, with the options rail on the
//! right and the thickness legend floating over the viewports.
//!
//! [`workspace::ArmorWorkspace`] holds the panes and the loads that fill them. The ship data comes
//! from [`library::ArmorLibrary`], which the shell holds so the palette lists the ships from any
//! tab. A ship opened from the palette or from a replay viewport is opened in the active pane once
//! the ship data has loaded.

mod dock;
pub mod library;
pub mod options;
pub mod panels;
pub mod sidebar;
#[cfg(test)]
mod tests;
pub mod viewport;
pub mod workspace;

use std::sync::Arc;

use landaire_ui::PanelSide;
use landaire_ui::PanelWidth;
use landaire_ui::SideEdge;
use landaire_ui::SidePanel;
use landaire_ui::SideSplitProps;
use landaire_ui::WidthRange;
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
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::UpdateValue;
use zgui::view;
use zgui::view::IntoView;
use zgui::view::ShowProps;
use zgui::view::install_stylesheet;

use crate::services::Services;
use crate::shell::Shell;
use crate::shell::palette::TabRequest;
use crate::shell::tabs::AppTab;
use crate::shell::tabs::ModeHint;
use crate::shell::tabs::ModifierKey;
use crate::shell::tabs::PanelToggle;
use crate::shell::tabs::TabChrome;
use crate::tabs::armor_viewer::dock::ArmorDockProps;
use crate::tabs::armor_viewer::library::ModelsState;
use crate::tabs::armor_viewer::library::ViewerFeed;
use crate::tabs::armor_viewer::options::OptionsRailProps;
use crate::tabs::armor_viewer::panels::export::ExportRequest;
use crate::tabs::armor_viewer::panels::legend::ArmorLegendProps;
use crate::tabs::armor_viewer::sidebar::ArmorSidebarProps;
use crate::tabs::armor_viewer::sidebar::ModelsSource;
use crate::tabs::armor_viewer::sidebar::SidebarCatalog;
use crate::tabs::armor_viewer::sidebar::SidebarEvent;
use crate::tabs::armor_viewer::viewport::XRAY_KEY;
use crate::tabs::armor_viewer::workspace::ArmorWorkspace;

const SHEET: &str = "wt-armor-viewer";

const CSS: &str = css!(
    ".wt-armor { position: relative; display: flex; flex-direction: column; flex: 1 1 auto; min-width: 0; min-height: 0; }
    .wt-armor__main { position: relative; display: flex; flex: 1 1 auto; min-width: 0; min-height: 0; overflow: hidden; }"
);

/// The ships list's width, as the replays listing's, and how far it may be dragged.
const SIDEBAR_WIDTH: PanelWidth = PanelWidth(240.0);
const SIDEBAR_RANGE: WidthRange = WidthRange { min: PanelWidth(180.0), max: PanelWidth(480.0) };

/// The rail is fixed: its sections are their own widths, so a drag could only overflow or pad them.
const OPTIONS_WIDTH: PanelWidth = PanelWidth(300.0);
const OPTIONS_RANGE: WidthRange = WidthRange::fixed(OPTIONS_WIDTH);

#[cfg(test)]
thread_local! {
    /// The workspace the tab last made, for a test to drive.
    pub static BUILT: std::cell::Cell<Option<ArmorWorkspace>> = const { std::cell::Cell::new(None) };
}

#[component]
pub fn ArmorViewerTab() -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let shell = Shell::current();
    let services = Services::current();
    let library = shell.armor;
    library.want();
    let workspace = ArmorWorkspace::new(services, library);
    #[cfg(test)]
    BUILT.with(|built| built.set(Some(workspace)));

    adopt_defaults(workspace);
    take_requests(shell, workspace);
    follow_panes(workspace);
    publish_chrome(shell, workspace);
    follow_xray_key(shell, workspace);

    let catalog: Signal<Option<Arc<SidebarCatalog>>, LocalStorage> = {
        let built = Memo::new(move |_| library.ready().map(|bundle| Arc::as_ptr(&bundle) as usize));
        // A memo read with `track` before it was ever computed follows nothing, so it is read.
        Signal::derive_local(move || {
            built.get()?;
            library.ready_untracked().map(|bundle| {
                Arc::new(SidebarCatalog { assets: Arc::clone(&bundle.assets), catalog: Arc::clone(&bundle.catalog) })
            })
        })
    };
    let models = Signal::derive_local(move || match library.models() {
        ModelsState::NotStarted | ModelsState::Loading => ModelsSource::Loading,
        ModelsState::Failed => ModelsSource::Unavailable,
        ModelsState::Ready(models) => ModelsSource::Ready(Arc::clone(&models.library)),
    });
    let common = Signal::derive_local(move || workspace.common());
    let open = Signal::derive_local(move || workspace.open_target());
    let hardpoints = Signal::derive_local(move || workspace.hardpoints.get());
    let on_event = UnsyncCallback::new(move |event: SidebarEvent| on_sidebar_event(shell, workspace, event));

    let sidebar_width = RwSignal::new_local(SIDEBAR_WIDTH);
    let options_width = RwSignal::new_local(OPTIONS_WIDTH);
    let on_legend_change = UnsyncCallback::new(move |()| workspace.save_defaults());
    let legend_shown = move || {
        let active = workspace.active();
        let armor = active.scene_features().map(|features| features.armor);
        workspace.legend.get().visible
            && active.scene_problem().is_none()
            && armor.unwrap_or_else(|| workspace.ship_loaded.get())
    };

    view! {
        column(class = "wt-armor", attr:data-testid = "armor-viewer-tab") {
            SideSplit(
                open = workspace.sidebar_open,
                width = sidebar_width,
                range = SIDEBAR_RANGE,
                resize_label = t!("ui.armor.ships").into_owned(),
                keep_mounted = true,
                attr:data-testid = "armor-viewer-ships"
            ) {
                SidePanel(slot) {
                    ArmorSidebar(
                        catalog = catalog,
                        models = models,
                        common = common,
                        open = open,
                        hardpoints = hardpoints,
                        on_event = on_event
                    )
                }
                SideSplit(
                    open = workspace.options_open,
                    width = options_width,
                    range = OPTIONS_RANGE,
                    edge = SideEdge::Right,
                    resize_label = t!("ui.armor.options").into_owned(),
                    attr:data-testid = "armor-viewer-options"
                ) {
                    SidePanel(slot) {
                        OptionsRail(workspace = workspace)
                    }
                    box(class = "wt-armor__main") {
                        ArmorDock(workspace = workspace)
                    }
                }
            }
            Show(when = legend_shown) {
                ArmorLegend(state = workspace.legend, on_saved_change = Some(on_legend_change))
            }
        }
    }
}

/// Seeds the panes, the legend and what a model opens showing from the saved settings, once they
/// are read.
fn adopt_defaults(workspace: ArmorWorkspace) {
    let settings = workspace.services.settings;
    let adopted = StoredValue::new(false);
    let adopting = RenderEffect::new(move |_| {
        if adopted.get_value() {
            return;
        }
        let Some((defaults, show_armor, xray_highlight)) = settings.read(|settings| {
            (settings.armor_defaults.clone(), settings.model_show_armor, settings.model_xray_highlight)
        }) else {
            return;
        };
        adopted.set_value(true);
        workspace.apply_defaults(defaults.as_ref(), show_armor, xray_highlight);
    });
    on_cleanup_local(move || drop(adopting));
}

/// Opens what the palette and replay viewports ask for, once the ship data has loaded: a ship
/// asked for before that waits rather than being dropped.
fn take_requests(shell: Shell, workspace: ArmorWorkspace) {
    let library = workspace.library;
    let waiting = StoredValue::new_local(Vec::<Waiting>::new());
    let requests = RenderEffect::new(move |_| {
        let mut taken: Vec<Waiting> = shell
            .palette
            .take_requests(AppTab::ArmorViewer)
            .into_iter()
            .map(|request| match request {
                TabRequest::ViewArmor { param_index, display_name } => Waiting::Open(param_index.0, display_name),
                TabRequest::Search(_) | TabRequest::ShowStats(_) => Waiting::Nothing,
            })
            .collect();
        taken.extend(library.take_feed().into_iter().map(Waiting::Feed));
        if !taken.is_empty() {
            waiting.update_value(|waiting| waiting.extend(taken));
        }
        if library.ready().is_none() {
            return;
        }
        let ready = waiting.try_update_value(std::mem::take).unwrap_or_default();
        for asked in ready {
            match asked {
                Waiting::Open(param_index, display_name) => workspace.open_ship(param_index, display_name),
                Waiting::Feed(ViewerFeed::Show { source, shown, seek }) => {
                    workspace.show_with_hits(source, *shown, seek)
                }
                Waiting::Feed(ViewerFeed::Followed { source, followed }) => workspace.follow_hits(source, followed),
                Waiting::Nothing => {}
            }
        }
    });
    on_cleanup_local(move || drop(requests));
}

/// A request held until the ship data has loaded.
enum Waiting {
    Open(String, String),
    Feed(ViewerFeed),
    Nothing,
}

/// Keeps every pane firing the checker's ship, and the checker reading the plate under the pointer.
fn follow_panes(workspace: ArmorWorkspace) {
    let cast = Memo::new(move |_| workspace.cast_key());
    let casting = RenderEffect::new(move |_| {
        let _ = cast.get();
        let _ = workspace.panes();
        workspace.push_cast_ship();
    });
    let plate = RenderEffect::new(move |_| workspace.follow_pointer_plate());
    on_cleanup_local(move || {
        drop(casting);
        drop(plate);
    });
}

/// Puts the ships list and options rail toggles on the status bar, and names what a held key
/// changes: Ctrl makes a zone's checkbox show that zone alone, and Shift makes a click on the hull
/// cast a shell. Neither does anything before a ship is shown.
fn publish_chrome(shell: Shell, workspace: ArmorWorkspace) {
    let Some(chrome) = TabChrome::current() else { return };
    let locale = workspace.services.locale;
    let toggles = RenderEffect::new(move |_| {
        locale.track();
        chrome.set_toggles(vec![
            PanelToggle {
                test_id: "armor-ships-toggle",
                name: t!("ui.armor.ships").into_owned(),
                side: PanelSide::Left,
                open: workspace.sidebar_open,
            },
            PanelToggle {
                test_id: "armor-options-toggle",
                name: t!("ui.armor.options").into_owned(),
                side: PanelSide::Right,
                open: workspace.options_open,
            },
        ]);
    });
    let mode = RenderEffect::new(move |_| {
        let mode = workspace.ship_loaded.get().then(|| {
            if ModifierKey::Primary.is_down(shell.held_modifiers()) {
                ModeHint { key: ModifierKey::Primary, name_key: "ui.armor.mode_solo_zone" }
            } else {
                ModeHint { key: ModifierKey::Shift, name_key: "ui.armor.mode_cast_trajectory" }
            }
        });
        chrome.set_mode(mode);
    });
    on_cleanup_local(move || {
        drop(toggles);
        drop(mode);
        chrome.set_mode(None);
    });
}

/// Keeps every pane's hover highlight with the x-ray mode and its key, so pressing or letting go
/// of the key takes effect without a pointer move.
fn follow_xray_key(shell: Shell, workspace: ArmorWorkspace) {
    let depth = RenderEffect::new(move |_| {
        workspace.set_highlight_depth(XRAY_KEY.is_down(shell.held_modifiers()));
    });
    on_cleanup_local(move || drop(depth));
}

fn on_sidebar_event(shell: Shell, workspace: ArmorWorkspace, event: SidebarEvent) {
    match event {
        SidebarEvent::OpenShip(ship) => workspace.open_ship(ship.param_index.as_str().to_owned(), ship.display_name),
        SidebarEvent::OpenModel { model, from_ship } => workspace.open_model(model, from_ship),
        SidebarEvent::OpenBuilding(building) => workspace.open_building(building),
        SidebarEvent::CompareSplit(ship) => {
            workspace.compare(ship.map(|ship| (ship.param_index.as_str().to_owned(), ship.display_name)));
        }
        SidebarEvent::ExportShip(ship) => {
            let Some(bundle) = workspace.library.ready_untracked() else {
                tracing::warn!("armor viewer: export requested before ship assets finished loading");
                return;
            };
            // The hull the pane is showing, which is the one the reader is looking at.
            let seed_hull = workspace.active_untracked().selected_hull();
            let request = ExportRequest::Ship {
                param_index: ship.param_index.as_str().to_owned(),
                display_name: ship.display_name,
                seed_hull,
            };
            panels::export::open_export_dialog(shell, request, Arc::clone(&bundle.assets));
        }
        SidebarEvent::ExportModel { model, from_ship } => {
            let (Some(bundle), Some(models)) =
                (workspace.library.ready_untracked(), workspace.library.models_ready_untracked())
            else {
                tracing::warn!("model viewer: export requested before the model library finished loading");
                return;
            };
            let request = wows_toolkit_armor::load_model::ModelLoadRequest::new(model, from_ship);
            panels::export::open_export_dialog(
                shell,
                ExportRequest::Model { request, models },
                Arc::clone(&bundle.assets),
            );
        }
        SidebarEvent::Hardpoints { target, visible } => workspace.set_hardpoints_visible(target, visible),
        SidebarEvent::Common(common) => workspace.set_common(common),
    }
}
