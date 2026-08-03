//! Drawing for the query bar. Every rect arrives already positioned by
//! `layout::lay_out`; nothing here decides where anything goes, and every
//! colour is a theme surface or a semantic role rather than a tuned value.

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
use crate::ui::theme::semantic::SemanticColors;
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

/// One row-slice of a bracketed group. `opens` and `closes` say whether this
/// slice carries the group's leading and trailing edge, so a group whose
/// contents wrap reads as an open-ended bracket on every row it touches.
///
/// Outline only: nesting is carried entirely by the stroke, and a group's
/// interior is the bar's own background. A fill was tried and dropped, because
/// any tone subtle enough to belong to this theme's chrome takes contrast
/// *away* from the pills drawn on top of it -- in the light theme an idle pill
/// on the quietest available fill is 1.02:1, against 1.17:1 on the bar itself.
/// The fill had nothing left to contribute once the stroke carried depth, so
/// there is none.
pub fn group_row(ui: &Ui, rect: Rect, depth: usize, opens: bool, closes: bool) {
    let stroke = depth_stroke(ui.sem(), depth);
    let painter = ui.painter();
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

/// The outline for one nesting level: heavier and brighter at the top,
/// lighter and dimmer below it, so a nested group reads as receding from the
/// one that contains it.
///
/// A group's tokens sit one level deeper than the group's own siblings, so the
/// shallowest bracket the bar ever draws is at depth one, not zero.
///
/// **The ramp stops at two and clamps.** It stops at two because the theme's
/// border family spans 1.64:1 to 2.40:1 against the bar in the dark theme,
/// which is 1.5x of headroom in total: three steps each clearly apart do not
/// fit inside it, and three nominal steps nobody can tell apart are worth less
/// than two real ones. It clamps rather than cycling because a cycle would put
/// the bright, heavy outermost stroke *inside* a dim, light one, which states
/// the nesting backwards; clamping only stops counting. Depth past the ramp is
/// still carried by indentation and by containment.
///
/// `depth_stroke_clears_its_floors` measures what these achieve. It asserts
/// ratios, which is a numeric guarantee and not a claim that anyone has looked
/// at the rendered bar.
fn depth_stroke(sem: &SemanticColors, depth: usize) -> Stroke {
    if depth <= 1 {
        Stroke::new(1.5, sem.bracket.shallow)
    } else {
        // The width the rest of the app draws a border at, so the deeper
        // levels settle into the chrome rather than competing with it.
        Stroke::new(1.0, sem.bracket.deep)
    }
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

    /// Descended from the guard for a real defect: the bracket tint once
    /// resolved to `extreme_bg_color`, the bar's own fill, so the first level
    /// of nesting -- the common single-group case -- was invisible.
    ///
    /// Two lessons from that are kept here now that the stroke carries depth.
    /// Inequality is too weak a test: a later repair paired two tones that
    /// differed by 1.02:1, which is unequal and indistinguishable. And the
    /// shallowest bracket is depth *one*, not zero, so a guard that only
    /// checks depth zero checks a level the bar never draws.
    #[test]
    fn depth_stroke_clears_its_floors() {
        use crate::ui::theme::contrast::CHROME_LINE_FLOOR;
        use crate::ui::theme::contrast::SURFACE_CONTRAST_FLOOR;
        use crate::ui::theme::contrast::contrast_ratio;
        use crate::ui::theme::semantic::semantic;

        for (theme, visuals) in [
            ("dark", crate::ui::theme::style::dark_style().visuals),
            ("light", crate::ui::theme::style::light_style().visuals),
        ] {
            let sem = semantic(&visuals);
            // A group has no fill, so every bracket sits on the bar itself.
            let ground = visuals.extreme_bg_color;
            for depth in 0..=5 {
                let stroke = depth_stroke(sem, depth);
                let ratio = contrast_ratio(stroke.color, ground);
                assert!(
                    ratio >= CHROME_LINE_FLOOR,
                    "{theme} bracket depth {depth} against the bar is {ratio:.3}, needs {CHROME_LINE_FLOOR}"
                );
            }

            let outer = depth_stroke(sem, 1);
            let inner = depth_stroke(sem, 2);
            let apart = contrast_ratio(outer.color, inner.color);
            assert!(
                apart >= SURFACE_CONTRAST_FLOOR,
                "{theme} the two bracket levels are {apart:.3} apart, needs {SURFACE_CONTRAST_FLOOR}"
            );
            assert!(outer.width > inner.width, "{theme} nesting must thin the stroke, not only dim it");
            assert!(
                contrast_ratio(outer.color, ground) > contrast_ratio(inner.color, ground),
                "{theme} nesting must recede from the bar, not advance toward it"
            );
        }
    }

    /// The crowding decision, pinned so it is a choice rather than a drift:
    /// past depth two the ramp clamps to its dimmest step. Cycling would draw
    /// the bright, heavy outermost stroke inside a dim one and state the
    /// nesting backwards.
    #[test]
    fn nesting_past_the_ramp_clamps_rather_than_cycling() {
        use crate::ui::theme::semantic::semantic;

        for (theme, visuals) in [
            ("dark", crate::ui::theme::style::dark_style().visuals),
            ("light", crate::ui::theme::style::light_style().visuals),
        ] {
            let sem = semantic(&visuals);
            let deepest = depth_stroke(sem, 2);
            for depth in 3..=8 {
                let stroke = depth_stroke(sem, depth);
                assert_eq!(stroke.color, deepest.color, "{theme} depth {depth} should clamp");
                assert!((stroke.width - deepest.width).abs() < f32::EPSILON, "{theme} depth {depth} should clamp");
            }
            assert_ne!(
                depth_stroke(sem, 3).color,
                depth_stroke(sem, 1).color,
                "{theme} a clamp must never return to the outermost stroke, which is what cycling would do"
            );
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
