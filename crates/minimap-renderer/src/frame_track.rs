//! Turning a replay into the frames a preview plays.
//!
//! The sampling (a bounded, evenly spaced subset of a battle), the render
//! options a preview bakes under, and the pass that drives the world forward
//! emitting draw commands. Shared so both front ends bake the same track;
//! what each wraps around the frames is its own.
//!
//! Drawing those frames is [`crate::preview`].

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use wows_battle_world::merged::MergedReplays;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wowsunpack::data::ResourceLoader;

use crate::draw_command::DrawCommand;
use crate::map_data::MinimapPos;
use crate::renderer::MinimapRenderer;
use crate::renderer::RenderOptions;

/// How often the forward pass takes a snapshot of the battle. The bake then
/// keeps an evenly spaced subset of those.
pub const SNAPSHOTS_PER_SECOND: f32 = 1.5;

/// Wall-clock length of one full preview loop for a replay long enough to
/// fill the frame budget.
pub const PREVIEW_LOOP_SECS: f32 = 10.0;
/// Display rate a preview advances the track at.
pub const PREVIEW_FPS: f32 = 15.0;
/// Frames retained per track. Battles shorter than
/// `PREVIEW_MAX_FRAMES / SNAPSHOTS_PER_SECOND` seconds keep fewer and loop
/// proportionally sooner, which is correct: a one-minute battle should not be
/// stretched to ten seconds.
pub const PREVIEW_MAX_FRAMES: usize = (PREVIEW_LOOP_SECS * PREVIEW_FPS) as usize;

/// Retains a bounded, evenly spaced subset of the frames it is fed.
pub struct TrackSink {
    frames: Vec<Vec<DrawCommand>>,
    /// The clock each retained frame arrived with. A hover preview plays at a
    /// fixed display rate and never reads these; a playback viewport labels
    /// its seek bar with them.
    clocks: Vec<GameClock>,
    /// The most frames this sink will retain. Reaching it halves the track
    /// and doubles the stride, so a long battle costs the same as a short one.
    budget: usize,
    /// Keep one frame in every `stride`. Doubles each time the budget fills.
    stride: usize,
    /// Source frames seen since the last frame was retained.
    since_kept: usize,
}

impl Default for TrackSink {
    fn default() -> Self {
        Self::new()
    }
}

impl TrackSink {
    /// A sink sized for the hover preview's looping track.
    pub fn new() -> Self {
        Self::with_budget(PREVIEW_MAX_FRAMES)
    }

    /// A sink that retains up to `budget` frames.
    ///
    /// A playback viewport wants far more of the battle than a hover preview
    /// does, and is the only reason this is a parameter.
    pub fn with_budget(budget: usize) -> Self {
        Self {
            frames: Vec::with_capacity(budget.saturating_mul(2).min(4096)),
            clocks: Vec::new(),
            budget: budget.max(1),
            stride: 1,
            since_kept: 0,
        }
    }

    #[cfg(test)]
    pub fn stride(&self) -> usize {
        self.stride
    }

    /// The clock each retained frame was drawn at, oldest first.
    pub fn kept_clocks(&self) -> &[GameClock] {
        &self.clocks
    }

    /// The retained frames, oldest first.
    ///
    /// What a front end wraps around them -- the map art, which build they
    /// were baked against -- is its own, so the sink hands back only what it
    /// sampled.
    pub fn finish(self) -> Vec<Vec<DrawCommand>> {
        self.frames
    }

    /// Drop every other retained frame, halving the track in place.
    fn halve(&mut self) {
        let mut keep = false;
        self.frames.retain(|_| {
            keep = !keep;
            keep
        });
        let mut keep = false;
        self.clocks.retain(|_| {
            keep = !keep;
            keep
        });
        self.stride *= 2;
    }
}

impl FrameSink for TrackSink {
    fn push(&mut self, _index: usize, clock: GameClock, commands: Vec<DrawCommand>) {
        if self.since_kept.is_multiple_of(self.stride) {
            self.frames.push(commands);
            self.clocks.push(clock);
            if self.frames.len() > self.budget {
                self.halve();
            }
        }
        self.since_kept += 1;
    }
}

