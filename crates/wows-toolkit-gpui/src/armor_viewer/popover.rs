//! Armor-visibility toolbar button and popover: the toolbar row
//! `viewport_view.rs` renders above the 3D viewport, and the popover content
//! it opens -- the tri-state zone/material/plate tree. Ports
//! `armor_viewer::ui::tab::draw_armor_visibility_popover` (`tab.rs:4403-4611`).
//! The popover's own `PopoverState`/`Context<PopoverState>` is a different
//! entity than [`ViewportView`], so every row's click/hover handler captures
//! a clone of `Entity<ViewportView>` and mutates it via `.update(cx, ..)`,
//! matching this file's own `.context_menu()` closures. The popover's
//! `.content()` closure is only invoked while the popover is open (see
//! `gpui_component::popover::Popover`'s `RenderOnce` impl), so rebuilding the
//! whole tree here on every open-render does not cost anything while the
//! popover is closed -- unlike the always-visible viewport, which re-renders
//! on every hover-driven `cx.notify()`.
//!
//! **Task 7b note.** This toolbar has room for a second button (display
//! settings): `render_toolbar` returns an `h_flex` row, so a sibling
//! `.child(..)` is all a later task needs to add.

use std::collections::HashMap;
use std::collections::HashSet;

use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_component::ActiveTheme;
use gpui_component::Disableable;
use gpui_component::Icon;
use gpui_component::IconName;
use gpui_component::button::Button;
use gpui_component::checkbox::Checkbox;
use gpui_component::h_flex;
use gpui_component::popover::Popover;
use gpui_component::popover::PopoverState;
use gpui_component::scroll::Scrollbar;
use gpui_component::v_flex;

use wowsunpack::export::gltf_export::thickness_to_color;

use super::legend::swatch_color;
use super::load_ship::ArmorZone;
use super::load_ship::PlateKey;
use super::load_ship::ZonePart;
use super::upload::SHOW_ZERO_MM;
use super::viewport_view::ViewportView;
use super::visibility::SidebarHighlightKey;
use super::visibility::TriState;
use super::visibility::part_any_plate_hidden;
use super::visibility::part_on;
use super::visibility::plate_explicitly_hidden;
use super::visibility::zone_all_on;
use super::visibility::zone_any_on;

/// Checkbox box size for [`TriState`] partial-dash placement (`Size::Medium`,
/// `checkbox.rs`'s own `size_4` = 1rem = 16px).
const CHECKBOX_BOX: Pixels = px(16.);

/// Toolbar row above the 3D viewport: currently just the visibility-popover
/// trigger button. `viewport_view.rs`'s `Render` impl wraps this above the
/// interactive viewport div.
pub fn render_toolbar(
    view: &ViewportView,
    entity: &Entity<ViewportView>,
    cx: &mut Context<ViewportView>,
) -> impl IntoElement + use<> {
    h_flex()
        .flex_none()
        .gap_2()
        .items_center()
        .px_2()
        .py_1()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(render_visibility_button(view, entity))
}

fn render_visibility_button(view: &ViewportView, entity: &Entity<ViewportView>) -> impl IntoElement + use<> {
    let has_armor = view.current_armor.is_some();
    let close_entity = entity.clone();
    let content_entity = entity.clone();

    Popover::new("armor-visibility-popover")
        .trigger(
            Button::new("armor-visibility-popover-trigger")
                .icon(IconName::Eye)
                .label("Visibility")
                .compact()
                .disabled(!has_armor),
        )
        .on_open_change(move |open, _window, cx| {
            if !*open {
                close_entity.update(cx, |view, cx| view.clear_sidebar_hover(cx));
            }
        })
        .content(move |_state, window, cx| render_popover_content(&content_entity, window, cx))
}

/// Builds the popover's whole tree from a snapshot of `entity`'s current
/// state, read once up front (see the module doc: the `.content()` closure
/// re-runs on every open-render, so this clone is cheap and short-lived, not
/// held across the click/hover closures built below).
fn render_popover_content(
    entity: &Entity<ViewportView>,
    _window: &mut Window,
    cx: &mut Context<PopoverState>,
) -> AnyElement {
    let (armor, part_visibility, plate_visibility, expanded_zones, expanded_parts, scroll) = {
        let view = entity.read(cx);
        (
            view.current_armor.clone(),
            view.part_visibility.clone(),
            view.plate_visibility.clone(),
            view.expanded_zones.clone(),
            view.expanded_parts.clone(),
            view.popover_scroll.clone(),
        )
    };

    let Some(armor) = armor else {
        return div().text_sm().opacity(0.6).p_2().child("No ship loaded").into_any_element();
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
        .child(Button::new("armor-vis-all").label("All").compact().on_click(move |_, _window, cx| {
            all_entity.update(cx, |view, cx| view.set_all_parts_visible(cx));
        }))
        .child(Button::new("armor-vis-none").label("None").compact().on_click(move |_, _window, cx| {
            none_entity.update(cx, |view, cx| view.set_all_parts_hidden(cx));
        }))
        .when(has_plate_overrides, |row| {
            row.child(Button::new("armor-vis-reset-plates").label("Reset plates").compact().on_click(
                move |_, _window, cx| {
                    reset_entity.update(cx, |view, cx| view.reset_plate_overrides(cx));
                },
            ))
        });

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
) -> AnyElement {
    let all_on = zone_all_on(zone, part_visibility, plate_visibility, SHOW_ZERO_MM);
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
            .child(div().text_sm().child(zone.name.clone()))
            .child(div().text_xs().opacity(0.5).child("(ctrl+click to solo)")),
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
) -> AnyElement {
    let part_key = (zone_name.to_string(), part.name.clone());
    let part_visible = part_on(part_visibility, zone_name, &part.name);
    let any_plate_hidden = part_any_plate_hidden(zone_name, part, plate_visibility, SHOW_ZERO_MM);
    let visible_plates: Vec<i32> = part.plates.iter().copied().filter(|&t| SHOW_ZERO_MM || t != 0).collect();

    let toggle_entity = entity.clone();
    let toggle_zone_name = zone_name.to_string();
    let toggle_part_name = part.name.clone();
    let toggle_plates = part.plates.clone();

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
            h_flex().gap_1().items_center().child(checkbox).child(div().text_sm().child(part.name.clone())),
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
        h_flex().gap_1().items_center().child(chevron).child(checkbox).child(div().text_sm().child(part.name.clone())),
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
    let key: PlateKey = (zone_name.to_string(), part_name.to_string(), thickness_tenths);
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
/// top of `gpui_component::checkbox::Checkbox` rather than a fully custom
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
