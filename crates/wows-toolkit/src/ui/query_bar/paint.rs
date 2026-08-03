//! Drawing for the query bar. Every rect arrives already positioned by
//! `layout::lay_out`; nothing here decides where anything goes, and every
//! colour is a theme surface or a semantic role rather than a tuned value.

// Consumed by the bar widget in this module's `mod.rs`; nothing outside the
// query bar draws these.
#![allow(dead_code)]

use std::ops::Range;
use std::sync::Arc;

use egui::Color32;
use egui::CornerRadius;
use egui::Galley;
use egui::Pos2;
use egui::Rect;
use egui::Stroke;
use egui::StrokeKind;
use egui::Ui;
use egui::text::CCursor;

use crate::ui::query_bar::tokens::TokenKind;
use crate::ui::theme::semantic::SemanticExt;

/// Corner radius shared by the bar's frame and its pills.
pub const RADIUS: u8 = 4;

/// Horizontal breathing room inside a pill, and the amount `mod.rs` adds to a
/// measured token when it asks `lay_out` for a width.
pub const PAD_X: f32 = 6.0;

/// How a token reads under the pointer and the current selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenState {
    Idle,
    Hovered,
    Selected,
}

/// The bar's own background and border. A focused bar takes the accent stroke
/// so it reads as the active control the way a plain `TextEdit` would.
pub fn bar_frame(ui: &Ui, focused: bool) -> egui::Frame {
    let visuals = ui.visuals();
    let stroke = if focused { visuals.selection.stroke } else { visuals.widgets.inactive.bg_stroke };
    egui::Frame::new()
        .fill(visuals.extreme_bg_color)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(RADIUS))
        .inner_margin(4)
}

/// Draws one token. Exhaustive over `TokenKind` so a new kind cannot slip
/// through unpainted.
pub fn token(ui: &Ui, rect: Rect, galley: Arc<Galley>, kind: &TokenKind, state: TokenState) {
    match kind {
        TokenKind::Pill { .. } => pill(ui, rect, galley, state),
        TokenKind::NotPrefix => text_run(ui, rect, galley, ui.sem().notice),
        TokenKind::Connector { .. }
        | TokenKind::GroupOpen { .. }
        | TokenKind::GroupClose
        | TokenKind::QuantOpen { .. }
        | TokenKind::QuantClose => text_run(ui, rect, galley, chrome_color(ui, state)),
        // The caret is a real `TextEdit`, which draws itself.
        TokenKind::Caret => {}
    }
}

fn pill(ui: &Ui, rect: Rect, galley: Arc<Galley>, state: TokenState) {
    let visuals = ui.visuals();
    let (fill, text) = match state {
        TokenState::Idle => (visuals.widgets.inactive.bg_fill, visuals.text_color()),
        TokenState::Hovered => (visuals.widgets.hovered.bg_fill, visuals.strong_text_color()),
        TokenState::Selected => (visuals.selection.bg_fill, visuals.strong_text_color()),
    };
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(RADIUS), fill);
    if state == TokenState::Selected {
        painter.rect_stroke(rect, CornerRadius::same(RADIUS), visuals.selection.stroke, StrokeKind::Inside);
    }
    text_run(ui, rect, galley, text);
}

/// One row-slice of a bracketed group, drawn behind the tokens it holds.
/// `opens` and `closes` say whether this slice carries the group's leading and
/// trailing edge, so a group whose contents wrap reads as an open-ended bracket
/// on every row it touches.
pub fn group_row(ui: &Ui, rect: Rect, depth: usize, opens: bool, closes: bool) {
    let stroke = ui.visuals().widgets.noninteractive.bg_stroke;
    let fill = depth_fill(ui.visuals(), depth);
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::ZERO, fill);
    painter.line_segment([rect.left_top(), rect.right_top()], stroke);
    painter.line_segment([rect.left_bottom(), rect.right_bottom()], stroke);
    if opens {
        painter.line_segment([rect.left_top(), rect.left_bottom()], stroke);
    }
    if closes {
        painter.line_segment([rect.right_top(), rect.right_bottom()], stroke);
    }
}