/// What a preview track is baked with.
///
/// Wider than [`preview_options`](crate::ui::replay_parser::preview_popup::preview_options),
/// the paint-time preset, so turning one of that preset's switches on does not
/// invalidate cached tracks. It is not everything the renderer can emit: the
/// panel commands (stats, rosters) carry a full roster per frame and position
/// trails carry the whole match's position history per ship per frame, so
/// baking either would cost tens of megabytes for one track. Widening this set
/// changes what a cached track contains, so a track baked before the change
/// cannot serve a preset that asks for the new class; the popup's presets are
/// checked against this set by `paint_preset_asks_for_nothing_unbaked`.
pub fn bake_options() -> RenderOptions {
    RenderOptions {
        show_kill_feed: true,
        show_chat: true,
        show_armament: true,
        show_stats_panel: false,
        show_team_rosters: false,
        show_ship_config: false,
        show_trails: false,
        show_speed_trails: false,
        ..RenderOptions::default()
    }
}

/// Bake a decimated preview track for `path` in a single forward pass.
///
/// Mirrors the setup `playback_thread` does before its own forward pass, then
/// skips everything else that thread does afterward: damage-event gathering,
/// timeline/shot extraction, salvo flight-time scanning, player-build
/// snapshotting, the live-session rebuild, silhouette loading,
/// `commands.scheme.xml` parsing, and the collab announce. That is the entire
/// reason this finishes cheaply enough to run on hover.
///
/// Cancellation is cooperative, and the checks before `data_map.resolve` matter
/// most: resolving an unloaded build is a full game-data load, so a bake that
/// has already been superseded must not enter one.
/// The clock a rendered frame was drawn at. Merged sessions seek by rebuilding
/// and stepping to a target clock, so a clock is all a seek hint needs.
pub struct FrameSnapshot {
    pub clock: GameClock,
}

/// Receives every frame the forward pass draws.
///
/// Playback keeps only the first frame; the preview baker keeps a decimated
/// track. Both drive the same walk so their frame boundaries cannot diverge.
pub trait FrameSink {
    fn push(&mut self, index: usize, clock: GameClock, commands: Vec<DrawCommand>);
}

/// Step the session to exhaustion, drawing one frame per `frame_duration` of
/// game time and handing each to `sink`.
///
/// Returns the clock of every frame drawn. Bails early when `cancel` is set,
/// leaving the snapshots collected so far. Checked before every `step()`,
/// before every `draw_frame()` inside the catch-up burst a single `step()`
/// can trigger, and inside the trailing final-tick drain, so a cancel lands
/// within one frame no matter which of those a single call is stalled in.
pub fn build_frame_track<G: ResourceLoader>(
    session: &mut MergedReplays<'_, '_, G>,
    renderer: &mut MinimapRenderer<'_>,
    frame_duration: f32,
    cancel: &AtomicBool,
    sink: &mut dyn FrameSink,
) -> Vec<FrameSnapshot> {
    let mut snapshots: Vec<FrameSnapshot> = Vec::new();
    let mut last_rendered_frame: i64 = -1;
    let mut prev_clock = GameClock(0.0);

    loop {
        if cancel.load(Ordering::Relaxed) {
            return snapshots;
        }
        let step = match session.step() {
            Ok(Some(c)) => c,
            Ok(None) => break,
            Err(e) => {
                tracing::error!("merge step failed during frame pass: {e}");
                break;
            }
        };
        if step.0 > prev_clock.0 {
            {
                let view = session.world_mut().view();
                renderer.populate_players(&view);
                renderer.update_squadron_info(&view);
                renderer.update_ship_abilities(&view);
            }

            let target_frame = (prev_clock.seconds() / frame_duration) as i64;
            while last_rendered_frame < target_frame {
                if cancel.load(Ordering::Relaxed) {
                    return snapshots;
                }
                last_rendered_frame += 1;
                let view = session.world_mut().view();
                let commands = renderer.draw_frame(&view);
                let index = snapshots.len();
                snapshots.push(FrameSnapshot { clock: prev_clock });
                sink.push(index, prev_clock, commands);
            }
            prev_clock = step;
        }
    }

    // The trailing partial frame carries no new draw, only a clock, so the
    // seek track stays dense to the end of the replay.
    if prev_clock.seconds() > 0.0 {
        let view = session.world_mut().view();
        renderer.populate_players(&view);
        renderer.update_squadron_info(&view);
        renderer.update_ship_abilities(&view);
        let target_frame = (prev_clock.seconds() / frame_duration) as i64;
        while last_rendered_frame < target_frame {
            if cancel.load(Ordering::Relaxed) {
                return snapshots;
            }
            last_rendered_frame += 1;
            snapshots.push(FrameSnapshot { clock: prev_clock });
        }
    }

    snapshots
}

