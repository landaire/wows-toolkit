//! Style construction. Both themes share spacing and interaction; each
//! supplies its own `Visuals`. Square corners throughout, and active states
//! invert to the bone accent rather than picking up a hue.

use egui::Color32;
use egui::CornerRadius;
use egui::Margin;
use egui::Stroke;
use egui::Style;
use egui::Vec2;
use egui::Visuals;
use egui::epaint::Shadow;
use egui::style::Interaction;
use egui::style::ScrollStyle;
use egui::style::Selection;
use egui::style::Spacing;
use egui::style::TextCursorStyle;
use egui::style::WidgetVisuals;
use egui::style::Widgets;

use crate::ui::theme::palette;
use crate::ui::theme::semantic;

pub fn dark_style() -> Style {
    base_style(dark_visuals())
}

pub fn light_style() -> Style {
    base_style(light_visuals())
}

/// Spacing, interaction and animation shared by both themes.
fn base_style(visuals: Visuals) -> Style {
    Style {
        spacing: Spacing {
            item_spacing: Vec2 { x: 8.0, y: 5.0 },
            window_margin: Margin::same(8),
            button_padding: Vec2 { x: 8.0, y: 3.0 },
            menu_margin: Margin::same(8),
            indent: 18.0,
            interact_size: Vec2 { x: 42.0, y: 22.0 },
            slider_width: 120.0,
            combo_width: 120.0,
            text_edit_width: 280.0,
            icon_width: 16.0,
            icon_width_inner: 9.0,
            icon_spacing: 5.0,
            tooltip_width: 520.0,
            indent_ends_with_horizontal_line: false,
            combo_height: 220.0,
            scroll: ScrollStyle {
                bar_width: 10.0,
                handle_min_length: 18.0,
                bar_inner_margin: 3.0,
                bar_outer_margin: 1.0,
                ..Default::default()
            },
            ..Default::default()
        },
        interaction: Interaction {
            resize_grab_radius_side: 6.0,
            resize_grab_radius_corner: 12.0,
            show_tooltips_only_when_still: true,
            ..Default::default()
        },
        visuals,
        animation_time: 1.0 / 12.0,
        explanation_tooltips: false,
        ..Default::default()
    }
}

fn dark_visuals() -> Visuals {
    use palette::dark as p;

    Visuals {
        dark_mode: true,
        override_text_color: None,
        widgets: Widgets {
            noninteractive: WidgetVisuals {
                bg_fill: p::CARD,
                weak_bg_fill: p::CARD,
                bg_stroke: Stroke { width: 1.0, color: p::BORDER },
                corner_radius: CornerRadius::ZERO,
                fg_stroke: Stroke { width: 1.0, color: p::TEXT_DIM },
                expansion: 0.0,
            },
            inactive: WidgetVisuals {
                bg_fill: p::WIDGET,
                weak_bg_fill: p::WIDGET,
                bg_stroke: Stroke { width: 1.0, color: p::BORDER },
                corner_radius: CornerRadius::ZERO,
                fg_stroke: Stroke { width: 1.0, color: p::TEXT },
                expansion: 0.0,
            },
            hovered: WidgetVisuals {
                bg_fill: p::WIDGET_HOT,
                weak_bg_fill: p::WIDGET_HOT,
                bg_stroke: Stroke { width: 1.0, color: p::BORDER_BRIGHT },
                corner_radius: CornerRadius::ZERO,
                fg_stroke: Stroke { width: 1.0, color: p::TEXT_BRIGHT },
                expansion: 1.0,
            },
            active: WidgetVisuals {
                bg_fill: p::ACCENT,
                weak_bg_fill: p::ACCENT,
                bg_stroke: Stroke { width: 1.0, color: p::ACCENT },
                corner_radius: CornerRadius::ZERO,
                fg_stroke: Stroke { width: 1.0, color: p::PANEL },
                expansion: 1.0,
            },
            open: WidgetVisuals {
                bg_fill: p::WIDGET_HOT,
                weak_bg_fill: p::WIDGET_HOT,
                bg_stroke: Stroke { width: 1.0, color: p::BORDER_BRIGHT },
                corner_radius: CornerRadius::ZERO,
                fg_stroke: Stroke { width: 1.0, color: p::TEXT },
                expansion: 0.0,
            },
        },
        selection: Selection { bg_fill: p::SELECTION, stroke: Stroke { width: 1.0, color: p::ACCENT } },
        hyperlink_color: p::TEXT_BRIGHT,
        faint_bg_color: p::FAINT,
        extreme_bg_color: p::EXTREME,
        code_bg_color: p::SURFACE,
        warn_fg_color: semantic::DARK.warn,
        error_fg_color: semantic::DARK.error,
        window_corner_radius: CornerRadius::ZERO,
        window_shadow: Shadow {
            spread: 0,
            color: Color32::from_rgba_premultiplied(0, 0, 0, 140),
            blur: 24,
            offset: [0, 16],
        },
        window_fill: p::PANEL,
        window_stroke: Stroke { width: 1.0, color: p::BORDER },
        menu_corner_radius: CornerRadius::ZERO,
        panel_fill: p::PANEL,
        popup_shadow: Shadow {
            spread: 0,
            color: Color32::from_rgba_premultiplied(0, 0, 0, 120),
            blur: 16,
            offset: [0, 10],
        },
        resize_corner_size: 12.0,
        text_cursor: TextCursorStyle {
            stroke: Stroke { width: 2.0, color: p::ACCENT },
            preview: false,
            ..Default::default()
        },
        clip_rect_margin: 3.0,
        button_frame: true,
        collapsing_header_frame: false,
        indent_has_left_vline: true,
        striped: true,
        slider_trailing_fill: true,
        ..Visuals::dark()
    }
}

