//! A battle someone else is playing back.
//!
//! A peer with no copy of the replay cannot open it, so the end of the session that has it sends
//! what it draws. A frame on that wire is a list of draw commands rather than an image, so this
//! draws the battle with its own art at whatever size its own window is, and a peer on a different
//! build sees the same battle rather than a picture of someone else's window.
//!
//! It does not steer: the clock belongs to the end that owns the replay, and this reports where
//! that end has reached rather than offering a transport of its own.

use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::sync_channel;
use std::time::Duration;

use rust_i18n::t;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_minimap_renderer::viewport::MapViewport;
use wows_replays::types::GameClock;
use wows_toolkit_replay_data::load::GameDataCache;
use wows_toolkit_replay_data::minimap_preview::render_map;
use wows_toolkit_replay_data::minimap_preview::render_map_art;
use wows_toolkit_services::collab::CollabLink;
use wows_toolkit_services::collab::ReplayId;
use wows_toolkit_services::collab::SharedWindow;
use wt_collab_client::PlaybackFrame;
use zgui::component;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::Owner;
use zgui::reactive::RwSignal;
use zgui::reactive::StoredValue;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::UpdateValue;
use zgui::reactive::prelude::WithValue;
use zgui::task::blocking;
use zgui::task::spawn_local;
use zgui::view;
use zgui::view::IntoView;
use zgui::view::ShowProps;
use zgui::view::install_stylesheet;
use zgui::view::set_interval;
use zgui_ui::SpinnerProps;

use crate::frames::FrameSink;
use crate::frames::FrameViewProps;
use crate::services::Services;

const SHEET: &str = "wt-watched";

const CSS: &str = css!(
    ".wt-watched { display: flex; flex-direction: column; flex: 1; min-height: 0; min-width: 0; }
    .wt-watched__clock {
        display: flex;
        flex: none;
        align-items: center;
        gap: 8px;
        padding: 4px 8px;
        font-size: 12px;
        color: var(--wt-color-text-dim);
    }
    .wt-watched__bar {
        flex: 1;
        min-width: 0;
        height: 3px;
        border-radius: var(--lui-radius);
        background-color: var(--lui-color-border);
    }
    .wt-watched__fill { height: 100%; border-radius: var(--lui-radius); background-color: var(--lui-color-accent); }
    .wt-watched__rule { flex: none; height: 1px; background-color: var(--lui-color-border); }
    .wt-watched__body { position: relative; flex: 1; min-height: 0; }
    .wt-watched__frame { position: absolute; left: 0; top: 0; right: 0; bottom: 0; }
    .wt-watched__waiting {
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        gap: 4px;
        height: 100%;
        font-size: 14px;
    }"
);

/// How often the frames the session has sent are looked for.
///
/// The shared sink's waker is a plain callback with no way onto the UI thread, so this drains on a
/// timer instead. Watching someone else's playback is worth a tenth of a second of lateness.
const DRAIN: Duration = Duration::from_millis(100);

/// What the end that owns the replay says about where it has reached.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HostClock {
    pub clock: GameClock,
    pub frame: usize,
    pub total_frames: usize,
}

impl HostClock {
    /// How far through the battle it is, as a fraction.
    ///
    /// `None` before the host has said how many frames there are, which is not the same as a
    /// battle with none.
    pub fn through(self) -> Option<f32> {
        (self.total_frames > 0).then(|| (self.frame as f32 / self.total_frames as f32).clamp(0.0, 1.0))
    }

    /// The host's clock as the strip reads it, minutes and seconds.
    pub fn label(self) -> String {
        let seconds = self.clock.0.max(0.0);
        format!("{}:{:02}", (seconds / 60.0).floor() as i32, (seconds % 60.0) as i32)
    }
}

/// One watched window's state, as one `Copy` handle.
#[derive(Clone, Copy)]
pub struct WatchSession {
    /// Where the host was when it drew the last frame that arrived. `None` until one has.
    pub clock: RwSignal<Option<HostClock>, LocalStorage>,
    /// Whether a frame has been drawn into the sink.
    pub drawn: RwSignal<bool, LocalStorage>,
    run: StoredValue<WatchRun, LocalStorage>,
}

