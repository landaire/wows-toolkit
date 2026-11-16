//! Whole-battle scans a renderer reads from its first frame.
//!
//! A forward pass only knows what has happened up to its clock, and only from
//! the perspectives it merges. The roster's damage column and the tracer
//! pacing need the whole battle as seen from every recording, so they are
//! scanned once up front and handed to each renderer that draws the battle.

use std::collections::HashMap;
use std::sync::Arc;

use wows_battle_world::SalvoFlightTimes;
use wows_battle_world::merged::MergedReplays;
use wows_replays::ReplayFile;
use wows_replays::analyzer::battle_controller::DamageEvent;
use wows_replays::game_constants::GameConstants;
use wows_replays::types::EntityId;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::Version;

use crate::renderer::MinimapRenderer;

/// Damage dealt per aggressor across every recording, and each salvo's flight
/// time learned from the primary recording's impacts.
///
/// Gathered once per battle. A session or renderer rebuilt for a seek takes
/// the same scans again through [`BattleScans::install`].
pub struct BattleScans {
    damage_events: HashMap<EntityId, Vec<DamageEvent>>,
    salvo_flight_times: Arc<SalvoFlightTimes>,
}

impl BattleScans {
    /// Scans `primary` and `alts`, the recordings of one battle.
    ///
    /// `constants` must be the replay's decode constants (the build's own
    /// merged with its versioned overrides), the same set the session that
    /// draws the battle is built with.
    pub fn gather<G>(
        game_params: &G,
        constants: &GameConstants,
        version: Version,
        primary: &ReplayFile,
        alts: &[ReplayFile],
    ) -> Self
    where
        G: ResourceLoader,
    {
        let mut replays: Vec<&ReplayFile> = Vec::with_capacity(1 + alts.len());
        replays.push(primary);
        replays.extend(alts.iter());

        let damage_events = wows_battle_world::merged::gather_damage_events(
            game_params,
            constants,
            version,
            game_params.entity_specs(),
            &replays,
        );
        let salvo_flight_times =
            wows_battle_world::scan::scan_salvo_flight_times(&primary.meta, game_params, constants, version, primary);

        Self { damage_events, salvo_flight_times: Arc::new(salvo_flight_times) }
    }

    /// Gives `renderer` these scans and what `session` scanned when it was
    /// built: every vehicle's facts and the position timeline.
    pub fn install<G>(&self, renderer: &mut MinimapRenderer<'_>, session: &MergedReplays<'_, '_, G>)
    where
        G: ResourceLoader,
    {
        renderer.set_damage_events(self.damage_events.clone());
        renderer.set_salvo_flight_times(Arc::clone(&self.salvo_flight_times));
        renderer.set_vehicle_facts(session.vehicle_facts().clone());
        renderer.set_position_timeline(session.position_timeline());
    }
}
