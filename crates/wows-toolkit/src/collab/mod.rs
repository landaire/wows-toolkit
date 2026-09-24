//! Collaborative replay sessions.
//!
//! The session itself lives in `wt-collab-client`, shared with the GPUI port
//! so both front ends drive one implementation. What stays here is the part
//! that is this app's own: turning its persisted render options into the wire
//! form, and waking its own viewports.

pub mod protocol;

pub use wt_collab_client::*;

use std::sync::Arc;

/// Wakes the egui app when the peer task changes the session.
pub struct EguiWaker(pub egui::Context);

impl wt_collab_client::SessionWaker for EguiWaker {
    fn wake(&self) {
        self.0.request_repaint();
    }
}

/// How to wake one viewport, for the sink that belongs to it.
///
/// A viewport is named by its own id rather than by the session's window id,
/// which is why the wake is handed over rather than derived.
pub fn viewport_wake(ctx: &egui::Context, viewport_id: egui::ViewportId) -> wt_collab_client::WindowWake {
    let ctx = ctx.clone();
    Arc::new(move || ctx.request_repaint_of(viewport_id))
}
