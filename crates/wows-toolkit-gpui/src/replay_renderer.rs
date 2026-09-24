//! Playing a replay's battle back on the minimap, at whatever speed the
//! reader asks for.
//!
//! The egui app draws its renderer by turning each frame's draw commands into
//! egui shapes (`replay/renderer/`). This port has no painter of its own, so
//! it rasterises the same commands through
//! [`wows_minimap_renderer::preview::PreviewRenderer`] -- the path the hover
//! preview already uses -- and shows the resulting image.
//!
//! **What is kept, and why.** A whole battle's frames as images would be tens
//! of gigabytes, so the bake keeps the draw commands and one frame is
//! rasterised at a time. The commands themselves are bounded by
//! [`TrackSink`]'s budget: a long battle is sampled more coarsely rather than
//! costing more memory than a short one.
//!
//! **What it does not draw.** The panel commands (stats, rosters) and
//! position trails are outside `bake_options`, for the reason that module
//! documents. The egui renderer's annotation toolbar and video export are not
//! here either; see `docs/gpui-port-parity.md`.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::minimap_preview::SharedPreviewRenderer;
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Disableable;
use gpui_kit::component::IconName;
use gpui_kit::component::Selectable;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::dock::BasePanel;
use gpui_kit::component::dock::Panel;
use gpui_kit::component::dock::PanelEvent;
use gpui_kit::component::h_flex;
use gpui_kit::component::slider::Slider;
use gpui_kit::component::slider::SliderState;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::v_flex;
use gpui_kit::*;
use rust_i18n::t;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_replays::types::GameClock;

use crate::replay_inspector::GameDataCache;

/// How many frames of a battle the track keeps.
///
/// Past this the sink halves what it holds and samples half as often, so a
/// twenty-minute battle costs what a five-minute one does. Twelve hundred
/// frames is two minutes of game time at the bake rate below, and a whole
/// twenty-minute battle once the stride has doubled four times.
const TRACK_BUDGET: usize = 1200;

/// How much game time one baked frame covers.
const BAKE_INTERVAL: f32 = 0.5;

/// How often the viewport advances while playing, before the speed
/// multiplier. One frame of game time per tick at 1x.
const TICK: Duration = Duration::from_millis(500);

/// The speeds the transport offers, as the egui renderer's own do.
const SPEEDS: [f32; 5] = [0.5, 1.0, 2.0, 4.0, 8.0];

/// What the viewport is doing.
enum State {
    /// Reading the replay and walking the battle.
    Baking,
    Ready(Track),
    Failed(String),
}

/// A baked battle: the commands of every kept frame, and when each was drawn.
struct Track {
    frames: Vec<Vec<DrawCommand>>,
    clocks: Vec<GameClock>,
}

impl Track {
    fn len(&self) -> usize {
        self.frames.len()
    }

    /// The game time `index` was drawn at, in seconds.
    fn seconds_at(&self, index: usize) -> f32 {
        self.clocks.get(index).map(|clock| clock.seconds()).unwrap_or(0.0)
    }
}

/// A replay played back on its own minimap.
pub struct ReplayRendererPanel {
    title: SharedString,
    state: State,
    /// The renderer the track is rasterised through, bound to the build and
    /// map this replay was recorded on, and shared with every other preview
    /// of the same map. Taken while a frame is being drawn on the background
    /// executor, which is also what keeps two rasters of the same frame from
    /// being asked for at once.
    renderer: Option<SharedPreviewRenderer>,
    /// The frame on screen. Stays put while the next one is drawn, so the
    /// viewport never blanks.
    frame: Option<Arc<RenderImage>>,
    /// Which frame of the track that is, or is being drawn for.
    at: usize,
    playing: bool,
    speed: f32,
    seek: Entity<SliderState>,
    /// Set when this panel is dropped, so a bake in flight stops walking a
    /// battle nobody is waiting for.
    cancel: Arc<AtomicBool>,
    _bake: Option<Task<()>>,
    _tick: Option<Task<()>>,
    _seek_subscription: Option<Subscription>,
    /// The export under way, if any. Only one at a time: it holds the same
    /// renderer the viewport draws through.
    export: Option<ExportProgress>,
    /// Why the last export stopped, when it did not finish. Cleared when the
    /// next one starts.
    export_failure: Option<String>,
    _export: Option<Task<()>>,
    focus_handle: FocusHandle,
}

