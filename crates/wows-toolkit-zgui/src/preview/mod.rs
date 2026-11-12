//! Hovering a replay row plays its battle back on the minimap, in a card beside the row.
//!
//! When that is worth doing is the shared rule (`wows_toolkit_viewmodel::preview_dwell`): the
//! pointer has to settle on a row before its replay is read. What this adds is the zgui half: the
//! dwell timed on the window's clock, the bake off the UI thread, the track played at the preview
//! rate, and the card that shows it with its loading and failure states.
//!
//! A list makes one [`HoverPreview`] in its body, calls [`HoverPreview::enter`] from each row's
//! `pointer_enter` with the row's `NodeRef` as the anchor and [`HoverPreview::leave`] from the
//! list's `pointer_leave`, and places one [`card::HoverPreviewCard`] anywhere in its view.
//!
//! ```text
//! let preview = HoverPreview::new();
//! row(node_ref = row, on:pointer_enter = move |_| preview.enter(PreviewTarget { path, map_name, anchor: row }))
//! HoverPreviewCard(preview = preview) { /* the row's facts, read from preview.watched() */ }
//! ```

mod bake;
pub mod card;
#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use futures::StreamExt as _;
use wows_toolkit_replay_data::frame::BgraFrame;
use wows_toolkit_replay_data::minimap_preview::FRAME_INTERVAL;
use wows_toolkit_replay_data::minimap_preview::PreviewEdge;
use wows_toolkit_replay_data::minimap_preview::PreviewError;
use wows_toolkit_replay_data::minimap_preview::PreviewFrames;
use wows_toolkit_viewmodel::preview_dwell::DWELL;
use wows_toolkit_viewmodel::preview_dwell::Dwell;
use zgui::reactive::LocalStorage;
use zgui::reactive::Owner;
use zgui::reactive::RwSignal;
use zgui::reactive::StoredValue;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::UpdateValue;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::reactive::prelude::WithValue;
use zgui::task::Task;
use zgui::task::spawn_local;
use zgui::view::IntervalHandle;
use zgui::view::NodeRef;
use zgui::view::TimeoutHandle;
use zgui::view::Timers;

use crate::frames::FrameSink;
use crate::preview::bake::BakeRequest;
use crate::preview::bake::BakeStreams;
use crate::preview::bake::GameDataBaker;
use crate::preview::bake::PreviewBaker;
use crate::services::Services;

/// The row the pointer settled on.
#[derive(Clone)]
pub struct PreviewTarget {
    pub path: PathBuf,
    /// The map's name as a build names it, so the still map can go up from a build already open
    /// before the replay's own build is loaded. `None` waits for the bake's own map.
    pub map_name: Option<String>,
    /// The row the card is placed beside.
    pub anchor: NodeRef,
}

/// The hover preview of one list: what is watched, what is baked, and the work driving both.
///
/// A `Copy` handle, made in a component's body and moved into the rows' listeners. Everything it
/// started stops when that component goes away.
#[derive(Clone, Copy)]
pub struct HoverPreview {
    status: RwSignal<Option<Watched>, LocalStorage>,
    run: StoredValue<Run, LocalStorage>,
}

impl HoverPreview {
    /// Bakes against the process's game data. Called from a component's body inside `AppRoot`.
    pub fn new() -> Self {
        let game_data = Services::current().game_data;
        Self::with_baker(Rc::new(GameDataBaker { game_data }))
    }

    fn with_baker(baker: Rc<dyn PreviewBaker>) -> Self {
        let run = StoredValue::new_local(Run {
            baker,
            timers: Timers::current(),
            owner: Owner::current(),
            sink: FrameSink::new(),
            dwell: Dwell::new(),
            track: None,
            cancel: Arc::new(AtomicBool::new(false)),
            dwell_timer: None,
            ticker: None,
            bake: None,
        });
        let preview = Self { status: RwSignal::new_local(None), run };
        // A bake keeps running on its worker until it reads the flag.
        on_cleanup_local(move || {
            run.try_with_value(|run| run.cancel.store(true, Ordering::Relaxed));
        });

        preview
    }

