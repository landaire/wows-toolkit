//! The table's colors that have no theme token: each in its light and dark
//! form, from the semantic palettes the other front ends draw with.

use nsh_core::Color;
use nsh_core::prelude::*;
use wows_toolkit_replay_data::cells::BattleOutcome;
use wows_toolkit_replay_data::cells::CaptainPointsTier;
use wows_toolkit_replay_data::cells::ColorRole;
use wows_toolkit_replay_data::cells::PlayerColorKind;
use wows_toolkit_replay_data::palette::player_color_kind_color;
use wows_toolkit_viewmodel::personal_rating;
use wows_toolkit_viewmodel::theme::Semantic;
use wows_toolkit_viewmodel::theme::semantic_for;

/// A palette color, in light and dark appearance.
pub fn semantic(pick: fn(&Semantic) -> u32) -> AdaptiveColor {
    adaptive(pick(semantic_for(false)), pick(semantic_for(true)))
}

pub fn player(kind: PlayerColorKind) -> AdaptiveColor {
    adaptive(player_color_kind_color(kind, semantic_for(false)), player_color_kind_color(kind, semantic_for(true)))
}

/// The color a cell's role names.
pub fn role(role: ColorRole) -> AdaptiveColor {
    match role {
        ColorRole::Player(kind) => player(kind),
        // The band's text color, not its hue: the hue is a chip background
        // and reads poorly as text.
        ColorRole::PrTier(category) => {
            adaptive(personal_rating::chip_text(category, false), personal_rating::chip_text(category, true))
        }
        ColorRole::PrTierTint(category) => fixed(personal_rating::chip_hue(category)),
        ColorRole::CaptainPoints(tier) => match tier {
            CaptainPointsTier::Bad => semantic(|s| s.loss),
            CaptainPointsTier::Warning => semantic(|s| s.warn),
            CaptainPointsTier::Caution => semantic(|s| s.notice),
            CaptainPointsTier::Good => semantic(|s| s.win),
        },
        ColorRole::WinLoss(outcome) => match outcome {
            BattleOutcome::Win => semantic(|s| s.win),
            BattleOutcome::Loss => semantic(|s| s.loss),
            BattleOutcome::Draw => semantic(|s| s.draw),
        },
        ColorRole::Fixed(rgb) => fixed(rgb),
    }
}

/// A color read from game data, the same in both appearances.
pub fn fixed(rgb: u32) -> AdaptiveColor {
    adaptive(rgb, rgb)
}

fn adaptive(light: u32, dark: u32) -> AdaptiveColor {
    AdaptiveColor { light: hsla(light), dark: hsla(dark) }
}

fn hsla(rgb: u32) -> Hsla {
    Hsla::from(Color(rgb << 8 | 0xff))
}