/// How far an export has got, in frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExportProgress {
    pub done: u64,
    pub total: u64,
}

impl EventEmitter<PanelEvent> for ReplayRendererPanel {}

impl ReplayRendererPanel {
    /// Opens a viewport on `path` and starts baking it.
    pub fn new(
        path: PathBuf,
        title: SharedString,
        game_data: GameDataCache,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let seek = cx.new(|_| SliderState::new().min(0.).max(1.).default_value(0.));
        let seek_subscription = cx.subscribe_in(&seek, window, Self::on_seek);

        let mut panel = Self {
            title,
            state: State::Baking,
            renderer: None,
            frame: None,
            at: 0,
            playing: false,
            export: None,
            export_failure: None,
            _export: None,
            speed: 1.0,
            seek,
            cancel: Arc::new(AtomicBool::new(false)),
            _bake: None,
            _tick: None,
            _seek_subscription: Some(seek_subscription),
            focus_handle: cx.focus_handle(),
        };
        panel.start_bake(path, game_data, cx);
        panel
    }

    /// A viewport already holding `clocks`' worth of empty frames, with no
    /// bake behind it and no renderer to rasterise through.
    ///
    /// Test-only: production viewports reach this state through a bake. The
    /// transport is what this exercises -- where playback is, what the clock
    /// reads, where the bar sits -- none of which needs a drawn frame.
    #[cfg(test)]
    pub(crate) fn ready_for_test(clocks: Vec<f32>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let seek = cx.new(|_| SliderState::new().min(0.).max(1.).default_value(0.));
        let seek_subscription = cx.subscribe_in(&seek, window, Self::on_seek);
        Self {
            title: SharedString::from("test"),
            state: State::Ready(Track {
                frames: vec![Vec::new(); clocks.len()],
                clocks: clocks.into_iter().map(GameClock).collect(),
            }),
            renderer: None,
            frame: None,
            at: 0,
            playing: false,
            export: None,
            export_failure: None,
            _export: None,
            speed: 1.0,
            seek,
            cancel: Arc::new(AtomicBool::new(false)),
            _bake: None,
            _tick: None,
            _seek_subscription: Some(seek_subscription),
            focus_handle: cx.focus_handle(),
        }
    }

    fn start_bake(&mut self, path: PathBuf, game_data: GameDataCache, cx: &mut Context<Self>) {
        let cancel = Arc::clone(&self.cancel);
        self._bake = Some(cx.spawn(async move |this, cx| {
            let baked = cx.background_spawn(async move { bake(&path, &game_data, &cancel) }).await;
            let _ = this.update(cx, |this, cx| {
                match baked {
                    Ok((track, renderer)) => {
                        this.renderer = Some(renderer);
                        this.state = State::Ready(track);
                        this.draw_current(cx);
                    }
                    Err(err) => this.state = State::Failed(err.to_string()),
                }
                cx.notify();
            });
        }));
    }

    /// The number of frames in the baked track, or zero while it is baking.
    fn frame_count(&self) -> usize {
        match &self.state {
            State::Ready(track) => track.len(),
            _ => 0,
        }
    }