/// The parts of a watch the view does not read.
struct WatchRun {
    /// Which of the session's windows this is, which is what its frames are addressed to and what
    /// its sink is registered under.
    replay_id: ReplayId,
    /// The map's space name, for drawing with this build's own art where it has it.
    space: String,
    /// The art the owning end sent, for a map this build ships none of.
    art: Option<Arc<image::RgbImage>>,
    game_data: Option<GameDataCache>,
    /// Bounded, and the far end drops rather than blocks: a frame nobody has drawn yet is stale.
    frames: Receiver<PlaybackFrame>,
    /// The commands the last frame carried.
    latest: Option<Arc<Vec<DrawCommand>>>,
    sink: FrameSink,
    /// One raster at a time: frames arrive faster than a map-sized image is drawn on a fast
    /// playback. A frame that arrives during one is drawn when it finishes.
    rasterising: bool,
    stale: bool,
    owner: Owner,
    link: CollabLink,
}

impl WatchSession {
    /// Asks the session for the frames of `shared`. Called from a component's body.
    pub fn open(shared: SharedWindow, link: CollabLink, game_data: Option<GameDataCache>) -> Self {
        // A depth of a few frames is a cushion against a slow raster, not a queue to be played out.
        let (tx, frames) = sync_channel(4);
        let replay_id = ReplayId::from_raw(shared.replay_id);
        link.watch_window(replay_id, tx);

        let art = shared.art_png.as_ref().and_then(|png| match image::load_from_memory(png) {
            Ok(art) => Some(Arc::new(art.to_rgb8())),
            // Art that will not decode leaves this drawing with the build's own, which is what a
            // peer that has the map does anyway.
            Err(err) => {
                tracing::warn!("watched playback: the art the host sent could not be read: {err}");
                None
            }
        });

        Self {
            clock: RwSignal::new_local(None),
            drawn: RwSignal::new_local(false),
            run: StoredValue::new_local(WatchRun {
                replay_id,
                space: shared.map_name,
                art,
                game_data,
                frames,
                latest: None,
                sink: FrameSink::new(),
                rasterising: false,
                stale: false,
                owner: Owner::current().expect("a watch is opened from a component's body"),
                link,
            }),
        }
    }

    pub fn sink(self) -> FrameSink {
        self.run.with_value(|run| run.sink.clone())
    }

    /// Takes whatever has arrived, keeping only the newest: the others are moments the host has
    /// already left, and drawing them would put this viewport behind it.
    pub fn take_frames(self) {
        let newest = self.run.try_with_value(|run| run.frames.try_iter().last()).flatten();
        let Some(frame) = newest else { return };

        self.run.update_value(|run| run.latest = Some(Arc::new(frame.commands)));
        self.clock.set(Some(HostClock {
            clock: frame.clock,
            frame: frame.frame_index,
            total_frames: frame.total_frames,
        }));
        self.redraw();
    }

    /// Says the session can stop sending frames nobody is drawing.
    pub fn stop(self) {
        self.run.try_with_value(|run| run.link.stop_watching(run.replay_id));
    }

    /// Draws the frame this viewport last had, off the UI thread.
    fn redraw(self) {
        let started = self.run.try_update_value(|run| {
            let commands = Arc::clone(run.latest.as_ref()?);
            // The fonts and icons a frame needs are this build's own: without them there is
            // nothing to draw the commands with.
            let game_data = run.game_data.clone()?;
            if run.rasterising {
                run.stale = true;
                return None;
            }
            run.rasterising = true;
            let job = RasterJob {
                commands,
                game_data,
                space: run.space.clone(),
                art: run.art.clone(),
                replay_id: run.replay_id,
                sink: run.sink.clone(),
            };
            Some((job, run.owner.clone()))
        });
        let Some((job, owner)) = started.flatten() else { return };

        owner.with(|| {
            spawn_local(async move {
                let drawn = blocking(move || job.run()).await;
                self.finish(drawn);
            });
        });
    }

