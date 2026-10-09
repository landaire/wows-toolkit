use gpui_kit::App;
use gpui_kit::BoxShadow;
use gpui_kit::Hsla;
use gpui_kit::Window;
use gpui_kit::WindowAppearance;
use gpui_kit::component::theme::Theme;
use gpui_kit::component::theme::ThemeMode;
use gpui_kit::component::theme::ThemeTokens;
use gpui_kit::px;
use gpui_kit::rgb;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;

use wows_toolkit_viewmodel::settings::ThemeChoice;

/// Named color schemes applied to the desktop's shared surface tokens.
///
/// Each scheme keeps distinct surface tiers for window chrome, panels, cards,
/// and controls while using one accent for focus and selection.
struct Palette {
    mode: ThemeMode,
    /// Behind everything: the window itself and the tab strip.
    surface: u32,
    /// A panel of content.
    panel: u32,
    /// A card, popover or menu, one step above the panel.
    card: u32,
    /// A control at rest, and the same control under the pointer.
    widget: u32,
    widget_hot: u32,
    /// Every other row of a list or table.
    faint: u32,
    border: u32,
    /// A rule that has to be seen, such as one over the tab strip.
    border_bright: u32,
    /// What a selected row is filled with.
    selection: u32,
    accent: u32,
    text: u32,
    text_dim: u32,
    text_bright: u32,
}

