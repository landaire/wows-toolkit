//! The options rail's tool rows and the viewport context menu's toggles,
//! and the three option sections `options_panel.rs` renders below them: the
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
use std::borrow::Cow;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use crate::slider_field::SliderField;
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Disableable;
use gpui_kit::component::Icon;
use gpui_kit::component::IconName;
use gpui_kit::component::Selectable;
use gpui_kit::component::Sizable;
use gpui_kit::component::Size;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::button::Toggle;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::h_flex;
use gpui_kit::component::input::Input;
use gpui_kit::component::input::InputState;
use gpui_kit::component::list::ListItem;
use gpui_kit::component::menu::DropdownMenu;
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::scroll::Scrollbar;
use gpui_kit::component::slider::Slider;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rust_i18n::t;

use wowsunpack::export::camo_textures::CamoSchemeId;
use wowsunpack::export::camo_textures::CamoSchemeInfo;
use wowsunpack::export::gltf_export::CamoOrigin;
use wowsunpack::export::gltf_export::thickness_to_color;
use wowsunpack::game_params::keys::ComponentType;
use wowsunpack::game_params::types::Species;

use crate::text_filter::MarkedLabel;
use crate::text_filter::MarkedQuery;
use crate::viewport::types::LightingSettings;

use super::assets::ArmorAssetsBundle;
use super::legend::swatch_color;
use super::load_model::MountRef;
use super::load_ship::ArmorThickness;
use super::load_ship::ArmorZone;
use super::load_ship::LoadedShipArmor;
use super::load_ship::PlateKey;
use super::load_ship::ZonePart;
use super::options_panel::HeadDepth;
use super::options_panel::folding_head;
use super::tree_filter::RowFilter;
use super::tree_filter::TreeFilter;
use super::tree_filter::match_count;
use super::viewport_view::SceneFeatures;
use wows_toolkit_viewmodel::armor::camera_perspective::LookMode;

use super::viewport_view::CameraRingSettings;
use super::viewport_view::PerspectiveSettings;
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
use crate::armor_viewer::pane::ArmorViewerPane;
use crate::armor_viewer::pane::OptionSection;
use crate::status_bar::ModifierKey;

/// Checkbox box size for [`TriState`] partial-dash placement (`Size::Medium`,
/// `checkbox.rs`'s own `size_4` = 1rem = 16px).
const CHECKBOX_BOX: Pixels = px(16.);

/// The options rail's tool rows for the active viewport: the view toggles,
/// then the splash tools and the export.
pub(super) fn render_tools(
    view: &ViewportView,
    entity: &Entity<ViewportView>,
    filter: &MarkedQuery,
) -> Option<AnyElement> {
    // An open model keeps only the tools it has anything for; with nothing
    // open every tool is drawn, disabled.
    let features = view.scene_features();
    let armor = features.is_none_or(|features| features.armor);
    let ship = features.is_none_or(|features| features.ship);
    let tools: Vec<AnyElement> = [
        armor.then(|| render_hidden_plates_button(view, entity, filter)).flatten(),
        armor.then(|| render_gaps_button(view, entity, filter)).flatten(),
        ship.then(|| render_splash_button(view, entity, filter)).flatten(),
        ship.then(|| render_splash_boxes_button(view, entity, filter)).flatten(),
        render_export_button(view, entity, filter),
    ]
    .into_iter()
    .flatten()
    .collect();
    if tools.is_empty() {
        return None;
    }

    // Two even columns rather than wrapped rows: the labels differ in length,
    // and wrapping leaves a ragged edge that moves as the rail is resized.
    Some(div().id("armor-options-tools").test_support().grid().grid_cols(2).gap_1().children(tools).into_any_element())
}

/// A tool button's label, shortened rather than cut off where a large zoom
/// leaves its column too narrow. The button's tooltip says what it does.
fn tool_label(label: StyledText) -> impl IntoElement {
    div().id("armor-tool-label").test_support().min_w_0().text_ellipsis().child(label)
}

/// A tool's label as `filter` marks it, kept when the filter matches the
/// label or the tooltip that says what the tool does.
fn tool_text(filter: &MarkedQuery, label: String, tooltip: &str) -> Option<StyledText> {
    if filter.keeps(&label) || filter.keeps(tooltip) { Some(filter.marked(label)) } else { None }
}

/// The viewport context menu's toggles, read from `entity` and its pane as
/// the menu opens.
pub(super) fn quick_toggle_items(mut menu: PopupMenu, entity: &Entity<ViewportView>, cx: &App) -> PopupMenu {
    let view = entity.read(cx);
    let has_armor = view.has_armor();
    let features = view.scene_features();
    let armor = features.is_none_or(|features| features.armor);
    let ship = features.is_none_or(|features| features.ship);
    let show_hidden = view.show_hidden_only();
    let gaps_shown = view.gaps().shown;
    let splash = view.splash_state();
    let pane = view.pane();

    let toggle = |label: String,
                  checked: bool,
                  enabled: bool,
                  apply: fn(&mut ViewportView, bool, &mut Context<ViewportView>)| {
        let entity = entity.clone();
        PopupMenuItem::new(label).checked(checked).disabled(!enabled).on_click(move |_event, _window, cx| {
            entity.update(cx, |view, cx| apply(view, !checked, cx));
        })
    };
    if armor {
        menu = menu
            .item(toggle(
                t!("ui.armor.show_hidden").into_owned(),
                show_hidden,
                has_armor,
                ViewportView::set_show_hidden_only,
            ))
            .item(toggle(t!("ui.armor.gaps").into_owned(), gaps_shown, has_armor, ViewportView::set_show_gaps));
    }
    if ship {
        menu = menu
            .item(toggle(
                t!("ui.armor.splash_mode").into_owned(),
                splash.shown,
                splash.has_data,
                ViewportView::set_splash_mode,
            ))
            .item(toggle(
                t!("ui.armor.splash_boxes").into_owned(),
                splash.boxes_shown,
                splash.has_data,
                ViewportView::set_show_splash_boxes,
            ));
    }

    if let Some(pane) = pane {
        let options_open = pane.read(cx).options_open();
        let pen_pane = pane.clone();
        if armor {
            menu = menu.item(PopupMenuItem::new(t!("ui.armor.pen_check").into_owned()).on_click(
                move |_event, _window, cx| {
                    pen_pane.update(cx, |pane, cx| pane.open_option_section(OptionSection::PenCheck, cx));
                },
            ));
        }
        menu = menu.item(PopupMenuItem::new(t!("ui.armor.options_panel").into_owned()).checked(options_open).on_click(
            move |_event, _window, cx| {
                pane.update(cx, |pane, cx| pane.toggle_options(cx));
            },
        ));
    }

    let export_entity = entity.clone();
    menu.separator().item(PopupMenuItem::new(t!("ui.armor.export_model").into_owned()).disabled(!has_armor).on_click(
        move |_event, window, cx| {
            export_entity.update(cx, |view, cx| view.open_export_confirm(window, cx));
        },
    ))
}

/// Rail toggle for "Show Hidden": the plates that are part of the combat
/// model but that the game's own armor viewer never draws.
///
/// A mode rather than an addition, so it reads as selected while it is on.
fn render_hidden_plates_button(
    view: &ViewportView,
    entity: &Entity<ViewportView>,
    filter: &MarkedQuery,
) -> Option<AnyElement> {
    let showing = view.show_hidden_only();
    let has_armor = view.has_armor();
    let entity = entity.clone();

    let label_text = tool_text(filter, t!("ui.armor.show_hidden").into_owned(), &t!("ui.armor.show_hidden_tooltip"))?;

    Some(
        crate::ui::selectable(
            "armor-show-hidden",
            showing,
            Button::new("armor-show-hidden-button")
                .icon(crate::icons::Glyph::EyeOff)
                .child(tool_label(label_text))
                .compact()
                .w_full()
                .selected(showing)
                .disabled(!has_armor)
                .tooltip(t!("ui.armor.show_hidden_tooltip").to_string())
                .on_click(move |_event, _window, cx: &mut App| {
                    entity.update(cx, |view, cx| view.set_show_hidden_only(!showing, cx));
                }),
        )
        .into_any_element(),
    )
}

