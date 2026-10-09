//! The viewport toolbar row `viewport_view.rs` renders above the 3D viewport,
//! and the three option sections `options_panel.rs` renders beside it: the
//! tri-state zone/material/plate tree, the hull-visibility tree and the
//! display settings. Ports `armor_viewer::ui::tab`'s
//! `draw_armor_visibility_popover` (`tab.rs:4403-4611`) and
//! `draw_hull_visibility_popover` (`tab.rs:3135-3317`).
//!
//! Every row's click/hover handler captures a clone of `Entity<ViewportView>`
//! and mutates it via `.update(cx, ..)`, which is what lets the three section
//! builders take a plain `&mut App` and so be called from the rail as well.
//!
//! The section builders must not read the `ArmorViewerPane` entity: the rail
//! renders inside that pane's own render, and reading it there panics on the
//! lease it already holds. Anything a section needs from the pane is passed
//! in (see `render_display_popover_content`'s `legend_visible`).
use std::collections::HashMap;
use std::collections::HashSet;

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Disableable;
use gpui_kit::component::Icon;
use gpui_kit::component::IconName;
use gpui_kit::component::Selectable;
use gpui_kit::component::Side;
use gpui_kit::component::Sizable;
use gpui_kit::component::Size;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::Toggle;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::h_flex;
use gpui_kit::component::list::ListItem;
use gpui_kit::component::menu::DropdownMenu;
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::scroll::Scrollbar;
use gpui_kit::component::sidebar::SidebarToggleButton;
use gpui_kit::component::slider::Slider;
use gpui_kit::component::slider::SliderState;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rust_i18n::t;

use wowsunpack::export::camo_textures::CamoSchemeId;
use wowsunpack::export::camo_textures::CamoSchemeInfo;
use wowsunpack::export::gltf_export::CamoOrigin;
use wowsunpack::export::gltf_export::thickness_to_color;
use wowsunpack::game_params::keys::ComponentType;

use crate::viewport::types::LightingSettings;

use super::legend::swatch_color;
use super::load_ship::ArmorZone;
use super::load_ship::LoadedShipArmor;
use super::load_ship::PlateKey;
use super::load_ship::ZonePart;
use wows_toolkit_viewmodel::armor::camera_perspective::LookMode;

use super::viewport_view::CameraRingSettings;
use super::viewport_view::PerspectiveSettings;
use super::viewport_view::TrajectoryState;
use super::viewport_view::ViewportView;
use super::visibility::SidebarHighlightKey;
use super::visibility::TriState;
use super::visibility::hull_group_all_on;
use super::visibility::hull_group_any_on;
use super::visibility::part_any_plate_hidden;
use super::visibility::part_on;
use super::visibility::plate_explicitly_hidden;
use super::visibility::zone_all_on;
use super::visibility::zone_any_on;
use crate::armor_viewer::analysis;
use crate::armor_viewer::pane::ArmorViewerPane;
use wowsunpack::game_params::types::Km;

/// Checkbox box size for [`TriState`] partial-dash placement (`Size::Medium`,
/// `checkbox.rs`'s own `size_4` = 1rem = 16px).
const CHECKBOX_BOX: Pixels = px(16.);

/// Toolbar row above the 3D viewport: the visibility-popover trigger button
/// (Task 7a) and the display-settings trigger button (Task 7b).
/// `viewport_view.rs`'s `Render` impl wraps this above the interactive
/// viewport div.
pub fn render_toolbar(
    view: &ViewportView,
    entity: &Entity<ViewportView>,
    cx: &mut Context<ViewportView>,
) -> impl IntoElement + use<> {
    let trajectory_active = view.trajectory_state().shown;
    let commands = h_flex()
        .flex_none()
        .flex_wrap()
        .gap_2()
        .items_center()
        .px_2()
        .py_1()
        // What this viewport is showing, where the controls that act on it
        // are, rather than on a strip of its own above them.
        .when_some(view.shown_ship_name(), |this, name| {
            this.child(div().text_xs().font_weight(FontWeight::BOLD).child(name)).child(crate::ui::rule_v(cx))
        })
        .child(render_hidden_plates_button(view, entity))
        .child(render_gaps_button(view, entity))
        .child(render_splash_button(view, entity))
        .child(render_splash_boxes_button(view, entity))
        .child(render_export_button(view, entity))
        // The pane's own controls: one toolbar, not two.
        .when_some(view.pane(), |this, pane| {
            this.child(crate::ui::rule_v(cx))
                .child(render_options_button(pane.clone(), cx))
                .child(render_penetration_button(pane, cx))
        });

    let trajectory = h_flex()
        .flex_wrap()
        .items_center()
        .gap_2()
        .px_2()
        .py_1()
        .border_l_2()
        .border_color(if trajectory_active { crate::theme::accent() } else { cx.theme().border })
        .when(trajectory_active, |row| row.bg(crate::theme::accent().opacity(0.08)))
        .child(render_trajectory_button(view, entity))
        .when_some(view.pane(), |this, pane| this.child(render_firing_controls(&pane, cx)));

    crate::ui::toolbar(cx).flex_col().items_stretch().p_0().gap_0().w_full().child(commands).child(trajectory).when(
        trajectory_active,
        |toolbar| {
            toolbar.child(
                div()
                    .px_2()
                    .pb_1()
                    .text_xs()
                    .text_color(crate::theme::text_dim())
                    .whitespace_normal()
                    .child(t!("ui.armor.trajectory_tooltip").to_string()),
            )
        },
    )
}

/// Toolbar toggle for "Show Hidden": the plates that are part of the combat
/// model but that the game's own armor viewer never draws.
///
/// A mode rather than an addition, so it reads as selected while it is on.
fn render_hidden_plates_button(view: &ViewportView, entity: &Entity<ViewportView>) -> impl IntoElement + use<> {
    let showing = view.show_hidden_only();
    let has_armor = view.has_armor();
    let entity = entity.clone();

    crate::ui::selectable(
        "armor-show-hidden",
        showing,
        Button::new("armor-show-hidden-button")
            .child(crate::icons::icon(crate::icons::EYE_SLASH))
            .label(t!("ui.armor.show_hidden").to_string())
            .compact()
            .selected(showing)
            .disabled(!has_armor)
            .tooltip(t!("ui.armor.show_hidden_tooltip").to_string())
            .on_click(move |_event, _window, cx: &mut App| {
                entity.update(cx, |view, cx| view.set_show_hidden_only(!showing, cx));
            }),
    )
}

/// The camera-rings section of the display popover: whether the orbits are
/// drawn, which mode's, and where on them the camera sits.
///
/// Every control but the first is refused while the rings are off, and the
/// whole section says so when the ship names no modes: a slider that moves
/// nothing is worse than one that will not move.
fn render_camera_rings_section(
    entity: &Entity<ViewportView>,
    rings: &CameraRingSettings,
    modes: &[String],
    perspective: &PerspectiveSettings,
    fov_slider: &Entity<SliderState>,
    height_slider: &Entity<SliderState>,
    perspective_fov_slider: &Entity<SliderState>,
) -> AnyElement {
    let has_modes = !modes.is_empty();
    let on = rings.shown && has_modes;

    let mut section = v_flex()
        .gap_1()
        .child(div().text_sm().font_weight(FontWeight::BOLD).child(t!("ui.armor.camera_rings").to_string()))
        .child({
            let entity = entity.clone();
            let rings = rings.clone();
            Checkbox::new("armor-camera-rings")
                .label(t!("ui.armor.show_camera_rings").to_string())
                .checked(rings.shown)
                .disabled(!has_modes)
                .on_click(move |checked, _window, cx| {
                    let shown = *checked;
                    let next = CameraRingSettings { shown, ..rings.clone() };
                    entity.update(cx, |view, cx| view.set_camera_rings(next.clone(), cx));
                })
        });

    if !has_modes {
        return section
            .child(
                div().text_xs().text_color(crate::theme::text_dim()).child(t!("ui.armor.no_ship_loaded").to_string()),
            )
            .into_any_element();
    }

    // One button per mode rather than a combo: a ship names two or three, and
    // which one is showing is the thing being read.
    section = section.child(h_flex().flex_wrap().gap_1().children(modes.iter().enumerate().map(|(ix, mode)| {
        let chosen = rings.mode.as_ref() == Some(mode);
        let entity = entity.clone();
        let rings = rings.clone();
        let mode = mode.clone();
        crate::ui::selectable(
            ("armor-camera-mode", ix),
            chosen,
            Button::new(("armor-camera-mode-button", ix))
                .label(mode.clone())
                .compact()
                .selected(chosen)
                .disabled(!on)
                .on_click(move |_event, _window, cx: &mut App| {
                    let next = CameraRingSettings { mode: Some(mode.clone()), ..rings.clone() };
                    entity.update(cx, |view, cx| view.set_camera_rings(next.clone(), cx));
                }),
        )
    })));

    section
        .child(labeled_slider_row(t!("ui.armor.camera_fov").into_owned(), fov_slider, rings.fov, !on))
        .child(labeled_slider_row(t!("ui.armor.camera_height").into_owned(), height_slider, rings.height, !on))
        .child({
            let entity = entity.clone();
            let rings = rings.clone();
            Checkbox::new("armor-camera-zoom-path")
                .label(t!("ui.armor.show_camera_zoom_path").to_string())
                .checked(rings.zoom_path)
                .disabled(!on)
                .on_click(move |checked, _window, cx| {
                    let zoom_path = *checked;
                    let next = CameraRingSettings { zoom_path, ..rings.clone() };
                    entity.update(cx, |view, cx| view.set_camera_rings(next.clone(), cx));
                })
        })
        .when(rings.zoom_path, |this| {
            this.child(div().pl(px(20.)).child(h_flex().gap_4().flex_wrap().children([
                zoom_path_checkbox(
                    entity,
                    "armor-camera-zoom-path-fov",
                    t!("ui.armor.zoom_path_regular_fov").into_owned(),
                    rings,
                    rings.zoom_path_at_fov,
                    on,
                    |settings, value| settings.zoom_path_at_fov = value,
                ),
                zoom_path_checkbox(
                    entity,
                    "armor-camera-zoom-path-max",
                    t!("ui.armor.zoom_path_max_fov").into_owned(),
                    rings,
                    rings.zoom_path_at_max_fov,
                    on,
                    |settings, value| settings.zoom_path_at_max_fov = value,
                ),
            ])))
        })
        .child(render_perspective_rows(entity, perspective, perspective_fov_slider))
        .into_any_element()
}

