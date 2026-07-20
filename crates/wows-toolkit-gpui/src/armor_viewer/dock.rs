//! Milestone 5 Task 9a's dock scaffold: a chrome-less container around the
//! Armor Viewer's viewport pane(s), owned by `ArmorViewerPane` in place of a
//! bare `Entity<ViewportView>`.
//!
//! This deliberately does not wire a real gpui-component `dock::DockArea`/
//! `TabPanel`, even though the Replay Inspector's dock (`replay_inspector::
//! view::ReplayInspectorView`) is the reference pattern: `TabPanel::
//! render_title_bar` always draws a 30px title bar over the active panel --
//! even with exactly one tab and `PanelStyle::default()` -- with no way to
//! suppress it (see that crate's `tab_panel.rs`). Task 9a is required to be a
//! no-visible-behavior-change refactor, so introducing that title bar here
//! would violate the brief. `ViewportDock` is the seam Task 9b extends: it
//! turns `panes` into an actual multi-pane split (a "Compare" action, close
//! buttons, resizable layout) once there is a reason to show tab chrome, or
//! lays panes out itself if that chrome is still unwanted for N panes.
//!
//! For this task there is exactly one pane, and this container renders it
//! filling the whole area exactly as the bare `ViewportView` did before.

use gpui::*;

use super::viewport_view::ViewportView;

pub struct ViewportDock {
    panes: Vec<Entity<ViewportView>>,
}

impl ViewportDock {
    /// Task 9a: wraps the single viewport the pane already owns. Task 9b
    /// adds a way to push additional panes onto `panes` (and the "Compare"
    /// action that calls it).
    pub fn new(viewport: Entity<ViewportView>) -> Self {
        Self { panes: vec![viewport] }
    }
}

impl Render for ViewportDock {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        // Task 9a: a single pane fills the container, matching the
        // pre-scaffold layout (`ArmorViewerPane` used to embed the viewport
        // directly). Task 9b lays out multiple panes here.
        div().size_full().children(self.panes.iter().cloned())
    }
}