/// The camera-rings group of the display section: whether the orbits are
/// drawn, which mode's, and where on them the camera sits.
///
/// Every control but the first is refused while the rings are off, and the
/// group says so when the ship names no modes: a slider that moves nothing is
/// worse than one that will not move.
fn camera_rings_group(
    entity: &Entity<ViewportView>,
    snapshot: &DisplayPopoverSnapshot,
    filter: &MarkedQuery,
    cx: &mut App,
) -> Option<AnyElement> {
    let rings = &snapshot.camera_rings;
    let modes = &snapshot.camera_modes;
    let has_modes = !modes.is_empty();
    let on = rings.shown && has_modes;

    let shown = {
        let entity = entity.clone();
        let rings = rings.clone();
        filtered_checkbox(
            "armor-camera-rings",
            t!("ui.armor.show_camera_rings").into_owned(),
            rings.shown,
            !has_modes,
            filter,
            move |checked, _window, cx| {
                let next = CameraRingSettings { shown: *checked, ..rings.clone() };
                entity.update(cx, |view, cx| view.set_camera_rings(next.clone(), cx));
            },
        )
    };
    if !has_modes {
        return shown.map(|shown| {
            v_flex()
                .gap_1()
                .child(shown)
                .child(
                    div()
                        .text_xs()
                        .text_color(crate::theme::text_dim())
                        .child(t!("ui.armor.no_ship_loaded").to_string()),
                )
                .into_any_element()
        });
    }

    // One button per mode rather than a combo: a ship names two or three, and
    // which one is showing is the thing being read.
    let mode_buttons: Vec<AnyElement> = modes
        .iter()
        .enumerate()
        .filter_map(|(ix, mode)| {
            let label = filter.label(mode.clone())?;
            let chosen = rings.mode.as_ref() == Some(mode);
            let entity = entity.clone();
            let rings = rings.clone();
            let mode = mode.clone();
            let button = crate::ui::selectable(
                ("armor-camera-mode", ix),
                chosen,
                Button::new(("armor-camera-mode-button", ix))
                    .child(label)
                    .compact()
                    .selected(chosen)
                    .disabled(!on)
                    .on_click(move |_event, _window, cx: &mut App| {
                        let next = CameraRingSettings { mode: Some(mode.clone()), ..rings.clone() };
                        entity.update(cx, |view, cx| view.set_camera_rings(next.clone(), cx));
                    }),
            );
            Some(button.into_any_element())
        })
        .collect();

    let zoom_path = {
        let entity = entity.clone();
        let rings = rings.clone();
        filtered_checkbox(
            "armor-camera-zoom-path",
            t!("ui.armor.show_camera_zoom_path").into_owned(),
            rings.zoom_path,
            !on,
            filter,
            move |checked, _window, cx| {
                let next = CameraRingSettings { zoom_path: *checked, ..rings.clone() };
                entity.update(cx, |view, cx| view.set_camera_rings(next.clone(), cx));
            },
        )
    };
    let zoom_path_choices: Vec<AnyElement> = if rings.zoom_path {
        [
            zoom_path_checkbox(
                entity,
                "armor-camera-zoom-path-fov",
                t!("ui.armor.zoom_path_regular_fov").into_owned(),
                rings,
                rings.zoom_path_at_fov,
                on,
                filter,
                |settings, value| settings.zoom_path_at_fov = value,
            ),
            zoom_path_checkbox(
                entity,
                "armor-camera-zoom-path-max",
                t!("ui.armor.zoom_path_max_fov").into_owned(),
                rings,
                rings.zoom_path_at_max_fov,
                on,
                filter,
                |settings, value| settings.zoom_path_at_max_fov = value,
            ),
        ]
        .into_iter()
        .flatten()
        .collect()
    } else {
        Vec::new()
    };

    let rows: Vec<AnyElement> = [
        shown,
        (!mode_buttons.is_empty()).then(|| h_flex().flex_wrap().gap_1().children(mode_buttons).into_any_element()),
        labeled_slider_row(t!("ui.armor.camera_fov").into_owned(), &snapshot.camera_fov_slider, !on, filter, cx),
        labeled_slider_row(t!("ui.armor.camera_height").into_owned(), &snapshot.camera_height_slider, !on, filter, cx),
        zoom_path,
        (!zoom_path_choices.is_empty()).then(|| {
            div().pl(px(20.)).child(h_flex().gap_4().flex_wrap().children(zoom_path_choices)).into_any_element()
        }),
    ]
    .into_iter()
    .flatten()
    .chain(perspective_rows(entity, &snapshot.perspective, &snapshot.perspective_fov_slider, filter, cx))
    .collect();

    (!rows.is_empty()).then(|| v_flex().gap_1().children(rows).into_any_element())
}

/// The camera lock: put the eye on the ship's own orbit and look out from it.
///
/// Under the orbits because it follows the same trajectory and the same two
/// sliders: which orbit is drawn is which orbit the eye rides.
fn perspective_rows(
    entity: &Entity<ViewportView>,
    perspective: &PerspectiveSettings,
    fov_slider: &Entity<SliderField>,
    filter: &MarkedQuery,
    cx: &mut App,
) -> Vec<AnyElement> {
    let toggled = entity.clone();
    let on = perspective.enabled;
    let mut rows: Vec<AnyElement> = filtered_checkbox(
        "armor-camera-perspective",
        t!("ui.armor.camera_perspective").into_owned(),
        on,
        false,
        filter,
        move |checked, _window, cx| {
            let checked = *checked;
            toggled.update(cx, |view, cx| view.set_perspective_enabled(checked, cx));
        },
    )
    .into_iter()
    .collect();
    if !on {
        return rows;
    }

    rows.extend(labeled_slider_row(t!("ui.armor.perspective_fov").into_owned(), fov_slider, false, filter, cx));
    let projection = t!("ui.armor.perspective_projection").into_owned();
    let modes = [
        ("armor-perspective-proj-game", t!("ui.armor.perspective_proj_game").into_owned(), LookMode::Game),
        ("armor-perspective-proj-center", t!("ui.armor.perspective_proj_center").into_owned(), LookMode::ThroughCenter),
    ];
    // The heading names the choice, so a match on it keeps both buttons.
    let heading = filter.label(projection.clone());
    let inner = if heading.is_some() { MarkedQuery::default() } else { filter.clone() };
    let buttons: Vec<AnyElement> = modes
        .into_iter()
        .filter_map(|(id, label, mode)| {
            let label = inner.label(label)?;
            Some(look_mode_button(entity, id, label, mode, perspective.camera.look_mode))
        })
        .collect();
    if !buttons.is_empty() {
        let heading = heading.unwrap_or_else(|| StyledText::new(projection));
        rows.push(
            h_flex()
                .gap_2()
                .items_center()
                .child(div().w(px(88.)).text_xs().child(heading))
                .child(h_flex().gap_1().children(buttons))
                .into_any_element(),
        );
    }
    rows
}

/// One of the two ways the locked camera can aim.
fn look_mode_button(
    entity: &Entity<ViewportView>,
    id: &'static str,
    label: StyledText,
    mode: LookMode,
    chosen: LookMode,
) -> AnyElement {
    let entity = entity.clone();
    let selected = mode == chosen;
    crate::ui::selectable(
        id,
        selected,
        Button::new(id).child(label).compact().selected(selected).on_click(move |_event, _window, cx: &mut App| {
            entity.update(cx, |view, cx| view.set_perspective_look_mode(mode, cx));
        }),
    )
    .into_any_element()
}

/// One of the zoom path's two field-of-view checkboxes.
#[allow(clippy::too_many_arguments)]
fn zoom_path_checkbox(
    entity: &Entity<ViewportView>,
    id: &'static str,
    label: String,
    rings: &CameraRingSettings,
    checked: bool,
    enabled: bool,
    filter: &MarkedQuery,
    apply: fn(&mut CameraRingSettings, bool),
) -> Option<AnyElement> {
    let entity = entity.clone();
    let rings = rings.clone();
    filtered_checkbox(id, label, checked, !enabled, filter, move |checked, _window, cx| {
        let mut next = rings.clone();
        apply(&mut next, *checked);
        entity.update(cx, |view, cx| view.set_camera_rings(next.clone(), cx));
    })
}