/// The camera lock: put the eye on the ship's own orbit and look out from it.
///
/// Under the orbits because it follows the same trajectory and the same two
/// sliders: which orbit is drawn is which orbit the eye rides.
fn render_perspective_rows(
    entity: &Entity<ViewportView>,
    perspective: &PerspectiveSettings,
    fov_slider: &Entity<SliderState>,
) -> impl IntoElement + use<> {
    let toggled = entity.clone();
    let on = perspective.enabled;
    v_flex()
        .gap_1()
        .child(
            Checkbox::new("armor-camera-perspective")
                .label(t!("ui.armor.camera_perspective").to_string())
                .checked(on)
                .on_click(move |checked, _window, cx| {
                    let checked = *checked;
                    toggled.update(cx, |view, cx| view.set_perspective_enabled(checked, cx));
                }),
        )
        .when(on, |this| {
            this.child(labeled_slider_row(
                t!("ui.armor.perspective_fov").into_owned(),
                fov_slider,
                perspective.camera.fov_deg,
                false,
            ))
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(div().w(px(96.)).text_xs().child(t!("ui.armor.perspective_projection").into_owned()))
                    .child(h_flex().gap_1().children([
                        look_mode_button(
                            entity,
                            "armor-perspective-proj-game",
                            t!("ui.armor.perspective_proj_game").into_owned(),
                            LookMode::Game,
                            perspective.camera.look_mode,
                        ),
                        look_mode_button(
                            entity,
                            "armor-perspective-proj-center",
                            t!("ui.armor.perspective_proj_center").into_owned(),
                            LookMode::ThroughCenter,
                            perspective.camera.look_mode,
                        ),
                    ])),
            )
        })
}

/// One of the two ways the locked camera can aim.
fn look_mode_button(
    entity: &Entity<ViewportView>,
    id: &'static str,
    label: String,
    mode: LookMode,
    chosen: LookMode,
) -> AnyElement {
    let entity = entity.clone();
    let selected = mode == chosen;
    crate::ui::selectable(
        id,
        selected,
        Button::new(id).label(label).compact().selected(selected).on_click(move |_event, _window, cx: &mut App| {
            entity.update(cx, |view, cx| view.set_perspective_look_mode(mode, cx));
        }),
    )
    .into_any_element()
}

/// One of the zoom path's two field-of-view checkboxes.
fn zoom_path_checkbox(
    entity: &Entity<ViewportView>,
    id: &'static str,
    label: String,
    rings: &CameraRingSettings,
    checked: bool,
    enabled: bool,
    apply: fn(&mut CameraRingSettings, bool),
) -> AnyElement {
    let entity = entity.clone();
    let rings = rings.clone();
    Checkbox::new(id)
        .label(label)
        .checked(checked)
        .disabled(!enabled)
        .on_click(move |checked, _window, cx| {
            let mut next = rings.clone();
            apply(&mut next, *checked);
            entity.update(cx, |view, cx| view.set_camera_rings(next.clone(), cx));
        })
        .into_any_element()
}

/// The trajectory section of the display popover: the range a cast shell is
/// fired from, whether a ricochet is followed, and a way to drop what has
/// been cast.
///
/// Everything here is refused while the mode is off, because each of them
/// only says something about a shell that has been or is about to be cast.
fn render_trajectory_section(
    entity: &Entity<ViewportView>,
    state: &TrajectoryState,
    cast_range: Km,
    range_slider: &Entity<SliderState>,
) -> AnyElement {
    let on = state.shown;

    v_flex()
        .gap_1()
        .child(div().text_sm().font_weight(FontWeight::BOLD).child(t!("ui.armor.trajectory").to_string()))
        .child(labeled_slider_row(t!("ui.armor.trajectory_range").into_owned(), range_slider, cast_range.value(), !on))
        .child({
            let entity = entity.clone();
            let continuing = state.continue_on_ricochet;
            Checkbox::new("armor-trajectory-ricochet")
                .label(t!("ui.armor.continue_ricochet").to_string())
                .checked(continuing)
                .disabled(!on)
                .on_click(move |checked, _window, cx| {
                    let next = *checked;
                    entity.update(cx, |view, cx| view.set_continue_on_ricochet(next, cx));
                })
        })
        .child({
            let entity = entity.clone();
            Button::new("armor-trajectory-clear")
                .label(t!("ui.armor.trajectory_clear").to_string())
                .compact()
                // Nothing cast is nothing to clear.
                .disabled(state.count == 0)
                .on_click(move |_event, _window, cx: &mut App| {
                    entity.update(cx, |view, cx| view.clear_trajectories(cx));
                })
        })
        .into_any_element()
}

/// Toolbar toggle for gap detection, which carries its own count: the
/// number is the answer, so it is on the control rather than behind a hover.
fn render_gaps_button(view: &ViewportView, entity: &Entity<ViewportView>) -> impl IntoElement + use<> {
    let gaps = view.gaps();
    let has_armor = view.has_armor();
    let entity = entity.clone();

    let label =
        if gaps.shown { format!("{} ({})", t!("ui.armor.gaps"), gaps.count) } else { t!("ui.armor.gaps").into_owned() };
    // A hull with openings is worth a warning tone; one with none, having
    // been looked at, is worth saying so quietly.
    let found = gaps.shown && gaps.count > 0;

    crate::ui::selectable(
        "armor-show-gaps",
        gaps.shown,
        Button::new("armor-show-gaps-button")
            .child(crate::icons::icon(if found { crate::icons::WARNING } else { crate::icons::EYE_SLASH }))
            .label(label)
            .compact()
            .selected(gaps.shown)
            .disabled(!has_armor)
            .tooltip(t!("ui.armor.gaps_tooltip").to_string())
            .on_click(move |_event, _window, cx: &mut App| {
                entity.update(cx, |view, cx| view.set_show_gaps(!gaps.shown, cx));
            }),
    )
}

/// Toolbar toggle for trajectory mode, which carries how many shells have
/// been cast: while the mode is on a click casts rather than hides a plate,
/// and the count is what says whether the last click landed.
fn render_trajectory_button(view: &ViewportView, entity: &Entity<ViewportView>) -> impl IntoElement + use<> {
    let state = view.trajectory_state();
    let has_armor = view.has_armor();
    let entity = entity.clone();

    let label = if state.shown {
        format!("{} ({})", t!("ui.armor.trajectory_on"), state.count)
    } else {
        t!("ui.armor.trajectory").into_owned()
    };

    crate::ui::selectable(
        "armor-trajectory",
        state.shown,
        Button::new("armor-trajectory-button")
            .child(crate::icons::icon(crate::icons::CROSSHAIR))
            .label(label)
            .compact()
            .selected(state.shown)
            .when(state.shown, |button| button.bg(crate::theme::accent()).text_color(crate::theme::accent_foreground()))
            .disabled(!has_armor)
            .tooltip(t!("ui.armor.trajectory_tooltip").to_string())
            .on_click(move |_event, _window, cx: &mut App| {
                entity.update(cx, |view, cx| view.set_trajectory_mode(!state.shown, cx));
            }),
    )
}

