//! The Armor Viewer's sidebar: one tree with the ships (nation > class > ship, each ship listing
//! its parts), the armored buildings under Fortifications, a "Models" heading, then every
//! catalogue model (nation > kind > sub-kind > model, each model listing its variants).
//!
//! The rows come from [`wows_toolkit_armor::sidebar_tree`], rebuilt on every search, expansion or
//! data change; only expanded folders are built. Folders are collapsed until opened; a search
//! drops the folders holding no match and opens the rest when it names few enough rows.
//!
//! The sidebar tells its owner what the reader asks for through [`SidebarEvent`]s: a row opens
//! its ship, building or model, a ship row's menu offers a comparison pane and an export, and a
//! model or part row's menu offers to open, export or copy the path of its model.

mod art;
mod header;
mod rows;
mod state;
mod tree_model;

#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

use landaire_ui::BranchClick;
use landaire_ui::EscapeClears;
use landaire_ui::LeafLead;
use landaire_ui::LoadingPlacement;
use landaire_ui::LoadingProps;
use landaire_ui::RowHeight;
use landaire_ui::TreeMenu;
use landaire_ui::TreeProps;
use landaire_ui::TreeRow;
use landaire_ui::TreeTooltip;
use rust_i18n::t;
use wows_toolkit_armor::catalog::ShipCatalog;
use wows_toolkit_armor::model_refs::HardpointTarget;
use wows_toolkit_armor::model_refs::OpenTarget;
use wows_toolkit_armor::model_refs::ShipPartRef;
use wows_toolkit_armor::model_refs::VehicleParamIndex;
use wows_toolkit_armor::sidebar_tree::RowId;
use wows_toolkit_icons::Glyph;
use wows_toolkit_viewmodel::settings::XrayHighlight;
use wowsunpack::export::model_catalog::ModelPath;
use wowsunpack::export::model_library::ModelLibrary;
use wowsunpack::export::ship::ShipAssets;
use wowsunpack::game_types::GameParamId;
use zgui::component;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::With;
use zgui::task::blocking;
use zgui::task::spawn_local;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::EventCx;
use zgui::view::IntoView;
use zgui::view::events;
use zgui::view::install_stylesheet;
use zgui::vocab::Key;
use zgui::vocab::NamedKey;
use zgui_ui::InputProps;
use zgui_ui::prelude::Binding;

use crate::icons::AppIconProps;
use crate::services::Services;
use crate::shell::Shell;
use crate::tabs::armor_viewer::sidebar::art::SidebarArt;
use crate::tabs::armor_viewer::sidebar::header::SidebarHeaderProps;
use crate::tabs::armor_viewer::sidebar::rows::RowSources;
use crate::tabs::armor_viewer::sidebar::rows::row_menu;
use crate::tabs::armor_viewer::sidebar::rows::row_tip;
use crate::tabs::armor_viewer::sidebar::rows::sidebar_row;
use crate::tabs::armor_viewer::sidebar::state::SidebarState;
use crate::tabs::armor_viewer::sidebar::tree_model::SidebarTree;
use crate::tabs::armor_viewer::viewport::OpeningLayer;

const SHEET: &str = "wt-armor-sidebar";

