//! The options rail's Trajectory section: what the next cast is fired with, a
//! coloured marker for each arc cast, and the editor for the arc a marker
//! selects.
//!
//! A shell is cast by Shift-clicking the hull (`viewport_view`), so nothing
//! here switches a mode on. A marker carries the colour its arc is drawn in,
//! which is how a marker and a line on the hull are matched.

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Disableable;
use gpui_kit::component::Selectable;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::h_flex;
use gpui_kit::component::menu::DropdownMenu;
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rust_i18n::t;

use wows_toolkit_viewmodel::armor::arc::ArcOutcome;
use wows_toolkit_viewmodel::armor::arc::StoppingPlate;
use wows_toolkit_viewmodel::armor::penetration::ComparisonShip;
use wowsunpack::game_params::types::ShellInfo;

use super::legend::swatch_color;
use super::options_filter::OptionsFilter;
use super::pane::ArmorViewerPane;
use super::pane::OptionSection;
use super::popover::labeled_slider_row;
use super::trajectory::ArcId;
use super::viewport_view::ArcSummary;
use super::viewport_view::CastShell;
use super::viewport_view::Isolate;
use super::viewport_view::ViewportView;
use crate::slider_field::SliderField;

/// How wide a marker is drawn.
const MARKER_SIZE: Pixels = px(22.);

/// The section body. `view` is the pane itself, read for the comparison
/// list: this renders inside the pane's own render, where reading the pane
/// entity would panic.
pub(super) fn render(
    view: &ArmorViewerPane,
    pane: &Entity<ArmorViewerPane>,
    viewport: &Entity<ViewportView>,
    cx: &mut App,
) -> AnyElement {
    let (state, next_range, arcs, editing) = {
        let shown = viewport.read(cx);
        let arcs = shown.arcs();
        // The arc being edited, where it is in the list, and what became of it.
        let editing = shown.selected_arc().and_then(|(id, range)| {
            let index = arcs.iter().position(|arc| arc.id == id)?;
            Some((index, range, describe_outcome(&arcs[index].outcome, shown)))
        });
        (shown.trajectory_state(), shown.display_sliders.cast_range.clone(), arcs, editing)
    };
    let selected_id = editing.as_ref().map(|(index, _, _)| arcs[*index].id);

    let continuing = state.continue_on_ricochet;
    let ricochet = {
        let viewport = viewport.clone();
        Checkbox::new("armor-trajectory-ricochet")
            .label(t!("ui.armor.continue_ricochet").to_string())
            .checked(continuing)
            .on_click(move |checked, _window, cx| {
                let next = *checked;
                viewport.update(cx, |view, cx| view.set_continue_on_ricochet(next, cx));
            })
    };
    let next_cast = v_flex()
        .gap_1()
        .child(heading(t!("ui.armor.trajectory_next_cast").into_owned()))
        .child(render_firing_controls(view, pane))
        .children(labeled_slider_row(
            t!("ui.armor.trajectory_range").into_owned(),
            &next_range,
            false,
            &OptionsFilter::default(),
            cx,
        ))
        .child(ricochet);

    let markers = render_markers(viewport, &arcs, selected_id, cx);
    let ships = view.penetration().ships.clone();
    let editor = editing
        .map(|(index, range, outcome)| render_editor(viewport, index + 1, &arcs[index], &range, outcome, &ships, cx));

    v_flex().gap_3().min_w_0().child(next_cast).child(markers).children(editor).into_any_element()
}