/// The attacker and ammunition used by trajectory casts, kept beside the
/// mode button so both inputs are visible at the point of use.
fn render_firing_controls(pane: &Entity<ArmorViewerPane>, cx: &App) -> AnyElement {
    let (ships, firing_ship, firing_shell) = {
        let pen = pane.read(cx).penetration();
        (pen.ships.clone(), pen.firing_ship.clone(), pen.firing_shell)
    };
    let selected_ship = ships.iter().find(|ship| Some(ship.param_index.as_str()) == firing_ship.as_deref());
    let ship_label = selected_ship.map_or_else(
        || t!("ui.armor.trajectory_select_ship").into_owned(),
        |ship| format!("{} {}", t!("ui.armor.trajectory_ship").to_string(), ship.display_name),
    );
    let active_shell_index = selected_ship.and_then(|ship| {
        firing_shell
            .filter(|index| *index < ship.shells.len())
            .or_else(|| {
                ship.shells.iter().position(|shell| shell.ammo_type == wowsunpack::game_params::types::AmmoType::AP)
            })
            .or_else(|| (!ship.shells.is_empty()).then_some(0))
    });
    let shell_label = selected_ship
        .zip(active_shell_index)
        .map(|(ship, index)| {
            let shell = &ship.shells[index];
            format!(
                "{} {} {:.0} mm",
                t!("ui.armor.trajectory_shell").to_string(),
                shell.ammo_type.display_name(),
                shell.caliber.value(),
            )
        })
        .unwrap_or_else(|| t!("ui.armor.trajectory_select_shell").into_owned());

    let ship_picker: AnyElement = if ships.is_empty() {
        let pane = pane.clone();
        Button::new("armor-trajectory-ship")
            .label(t!("ui.armor.trajectory_add_ship").to_string())
            .compact()
            .tooltip(t!("ui.armor.trajectory_add_ship_tooltip").to_string())
            .on_click(move |_event, _window, cx: &mut App| {
                pane.update(cx, |pane, cx| {
                    if !pane.analysis_open() {
                        pane.toggle_analysis(cx);
                    }
                });
            })
            .into_any_element()
    } else {
        let pane = pane.clone();
        let menu_ships = ships.clone();
        Button::new("armor-trajectory-ship")
            .label(ship_label.clone())
            .max_w(px(220.))
            .compact()
            .dropdown_caret(true)
            .tooltip(format!("{}\n{}", ship_label, t!("ui.armor.trajectory_ship_tooltip")))
            .dropdown_menu(move |mut menu, _window, _cx| {
                for ship in &menu_ships {
                    let pane = pane.clone();
                    let param_index = ship.param_index.clone();
                    let active = Some(ship.param_index.as_str()) == firing_ship.as_deref();
                    let label =
                        if active { format!("{} (selected)", ship.display_name) } else { ship.display_name.clone() };
                    menu = menu.item(PopupMenuItem::new(label).on_click(move |_event, _window, cx| {
                        pane.update(cx, |pane, cx| pane.set_firing_ship(&param_index, cx));
                    }));
                }
                menu
            })
            .into_any_element()
    };

    let shell_picker = {
        let pane = pane.clone();
        let selected_shells = selected_ship.map(|ship| ship.shells.clone()).unwrap_or_default();
        Button::new("armor-trajectory-shell")
            .label(shell_label)
            .max_w(px(180.))
            .compact()
            .dropdown_caret(true)
            .disabled(selected_shells.is_empty())
            .tooltip(t!("ui.armor.trajectory_shell_tooltip").to_string())
            .dropdown_menu(move |mut menu, _window, _cx| {
                for (index, shell) in selected_shells.iter().enumerate() {
                    let pane = pane.clone();
                    let ammo = shell.ammo_type.display_name();
                    let caliber = shell.caliber.value();
                    let selected = Some(index) == active_shell_index;
                    let label = if selected {
                        format!("{} {:.0} mm (selected)", ammo, caliber)
                    } else {
                        format!("{} {:.0} mm", ammo, caliber)
                    };
                    menu = menu.item(PopupMenuItem::new(label).on_click(move |_event, _window, cx| {
                        pane.update(cx, |pane, cx| pane.set_firing_shell(index, cx));
                    }));
                }
                menu
            })
    };

    h_flex().flex_wrap().min_w(px(0.)).gap_1().items_center().child(ship_picker).child(shell_picker).into_any_element()
}

/// Toolbar toggle for splash mode, carrying how many zones the last burst
/// reached: that number is the answer the mode exists to give.
///
/// Refused on a hull that ships no splash file, and on one where the chosen
/// attacker carries nothing that bursts; the tooltip says which.
fn render_splash_button(view: &ViewportView, entity: &Entity<ViewportView>) -> impl IntoElement + use<> {
    let state = view.splash_state();
    let entity = entity.clone();

    let label = if state.shown {
        format!("{} ({})", t!("ui.armor.splash_mode"), state.zones_reached)
    } else {
        t!("ui.armor.splash_mode").into_owned()
    };
    let tooltip = if state.has_data {
        t!("ui.armor.splash_tooltip_mode").into_owned()
    } else {
        t!("ui.armor.splash_no_data").into_owned()
    };

    crate::ui::selectable(
        "armor-splash",
        state.shown,
        Button::new("armor-splash-button")
            .child(crate::icons::icon(crate::icons::BOMB))
            .label(label)
            .compact()
            .selected(state.shown)
            .disabled(!state.has_data)
            .tooltip(tooltip)
            .on_click(move |_event, _window, cx: &mut App| {
                entity.update(cx, |view, cx| view.set_splash_mode(!state.shown, cx));
            }),
    )
}

/// Toolbar toggle for the splash-box outlines, which stand on their own: they
/// are worth seeing while placing a burst and after it.
fn render_splash_boxes_button(view: &ViewportView, entity: &Entity<ViewportView>) -> impl IntoElement + use<> {
    let state = view.splash_state();
    let entity = entity.clone();

    crate::ui::selectable(
        "armor-splash-boxes",
        state.boxes_shown,
        Button::new("armor-splash-boxes-button")
            .child(crate::icons::icon(crate::icons::CUBE))
            .label(t!("ui.armor.splash_toggle").to_string())
            .compact()
            .selected(state.boxes_shown)
            .disabled(!state.has_data)
            .tooltip(t!("ui.armor.splash_tooltip").to_string())
            .on_click(move |_event, _window, cx: &mut App| {
                entity.update(cx, |view, cx| view.set_show_splash_boxes(!state.boxes_shown, cx));
            }),
    )
}

/// Toolbar toggle for the options rail, which holds the visibility, hull and
/// display sections. `SidebarToggleButton` is the component the rest of the app
/// collapses a side panel with, and it picks the panel-right icons from `side`.
fn render_options_button(pane: Entity<ArmorViewerPane>, cx: &App) -> impl IntoElement + use<> {
    let open = pane.read(cx).options_open();

    // `SidebarToggleButton` carries no tooltip of its own; the panel-right
    // icons it picks are the app's own vocabulary for this.
    SidebarToggleButton::new().side(Side::Right).collapsed(!open).on_click(move |_event, _window, cx: &mut App| {
        pane.update(cx, |pane, cx| pane.toggle_options(cx));
    })
}

/// Toolbar toggle for the penetration checker, which belongs to the pane
/// rather than to this viewport: the egui app puts its own Pen Check button
/// in this same row (`ui/armor.pen_check`).
///
/// A toggle rather than a popover trigger. The verdicts are read against the
/// plate the pointer is on, so the panel has to stay up while the pointer is
/// on the hull; a popover closes on the first click and swallows the pointer
/// moves that would move the plate.
fn render_penetration_button(pane: Entity<ArmorViewerPane>, cx: &App) -> impl IntoElement + use<> {
    let open = pane.read(cx).analysis_open();
    let count = pane.read(cx).comparison_count();
    let label = if count > 0 {
        format!("{} ({})", t!("ui.armor.pen_check"), count)
    } else {
        t!("ui.armor.pen_check").into_owned()
    };

    crate::ui::selectable(
        "armor-analysis",
        open,
        Button::new("armor-analysis-toggle")
            .icon(IconName::Search)
            .label(label)
            .compact()
            .selected(open)
            // Not gated on a loaded hull, as the egui button is not: the
            // list is of attackers, and it is worth building before a target
            // is opened.
            .tooltip(t!("ui.armor.pen_check_tooltip").to_string())
            .on_click(move |_event, _window, cx: &mut App| {
                pane.update(cx, |pane, cx| pane.toggle_analysis(cx));
            }),
    )
}

/// Toolbar trigger for the export-confirm flow (Milestone 5 Task 10): opens
/// `ViewportView`'s confirm panel for the ship currently displayed in this
/// pane, at its current hull/LOD/module selection. Ports the egui app's
/// "Export Ship Model" tab button (`tab.rs:1696-1709`), which is likewise
/// gated on a ship being loaded.
fn render_export_button(view: &ViewportView, entity: &Entity<ViewportView>) -> impl IntoElement + use<> {
    let has_armor = view.current_armor.is_some();
    let click_entity = entity.clone();

    Button::new("armor-export-trigger")
        .icon(IconName::HardDrive)
        .label(t!("ui.replay.section_export").to_string())
        .compact()
        .tooltip(t!("ui.armor.export_tooltip").to_string())
        .disabled(!has_armor)
        .on_click(move |_, _window, cx| {
            click_entity.update(cx, |view, cx| view.open_export_confirm(cx));
        })
}