const DARK: Palette = Palette {
    mode: ThemeMode::Dark,
    surface: 0x101010,
    panel: 0x181816,
    card: 0x1f1f1c,
    widget: 0x252522,
    widget_hot: 0x2f2f29,
    faint: 0x1c1c1a,
    border: 0x383733,
    border_bright: 0x524f4a,
    selection: 0x282820,
    accent: 0xc7c3b8,
    text: 0xc9c6be,
    text_dim: 0x979789,
    text_bright: 0xe8e5dc,
};
const LIGHT: Palette = Palette {
    mode: ThemeMode::Light,
    surface: 0xe6e5e0,
    panel: 0xf4f3ef,
    card: 0xffffff,
    widget: 0xeeede8,
    widget_hot: 0xdedcd3,
    faint: 0xf0efeb,
    border: 0xcbc9c1,
    border_bright: 0x9a978d,
    selection: 0xdad8cc,
    accent: 0x26251f,
    text: 0x1a1a17,
    text_dim: 0x5c5a53,
    text_bright: 0x0a0a08,
};
const DRACULA: Palette = Palette {
    mode: ThemeMode::Dark,
    surface: 0x21222c,
    panel: 0x282a36,
    card: 0x343746,
    widget: 0x3b3e50,
    widget_hot: 0x44475a,
    faint: 0x2c2e3b,
    border: 0x44475a,
    border_bright: 0x6272a4,
    selection: 0x343746,
    accent: 0xbd93f9,
    text: 0xf0eef8,
    text_dim: 0xb9b6ca,
    text_bright: 0xf8f8f2,
};
const NORD: Palette = Palette {
    mode: ThemeMode::Dark,
    surface: 0x242933,
    panel: 0x2e3440,
    card: 0x353d4b,
    widget: 0x3b4252,
    widget_hot: 0x434c5e,
    faint: 0x303744,
    border: 0x434c5e,
    border_bright: 0x5e81ac,
    selection: 0x3b4252,
    accent: 0x88c0d0,
    text: 0xd8dee9,
    text_dim: 0xaeb8c7,
    text_bright: 0xeceff4,
};
const MONOKAI: Palette = Palette {
    mode: ThemeMode::Dark,
    surface: 0x1b1d1e,
    panel: 0x272822,
    card: 0x30312a,
    widget: 0x393a32,
    widget_hot: 0x494a40,
    faint: 0x2b2c26,
    border: 0x494a40,
    border_bright: 0x75715e,
    selection: 0x393a32,
    accent: 0xa6e22e,
    text: 0xe4e4dc,
    text_dim: 0xa5a59a,
    text_bright: 0xf8f8f2,
};
const SOLARIZED_DARK: Palette = Palette {
    mode: ThemeMode::Dark,
    surface: 0x002b36,
    panel: 0x073642,
    card: 0x0b3d49,
    widget: 0x104450,
    widget_hot: 0x17505b,
    faint: 0x093944,
    border: 0x24545e,
    border_bright: 0x586e75,
    selection: 0x104450,
    accent: 0x268bd2,
    text: 0xb6c2bf,
    text_dim: 0x93a1a1,
    text_bright: 0xeee8d5,
};
const SOLARIZED_LIGHT: Palette = Palette {
    mode: ThemeMode::Light,
    surface: 0xfdf6e3,
    panel: 0xeee8d5,
    card: 0xfff9e8,
    widget: 0xe6dfca,
    widget_hot: 0xded6c0,
    faint: 0xf5eedb,
    border: 0xd4ccb8,
    border_bright: 0x93a1a1,
    selection: 0xe6dfca,
    accent: 0x268bd2,
    text: 0x40545c,
    text_dim: 0x50666d,
    text_bright: 0x586e75,
};
const GRUVBOX_DARK: Palette = Palette {
    mode: ThemeMode::Dark,
    surface: 0x1d2021,
    panel: 0x282828,
    card: 0x32302f,
    widget: 0x3c3836,
    widget_hot: 0x504945,
    faint: 0x2e2b29,
    border: 0x504945,
    border_bright: 0x665c54,
    selection: 0x3c3836,
    accent: 0xd79921,
    text: 0xd5c4a1,
    text_dim: 0xa89984,
    text_bright: 0xebdbb2,
};
const GRUVBOX_LIGHT: Palette = Palette {
    mode: ThemeMode::Light,
    surface: 0xfbf1c7,
    panel: 0xf2e5bc,
    card: 0xfff5d6,
    widget: 0xebddb2,
    widget_hot: 0xe2d2a3,
    faint: 0xf7ecc2,
    border: 0xd5c4a1,
    border_bright: 0xa89984,
    selection: 0xebddb2,
    accent: 0xaf3a03,
    text: 0x504945,
    text_dim: 0x665c54,
    text_bright: 0x3c3836,
};
const CATPPUCCIN: Palette = Palette {
    mode: ThemeMode::Dark,
    surface: 0x181825,
    panel: 0x1e1e2e,
    card: 0x252538,
    widget: 0x303047,
    widget_hot: 0x3b3b55,
    faint: 0x222235,
    border: 0x45455f,
    border_bright: 0x6c7086,
    selection: 0x303047,
    accent: 0xcba6f7,
    text: 0xbac2de,
    text_dim: 0x9399b2,
    text_bright: 0xcdd6f4,
};
const TOKYO_NIGHT: Palette = Palette {
    mode: ThemeMode::Dark,
    surface: 0x16161e,
    panel: 0x1a1b26,
    card: 0x222433,
    widget: 0x292e42,
    widget_hot: 0x343b58,
    faint: 0x1e2030,
    border: 0x3b4261,
    border_bright: 0x565f89,
    selection: 0x292e42,
    accent: 0x7aa2f7,
    text: 0xa9b1d6,
    text_dim: 0x7982a9,
    text_bright: 0xc0caf5,
};

static CURRENT_ACCENT: AtomicU32 = AtomicU32::new(DARK.accent);
static CURRENT_SURFACE: AtomicU32 = AtomicU32::new(DARK.surface);
static CURRENT_BORDER_BRIGHT: AtomicU32 = AtomicU32::new(DARK.border_bright);
static CURRENT_TEXT_DIM: AtomicU32 = AtomicU32::new(DARK.text_dim);

