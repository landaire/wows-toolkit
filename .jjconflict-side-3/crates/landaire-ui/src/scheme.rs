use zgui_ui_tokens::ColorScheme;

use crate::color::Rgb;
use crate::palette::Appearance;
use crate::palette::Palette;

/// A named palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Scheme {
    Dark,
    Light,
    Dracula,
    Nord,
    Monokai,
    SolarizedDark,
    SolarizedLight,
    GruvboxDark,
    GruvboxLight,
    CatppuccinMocha,
    TokyoNight,
}

impl Scheme {
    pub const ALL: [Self; 11] = [
        Self::Dark,
        Self::Light,
        Self::Dracula,
        Self::Nord,
        Self::Monokai,
        Self::SolarizedDark,
        Self::SolarizedLight,
        Self::GruvboxDark,
        Self::GruvboxLight,
        Self::CatppuccinMocha,
        Self::TokyoNight,
    ];

    pub const fn palette(self) -> &'static Palette {
        match self {
            Self::Dark => &DARK,
            Self::Light => &LIGHT,
            Self::Dracula => &DRACULA,
            Self::Nord => &NORD,
            Self::Monokai => &MONOKAI,
            Self::SolarizedDark => &SOLARIZED_DARK,
            Self::SolarizedLight => &SOLARIZED_LIGHT,
            Self::GruvboxDark => &GRUVBOX_DARK,
            Self::GruvboxLight => &GRUVBOX_LIGHT,
            Self::CatppuccinMocha => &CATPPUCCIN_MOCHA,
            Self::TokyoNight => &TOKYO_NIGHT,
        }
    }

    /// The name shown to a reader, in English.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Dark => "Dark",
            Self::Light => "Light",
            Self::Dracula => "Dracula",
            Self::Nord => "Nord",
            Self::Monokai => "Monokai",
            Self::SolarizedDark => "Solarized Dark",
            Self::SolarizedLight => "Solarized Light",
            Self::GruvboxDark => "Gruvbox Dark",
            Self::GruvboxLight => "Gruvbox Light",
            Self::CatppuccinMocha => "Catppuccin Mocha",
            Self::TokyoNight => "Tokyo Night",
        }
    }
}

/// The palette an application asks for: a fixed scheme, or light and dark following the desktop.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ThemeChoice {
    /// [`Scheme::Light`] or [`Scheme::Dark`], whichever the desktop prefers. The style engine
    /// resolves this through `prefers-color-scheme`, so it follows the desktop while running.
    System,
    Fixed(Scheme),
}

impl ThemeChoice {
    pub const ALL: [Self; 12] = [
        Self::System,
        Self::Fixed(Scheme::Dark),
        Self::Fixed(Scheme::Light),
        Self::Fixed(Scheme::Dracula),
        Self::Fixed(Scheme::Nord),
        Self::Fixed(Scheme::Monokai),
        Self::Fixed(Scheme::SolarizedDark),
        Self::Fixed(Scheme::SolarizedLight),
        Self::Fixed(Scheme::GruvboxDark),
        Self::Fixed(Scheme::GruvboxLight),
        Self::Fixed(Scheme::CatppuccinMocha),
        Self::Fixed(Scheme::TokyoNight),
    ];

    /// The palette in force when the desktop prefers `desktop`.
    pub const fn palette(self, desktop: Appearance) -> &'static Palette {
        match (self, desktop) {
            (Self::System, Appearance::Light) => &LIGHT,
            (Self::System, Appearance::Dark) => &DARK,
            (Self::Fixed(scheme), _) => scheme.palette(),
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Fixed(scheme) => scheme.label(),
        }
    }

    /// The palettes for the two theme slots, and which slot is in force.
    pub(crate) const fn slots(self) -> Slots {
        match self {
            Self::System => Slots { light: &LIGHT, dark: &DARK, scheme: ColorScheme::System },
            Self::Fixed(scheme) => {
                let palette = scheme.palette();
                let scheme = match palette.appearance {
                    Appearance::Light => ColorScheme::Light,
                    Appearance::Dark => ColorScheme::Dark,
                };
                Slots { light: palette, dark: palette, scheme }
            }
        }
    }
}

