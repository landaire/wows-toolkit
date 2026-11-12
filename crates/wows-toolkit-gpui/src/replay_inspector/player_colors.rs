//! The palette tones a replay's players and chat senders are drawn in.

use wows_replays::types::Relation;
use wows_toolkit_replay_data::columns::PlayerColorKind;
use wows_toolkit_replay_data::model::ClanColor;

/// Packed `0xRRGGBB` color for the self/ally/enemy team-relation triad.
///
/// The tones are the app's own (`crate::theme::semantic`), so they follow the
/// palette on screen rather than being a second set of literals: self is
/// emphasised body text, an ally reads as a win and an enemy as a loss, which
/// is what the egui table colours them (`ui/replay_parser/models.rs`).
pub(crate) fn relation_color_rgb(relation: Relation) -> u32 {
    let semantic = crate::theme::semantic();
    if relation.is_self() {
        semantic.text_strong
    } else if relation.is_ally() {
        semantic.win
    } else {
        semantic.loss
    }
}

/// Packed `0xRRGGBB` color for a `PlayerColorKind` in the palette on screen.
/// `table.rs::resolve_color`'s `ColorRole::Player` arm resolves through it
/// for the name text and stat cells.
pub(crate) fn player_color_kind_rgb(kind: PlayerColorKind) -> u32 {
    player_tone(kind, crate::theme::semantic())
}

/// The tint a ship-class icon takes for `kind`, in either theme: the dark
/// theme's tones. The tint multiplies the icon's white body, so a dark tone
/// leaves the body the shade of its outline and the icon reads as a solid
/// shape; a bright one keeps the body and the outline apart.
pub(crate) fn class_icon_tint(kind: PlayerColorKind) -> u32 {
    player_tone(kind, &crate::theme::DARK_SEMANTIC)
}

/// The tone `kind` takes in `tones`: self is emphasised body text, an ally
/// reads as a win and an enemy as a loss, as `relation_color_rgb` colours
/// them.
fn player_tone(kind: PlayerColorKind, tones: &crate::theme::Semantic) -> u32 {
    match kind {
        PlayerColorKind::SelfPlayer => tones.text_strong,
        PlayerColorKind::Ally => tones.win,
        PlayerColorKind::Enemy => tones.loss,
        PlayerColorKind::DivisionMate => tones.division,
        PlayerColorKind::Abuser => tones.abuser,
    }
}

/// Packed `0xRRGGBB` color for a chat clan tag.
pub(crate) fn clan_color_rgb(color: ClanColor) -> u32 {
    match color {
        ClanColor::League(rgb) => rgb,
        ClanColor::Relation(relation) => relation_color_rgb(relation),
    }
}
