//! What was fired at one ship, grouped by the salvo it came from.
//!
//! The armor viewers draw the hits; this is the log beside them, which says who
//! fired, when, and what each shell did. Both front ends read the same grouping,
//! so the same replay reads the same way in either.

use std::collections::BTreeMap;
use std::collections::HashSet;

use wows_replay_insights::timeline::PreExtractedHit;
use wows_replays::analyzer::decoder::HitType;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wows_replays::types::GameParamId;
use wows_replays::types::ShotId;
use wowsunpack::game_types::ShellHitType;
use wowsunpack::recognized::Recognized;

/// Which shells the log counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IncomingFilter {
    /// One attacker's shells only. `None` counts every enemy's.
    pub attacker: Option<EntityId>,
    /// Whether secondary armament is counted. The main batteries alone is the
    /// question most of the time, and a battleship's secondaries otherwise bury
    /// the salvo that mattered.
    pub secondaries: bool,
}

/// What one salvo is known by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SalvoKey {
    pub owner: EntityId,
    pub salvo_id: u32,
}

/// One shell of a salvo, as the log lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct IncomingShell {
    pub shot_id: ShotId,
    /// When it landed.
    pub clock: GameClock,
    /// What the server said it did.
    pub hit_type: HitType,
}

/// One salvo's worth of shells that reached the ship.
#[derive(Clone, Debug, PartialEq)]
pub struct IncomingSalvo {
    pub key: SalvoKey,
    /// Who fired it.
    pub attacker: EntityId,
    /// The shell that was fired, for naming it.
    pub shell: GameParamId,
    /// When its first shell landed, which is the order the log is read in.
    pub first_clock: GameClock,
    /// When its last one did.
    pub latest_clock: GameClock,
    pub shells: Vec<IncomingShell>,
}

/// Server-authoritative shell outcome (mapped from ShellHitType).
#[derive(Clone, Debug, PartialEq)]
pub enum ServerOutcome {
    Penetration,
    Citadel,
    Ricochet,
    Shatter,
    Overpenetration,
    Underwater,
    Unknown(String),
}

impl ServerOutcome {
    pub fn from_shell_hit_type(hit: &Recognized<ShellHitType>) -> Self {
        match hit {
            Recognized::Known(ShellHitType::Normal) => Self::Penetration,
            Recognized::Known(ShellHitType::MajorHit) => Self::Citadel,
            Recognized::Known(ShellHitType::Ricochet) => Self::Ricochet,
            Recognized::Known(ShellHitType::NoPenetration) => Self::Shatter,
            Recognized::Known(ShellHitType::Overpenetration) => Self::Overpenetration,
            Recognized::Known(ShellHitType::ExitOverpenetration) => Self::Overpenetration,
            Recognized::Known(ShellHitType::Underwater) => Self::Underwater,
            Recognized::Known(ShellHitType::None) => Self::Unknown("None".into()),
            Recognized::Unknown(s) => Self::Unknown(s.clone()),
        }
    }

    /// The key naming this outcome, for a front end that translates it.
    pub const fn label_key(&self) -> &'static str {
        match self {
            Self::Penetration => "ui.armor.outcome.penetration",
            Self::Citadel => "ui.armor.outcome.citadel",
            Self::Ricochet => "ui.armor.outcome.ricochet",
            Self::Shatter => "ui.armor.outcome.shatter",
            Self::Overpenetration => "ui.armor.outcome.overpenetration",
            Self::Underwater => "ui.armor.outcome.underwater",
            Self::Unknown(_) => "ui.armor.outcome.unknown",
        }
    }

    pub fn display_name(&self) -> &str {
        match self {
            Self::Penetration => "Penetration",
            Self::Citadel => "Citadel",
            Self::Ricochet => "Ricochet",
            Self::Shatter => "Shatter",
            Self::Overpenetration => "Overpenetration",
            Self::Underwater => "Underwater",
            Self::Unknown(s) => s.as_str(),
        }
    }
}

/// Groups the hits a ship has taken into the salvos they were fired in.
///
/// `enemies` is who counts as incoming fire with no attacker chosen: a shell
/// from the ship's own team is not. `main_battery` is which shells are main
/// battery ones; an empty set is not knowledge that none are, so it counts
/// every shell rather than hiding the lot.
pub fn group_incoming(
    hits: &[PreExtractedHit],
    filter: &IncomingFilter,
    enemies: &HashSet<EntityId>,
    main_battery: &HashSet<GameParamId>,
) -> Vec<IncomingSalvo> {
    let mut groups: BTreeMap<SalvoKey, IncomingSalvo> = BTreeMap::new();

    for taken in hits {
        let hit = &taken.hit;
        if !counted(hit.hit.owner_id, filter, enemies) {
            continue;
        }
        // Only gunfire: a hit the parser could not match back to a salvo is a
        // torpedo, a bomb or a rocket, which this log has nothing to say about
        // and which the egui panel drops for the same reason.
        let Some(salvo) = hit.salvo.as_ref() else { continue };
        if !filter.secondaries && !main_battery.is_empty() && !main_battery.contains(&salvo.params_id) {
            continue;
        }

        let key = SalvoKey { owner: salvo.owner_id, salvo_id: salvo.salvo_id };
        let shell_entry =
            IncomingShell { shot_id: hit.hit.shot_id, clock: taken.clock, hit_type: hit.hit.hit_type.clone() };

        let group = groups.entry(key).or_insert_with(|| IncomingSalvo {
            key,
            attacker: salvo.owner_id,
            shell: salvo.params_id,
            first_clock: taken.clock,
            latest_clock: taken.clock,
            shells: Vec::new(),
        });
        group.first_clock = group.first_clock.min(taken.clock);
        group.latest_clock = group.latest_clock.max(taken.clock);
        group.shells.push(shell_entry);
    }

    let mut ordered: Vec<IncomingSalvo> = groups.into_values().collect();
    ordered.sort_by_key(|salvo| (salvo.first_clock, salvo.key));
    ordered
}

