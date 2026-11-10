//! The armor viewer a replay viewport drives: which ship it shows, and what
//! that ship had taken by where playback has reached.

use std::collections::HashMap;

use wows_minimap_renderer::draw_command::DrawCommand;
use wows_minimap_renderer::draw_command::RosterRow;
use wows_replay_insights::timeline::PreExtractedHit;
use wows_replay_insights::timeline::ShipShotTimeline;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wows_toolkit_armor::realtime::Advance;
use wows_toolkit_armor::realtime::IncomingContext;
use wows_toolkit_armor::realtime::RealtimeArmorFeed;
use wows_toolkit_replay_data::load::LoadedGameData;
use wows_toolkit_replay_data::playback::Track;
use wowsunpack::game_params::types::GameParamProvider as _;

use crate::renderer::ids::FrameIndex;

/// What the viewer is asked to open on.
pub struct RealtimeShow {
    pub param_index: String,
    pub display_name: String,
    pub hits: Vec<PreExtractedHit>,
    pub incoming: IncomingContext,
}

/// What the viewer is told as playback moves.
pub struct RealtimeFollow {
    pub hits: Vec<PreExtractedHit>,
    pub health: Option<f32>,
    pub at: GameClock,
}

#[derive(Default)]
pub struct ArmorLink {
    shots: HashMap<EntityId, ShipShotTimeline>,
    following: Option<(EntityId, RealtimeArmorFeed)>,
}

impl ArmorLink {
    pub fn set_shots(&mut self, shots: HashMap<EntityId, ShipShotTimeline>) {
        self.shots = shots;
    }

    /// Whether `entity` has anything a viewer could show: only a ship the
    /// battle recorded hits on, since the viewer's point is where they landed.
    pub fn can_show(&self, track: &Track, at: FrameIndex, entity: EntityId, loaded: Option<&LoadedGameData>) -> bool {
        self.shots.get(&entity).is_some_and(|timeline| !timeline.hits.is_empty())
            && armor_target(track, at, entity, loaded).is_some()
    }

    /// The viewer request for `entity`, and follows it from then on.
    pub fn show(
        &mut self,
        track: &Track,
        at: FrameIndex,
        entity: EntityId,
        loaded: Option<&LoadedGameData>,
    ) -> Option<RealtimeShow> {
        let target = armor_target(track, at, entity, loaded)?;
        let timeline = self.shots.get(&entity)?.clone();
        let clock = track.clock_at(at.get());

        let mut feed = RealtimeArmorFeed::new(timeline);
        feed.advance_to(clock);
        let mut incoming = incoming_context(track, at, entity, loaded);
        // The log reads the whole battle, not the part played so far:
        // scrubbing back would otherwise take salvos out of the list.
        incoming.taken = feed.whole_battle().to_vec();
        resolve_incoming_shells(&mut incoming, loaded);
        incoming.health = feed.health_strip();
        incoming.at = Some(clock);
        let hits = feed.taken().to_vec();
        self.following = Some((entity, feed));
        Some(RealtimeShow { param_index: target.param_index, display_name: target.display_name, hits, incoming })
    }

    /// What the followed ship had taken by `clock`, when that changed. Telling
    /// the viewer again rebuilds the hull's meshes, so an unchanged set is not
    /// sent.
    pub fn follow(&mut self, clock: GameClock) -> Option<RealtimeFollow> {
        let (_, feed) = self.following.as_mut()?;
        let changed = match feed.advance_to(clock) {
            Advance::Rewound => true,
            Advance::Gained(gained) => !gained.is_empty(),
        };
        changed.then(|| RealtimeFollow { hits: feed.taken().to_vec(), health: feed.health(), at: clock })
    }
}

/// Which ship `ArmorLink::show` opens on.
struct ArmorTarget {
    param_index: String,
    display_name: String,
}

fn roster(track: &Track, at: FrameIndex) -> Vec<&RosterRow> {
    let Some(commands) = track.frames.get(at.get()) else { return Vec::new() };
    commands
        .iter()
        .filter_map(|command| match command {
            DrawCommand::TeamRoster { rows, .. } => Some(rows.iter()),
            _ => None,
        })
        .flatten()
        .collect()
}

/// Which ship `entity` was in and what it is called, read off the roster the
/// frame carries: a ship command says who is in it but not which ship it is.
fn armor_target(
    track: &Track,
    at: FrameIndex,
    entity: EntityId,
    loaded: Option<&LoadedGameData>,
) -> Option<ArmorTarget> {
    let param_id = roster(track, at).into_iter().find(|row| row.entity_id == entity)?.ship_param_id?;
    let param = loaded?.provider().game_param_by_id(param_id)?;
    Some(ArmorTarget { param_index: param.index().to_string(), display_name: param.name().to_string() })
}

/// Who was shooting at `victim`, and which shells are main battery ones.
fn incoming_context(
    track: &Track,
    at: FrameIndex,
    victim: EntityId,
    loaded: Option<&LoadedGameData>,
) -> IncomingContext {
    let mut context = IncomingContext::default();
    let rows = roster(track, at);
    let Some(victim_team) = rows.iter().find(|row| row.entity_id == victim).map(|row| row.team_id) else {
        return context;
    };
    for row in rows.iter().filter(|row| row.team_id != victim_team) {
        context.attackers.insert(row.entity_id, format!("{} ({})", row.player_name, row.ship_name));
    }

    let Some(loaded) = loaded else { return context };
    let provider = loaded.provider();
    for row in &rows {
        let Some(ship) = row.ship_param_id.and_then(|id| provider.game_param_by_id(id)) else { continue };
        let Some(config) = ship.vehicle().and_then(|vehicle| vehicle.config_data()) else { continue };
        for name in &config.main_battery_ammo {
            if let Some(shell) = provider.game_param_by_name(name) {
                context.main_battery.insert(shell.id());
            }
        }
    }
    context
}

/// Names the shell behind each salvo that landed on the ship.
fn resolve_incoming_shells(context: &mut IncomingContext, loaded: Option<&LoadedGameData>) {
    let Some(loaded) = loaded else {
        // Without game data no shell can be named, so none is simulated and
        // the log carries no verdicts.
        log::warn!("armor viewer: no game data loaded, so the incoming shells cannot be named");
        return;
    };
    let provider = loaded.provider();
    for hit in &context.taken {
        let Some(salvo) = hit.hit.salvo.as_ref() else { continue };
        if context.shells.contains_key(&salvo.params_id) {
            continue;
        }
        if let Some(shell) = provider.resolve_shell_from_param_id(salvo.params_id) {
            context.shells.insert(salvo.params_id, shell);
        }
    }
}
