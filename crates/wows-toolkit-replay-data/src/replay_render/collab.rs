//! What a playback says to a collab session, and how a peer's pings are
//! drawn over it.

use std::collections::HashMap;
use std::time::Duration;

use wows_minimap_renderer::RenderOptions;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_minimap_renderer::draw_command::ShipConfigFilter;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wows_toolkit_services::collab::CollabLink;
use wows_toolkit_services::collab::ReplayId;
use wows_toolkit_services::collab::SharedReplay;
use wows_toolkit_services::collab::Silhouette;
use wt_collab_client::peer::FrameBroadcast;

use crate::load::GameDataCache;
use crate::replay_render::motion::FrameMotion;
use crate::replay_render::motion::reach_ships;
use crate::replay_render::ships::NO_RANGES;
use crate::replay_render::ships::ShipChoices;
use crate::replay_render::track::Track;

/// How long a ping stays on the map, in seconds.
pub const PING_SECONDS: f32 = 1.0;

/// How often a running session redraws, which is what moves a peer's cursor
/// and carries a ping through its ripple.
pub const COLLAB_TICK: Duration = Duration::from_millis(33);

/// How wide a ping's ring grows, in element pixels.
pub const PING_RADIUS: f32 = 26.0;

/// How a ping is drawn at one moment of its ripple.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PingRing {
    /// The ring's radius, in element pixels.
    pub radius: f32,
    /// How opaque its border is, from 1 down to 0.
    pub opacity: f32,
}

/// Which map point was last told to the session as this end's pointer, so an
/// unmoved pointer is not sent again.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CursorReport {
    reported: Option<[f32; 2]>,
}

impl CursorReport {
    pub fn new() -> Self {
        Self { reported: None }
    }

    /// Tells the session where this end's pointer is on the map, `None` once
    /// it has left the map.
    ///
    /// Sent only while a session is running, and only when it has moved to a
    /// different map point: a pointer crossing a zoomed-in map would otherwise
    /// send a message per pixel.
    pub fn report(&mut self, link: &CollabLink, at: Option<[f32; 2]>) {
        if !link.is_active() {
            return;
        }
        let moved = match (self.reported, at) {
            (Some(was), Some(now)) => (was[0] - now[0]).abs() >= 1.0 || (was[1] - now[1]).abs() >= 1.0,
            (None, None) => false,
            _ => true,
        };
        if !moved {
            return;
        }
        self.reported = at;
        link.report_cursor(at);
    }
}

/// How a ping `age` old is drawn, or `None` once its ripple has run out.
///
/// A ring that grows and fades over its first second, as the egui renderer's
/// ripple does.
pub fn ping_ring(age: Duration) -> Option<PingRing> {
    let age = age.as_secs_f32();
    if age > PING_SECONDS {
        return None;
    }

    Some(PingRing { radius: PING_RADIUS * (0.3 + age / PING_SECONDS), opacity: 1.0 - age / PING_SECONDS })
}

/// How opaque a peer's pointer that last moved `age` ago is drawn, from 1 down to 0, or `None`
/// once it has stood still long enough to be taken down.
///
/// A peer whose app froze, or that left without saying its pointer had left the map, would
/// otherwise leave its name on the map for good. The egui renderer's timings.
pub fn cursor_opacity(age: Duration) -> Option<f32> {
    const FADES_AFTER: Duration = Duration::from_secs(3);
    const GONE_AFTER: Duration = Duration::from_secs(5);

    if age > GONE_AFTER {
        return None;
    }
    let Some(fading) = age.checked_sub(FADES_AFTER) else { return Some(1.0) };

    Some(1.0 - fading.as_secs_f32() / (GONE_AFTER - FADES_AFTER).as_secs_f32())
}