/// A theme choice resolved into the light and dark slots of a theme sheet.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Slots {
    pub(crate) light: &'static Palette,
    pub(crate) dark: &'static Palette,
    pub(crate) scheme: ColorScheme,
}

const DARK: Palette = Palette {
    appearance: Appearance::Dark,
    surface: Rgb::new(0x101010),
    panel: Rgb::new(0x181816),
    card: Rgb::new(0x1f1f1c),
    widget: Rgb::new(0x252522),
    widget_hot: Rgb::new(0x2f2f29),
    faint: Rgb::new(0x1c1c1a),
    border: Rgb::new(0x383733),
    border_bright: Rgb::new(0x524f4a),
    selection: Rgb::new(0x282820),
    accent: Rgb::new(0xc7c3b8),
    text: Rgb::new(0xc9c6be),
    text_dim: Rgb::new(0x979789),
    text_bright: Rgb::new(0xe8e5dc),
};

const LIGHT: Palette = Palette {
    appearance: Appearance::Light,
    surface: Rgb::new(0xe6e5e0),
    panel: Rgb::new(0xf4f3ef),
    card: Rgb::new(0xffffff),
    widget: Rgb::new(0xeeede8),
    widget_hot: Rgb::new(0xdedcd3),
    faint: Rgb::new(0xf0efeb),
    border: Rgb::new(0xcbc9c1),
    border_bright: Rgb::new(0x9a978d),
    selection: Rgb::new(0xdad8cc),
    accent: Rgb::new(0x26251f),
    text: Rgb::new(0x1a1a17),
    text_dim: Rgb::new(0x5c5a53),
    text_bright: Rgb::new(0x0a0a08),
};

const DRACULA: Palette = Palette {
    appearance: Appearance::Dark,
    surface: Rgb::new(0x21222c),
    panel: Rgb::new(0x282a36),
    card: Rgb::new(0x343746),
    widget: Rgb::new(0x3b3e50),
    widget_hot: Rgb::new(0x44475a),
    faint: Rgb::new(0x2c2e3b),
    border: Rgb::new(0x44475a),
    border_bright: Rgb::new(0x6272a4),
    selection: Rgb::new(0x343746),
    accent: Rgb::new(0xbd93f9),
    text: Rgb::new(0xf0eef8),
    text_dim: Rgb::new(0xb9b6ca),
    text_bright: Rgb::new(0xf8f8f2),
};

const NORD: Palette = Palette {
    appearance: Appearance::Dark,
    surface: Rgb::new(0x242933),
    panel: Rgb::new(0x2e3440),
    card: Rgb::new(0x353d4b),
    widget: Rgb::new(0x3b4252),
    widget_hot: Rgb::new(0x434c5e),
    faint: Rgb::new(0x303744),
    border: Rgb::new(0x434c5e),
    border_bright: Rgb::new(0x5e81ac),
    selection: Rgb::new(0x3b4252),
    accent: Rgb::new(0x88c0d0),
    text: Rgb::new(0xd8dee9),
    text_dim: Rgb::new(0xaeb8c7),
    text_bright: Rgb::new(0xeceff4),
};

const MONOKAI: Palette = Palette {
    appearance: Appearance::Dark,
    surface: Rgb::new(0x1b1d1e),
    panel: Rgb::new(0x272822),
    card: Rgb::new(0x30312a),
    widget: Rgb::new(0x393a32),
    widget_hot: Rgb::new(0x494a40),
    faint: Rgb::new(0x2b2c26),
    border: Rgb::new(0x494a40),
    border_bright: Rgb::new(0x75715e),
    selection: Rgb::new(0x393a32),
    accent: Rgb::new(0xa6e22e),
    text: Rgb::new(0xe4e4dc),
    text_dim: Rgb::new(0xa5a59a),
    text_bright: Rgb::new(0xf8f8f2),
};

