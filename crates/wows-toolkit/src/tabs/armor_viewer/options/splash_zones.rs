//! The splash zone section: a ship's splash boxes grouped by the part of the ship they cover, each
//! ticked to outline it over the hull. Hovering a group or box row highlights its boxes.

use std::collections::HashSet;
use std::rc::Rc;

use wows_toolkit_armor::splash;
use wows_toolkit_armor::visibility::SidebarHighlightKey;
use wows_toolkit_armor::visibility::TriState;
use wt_translations::t;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::Signal;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;

use crate::tabs::armor_viewer::options::SectionCx;
use crate::tabs::armor_viewer::options::filter::RailFilter;
use crate::tabs::armor_viewer::options::tree_filter::RowFilter;
use crate::tabs::armor_viewer::options::tree_filter::TreeFilter;
use crate::tabs::armor_viewer::options::tree_filter::filtered_checkbox;
use crate::tabs::armor_viewer::options::tree_filter::match_count;
use crate::tabs::armor_viewer::options::visibility::TriCheck;
use crate::tabs::armor_viewer::options::visibility::chevron;
use crate::tabs::armor_viewer::options::visibility::hoverable_row;
use crate::tabs::armor_viewer::options::visibility::tri_checkbox;
use crate::tabs::armor_viewer::viewport::ArmorViewportState;
use crate::tabs::armor_viewer::viewport::SplashBoxName;
use crate::tabs::armor_viewer::viewport::ViewportModel;

const SHEET: &str = "wt-armor-splash";

const CSS: &str = css!(
    ".wt-armor-splash { display: flex; flex-direction: column; gap: 8px; width: 100%; }
    .wt-armor-splash__rows { display: flex; flex-direction: column; gap: 4px; }
    .wt-armor-splash__children { display: flex; flex-direction: column; gap: 4px; padding-left: 20px; }
    .wt-armor-splash__row { display: flex; align-items: center; gap: 4px; min-width: 0; }
    .wt-armor-splash__header { flex: none; display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
    .wt-armor-splash__rule { height: 1px; background-color: var(--lui-color-border); }
    .wt-armor-splash__count { flex: none; font-size: 12px; color: var(--wt-color-text-dim); }"
);

/// The splash zone section's body: `None` when the rail's filter keeps nothing in it or the scene
/// has no splash boxes.
pub fn body(cx: &SectionCx) -> Option<AnyView> {
    install_stylesheet(SHEET, CSS);
    let viewport = cx.viewport;
    let filter = cx.filter.clone();

    let armor = viewport.shown_untracked()?;
    armor.splash_data.as_ref()?;
    let groups: Rc<Vec<SplashGroup>> = Rc::new(
        armor
            .splash_box_groups
            .iter()
            .map(|(name, boxes)| SplashGroup {
                name: name.clone(),
                boxes: boxes
                    .iter()
                    .map(|name| SplashRow {
                        name: SplashBoxName::new(name.clone()),
                        label: splash::prettify_box_name(name),
                    })
                    .collect(),
            })
            .collect(),
    );
    if filter.is_active() {
        let labels: Vec<(String, Vec<String>)> = groups
            .iter()
            .map(|group| (group.name.clone(), group.boxes.iter().map(|row| row.label.clone()).collect()))
            .collect();
        if match_count(&filter, &labels) == 0 {
            return None;
        }
    }

    // All and None act on every box, not only the ones a filter left showing, so they are put
    // away while one is typed.
    let header = (!filter.is_active()).then(|| bulk_switches(viewport));
    let tree_filter = TreeFilter::new(filter, RailFilter::default());
    let rows = move || {
        let Some(state) = viewport.read(SplashTreeState::of) else {
            return Vec::new();
        };
        groups.iter().filter_map(|group| group_row(viewport, &state, group, &tree_filter)).collect::<Vec<AnyView>>()
    };

    Some(AnyView::new(view! {
        column(class = "wt-armor-splash", attr:data-testid = "armor-splash-zones") {
            {header}
            column(class = "wt-armor-splash__rows") {{rows}}
        }
    }))
}

/// One group of the list: the boxes covering one part of the ship.
struct SplashGroup {
    name: String,
    boxes: Vec<SplashRow>,
}

/// One box of the list, with the name the reader reads.
struct SplashRow {
    name: SplashBoxName,
    label: String,
}

/// What the list rows are drawn from, copied out of the viewport so no borrow of it is held while
/// they are built.
struct SplashTreeState {
    checked: HashSet<SplashBoxName>,
    expanded: HashSet<String>,
}

impl SplashTreeState {
    fn of(model: &ViewportModel) -> Self {
        Self { checked: model.splash_boxes_checked().clone(), expanded: model.expanded_splash_groups().clone() }
    }
}

/// The All and None switches over every box.
fn bulk_switches(viewport: ArmorViewportState) -> AnyView {
    AnyView::new(view! {
        column(class = "wt-armor-splash") {
            row(class = "wt-armor-splash__header") {
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Xs,
                    attr:data-testid = "armor-splash-zones-all",
                    on:click = move |_| viewport.set_every_splash_box_checked(true)
                ) {{t!("ui-armor-all_btn").into_owned()}}
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Xs,
                    attr:data-testid = "armor-splash-zones-none",
                    on:click = move |_| viewport.set_every_splash_box_checked(false)
                ) {{t!("ui-armor-none_btn").into_owned()}}
            }
            box(class = "wt-armor-splash__rule") {}
        }
    })
}