fn light_visuals() -> Visuals {
    use palette::light as p;

    Visuals {
        dark_mode: false,
        override_text_color: None,
        widgets: Widgets {
            noninteractive: WidgetVisuals {
                bg_fill: p::CARD,
                weak_bg_fill: p::CARD,
                bg_stroke: Stroke { width: 1.0, color: p::BORDER },
                corner_radius: CornerRadius::ZERO,
                fg_stroke: Stroke { width: 1.0, color: p::TEXT_DIM },
                expansion: 0.0,
            },
            inactive: WidgetVisuals {
                bg_fill: p::WIDGET,
                weak_bg_fill: p::WIDGET,
                bg_stroke: Stroke { width: 1.0, color: p::BORDER },
                corner_radius: CornerRadius::ZERO,
                fg_stroke: Stroke { width: 1.0, color: p::TEXT },
                expansion: 0.0,
            },
            hovered: WidgetVisuals {
                bg_fill: p::WIDGET_HOT,
                weak_bg_fill: p::WIDGET_HOT,
                bg_stroke: Stroke { width: 1.0, color: p::BORDER_BRIGHT },
                corner_radius: CornerRadius::ZERO,
                fg_stroke: Stroke { width: 1.0, color: p::TEXT_BRIGHT },
                expansion: 1.0,
            },
            active: WidgetVisuals {
                bg_fill: p::ACCENT,
                weak_bg_fill: p::ACCENT,
                bg_stroke: Stroke { width: 1.0, color: p::ACCENT },
                corner_radius: CornerRadius::ZERO,
                fg_stroke: Stroke { width: 1.0, color: p::PANEL },
                expansion: 1.0,
            },
            open: WidgetVisuals {
                bg_fill: p::WIDGET_HOT,
                weak_bg_fill: p::WIDGET_HOT,
                bg_stroke: Stroke { width: 1.0, color: p::BORDER_BRIGHT },
                corner_radius: CornerRadius::ZERO,
                fg_stroke: Stroke { width: 1.0, color: p::TEXT },
                expansion: 0.0,
            },
        },
        selection: Selection { bg_fill: p::SELECTION, stroke: Stroke { width: 1.0, color: p::ACCENT } },
        hyperlink_color: p::TEXT_BRIGHT,
        faint_bg_color: p::FAINT,
        extreme_bg_color: p::EXTREME,
        code_bg_color: p::SURFACE,
        warn_fg_color: semantic::LIGHT.warn,
        error_fg_color: semantic::LIGHT.error,
        window_corner_radius: CornerRadius::ZERO,
        window_shadow: Shadow {
            spread: 0,
            color: Color32::from_rgba_premultiplied(0, 0, 0, 50),
            blur: 24,
            offset: [0, 16],
        },
        window_fill: p::PANEL,
        window_stroke: Stroke { width: 1.0, color: p::BORDER },
        menu_corner_radius: CornerRadius::ZERO,
        panel_fill: p::PANEL,
        popup_shadow: Shadow {
            spread: 0,
            color: Color32::from_rgba_premultiplied(0, 0, 0, 40),
            blur: 16,
            offset: [0, 10],
        },
        resize_corner_size: 12.0,
        text_cursor: TextCursorStyle {
            stroke: Stroke { width: 2.0, color: p::ACCENT },
            preview: false,
            ..Default::default()
        },
        clip_rect_margin: 3.0,
        button_frame: true,
        collapsing_header_frame: false,
        indent_has_left_vline: true,
        striped: true,
        slider_trailing_fill: true,
        ..Visuals::light()
    }
}

