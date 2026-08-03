use wows_minimap_renderer::draw_command::DrawCommand;
use wows_replays::types::GameClock;

use super::frame_pass::FrameSink;

pub(crate) const PREVIEW_LOOP_SECS: f32 = 10.0;
pub(crate) const PREVIEW_FPS: f32 = 15.0;
pub(crate) const PREVIEW_MAX_FRAMES: usize = (PREVIEW_LOOP_SECS * PREVIEW_FPS) as usize;

pub(crate) struct PreviewTrack {
    pub frames: Vec<Vec<DrawCommand>>,
    pub map_name: String,
}

pub(crate) struct TrackSink {
    frames: Vec<Vec<DrawCommand>>,
    clocks: Vec<GameClock>,
    stride: usize,
    since_kept: usize,
}

impl TrackSink {
    pub(crate) fn new() -> Self {
        Self { frames: Vec::with_capacity(PREVIEW_MAX_FRAMES * 2), clocks: Vec::new(), stride: 1, since_kept: 0 }
    }

    pub(crate) fn stride(&self) -> usize {
        self.stride
    }

    pub(crate) fn kept_clocks(&self) -> &[GameClock] {
        &self.clocks
    }

    pub(crate) fn finish(self, map_name: String) -> PreviewTrack {
        PreviewTrack { frames: self.frames, map_name }
    }

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
        if self.since_kept % self.stride == 0 {
            self.frames.push(commands);
            self.clocks.push(clock);
            if self.frames.len() > PREVIEW_MAX_FRAMES {
                self.halve();
            }
        }
        self.since_kept += 1;
    }
}

#[cfg(test)]
mod track_tests {
    use super::*;

    fn feed(sink: &mut TrackSink, count: usize) {
        for i in 0..count {
            sink.push(i, GameClock(i as f32), Vec::new());
        }
    }

    #[test]
    fn a_short_replay_keeps_every_frame() {
        let mut sink = TrackSink::new();
        feed(&mut sink, 40);
        let track = sink.finish("spaces/test".to_string());
        assert_eq!(track.frames.len(), 40);
    }

    #[test]
    fn a_long_replay_is_decimated_to_the_budget() {
        let mut sink = TrackSink::new();
        feed(&mut sink, 1800);
        let track = sink.finish("spaces/test".to_string());
        assert!(track.frames.len() <= PREVIEW_MAX_FRAMES, "kept {}", track.frames.len());
        assert!(track.frames.len() > PREVIEW_MAX_FRAMES / 2, "kept too few: {}", track.frames.len());
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
        let track = sink.finish("spaces/test".to_string());
        assert!(track.frames.is_empty());
    }
}
