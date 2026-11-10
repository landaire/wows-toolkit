//! The player list's color roles in the palette on screen. The colors
//! themselves are `wows_toolkit_replay_data::palette`.

use wows_toolkit_replay_data::cells::PlayerColorKind;
use wows_toolkit_replay_data::palette;

/// Packed `0xRRGGBB` for a `PlayerColorKind` in the current theme: what
/// `table.rs::resolve_color`'s `ColorRole::Player` arm and the ship-class
/// icon tint in `icons.rs` resolve through.
pub(crate) fn player_color_kind_rgb(kind: PlayerColorKind) -> u32 {
    palette::player_color_kind_color(kind, crate::theme::semantic())
}
