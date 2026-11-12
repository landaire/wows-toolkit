//! The armor viewer's options rail: the viewport tools, then the trajectory,
//! penetration, armor, hull and display sections.
//!
//! The tools are the one-click toggles (also in the viewport's context menu).
//! The sections hold a tree, a list or a row of sliders, which a popover can
//! only show by covering the hull they describe, so they are a panel beside it
//! instead.
//!
//! Everything here acts on the pane the reader last clicked in
//! (`ViewportDock::active_viewport`), which the header names. Nothing here
//! knows about `sync_options`: a change emits `ViewportChange::Settings`
//! from that pane, and `ArmorViewerPane::push_settings_from` is what widens it
//! to the others.
//!
//! A collapsed section's body is never built. This rail renders on every window
//! repaint, and a hull hover notifies, so building all three every frame would
//! cost what the popovers only paid while they were open.

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Icon;
use gpui_kit::component::IconName;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::h_flex;
use gpui_kit::component::input::Input;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rust_i18n::t;

use super::pane::ArmorViewerPane;
use super::pane::OptionSection;
use super::popover;
use crate::text_filter::MarkedLabel;
use crate::text_filter::MarkedQuery;

/// Fixed: the sections below are their own widths, so there is nothing for a
/// drag handle to do but overflow or pad them.
pub const PANEL_WIDTH: Pixels = px(300.);

