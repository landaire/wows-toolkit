//! The board's two toolbar rows: what it is set on, the ship picker, the range circles, the presets
//! and the scan above; the capture point and drawing tools below.

use landaire_ui::EscapeClears;
use landaire_ui::MarkedTextProps;
use landaire_ui::TooltipContentProps;
use landaire_ui::TooltipProps;
use landaire_ui::TooltipTriggerProps;
use rust_i18n::t;
use wows_toolkit_icons::Glyph;
use wows_toolkit_replay_data::tactics::BoardTool;
use wows_toolkit_replay_data::tactics::INKS;
use wows_toolkit_replay_data::tactics::MAX_CAP_RADIUS;
use wows_toolkit_replay_data::tactics::MAX_NIB;
use wows_toolkit_replay_data::tactics::MIN_CAP_RADIUS;
use wows_toolkit_replay_data::tactics::MIN_NIB;
use wows_toolkit_replay_data::tactics::MapEntry;
use wows_toolkit_replay_data::tactics::RADIUS_STEP_KM;
use wows_toolkit_replay_data::tactics::RANGE_CIRCLES;
use wows_toolkit_replay_data::tactics::RangeCircle;
use wows_toolkit_replay_data::tactics::map_entries;
use wows_toolkit_replay_data::tactics::mode_value;
use wows_toolkit_replay_data::tactics::radius_km;
use wows_toolkit_replay_data::tactics::ships::ShipChoice;
use wows_toolkit_replay_data::tactics::ships::tier_roman;
use wows_toolkit_replay_data::tactics::shortcut_modifier_name;
use wows_toolkit_replay_data::tactics::team_color;
use wows_toolkit_viewmodel::text_filter::FilterQuery;
use zgui::component;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::Memo;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::Children;
use zgui::view::ForProps;
use zgui::view::IntoView;
use zgui::view::NodeRef;
use zgui::view::ShowProps;
use zgui::view::install_stylesheet;
use zgui::vocab::Key;
use zgui::vocab::NamedKey;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::CheckboxProps;
use zgui_ui::Checked;
use zgui_ui::ComboboxContentProps;
use zgui_ui::ComboboxInputProps;
use zgui_ui::ComboboxItemProps;
use zgui_ui::ComboboxProps;
use zgui_ui::InputProps;
use zgui_ui::LabelProps;
use zgui_ui::PopoverContentProps;
use zgui_ui::PopoverProps;
use zgui_ui::PopoverTriggerProps;
use zgui_ui::SeparatorProps;
use zgui_ui::SpinnerProps;
use zgui_ui_primitives::Binding;

use crate::icons::AppIconProps;
use crate::services::Services;
use crate::text_marks::marks;
use crate::text_marks::marks_within;
use crate::windows::tactics::model::BoardModel;

const SHEET: &str = "wt-tactics-toolbar";

const CSS: &str = css!(
    ".wt-tactics-toolbar {
        display: flex;
        flex-direction: column;
        flex: none;
        padding: 0 8px;
        background-color: var(--lui-color-surface);
        border-bottom: 1px solid var(--lui-color-border);
    }
    .wt-tactics-toolbar__row {
        display: flex;
        flex-direction: row;
        flex-wrap: wrap;
        align-items: center;
        gap: 8px;
        padding: 4px 0;
    }
    .wt-tactics-toolbar__row--tools {
        gap: 12px;
        border-top: 1px solid var(--lui-color-border);
    }
    .wt-tactics-toolbar__group {
        display: flex;
        flex-direction: row;
        flex-wrap: wrap;
        align-items: center;
        gap: 4px;
    }
    .wt-tactics-toolbar__group--wide { gap: 8px; }
    .wt-tactics-toolbar__label {
        font-size: var(--zui-type-size-xs);
        color: var(--wt-color-text-dim);
        white-space: nowrap;
    }
    .wt-tactics-toolbar__rule {
        width: 1px;
        height: 16px;
        margin: 0 4px;
        flex: none;
        background-color: var(--lui-color-border);
    }
    .wt-tactics-toolbar .zui-button.wt-tactics-toolbar__map { width: 280px; flex: none; justify-content: flex-start; }
    .wt-tactics-toolbar__mode { width: 240px; flex: none; }
    .wt-tactics-toolbar__typing .wt-tactics-toolbar__search { width: 180px; flex: none; }
    .wt-tactics-toolbar__typing .wt-tactics-toolbar__map-search { width: 280px; flex: none; }
    .wt-tactics-panel--maps { min-width: 300px; gap: 8px; }
    .wt-tactics-panel--maps .wt-tactics-toolbar__matches { max-width: none; max-height: 360px; }
    .wt-tactics-toolbar__typing { display: flex; flex: none; }
    .wt-tactics-toolbar__ships {
        display: flex;
        flex-direction: column;
        gap: 4px;
    }
    .wt-tactics-toolbar__matches {
        display: flex;
        flex-direction: column;
        gap: 4px;
        max-width: 360px;
        max-height: 144px;
        overflow-y: auto;
    }
    .wt-tactics-toolbar__matches .zui-button { justify-content: flex-start; }
    .wt-tactics-toolbar .zui-button[data-chosen] {
        background-color: var(--lui-color-accent-subtle);
        border-color: var(--lui-color-accent);
    }
    .wt-tactics-toolbar__ink {
        width: 16px;
        height: 16px;
        border-radius: 8px;
        border: 1px solid rgb(255, 254, 252);
        background-color: rgb(255, 254, 252);
        flex: none;
    }
    .wt-tactics-toolbar__ink-fill {
        width: 100%;
        height: 100%;
        border-radius: 7px;
        border: 1px solid rgb(23, 23, 21);
    }
    .wt-tactics-toolbar__team-dot {
        width: 12px;
        height: 12px;
        border-radius: 6px;
        border: 1px solid var(--lui-color-border);
        flex: none;
    }
    .wt-tactics-toolbar__nib {
        min-width: 12px;
        text-align: center;
        font-size: var(--zui-type-size-xs);
        color: var(--wt-color-text-dim);
    }
    .wt-tactics-toolbar .zui-spinner {
        width: 12px;
        height: 12px;
    }
    .wt-tactics-panel {
        display: flex;
        flex-direction: column;
        gap: 4px;
        padding: 8px;
    }
    .wt-tactics-panel--ranges { min-width: 190px; }
    .wt-tactics-panel--presets { min-width: 300px; gap: 8px; }
    .wt-tactics-panel--shortcuts { min-width: 220px; }
    .wt-tactics-panel__row {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 4px;
    }
    .wt-tactics-panel__row--between {
        justify-content: space-between;
        gap: 12px;
    }
    .wt-tactics-panel__name { flex: 1 1 auto; min-width: 0; }
    .wt-tactics-panel__list {
        display: flex;
        flex-direction: column;
        gap: 4px;
        max-height: 320px;
        overflow-y: auto;
    }
    .wt-tactics-panel__text { font-size: var(--zui-type-size-xs); }
    .wt-tactics-panel__dim {
        font-size: var(--zui-type-size-xs);
        color: var(--wt-color-text-dim);
    }"
);