/// A light panel must be lighter than the text on it, and the other way round
/// in dark. Checked at compile time, so a palette whose fields were filled in
/// from the wrong column does not build.
const _: () = {
    assert!(LIGHT.panel > LIGHT.text);
    assert!(DARK.panel < DARK.text);
    assert!(DARK.surface < DARK.panel && DARK.panel < DARK.card && DARK.card < DARK.widget);
    assert!(LIGHT.surface < LIGHT.panel && LIGHT.panel < LIGHT.card);
};

/// What a colour means, rather than what it looks like
/// (`crates/wows-toolkit/src/ui/theme/semantic.rs`).
///
/// Each tone is held against the surfaces it is drawn on, and the two sets
/// are not each other's inverse: a light panel caps how light a tone may be,
/// so the light set is tuned apart along lightness instead.
pub struct Semantic {
    pub win: u32,
    pub loss: u32,
    pub draw: u32,
    pub warn: u32,
    pub error: u32,
    pub ok: u32,
    pub text_strong: u32,
    pub text_dim: u32,
    /// Below dim: grammar punctuation and hints that are there to be found,
    /// not read.
    pub text_faint: u32,
    /// Division mates.
    pub division: u32,
    /// Players flagged by the abuse list.
    pub abuser: u32,
    /// A value worth noticing that is not a warning.
    pub notice: u32,
    pub chat_division: u32,
    pub chat_team: u32,
    pub chat_other: u32,
    /// What a shell did to the armor it met. The same four the egui viewer
    /// paints its verdicts in (`ui/theme/semantic.rs`), so the two apps read a
    /// cast the same way.
    pub armor_pen: u32,
    pub armor_overpen: u32,
    pub armor_ricochet: u32,
    pub armor_shatter: u32,
    /// How square a strike was: head-on, angled, glancing.
    pub armor_angle_good: u32,
    pub armor_angle_mid: u32,
    pub armor_angle_bad: u32,
}

pub const DARK_SEMANTIC: Semantic = Semantic {
    win: 0x6fd98a,
    loss: 0xea7078,
    draw: 0xe9e5dd,
    warn: 0xe8a54a,
    error: 0xe8737b,
    ok: 0x6fd98a,
    text_strong: 0xe8e5dc,
    text_dim: 0x7c7c6e,
    text_faint: 0x5c5c52,
    division: 0xe5c158,
    abuser: 0xf09bc0,
    notice: 0xe5c158,
    chat_division: 0xe5c158,
    chat_team: 0x6fd98a,
    chat_other: 0xe8a54a,
    armor_pen: 0x6fd98a,
    armor_overpen: 0xe0be64,
    armor_ricochet: 0x7fb4e8,
    armor_shatter: 0xa9a49a,
    armor_angle_good: 0x64d98a,
    armor_angle_mid: 0xe0be64,
    armor_angle_bad: 0xe8737b,
};

pub const LIGHT_SEMANTIC: Semantic = Semantic {
    win: 0x0c5027,
    loss: 0x601118,
    draw: 0x5c5950,
    warn: 0x8a4b00,
    error: 0xae2230,
    ok: 0x106c34,
    text_strong: 0x0a0a08,
    text_dim: 0x78766f,
    text_faint: 0x9b9890,
    division: 0x775800,
    abuser: 0xa33270,
    notice: 0x775800,
    chat_division: 0x775800,
    chat_team: 0x106c34,
    chat_other: 0x8a4b00,
    armor_pen: 0x106c34,
    armor_overpen: 0x785808,
    armor_ricochet: 0x1b5fa8,
    armor_shatter: 0x5f5c52,
    armor_angle_good: 0x116b34,
    armor_angle_mid: 0x785808,
    armor_angle_bad: 0xae2230,
};

/// The semantic set for whatever is on screen now.
pub fn semantic() -> &'static Semantic {
    if is_dark_mode() { &DARK_SEMANTIC } else { &LIGHT_SEMANTIC }
}

/// Whether the desktop is currently set to a dark appearance.
///
/// GPUI reports vibrant variants separately; both are the same choice as far
/// as a palette is concerned.
pub fn system_is_dark(cx: &App) -> bool {
    matches!(cx.window_appearance(), WindowAppearance::Dark | WindowAppearance::VibrantDark)
}

