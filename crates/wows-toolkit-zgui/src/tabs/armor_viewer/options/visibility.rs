//! The armor section: the tri-state zone, material and plate tree, its own filter field, the bulk
//! switches and, for a lone model, which ship its plate thickness is read from. Also the tree rows
//! the hull section draws the same way.

use std::borrow::Cow;
use std::collections::HashMap;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

use landaire_ui::EscapeClears;
use landaire_ui::TooltipContentProps;
use landaire_ui::TooltipProps;
use landaire_ui::TooltipTriggerProps;
use rust_i18n::t;
use wows_toolkit_armor::load_model::MountRef;
use wows_toolkit_armor::load_ship::ArmorThickness;
use wows_toolkit_armor::load_ship::ArmorZone;
use wows_toolkit_armor::load_ship::LoadedShipArmor;
use wows_toolkit_armor::load_ship::PlateKey;
use wows_toolkit_armor::load_ship::ZonePart;
use wows_toolkit_armor::visibility::SidebarHighlightKey;
use wows_toolkit_armor::visibility::TriState;
use wows_toolkit_armor::visibility::part_any_plate_hidden;
use wows_toolkit_armor::visibility::part_on;
use wows_toolkit_armor::visibility::plate_explicitly_hidden;
use wows_toolkit_armor::visibility::zone_all_on;
use wows_toolkit_armor::visibility::zone_any_on;
use wows_toolkit_icons::Glyph;
use wowsunpack::export::gltf_export::thickness_to_color;
use wowsunpack::export::ship::ShipAssets;
use wowsunpack::game_params::types::Species;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::Set;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::ShowProps;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::CheckboxProps;
use zgui_ui::Checked;
use zgui_ui::DropdownMenuContentProps;
use zgui_ui::DropdownMenuProps;
use zgui_ui::DropdownMenuTriggerProps;
use zgui_ui::InputProps;
use zgui_ui::MenuCheckboxItemProps;
use zgui_ui::prelude::Binding;
use zgui_ui_icons::IconProps;
use zgui_ui_icons::IconSize;
use zgui_ui_icons::set::chevron::CHEVRON_RIGHT;

use crate::icons::AppIconProps;
use crate::shell::Shell;
use crate::shell::tabs::ModifierKey;
use crate::tabs::armor_viewer::options::SectionCx;
use crate::tabs::armor_viewer::options::filter::MarkedLabel;
use crate::tabs::armor_viewer::options::filter::RailFilter;
use crate::tabs::armor_viewer::options::tree_filter::RowFilter;
use crate::tabs::armor_viewer::options::tree_filter::TreeFilter;
use crate::tabs::armor_viewer::options::tree_filter::match_count;
use crate::tabs::armor_viewer::viewport::ArmorViewportState;
use crate::tabs::armor_viewer::viewport::ViewportModel;

const SHEET: &str = "wt-armor-tree";