    fn finish(self, drawn: bool) {
        let again = self.run.try_update_value(|run| {
            run.rasterising = false;
            std::mem::take(&mut run.stale)
        });
        let Some(again) = again else { return };
        if drawn && !self.drawn.get_untracked() {
            self.drawn.set(true);
        }
        if again {
            self.redraw();
        }
    }
}

/// One frame to rasterise, carried off the UI thread.
struct RasterJob {
    commands: Arc<Vec<DrawCommand>>,
    game_data: GameDataCache,
    space: String,
    art: Option<Arc<image::RgbImage>>,
    replay_id: ReplayId,
    sink: FrameSink,
}

impl RasterJob {
    /// Draws the frame into the sink. Returns whether there was anything to draw it over.
    fn run(self) -> bool {
        let view = MapViewport::default();
        let frame = match &self.art {
            Some(art) => {
                let held_as = format!("watched-{}", self.replay_id.raw());
                render_map_art(&held_as, art, &self.game_data, view, &self.commands)
            }
            None => render_map(&self.space, &self.game_data, view, &self.commands),
        };
        let Some(frame) = frame else { return false };
        self.sink.push(frame);

        true
    }
}

/// A viewport onto a battle the session is playing elsewhere.
#[component]
pub fn WatchedPlayback(shared: SharedWindow, link: CollabLink) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let game_data = Services::current().game_data.cache_untracked();
    let watch = WatchSession::open(shared, link, game_data);
    let drain = set_interval(DRAIN, move || watch.take_frames());
    on_cleanup_local(move || {
        drop(drain);
        watch.stop();
    });
    let sink = watch.sink();

    view! {
        column(class = "wt-watched", attr:data-testid = "watched-playback") {
            HostClockStrip(watch = watch)
            box(class = "wt-watched__rule")
            box(class = "wt-watched__body") {
                Show(when = move || watch.drawn.get()) {
                    FrameView(sink = sink.clone(), class = "wt-watched__frame")
                }
                Show(when = move || !watch.drawn.get()) {
                    column(class = "wt-watched__waiting", attr:data-testid = "watched-playback-waiting") {
                        // The host may never send a frame, so the wait has no end.
                        Spinner(label = t!("ui.collab.waiting_for_frames").into_owned())
                        text {{t!("ui.collab.waiting_for_frames").into_owned()}}
                    }
                }
            }
        }
    }
}

/// Where the end that owns the replay has reached, as a strip to read.
#[component]
fn HostClockStrip(watch: WatchSession) -> impl IntoView {
    let clock = move || watch.clock.get();
    let label = move || clock().map(HostClock::label).unwrap_or_default();
    let through = move || clock().and_then(HostClock::through);

    view! {
        Show(when = move || clock().is_some()) {
            row(class = "wt-watched__clock", attr:data-testid = "watched-playback-clock") {
                text {{t!("ui.collab.watching").into_owned()}}
                text(attr:data-testid = "watched-playback-clock-time") {{label}}
                Show(when = move || through().is_some()) {
                    box(class = "wt-watched__bar") {
                        box(
                            class = "wt-watched__fill",
                            attr:data-testid = "watched-playback-progress",
                            style:width = move || through().map(|through| format!("{}%", through * 100.0))
                        )
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use wows_replays::types::GameClock;

    use super::HostClock;

    /// How far through reads as a fraction, and says nothing at all before the host has said how
    /// long the battle is.
    #[test]
    fn progress_needs_a_length_to_be_a_fraction() {
        let at = |frame, total_frames| HostClock { clock: GameClock(10.0), frame, total_frames };

        assert_eq!(HostClock { clock: GameClock(0.0), frame: 0, total_frames: 0 }.through(), None);
        assert_eq!(at(5, 10).through(), Some(0.5));
        assert_eq!(at(0, 10).through(), Some(0.0));
    }

    /// A frame past the end reads as the end rather than as more than all of it.
    #[test]
    fn a_frame_past_the_end_reads_as_the_end() {
        assert_eq!(HostClock { clock: GameClock(10.0), frame: 30, total_frames: 10 }.through(), Some(1.0));
    }
}
