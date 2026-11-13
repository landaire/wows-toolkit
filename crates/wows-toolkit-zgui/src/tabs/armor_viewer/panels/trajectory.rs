//! The options rail's Trajectory section: what the next cast is fired with, a coloured marker for
//! each cast arc, and the editor of the arc a marker selects.
//!
//! A shell is cast by Shift-clicking the hull, so nothing here switches a mode on. A marker carries
//! the colour its arc is drawn in, which is how a marker and a line on the hull are matched.

use landaire_ui::SliderFieldProps;
use landaire_ui::SliderRange;
use landaire_ui::TooltipContentProps;
use landaire_ui::TooltipProps;
use landaire_ui::TooltipTriggerProps;
use rust_i18n::t;
use wows_toolkit_armor::trajectory::ArcId;
use wows_toolkit_icons::Glyph;
use wows_toolkit_viewmodel::armor::arc::ArcOutcome;
use wows_toolkit_viewmodel::armor::arc::StoppingPlate;
use wows_toolkit_viewmodel::armor::penetration::ComparisonShip;
use wowsunpack::game_params::types::Km;
use wowsunpack::game_params::types::ShellInfo;
use zgui::component;
use zgui::css;
use zgui::elements::Focus;
use zgui::reactive::Callable;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithValue;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;
use zgui::vocab::PointerButton;
use zgui::vocab::Role;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::CheckboxProps;
use zgui_ui::Checked;
use zgui_ui::DropdownMenuContentProps;
use zgui_ui::DropdownMenuProps;
use zgui_ui::DropdownMenuTriggerProps;
use zgui_ui::MenuCheckboxItemProps;
use zgui_ui::ToggleProps;
use zgui_ui::ToggleSize;
use zgui_ui_primitives::Binding;

use crate::icons::AppIconProps;
use crate::services::Services;
use crate::tabs::armor_viewer::panels::legend::swatch_css;
use crate::tabs::armor_viewer::panels::pen::PenetrationState;
use crate::tabs::armor_viewer::viewport::ArcSummary;
use crate::tabs::armor_viewer::viewport::ArmorViewportState;
use crate::tabs::armor_viewer::viewport::CastShell;
use crate::tabs::armor_viewer::viewport::Isolate;

const SHEET: &str = "wt-armor-trajectory";

