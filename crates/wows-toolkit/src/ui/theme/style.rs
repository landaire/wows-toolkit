//! Style construction. Both themes share spacing and interaction; each
//! supplies its own `Visuals`. Corners carry a slight radius graded by how
//! much a surface floats or responds; structure (panels, tables, separators)
//! stays square. Active states raise to a hot surface with bright text and an
//! accent border rather than inverting to a filled block. That description is
//! for `Visuals::widgets`; the dock tab strip built here deliberately does not
//! raise its active state, see `dock_style`. The module also owns
//! `paint_active_tab_marker`, a painting function rather than a style value:
//! `egui_dock` has no style field for the marker bar, and `palette` is private
//! to `theme`, so `app.rs` cannot resolve the colour itself.

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
                corner_radius: CornerRadius::same(3),
                fg_stroke: Stroke { width: 1.0, color: p::TEXT_DIM },
                expansion: 0.0,
            },
            inactive: WidgetVisuals {
                bg_fill: p::WIDGET,
                weak_bg_fill: p::WIDGET,
                bg_stroke: Stroke { width: 1.0, color: p::BORDER },
                corner_radius: CornerRadius::same(3),
                fg_stroke: Stroke { width: 1.0, color: p::TEXT },
                expansion: 0.0,
            },
            hovered: WidgetVisuals {
                bg_fill: p::WIDGET_HOT,
                weak_bg_fill: p::WIDGET_HOT,
                bg_stroke: Stroke { width: 1.0, color: p::BORDER_BRIGHT },
                corner_radius: CornerRadius::same(3),
                fg_stroke: Stroke { width: 1.0, color: p::TEXT_BRIGHT },
                expansion: 1.0,
            },
            active: WidgetVisuals {
                bg_fill: p::WIDGET_HOT,
                weak_bg_fill: p::WIDGET_HOT,
                bg_stroke: Stroke { width: 1.0, color: p::ACCENT },
                corner_radius: CornerRadius::same(3),
                fg_stroke: Stroke { width: 1.0, color: p::TEXT_BRIGHT },
                expansion: 1.0,
            },
            open: WidgetVisuals {
                bg_fill: p::WIDGET_HOT,
                weak_bg_fill: p::WIDGET_HOT,
                bg_stroke: Stroke { width: 1.0, color: p::BORDER_BRIGHT },
                corner_radius: CornerRadius::same(3),
                fg_stroke: Stroke { width: 1.0, color: p::TEXT },
                expansion: 0.0,
            },
        },
        selection: Selection { bg_fill: p::SELECTION, stroke: Stroke { width: 1.0, color: p::ACCENT } },
        hyperlink_color: Color32::from_rgb(0x7F, 0xB4, 0xE8),
        faint_bg_color: p::FAINT,
        extreme_bg_color: p::EXTREME,
        code_bg_color: p::SURFACE,
        warn_fg_color: semantic::DARK.warn,
        error_fg_color: semantic::DARK.error,
        window_corner_radius: CornerRadius::same(6),
        window_shadow: Shadow {
            spread: 0,
            color: Color32::from_rgba_premultiplied(0, 0, 0, 140),
            blur: 24,
            offset: [0, 16],
        },
        window_fill: p::PANEL,
        window_stroke: Stroke { width: 1.0, color: p::BORDER },
        menu_corner_radius: CornerRadius::same(4),
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
                corner_radius: CornerRadius::same(3),
                fg_stroke: Stroke { width: 1.0, color: p::TEXT_DIM },
                expansion: 0.0,
            },
            inactive: WidgetVisuals {
                bg_fill: p::WIDGET,
                weak_bg_fill: p::WIDGET,
                bg_stroke: Stroke { width: 1.0, color: p::BORDER },
                corner_radius: CornerRadius::same(3),
                fg_stroke: Stroke { width: 1.0, color: p::TEXT },
                expansion: 0.0,
            },
            hovered: WidgetVisuals {
                bg_fill: p::WIDGET_HOT,
                weak_bg_fill: p::WIDGET_HOT,
                bg_stroke: Stroke { width: 1.0, color: p::BORDER_BRIGHT },
                corner_radius: CornerRadius::same(3),
                fg_stroke: Stroke { width: 1.0, color: p::TEXT_BRIGHT },
                expansion: 1.0,
            },
            active: WidgetVisuals {
                bg_fill: p::WIDGET_HOT,
                weak_bg_fill: p::WIDGET_HOT,
                bg_stroke: Stroke { width: 1.0, color: p::ACCENT },
                corner_radius: CornerRadius::same(3),
                fg_stroke: Stroke { width: 1.0, color: p::TEXT_BRIGHT },
                expansion: 1.0,
            },
            open: WidgetVisuals {
                bg_fill: p::WIDGET_HOT,
                weak_bg_fill: p::WIDGET_HOT,
                bg_stroke: Stroke { width: 1.0, color: p::BORDER_BRIGHT },
                corner_radius: CornerRadius::same(3),
                fg_stroke: Stroke { width: 1.0, color: p::TEXT },
                expansion: 0.0,
            },
        },
        selection: Selection { bg_fill: p::SELECTION, stroke: Stroke { width: 1.0, color: p::ACCENT } },
        hyperlink_color: Color32::from_rgb(0x1B, 0x5F, 0xA8),
        faint_bg_color: p::FAINT,
        extreme_bg_color: p::EXTREME,
        code_bg_color: p::SURFACE,
        warn_fg_color: semantic::LIGHT.warn,
        error_fg_color: semantic::LIGHT.error,
        window_corner_radius: CornerRadius::same(6),
        window_shadow: Shadow {
            spread: 0,
            color: Color32::from_rgba_premultiplied(0, 0, 0, 50),
            blur: 24,
            offset: [0, 16],
        },
        window_fill: p::PANEL,
        window_stroke: Stroke { width: 1.0, color: p::BORDER },
        menu_corner_radius: CornerRadius::same(4),
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
/// The active tab is filled with the panel tone and the strip's bottom border
/// is suppressed beneath it, so the tab and the page it introduces read as one
/// shape. Every other tab is flat against the strip: the only filled shapes in
/// the row are the active tab and whatever the cursor is over.
pub fn dock_style(egui_style: &egui::Style) -> egui_dock::Style {
    let (surface, panel, widget, border, accent, text_bright, text_dim) = if egui_style.visuals.dark_mode {
        (
            palette::dark::SURFACE,
            palette::dark::PANEL,
            palette::dark::WIDGET,
            palette::dark::BORDER,
            palette::dark::ACCENT,
            palette::dark::TEXT_BRIGHT,
            palette::dark::TEXT_DIM,
        )
    } else {
        (
            palette::light::SURFACE,
            palette::light::PANEL,
            palette::light::WIDGET,
            palette::light::BORDER,
            palette::light::ACCENT,
            palette::light::TEXT_BRIGHT,
            palette::light::TEXT_DIM,
        )
    };

    let mut style = egui_dock::Style::from_egui(egui_style);
    style.tab_bar.bg_fill = surface;
    style.tab_bar.hline_color = border;
    // from_egui adds +2 to the noninteractive corner radius unconditionally and rounds all
    // four corners; top-only rounding matches the tabs sitting on it.
    style.tab_bar.corner_radius = CornerRadius { nw: 3, ne: 3, sw: 0, se: 0 };
    // The panel fill is what merges tab and page: egui_dock overpaints each
    // tab's bottom stroke with a 2px line of that tab's own fill, so with the
    // hline below suppressed there is no seam left between the two.
    style.tab.active.bg_fill = panel;
    style.tab.active.text_color = text_bright;
    style.tab.active.outline_color = border;
    style.tab.focused = style.tab.active.clone();
    style.tab.inactive.bg_fill = surface;
    style.tab.inactive.text_color = text_dim;
    style.tab.inactive.outline_color = Color32::TRANSPARENT;
    style.tab.hovered.bg_fill = widget;
    style.tab.hovered.text_color = text_bright;
    style.tab.hovered.outline_color = Color32::TRANSPARENT;
    // egui_dock swaps to a *_with_kb_focus variant the moment a tab holds
    // keyboard focus, and from_egui derives those from egui's widget visuals
    // rather than from the fills set above. Mirror each base state so the swap
    // is invisible; an inactive tab additionally takes the accent outline,
    // which is the only marker a keyboard user gets there.
    style.tab.active_with_kb_focus = style.tab.active.clone();
    style.tab.focused_with_kb_focus = style.tab.focused.clone();
    style.tab.inactive_with_kb_focus = style.tab.inactive.clone();
    style.tab.inactive_with_kb_focus.outline_color = accent;
    style.tab.hline_below_active_tab_name = false;
    // Tab and its panel read as one shape; the panel fill provides the boundary.
    style.tab.tab_body.stroke = egui::Stroke::NONE;
    style.tab.tab_body.bg_fill = panel;
    // from_egui derives this from widgets.active.fg_stroke; the accent reads as a
    // clearer drag indicator than the plain bright text tone it would default to.
    style.separator.color_dragged = accent;
    // egui_dock halves selection.bg_fill for this, which the theme's dim
    // selection renders invisible. The overlay marks where a dragged tab will
    // land, so it has to read over arbitrary content.
    style.overlay.selection_color = Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 96);
    style
}