    /// The pointer settled on `target`'s row.
    pub fn enter(self, target: PreviewTarget) {
        let PreviewTarget { path, map_name, anchor } = target;
        let same_row = self.status.with_untracked(|status| status.as_ref().is_some_and(|watched| watched.path == path));
        if same_row {
            // A list that builds its rows again hands over a new handle for the same row.
            self.status.update(|status| {
                if let Some(watched) = status.as_mut().filter(|watched| watched.anchor != anchor) {
                    watched.anchor = anchor;
                }
            });
            return;
        }

        let replaced = self.status.with_untracked(Option::is_some);
        let started = self.with_run(|run| {
            run.stop();
            run.dwell = Dwell::new();
            run.dwell.hover(path.clone(), Duration::ZERO);
            let request = BakeRequest {
                path: path.clone(),
                map_name,
                edge: PreviewEdge::for_scale_factor(anchor.scale()),
                cancel: Arc::clone(&run.cancel),
            };
            (request, run.baker.is_available(), run.baker.is_walked(&path))
        });
        let Some((request, available, walked)) = started else { return };

        // Without game data there is no preview, so nothing is waited for and no space is held:
        // the card is the row's facts on their own.
        self.status.set(Some(Watched {
            path,
            anchor,
            replaced,
            expecting: available,
            baking: false,
            shown: None,
            failed: None,
        }));
        if !available {
            return;
        }

        // A battle walked recently costs only its drawing, which the dwell is not there to hold
        // back.
        if walked {
            self.bake(request);
            return;
        }
        self.with_run(|run| {
            run.dwell_timer =
                run.timers.as_ref().map(|timers| timers.set_timeout(DWELL, move || self.dwelled(request)));
        });
    }

    /// The pointer left the rows, so whatever was dwelling or baking is abandoned.
    pub fn leave(self) {
        let watching = self.status.with_untracked(Option::is_some);
        if !watching {
            return;
        }

        self.with_run(|run| {
            run.stop();
            run.dwell.leave();
        });
        self.status.set(None);
    }

    /// The replay under the pointer, previewed or not. Reactive.
    pub fn watched(self) -> Option<PathBuf> {
        self.status.with(|status| status.as_ref().map(|watched| watched.path.clone()))
    }

    /// [`Self::watched`], for a listener.
    pub fn watched_untracked(self) -> Option<PathBuf> {
        self.status.with_untracked(|status| status.as_ref().map(|watched| watched.path.clone()))
    }

    /// What the card shows where the map goes. Reactive.
    pub fn map_area(self) -> MapArea {
        self.status.with(|status| status.as_ref().map_or(MapArea::Absent, Watched::map_area))
    }

    /// The anchor the card is placed beside, and whether it replaced a card beside another row.
    fn placement(self) -> Option<(NodeRef, bool)> {
        self.status.with(|status| status.as_ref().map(|watched| (watched.anchor, watched.replaced)))
    }

    fn sink(self) -> Option<FrameSink> {
        self.with_run(|run| run.sink.clone())
    }

    /// The dwell timer fired: bakes if the pointer is still on the row it was started for.
    fn dwelled(self, request: BakeRequest) {
        let settled = self.with_run(|run| {
            run.dwell_timer = None;
            run.dwell.hover(request.path.clone(), DWELL);
            run.dwell.pending_request().is_some_and(|pending| pending == request.path)
        });
        let still_watched =
            self.status.with_untracked(|status| status.as_ref().is_some_and(|watched| watched.path == request.path));
        if settled == Some(true) && still_watched {
            self.bake(request);
        }
    }

    fn bake(self, request: BakeRequest) {
        let Some((streams, owner)) = self.with_run(|run| (run.baker.start(request), run.owner.clone())) else { return };
        let Some(streams) = streams else {
            self.edit(|watched| watched.expecting = false);
            return;
        };
        self.edit(|watched| watched.baking = true);

        let work = async move {
            let BakeStreams { open_map, own_map, mut frames, finished } = streams;
            if let Some(map) = open_map.await {
                self.show_map(map);
            }
            if let Ok(map) = own_map.await {
                self.show_map(map);
            }
            while let Some(frame) = frames.next().await {
                self.take_frame(frame);
            }
            let finished = finished.await;
            self.finish(finished);
        };
        // Spawned under the list's own scope, since a listener runs in none.
        let task = match owner {
            Some(owner) => owner.with(|| spawn_local(work)),
            None => spawn_local(work),
        };
        self.with_run(|run| run.bake = Some(task));
    }

    /// Puts up the still map standing in for a track that has not arrived, unless something is
    /// already up: another build's art of the same map would only stutter.
    fn show_map(self, frames: PreviewFrames) {
        let shown = self.with_run(|run| {
            if run.track.is_some() {
                return false;
            }
            run.track = Some(Track { frames, ticks: 0, presented: None, map_only: true });
            run.present();
            true
        });
        if shown == Some(true) {
            self.edit(|watched| watched.shown = Some(Shown::Map));
        }
    }

    /// Adds a drawn frame to the track. The first replaces the still map and starts playback.
    fn take_frame(self, frame: BgraFrame) {
        let first = self.with_run(|run| {
            let first = run.track.as_ref().is_none_or(|track| track.map_only);
            if first {
                run.track =
                    Some(Track { frames: PreviewFrames::streaming(), ticks: 0, presented: None, map_only: false });
                run.ticker = run.timers.as_ref().map(|timers| timers.set_interval(FRAME_INTERVAL, move || self.tick()));
            }
            if let Some(track) = run.track.as_mut() {
                track.frames.push(frame);
            }
            run.present();
            first
        });
        if first == Some(true) {
            self.edit(|watched| watched.shown = Some(Shown::Track));
        }
    }