    /// Asks for the current frame to be drawn, unless one already is.
    ///
    /// The renderer is moved onto the background executor for the draw and
    /// moved back with the image, so a tick that arrives while a frame is
    /// still being drawn simply does nothing and the next one catches up.
    fn draw_current(&mut self, cx: &mut Context<Self>) {
        let Some(renderer) = self.renderer.take() else { return };
        let State::Ready(track) = &self.state else {
            self.renderer = Some(renderer);
            return;
        };
        let Some(commands) = track.frames.get(self.at).cloned() else {
            self.renderer = Some(renderer);
            return;
        };

        cx.spawn(async move |this, cx| {
            let drawn = cx.background_spawn(async move {
                let image = {
                    let mut drawing = renderer.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    to_image(drawing.render(&commands))
                };
                (renderer, image)
            });
            let (renderer, image) = drawn.await;
            let _ = this.update(cx, |this, cx| {
                this.renderer = Some(renderer);
                this.frame = Some(image);
                cx.notify();
            });
        })
        .detach();
    }

    /// Asks where to write, then encodes the baked track there.
    ///
    /// The frames are the ones already baked, so nothing is parsed twice: the
    /// track is rasterised through the same renderer the viewport draws with,
    /// and the pixels go straight to the encoder.
    fn export_video(&mut self, cx: &mut Context<Self>) {
        if self.export.is_some() {
            return;
        }
        let State::Ready(track) = &self.state else { return };
        if track.frames.is_empty() {
            return;
        }
        // Held for the whole encode, which is why the transport is refused
        // while one runs: a frame cannot be drawn for two things at once.
        let Some(renderer) = self.renderer.take() else { return };

        let frames = track.frames.clone();
        let duration = track.seconds_at(track.len().saturating_sub(1));
        let suggested = format!("{}.mp4", self.title);
        let asked = crate::dialog::save_file(Some(&t!("ui.replay.renderer.export_video")), &suggested, Some(MP4));

        self.export_failure = None;
        self.export = Some(ExportProgress { done: 0, total: frames.len() as u64 });
        self.playing = false;
        cx.notify();

        let (progress_tx, mut progress_rx) = futures::channel::mpsc::unbounded::<ExportProgress>();
        let watched = cx.entity();
        cx.spawn(async move |_this, cx| {
            use futures::StreamExt as _;
            while let Some(step) = progress_rx.next().await {
                watched.update(cx, |this, cx| {
                    this.export = Some(step);
                    cx.notify();
                });
            }
        })
        .detach();

        self._export = Some(cx.spawn(async move |this, cx| {
            let Some(output) = asked.await else {
                // Cancelled at the dialog: the renderer goes back to the
                // viewport and nothing else changes.
                let _ = this.update(cx, |this, cx| {
                    this.renderer = Some(renderer);
                    this.export = None;
                    cx.notify();
                });
                return;
            };

            let (renderer, outcome) = cx
                .background_spawn(async move {
                    let outcome = encode_track(&renderer, &frames, duration, &output, progress_tx);
                    (renderer, outcome)
                })
                .await;

            let _ = this.update(cx, |this, cx| {
                this.renderer = Some(renderer);
                this.export = None;
                if let Err(reason) = outcome {
                    this.export_failure = Some(reason);
                }
                cx.notify();
            });
        }));
    }

    fn set_at(&mut self, at: usize, cx: &mut Context<Self>) {
        let last = self.frame_count().saturating_sub(1);
        let at = at.min(last);
        if at == self.at && self.frame.is_some() {
            return;
        }
        self.at = at;
        self.draw_current(cx);
        cx.notify();
    }

