use gpui::{App, Window, px, rgb};
use gpui_component::theme::{Theme, ThemeMode, ThemeTokens};

/// Pin gpui-component's Theme to egui's stock dark palette, scaled by `zoom`.
///
/// Values are taken directly from egui's `Visuals::dark()` / `Widgets::dark()`
/// (egui 0.34, `style.rs`):
/// - `panel_fill` / `window_fill`: `Color32::from_gray(27)`
/// - `Visuals::text_color()` (== `widgets.noninteractive.fg_stroke`): `from_gray(140)`
/// - `widgets.noninteractive.bg_stroke` / `window_stroke`: `from_gray(60)`
/// - `widgets.inactive.bg_fill` (button/secondary surface background): `from_gray(60)`
/// - `Selection::bg_fill`: `Color32::from_rgb(0, 92, 128)`
/// - `widgets.noninteractive.corner_radius`: `2`
///
/// `list_active`/`list_active_border` (the tree's selected-row background)
/// are pinned to the same `from_gray(60)` surface gray rather than
/// gpui-component's default bright-blue `#1e40af` list-active token, matching
/// egui's `colorize_label` selection style (`ui/replay_parser/mod.rs`'s
/// white-on-`Color32::DARK_GRAY` selected label) -- a quiet dark highlight,
/// not an accent color.
pub fn apply_egui_dark_theme(zoom: f32, window: &mut Window, cx: &mut App) {
    Theme::change(ThemeMode::Dark, Some(window), cx);

    let theme = Theme::global_mut(cx);
    theme.background = rgb(0x1b1b1b).into();
    theme.foreground = rgb(0x8c8c8c).into();
    theme.border = rgb(0x3c3c3c).into();
    theme.secondary = rgb(0x3c3c3c).into();
    theme.accent = rgb(0x005c80).into();
    theme.selection = rgb(0x005c80).into();
    theme.list_active = rgb(0x3c3c3c).into();
    theme.list_active_border = rgb(0x3c3c3c).into();
    theme.tokens = ThemeTokens::from(&theme.colors);

    // egui body text 12.5px, small rounding, scaled by zoom.
    theme.font_size = px(12.5 * zoom);
    theme.radius = px(2.0 * zoom);

    // rem-based helpers scale via rem size; px-valued fields were scaled above.
    window.set_rem_size(px(16.0 * zoom));
    cx.refresh_windows();
}
