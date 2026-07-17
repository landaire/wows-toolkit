//! Debug-mode raw JSON viewer: a scrollable monospace panel showing one
//! pretty-printed JSON payload (replay header metadata or battle results;
//! see `load::ParsedReplay`). Mirrors the egui app's debug-mode "Raw
//! Metadata" / "View Results > Raw JSON" viewers (`ui/replay_parser/mod.rs`
//! ~3018-3060), which open the same pretty-printed text in a standalone
//! `PlaintextFileViewer` window; this port shows it inline in the per-replay
//! side-panel slot instead, the same v1 tradeoff `chat.rs` documents for the
//! chat window.

use gpui::*;
use gpui_component::ActiveTheme;
use gpui_component::scroll::Scrollbar;
use gpui_component::v_flex;

/// One JSON payload's scrollable text view, split into one child `div` per
/// line so gpui lays out each line individually rather than wrapping the
/// whole payload into a single unbroken text node (mirrors
/// `table.rs::hover_tooltip`'s per-line-`div` convention for the same
/// reason).
pub struct RawJsonPanel {
    /// Split into lines once, at construction, rather than on every render:
    /// `content` never changes after `new` (this panel is rebuilt, not
    /// mutated, on a new payload -- see `panel.rs`), so re-splitting it and
    /// heap-allocating a fresh `String` per line on every repaint (a JSON
    /// payload can run thousands of lines) was pure per-frame waste. Cloning
    /// a `SharedString` in `render` is a refcount bump, not a copy.
    lines: Vec<SharedString>,
    scroll: ScrollHandle,
}

impl RawJsonPanel {
    pub fn new(content: SharedString, _cx: &mut Context<Self>) -> Self {
        let lines = content.split('\n').map(|line| SharedString::from(line.to_string())).collect();
        Self { lines, scroll: ScrollHandle::new() }
    }
}

impl Render for RawJsonPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mono_font_family = cx.theme().mono_font_family.clone();

        let body = div().id("raw-json-body").size_full().overflow_y_scroll().track_scroll(&self.scroll).child(
            v_flex()
                .w_full()
                .p_2()
                .gap_0()
                .text_xs()
                .font_family(mono_font_family)
                .children(self.lines.iter().cloned().map(|line| div().child(line))),
        );

        div().relative().size_full().child(body).child(Scrollbar::vertical(&self.scroll))
    }
}