    fn toggle_playing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.playing = !self.playing;
        if self.playing {
            // Playing from the end starts again at the beginning, which is
            // what a transport with no stop button has to mean.
            if self.at + 1 >= self.frame_count() {
                self.set_at(0, cx);
            }
            self.start_ticker(window, cx);
        } else {
            self._tick = None;
        }
        cx.notify();
    }

    fn start_ticker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let interval = Duration::from_secs_f32(TICK.as_secs_f32() / self.speed.max(0.1));
        self._tick = Some(cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(interval).await;
                let still_playing = this
                    .update_in(cx, |this, window, cx| {
                        if !this.playing {
                            return false;
                        }
                        let next = this.at + 1;
                        if next >= this.frame_count() {
                            this.playing = false;
                            cx.notify();
                            return false;
                        }
                        this.set_at(next, cx);
                        // The bar follows playback, so a reader can see
                        // where in the battle they are.
                        this.sync_seek(window, cx);
                        true
                    })
                    .unwrap_or(false);
                if !still_playing {
                    return;
                }
            }
        }));
    }

    fn set_speed(&mut self, speed: f32, window: &mut Window, cx: &mut Context<Self>) {
        if (self.speed - speed).abs() < f32::EPSILON {
            return;
        }
        self.speed = speed;
        // The ticker's interval is fixed when it starts, so a speed change
        // replaces it rather than waiting for the next tick.
        if self.playing {
            self.start_ticker(window, cx);
        }
        cx.notify();
    }

    /// Moves the seek slider to where playback has reached.
    ///
    /// The slider reports the move back as a change, which would ask for the
    /// frame it is already on; `set_at` answers that with nothing to do.
    fn sync_seek(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let last = self.frame_count().saturating_sub(1);
        let fraction = if last == 0 { 0.0 } else { self.at as f32 / last as f32 };
        self.seek.update(cx, |slider, cx| slider.set_value(fraction, window, cx));
    }

    fn on_seek(
        &mut self,
        _state: &Entity<SliderState>,
        event: &gpui_kit::component::slider::SliderEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let gpui_kit::component::slider::SliderEvent::Change(value) = event else { return };
        let last = self.frame_count().saturating_sub(1);
        self.set_at((value.start() * last as f32).round() as usize, cx);
    }

    /// Where playback has reached, as "M:SS / M:SS" of game time.
    fn clock_label(&self) -> String {
        let State::Ready(track) = &self.state else { return String::new() };
        let last = track.len().saturating_sub(1);
        format!("{} / {}", mmss(track.seconds_at(self.at)), mmss(track.seconds_at(last)))
    }
}

/// A speed as the transport labels it: no trailing zero on a whole one.
fn speed_label(speed: f32) -> String {
    if (speed - speed.round()).abs() < f32::EPSILON {
        format!("{}x", speed.round() as i32)
    } else {
        format!("{speed}x")
    }
}

