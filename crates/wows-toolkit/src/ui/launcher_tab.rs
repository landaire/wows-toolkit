//! The Launcher tab: discover, launch, and manage WoWs installs.

use crate::app::ToolkitTabViewer;

impl ToolkitTabViewer<'_> {
    pub fn build_launcher_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Launcher");
        ui.label("Install discovery and launching wire up in Tasks 17-18.");
    }
}
