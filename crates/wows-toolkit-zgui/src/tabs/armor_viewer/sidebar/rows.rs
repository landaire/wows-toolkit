//! One sidebar row drawn, and the menu a secondary click on it opens.

use std::collections::HashSet;
use std::rc::Rc;

use landaire_ui::MarkedTextProps;
use landaire_ui::MenuItems;
use landaire_ui::TooltipContentProps;
use landaire_ui::TooltipProps;
use landaire_ui::TooltipTriggerProps;
use landaire_ui::TreeRow;
use rust_i18n::t;
use wows_toolkit_armor::model_refs::HardpointTarget;
use wows_toolkit_armor::model_refs::ShipPartRef;
use wows_toolkit_armor::model_refs::VehicleParamIndex;
use wows_toolkit_armor::model_tree::SkinUse;
use wows_toolkit_armor::sidebar_tree::RowId;
use wows_toolkit_armor::sidebar_tree::RowKind;
use wows_toolkit_armor::sidebar_tree::RowNote;
use wows_toolkit_armor::sidebar_tree::row_tooltip;
use wowsunpack::export::model_catalog::ModelPath;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::EventCx;
use zgui::view::IntoView;
use zgui::view::events;
use zgui::vocab::PropValue;
use zgui_ui::CheckboxProps;
use zgui_ui::Checked;
use zgui_ui::MenuItemProps;
use zgui_ui::prelude::Binding;

use crate::services::Services;
use crate::shell::Shell;
use crate::tabs::armor_viewer::sidebar::ShipChoice;
use crate::tabs::armor_viewer::sidebar::SidebarEvent;
use crate::tabs::armor_viewer::sidebar::art::SidebarArt;
use crate::tabs::armor_viewer::sidebar::state::ModelsState;
use crate::tabs::armor_viewer::sidebar::state::SidebarState;
use crate::tabs::armor_viewer::sidebar::tree_model::SidebarTree;
use crate::text_marks::Marks;
use crate::text_marks::marks;
use crate::text_marks::marks_within;

/// What every row reads besides the listing.
#[derive(Clone, Copy)]
pub struct RowSources {
    pub state: SidebarState,
    pub art: RwSignal<Rc<SidebarArt>, LocalStorage>,
    pub hardpoints: Signal<Rc<HashSet<HardpointTarget>>, LocalStorage>,
    pub services: Services,
}

/// The test id of row `id`.
pub fn row_test_id(id: &RowId) -> String {
    format!("armor-sidebar-row-{id}")
}

/// One row. Read through the listing, since the tree keeps a row by its position while its
/// label, count or note changes.
pub fn sidebar_row(sources: RowSources, row: TreeRow<RowId>) -> impl IntoView {
    let key = row.key;
    let test_id = row_test_id(&key);
    let content = move || {
        sources.services.locale.track();
        let tree = sources.state.tree.get();
        row_content(sources, &tree, &key)
    };

    view! {
        box(class = "wt-armor-row-host", attr:data-testid = test_id) {{content}}
    }
}

fn row_content(sources: RowSources, tree: &SidebarTree, id: &RowId) -> AnyView {
    let Some(kind) = tree.kind(id).cloned() else {
        return AnyView::new(());
    };
    let label = tree.label(id).unwrap_or_default().to_string();

    if kind == RowKind::Separator {
        return AnyView::new(view! { row(class = "wt-armor-row wt-armor-row--heading") { text {{label}} } });
    }

    let state = sources.state;
    let missing = tree.missing(id);
    let note = tree.note(id).cloned();

    let icon = row_icon(sources, &kind);
    let name = row_name(sources, &kind, label);
    let note = note.map(|note| {
        let ranges = match &note {
            RowNote::Matched(text) => state.query.with(|query| marks(query, text)),
            RowNote::Tag(_) => Marks::default(),
        };
        AnyView::new(
            view! { MarkedText(class = "wt-armor-row__note", text = note.text().to_string(), ranges = ranges) },
        )
    });
    let class =
        if missing.is_some() { "wt-armor-row__label wt-armor-row__label--missing" } else { "wt-armor-row__label" };
    let label = AnyView::new(view! {
        row(class = class) {
            {name}
            {note}
        }
    });
    let checkbox = kind.hardpoint_target().map(|target| hardpoint_checkbox(sources, id, target));

    let opened = id.clone();
    let on_click = move |_: &mut EventCx<'_, events::Click>| state.open_target(&opened);
    // The tree opens a row's target, or a folder, on a double click; a click already did.
    let on_double_click = move |ev: &mut EventCx<'_, events::DoubleClick>| ev.stop_propagation();

    AnyView::new(view! {
        row(class = "wt-armor-row", on:click = on_click, on:double_click = on_double_click) {
            {icon}
            {label}
            {checkbox}
        }
    })
}