const CSS: &str = css!(
    ".wt-armor-sidebar { display: flex; flex-direction: column; flex: 1 1 auto; min-width: 0; min-height: 0; }
    .wt-armor-sidebar__header, .wt-armor-sidebar__search {
        display: flex;
        flex: none;
        align-items: center;
        gap: 4px;
        padding: 4px 8px;
        border-bottom: 1px solid var(--lui-color-border);
    }
    .wt-armor-sidebar__title { flex: 1 1 auto; font-size: 13px; font-weight: 700; }
    .wt-armor-sidebar__field { flex: 1 1 auto; min-width: 0; }
    .wt-armor-sidebar__list { display: flex; flex-direction: column; flex: 1 1 auto; min-height: 0; }
    .wt-armor-sidebar__list .lui-tree-area, .wt-armor-sidebar__list .lui-tree { flex: 1; min-height: 0; }
    .wt-armor-row-host { display: flex; flex: 1; min-width: 0; height: 100%; }
    .wt-armor-row {
        display: flex;
        flex: 1;
        align-items: center;
        gap: 4px;
        min-width: 0;
        height: 100%;
        padding-right: 16px;
        font-size: 13px;
    }
    .wt-armor-row--heading {
        padding-right: 0;
        border-top: 1px solid var(--lui-color-border);
        font-weight: 700;
    }
    /* The game's flag sits in the middle of a square picture; cover crops the empty band above
       and below it. */
    .wt-armor-row__flag { flex: none; width: 23px; height: 16px; object-fit: cover; }
    .wt-armor-row__class { flex: none; width: 16px; height: 16px; transform: rotate(90deg); }
    .wt-armor-row__label { display: flex; flex: 1 1 auto; align-items: center; gap: 4px; min-width: 0; overflow: hidden; }
    .wt-armor-row__label--missing { color: var(--wt-color-text-dim); }
    .wt-armor-row__name, .wt-armor-row__note, .wt-armor-row__status {
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .wt-armor-row__node { flex: none; white-space: nowrap; color: var(--wt-color-text-dim); }
    .wt-armor-row__note, .wt-armor-row__status { color: var(--wt-color-text-dim); }
    .wt-armor-row__hardpoints { display: flex; flex: none; }
    .wt-armor-row__tip-lines { display: flex; flex-direction: column; }
    .wt-armor-common { display: flex; flex-direction: column; gap: 8px; width: 220px; padding: 8px; }
    .wt-armor-common__check { display: flex; flex-direction: row; align-items: center; gap: 8px; font-size: 13px; }
    .wt-armor-common__tiers { font-size: 13px; }
    .wt-armor-common__xray { display: flex; flex-direction: column; gap: 6px; font-size: 13px; }
    .wt-armor-common__xray-choices { display: flex; flex-direction: column; gap: 6px; padding-left: 4px; }"
);

/// The loaded ship catalog the sidebar lists, with the assets its models and icons are read
/// from.
pub struct SidebarCatalog {
    pub assets: Arc<ShipAssets>,
    pub catalog: Arc<ShipCatalog>,
}

/// Where the model library behind the Models section stands.
#[derive(Clone)]
pub enum ModelsSource {
    Loading,
    /// The library could not be built.
    Unavailable,
    Ready(Arc<ModelLibrary>),
}

/// What the comparison panes share, and whether there is more than one pane to share it
/// between. Held by the tab; the sidebar's menu shows it and asks for changes through
/// [`SidebarEvent::Common`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CommonPaneSettings {
    pub comparing: bool,
    pub mirror_cameras: bool,
    pub sync_options: bool,
    /// Whether a comparison split opens below the active pane rather than beside it.
    pub stack_panes: bool,
    /// What a newly opened model shows first. Not a comparison setting.
    pub opening_layer: OpeningLayer,
    /// When a part hovered in the view shows its highlight through what is in front of it. Not
    /// a comparison setting.
    pub xray_highlight: XrayHighlight,
}

/// A ship the reader picked from the listing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShipChoice {
    pub param_index: VehicleParamIndex,
    pub display_name: String,
}

/// What the reader asked the sidebar for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SidebarEvent {
    OpenShip(ShipChoice),
    /// A catalogue model, or a ship part's model with the part it was opened from.
    OpenModel {
        model: ModelPath,
        from_ship: Option<ShipPartRef>,
    },
    /// A building, under Fortifications or under its hull model.
    OpenBuilding(GameParamId),
    /// A new comparison pane in a split beside the active one: with `Some`, showing that ship;
    /// with `None`, a copy of the pane already showing.
    CompareSplit(Option<ShipChoice>),
    /// A new pane showing the ship, as another tab of the active pane's group.
    OpenInNewTab(ShipChoice),
    /// The ship at stock hull and default LOD, whatever any pane shows.
    ExportShip(ShipChoice),
    ExportModel {
        model: ModelPath,
        from_ship: Option<ShipPartRef>,
    },
    /// A row's hardpoint checkbox was ticked or cleared.
    Hardpoints {
        target: HardpointTarget,
        visible: bool,
    },
    /// The reader changed what the panes share.
    Common(CommonPaneSettings),
}

