//! The icons the app draws, from `wows_toolkit_icons`.
//!
//! Each is an SVG served by [`AppAssets`] and drawn by `gpui_kit`'s `Icon`, so
//! it takes the size and colour of the text around it.

use std::borrow::Cow;

use gpui_kit::AssetSource;
use gpui_kit::Div;
use gpui_kit::ParentElement as _;
use gpui_kit::SharedString;
use gpui_kit::Styled as _;
use gpui_kit::component::Icon;
use gpui_kit::div;

use wows_toolkit_viewmodel::listing_row::LineGlyph;

/// Where the vendored icons are served from, apart from `gpui_kit`'s own. A
/// macro so `concat!` can build each icon's path from it.
macro_rules! prefix {
    () => {
        "app-icons/"
    };
}

const PREFIX: &str = prefix!();

/// Declares [`Glyph`], one variant per icon `wows_toolkit_icons` lists.
macro_rules! glyphs {
    ($($variant:ident => $name:literal,)*) => {
        /// One icon.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Glyph {
            $($variant,)*
        }

        impl Glyph {
            /// The asset path [`AppAssets`] serves the icon at.
            pub fn path(self) -> &'static str {
                match self {
                    $(Self::$variant => concat!(prefix!(), $name, ".svg"),)*
                }
            }

            fn svg(self) -> &'static [u8] {
                match self {
                    $(Self::$variant => wows_toolkit_icons::Glyph::$variant.svg().as_bytes(),)*
                }
            }

            fn from_path(path: &str) -> Option<Self> {
                let name = path.strip_prefix(PREFIX)?.strip_suffix(".svg")?;
                match name {
                    $($name => Some(Self::$variant),)*
                    _ => None,
                }
            }
        }
    };
}

wows_toolkit_icons::glyph_list!(glyphs);

impl From<LineGlyph> for Glyph {
    fn from(glyph: LineGlyph) -> Self {
        match glyph {
            LineGlyph::Clock => Self::Clock,
            LineGlyph::Crosshair => Self::Crosshair,
            LineGlyph::Sword => Self::Sword,
        }
    }
}

impl From<Glyph> for Icon {
    fn from(glyph: Glyph) -> Self {
        Icon::empty().path(SharedString::new_static(glyph.path()))
    }
}

/// One icon, sized to and coloured like the surrounding text.
///
/// A caller that wants it larger or in another colour styles the returned
/// element, as it would a line of text.
pub fn icon(glyph: Glyph) -> Div {
    div().flex_none().flex().items_center().child(Icon::from(glyph))
}

/// The vendored icons, then everything `gpui_kit` bundles.
pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> gpui_kit::Result<Option<Cow<'static, [u8]>>> {
        match Glyph::from_path(path) {
            Some(glyph) => Ok(Some(Cow::Borrowed(glyph.svg()))),
            None => gpui_kit::assets::Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> gpui_kit::Result<Vec<SharedString>> {
        gpui_kit::assets::Assets.list(path)
    }
}
