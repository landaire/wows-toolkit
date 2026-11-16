//! Following one ship's hits as playback moves, for an armor viewer.
//!
//! The armor viewer draws hits on a hull. Driven from a replay, which hits it
//! should be drawing depends on where playback has reached, and playback can
//! be scrubbed in either direction. This is that bookkeeping, and the reading
//! of the frame that says which ship an entity is and who was firing at it.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::ops::Range;

use wows_minimap_renderer::draw_command::DrawCommand;
use wows_minimap_renderer::draw_command::RosterRow;
use wows_replay_insights::timeline::PreExtractedHit;
use wows_replay_insights::timeline::ShipShotTimeline;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wows_replays::types::GameParamId;
use wows_toolkit_viewmodel::armor::health_strip::HealthStrip;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::ShellInfo;

use crate::load::GameDataCache;

/// One ship's hits, as far as playback has reached.
pub struct RealtimeArmorFeed {
    timeline: ShipShotTimeline,
    /// Where playback was when this was last moved. `None` before it has been
    /// moved at all, which is not the same as clock zero: a viewport that
    /// opens mid-battle has hits behind it already.
    at: Option<GameClock>,
}

/// What moving the clock turned up.
#[derive(Debug, PartialEq, Eq)]
pub enum Advance {
    /// Playback moved forward and these hits are new since the last move.
    Gained(Range<usize>),
    /// Playback moved backwards, so what was drawn is no longer right and the
    /// hull has to be drawn again from nothing.
    Rewound,
}

impl RealtimeArmorFeed {
    pub fn new(timeline: ShipShotTimeline) -> Self {
        Self { timeline, at: None }
    }

    /// Every hit this ship had taken by where playback has reached.
    pub fn taken(&self) -> &[PreExtractedHit] {
        match self.at {
            Some(clock) => self.timeline.hits_through(clock),
            None => &[],
        }
    }

    /// Every hit this ship takes over the whole battle.
    ///
    /// What the incoming-fire log lists, as against what the hull draws: the log
    /// is a reading of the battle and does not shrink as playback is scrubbed
    /// back, which is how the egui panel reads it.
    pub fn whole_battle(&self) -> &[PreExtractedHit] {
        &self.timeline.hits
    }

    /// This ship's health over the whole battle, as a strip to draw.
    pub fn health_strip(&self) -> Option<HealthStrip> {
        wows_toolkit_viewmodel::armor::health_strip::health_strip(&self.timeline)
    }

    /// What this ship's health was at that moment.
    pub fn health(&self) -> Option<f32> {
        self.timeline.health_at(self.at?).map(|snapshot| snapshot.health)
    }

    /// Where playback has reached, if it has been said.
    pub fn at(&self) -> Option<GameClock> {
        self.at
    }

    /// Moves to `clock` and says what changed.
    ///
    /// A step forward is the hits it gained, as a range into [`Self::taken`],
    /// so a caller draws those rather than all of them again. A step
    /// backwards cannot be undrawn hit by hit, so it says so and the caller
    /// starts the hull again.
    pub fn advance_to(&mut self, clock: GameClock) -> Advance {
        let was = self.at;
        self.at = Some(clock);
        match was {
            Some(was) if clock.seconds() < was.seconds() => Advance::Rewound,
            Some(was) => {
                let from = self.timeline.hits_through(was).len();
                let to = self.timeline.hits_through(clock).len();
                Advance::Gained(from..to)
            }
            // The first move is everything behind it, which is a rewind's
            // answer: draw the hull from nothing.
            None => Advance::Rewound,
        }
    }
}

/// Who was firing at one ship, and with what.
///
/// Read off the battle rather than off the hits, because the filter has to be
/// able to offer an attacker before any of their shells have landed.
#[derive(Clone, Debug, Default)]
pub struct IncomingContext {
    /// Every enemy of the ship being looked at, named as the log lists them.
    /// The keys are also what counts as incoming fire.
    pub attackers: BTreeMap<EntityId, String>,
    /// Every hit that ship takes over the whole battle, which is what the log
    /// lists: it is a reading of the battle rather than of where playback is.
    pub taken: Vec<PreExtractedHit>,
    /// That ship's health over the battle, as the strip beside the log draws
    /// it. `None` for a ship nothing recorded the health of.
    pub health: Option<HealthStrip>,
    /// Where playback stood when the viewer was asked for, so the strip marks it
    /// straight away rather than waiting for playback to move.
    pub at: Option<GameClock>,
    /// The main battery shells of every ship in the battle, so secondaries can
    /// be told apart from them. Empty is not knowledge that none are main
    /// battery, and the filter reads it that way.
    pub main_battery: HashSet<GameParamId>,
    /// The shell each landed salvo was fired with, so the viewer can send that
    /// shell through the hull itself and say whether it agrees with the server.
    /// A salvo whose shell this build cannot name is left out rather than
    /// simulated with a stand-in.
    pub shells: HashMap<GameParamId, ShellInfo>,
}

/// Which ship an entity was in, by its game param index, and what it is
/// called.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArmorTarget {
    pub param_index: String,
    pub display_name: String,
}

/// What an armor viewer opened on a ship is first given.
#[derive(Clone, Debug)]
pub struct ShowArmor {
    pub target: ArmorTarget,
    /// What the ship had taken by where playback is.
    pub hits: Vec<PreExtractedHit>,
    /// Who was shooting at that ship, so the viewer can say which salvo a
    /// hit came from and offer one attacker at a time.
    pub incoming: Box<IncomingContext>,
}

