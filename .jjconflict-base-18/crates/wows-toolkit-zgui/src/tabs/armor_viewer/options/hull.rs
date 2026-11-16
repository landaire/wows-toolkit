//! The hull section: the hull part tree with its own filter field, the opaque switch, the hull
//! upgrade, module alternative and LOD choices, and the camo picker.
//!
//! Each choice reloads the scene in the viewport, apart from the camo, which is applied in place.
//! Hovering a group or mesh row highlights its meshes in the viewport, shown or not.

use std::collections::HashMap;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

use rust_i18n::t;
use wows_toolkit_armor::load_ship::LoadedShipArmor;
use wows_toolkit_armor::visibility::SidebarHighlightKey;
use wows_toolkit_armor::visibility::TriState;
use wows_toolkit_armor::visibility::hull_group_all_on;
use wows_toolkit_armor::visibility::hull_group_any_on;
use wowsunpack::export::camo_textures::CamoSchemeId;
use wowsunpack::export::camo_textures::CamoSchemeInfo;
use wowsunpack::export::gltf_export::CamoOrigin;
use wowsunpack::game_params::keys::ComponentType;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::Signal;
use zgui::reactive::prelude::Get;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;

use crate::tabs::armor_viewer::options::SectionCx;
use crate::tabs::armor_viewer::options::filter::MarkedLabel;
use crate::tabs::armor_viewer::options::filter::RailFilter;
use crate::tabs::armor_viewer::options::tools::choice_button;
use crate::tabs::armor_viewer::options::tree_filter::RowFilter;
use crate::tabs::armor_viewer::options::tree_filter::TreeFilter;
use crate::tabs::armor_viewer::options::tree_filter::filtered_checkbox;
use crate::tabs::armor_viewer::options::tree_filter::match_count;
use crate::tabs::armor_viewer::options::visibility::chevron;
use crate::tabs::armor_viewer::options::visibility::hoverable_row;
use crate::tabs::armor_viewer::options::visibility::no_ship_loaded;
use crate::tabs::armor_viewer::options::visibility::row_label;
use crate::tabs::armor_viewer::options::visibility::tree_field;
use crate::tabs::armor_viewer::options::visibility::tri_checkbox;
use crate::tabs::armor_viewer::viewport::ArmorViewportState;
use crate::tabs::armor_viewer::viewport::ViewportModel;

const SHEET: &str = "wt-armor-hull";