#[cfg(test)]
mod track_tests {
    use super::*;
    use crate::draw_command::DrawCommand;
    use wows_replays::types::ElapsedClock;

    fn feed(sink: &mut TrackSink, count: usize) {
        for i in 0..count {
            sink.push(i, GameClock(i as f32), Vec::new());
        }
    }

    fn tagged(index: usize) -> Vec<DrawCommand> {
        vec![DrawCommand::Timer { time_remaining: Some(index as i64), elapsed: ElapsedClock(index as f32) }]
    }

    #[test]
    fn a_short_replay_keeps_every_frame() {
        let mut sink = TrackSink::new();
        feed(&mut sink, 40);
        let frames = sink.finish();
        assert_eq!(frames.len(), 40);
    }

    #[test]
    fn a_long_replay_is_decimated_to_the_budget() {
        let mut sink = TrackSink::new();
        feed(&mut sink, 1800);
        let frames = sink.finish();
        assert!(frames.len() <= PREVIEW_MAX_FRAMES, "kept {}", frames.len());
        assert!(frames.len() > PREVIEW_MAX_FRAMES / 2, "kept too few: {}", frames.len());
    }

    #[test]
    fn decimation_keeps_the_first_frame_and_reaches_the_end() {
        let mut sink = TrackSink::new();
        feed(&mut sink, 1800);
        assert_eq!(sink.kept_clocks()[0], GameClock(0.0));
        let last = *sink.kept_clocks().last().expect("a kept frame");
        assert!(last.0 >= 1800.0 - sink.stride() as f32, "last kept clock {last:?}");
    }

    #[test]
    fn an_empty_replay_yields_an_empty_track() {
        let sink = TrackSink::new();
        let frames = sink.finish();
        assert!(frames.is_empty());
    }

    #[test]
    fn every_retained_frame_keeps_the_clock_it_arrived_with() {
        let mut sink = TrackSink::new();
        for i in 0..1800 {
            sink.push(i, GameClock(i as f32), tagged(i));
        }
        let clocks: Vec<GameClock> = sink.kept_clocks().to_vec();
        let frames = sink.finish();
        assert_eq!(frames.len(), clocks.len(), "frames and clocks diverged");
        for (frame, clock) in frames.iter().zip(clocks.iter()) {
            let DrawCommand::Timer { time_remaining: Some(index), .. } = frame[0] else {
                panic!("expected a tagged Timer frame");
            };
            assert_eq!(index as f32, clock.0, "frame {index} was paired with clock {}", clock.0);
        }
    }
}

/// A hue on the blue-to-red ramp a trail is coloured along.
pub fn hue_to_rgb(hue: f32) -> [u8; 3] {
    let h = hue / 60.0;
    let i = h.floor() as i32;
    let f = h - i as f32;
    let q = (1.0 - f) * 255.0;
    let t = f * 255.0;
    match i % 6 {
        0 => [255, t as u8, 0],
        1 => [q as u8, 255, 0],
        2 => [0, 255, t as u8],
        3 => [0, q as u8, 255],
        4 => [t as u8, 0, 255],
        _ => [255, 0, q as u8],
    }
}

