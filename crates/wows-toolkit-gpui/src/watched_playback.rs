//! A battle someone else is playing back.
//!
//! A peer with no copy of the replay cannot open it, so the end of the session
//! that has it sends what it draws. A frame on that wire is a list of draw
//! commands rather than an image, so this draws the battle with its own art at
//! whatever size its own window is, and a peer on a different build sees the same
//! battle rather than a picture of someone else's window.
//!
//! The egui app opens the same thing from the same button
//! (`launch_client_renderer`). What it does not do here is steer: the clock
//! belongs to the end that owns the replay, and this reports where that end has
//! reached rather than offering a transport of its own.

use std::sync::Arc;

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::dock::BasePanel;
use gpui_kit::component::dock::Panel;
use gpui_kit::component::dock::PanelEvent;
use gpui_kit::component::h_flex;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rust_i18n::t;
use wows_minimap_renderer::draw_command::DrawCommand;

use crate::collab::ReplayId;
use crate::replay_inspector::GameDataCache;

/// How often the frames the session has sent are looked for.
///
/// This is the mechanism rather than a fallback: the shared sink's waker is a
/// plain callback, which cannot reach a gpui entity without a channel into the
/// UI thread that this is already draining. Watching someone else's playback is
/// worth a tenth of a second of lateness.
const DRAIN: std::time::Duration = std::time::Duration::from_millis(100);

/// What the end that owns the replay says about where it has reached.
#[derive(Clone, Copy, Debug, PartialEq)]
struct HostClock {
    seconds: f32,
    frame: usize,
    of: usize,
}

impl HostClock {
    /// How far through the battle it is, as a fraction.
    ///
    /// `None` before the host has said how many frames there are, which is not
    /// the same as a battle with none.
    fn through(self) -> Option<f32> {
        (self.of > 0).then(|| (self.frame as f32 / self.of as f32).clamp(0.0, 1.0))
    }
}

/// A viewport onto a battle the session is playing elsewhere.
pub struct WatchedPlayback {
    /// Which of the session's windows this is, which is what its frames are
    /// addressed to and what its sink is registered under.
    replay_id: ReplayId,
    title: SharedString,
    /// The map's space name, for drawing with this build's own art where it has
    /// it.
    space: String,
    /// The art the owning end sent, for a map this build ships none of.
    art: Option<Arc<image::RgbImage>>,
    game_data: Option<GameDataCache>,
    /// Where the frames arrive. Bounded, and the far end drops rather than
    /// blocks: a frame nobody has drawn yet is already stale.
    frames: std::sync::mpsc::Receiver<wt_collab_client::PlaybackFrame>,
    /// The commands the last frame carried, and where the host was when it drew
    /// them.
    latest: Option<(Vec<DrawCommand>, HostClock)>,
    /// The frame as drawn. `None` until one has arrived and been rasterised.
    drawn: Option<Arc<RenderImage>>,
    /// One raster at a time: the cost is a map-sized image, and frames arrive
    /// faster than that on a fast playback. A frame that arrives during one is
    /// drawn when it finishes.
    rasterising: bool,
    stale: bool,
    /// Kept alive to keep draining; dropped with the panel.
    _tick: Option<Task<()>>,
    focus_handle: FocusHandle,
    /// The session this is watching, for taking the sink out again.
    collab: crate::collab::CollabLink,
}

impl WatchedPlayback {
    /// Opens a viewport on one of the session's windows and asks for its frames.
    pub fn new(
        shared: crate::collab::SharedWindow,
        collab: crate::collab::CollabLink,
        game_data: Option<GameDataCache>,
        cx: &mut Context<Self>,
    ) -> Self {
        // Bounded: the owning end drops a frame it cannot hand over rather than
        // waiting, so a depth of a few frames is a cushion against a slow raster
        // and not a queue to be played out later.
        let (tx, frames) = std::sync::mpsc::sync_channel(4);
        let replay_id = ReplayId::from_raw(shared.replay_id);
        collab.watch_window(replay_id, tx);

        let art = shared.art_png.as_ref().and_then(|png| match image::load_from_memory(png) {
            Ok(art) => Some(Arc::new(art.to_rgb8())),
            // Art that will not decode leaves this drawing with the build's own,
            // which is what a peer that has the map does anyway.
            Err(err) => {
                tracing::warn!("watched playback: the art the host sent could not be read: {err}");
                None
            }
        });

        let mut watching = Self {
            replay_id,
            title: shared.replay_name.into(),
            space: shared.map_name,
            art,
            game_data,
            frames,
            latest: None,
            drawn: None,
            rasterising: false,
            stale: false,
            _tick: None,
            focus_handle: cx.focus_handle(),
            collab,
        };
        watching.follow(cx);
        watching
    }

