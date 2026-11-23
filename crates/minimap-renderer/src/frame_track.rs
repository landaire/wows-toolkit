//! Turning a replay into the frames a preview plays.
//!
//! The sampling (a bounded, evenly spaced subset of a battle), the render
//! options a preview bakes under, and the pass that drives the world forward
//! emitting draw commands. Shared so both front ends bake the same track;
//! what each wraps around the frames is its own.
//!
//! Drawing those frames is [`crate::preview`].

use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use wows_battle_world::merged::MergedReplays;
use wows_replays::types::GameClock;
use wowsunpack::data::ResourceLoader;

use crate::draw_command::DrawCommand;
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
    /// Frames drawn before this clock are not fed to the track.
    start: Option<GameClock>,
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
            start: None,
        }
    }

    /// Ignores every frame drawn before `start`, so the budget is spent on
    /// the battle rather than the pre-battle countdown.
    pub fn starting_at(self, start: GameClock) -> Self {
        Self { start: Some(start), ..self }
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
        if self.start.is_some_and(|start| clock.seconds() < start.seconds()) {
            return;
        }
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
/// Wider than [`paint_options`], the paint-time preset, so turning one of that
/// preset's switches on does not invalidate cached tracks. It is not everything
/// the renderer can emit: the
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

/// What a hover preview paints out of a track baked with [`bake_options`].
///
/// At 384x384 the canvas scales to about 0.48x, so ship names render around
/// 5px tall: readable at a glance but only just. Player names and the HUD
/// panels stay off to keep the map uncluttered. Shared so every preview
/// shows the same layers.
pub fn paint_options() -> RenderOptions {
    RenderOptions {
        show_stats_panel: false,
        show_team_rosters: false,
        show_kill_feed: false,
        show_chat: false,
        show_player_names: false,
        show_ship_names: true,
        show_dead_ship_names: false,
        show_ship_config: false,
        show_battle_result: false,
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

/// Whether the forward pass records every ship's position at each packet step for trails, as
/// the egui playback does, rather than only at the frames it draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrailRecording {
    /// The frames drawn are all the pass keeps.
    Skip,
    /// The renderer's trail history gains the visible ships at every step, for
    /// [`MinimapRenderer::take_trails`].
    EveryStep,
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
    trails: TrailRecording,
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
                // As the egui playback steps: from the first nonzero clock, labelled with it.
                if trails == TrailRecording::EveryStep && prev_clock.seconds() > 0.0 {
                    renderer.record_visible_positions(&view, prev_clock);
                }
            }

            let target_frame = (prev_clock.seconds() / frame_duration) as i64;
            // The world has already taken the first packet at `step`; drawing at the clock the
            // frame is labelled with keeps the timer and positions on one base across frames.
            renderer.set_render_clock(prev_clock);
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
        vec![DrawCommand::Timer {
            time_remaining: Some(index as i64),
            elapsed: ElapsedClock(index as f32),
            remaining: None,
        }]
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
    fn a_track_started_at_the_battle_spends_its_budget_on_the_battle() {
        let countdown = 30;
        let mut sink = TrackSink::new().starting_at(GameClock(countdown as f32));
        feed(&mut sink, countdown + PREVIEW_MAX_FRAMES);
        assert_eq!(sink.kept_clocks()[0], GameClock(countdown as f32));
        assert_eq!(sink.stride(), 1, "countdown frames were counted against the budget");
        assert_eq!(sink.finish().len(), PREVIEW_MAX_FRAMES);
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