/// Pin gpui-component's Theme to the palette `choice` selects, scaled by
/// `zoom`.
///
/// Every surface token is mapped by hand. `ThemeTokens::from` fills the rest
/// in from the colours set here, and anything left at a shadcn default would
/// read as a different app in the middle of this one.
pub fn apply_egui_theme(choice: ThemeChoice, zoom: f32, window: &mut Window, cx: &mut App) {
    let system_dark = system_is_dark(cx);
    let dark = choice.is_dark(system_dark);
    DARK_MODE.store(dark, std::sync::atomic::Ordering::Relaxed);
    let palette = palette_for(choice, system_dark);
    CURRENT_ACCENT.store(palette.accent, Ordering::Relaxed);
    CURRENT_SURFACE.store(palette.surface, Ordering::Relaxed);
    CURRENT_BORDER_BRIGHT.store(palette.border_bright, Ordering::Relaxed);
    CURRENT_TEXT_DIM.store(palette.text_dim, Ordering::Relaxed);
    let semantic = semantic_for(dark);
    Theme::change(palette.mode, Some(window), cx);

    let theme = Theme::global_mut(cx);
    theme.background = rgb(palette.panel).into();
    theme.foreground = rgb(palette.text).into();
    theme.border = rgb(palette.border).into();
    theme.secondary = rgb(palette.widget).into();
    theme.secondary_hover = rgb(palette.widget_hot).into();
    theme.secondary_active = rgb(palette.widget_hot).into();
    theme.secondary_foreground = rgb(palette.text).into();
    theme.button = rgb(palette.widget).into();
    theme.button_hover = rgb(palette.widget_hot).into();
    theme.button_active = rgb(palette.widget_hot).into();
    theme.button_foreground = rgb(palette.text).into();
    theme.button_secondary = theme.secondary;
    theme.button_secondary_hover = theme.secondary_hover;
    theme.button_secondary_active = theme.secondary_active;
    theme.button_secondary_foreground = theme.secondary_foreground;
    theme.input = rgb(palette.border).into();
    theme.ring = rgb(palette.accent).into();
    theme.caret = rgb(palette.accent).into();
    theme.muted = rgb(palette.faint).into();
    theme.muted_foreground = rgb(palette.text_dim).into();

    // A popover and a menu sit one tier above the panel.
    theme.popover = rgb(palette.card).into();
    theme.popover_foreground = rgb(palette.text).into();

    // The chrome behind the panels: window, tab strip and title bar.
    theme.sidebar = rgb(palette.surface).into();
    theme.sidebar_border = rgb(palette.border).into();
    theme.sidebar_foreground = rgb(palette.text).into();
    theme.sidebar_accent = rgb(palette.selection).into();
    theme.sidebar_accent_foreground = rgb(palette.text_bright).into();
    theme.sidebar_primary = rgb(palette.accent).into();
    theme.sidebar_primary_foreground = accent_foreground();
    theme.tab_bar = rgb(palette.surface).into();
    theme.tab = rgb(palette.surface).into();
    theme.tab_active = rgb(palette.panel).into();
    theme.tab_foreground = rgb(palette.text_dim).into();
    theme.tab_active_foreground = rgb(palette.text_bright).into();
    theme.tab_bar_segmented = rgb(palette.surface).into();
    theme.title_bar = rgb(palette.surface).into();
    theme.title_bar_border = rgb(palette.border).into();
    theme.status_bar = rgb(palette.surface).into();
    theme.status_bar_border = rgb(palette.border).into();
    theme.group_box = rgb(palette.card).into();
    theme.group_box_foreground = rgb(palette.text).into();

    // Selection and hover use separate tones so a keyboard cursor remains
    // distinct from the pointer.
    theme.selection = rgb(palette.selection).into();
    theme.accent = rgb(palette.widget_hot).into();
    theme.accent_foreground = rgb(palette.text_bright).into();
    theme.colors.list = rgb(palette.panel).into();
    theme.list_even = rgb(palette.faint).into();
    theme.list_head = rgb(palette.surface).into();
    theme.list_hover = rgb(palette.widget_hot).into();
    theme.list_active = rgb(palette.selection).into();
    theme.list_active_border = rgb(palette.accent).into();
    theme.table = rgb(palette.panel).into();
    theme.table_even = rgb(palette.faint).into();
    theme.table_head = rgb(palette.surface).into();
    theme.table_head_foreground = rgb(palette.text_dim).into();
    theme.table_foot = rgb(palette.surface).into();
    theme.table_foot_foreground = rgb(palette.text_dim).into();
    theme.table_hover = rgb(palette.widget_hot).into();
    theme.table_active = rgb(palette.selection).into();
    theme.table_active_border = rgb(palette.accent).into();
    theme.table_row_border = rgb(palette.border).into();

    theme.primary = rgb(palette.accent).into();
    theme.primary_foreground = accent_foreground();
    theme.primary_hover = accent().opacity(0.9);
    theme.primary_active = accent().opacity(0.8);
    theme.button_primary = theme.primary;
    theme.button_primary_hover = theme.primary_hover;
    theme.button_primary_active = theme.primary_active;
    theme.button_primary_foreground = theme.primary_foreground;
    theme.slider_bar = theme.primary;
    theme.slider_thumb = rgb(palette.text_bright).into();
    theme.progress_bar = theme.primary;
    theme.switch = rgb(palette.widget).into();
    theme.switch_thumb = rgb(palette.text_bright).into();
    theme.scrollbar = rgb(palette.surface).into();
    theme.scrollbar_thumb = rgb(palette.border_bright).into();
    theme.scrollbar_thumb_hover = rgb(palette.text_dim).into();
    theme.drag_border = theme.primary;
    theme.drop_target = rgb(palette.selection).into();

    theme.danger = rgb(semantic.error).into();
    theme.warning = rgb(semantic.warn).into();
    theme.success = rgb(semantic.ok).into();
    theme.info = rgb(semantic.notice).into();

    theme.tokens = ThemeTokens::from(&theme.colors);

    // egui body text 12.5px, small rounding, scaled by zoom.
    theme.font_size = px(12.5 * zoom);
    theme.radius = px(2.0 * zoom);

    // rem-based helpers scale via rem size; px-valued fields were scaled above.
    window.set_rem_size(px(16.0 * zoom));
    cx.refresh_windows();
}

