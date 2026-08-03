use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use wows_battle_world::merged::MergedReplays;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_minimap_renderer::renderer::MinimapRenderer;
use wows_replays::types::GameClock;
use wowsunpack::data::ResourceLoader;

/// The clock a rendered frame was drawn at. Merged sessions seek by rebuilding
/// and stepping to a target clock, so a clock is all a seek hint needs.
pub(crate) struct FrameSnapshot {
    pub clock: GameClock,
}

/// Receives every frame the forward pass draws.
///
/// Playback keeps only the first frame; the preview baker keeps a decimated
/// track. Both drive the same walk so their frame boundaries cannot diverge.
pub(crate) trait FrameSink {
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
pub(crate) fn build_frame_track<G: ResourceLoader>(
    session: &mut MergedReplays<'_, '_, '_, G>,
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