const CSS: &str = css!(
    ".wt-armor-tree { display: flex; flex-direction: column; gap: 8px; width: 100%; }
    .wt-armor-tree__rows { display: flex; flex-direction: column; gap: 4px; }
    .wt-armor-tree__children { display: flex; flex-direction: column; gap: 4px; padding-left: 20px; }
    .wt-armor-tree__row { display: flex; align-items: center; gap: 4px; min-width: 0; }
    .wt-armor-tree__label { font-size: 13px; min-width: 0; }
    .wt-armor-tree__heading { font-size: 13px; font-weight: 700; }
    .wt-armor-tree__chevron {
        flex: none;
        display: flex;
        align-items: center;
        transition: transform var(--lui-motion-turn) var(--lui-motion-ease);
    }
    .wt-armor-tree__chevron[data-interactive] { cursor: pointer; }
    .wt-armor-tree__chevron[data-open] { transform: rotate(90deg); }
    .wt-armor-tree__swatch { flex: none; width: 10px; height: 10px; border-radius: 2px; }
    .wt-armor-tree__header { flex: none; display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
    .wt-armor-tree__rule { height: 1px; background-color: var(--lui-color-border); }
    .wt-armor-tree__scroll { max-height: 360px; overflow-y: auto; }
    .wt-armor-tree__field { display: flex; flex-direction: column; gap: 4px; }
    .wt-armor-tree__field-row { display: flex; align-items: center; gap: 4px; }
    .wt-armor-tree__input { flex: 1 1 auto; min-width: 0; }
    .wt-armor-tree__dim { font-size: 12px; color: var(--wt-color-text-dim); }
    .wt-armor-tree__none { font-size: 13px; color: var(--wt-color-text-dim); padding: 8px; }
    .wt-armor-tree__thickness { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
    .wt-armor-tree__thickness-label { flex: 1 1 auto; min-width: 0; font-size: 12px; color: var(--wt-color-text-dim); }
    .wt-armor-tri.wt-armor-tri:indeterminate { background-color: transparent; border-color: var(--zui-color-input); }
    .wt-armor-tri.wt-armor-tri:indeterminate > .zui-checkbox__dash { color: var(--wt-color-warn); }"
);

/// The armor section's body: `None` when the rail's filter keeps nothing in it.
pub fn body(cx: &SectionCx) -> Option<AnyView> {
    install_stylesheet(SHEET, CSS);
    let viewport = cx.viewport;
    let workspace = cx.workspace;
    let filter = cx.filter.clone();

    let Some(armor) = viewport.shown_untracked() else {
        return (!filter.is_active()).then(no_ship_loaded);
    };

    let names = TreeNames {
        assets: workspace.library.ready_untracked().map(|bundle| Arc::clone(&bundle.assets)),
        species: armor.species,
    };
    let labels: Rc<Vec<(String, Vec<String>)>> = Rc::new(
        armor
            .zone_part_plates
            .iter()
            .map(|zone| (names.zone(&zone.name), zone.parts.iter().map(|part| names.part(&part.name)).collect()))
            .collect(),
    );
    // Only the rail's query drops the section; the tree's own field narrows the rows under it.
    if filter.is_active() && match_count(&filter, &labels) == 0 {
        return None;
    }

    let thickness = (!filter.is_active()).then(|| thickness_source(viewport, &armor)).flatten();
    // The bulk switches act on every plate, not only the ones a filter left showing, so they are
    // put away while one is typed.
    let header = (!filter.is_active()).then(|| bulk_switches(viewport));

    let zone_filter = workspace.zone_filter;
    let counted = Rc::clone(&labels);
    let field_matches = Signal::derive_local(move || {
        let query = RailFilter::new(&zone_filter.get());
        query.is_active().then(|| match_count(&query, &counted))
    });
    let field = tree_field("armor-zone-filter", zone_filter, t!("ui.armor.filter_zones").into_owned(), field_matches);

    let tree = TreeCx {
        viewport,
        shell: Shell::current(),
        zone_tooltip: Rc::from(t!("ui.armor.zone_toggle_tooltip", key = ModifierKey::Primary.label()).as_ref()),
    };
    let rows = move || {
        let field_query = RailFilter::new(&zone_filter.get());
        let tree_filter = TreeFilter::new(filter.clone(), field_query);
        let show_zero_mm = viewport.show_zero_mm();
        let Some(state) = viewport.read(ArmorTreeState::of) else {
            return Vec::new();
        };
        armor
            .zone_part_plates
            .iter()
            .zip(labels.iter())
            .filter_map(|(zone, labels)| zone_row(&tree, &state, show_zero_mm, zone, labels, &tree_filter))
            .collect::<Vec<AnyView>>()
    };

    Some(AnyView::new(view! {
        column(class = "wt-armor-tree") {
            {thickness}
            {header}
            {field}
            box(class = "wt-armor-tree__scroll", attr:data-testid = "armor-visibility-tree-scroll") {
                column(class = "wt-armor-tree__rows") {{rows}}
            }
        }
    }))
}

/// What a section says while no scene is open.
pub(super) fn no_ship_loaded() -> AnyView {
    install_stylesheet(SHEET, CSS);

    AnyView::new(view! {
        text(class = "wt-armor-tree__none") {{t!("ui.armor.no_ship_loaded").into_owned()}}
    })
}

/// A tree's own filter field, and under it how many of the tree's rows the field matched by name
/// while something is typed in it. Escape clears what was typed.
pub(super) fn tree_field(
    id: &'static str,
    value: RwSignal<String, LocalStorage>,
    placeholder: String,
    matches: Signal<Option<usize>, LocalStorage>,
) -> AnyView {
    install_stylesheet(SHEET, CSS);
    let count = move || {
        let said = match matches.get()? {
            0 => t!("ui.armor.tree_no_matches").into_owned(),
            count => t!("ui.armor.tree_matches", count = count).into_owned(),
        };
        let label = said.clone();
        Some(view! {
            text(class = "wt-armor-tree__dim", attr:data-testid = format!("{id}-count"), a11y:label = label) {{said}}
        })
    };
    let escape = EscapeClears::new(value);
    let clear_label = t!("ui.stats.clear").into_owned();

    AnyView::new(view! {
        column(class = "wt-armor-tree__field") {
            row(class = "wt-armor-tree__field-row") {
                AppIcon(glyph = Glyph::Search)
                Input(
                    value = value,
                    placeholder = placeholder.clone(),
                    label = placeholder,
                    class = "wt-armor-tree__input",
                    attr:data-testid = id,
                    on:key_down:capture = move |ev| {
                        escape.key_down(ev);
                    }
                )
                if move || !value.get().is_empty() {
                    Button(
                        variant = ButtonVariant::Ghost,
                        size = ButtonSize::IconXs,
                        attr:data-testid = format!("{id}-clear"),
                        a11y:label = clear_label.clone(),
                        on:click = move |_| value.set(String::new())
                    ) {
                        AppIcon(glyph = Glyph::X)
                    }
                }
            }
            {count}
        }
    })
}

/// A checkbox that can show "some of these": a box whose rows are partly shown reads as mixed,
/// an empty box with a dash in the warning colour. `on_change` is told whether the box was ticked.
pub(super) fn tri_checkbox(test_id: String, state: TriState, on_change: impl Fn(bool) + 'static) -> AnyView {
    install_stylesheet(SHEET, CSS);
    let checked = match state {
        TriState::On => Checked::Yes,
        TriState::Partial => Checked::Mixed,
        TriState::Off => Checked::No,
    };

    AnyView::new(view! {
        Checkbox(
            checked = Binding::controlled(Signal::stored_local(checked), move |next: Checked| on_change(next.is_checked())),
            class = "wt-armor-tri",
            attr:data-testid = test_id
        )
    })
}

/// Wraps `content` in a row that highlights `key`'s plates or meshes in the viewport while the
/// pointer is over it.
pub(super) fn hoverable_row(
    test_id: String,
    viewport: ArmorViewportState,
    key: SidebarHighlightKey,
    content: AnyView,
) -> AnyView {
    let entered = key.clone();

    AnyView::new(view! {
        box(
            attr:data-testid = test_id,
            on:pointer_enter = move |_| viewport.set_sidebar_hover(entered.clone()),
            on:pointer_leave = move |_| viewport.clear_sidebar_hover_if(&key)
        ) {{content}}
    })
}

/// The caret before a row that folds. Inert while `on_click` is `None`.
pub(super) fn chevron(test_id: String, open: bool, on_click: Option<Box<dyn Fn()>>) -> AnyView {
    let interactive = on_click.is_some();

    AnyView::new(view! {
        box(
            class = "wt-armor-tree__chevron",
            attr:data-testid = test_id,
            attr:data-open = open.then(String::new),
            attr:data-interactive = interactive.then(String::new),
            a11y:expanded = open,
            on:click = move |_| {
                if let Some(on_click) = &on_click {
                    on_click();
                }
            }
        ) {
            Icon(icon = CHEVRON_RIGHT, size = IconSize::Sm)
        }
    })
}

/// A row label as a view.
pub(super) fn row_label(label: &MarkedLabel) -> AnyView {
    let label = label.view();

    AnyView::new(view! { box(class = "wt-armor-tree__label") {{label}} })
}

/// The All, None and Reset Plates switches over the whole tree.
fn bulk_switches(viewport: ArmorViewportState) -> AnyView {
    AnyView::new(view! {
        column(class = "wt-armor-tree") {
            row(class = "wt-armor-tree__header") {
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Xs,
                    attr:data-testid = "armor-vis-all",
                    on:click = move |_| viewport.set_all_parts_visible()
                ) {{t!("ui.armor.all_btn").into_owned()}}
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Xs,
                    attr:data-testid = "armor-vis-none",
                    on:click = move |_| viewport.set_all_parts_hidden()
                ) {{t!("ui.armor.none_btn").into_owned()}}
                if move || viewport.read(|model| !model.plate_visibility.is_empty()).unwrap_or(false) {
                    Button(
                        variant = ButtonVariant::Outline,
                        size = ButtonSize::Xs,
                        attr:data-testid = "armor-vis-reset-plates",
                        on:click = move |_| viewport.reset_plate_overrides()
                    ) {{t!("ui.armor.reset_plates").into_owned()}}
                }
            }
            box(class = "wt-armor-tree__rule") {}
        }
    })
}