fn semantic_for(dark: bool) -> &'static Semantic {
    if dark { &DARK_SEMANTIC } else { &LIGHT_SEMANTIC }
}

/// A rule that has to be seen: over the tab strip, or between two controls
/// that are otherwise flush.
pub fn border_bright() -> Hsla {
    rgb(CURRENT_BORDER_BRIGHT.load(Ordering::Relaxed)).into()
}

/// The surface behind the panels, for chrome that is not a panel itself.
pub fn surface() -> Hsla {
    rgb(CURRENT_SURFACE.load(Ordering::Relaxed)).into()
}

/// The tint for an affordance icon that is not status, such as a folder.
pub fn icon_accent() -> Hsla {
    rgb(CURRENT_ACCENT.load(Ordering::Relaxed)).into()
}

/// The active scheme's accent, used for selected modes and focus indicators.
pub fn accent() -> Hsla {
    icon_accent()
}

/// Selects the text tone with the greater contrast against the accent.
pub fn accent_foreground() -> Hsla {
    let color = CURRENT_ACCENT.load(Ordering::Relaxed);
    let linear = |shift: u32| {
        let channel = ((color >> shift) & 0xff_u32) as f32 / 255.;
        if channel <= 0.04045 { channel / 12.92 } else { ((channel + 0.055) / 1.055).powf(2.4) }
    };
    let luminance = 0.2126 * linear(16) + 0.7152 * linear(8) + 0.0722 * linear(0);
    rgb(if luminance > 0.179 { 0x000000 } else { 0xffffff }).into()
}