    /// Looks for frames until the panel goes.
    fn follow(&mut self, cx: &mut Context<Self>) {
        self._tick = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(DRAIN).await;
                if this.update(cx, |this, cx| this.take_frames(cx)).is_err() {
                    return;
                }
            }
        }));
    }

    /// Takes whatever has arrived, keeping only the newest.
    ///
    /// Only the newest: the others are moments the host has already left, and
    /// drawing them would put this viewport behind rather than beside it.
    pub(crate) fn take_frames(&mut self, cx: &mut Context<Self>) {
        let mut newest = None;
        while let Ok(frame) = self.frames.try_recv() {
            newest = Some(frame);
        }
        let Some(frame) = newest else { return };
        self.latest = Some((
            frame.commands,
            HostClock { seconds: frame.clock.0, frame: frame.frame_index, of: frame.total_frames },
        ));
        self.redraw(cx);
    }

    /// Draws the frame this viewport last had.
    fn redraw(&mut self, cx: &mut Context<Self>) {
        let Some((commands, _)) = self.latest.clone() else { return };
        let Some(game_data) = self.game_data.clone() else {
            // Nothing to draw the commands over: the fonts and icons a frame
            // needs are this build's own.
            cx.notify();
            return;
        };
        if self.rasterising {
            self.stale = true;
            return;
        }
        self.rasterising = true;

        let (space, art, replay_id) = (self.space.clone(), self.art.clone(), self.replay_id);
        cx.spawn(async move |this, cx| {
            let drawn = cx
                .background_spawn(async move {
                    let view = wows_minimap_renderer::viewport::MapViewport::default();
                    match art {
                        Some(art) => crate::minimap_preview::render_map_art(
                            &format!("watched-{}", replay_id.raw()),
                            &art,
                            &game_data,
                            view,
                            &commands,
                        ),
                        None => crate::minimap_preview::render_map(&space, &game_data, view, &commands),
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.rasterising = false;
                if drawn.is_some() {
                    this.drawn = drawn;
                }
                if std::mem::take(&mut this.stale) {
                    this.redraw(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Where the end that owns the replay has reached, as a strip to read.
    fn render_clock(&self, cx: &App) -> AnyElement {
        let Some((_, clock)) = &self.latest else {
            return div().into_any_element();
        };
        let seconds = clock.seconds.max(0.0);
        let read = format!("{}:{:02}", (seconds / 60.0).floor() as i32, (seconds % 60.0) as i32);
        h_flex()
            .flex_none()
            .gap_2()
            .items_center()
            .px_2()
            .py_1()
            .text_xs()
            .text_color(crate::theme::text_dim())
            .child(div().child(t!("ui.collab.watching").to_string()))
            .child(div().child(read))
            .children(clock.through().map(|through| {
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .h(px(3.))
                    .rounded(cx.theme().radius)
                    .bg(cx.theme().border)
                    .child(div().h_full().w(relative(through)).rounded(cx.theme().radius).bg(cx.theme().primary))
            }))
            .into_any_element()
    }
}

impl Drop for WatchedPlayback {
    fn drop(&mut self) {
        // The session stops sending frames nobody is drawing.
        self.collab.stop_watching(self.replay_id);
    }
}

impl BasePanel for WatchedPlayback {
    fn panel_name(&self) -> &'static str {
        "WatchedPlayback"
    }

    fn closable(&self, _cx: &App) -> bool {
        true
    }
}

impl Panel for WatchedPlayback {
    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.title.clone()
    }
}

impl EventEmitter<PanelEvent> for WatchedPlayback {}

impl Focusable for WatchedPlayback {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for WatchedPlayback {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().border;
        v_flex()
            .size_full()
            .track_focus(&self.focus_handle)
            .child(self.render_clock(cx))
            .child(div().h(px(1.)).bg(border))
            .when_some(self.drawn.clone(), |this, drawn| {
                this.child(div().flex_1().min_h(px(0.)).child(img(drawn).size_full().object_fit(ObjectFit::Contain)))
            })
            .when(self.drawn.is_none(), |this| {
                this.child(
                    v_flex()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .text_sm()
                        .text_color(crate::theme::text_dim())
                        .child(t!("ui.collab.waiting_for_frames").to_string()),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::HostClock;

    /// How far through reads as a fraction, and says nothing at all before the
    /// host has said how long the battle is.
    #[test]
    fn progress_needs_a_length_to_be_a_fraction() {
        assert_eq!(HostClock { seconds: 0.0, frame: 0, of: 0 }.through(), None);
        assert_eq!(HostClock { seconds: 10.0, frame: 5, of: 10 }.through(), Some(0.5));
        assert_eq!(HostClock { seconds: 10.0, frame: 0, of: 10 }.through(), Some(0.0));
    }

    /// A frame past the end reads as the end rather than as more than all of it.
    #[test]
    fn a_frame_past_the_end_reads_as_the_end() {
        assert_eq!(HostClock { seconds: 10.0, frame: 30, of: 10 }.through(), Some(1.0));
    }
}