/// A nation row's flag or a class row's icon.
fn row_icon(sources: RowSources, kind: &RowKind) -> Option<AnyView> {
    let flag = match kind {
        RowKind::Nation(nation) => Some(nation.as_str()),
        RowKind::ModelNation { flag } => flag.as_deref(),
        _ => None,
    };
    if let Some(src) = flag.and_then(|nation| sources.art.with(|art| art.flag(nation).map(|bytes| bytes.url()))) {
        return Some(AnyView::new(view! { image(class = "wt-armor-row__flag", src = src, a11y:hidden = true) }));
    }

    let RowKind::Class(species) = kind else {
        return None;
    };
    let svg = sources.art.with(|art| art.class_icon(*species))?;

    Some(AnyView::new(view! {
        vector(class = "wt-armor-row__class", prop:svg = PropValue::from(svg.to_string()), a11y:hidden = true)
    }))
}

/// The row's name, marked where the search matched it.
fn row_name(sources: RowSources, kind: &RowKind, label: String) -> AnyView {
    let state = sources.state;
    match kind {
        RowKind::ShipPart { node: Some(node), model, .. } => {
            let stem = model.stem().to_string();
            let ranges = state.query.with(|query| marks(query, &stem));
            AnyView::new(view! {
                text(class = "wt-armor-row__node") {{node.clone()}}
                MarkedText(class = "wt-armor-row__name", text = stem, ranges = ranges)
            })
        }
        RowKind::ModelsStatus => AnyView::new(view! { text(class = "wt-armor-row__status") {{label}} }),
        // The tier is not one of the texts a search finds a ship by.
        RowKind::Ship { display_name, .. } => {
            let ranges = state.query.with(|query| marks_within(query, &label, display_name));
            AnyView::new(view! { MarkedText(class = "wt-armor-row__name", text = label, ranges = ranges) })
        }
        RowKind::Model(_) | RowKind::ShipPart { node: None, .. } | RowKind::Building { .. } => {
            let ranges = state.query.with(|query| marks(query, &label));
            AnyView::new(view! { MarkedText(class = "wt-armor-row__name", text = label, ranges = ranges) })
        }
        _ => AnyView::new(view! { text(class = "wt-armor-row__name") {{label}} }),
    }
}

/// The tooltip of row `id`: its whole name and what it opens. `None` for a row that opens
/// nothing.
pub fn row_tip(state: SidebarState, id: &RowId) -> Option<AnyView> {
    let (kind, missing) = state.tree.with_untracked(|tree| Some((tree.kind(id)?.clone(), tree.missing(id))))?;
    if !kind.opens_on_its_own() {
        return None;
    }

    let skin_uses: Vec<SkinUse> = match &kind {
        RowKind::Model(model) => state.models.with_untracked(|models| match models {
            ModelsState::Ready(index) => index.skins.get(model).map(|uses| uses.to_vec()).unwrap_or_default(),
            ModelsState::Loading | ModelsState::Unavailable => Vec::new(),
        }),
        _ => Vec::new(),
    };
    let text = row_tooltip(&kind, &skin_uses, missing)?;
    let lines = text.lines().map(|line| view! { text {{line.to_owned()}} }).collect::<Vec<_>>();

    Some(AnyView::new(view! { column(class = "wt-armor-row__tip-lines") {{lines}} }))
}

