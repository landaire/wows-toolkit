//! Const fn mappings from game enums to translation key strings.

use wows_core::game_types::AdvantageLevel;
use wows_core::game_types::BattleResult;
use wows_core::game_types::ExclusionReason;
use wows_core::game_types::FinishType;

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

pub const fn exclusion_reason_key(reason: ExclusionReason) -> &'static str {
    match reason {
        ExclusionReason::SectionAlreadyBurning => "ui.replay.sections.fire_chance_exclusion_already_burning",
        ExclusionReason::SectionSuppressedByFirePrevention => {
            "ui.replay.sections.fire_chance_exclusion_fire_prevention"
        }
        ExclusionReason::SectionSuppressibleVictimBuildUnknown => {
            "ui.replay.sections.fire_chance_exclusion_victim_build_unknown"
        }
        ExclusionReason::DamageControlActive => "ui.replay.sections.fire_chance_exclusion_damage_control_active",
        ExclusionReason::DamageControlUnknown => "ui.replay.sections.fire_chance_exclusion_damage_control_unknown",
        ExclusionReason::ObservationGap => "ui.replay.sections.fire_chance_exclusion_observation_gap",
        ExclusionReason::ConsumableModelUnreliable => "ui.replay.sections.fire_chance_exclusion_consumable_unreliable",
        ExclusionReason::VictimDead => "ui.replay.sections.fire_chance_exclusion_victim_dead",
        ExclusionReason::VictimFateUnknown => "ui.replay.sections.fire_chance_exclusion_victim_fate_unknown",
        ExclusionReason::ShellCannotBurn => "ui.replay.sections.fire_chance_exclusion_shell_cannot_burn",
        ExclusionReason::NotMainBattery => "ui.replay.sections.fire_chance_exclusion_not_main_battery",
        ExclusionReason::HitTypeDoesNotRoll => "ui.replay.sections.fire_chance_exclusion_hit_type_does_not_roll",
        ExclusionReason::NoSectionGeometry => "ui.replay.sections.fire_chance_exclusion_no_geometry",
        ExclusionReason::ImpactNotOnAShip => "ui.replay.sections.fire_chance_exclusion_impact_not_on_a_ship",
        ExclusionReason::ImpactOffTheHull => "ui.replay.sections.fire_chance_exclusion_impact_off_the_hull",
        ExclusionReason::VictimPoseUnknown => "ui.replay.sections.fire_chance_exclusion_victim_pose_unknown",
        ExclusionReason::AmbiguousWithAnotherHit => "ui.replay.sections.fire_chance_exclusion_ambiguous_hit",
    }
}