/// Seconds of game time as the clock the game shows.
fn mmss(seconds: f32) -> String {
    let seconds = seconds.max(0.0) as u32;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

impl Drop for ReplayRendererPanel {
    fn drop(&mut self) {
        // A viewport that has been closed is not one whose battle is worth
        // finishing.
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl Focusable for ReplayRendererPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl BasePanel for ReplayRendererPanel {
    fn panel_name(&self) -> &'static str {
        "ReplayRendererPanel"
    }

    fn closable(&self, _cx: &App) -> bool {
        true
    }
}

impl Panel for ReplayRendererPanel {
    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.title.clone()
    }
}

impl Render for ReplayRendererPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().border;

        let body: AnyElement = match &self.state {
            State::Baking => v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_2()
                .child(Spinner::new().large())
                .child(
                    div()
                        .text_sm()
                        .text_color(crate::theme::text_dim())
                        .child(t!("ui.replay.renderer.baking").into_owned()),
                )
                .into_any_element(),
            State::Failed(reason) => v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_1()
                .child(
                    div().text_sm().font_weight(FontWeight::BOLD).child(t!("ui.replay.renderer.failed").into_owned()),
                )
                .child(div().text_sm().text_color(crate::theme::text_dim()).child(reason.clone()))
                .into_any_element(),
            State::Ready(_) => match self.frame.clone() {
                Some(frame) => div()
                    .id("replay-renderer-viewport")
                    .test_support()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(img(frame).size_full())
                    .into_any_element(),
                None => div().size_full().into_any_element(),
            },
        };

        let ready = matches!(self.state, State::Ready(_));
        let transport = h_flex()
            .flex_none()
            .gap_2()
            .items_center()
            .px_2()
            .py_1()
            .border_t_1()
            .border_color(border)
            .child(
                Button::new("replay-renderer-play")
                    .icon(if self.playing { IconName::Pause } else { IconName::Play })
                    .compact()
                    .disabled(!ready)
                    .tooltip(
                        t!(if self.playing { "ui.replay.renderer.pause" } else { "ui.replay.renderer.play" })
                            .into_owned(),
                    )
                    .on_click(cx.listener(|this, _event, window, cx| this.toggle_playing(window, cx))),
            )
            .child(
                Button::new("replay-renderer-export")
                    .child(crate::icons::icon(crate::icons::DOWNLOAD_SIMPLE))
                    .compact()
                    .disabled(!ready || self.export.is_some())
                    .tooltip(t!("ui.replay.renderer.export_video").into_owned())
                    .on_click(cx.listener(|this, _event, _window, cx| this.export_video(cx))),
            )
            .child(crate::ui::rule_v(cx))
            .child(div().flex_1().min_w(px(0.)).child(Slider::new(&self.seek).disabled(!ready)))
            .child(
                div()
                    .flex_none()
                    .w(CLOCK_WIDTH)
                    .text_xs()
                    .text_color(crate::theme::text_dim())
                    // While an export runs it says how far it has got rather
                    // than where playback is: the transport is held and the
                    // clock would sit still.
                    .child(match self.export {
                        Some(progress) => format!("{} / {}", progress.done, progress.total),
                        None => self.clock_label(),
                    }),
            )
            .children(self.export_failure.as_ref().map(|reason| {
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(rgb(crate::theme::semantic().error))
                    .child(reason.clone())
                    .into_any_element()
            }))
            .child(crate::ui::rule_v(cx))
            .children(SPEEDS.map(|speed| {
                let chosen = (self.speed - speed).abs() < f32::EPSILON;
                crate::ui::selectable(
                    ("replay-renderer-speed", (speed * 10.0) as usize),
                    chosen,
                    Button::new(("replay-renderer-speed-button", (speed * 10.0) as usize))
                        .label(speed_label(speed))
                        .compact()
                        .disabled(!ready)
                        .selected(chosen)
                        .on_click(cx.listener(move |this, _event, window, cx| this.set_speed(speed, window, cx))),
                )
            }));

        v_flex()
            .id("replay-renderer")
            .track_focus(&self.focus_handle)
            .size_full()
            .child(div().flex_1().min_h(px(0.)).child(body))
            .child(transport)
    }
}

/// Room for "MM:SS / MM:SS" without the transport shifting as it counts.
const CLOCK_WIDTH: Pixels = px(86.);

/// Why a battle could not be played back.
#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error(transparent)]
    Preview(#[from] crate::minimap_preview::PreviewError),
}

/// Walks `path`'s battle once, keeping every frame's draw commands and the
/// renderer they are drawn through.
fn bake(
    path: &std::path::Path,
    game_data: &GameDataCache,
    cancel: &AtomicBool,
) -> Result<(Track, SharedPreviewRenderer), RenderError> {
    let baked = crate::minimap_preview::bake_track(path, game_data, cancel, TRACK_BUDGET, BAKE_INTERVAL)?;
    Ok((Track { frames: baked.frames, clocks: baked.clocks }, baked.renderer))
}

/// One rasterised frame, as an image gpui can draw.
fn to_image(frame: image::RgbImage) -> Arc<RenderImage> {
    let (width, height) = frame.dimensions();
    let mut bgra = Vec::with_capacity((width * height * 4) as usize);
    for pixel in frame.pixels() {
        bgra.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
    }
    let buffer = image::RgbaImage::from_raw(width, height, bgra)
        .expect("the buffer is four bytes per pixel of the size it was built at");
    Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]))
}

