//! Debug-mode raw JSON viewer: one pretty-printed payload (the replay header
//! metadata or the battle results; see `load::ParsedReplay`).
//!
//! Mirrors the egui app's debug-mode "Raw Metadata" / "View Results > Raw
//! JSON" viewers (`ui/replay_parser/mod.rs` ~3018-3060), which open the same
//! text in a standalone `PlaintextFileViewer` window; this port shows it in
//! the per-replay side-panel slot instead, the same tradeoff `chat.rs`
//! documents for the chat window.
//!
//! A read-only editor rather than a column of labels: it highlights the
//! payload as the JSON it is, and it can be selected, searched and copied,
//! none of which painted glyphs can do.

use gpui_kit::component::input::Editor;
use gpui_kit::component::input::EditorState;
use gpui_kit::*;

/// The payload is read, not edited, so it is drawn at the app's own small
/// size rather than an editor's default.
const RAW_JSON_TEXT_SIZE: Pixels = px(12.);

pub struct RawJsonPanel {
    editor: Entity<EditorState>,
}

impl RawJsonPanel {
    pub fn new(content: SharedString, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let editor = cx.new(|cx| EditorState::new(window, cx).language("json").default_value(content.to_string()));
        Self { editor }
    }
}

impl Render for RawJsonPanel {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        Editor::new(&self.editor).readonly(true).text_size(RAW_JSON_TEXT_SIZE).size_full()
    }
}
