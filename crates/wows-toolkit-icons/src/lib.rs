//! The icons the desktop front ends draw: Lucide's (ISC, `icons/LICENSE-LUCIDE`), and brand logos
//! from Simple Icons (CC0, `icons/LICENSE-SIMPLE-ICONS`), which Lucide does not draw.
//!
//! Every document paints in `currentColor`, so an icon takes the colour of the text around it.

/// Invokes `$callback!` with every icon as `Variant => "file-stem",`, so a front end can declare
/// a type of its own over the same list.
#[macro_export]
macro_rules! glyph_list {
    ($callback:ident) => {
        $callback! {
            AppWindow => "app-window",
            Archive => "archive",
            Bell => "bell",
            Bomb => "bomb",
            Box => "box",
            CaseSensitive => "case-sensitive",
            Castle => "castle",
            ChartColumn => "chart-column",
            ChartLine => "chart-line",
            Circle => "circle",
            CircleCheck => "circle-check",
            CircleX => "circle-x",
            Clipboard => "clipboard",
            ClipboardList => "clipboard-list",
            Clock => "clock",
            Copy => "copy",
            Crosshair => "crosshair",
            Database => "database",
            Download => "download",
            Equal => "equal",
            Eraser => "eraser",
            Eye => "eye",
            EyeOff => "eye-off",
            FaceSlightlyFrowning => "face-slightly-frowning",
            FastForward => "fast-forward",
            File => "file",
            FileText => "file-text",
            Flame => "flame",
            Folder => "folder",
            FolderOpen => "folder-open",
            Funnel => "funnel",
            HardDrive => "hard-drive",
            Image => "image",
            Info => "info",
            LayoutDashboard => "layout-dashboard",
            List => "list",
            Lock => "lock",
            Monitor => "monitor",
            MousePointer => "mouse-pointer",
            MoveUpRight => "move-up-right",
            Music => "music",
            NotebookPen => "notebook-pen",
            Paintbrush => "paintbrush",
            PanelBottom => "panel-bottom",
            PanelLeft => "panel-left",
            PanelRight => "panel-right",
            PanelRightClose => "panel-right-close",
            Pause => "pause",
            Pin => "pin",
            Play => "play",
            Plug => "plug",
            Radio => "radio",
            Rewind => "rewind",
            Rocket => "rocket",
            RotateCcw => "rotate-ccw",
            RotateCw => "rotate-cw",
            Ruler => "ruler",
            Search => "search",
            Settings => "settings",
            Shield => "shield",
            SkipBack => "skip-back",
            SkipForward => "skip-forward",
            Slash => "slash",
            Square => "square",
            Star => "star",
            Sword => "sword",
            SquareArrowOutUpRight => "square-arrow-out-up-right",
            Table => "table",
            Trash => "trash",
            Triangle => "triangle",
            TriangleAlert => "triangle-alert",
            Trophy => "trophy",
            Twitch => "twitch",
            Users => "users",
            VenetianMask => "venetian-mask",
            X => "x",
        }
    };
}

macro_rules! declare_glyphs {
    ($($variant:ident => $name:literal,)*) => {
        /// One icon.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Glyph {
            $($variant,)*
        }

        impl Glyph {
            pub const ALL: &[Self] = &[$(Self::$variant,)*];

            /// The icon's file stem, in kebab case.
            pub const fn name(self) -> &'static str {
                match self {
                    $(Self::$variant => $name,)*
                }
            }

            /// The SVG document.
            pub const fn svg(self) -> &'static str {
                match self {
                    $(Self::$variant => include_str!(concat!("../icons/", $name, ".svg")),)*
                }
            }

            pub fn from_name(name: &str) -> Option<Self> {
                match name {
                    $($name => Some(Self::$variant),)*
                    _ => None,
                }
            }
        }
    };
}

glyph_list!(declare_glyphs);

#[cfg(test)]
mod tests {
    use super::*;

    /// A document that names its own colour would not follow the text it sits in.
    #[test]
    fn every_icon_paints_in_the_text_colour() {
        for glyph in Glyph::ALL {
            assert!(glyph.svg().contains("currentColor"), "{} paints in a colour of its own", glyph.name());
        }
    }

    #[test]
    fn every_name_finds_its_icon() {
        for glyph in Glyph::ALL {
            assert_eq!(Glyph::from_name(glyph.name()), Some(*glyph));
        }
    }
}