/// Style for a dock tab flagging that it needs attention.
///
/// The tab is tinted rather than filled. The active tab holds the only
/// full-strength fill in the strip, and an alert loud enough to beat it hides
/// which tab is open. Hover moves the outline to full-strength `error` and
/// leaves the fill alone, because a deeper tint drops the light theme's `error`
/// label under `contrast::CONTRAST_FLOOR`. Once the flagged tab is the active
/// one the alert has served its purpose, so the active states keep the ordinary
/// active treatment and carry the mark in the label alone.
pub fn alert_tab_style(global_style: &egui_dock::TabStyle, theme: egui::Theme) -> egui_dock::TabStyle {
    let (fill, outline, accent, error) = match theme {
        egui::Theme::Dark => (
            semantic::DARK.alert_tab_fill,
            semantic::DARK.alert_tab_outline,
            palette::dark::ACCENT,
            semantic::DARK.error,
        ),
        egui::Theme::Light => (
            semantic::LIGHT.alert_tab_fill,
            semantic::LIGHT.alert_tab_outline,
            palette::light::ACCENT,
            semantic::LIGHT.error,
        ),
    };

    let mut style = global_style.clone();
    style.inactive.bg_fill = fill;
    style.inactive.outline_color = outline;
    style.inactive.text_color = error;
    // Keeps the accent outline the global style uses for keyboard focus: losing
    // it on this one tab would be a hole in the only affordance a keyboard user
    // has for where focus sits.
    style.inactive_with_kb_focus.bg_fill = fill;
    style.inactive_with_kb_focus.outline_color = accent;
    style.inactive_with_kb_focus.text_color = error;
    style.hovered.bg_fill = fill;
    style.hovered.outline_color = error;
    style.hovered.text_color = error;
    style.active.text_color = error;
    style.focused.text_color = error;
    style.active_with_kb_focus.text_color = error;
    style.focused_with_kb_focus.text_color = error;
    style
}