/// Which ship a model's plate thickness is read from, with a picker for any other ship mounting
/// it. `None` for a ship, whose own armor it is.
fn thickness_source(viewport: ArmorViewportState, armor: &LoadedShipArmor) -> Option<AnyView> {
    let model = armor.model()?;
    let label = match &model.thickness {
        ArmorThickness::Ship(source) => {
            t!("ui.model_view.thickness_from", ship = source.display_name, hp = source.hp.as_str()).into_owned()
        }
        ArmorThickness::Hull(source) => {
            t!("ui.model_view.thickness_from", ship = source.display_name, hp = source.section.stem()).into_owned()
        }
        ArmorThickness::Building { name } => t!("ui.model_view.thickness_from_building", building = name).into_owned(),
        ArmorThickness::Unknown => t!("ui.model_view.thickness_unknown").into_owned(),
    };
    let current = match &model.thickness {
        ArmorThickness::Ship(source) => Some(source.vehicle_param_index.clone()),
        ArmorThickness::Hull(source) => Some(source.vehicle_param_index.clone()),
        ArmorThickness::Building { .. } | ArmorThickness::Unknown => None,
    };

    let picker = (model.thickness_choices.len() > 1).then(|| {
        let choices: Vec<ThicknessChoice> = model
            .thickness_choices
            .iter()
            .map(|choice| ThicknessChoice {
                mount: MountRef { vehicle_param_index: choice.vehicle_param_index.clone(), hp: choice.hp.clone() },
                checked: current.as_ref() == Some(&choice.vehicle_param_index),
                name: choice.display_name.clone(),
            })
            .collect();
        let items = move || {
            choices
                .iter()
                .map(|choice| {
                    let (mount, name) = (choice.mount.clone(), choice.name.clone());
                    AnyView::new(view! {
                        MenuCheckboxItem(
                            checked = Binding::controlled(Signal::stored_local(choice.checked), move |_: bool| {
                                viewport.set_thickness_source(mount.clone())
                            })
                        ) {{name}}
                    })
                })
                .collect::<Vec<AnyView>>()
        };
        AnyView::new(view! {
            DropdownMenu {
                DropdownMenuTrigger(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Xs,
                    attr:data-testid = "model-thickness-pick"
                ) {{t!("ui.model_view.thickness_other_ships").into_owned()}}
                DropdownMenuContent {{items()}}
            }
        })
    });

    Some(AnyView::new(view! {
        row(class = "wt-armor-tree__thickness", attr:data-testid = "model-thickness-source") {
            text(class = "wt-armor-tree__thickness-label") {{label}}
            {picker}
        }
    }))
}