/// Whether a shell from `owner` is incoming fire under `filter`.
fn counted(owner: EntityId, filter: &IncomingFilter, enemies: &HashSet<EntityId>) -> bool {
    match filter.attacker {
        Some(attacker) => owner == attacker,
        None => enemies.contains(&owner),
    }
}

/// Every attacker the log holds shells from, in the order they first appear.
///
/// What the attacker filter offers: a ship that never hit this one is not worth
/// filtering to.
pub fn attackers(salvos: &[IncomingSalvo]) -> Vec<EntityId> {
    let mut seen = HashSet::new();
    let mut ordered = Vec::new();
    for salvo in salvos {
        if seen.insert(salvo.attacker) {
            ordered.push(salvo.attacker);
        }
    }
    ordered
}

#[cfg(test)]
mod tests {
    use wows_replays::analyzer::battle_controller::state::ResolvedShotHit;
    use wows_replays::analyzer::decoder::ArtillerySalvo;
    use wows_replays::analyzer::decoder::HitType;
    use wows_replays::analyzer::decoder::ShotHit;
    use wows_replays::types::ShotId;
    use wowsunpack::game_types::CollisionType;
    use wowsunpack::game_types::WorldPos;

    use super::*;

    fn hit(owner: u32, salvo_id: Option<u32>, shell: u32, at: f32) -> PreExtractedHit {
        let owner = EntityId::from(owner as i64);
        let salvo = salvo_id.map(|salvo_id| ArtillerySalvo {
            owner_id: owner,
            params_id: GameParamId::from(shell),
            salvo_id,
            shots: Vec::new(),
        });
        PreExtractedHit {
            clock: GameClock(at),
            hit: ResolvedShotHit {
                clock: GameClock(at),
                hit: ShotHit {
                    owner_id: owner,
                    hit_type: HitType {
                        collision: Recognized::Known(CollisionType::HitEntity),
                        shell_hit: Recognized::Known(ShellHitType::Normal),
                        raw: 0,
                    },
                    shot_id: ShotId::from(1),
                    position: WorldPos::default(),
                    terminal_ballistics: None,
                },
                victim_entity_id: EntityId::from(99),
                salvo,
                fired_at: None,
                victim_pose: None,
            },
        }
    }

    fn enemies(ids: &[u32]) -> HashSet<EntityId> {
        ids.iter().map(|id| EntityId::from(*id as i64)).collect()
    }

    /// With no attacker chosen, a shell from the ship's own side is not
    /// incoming fire.
    #[test]
    fn only_an_enemys_shells_count_as_incoming() {
        let hits = [hit(1, Some(7), 500, 10.0), hit(2, Some(8), 500, 11.0)];
        let salvos = group_incoming(&hits, &IncomingFilter::default(), &enemies(&[1]), &HashSet::new());
        assert_eq!(salvos.len(), 1);
        assert_eq!(salvos[0].attacker, EntityId::from(1));
    }

    /// An empty main-battery set is not knowledge that nothing is main battery,
    /// so nothing is hidden by it.
    #[test]
    fn nothing_is_hidden_when_no_shell_is_known_to_be_main_battery() {
        let hits = [hit(1, Some(7), 500, 10.0)];
        let salvos = group_incoming(&hits, &IncomingFilter::default(), &enemies(&[1]), &HashSet::new());
        assert_eq!(salvos.len(), 1);

        let known = HashSet::from([GameParamId::from(999u32)]);
        let filtered = group_incoming(&hits, &IncomingFilter::default(), &enemies(&[1]), &known);
        assert!(filtered.is_empty(), "a shell that is not main battery is a secondary");

        let counted = IncomingFilter { attacker: None, secondaries: true };
        assert_eq!(group_incoming(&hits, &counted, &enemies(&[1]), &known).len(), 1);
    }

    /// A hit with no salvo behind it is a torpedo or a bomb, which this log has
    /// nothing to say about.
    #[test]
    fn a_hit_with_no_salvo_is_not_listed() {
        let hits = [hit(1, None, 0, 10.0)];
        assert!(group_incoming(&hits, &IncomingFilter::default(), &enemies(&[1]), &HashSet::new()).is_empty());
    }

    /// One salvo's shells are one group, in the order its first shell landed.
    #[test]
    fn a_salvos_shells_are_one_group() {
        let hits = [hit(1, Some(7), 500, 30.0), hit(1, Some(4), 500, 10.0), hit(1, Some(7), 500, 31.0)];
        let salvos = group_incoming(&hits, &IncomingFilter::default(), &enemies(&[1]), &HashSet::new());
        assert_eq!(salvos.len(), 2);
        assert_eq!(salvos[0].first_clock, GameClock(10.0));
        assert_eq!(salvos[1].shells.len(), 2);
        assert_eq!(salvos[1].latest_clock, GameClock(31.0));
    }
}