/// What an open armor viewer is told when its ship's hits change as
/// playback moves.
#[derive(Clone, Debug)]
pub struct ArmorFollowed {
    pub hits: Vec<PreExtractedHit>,
    pub health: Option<f32>,
    /// Where playback has reached, which the health strip marks.
    pub at: GameClock,
}

/// The ship an armor viewer was opened on, and what it had taken when it was
/// last told.
///
/// Kept so playback only disturbs the viewer when the set of hits actually
/// changes: showing them again rebuilds the hull's meshes, which is far too
/// much work for every frame.
pub struct ArmorFollow {
    entity_id: EntityId,
    feed: RealtimeArmorFeed,
}

impl ArmorFollow {
    pub fn new(entity_id: EntityId, feed: RealtimeArmorFeed) -> Self {
        Self { entity_id, feed }
    }

    /// Starts following `entity_id` from `clock`, and says what the viewer
    /// opens on.
    ///
    /// `commands` are the frame on screen, whose rosters say which ship the
    /// entity is. `None` when the build cannot name the ship or the battle
    /// recorded no shots on it.
    pub fn start(
        entity_id: EntityId,
        commands: &[DrawCommand],
        shots: &HashMap<EntityId, ShipShotTimeline>,
        clock: GameClock,
        game_data: &GameDataCache,
    ) -> Option<(Self, ShowArmor)> {
        let target = armor_target(commands, entity_id, game_data)?;
        let timeline = shots.get(&entity_id)?.clone();

        let mut feed = RealtimeArmorFeed::new(timeline);
        feed.advance_to(clock);
        let mut incoming = incoming_context(commands, entity_id, game_data);
        // The log reads the whole battle, not the part played so far: scrubbing
        // back would otherwise take salvos out of a list nobody was scrubbing.
        incoming.taken = feed.whole_battle().to_vec();
        resolve_incoming_shells(&mut incoming, game_data);
        incoming.health = feed.health_strip();
        incoming.at = Some(clock);

        let shown = ShowArmor { target, hits: feed.taken().to_vec(), incoming: Box::new(incoming) };
        Some((Self { entity_id, feed }, shown))
    }

    pub fn entity_id(&self) -> EntityId {
        self.entity_id
    }

    /// Moves to where playback has reached, and says what the viewer is to be
    /// told, if anything.
    ///
    /// Only when the set of hits has changed: telling it again rebuilds the
    /// hull's meshes, which is far more than a frame of playback is worth.
    pub fn follow(&mut self, clock: GameClock) -> Option<ArmorFollowed> {
        let changed = match self.feed.advance_to(clock) {
            Advance::Rewound => true,
            Advance::Gained(gained) => !gained.is_empty(),
        };
        if !changed {
            return None;
        }

        Some(ArmorFollowed { hits: self.feed.taken().to_vec(), health: self.feed.health(), at: clock })
    }
}

/// Which ship `entity_id` was in, and what it is called, when the build
/// knows.
///
/// Read off the roster the frame already carries: a ship command says who is
/// in it but not which ship it is, and the roster says both.
pub fn armor_target(commands: &[DrawCommand], entity_id: EntityId, game_data: &GameDataCache) -> Option<ArmorTarget> {
    let param_id = commands.iter().find_map(|command| {
        let DrawCommand::TeamRoster { rows, .. } = command else { return None };
        rows.iter().find(|row| row.entity_id == entity_id)?.ship_param_id
    })?;
    let loaded = game_data.newest_loaded()?;
    let param = loaded.provider().game_param_by_id(param_id)?;

    Some(ArmorTarget { param_index: param.index().to_string(), display_name: param.name().to_string() })
}

/// Whether `entity_id`'s ship has anything an armor viewer could show.
///
/// Only a ship the battle recorded hits on, since the viewer's whole point is
/// where those landed.
pub fn has_armor_to_show(
    shots: &HashMap<EntityId, ShipShotTimeline>,
    commands: &[DrawCommand],
    entity_id: EntityId,
    game_data: &GameDataCache,
) -> bool {
    shots.get(&entity_id).is_some_and(|timeline| !timeline.hits.is_empty())
        && armor_target(commands, entity_id, game_data).is_some()
}

/// Who was shooting at `victim`, and which shells are main battery ones.
///
/// Read off the rosters the frame carries, which are the only place that
/// says which team a ship is on and what it is called at once.
pub fn incoming_context(commands: &[DrawCommand], victim: EntityId, game_data: &GameDataCache) -> IncomingContext {
    let mut context = IncomingContext::default();

    // One roster per team, so every one of them is read: the enemies are in
    // the roster the victim is not in.
    let rows: Vec<&RosterRow> = commands
        .iter()
        .filter_map(|command| {
            let DrawCommand::TeamRoster { rows, .. } = command else { return None };
            Some(rows.iter())
        })
        .flatten()
        .collect();
    let Some(victim_team) = rows.iter().find(|row| row.entity_id == victim).map(|row| row.team_id) else {
        return context;
    };

    for row in rows.iter().filter(|row| row.team_id != victim_team) {
        context.attackers.insert(row.entity_id, format!("{} ({})", row.player_name, row.ship_name));
    }

    let Some(loaded) = game_data.newest_loaded() else { return context };
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
pub fn resolve_incoming_shells(context: &mut IncomingContext, game_data: &GameDataCache) {
    let Some(loaded) = game_data.newest_loaded() else {
        // Without game data no shell can be named, so no shell can be
        // simulated and the log carries no verdicts at all.
        tracing::warn!("armor viewer: no game data loaded, so the incoming shells cannot be named");
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