/// One ship a model's plate thickness can be read from, as the picker offers it.
struct ThicknessChoice {
    mount: MountRef,
    /// Whether the thickness is read from it now.
    checked: bool,
    name: String,
}

/// Names the tree's zones and parts the way the game names them.
struct TreeNames {
    assets: Option<Arc<ShipAssets>>,
    species: Option<Species>,
}

impl TreeNames {
    fn zone(&self, name: &str) -> String {
        match &self.assets {
            Some(assets) => wows_toolkit_armor::catalog::translate_name(assets.metadata(), name),
            None => name.to_owned(),
        }
    }

    /// `key` is the part's material key, shown as is while the build is still loading.
    fn part(&self, key: &str) -> String {
        match &self.assets {
            Some(assets) => wows_toolkit_armor::catalog::translate_material_key(assets.metadata(), key, self.species),
            None => key.to_owned(),
        }
    }
}

/// What the armor tree rows are drawn from, copied out of the viewport so no borrow of it is held
/// while they are built.
struct ArmorTreeState {
    part_visibility: HashMap<(String, String), bool>,
    plate_visibility: HashMap<PlateKey, bool>,
    expanded_zones: HashSet<String>,
    expanded_parts: HashSet<(String, String)>,
}

impl ArmorTreeState {
    fn of(model: &ViewportModel) -> Self {
        Self {
            part_visibility: model.part_visibility.clone(),
            plate_visibility: model.plate_visibility.clone(),
            expanded_zones: model.expanded_zones.clone(),
            expanded_parts: model.expanded_parts.clone(),
        }
    }
}

/// What every armor tree row reads besides its own zone.
struct TreeCx {
    viewport: ArmorViewportState,
    shell: Shell,
    /// What a zone's box does, and what holding the solo key does instead.
    zone_tooltip: Rc<str>,
}

