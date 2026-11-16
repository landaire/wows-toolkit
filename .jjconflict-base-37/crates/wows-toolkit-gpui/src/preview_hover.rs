//! Hovering a replay row plays its battle back on the minimap.
//!
//! The rule for when that is worth doing is the shared one
//! (`wows_toolkit_viewmodel::preview_dwell`); what this adds is the gpui half
//! of it: real elapsed time, the background bake, and the ticker that plays
//! the baked track. The replay listing and the Search tab both drive it, so a
//! preview behaves the same wherever it is hovered.
//!
//! gpui reports that the pointer entered or left an element, never how long it
//! has rested there, so the dwell is measured against the clock: entering a
//! row starts a timer for the shared delay, and the row is only baked if the
//! pointer is still on it when the timer fires.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use gpui_kit::App;
use gpui_kit::Context;
use gpui_kit::Render;
use gpui_kit::SharedString;
use gpui_kit::Task;
use wows_toolkit_replay_data::load::GameDataCache;
use wows_toolkit_viewmodel::preview_dwell::DWELL;
use wows_toolkit_viewmodel::preview_dwell::Dwell;

use crate::frame_texture::FrameTexture;
use wows_toolkit_replay_data::minimap_preview::FRAME_INTERVAL;
use wows_toolkit_replay_data::minimap_preview::PreviewEdge;
use wows_toolkit_replay_data::minimap_preview::PreviewFrames;

/// A baked preview and where it is in its loop.
struct Shown {
    frames: PreviewFrames,
    started: Instant,
    /// Whether this is the still map standing in for a track that has not
    /// arrived. The map goes up as soon as there is one, so without this the
    /// spinner would come down the moment it did and the reader would watch a
    /// motionless map with nothing saying more is coming.
    map_only: bool,
    /// Which frame of the track the texture holds, so a tick that lands on
    /// the same frame uploads nothing.
    presented: Option<usize>,
}

/// The hover state behind one surface's preview: what is being watched, what
/// has been baked, and the tasks driving both.
pub struct PreviewHover {
    dwell: Dwell<PathBuf>,
    /// The row under the pointer and when it got there, so the dwell is fed
    /// real elapsed time rather than a guess at how often gpui reports a
    /// hover.
    watched: Option<(PathBuf, Instant)>,
    shown: Option<Shown>,
    /// The current frame of whatever track is shown, written in place on
    /// every tick.
    texture: FrameTexture,
    /// Whether a bake is in flight, so the surface can say the preview is
    /// coming rather than showing nothing for the seconds it takes to read
    /// the replay and load the build it was recorded on. Stays set while the
    /// map is shown on its own, which is most of that time.
    baking: bool,
    /// Whether a preview is on its way at all: the dwell is counting down, or a
    /// bake is running. Set apart from `baking` because the wait a reader sees
    /// starts before the bake does, and the space the map will take has to be
    /// held for the whole of it or the popup jumps when the map lands.
    ///
    /// Cleared once a bake has finished, so a replay whose build is not
    /// installed stops holding space it will never fill.
    expecting: bool,
    /// Why the watched row has no preview, once a bake has failed.
    ///
    /// Kept so the popup can say it: a replay recorded on a build that is not
    /// installed is the ordinary case here, and a blank square reads as a broken
    /// preview rather than as a missing build.
    failed: Option<SharedString>,
    /// The physical size frames are rasterised at, from the window the
    /// preview is drawn in (`sync_with_window`).
    edge: PreviewEdge,
    /// Set when a bake in flight should stop: the pointer has moved on.
    cancel: Arc<AtomicBool>,
    _bake: Option<Task<()>>,
    _dwell_timer: Option<Task<()>>,
    _ticker: Option<Task<()>>,
}

/// What a preview occupies before its first frame arrives.
///
/// Ocean's water, which is the one map that is nothing but water: every pixel of
/// `spaces/00_CO_ocean` is this colour. A preview that starts as open water and
/// then becomes the real map does not jump the way one that starts as a hole in
/// the panel does.
pub const MAP_PLACEHOLDER: gpui_kit::Rgba =
    gpui_kit::Rgba { r: 0x06 as f32 / 255., g: 0x27 as f32 / 255., b: 0x2e as f32 / 255., a: 1. };

