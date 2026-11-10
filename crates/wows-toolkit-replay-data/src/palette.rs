//! The colors the player list and chat give a player, from a semantic
//! palette. Each front end passes the palette for the appearance on screen.

use wows_replays::types::Relation;
use wows_toolkit_viewmodel::theme::Semantic;

use crate::cells::PlayerColorKind;

/// Packed `0xRRGGBB` for the self/ally/enemy relation: self is emphasised
/// body text, an ally reads as a win and an enemy as a loss, which is what
/// the egui table colours them (`ui/replay_parser/models.rs`).
pub fn relation_color(relation: Relation, semantic: &Semantic) -> u32 {
    if relation.is_self() {
        semantic.text_strong
    } else if relation.is_ally() {
        semantic.win
    } else {
        semantic.loss
    }
}

/// Packed `0xRRGGBB` for a `PlayerColorKind`: the name text, stat cells and
/// ship-class icon tint of a row.
pub fn player_color_kind_color(kind: PlayerColorKind, semantic: &Semantic) -> u32 {
    match kind {
        PlayerColorKind::SelfPlayer => relation_color(Relation::new(0), semantic),
        PlayerColorKind::Ally => relation_color(Relation::new(1), semantic),
        PlayerColorKind::Enemy => relation_color(Relation::new(2), semantic),
        PlayerColorKind::DivisionMate => semantic.division,
        PlayerColorKind::Abuser => semantic.abuser,
    }
}