/// Tweaks on top of `egui_dock`'s `Style::from_egui`.
///
/// `from_egui` makes the active tab's fill match `window_fill`, fusing it with
/// the panel below. Graphite & Bone instead inverts the active tab into a solid
/// bone block so it reads as a stencilled label, with the tab strip pushed to
/// the surface tone so it is clearly its own band.
pub fn dock_style(egui_style: &egui::Style) -> egui_dock::Style {
    let (surface, panel, widget_hot, border, accent, text_bright) = if egui_style.visuals.dark_mode {
        (
            palette::dark::SURFACE,
            palette::dark::PANEL,
            palette::dark::WIDGET_HOT,
            palette::dark::BORDER,
            palette::dark::ACCENT,
            palette::dark::TEXT_BRIGHT,
        )
    } else {
        (
            palette::light::SURFACE,
            palette::light::PANEL,
            palette::light::WIDGET_HOT,
            palette::light::BORDER,
            palette::light::ACCENT,
            palette::light::TEXT_BRIGHT,
        )
    };

    let mut style = egui_dock::Style::from_egui(egui_style);
    style.tab_bar.bg_fill = surface;
    style.tab_bar.hline_color = border;
    // from_egui adds +2 to the noninteractive corner radius unconditionally; square it back off.
    style.tab_bar.corner_radius = CornerRadius::ZERO;
    style.tab.active.bg_fill = accent;
    style.tab.active.text_color = panel;
    style.tab.active.outline_color = accent;
    style.tab.focused.bg_fill = accent;
    style.tab.focused.text_color = panel;
    style.tab.focused.outline_color = accent;
    style.tab.inactive.bg_fill = surface;
    style.tab.inactive.outline_color = border;
    style.tab.hovered.bg_fill = widget_hot;
    style.tab.hovered.outline_color = border;
    style.tab.hovered.text_color = text_bright;
    // egui_dock swaps to a *_with_kb_focus variant the moment a tab holds
    // keyboard focus. Left underived they lose the bone inversion, so mirror
    // each base state and mark keyboard focus with the accent outline.
    style.tab.active_with_kb_focus = style.tab.active.clone();
    style.tab.focused_with_kb_focus = style.tab.focused.clone();
    style.tab.inactive_with_kb_focus = style.tab.inactive.clone();
    style.tab.inactive_with_kb_focus.outline_color = accent;
    style.tab.hline_below_active_tab_name = false;
    // Tab and its panel read as one shape; the panel fill provides the boundary.
    style.tab.tab_body.stroke = egui::Stroke::NONE;
    style.tab.tab_body.bg_fill = panel;
    // from_egui derives this from widgets.active.fg_stroke, which this theme sets to
    // PANEL for the knocked-out label. That would make a dragged separator invisible.
    style.separator.color_dragged = accent;
    // egui_dock halves selection.bg_fill for this, which the theme's dim
    // selection renders invisible. The overlay marks where a dragged tab will
    // land, so it has to read over arbitrary content.
    style.overlay.selection_color = Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 96);
    style
}

#[cfg(test)]
mod tests {
    use egui::Color32;
    use egui::CornerRadius;
    use egui::Stroke;

    use super::*;
    use crate::ui::theme::palette;

    #[test]
    fn every_widget_state_has_square_corners() {
        for (name, style) in [("dark", dark_style()), ("light", light_style())] {
            let w = &style.visuals.widgets;
            for (state, visuals) in [
                ("noninteractive", &w.noninteractive),
                ("inactive", &w.inactive),
                ("hovered", &w.hovered),
                ("active", &w.active),
                ("open", &w.open),
            ] {
                assert_eq!(visuals.corner_radius, CornerRadius::ZERO, "{name} {state} is rounded");
            }
            assert_eq!(style.visuals.window_corner_radius, CornerRadius::ZERO, "{name} window is rounded");
            assert_eq!(style.visuals.menu_corner_radius, CornerRadius::ZERO, "{name} menu is rounded");
        }
    }