impl Default for PreviewHover {
    fn default() -> Self {
        Self {
            dwell: Dwell::new(),
            watched: None,
            shown: None,
            texture: FrameTexture::new(),
            baking: false,
            expecting: false,
            failed: None,
            // Corrected by the owning view's first render, which comes before
            // any row can be hovered.
            edge: PreviewEdge::for_scale_factor(1.0),
            cancel: Arc::new(AtomicBool::new(false)),
            _bake: None,
            _dwell_timer: None,
            _ticker: None,
        }
    }
}

impl PreviewHover {
    /// The texture to paint, if a preview has a frame to show.
    pub fn texture(&self) -> Option<&FrameTexture> {
        let shown = self.shown.as_ref()?;
        shown.presented.map(|_| &self.texture)
    }

    /// Takes the window's scale factor for the frames baked next.
    ///
    /// Called from the owning view's `render`, which is where a `Window` is
    /// at hand.
    pub fn sync_with_window(&mut self, window: &mut gpui_kit::Window) {
        self.edge = PreviewEdge::for_scale_factor(window.scale_factor());
    }

    /// The row under the pointer, whether or not its preview has baked yet.
    pub fn watched_path(&self) -> Option<&std::path::Path> {
        self.watched.as_ref().map(|(path, _)| path.as_path())
    }

    /// Why the watched row has no preview, if a bake has said so.
    pub fn failure(&self) -> Option<&SharedString> {
        self.failed.as_ref()
    }

    /// Whether a preview is on its way for the watched row, dwell included.
    ///
    /// What a surface holds the map's space on: a popup that waited for the
    /// first frame would go up without a map and then grow around one.
    pub fn awaits_preview(&self) -> bool {
        self.expecting
    }

    /// Whether the reader is still waiting for the preview to start playing.
    ///
    /// Only before the first frame: a bake keeps running behind a preview that
    /// is already playing, and a spinner raised each time it falls behind
    /// playback blinks over the map.
    pub fn awaits_first_frame(&self) -> bool {
        if !self.baking {
            return false;
        }
        match &self.shown {
            None => true,
            // The still map is not a preview, so it is shown under a spinner
            // for as long as it stands in for one.
            Some(shown) => shown.map_only,
        }
    }

    /// Whether a row is under the pointer at all, dwelled or not.
    #[cfg(test)]
    pub(crate) fn is_watching(&self) -> bool {
        self.dwell.is_watching()
    }

    /// Seeds a bake in flight that has produced nothing yet. Test-only.
    #[cfg(test)]
    pub(crate) fn seed_baking_for_test(&mut self, path: PathBuf) {
        self.baking = true;
        self.expecting = true;
        self.shown = None;
        self.watched = Some((path, Instant::now()));
    }

    /// Seeds a bake that finished with nothing to show, and why. Test-only.
    #[cfg(test)]
    pub(crate) fn seed_failure_for_test(&mut self, path: PathBuf, reason: &str) {
        self.expecting = false;
        self.baking = false;
        self.shown = None;
        self.failed = Some(SharedString::from(reason.to_owned()));
        self.watched = Some((path, Instant::now()));
    }

    /// Seeds a row being dwelled on, before any bake. Test-only.
    #[cfg(test)]
    pub(crate) fn seed_dwelling_for_test(&mut self, path: PathBuf) {
        self.expecting = true;
        self.shown = None;
        self.watched = Some((path, Instant::now()));
    }

    /// Seeds a preview that is playing while its bake runs on. Test-only.
    #[cfg(test)]
    pub(crate) fn seed_playing_for_test(&mut self, frames: PreviewFrames) {
        self.baking = true;
        // A track, not the still map: this seeds the playing case.
        self.shown = Some(Shown { frames, started: Instant::now(), map_only: false, presented: None });
    }

    /// How many frames the current preview has. Test-only.
    #[cfg(test)]
    pub(crate) fn frame_count(&self) -> Option<usize> {
        self.shown.as_ref().map(|shown| shown.frames.len())
    }