const CSS: &str = css!(
    ".wt-armor-hull { display: flex; flex-direction: column; gap: 8px; width: 100%; }
    .wt-armor-hull[data-filtering] { gap: 4px; }
    .wt-armor-hull__stack { display: flex; flex-direction: column; gap: 4px; }
    .wt-armor-hull__header { flex: none; display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
    .wt-armor-hull__choices { display: flex; align-items: center; gap: 4px; flex-wrap: wrap; }
    .wt-armor-hull__heading { flex: none; font-size: 13px; }
    .wt-armor-hull__title { font-size: 13px; font-weight: 700; }
    .wt-armor-hull__rule { height: 1px; background-color: var(--lui-color-border); }
    .wt-armor-hull__row { display: flex; align-items: center; gap: 4px; min-width: 0; }
    .wt-armor-hull__children { display: flex; flex-direction: column; gap: 4px; padding-left: 20px; }
    .wt-armor-hull__camo-scroll {
        display: flex;
        flex-direction: column;
        gap: 4px;
        max-height: 240px;
        overflow-y: auto;
        padding-left: 20px;
    }
    .wt-armor-hull__camo { width: 100%; justify-content: flex-start; font-size: 13px; }
    .wt-armor-hull__camo[data-selected] {
        background-color: var(--lui-color-selection);
        color: var(--lui-color-text-bright);
    }"
);

/// The hull section's body: `None` when the rail's filter keeps nothing in it.
pub fn body(cx: &SectionCx) -> Option<AnyView> {
    install_stylesheet(SHEET, CSS);
    let viewport = cx.viewport;
    let filter = cx.filter.clone();

    let Some(armor) = viewport.shown_untracked() else {
        return (!filter.is_active()).then(no_ship_loaded);
    };

    let groups: Rc<Vec<(String, Vec<String>)>> = Rc::new(armor.hull_part_groups.clone());
    let part_filter = cx.workspace.part_filter;
    let counted = Rc::clone(&groups);
    let field_matches = Signal::derive_local(move || {
        let query = RailFilter::new(&part_filter.get());
        query.is_active().then(|| match_count(&query, &counted))
    });
    let field = tree_field("armor-part-filter", part_filter, t!("ui.armor.filter_parts").into_owned(), field_matches);
    let tree_rows = {
        let filter = filter.clone();
        let groups = Rc::clone(&groups);
        move || {
            let tree_filter = TreeFilter::new(filter.clone(), RailFilter::new(&part_filter.get()));
            let Some(state) = viewport.read(HullTreeState::of) else {
                return Vec::new();
            };
            groups
                .iter()
                .filter_map(|(group, names)| hull_group_row(viewport, &state, group, names, &tree_filter))
                .collect::<Vec<AnyView>>()
        }
    };
    let tree = view! {
        column(class = "wt-armor-hull__stack", attr:data-testid = "armor-hull-visibility-tree") {{tree_rows}}
    };

    let upgrade = (armor.hull_upgrade_names.len() > 1).then(|| hull_upgrade_row(viewport, &armor, &filter)).flatten();
    let modules: Vec<AnyView> = armor
        .module_alternatives
        .iter()
        .filter(|(_, alternatives)| alternatives.len() > 1)
        .filter_map(|(ct, alternatives)| module_alternative_row(viewport, *ct, alternatives, &filter))
        .collect();
    let camo = (!armor.camo_schemes().is_empty()).then(|| camo_picker(viewport, &armor, &filter)).flatten();
    let lod = (armor.hull_lod_count > 1).then(|| lod_row(viewport, &armor, &filter)).flatten();
    let opaque = filtered_checkbox(
        "armor-hull-vis-opaque".to_owned(),
        t!("ui.armor.opaque").into_owned(),
        Signal::derive_local(move || viewport.hull_opaque()),
        false,
        &filter,
        move |checked| viewport.set_hull_opaque(checked),
    );

    // While filtering every row that matches by name is drawn, without the bulk switches and
    // dividers. Only the rail's query drops the tree; its own field narrows the rows in it.
    if filter.is_active() {
        let tree = (match_count(&filter, &groups) > 0).then(|| {
            AnyView::new(view! {
                column(class = "wt-armor-hull__stack") {
                    {field}
                    {tree}
                }
            })
        });
        let rows: Vec<AnyView> =
            opaque.into_iter().chain(upgrade).chain(modules).chain(tree).chain(camo).chain(lod).collect();
        return (!rows.is_empty()).then(|| {
            AnyView::new(view! {
                column(class = "wt-armor-hull", attr:data-filtering = String::new()) {{rows}}
            })
        });
    }

    let all_names: Rc<Vec<String>> = Rc::new(groups.iter().flat_map(|(_, names)| names.iter().cloned()).collect());
    let none_names = Rc::clone(&all_names);
    let header = view! {
        row(class = "wt-armor-hull__header") {
            Button(
                variant = ButtonVariant::Outline,
                size = ButtonSize::Xs,
                attr:data-testid = "armor-hull-vis-all",
                on:click = move |_| set_meshes(viewport, &all_names, true)
            ) {{t!("ui.armor.all_btn").into_owned()}}
            Button(
                variant = ButtonVariant::Outline,
                size = ButtonSize::Xs,
                attr:data-testid = "armor-hull-vis-none",
                on:click = move |_| set_meshes(viewport, &none_names, false)
            ) {{t!("ui.armor.none_btn").into_owned()}}
            {opaque}
        }
    };
    // The part tree comes before the camo and LOD pickers: it is what the reader turns to most,
    // and the camo list can run long. It has no scroll of its own, so a wheel over it moves only
    // the rail.
    let camo = camo.map(|camo| {
        view! {
            column(class = "wt-armor-hull") {
                box(class = "wt-armor-hull__rule") {}
                {camo}
            }
        }
    });

    Some(AnyView::new(view! {
        column(class = "wt-armor-hull") {
            {header}
            {upgrade}
            {modules}
            box(class = "wt-armor-hull__rule") {}
            {field}
            {tree}
            {camo}
            {lod}
        }
    }))
}

/// Shows or hides every mesh in `names`.
fn set_meshes(viewport: ArmorViewportState, names: &[String], visible: bool) {
    let names = names.to_vec();
    viewport.mutate_hull_visibility(move |hull_visibility| {
        for name in names {
            hull_visibility.insert(name, visible);
        }
    });
}

/// What a row's choices are filtered by: nothing when the row's heading matches, which keeps every
/// choice in it, and `filter` otherwise.
fn within(filter: &RailFilter, heading: &str) -> RailFilter {
    if filter.keeps(heading) { RailFilter::default() } else { filter.clone() }
}

/// A row of choices under a heading marked by `filter`.
fn choice_row(heading: MarkedLabel, choices: Vec<AnyView>) -> AnyView {
    let heading = heading.view();

    AnyView::new(view! {
        row(class = "wt-armor-hull__choices") {
            box(class = "wt-armor-hull__heading") {{heading}}
            {choices}
        }
    })
}

/// The hull upgrade choices, one per upgrade. No upgrade selected reads as the first, the stock
/// hull.
fn hull_upgrade_row(viewport: ArmorViewportState, armor: &LoadedShipArmor, filter: &RailFilter) -> Option<AnyView> {
    let stock_key = armor.hull_upgrade_names[0].0.clone();
    let heading = t!("ui.armor.upgrade").into_owned();
    let inner = within(filter, &heading);
    let kept: Vec<&(String, String)> =
        armor.hull_upgrade_names.iter().filter(|(_, label)| inner.keeps(label)).collect();
    if kept.is_empty() {
        return None;
    }

    let choices: Vec<AnyView> = kept
        .into_iter()
        .map(|(key, label)| {
            let is_stock = *key == stock_key;
            let chosen = key.clone();
            let is_selected =
                move |model: &ViewportModel| model.selected_hull.as_ref().map_or(is_stock, |sel| *sel == chosen);
            let selected_read = is_selected.clone();
            let selected = Signal::derive_local(move || viewport.read(&selected_read).unwrap_or(false));
            let key = key.clone();
            choice_button(
                format!("armor-hull-upgrade-{key}"),
                MarkedLabel::plain(label.clone()),
                selected,
                false,
                None,
                move || {
                    if viewport.read_untracked(&is_selected).unwrap_or(true) {
                        return;
                    }
                    viewport.select_hull_upgrade(key.clone());
                },
            )
        })
        .collect();

    Some(choice_row(filter.marked(heading), choices))
}

/// One module alternative row for component type `ct`, one choice per alternative labelled
/// "{ct} {letter}" with the component's full name as its tooltip. No alternative selected reads as
/// the first. While filtering, an alternative is kept when its label or its name matches.
fn module_alternative_row(
    viewport: ArmorViewportState,
    ct: ComponentType,
    alternatives: &[String],
    filter: &RailFilter,
) -> Option<AnyView> {
    let heading = format!("{ct}:");
    let inner = within(filter, &heading);
    // Past Z the letters run on through ASCII, as the GPUI viewer labels them.
    let kept: Vec<(usize, String, String)> = alternatives
        .iter()
        .enumerate()
        .map(|(index, name)| (index, name.clone(), format!("{ct} {}", char::from(b'A'.wrapping_add(index as u8)))))
        .filter(|(_, name, label)| inner.keeps(label) || inner.keeps(name))
        .collect();
    if kept.is_empty() {
        return None;
    }

    let choices: Vec<AnyView> = kept
        .into_iter()
        .map(|(index, name, label)| {
            let chosen = name.clone();
            let is_selected = move |model: &ViewportModel| {
                model.selected_modules.get(&ct).map_or(index == 0, |selected| *selected == chosen)
            };
            let selected_read = is_selected.clone();
            let selected = Signal::derive_local(move || viewport.read(&selected_read).unwrap_or(false));
            choice_button(
                format!("armor-module-alt-{ct}-{index}"),
                MarkedLabel::plain(label),
                selected,
                false,
                Some(name.clone()),
                move || {
                    if viewport.read_untracked(&is_selected).unwrap_or(true) {
                        return;
                    }
                    viewport.select_module_alternative(ct, name.clone());
                },
            )
        })
        .collect();

    Some(choice_row(filter.marked(heading), choices))
}

/// The hull level of detail choices, 0 labelled as the highest.
fn lod_row(viewport: ArmorViewportState, armor: &LoadedShipArmor, filter: &RailFilter) -> Option<AnyView> {
    let heading = t!("ui.armor.lod").into_owned();
    let inner = within(filter, &heading);
    let kept: Vec<(usize, String)> = (0..armor.hull_lod_count)
        .map(|lod| (lod, if lod == 0 { t!("ui.armor.lod_highest").into_owned() } else { lod.to_string() }))
        .filter(|(_, label)| inner.keeps(label))
        .collect();
    if kept.is_empty() {
        return None;
    }

    let choices: Vec<AnyView> = kept
        .into_iter()
        .map(|(lod, label)| {
            let selected = Signal::derive_local(move || viewport.read(|model| model.hull_lod == lod).unwrap_or(false));
            choice_button(
                format!("armor-hull-lod-{lod}"),
                MarkedLabel::plain(label),
                selected,
                false,
                None,
                move || {
                    if viewport.read_untracked(|model| model.hull_lod == lod).unwrap_or(true) {
                        return;
                    }
                    viewport.select_hull_lod(lod);
                },
            )
        })
        .collect();

    Some(choice_row(filter.marked(heading), choices))
}

/// The camo picker: Stock, which takes the camo off, and the ship's own schemes, then folding
/// Universal, Expendable and other groups, each sorted by name and left out when empty. While
/// filtering only the schemes whose names match are kept, each group held open; `None` when none
/// match.
fn camo_picker(viewport: ArmorViewportState, armor: &LoadedShipArmor, filter: &RailFilter) -> Option<AnyView> {
    let heading = t!("ui.armor.camouflage").into_owned();
    let inner = within(filter, &heading);
    let mut rows: Vec<AnyView> = Vec::new();

    let none_label = t!("ui.armor.camo_none").into_owned();
    if inner.keeps(&none_label) {
        rows.push(camo_choice(viewport, "armor-camo-none".to_owned(), inner.marked(none_label), None));
    }

    let mut ship_infos: Vec<&CamoSchemeInfo> = armor
        .camo_schemes()
        .iter()
        .filter(|info| info.origin == CamoOrigin::ShipSpecific && inner.keeps(&info.display_name))
        .collect();
    ship_infos.sort_by_key(|info| info.display_name.to_lowercase());
    for info in ship_infos {
        rows.push(camo_row(viewport, info, &inner));
    }

    for (origin, key) in [
        (CamoOrigin::Universal, "ui.armor.camo_group_universal"),
        (CamoOrigin::Expendable, "ui.armor.camo_group_expendable"),
        (CamoOrigin::LegacyScan, "ui.armor.camo_group_other"),
    ] {
        let label = t!(key).into_owned();
        let group_filter = within(&inner, &label);
        let mut group: Vec<CamoSchemeInfo> = armor
            .camo_schemes()
            .iter()
            .filter(|info| info.origin == origin && group_filter.keeps(&info.display_name))
            .cloned()
            .collect();
        if group.is_empty() {
            continue;
        }
        group.sort_by_key(|info| info.display_name.to_lowercase());
        rows.push(camo_group(viewport, label, group, &inner, filter.is_active()));
    }
    if rows.is_empty() {
        return None;
    }

    let heading = filter.marked(heading).view();
    Some(AnyView::new(view! {
        column(class = "wt-armor-hull__stack", attr:data-testid = "armor-camo-picker") {
            box(class = "wt-armor-hull__title") {{heading}}
            {rows}
        }
    }))
}

/// One selectable camo scheme row, its name marked where `filter` matched.
fn camo_row(viewport: ArmorViewportState, info: &CamoSchemeInfo, filter: &RailFilter) -> AnyView {
    camo_choice(viewport, format!("armor-camo-{}", info.id.0), filter.marked(info.display_name.clone()), Some(info.id))
}

/// A camo row that selects `id`, `None` taking the camo off.
fn camo_choice(viewport: ArmorViewportState, test_id: String, label: MarkedLabel, id: Option<CamoSchemeId>) -> AnyView {
    let selected = Signal::derive_local(move || viewport.read(|model| model.selected_camo == id).unwrap_or(false));
    let label = label.view();

    AnyView::new(view! {
        Button(
            variant = ButtonVariant::Ghost,
            size = ButtonSize::Xs,
            class = "wt-armor-hull__camo",
            attr:data-testid = test_id,
            attr:data-selected = move || selected.get().then(String::new),
            a11y:selected = move || selected.get(),
            on:click = move |_| viewport.select_camo(id)
        ) {{label}}
    })
}

/// One folding camo origin group and, while it is open, its scheme rows, scrolled past 240 pixels.
/// Held open, with its caret inert, while the rail is `filtering`.
fn camo_group(
    viewport: ArmorViewportState,
    label: String,
    infos: Vec<CamoSchemeInfo>,
    filter: &RailFilter,
    filtering: bool,
) -> AnyView {
    let row_filter = within(filter, &label);
    let infos = Rc::new(infos);
    let held = label.clone();
    let expanded = Signal::derive_local(move || {
        filtering || viewport.read(|model| model.expanded_camo_groups.contains(&held)).unwrap_or(false)
    });

    let toggled = label.clone();
    let caret_id = format!("armor-camo-group-expand-{label}");
    let caret = move || {
        let toggled = toggled.clone();
        chevron(
            caret_id.clone(),
            expanded.get(),
            (!filtering)
                .then(|| Box::new(move || viewport.toggle_camo_group_expanded(toggled.clone())) as Box<dyn Fn()>),
        )
    };
    let title = filter.marked(label.clone()).view();
    let scroll_id = format!("armor-camo-group-scroll-{label}");
    let body = move || {
        expanded.get().then(|| {
            let rows: Vec<AnyView> = infos.iter().map(|info| camo_row(viewport, info, &row_filter)).collect();
            view! {
                column(class = "wt-armor-hull__camo-scroll", attr:data-testid = scroll_id.clone()) {{rows}}
            }
        })
    };

    AnyView::new(view! {
        column(class = "wt-armor-hull__stack") {
            row(class = "wt-armor-hull__row", attr:data-testid = format!("armor-camo-group-row-{label}")) {
                {caret}
                box(class = "wt-armor-hull__heading") {{title}}
            }
            {body}
        }
    })
}

/// What the hull tree rows are drawn from, copied out of the viewport so no borrow of it is held
/// while they are built.
struct HullTreeState {
    hull_visibility: HashMap<String, bool>,
    expanded_hull_groups: HashSet<String>,
}

impl HullTreeState {
    fn of(model: &ViewportModel) -> Self {
        Self {
            hull_visibility: model.hull_visibility.clone(),
            expanded_hull_groups: model.expanded_hull_groups.clone(),
        }
    }
}

/// One hull part group header row and, while it is expanded, its mesh checkboxes.
fn hull_group_row(
    viewport: ArmorViewportState,
    state: &HullTreeState,
    group: &str,
    names: &[String],
    filter: &TreeFilter,
) -> Option<AnyView> {
    let matched = filter.row_label(group.to_owned()).is_some();
    // A query the group matches keeps every mesh in it.
    let mesh_filter = filter.below(group);
    let expanded = filter.is_active() || state.expanded_hull_groups.contains(group);

    let rows: Vec<AnyView> = if expanded {
        names.iter().filter_map(|name| hull_mesh_row(viewport, name, &state.hull_visibility, &mesh_filter)).collect()
    } else {
        Vec::new()
    };
    if filter.is_active() && rows.is_empty() && !matched {
        return None;
    }

    // While the rail's query is typed, the group box reads and sets only the meshes it keeps; the
    // tree's own field only narrows what is shown.
    let acted_on: Arc<[String]> = if filter.narrows() {
        let rail = filter.rail_only().below(group);
        names.iter().filter(|name| rail.keeps(name)).cloned().collect()
    } else {
        names.iter().cloned().collect()
    };
    let all_on = hull_group_all_on(&state.hull_visibility, &acted_on);
    let any_on = hull_group_any_on(&state.hull_visibility, &acted_on);
    let checkbox =
        tri_checkbox(format!("armor-hull-vis-group-{group}"), TriState::from_all_any(all_on, any_on), move |checked| {
            set_meshes(viewport, &acted_on, checked)
        });

    // Inert while filtering, where every group is open whatever it was left at.
    let expand_group = group.to_owned();
    let caret = chevron(
        format!("armor-hull-vis-group-expand-{group}"),
        expanded,
        (!filter.is_active())
            .then(|| Box::new(move || viewport.toggle_hull_group_expanded(expand_group.clone())) as Box<dyn Fn()>),
    );
    let title = row_label(&filter.marked(group.to_owned()));
    let header = hoverable_row(
        format!("armor-hull-vis-group-row-{group}"),
        viewport,
        SidebarHighlightKey::HullMeshes(names.to_vec()),
        AnyView::new(view! { row(class = "wt-armor-hull__row") { {caret} {checkbox} {title} } }),
    );
    let children = expanded.then(|| view! { column(class = "wt-armor-hull__children") {{rows}} });

    Some(AnyView::new(view! {
        column(class = "wt-armor-hull__stack") {
            {header}
            {children}
        }
    }))
}

/// One hull mesh checkbox row.
fn hull_mesh_row(
    viewport: ArmorViewportState,
    name: &str,
    hull_visibility: &HashMap<String, bool>,
    filter: &TreeFilter,
) -> Option<AnyView> {
    // An absent mesh is hidden.
    let visible = hull_visibility.get(name).copied().unwrap_or(false);
    let toggled = name.to_owned();
    let row = filtered_checkbox(
        format!("armor-hull-vis-mesh-{name}"),
        name.to_owned(),
        Signal::<bool, LocalStorage>::stored_local(visible),
        false,
        filter,
        move |checked| {
            let name = toggled.clone();
            viewport.mutate_hull_visibility(move |hull_visibility| {
                hull_visibility.insert(name, checked);
            });
        },
    )?;
    let key = SidebarHighlightKey::HullMeshes(vec![name.to_owned()]);

    Some(hoverable_row(format!("armor-hull-vis-mesh-row-{name}"), viewport, key, row))
}