/// One group header row with its box count and, while it is expanded, its box rows.
fn group_row(
    viewport: ArmorViewportState,
    state: &SplashTreeState,
    group: &SplashGroup,
    filter: &TreeFilter,
) -> Option<AnyView> {
    let name = &group.name;
    let matched = filter.row_label(name.clone()).is_some();
    // A query the group matches keeps every box in it.
    let box_filter = filter.below(name);
    let expanded = filter.is_active() || state.expanded.contains(name);
    let rows: Vec<AnyView> = if expanded {
        group.boxes.iter().filter_map(|row| box_row(viewport, state, row, &box_filter)).collect()
    } else {
        Vec::new()
    };
    if filter.is_active() && rows.is_empty() && !matched {
        return None;
    }

    // While the rail's query is typed, the group box reads and sets only the boxes it keeps.
    let acted_on: Vec<SplashBoxName> = if filter.narrows() {
        let rail = filter.rail_only().below(name);
        group.boxes.iter().filter(|row| rail.keeps(&row.label)).map(|row| row.name.clone()).collect()
    } else {
        group.boxes.iter().map(|row| row.name.clone()).collect()
    };
    let all_on = acted_on.iter().all(|box_name| state.checked.contains(box_name));
    let any_on = acted_on.iter().any(|box_name| state.checked.contains(box_name));
    let TriCheck { checkbox, title } = tri_checkbox(
        format!("armor-splash-zone-group-check-{name}"),
        &filter.marked(name.clone()),
        TriState::from_all_any(all_on, any_on),
        move |checked| viewport.set_splash_boxes_checked(acted_on.clone(), checked),
    );

    // Inert while filtering, where every group is open whatever it was left at.
    let expand_group = name.clone();
    let caret = chevron(
        format!("armor-splash-zone-group-expand-{name}"),
        expanded,
        (!filter.is_active())
            .then(|| Box::new(move || viewport.toggle_splash_group_expanded(expand_group.clone())) as Box<dyn Fn()>),
    );
    let count = format!("({})", group.boxes.len());
    let key = SidebarHighlightKey::SplashBoxes(group.boxes.iter().map(|row| row.name.as_str().to_owned()).collect());
    let header = hoverable_row(
        format!("armor-splash-zone-group-{name}"),
        viewport,
        key,
        AnyView::new(view! {
            row(class = "wt-armor-splash__row") {
                {caret}
                {checkbox}
                {title}
                text(class = "wt-armor-splash__count", attr:data-testid = format!("armor-splash-zone-group-count-{name}")) {{count}}
            }
        }),
    );
    let children = expanded.then(|| view! { column(class = "wt-armor-splash__children") {{rows}} });

    Some(AnyView::new(view! {
        column(class = "wt-armor-splash__rows") {
            {header}
            {children}
        }
    }))
}

/// One box checkbox row, kept only when `filter` keeps its name.
fn box_row(
    viewport: ArmorViewportState,
    state: &SplashTreeState,
    row: &SplashRow,
    filter: &TreeFilter,
) -> Option<AnyView> {
    let name = row.name.as_str();
    let toggled = row.name.clone();
    let checkbox = filtered_checkbox(
        format!("armor-splash-zone-box-check-{name}"),
        row.label.clone(),
        Signal::<bool, LocalStorage>::stored_local(state.checked.contains(&row.name)),
        false,
        filter,
        move |checked| viewport.set_splash_boxes_checked(vec![toggled.clone()], checked),
    )?;
    let key = SidebarHighlightKey::SplashBoxes(vec![name.to_owned()]);

    Some(hoverable_row(format!("armor-splash-zone-box-{name}"), viewport, key, checkbox))
}
