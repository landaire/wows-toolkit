//! Per-victim burn-state and Damage Control Party reconstruction.
//!
//! Both queries here answer "could this shell have started a fire": which
//! sections were already burning immediately before a hit's clock, and was
//! Damage Control Party running then. Both are read from logs that are only
//! complete over a range the caller has separately confirmed was
//! continuously observed (`PresenceLog::continuously_observed`); this module
//! does not check that itself, except for the one presence-derived rule
//! inside `damage_control_at` documented below.

use wows_battle_world::resources::BurnStateChange;
use wows_battle_world::resources::PresenceLog;
use wows_replays::analyzer::battle_controller::state::ActiveConsumable;
use wows_replays::analyzer::battle_controller::state::ConsumableInventory;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wowsunpack::game_types::Consumable;

/// Two observed Damage Control Party activations closer together than this
/// many seconds below the modelled reload are still treated as a normal
/// reload, not a refund; absorbs clock jitter rather than requiring an exact
/// match against `reload_time`.
const COOLDOWN_TOLERANCE_SECS: f32 = 1.0;

/// Per-victim burn-node transitions and Damage Control Party activations,
/// reconstructed from the whole-match logs and narrowed to one ship.
pub struct VictimTrack {
    /// Burn-node bitmask changes for this victim, sorted ascending by clock.
    changes: Vec<BurnStateChange>,
    /// Observed Damage Control Party activations, sorted ascending by
    /// `activated_at`. Filtered to `Consumable::DamageControl`.
    dcp: Vec<ActiveConsumable>,
    /// Reload for the victim's Damage Control Party slot, build modifiers
    /// already applied by `ConsumableInventory`. `None` when the ship carries
    /// no such slot, which makes every cooldown inference `Unknown`.
    dcp_reload_secs: Option<f32>,
    /// Earliest clock this victim was observed from, set only when that
    /// observation is a single presence window still open (`left: None`):
    /// the one case where "unbroken since" holds for any future query clock
    /// without also needing the full window list at query time. Any gap, any
    /// second window, or a window that has since closed all clear this to
    /// `None`, which is conservative (queries inside a closed early window
    /// report `Unknown` instead of the `Down` they could technically prove)
    /// but never turns a real gap into a false `Down`.
    first_seen: Option<GameClock>,
    cooldown_unreliable: bool,
}

/// What is known about Damage Control Party at a given clock.
pub enum DamageControlState {
    /// Observed activation covering this clock.
    Running,
    /// Either observed continuously with no activation covering this clock, or
    /// provably still on cooldown from the last observed activation.
    Down,
    /// An observation gap long enough that an unseen activation could have
    /// happened, or a ship whose cooldown model is unreliable.
    Unknown,
}

impl VictimTrack {
    pub fn build(
        victim: EntityId,
        changes: &[BurnStateChange],
        activations: &[ActiveConsumable],
        inventory: &[ConsumableInventory],
        presence: &PresenceLog,
        died_at: Option<GameClock>,
    ) -> VictimTrack {
        let mut changes: Vec<BurnStateChange> = changes
            .iter()
            .filter(|c| c.victim == victim)
            .filter(|c| died_at.is_none_or(|died_at| c.clock < died_at))
            .cloned()
            .collect();
        changes.sort_by_key(|c| c.clock);

        let mut dcp: Vec<ActiveConsumable> =
            activations.iter().filter(|a| a.consumable.known() == Some(&Consumable::DamageControl)).cloned().collect();
        dcp.sort_by_key(|a| a.activated_at);

        let dcp_reload_secs = inventory
            .iter()
            .find(|slot| slot.consumable.known() == Some(&Consumable::DamageControl))
            .map(|slot| slot.reload_time);

        let cooldown_unreliable = dcp_reload_secs.is_some_and(|reload| {
            dcp.windows(2).any(|pair| (pair[1].activated_at - pair[0].activated_at) < reload - COOLDOWN_TOLERANCE_SECS)
        });

        let first_seen = presence.0.get(&victim).and_then(|windows| match windows.as_slice() {
            [only] if only.left.is_none() => Some(only.entered),
            _ => None,
        });

        VictimTrack { changes, dcp, dcp_reload_secs, first_seen, cooldown_unreliable }
    }

