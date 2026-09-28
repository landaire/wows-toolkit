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
///
/// A hit the parser could not match back to a salvo is its own group rather than
/// being folded into a neighbouring one: two unmatched hits are not evidence of
/// one salvo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SalvoKey {
    Matched {
        owner: EntityId,
        salvo_id: u32,
    },
    /// Keyed by the shot it came from, which is unique within a battle. The raw
    /// id rather than the newtype, which carries no ordering and is only wanted
    /// here to keep the groups in a stable order.
    Unmatched(u32),
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
    /// Who fired it, where the hit was matched to a salvo.
    pub attacker: Option<EntityId>,
    /// The shell that was fired, for naming it.
    pub shell: Option<GameParamId>,
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
        let shell = hit.salvo.as_ref().map(|salvo| salvo.params_id);
        if !filter.secondaries
            && !main_battery.is_empty()
            && let Some(shell) = shell
            && !main_battery.contains(&shell)
        {
            continue;
        }

        let key = match &hit.salvo {
            Some(salvo) => SalvoKey::Matched { owner: salvo.owner_id, salvo_id: salvo.salvo_id },
            None => SalvoKey::Unmatched(hit.hit.shot_id.raw()),
        };
        let shell_entry =
            IncomingShell { shot_id: hit.hit.shot_id, clock: taken.clock, hit_type: hit.hit.hit_type.clone() };

        let group = groups.entry(key).or_insert_with(|| IncomingSalvo {
            key,
            attacker: hit.salvo.as_ref().map(|salvo| salvo.owner_id),
            shell,
            first_clock: taken.clock,
            latest_clock: taken.clock,
            shells: Vec::new(),
        });
        group.first_clock = min_clock(group.first_clock, taken.clock);
        group.latest_clock = max_clock(group.latest_clock, taken.clock);
        group.shells.push(shell_entry);
    }

    let mut ordered: Vec<IncomingSalvo> = groups.into_values().collect();
    ordered.sort_by(|a, b| {
        a.first_clock.0.partial_cmp(&b.first_clock.0).unwrap_or(std::cmp::Ordering::Equal).then(a.key.cmp(&b.key))
    });
    ordered
}

/// Whether a shell from `owner` is incoming fire under `filter`.
fn counted(owner: EntityId, filter: &IncomingFilter, enemies: &HashSet<EntityId>) -> bool {
    match filter.attacker {
        Some(attacker) => owner == attacker,
        None => enemies.contains(&owner),
    }
}

fn min_clock(a: GameClock, b: GameClock) -> GameClock {
    if b.0 < a.0 { b } else { a }
}

fn max_clock(a: GameClock, b: GameClock) -> GameClock {
    if b.0 > a.0 { b } else { a }
}

/// Every attacker the log holds shells from, in the order they first appear.
///
/// What the attacker filter offers: a ship that never hit this one is not worth
/// filtering to.
pub fn attackers(salvos: &[IncomingSalvo]) -> Vec<EntityId> {
    let mut seen = HashSet::new();
    let mut ordered = Vec::new();
    for salvo in salvos {
        if let Some(attacker) = salvo.attacker
            && seen.insert(attacker)
        {
            ordered.push(attacker);
        }
    }
    ordered
}