/// The attacker and ammunition the next cast is fired with.
///
/// With no attacker in the comparison list there is nothing to choose from;
/// the button opens Pen Check, which is where ships are added.
fn render_firing_controls(view: &ArmorViewerPane, pane: &Entity<ArmorViewerPane>) -> AnyElement {
    let (ships, firing_ship, firing_shell) = {
        let pen = view.penetration();
        (pen.ships.clone(), pen.firing_ship.clone(), pen.firing_shell)
    };
    if ships.is_empty() {
        let pane = pane.clone();
        return Button::new("armor-trajectory-add-ship")
            .label(t!("ui.armor.trajectory_add_ship").to_string())
            .compact()
            .tooltip(t!("ui.armor.trajectory_add_ship_tooltip").to_string())
            .on_click(move |_event, _window, cx: &mut App| {
                pane.update(cx, |pane, cx| pane.open_option_section(OptionSection::PenCheck, cx));
            })
            .into_any_element();
    }

    let selected_ship = ships.iter().find(|ship| Some(ship.param_index.as_str()) == firing_ship.as_deref());
    let ship_label = selected_ship.map_or_else(
        || t!("ui.armor.trajectory_select_ship").into_owned(),
        |ship| format!("{} {}", t!("ui.armor.trajectory_ship"), ship.display_name),
    );
    let cast = selected_ship.map(|ship| CastShell { ship: ship.clone(), shell_index: firing_shell });
    let active_shell = cast.as_ref().and_then(CastShell::fired_index);
    let shell_label = cast
        .as_ref()
        .and_then(CastShell::shell)
        .map(|shell| format!("{} {}", t!("ui.armor.trajectory_shell"), shell_name(shell)))
        .unwrap_or_else(|| t!("ui.armor.trajectory_select_shell").into_owned());
    let shells = selected_ship.map(|ship| ship.shells.clone()).unwrap_or_default();

    let ship_picker = {
        let pane = pane.clone();
        Button::new("armor-trajectory-ship")
            .label(ship_label.clone())
            .max_w(px(220.))
            .compact()
            .dropdown_caret(true)
            .tooltip(format!("{}\n{}", ship_label, t!("ui.armor.trajectory_ship_tooltip")))
            .dropdown_menu(move |mut menu, _window, _cx| {
                for ship in &ships {
                    let pane = pane.clone();
                    let param_index = ship.param_index.clone();
                    let active = Some(ship.param_index.as_str()) == firing_ship.as_deref();
                    menu = menu.item(PopupMenuItem::new(ship.display_name.clone()).checked(active).on_click(
                        move |_event, _window, cx| {
                            pane.update(cx, |pane, cx| pane.set_firing_ship(&param_index, cx));
                        },
                    ));
                }
                menu
            })
    };

    let shell_picker = {
        let pane = pane.clone();
        Button::new("armor-trajectory-shell")
            .label(shell_label)
            .max_w(px(180.))
            .compact()
            .dropdown_caret(true)
            .disabled(shells.is_empty())
            .tooltip(t!("ui.armor.trajectory_shell_tooltip").to_string())
            .dropdown_menu(move |mut menu, _window, _cx| {
                for (index, shell) in shells.iter().enumerate() {
                    let pane = pane.clone();
                    menu =
                        menu.item(PopupMenuItem::new(shell_name(shell)).checked(Some(index) == active_shell).on_click(
                            move |_event, _window, cx| {
                                pane.update(cx, |pane, cx| pane.set_firing_shell(index, cx));
                            },
                        ));
                }
                menu
            })
    };

    v_flex().min_w_0().gap_1().items_start().child(ship_picker).child(shell_picker).into_any_element()
}