/// Underlines exactly the substring a parse error names.
///
/// `span` is the byte range the grammar reported and `text` is the caret's
/// contents, so the two agree by construction. A span the text edit renders as
/// zero-width (the parse died at end of input) still gets a visible stub,
/// because an invisible error report is the same as no report.
pub fn error_underline(ui: &Ui, galley_pos: Pos2, galley: &Galley, text: &str, span: &Range<usize>) {
    let start = galley.pos_from_cursor(CCursor::new(char_index(text, span.start)));
    let end = galley.pos_from_cursor(CCursor::new(char_index(text, span.end)));
    let left = galley_pos + start.left_bottom().to_vec2();
    let mut right = galley_pos + end.left_bottom().to_vec2();
    right.x = right.x.max(left.x + PAD_X);
    ui.painter().line_segment([left, right], Stroke::new(1.5, ui.sem().error));
}

/// Alternating surface behind each nesting level.
///
/// A group's tokens sit one level deeper than the group's own siblings, so the
/// shallowest bracket the bar ever draws is at depth one, not zero. Neither
/// surface may be `extreme_bg_color`, which is the bar's own fill, nor
/// `widgets.inactive.bg_fill`, which is a pill's: a group or a pill painted in
/// the colour behind it is invisible. `depth_fill_is_visible_against_the_bar`
/// pins all three properties against the shipped themes.
fn depth_fill(visuals: &egui::Visuals, depth: usize) -> Color32 {
    if depth.is_multiple_of(2) { visuals.panel_fill } else { visuals.faint_bg_color }
}

fn chrome_color(ui: &Ui, state: TokenState) -> Color32 {
    match state {
        TokenState::Idle => ui.sem().text_dim,
        TokenState::Hovered | TokenState::Selected => ui.visuals().strong_text_color(),
    }
}

fn text_run(ui: &Ui, rect: Rect, galley: Arc<Galley>, color: Color32) {
    ui.painter().galley(text_origin(rect, &galley), galley, color);
}

/// Left-aligns and vertically centres a run inside the rect the layout pass
/// already sized for it.
fn text_origin(rect: Rect, galley: &Galley) -> Pos2 {
    Pos2::new(rect.left() + PAD_X, rect.center().y - galley.size().y * 0.5)
}

/// Characters before `byte`, so a byte span from the grammar becomes the
/// character index `CCursor` takes. Counting avoids slicing the string, which
/// would panic on a span that lands mid-character.
fn char_index(text: &str, byte: usize) -> usize {
    text.char_indices().take_while(|(i, _)| *i < byte).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The regression guard for a real defect: `depth_fill` returned
    /// `extreme_bg_color` on odd depths, and that is the bar's own fill, so the
    /// first level of nesting -- the common single-group case -- painted a
    /// rectangle identical to the background and no bracket tint was visible
    /// at all.
    #[test]
    fn depth_fill_is_visible_against_the_bar_and_against_a_pill() {
        for (theme, visuals) in [
            ("dark", crate::ui::theme::style::dark_style().visuals),
            ("light", crate::ui::theme::style::light_style().visuals),
        ] {
            // Depth zero is never a bracket, but is covered so a future change
            // to how `tokenize` assigns depth cannot reintroduce the defect.
            for depth in 0..=4 {
                let fill = depth_fill(&visuals, depth);
                assert_ne!(fill, visuals.extreme_bg_color, "{theme} depth {depth} matches the bar's own fill");
                assert_ne!(
                    fill, visuals.widgets.inactive.bg_fill,
                    "{theme} depth {depth} matches a pill, so a pill inside it would vanish"
                );
                assert_ne!(
                    fill,
                    depth_fill(&visuals, depth + 1),
                    "{theme} depth {depth} and the level inside it are the same colour"
                );
            }
        }
    }

    #[test]
    fn a_byte_span_converts_to_a_character_index_without_slicing() {
        // "aa\u{e9}b": the accented character occupies two bytes, so the byte
        // offset of the final `b` is 4 while its character index is 3.
        let text = "aa\u{e9}b";
        assert_eq!(char_index(text, 0), 0);
        assert_eq!(char_index(text, 2), 2);
        assert_eq!(char_index(text, 4), 3);
        assert_eq!(char_index(text, text.len()), 4);
    }

    #[test]
    fn an_offset_inside_a_character_or_past_the_end_resolves_instead_of_panicking() {
        // The grammar reports byte offsets against its own input, so a span
        // landing mid-character is a bug elsewhere; it must still not take the
        // bar down on the keystroke that produces it. Byte 3 sits inside the
        // two-byte accented character, and resolves to the boundary after it.
        let text = "aa\u{e9}b";
        assert_eq!(char_index(text, 3), 3);
        assert_eq!(char_index(text, 999), 4);
    }
}