/// One zone header row and, while it is expanded, its parts.
fn zone_row(
    tree: &TreeCx,
    state: &ArmorTreeState,
    show_zero_mm: bool,
    zone: &ArmorZone,
    labels: &(String, Vec<String>),
    filter: &TreeFilter,
) -> Option<AnyView> {
    let viewport = tree.viewport;
    let (zone_label, part_labels) = labels;
    let matched = filter.row_label(zone_label.clone()).is_some();
    // A query the zone matches keeps every part in it; the others keep only the parts that match
    // them, under their zone.
    let part_filter = filter.below(zone_label);
    let expanded = filter.is_active() || state.expanded_zones.contains(&zone.name);
    let parts: Vec<(&ZonePart, MarkedLabel)> = if expanded {
        zone.parts
            .iter()
            .zip(part_labels)
            .filter_map(|(part, label)| Some((part, part_filter.row_label(label.clone())?)))
            .collect()
    } else {
        Vec::new()
    };
    if filter.is_active() && parts.is_empty() && !matched {
        return None;
    }

    // While the rail's query is typed, the zone box reads and sets only the parts it keeps; the
    // tree's own field only narrows what is shown.
    let acted_on = if filter.narrows() {
        let rail = filter.rail_only().below(zone_label);
        Cow::Owned(ArmorZone {
            name: zone.name.clone(),
            parts: zone
                .parts
                .iter()
                .zip(part_labels)
                .filter(|(_, label)| rail.keeps(label))
                .map(|(part, _)| part.clone())
                .collect(),
        })
    } else {
        Cow::Borrowed(zone)
    };
    let all_on = zone_all_on(&acted_on, &state.part_visibility, &state.plate_visibility, show_zero_mm);
    let any_on = zone_any_on(&acted_on, &state.part_visibility);
    let tri = TriState::from_all_any(all_on, any_on);

    let shell = tree.shell;
    let toggle_zone = zone.name.clone();
    let toggle_parts: Vec<String> = acted_on.parts.iter().map(|part| part.name.clone()).collect();
    let checkbox = tri_checkbox(format!("armor-vis-zone-{}", zone.name), tri, move |checked| {
        let solo = ModifierKey::Primary.is_down(shell.held_modifiers());
        viewport.toggle_zone(toggle_zone.clone(), toggle_parts.clone(), checked, solo);
    });

    // Inert while filtering, where every zone is open whatever it was left at.
    let expand_zone = zone.name.clone();
    let caret = chevron(
        format!("armor-vis-zone-expand-{}", zone.name),
        expanded,
        (!filter.is_active())
            .then(|| Box::new(move || viewport.toggle_zone_expanded(expand_zone.clone())) as Box<dyn Fn()>),
    );

    let title = row_label(&filter.marked(zone_label.clone()));
    let tooltip = tree.zone_tooltip.to_string();
    let toggle_id = format!("armor-vis-zone-toggle-{}", zone.name);
    let toggle = view! {
        Tooltip {
            TooltipTrigger {
                row(class = "wt-armor-tree__row", attr:data-testid = toggle_id) {
                    {checkbox}
                    {title}
                }
            }
            TooltipContent {{tooltip.clone()}}
        }
    };
    let header = hoverable_row(
        format!("armor-vis-zone-row-{}", zone.name),
        viewport,
        SidebarHighlightKey::Zone(zone.name.clone()),
        AnyView::new(view! { row(class = "wt-armor-tree__row") { {caret} {toggle} } }),
    );

    let children = expanded.then(|| {
        let rows: Vec<AnyView> = parts
            .into_iter()
            .map(|(part, label)| part_row(viewport, state, show_zero_mm, &zone.name, part, label))
            .collect();
        view! { column(class = "wt-armor-tree__children") {{rows}} }
    });

    Some(AnyView::new(view! {
        column(class = "wt-armor-tree__rows") {
            {header}
            {children}
        }
    }))
}

