use gpui_kit::App;
use gpui_kit::Window;
use gpui_kit::WindowAppearance;
use gpui_kit::component::theme::Theme;
use gpui_kit::component::theme::ThemeMode;
use gpui_kit::component::theme::ThemeTokens;
use gpui_kit::px;
use gpui_kit::rgb;

use wows_toolkit_viewmodel::settings::ThemeChoice;

/// egui's stock palettes, as the two front ends both render them.
///
/// Values are taken directly from egui 0.35's `Visuals::dark()` /
/// `Widgets::dark()` and their `light()` counterparts (`style.rs`):
///
/// |                                          | dark              | light             |
/// |------------------------------------------|-------------------|-------------------|
/// | `panel_fill` / `window_fill`             | `from_gray(27)`   | `from_gray(248)`  |
/// | `Visuals::text_color()`                  | `from_gray(140)`  | `from_gray(80)`   |
/// | `noninteractive.bg_stroke`               | `from_gray(60)`   | `from_gray(190)`  |
/// | `inactive.bg_fill` (button surface)      | `from_gray(60)`   | `from_gray(230)`  |
/// | `Selection::bg_fill`                     | `rgb(0, 92, 128)` | `rgb(144,209,255)`|
struct Palette {
    mode: ThemeMode,
    background: u32,
    foreground: u32,
    border: u32,
    secondary: u32,
    accent: u32,
}

/// `list_active`/`list_active_border` (the tree's selected-row background)
/// are pinned to the same surface gray as `secondary` rather than
/// gpui-component's default bright-blue `#1e40af` list-active token, matching
/// egui's `colorize_label` selection style (`ui/replay_parser/mod.rs`'s
/// white-on-`Color32::DARK_GRAY` selected label) -- a quiet highlight, not an
/// accent color.
const DARK: Palette = Palette {
    mode: ThemeMode::Dark,
    background: 0x1b1b1b,
    foreground: 0x8c8c8c,
    border: 0x3c3c3c,
    secondary: 0x3c3c3c,
    accent: 0x005c80,
};

const LIGHT: Palette = Palette {
    mode: ThemeMode::Light,
    background: 0xf8f8f8,
    foreground: 0x505050,
    border: 0xbebebe,
    secondary: 0xe6e6e6,
    accent: 0x90d1ff,
};

/// A light background must be lighter than the text on it, and the other way
/// round in dark. Checked at compile time, so a palette whose fields were
/// filled in from the wrong column does not build.
const _: () = {
    assert!(LIGHT.background > LIGHT.foreground);
    assert!(DARK.background < DARK.foreground);
};

/// Whether the desktop is currently set to a dark appearance.
///
/// GPUI reports vibrant variants separately; both are the same choice as far
/// as a palette is concerned.
pub fn system_is_dark(cx: &App) -> bool {
    matches!(cx.window_appearance(), WindowAppearance::Dark | WindowAppearance::VibrantDark)
}

/// Pin gpui-component's Theme to the egui palette `choice` selects, scaled by
/// `zoom`.
pub fn apply_egui_theme(choice: ThemeChoice, zoom: f32, window: &mut Window, cx: &mut App) {
    let dark = choice.is_dark(system_is_dark(cx));
    DARK_MODE.store(dark, std::sync::atomic::Ordering::Relaxed);
    let palette = if dark { &DARK } else { &LIGHT };
    Theme::change(palette.mode, Some(window), cx);

    let theme = Theme::global_mut(cx);
    theme.background = rgb(palette.background).into();
    theme.foreground = rgb(palette.foreground).into();
    theme.border = rgb(palette.border).into();
    theme.secondary = rgb(palette.secondary).into();
    theme.accent = rgb(palette.accent).into();
    theme.selection = rgb(palette.accent).into();
    theme.list_active = rgb(palette.secondary).into();
    theme.list_active_border = rgb(palette.secondary).into();
    theme.tokens = ThemeTokens::from(&theme.colors);

    // egui body text 12.5px, small rounding, scaled by zoom.
    theme.font_size = px(12.5 * zoom);
    theme.radius = px(2.0 * zoom);

    // rem-based helpers scale via rem size; px-valued fields were scaled above.
    window.set_rem_size(px(16.0 * zoom));
    cx.refresh_windows();
}

/// Whether what is on screen right now is the dark palette.
///
/// Read at draw time by the surfaces that pick a colour themselves rather
/// than taking one from the theme -- the personal-rating bands, whose two
/// palettes are the egui app's, not gpui-component's.
///
/// Backed by a process global rather than by `cx` because the functions that
/// need it resolve a colour from a value alone (`resolve_color`, the tracker's
/// cell colours) and are called from element builders that have no context to
/// hand. The theme it mirrors is itself global, and `apply_egui_theme` is the
/// only writer.
pub fn is_dark_mode() -> bool {
    DARK_MODE.load(std::sync::atomic::Ordering::Relaxed)
}

static DARK_MODE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

#[cfg(test)]
mod tests {
    use super::DARK;
    use super::LIGHT;
    use gpui_kit::component::theme::ThemeMode;

    /// The two palettes are the egui values, and no field is shared by
    /// accident: a light theme that kept a dark surface would be unreadable.
    #[test]
    fn the_palettes_are_egui_dark_and_egui_light() {
        assert_eq!(DARK.mode, ThemeMode::Dark);
        assert_eq!(LIGHT.mode, ThemeMode::Light);

        assert_eq!(DARK.background, 0x1b1b1b, "egui Visuals::dark panel_fill is from_gray(27)");
        assert_eq!(LIGHT.background, 0xf8f8f8, "egui Visuals::light panel_fill is from_gray(248)");
        assert_eq!(DARK.foreground, 0x8c8c8c, "egui dark text is from_gray(140)");
        assert_eq!(LIGHT.foreground, 0x505050, "egui light text is from_gray(80)");
        assert_eq!(DARK.border, 0x3c3c3c, "egui dark bg_stroke is from_gray(60)");
        assert_eq!(LIGHT.border, 0xbebebe, "egui light bg_stroke is from_gray(190)");
        assert_eq!(DARK.secondary, 0x3c3c3c, "egui dark inactive.bg_fill is from_gray(60)");
        assert_eq!(LIGHT.secondary, 0xe6e6e6, "egui light inactive.bg_fill is from_gray(230)");
        assert_eq!(DARK.accent, 0x005c80, "egui dark Selection::bg_fill");
        assert_eq!(LIGHT.accent, 0x90d1ff, "egui light Selection::bg_fill");
    }
}
