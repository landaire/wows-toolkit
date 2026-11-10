//! A battle someone else in the session is playing back.
//!
//! A peer with no copy of the replay cannot open it, so the end that has it
//! sends what it draws. A frame on that wire is a list of draw commands, so
//! this draws the battle with its own art at its own size. It does not steer:
//! the clock belongs to the end that owns the replay, and this reports where
//! that end has reached.

use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use nsh_core::prelude::*;
use tokio::sync::watch;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_minimap_renderer::viewport::MapViewport;
use wows_replays::types::GameClock;
use wows_toolkit_replay_data::minimap;
use wt_collab_client::controller::SharedWindow;

use crate::frames;
use crate::frames::PixelSize;
use crate::frames::Surface;
use crate::frames::SurfaceId;
use crate::renderer::ids::FrameIndex;
use crate::renderer::ids::RendererId;
use crate::renderer::ids::Revision;
use crate::renderer::raster;

/// How often the frames the session has sent are looked for. A plain wait
/// on the channel: watching someone else's playback is worth a tenth of a
/// second of lateness.
const DRAIN: Duration = Duration::from_millis(100);

/// How many frames the session may hold for this view. The far end drops
/// rather than blocks, so this is a cushion against a slow raster and not a
/// queue to play out later.
const DEPTH: usize = 4;

/// What the end that owns the replay says about where it has reached.
#[derive(Clone, Copy, Debug, PartialEq)]
struct HostClock {
    clock: GameClock,
    frame: FrameIndex,
    /// How many frames the battle has; `None` before the host has said, which
    /// is not the same as a battle with none.
    total: Option<NonZeroUsize>,
}

impl HostClock {
    /// How far through the battle it is.
    fn through(self) -> Option<f32> {
        let total = self.total?;
        Some((self.frame.get() as f32 / total.get() as f32).clamp(0.0, 1.0))
    }

    fn label(self) -> String {
        let seconds = self.clock.seconds().max(0.0);
        format!("{}:{:02}", (seconds / 60.0).floor() as i32, (seconds % 60.0) as i32)
    }
}

/// What the view of a watched window waits on.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct WatchedState {
    revision: Revision,
    clock: Option<HostClock>,
    /// Whether a frame has been drawn yet.
    drawn: bool,
}

/// The watched battle, as the view shows it.
#[derive(Debug, Clone, PartialEq, JsObject)]
pub struct WatchedView {
    pub id: RendererId,
    pub surface: String,
    /// Changes whenever anything here does.
    pub revision: Revision,
    pub title: String,
    /// Where the host has reached, as the game's clock; `None` before a frame
    /// has arrived.
    pub clock: Option<String>,
    /// How far through the battle that is.
    pub through: Option<f64>,
    /// Whether a frame has been drawn yet.
    pub drawn: bool,
}

/// One watched window and the thread drawing it.
pub struct Watched {
    id: RendererId,
    surface: Arc<Surface>,
    title: String,
    state: watch::Sender<WatchedState>,
    cancel: Arc<AtomicBool>,
}

impl Watched {
    /// Starts drawing the session's window `window` on the surface `name`.
    pub fn open(id: RendererId, name: &str, window: SharedWindow) -> Arc<Self> {
        let surface = frames::surface(&SurfaceId::new(name));
        let watched = Arc::new(Self {
            id,
            surface,
            title: window.replay_name.clone(),
            state: watch::channel(WatchedState::default()).0,
            cancel: Arc::new(AtomicBool::new(false)),
        });
        let drawing = Arc::clone(&watched);
        std::thread::Builder::new()
            .name("watched-playback".to_owned())
            .spawn(move || drawing.run(window))
            .expect("the watched playback thread starts");

        watched
    }