/// Builds the popover's whole tree from a snapshot of `entity`'s current
/// state, read once up front (see the module doc: the `.content()` closure
/// re-runs on every open-render, so this clone is cheap and short-lived, not
/// held across the click/hover closures built below).
pub(crate) fn render_popover_content(entity: &Entity<ViewportView>, cx: &mut App) -> AnyElement {
    let (armor, part_visibility, plate_visibility, expanded_zones, expanded_parts, scroll, show_zero_mm) = {
        let view = entity.read(cx);
        (
            view.current_armor.clone(),
            view.part_visibility.clone(),
            view.plate_visibility.clone(),
            view.expanded_zones.clone(),
            view.expanded_parts.clone(),
            view.popover_scroll.clone(),
            view.display_settings.show_zero_mm,
        )
    };

    let Some(armor) = armor else {
        return div()
            .text_sm()
            .text_color(crate::theme::text_dim())
            .p_2()
            .child(t!("ui.armor.no_ship_loaded").to_string())
            .into_any_element();
    };

    let warn = cx.theme().warning;
    let border = cx.theme().border;

    let all_entity = entity.clone();
    let none_entity = entity.clone();
    let reset_entity = entity.clone();
    let has_plate_overrides = !plate_visibility.is_empty();

    let header = h_flex()
        .flex_none()
        .gap_2()
        .items_center()
        .child(Button::new("armor-vis-all").label(t!("ui.armor.all_btn").to_string()).compact().on_click(
            move |_, _window, cx| {
                all_entity.update(cx, |view, cx| view.set_all_parts_visible(cx));
            },
        ))
        .child(Button::new("armor-vis-none").label(t!("ui.armor.none_btn").to_string()).compact().on_click(
            move |_, _window, cx| {
                none_entity.update(cx, |view, cx| view.set_all_parts_hidden(cx));
            },
        ))
        .when(has_plate_overrides, |row| {
            row.child(
                Button::new("armor-vis-reset-plates")
                    .label(t!("ui.armor.reset_plates").to_string())
                    .compact()
                    .on_click(move |_, _window, cx| {
                        reset_entity.update(cx, |view, cx| view.reset_plate_overrides(cx));
                    }),
            )
        });

    // A zone and a part are named the way the game names them, as the egui
    // popover does through its own `translate_part`.
    let bundle = entity.read(cx).reload_bundle();
    let translate = move |name: &str| match bundle.as_ref() {
        Some(bundle) => super::catalog::translate_part(bundle.assets.metadata(), name),
        None => name.to_string(),
    };

    let mut tree = v_flex().gap_1();
    for zone in &armor.zone_part_plates {
        tree = tree.child(render_zone_row(
            entity,
            zone,
            &part_visibility,
            &plate_visibility,
            &expanded_zones,
            &expanded_parts,
            warn,
            show_zero_mm,
            &translate,
        ));
    }

    let scroll_area =
        div().id("armor-visibility-tree-scroll").max_h(px(360.)).overflow_y_scroll().track_scroll(&scroll).child(tree);

    v_flex()
        .w(px(280.))
        .gap_2()
        .child(header)
        .child(div().h(px(1.)).bg(border))
        .child(div().relative().max_h(px(360.)).child(scroll_area).child(Scrollbar::vertical(&scroll)))
        .into_any_element()
}

/// Builds the hull-visibility popover's tree from a snapshot of `entity`'s
/// current state (same lazy-`.content()` rationale as
/// `render_popover_content`'s doc). Between the header and the hull tree,
/// adds -- in the same order as egui's `draw_hull_visibility_popover`
/// (`tab.rs:3135-3317`) -- the Milestone 4 Task 8c selectors: hull upgrade
/// (`render_hull_upgrade_row`, when the ship has more than one), module
/// alternatives (`render_module_alternative_row`, one row per component type
/// with more than one option), the camo picker (Milestone 4 Task 8b,
/// `render_camo_picker`, when `armor.camo_scheme_infos` is non-empty), and
/// LOD (`render_lod_row`, when the ship has more than one) -- the selectors
/// each select a new value on `ViewportView` and trigger a background reload
/// (`ViewportView::reload_ship`), the camo picker changes the active camo
/// directly (`ViewportView::select_camo`).
///
/// **Deferred.** The sidebar-hover highlight for a hovered hull row (egui's
/// `SidebarHighlightKey::HullMeshes`) is out of this port's scope -- see
/// `upload_hull.rs`'s module doc.
pub(crate) fn render_hull_popover_content(entity: &Entity<ViewportView>, cx: &mut App) -> AnyElement {
    let (
        armor,
        hull_visibility,
        hull_opaque,
        expanded_hull_groups,
        selected_camo,
        expanded_camo_groups,
        selected_hull,
        hull_lod,
        selected_modules,
    ) = {
        let view = entity.read(cx);
        (
            view.current_armor.clone(),
            view.hull_visibility.clone(),
            view.display_settings.hull_opaque,
            view.expanded_hull_groups.clone(),
            view.selected_camo,
            view.expanded_camo_groups.clone(),
            view.selected_hull.clone(),
            view.hull_lod,
            view.selected_modules.clone(),
        )
    };

    let Some(armor) = armor else {
        return div()
            .text_sm()
            .text_color(crate::theme::text_dim())
            .p_2()
            .child(t!("ui.armor.no_ship_loaded").to_string())
            .into_any_element();
    };

    let warn = cx.theme().warning;
    let border = cx.theme().border;

    let all_names: Vec<String> = armor.hull_part_groups.iter().flat_map(|(_, names)| names.clone()).collect();
    let all_entity = entity.clone();
    let all_names_for_all = all_names.clone();
    let none_entity = entity.clone();
    let opaque_entity = entity.clone();

    let header = h_flex()
        .flex_none()
        .gap_2()
        .items_center()
        .child(Button::new("armor-hull-vis-all").label(t!("ui.armor.all_btn").to_string()).compact().on_click(
            move |_, _window, cx| {
                let names = all_names_for_all.clone();
                all_entity.update(cx, |view, cx| {
                    view.mutate_hull_visibility(cx, |hull_visibility| {
                        for name in names {
                            hull_visibility.insert(name, true);
                        }
                    })
                });
            },
        ))
        .child(Button::new("armor-hull-vis-none").label(t!("ui.armor.none_btn").to_string()).compact().on_click(
            move |_, _window, cx| {
                let names = all_names.clone();
                none_entity.update(cx, |view, cx| {
                    view.mutate_hull_visibility(cx, |hull_visibility| {
                        for name in names {
                            hull_visibility.insert(name, false);
                        }
                    })
                });
            },
        ))
        .child(
            Checkbox::new("armor-hull-vis-opaque")
                .label(t!("ui.armor.opaque").to_string())
                .checked(hull_opaque)
                .on_click(move |checked, _window, cx| {
                    let checked = *checked;
                    opaque_entity.update(cx, |view, cx| view.set_hull_opaque(checked, cx));
                }),
        );

    let mut col = v_flex().w(px(260.)).gap_2().child(header);

    if armor.hull_upgrade_names.len() > 1 {
        col = col.child(render_hull_upgrade_row(entity, &armor, &selected_hull));
    }
    for (ct, alternatives) in &armor.module_alternatives {
        if alternatives.len() > 1 {
            col = col.child(render_module_alternative_row(entity, *ct, alternatives, selected_modules.get(ct)));
        }
    }

    // Matches egui's `draw_hull_visibility_popover` order (`tab.rs:3135-
    // 3317`): header, hull upgrade, module alternatives, camo, LOD, then the
    // hull tree.
    if !armor.camo_scheme_infos.is_empty() {
        col = col.child(div().h(px(1.)).bg(border)).child(render_camo_picker(
            entity,
            &armor,
            selected_camo,
            &expanded_camo_groups,
        ));
    }

    if armor.hull_lod_count > 1 {
        col = col.child(render_lod_row(entity, &armor, hull_lod));
    }

    let mut tree = v_flex().gap_1();
    for (group, names) in &armor.hull_part_groups {
        tree = tree.child(render_hull_group_row(entity, group, names, &hull_visibility, &expanded_hull_groups, warn));
    }

    col = col
        .child(div().h(px(1.)).bg(border))
        .child(div().id("armor-hull-visibility-tree-scroll").max_h(px(360.)).overflow_y_scroll().child(tree));

    col.into_any_element()
}

/// Hull-upgrade selector row: a "Hull:" label followed by one selectable
/// toggle per hull upgrade (`armor.hull_upgrade_names`, key -> label).
/// Selecting a different key switches to it (`ViewportView::select_hull_upgrade`,
/// which also clears `selected_modules` and triggers a reload). Ports the
/// egui `selectable_label` row (`tab.rs:3163-3175`); `selected_hull.is_none()`
/// is treated as the first (alphabetically stock) entry, matching the egui
/// original's own fallback.
fn render_hull_upgrade_row(
    entity: &Entity<ViewportView>,
    armor: &LoadedShipArmor,
    selected_hull: &Option<String>,
) -> AnyElement {
    let stock_key = &armor.hull_upgrade_names[0].0;
    let mut row = h_flex()
        .gap_1()
        .items_center()
        .flex_wrap()
        .child(div().text_sm().flex_none().child(t!("ui.armor.upgrade").to_string()));
    for (key, label) in &armor.hull_upgrade_names {
        let is_selected = selected_hull.as_ref().map(|sel| sel == key).unwrap_or(key == stock_key);
        let toggle_entity = entity.clone();
        let toggle_key = key.clone();
        row = row.child(
            Toggle::new(format!("armor-hull-upgrade-{key}"))
                .label(label.clone())
                .with_size(Size::XSmall)
                .checked(is_selected)
                .on_click(move |_, _window, cx| {
                    if is_selected {
                        return;
                    }
                    let key = toggle_key.clone();
                    toggle_entity.update(cx, |view, cx| view.select_hull_upgrade(key, cx));
                }),
        );
    }
    row.into_any_element()
}