pub fn render_panel(view: &ArmorViewerPane, pane: &Entity<ArmorViewerPane>, cx: &mut App) -> AnyElement {
    let viewport = view.active_viewport(cx);
    let position = view.active_pane_position(cx);
    let border = cx.theme().border;
    let accent = cx.theme().primary;
    // The controls of the trajectory and pen check panels, which a filter
    // finds by these names and then shows whole.
    const TRAJECTORY_LABELS: [&str; 17] = [
        "ui.armor.continue_ricochet",
        "ui.armor.trajectory_next_cast",
        "ui.armor.trajectory_range",
        "ui.armor.trajectory_add_ship",
        "ui.armor.trajectory_add_ship_tooltip",
        "ui.armor.trajectory_ship",
        "ui.armor.trajectory_select_ship",
        "ui.armor.trajectory_shell",
        "ui.armor.trajectory_select_shell",
        "ui.armor.trajectory_ship_tooltip",
        "ui.armor.trajectory_shell_tooltip",
        "ui.armor.trajectory_clear",
        "ui.armor.arc_isolate_plates",
        "ui.armor.arc_isolate_plates_tooltip",
        "ui.armor.arc_isolate_zones",
        "ui.armor.arc_isolate_zones_tooltip",
        "ui.armor.arc_delete",
    ];
    const PEN_CHECK_LABELS: [&str; 12] = [
        "ui.armor.pen.ifhe",
        "ui.armor.pen.ifhe_tooltip",
        "ui.armor.pen.clear_all",
        "ui.armor.pen.remove",
        "ui.armor.pen.search_hint",
        "ui.armor.pen.use_for_trajectory",
        "ui.armor.pen.firing_ship_tooltip",
        "ui.armor.realtime.incoming_fire",
        "ui.armor.realtime.attacker_filter",
        "ui.armor.realtime.all_enemies",
        "ui.armor.realtime.show_secondaries",
        "ui.armor.realtime.seek_to_salvo",
    ];

    // Which pane these act on. With a comparison split open it is named in
    // the accent its border is drawn in.
    let target = match (position.count > 1, viewport.read(cx).shown_ship_name()) {
        (true, name) => {
            let pane_label = t!("ui.armor.options_pane", number = position.number, count = position.count);
            let label = match name {
                Some(name) => format!("{pane_label}: {name}"),
                None => pane_label.into_owned(),
            };
            Some(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(accent).child(label))
        }
        (false, Some(name)) => Some(div().text_xs().text_color(crate::theme::text_dim()).child(name)),
        (false, None) => None,
    };

    let filter = view.options_filter(cx);
    let zone_query = view.zone_filter(cx);
    let part_query = view.part_filter(cx);
    let filter_input = view.options_filter_input().clone();
    let header = v_flex()
        .flex_none()
        .gap_1()
        .px_2()
        .py_1()
        .border_b_1()
        .border_color(border)
        .child(div().text_sm().font_weight(FontWeight::BOLD).child(t!("ui.armor.options").to_string()))
        .children(target)
        .child({
            // Escape clears what was typed, rather than only leaving the field.
            let clear_input = filter_input.clone();
            div()
                .capture_key_down(move |event: &KeyDownEvent, window: &mut Window, cx: &mut App| {
                    if event.keystroke.key != "escape" || clear_input.read(cx).value().is_empty() {
                        return;
                    }
                    clear_input.update(cx, |state, cx| state.set_value("", window, cx));
                    cx.stop_propagation();
                })
                .child(
                    Input::new(&filter_input)
                        .id("armor-options-filter")
                        .small()
                        .prefix(Icon::new(IconName::Search).small())
                        .cleanable(true),
                )
        });

    let tools = popover::render_tools(viewport.read(cx), &viewport, &filter);
    // Nothing open yet keeps every section, each saying so; an open model
    // drops what it has nothing for.
    let features = viewport.read(cx).scene_features();
    let has_armor = features.is_none_or(|features| features.armor);
    let shift = crate::status_bar::ModifierKey::Shift.label(cx);
    // Where the trajectory toggle was: a Shift-click casts, with nothing to
    // switch on first, and this is where a reader looks for how.
    let cast_hint = (!filter.is_active() && has_armor).then(|| {
        h_flex()
            .id("armor-cast-hint")
            .test_support()
            .gap_1()
            .items_start()
            .text_xs()
            .text_color(crate::theme::text_dim())
            .child(crate::icons::icon(crate::icons::Glyph::Crosshair))
            .child(
                div()
                    .id("armor-cast-hint-text")
                    .test_support()
                    .flex_1()
                    // Without it the hint keeps the width of its unwrapped text.
                    .min_w_0()
                    .whitespace_normal()
                    .child(t!("ui.armor.cast_hint", key = shift).into_owned()),
            )
    });

    let visibility = viewport.clone();
    let hull = viewport.clone();
    let display = viewport.clone();
    let groups = popover::DisplayGroups {
        pane: pane.clone(),
        legend_visible: view.legend_visible(),
        open: view.option_sections_open(),
    };

    // Not built at all for a model without armor: a collapsed section costs
    // nothing, but an open one builds its whole body.
    let armor_sections = if has_armor {
        vec![
            section(
                view,
                pane,
                OptionSection::Trajectory,
                t!("ui.armor.trajectory").as_ref(),
                &filter,
                cx,
                |filter, cx| {
                    // Its controls are named here rather than filtered one by
                    // one: a match on any shows the panel whole.
                    keeps_any(filter, &TRAJECTORY_LABELS)
                        .then(|| super::trajectory_panel::render(view, pane, &viewport, cx))
                },
            ),
            section(
                view,
                pane,
                OptionSection::PenCheck,
                t!("ui.armor.pen_check").as_ref(),
                &filter,
                cx,
                |filter, cx| {
                    keeps_any(filter, &PEN_CHECK_LABELS).then(|| super::analysis::render_section(view, pane, cx))
                },
            ),
            section(
                view,
                pane,
                OptionSection::Visibility,
                t!("ui.armor.armor_options").as_ref(),
                &filter,
                cx,
                |filter, cx| {
                    popover::render_popover_content(&visibility, filter, view.zone_filter_input(), &zone_query, cx)
                },
            ),
        ]
    } else {
        Vec::new()
    };
    let mesh_sections = [
        section(view, pane, OptionSection::Hull, t!("ui.armor.hull_toggle").as_ref(), &filter, cx, |filter, cx| {
            popover::render_hull_popover_content(&hull, filter, view.part_filter_input(), &part_query, cx)
        }),
        section(view, pane, OptionSection::Display, t!("ui.armor.display").as_ref(), &filter, cx, |filter, cx| {
            popover::render_display_popover_content(&display, &groups, filter, cx)
        }),
    ];
    let sections: Vec<AnyElement> = armor_sections.into_iter().chain(mesh_sections).flatten().collect();
    let nothing_matched = filter.is_active() && tools.is_none() && sections.is_empty();

    let body =
        v_flex().gap_2().p_2().children(tools).children(cast_hint).children(sections).when(nothing_matched, |body| {
            let said: SharedString = t!("ui.armor.no_matching_options").into_owned().into();
            body.child(
                div()
                    .id("armor-options-no-match")
                    .test_support()
                    .aria_label(said.clone())
                    .text_sm()
                    .text_color(crate::theme::text_dim())
                    .child(said),
            )
        });

    v_flex()
        .id("armor-options-panel")
        // Without this the harness cannot find a plain container by its id.
        .test_support()
        .size_full()
        .border_l_1()
        .border_color(border)
        .child(header)
        .child(div().id("armor-options-body").flex_1().min_h(px(0.)).overflow_y_scroll().child(body))
        .into_any_element()
}