    /// The pointer settled on `path`'s row.
    ///
    /// `field` reaches this state back out of the owning view, so the timer
    /// and bake can be spawned against the view's own entity.
    pub fn enter<V: Render>(
        &mut self,
        path: PathBuf,
        map_name: Option<String>,
        game_data: Option<GameDataCache>,
        cx: &mut Context<V>,
        field: fn(&mut V) -> &mut Self,
    ) {
        // Already on this row: it is either counting down or already showing.
        if self.watched.as_ref().is_some_and(|(watched, _)| watched == &path) {
            return;
        }

        self.cancel_bake();
        self.dwell = Dwell::new();
        self.dwell.hover(path.clone(), Duration::ZERO);
        self.watched = Some((path.clone(), Instant::now()));
        self.drop_shown();
        self._ticker = None;

        cx.notify();

        // No game data is no preview, so nothing is waited for and no space is
        // held: the popup is the row's words on their own.
        let Some(game_data) = game_data else { return };
        self.expecting = true;
        self.failed = None;

        // A battle walked recently plays without the dwell, as the egui popup
        // plays a cached track: what the dwell holds back is the walk.
        if wows_toolkit_replay_data::minimap_preview::is_walked(&path) {
            self.bake(path, map_name, game_data, cx, field);
            return;
        }
        self._dwell_timer = Some(cx.spawn(async move |view, cx| {
            cx.background_executor().timer(DWELL).await;
            let _ = view.update(cx, |view, cx| {
                let this = field(view);
                let Some((watched, since)) = this.watched.clone() else { return };
                if watched != path {
                    return;
                }
                this.dwell.hover(path.clone(), since.elapsed());
                if this.dwell.pending_request().as_deref() != Some(path.as_path()) {
                    return;
                }
                this.bake(path, map_name, game_data, cx, field);
            });
        }));
    }

    /// The pointer is on no replay row, so nothing is dwelling and whatever
    /// was baking is abandoned.
    pub fn leave<V: Render>(&mut self, cx: &mut Context<V>) {
        if !self.dwell.is_watching() && self.shown.is_none() {
            return;
        }
        self.dwell.leave();
        self.watched = None;
        self.drop_shown();
        self._dwell_timer = None;
        self._ticker = None;
        self.cancel_bake();
        cx.notify();
    }

    /// Stops showing the current track and releases its texture.
    ///
    /// Not kept for a return visit: a track's frames are tens of megabytes,
    /// and the battle's commands are kept instead (`minimap_preview`), which
    /// rasterise again in a few frames' time.
    fn drop_shown(&mut self) {
        if self.shown.take().is_some() {
            self.texture.clear();
        }
    }

    /// Uploads the frame the clock has reached, unless the texture already
    /// holds it. Returns whether the view must repaint to show it, which is
    /// when it could not reach the window yet.
    fn present_current(&mut self, cx: &mut App) -> bool {
        let Some(shown) = self.shown.as_mut() else { return false };
        let Some(index) = shown.frames.index_at(shown.started.elapsed()) else { return false };
        if shown.presented == Some(index) {
            return false;
        }
        let Some(frame) = shown.frames.frame(index) else { return false };

        shown.presented = Some(index);
        !self.texture.present(frame.clone(), cx)
    }

    fn cancel_bake(&mut self) {
        self.baking = false;
        self.expecting = false;
        self.failed = None;
        self.cancel.store(true, Ordering::Relaxed);
        self.cancel = Arc::new(AtomicBool::new(false));
        self._bake = None;
    }

    /// Puts up the still map standing in for a track that has not arrived,
    /// unless one is already up: replacing a map with another build's art of
    /// the same map would only stutter.
    fn show_map<V: Render>(&mut self, frames: PreviewFrames, cx: &mut Context<V>) {
        if self.shown.is_some() {
            return;
        }
        self.shown = Some(Shown { frames, started: Instant::now(), map_only: true, presented: None });
        let _ = self.present_current(cx);
        cx.notify();
    }

