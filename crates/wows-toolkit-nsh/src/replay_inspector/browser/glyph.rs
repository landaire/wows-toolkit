//! The icons the listing draws for `wows_toolkit_viewmodel::glyphs`, which
//! the other front ends draw from an icon font.

use wows_toolkit_viewmodel::glyphs;

use crate::icons::Icon;

/// The icon drawn for one of `wows_toolkit_viewmodel::glyphs`.
pub(super) fn icon_of(glyph: &str) -> Option<Icon> {
    match glyph {
        glyphs::CLOCK => Some(Icon::Clock),
        glyphs::CROSSHAIR_SIMPLE => Some(Icon::Crosshair),
        glyphs::SWORD => Some(Icon::Sword),
        glyphs::USERS_THREE => Some(Icon::Users),
        _ => None,
    }
}