    /// The view closed: the session stops sending frames nobody draws.
    pub fn close(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn view(&self) -> WatchedView {
        let state = *self.state.borrow();
        WatchedView {
            id: self.id,
            surface: self.surface.id().as_str().to_owned(),
            revision: state.revision,
            title: self.title.clone(),
            clock: state.clock.map(HostClock::label),
            through: state.clock.and_then(HostClock::through).map(f64::from),
            drawn: state.drawn,
        }
    }

    pub async fn next_change(&self, after: Revision) -> WatchedView {
        let mut changes = self.state.subscribe();
        let _ = changes.wait_for(|state| state.revision != after).await;
        self.view()
    }

    fn run(self: Arc<Self>, window: SharedWindow) {
        let replay_id = window.replay_id;
        let link = crate::collab::link();
        let (tx, frames) = std::sync::mpsc::sync_channel(DEPTH);
        link.watch_window(replay_id, tx);
        let art = window.art_png.as_ref().and_then(|png| match image::load_from_memory(png) {
            Ok(art) => Some(art.to_rgb8()),
            // Art that will not decode leaves this drawing with the build's
            // own, which is what a peer that has the map does anyway.
            Err(err) => {
                log::warn!("watched playback: the art the host sent could not be read: {err}");
                None
            }
        });
        let mut input = self.surface.take_input();

        let mut latest: Option<Vec<DrawCommand>> = None;
        let mut drawn: Option<PixelSize> = None;
        while !self.cancel.load(Ordering::Relaxed) {
            if let Some(input) = input.as_mut() {
                while input.try_recv().is_ok() {}
            }
            // Only the newest: the others are moments the host has left.
            let mut newest = frames.recv_timeout(DRAIN).ok();
            while let Ok(frame) = frames.try_recv() {
                newest = Some(frame);
            }
            if let Some(frame) = newest {
                let clock = HostClock {
                    clock: frame.clock,
                    frame: FrameIndex::new(frame.frame_index),
                    // The host sends no frame count before it knows one.
                    total: NonZeroUsize::new(frame.total_frames),
                };
                self.state.send_modify(|state| {
                    state.clock = Some(clock);
                    state.revision = state.revision.next();
                });
                latest = Some(frame.commands);
                drawn = None;
            }

            let Some(view) = self.surface.view_size() else { continue };
            let size = view.size;
            let Some(commands) = latest.as_ref() else { continue };
            if drawn == Some(size) {
                continue;
            }
            let Some(game_data) = crate::game_data::cache() else { continue };
            let map = match &art {
                Some(art) => minimap::render_map_art(
                    &format!("watched-{}", replay_id.raw()),
                    art,
                    &game_data,
                    MapViewport::default(),
                    commands,
                ),
                None => minimap::render_map(&window.map_name, &game_data, MapViewport::default(), commands),
            };
            // Nothing to draw the commands over: the fonts and icons a frame
            // needs are this build's own.
            let Some(map) = map else { continue };
            match raster::fit(&map, size, raster::MARGIN).and_then(|pixmap| raster::finish(pixmap, size)) {
                Ok(frame) => {
                    self.surface.present(frame);
                    drawn = Some(size);
                    self.state.send_if_modified(|state| {
                        if state.drawn {
                            return false;
                        }
                        state.drawn = true;
                        state.revision = state.revision.next();
                        true
                    });
                }
                Err(err) => log::warn!("watched playback: a frame could not be built: {err}"),
            }
        }

        link.stop_watching(replay_id);
        self.surface.clear();
        frames::release(self.surface.id());
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;

    use wows_replays::types::GameClock;

    use super::HostClock;
    use crate::renderer::ids::FrameIndex;

    /// How far through reads as a fraction, and says nothing at all before the
    /// host has said how long the battle is.
    #[test]
    fn progress_needs_a_length_to_be_a_fraction() {
        let unknown = HostClock { clock: GameClock(12.0), frame: FrameIndex::new(3), total: None };
        assert_eq!(unknown.through(), None);
        let half = HostClock { clock: GameClock(75.5), frame: FrameIndex::new(50), total: NonZeroUsize::new(100) };
        assert_eq!(half.through(), Some(0.5));
        assert_eq!(half.label(), "1:15");
    }
}
