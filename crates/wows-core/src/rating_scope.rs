//! Which battles a performance rating applies to.
//!
//! Personal rating compares a battle against wows-numbers expected values,
//! which are built from Random battle statistics. Ranked plays to the same
//! rules and ships. Every other battle type changes the opponents, the ships
//! or the objective, so a rating against those values says nothing there.

use crate::game_types::BattleType;
use crate::recognized::Recognized;

/// Whether a battle is rated, and as which kind when it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RatingScope {
    Rated(RatedBattle),
    Unrated,
}

/// The battle kinds a rating applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RatedBattle {
    Random,
    Ranked,
}

impl RatingScope {
    /// Classifies a battle by its `gameType`, falling back to its
    /// `matchGroup` when `gameType` is absent, as it is in older replays. An
    /// unrecognized `gameType` is unrated until it is classified here.
    pub fn of_battle(battle_type: &Recognized<BattleType>, match_group: &str) -> Self {
        match battle_type {
            Recognized::Known(battle_type) => Self::of_battle_type(*battle_type),
            Recognized::Unknown(raw) if raw.is_empty() => Self::of_match_group(match_group),
            Recognized::Unknown(_) => Self::Unrated,
        }
    }

    pub const fn of_battle_type(battle_type: BattleType) -> Self {
        match battle_type {
            BattleType::Random => Self::Rated(RatedBattle::Random),
            BattleType::Ranked | BattleType::OldRanked => Self::Rated(RatedBattle::Ranked),
            BattleType::Standard
            | BattleType::Single
            | BattleType::Study
            | BattleType::Training
            | BattleType::Cooperative
            | BattleType::IntroMission
            | BattleType::Club
            | BattleType::Pve
            | BattleType::Clan
            | BattleType::Event
            | BattleType::Brawl => Self::Unrated,
        }
    }

    /// Classifies a battle by its `matchGroup` alone, for records that keep
    /// no `gameType`. A group this does not name is unrated: a rating is
    /// only shown where it is known to apply.
    pub fn of_match_group(match_group: &str) -> Self {
        match match_group {
            "pvp" => Self::Rated(RatedBattle::Random),
            "ranked" => Self::Rated(RatedBattle::Ranked),
            _ => Self::Unrated,
        }
    }

    pub const fn is_rated(self) -> bool {
        matches!(self, Self::Rated(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Version;

    #[test]
    fn every_battle_type_has_a_scope() {
        let scopes: Vec<(BattleType, RatingScope)> = [
            BattleType::Standard,
            BattleType::Single,
            BattleType::Study,
            BattleType::Random,
            BattleType::Training,
            BattleType::Cooperative,
            BattleType::Ranked,
            BattleType::OldRanked,
            BattleType::IntroMission,
            BattleType::Club,
            BattleType::Pve,
            BattleType::Clan,
            BattleType::Event,
            BattleType::Brawl,
        ]
        .into_iter()
        .map(|battle_type| (battle_type, RatingScope::of_battle_type(battle_type)))
        .collect();

        assert_eq!(
            scopes,
            vec![
                (BattleType::Standard, RatingScope::Unrated),
                (BattleType::Single, RatingScope::Unrated),
                (BattleType::Study, RatingScope::Unrated),
                (BattleType::Random, RatingScope::Rated(RatedBattle::Random)),
                (BattleType::Training, RatingScope::Unrated),
                (BattleType::Cooperative, RatingScope::Unrated),
                (BattleType::Ranked, RatingScope::Rated(RatedBattle::Ranked)),
                (BattleType::OldRanked, RatingScope::Rated(RatedBattle::Ranked)),
                (BattleType::IntroMission, RatingScope::Unrated),
                (BattleType::Club, RatingScope::Unrated),
                (BattleType::Pve, RatingScope::Unrated),
                (BattleType::Clan, RatingScope::Unrated),
                (BattleType::Event, RatingScope::Unrated),
                (BattleType::Brawl, RatingScope::Unrated),
            ]
        );
    }

    #[test]
    fn match_groups_classify_old_replays() {
        assert_eq!(RatingScope::of_match_group("pvp"), RatingScope::Rated(RatedBattle::Random));
        assert_eq!(RatingScope::of_match_group("ranked"), RatingScope::Rated(RatedBattle::Ranked));
        for group in ["cooperative", "pve", "clan", "brawl", "event", "training", ""] {
            assert_eq!(RatingScope::of_match_group(group), RatingScope::Unrated, "{group}");
        }
    }

    #[test]
    fn a_known_battle_type_outranks_the_match_group() {
        let coop = BattleType::from_value("CooperativeBattle", Version::default());
        assert_eq!(RatingScope::of_battle(&coop, "pvp"), RatingScope::Unrated);

        let ranked = BattleType::from_value("RankedBattle", Version::default());
        assert_eq!(RatingScope::of_battle(&ranked, "ranked"), RatingScope::Rated(RatedBattle::Ranked));
    }

    #[test]
    fn a_missing_battle_type_falls_back_to_the_match_group() {
        let absent = BattleType::from_value("", Version::default());
        assert_eq!(RatingScope::of_battle(&absent, "pvp"), RatingScope::Rated(RatedBattle::Random));
        assert_eq!(RatingScope::of_battle(&absent, "ranked"), RatingScope::Rated(RatedBattle::Ranked));
        assert_eq!(RatingScope::of_battle(&absent, "pve"), RatingScope::Unrated);
        assert_eq!(RatingScope::of_battle(&absent, ""), RatingScope::Unrated);
    }

    #[test]
    fn an_unrecognized_battle_type_is_unrated_whatever_the_match_group() {
        let future = BattleType::from_value("FutureBattle", Version::default());
        assert_eq!(RatingScope::of_battle(&future, "pvp"), RatingScope::Unrated);
        assert_eq!(RatingScope::of_battle(&future, "ranked"), RatingScope::Unrated);
    }
}