/// A checkbox kept only when `filter` keeps its label.
///
/// While filtering, the label is drawn beside the box rather than by it, so
/// the matched letters can be marked; a click on it toggles the box as the
/// box's own label would.
fn filtered_checkbox(
    id: impl Into<SharedString>,
    label: String,
    checked: bool,
    disabled: bool,
    filter: &impl RowFilter,
    on_click: impl Fn(&bool, &mut Window, &mut App) + 'static,
) -> Option<AnyElement> {
    let id: SharedString = id.into();
    let checkbox = Checkbox::new(id.clone()).checked(checked).disabled(disabled);
    if !filter.is_active() {
        return Some(checkbox.label(label).on_click(on_click).into_any_element());
    }
    let marked = filter.row_label(label)?;

    let on_click = std::rc::Rc::new(on_click);
    let from_label = std::rc::Rc::clone(&on_click);
    Some(
        h_flex()
            .gap_2()
            .items_center()
            .child(checkbox.on_click(move |checked, window, cx| on_click(checked, window, cx)))
            .child(
                div()
                    .id(SharedString::from(format!("{id}-label")))
                    .text_sm()
                    .when(disabled, |label| label.text_color(crate::theme::text_dim()))
                    .when(!disabled, |label| {
                        label.cursor_pointer().on_click(move |_event, window, cx| from_label(&!checked, window, cx))
                    })
                    .child(marked),
            )
            .into_any_element(),
    )
}

/// Rail toggle for gap detection, which carries its own count: the
/// number is the answer, so it is on the control rather than behind a hover.
fn render_gaps_button(view: &ViewportView, entity: &Entity<ViewportView>, filter: &MarkedQuery) -> Option<AnyElement> {
    let gaps = view.gaps();
    let has_armor = view.has_armor();
    let entity = entity.clone();

    let label =
        if gaps.shown { format!("{} ({})", t!("ui.armor.gaps"), gaps.count) } else { t!("ui.armor.gaps").into_owned() };
    // A hull with openings is worth a warning tone; one with none, having
    // been looked at, is worth saying so quietly.
    let found = gaps.shown && gaps.count > 0;

    let label_text = tool_text(filter, label, &t!("ui.armor.gaps_tooltip"))?;

    Some(
        crate::ui::selectable(
            "armor-show-gaps",
            gaps.shown,
            Button::new("armor-show-gaps-button")
                .icon(if found { crate::icons::Glyph::TriangleAlert } else { crate::icons::Glyph::EyeOff })
                .child(tool_label(label_text))
                .compact()
                .w_full()
                .selected(gaps.shown)
                .disabled(!has_armor)
                .tooltip(t!("ui.armor.gaps_tooltip").to_string())
                .on_click(move |_event, _window, cx: &mut App| {
                    entity.update(cx, |view, cx| view.set_show_gaps(!gaps.shown, cx));
                }),
        )
        .into_any_element(),
    )
}

/// Rail toggle for splash mode, carrying how many zones the last burst
/// reached: that number is the answer the mode exists to give.
///
/// Refused on a hull that ships no splash file, and on one where the chosen
/// attacker carries nothing that bursts; the tooltip says which.
fn render_splash_button(
    view: &ViewportView,
    entity: &Entity<ViewportView>,
    filter: &MarkedQuery,
) -> Option<AnyElement> {
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

    let label_text = tool_text(filter, label, &tooltip)?;

    Some(
        crate::ui::selectable(
            "armor-splash",
            state.shown,
            Button::new("armor-splash-button")
                .icon(crate::icons::Glyph::Bomb)
                .child(tool_label(label_text))
                .compact()
                .w_full()
                .selected(state.shown)
                .disabled(!state.has_data)
                .tooltip(tooltip)
                .on_click(move |_event, _window, cx: &mut App| {
                    entity.update(cx, |view, cx| view.set_splash_mode(!state.shown, cx));
                }),
        )
        .into_any_element(),
    )
}

/// Rail toggle for the splash-box outlines, which stand on their own: they
/// are worth seeing while placing a burst and after it.
fn render_splash_boxes_button(
    view: &ViewportView,
    entity: &Entity<ViewportView>,
    filter: &MarkedQuery,
) -> Option<AnyElement> {
    let state = view.splash_state();
    let entity = entity.clone();

    let label_text = tool_text(filter, t!("ui.armor.splash_boxes").into_owned(), &t!("ui.armor.splash_tooltip"))?;

    Some(
        crate::ui::selectable(
            "armor-splash-boxes",
            state.boxes_shown,
            Button::new("armor-splash-boxes-button")
                .icon(crate::icons::Glyph::Box)
                .child(tool_label(label_text))
                .compact()
                .w_full()
                .selected(state.boxes_shown)
                .disabled(!state.has_data)
                .tooltip(t!("ui.armor.splash_tooltip").to_string())
                .on_click(move |_event, _window, cx: &mut App| {
                    entity.update(cx, |view, cx| view.set_show_splash_boxes(!state.boxes_shown, cx));
                }),
        )
        .into_any_element(),
    )
}

/// Rail trigger for the export-confirm flow (Milestone 5 Task 10): opens
/// `ViewportView`'s confirm panel for the ship currently displayed in this
/// pane, at its current hull/LOD/module selection. Ports the egui app's
/// "Export Ship Model" tab button (`tab.rs:1696-1709`), which is likewise
/// gated on a ship being loaded.
fn render_export_button(
    view: &ViewportView,
    entity: &Entity<ViewportView>,
    filter: &MarkedQuery,
) -> Option<AnyElement> {
    let has_armor = view.current_armor.is_some();
    let click_entity = entity.clone();
    let tooltip = match view.scene_features() {
        Some(SceneFeatures { ship: false, .. }) => t!("ui.model_view.export_tooltip").into_owned(),
        Some(_) | None => t!("ui.armor.export_tooltip").into_owned(),
    };

    let label_text = tool_text(filter, t!("ui.replay.section_export").into_owned(), &tooltip)?;

    Some(
        Button::new("armor-export-trigger")
            .icon(IconName::HardDrive)
            .child(tool_label(label_text))
            .compact()
            .w_full()
            .tooltip(tooltip)
            .disabled(!has_armor)
            .on_click(move |_, window, cx| {
                click_entity.update(cx, |view, cx| view.open_export_confirm(window, cx));
            })
            .into_any_element(),
    )
}