/// One section, labelled as its popover button was. `body` is called only while
/// the section is expanded, which is what the collapse is for.
///
/// While filtering, every section is open whatever the reader left it at, a
/// match on its title keeps everything in it, and one with nothing matched is
/// not drawn at all. Its head is inert then, so the open state the reader left
/// is what comes back when the filter is cleared.
fn section(
    view: &ArmorViewerPane,
    pane: &Entity<ArmorViewerPane>,
    which: OptionSection,
    title: &str,
    filter: &MarkedQuery,
    cx: &mut App,
    body: impl FnOnce(&MarkedQuery, &mut App) -> Option<AnyElement>,
) -> Option<AnyElement> {
    let marked = filter.label(title.to_string());
    let (expanded, content) = if filter.is_active() {
        let inner = if marked.is_some() { MarkedQuery::default() } else { filter.clone() };
        let content = body(&inner, cx);
        if content.is_none() && marked.is_none() {
            return None;
        }
        (true, content)
    } else {
        let expanded = view.option_sections_open().is_open(which);
        (expanded, if expanded { body(filter, cx) } else { None })
    };
    let title = marked.unwrap_or_else(|| StyledText::new(title.to_string()));
    let pane = pane.clone();

    let id = SharedString::from(format!("armor-options-section-{which:?}"));
    let head = folding_head(id, title, expanded, HeadDepth::Section)
        .when(!filter.is_active(), |head| {
            head.cursor_pointer().on_click(move |_event, _window, cx: &mut App| {
                pane.update(cx, |pane, cx| pane.toggle_option_section(which, cx));
            })
        })
        .test_support();
    let body_id = SharedString::from(format!("armor-options-body-{which:?}"));

    Some(
        v_flex()
            .gap_1()
            .child(head)
            .children(content.map(|content| div().id(body_id).test_support().pl_1().child(content)))
            .into_any_element(),
    )
}

/// Whether `filter` keeps any of the labels the translation `keys` name.
fn keeps_any(filter: &MarkedQuery, keys: &[&str]) -> bool {
    !filter.is_active() || keys.iter().any(|key| filter.keeps(&t!(*key)))
}

/// How deep a folding head sits in the rail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HeadDepth {
    /// A section of the rail.
    Section,
    /// A group inside a section.
    Group,
}

/// The head of a part of the rail that folds away: a band one surface step
/// above the rail, ruled off at its top, so the reader can see where each
/// part begins. A section's title is set larger than a group's inside it.
/// Callers add `test_support` after their own handlers.
pub(crate) fn folding_head(id: SharedString, title: StyledText, open: bool, depth: HeadDepth) -> Stateful<Div> {
    let chevron = crate::motion::DisclosureChevron::new("caret", open);
    let (chevron, label) = match depth {
        HeadDepth::Section => (chevron, div().text_base().font_weight(FontWeight::BOLD)),
        HeadDepth::Group => (chevron.xsmall(), div().text_sm().font_weight(FontWeight::SEMIBOLD)),
    };

    h_flex()
        .id(id)
        .w_full()
        .gap_1()
        .items_center()
        .px_2()
        .py_1()
        .bg(crate::theme::card())
        .border_t_1()
        .border_color(crate::theme::border_bright())
        .child(chevron)
        .child(label.child(title))
}