/// The file an export writes.
const MP4: crate::dialog::Filter = crate::dialog::Filter { label: "MP4", extensions: &["mp4"] };

/// Rasterises a baked track and encodes it to `output`.
///
/// Runs on a background thread: it draws every frame and blocks on the
/// encoder. The renderer is the viewport's own, which is why the caller hands
/// it over for the duration rather than sharing it.
fn encode_track(
    renderer: &SharedPreviewRenderer,
    frames: &[Vec<DrawCommand>],
    duration_seconds: f32,
    output: &std::path::Path,
    progress: futures::channel::mpsc::UnboundedSender<ExportProgress>,
) -> Result<(), String> {
    let mut drawing = renderer.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let (width, height) = drawing.canvas_size();

    let Some(path) = output.to_str() else {
        return Err(t!("ui.replay.renderer.export_path_unusable").into_owned());
    };
    let mut encoder =
        wows_minimap_renderer::VideoEncoder::new(Some(path), None, false, duration_seconds, width, height);
    encoder.init().map_err(|err| err.to_string())?;

    let total = frames.len() as u64;
    for (index, commands) in frames.iter().enumerate() {
        let image = drawing.render(commands);
        encoder.submit_frame(&image).map_err(|err| err.to_string())?;
        let _ = progress.unbounded_send(ExportProgress { done: index as u64 + 1, total });
    }
    encoder.finish_submitted().map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use gpui_kit::TestAppContext;
    use gpui_kit::px;
    use gpui_kit::size;

    use super::ReplayRendererPanel;
    use super::mmss;

    /// The transport reads in the clock the game shows, not in seconds.
    #[test]
    fn game_time_reads_as_minutes_and_seconds() {
        assert_eq!(mmss(0.0), "0:00");
        assert_eq!(mmss(9.4), "0:09");
        assert_eq!(mmss(75.0), "1:15");
        assert_eq!(mmss(1205.0), "20:05");
        // A clock that has gone negative is a bug elsewhere, not a reason to
        // print a minus sign here.
        assert_eq!(mmss(-3.0), "0:00");
    }

    /// Playing advances through the track and stops at the end rather than
    /// running past it, and the clock reads the game time of where it got to.
    #[gpui_kit::test]
    fn playing_walks_the_track_and_stops_at_its_end(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(600.), px(400.)), |window, cx| {
            ReplayRendererPanel::ready_for_test(vec![0.0, 30.0, 61.0], window, cx)
        });

        window
            .update(cx, |panel, window, cx| {
                assert_eq!(
                    panel.clock_label(),
                    "0:00 / 1:01",
                    "it opens at the start, and says how long the battle is"
                );
                panel.toggle_playing(window, cx);
                assert!(panel.playing);
            })
            .expect("the window is open");

        // Two ticks reach the last frame; the third would run past it.
        for _ in 0..3 {
            cx.executor().advance_clock(super::TICK * 2);
            cx.run_until_parked();
        }

        window
            .update(cx, |panel, _window, _cx| {
                assert_eq!(panel.at, 2, "playback reached the last frame");
                assert!(!panel.playing, "and stopped there rather than running past it");
                assert_eq!(panel.clock_label(), "1:01 / 1:01");
            })
            .expect("the window is open");
    }

    /// A viewport with nothing baked yet has no frames to walk, and the
    /// transport must not divide by their count.
    #[gpui_kit::test]
    fn an_empty_track_has_nowhere_to_play_to(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(600.), px(400.)), |window, cx| {
            ReplayRendererPanel::ready_for_test(Vec::new(), window, cx)
        });

        window
            .update(cx, |panel, window, cx| {
                assert_eq!(panel.clock_label(), "0:00 / 0:00");
                panel.toggle_playing(window, cx);
                panel.set_at(5, cx);
                assert_eq!(panel.at, 0, "there is nowhere to seek to");
            })
            .expect("the window is open");
    }
}