/// One marker per arc, in its colour, with what applies to every arc at once
/// below them.
fn render_markers(
    viewport: &Entity<ViewportView>,
    arcs: &[ArcSummary],
    selected: Option<ArcId>,
    cx: &App,
) -> AnyElement {
    if arcs.is_empty() {
        return hint(t!("ui.armor.arc_none").into_owned());
    }

    let clear = {
        let viewport = viewport.clone();
        Button::new("armor-trajectory-clear").label(t!("ui.armor.trajectory_clear").to_string()).compact().on_click(
            move |_event, _window, cx: &mut App| {
                viewport.update(cx, |view, cx| view.clear_trajectories(cx));
            },
        )
    };
    let isolate_all = |id: &'static str, what: Isolate, label: String, tooltip: String| {
        let viewport = viewport.clone();
        Button::new(id).label(label).compact().tooltip(tooltip).on_click(move |_event, _window, cx: &mut App| {
            viewport.update(cx, |view, cx| view.isolate_all_arcs(what, cx));
        })
    };

    v_flex()
        .gap_1()
        .child(
            h_flex()
                .id("armor-arc-markers")
                .test_support()
                .gap_1()
                .flex_wrap()
                .items_center()
                .children(
                    arcs.iter()
                        .enumerate()
                        .map(|(index, arc)| marker(viewport, index, arc, Some(arc.id) == selected, cx)),
                )
                .child(div().flex_1())
                .child(clear),
        )
        .when(selected.is_none(), |column| column.child(hint(t!("ui.armor.arc_pick_hint").into_owned())))
        .child(
            h_flex()
                .gap_1()
                .child(isolate_all(
                    "armor-arcs-isolate-plates",
                    Isolate::Plates,
                    t!("ui.armor.arc_isolate_plates").into_owned(),
                    t!("ui.armor.arc_isolate_plates_tooltip").into_owned(),
                ))
                .child(isolate_all(
                    "armor-arcs-isolate-zones",
                    Isolate::Zones,
                    t!("ui.armor.arc_isolate_zones").into_owned(),
                    t!("ui.armor.arc_isolate_zones_tooltip").into_owned(),
                )),
        )
        // The simulation is read off the game's own data but is not the game's
        // own code, which is worth saying where its verdicts are read.
        .child(
            div()
                .text_xs()
                .whitespace_normal()
                .text_color(rgb(crate::theme::semantic().warn))
                .child(t!("ui.armor.arc_simulation_caveat").to_string()),
        )
        .child(render_angle_legend())
        .into_any_element()
}

/// One arc's marker: its number on its colour. A click opens its editor, or
/// puts the editor away when it is the arc open; a right-click removes it.
fn marker(viewport: &Entity<ViewportView>, index: usize, arc: &ArcSummary, selected: bool, cx: &App) -> AnyElement {
    let id = arc.id;
    let number = index + 1;
    let tooltip: SharedString = t!("ui.armor.arc_marker_tooltip", number = number).into_owned().into();
    let ring = if selected { cx.theme().foreground } else { black().opacity(0.6) };
    let select = viewport.clone();
    let remove = viewport.clone();

    div()
        .id(("armor-arc-marker", index))
        .test_support()
        .aria_selected(selected)
        .flex_none()
        .size(MARKER_SIZE)
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(swatch_color(arc.color.rgba()))
        .border_2()
        .border_color(ring)
        .text_xs()
        .font_weight(FontWeight::BOLD)
        .text_color(black())
        .cursor_pointer()
        .child(number.to_string())
        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
        .on_click(move |_event, _window, cx: &mut App| {
            select.update(cx, |view, cx| view.toggle_arc_selection(id, cx));
        })
        .on_mouse_down(MouseButton::Right, move |_event, _window, cx: &mut App| {
            remove.update(cx, |view, cx| view.remove_arc(id, cx));
        })
        .into_any_element()
}

