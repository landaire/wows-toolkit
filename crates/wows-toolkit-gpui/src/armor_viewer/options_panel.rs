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
    let shift = crate::status_bar::ModifierKey::Shift.label(cx);
    // Where the trajectory toggle was: a Shift-click casts, with nothing to
    // switch on first, and this is where a reader looks for how.
    let cast_hint = (!filter.is_active()).then(|| {
        h_flex()
            .id("armor-cast-hint")
            .test_support()
            .gap_1()
            .items_start()
            .text_xs()
            .text_color(crate::theme::text_dim())
            .child(crate::icons::icon(crate::icons::Glyph::Crosshair))
            .child(div().flex_1().whitespace_normal().child(t!("ui.armor.cast_hint", key = shift).into_owned()))
    });

    let visibility = viewport.clone();
    let hull = viewport.clone();
    let display = viewport.clone();
    let groups = popover::DisplayGroups {
        pane: pane.clone(),
        legend_visible: view.legend_visible(),
        open: view.option_sections_open(),
    };

    let sections: Vec<AnyElement> = [
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
        section(view, pane, OptionSection::PenCheck, t!("ui.armor.pen_check").as_ref(), &filter, cx, |filter, cx| {
            (!filter.is_active()).then(|| super::analysis::render_section(view, pane, cx))
        }),
        section(
            view,
            pane,
            OptionSection::Visibility,
            t!("ui.armor.armor_options").as_ref(),
            &filter,
            cx,
            |filter, cx| popover::render_popover_content(&visibility, filter, cx),
        ),
        section(view, pane, OptionSection::Hull, t!("ui.armor.hull_toggle").as_ref(), &filter, cx, |filter, cx| {
            popover::render_hull_popover_content(&hull, filter, cx)
        }),
        section(view, pane, OptionSection::Display, t!("ui.armor.display").as_ref(), &filter, cx, |filter, cx| {
            popover::render_display_popover_content(&display, &groups, filter, cx)
        }),
    ]
    .into_iter()
    .flatten()
    .collect();
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

    let head = h_flex()
        .id(SharedString::from(format!("armor-options-section-{which:?}")))
        .test_support()
        .gap_1()
        .items_center()
        // A band, not dim small caps: the trees inside carry their own group
        // labels, and a section title has to read above them.
        .px_2()
        .py_1()
        .rounded(cx.theme().radius)
        .bg(cx.theme().muted)
        .child(crate::motion::DisclosureChevron::new("caret", expanded))
        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(title))
        .when(!filter.is_active(), |head| {
            head.cursor_pointer().on_click(move |_event, _window, cx: &mut App| {
                pane.update(cx, |pane, cx| pane.toggle_option_section(which, cx));
            })
        });

    Some(
        v_flex()
            .gap_1()
            .child(head)
            .children(content.map(|content| {
                div().id(SharedString::from(format!("armor-options-body-{which:?}"))).test_support().child(content)
            }))
            .into_any_element(),
    )
}
