//! Const fn mappings from game enums to translation key strings.

use wowsunpack::game_types::{AdvantageLevel, BattleResult, FinishType};

// -- Battle result keys --

pub const fn battle_result_key(result: BattleResult) -> &'static str {
    match result {
        BattleResult::Victory => "battle.victory",
        BattleResult::Defeat => "battle.defeat",
        BattleResult::Draw => "battle.draw",
    }
}

pub const PRE_BATTLE_KEY: &str = "battle.battle_starts_in";

// -- Finish type keys --

pub const fn finish_type_key(ft: FinishType) -> &'static str {
    match ft {
        FinishType::Unknown => "finish_type.unknown",
        FinishType::Extermination => "finish_type.extermination",
        FinishType::BaseCaptured => "finish_type.base_captured",
        FinishType::Timeout => "finish_type.timeout",
        FinishType::Failure => "finish_type.failure",
        FinishType::Technical => "finish_type.technical",
        FinishType::Score => "finish_type.score",
        FinishType::ScoreOnTimeout => "finish_type.score_on_timeout",
        FinishType::PveMainTaskSucceeded => "finish_type.pve_success",
        FinishType::PveMainTaskFailed => "finish_type.pve_failed",
        FinishType::ScoreZero => "finish_type.score_zero",
        FinishType::ScoreExcess => "finish_type.score_excess",
    }
}

// -- Advantage level keys --

pub const fn advantage_key(level: AdvantageLevel) -> &'static str {
    match level {
        AdvantageLevel::Absolute => "advantage.absolute",
        AdvantageLevel::Strong => "advantage.strong",
        AdvantageLevel::Moderate => "advantage.moderate",
        AdvantageLevel::Weak => "advantage.weak",
    }
}