/// The arc a marker selected: its range, the shell it is fired with and
/// what it hides, each applied as it changes; then what became of it.
fn render_editor(
    viewport: &Entity<ViewportView>,
    number: usize,
    arc: &ArcSummary,
    range: &Entity<SliderField>,
    outcome: String,
    ships: &[ComparisonShip],
    cx: &mut App,
) -> AnyElement {
    let id = arc.id;
    let color = swatch_color(arc.color.rgba());

    let summary = t!(
        "ui.armor.arc_summary",
        hits = arc.hits,
        armor = format!("{:.0}", arc.total_armor.value()),
        range = format!("{:.1}", arc.range.value()),
    )
    .into_owned();
    let delete = {
        let viewport = viewport.clone();
        Button::new("armor-arc-delete")
            .child(crate::icons::icon(crate::icons::Glyph::Trash))
            .compact()
            .tooltip(t!("ui.armor.arc_delete").to_string())
            .on_click(move |_event, _window, cx: &mut App| {
                viewport.update(cx, |view, cx| view.remove_arc(id, cx));
            })
    };
    let header = h_flex()
        .gap_2()
        .items_center()
        .child(div().flex_none().size_3().rounded_full().bg(color))
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::BOLD)
                .child(t!("ui.armor.arc_heading", number = number).to_string()),
        )
        .child(div().flex_1().min_w_0().text_xs().truncate().text_color(crate::theme::text_dim()).child(summary))
        .child(delete);

    let isolate = |what: Isolate, on: bool, id_base: &'static str, label: String, tooltip: String| {
        let viewport = viewport.clone();
        crate::ui::selectable(
            id_base,
            on,
            Button::new(SharedString::from(format!("{id_base}-button")))
                .label(label)
                .compact()
                .selected(on)
                .tooltip(tooltip)
                .on_click(move |_event, _window, cx: &mut App| {
                    viewport.update(cx, |view, cx| view.isolate_arc(id, what, !on, cx));
                }),
        )
    };

    v_flex()
        .id("armor-arc-editor")
        .test_support()
        .gap_1()
        .p_2()
        .rounded_md()
        .border_2()
        .border_color(color)
        .child(header)
        .children(labeled_slider_row(
            t!("ui.armor.trajectory_range").into_owned(),
            range,
            false,
            &OptionsFilter::default(),
            cx,
        ))
        .child(render_arc_shell_picker(viewport, id, arc.shell.as_ref(), ships))
        .child(
            h_flex()
                .gap_1()
                .child(isolate(
                    Isolate::Plates,
                    arc.isolating_plates,
                    "armor-arc-plates",
                    t!("ui.armor.arc_isolate_plates").into_owned(),
                    t!("ui.armor.arc_isolate_plates_tooltip").into_owned(),
                ))
                .child(isolate(
                    Isolate::Zones,
                    arc.isolating_zones,
                    "armor-arc-zones",
                    t!("ui.armor.arc_isolate_zones").into_owned(),
                    t!("ui.armor.arc_isolate_zones_tooltip").into_owned(),
                )),
        )
        .child(div().text_xs().whitespace_normal().text_color(outcome_color(&arc.outcome)).child(outcome))
        .into_any_element()
}

/// The shell one arc is fired with, chosen from every shell of every ship
/// being compared.
fn render_arc_shell_picker(
    viewport: &Entity<ViewportView>,
    id: ArcId,
    held: Option<&CastShell>,
    ships: &[ComparisonShip],
) -> AnyElement {
    let label = held
        .and_then(|cast| Some(format!("{} {}", cast.ship.display_name, shell_name(cast.shell()?))))
        .unwrap_or_else(|| t!("ui.armor.trajectory_select_shell").into_owned());
    let held_key = held.and_then(|cast| Some((cast.ship.param_index.clone(), cast.fired_index()?)));
    let ships = ships.to_vec();
    let viewport = viewport.clone();

    Button::new("armor-arc-shell")
        .label(label)
        .max_w(px(260.))
        .compact()
        .dropdown_caret(true)
        .disabled(ships.iter().all(|ship| ship.shells.is_empty()))
        .tooltip(t!("ui.armor.trajectory_shell_tooltip").to_string())
        .dropdown_menu(move |mut menu, _window, _cx| {
            for ship in &ships {
                for (index, shell) in ship.shells.iter().enumerate() {
                    let held = held_key
                        .as_ref()
                        .is_some_and(|(param_index, at)| *param_index == ship.param_index && *at == index);
                    let viewport = viewport.clone();
                    let cast = CastShell { ship: ship.clone(), shell_index: Some(index) };
                    let label = format!("{}: {}", ship.display_name, shell_name(shell));
                    menu = menu.item(PopupMenuItem::new(label).checked(held).on_click(move |_event, _window, cx| {
                        let cast = cast.clone();
                        viewport.update(cx, |view, cx| view.set_arc_shell(id, cast, cx));
                    }));
                }
            }
            menu
        })
        .into_any_element()
}