/// One module-alternative selector row for component type `ct`: a "{ct}:"
/// label followed by one selectable toggle per alternative, labeled
/// "{ct} {letter}" with the full component name as its tooltip. Selecting a
/// different alternative overrides it (`ViewportView::select_module_alternative`,
/// which triggers a reload). Ports the egui `selectable_label` row
/// (`tab.rs:3178-3190`); `selected.is_none()` is treated as the first entry,
/// matching the egui original's own fallback.
fn render_module_alternative_row(
    entity: &Entity<ViewportView>,
    ct: ComponentType,
    alternatives: &[String],
    selected: Option<&String>,
) -> AnyElement {
    let mut row =
        h_flex().gap_1().items_center().flex_wrap().child(div().text_sm().flex_none().child(format!("{ct}:")));
    for (i, name) in alternatives.iter().enumerate() {
        let is_selected = selected.map(|sel| sel == name).unwrap_or(i == 0);
        let letter = (b'A' + i as u8) as char;
        let toggle_entity = entity.clone();
        let toggle_name = name.clone();
        row = row.child(
            Toggle::new(format!("armor-module-alt-{ct}-{i}"))
                .label(format!("{ct} {letter}"))
                .tooltip(name.clone())
                .with_size(Size::XSmall)
                .checked(is_selected)
                .on_click(move |_, _window, cx| {
                    if is_selected {
                        return;
                    }
                    let name = toggle_name.clone();
                    toggle_entity.update(cx, |view, cx| view.select_module_alternative(ct, name, cx));
                }),
        );
    }
    row.into_any_element()
}

/// LOD selector row: a "LOD:" label followed by one selectable toggle per
/// level `0..armor.hull_lod_count` (`0` labeled "0 (highest)", matching the
/// egui original). Selecting a different level switches to it
/// (`ViewportView::select_hull_lod`, which triggers a reload). Ports the egui
/// `selectable_label` row (`tab.rs:3253-3264`).
fn render_lod_row(entity: &Entity<ViewportView>, armor: &LoadedShipArmor, hull_lod: usize) -> AnyElement {
    let mut row = h_flex()
        .gap_1()
        .items_center()
        .flex_wrap()
        .child(div().text_sm().flex_none().child(t!("ui.armor.lod").to_string()));
    for i in 0..armor.hull_lod_count {
        let label = if i == 0 { t!("ui.armor.lod_highest").into_owned() } else { i.to_string() };
        let is_selected = hull_lod == i;
        let toggle_entity = entity.clone();
        row = row.child(
            Toggle::new(format!("armor-hull-lod-{i}"))
                .label(label)
                .with_size(Size::XSmall)
                .checked(is_selected)
                .on_click(move |_, _window, cx| {
                    if is_selected {
                        return;
                    }
                    toggle_entity.update(cx, |view, cx| view.select_hull_lod(i, cx));
                }),
        );
    }
    row.into_any_element()
}

/// Camo picker: "Stock" (deselects the active camo) + ship-specific schemes
/// at top level, then collapsible Universal/Expendable/LegacyScan groups
/// (each sorted by lowercased display name, skipped when empty). Ports the
/// egui camo selector (`tab.rs:3196-3251`). The caller hides this entirely
/// when `armor.camo_scheme_infos` is empty (old game versions carry no camo
/// metadata).
fn render_camo_picker(
    entity: &Entity<ViewportView>,
    armor: &LoadedShipArmor,
    selected_camo: Option<CamoSchemeId>,
    expanded_camo_groups: &HashSet<String>,
) -> AnyElement {
    let mut col = v_flex()
        .gap_1()
        .child(div().text_sm().font_weight(FontWeight::BOLD).child(t!("ui.armor.camouflage").to_string()));

    let none_entity = entity.clone();
    col = col.child(
        ListItem::new("armor-camo-none")
            .selected(selected_camo.is_none())
            .child(div().text_sm().child(t!("ui.armor.camo_none").to_string()))
            .on_click(move |_, _window, cx| {
                none_entity.update(cx, |view, cx| view.select_camo(None, cx));
            }),
    );

    let mut ship_infos: Vec<&CamoSchemeInfo> =
        armor.camo_scheme_infos.iter().filter(|i| i.origin == CamoOrigin::ShipSpecific).collect();
    ship_infos.sort_by_key(|i| i.display_name.to_lowercase());
    for info in &ship_infos {
        col = col.child(render_camo_row(entity, info, selected_camo));
    }

    for (origin, label) in [
        (CamoOrigin::Universal, "ui.armor.camo_group_universal"),
        (CamoOrigin::Expendable, "ui.armor.camo_group_expendable"),
        (CamoOrigin::LegacyScan, "ui.armor.camo_group_other"),
    ] {
        let mut group: Vec<&CamoSchemeInfo> = armor.camo_scheme_infos.iter().filter(|i| i.origin == origin).collect();
        if group.is_empty() {
            continue;
        }
        group.sort_by_key(|i| i.display_name.to_lowercase());
        col = col.child(render_camo_group(entity, &t!(label), &group, selected_camo, expanded_camo_groups));
    }

    col.into_any_element()
}

/// One selectable camo-scheme row. Ports one `selectable_label` iteration of
/// `tab.rs:3216`/`3243`.
fn render_camo_row(
    entity: &Entity<ViewportView>,
    info: &CamoSchemeInfo,
    selected_camo: Option<CamoSchemeId>,
) -> AnyElement {
    let id = info.id;
    let is_selected = selected_camo == Some(id);
    let toggle_entity = entity.clone();
    ListItem::new(format!("armor-camo-{}", id.0))
        .selected(is_selected)
        .child(div().text_sm().child(info.display_name.clone()))
        .on_click(move |_, _window, cx| {
            toggle_entity.update(cx, |view, cx| view.select_camo(Some(id), cx));
        })
        .into_any_element()
}

/// One collapsible camo-origin group header row + (if expanded) its scheme
/// rows, scroll-clamped to match egui's own per-group `ScrollArea::max_height(240.0)`.
/// Ports the `CollapsingState` block (`tab.rs:3234-3250`).
fn render_camo_group(
    entity: &Entity<ViewportView>,
    label: &str,
    infos: &[&CamoSchemeInfo],
    selected_camo: Option<CamoSchemeId>,
    expanded_camo_groups: &HashSet<String>,
) -> AnyElement {
    let expanded = expanded_camo_groups.contains(label);

    let expand_entity = entity.clone();
    let expand_label = label.to_string();
    let chevron = div()
        .id(format!("armor-camo-group-expand-{label}"))
        .flex_none()
        .child(Icon::new(if expanded { IconName::ChevronDown } else { IconName::ChevronRight }))
        .on_click(move |_, _window, cx| {
            expand_entity.update(cx, |view, cx| view.toggle_camo_group_expanded(expand_label.clone(), cx));
        });

    let header = h_flex()
        .id(format!("armor-camo-group-row-{label}"))
        .gap_1()
        .items_center()
        .child(chevron)
        .child(div().text_sm().child(label.to_string()));

    let mut column = v_flex().gap_1().child(header);
    if expanded {
        let mut inner = v_flex().gap_1();
        for info in infos {
            inner = inner.child(render_camo_row(entity, info, selected_camo));
        }
        let body = div()
            .id(format!("armor-camo-group-scroll-{label}"))
            .max_h(px(240.))
            .overflow_y_scroll()
            .pl(px(20.))
            .child(inner);
        column = column.child(body);
    }
    column.into_any_element()
}

/// One hull-part-group header row + (if expanded) its individual mesh
/// checkboxes. Ports the group `CollapsingState` block
/// (`tab.rs:3272-3312`).
fn render_hull_group_row(
    entity: &Entity<ViewportView>,
    group: &str,
    names: &[String],
    hull_visibility: &HashMap<String, bool>,
    expanded_hull_groups: &HashSet<String>,
    warn: Hsla,
) -> AnyElement {
    let all_on = hull_group_all_on(hull_visibility, names);
    let any_on = hull_group_any_on(hull_visibility, names);
    let state = TriState::from_all_any(all_on, any_on);
    let expanded = expanded_hull_groups.contains(group);

    let toggle_entity = entity.clone();
    let toggle_names = names.to_vec();
    let checkbox = tri_checkbox(format!("armor-hull-vis-group-{group}"), state, warn, move |checked, _window, cx| {
        let checked = *checked;
        let names = toggle_names.clone();
        toggle_entity.update(cx, |view, cx| {
            view.mutate_hull_visibility(cx, |hull_visibility| {
                for name in names {
                    hull_visibility.insert(name, checked);
                }
            })
        });
    });

    let expand_entity = entity.clone();
    let expand_group = group.to_string();
    let chevron = div()
        .id(format!("armor-hull-vis-group-expand-{group}"))
        .flex_none()
        .child(Icon::new(if expanded { IconName::ChevronDown } else { IconName::ChevronRight }))
        .on_click(move |_, _window, cx| {
            expand_entity.update(cx, |view, cx| view.toggle_hull_group_expanded(expand_group.clone(), cx));
        });

    let header = h_flex()
        .id(format!("armor-hull-vis-group-row-{group}"))
        .gap_1()
        .items_center()
        .child(chevron)
        .child(checkbox)
        .child(div().text_sm().child(group.to_string()));

    let mut column = v_flex().gap_1().child(header);
    if expanded {
        let mut body = v_flex().gap_1().pl(px(20.));
        for name in names {
            body = body.child(render_hull_mesh_row(entity, name, hull_visibility));
        }
        column = column.child(body);
    }
    column.into_any_element()
}