// The test module imports by name: `use super::*` would pull in the glob
// `gpui_kit::*` re-export of GPUI's own `test` macro.
#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use gpui_kit::AppContext as _;
    use gpui_kit::Bounds;
    use gpui_kit::Pixels;
    use gpui_kit::TestAppContext;
    use gpui_kit::Window;
    use gpui_kit::WindowHandle;
    use gpui_kit::px;
    use gpui_kit::size;
    use gpui_kit::test::TestWindowExt as _;

    use wowsunpack::export::camo_textures::CamoSchemeId;
    use wowsunpack::export::camo_textures::CamoSchemeInfo;
    use wowsunpack::export::gltf_export::CamoOrigin;
    use wowsunpack::export::model_catalog::ModelPath;

    use crate::armor_viewer::load_model::fixtures;
    use crate::armor_viewer::load_ship::ArmorZone;
    use crate::armor_viewer::load_ship::LoadedShipArmor;
    use crate::armor_viewer::load_ship::ZonePart;
    use crate::armor_viewer::pane::ArmorViewerPane;
    use crate::armor_viewer::pane::OptionSection;

    /// The rail's tool buttons.
    const TOOLS: [&str; 5] = [
        "armor-show-hidden-button",
        "armor-show-gaps-button",
        "armor-splash-button",
        "armor-splash-boxes-button",
        "armor-export-trigger",
    ];

    /// The pane with its rail up, drawn at twice the text size it opens at.
    fn open_at_double_size(cx: &mut TestAppContext) -> WindowHandle<ArmorViewerPane> {
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(1600.), px(1000.)), ArmorViewerPane::new);
        cx.update_window(window.into(), |_, window, cx| {
            window.set_rem_size(window.rem_size() * 2.0);
            window.render_frame(cx);
        })
        .expect("the test window stays open");

        window
    }

    /// The pane with its rail up, tall enough that an open tree is in view
    /// below the tools.
    fn open_tall(cx: &mut TestAppContext) -> WindowHandle<ArmorViewerPane> {
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(1600.), px(2400.)), ArmorViewerPane::new);
        cx.update_window(window.into(), |_, window, cx| window.render_frame(cx)).expect("the test window stays open");

        window
    }

    /// Whether `inner` lies between the left and right edges of `outer`.
    fn inside_across(outer: Bounds<Pixels>, inner: Bounds<Pixels>) -> bool {
        inner.left() >= outer.left() && inner.right() <= outer.right()
    }

    fn rail(window: &Window) -> Bounds<Pixels> {
        window.find("armor-options-panel").bounds()
    }

    /// The cast hint wraps inside the rail rather than running on as one line
    /// past its edge, where the rest of it cannot be read.
    #[gpui_kit::test]
    fn the_cast_hint_wraps_inside_the_rail(cx: &mut TestAppContext) {
        let window = open_at_double_size(cx);

        cx.update_window(window.into(), |_, window, _cx| {
            let hint = window.find("armor-cast-hint-text").bounds();
            assert!(inside_across(rail(window), hint), "the hint at {hint:?} is inside the rail at {:?}", rail(window));
        })
        .expect("the test window stays open");
    }

    /// Each tool's label stays inside its button, shortened where the column
    /// is too narrow for it, rather than cut off at the button's edge.
    #[gpui_kit::test]
    fn each_tool_label_stays_inside_its_button(cx: &mut TestAppContext) {
        let window = open_at_double_size(cx);

        cx.update_window(window.into(), |_, window, _cx| {
            for tool in TOOLS {
                let button = window.find(tool).bounds();
                let label = window.within(tool).find("armor-tool-label").bounds();
                assert!(inside_across(button, label), "{tool}'s label at {label:?} is inside the button at {button:?}");
                assert!(inside_across(rail(window), button), "{tool} at {button:?} is inside the rail");
            }
        })
        .expect("the test window stays open");
    }

    fn turret() -> ModelPath {
        ModelPath::parse("content/gameplay/usa/gun/main/AGM034/AGM034.model").expect("a model path")
    }

    /// Hands the active pane `scene`, as a finished model load does.
    fn show(window: WindowHandle<ArmorViewerPane>, scene: LoadedShipArmor, cx: &mut TestAppContext) {
        window
            .update(cx, |pane, _window, cx| {
                let layer = pane.opening_layer();
                pane.active_viewport(cx).update(cx, |view, cx| view.show_armor(Arc::new(scene), layer, cx));
            })
            .expect("the test window stays open");
        cx.update_window(window.into(), |_, window, cx| window.render_frame(cx)).expect("the test window stays open");
    }

    fn section(which: OptionSection) -> String {
        format!("armor-options-section-{which:?}")
    }

    #[gpui_kit::test]
    fn a_model_without_armor_keeps_only_the_mesh_and_display_sections(cx: &mut TestAppContext) {
        let window = open_at_double_size(cx);
        show(window, fixtures::model_scene(turret(), Vec::new()), cx);

        cx.update_window(window.into(), |_, window, _cx| {
            for gone in [OptionSection::Trajectory, OptionSection::PenCheck, OptionSection::Visibility] {
                assert!(window.try_find(section(gone)).is_none(), "{gone:?} has nothing to act on");
            }
            for kept in [OptionSection::Hull, OptionSection::Display] {
                assert!(window.try_find(section(kept)).is_some(), "{kept:?} stays");
            }
            for gone in TOOLS.iter().filter(|tool| **tool != "armor-export-trigger") {
                assert!(window.try_find(*gone).is_none(), "{gone} is for armor or a ship");
            }
            assert!(window.try_find("armor-export-trigger").is_some(), "a model still exports");
            assert!(window.try_find("armor-cast-hint").is_none(), "there is nothing to cast at");
        })
        .expect("the test window stays open");
    }

    #[gpui_kit::test]
    fn a_model_with_armor_keeps_the_armor_sections_but_not_the_ship_tools(cx: &mut TestAppContext) {
        let window = open_at_double_size(cx);
        show(window, fixtures::armored_model_scene(turret()), cx);

        cx.update_window(window.into(), |_, window, _cx| {
            for kept in [OptionSection::Trajectory, OptionSection::PenCheck, OptionSection::Visibility] {
                assert!(window.try_find(section(kept)).is_some(), "{kept:?} reads the model's plates");
            }
            assert!(window.try_find("armor-show-hidden-button").is_some(), "plate tools stay");
            assert!(window.try_find("armor-splash-button").is_none(), "splash boxes are a ship's");
        })
        .expect("the test window stays open");
    }

    /// In the Hull section the part tree comes before the camouflage picker,
    /// and the camouflage picker before the LOD row.
    #[gpui_kit::test]
    fn the_hull_part_tree_comes_before_the_camo_and_lod_pickers(cx: &mut TestAppContext) {
        let window = open_at_double_size(cx);
        let mut scene = fixtures::hull_scene(turret(), vec![fixtures::hull_mesh()], 2);
        scene.camo_scheme_infos = vec![CamoSchemeInfo {
            id: CamoSchemeId(0),
            display_name: "Default".to_string(),
            raw_name: "default".to_string(),
            origin: CamoOrigin::ShipSpecific,
            use_color_scheme: false,
            uv_transforms: HashMap::new(),
        }]
        .into();
        install(window, scene, &[OptionSection::Hull], cx);

        cx.update_window(window.into(), |_, window, _cx| {
            let tree = window.find("armor-hull-visibility-tree").bounds();
            let camo = window.find("armor-camo-picker").bounds();
            let lod = window.find("armor-hull-lod-0").bounds();
            assert!(tree.bottom() <= camo.top(), "the tree at {tree:?} is above the camo picker at {camo:?}");
            assert!(camo.bottom() <= lod.top(), "the camo picker at {camo:?} is above the LOD row at {lod:?}");
        })
        .expect("the test window stays open");
    }

    /// Gives the active pane `scene` and opens `sections`. The test window has
    /// no GPU, so a shown scene would wait for one; the rail reads only the
    /// scene itself. Every other rail section is closed, so the ones asked
    /// for are in view.
    fn install(
        window: WindowHandle<ArmorViewerPane>,
        scene: LoadedShipArmor,
        sections: &[OptionSection],
        cx: &mut TestAppContext,
    ) {
        window
            .update(cx, |pane, _window, cx| {
                pane.active_viewport(cx).update(cx, |view, cx| {
                    view.current_armor = Some(Arc::new(scene));
                    cx.notify();
                });
                const SECTIONS: [OptionSection; 5] = [
                    OptionSection::Trajectory,
                    OptionSection::PenCheck,
                    OptionSection::Visibility,
                    OptionSection::Hull,
                    OptionSection::Display,
                ];
                for section in SECTIONS {
                    if pane.option_sections_open().is_open(section) {
                        pane.toggle_option_section(section, cx);
                    }
                }
                for section in sections {
                    pane.open_option_section(*section, cx);
                }
            })
            .expect("the test window stays open");
        cx.update_window(window.into(), |_, window, cx| window.render_frame(cx)).expect("the test window stays open");
    }

    /// Each section head is a band across its section's whole width, and a
    /// group head across the width of the section it sits in, so each reads
    /// as where that part of the rail begins.
    #[gpui_kit::test]
    fn each_folding_head_spans_its_column(cx: &mut TestAppContext) {
        let window = open_at_double_size(cx);
        let scene = fixtures::hull_scene(turret(), vec![fixtures::hull_mesh()], 1);
        install(window, scene, &[OptionSection::Hull, OptionSection::Display, OptionSection::Lighting], cx);

        cx.update_window(window.into(), |_, window, _cx| {
            for which in [OptionSection::Hull, OptionSection::Display] {
                let head = window.find(section(which)).bounds();
                let body = window.find(format!("armor-options-body-{which:?}")).bounds();
                assert!(
                    head.left() == body.left() && head.right() == body.right(),
                    "{which:?}'s head at {head:?} spans its body at {body:?}"
                );
            }

            let display = window.find("armor-options-body-Display").bounds();
            let group = window.find("armor-options-group-Lighting").bounds();
            assert!(inside_across(display, group), "the lighting head at {group:?} is inside {display:?}");
            assert_eq!(group.right(), display.right(), "the lighting head reaches the section's edge");
        })
        .expect("the test window stays open");
    }

    /// A model with two armor zones and two hull part groups.
    fn two_zone_scene() -> LoadedShipArmor {
        let part = |name: &str, plate: i32| ZonePart { name: name.to_string(), plates: vec![plate] };
        let mut scene = fixtures::armored_model_scene(turret());
        scene.zone_part_plates = vec![
            ArmorZone { name: "Bow".to_string(), parts: vec![part("Bow_Belt", 1000), part("Bow_Deck", 250)] },
            ArmorZone { name: "Stern".to_string(), parts: vec![part("Stern_Deck", 250)] },
        ];
        scene.hull_part_groups = vec![
            ("Hull".to_string(), vec!["Hull_Fore".to_string(), "Hull_Aft".to_string()]),
            ("Main Battery".to_string(), vec!["Turret_A".to_string()]),
        ];

        scene
    }

    /// Opens `zone` in the active viewport's armor tree, as a click on its
    /// caret does.
    fn expand_zone(window: WindowHandle<ArmorViewerPane>, zone: &str, cx: &mut TestAppContext) {
        window
            .update(cx, |pane, _window, cx| {
                pane.active_viewport(cx).update(cx, |view, cx| view.toggle_zone_expanded(zone.to_string(), cx))
            })
            .expect("the test window stays open");
        cx.update_window(window.into(), |_, window, cx| window.render_frame(cx)).expect("the test window stays open");
    }

    /// Deletes what was typed into the focused field, a key at a time.
    fn erase(typed: &str, window: &mut Window, cx: &mut gpui_kit::App) {
        window.press("end", cx);
        for _ in typed.chars() {
            window.press("backspace", cx);
        }
        window.render_frame(cx);
    }

    /// The zone tree's field keeps the parts it matches, each under its zone
    /// opened to show it, counts them, and says when nothing matched;
    /// clearing it brings back the zones open as they were left.
    #[gpui_kit::test]
    fn the_zone_filter_shows_matches_under_their_zones_and_clearing_restores_the_tree(cx: &mut TestAppContext) {
        let window = open_tall(cx);
        install(window, two_zone_scene(), &[OptionSection::Visibility], cx);
        expand_zone(window, "Stern", cx);
        let (bow, stern) = ("armor-vis-zone-row-Bow", "armor-vis-zone-row-Stern");
        let belt = "armor-vis-part-row-Bow-Bow_Belt";
        let bow_deck = "armor-vis-part-row-Bow-Bow_Deck";
        let stern_deck = "armor-vis-part-row-Stern-Stern_Deck";

        cx.update_window(window.into(), |_, window, cx| {
            assert!(window.try_find(bow_deck).is_none(), "Bow starts collapsed");
            assert!(window.try_find(stern_deck).is_some(), "Stern was opened");

            window.click("armor-zone-filter", cx);
            crate::text_filter::take_marks();
            window.input("deck", cx);
            window.render_frame(cx);
            assert!(window.try_find(bow_deck).is_some(), "a match in a collapsed zone is shown");
            assert!(window.try_find(stern_deck).is_some());
            assert!(window.try_find(belt).is_none(), "the belt does not match");
            assert!(window.try_find(bow).is_some() && window.try_find(stern).is_some(), "each match keeps its zone");
            let marked = crate::text_filter::marks_for("Bow_Deck");
            assert_eq!(marked, Some(std::iter::once(4..8).collect()), "the match is marked");
            let count = rust_i18n::t!("ui.armor.tree_matches", count = 2);
            assert_eq!(window.find("armor-zone-filter-count").label(), Some(&*count));

            window.input("zzz", cx);
            window.render_frame(cx);
            assert!(window.try_find(bow).is_none() && window.try_find(stern).is_none(), "nothing is left");
            let none = rust_i18n::t!("ui.armor.tree_no_matches");
            assert_eq!(window.find("armor-zone-filter-count").label(), Some(&*none));

            erase("deckzzz", window, cx);
            assert!(window.try_find("armor-zone-filter-count").is_none(), "no count without a query");
            assert!(window.try_find(bow_deck).is_none(), "Bow is collapsed again");
            assert!(window.try_find(stern_deck).is_some(), "Stern is still open");
        })
        .expect("the test window stays open");
    }

    /// The part tree's field keeps the meshes it matches under their group,
    /// drops groups with none, and clearing it closes the groups again.
    #[gpui_kit::test]
    fn the_part_filter_shows_matches_under_their_groups_and_clearing_restores_the_tree(cx: &mut TestAppContext) {
        let window = open_tall(cx);
        install(window, two_zone_scene(), &[OptionSection::Hull], cx);
        let (hull, battery) = ("armor-hull-vis-group-row-Hull", "armor-hull-vis-group-row-Main Battery");
        let (fore, aft) = ("armor-hull-vis-mesh-row-Hull_Fore", "armor-hull-vis-mesh-row-Hull_Aft");

        cx.update_window(window.into(), |_, window, cx| {
            assert!(window.try_find(aft).is_none(), "groups start collapsed");

            window.click("armor-part-filter", cx);
            crate::text_filter::take_marks();
            window.input("aft", cx);
            window.render_frame(cx);
            assert!(window.try_find(hull).is_some() && window.try_find(aft).is_some(), "the match under its group");
            assert!(window.try_find(fore).is_none() && window.try_find(battery).is_none(), "the rest is dropped");
            assert_eq!(crate::text_filter::marks_for("Hull_Aft"), Some(std::iter::once(5..8).collect()));
            let count = rust_i18n::t!("ui.armor.tree_matches", count = 1);
            assert_eq!(window.find("armor-part-filter-count").label(), Some(&*count));

            erase("aft", window, cx);
            assert!(window.try_find(aft).is_none(), "the group is collapsed again");
            assert!(window.try_find(battery).is_some(), "every group is back");
        })
        .expect("the test window stays open");
    }

    /// The rail's own filter narrows a tree together with the tree's field:
    /// a row is kept when both match it or a row under it.
    #[gpui_kit::test]
    fn the_rail_filter_and_a_tree_field_narrow_together(cx: &mut TestAppContext) {
        let window = open_tall(cx);
        install(window, two_zone_scene(), &[OptionSection::Visibility], cx);

        cx.update_window(window.into(), |_, window, cx| {
            window.click("armor-options-filter", cx);
            window.input("stern", cx);
            window.render_frame(cx);
            window.click("armor-zone-filter", cx);
            window.input("deck", cx);
            window.render_frame(cx);
            assert!(window.try_find("armor-vis-part-row-Stern-Stern_Deck").is_some());
            assert!(window.try_find("armor-vis-zone-row-Bow").is_none(), "Bow does not match the rail's filter");
        })
        .expect("the test window stays open");
    }

    /// The rail's filter finds a camouflage, a zone and a slider, each in a
    /// collapsed section it opens to show it, and Escape clears the filter
    /// and puts every section back as it was.
    #[gpui_kit::test]
    fn the_rail_filter_opens_the_section_holding_a_match_and_escape_restores_the_rail(cx: &mut TestAppContext) {
        let window = open_tall(cx);
        let mut scene = two_zone_scene();
        let camo = |id: usize, name: &str, origin: CamoOrigin| CamoSchemeInfo {
            id: CamoSchemeId(id),
            display_name: name.to_string(),
            raw_name: name.to_lowercase(),
            origin,
            use_color_scheme: false,
            uv_transforms: HashMap::new(),
        };
        scene.camo_scheme_infos =
            vec![camo(0, "Default", CamoOrigin::ShipSpecific), camo(1, "Ocean Wave", CamoOrigin::Universal)].into();
        install(window, scene, &[], cx);
        let roll = rust_i18n::t!("ui.armor.roll").into_owned();
        let bodies = [OptionSection::Visibility, OptionSection::Hull, OptionSection::Display]
            .map(|which| format!("armor-options-body-{which:?}"));

        cx.update_window(window.into(), |_, window, cx| {
            assert!(bodies.iter().all(|body| window.try_find(body.clone()).is_none()), "every section starts closed");

            let mut find = |query: &str, check: &dyn Fn(&Window)| {
                window.click("armor-options-filter", cx);
                window.input(query, cx);
                // Typing draws a frame a key; only the last is checked.
                crate::text_filter::take_marks();
                window.render_frame(cx);
                check(window);

                window.press("escape", cx);
                window.render_frame(cx);
                assert!(
                    bodies.iter().all(|body| window.try_find(body.clone()).is_none()),
                    "Escape after {query:?} puts the sections back closed"
                );
            };
            find("ocean wave", &|window| {
                assert!(window.try_find("armor-options-body-Hull").is_some(), "the hull section opens");
                assert!(crate::text_filter::marks_for("Ocean Wave").is_some(), "the camouflage is drawn marked");
                assert!(crate::text_filter::marks_for("Default").is_none(), "the other camouflage is not drawn");
            });
            find("stern", &|window| {
                assert!(window.try_find("armor-options-body-Visibility").is_some(), "the armor section opens");
                assert!(window.try_find("armor-vis-zone-row-Stern").is_some());
                assert!(window.try_find("armor-vis-zone-row-Bow").is_none());
            });
            find("roll", &|window| {
                assert!(window.try_find("armor-options-body-Display").is_some(), "the display section opens");
                assert!(crate::text_filter::marks_for(&roll).is_some(), "the roll slider is drawn marked");
            });
        })
        .expect("the test window stays open");
    }
}