/// A shell as the pickers name it: its ammunition and calibre.
fn shell_name(shell: &ShellInfo) -> String {
    format!("{} {:.0} mm", shell.ammo_type.display_name(), shell.caliber.value())
}

/// What the impact markers' colours mean, which is how square each strike was.
fn render_angle_legend() -> impl IntoElement {
    let semantic = crate::theme::semantic();
    h_flex().gap_2().items_center().flex_wrap().children(
        [
            (semantic.armor_angle_good, "ui.armor.angle_good"),
            (semantic.armor_angle_mid, "ui.armor.angle_mid"),
            (semantic.armor_angle_bad, "ui.armor.angle_bad"),
        ]
        .into_iter()
        .map(|(color, key)| {
            h_flex()
                .gap_1()
                .items_center()
                .child(div().size_2().rounded_full().bg(rgb(color)))
                .child(div().text_xs().text_color(crate::theme::text_dim()).child(t!(key).to_string()))
        }),
    )
}

/// What an outcome reads as.
fn describe_outcome(outcome: &ArcOutcome, shown: &ViewportView) -> String {
    match outcome {
        ArcOutcome::NotSimulated => t!("ui.armor.arc_no_shell").into_owned(),
        ArcOutcome::Detonated { zone } => match zone {
            Some(zone) => t!("ui.armor.arc_detonated_in", zone = zone.clone()).into_owned(),
            None => t!("ui.armor.arc_detonated_past_armor").into_owned(),
        },
        ArcOutcome::Ricocheted { plate } => {
            t!("ui.armor.arc_ricochet", plate = describe_plate(plate, shown)).into_owned()
        }
        ArcOutcome::Shattered { plate } => {
            t!("ui.armor.arc_shatter", plate = describe_plate(plate, shown)).into_owned()
        }
        ArcOutcome::Stopped { plate } => t!("ui.armor.arc_stopped", plate = describe_plate(plate, shown)).into_owned(),
        ArcOutcome::Overpenetrated { fuse_armed: true } => t!("ui.armor.arc_overpen").into_owned(),
        ArcOutcome::Overpenetrated { fuse_armed: false } => t!("ui.armor.arc_overpen_unarmed").into_owned(),
    }
}

/// The plate a shell stopped at, or that it stopped at one nothing is known
/// about, which a cast whose plate list is shorter than its simulation reports.
fn describe_plate(plate: &Option<StoppingPlate>, shown: &ViewportView) -> String {
    match plate {
        Some(plate) => {
            let named = shown.material_label(plate.material);
            format!("#{} {:.0}mm {}", plate.number, plate.thickness.value(), named)
        }
        None => t!("ui.armor.arc_plate_unknown").into_owned(),
    }
}

/// The tone an outcome is read in: the armor palette both viewers paint their
/// verdicts in, where a detonation is the result the reader was after.
fn outcome_color(outcome: &ArcOutcome) -> Hsla {
    let semantic = crate::theme::semantic();
    match outcome {
        ArcOutcome::Detonated { .. } => rgb(semantic.armor_pen).into(),
        ArcOutcome::Overpenetrated { .. } => rgb(semantic.armor_overpen).into(),
        ArcOutcome::Ricocheted { .. } => rgb(semantic.armor_ricochet).into(),
        ArcOutcome::Shattered { .. } | ArcOutcome::Stopped { .. } => rgb(semantic.armor_shatter).into(),
        ArcOutcome::NotSimulated => crate::theme::text_dim(),
    }
}

fn heading(text: String) -> impl IntoElement {
    div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(crate::theme::text_dim()).child(text)
}

fn hint(text: String) -> AnyElement {
    div().text_xs().whitespace_normal().text_color(crate::theme::text_dim()).child(text).into_any_element()
}