/// A vertical rule between two groups of the toolbar.
#[component]
fn Rule() -> impl IntoView {
    view! { box(class = "wt-tactics-toolbar__rule", a11y:hidden = true) }
}

/// A control with a tooltip.
#[component]
fn Tip(#[prop(into)] tip: Signal<String, LocalStorage>, children: Children) -> impl IntoView {
    view! {
        Tooltip {
            TooltipTrigger {
                {children.into_view_once()}
            }
            TooltipContent {{move || tip.get()}}
        }
    }
}

/// Keeps a typed key from reaching the board's chords: a text field's keys are its own.
#[component]
fn Typing(children: Children) -> impl IntoView {
    view! {
        box(class = "wt-tactics-toolbar__typing", on:key_down = |ev| ev.stop_propagation()) {
            {children.into_view_once()}
        }
    }
}

/// The toolbar.
#[component]
pub fn BoardToolbar(model: BoardModel) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let may_steer = Memo::new(move |_| model.with(|board| board.may_steer()));
    let has_map = Memo::new(move |_| model.with(|board| board.map().is_some()));
    let locale = Services::current().locale;

    view! {
        column(class = "wt-tactics-toolbar", attr:data-testid = "tactics-toolbar") {
            row(class = "wt-tactics-toolbar__row") {
                if move || may_steer.get() {
                    row(class = "wt-tactics-toolbar__group wt-tactics-toolbar__group--wide") {
                        text(class = "wt-tactics-toolbar__label") {{locale.text("ui.tactics.map")}}
                        MapPicker(model = model)
                        if move || has_map.get() {
                            Rule()
                            text(class = "wt-tactics-toolbar__label") {{locale.text("ui.tactics.mode")}}
                            ModePicker(model = model)
                        } else {}
                        Rule()
                    }
                } else {}
                ShipPicker(model = model)
                Rule()
                RangeCircles(model = model)
                if move || may_steer.get() {
                    Rule()
                    Presets(model = model)
                    Scan(model = model)
                } else {}
            }
            row(class = "wt-tactics-toolbar__row wt-tactics-toolbar__row--tools") {
                if move || may_steer.get() {
                    CapTools(model = model)
                    Rule()
                } else {}
                DrawTools(model = model)
            }
        }
    }
}