    fn tick(self) {
        self.with_run(|run| {
            if let Some(track) = run.track.as_mut() {
                track.ticks += 1;
            }
            run.present();
        });
    }

    fn finish(self, finished: Result<(), PreviewError>) {
        if let Err(err) = &finished {
            tracing::debug!("no preview: {err}");
        }
        // A bake superseded under a row still watched had its source replaced, and the map it put
        // up came from that source.
        let superseded = matches!(finished, Err(PreviewError::Cancelled));
        self.with_run(|run| {
            run.bake = None;
            if superseded {
                run.drop_track();
            }
            // Playback loops only once the whole track is here.
            if let Some(track) = run.track.as_mut() {
                track.frames.finish();
            }
        });

        // Nothing more is coming, so a replay that produced no preview stops holding the map's
        // space and says why.
        let failed = finished.err().and_then(|err| err.said());
        self.edit(|watched| {
            watched.baking = false;
            watched.expecting = false;
            watched.failed = failed;
            if superseded {
                watched.shown = None;
            }
        });
    }

    /// Changes the watched row's state, when a row is watched.
    fn edit(self, change: impl FnOnce(&mut Watched)) {
        self.status.update(|status| {
            if let Some(watched) = status.as_mut() {
                change(watched);
            }
        });
    }

    /// `None` once the list that made this preview is gone.
    fn with_run<U>(self, change: impl FnOnce(&mut Run) -> U) -> Option<U> {
        self.run.try_update_value(change)
    }
}

/// What the card shows where the map goes.
#[derive(Clone, Debug, PartialEq)]
pub enum MapArea {
    /// No map is coming, as without game data.
    Absent,
    /// The map's space, held while the pointer settles and the bake starts.
    Holding { baking: bool },
    /// Frames are up. `waiting` while a still map stands in for a track still being drawn.
    Frames { waiting: bool },
    /// Why there is no preview, which is ordinarily a replay from a build that is not installed.
    Failed(String),
}

#[derive(Clone, PartialEq)]
struct Watched {
    path: PathBuf,
    anchor: NodeRef,
    /// Whether the card replaced one beside another row, so it goes up without its entrance.
    replaced: bool,
    /// Whether a preview is on its way, dwell included. The map's space is held for all of it.
    expecting: bool,
    baking: bool,
    shown: Option<Shown>,
    failed: Option<String>,
}

impl Watched {
    fn map_area(&self) -> MapArea {
        if let Some(shown) = self.shown {
            // The still map is not the preview, so it stays under the spinner until the track starts.
            return MapArea::Frames { waiting: self.baking && shown == Shown::Map };
        }
        if self.expecting {
            return MapArea::Holding { baking: self.baking };
        }

        self.failed.clone().map_or(MapArea::Absent, MapArea::Failed)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shown {
    Map,
    Track,
}

/// The parts of a preview that are not read by the view.
struct Run {
    baker: Rc<dyn PreviewBaker>,
    /// `None` outside a window, where nothing is timed and nothing plays.
    timers: Option<Timers>,
    owner: Option<Owner>,
    sink: FrameSink,
    dwell: Dwell<PathBuf>,
    track: Option<Track>,
    /// Set when a bake in flight should stop.
    cancel: Arc<AtomicBool>,
    dwell_timer: Option<TimeoutHandle>,
    ticker: Option<IntervalHandle>,
    bake: Option<Task>,
}

impl Run {
    /// Abandons the dwell, the bake and the track.
    fn stop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.cancel = Arc::new(AtomicBool::new(false));
        if let Some(bake) = self.bake.take() {
            bake.cancel();
        }
        self.dwell_timer = None;
        self.drop_track();
    }

    /// Not kept for a return visit: a track's frames are tens of megabytes, and the battle's
    /// commands are kept instead (`minimap_preview`), which draw again in a few frames' time.
    fn drop_track(&mut self) {
        self.ticker = None;
        if self.track.take().is_some() {
            self.sink.clear();
        }
    }

    /// Hands over the frame the playback clock has reached, unless it is the one already shown.
    fn present(&mut self) {
        let Some(track) = self.track.as_mut() else { return };
        let Some(index) = track.frames.index_at(FRAME_INTERVAL * track.ticks) else { return };
        if track.presented == Some(index) {
            return;
        }
        let Some(frame) = track.frames.frame(index) else { return };

        track.presented = Some(index);
        self.sink.push(frame.clone());
    }
}

/// A baked track and where playback is in it.
struct Track {
    frames: PreviewFrames,
    /// Frame intervals since the track started. Counted rather than measured, so playback runs on
    /// the window's clock.
    ticks: u32,
    presented: Option<usize>,
    /// Whether this is the still map standing in for a track that has not arrived.
    map_only: bool,
}
