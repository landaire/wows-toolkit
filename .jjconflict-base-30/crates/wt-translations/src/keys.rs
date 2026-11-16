//! Const fn mappings from game enums to translation key strings.

use wows_core::game_types::AdvantageLevel;
use wows_core::game_types::BattleResult;
use wows_core::game_types::ExclusionReason;
use wows_core::game_types::FinishType;
use wows_core::game_types::UnattributedFireReason;

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
        ExclusionReason::MergedSectionVictimBuildUnknown => {
            "ui.replay.sections.fire_chance_exclusion_victim_build_unknown"
        }
        ExclusionReason::DamageControlActive => "ui.replay.sections.fire_chance_exclusion_damage_control_active",
        ExclusionReason::DamageControlUnknown => "ui.replay.sections.fire_chance_exclusion_damage_control_unknown",
        ExclusionReason::ObservationGap => "ui.replay.sections.fire_chance_exclusion_observation_gap",
        ExclusionReason::ConsumableModelUnreliable => "ui.replay.sections.fire_chance_exclusion_consumable_unreliable",
        ExclusionReason::VictimFateUnknown => "ui.replay.sections.fire_chance_exclusion_victim_fate_unknown",
        ExclusionReason::HitTypeDoesNotRoll => "ui.replay.sections.fire_chance_exclusion_hit_type_does_not_roll",
        ExclusionReason::NoSectionGeometry => "ui.replay.sections.fire_chance_exclusion_no_geometry",
        ExclusionReason::ImpactUnplaceableOnVictim => "ui.replay.sections.fire_chance_exclusion_impact_unplaceable",
        ExclusionReason::VictimPoseUnknown => "ui.replay.sections.fire_chance_exclusion_victim_pose_unknown",
        ExclusionReason::AmbiguousWithAnotherHit => "ui.replay.sections.fire_chance_exclusion_ambiguous_hit",
    }
}

/// Why a fire the game credited us with could not be tied to one of our shells.
/// Bare rows in a tally, so the count sits in its own column and the string is
/// the reason alone.
pub const fn unattributed_fire_reason_key(reason: UnattributedFireReason) -> &'static str {
    match reason {
        UnattributedFireReason::BurnStateNotObserved => "ui.replay.sections.fire_chance_unattributed_no_burn_state",
        UnattributedFireReason::AlreadyCreditedToAnEarlierFire => {
            "ui.replay.sections.fire_chance_unattributed_already_credited"
        }
        UnattributedFireReason::ContestedByOurSecondary => {
            "ui.replay.sections.fire_chance_unattributed_secondary_contest"
        }
        UnattributedFireReason::ContestedByAnotherHitOfOurs => {
            "ui.replay.sections.fire_chance_unattributed_hit_contest"
        }
        UnattributedFireReason::EveryNearbyHitExcluded => "ui.replay.sections.fire_chance_unattributed_all_excluded",
        UnattributedFireReason::NoNearbyHitCouldStartAFire => {
            "ui.replay.sections.fire_chance_unattributed_none_could_burn"
        }
        UnattributedFireReason::NoHitInWindow => "ui.replay.sections.fire_chance_unattributed_no_hit",
    }
}