    #[test]
    fn active_state_inverts_to_the_bone_accent() {
        let dark = dark_style();
        assert_eq!(dark.visuals.widgets.active.bg_fill, palette::dark::ACCENT);
        assert_eq!(dark.visuals.widgets.active.fg_stroke.color, palette::dark::PANEL);

        let light = light_style();
        assert_eq!(light.visuals.widgets.active.bg_fill, palette::light::ACCENT);
        assert_eq!(light.visuals.widgets.active.fg_stroke.color, palette::light::PANEL);
    }

    #[test]
    fn dock_style_themes_every_field_it_sets() {
        for (name, egui_style, surface, panel, widget_hot, border, accent, text_bright) in [
            (
                "dark",
                dark_style(),
                palette::dark::SURFACE,
                palette::dark::PANEL,
                palette::dark::WIDGET_HOT,
                palette::dark::BORDER,
                palette::dark::ACCENT,
                palette::dark::TEXT_BRIGHT,
            ),
            (
                "light",
                light_style(),
                palette::light::SURFACE,
                palette::light::PANEL,
                palette::light::WIDGET_HOT,
                palette::light::BORDER,
                palette::light::ACCENT,
                palette::light::TEXT_BRIGHT,
            ),
        ] {
            let s = dock_style(&egui_style);
            assert_eq!(s.tab_bar.bg_fill, surface, "{name} tab_bar.bg_fill");
            assert_eq!(s.tab_bar.hline_color, border, "{name} tab_bar.hline_color");
            assert_eq!(s.tab_bar.corner_radius, CornerRadius::ZERO, "{name} tab_bar is rounded");
            assert_eq!(s.tab.active.bg_fill, accent, "{name} tab.active.bg_fill");
            assert_eq!(s.tab.active.text_color, panel, "{name} tab.active.text_color");
            assert_eq!(s.tab.active.outline_color, accent, "{name} tab.active.outline_color");
            assert_eq!(s.tab.focused.bg_fill, accent, "{name} tab.focused.bg_fill");
            assert_eq!(s.tab.focused.text_color, panel, "{name} tab.focused.text_color");
            assert_eq!(s.tab.focused.outline_color, accent, "{name} tab.focused.outline_color");
            assert_eq!(s.tab.inactive.bg_fill, surface, "{name} tab.inactive.bg_fill");
            assert_eq!(s.tab.inactive.outline_color, border, "{name} tab.inactive.outline_color");
            assert_eq!(s.tab.hovered.bg_fill, widget_hot, "{name} tab.hovered.bg_fill");
            assert_eq!(s.tab.hovered.outline_color, border, "{name} tab.hovered.outline_color");
            assert_eq!(s.tab.hovered.text_color, text_bright, "{name} tab.hovered.text_color");
            assert_eq!(s.tab.active_with_kb_focus.bg_fill, accent, "{name} tab.active_with_kb_focus.bg_fill");
            assert_eq!(s.tab.active_with_kb_focus.text_color, panel, "{name} tab.active_with_kb_focus.text_color");
            assert_eq!(
                s.tab.active_with_kb_focus.outline_color, accent,
                "{name} tab.active_with_kb_focus.outline_color"
            );
            assert_eq!(s.tab.focused_with_kb_focus.bg_fill, accent, "{name} tab.focused_with_kb_focus.bg_fill");
            assert_eq!(s.tab.focused_with_kb_focus.text_color, panel, "{name} tab.focused_with_kb_focus.text_color");
            assert_eq!(
                s.tab.focused_with_kb_focus.outline_color, accent,
                "{name} tab.focused_with_kb_focus.outline_color"
            );
            assert_eq!(s.tab.inactive_with_kb_focus.bg_fill, surface, "{name} tab.inactive_with_kb_focus.bg_fill");
            assert_eq!(
                s.tab.inactive_with_kb_focus.outline_color, accent,
                "{name} tab.inactive_with_kb_focus.outline_color"
            );
            assert!(!s.tab.hline_below_active_tab_name, "{name} hline_below_active_tab_name");
            assert_eq!(s.tab.tab_body.stroke, Stroke::NONE, "{name} tab_body.stroke");
            assert_eq!(s.tab.tab_body.bg_fill, panel, "{name} tab_body.bg_fill");
            assert_eq!(s.separator.color_dragged, accent, "{name} separator.color_dragged");
            assert_eq!(
                s.overlay.selection_color,
                Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 96),
                "{name} overlay.selection_color"
            );
        }
    }

    #[test]
    fn themes_declare_their_own_mode() {
        assert!(dark_style().visuals.dark_mode);
        assert!(!light_style().visuals.dark_mode);
    }
}