// The test module imports by name: `use super::*` would pull in the glob
// `gpui_kit::*` re-export of GPUI's own `test` macro.
#[cfg(test)]
mod tests {
    use gpui_kit::AppContext as _;
    use gpui_kit::TestAppContext;
    use gpui_kit::px;
    use gpui_kit::size;
    use gpui_kit::test::TestWindowExt;
    use wowsunpack::game_params::ttx::armor_material::ArmorMaterial;
    use wowsunpack::game_params::types::Degrees;
    use wowsunpack::game_params::types::Km;
    use wowsunpack::game_params::types::Millimeters;
    use wowsunpack::game_params::types::ShipModelDistance;

    use crate::armor_viewer::pane::ArmorViewerPane;
    use crate::armor_viewer::trajectory;
    use crate::armor_viewer::trajectory::TrajectoryHit;
    use crate::slider_field::SliderFieldChange;
    use crate::viewport::types::Vec3;

    /// A shell through two plates, cast with no ship chosen.
    fn cast_from(range: f32) -> trajectory::Trajectory {
        let hit = |at: f32| TrajectoryHit {
            position: Vec3::new(at, 0.0, 0.0),
            thickness: Millimeters::from(32.0),
            zone: "Citadel".to_string(),
            material: ArmorMaterial::parse("Belt").expect("Belt is a table key"),
            angle_from_normal: Degrees::from(10.0),
            distance_from_start: ShipModelDistance::from(at),
            normal: Vec3::x(),
        };
        trajectory::cast(vec![hit(0.0), hit(3.0)], Vec3::x(), None, 100.0, false, Km::new(range))
    }

    /// Each arc has a marker in the rail. A click on one opens its editor,
    /// whose range slider fires that arc again and no other; a right-click
    /// removes its arc; a click on the open arc's marker puts the editor away.
    #[gpui_kit::test]
    fn a_marker_selects_its_arc_for_editing_and_removes_it(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(1600.), px(1000.)), ArmorViewerPane::new);

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("armor-arc-markers").is_none(), "nothing is cast yet");
        })
        .expect("the test window stays open");

        let viewport = window.update(cx, |pane, _window, cx| pane.active_viewport(cx)).expect("the window is open");
        let first = viewport.update(cx, |view, cx| {
            let first = view.add_arc(Vec3::x(), cast_from(10.0), cx);
            view.add_arc(Vec3::x(), cast_from(12.0), cx);
            first
        });

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(window.find(("armor-arc-marker", 1usize)).selected(), Some(true), "a new arc opens for editing");
            assert_eq!(window.find(("armor-arc-marker", 0usize)).selected(), Some(false));
            assert!(window.try_find("armor-arc-editor").is_some());

            window.click(("armor-arc-marker", 0usize), cx);
            window.render_frame(cx);
            assert_eq!(window.find(("armor-arc-marker", 0usize)).selected(), Some(true), "a marker selects its arc");
        })
        .expect("the test window stays open");

        let (selected, range) = viewport.read_with(cx, |view, _| view.selected_arc()).expect("an arc is being edited");
        assert_eq!(selected, first);
        range.update(cx, |_, cx| cx.emit(SliderFieldChange(15.0)));
        let ranges: Vec<f32> =
            viewport.read_with(cx, |view, _| view.arcs().iter().map(|arc| arc.range.value()).collect());
        assert_eq!(
            ranges,
            vec![15.0, 12.0],
            "the slider fires the selected arc from its new range and leaves the other"
        );

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            window.right_click(("armor-arc-marker", 1usize), cx);
            window.render_frame(cx);
            assert!(window.try_find(("armor-arc-marker", 1usize)).is_none(), "a right-click removes its arc");
            assert_eq!(window.find(("armor-arc-marker", 0usize)).selected(), Some(true), "and leaves the one open");

            window.click(("armor-arc-marker", 0usize), cx);
            window.render_frame(cx);
            assert!(window.try_find("armor-arc-editor").is_none(), "the open arc's marker puts its editor away");
            assert!(window.try_find(("armor-arc-marker", 0usize)).is_some(), "without removing the arc");
        })
        .expect("the test window stays open");
    }
}