/// One hull-mesh checkbox row. Ports `tab.rs:3300-3310`.
fn render_hull_mesh_row(
    entity: &Entity<ViewportView>,
    name: &str,
    hull_visibility: &HashMap<String, bool>,
) -> AnyElement {
    let visible = hull_visibility.get(name).copied().unwrap_or(false);
    let toggle_entity = entity.clone();
    let toggle_name = name.to_string();
    Checkbox::new(format!("armor-hull-vis-mesh-{name}"))
        .label(name.to_string())
        .checked(visible)
        .on_click(move |checked, _window, cx| {
            let checked = *checked;
            let name = toggle_name.clone();
            toggle_entity.update(cx, |view, cx| {
                view.mutate_hull_visibility(cx, |hull_visibility| {
                    hull_visibility.insert(name, checked);
                })
            });
        })
        .into_any_element()
}

/// Builds the display-settings popover's content from a snapshot of
/// `entity`'s current state (same lazy-`.content()` rationale as
/// `render_popover_content`'s doc). Reproduces the egui app's
/// `draw_display_settings_popover` V1 subset in order: plate edges,
/// waterline (+ its own opacity slider when on), zero-mm plates, armor
/// opacity, then the Hull Lighting section (enabled, In-Game/Flat/Studio
/// presets, and the flat/key intensity, azimuth, elevation, rim, specular,
/// and shininess sliders). Every slider here is a persistent `Entity<
/// SliderState>` already wired (in `viewport_view.rs`'s `build_display_
/// sliders`/`build_lighting_sliders`) to mutate the right field on its own
/// `SliderEvent::Change`, so this function only needs to render each one
/// alongside a live numeric readout -- it never attaches its own change
/// handler to a slider the way the checkboxes below attach `on_click`.
///
/// **Deferred** (later milestones or out of this task's scope, matching the
/// M3 Task 7b brief): the ship-center toggle and the hull-visibility master
/// toggle (the egui original's duplicate `hull_opaque` checkbox here,
/// `tab.rs:4911`, is likewise not mirrored -- the Hull popover's own
/// `hull_opaque` checkbox, `render_hull_popover_content`, is this port's only
/// control for it), the entire camera-rings/camera-perspective analysis section
/// (`tab.rs:4924-5043`), the save-defaults button (this port has no settings
/// write-back path yet -- same limitation `legend.rs`'s `end_legend_drag`
/// documents), the key/ambient color pickers (`gpui_kit::component::color_picker`
/// exists, but wiring two more persistent picker entities is left for later
/// polish -- the flat-intensity slider already covers the egui popover's
/// numeric ambient-intensity control), and the transient light-source marker
/// (`light_moved`/`light_changed_at`) the egui app shows for a few seconds
/// after a light change.
/// What the display popover draws, read off the viewport in one pass.
///
/// Taken as a snapshot because the content builder runs while the popover is
/// open and must not hold a borrow of the view it came from.
struct DisplayPopoverSnapshot {
    display: super::upload::DisplaySettings,
    lighting: LightingSettings,
    waterline_slider: Entity<SliderState>,
    marker_slider: Entity<SliderState>,
    /// Whether any impact marker is drawn, since a marker control with nothing
    /// to act on is what the egui popover hides.
    has_markers: bool,
    armor_slider: Entity<SliderState>,
    roll_slider: Entity<SliderState>,
    /// How far the hull is currently heeled over, for the readout beside its
    /// slider.
    roll_deg: f32,
    trajectory: TrajectoryState,
    /// The range cast shells are fired from, for the readout beside its
    /// slider.
    cast_range: Km,
    cast_range_slider: Entity<SliderState>,
    camera_rings: CameraRingSettings,
    /// The modes this ship's own GameParams name.
    camera_modes: Vec<String>,
    camera_fov_slider: Entity<SliderState>,
    camera_height_slider: Entity<SliderState>,
    /// Whether the camera is locked to the ship's own orbit, and how it aims
    /// from there.
    perspective: PerspectiveSettings,
    perspective_fov_slider: Entity<SliderState>,
    flat_slider: Entity<SliderState>,
    /// The colour each of the two lighting terms is tinted by. A picker keeps its
    /// own state, so these are the view's own rather than rebuilt per frame.
    flat_color: Entity<gpui_kit::component::color_picker::ColorPickerState>,
    key_color: Entity<gpui_kit::component::color_picker::ColorPickerState>,
    key_slider: Entity<SliderState>,
    azimuth_slider: Entity<SliderState>,
    elevation_slider: Entity<SliderState>,
    rim_slider: Entity<SliderState>,
    specular_slider: Entity<SliderState>,
    shininess_slider: Entity<SliderState>,
}