/// The map the board is set on, searched by its title, its space name and its id.
///
/// A popover with a list rather than a combobox: a combobox filters by the text it shows, and the
/// space and id are not shown.
#[component]
fn MapPicker(model: BoardModel) -> impl IntoView {
    let entries = Memo::new(move |_| model.with(|board| map_entries(board.maps())));
    let open = RwSignal::new_local(false);
    let typed = RwSignal::new_local(String::new());
    let chosen = move || {
        model.with(|board| board.map().map(|map| map.key())).and_then(|key| {
            entries
                .with(|entries| entries.iter().find(|entry| entry.choice.key() == key).map(|entry| entry.title.clone()))
        })
    };
    let trigger = move || chosen().unwrap_or_else(|| t!("ui.tactics.map_hint").into_owned());
    let query = Memo::new(move |_| FilterQuery::new(&typed.get()));
    let found = Memo::new(move |_| {
        query.with(|query| {
            entries.with(|entries| MapEntry::rank(entries, query).into_iter().cloned().collect::<Vec<_>>())
        })
    });
    let choose = move |entry: MapEntry| {
        open.set(false);
        typed.set(String::new());
        model.update(|board| board.set_map(entry.choice));
    };

    view! {
        Popover(open = open) {
            PopoverTrigger(
                variant = ButtonVariant::Outline,
                size = ButtonSize::Sm,
                class = "wt-tactics-toolbar__map",
                attr:data-testid = "tactics-map-select"
            ) {
                {trigger}
            }
            PopoverContent(class = "wt-tactics-panel wt-tactics-panel--maps", attr:data-testid = "tactics-map-menu") {
                // Enter takes the first map the search finds, so a reader can pick one without
                // leaving the keyboard.
                box(
                    class = "wt-tactics-toolbar__typing",
                    on:key_down = move |ev| {
                        ev.stop_propagation();
                        if ev.key_without_modifiers == Key::Named(NamedKey::Enter)
                            && let Some(first) = found.with_untracked(|found| found.first().cloned())
                        {
                            choose(first);
                        }
                    }
                ) {
                    Input(
                        value = typed,
                        placeholder = t!("ui.tactics.map_hint").into_owned(),
                        label = t!("ui.tactics.map").into_owned(),
                        class = "wt-tactics-toolbar__map-search",
                        attr:data-testid = "tactics-map-search"
                    )
                }
                column(class = "wt-tactics-toolbar__matches") {
                    for entry in move || found.get(), key = |entry: &MapEntry| entry.choice.key() {
                        {map_match(entry, query, choose)}
                    }
                }
            }
        }
    }
}

/// One map the search found, which a press sets the board on.
fn map_match(entry: MapEntry, query: Memo<FilterQuery>, choose: impl Fn(MapEntry) + Copy + 'static) -> AnyView {
    let label = entry.title.clone();
    AnyView::new(view! {
        Button(variant = ButtonVariant::Ghost, size = ButtonSize::Xs, a11y:label = label.clone(), on:click = move |_| choose(entry.clone())) {
            {move || {
                let ranges = query.with(|query| marks(query, &label));
                view! { MarkedText(text = label.clone(), ranges = ranges) }
            }}
        }
    })
}

/// The value the mode picker's blank entry carries.
const BLANK_MODE: &str = "blank";