    /// Burn-node bitmask immediately before `clock`. Changes exactly at
    /// `clock` are excluded: a fire started by this very salvo must not make
    /// its own hits ineligible.
    pub fn burn_mask_before(&self, clock: GameClock) -> u16 {
        let idx = self.changes.partition_point(|c| c.clock < clock);
        // No prior change is 0, not a missing-data sentinel: BurnStateLog logs
        // a baseline transition from 0 on first sighting, so within a range
        // the caller has confirmed continuously observed, "no change yet"
        // and "unburned" are the same fact.
        idx.checked_sub(1).map(|i| self.changes[i].current).unwrap_or(0)
    }

    pub fn damage_control_at(&self, clock: GameClock) -> DamageControlState {
        let running = self.dcp.iter().any(|a| clock >= a.activated_at && clock <= a.activated_at + a.duration);
        if running {
            return DamageControlState::Running;
        }

        if self.cooldown_unreliable {
            return DamageControlState::Unknown;
        }

        if let Some(reload) = self.dcp_reload_secs
            && let Some(last) = self.dcp.iter().rev().find(|a| a.activated_at <= clock)
            && clock < last.activated_at + reload
        {
            return DamageControlState::Down;
        }

        // Activations are broadcast for any entity in the recording client's
        // AOI, not just the self ship. So a victim continuously observed from
        // its first presence window through `clock`, with no activation
        // covering `clock` and none whose cooldown could still be running,
        // provably never used Damage Control Party in that span: an
        // activation would have produced an entry in `dcp`.
        if self.first_seen.is_some_and(|seen| seen <= clock) {
            return DamageControlState::Down;
        }

        DamageControlState::Unknown
    }

    /// True when two observed activations of the same slot were closer
    /// together than the modelled reload, so cooldown inferences are unsafe.
    pub fn cooldown_unreliable(&self) -> bool {
        self.cooldown_unreliable
    }
}

#[cfg(test)]
mod tests {
    use wows_battle_world::resources::PresenceWindow;
    use wowsunpack::recognized::Recognized;

    use super::*;

    fn victim_id() -> EntityId {
        EntityId::from(1u32)
    }

    fn track_with_changes(entries: &[(GameClock, u16, u16)]) -> VictimTrack {
        track_with_changes_and_death(entries, None)
    }

    fn track_with_changes_and_death(entries: &[(GameClock, u16, u16)], died_at: Option<GameClock>) -> VictimTrack {
        let victim = victim_id();
        let changes: Vec<BurnStateChange> = entries
            .iter()
            .map(|&(clock, previous, current)| BurnStateChange { victim, clock, previous, current })
            .collect();
        VictimTrack::build(victim, &changes, &[], &[], &PresenceLog::default(), died_at)
    }

    fn track_with_dcp(activations: &[(GameClock, f32)], reload_time: f32, presence: PresenceLog) -> VictimTrack {
        let victim = victim_id();
        let activations: Vec<ActiveConsumable> = activations
            .iter()
            .map(|&(activated_at, duration)| ActiveConsumable {
                consumable: Recognized::Known(Consumable::DamageControl),
                activated_at,
                duration,
                usage_params: None,
            })
            .collect();
        let inventory = vec![ConsumableInventory {
            slot_index: 0,
            consumable_type_raw: "PCY001_CrashCrew".to_string(),
            consumable: Recognized::Known(Consumable::DamageControl),
            icon_key: "PCY001_CrashCrew".to_string(),
            total_charges: wowsunpack::game_types::ChargeCount::Unlimited,
            charges_used: 0,
            work_time: 5.0,
            reload_time,
            regen_hp_speed: None,
            regen_hp_speed_units: None,
            active_until: None,
        }];
        VictimTrack::build(victim, &[], &activations, &inventory, &presence, None)
    }

    fn observed_from(entered: GameClock) -> PresenceLog {
        let mut log = PresenceLog::default();
        log.0.insert(victim_id(), vec![PresenceWindow { entered, left: None }]);
        log
    }

    fn observed_with_gap(entered1: GameClock, left1: GameClock, entered2: GameClock) -> PresenceLog {
        let mut log = PresenceLog::default();
        log.0.insert(
            victim_id(),
            vec![
                PresenceWindow { entered: entered1, left: Some(left1) },
                PresenceWindow { entered: entered2, left: None },
            ],
        );
        log
    }

