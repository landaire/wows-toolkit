//! The colours each theme choice paints with, as plain RGB, for every front end
//! to map onto its own toolkit.

use crate::settings::ThemeChoice;

/// Named color schemes applied to the desktop's shared surface tokens.
///
/// Each scheme keeps distinct surface tiers for window chrome, panels, cards,
/// and controls while using one accent for focus and selection.
pub struct Palette {
    pub dark: bool,
    /// Behind everything: the window itself and the tab strip.
    pub surface: u32,
    /// A panel of content.
    pub panel: u32,
    /// A card, popover or menu, one step above the panel.
    pub card: u32,
    /// A control at rest, and the same control under the pointer.
    pub widget: u32,
    pub widget_hot: u32,
    /// Every other row of a list or table.
    pub faint: u32,
    pub border: u32,
    /// A rule that has to be seen, such as one over the tab strip.
    pub border_bright: u32,
    /// What a selected row is filled with.
    pub selection: u32,
    pub accent: u32,
    pub text: u32,
    pub text_dim: u32,
    pub text_bright: u32,
}

pub const DARK: Palette = Palette {
    dark: true,
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
pub const LIGHT: Palette = Palette {
    dark: false,
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
pub const DRACULA: Palette = Palette {
    dark: true,
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
pub const NORD: Palette = Palette {
    dark: true,
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
pub const MONOKAI: Palette = Palette {
    dark: true,
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
pub const SOLARIZED_DARK: Palette = Palette {
    dark: true,
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
pub const SOLARIZED_LIGHT: Palette = Palette {
    dark: false,
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
pub const GRUVBOX_DARK: Palette = Palette {
    dark: true,
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
pub const GRUVBOX_LIGHT: Palette = Palette {
    dark: false,
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
pub const CATPPUCCIN: Palette = Palette {
    dark: true,
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
pub const TOKYO_NIGHT: Palette = Palette {
    dark: true,
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

pub fn semantic_for(dark: bool) -> &'static Semantic {
    if dark { &DARK_SEMANTIC } else { &LIGHT_SEMANTIC }
}

pub fn palette_for(choice: ThemeChoice, system_dark: bool) -> &'static Palette {
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
pub fn swatches(choice: ThemeChoice, system_dark: bool) -> [u32; 4] {
    let palette = palette_for(choice, system_dark);
    [palette.surface, palette.panel, palette.text, palette.accent]
}

/// Selects the text tone with the greater contrast against `accent`.
pub fn accent_foreground(accent: u32) -> u32 {
    let linear = |shift: u32| {
        let channel = ((accent >> shift) & 0xff_u32) as f32 / 255.;
        if channel <= 0.04045 { channel / 12.92 } else { ((channel + 0.055) / 1.055).powf(2.4) }
    };
    let luminance = 0.2126 * linear(16) + 0.7152 * linear(8) + 0.0722 * linear(0);
    if luminance > 0.179 { 0x000000 } else { 0xffffff }
}

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