/// The mode of the chosen map whose capture points are on the board, or none.
#[component]
fn ModePicker(model: BoardModel) -> impl IntoView {
    let modes = Memo::new(move |_| model.with(|board| board.modes().to_vec()));
    let chosen = Signal::derive_local(move || {
        model.with(|board| board.mode().map_or_else(|| BLANK_MODE.to_owned(), mode_value))
    });
    let choose = move |value: String| {
        if value == BLANK_MODE {
            model.update(|board| board.set_blank_mode());
            return;
        }
        let Some(mode) = modes.get_untracked().into_iter().find(|mode| mode_value(&mode.key) == value) else { return };
        if model.with_untracked(|board| board.mode() != Some(&mode.key)) {
            model.update(|board| board.set_mode(mode.key));
        }
    };

    view! {
        Typing {
            box(class = "wt-tactics-toolbar__mode") {
                Combobox(value = Binding::controlled(chosen, choose)) {
                    ComboboxInput(
                        placeholder = t!("ui.tactics.mode").into_owned(),
                        label = t!("ui.tactics.mode").into_owned(),
                        attr:data-testid = "tactics-mode-select"
                    )
                    ComboboxContent {
                        ComboboxItem(value = BLANK_MODE, text = t!("ui.replay.hover.blank_mode").into_owned()) {
                            {t!("ui.replay.hover.blank_mode").into_owned()}
                        }
                        for mode in move || modes.get(), key = |mode: &wows_toolkit_replay_data::tactics::ModeChoice| mode_value(&mode.key) {
                            ComboboxItem(value = mode_value(&mode.key), text = mode.label.clone()) {
                                {mode.label.clone()}
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Picking a ship to place, and which side it is on.
#[component]
fn ShipPicker(model: BoardModel) -> impl IntoView {
    let locked = Signal::derive_local(move || model.with(|board| board.annotations_locked()));
    let typed = RwSignal::new_local(String::new());
    let query = Memo::new(move |_| FilterQuery::new(&typed.get()));
    let matched: RwSignal<Vec<ShipChoice>, LocalStorage> = RwSignal::new_local(Vec::new());
    let placing =
        Memo::new(move |_| model.with(|board| board.placing().map(|placed| (placed.name.clone(), placed.friendly))));
    let on_change = UnsyncCallback::new(move |text: String| {
        let query = FilterQuery::new(&text);
        matched.set(model.change(|board| board.matching_ships(&query)).unwrap_or_default());
    });
    let pick = move |ship: ShipChoice| {
        model.update(|board| board.pick_ship(&ship));
        matched.set(Vec::new());
        typed.set(String::new());
    };
    let escape = EscapeClears::with(Signal::derive_local(move || typed.with(|text| !text.is_empty())), move || {
        typed.set(String::new());
        matched.set(Vec::new());
    });

    view! {
        column(class = "wt-tactics-toolbar__ships") {
            row(class = "wt-tactics-toolbar__group wt-tactics-toolbar__group--wide") {
                Typing {
                    Input(
                        value = typed,
                        on_change = on_change,
                        placeholder = t!("ui.renderer.annotations.ship_hint").into_owned(),
                        label = t!("ui.renderer.annotations.ship_hint").into_owned(),
                        disabled = locked,
                        class = "wt-tactics-toolbar__search",
                        attr:data-testid = "tactics-ship-search",
                        on:key_down = move |ev| {
                            escape.key_down(ev);
                        }
                    )
                }
                {move || placing.get().map(|(name, friendly)| {
                    let side = if friendly { t!("ui.tactics.side_friendly") } else { t!("ui.tactics.side_enemy") };
                    let side = side.into_owned();
                    AnyView::new(view! {
                        text(class = "wt-tactics-toolbar__label") {{t!("ui.tactics.placing", ship = name).into_owned()}}
                        Button(
                            variant = ButtonVariant::Outline,
                            size = ButtonSize::Xs,
                            disabled = locked,
                            attr:data-testid = "tactics-ship-side",
                            on:click = move |_| model.update(|board| board.set_placing_friendly(!friendly))
                        ) {
                            {side}
                        }
                    })
                })}
            }
            if move || !matched.with(Vec::is_empty) {
                column(class = "wt-tactics-toolbar__matches", attr:data-testid = "tactics-ship-matches") {
                    for ship in move || matched.get(), key = |ship: &ShipChoice| ship.param_index.clone() {
                        {ship_match(ship, query, locked, pick)}
                    }
                }
            } else {}
        }
    }
}

/// One ship the search found, which a press takes up for placing.
fn ship_match(
    ship: ShipChoice,
    query: Memo<FilterQuery>,
    locked: Signal<bool, LocalStorage>,
    pick: impl Fn(ShipChoice) + Copy + 'static,
) -> AnyView {
    let label = format!("{} {}", tier_roman(ship.tier), ship.display_name);
    let name = ship.display_name.clone();
    AnyView::new(view! {
        Button(
            variant = ButtonVariant::Ghost,
            size = ButtonSize::Xs,
            disabled = locked,
            a11y:label = label.clone(),
            on:click = move |_| pick(ship.clone())
        ) {
            {move || {
                let ranges = query.with(|query| marks_within(query, &label, &name));
                view! { MarkedText(text = label.clone(), ranges = ranges) }
            }}
        }
    })
}

/// Which range circles the placed ships show.
#[component]
fn RangeCircles(model: BoardModel) -> impl IntoView {
    let locked = Signal::derive_local(move || model.with(|board| board.annotations_locked()));
    let filters = Memo::new(move |_| model.with(|board| (board.range_filter().clone(), board.ship_filters())));
    let active = move || {
        filters.with(|(filter, ships)| {
            RANGE_CIRCLES
                .iter()
                .filter(|(circle, _)| circle.is_on(filter) || ships.iter().any(|ship| circle.is_on(ship)))
                .count()
        })
    };
    let all_on = Signal::derive_local(move || {
        filters.with(|(filter, ships)| {
            RANGE_CIRCLES.iter().all(|(circle, _)| circle.is_on(filter) && ships.iter().all(|ship| circle.is_on(ship)))
        })
    });
    let all_off = Signal::derive_local(move || {
        filters.with(|(filter, ships)| {
            RANGE_CIRCLES
                .iter()
                .all(|(circle, _)| !circle.is_on(filter) && ships.iter().all(|ship| !circle.is_on(ship)))
        })
    });
    // A memo, so the popover is rebuilt only when the lock changes.
    let held = Memo::new(move |_| locked.get());
    let label = move || format!("{} ({})", t!("ui.renderer.context.ranges"), active());

    view! {
        if move || held.get() {
            Button(variant = ButtonVariant::Outline, size = ButtonSize::Xs, disabled = true, attr:data-testid = "tactics-ranges") {
                {label}
            }
        } else {
            Popover {
                PopoverTrigger(variant = ButtonVariant::Outline, size = ButtonSize::Xs, attr:data-testid = "tactics-ranges") {
                    {label}
                }
                PopoverContent(class = "wt-tactics-panel wt-tactics-panel--ranges", attr:data-testid = "tactics-ranges-menu") {
                    row(class = "wt-tactics-panel__row") {
                        Button(
                            variant = ButtonVariant::Outline,
                            size = ButtonSize::Xs,
                            disabled = all_on,
                            attr:data-testid = "tactics-ranges-enable-all",
                            on:click = move |_| model.update(|board| board.set_all_range_circles(true))
                        ) {
                            {t!("ui.renderer.context.enable_all").into_owned()}
                        }
                        Button(
                            variant = ButtonVariant::Outline,
                            size = ButtonSize::Xs,
                            disabled = all_off,
                            attr:data-testid = "tactics-ranges-disable-all",
                            on:click = move |_| model.update(|board| board.set_all_range_circles(false))
                        ) {
                            {t!("ui.renderer.context.disable_all").into_owned()}
                        }
                    }
                    for entry in move || RANGE_CIRCLES.to_vec(), key = |entry: &(RangeCircle, &str)| entry.0 as u8 {
                        RangeCheck(model = model, circle = entry.0, key = entry.1, locked = locked)
                    }
                }
            }
        }
    }
}

/// One circle's checkbox, counting the ships that show it when some do and some do not.
#[component]
fn RangeCheck(
    model: BoardModel,
    circle: RangeCircle,
    key: &'static str,
    locked: Signal<bool, LocalStorage>,
) -> impl IntoView {
    let counts = Memo::new(move |_| {
        model.with(|board| {
            let ships = board.ship_filters();
            let enabled = usize::from(circle.is_on(board.range_filter()))
                + ships.iter().filter(|ship| circle.is_on(ship)).count();
            (enabled, ships.len() + 1)
        })
    });
    let checked = Signal::derive_local(move || {
        let (enabled, total) = counts.get();
        if enabled == total { Checked::Yes } else { Checked::No }
    });
    let set = move |checked: Checked| {
        let on = checked == Checked::Yes;
        model.update(|board| board.set_range_circles(&[circle], on));
    };
    let label = move || {
        let (enabled, total) = counts.get();
        if enabled > 0 && enabled < total { format!("{} ({enabled}/{total})", t!(key)) } else { t!(key).into_owned() }
    };

    let control = NodeRef::new();
    let toggle = move || {
        if !locked.get_untracked() {
            set(Checked::from(checked.get_untracked() != Checked::Yes));
        }
    };

    view! {
        row(class = "wt-tactics-panel__row") {
            Checkbox(
                checked = Binding::controlled(checked, set),
                disabled = locked,
                node_ref = control,
                attr:data-testid = format!("tactics-range-{}", circle as u8),
                a11y:label = t!(key).into_owned()
            )
            Label(
                control = control,
                class = "wt-tactics-panel__text",
                attr:data-testid = format!("tactics-range-{}-name", circle as u8),
                on:click = move |_| toggle()
            ) {
                {label}
            }
        }
    }
}

/// Saving the board, and opening one that was saved.
#[component]
fn Presets(model: BoardModel) -> impl IntoView {
    let name = RwSignal::new_local(String::new());
    let presets = Memo::new(move |_| model.with(|board| board.presets().to_vec()));
    let locked = Signal::derive_local(move || model.with(|board| board.annotations_locked()));
    let can_reset = Signal::derive_local(move || model.with(|board| board.may_steer() && !board.annotations_locked()));
    let can_save = Signal::derive_local(move || {
        let named = !name.with(|name| name.trim().is_empty());
        model.with(|board| board.may_steer() && board.map().is_some()) && named
    });
    let trigger = move || {
        let presets = presets.get();
        if presets.is_empty() {
            t!("ui.tactics.presets").into_owned()
        } else {
            format!("{} ({})", t!("ui.tactics.presets"), presets.len())
        }
    };

    view! {
        Popover {
            PopoverTrigger(variant = ButtonVariant::Outline, size = ButtonSize::Xs, attr:data-testid = "tactics-presets") {
                {trigger}
            }
            PopoverContent(class = "wt-tactics-panel wt-tactics-panel--presets", attr:data-testid = "tactics-presets-menu") {
                Tip(tip = t!("ui.tactics.reset_board_tooltip").into_owned()) {
                    Button(
                        variant = ButtonVariant::Outline,
                        size = ButtonSize::Xs,
                        disabled = Signal::derive_local(move || !can_reset.get()),
                        attr:data-testid = "tactics-preset-reset",
                        on:click = move |_| model.update(|board| board.reset_board())
                    ) {
                        {t!("ui.tactics.reset_board").into_owned()}
                    }
                }
                row(class = "wt-tactics-panel__row") {
                    Typing {
                        Input(
                            value = name,
                            placeholder = t!("ui.tactics.preset_name").into_owned(),
                            label = t!("ui.tactics.preset_name").into_owned(),
                            class = "wt-tactics-toolbar__search",
                            attr:data-testid = "tactics-preset-name"
                        )
                    }
                    Button(
                        variant = ButtonVariant::Outline,
                        size = ButtonSize::Xs,
                        disabled = Signal::derive_local(move || !can_save.get()),
                        attr:data-testid = "tactics-preset-save",
                        on:click = move |_| {
                            let typed = name.get_untracked();
                            model.update(|board| board.save_preset(&typed));
                        }
                    ) {
                        {t!("ui.tactics.preset_save").into_owned()}
                    }
                }
                if move || !presets.with(Vec::is_empty) {
                    Separator()
                    column(class = "wt-tactics-panel__list", attr:data-testid = "tactics-preset-list") {
                        for preset in move || presets.get(), key = |preset: &String| preset.clone() {
                            {preset_row(model, preset, locked)}
                        }
                    }
                } else {}
            }
        }
    }
}

/// One saved board, with what opens it and what drops it.
fn preset_row(model: BoardModel, name: String, locked: Signal<bool, LocalStorage>) -> AnyView {
    let open = name.clone();
    let drop = name.clone();
    AnyView::new(view! {
        row(class = "wt-tactics-panel__row wt-tactics-panel__row--between") {
            text(class = "wt-tactics-panel__name wt-tactics-panel__text") {{name}}
            row(class = "wt-tactics-panel__row") {
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Xs,
                    disabled = locked,
                    on:click = move |_| model.update(|board| board.load_preset(&open))
                ) {
                    {t!("ui.tactics.preset_open").into_owned()}
                }
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Xs,
                    on:click = move |_| model.update(|board| board.delete_preset(&drop))
                ) {
                    {t!("ui.tactics.preset_delete").into_owned()}
                }
            }
        }
    })
}

/// Reading the replays for the modes they were played in.
#[component]
fn Scan(model: BoardModel) -> impl IntoView {
    let scanning = Memo::new(move |_| model.with(|board| board.scanning()));

    move || match scanning.get() {
        Some(progress) => AnyView::new(view! {
            row(class = "wt-tactics-toolbar__group wt-tactics-toolbar__group--wide", attr:data-testid = "tactics-scanning") {
                Spinner(label = t!("ui.tactics.scan_running").into_owned())
                text(class = "wt-tactics-toolbar__label") {{match progress.total {
                    Some(total) => t!("ui.tactics.scan_progress", read = progress.read, total = total).into_owned(),
                    None => t!("ui.tactics.scan_running").into_owned(),
                }}}
            }
        }),
        None => AnyView::new(view! {
            Tip(tip = t!("ui.tactics.scan_replays_tooltip").into_owned()) {
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Xs,
                    attr:data-testid = "tactics-scan",
                    on:click = move |_| model.scan_replays()
                ) {
                    {t!("ui.tactics.scan_replays").into_owned()}
                }
            }
        }),
    }
}

/// What can be done to the capture points on the board.
#[component]
fn CapTools(model: BoardModel) -> impl IntoView {
    let adding = Memo::new(move |_| model.with(|board| board.adding()));
    let selected = Memo::new(move |_| model.with(|board| board.selected_cap().cloned()));
    let removable = Memo::new(move |_| model.with(|board| board.caps().iter().any(|cap| !cap.frozen)));

    view! {
        row(class = "wt-tactics-toolbar__group") {
            Tip(tip = t!("ui.tactics.add_cap_tooltip").into_owned()) {
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Xs,
                    attr:data-chosen = move || adding.get().then(|| "tactics-add-cap".to_owned()),
                    attr:data-testid = "tactics-add-cap",
                    on:click = move |_| model.update(|board| {
                        let adding = !board.adding();
                        board.set_adding(adding);
                    })
                ) {
                    {t!("ui.tactics.add_cap").into_owned()}
                }
            }
            {move || selected.get().map(|cap| {
                let side = cap.side();
                let color = team_color(cap.team);
                let dot = format!("rgb({}, {}, {})", color[0], color[1], color[2]);
                let team_label = t!(side.label_key()).into_owned();
                let frozen = cap.frozen;
                AnyView::new(view! {
                    text(class = "wt-tactics-toolbar__label", attr:data-testid = "tactics-selected-cap") {
                        {t!("ui.tactics.selected_cap", letter = cap.letter()).into_owned()}
                    }
                    text(class = "wt-tactics-toolbar__label") {
                        {t!("ui.tactics.cap_radius", km = format!("{:.1}", radius_km(cap.radius))).into_owned()}
                    }
                    Tip(tip = t!("ui.tactics.cap_narrow_tooltip").into_owned()) {
                        Button(
                            variant = ButtonVariant::Outline,
                            size = ButtonSize::Xs,
                            disabled = cap.radius <= MIN_CAP_RADIUS,
                            a11y:label = t!("ui.tactics.cap_narrow_tooltip").into_owned(),
                            attr:data-testid = "tactics-cap-narrow",
                            on:click = move |_| model.update(|board| board.step_selected_radius(-RADIUS_STEP_KM))
                        ) {"-"}
                    }
                    Tip(tip = t!("ui.tactics.cap_widen_tooltip").into_owned()) {
                        Button(
                            variant = ButtonVariant::Outline,
                            size = ButtonSize::Xs,
                            disabled = cap.radius >= MAX_CAP_RADIUS,
                            a11y:label = t!("ui.tactics.cap_widen_tooltip").into_owned(),
                            attr:data-testid = "tactics-cap-widen",
                            on:click = move |_| model.update(|board| board.step_selected_radius(RADIUS_STEP_KM))
                        ) {"+"}
                    }
                    row(class = "wt-tactics-toolbar__group") {
                        text(class = "wt-tactics-toolbar__label") {{t!("ui.tactics.cap_team").into_owned()}}
                        Tip(tip = t!("ui.tactics.cap_team_tooltip").into_owned()) {
                            Button(
                                variant = ButtonVariant::Outline,
                                size = ButtonSize::Xs,
                                a11y:label = format!("{}: {team_label}", t!("ui.tactics.cap_team")),
                                attr:data-testid = "tactics-cap-team",
                                on:click = move |_| model.update(|board| board.cycle_selected_team())
                            ) {
                                box(class = "wt-tactics-toolbar__team-dot", style:background-color = dot)
                                text {{team_label.clone()}}
                            }
                        }
                    }
                    Tip(tip = if frozen {
                        t!("ui.tactics.cap_replay_delete_tooltip").into_owned()
                    } else {
                        t!("ui.tactics.delete_cap_tooltip").into_owned()
                    }) {
                        Button(
                            variant = ButtonVariant::Outline,
                            size = ButtonSize::Xs,
                            disabled = frozen,
                            attr:data-testid = "tactics-cap-delete",
                            on:click = move |_| model.update(|board| board.remove_selected())
                        ) {
                            {t!("ui.tactics.delete_cap").into_owned()}
                        }
                    }
                })
            })}
            if move || removable.get() {
                Tip(tip = t!("ui.tactics.clear_added_caps_tooltip").into_owned()) {
                    Button(
                        variant = ButtonVariant::Outline,
                        size = ButtonSize::Xs,
                        attr:data-testid = "tactics-clear-caps",
                        on:click = move |_| model.update(|board| board.clear_caps())
                    ) {
                        {t!("ui.tactics.clear_caps").into_owned()}
                    }
                }
            } else {}
        }
    }
}

/// What can be drawn on the board, and in what.
#[component]
fn DrawTools(model: BoardModel) -> impl IntoView {
    let locked = Signal::derive_local(move || model.with(|board| board.annotations_locked()));
    let nib = Memo::new(move |_| model.with(|board| board.nib()));
    let can_undo = Signal::derive_local(move || model.with(|board| board.can_undo() && !board.annotations_locked()));
    let can_redo = Signal::derive_local(move || model.with(|board| board.can_redo() && !board.annotations_locked()));
    let has_drawing = Memo::new(move |_| model.with(|board| board.annotation_count() > 0));
    let modifier = shortcut_modifier_name();

    view! {
        row(class = "wt-tactics-toolbar__group") {
            if move || locked.get() {
                AppIcon(glyph = Glyph::Lock, attr:data-testid = "tactics-locked")
            } else {}
            for tool in move || BoardTool::ALL.to_vec(), key = |tool: &BoardTool| *tool as u8 {
                ToolButton(model = model, tool = tool, locked = locked)
            }
            Rule()
            for entry in move || INKS.into_iter().enumerate().collect::<Vec<_>>(), key = |entry: &(usize, ([u8; 4], &str))| entry.0 {
                InkButton(model = model, index = entry.0, ink = entry.1.0, key = entry.1.1, locked = locked)
            }
            Rule()
            row(class = "wt-tactics-toolbar__group") {
                text(class = "wt-tactics-toolbar__label") {{t!("ui.renderer.annotations.nib").into_owned()}}
                Tip(tip = t!("ui.renderer.annotations.thinner").into_owned()) {
                    Button(
                        variant = ButtonVariant::Outline,
                        size = ButtonSize::Xs,
                        disabled = Signal::derive_local(move || locked.get() || nib.get() <= MIN_NIB),
                        a11y:label = t!("ui.renderer.annotations.thinner").into_owned(),
                        attr:data-testid = "tactics-nib-down",
                        on:click = move |_| model.update(|board| board.step_nib(-1.0))
                    ) {"-"}
                }
                text(class = "wt-tactics-toolbar__nib", attr:data-testid = "tactics-nib") {{move || format!("{:.0}", nib.get())}}
                Tip(tip = t!("ui.renderer.annotations.thicker").into_owned()) {
                    Button(
                        variant = ButtonVariant::Outline,
                        size = ButtonSize::Xs,
                        disabled = Signal::derive_local(move || locked.get() || nib.get() >= MAX_NIB),
                        a11y:label = t!("ui.renderer.annotations.thicker").into_owned(),
                        attr:data-testid = "tactics-nib-up",
                        on:click = move |_| model.update(|board| board.step_nib(1.0))
                    ) {"+"}
                }
            }
            Rule()
            Shortcuts()
            Rule()
            Tip(tip = t!("ui.tactics.undo_tooltip", shortcut = format!("{modifier}+Z")).into_owned()) {
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Xs,
                    disabled = Signal::derive_local(move || !can_undo.get()),
                    attr:data-testid = "tactics-undo",
                    on:click = move |_| model.update(|board| board.undo())
                ) {
                    {t!("ui.renderer.annotations.undo").into_owned()}
                }
            }
            Tip(tip = t!(
                "ui.tactics.redo_tooltip",
                shortcut = format!("{modifier}+Y"),
                alternate = format!("{modifier}+Shift+Z")
            ).into_owned()) {
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Xs,
                    disabled = Signal::derive_local(move || !can_redo.get()),
                    attr:data-testid = "tactics-redo",
                    on:click = move |_| model.update(|board| board.redo())
                ) {
                    {t!("ui.renderer.annotations.redo").into_owned()}
                }
            }
            if move || has_drawing.get() {
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Xs,
                    disabled = locked,
                    attr:data-testid = "tactics-clear-drawing",
                    on:click = move |_| model.update(|board| board.clear_annotations())
                ) {
                    {t!("ui.tactics.clear_drawing").into_owned()}
                }
            } else {}
        }
    }
}

/// The icon a tool's button shows.
fn tool_glyph(tool: BoardTool) -> Glyph {
    match tool {
        BoardTool::Freehand => Glyph::Paintbrush,
        BoardTool::Line => Glyph::Slash,
        BoardTool::Arrow => Glyph::MoveUpRight,
        BoardTool::Circle => Glyph::Circle,
        BoardTool::Rectangle => Glyph::Square,
        BoardTool::Triangle => Glyph::Triangle,
        BoardTool::Measurement => Glyph::Ruler,
        BoardTool::Eraser => Glyph::Eraser,
    }
}

/// One drawing tool, shown taken up while it is in hand.
#[component]
fn ToolButton(model: BoardModel, tool: BoardTool, locked: Signal<bool, LocalStorage>) -> impl IntoView {
    let chosen = Memo::new(move |_| model.with(|board| *board.tool() == tool.tool()));
    let label = t!(tool.label_key()).into_owned();
    let tip = format!("{label} ({}+{})", shortcut_modifier_name(), tool.shortcut());

    view! {
        Tip(tip = tip) {
            Button(
                variant = ButtonVariant::Ghost,
                size = ButtonSize::IconSm,
                disabled = locked,
                a11y:label = label,
                attr:data-chosen = move || chosen.get().then(|| format!("tactics-tool-{}", tool as u8)),
                attr:data-testid = format!("tactics-tool-{}", tool as u8),
                on:click = move |_| model.update(|board| board.set_tool(tool.tool()))
            ) {
                AppIcon(glyph = tool_glyph(tool))
            }
        }
    }
}

/// One ink, shown as a swatch.
#[component]
fn InkButton(
    model: BoardModel,
    index: usize,
    ink: [u8; 4],
    key: &'static str,
    locked: Signal<bool, LocalStorage>,
) -> impl IntoView {
    let chosen = Memo::new(move |_| model.with(|board| board.ink() == ink));
    let fill = format!("rgb({}, {}, {})", ink[0], ink[1], ink[2]);
    let label = t!(key).into_owned();

    view! {
        Tip(tip = label.clone()) {
            Button(
                variant = ButtonVariant::Ghost,
                size = ButtonSize::IconSm,
                disabled = locked,
                a11y:label = label,
                attr:data-chosen = move || chosen.get().then(|| format!("tactics-ink-{index}")),
                attr:data-testid = format!("tactics-ink-{index}"),
                on:click = move |_| model.update(|board| board.set_ink(ink))
            ) {
                box(class = "wt-tactics-toolbar__ink") {
                    box(class = "wt-tactics-toolbar__ink-fill", style:background-color = fill)
                }
            }
        }
    }
}

/// The drawing chords in one place, for a reader who does not hover each tool.
#[component]
fn Shortcuts() -> impl IntoView {
    let heading = t!("ui.renderer.annotations.shortcuts_title").into_owned();
    let tip = heading.clone();
    let label = heading.clone();

    view! {
        Popover {
            Tip(tip = tip) {
                PopoverTrigger(
                    variant = ButtonVariant::Ghost,
                    size = ButtonSize::IconSm,
                    a11y:label = label,
                    attr:data-testid = "tactics-shortcuts"
                ) {
                    AppIcon(glyph = Glyph::Info)
                }
            }
            PopoverContent(class = "wt-tactics-panel wt-tactics-panel--shortcuts", attr:data-testid = "tactics-shortcuts-menu") {
                text(class = "wt-tactics-panel__text") {{heading.clone()}}
                text(class = "wt-tactics-panel__dim") {{t!("ui.renderer.annotations.shortcuts_tools").into_owned()}}
                {shortcut_tools()}
                text(class = "wt-tactics-panel__dim") {{t!("ui.renderer.annotations.shortcuts_actions").into_owned()}}
                {shortcut_actions()}
            }
        }
    }
}

/// One chord and what it does.
fn shortcut_line(chord: String, label: String) -> AnyView {
    AnyView::new(view! {
        row(class = "wt-tactics-panel__row wt-tactics-panel__row--between") {
            text(class = "wt-tactics-panel__text") {{chord}}
            text(class = "wt-tactics-panel__dim") {{label}}
        }
    })
}

fn shortcut_tools() -> Vec<AnyView> {
    let modifier = shortcut_modifier_name();
    BoardTool::ALL
        .into_iter()
        .map(|tool| shortcut_line(format!("{modifier}+{}", tool.shortcut()), t!(tool.label_key()).into_owned()))
        .collect()
}

fn shortcut_actions() -> Vec<AnyView> {
    let modifier = shortcut_modifier_name();
    vec![
        shortcut_line(format!("{modifier}+Z"), t!("ui.renderer.annotations.action_undo").into_owned()),
        shortcut_line(format!("{modifier}+Y / {modifier}+Shift+Z"), t!("ui.renderer.annotations.redo").into_owned()),
        shortcut_line(format!("{modifier}+Click"), t!("ui.renderer.annotations.action_multi_select").into_owned()),
        shortcut_line("[ / ]".to_owned(), t!("ui.renderer.annotations.action_stroke_width").into_owned()),
        shortcut_line("Del".to_owned(), t!("ui.renderer.annotations.action_delete").into_owned()),
        shortcut_line("Esc".to_owned(), t!("ui.renderer.annotations.action_cancel").into_owned()),
    ]
}