    /// A change exactly at the hit clock must not count: the fire this salvo
    /// started would otherwise make the salvo's own hits ineligible.
    #[test]
    fn burn_mask_before_excludes_a_change_at_the_same_clock() {
        let track = track_with_changes(&[(GameClock(100.0), 0b0000, 0b0001)]);
        assert_eq!(track.burn_mask_before(GameClock(100.0)), 0b0000);
        assert_eq!(track.burn_mask_before(GameClock(100.1)), 0b0001);
    }

    #[test]
    fn burn_mask_before_takes_the_latest_prior_change() {
        let track = track_with_changes(&[
            (GameClock(10.0), 0b0000, 0b0001),
            (GameClock(20.0), 0b0001, 0b0011),
            (GameClock(30.0), 0b0011, 0b0010),
        ]);
        assert_eq!(track.burn_mask_before(GameClock(25.0)), 0b0011);
        assert_eq!(track.burn_mask_before(GameClock(35.0)), 0b0010);
        assert_eq!(track.burn_mask_before(GameClock(5.0)), 0b0000);
    }

    /// Inside work time DCP is running; between work time and reload it is
    /// provably down, because the ship cannot reactivate inside reload_time.
    #[test]
    fn dcp_is_running_then_provably_down_through_the_cooldown() {
        let track = track_with_dcp(&[(GameClock(100.0), 15.0)], 80.0, observed_from(GameClock(0.0)));
        assert!(matches!(track.damage_control_at(GameClock(105.0)), DamageControlState::Running));
        assert!(matches!(track.damage_control_at(GameClock(120.0)), DamageControlState::Down));
        assert!(matches!(track.damage_control_at(GameClock(175.0)), DamageControlState::Down));
    }

    /// Past the cooldown, a continuously observed ship is still known: we
    /// would have seen any activation. Continuity is what makes this safe.
    #[test]
    fn dcp_past_cooldown_is_down_while_observed_and_unknown_across_a_gap() {
        let observed = observed_from(GameClock(0.0));
        let track = track_with_dcp(&[(GameClock(100.0), 15.0)], 80.0, observed);
        assert!(matches!(track.damage_control_at(GameClock(300.0)), DamageControlState::Down));

        let gapped = observed_with_gap(GameClock(0.0), GameClock(200.0), GameClock(280.0));
        let track = track_with_dcp(&[(GameClock(100.0), 15.0)], 80.0, gapped);
        assert!(matches!(track.damage_control_at(GameClock(300.0)), DamageControlState::Unknown));
    }

    /// Two activations closer than reload_time mean the ship refunds charges
    /// (Valparaiso, San Martin). Cooldown inference stops being sound, so
    /// everything outside an observed work window becomes Unknown.
    #[test]
    fn a_refund_ship_is_detected_and_stops_cooldown_inference() {
        let track =
            track_with_dcp(&[(GameClock(100.0), 15.0), (GameClock(130.0), 15.0)], 80.0, observed_from(GameClock(0.0)));
        assert!(track.cooldown_unreliable());
        assert!(matches!(track.damage_control_at(GameClock(135.0)), DamageControlState::Running));
        assert!(matches!(track.damage_control_at(GameClock(300.0)), DamageControlState::Unknown));
    }

    /// On death the server lights extra burn nodes for effect. Those must not
    /// enter the track, or a fire we did not set becomes attributable to a
    /// shell that landed near the kill.
    #[test]
    fn transitions_at_or_after_death_are_discarded() {
        let track = track_with_changes_and_death(
            &[(GameClock(100.0), 0b0000, 0b0001), (GameClock(200.0), 0b0001, 0b1111)],
            Some(GameClock(200.0)),
        );
        assert_eq!(track.burn_mask_before(GameClock(250.0)), 0b0001);
    }

    /// A ship that never died keeps every transition.
    #[test]
    fn a_surviving_victim_keeps_every_transition() {
        let track = track_with_changes_and_death(
            &[(GameClock(100.0), 0b0000, 0b0001), (GameClock(200.0), 0b0001, 0b0011)],
            None,
        );
        assert_eq!(track.burn_mask_before(GameClock(250.0)), 0b0011);
    }

    /// A ship never observed to activate DCP, but watched continuously from
    /// the start, has definitely not used it.
    #[test]
    fn never_activated_and_always_observed_is_down() {
        let track = track_with_dcp(&[], 80.0, observed_from(GameClock(0.0)));
        assert!(matches!(track.damage_control_at(GameClock(200.0)), DamageControlState::Down));
    }
}