/// Where each ship has been, read back out of the frames already baked.
///
/// A trail command carries every point so far, so baking one per frame would
/// cost the square of the track's length. The positions are already in the
/// track's own ship commands, so a viewport derives the trails for the frame
/// it is about to draw instead, at the cost of one walk of the frames behind
/// it.
///
/// Oldest points are blue and newest red, as the renderer's own trails are.
pub fn trails_through(frames: &[Vec<DrawCommand>]) -> Vec<DrawCommand> {
    let mut order: Vec<EntityId> = Vec::new();
    let mut tracks: HashMap<EntityId, (Option<String>, Vec<MinimapPos>)> = HashMap::new();

    for commands in frames {
        for command in commands {
            let (entity_id, pos, player_name) = match command {
                DrawCommand::Ship { entity_id, pos, player_name, .. } => (entity_id, pos, player_name.clone()),
                // A sunk ship keeps the trail it made while it was alive.
                DrawCommand::DeadShip { entity_id, pos, player_name, .. } => (entity_id, pos, player_name.clone()),
                _ => continue,
            };
            let entry = tracks.entry(*entity_id).or_insert_with(|| {
                order.push(*entity_id);
                (None, Vec::new())
            });
            if entry.0.is_none() {
                entry.0 = player_name;
            }
            // A ship that has not moved since the last frame adds nothing.
            if entry.1.last().map(|last| (last.x, last.y)) != Some((pos.x, pos.y)) {
                entry.1.push(*pos);
            }
        }
    }

    order
        .into_iter()
        .filter_map(|entity_id| {
            let (player_name, points) = tracks.remove(&entity_id)?;
            // One point is a position, not a trail.
            if points.len() < 2 {
                return None;
            }
            let last = points.len() - 1;
            let points = points
                .into_iter()
                .enumerate()
                .map(|(index, pos)| (pos, hue_to_rgb(240.0 * (1.0 - index as f32 / last as f32))))
                .collect();
            Some(DrawCommand::PositionTrail { entity_id, player_name, points })
        })
        .collect()
}

#[cfg(test)]
mod trail_tests {
    use super::*;
    use crate::draw_command::ShipVisibility;

    fn ship(entity: u32, x: f32, y: f32) -> DrawCommand {
        DrawCommand::Ship {
            entity_id: EntityId::from(entity),
            pos: MinimapPos { x, y },
            yaw: 0.0,
            species: None,
            color: None,
            visibility: ShipVisibility::Visible,
            opacity: 1.0,
            is_self: false,
            player_name: Some(format!("player{entity}")),
            ship_name: None,
            is_detected_teammate: false,
            is_disconnected: false,
            name_color: None,
        }
    }

    /// A trail is where a ship has been across the frames behind this one.
    #[test]
    fn a_trail_follows_one_ship_through_the_frames() {
        let frames = vec![
            vec![ship(1, 0.0, 0.0), ship(2, 100.0, 100.0)],
            vec![ship(1, 10.0, 0.0), ship(2, 110.0, 100.0)],
            vec![ship(1, 20.0, 0.0)],
        ];

        let trails = trails_through(&frames);
        assert_eq!(trails.len(), 2, "one per ship that moved");

        let DrawCommand::PositionTrail { points, player_name, .. } = &trails[0] else {
            panic!("expected a trail, got {:?}", trails[0]);
        };
        assert_eq!(player_name.as_deref(), Some("player1"));
        assert_eq!(points.len(), 3, "one point per frame it moved in");
        assert_eq!(points[0].0.x, 0.0);
        assert_eq!(points[2].0.x, 20.0);
        assert_ne!(points[0].1, points[2].1, "oldest and newest are coloured differently");
    }

    /// A ship that has not moved is a position, not a trail.
    #[test]
    fn a_ship_that_never_moved_has_no_trail() {
        let frames = vec![vec![ship(1, 5.0, 5.0)], vec![ship(1, 5.0, 5.0)], vec![ship(1, 5.0, 5.0)]];
        assert!(trails_through(&frames).is_empty());
    }

    /// Deriving costs one walk of the frames behind, where baking a trail per
    /// frame would have cost the square of the track's length.
    #[test]
    fn deriving_costs_one_walk_rather_than_a_point_per_frame_per_frame() {
        let frames: Vec<Vec<DrawCommand>> = (0..200).map(|step| vec![ship(1, step as f32, 0.0)]).collect();

        let derived = trails_through(&frames);
        let DrawCommand::PositionTrail { points, .. } = &derived[0] else { panic!("expected a trail") };
        assert_eq!(points.len(), 200);

        // Baked, every frame would have carried its own copy of the trail so
        // far, which is what this avoids.
        let baked_points: usize = (1..=200).sum();
        assert!(points.len() < baked_points / 100, "{} against {}", points.len(), baked_points);
    }
}
