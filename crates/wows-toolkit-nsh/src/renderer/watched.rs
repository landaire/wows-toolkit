//! A battle someone else in the session is playing back.
//!
//! A peer with no copy of the replay cannot open it, so the end that has it
//! sends what it draws. A frame on that wire is a list of draw commands, so
//! this draws the battle with its own art at its own size. It does not steer:
//! the clock belongs to the end that owns the replay, and this reports where
//! that end has reached.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use image::RgbImage;
use nsh_core::prelude::*;
use tokio::sync::watch;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_minimap_renderer::viewport::MapViewport;
use wows_toolkit_replay_data::minimap;
use wt_collab_client::controller::SharedWindow;
use wt_collab_client::ids::ReplayId;

use crate::frames;
use crate::frames::PixelSize;
use crate::frames::Surface;
use crate::frames::SurfaceId;
use crate::renderer::raster;
use crate::renderer::services;

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
    seconds: f32,
    frame: usize,
    of: usize,
}

impl HostClock {
    /// How far through the battle it is. `None` before the host has said how
    /// many frames there are, which is not the same as a battle with none.
    fn through(self) -> Option<f32> {
        (self.of > 0).then(|| (self.frame as f32 / self.of as f32).clamp(0.0, 1.0))
    }

    fn label(self) -> String {
        let seconds = self.seconds.max(0.0);
        format!("{}:{:02}", (seconds / 60.0).floor() as i32, (seconds % 60.0) as i32)
    }
}

/// The watched battle, as the view shows it.
#[derive(Debug, Clone, PartialEq, JsObject)]
pub struct WatchedView {
    pub id: u32,
    pub surface: String,
    /// Changes whenever anything here does.
    pub revision: u32,
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
    id: u32,
    surface: Arc<Surface>,
    title: String,
    state: watch::Sender<(u32, Option<HostClock>, bool)>,
    cancel: Arc<AtomicBool>,
}

impl Watched {
    /// Starts drawing the session's window `window` on the surface `name`.
    pub fn open(id: u32, name: &str, window: SharedWindow) -> Arc<Self> {
        let surface = frames::surface(&SurfaceId::new(name));
        let watched = Arc::new(Self {
            id,
            surface,
            title: window.replay_name.clone(),
            state: watch::channel((0, None, false)).0,
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
        let (revision, clock, drawn) = *self.state.borrow();
        WatchedView {
            id: self.id,
            surface: self.surface.id().as_str().to_owned(),
            revision,
            title: self.title.clone(),
            clock: clock.map(HostClock::label),
            through: clock.and_then(HostClock::through).map(f64::from),
            drawn,
        }
    }

    pub async fn next_change(&self, after: u32) -> WatchedView {
        let mut changes = self.state.subscribe();
        let _ = changes.wait_for(|(revision, _, _)| *revision != after).await;
        self.view()
    }

    fn run(self: Arc<Self>, window: SharedWindow) {
        let replay_id = ReplayId::new(window.replay_id);
        let link = services::collab_link();
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
        let mut drawn: Option<(RgbImage, PixelSize)> = None;
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
                let clock = HostClock { seconds: frame.clock.seconds(), frame: frame.frame_index, of: frame.total_frames };
                self.state.send_modify(|(revision, held, _)| {
                    *held = Some(clock);
                    *revision = revision.wrapping_add(1);
                });
                latest = Some(frame.commands);
                drawn = None;
            }

            let Some((size, _)) = self.surface.view_size() else { continue };
            let Some(commands) = latest.as_ref() else { continue };
            if drawn.as_ref().is_some_and(|(_, at)| *at == size) {
                continue;
            }
            let map = match &art {
                Some(art) => minimap::render_map_art(
                    &format!("watched-{}", replay_id.raw()),
                    art,
                    &services::game_data(),
                    MapViewport::default(),
                    commands,
                ),
                None => minimap::render_map(&window.map_name, &services::game_data(), MapViewport::default(), commands),
            };
            // Nothing to draw the commands over: the fonts and icons a frame
            // needs are this build's own.
            let Some(map) = map else { continue };
            match raster::fit(&map, size, raster::MARGIN).and_then(|pixmap| raster::finish(pixmap, size)) {
                Ok(frame) => {
                    self.surface.present(frame);
                    drawn = Some((map, size));
                    self.state.send_if_modified(|(revision, _, shown)| {
                        if *shown {
                            return false;
                        }
                        *shown = true;
                        *revision = revision.wrapping_add(1);
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
    use super::HostClock;

    /// How far through reads as a fraction, and says nothing at all before the
    /// host has said how long the battle is.
    #[test]
    fn progress_needs_a_length_to_be_a_fraction() {
        let unknown = HostClock { seconds: 12.0, frame: 3, of: 0 };
        assert_eq!(unknown.through(), None);
        let half = HostClock { seconds: 75.5, frame: 50, of: 100 };
        assert_eq!(half.through(), Some(0.5));
        assert_eq!(half.label(), "1:15");
    }
}
