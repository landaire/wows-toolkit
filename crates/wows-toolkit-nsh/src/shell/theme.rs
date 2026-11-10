//! The named palettes as nsh design tokens.
//!
//! The palettes themselves are `wows_toolkit_viewmodel::theme`, shared with the
//! GPUI app; this maps their tiers onto shadcn's token names, and adds the
//! toolkit's own tones (`surface`, `win`, `warn`, ...) under names the TSX
//! reads with `token()`.

use nsh_core::Color;
use nsh_core::Hsla;
use nsh_core::element::AdaptiveColor;
use nsh_core::element::Tokens;
use nsh_core::platform::Appearance;
use wows_toolkit_viewmodel::settings::ThemeChoice;
use wows_toolkit_viewmodel::theme::DARK;
use wows_toolkit_viewmodel::theme::LIGHT;
use wows_toolkit_viewmodel::theme::Palette;
use wows_toolkit_viewmodel::theme::Semantic;
use wows_toolkit_viewmodel::theme::accent_foreground;
use wows_toolkit_viewmodel::theme::palette_for;
use wows_toolkit_viewmodel::theme::semantic_for;

/// The corner radius every palette draws with, in points: the small rounding
/// the other front ends use.
const RADIUS: f32 = 4.0;

/// Every token a palette sets, by name, as `0xRRGGBB`.
fn colors(palette: &Palette) -> [(&'static str, u32); 44] {
    let semantic: &Semantic = semantic_for(palette.dark);
    let on_accent = accent_foreground(palette.accent);

    [
        ("background", palette.panel),
        ("foreground", palette.text),
        ("card", palette.card),
        ("card-foreground", palette.text),
        ("popover", palette.card),
        ("popover-foreground", palette.text),
        ("primary", palette.accent),
        ("primary-foreground", on_accent),
        ("secondary", palette.widget),
        ("secondary-foreground", palette.text),
        ("muted", palette.faint),
        ("muted-foreground", palette.text_dim),
        ("accent", palette.widget_hot),
        ("accent-foreground", palette.text_bright),
        ("destructive", semantic.error),
        ("border", palette.border),
        ("input", palette.border),
        ("ring", palette.accent),
        ("sidebar", palette.surface),
        ("sidebar-foreground", palette.text),
        ("sidebar-border", palette.border),
        ("sidebar-accent", palette.selection),
        ("sidebar-accent-foreground", palette.text_bright),
        ("sidebar-primary", palette.accent),
        ("sidebar-primary-foreground", on_accent),
        ("surface", palette.surface),
        ("border-bright", palette.border_bright),
        ("selection", palette.selection),
        ("text-bright", palette.text_bright),
        ("text-faint", semantic.text_faint),
        ("win", semantic.win),
        ("loss", semantic.loss),
        ("draw", semantic.draw),
        ("warn", semantic.warn),
        ("error", semantic.error),
        ("ok", semantic.ok),
        ("notice", semantic.notice),
        ("text-strong", semantic.text_strong),
        ("text-dim", semantic.text_dim),
        ("division", semantic.division),
        ("abuser", semantic.abuser),
        ("chat-division", semantic.chat_division),
        ("chat-team", semantic.chat_team),
        ("chat-other", semantic.chat_other),
    ]
}

/// The palettes `choice` draws in light and in dark appearance: the desktop's
/// pair for `System`, and one palette for both otherwise.
fn palettes(choice: ThemeChoice) -> (&'static Palette, &'static Palette) {
    match choice {
        ThemeChoice::System => (&LIGHT, &DARK),
        // An explicit choice ignores the desktop, so either answer gives it.
        chosen => {
            let palette = palette_for(chosen, false);
            (palette, palette)
        }
    }
}

/// The appearance `choice` is shown in: the desktop's for `System`, its own
/// palette's otherwise, so `dark:` classes and the platform's controls agree
/// with the palette.
pub fn appearance(choice: ThemeChoice) -> Appearance {
    match choice {
        ThemeChoice::System => Appearance::System,
        chosen if palette_for(chosen, false).dark => Appearance::Dark,
        _ => Appearance::Light,
    }
}

/// The tokens `choice` draws with, over the default theme's (its chart and
/// font tokens, which no palette sets).
pub fn tokens(choice: ThemeChoice) -> Tokens {
    let (light, dark) = palettes(choice);
    let mut tokens = nsh_base::Theme::default().into_tokens();
    for ((name, light), (_, dark)) in colors(light).into_iter().zip(colors(dark)) {
        tokens.set_color(name, AdaptiveColor { light: hsla(light), dark: hsla(dark) });
    }
    tokens.set_number("radius", RADIUS);

    tokens
}

/// The window, panel, text and accent colors of `choice`, as `#rrggbb`, for its
/// preview tile.
pub fn swatches(choice: ThemeChoice, system_dark: bool) -> Vec<String> {
    wows_toolkit_viewmodel::theme::swatches(choice, system_dark).into_iter().map(hex).collect()
}

/// `choice` as a shadcn theme file, which is what `ui/theme.json` holds for
/// `System`: the first frame is drawn in it, before the saved theme is read.
#[cfg(test)]
pub fn theme_json(choice: ThemeChoice) -> String {
    let (light, dark) = palettes(choice);
    let section = |palette: &Palette| {
        colors(palette)
            .into_iter()
            .map(|(name, color)| (name.to_owned(), serde_json::Value::String(hex(color))))
            .collect::<serde_json::Map<_, _>>()
    };
    let file = serde_json::json!({
        "cssVars": {
            "theme": { "radius": format!("{}rem", RADIUS / 16.0) },
            "light": section(light),
            "dark": section(dark),
        }
    });

    let mut text = serde_json::to_string_pretty(&file).expect("a theme file serializes");
    text.push('\n');

    text
}

fn hsla(rgb: u32) -> Hsla {
    Hsla::from(Color(rgb << 8 | 0xff))
}

fn hex(rgb: u32) -> String {
    format!("#{rgb:06x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The theme file the bundle starts with is the `System` palette pair, so
    /// the window does not change color when the saved theme is applied. Run
    /// with `UPDATE_THEME_JSON=1` to rewrite it after a palette changes.
    #[test]
    fn the_startup_theme_file_is_the_system_palettes() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/theme.json");
        let expected = theme_json(ThemeChoice::System);
        if std::env::var_os("UPDATE_THEME_JSON").is_some() {
            std::fs::write(&path, &expected).expect("the theme file is written");
        }
        let found = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(found == expected, "{} is stale; run with UPDATE_THEME_JSON=1", path.display());
    }

    #[test]
    fn an_explicit_palette_draws_the_same_in_both_appearances() {
        let tokens = tokens(ThemeChoice::Dracula);
        let background = tokens.color("background").expect("the palette sets the background");
        assert_eq!(background.light, background.dark);
        assert_eq!(appearance(ThemeChoice::Dracula), Appearance::Dark);
        assert_eq!(appearance(ThemeChoice::SolarizedLight), Appearance::Light);
    }

    #[test]
    fn following_the_system_pairs_the_light_and_dark_palettes() {
        let tokens = tokens(ThemeChoice::System);
        let background = tokens.color("background").expect("the palette sets the background");
        assert_eq!(background.light, hsla(LIGHT.panel));
        assert_eq!(background.dark, hsla(DARK.panel));
    }
}
