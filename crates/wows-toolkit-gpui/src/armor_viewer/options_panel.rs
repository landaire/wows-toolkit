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

use super::options_filter::OptionsFilter;
use super::pane::ArmorViewerPane;
use super::pane::OptionSection;
use super::popover;

/// Fixed: the sections below are their own widths, so there is nothing for a
/// drag handle to do but overflow or pad them.
pub const PANEL_WIDTH: Pixels = px(300.);

pub fn render_panel(view: &ArmorViewerPane, pane: &Entity<ArmorViewerPane>, cx: &mut App) -> AnyElement {
    let viewport = view.active_viewport(cx);
    let position = view.active_pane_position(cx);
    let border = cx.theme().border;
    let accent = cx.theme().primary;

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
        .child(
            Input::new(&filter_input)
                .id("armor-options-filter")
                .small()
                .prefix(Icon::new(IconName::Search).small())
                .cleanable(true),
        );

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
                    // Nothing in it is a setting a filter could find.
                    (!filter.is_active()).then(|| super::trajectory_panel::render(view, pane, &viewport, cx))
                },
            ),
            section(
                view,
                pane,
                OptionSection::PenCheck,
                t!("ui.armor.pen_check").as_ref(),
                &filter,
                cx,
                |filter, cx| (!filter.is_active()).then(|| super::analysis::render_section(view, pane, cx)),
            ),
            section(
                view,
                pane,
                OptionSection::Visibility,
                t!("ui.armor.armor_options").as_ref(),
                &filter,
                cx,
                |filter, cx| popover::render_popover_content(&visibility, filter, cx),
            ),
        ]
    } else {
        Vec::new()
    };
    let mesh_sections = [
        section(view, pane, OptionSection::Hull, t!("ui.armor.hull_toggle").as_ref(), &filter, cx, |filter, cx| {
            popover::render_hull_popover_content(&hull, filter, cx)
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
    filter: &OptionsFilter,
    cx: &mut App,
    body: impl FnOnce(&OptionsFilter, &mut App) -> Option<AnyElement>,
) -> Option<AnyElement> {
    let marked = filter.label(title.to_string(), popover::mark());
    let (expanded, content) = if filter.is_active() {
        let inner = if marked.is_some() { OptionsFilter::default() } else { filter.clone() };
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
    use crate::armor_viewer::load_ship::LoadedShipArmor;
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
        }];
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
    /// scene itself.
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
}
