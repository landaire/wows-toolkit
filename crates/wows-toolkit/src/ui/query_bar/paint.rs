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

use crate::ui::query_bar::layout;
use crate::ui::query_bar::layout::LayoutCfg;
use crate::ui::query_bar::tokens::TokenKind;
use crate::ui::theme::semantic::SemanticColors;
use crate::ui::theme::semantic::SemanticExt;

/// Corner radius shared by the bar's frame and its pills.
pub const RADIUS: u8 = 4;

/// Horizontal breathing room inside a pill, and the amount `mod.rs` adds to a
/// measured token when it asks `lay_out` for a width.
pub const PAD_X: f32 = 6.0;

/// How strongly the hovered segment's background stands out from the base
/// pill fill it sits on. Kept low: the base fill already carries the pill's
/// idle/hovered/selected state, so this only has to mark which segment the
/// pointer is over, not repeat that state.
const SEGMENT_HOVER_ALPHA: f32 = 0.14;

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
/// through unpainted. `galleys` carries one run per segment for a `Pill` and
/// a single run for every other kind.
pub fn token(ui: &Ui, rect: Rect, galleys: &[Arc<Galley>], kind: &TokenKind, state: TokenState, cfg: &LayoutCfg) {
    match kind {
        TokenKind::Pill { .. } => pill(ui, rect, galleys, state, cfg),
        TokenKind::NotPrefix => text_run(ui, rect, galleys[0].clone(), ui.sem().notice),
        TokenKind::Connector { .. }
        | TokenKind::GroupOpen { .. }
        | TokenKind::GroupClose
        | TokenKind::QuantOpen { .. }
        | TokenKind::QuantClose => text_run(ui, rect, galleys[0].clone(), chrome_color(ui, state)),
        // The caret is a real `TextEdit`, which draws itself.
        TokenKind::Caret => {}
    }
}

/// The pill's background and outline are painted once, over the whole rect,
/// so a pill's selection fill reads as one continuous surface rather than a
/// strip per segment. Each segment then paints its own run inside the rect
/// `segment_rects` gives it, with a separator between adjacent segments and a
/// background of its own when the pointer sits over it -- the click target
/// `select`'s Task 7 caller will interact, made discoverable ahead of that.
fn pill(ui: &Ui, rect: Rect, galleys: &[Arc<Galley>], state: TokenState, cfg: &LayoutCfg) {
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

    let segment_widths: Vec<f32> = galleys.iter().map(|g| g.size().x + 2.0 * PAD_X).collect();
    let segments = segment_rects(rect, &segment_widths, cfg);
    let hover_fill = ui.sem().text_strong.gamma_multiply(SEGMENT_HOVER_ALPHA);
    let separator = Stroke::new(1.0, ui.sem().bracket.deep);

    for (i, (seg_rect, galley)) in segments.iter().zip(galleys).enumerate() {
        if i > 0 {
            let sep_x = seg_rect.left() - cfg.segment_gap * 0.5;
            painter.line_segment([Pos2::new(sep_x, rect.top()), Pos2::new(sep_x, rect.bottom())], separator);
        }
        if ui.rect_contains_pointer(*seg_rect) {
            painter.rect_filled(*seg_rect, CornerRadius::same(RADIUS), hover_fill);
        }
        text_run(ui, *seg_rect, galley.clone(), text);
    }
}

