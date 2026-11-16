//! What a playback says to a collab session, and how a peer's pings are
//! drawn over it.

use std::time::Duration;

use wows_minimap_renderer::draw_command::DrawCommand;
use wows_toolkit_services::collab::CollabLink;
use wows_toolkit_services::collab::ReplayId;
use wows_toolkit_services::collab::SharedReplay;
use wt_collab_client::peer::FrameBroadcast;

use crate::load::GameDataCache;
use crate::replay_render::track::FrameIndex;
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

/// What a session watching this playback is sent for frame `at`.
///
/// Draw commands rather than pixels, so each peer draws the battle with its
/// own art at its own size.
pub fn frame_broadcast(replay_id: ReplayId, track: &Track, at: FrameIndex, commands: &[DrawCommand]) -> FrameBroadcast {
    FrameBroadcast {
        replay_id: replay_id.raw(),
        clock: track.seconds_at(at),
        frame_index: at.get() as u32,
        total_frames: track.len() as u32,
        game_duration: track.duration_seconds(),
        commands: commands.to_vec(),
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