/// One material row: a single checkbox when it has at most one visible plate thickness, otherwise
/// a folding header over its plate rows. `label` is the part's name as the filter marked it.
fn part_row(
    viewport: ArmorViewportState,
    state: &ArmorTreeState,
    show_zero_mm: bool,
    zone_name: &str,
    part: &ZonePart,
    label: MarkedLabel,
) -> AnyView {
    let part_key = (zone_name.to_owned(), part.name.clone());
    let part_visible = part_on(&state.part_visibility, zone_name, &part.name);
    let any_plate_hidden = part_any_plate_hidden(zone_name, part, &state.plate_visibility, show_zero_mm);
    let visible_plates: Vec<i32> = part.plates.iter().copied().filter(|&t| show_zero_mm || t != 0).collect();

    // Clears overrides for the plates the zero-mm setting leaves shown, not every plate on the
    // part, so an override on a thickness hidden by that setting survives a toggle.
    let toggle_zone = zone_name.to_owned();
    let toggle_part = part.name.clone();
    let toggle_plates = visible_plates.clone();
    let on_change = move |checked: bool| {
        viewport.toggle_part(toggle_zone.clone(), toggle_part.clone(), toggle_plates.clone(), checked);
    };
    let row_id = format!("armor-vis-part-row-{zone_name}-{}", part.name);
    let box_id = format!("armor-vis-part-{zone_name}-{}", part.name);
    let key = SidebarHighlightKey::Part(zone_name.to_owned(), part.name.clone());
    let title = row_label(&label);

    if visible_plates.len() <= 1 {
        let checked = part_visible && !any_plate_hidden;
        let checkbox = tri_checkbox(box_id, TriState::from_all_any(checked, checked), on_change);
        return hoverable_row(
            row_id,
            viewport,
            key,
            AnyView::new(view! { row(class = "wt-armor-tree__row") { {checkbox} {title} } }),
        );
    }

    let expanded = state.expanded_parts.contains(&part_key);
    let tri = TriState::from_all_any(part_visible && !any_plate_hidden, part_visible);
    let checkbox = tri_checkbox(box_id, tri, on_change);
    let caret = chevron(
        format!("armor-vis-part-expand-{zone_name}-{}", part.name),
        expanded,
        Some(Box::new(move || viewport.toggle_part_expanded(part_key.clone()))),
    );
    let header = hoverable_row(
        row_id,
        viewport,
        key,
        AnyView::new(view! { row(class = "wt-armor-tree__row") { {caret} {checkbox} {title} } }),
    );

    let children = expanded.then(|| {
        let rows: Vec<AnyView> = visible_plates
            .iter()
            .map(|&thickness| {
                plate_row(viewport, zone_name, &part.name, thickness, part_visible, &state.plate_visibility)
            })
            .collect();
        view! { column(class = "wt-armor-tree__children") {{rows}} }
    });

    AnyView::new(view! {
        column(class = "wt-armor-tree__rows") {
            {header}
            {children}
        }
    })
}

/// One plate row: a swatch in the thickness's legend color and a "{mm} mm" checkbox.
fn plate_row(
    viewport: ArmorViewportState,
    zone_name: &str,
    part_name: &str,
    thickness_tenths: i32,
    part_visible: bool,
    plate_visibility: &HashMap<PlateKey, bool>,
) -> AnyView {
    let key = PlateKey { zone: zone_name.to_owned(), material_name: part_name.to_owned(), thickness_tenths };
    let plate_visible = !plate_explicitly_hidden(plate_visibility, &key);
    let thickness_mm = thickness_tenths as f32 / 10.0;
    let swatch = swatch_color(thickness_to_color(thickness_mm));

    let checked = part_visible && plate_visible;
    let toggled = key.clone();
    let checkbox = tri_checkbox(
        format!("armor-vis-plate-{zone_name}-{part_name}-{thickness_tenths}"),
        TriState::from_all_any(checked, checked),
        move |_| viewport.toggle_plate(toggled.clone()),
    );
    let label = format!("{thickness_mm:.0} mm");

    hoverable_row(
        format!("armor-vis-plate-row-{zone_name}-{part_name}-{thickness_tenths}"),
        viewport,
        SidebarHighlightKey::Plate(key),
        AnyView::new(view! {
            row(class = "wt-armor-tree__row") {
                box(class = "wt-armor-tree__swatch", style:background-color = swatch) {}
                {checkbox}
                text(class = "wt-armor-tree__label") {{label}}
            }
        }),
    )
}

/// An RGBA color in 0 to 1 as CSS, each channel scaled to a byte the way the legend does it:
/// truncated toward zero and saturated to 0..=255.
fn swatch_color(color: [f32; 4]) -> String {
    let to_u8 = |channel: f32| (channel * 255.0) as u8;
    let alpha = f32::from(to_u8(color[3])) / 255.0;

    format!("rgba({}, {}, {}, {alpha:.3})", to_u8(color[0]), to_u8(color[1]), to_u8(color[2]))
}