#[cfg(test)]
thread_local! {
    /// The state of the sidebar built last on this thread, for tests to drive.
    static BUILT: std::cell::Cell<Option<SidebarState>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
fn built_state() -> Option<SidebarState> {
    BUILT.with(std::cell::Cell::get)
}

/// The sidebar.
#[component]
pub fn ArmorSidebar(
    /// The ship catalog, once loaded.
    catalog: Signal<Option<Arc<SidebarCatalog>>, LocalStorage>,
    models: Signal<ModelsSource, LocalStorage>,
    common: Signal<CommonPaneSettings, LocalStorage>,
    /// What the active pane shows, highlighted in the listing.
    open: Signal<Option<OpenTarget>, LocalStorage>,
    /// The targets whose hardpoints the active pane shows.
    hardpoints: Signal<Rc<HashSet<HardpointTarget>>, LocalStorage>,
    on_event: UnsyncCallback<SidebarEvent>,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let services = Services::current();
    let shell = Shell::current();
    let state = SidebarState::new(services, on_event);
    #[cfg(test)]
    BUILT.with(|built| built.set(Some(state)));
    let art = RwSignal::new_local(Rc::new(SidebarArt::default()));

    let following = RenderEffect::new(move |_| {
        let catalog = catalog.get();
        state.set_catalog(catalog.clone());
        let Some(catalog) = catalog else {
            art.set(Rc::new(SidebarArt::default()));
            return;
        };
        spawn_local(async move {
            let read = Arc::clone(&catalog);
            let loaded = blocking(move || SidebarArt::read(read.assets.vfs(), &read.catalog)).await;
            if state.catalog_untracked().is_some_and(|current| Arc::ptr_eq(&current, &catalog)) {
                art.set(Rc::new(loaded));
            }
        });
    });
    let models_following = RenderEffect::new(move |_| state.set_models_source(models.get()));
    let open_following = RenderEffect::new(move |_| state.set_open(open.get()));
    // The listing's labels are translated as it is built.
    let locale_following = RenderEffect::new(move |seen: Option<()>| {
        services.locale.track();
        if seen.is_some() {
            state.rebuild(state::Rebuild::Keep);
        }
    });
    // The saved filters are read once the settings database is open.
    let restoring = RenderEffect::new(move |_| {
        if services.settings.read(|_| ()).is_some() {
            state.restore_settings();
        }
    });
    on_cleanup_local(move || {
        drop(following);
        drop(models_following);
        drop(open_following);
        drop(locale_following);
        drop(restoring);
    });

    let sources = RowSources { state, art, hardpoints, services };
    let escape =
        EscapeClears::with(Signal::derive_local(move || state.search_text.with(|text| !text.is_empty())), move || {
            state.search_text.set(String::new());
            state.search_changed("");
        });
    let on_search_key = move |ev: &mut EventCx<'_, events::KeyDown>| {
        if escape.key_down(ev) {
            return;
        }
        match ev.key {
            Key::Named(NamedKey::Enter) => {
                state.open_from_search();
                ev.prevent_default();
            }
            Key::Named(NamedKey::ArrowDown) if ev.modifiers.is_empty() && state.enter_listing() => {
                ev.prevent_default();
                ev.stop_propagation();
            }
            _ => {}
        }
    };
    let search_label = t!("ui.models.search").into_owned();
    let tree_model: Signal<Rc<SidebarTree>, LocalStorage> = state.tree.into();

    let body = move || match state.catalog() {
        None => AnyView::new(view! {
            Loading(
                label = Signal::derive_local(services.locale.text("ui.armor.loading_catalog")),
                placement = LoadingPlacement::Top,
                attr:data-testid = "armor-sidebar-loading"
            )
        }),
        Some(_) => AnyView::new(view! {
            Tree(
                model = tree_model,
                row = move |row: TreeRow<RowId>| sidebar_row(sources, row),
                row_height = RowHeight(22.0),
                expanded = Binding::controlled(Signal::from(state.expanded), move |next| state.set_expanded(next)),
                selection = state.selection,
                branch_click = BranchClick::SelectAndToggle,
                leaf_lead = LeafLead::Space,
                on_activate = UnsyncCallback::new(move |id: RowId| state.open_row(&id)),
                context_menu = TreeMenu::new(move |id: &RowId| row_menu(state, shell, id)),
                tooltip = TreeTooltip::new(move |id: &RowId| row_tip(state, id)),
                handle = state.handle,
                label = t!("ui.armor.ships").into_owned(),
                attr:data-testid = "armor-sidebar-tree"
            )
        }),
    };

    view! {
        column(class = "wt-armor-sidebar", attr:data-testid = "armor-sidebar") {
            SidebarHeader(state = state, common = common)
            row(class = "wt-armor-sidebar__search") {
                AppIcon(glyph = Glyph::Search)
                Input(
                    value = state.search_text,
                    on_change = UnsyncCallback::new(move |text: String| state.search_changed(&text)),
                    placeholder = search_label.clone(),
                    label = search_label,
                    class = "wt-armor-sidebar__field",
                    attr:data-testid = "armor-sidebar-search",
                    on:key_down = on_search_key
                )
            }
            box(class = "wt-armor-sidebar__list") {{body}}
        }
    }
}
