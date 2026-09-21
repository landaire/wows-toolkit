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

/// A baked preview and where it is in its loop.
struct Shown {
    /// The row this track was baked for. Held here because the track outlives
    /// the hover: `drop_shown` runs once the pointer is already on the next
    /// row, and the cache has to be keyed by the row the frames belong to.
    path: PathBuf,
    frames: PreviewFrames,
    started: Instant,
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

impl Default for PreviewHover {
    fn default() -> Self {
        Self {
            dwell: Dwell::new(),
            watched: None,
            shown: None,
            released: Vec::new(),
            baking: false,
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

    /// Whether a preview is being baked for the watched row.
    pub fn is_baking(&self) -> bool {
        self.baking
    }

    /// Whether a row is under the pointer at all, dwelled or not.
    #[cfg(test)]
    pub(crate) fn is_watching(&self) -> bool {
        self.dwell.is_watching()
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
            self.shown = Some(Shown { path: path.clone(), frames, started: Instant::now() });
            self.start_ticker(cx, field);
            cx.notify();
            return;
        }
        cx.notify();

        let Some(game_data) = game_data else { return };
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
                        this.shown = Some(Shown { path: path.clone(), frames, started: Instant::now() });
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
                    this.shown = Some(Shown { path: path.clone(), frames: map, started: Instant::now() });
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
                        });
                        this.start_ticker(cx, field);
                    }
                    if let Some(shown) = this.shown.as_mut() {
                        shown.frames.push(frame);
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