const CSS: &str = css!(
    ".wt-armor-trajectory { display: flex; flex-direction: column; gap: 12px; min-width: 0; }
    .wt-armor-trajectory__group { display: flex; flex-direction: column; gap: 4px; }
    .wt-armor-trajectory__heading { font-size: 12px; font-weight: 600; color: var(--wt-color-text-dim); }
    .wt-armor-trajectory__hint { font-size: 12px; color: var(--wt-color-text-dim); }
    .wt-armor-trajectory__caveat { font-size: 12px; color: var(--wt-color-warn); }
    .wt-armor-trajectory__pickers { display: flex; flex-direction: column; align-items: flex-start; gap: 4px;
        min-width: 0; }
    .wt-armor-trajectory__picker { max-width: 220px; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
    .wt-armor-trajectory__check { display: flex; align-items: center; gap: 6px; font-size: 12px; }
    .wt-armor-trajectory__row { display: flex; align-items: center; gap: 4px; }
    .wt-armor-trajectory__markers { display: flex; flex-wrap: wrap; align-items: center; gap: 4px; }
    .wt-armor-trajectory__spacer { flex: 1; }
    .wt-armor-trajectory__marker { flex: none; display: flex; align-items: center; justify-content: center;
        width: 22px; height: 22px; border-radius: 11px; border: 2px solid #00000099; font-size: 12px;
        font-weight: 700; color: #000000; cursor: pointer; }
    .wt-armor-trajectory__marker[data-selected] { border-color: var(--lui-color-text); }
    .wt-armor-trajectory__editor { display: flex; flex-direction: column; gap: 4px; padding: 8px;
        border-radius: 6px; border: 2px solid; }
    .wt-armor-trajectory__editor-head { display: flex; align-items: center; gap: 8px; }
    .wt-armor-trajectory__dot { flex: none; width: 12px; height: 12px; border-radius: 6px; }
    .wt-armor-trajectory__title { font-size: 14px; font-weight: 700; }
    .wt-armor-trajectory__summary { flex: 1; min-width: 0; font-size: 12px; color: var(--wt-color-text-dim);
        overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
    .wt-armor-trajectory__outcome { font-size: 12px; }
    .wt-armor-trajectory__outcome[data-tone=pen] { color: var(--wt-color-armor-pen); }
    .wt-armor-trajectory__outcome[data-tone=overpen] { color: var(--wt-color-armor-overpen); }
    .wt-armor-trajectory__outcome[data-tone=ricochet] { color: var(--wt-color-armor-ricochet); }
    .wt-armor-trajectory__outcome[data-tone=shatter] { color: var(--wt-color-armor-shatter); }
    .wt-armor-trajectory__outcome[data-tone=dim] { color: var(--wt-color-text-dim); }
    .wt-armor-trajectory__legend { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; }
    .wt-armor-trajectory__key { display: flex; align-items: center; gap: 4px; font-size: 12px;
        color: var(--wt-color-text-dim); }
    .wt-armor-trajectory__key-dot { width: 8px; height: 8px; border-radius: 4px; }
    .wt-armor-trajectory__key-dot[data-angle=good] { background-color: var(--wt-color-armor-angle-good); }
    .wt-armor-trajectory__key-dot[data-angle=mid] { background-color: var(--wt-color-armor-angle-mid); }
    .wt-armor-trajectory__key-dot[data-angle=bad] { background-color: var(--wt-color-armor-angle-bad); }"
);

/// The span a cast range is chosen over, in kilometres: the egui viewer's own range slider.
const CAST_RANGE_SLIDER: SliderRange = SliderRange::new(0.0, 30.0, 0.5);

/// The range a cast is fired from until one is chosen, where the reset button puts it.
const DEFAULT_CAST_RANGE: f64 = 10.0;

/// The section body.
#[component]
pub fn TrajectoryPanel(
    pen: RwSignal<PenetrationState, LocalStorage>,
    viewport: ArmorViewportState,
    /// Opens the penetration checker, which is where ships are added.
    #[prop(optional)]
    on_open_pen_check: Option<UnsyncCallback<()>>,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let locale = Services::current().locale;
    let continuing =
        Signal::derive_local(
            move || {
                if viewport.trajectory_state().continue_on_ricochet { Checked::Yes } else { Checked::No }
            },
        );
    let next_range = Binding::controlled(
        Signal::derive_local(move || f64::from(viewport.cast_range().value())),
        move |range: f64| viewport.set_cast_range(Km::new(range as f32)),
    );
    let range_label = t!("ui.armor.trajectory_range").into_owned();

    let markers = move || markers(viewport);
    let editor = move || editor(pen, viewport);

    view! {
        column(class = "wt-armor-trajectory", attr:data-testid = "armor-trajectory") {
            column(class = "wt-armor-trajectory__group") {
                text(class = "wt-armor-trajectory__heading") {{locale.text("ui.armor.trajectory_next_cast")}}
                FiringControls(pen = pen, on_open_pen_check = on_open_pen_check)
                SliderField(
                    value = next_range,
                    range = CAST_RANGE_SLIDER,
                    reset_to = DEFAULT_CAST_RANGE,
                    label = range_label,
                    attr:data-testid = "armor-trajectory-range"
                )
                row(class = "wt-armor-trajectory__check") {
                    Checkbox(
                        checked = Binding::controlled(continuing, move |checked: Checked| {
                            viewport.set_continue_on_ricochet(checked == Checked::Yes)
                        }),
                        attr:data-testid = "armor-trajectory-ricochet",
                        a11y:label = locale.text("ui.armor.continue_ricochet")
                    )
                    text {{locale.text("ui.armor.continue_ricochet")}}
                }
            }
            {markers}
            {editor}
        }
    }
}

/// The attacker and ammunition the next cast is fired with. With no ship in the comparison list
/// there is nothing to choose from, and the button opens the penetration checker instead.
#[component]
fn FiringControls(
    pen: RwSignal<PenetrationState, LocalStorage>,
    on_open_pen_check: Option<UnsyncCallback<()>>,
) -> impl IntoView {
    let open_pen_check = StoredValue::new_local(on_open_pen_check);

    let body = move || {
        let (ships, firing, shell_index) =
            pen.with(|pen| (pen.ships.clone(), pen.firing_ship.clone(), pen.firing_shell));
        if ships.is_empty() {
            let tip = t!("ui.armor.trajectory_add_ship_tooltip").into_owned();
            return AnyView::new(view! {
                Tooltip {
                    TooltipTrigger {
                        Button(
                            variant = ButtonVariant::Outline,
                            size = ButtonSize::Sm,
                            attr:data-testid = "armor-trajectory-add-ship",
                            on:click = move |_| {
                                if let Some(open) = open_pen_check.get_value() {
                                    open.run(());
                                }
                            }
                        ) {
                            {t!("ui.armor.trajectory_add_ship").into_owned()}
                        }
                    }
                    TooltipContent {{tip.clone()}}
                }
            });
        }

        let selected = ships.iter().find(|ship| Some(ship.param_index.as_str()) == firing.as_deref()).cloned();
        let ship_label = selected.as_ref().map_or_else(
            || t!("ui.armor.trajectory_select_ship").into_owned(),
            |ship| format!("{} {}", t!("ui.armor.trajectory_ship"), ship.display_name),
        );
        let cast = selected.map(|ship| CastShell { ship, shell_index });
        let active_shell = cast.as_ref().and_then(CastShell::fired_index);
        let shell_label = cast
            .as_ref()
            .and_then(CastShell::shell)
            .map(|shell| format!("{} {}", t!("ui.armor.trajectory_shell"), shell_name(shell)))
            .unwrap_or_else(|| t!("ui.armor.trajectory_select_shell").into_owned());
        let shells = cast.map(|cast| cast.ship.shells).unwrap_or_default();
        let no_shells = shells.is_empty();

        // Built each time a menu opens its content, from data held here so the builders are `Copy`.
        let held = StoredValue::new_local((ships, firing, shells));
        let ship_items = move || -> Vec<AnyView> {
            held.with_value(|(ships, firing, _)| {
                ships
                    .iter()
                    .map(|ship| {
                        let active = Some(ship.param_index.as_str()) == firing.as_deref();
                        let param_index = ship.param_index.clone();
                        menu_item(ship.display_name.clone(), active, move || {
                            pen.update(|pen| pen.set_firing_ship(&param_index))
                        })
                    })
                    .collect()
            })
        };
        let shell_items = move || -> Vec<AnyView> {
            held.with_value(|(_, _, shells)| {
                shells
                    .iter()
                    .enumerate()
                    .map(|(index, shell)| {
                        menu_item(shell_name(shell), Some(index) == active_shell, move || {
                            pen.update(|pen| pen.set_firing_shell(index))
                        })
                    })
                    .collect()
            })
        };
        let ship_tip = format!("{}\n{}", ship_label, t!("ui.armor.trajectory_ship_tooltip"));
        let shell_tip = t!("ui.armor.trajectory_shell_tooltip").into_owned();

        AnyView::new(view! {
            column(class = "wt-armor-trajectory__pickers") {
                Tooltip {
                    TooltipTrigger {
                        DropdownMenu {
                            DropdownMenuTrigger(
                                class = "wt-armor-trajectory__picker",
                                variant = ButtonVariant::Outline,
                                size = ButtonSize::Sm,
                                attr:data-testid = "armor-trajectory-ship"
                            ) {
                                {ship_label.clone()}
                            }
                            DropdownMenuContent {
                                {ship_items}
                            }
                        }
                    }
                    TooltipContent {{ship_tip.clone()}}
                }
                Tooltip {
                    TooltipTrigger {
                        DropdownMenu {
                            DropdownMenuTrigger(
                                class = "wt-armor-trajectory__picker",
                                variant = ButtonVariant::Outline,
                                size = ButtonSize::Sm,
                                disabled = no_shells,
                                attr:data-testid = "armor-trajectory-shell"
                            ) {
                                {shell_label.clone()}
                            }
                            DropdownMenuContent {
                                {shell_items}
                            }
                        }
                    }
                    TooltipContent {{shell_tip.clone()}}
                }
            }
        })
    };

    view! {
        {body}
    }
}

/// A menu entry ticked while it is the one in use.
fn menu_item(label: String, active: bool, choose: impl Fn() + 'static) -> AnyView {
    AnyView::new(view! {
        MenuCheckboxItem(checked = Binding::controlled(Signal::stored_local(active), move |_: bool| choose())) {
            {label}
        }
    })
}

/// One marker per arc, in its colour, with what applies to every arc at once below them.
fn markers(viewport: ArmorViewportState) -> AnyView {
    let arcs = viewport.arcs();
    if arcs.is_empty() {
        return AnyView::new(view! {
            text(class = "wt-armor-trajectory__hint", attr:data-testid = "armor-arc-none") {{t!("ui.armor.arc_none").into_owned()}}
        });
    }
    let selected = viewport.selected_arc().map(|(id, _)| id);
    let markers: Vec<AnyView> =
        arcs.iter().enumerate().map(|(index, arc)| marker(viewport, index, arc, Some(arc.id) == selected)).collect();
    let pick_hint = selected.is_none().then(|| {
        AnyView::new(view! {
            text(class = "wt-armor-trajectory__hint") {{t!("ui.armor.arc_pick_hint").into_owned()}}
        })
    });

    AnyView::new(view! {
        column(class = "wt-armor-trajectory__group") {
            row(class = "wt-armor-trajectory__markers", attr:data-testid = "armor-arc-markers") {
                {markers}
                box(class = "wt-armor-trajectory__spacer")
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Sm,
                    attr:data-testid = "armor-trajectory-clear",
                    on:click = move |_| viewport.clear_trajectories()
                ) {
                    {t!("ui.armor.trajectory_clear").into_owned()}
                }
            }
            {pick_hint}
            row(class = "wt-armor-trajectory__row") {
                {isolate_all(viewport, Isolate::Plates)}
                {isolate_all(viewport, Isolate::Zones)}
            }
            // The simulation is read off the game's own data but is not the game's own code, which
            // is worth saying where its verdicts are read.
            text(class = "wt-armor-trajectory__caveat") {{t!("ui.armor.arc_simulation_caveat").into_owned()}}
            {angle_legend()}
        }
    })
}

/// The label and tooltip keys of an isolation, and its test id stem.
fn isolate_keys(what: Isolate) -> (&'static str, &'static str, &'static str) {
    match what {
        Isolate::Plates => ("ui.armor.arc_isolate_plates", "ui.armor.arc_isolate_plates_tooltip", "plates"),
        Isolate::Zones => ("ui.armor.arc_isolate_zones", "ui.armor.arc_isolate_zones_tooltip", "zones"),
    }
}

/// Isolates what every arc crossed at once, which reads out the whole cast.
fn isolate_all(viewport: ArmorViewportState, what: Isolate) -> AnyView {
    let (label, tip, stem) = isolate_keys(what);
    let tip = t!(tip).into_owned();

    AnyView::new(view! {
        Tooltip {
            TooltipTrigger {
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Sm,
                    attr:data-testid = format!("armor-arcs-isolate-{stem}"),
                    on:click = move |_| viewport.isolate_all_arcs(what)
                ) {
                    {t!(label).into_owned()}
                }
            }
            TooltipContent {{tip.clone()}}
        }
    })
}

/// One arc's marker: its number on its colour. A click opens its editor, or puts the editor away
/// when it is the arc open; a right-click removes it.
fn marker(viewport: ArmorViewportState, index: usize, arc: &ArcSummary, selected: bool) -> AnyView {
    let id = arc.id;
    let number = index + 1;
    let tip = t!("ui.armor.arc_marker_tooltip", number = number).into_owned();
    let label = tip.clone();
    let color = swatch_css(arc.color.rgba());

    AnyView::new(view! {
        Tooltip {
            TooltipTrigger {
                control(
                    class = "wt-armor-trajectory__marker",
                    tabindex = {Focus::Sequential},
                    a11y:role = {Role::Button},
                    a11y:label = label,
                    a11y:selected = selected,
                    attr:data-selected = selected.then(String::new),
                    attr:data-testid = format!("armor-arc-marker-{index}"),
                    style:background-color = color,
                    on:click = move |_| viewport.toggle_arc_selection(id),
                    on:pointer_down = move |ev| {
                        if ev.button == Some(PointerButton::Secondary) {
                            viewport.remove_arc(id);
                        }
                    },
                    on:context_menu = move |ev| ev.prevent_default()
                ) {
                    {number.to_string()}
                }
            }
            TooltipContent {{tip.clone()}}
        }
    })
}

/// The arc a marker selected: its range, the shell it is fired with and what it hides, each
/// applied as it changes; then what became of it.
fn editor(pen: RwSignal<PenetrationState, LocalStorage>, viewport: ArmorViewportState) -> Option<AnyView> {
    let (id, _) = viewport.selected_arc()?;
    let arcs = viewport.arcs();
    let index = arcs.iter().position(|arc| arc.id == id)?;
    let arc = arcs[index].clone();
    let number = index + 1;
    let color = swatch_css(arc.color.rgba());
    let summary = t!(
        "ui.armor.arc_summary",
        hits = arc.hits,
        armor = format!("{:.0}", arc.total_armor.value()),
        range = format!("{:.1}", arc.range.value()),
    )
    .into_owned();
    let (outcome, tone) = (describe_outcome(&arc.outcome, viewport), outcome_tone(&arc.outcome));
    let range = Binding::controlled(
        Signal::derive_local(move || {
            viewport.selected_arc().map_or(DEFAULT_CAST_RANGE, |(_, range)| f64::from(range.value()))
        }),
        move |range: f64| viewport.set_arc_range(id, Km::new(range as f32)),
    );
    let ships = pen.with(|pen| pen.ships.clone());
    let delete_tip = t!("ui.armor.arc_delete").into_owned();
    let delete_label = delete_tip.clone();

    Some(AnyView::new(view! {
        column(
            class = "wt-armor-trajectory__editor",
            attr:data-testid = "armor-arc-editor",
            style:border-color = color.clone()
        ) {
            row(class = "wt-armor-trajectory__editor-head") {
                box(class = "wt-armor-trajectory__dot", style:background-color = color.clone())
                text(class = "wt-armor-trajectory__title") {{t!("ui.armor.arc_heading", number = number).into_owned()}}
                text(class = "wt-armor-trajectory__summary") {{summary}}
                Tooltip {
                    TooltipTrigger {
                        Button(
                            variant = ButtonVariant::Ghost,
                            size = ButtonSize::IconSm,
                            a11y:label = delete_label,
                            attr:data-testid = "armor-arc-delete",
                            on:click = move |_| viewport.remove_arc(id)
                        ) {
                            AppIcon(glyph = Glyph::Trash)
                        }
                    }
                    TooltipContent {{delete_tip.clone()}}
                }
            }
            SliderField(
                value = range,
                range = CAST_RANGE_SLIDER,
                reset_to = DEFAULT_CAST_RANGE,
                label = t!("ui.armor.trajectory_range").into_owned(),
                attr:data-testid = "armor-arc-range"
            )
            row(class = "wt-armor-trajectory__row") {
                {arc_shell_picker(viewport, id, arc.shell.as_ref(), ships)}
            }
            row(class = "wt-armor-trajectory__row") {
                {isolate_one(viewport, id, Isolate::Plates, arc.isolating_plates)}
                {isolate_one(viewport, id, Isolate::Zones, arc.isolating_zones)}
            }
            text(class = "wt-armor-trajectory__outcome", attr:data-tone = tone, attr:data-testid = "armor-arc-outcome") {{outcome}}
        }
    }))
}

/// One arc's isolation, pressed while it is on.
fn isolate_one(viewport: ArmorViewportState, id: ArcId, what: Isolate, on: bool) -> AnyView {
    let (label, tip, stem) = isolate_keys(what);
    let (label, tip) = (t!(label).into_owned(), t!(tip).into_owned());

    AnyView::new(view! {
        Tooltip {
            TooltipTrigger {
                Toggle(
                    size = ToggleSize::Sm,
                    pressed = Binding::controlled(Signal::stored_local(on), move |next: bool| {
                        viewport.isolate_arc(id, what, next)
                    }),
                    label = label.clone(),
                    attr:data-testid = format!("armor-arc-{stem}")
                ) {
                    {label.clone()}
                }
            }
            TooltipContent {{tip.clone()}}
        }
    })
}

/// The shell one arc is fired with, chosen from every shell of every ship being compared.
fn arc_shell_picker(
    viewport: ArmorViewportState,
    id: ArcId,
    held: Option<&CastShell>,
    ships: Vec<ComparisonShip>,
) -> AnyView {
    let label = held
        .and_then(|cast| Some(format!("{} {}", cast.ship.display_name, shell_name(cast.shell()?))))
        .unwrap_or_else(|| t!("ui.armor.trajectory_select_shell").into_owned());
    let held_key = held.and_then(|cast| Some((cast.ship.param_index.clone(), cast.fired_index()?)));
    let disabled = ships.iter().all(|ship| ship.shells.is_empty());
    // Built each time the menu opens its content, from data held here so the builder is `Copy`.
    let choices = StoredValue::new_local((ships, held_key));
    let items = move || -> Vec<AnyView> {
        choices.with_value(|(ships, held_key)| {
            ships
                .iter()
                .flat_map(|ship| {
                    ship.shells.iter().enumerate().map(move |(index, shell)| {
                        let held = held_key
                            .as_ref()
                            .is_some_and(|(param_index, at)| *param_index == ship.param_index && *at == index);
                        let cast = CastShell { ship: ship.clone(), shell_index: Some(index) };
                        let label = format!("{}: {}", ship.display_name, shell_name(shell));
                        menu_item(label, held, move || viewport.set_arc_shell(id, cast.clone()))
                    })
                })
                .collect()
        })
    };
    let tip = t!("ui.armor.trajectory_shell_tooltip").into_owned();

    AnyView::new(view! {
        Tooltip {
            TooltipTrigger {
                DropdownMenu {
                    DropdownMenuTrigger(
                        class = "wt-armor-trajectory__picker",
                        variant = ButtonVariant::Outline,
                        size = ButtonSize::Sm,
                        disabled = disabled,
                        attr:data-testid = "armor-arc-shell"
                    ) {
                        {label}
                    }
                    DropdownMenuContent {
                        {items}
                    }
                }
            }
            TooltipContent {{tip.clone()}}
        }
    })
}

/// A shell as the pickers name it: its ammunition and calibre.
fn shell_name(shell: &ShellInfo) -> String {
    format!("{} {:.0} mm", shell.ammo_type.display_name(), shell.caliber.value())
}

/// What the impact markers' colours mean, which is how square each strike was.
fn angle_legend() -> AnyView {
    let keys: Vec<AnyView> =
        [("good", "ui.armor.angle_good"), ("mid", "ui.armor.angle_mid"), ("bad", "ui.armor.angle_bad")]
            .into_iter()
            .map(|(angle, key)| {
                AnyView::new(view! {
                    row(class = "wt-armor-trajectory__key") {
                        box(class = "wt-armor-trajectory__key-dot", attr:data-angle = angle)
                        text {{t!(key).into_owned()}}
                    }
                })
            })
            .collect();

    AnyView::new(view! {
        row(class = "wt-armor-trajectory__legend") {
            {keys}
        }
    })
}

/// What an outcome reads as.
fn describe_outcome(outcome: &ArcOutcome, viewport: ArmorViewportState) -> String {
    match outcome {
        ArcOutcome::NotSimulated => t!("ui.armor.arc_no_shell").into_owned(),
        ArcOutcome::Detonated { zone } => match zone {
            Some(zone) => t!("ui.armor.arc_detonated_in", zone = zone.clone()).into_owned(),
            None => t!("ui.armor.arc_detonated_past_armor").into_owned(),
        },
        ArcOutcome::Ricocheted { plate } => {
            t!("ui.armor.arc_ricochet", plate = describe_plate(plate, viewport)).into_owned()
        }
        ArcOutcome::Shattered { plate } => {
            t!("ui.armor.arc_shatter", plate = describe_plate(plate, viewport)).into_owned()
        }
        ArcOutcome::Stopped { plate } => {
            t!("ui.armor.arc_stopped", plate = describe_plate(plate, viewport)).into_owned()
        }
        ArcOutcome::Overpenetrated { fuse_armed: true } => t!("ui.armor.arc_overpen").into_owned(),
        ArcOutcome::Overpenetrated { fuse_armed: false } => t!("ui.armor.arc_overpen_unarmed").into_owned(),
    }
}

/// The plate a shell stopped at, or that nothing is known about it, which a cast whose plate list
/// is shorter than its simulation reports.
fn describe_plate(plate: &Option<StoppingPlate>, viewport: ArmorViewportState) -> String {
    match plate {
        Some(plate) => {
            let named = viewport.material_label(plate.material);
            format!("#{} {:.0}mm {}", plate.number, plate.thickness.value(), named)
        }
        None => t!("ui.armor.arc_plate_unknown").into_owned(),
    }
}

/// The tone an outcome reads in: the armor palette the viewers paint their verdicts in, where a
/// detonation is the result the reader was after.
fn outcome_tone(outcome: &ArcOutcome) -> &'static str {
    match outcome {
        ArcOutcome::Detonated { .. } => "pen",
        ArcOutcome::Overpenetrated { .. } => "overpen",
        ArcOutcome::Ricocheted { .. } => "ricochet",
        ArcOutcome::Shattered { .. } | ArcOutcome::Stopped { .. } => "shatter",
        ArcOutcome::NotSimulated => "dim",
    }
}