/// The checkbox showing a model's hardpoints in the pane. A press on it neither opens nor
/// selects the row.
fn hardpoint_checkbox(sources: RowSources, id: &RowId, target: HardpointTarget) -> AnyView {
    let state = sources.state;
    let hardpoints = sources.hardpoints;
    let shown = {
        let target = target.clone();
        Signal::derive_local(move || Checked::from(hardpoints.with(|shown| shown.contains(&target))))
    };
    let toggle = move |checked: Checked| {
        state.send(SidebarEvent::Hardpoints { target: target.clone(), visible: checked == Checked::Yes });
    };
    let label = t!("ui.models.hardpoints_tooltip").into_owned();
    let test_id = format!("armor-sidebar-hardpoints-{id}");
    let stop_click = |ev: &mut EventCx<'_, events::Click>| ev.stop_propagation();
    let stop_double = |ev: &mut EventCx<'_, events::DoubleClick>| ev.stop_propagation();

    AnyView::new(view! {
        Tooltip {
            TooltipTrigger(class = "wt-armor-row__hardpoints") {
                box(on:click = stop_click, on:double_click = stop_double) {
                    Checkbox(
                        checked = Binding::controlled(shown, toggle),
                        a11y:label = label,
                        attr:data-testid = test_id
                    )
                }
            }
            TooltipContent {{sources.services.locale.text("ui.models.hardpoints_tooltip")}}
        }
    })
}

/// The menu for row `id`: a ship row offers a comparison pane, a new tab and an export; a model or
/// part row offers to open it, export it and copy its path. Folders, headings and buildings have
/// none.
pub fn row_menu(state: SidebarState, shell: Shell, id: &RowId) -> Option<MenuItems> {
    let kind = state.tree.with_untracked(|tree| tree.kind(id).cloned())?;

    if let RowKind::Ship { param_index, display_name } = &kind {
        let ship =
            ShipChoice { param_index: VehicleParamIndex::new(param_index.clone()), display_name: display_name.clone() };
        return Some(MenuItems::new(move || ship_menu(state, ship.clone())));
    }

    let (model, from_ship) = kind.model_selection()?;

    Some(MenuItems::new(move || model_menu(state, shell, model.clone(), from_ship.clone())))
}

fn ship_menu(state: SidebarState, ship: ShipChoice) -> impl IntoView {
    let compared = ship.clone();
    let tabbed = ship.clone();

    view! {
        MenuItem(
            attr:data-testid = "armor-sidebar-menu-compare",
            on_select = UnsyncCallback::new(move |()| state.send(SidebarEvent::CompareSplit(Some(compared.clone()))))
        ) {
            {t!("ui.armor.compare_split").into_owned()}
        }
        MenuItem(
            attr:data-testid = "armor-sidebar-menu-new-tab",
            on_select = UnsyncCallback::new(move |()| state.send(SidebarEvent::OpenInNewTab(tabbed.clone())))
        ) {
            {t!("ui.armor.open_in_new_tab").into_owned()}
        }
        MenuItem(
            attr:data-testid = "armor-sidebar-menu-export-ship",
            on_select = UnsyncCallback::new(move |()| state.send(SidebarEvent::ExportShip(ship.clone())))
        ) {
            {t!("ui.armor.export_model").into_owned()}
        }
    }
}

fn model_menu(state: SidebarState, shell: Shell, model: ModelPath, from_ship: Option<ShipPartRef>) -> impl IntoView {
    let (opened, opened_from) = (model.clone(), from_ship.clone());
    let path = model.as_str().to_string();
    let copy = move |()| {
        shell.copy_text(path.clone());
        shell.toasts().ok(t!("ui.models.path_copied", path = path).into_owned());
    };

    view! {
        MenuItem(
            attr:data-testid = "armor-sidebar-menu-open",
            on_select = UnsyncCallback::new(move |()| {
                state.send(SidebarEvent::OpenModel { model: opened.clone(), from_ship: opened_from.clone() });
            })
        ) {
            {t!("ui.models.open").into_owned()}
        }
        MenuItem(
            attr:data-testid = "armor-sidebar-menu-export-model",
            on_select = UnsyncCallback::new(move |()| {
                state.send(SidebarEvent::ExportModel { model: model.clone(), from_ship: from_ship.clone() });
            })
        ) {
            {t!("ui.models.export").into_owned()}
        }
        MenuItem(attr:data-testid = "armor-sidebar-menu-copy-path", on_select = UnsyncCallback::new(copy)) {
            {t!("ui.models.copy_path").into_owned()}
        }
    }
}