/// One rect per segment, positioned inside `pill_rect` by
/// `layout::segment_offsets`. `pill_rect`'s own width must already equal
/// `layout::pill_width(segment_widths, cfg)` -- `mod.rs` sizes a pill token
/// that way -- so every returned rect lands inside `pill_rect` with nothing
/// left over. Pure arithmetic: this is the half of segment hit-testing that
/// does not need a rendered frame to check, and is what Task 7 will
/// `ui.interact` against.
pub fn segment_rects(pill_rect: Rect, segment_widths: &[f32], cfg: &LayoutCfg) -> Vec<Rect> {
    layout::segment_offsets(segment_widths, cfg)
        .into_iter()
        .map(|(x, w)| {
            Rect::from_min_max(
                Pos2::new(pill_rect.left() + x, pill_rect.top()),
                Pos2::new(pill_rect.left() + x + w, pill_rect.bottom()),
            )
        })
        .collect()
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

    fn segment_rects_cfg() -> LayoutCfg {
        LayoutCfg { row_height: 20.0, gap: 4.0, indent: 8.0, max_rows: 6, min_segment_width: 16.0, segment_gap: 4.0 }
    }

    /// Builds the pill rect the same way `mod.rs` would size the token: wide
    /// enough for `layout::pill_width(widths, cfg)`, at an origin away from
    /// `(0, 0)` so a derivation that forgets to add the pill's own offset
    /// cannot pass by accident.
    fn pill_rect_for(widths: &[f32], cfg: &LayoutCfg) -> Rect {
        Rect::from_min_size(Pos2::new(137.0, 41.0), egui::vec2(layout::pill_width(widths, cfg), 20.0))
    }

    #[test]
    fn one_rect_per_segment_in_order() {
        let cfg = segment_rects_cfg();
        let widths = [30.0, 20.0, 40.0];
        let rects = segment_rects(pill_rect_for(&widths, &cfg), &widths, &cfg);
        assert_eq!(rects.len(), widths.len(), "got {rects:?}");
    }

    #[test]
    fn every_segment_rect_is_contained_in_the_pill_rect() {
        let cfg = segment_rects_cfg();
        let widths = [30.0, 20.0, 40.0];
        let pill_rect = pill_rect_for(&widths, &cfg);
        let rects = segment_rects(pill_rect, &widths, &cfg);
        for r in &rects {
            assert!(pill_rect.contains_rect(*r), "segment {r:?} escapes the pill {pill_rect:?}");
        }
    }

    #[test]
    fn segment_rects_do_not_overlap() {
        let cfg = segment_rects_cfg();
        let widths = [30.0, 20.0, 40.0];
        let rects = segment_rects(pill_rect_for(&widths, &cfg), &widths, &cfg);
        for pair in rects.windows(2) {
            assert!(pair[1].left() + f32::EPSILON >= pair[0].right(), "segments overlap: {rects:?}");
        }
    }

    #[test]
    fn segment_rects_span_the_pill_edge_to_edge() {
        let cfg = segment_rects_cfg();
        let widths = [30.0, 20.0, 40.0];
        let pill_rect = pill_rect_for(&widths, &cfg);
        let rects = segment_rects(pill_rect, &widths, &cfg);
        let first = rects.first().expect("at least one segment");
        let last = rects.last().expect("at least one segment");
        assert!((first.left() - pill_rect.left()).abs() < f32::EPSILON, "first segment {first:?} vs {pill_rect:?}");
        assert!((last.right() - pill_rect.right()).abs() < f32::EPSILON, "last segment {last:?} vs {pill_rect:?}");
    }

    /// `MatchTerm::FreeText` (`label.rs`) always yields exactly one segment,
    /// so this shape ships and must reduce to the pill rect itself rather than
    /// a narrower inset copy of it.
    #[test]
    fn a_single_segment_pill_yields_one_rect_equal_to_the_pill_rect() {
        let cfg = segment_rects_cfg();
        let widths = [30.0];
        let pill_rect = pill_rect_for(&widths, &cfg);
        let rects = segment_rects(pill_rect, &widths, &cfg);
        assert_eq!(rects.len(), 1, "got {rects:?}");
        let only = rects[0];
        assert!((only.left() - pill_rect.left()).abs() < f32::EPSILON);
        assert!((only.right() - pill_rect.right()).abs() < f32::EPSILON);
        assert!((only.top() - pill_rect.top()).abs() < f32::EPSILON);
        assert!((only.bottom() - pill_rect.bottom()).abs() < f32::EPSILON);
    }
}
