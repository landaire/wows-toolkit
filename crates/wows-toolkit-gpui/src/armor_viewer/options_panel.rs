//! The armor viewer's options rail: the armor, hull and display sections.
//!
//! The viewport toolbar carries the controls that are one click. These three
//! hold a tree or a row of sliders, which a popover can only show by covering
//! the hull they describe, so they are a panel beside it instead.
//!
//! The sections act on the pane the reader last clicked in
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
use gpui_kit::component::h_flex;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rust_i18n::t;

use super::pane::ArmorViewerPane;
use super::pane::OptionSection;
use super::popover;

/// Fixed: the sections below are their own widths, so there is nothing for a
/// drag handle to do but overflow or pad them.
pub const PANEL_WIDTH: Pixels = px(300.);

pub fn render_panel(view: &ArmorViewerPane, pane: &Entity<ArmorViewerPane>, cx: &mut App) -> AnyElement {
    let viewport = view.active_viewport(cx);
    let legend_visible = view.legend_visible();

    // Which pane these act on, since with a comparison split open the rail
    // edits whichever was last clicked in.
    let header = h_flex()
        .gap_2()
        .items_center()
        .child(div().text_sm().font_weight(FontWeight::BOLD).child(t!("ui.armor.options").to_string()))
        .when_some(viewport.read(cx).shown_ship_name(), |this, name| {
            this.child(div().flex_1().text_xs().text_color(crate::theme::text_dim()).child(name))
        });

    let visibility = viewport.clone();
    let hull = viewport.clone();
    let display = viewport.clone();

    v_flex()
        .id("armor-options-panel")
        // Without this the harness cannot find a plain container by its id.
        .test_support()
        .size_full()
        .border_l_1()
        .border_color(cx.theme().border)
        .gap_2()
        .p_2()
        .overflow_y_scroll()
        .when(viewport.read(cx).trajectory_state().shown, |panel| {
            panel.child(popover::render_trajectory_panel(view, pane, &viewport, cx))
        })
        .child(header)
        .child(section(view, pane, OptionSection::Visibility, t!("ui.armor.armor_options").as_ref(), cx, |cx| {
            popover::render_popover_content(&visibility, cx)
        }))
        .child(section(view, pane, OptionSection::Hull, t!("ui.armor.hull_toggle").as_ref(), cx, |cx| {
            popover::render_hull_popover_content(&hull, cx)
        }))
        .child(section(view, pane, OptionSection::Display, t!("ui.armor.display").as_ref(), cx, |cx| {
            popover::render_display_popover_content(&display, legend_visible, cx)
        }))
        .into_any_element()
}

/// One section, labelled as its toolbar button was. `body` is called only while
/// the section is expanded, which is what the collapse is for.
fn section(
    view: &ArmorViewerPane,
    pane: &Entity<ArmorViewerPane>,
    which: OptionSection,
    title: &str,
    cx: &mut App,
    body: impl FnOnce(&mut App) -> AnyElement,
) -> AnyElement {
    let expanded = view.option_sections_open().is_open(which);
    let pane = pane.clone();

    let head = h_flex()
        .id(SharedString::from(format!("armor-options-section-{which:?}")))
        .test_support()
        .gap_1()
        .items_center()
        .cursor_pointer()
        // A band, not dim small caps: the trees inside carry their own group
        // labels, and a section title has to read above them.
        .px_2()
        .py_1()
        .rounded(cx.theme().radius)
        .bg(cx.theme().muted)
        .child(Icon::new(if expanded { IconName::ChevronDown } else { IconName::ChevronRight }))
        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(title.to_string()))
        .on_click(move |_event, _window, cx: &mut App| {
            pane.update(cx, |pane, cx| pane.toggle_option_section(which, cx));
        });

    v_flex()
        .gap_1()
        .child(head)
        .when(expanded, |this| {
            this.child(
                div().id(SharedString::from(format!("armor-options-body-{which:?}"))).test_support().child(body(cx)),
            )
        })
        .into_any_element()
}
