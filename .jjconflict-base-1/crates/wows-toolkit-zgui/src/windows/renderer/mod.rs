//! A replay played back on its own minimap, at whatever speed the reader asks for.
//!
//! The bake, the playback clock, the drawing tools and the export are the shared
//! `wows_toolkit_replay_data::replay_render`; this is the zgui half. A battle is baked to draw
//! commands once, and one frame at a time is rasterised off the UI thread and shown through a
//! [`FrameView`](crate::frames::FrameView): the newest frame asked for wins, and the frame on
//! screen stays up while the next is drawn.
//!
//! A host opens a [`RendererSession`] for a [`RenderRequest`] and shows it with
//! [`view::ReplayRenderer`]:
//!
//! ```text
//! let session = RendererSession::open(RenderRequest { path, alts }, collab.link());
//! view! { ReplayRenderer(session = session) }
//! ```
//!
//! The session outlives the view, so the pop-out control moves it into a window of its own
//! without baking again. The host follows `session.popped_out` to take its tab away, and calls
//! [`RendererSession::close`] when the tab is closed; a popped-out session closes with its window.
//! [`watched::WatchedPlayback`] shows a battle a collab peer is playing back instead.

#[cfg(test)]
mod annotate_tests;
mod export;
mod hover;
mod options;
#[cfg(test)]
mod options_tests;
mod overlays;
#[cfg(test)]
mod renderer_tests;
pub mod session;
mod ship_menu;
#[cfg(test)]
mod shot_tests;
mod timeline;
mod tools;
pub mod view;
pub mod watched;

use wows_toolkit_config::WindowKind;
use zgui::prelude::WindowHandle;
use zgui::prelude::WindowOptions;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::GetUntracked;

use crate::window_shell::RememberGeometryProps;
use crate::windows::renderer::session::RendererSession;
use crate::windows::renderer::view::ReplayRendererProps;

/// Where a renderer's window opens when nothing was remembered, and the smallest it may be made.
const DEFAULT_SIZE: (f32, f32) = (1000.0, 860.0);
const MIN_SIZE: (f32, f32) = (480.0, 420.0);

/// Moves `session` into a window of its own. The session closes with the window.
///
/// Called from a listener inside the main window.
pub fn pop_out(session: RendererSession) -> Option<WindowHandle> {
    if !session.is_open() || session.popped_out.get_untracked() {
        return None;
    }
    let saved = session.remembered_window().unwrap_or_default();
    let [width, height] = saved.inner_size_points.unwrap_or([DEFAULT_SIZE.0, DEFAULT_SIZE.1]);
    let mut options =
        WindowOptions::new(session.title()).with_size(width, height).with_min_size(MIN_SIZE.0, MIN_SIZE.1);
    if let Some([x, y]) = saved.outer_position_pixels {
        options = options.with_position(x, y);
    }
    session.mark_popped_out();
    let handle = crate::windows::open(options, move || {
        on_cleanup_local(move || session.close());
        zgui::view! {
            RememberGeometry(kind = WindowKind::ReplayRenderer)
            ReplayRenderer(session = session)
        }
    });
    if saved.maximized {
        handle.set_maximized(true);
    }
    Some(handle)
}