/// A stationary halo for active controls on dark surfaces.
pub fn phosphor_glow() -> BoxShadow {
    BoxShadow::new(px(0.), px(0.), accent().opacity(if is_dark_mode() { 0.18 } else { 0. })).blur_radius(px(8.))
}

fn palette_for(choice: ThemeChoice, system_dark: bool) -> &'static Palette {
    match choice {
        ThemeChoice::System => {
            if system_dark {
                &DARK
            } else {
                &LIGHT
            }
        }
        ThemeChoice::Dark => &DARK,
        ThemeChoice::Light => &LIGHT,
        ThemeChoice::Dracula => &DRACULA,
        ThemeChoice::Nord => &NORD,
        ThemeChoice::Monokai => &MONOKAI,
        ThemeChoice::SolarizedDark => &SOLARIZED_DARK,
        ThemeChoice::SolarizedLight => &SOLARIZED_LIGHT,
        ThemeChoice::GruvboxDark => &GRUVBOX_DARK,
        ThemeChoice::GruvboxLight => &GRUVBOX_LIGHT,
        ThemeChoice::CatppuccinMocha => &CATPPUCCIN,
        ThemeChoice::TokyoNight => &TOKYO_NIGHT,
    }
}

/// Window, panel, text, and accent colors for a theme preview.
pub fn swatches(choice: ThemeChoice, cx: &App) -> [u32; 4] {
    let palette = palette_for(choice, system_is_dark(cx));
    [palette.surface, palette.panel, palette.text, palette.accent]
}

/// De-emphasised detail text, held against the panel rather than faded out of
/// the body tone with opacity.
pub fn text_dim() -> Hsla {
    rgb(CURRENT_TEXT_DIM.load(Ordering::Relaxed)).into()
}

/// Below [`text_dim`]: grammar punctuation and hints.
pub fn text_faint() -> Hsla {
    rgb(semantic().text_faint).into()
}

/// Whether what is on screen right now is the dark palette.
///
/// Read at draw time by the surfaces that pick a colour themselves rather
/// than taking one from the theme -- the personal-rating bands and the
/// semantic tones above, which resolve a colour from a value alone
/// (`resolve_color`, the tracker's cell colours) in element builders that
/// have no context to hand. The theme it mirrors is itself global, and
/// `apply_egui_theme` is the only writer.
pub fn is_dark_mode() -> bool {
    DARK_MODE.load(std::sync::atomic::Ordering::Relaxed)
}

static DARK_MODE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

#[cfg(test)]
mod tests {
    use super::DARK;
    use super::DARK_SEMANTIC;
    use super::LIGHT;
    use super::LIGHT_SEMANTIC;

    /// Distinct surface tiers preserve panel and control boundaries.
    #[test]
    fn surface_tiers_are_distinct() {
        for palette in [&DARK, &LIGHT] {
            let tiers = [palette.surface, palette.panel, palette.card, palette.widget, palette.widget_hot];
            for (ix, tier) in tiers.iter().enumerate() {
                assert!(!tiers[..ix].contains(tier), "two surface tiers share a value: {tier:#08x}");
            }
        }
    }

    /// A battle result is read by its colour, so the three cannot collide,
    /// and neither set may be the other's copy.
    #[test]
    fn the_battle_results_are_told_apart_in_both_themes() {
        for semantic in [&DARK_SEMANTIC, &LIGHT_SEMANTIC] {
            assert_ne!(semantic.win, semantic.loss);
            assert_ne!(semantic.win, semantic.draw);
            assert_ne!(semantic.loss, semantic.draw);
        }
        assert_ne!(DARK_SEMANTIC.win, LIGHT_SEMANTIC.win);
        assert_ne!(DARK_SEMANTIC.text_dim, LIGHT_SEMANTIC.text_dim);
    }
}