/// Builds the popover's whole tree from a snapshot of `entity`'s current
/// state, read once up front (see the module doc: the `.content()` closure
/// re-runs on every open-render, so this clone is cheap and short-lived, not
/// held across the click/hover closures built below). `zone_field` is the
/// tree's own filter field.
pub(crate) fn render_popover_content(
    entity: &Entity<ViewportView>,
    filter: &MarkedQuery,
    zone_field: &Entity<InputState>,
    field_query: &MarkedQuery,
    cx: &mut App,
) -> Option<AnyElement> {
    let (armor, part_visibility, plate_visibility, expanded_zones, expanded_parts, scroll, show_zero_mm) = {
        let view = entity.read(cx);
        (
            view.current_armor.clone(),
            view.part_visibility.clone(),
            view.plate_visibility.clone(),
            view.expanded_zones.clone(),
            view.expanded_parts.clone(),
            view.popover_scroll.clone(),
            view.show_zero_mm(),
        )
    };

    let Some(armor) = armor else {
        return (!filter.is_active()).then(|| {
            div()
                .text_sm()
                .text_color(crate::theme::text_dim())
                .p_2()
                .child(t!("ui.armor.no_ship_loaded").to_string())
                .into_any_element()
        });
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

    let shown = entity.read(cx);
    let names = TreeNames {
        bundle: shown.reload_bundle(),
        species: shown.shown_species(),
        zone_tooltip: t!("ui.armor.zone_toggle_tooltip", key = ModifierKey::Secondary.label(cx)).into_owned().into(),
    };

    // Only a query counts matches; the rows translate their own labels as
    // they are drawn.
    let labels: Option<Vec<(String, Vec<String>)>> = (filter.is_active() || field_query.is_active()).then(|| {
        armor
            .zone_part_plates
            .iter()
            .map(|zone| (names.zone(&zone.name), zone.parts.iter().map(|part| names.part(&part.name)).collect()))
            .collect()
    });
    // Only the rail's query drops the section; the tree's own field narrows
    // the rows under it.
    if let Some(labels) = &labels
        && filter.is_active()
        && match_count(filter, labels) == 0
    {
        return None;
    }
    let field_matches = match &labels {
        Some(labels) if field_query.is_active() => Some(match_count(field_query, labels)),
        _ => None,
    };

    let tree_filter = TreeFilter::new(filter.clone(), field_query.clone());
    let zones: Vec<AnyElement> = armor
        .zone_part_plates
        .iter()
        .filter_map(|zone| {
            render_zone_row(
                entity,
                zone,
                &part_visibility,
                &plate_visibility,
                &expanded_zones,
                &expanded_parts,
                warn,
                show_zero_mm,
                &names,
                &tree_filter,
            )
        })
        .collect();
    let tree = v_flex().gap_1().children(zones);

    let scroll_area =
        div().id("armor-visibility-tree-scroll").max_h(px(360.)).overflow_y_scroll().track_scroll(&scroll).child(tree);

    let thickness = (!filter.is_active()).then(|| render_thickness_source(entity, &armor)).flatten();

    Some(
        v_flex()
            .w_full()
            .gap_2()
            .children(thickness)
            // The bulk switches act on every plate, not only the ones a filter
            // left showing, so they are put away while one is typed.
            .when(!filter.is_active(), |this| this.child(header).child(div().h(px(1.)).bg(border)))
            .child(tree_field("armor-zone-filter", zone_field, field_matches))
            .child(div().relative().max_h(px(360.)).child(scroll_area).child(Scrollbar::vertical(&scroll)))
            .into_any_element(),
    )
}

/// A tree's own filter field, and under it how many of the tree's rows the
/// field matched by name, `field_matches`, while something is typed in it.
fn tree_field(id: &'static str, field: &Entity<InputState>, field_matches: Option<usize>) -> AnyElement {
    let count = field_matches.map(|count| match count {
        0 => t!("ui.armor.tree_no_matches").into_owned(),
        count => t!("ui.armor.tree_matches", count = count).into_owned(),
    });

    v_flex()
        .gap_1()
        .capture_action(crate::text_filter::clear_on_escape(field))
        .child(Input::new(field).id(id).small().prefix(Icon::new(IconName::Search).small()).cleanable(true))
        .children(count.map(|count| {
            let said = SharedString::from(count);
            div()
                .id(SharedString::from(format!("{id}-count")))
                .test_support()
                .aria_label(said.clone())
                .text_xs()
                .text_color(crate::theme::text_dim())
                .child(said)
        }))
        .into_any_element()
}

/// Which ship a model's plate thickness is read from, with a picker for any
/// other ship mounting it. `None` for a ship, whose own armor it is.
fn render_thickness_source(entity: &Entity<ViewportView>, armor: &LoadedShipArmor) -> Option<AnyElement> {
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
        let entity = entity.clone();
        let choices = model.thickness_choices.clone();
        Button::new("model-thickness-pick")
            .label(t!("ui.model_view.thickness_other_ships").into_owned())
            .compact()
            .dropdown_menu(move |mut menu, _window, _cx| {
                for choice in &choices {
                    let entity = entity.clone();
                    let mount =
                        MountRef { vehicle_param_index: choice.vehicle_param_index.clone(), hp: choice.hp.clone() };
                    let checked = current.as_ref() == Some(&choice.vehicle_param_index);
                    menu = menu.item(PopupMenuItem::new(choice.display_name.clone()).checked(checked).on_click(
                        move |_event, _window, cx| {
                            let mount = mount.clone();
                            entity.update(cx, |view, cx| view.set_thickness_source(mount, cx));
                        },
                    ));
                }
                menu
            })
    });

    Some(
        h_flex()
            .id("model-thickness-source")
            .test_support()
            .gap_2()
            .items_center()
            .flex_wrap()
            .child(div().flex_1().min_w_0().text_xs().text_color(crate::theme::text_dim()).child(label))
            .children(picker)
            .into_any_element(),
    )
}