/// Paints the active dock tab's marker, a bar across the top edge of its button.
///
/// `egui_dock` has no style field for it, so `app.rs` paints it from the tab
/// body, the only hook that runs for the active tab once its strip is already
/// on the layer. The painter has to come from `Context::layer_painter`, which
/// clips to the context's content rect: `Painter::with_clip_rect` intersects
/// with the caller's clip instead of replacing it, and the tab body's clip
/// excludes the strip entirely, so the bar would be clipped away to nothing.
///
/// The bar is inset a point on each side so the tab's own border stays visible
/// down the sides of the button.
pub fn paint_active_tab_marker(painter: &egui::Painter, tab_rect: egui::Rect, theme: egui::Theme) {
    let accent = match theme {
        egui::Theme::Dark => palette::dark::ACCENT,
        egui::Theme::Light => palette::light::ACCENT,
    };
    let bar = egui::Rect::from_min_max(
        egui::pos2(tab_rect.left() + 1.0, tab_rect.top()),
        egui::pos2(tab_rect.right() - 1.0, tab_rect.top() + 2.0),
    );
    painter.rect_filled(bar, CornerRadius { nw: 2, ne: 2, sw: 0, se: 0 }, accent);
}

/// Paints a rule in the gap between each pair of neighbouring dock tabs.
///
/// Inactive tabs carry no fill or outline of their own, which is what keeps the
/// strip quiet, but it also leaves a run of them reading as one block. A short
/// rule centred in the gap divides them without giving each tab an edge back.
///
/// `BORDER_BRIGHT` rather than `BORDER`: this is a line that has to be seen, and
/// `BORDER` on the light theme's strip falls under `contrast::CHROME_LINE_FLOOR`.
/// It is the same tone the query bar's outermost bracket uses, for the same
/// reason.
///
/// Gaps flanking a rect in `outlined` are skipped: a tab drawing its own border
/// already separates itself from its neighbours, and a rule beside it lands hard
/// against that border and reads as a doubled edge. Pairs that do not share a
/// row, or that sit too far apart to be neighbours, belong to different leaves
/// and are skipped too.
pub fn paint_tab_dividers(
    painter: &egui::Painter,
    tab_rects: &[egui::Rect],
    outlined: &[egui::Rect],
    theme: egui::Theme,
) {
    let divider = match theme {
        egui::Theme::Dark => palette::dark::BORDER_BRIGHT,
        egui::Theme::Light => palette::light::BORDER_BRIGHT,
    };
    for pair in tab_rects.windows(2) {
        let (left, right) = (pair[0], pair[1]);
        let gap = right.left() - left.right();
        if (left.top() - right.top()).abs() > 0.5 || !(0.0..=12.0).contains(&gap) {
            continue;
        }
        if outlined.contains(&left) || outlined.contains(&right) {
            continue;
        }
        let inset = (left.height() * 0.28).round();
        painter.vline(
            (left.right() + right.left()) / 2.0,
            (left.top() + inset)..=(left.bottom() - inset),
            Stroke::new(1.0, divider),
        );
    }
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
                assert_eq!(visuals.corner_radius, CornerRadius::same(3), "{name} {state}");
            }
            assert_eq!(style.visuals.window_corner_radius, CornerRadius::same(6), "{name} window");
            assert_eq!(style.visuals.menu_corner_radius, CornerRadius::same(4), "{name} menu");
        }
    }

    #[test]
    fn active_state_uses_bright_text_not_an_inverted_block() {
        let dark = dark_style();
        assert_eq!(dark.visuals.widgets.active.bg_fill, palette::dark::WIDGET_HOT);
        assert_eq!(dark.visuals.widgets.active.fg_stroke.color, palette::dark::TEXT_BRIGHT);

        let light = light_style();
        assert_eq!(light.visuals.widgets.active.bg_fill, palette::light::WIDGET_HOT);
        assert_eq!(light.visuals.widgets.active.fg_stroke.color, palette::light::TEXT_BRIGHT);
    }

    #[test]
    fn dock_style_themes_every_field_it_sets() {
        for (name, egui_style, surface, panel, widget, border, accent, text_bright, text_dim) in [
            (
                "dark",
                dark_style(),
                palette::dark::SURFACE,
                palette::dark::PANEL,
                palette::dark::WIDGET,
                palette::dark::BORDER,
                palette::dark::ACCENT,
                palette::dark::TEXT_BRIGHT,
                palette::dark::TEXT_DIM,
            ),
            (
                "light",
                light_style(),
                palette::light::SURFACE,
                palette::light::PANEL,
                palette::light::WIDGET,
                palette::light::BORDER,
                palette::light::ACCENT,
                palette::light::TEXT_BRIGHT,
                palette::light::TEXT_DIM,
            ),
        ] {
            let s = dock_style(&egui_style);
            let tab_r = CornerRadius { nw: 3, ne: 3, sw: 0, se: 0 };
            assert_eq!(s.tab_bar.bg_fill, surface, "{name} tab_bar.bg_fill");
            assert_eq!(s.tab_bar.hline_color, border, "{name} tab_bar.hline_color");
            assert_eq!(s.tab_bar.corner_radius, tab_r, "{name} tab_bar");

            for (variant, tab, fill, outline, text) in [
                ("active", &s.tab.active, panel, border, text_bright),
                ("focused", &s.tab.focused, panel, border, text_bright),
                ("active_with_kb_focus", &s.tab.active_with_kb_focus, panel, border, text_bright),
                ("focused_with_kb_focus", &s.tab.focused_with_kb_focus, panel, border, text_bright),
                ("inactive", &s.tab.inactive, surface, Color32::TRANSPARENT, text_dim),
                ("inactive_with_kb_focus", &s.tab.inactive_with_kb_focus, surface, accent, text_dim),
                ("hovered", &s.tab.hovered, widget, Color32::TRANSPARENT, text_bright),
            ] {
                assert_eq!(tab.bg_fill, fill, "{name} tab.{variant}.bg_fill");
                assert_eq!(tab.outline_color, outline, "{name} tab.{variant}.outline_color");
                assert_eq!(tab.text_color, text, "{name} tab.{variant}.text_color");
                assert_eq!(tab.corner_radius, tab_r, "{name} tab.{variant}.corner_radius");
            }

            // The one field that carries the merge: with it set, egui_dock skips
            // the strip's bottom border under the active tab, so the tab opens
            // into the page instead of being sealed off from it.
            assert!(!s.tab.hline_below_active_tab_name, "{name} hline_below_active_tab_name");
            assert_eq!(s.tab.tab_body.stroke, Stroke::NONE, "{name} tab_body.stroke");
            assert_eq!(s.tab.tab_body.bg_fill, panel, "{name} tab_body.bg_fill");
            assert_eq!(s.tab.active.bg_fill, s.tab.tab_body.bg_fill, "{name} active tab merges with its body");
            assert_eq!(s.separator.color_dragged, accent, "{name} separator.color_dragged");
            assert_eq!(
                s.overlay.selection_color,
                Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 96),
                "{name} overlay.selection_color"
            );
        }
    }

    #[test]
    fn alert_tab_style_is_legible_and_never_outranks_the_active_tab() {
        use crate::ui::theme::contrast::CHROME_LINE_FLOOR;
        use crate::ui::theme::contrast::CONTRAST_FLOOR;
        use crate::ui::theme::contrast::contrast_ratio;

        for (name, egui_style, theme, surface, fill, outline, accent, error) in [
            (
                "dark",
                dark_style(),
                egui::Theme::Dark,
                palette::dark::SURFACE,
                semantic::DARK.alert_tab_fill,
                semantic::DARK.alert_tab_outline,
                palette::dark::ACCENT,
                semantic::DARK.error,
            ),
            (
                "light",
                light_style(),
                egui::Theme::Light,
                palette::light::SURFACE,
                semantic::LIGHT.alert_tab_fill,
                semantic::LIGHT.alert_tab_outline,
                palette::light::ACCENT,
                semantic::LIGHT.error,
            ),
        ] {
            let global = dock_style(&egui_style).tab;
            let s = alert_tab_style(&global, theme);

            let outline_ratio = contrast_ratio(outline, surface);
            assert!(
                outline_ratio >= CHROME_LINE_FLOOR,
                "{name} alert outline {outline:?} on surface {surface:?} only reached {outline_ratio}"
            );

            // Hover moves the outline, not the fill: a deeper tint drops the
            // light theme's error label under the contrast floor.
            for (variant, tab, want_outline) in [
                ("inactive", &s.inactive, outline),
                ("inactive_with_kb_focus", &s.inactive_with_kb_focus, accent),
                ("hovered", &s.hovered, error),
            ] {
                assert_eq!(tab.bg_fill, fill, "{name} alert.{variant}.bg_fill");
                assert_eq!(tab.outline_color, want_outline, "{name} alert.{variant}.outline_color");
                assert_eq!(tab.text_color, error, "{name} alert.{variant}.text_color");
                let ratio = contrast_ratio(tab.text_color, tab.bg_fill);
                assert!(
                    ratio >= CONTRAST_FLOOR,
                    "{name} alert.{variant}: error {error:?} on {fill:?} only reached {ratio}"
                );
            }

            // Once the tab is the active one the alert has done its job, so it
            // keeps the ordinary active fill and carries the mark in the label.
            for (variant, tab, global_tab) in [
                ("active", &s.active, &global.active),
                ("focused", &s.focused, &global.focused),
                ("active_with_kb_focus", &s.active_with_kb_focus, &global.active_with_kb_focus),
                ("focused_with_kb_focus", &s.focused_with_kb_focus, &global.focused_with_kb_focus),
            ] {
                assert_eq!(tab.bg_fill, global_tab.bg_fill, "{name} alert.{variant}.bg_fill");
                assert_eq!(tab.outline_color, global_tab.outline_color, "{name} alert.{variant}.outline_color");
                assert_eq!(tab.text_color, error, "{name} alert.{variant}.text_color");
            }
        }
    }

    #[test]
    fn the_tab_divider_reads_against_the_strip() {
        use crate::ui::theme::contrast::CHROME_LINE_FLOOR;
        use crate::ui::theme::contrast::contrast_ratio;

        // The divider is the only thing separating one inactive tab from the
        // next, so it has to clear the floor on the strip it sits on. Plain
        // BORDER does not, on the light theme.
        for (name, divider, surface) in [
            ("dark", palette::dark::BORDER_BRIGHT, palette::dark::SURFACE),
            ("light", palette::light::BORDER_BRIGHT, palette::light::SURFACE),
        ] {
            let ratio = contrast_ratio(divider, surface);
            assert!(ratio >= CHROME_LINE_FLOOR, "{name} tab divider on the strip only reached {ratio}");
        }
    }

    #[test]
    fn themes_declare_their_own_mode() {
        assert!(dark_style().visuals.dark_mode);
        assert!(!light_style().visuals.dark_mode);
    }

    #[test]
    fn every_tab_style_variant_is_legible() {
        use crate::ui::theme::contrast::CONTRAST_FLOOR;
        use crate::ui::theme::contrast::contrast_ratio;

        // Color32 is premultiplied; a translucent fill must be composited over
        // what it actually sits on (the tab bar) before measuring contrast.
        fn over(fg: egui::Color32, bg: egui::Color32) -> egui::Color32 {
            let inv = 1.0 - (f32::from(fg.a()) / 255.0);
            let mix = |f: u8, b: u8| (f32::from(f) + f32::from(b) * inv).round() as u8;
            egui::Color32::from_rgb(mix(fg.r(), bg.r()), mix(fg.g(), bg.g()), mix(fg.b(), bg.b()))
        }

        for (name, egui_style) in [("dark", dark_style()), ("light", light_style())] {
            let s = dock_style(&egui_style);
            let tab_bar_bg = s.tab_bar.bg_fill;
            for (variant, style) in [
                ("active", &s.tab.active),
                ("inactive", &s.tab.inactive),
                ("focused", &s.tab.focused),
                ("hovered", &s.tab.hovered),
                ("active_with_kb_focus", &s.tab.active_with_kb_focus),
                ("inactive_with_kb_focus", &s.tab.inactive_with_kb_focus),
                ("focused_with_kb_focus", &s.tab.focused_with_kb_focus),
            ] {
                let bg = over(style.bg_fill, tab_bar_bg);
                let ratio = contrast_ratio(style.text_color, bg);
                assert!(
                    ratio >= CONTRAST_FLOOR,
                    "{name} tab.{variant}: text {:?} on bg_fill {:?} (composited over tab_bar {:?}) only reached {ratio}",
                    style.text_color,
                    style.bg_fill,
                    bg
                );
            }
        }
    }

    #[test]
    fn derived_text_colors_are_legible_on_their_surfaces() {
        use crate::ui::theme::contrast::CONTRAST_FLOOR;
        use crate::ui::theme::contrast::contrast_ratio;

        for (name, style) in [("dark", dark_style()), ("light", light_style())] {
            let v = &style.visuals;
            // egui derives these from widget fields; a theme that repurposes
            // those fields can make them collide with the surfaces they land on.
            for (what, fg) in [("text_color", v.text_color()), ("strong_text_color", v.strong_text_color())] {
                for (sname, bg) in [("panel", v.panel_fill), ("window", v.window_fill), ("extreme", v.extreme_bg_color)]
                {
                    let r = contrast_ratio(fg, bg);
                    assert!(r >= CONTRAST_FLOOR, "{name} {what} on {sname} is {r}");
                }
            }
        }
    }
}