/// What a session watching this playback is sent at `clock`, between `motion`'s frame and the
/// next: every layer, moved on to the clock as the host's own frame is, so a watcher sees the
/// battle move between baked frames rather than in their steps.
///
/// Draw commands rather than pixels, so each peer draws the battle with its own art at its own
/// size. The trails `options` ask for go behind it: a watcher has no track to read trails from,
/// and a baked frame holds none, so they travel with it as the egui host sends them. Every ship's
/// trail is sent; a watcher hides the ones the session hides.
pub fn moment_broadcast(
    replay_id: ReplayId,
    track: &Track,
    motion: &FrameMotion,
    clock: GameClock,
    options: &RenderOptions,
) -> FrameBroadcast {
    let frame: Vec<DrawCommand> = motion.commands_at(track, clock, |_| true).collect();
    let mut commands = track.trails.trails_at(clock, options);
    reach_ships(&mut commands, &frame);
    commands.extend(frame);

    FrameBroadcast {
        replay_id: replay_id.raw(),
        clock: clock.seconds(),
        frame_index: motion.base().get() as u32,
        total_frames: track.len() as u32,
        game_duration: track.duration_seconds(),
        commands,
    }
}

/// What a session is told about this playback when it opens, map art
/// included. Blocks while the art is read and encoded.
///
/// The art travels with it: a peer may not have the build the replay was
/// recorded on, and it still has to draw the battle. `None` for a track with
/// no map to name.
pub fn shared_replay(
    replay_id: ReplayId,
    replay_name: String,
    track: &Track,
    game_data: &GameDataCache,
) -> Option<SharedReplay> {
    if track.space.is_empty() {
        return None;
    }

    let art_png = game_data
        .newest_loaded()
        .and_then(|loaded| wows_minimap_renderer::assets::load_map_image(&track.space, loaded.vfs()))
        .and_then(|art| {
            let mut png = Vec::new();
            art.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).ok().map(|()| png)
        });

    Some(SharedReplay {
        replay_id,
        replay_name,
        map_label: wows_toolkit_viewmodel::tactics::naming::pretty_map_name(&track.space),
        map_name: track.space.clone(),
        game_version: track.version.to_path(),
        art_png,
    })
}

/// The ranges each ship shows, as a session is told them: by entity, leaving out the ships that
/// show none and the players `names` has no ship for.
pub fn range_overrides(choices: &ShipChoices, names: &HashMap<EntityId, String>) -> Vec<(EntityId, ShipConfigFilter)> {
    let mut overrides: Vec<(EntityId, ShipConfigFilter)> = names
        .iter()
        .map(|(entity_id, name)| (*entity_id, choices.ranges_for(name)))
        .filter(|(_, filter)| *filter != NO_RANGES)
        .collect();
    // In one order, so the same choices always read the same on the wire.
    overrides.sort_by_key(|(entity_id, _)| *entity_id);

    overrides
}

/// The ranges a session says each ship shows, by the player names the commands carry. An entity
/// `names` has no ship for has nothing drawn to show them on.
pub fn ranges_by_player(
    overrides: &[(EntityId, ShipConfigFilter)],
    names: &HashMap<EntityId, String>,
) -> HashMap<String, ShipConfigFilter> {
    overrides.iter().filter_map(|(entity_id, filter)| Some((names.get(entity_id)?.clone(), *filter))).collect()
}

/// Takes up the ranges a session says each ship shows, in place of every ship's own. The range
/// layer goes on when any ship shows one, as the egui renderer does.
pub fn adopt_range_overrides(
    overrides: &[(EntityId, ShipConfigFilter)],
    names: &HashMap<EntityId, String>,
    choices: &mut ShipChoices,
    options: &mut RenderOptions,
) {
    choices.replace_ranges(ranges_by_player(overrides, names));
    if !overrides.is_empty() {
        options.show_ship_config = true;
    }
}

/// Takes up whose trails a session hides. The trail layer goes on when some are hidden, since
/// the rest are still meant to be drawn, as the egui renderer does.
pub fn adopt_trail_overrides(hidden: &[String], choices: &mut ShipChoices, options: &mut RenderOptions) {
    choices.replace_trail_hidden(hidden.iter().cloned().collect());
    if !hidden.is_empty() {
        options.show_trails = true;
    }
}

/// The recording player's ship silhouette as a track's stats panel draws it, for a web client
/// that has no game files to read it from. `None` for a track baked without one.
pub fn track_silhouette(track: &Track) -> Option<Silhouette> {
    let image = track.frames.iter().flatten().find_map(|command| match command {
        DrawCommand::StatsSilhouette { silhouette: Some(silhouette), .. } => Some(silhouette),
        _ => None,
    })?;

    Some(Silhouette { width: image.width(), height: image.height(), rgba: image.as_raw().clone() })
}
