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

use gpui_kit::Context;
use gpui_kit::Render;
use gpui_kit::Task;
use wows_toolkit_viewmodel::preview_dwell::DWELL;
use wows_toolkit_viewmodel::preview_dwell::Dwell;

use crate::minimap_preview::FRAME_INTERVAL;
use crate::minimap_preview::PreviewFrames;
use crate::replay_inspector::GameDataCache;

/// How long a preview goes without a new frame before the bake behind it
/// reads as stuck rather than busy.
///
/// Comfortably longer than the gap between frames a bake produces, so a
/// spinner never flickers over a preview that is merely slow.
const STALLED_AFTER: std::time::Duration = std::time::Duration::from_millis(1500);

/// A baked preview and where it is in its loop.
struct Shown {
    /// The row this track was baked for. Held here because the track outlives
    /// the hover: `drop_shown` runs once the pointer is already on the next
    /// row, and the cache has to be keyed by the row the frames belong to.
    path: PathBuf,
    frames: PreviewFrames,
    started: Instant,
    /// When the newest frame arrived. A bake that has stopped producing them
    /// is what a spinner over a playing preview is for.
    last_frame: Instant,
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
    /// Tracks whose textures are still held by the window. gpui keeps an
    /// image's texture until it is told to let go, and only a `Window` can be
    /// told; the surface hands them back on its next render
    /// (`release_dropped`).
    released: Vec<PreviewFrames>,
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
    /// The last track baked in full, so moving the pointer away and back is
    /// instant rather than another bake.
    ///
    /// One entry, not a table: a track is a few dozen megabytes of frames,
    /// and the hover that is worth saving is the one just left.
    cached: Option<(PathBuf, PreviewFrames)>,
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
            released: Vec::new(),
            baking: false,
            expecting: false,
            cached: None,
            cancel: Arc::new(AtomicBool::new(false)),
            _bake: None,
            _dwell_timer: None,
            _ticker: None,
        }
    }
}

impl PreviewHover {
    /// The frame to draw now, if a preview is playing.
    pub fn frame(&self) -> Option<Arc<gpui_kit::RenderImage>> {
        let shown = self.shown.as_ref()?;
        shown.frames.at(shown.started.elapsed())
    }

    /// Releases the textures of every track that is no longer showing.
    ///
    /// Called from the owning view's `render`, which is where a `Window` is
    /// at hand. Without it a hover over a dozen replays leaves a dozen
    /// tracks' frames resident.
    pub fn release_dropped(&mut self, window: &mut gpui_kit::Window) {
        for track in self.released.drain(..) {
            for image in track.images() {
                let _ = window.drop_image(image);
            }
        }
    }

    /// The row under the pointer, whether or not its preview has baked yet.
    pub fn watched_path(&self) -> Option<&std::path::Path> {
        self.watched.as_ref().map(|(path, _)| path.as_path())
    }

    /// Whether a preview is on its way for the watched row, dwell included.
    ///
    /// What a surface holds the map's space on: a popup that waited for the
    /// first frame would go up without a map and then grow around one.
    pub fn awaits_preview(&self) -> bool {
        self.expecting
    }

    /// Whether the bake has stopped feeding the preview.
    ///
    /// A bake keeps running after the first frame, so a spinner shown for
    /// the whole of it sits over a preview that is already playing and says
    /// nothing. It is worth showing only once the frames stop arriving.
    pub fn is_stalled(&self) -> bool {
        if !self.baking {
            return false;
        }
        match &self.shown {
            // Nothing to play yet: the bake has produced nothing, which is
            // what a reader is waiting on.
            None => true,
            Some(shown) => shown.last_frame.elapsed() > STALLED_AFTER,
        }
    }

    /// Whether a row is under the pointer at all, dwelled or not.
    #[cfg(test)]
    pub(crate) fn is_watching(&self) -> bool {
        self.dwell.is_watching()
    }

    /// Ages the newest frame by `by`, so a test can reach the case a bake
    /// has stopped feeding the preview without waiting for one to.
    #[cfg(test)]
    pub(crate) fn age_last_frame_for_test(&mut self, by: std::time::Duration) {
        if let Some(shown) = self.shown.as_mut() {
            shown.last_frame -= by;
        }
    }