/// `legend_visible` is passed in rather than read off the pane here: the
/// options rail renders inside `ArmorViewerPane`'s own render, and reading the
/// pane entity there panics on the borrow it already holds.
pub(crate) fn render_display_popover_content(
    entity: &Entity<ViewportView>,
    legend_visible: bool,
    cx: &mut App,
) -> AnyElement {
    // The trigger button carried `.disabled(!has_armor)`; the rail has no
    // button, so the guard the other two sections already had belongs here.
    // The egui app draws none of the three without a loaded hull
    // (`ui/tab.rs`'s `if let Some(armor) = pane.loaded_armor.take()`).
    if !entity.read(cx).has_armor() {
        return div()
            .text_sm()
            .text_color(crate::theme::text_dim())
            .p_2()
            .child(t!("ui.armor.no_ship_loaded").to_string())
            .into_any_element();
    }

    let pane = entity.read(cx).pane();
    let snapshot = {
        let view = entity.read(cx);
        DisplayPopoverSnapshot {
            display: view.display_settings,
            lighting: view.lighting(),
            waterline_slider: view.display_sliders.waterline_opacity.clone(),
            armor_slider: view.display_sliders.armor_opacity.clone(),
            roll_slider: view.display_sliders.model_roll_deg.clone(),
            roll_deg: view.model_roll_deg(),
            camera_rings: view.camera_rings(),
            camera_modes: view.camera_modes(),
            trajectory: view.trajectory_state(),
            cast_range: view.cast_range(),
            cast_range_slider: view.display_sliders.cast_range.clone(),
            camera_fov_slider: view.display_sliders.camera_fov.clone(),
            camera_height_slider: view.display_sliders.camera_height.clone(),
            perspective: view.perspective(),
            perspective_fov_slider: view.display_sliders.perspective_fov.clone(),
            marker_slider: view.display_sliders.marker_opacity.clone(),
            has_markers: view.hits_drawn() > 0,
            flat_slider: view.lighting_sliders.flat_intensity.clone(),
            flat_color: view.lighting_colors.flat.clone(),
            key_color: view.lighting_colors.key.clone(),
            key_slider: view.lighting_sliders.key_intensity.clone(),
            azimuth_slider: view.lighting_sliders.azimuth_deg.clone(),
            elevation_slider: view.lighting_sliders.elevation_deg.clone(),
            rim_slider: view.lighting_sliders.rim_strength.clone(),
            specular_slider: view.lighting_sliders.specular_strength.clone(),
            shininess_slider: view.lighting_sliders.shininess.clone(),
        }
    };
    let DisplayPopoverSnapshot {
        display,
        lighting,
        camera_rings,
        camera_modes,
        trajectory,
        cast_range,
        cast_range_slider,
        camera_fov_slider,
        camera_height_slider,
        perspective,
        perspective_fov_slider,
        waterline_slider,
        marker_slider,
        has_markers,
        armor_slider,
        roll_slider,
        roll_deg,
        flat_slider,
        flat_color,
        key_color,
        key_slider,
        azimuth_slider,
        elevation_slider,
        rim_slider,
        specular_slider,
        shininess_slider,
    } = snapshot;

    let border = cx.theme().border;

    let edges_entity = entity.clone();
    let waterline_entity = entity.clone();
    let zero_mm_entity = entity.clone();
    let ship_center_entity = entity.clone();
    let lighting_enabled_entity = entity.clone();
    let preset_ingame_entity = entity.clone();
    let preset_flat_entity = entity.clone();
    let preset_studio_entity = entity.clone();

    let mut col = v_flex()
        .w(px(260.))
        .gap_2()
        // The legend belongs to the pane, not to this viewport, but the
        // reader looks for it here: the egui display popover carries the same
        // checkbox (`ui.armor.show_armor_thickness`).
        .when_some(pane, |this, pane| {
            this.child(
                Checkbox::new("armor-display-legend")
                    .label(t!("ui.armor.show_armor_thickness").to_string())
                    .checked(legend_visible)
                    .on_click(move |checked: &bool, _window, cx: &mut App| {
                        let checked = *checked;
                        pane.update(cx, |pane, cx| pane.set_legend_visible(checked, cx));
                    }),
            )
        })
        .child(
            Checkbox::new("armor-display-plate-edges")
                .label(t!("ui.armor.plate_edges").to_string())
                .checked(display.show_plate_edges)
                .on_click(move |checked, _window, cx| {
                    let checked = *checked;
                    edges_entity
                        .update(cx, |view, cx| view.mutate_display_settings(cx, |d| d.show_plate_edges = checked));
                }),
        )
        .child(
            Checkbox::new("armor-display-waterline")
                .label(t!("ui.armor.waterline").to_string())
                .checked(display.show_waterline)
                .on_click(move |checked, _window, cx| {
                    let checked = *checked;
                    waterline_entity
                        .update(cx, |view, cx| view.mutate_display_settings(cx, |d| d.show_waterline = checked));
                }),
        );

    if display.show_waterline {
        col = col.child(div().pl(px(20.)).child(labeled_slider_row(
            t!("ui.armor.opacity").into_owned(),
            &waterline_slider,
            display.waterline_opacity,
            false,
        )));
    }

    col = col
        .child(
            Checkbox::new("armor-display-zero-mm")
                .label(t!("ui.armor.zero_mm_plates").to_string())
                .checked(display.show_zero_mm)
                .on_click(move |checked, _window, cx| {
                    let checked = *checked;
                    zero_mm_entity
                        .update(cx, |view, cx| view.mutate_display_settings(cx, |d| d.show_zero_mm = checked));
                }),
        )
        .child(
            Checkbox::new("armor-display-ship-center")
                .label(t!("ui.armor.ship_center").to_string())
                .checked(display.show_ship_center)
                .on_click(move |checked, _window, cx| {
                    let checked = *checked;
                    ship_center_entity
                        .update(cx, |view, cx| view.mutate_display_settings(cx, |d| d.show_ship_center = checked));
                }),
        )
        .child(labeled_slider_row(
            t!("ui.armor.armor_opacity").into_owned(),
            &armor_slider,
            display.armor_opacity,
            false,
        ))
        .when(has_markers, |this| {
            this.child(labeled_slider_row(
                t!("ui.armor.marker_opacity").into_owned(),
                &marker_slider,
                display.marker_opacity,
                false,
            ))
        })
        // Heeling the hull over is what says whether a belt is still a belt
        // at the angle the ship is fighting at.
        .child(labeled_slider_row(t!("ui.armor.roll").into_owned(), &roll_slider, roll_deg, false))
        .child(div().h(px(1.)).bg(border))
        .child(render_trajectory_section(entity, &trajectory, cast_range, &cast_range_slider))
        .child(div().h(px(1.)).bg(border))
        .child(render_camera_rings_section(
            entity,
            &camera_rings,
            &camera_modes,
            &perspective,
            &camera_fov_slider,
            &camera_height_slider,
            &perspective_fov_slider,
        ))
        .child(div().h(px(1.)).bg(border))
        .child(div().text_sm().font_weight(FontWeight::BOLD).child(t!("ui.armor.lighting").to_string()))
        .child(
            Checkbox::new("armor-display-lighting-enabled")
                .label(t!("ui.armor.lighting_enabled").to_string())
                .checked(lighting.enabled)
                .on_click(move |checked, _window, cx| {
                    let checked = *checked;
                    lighting_enabled_entity.update(cx, |view, cx| view.mutate_lighting(cx, |l| l.enabled = checked));
                }),
        )
        .child(
            h_flex()
                .gap_1()
                .child(
                    Button::new("armor-lighting-preset-ingame")
                        .label(t!("ui.armor.lighting_preset_ingame").to_string())
                        .compact()
                        .disabled(!lighting.enabled)
                        .on_click(move |_, window, cx| {
                            preset_ingame_entity.update(cx, |view, cx| {
                                view.set_lighting_preset(LightingSettings::in_game(), window, cx)
                            });
                        }),
                )
                .child(
                    Button::new("armor-lighting-preset-flat")
                        .label(t!("ui.armor.lighting_preset_flat").to_string())
                        .compact()
                        .disabled(!lighting.enabled)
                        .on_click(move |_, window, cx| {
                            preset_flat_entity
                                .update(cx, |view, cx| view.set_lighting_preset(LightingSettings::flat(), window, cx));
                        }),
                )
                .child(
                    Button::new("armor-lighting-preset-studio")
                        .label(t!("ui.armor.lighting_preset_studio").to_string())
                        .compact()
                        .disabled(!lighting.enabled)
                        .on_click(move |_, window, cx| {
                            preset_studio_entity.update(cx, |view, cx| {
                                view.set_lighting_preset(LightingSettings::studio(), window, cx)
                            });
                        }),
                ),
        )
        .child(labeled_slider_row(
            t!("ui.armor.lighting_flat").into_owned(),
            &flat_slider,
            lighting.flat_intensity,
            !lighting.enabled,
        ))
        .child(labeled_color_row(t!("ui.armor.lighting_ambient_color").into_owned(), &flat_color, !lighting.enabled))
        .child(labeled_slider_row(
            t!("ui.armor.lighting_intensity").into_owned(),
            &key_slider,
            lighting.key_intensity,
            !lighting.enabled,
        ))
        .child(labeled_color_row(t!("ui.armor.lighting_key_color").into_owned(), &key_color, !lighting.enabled))
        .child(labeled_slider_row(
            t!("ui.armor.lighting_azimuth").into_owned(),
            &azimuth_slider,
            lighting.azimuth_deg,
            !lighting.enabled,
        ))
        .child(labeled_slider_row(
            t!("ui.armor.lighting_elevation").into_owned(),
            &elevation_slider,
            lighting.elevation_deg,
            !lighting.enabled,
        ))
        .child(labeled_slider_row(
            t!("ui.armor.lighting_rim").into_owned(),
            &rim_slider,
            lighting.rim_strength,
            !lighting.enabled,
        ))
        .child(labeled_slider_row(
            t!("ui.armor.lighting_specular").into_owned(),
            &specular_slider,
            lighting.specular_strength,
            !lighting.enabled,
        ))
        .child(labeled_slider_row(
            t!("ui.armor.lighting_shininess").into_owned(),
            &shininess_slider,
            lighting.shininess,
            !lighting.enabled,
        ));

    col.into_any_element()
}

/// One labelled colour row: what it tints, and the picker that sets it.
fn labeled_color_row(
    label: String,
    picker: &Entity<gpui_kit::component::color_picker::ColorPickerState>,
    lighting_off: bool,
) -> impl IntoElement + use<> {
    // Dimmed rather than taken away when the lighting is off: the colour it would
    // be lit in is still what this sets, and a row that vanished would read as a
    // setting that had gone.
    h_flex()
        .gap_2()
        .items_center()
        .when(lighting_off, |row| row.opacity(0.5))
        .child(div().w(px(96.)).text_xs().child(label.clone()))
        .child(
            gpui_kit::component::color_picker::ColorPicker::new(picker)
                .small()
                .accessibility_label(SharedString::from(label)),
        )
}

/// One labeled slider row: `label`, the slider itself, and a live numeric
/// readout. `slider` is always a persistent `Entity<SliderState>` already
/// subscribed (see `render_display_popover_content`'s doc) to mutate the
/// right domain field on drag, so this is purely presentational. `disabled`
/// matches the egui original's `ui.add_enabled_ui(pane.lighting.enabled, ..)`
/// wrapping the lighting sliders (`tab.rs:5053`); always `false` for the
/// waterline/armor-opacity rows, which aren't lighting-gated.
fn labeled_slider_row(label: String, slider: &Entity<SliderState>, value: f32, disabled: bool) -> AnyElement {
    h_flex()
        .gap_2()
        .items_center()
        .child(div().text_sm().w(px(110.)).child(label))
        .child(crate::ui::boxed(px(110.), crate::ui::SELECT_SMALL_HEIGHT).child(Slider::new(slider).disabled(disabled)))
        .child(div().text_sm().w(px(40.)).child(format!("{value:.2}")))
        .into_any_element()
}

/// One zone header row + (if expanded) its parts. Ports the zone
/// `CollapsingState` block (`tab.rs:4470-4608`).
#[allow(clippy::too_many_arguments)]
fn render_zone_row(
    entity: &Entity<ViewportView>,
    zone: &ArmorZone,
    part_visibility: &HashMap<(String, String), bool>,
    plate_visibility: &HashMap<PlateKey, bool>,
    expanded_zones: &HashSet<String>,
    expanded_parts: &HashSet<(String, String)>,
    warn: Hsla,
    show_zero_mm: bool,
    translate: &dyn Fn(&str) -> String,
) -> AnyElement {
    let all_on = zone_all_on(zone, part_visibility, plate_visibility, show_zero_mm);
    let any_on = zone_any_on(zone, part_visibility);
    let state = TriState::from_all_any(all_on, any_on);
    let expanded = expanded_zones.contains(&zone.name);

    let toggle_entity = entity.clone();
    let toggle_zone_name = zone.name.clone();
    let checkbox = tri_checkbox(format!("armor-vis-zone-{}", zone.name), state, warn, move |checked, window, cx| {
        let solo = window.modifiers().secondary();
        toggle_entity.update(cx, |view, cx| view.toggle_zone(toggle_zone_name.clone(), *checked, solo, cx));
    });

    let expand_entity = entity.clone();
    let expand_zone_name = zone.name.clone();
    let chevron = div()
        .id(format!("armor-vis-zone-expand-{}", zone.name))
        .flex_none()
        .child(Icon::new(if expanded { IconName::ChevronDown } else { IconName::ChevronRight }))
        .on_click(move |_, _window, cx| {
            expand_entity.update(cx, |view, cx| view.toggle_zone_expanded(expand_zone_name.clone(), cx));
        });

    // Matches egui's own `ctrl_click_solo` hover-text on the zone label
    // (`tab.rs:4512`); generic rows have no public tooltip hook in this
    // port's widget set, so the hint is a plain muted label instead.
    let header = hoverable_row(
        format!("armor-vis-zone-row-{}", zone.name),
        entity,
        SidebarHighlightKey::Zone(zone.name.clone()),
        h_flex()
            .gap_1()
            .items_center()
            .child(chevron)
            .child(checkbox)
            .child(div().text_sm().child(translate(&zone.name)))
            .child(
                div()
                    .text_xs()
                    .text_color(crate::theme::text_faint())
                    .child(t!("ui.armor.ctrl_click_solo").to_string()),
            ),
    );

    let mut column = v_flex().gap_1().child(header);
    if expanded {
        let mut body = v_flex().gap_1().pl(px(20.));
        for part in &zone.parts {
            body = body.child(render_part_row(
                entity,
                &zone.name,
                part,
                part_visibility,
                plate_visibility,
                expanded_parts,
                warn,
                show_zero_mm,
                translate,
            ));
        }
        column = column.child(body);
    }
    column.into_any_element()
}