/// Builds the hull-visibility popover's tree from a snapshot of `entity`'s
/// current state (same lazy-`.content()` rationale as
/// `render_popover_content`'s doc). After the header come the hull upgrade
/// (`render_hull_upgrade_row`, when the ship has more than one), the module
/// alternatives (`render_module_alternative_row`, one row per component type
/// with more than one option), the hull part tree, the camo picker
/// (`render_camo_picker`, when `armor.camo_scheme_infos` is non-empty), and
/// LOD (`render_lod_row`, when the ship has more than one). The selectors
/// each select a new value on `ViewportView` and trigger a background reload
/// (`ViewportView::reload_ship`); the camo picker changes the active camo
/// directly (`ViewportView::select_camo`).
///
/// Hovering a group or mesh row highlights its meshes in the viewport
/// (`SidebarHighlightKey::HullMeshes`), shown or not, as egui does.
pub(crate) fn render_hull_popover_content(
    entity: &Entity<ViewportView>,
    filter: &MarkedQuery,
    part_field: &Entity<InputState>,
    field_query: &MarkedQuery,
    cx: &mut App,
) -> Option<AnyElement> {
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
            view.hull_opaque(),
            view.expanded_hull_groups.clone(),
            view.selected_camo,
            view.expanded_camo_groups.clone(),
            view.selected_hull.clone(),
            view.hull_lod,
            view.selected_modules.clone(),
        )
    };

    let Some(armor) = armor else {
        return (!filter.is_active()).then(|| {
            div()
                .text_sm()
                .text_color(crate::theme::text_dim())
                .p_2()
                .child(t!("ui.armor.no_ship_loaded").to_string())
                .into_any_element()
        });
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
        ));

    let tree_filter = TreeFilter::new(filter.clone(), field_query.clone());
    let groups: Vec<AnyElement> = armor
        .hull_part_groups
        .iter()
        .filter_map(|(group, names)| {
            render_hull_group_row(entity, group, names, &hull_visibility, &expanded_hull_groups, warn, &tree_filter)
        })
        .collect();
    let field_matches = field_query.is_active().then(|| match_count(field_query, &armor.hull_part_groups));
    let field = tree_field("armor-part-filter", part_field, field_matches);
    let upgrade =
        (armor.hull_upgrade_names.len() > 1).then(|| render_hull_upgrade_row(entity, &armor, &selected_hull, filter));
    let modules: Vec<AnyElement> = armor
        .module_alternatives
        .iter()
        .filter(|(_, alternatives)| alternatives.len() > 1)
        .filter_map(|(ct, alternatives)| {
            render_module_alternative_row(entity, *ct, alternatives, selected_modules.get(ct), filter)
        })
        .collect();
    let camo = (!armor.camo_schemes().is_empty())
        .then(|| render_camo_picker(entity, &armor, selected_camo, &expanded_camo_groups, filter))
        .flatten();
    let lod = (armor.hull_lod_count > 1).then(|| render_lod_row(entity, &armor, hull_lod, filter)).flatten();

    // While filtering every row that matches by name is drawn, without the
    // bulk switches and dividers. Only the rail's query drops the tree; its
    // own field narrows the rows in it.
    if filter.is_active() {
        let opaque = filtered_checkbox(
            "armor-hull-vis-opaque",
            t!("ui.armor.opaque").into_owned(),
            hull_opaque,
            false,
            filter,
            move |checked, _window, cx| {
                let checked = *checked;
                opaque_entity.update(cx, |view, cx| view.set_hull_opaque(checked, cx));
            },
        );
        let tree = (match_count(filter, &armor.hull_part_groups) > 0).then(|| {
            let tree = v_flex().id("armor-hull-visibility-tree").test_support().gap_1().children(groups);
            v_flex().gap_1().child(field).child(tree).into_any_element()
        });
        let rows: Vec<AnyElement> =
            opaque.into_iter().chain(upgrade.flatten()).chain(modules).chain(tree).chain(camo).chain(lod).collect();
        return (!rows.is_empty()).then(|| v_flex().w_full().gap_1().children(rows).into_any_element());
    }

    let header = header.child(
        Checkbox::new("armor-hull-vis-opaque").label(t!("ui.armor.opaque").to_string()).checked(hull_opaque).on_click(
            move |checked, _window, cx| {
                let checked = *checked;
                opaque_entity.update(cx, |view, cx| view.set_hull_opaque(checked, cx));
            },
        ),
    );

    // The part tree comes before the camo and LOD pickers: it is what the
    // reader turns to most, and the camo list can run long. It has no scroll
    // of its own, so a wheel over it moves only the rail.
    let tree = v_flex().id("armor-hull-visibility-tree").test_support().gap_1().children(groups);
    let col = v_flex()
        .w_full()
        .gap_2()
        .child(header)
        .children(upgrade.flatten())
        .children(modules)
        .child(div().h(px(1.)).bg(border))
        .child(field)
        .child(tree)
        .children(camo.map(|camo| v_flex().gap_2().child(div().h(px(1.)).bg(border)).child(camo)))
        .children(lod);

    Some(col.into_any_element())
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
    filter: &MarkedQuery,
) -> Option<AnyElement> {
    let stock_key = &armor.hull_upgrade_names[0].0;
    let heading = t!("ui.armor.upgrade").into_owned();
    let inner = within(filter, &heading);
    let kept: Vec<&(String, String)> =
        armor.hull_upgrade_names.iter().filter(|(_, label)| inner.keeps(label)).collect();
    if kept.is_empty() {
        return None;
    }

    let mut row =
        h_flex().gap_1().items_center().flex_wrap().child(div().text_sm().flex_none().child(filter.marked(heading)));
    for (key, label) in kept {
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

    Some(row.into_any_element())
}

/// What a row's choices are filtered by: nothing when the row's heading
/// matches, which keeps every choice in it, and `filter` otherwise.
fn within(filter: &MarkedQuery, heading: &str) -> MarkedQuery {
    if filter.keeps(heading) { MarkedQuery::default() } else { filter.clone() }
}

/// One module-alternative selector row for component type `ct`: a "{ct}:"
/// label followed by one selectable toggle per alternative, labeled
/// "{ct} {letter}" with the full component name as its tooltip. Selecting a
/// different alternative overrides it (`ViewportView::select_module_alternative`,
/// which triggers a reload). Ports the egui `selectable_label` row
/// (`tab.rs:3178-3190`); `selected.is_none()` is treated as the first entry,
/// matching the egui original's own fallback. While filtering, an
/// alternative is kept when its label or its component name matches.
fn render_module_alternative_row(
    entity: &Entity<ViewportView>,
    ct: ComponentType,
    alternatives: &[String],
    selected: Option<&String>,
    filter: &MarkedQuery,
) -> Option<AnyElement> {
    let heading = format!("{ct}:");
    let inner = within(filter, &heading);
    let mut kept = Vec::new();
    for (i, name) in alternatives.iter().enumerate() {
        let letter = (b'A' + i as u8) as char;
        let label = format!("{ct} {letter}");
        if inner.keeps(&label) || inner.keeps(name) {
            kept.push((i, name, label));
        }
    }
    if kept.is_empty() {
        return None;
    }

    let mut row =
        h_flex().gap_1().items_center().flex_wrap().child(div().text_sm().flex_none().child(filter.marked(heading)));
    for (i, name, label) in kept {
        let is_selected = selected.map(|sel| sel == name).unwrap_or(i == 0);
        let toggle_entity = entity.clone();
        let toggle_name = name.clone();
        row = row.child(
            Toggle::new(format!("armor-module-alt-{ct}-{i}"))
                .label(label)
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

    Some(row.into_any_element())
}

/// LOD selector row: a "LOD:" label followed by one selectable toggle per
/// level `0..armor.hull_lod_count` (`0` labeled "0 (highest)", matching the
/// egui original). Selecting a different level switches to it
/// (`ViewportView::select_hull_lod`, which triggers a reload). Ports the egui
/// `selectable_label` row (`tab.rs:3253-3264`).
fn render_lod_row(
    entity: &Entity<ViewportView>,
    armor: &LoadedShipArmor,
    hull_lod: usize,
    filter: &MarkedQuery,
) -> Option<AnyElement> {
    let heading = t!("ui.armor.lod").into_owned();
    let inner = within(filter, &heading);
    let kept: Vec<(usize, String)> = (0..armor.hull_lod_count)
        .map(|i| (i, if i == 0 { t!("ui.armor.lod_highest").into_owned() } else { i.to_string() }))
        .filter(|(_, label)| inner.keeps(label))
        .collect();
    if kept.is_empty() {
        return None;
    }

    let mut row =
        h_flex().gap_1().items_center().flex_wrap().child(div().text_sm().flex_none().child(filter.marked(heading)));
    for (i, label) in kept {
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

    Some(row.into_any_element())
}

/// Camo picker: "Stock" (deselects the active camo) + ship-specific schemes
/// at top level, then collapsible Universal/Expendable/LegacyScan groups
/// (each sorted by lowercased display name, skipped when empty). Ports the
/// egui camo selector (`tab.rs:3196-3251`). The caller hides this entirely
/// when `armor.camo_scheme_infos` is empty (old game versions carry no camo
/// metadata). While filtering, only the schemes whose names match are kept,
/// each group holding one open; `None` when none match.
fn render_camo_picker(
    entity: &Entity<ViewportView>,
    armor: &LoadedShipArmor,
    selected_camo: Option<CamoSchemeId>,
    expanded_camo_groups: &HashSet<String>,
    filter: &MarkedQuery,
) -> Option<AnyElement> {
    let heading = t!("ui.armor.camouflage").into_owned();
    let inner = within(filter, &heading);
    let mut rows: Vec<AnyElement> = Vec::new();

    let none_label = t!("ui.armor.camo_none").into_owned();
    if inner.keeps(&none_label) {
        let none_entity = entity.clone();
        rows.push(
            ListItem::new("armor-camo-none")
                .selected(selected_camo.is_none())
                .child(div().text_sm().child(inner.marked(none_label)))
                .on_click(move |_, _window, cx| {
                    none_entity.update(cx, |view, cx| view.select_camo(None, cx));
                })
                .into_any_element(),
        );
    }

    let mut ship_infos: Vec<&CamoSchemeInfo> = armor
        .camo_schemes()
        .iter()
        .filter(|i| i.origin == CamoOrigin::ShipSpecific && inner.keeps(&i.display_name))
        .collect();
    ship_infos.sort_by_key(|i| i.display_name.to_lowercase());
    for info in &ship_infos {
        rows.push(render_camo_row(entity, info, selected_camo, &inner));
    }

    for (origin, label) in [
        (CamoOrigin::Universal, "ui.armor.camo_group_universal"),
        (CamoOrigin::Expendable, "ui.armor.camo_group_expendable"),
        (CamoOrigin::LegacyScan, "ui.armor.camo_group_other"),
    ] {
        let label = t!(label).into_owned();
        let group_filter = within(&inner, &label);
        let mut group: Vec<&CamoSchemeInfo> =
            armor.camo_schemes().iter().filter(|i| i.origin == origin && group_filter.keeps(&i.display_name)).collect();
        if group.is_empty() {
            continue;
        }
        group.sort_by_key(|i| i.display_name.to_lowercase());
        let filtering = filter.is_active();
        rows.push(render_camo_group(entity, &label, &group, selected_camo, expanded_camo_groups, &inner, filtering));
    }
    if rows.is_empty() {
        return None;
    }

    Some(
        v_flex()
            .id("armor-camo-picker")
            .test_support()
            .gap_1()
            .child(div().text_sm().font_weight(FontWeight::BOLD).child(filter.marked(heading)))
            .children(rows)
            .into_any_element(),
    )
}

/// One selectable camo-scheme row, its name marked where `filter` matched.
/// Ports one `selectable_label` iteration of `tab.rs:3216`/`3243`.
fn render_camo_row(
    entity: &Entity<ViewportView>,
    info: &CamoSchemeInfo,
    selected_camo: Option<CamoSchemeId>,
    filter: &MarkedQuery,
) -> AnyElement {
    let id = info.id;
    let is_selected = selected_camo == Some(id);
    let toggle_entity = entity.clone();
    ListItem::new(format!("armor-camo-{}", id.0))
        .selected(is_selected)
        .child(div().text_sm().child(filter.marked(info.display_name.clone())))
        .on_click(move |_, _window, cx| {
            toggle_entity.update(cx, |view, cx| view.select_camo(Some(id), cx));
        })
        .into_any_element()
}

/// One collapsible camo-origin group header row + (if expanded) its scheme
/// rows, scroll-clamped to match egui's own per-group `ScrollArea::max_height(240.0)`.
/// Ports the `CollapsingState` block (`tab.rs:3234-3250`). Held open, with
/// its caret inert, while the rail is `filtering`, whether or not `filter`
/// still narrows its rows.
fn render_camo_group(
    entity: &Entity<ViewportView>,
    label: &str,
    infos: &[&CamoSchemeInfo],
    selected_camo: Option<CamoSchemeId>,
    expanded_camo_groups: &HashSet<String>,
    filter: &MarkedQuery,
    filtering: bool,
) -> AnyElement {
    let expanded = filtering || expanded_camo_groups.contains(label);
    let row_filter = within(filter, label);

    let expand_entity = entity.clone();
    let expand_label = label.to_string();
    let chevron = div()
        .id(format!("armor-camo-group-expand-{label}"))
        .flex_none()
        .child(crate::motion::DisclosureChevron::new(viewport_caret_id(entity, "camo", label), expanded))
        .when(!filtering, |chevron| {
            chevron.on_click(move |_, _window, cx| {
                expand_entity.update(cx, |view, cx| view.toggle_camo_group_expanded(expand_label.clone(), cx));
            })
        });

    let header = h_flex()
        .id(format!("armor-camo-group-row-{label}"))
        .gap_1()
        .items_center()
        .child(chevron)
        .child(div().text_sm().child(filter.marked(label.to_string())));

    let mut column = v_flex().gap_1().child(header);
    if expanded {
        let mut inner = v_flex().gap_1();
        for info in infos {
            inner = inner.child(render_camo_row(entity, info, selected_camo, &row_filter));
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
    filter: &TreeFilter,
) -> Option<AnyElement> {
    let matched = filter.row_label(group.to_string()).is_some();
    // A query the group matches keeps every mesh in it.
    let mesh_filter = filter.below(group);
    let expanded = filter.is_active() || expanded_hull_groups.contains(group);

    let mut shown: Vec<String> = Vec::new();
    let mut rows: Vec<AnyElement> = Vec::new();
    if expanded {
        for name in names {
            if let Some(row) = render_hull_mesh_row(entity, name, hull_visibility, &mesh_filter) {
                shown.push(name.clone());
                rows.push(row);
            }
        }
    }
    if filter.is_active() && shown.is_empty() && !matched {
        return None;
    }

    // While the rail's query is typed, the group box reads and sets only the
    // meshes it keeps; the tree's own field only narrows what is shown.
    let acted_on: Vec<String> = if filter.narrows() {
        let rail = filter.rail_only().below(group);
        names.iter().filter(|name| rail.keeps(name)).cloned().collect()
    } else {
        names.to_vec()
    };
    let all_on = hull_group_all_on(hull_visibility, &acted_on);
    let any_on = hull_group_any_on(hull_visibility, &acted_on);
    let state = TriState::from_all_any(all_on, any_on);

    let toggle_entity = entity.clone();
    let checkbox = tri_checkbox(format!("armor-hull-vis-group-{group}"), state, warn, move |checked, _window, cx| {
        let checked = *checked;
        let names = acted_on.clone();
        toggle_entity.update(cx, |view, cx| {
            view.mutate_hull_visibility(cx, |hull_visibility| {
                for name in names {
                    hull_visibility.insert(name, checked);
                }
            })
        });
    });

    // Inert while filtering, where every group is open whatever it was left at.
    let expand_entity = entity.clone();
    let expand_group = group.to_string();
    let chevron = div()
        .id(format!("armor-hull-vis-group-expand-{group}"))
        .flex_none()
        .child(crate::motion::DisclosureChevron::new(viewport_caret_id(entity, "hull", group), expanded))
        .when(!filter.is_active(), |chevron| {
            chevron.on_click(move |_, _window, cx| {
                expand_entity.update(cx, |view, cx| view.toggle_hull_group_expanded(expand_group.clone(), cx));
            })
        });

    let title = filter.marked(group.to_string());
    let header = hoverable_row(
        format!("armor-hull-vis-group-row-{group}"),
        entity,
        SidebarHighlightKey::HullMeshes(names.to_vec()),
        h_flex().gap_1().items_center().child(chevron).child(checkbox).child(div().text_sm().child(title)),
    );

    let mut column = v_flex().gap_1().child(header);
    if expanded {
        column = column.child(v_flex().gap_1().pl(px(20.)).children(rows));
    }
    Some(column.into_any_element())
}

/// One hull-mesh checkbox row. Ports `tab.rs:3300-3310`.
fn render_hull_mesh_row(
    entity: &Entity<ViewportView>,
    name: &str,
    hull_visibility: &HashMap<String, bool>,
    filter: &TreeFilter,
) -> Option<AnyElement> {
    let visible = hull_visibility.get(name).copied().unwrap_or(false);
    let toggle_entity = entity.clone();
    let toggle_name = name.to_string();
    let row = filtered_checkbox(
        format!("armor-hull-vis-mesh-{name}"),
        name.to_string(),
        visible,
        false,
        filter,
        move |checked, _window, cx| {
            let checked = *checked;
            let name = toggle_name.clone();
            toggle_entity.update(cx, |view, cx| {
                view.mutate_hull_visibility(cx, |hull_visibility| {
                    hull_visibility.insert(name, checked);
                })
            });
        },
    )?;
    let key = SidebarHighlightKey::HullMeshes(vec![name.to_string()]);

    Some(hoverable_row(format!("armor-hull-vis-mesh-row-{name}"), entity, key, row).into_any_element())
}

/// What the display section draws, read off the viewport in one pass.
///
/// Taken as a snapshot so no borrow of the view is held while the rows below
/// are built.
struct DisplayPopoverSnapshot {
    display: super::upload::DisplaySettings,
    lighting: LightingSettings,
    marker_slider: Entity<SliderField>,
    /// Whether any impact marker is drawn, since a marker control with nothing
    /// to act on is what the egui popover hides.
    has_markers: bool,
    armor_slider: Entity<SliderField>,
    roll_slider: Entity<SliderField>,
    camera_rings: CameraRingSettings,
    /// The modes this ship's own GameParams name.
    camera_modes: Vec<String>,
    camera_fov_slider: Entity<SliderField>,
    camera_height_slider: Entity<SliderField>,
    /// Whether the camera is locked to the ship's own orbit, and how it aims
    /// from there.
    perspective: PerspectiveSettings,
    perspective_fov_slider: Entity<SliderField>,
    flat_slider: Entity<SliderField>,
    /// The colour each of the two lighting terms is tinted by. A picker keeps its
    /// own state, so these are the view's own rather than rebuilt per frame.
    flat_color: Entity<gpui_kit::component::color_picker::ColorPickerState>,
    key_color: Entity<gpui_kit::component::color_picker::ColorPickerState>,
    key_slider: Entity<SliderField>,
    azimuth_slider: Entity<SliderField>,
    elevation_slider: Entity<SliderField>,
    rim_slider: Entity<SliderField>,
    specular_slider: Entity<SliderField>,
    shininess_slider: Entity<SliderField>,
}

/// The pane's say in the display section: the legend it owns, and which of
/// the section's groups the reader left open.
pub(crate) struct DisplayGroups {
    pub pane: Entity<ArmorViewerPane>,
    pub legend_visible: bool,
    pub open: super::pane::OptionSectionsOpen,
}

/// The display section: the model settings, then the camera rings and the
/// lighting, each a group that folds away.
///
/// Every slider is a persistent `SliderField` already wired (in
/// `viewport_view.rs`'s `build_display_sliders`/`build_lighting_sliders`) to
/// mutate the right field when it moves, so this only lays them out.
///
/// `None` when `filter` keeps nothing here.
pub(crate) fn render_display_popover_content(
    entity: &Entity<ViewportView>,
    groups: &DisplayGroups,
    filter: &MarkedQuery,
    cx: &mut App,
) -> Option<AnyElement> {
    // The egui app draws none of this without a loaded hull
    // (`ui/tab.rs`'s `if let Some(armor) = pane.loaded_armor.take()`).
    if !entity.read(cx).has_armor() {
        return (!filter.is_active()).then(|| {
            div()
                .text_sm()
                .text_color(crate::theme::text_dim())
                .p_2()
                .child(t!("ui.armor.no_ship_loaded").to_string())
                .into_any_element()
        });
    }

    let snapshot = {
        let view = entity.read(cx);
        DisplayPopoverSnapshot {
            display: view.display_settings,
            lighting: view.lighting(),
            armor_slider: view.display_sliders.armor_opacity.clone(),
            roll_slider: view.display_sliders.model_roll_deg.clone(),
            camera_rings: view.camera_rings(),
            camera_modes: view.camera_modes(),
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

    let features = entity.read(cx).scene_features();
    let armor = features.is_none_or(|features| features.armor);
    let ship = features.is_none_or(|features| features.ship);
    let model = model_rows(entity, groups, &snapshot, armor, filter, cx);
    // The camera orbits are the ship's own, which a model has none of.
    let camera = ship
        .then(|| {
            display_group(
                groups,
                OptionSection::CameraRings,
                t!("ui.armor.camera_rings").into_owned(),
                filter,
                cx,
                |filter, cx| camera_rings_group(entity, &snapshot, filter, cx),
            )
        })
        .flatten();
    let lighting = display_group(
        groups,
        OptionSection::Lighting,
        t!("ui.armor.lighting").into_owned(),
        filter,
        cx,
        |filter, cx| lighting_group(entity, &snapshot, filter, cx),
    );

    let parts: Vec<AnyElement> = model.into_iter().chain(camera).chain(lighting).collect();
    (!parts.is_empty()).then(|| v_flex().w_full().gap_2().children(parts).into_any_element())
}

/// The settings at the top of the display section, about the model itself.
/// `armor` shows the legend, plate edges, 0 mm plates and the armor and
/// marker opacity.
fn model_rows(
    entity: &Entity<ViewportView>,
    groups: &DisplayGroups,
    snapshot: &DisplayPopoverSnapshot,
    armor: bool,
    filter: &MarkedQuery,
    cx: &mut App,
) -> Option<AnyElement> {
    let display = snapshot.display;
    let setting = |apply: fn(&mut super::upload::DisplaySettings, bool)| {
        let entity = entity.clone();
        move |checked: &bool, _window: &mut Window, cx: &mut App| {
            let checked = *checked;
            entity.update(cx, |view, cx| view.mutate_display_settings(cx, |d| apply(d, checked)));
        }
    };

    // The legend belongs to the pane, not to this viewport, but the reader
    // looks for it here: the egui display popover carries the same checkbox
    // (`ui.armor.show_armor_thickness`).
    let legend = {
        let pane = groups.pane.clone();
        filtered_checkbox(
            "armor-display-legend",
            t!("ui.armor.show_armor_thickness").into_owned(),
            groups.legend_visible,
            false,
            filter,
            move |checked, _window, cx| {
                let checked = *checked;
                pane.update(cx, |pane, cx| pane.set_legend_visible(checked, cx));
            },
        )
    };
    let armor_rows = armor.then(|| {
        [
            legend,
            filtered_checkbox(
                "armor-display-plate-edges",
                t!("ui.armor.plate_edges").into_owned(),
                display.show_plate_edges,
                false,
                filter,
                setting(|d, on| d.show_plate_edges = on),
            ),
            filtered_checkbox(
                "armor-display-zero-mm",
                t!("ui.armor.zero_mm_plates").into_owned(),
                display.show_zero_mm,
                false,
                filter,
                setting(|d, on| d.show_zero_mm = on),
            ),
        ]
    });
    let opacity_rows = armor.then(|| {
        [
            labeled_slider_row(t!("ui.armor.armor_opacity").into_owned(), &snapshot.armor_slider, false, filter, cx),
            snapshot
                .has_markers
                .then(|| {
                    labeled_slider_row(
                        t!("ui.armor.marker_opacity").into_owned(),
                        &snapshot.marker_slider,
                        false,
                        filter,
                        cx,
                    )
                })
                .flatten(),
        ]
    });

    let rows: Vec<AnyElement> = armor_rows
        .into_iter()
        .flatten()
        .chain([
            filtered_checkbox(
                "armor-display-grid",
                t!("ui.armor.grid").into_owned(),
                display.show_grid,
                false,
                filter,
                setting(|d, on| d.show_grid = on),
            ),
            filtered_checkbox(
                "armor-display-length-marks",
                t!("ui.armor.length_marks").into_owned(),
                display.show_length_marks,
                false,
                filter,
                setting(|d, on| d.show_length_marks = on),
            ),
            filtered_checkbox(
                "armor-display-ship-center",
                t!("ui.armor.ship_center").into_owned(),
                display.show_ship_center,
                false,
                filter,
                setting(|d, on| d.show_ship_center = on),
            ),
        ])
        .chain(opacity_rows.into_iter().flatten())
        // Heeling the hull over is what says whether a belt is still a belt
        // at the angle the ship is fighting at.
        .chain([labeled_slider_row(t!("ui.armor.roll").into_owned(), &snapshot.roll_slider, false, filter, cx)])
        .flatten()
        .collect();

    (!rows.is_empty()).then(|| v_flex().gap_2().children(rows).into_any_element())
}

/// The lighting group: whether the hull is lit, three presets, and the
/// terms the presets set.
fn lighting_group(
    entity: &Entity<ViewportView>,
    snapshot: &DisplayPopoverSnapshot,
    filter: &MarkedQuery,
    cx: &mut App,
) -> Option<AnyElement> {
    let lighting = &snapshot.lighting;
    let off = !lighting.enabled;

    let enabled = {
        let entity = entity.clone();
        filtered_checkbox(
            "armor-display-lighting-enabled",
            t!("ui.armor.lighting_enabled").into_owned(),
            lighting.enabled,
            false,
            filter,
            move |checked, _window, cx| {
                let checked = *checked;
                entity.update(cx, |view, cx| view.mutate_lighting(cx, |l| l.enabled = checked));
            },
        )
    };

    let presets: Vec<AnyElement> = [
        (
            "armor-lighting-preset-ingame",
            t!("ui.armor.lighting_preset_ingame").into_owned(),
            LightingSettings::in_game as fn() -> LightingSettings,
        ),
        ("armor-lighting-preset-flat", t!("ui.armor.lighting_preset_flat").into_owned(), LightingSettings::flat),
        ("armor-lighting-preset-studio", t!("ui.armor.lighting_preset_studio").into_owned(), LightingSettings::studio),
    ]
    .into_iter()
    .filter_map(|(id, label, preset)| {
        let label = filter.label(label)?;
        let entity = entity.clone();
        let button = Button::new(id).child(label).compact().disabled(off).on_click(move |_, window, cx| {
            entity.update(cx, |view, cx| view.set_lighting_preset(preset(), window, cx));
        });
        Some(button.into_any_element())
    })
    .collect();

    let rows: Vec<AnyElement> = [
        enabled,
        (!presets.is_empty()).then(|| h_flex().gap_1().children(presets).into_any_element()),
        labeled_slider_row(t!("ui.armor.lighting_flat").into_owned(), &snapshot.flat_slider, off, filter, cx),
        labeled_color_row(t!("ui.armor.lighting_ambient_color").into_owned(), &snapshot.flat_color, off, filter),
        labeled_slider_row(t!("ui.armor.lighting_intensity").into_owned(), &snapshot.key_slider, off, filter, cx),
        labeled_color_row(t!("ui.armor.lighting_key_color").into_owned(), &snapshot.key_color, off, filter),
        labeled_slider_row(t!("ui.armor.lighting_azimuth").into_owned(), &snapshot.azimuth_slider, off, filter, cx),
        labeled_slider_row(t!("ui.armor.lighting_elevation").into_owned(), &snapshot.elevation_slider, off, filter, cx),
        labeled_slider_row(t!("ui.armor.lighting_rim").into_owned(), &snapshot.rim_slider, off, filter, cx),
        labeled_slider_row(t!("ui.armor.lighting_specular").into_owned(), &snapshot.specular_slider, off, filter, cx),
        labeled_slider_row(t!("ui.armor.lighting_shininess").into_owned(), &snapshot.shininess_slider, off, filter, cx),
    ]
    .into_iter()
    .flatten()
    .collect();

    (!rows.is_empty()).then(|| v_flex().gap_2().children(rows).into_any_element())
}

/// One folding group inside the display section.
///
/// While filtering the group is open whatever the reader left it at, and a
/// match on its title keeps everything in it; with nothing matched it is not
/// drawn at all. Its head is inert then, so the open state the reader left is
/// what comes back when the filter is cleared.
fn display_group(
    groups: &DisplayGroups,
    which: OptionSection,
    title: String,
    filter: &MarkedQuery,
    cx: &mut App,
    body: impl FnOnce(&MarkedQuery, &mut App) -> Option<AnyElement>,
) -> Option<AnyElement> {
    let marked = filter.label(title.clone());
    let (open, content) = if filter.is_active() {
        let inner = if marked.is_some() { MarkedQuery::default() } else { filter.clone() };
        let content = body(&inner, cx);
        if content.is_none() && marked.is_none() {
            return None;
        }
        (true, content)
    } else {
        let open = groups.open.is_open(which);
        (open, if open { body(filter, cx) } else { None })
    };
    let title = marked.unwrap_or_else(|| StyledText::new(title));

    let pane = groups.pane.clone();
    let id = SharedString::from(format!("armor-options-group-{which:?}"));
    let head = folding_head(id, title, open, HeadDepth::Group)
        .when(!filter.is_active(), |head| {
            head.cursor_pointer().on_click(move |_event, _window, cx: &mut App| {
                pane.update(cx, |pane, cx| pane.toggle_option_section(which, cx));
            })
        })
        .test_support();

    Some(v_flex().gap_2().child(head).children(content.map(|content| div().pl_1().child(content))).into_any_element())
}

/// One labelled colour row: what it tints, and the picker that sets it.
fn labeled_color_row(
    label: String,
    picker: &Entity<gpui_kit::component::color_picker::ColorPickerState>,
    lighting_off: bool,
    filter: &MarkedQuery,
) -> Option<AnyElement> {
    let marked = filter.label(label.clone())?;
    // Dimmed rather than taken away when the lighting is off: the colour it would
    // be lit in is still what this sets, and a row that vanished would read as a
    // setting that had gone.
    Some(
        h_flex()
            .gap_1()
            .items_center()
            .when(lighting_off, |row| row.opacity(0.5))
            .child(div().flex_none().w(px(88.)).text_sm().child(marked))
            .child(
                gpui_kit::component::color_picker::ColorPicker::new(picker)
                    .small()
                    .accessibility_label(SharedString::from(label)),
            )
            .into_any_element(),
    )
}

/// One labeled slider row: `label`, the slider, and the field's number and
/// reset. `disabled` matches the egui original's
/// `ui.add_enabled_ui(pane.lighting.enabled, ..)` wrapping the lighting sliders
/// (`tab.rs:5053`).
pub(super) fn labeled_slider_row(
    label: String,
    field: &Entity<SliderField>,
    disabled: bool,
    filter: &MarkedQuery,
    cx: &mut App,
) -> Option<AnyElement> {
    let marked = filter.label(label)?;
    field.update(cx, |field, cx| field.set_disabled(disabled, cx));
    let slider = field.read(cx).slider().clone();
    Some(
        h_flex()
            .gap_1()
            .items_center()
            .child(div().flex_none().w(px(88.)).text_sm().child(marked))
            // Boxed at a row's height with the thumb centred in it: unboxed, the
            // slider sits at the top of the row and the label beside it does not.
            .child(
                div()
                    .flex_1()
                    .min_w(px(48.))
                    .h(crate::ui::SELECT_SMALL_HEIGHT)
                    .flex()
                    .items_center()
                    .child(Slider::new(&slider).disabled(disabled)),
            )
            .child(field.clone())
            .into_any_element(),
    )
}

/// Names the tree's zones and parts the way the game names them, as the egui
/// popover does, and says what a zone's box does.
struct TreeNames {
    bundle: Option<Arc<ArmorAssetsBundle>>,
    species: Option<Species>,
    /// What a zone's box does, and what holding the solo key does instead.
    zone_tooltip: SharedString,
}

impl TreeNames {
    fn zone(&self, name: &str) -> String {
        match &self.bundle {
            Some(bundle) => super::catalog::translate_name(bundle.assets.metadata(), name),
            None => name.to_owned(),
        }
    }

    /// `key` is the part's material key, shown as is while the build is
    /// still loading.
    fn part(&self, key: &str) -> String {
        match &self.bundle {
            Some(bundle) => super::catalog::translate_material_key(bundle.assets.metadata(), key, self.species),
            None => key.to_owned(),
        }
    }
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
    names: &TreeNames,
    filter: &TreeFilter,
) -> Option<AnyElement> {
    let zone_label = names.zone(&zone.name);
    let matched = filter.row_label(zone_label.clone()).is_some();
    // A query the zone matches keeps every part in it; the others keep only
    // the parts that match them, under their zone.
    let part_filter = filter.below(&zone_label);
    let expanded = filter.is_active() || expanded_zones.contains(&zone.name);
    let parts: Vec<(&ZonePart, StyledText)> = if expanded {
        zone.parts.iter().filter_map(|part| Some((part, part_filter.row_label(names.part(&part.name))?))).collect()
    } else {
        Vec::new()
    };
    if filter.is_active() && parts.is_empty() && !matched {
        return None;
    }

    // While the rail's query is typed, the zone box reads and sets only the
    // parts it keeps; the tree's own field only narrows what is shown.
    let acted_on = if filter.narrows() {
        let rail = filter.rail_only().below(&zone_label);
        Cow::Owned(ArmorZone {
            name: zone.name.clone(),
            parts: zone.parts.iter().filter(|part| rail.keeps(&names.part(&part.name))).cloned().collect(),
        })
    } else {
        Cow::Borrowed(zone)
    };
    let all_on = zone_all_on(&acted_on, part_visibility, plate_visibility, show_zero_mm);
    let any_on = zone_any_on(&acted_on, part_visibility);
    let state = TriState::from_all_any(all_on, any_on);

    let toggle_entity = entity.clone();
    let toggle_zone_name = zone.name.clone();
    let toggle_parts: Vec<String> = acted_on.parts.iter().map(|part| part.name.clone()).collect();
    let checkbox = tri_checkbox(format!("armor-vis-zone-{}", zone.name), state, warn, move |checked, window, cx| {
        let solo = window.modifiers().secondary(cx);
        toggle_entity.update(cx, |view, cx| {
            view.toggle_zone(toggle_zone_name.clone(), toggle_parts.clone(), *checked, solo, cx)
        });
    });

    // Inert while filtering, where every zone is open whatever it was left at.
    let expand_entity = entity.clone();
    let expand_zone_name = zone.name.clone();
    let chevron = div()
        .id(format!("armor-vis-zone-expand-{}", zone.name))
        .flex_none()
        .child(crate::motion::DisclosureChevron::new(viewport_caret_id(entity, "zone", &zone.name), expanded))
        .when(!filter.is_active(), |chevron| {
            chevron.on_click(move |_, _window, cx| {
                expand_entity.update(cx, |view, cx| view.toggle_zone_expanded(expand_zone_name.clone(), cx));
            })
        });

    let title = filter.marked(zone_label);

    // The egui viewer says the same on hover of the zone's name
    // (`tab.rs:4512`).
    let tooltip = names.zone_tooltip.clone();
    let toggle = h_flex()
        .id(SharedString::from(format!("armor-vis-zone-toggle-{}", zone.name)))
        .gap_1()
        .items_center()
        .child(checkbox)
        .child(div().text_sm().child(title))
        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx));
    let header = hoverable_row(
        format!("armor-vis-zone-row-{}", zone.name),
        entity,
        SidebarHighlightKey::Zone(zone.name.clone()),
        h_flex().gap_1().items_center().child(chevron).child(toggle),
    );

    let mut column = v_flex().gap_1().child(header);
    if expanded {
        let mut body = v_flex().gap_1().pl(px(20.));
        for (part, label) in parts {
            body = body.child(render_part_row(
                entity,
                &zone.name,
                part,
                part_visibility,
                plate_visibility,
                expanded_parts,
                warn,
                show_zero_mm,
                label,
            ));
        }
        column = column.child(body);
    }
    Some(column.into_any_element())
}

/// One material/part row: a single checkbox if it has at most one visible
/// plate thickness, otherwise a collapsible header + plate rows. Ports
/// `tab.rs:4518-4606`. `label` is the part's name as the filter marked it.
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
    label: StyledText,
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
            h_flex().gap_1().items_center().child(checkbox).child(div().text_sm().child(label)),
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
        .child(crate::motion::DisclosureChevron::new(
            ElementId::NamedChild(Arc::new(viewport_caret_id(entity, "part", zone_name)), part.name.clone().into()),
            expanded,
        ))
        .on_click(move |_, _window, cx| {
            expand_entity.update(cx, |view, cx| view.toggle_part_expanded(expand_key.clone(), cx));
        });

    let header = hoverable_row(
        format!("armor-vis-part-row-{}-{}", zone_name, part.name),
        entity,
        SidebarHighlightKey::Part(zone_name.to_string(), part.name.clone()),
        h_flex().gap_1().items_center().child(chevron).child(checkbox).child(div().text_sm().child(label)),
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
    div()
        .id(id)
        .child(content)
        .on_hover(move |hovered, _window, cx| {
            if *hovered {
                enter_entity.update(cx, |view, cx| view.set_sidebar_hover(enter_key.clone(), cx));
            } else {
                leave_entity.update(cx, |view, cx| view.clear_sidebar_hover_if(&key, cx));
            }
        })
        .test_support()
}

/// Keys a caret by the viewport whose expanded sets it shows, so switching
/// the active viewport does not turn it.
fn viewport_caret_id(entity: &Entity<ViewportView>, kind: &'static str, name: &str) -> ElementId {
    let viewport = ElementId::NamedChild(Arc::new(ElementId::View(entity.entity_id())), kind.into());
    ElementId::NamedChild(Arc::new(viewport), SharedString::from(name.to_string()))
}
