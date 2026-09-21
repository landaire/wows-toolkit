use gpui_kit::App;
use gpui_kit::Hsla;
use gpui_kit::Window;
use gpui_kit::WindowAppearance;
use gpui_kit::component::theme::Theme;
use gpui_kit::component::theme::ThemeMode;
use gpui_kit::component::theme::ThemeTokens;
use gpui_kit::px;
use gpui_kit::rgb;

use wows_toolkit_viewmodel::settings::ThemeChoice;

/// Graphite and Bone, the palette the egui app draws with
/// (`crates/wows-toolkit/src/ui/theme/palette.rs`).
///
/// Warm achromatic chrome in seven surface tiers, where the only accent is
/// bone: an engaged control inverts rather than taking a hue. The tiers are
/// what give a panel depth -- a card sits above the panel, a widget above the
/// card, a hovered widget above that -- so they are mapped onto
/// gpui-component's own tokens rather than left at its defaults.
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
    text_dim: 0x97_9789,
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
    /// Division mates, and the tint for affordance icons such as folders.
    pub division: u32,
    pub icon_accent: u32,
    /// Players flagged by the abuse list.
    pub abuser: u32,
    /// A value worth noticing that is not a warning.
    pub notice: u32,
    pub chat_division: u32,
    pub chat_team: u32,
    pub chat_other: u32,
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
    division: 0xe5c158,
    icon_accent: 0xe5c158,
    abuser: 0xf09bc0,
    notice: 0xe5c158,
    chat_division: 0xe5c158,
    chat_team: 0x6fd98a,
    chat_other: 0xe8a54a,
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
    division: 0x775800,
    icon_accent: 0x775800,
    abuser: 0xa33270,
    notice: 0x775800,
    chat_division: 0x775800,
    chat_team: 0x106c34,
    chat_other: 0x8a4b00,
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
    let dark = choice.is_dark(system_is_dark(cx));
    DARK_MODE.store(dark, std::sync::atomic::Ordering::Relaxed);
    let palette = if dark { &DARK } else { &LIGHT };
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
    theme.muted = rgb(palette.faint).into();
    theme.muted_foreground = rgb(palette.text_dim).into();

    // A popover and a menu sit one tier above the panel.
    theme.popover = rgb(palette.card).into();
    theme.popover_foreground = rgb(palette.text).into();

    // The chrome behind the panels: window, tab strip and title bar.
    theme.sidebar = rgb(palette.surface).into();
    theme.sidebar_border = rgb(palette.border).into();
    theme.sidebar_foreground = rgb(palette.text).into();
    theme.tab_bar = rgb(palette.surface).into();
    theme.tab = rgb(palette.surface).into();
    theme.tab_active = rgb(palette.panel).into();
    theme.tab_foreground = rgb(palette.text_dim).into();
    theme.tab_active_foreground = rgb(palette.text_bright).into();
    theme.title_bar = rgb(palette.surface).into();
    theme.title_bar_border = rgb(palette.border).into();

    // Selection is bone by inversion; hover is a raised neutral. Keeping the
    // two apart is what lets a keyboard cursor be followed through a list the
    // pointer is also in.
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
    theme.table_hover = rgb(palette.widget_hot).into();
    theme.table_active = rgb(palette.selection).into();
    theme.table_active_border = rgb(palette.accent).into();
    theme.table_row_border = rgb(palette.border).into();

    theme.primary = rgb(palette.accent).into();
    theme.primary_foreground = rgb(palette.panel).into();
    theme.primary_hover = rgb(palette.text_bright).into();
    theme.primary_active = rgb(palette.text).into();

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
    rgb(if is_dark_mode() { DARK.border_bright } else { LIGHT.border_bright }).into()
}

/// The surface behind the panels, for chrome that is not a panel itself.
pub fn surface() -> Hsla {
    rgb(if is_dark_mode() { DARK.surface } else { LIGHT.surface }).into()
}

/// The tint for an affordance icon that is not status, such as a folder.
pub fn icon_accent() -> Hsla {
    rgb(semantic().icon_accent).into()
}

/// De-emphasised detail text, held against the panel rather than faded out of
/// the body tone with opacity.
pub fn text_dim() -> Hsla {
    rgb(semantic().text_dim).into()
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
    use gpui_kit::component::theme::ThemeMode;

    /// The two palettes are Graphite and Bone, as the egui app defines it
    /// (`ui/theme/palette.rs`), and no tier is shared by accident: a theme
    /// whose card matched its panel would have no depth at all.
    #[test]
    fn the_palettes_are_graphite_and_bone() {
        assert_eq!(DARK.mode, ThemeMode::Dark);
        assert_eq!(LIGHT.mode, ThemeMode::Light);

        assert_eq!(DARK.surface, 0x101010);
        assert_eq!(DARK.panel, 0x181816);
        assert_eq!(DARK.card, 0x1f1f1c);
        assert_eq!(DARK.widget_hot, 0x2f2f29);
        assert_eq!(DARK.accent, 0xc7c3b8, "the only accent is bone");
        assert_eq!(LIGHT.surface, 0xe6e5e0);
        assert_eq!(LIGHT.panel, 0xf4f3ef);
        assert_eq!(LIGHT.card, 0xffffff);

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