/// One material/part row: a single checkbox if it has at most one visible
/// plate thickness, otherwise a collapsible header + plate rows. Ports
/// `tab.rs:4518-4606`.
#[allow(clippy::too_many_arguments)]
fn render_part_row(
    entity: &Entity<ViewportView>,
    zone_name: &str,
    part: &ZonePart,
    part_visibility: &HashMap<(String, String), bool>,
    plate_visibility: &HashMap<PlateKey, bool>,
    expanded_parts: &HashSet<(String, String)>,
    warn: Hsla,
    show_zero_mm: bool,
    translate: &dyn Fn(&str) -> String,
) -> AnyElement {
    let part_key = (zone_name.to_string(), part.name.clone());
    let part_visible = part_on(part_visibility, zone_name, &part.name);
    let any_plate_hidden = part_any_plate_hidden(zone_name, part, plate_visibility, show_zero_mm);
    let visible_plates: Vec<i32> = part.plates.iter().copied().filter(|&t| show_zero_mm || t != 0).collect();

    let toggle_entity = entity.clone();
    let toggle_zone_name = zone_name.to_string();
    let toggle_part_name = part.name.clone();
    // Matches the egui original (`tab.rs:4538-4540`, `4565-4567`): clearing
    // overrides for the `show_zero_mm`-filtered plate set, not every plate
    // on the part, so an override on a thickness currently hidden by the
    // zero-mm filter survives a checkbox toggle.
    let toggle_plates = visible_plates.clone();

    if visible_plates.len() <= 1 {
        let checked = part_visible && !any_plate_hidden;
        let checkbox = tri_checkbox(
            format!("armor-vis-part-{}-{}", zone_name, part.name),
            TriState::from_all_any(checked, checked),
            warn,
            move |checked, _window, cx| {
                toggle_entity.update(cx, |view, cx| {
                    view.toggle_part(
                        toggle_zone_name.clone(),
                        toggle_part_name.clone(),
                        toggle_plates.clone(),
                        *checked,
                        cx,
                    )
                });
            },
        );
        return hoverable_row(
            format!("armor-vis-part-row-{}-{}", zone_name, part.name),
            entity,
            SidebarHighlightKey::Part(zone_name.to_string(), part.name.clone()),
            h_flex().gap_1().items_center().child(checkbox).child(div().text_sm().child(translate(&part.name))),
        )
        .into_any_element();
    }

    let expanded = expanded_parts.contains(&part_key);
    let state = TriState::from_all_any(part_visible && !any_plate_hidden, part_visible);
    let checkbox = tri_checkbox(
        format!("armor-vis-part-{}-{}", zone_name, part.name),
        state,
        warn,
        move |checked, _window, cx| {
            toggle_entity.update(cx, |view, cx| {
                view.toggle_part(
                    toggle_zone_name.clone(),
                    toggle_part_name.clone(),
                    toggle_plates.clone(),
                    *checked,
                    cx,
                )
            });
        },
    );

    let expand_entity = entity.clone();
    let expand_key = part_key.clone();
    let chevron = div()
        .id(format!("armor-vis-part-expand-{}-{}", zone_name, part.name))
        .flex_none()
        .child(Icon::new(if expanded { IconName::ChevronDown } else { IconName::ChevronRight }))
        .on_click(move |_, _window, cx| {
            expand_entity.update(cx, |view, cx| view.toggle_part_expanded(expand_key.clone(), cx));
        });

    let header = hoverable_row(
        format!("armor-vis-part-row-{}-{}", zone_name, part.name),
        entity,
        SidebarHighlightKey::Part(zone_name.to_string(), part.name.clone()),
        h_flex()
            .gap_1()
            .items_center()
            .child(chevron)
            .child(checkbox)
            .child(div().text_sm().child(translate(&part.name))),
    );

    let mut column = v_flex().gap_1().child(header);
    if expanded {
        let mut body = v_flex().gap_1().pl(px(20.));
        for &thickness in &visible_plates {
            body = body.child(render_plate_row(
                entity,
                zone_name,
                &part.name,
                thickness,
                part_visible,
                plate_visibility,
                warn,
            ));
        }
        column = column.child(body);
    }
    column.into_any_element()
}

/// One plate checkbox row: a thickness-color swatch + "{mm} mm" checkbox.
/// Ports `tab.rs:4576-4605`.
fn render_plate_row(
    entity: &Entity<ViewportView>,
    zone_name: &str,
    part_name: &str,
    thickness_tenths: i32,
    part_visible: bool,
    plate_visibility: &HashMap<PlateKey, bool>,
    warn: Hsla,
) -> AnyElement {
    let key = PlateKey { zone: zone_name.to_string(), material_name: part_name.to_string(), thickness_tenths };
    let plate_visible = !plate_explicitly_hidden(plate_visibility, &key);
    let thickness_mm = thickness_tenths as f32 / 10.0;
    let color = swatch_color(thickness_to_color(thickness_mm));

    let toggle_entity = entity.clone();
    let toggle_key = key.clone();
    // A plate row's checked state is never partial (`from_all_any(x, x)`) --
    // `warn` is only ever read by `tri_checkbox` when partial, so this is
    // just threading the same color the zone/part rows use, for consistency.
    let checked = part_visible && plate_visible;
    let checkbox = tri_checkbox(
        format!("armor-vis-plate-{}-{}-{}", zone_name, part_name, thickness_tenths),
        TriState::from_all_any(checked, checked),
        warn,
        move |_checked, _window, cx| {
            let key = toggle_key.clone();
            toggle_entity.update(cx, |view, cx| view.toggle_plate(key, cx));
        },
    );

    hoverable_row(
        format!("armor-vis-plate-row-{}-{}-{}", zone_name, part_name, thickness_tenths),
        entity,
        SidebarHighlightKey::Plate(key),
        h_flex()
            .gap_1()
            .items_center()
            .child(div().flex_none().w(px(10.)).h(px(10.)).rounded(px(2.)).bg(color))
            .child(checkbox)
            .child(div().text_sm().child(format!("{thickness_mm:.0} mm"))),
    )
    .into_any_element()
}

/// A checkbox with an optional indeterminate "partial" dash drawn over its
/// box, reproducing egui's own partial-state indicator (a warn-colored
/// horizontal line across the checkbox center, `tab.rs:4475-4481`). Built on
/// top of `gpui_kit::component::checkbox::Checkbox` rather than a fully custom
/// widget so the on/off visuals (border, fill, focus ring) match the rest of
/// the app; the dash is a small absolutely-positioned overlay `div()`.
fn tri_checkbox(
    id: impl Into<ElementId>,
    state: TriState,
    warn: Hsla,
    on_click: impl Fn(&bool, &mut Window, &mut App) + 'static,
) -> AnyElement {
    div()
        .relative()
        .child(Checkbox::new(id).checked(state.checked()).on_click(on_click))
        .when(state.partial(), |wrapper| {
            wrapper.child(
                div()
                    .absolute()
                    .left(CHECKBOX_BOX * 0.25)
                    .top(CHECKBOX_BOX * 0.44)
                    .w(CHECKBOX_BOX * 0.5)
                    .h(px(2.))
                    .rounded(px(1.))
                    .bg(warn),
            )
        })
        .into_any_element()
}

/// Wraps `content` in a row that reports hover in/out to `ViewportView`'s
/// sidebar-hover highlight (`set_sidebar_hover`/`clear_sidebar_hover_if`).
fn hoverable_row(
    id: impl Into<ElementId>,
    entity: &Entity<ViewportView>,
    key: SidebarHighlightKey,
    content: impl IntoElement,
) -> impl IntoElement {
    let enter_entity = entity.clone();
    let leave_entity = entity.clone();
    let enter_key = key.clone();
    div().id(id).child(content).on_hover(move |hovered, _window, cx| {
        if *hovered {
            enter_entity.update(cx, |view, cx| view.set_sidebar_hover(enter_key.clone(), cx));
        } else {
            leave_entity.update(cx, |view, cx| view.clear_sidebar_hover_if(&key, cx));
        }
    })
}