const SOLARIZED_DARK: Palette = Palette {
    appearance: Appearance::Dark,
    surface: Rgb::new(0x002b36),
    panel: Rgb::new(0x073642),
    card: Rgb::new(0x0b3d49),
    widget: Rgb::new(0x104450),
    widget_hot: Rgb::new(0x17505b),
    faint: Rgb::new(0x093944),
    border: Rgb::new(0x24545e),
    border_bright: Rgb::new(0x586e75),
    selection: Rgb::new(0x104450),
    accent: Rgb::new(0x268bd2),
    text: Rgb::new(0xb6c2bf),
    text_dim: Rgb::new(0x93a1a1),
    text_bright: Rgb::new(0xeee8d5),
};

const SOLARIZED_LIGHT: Palette = Palette {
    appearance: Appearance::Light,
    surface: Rgb::new(0xfdf6e3),
    panel: Rgb::new(0xeee8d5),
    card: Rgb::new(0xfff9e8),
    widget: Rgb::new(0xe6dfca),
    widget_hot: Rgb::new(0xded6c0),
    faint: Rgb::new(0xf5eedb),
    border: Rgb::new(0xd4ccb8),
    border_bright: Rgb::new(0x93a1a1),
    selection: Rgb::new(0xe6dfca),
    accent: Rgb::new(0x268bd2),
    text: Rgb::new(0x40545c),
    text_dim: Rgb::new(0x50666d),
    text_bright: Rgb::new(0x586e75),
};

const GRUVBOX_DARK: Palette = Palette {
    appearance: Appearance::Dark,
    surface: Rgb::new(0x1d2021),
    panel: Rgb::new(0x282828),
    card: Rgb::new(0x32302f),
    widget: Rgb::new(0x3c3836),
    widget_hot: Rgb::new(0x504945),
    faint: Rgb::new(0x2e2b29),
    border: Rgb::new(0x504945),
    border_bright: Rgb::new(0x665c54),
    selection: Rgb::new(0x3c3836),
    accent: Rgb::new(0xd79921),
    text: Rgb::new(0xd5c4a1),
    text_dim: Rgb::new(0xa89984),
    text_bright: Rgb::new(0xebdbb2),
};

const GRUVBOX_LIGHT: Palette = Palette {
    appearance: Appearance::Light,
    surface: Rgb::new(0xfbf1c7),
    panel: Rgb::new(0xf2e5bc),
    card: Rgb::new(0xfff5d6),
    widget: Rgb::new(0xebddb2),
    widget_hot: Rgb::new(0xe2d2a3),
    faint: Rgb::new(0xf7ecc2),
    border: Rgb::new(0xd5c4a1),
    border_bright: Rgb::new(0xa89984),
    selection: Rgb::new(0xebddb2),
    accent: Rgb::new(0xaf3a03),
    text: Rgb::new(0x504945),
    text_dim: Rgb::new(0x665c54),
    text_bright: Rgb::new(0x3c3836),
};

const CATPPUCCIN_MOCHA: Palette = Palette {
    appearance: Appearance::Dark,
    surface: Rgb::new(0x181825),
    panel: Rgb::new(0x1e1e2e),
    card: Rgb::new(0x252538),
    widget: Rgb::new(0x303047),
    widget_hot: Rgb::new(0x3b3b55),
    faint: Rgb::new(0x222235),
    border: Rgb::new(0x45455f),
    border_bright: Rgb::new(0x6c7086),
    selection: Rgb::new(0x303047),
    accent: Rgb::new(0xcba6f7),
    text: Rgb::new(0xbac2de),
    text_dim: Rgb::new(0x9399b2),
    text_bright: Rgb::new(0xcdd6f4),
};

const TOKYO_NIGHT: Palette = Palette {
    appearance: Appearance::Dark,
    surface: Rgb::new(0x16161e),
    panel: Rgb::new(0x1a1b26),
    card: Rgb::new(0x222433),
    widget: Rgb::new(0x292e42),
    widget_hot: Rgb::new(0x343b58),
    faint: Rgb::new(0x1e2030),
    border: Rgb::new(0x3b4261),
    border_bright: Rgb::new(0x565f89),
    selection: Rgb::new(0x292e42),
    accent: Rgb::new(0x7aa2f7),
    text: Rgb::new(0xa9b1d6),
    text_dim: Rgb::new(0x7982a9),
    text_bright: Rgb::new(0xc0caf5),
};