    /// Seeds a bake in flight that has produced nothing yet. Test-only.
    #[cfg(test)]
    pub(crate) fn seed_baking_for_test(&mut self, path: PathBuf) {
        self.baking = true;
        self.expecting = true;
        self.shown = None;
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
    pub(crate) fn seed_playing_for_test(&mut self, path: PathBuf, frames: PreviewFrames) {
        self.baking = true;
        self.shown = Some(Shown { path, frames, started: Instant::now(), last_frame: Instant::now() });
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

        // The row just left is the one most likely to be returned to, and its
        // track is still here; there is nothing to bake or to wait for.
        if self.cached.as_ref().is_some_and(|(cached, _)| cached == &path) {
            let (_, frames) = self.cached.take().expect("the cache was just checked");
            self.shown =
                Some(Shown { path: path.clone(), frames, started: Instant::now(), last_frame: Instant::now() });
            self.start_ticker(cx, field);
            cx.notify();
            return;
        }
        cx.notify();

        // No game data is no preview, so nothing is waited for and no space is
        // held: the popup is the row's words on their own.
        let Some(game_data) = game_data else { return };
        self.expecting = true;
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

    /// The pointer left the rows, so nothing is dwelling and whatever was
    /// baking is abandoned.
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

    /// Stops showing the current track.
    ///
    /// A track that finished baking is kept for a return visit; the previous
    /// tenant of that one slot, and any unfinished track, go back to the
    /// window to have their textures released.
    fn drop_shown(&mut self) {
        let Some(shown) = self.shown.take() else { return };
        if !shown.frames.is_complete() {
            self.released.push(shown.frames);
            return;
        }
        if let Some((_, evicted)) = self.cached.replace((shown.path, shown.frames)) {
            self.released.push(evicted);
        }
    }

    fn cancel_bake(&mut self) {
        self.baking = false;
        self.expecting = false;
        self.cancel.store(true, Ordering::Relaxed);
        self.cancel = Arc::new(AtomicBool::new(false));
        self._bake = None;
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
        self._bake = Some(cx.spawn(async move |view, cx| {
            // The map, from a build that is already open. This is the first
            // thing on screen: it costs one image decode, where the track
            // costs a replay read and possibly a build load.
            if let Some(name) = map_name {
                let game_data = game_data.clone();
                let drawn =
                    cx.background_executor().spawn(async move { crate::minimap_preview::map_frame(&name, &game_data) });
                if let Some(frames) = drawn.await {
                    let _ = view.update(cx, |view, cx| {
                        let this = field(view);
                        this.drop_shown();
                        this.shown = Some(Shown {
                            path: path.clone(),
                            frames,
                            started: Instant::now(),
                            last_frame: Instant::now(),
                        });
                        cx.notify();
                    });
                }
            }

            let (map_tx, map_rx) = futures::channel::oneshot::channel();
            let (frame_tx, mut frame_rx) = futures::channel::mpsc::unbounded();
            // gpui's own pool, not the two-worker tokio runtime: a bake is
            // seconds of CPU work, and that runtime is sized for the database
            // and network awaits it would otherwise be blocking.
            let baked = cx.background_executor().spawn({
                let path = path.clone();
                async move {
                    crate::minimap_preview::bake_from_file(
                        &path,
                        &game_data,
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

            // The bake's own map, drawn from the build the replay was
            // recorded on. Only wanted when the stage above had no build open
            // or no art for this map; otherwise the map is already showing and
            // restarting it would stutter.
            if let Ok(map) = map_rx.await {
                let _ = view.update(cx, |view, cx| {
                    let this = field(view);
                    if this.shown.is_some() {
                        return;
                    }
                    this.shown = Some(Shown {
                        path: path.clone(),
                        frames: map,
                        started: Instant::now(),
                        last_frame: Instant::now(),
                    });
                    cx.notify();
                });
            }

            // Frames play as they are rasterised: the first replaces the
            // still map, and the rest extend the track under a ticker that is
            // already running.
            let mut started = false;
            while let Some(frame) = futures::StreamExt::next(&mut frame_rx).await {
                let alive = view.update(cx, |view, cx| {
                    let this = field(view);
                    if !started {
                        started = true;
                        this.drop_shown();
                        this.shown = Some(Shown {
                            path: path.clone(),
                            frames: PreviewFrames::streaming(),
                            started: Instant::now(),
                            last_frame: Instant::now(),
                        });
                        this.start_ticker(cx, field);
                    }
                    if let Some(shown) = this.shown.as_mut() {
                        shown.frames.push(frame);
                        shown.last_frame = Instant::now();
                    }
                    cx.notify();
                });
                if alive.is_err() {
                    return;
                }
            }

            let baked = baked.await;
            if let Err(err) = &baked {
                tracing::debug!("no preview for {}: {err}", path.display());
            }

            let _ = view.update(cx, |view, cx| {
                let this = field(view);
                this.baking = false;
                // Nothing more is coming, so a replay that produced no preview
                // stops holding the map's space.
                this.expecting = false;
                // Looping only starts once the whole track is here.
                if let Some(shown) = this.shown.as_mut() {
                    shown.frames.finish();
                }
                cx.notify();
            });
        }));
    }

    /// Redraws the owning view every frame interval for as long as a preview
    /// is showing: the frames are a track played against the clock, and
    /// nothing else in the view would ask for those repaints.
    fn start_ticker<V: Render>(&mut self, cx: &mut Context<V>, field: fn(&mut V) -> &mut Self) {
        self._ticker = Some(cx.spawn(async move |view, cx| {
            loop {
                cx.background_executor().timer(FRAME_INTERVAL).await;
                let playing = view.update(cx, |view, cx| {
                    let this = field(view);
                    if this.shown.is_some() {
                        cx.notify();
                        true
                    } else {
                        false
                    }
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
    /// all of it sits over a preview that is already playing. It is worth
    /// showing only before there is anything to play, or once the frames
    /// have stopped arriving.
    #[test]
    fn the_spinner_waits_for_a_stall_rather_than_the_whole_bake() {
        let mut hover = PreviewHover::default();
        assert!(!hover.is_stalled(), "nothing is being baked");

        hover.baking = true;
        assert!(hover.is_stalled(), "a bake with nothing to play yet is what a reader is waiting on");

        hover.seed_playing_for_test(PathBuf::from("a.wowsreplay"), PreviewFrames::streaming());
        assert!(!hover.is_stalled(), "a preview that is playing does not need one");

        hover.age_last_frame_for_test(STALLED_AFTER + std::time::Duration::from_millis(1));
        assert!(hover.is_stalled(), "until the frames stop arriving");
    }
}