    /// Bakes and rasterises `path`'s preview off the UI thread.
    ///
    /// The build the replay was recorded on has to be loaded for it, which is
    /// the expensive half; a replay whose build is not installed simply shows
    /// no preview rather than reporting an error over the rows.
    fn bake<V: Render>(
        &mut self,
        path: PathBuf,
        map_name: Option<String>,
        game_data: GameDataCache,
        cx: &mut Context<V>,
        field: fn(&mut V) -> &mut Self,
    ) {
        self.baking = true;
        cx.notify();
        let cancel = Arc::clone(&self.cancel);
        let edge = self.edge;
        self._bake = Some(cx.spawn(async move |view, cx| {
            let (map_tx, map_rx) = futures::channel::oneshot::channel();
            let (frame_tx, mut frame_rx) = futures::channel::mpsc::unbounded();
            // gpui's own pool, not the two-worker tokio runtime: a bake is
            // seconds of CPU work, and that runtime is sized for the database
            // and network awaits it would otherwise be blocking.
            let baked = cx.background_executor().spawn({
                let path = path.clone();
                let game_data = game_data.clone();
                async move {
                    wows_toolkit_replay_data::minimap_preview::bake_from_file(
                        &path,
                        &game_data,
                        edge,
                        &cancel,
                        move |map| {
                            let _ = map_tx.send(map);
                        },
                        move |frame| {
                            let _ = frame_tx.unbounded_send(frame);
                        },
                    )
                }
            });

            // The map, from a build that is already open, while the bake
            // runs: it costs one image decode, where the bake may cost a
            // build load. The first map put up stays (`show_map`).
            if let Some(name) = map_name {
                let drawn = cx.background_executor().spawn(async move {
                    wows_toolkit_replay_data::minimap_preview::map_frame(&name, &game_data, edge)
                });
                if let Some(frames) = drawn.await {
                    let _ = view.update(cx, |view, cx| field(view).show_map(frames, cx));
                }
            }
            // The bake's own map, drawn from the build the replay was
            // recorded on, for when the stage above had no build open or no
            // art for this map.
            if let Ok(map) = map_rx.await {
                let _ = view.update(cx, |view, cx| field(view).show_map(map, cx));
            }

            // Frames play as they are rasterised: the first replaces the
            // still map, and the rest extend the track under a ticker that is
            // already running.
            let mut started = false;
            while let Some(frame) = futures::StreamExt::next(&mut frame_rx).await {
                let alive = view.update(cx, |view, cx| {
                    let this = field(view);
                    let first = !started;
                    if first {
                        started = true;
                        this.drop_shown();
                        this.shown = Some(Shown {
                            frames: PreviewFrames::streaming(),
                            started: Instant::now(),
                            map_only: false,
                            presented: None,
                        });
                        this.start_ticker(cx, field);
                    }
                    if let Some(shown) = this.shown.as_mut() {
                        shown.frames.push(frame);
                    }
                    if this.present_current(cx) || first {
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    return;
                }
            }

            let baked = baked.await;
            // Why there is none, for the surface to say. A superseded bake says
            // nothing: the row it was for is not the row being watched.
            let failed = match &baked {
                Err(err) => {
                    tracing::debug!("no preview for {}: {err}", path.display());
                    err.said().map(SharedString::from)
                }
                Ok(()) => None,
            };
            let superseded = matches!(baked, Err(wows_toolkit_replay_data::minimap_preview::PreviewError::Cancelled));

            let _ = view.update(cx, |view, cx| {
                let this = field(view);
                this.baking = false;
                // Nothing more is coming, so a replay that produced no preview
                // stops holding the map's space.
                this.expecting = false;
                this.failed = failed;
                // A pointer that moves on drops this task, so a bake that gets
                // here superseded had its source replaced under it; the map it
                // put up came from that source and comes down with it.
                if superseded {
                    this.drop_shown();
                }
                // Looping only starts once the whole track is here.
                if let Some(shown) = this.shown.as_mut() {
                    shown.frames.finish();
                }
                cx.notify();
            });
        }));
    }

    /// Uploads the frame the clock has reached every frame interval for as
    /// long as a preview is showing: the frames are a track played against
    /// the clock, and nothing else in the view would ask for them.
    fn start_ticker<V: Render>(&mut self, cx: &mut Context<V>, field: fn(&mut V) -> &mut Self) {
        self._ticker = Some(cx.spawn(async move |view, cx| {
            loop {
                cx.background_executor().timer(FRAME_INTERVAL).await;
                let playing = view.update(cx, |view, cx| {
                    let this = field(view);
                    if this.shown.is_none() {
                        return false;
                    }
                    // A frame that could not reach the window yet goes up
                    // with the next paint.
                    if this.present_current(cx) {
                        cx.notify();
                    }
                    true
                });
                if !matches!(playing, Ok(true)) {
                    return;
                }
            }
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bake keeps running after the first frame, so a spinner shown for
    /// all of it sits over a preview that is already playing.
    #[test]
    fn the_spinner_shows_only_until_the_preview_plays() {
        let mut hover = PreviewHover::default();
        assert!(!hover.awaits_first_frame(), "nothing is being baked");

        hover.baking = true;
        assert!(hover.awaits_first_frame(), "a bake with nothing to play yet is what a reader is waiting on");

        hover.seed_playing_for_test(PreviewFrames::streaming());
        assert!(
            !hover.awaits_first_frame(),
            "a preview that is playing does not need one, however far the bake trails"
        );
    }
}
