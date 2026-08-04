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
/// pill fill it sits on. Composited over any fill a pill can have (idle,
/// hovered, or selected) this clears `contrast::SURFACE_CONTRAST_FLOOR`
/// against that fill -- `segment_hover_wash_clears_every_pill_fill` pins it --
/// and is kept no higher than that: the base fill already carries the pill's
/// own state, so this only has to mark which segment the pointer is over, not
/// repeat that state.
const SEGMENT_HOVER_ALPHA: f32 = 0.14;

/// How thick the line dividing two settled segments of one pill is drawn.
const SEGMENT_SEPARATOR_WIDTH: f32 = 1.0;

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
        TokenKind::Pill { .. } => pill(ui, rect, galleys, state, cfg, None),
        // `galleys` always carries exactly one run for these kinds (`mod.rs`'s
        // `token_galleys` builds it that way); `first()` skips painting rather
        // than trusting that invariant with an index that would panic if it
        // were ever violated.
        TokenKind::NotPrefix => {
            if let Some(galley) = galleys.first() {
                text_run(ui, rect, galley.clone(), ui.sem().notice);
            }
        }
        TokenKind::Connector { .. }
        | TokenKind::GroupOpen { .. }
        | TokenKind::GroupClose
        | TokenKind::QuantOpen { .. }
        | TokenKind::QuantClose => {
            if let Some(galley) = galleys.first() {
                text_run(ui, rect, galley.clone(), chrome_color(ui, state));
            }
        }
        // The caret is a real `TextEdit`, which draws itself.
        TokenKind::Caret => {}
    }
}

/// The pill's background and outline are painted once, over the whole rect,
/// so a pill's selection fill reads as one continuous surface rather than a
/// strip per segment. Each segment then paints its own run inside the rect
/// `segment_rects` gives it, with a separator between adjacent segments and a
/// background of its own when the pointer sits over it, which is the click
/// target `mod.rs` registers over the same rect.
fn pill(ui: &Ui, rect: Rect, galleys: &[Arc<Galley>], state: TokenState, cfg: &LayoutCfg, boxed: Option<usize>) {
    let visuals = ui.visuals();
    let (fill, text) = match state {
        TokenState::Idle => (visuals.widgets.inactive.bg_fill, visuals.text_color()),
        TokenState::Hovered => (visuals.widgets.hovered.bg_fill, visuals.strong_text_color()),
        TokenState::Selected => (visuals.selection.bg_fill, visuals.strong_text_color()),
    };
    let segments = segment_rects(rect, &segment_widths(galleys), cfg);
    let surface = pill_surface(rect, &segments);
    let painter = ui.painter();
    painter.rect_filled(surface, CornerRadius::same(RADIUS), fill);
    if state == TokenState::Selected {
        painter.rect_stroke(surface, CornerRadius::same(RADIUS), visuals.selection.stroke, StrokeKind::Inside);
    }

    let hover_fill = ui.sem().text_strong.gamma_multiply(SEGMENT_HOVER_ALPHA);
    let separator = Stroke::new(SEGMENT_SEPARATOR_WIDTH, ui.sem().pill_separator);
    let last = segments.len().saturating_sub(1);

    for (i, (seg_rect, galley)) in segments.iter().zip(galleys).enumerate() {
        if i > 0 {
            let sep_x = seg_rect.left() - cfg.segment_gap * 0.5;
            painter.line_segment([Pos2::new(sep_x, rect.top()), Pos2::new(sep_x, rect.bottom())], separator);
        }
        // The boxed segment is a `TextEdit` drawn over this same rect by the
        // egui layer, so neither its run nor a hover wash belongs under it.
        if boxed == Some(i) {
            continue;
        }
        if ui.rect_contains_pointer(*seg_rect) {
            painter.rect_filled(*seg_rect, segment_corner_radius(i, last), hover_fill);
        }
        text_run(ui, *seg_rect, galley.clone(), text);
    }
}

/// The pill a value is being edited inside: the same surface and segments a
/// committed pill draws, minus the one the inline box occupies. `galleys`
/// carries that pill's runs with the boxed segment measured from the text being
/// typed, so the pill is exactly as wide as the box currently needs.
pub fn inline_pill(ui: &Ui, rect: Rect, galleys: &[Arc<Galley>], boxed: usize, state: TokenState, cfg: &LayoutCfg) {
    pill(ui, rect, galleys, state, cfg, Some(boxed));
}

/// The surface a pill paints over: the cell it was handed, trimmed to what its
/// segments occupy. `lay_out` gives a trailing caret the rest of its row, and a
/// caret standing in the last pill's slot is a trailing caret, so the cell can
/// be far wider than the pill has segments to fill.
pub fn pill_surface(cell: Rect, segments: &[Rect]) -> Rect {
    segments.last().map_or(cell, |last| cell.with_max_x(last.right()))
}

/// The mark that says one segment of a pill is open for typing: a frame around
/// that segment, in the accent stroke a focused bar and a selected pill already
/// carry, so "this is the control being engaged" is stated one way throughout.
///
/// Without it the box is the pill's own fill with a cursor blinking on it, which
/// reads as the committed value rather than as a field -- `(FIELD|OP|text)`
/// where the whole point of the gesture is `(FIELD|OP|[text])`. A frame is the
/// closest thing to the brackets that notation draws, and it is what a desktop
/// text box looks like; an underline alone is the Material idiom.
///
/// A frame rather than a fill, for one reason and not the other. The *flat*
/// tones the palette offers cannot do it: a pill carries three fills, they
/// straddle any candidate from both sides in the light theme, and the closest
/// reaches 1.273 against an idle pill in the dark theme and 1.172 in the light
/// against a floor of 1.3, which
/// `no_flat_palette_tone_is_tellable_from_every_pill_fill` measures. A
/// *composited* wash is a different matter and does clear the floor -- the hover
/// wash beside it is the standing proof -- so it is ruled out on meaning rather
/// than on contrast: at any strength that reads, a wash over the boxed segment
/// is the mark that already means "the pointer is over this segment".
///
/// Inside the segment's rect, so the frame lands within the pill rather than
/// straddling its outer edge, where the corner radius would clip it.
pub fn inline_box(ui: &Ui, rect: Rect) {
    let visuals = ui.visuals();
    let stroke = Stroke::new(visuals.selection.stroke.width, inline_box_stroke(visuals));
    ui.painter().rect_stroke(rect, CornerRadius::same(RADIUS), stroke, StrokeKind::Inside);
}

/// The frame's colour, routed through one function so the contrast test measures
/// what is painted rather than a copy of it.
fn inline_box_stroke(visuals: &egui::Visuals) -> Color32 {
    visuals.selection.stroke.color
}

/// The hover wash's corners: rounded only where a segment sits on the pill's
/// own outer boundary, so an interior segment reads as a straight-edged slice
/// of the pill rather than a chip floating mid-pill. `last` is the index of
/// the final segment (`segments.len() - 1`), so a single-segment pill rounds
/// every corner, matching the pill's own outline.
fn segment_corner_radius(index: usize, last: usize) -> CornerRadius {
    let round_left = index == 0;
    let round_right = index == last;
    CornerRadius {
        nw: if round_left { RADIUS } else { 0 },
        sw: if round_left { RADIUS } else { 0 },
        ne: if round_right { RADIUS } else { 0 },
        se: if round_right { RADIUS } else { 0 },
    }
}

/// The cell width one measured run needs: the glyphs plus the padding
/// `text_origin` insets them by, on both sides. The single derivation, so the
/// width a pill is sized to, the width its segments are painted at, and the
/// width its click targets are hit-tested against cannot disagree.
pub fn segment_widths(galleys: &[Arc<Galley>]) -> Vec<f32> {
    galleys.iter().map(|g| g.size().x + 2.0 * PAD_X).collect()
}

/// One rect per segment, positioned inside `pill_rect` by
/// `layout::segment_offsets`. `pill_rect`'s own width must already equal
/// `layout::pill_width(segment_widths, cfg)` -- `mod.rs` sizes a pill token
/// that way -- so every returned rect lands inside `pill_rect` with nothing
/// left over. Pure arithmetic: this is the half of segment hit-testing that
/// does not need a rendered frame to check, and is what `mod.rs` calls
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
pub(crate) fn char_index(text: &str, byte: usize) -> usize {
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

    /// The separator is drawn on a pill's own fill, not on the bar, so it
    /// cannot rely on `depth_stroke_clears_its_floors`' guarantee (that one
    /// measures against `extreme_bg_color`, the bar). Walks every fill a pill
    /// can actually have.
    #[test]
    fn pill_separator_clears_every_pill_fill() {
        use crate::ui::theme::contrast::CHROME_LINE_FLOOR;
        use crate::ui::theme::contrast::contrast_ratio;

        for (theme, visuals) in [
            ("dark", crate::ui::theme::style::dark_style().visuals),
            ("light", crate::ui::theme::style::light_style().visuals),
        ] {
            let sem = visuals.sem();
            for (fill_name, fill) in [
                ("inactive", visuals.widgets.inactive.bg_fill),
                ("hovered", visuals.widgets.hovered.bg_fill),
                ("selected", visuals.selection.bg_fill),
            ] {
                let ratio = contrast_ratio(sem.pill_separator, fill);
                assert!(
                    ratio >= CHROME_LINE_FLOOR,
                    "{theme} pill separator on {fill_name} pill fill is {ratio:.3}, needs {CHROME_LINE_FLOOR}"
                );
            }
        }
    }

    /// The hover wash is translucent, so it must be composited over the fill
    /// it actually lands on before its contrast means anything -- `Color32` is
    /// not premultiplied, but `gamma_multiply` on an opaque colour produces
    /// exactly the premultiplied form `over` expects, so the addition alone
    /// reproduces what the painter draws.
    #[test]
    fn segment_hover_wash_clears_every_pill_fill() {
        use crate::ui::theme::contrast::SURFACE_CONTRAST_FLOOR;
        use crate::ui::theme::contrast::contrast_ratio;

        fn over(fg: Color32, bg: Color32) -> Color32 {
            let inv = 1.0 - (f32::from(fg.a()) / 255.0);
            let mix = |f: u8, b: u8| (f32::from(f) + f32::from(b) * inv).round() as u8;
            Color32::from_rgb(mix(fg.r(), bg.r()), mix(fg.g(), bg.g()), mix(fg.b(), bg.b()))
        }

        for (theme, visuals) in [
            ("dark", crate::ui::theme::style::dark_style().visuals),
            ("light", crate::ui::theme::style::light_style().visuals),
        ] {
            let wash = visuals.sem().text_strong.gamma_multiply(SEGMENT_HOVER_ALPHA);
            for (fill_name, fill) in [
                ("inactive", visuals.widgets.inactive.bg_fill),
                ("hovered", visuals.widgets.hovered.bg_fill),
                ("selected", visuals.selection.bg_fill),
            ] {
                let composited = over(wash, fill);
                let ratio = contrast_ratio(composited, fill);
                assert!(
                    ratio >= SURFACE_CONTRAST_FLOOR,
                    "{theme} hover wash on {fill_name} pill fill is {ratio:.3}, needs {SURFACE_CONTRAST_FLOOR}"
                );
            }
        }
    }

    /// The frame around an open box is drawn on the pill's own fill, so like
    /// `pill_separator` it has to clear every fill a pill can carry. Without a
    /// mark at all the box was the pill's fill with a cursor on it, which is the
    /// state the review found and the state the user complained about.
    ///
    /// It also has to out-read the separator beside it, or the segment being
    /// typed into looks exactly like the boundary between two settled ones.
    #[test]
    fn the_inline_box_frame_clears_every_pill_fill() {
        use crate::ui::theme::contrast::CHROME_LINE_FLOOR;
        use crate::ui::theme::contrast::contrast_ratio;

        for (theme, visuals) in [
            ("dark", crate::ui::theme::style::dark_style().visuals),
            ("light", crate::ui::theme::style::light_style().visuals),
        ] {
            let frame = inline_box_stroke(&visuals);
            for (fill_name, fill) in [
                ("inactive", visuals.widgets.inactive.bg_fill),
                ("hovered", visuals.widgets.hovered.bg_fill),
                ("selected", visuals.selection.bg_fill),
            ] {
                let ratio = contrast_ratio(frame, fill);
                assert!(
                    ratio >= CHROME_LINE_FLOOR,
                    "{theme} inline box frame on {fill_name} pill fill is {ratio:.3}, needs {CHROME_LINE_FLOOR}"
                );
                assert!(
                    ratio > contrast_ratio(visuals.sem().pill_separator, fill),
                    "{theme} the frame must out-read the separator on {fill_name} pill fill"
                );
            }
        }
    }

    /// Half the reason the mark is a frame and not a fill, recorded as a
    /// measurement rather than as a claim: a pill's three fills span so much of
    /// the theme's tonal room -- in both directions in the light theme -- that
    /// no *flat* tone the palette offers is tellable from all of them.
    ///
    /// **This is a sweep of flat surfaces and nothing else, and it is not an
    /// impossibility proof.** A composited wash adapts to the fill it lands on
    /// and does clear the floor: `segment_hover_wash_clears_every_pill_fill`,
    /// forty lines above, is a standing existence proof of exactly that, and the
    /// same alpha over the box measures 1.476/1.463/1.465 in the dark theme and
    /// 1.361/1.355/1.350 in the light. A wash is ruled out because at any
    /// strength that reads it is pixel-identical to the mark that already means
    /// "the pointer is over this segment" -- a collision of meaning, not a wall
    /// of contrast. An accent frame is a chrome line at the same colour budget
    /// and clears its own floor comfortably, which is what ships.
    #[test]
    fn no_flat_palette_tone_is_tellable_from_every_pill_fill() {
        use crate::ui::theme::contrast::SURFACE_CONTRAST_FLOOR;
        use crate::ui::theme::contrast::contrast_ratio;

        for (theme, visuals) in [
            ("dark", crate::ui::theme::style::dark_style().visuals),
            ("light", crate::ui::theme::style::light_style().visuals),
        ] {
            let fills = [visuals.widgets.inactive.bg_fill, visuals.widgets.hovered.bg_fill, visuals.selection.bg_fill];
            // Surfaces only. A chrome line's tone can sit clear of every fill --
            // `pill_separator` does -- but a line colour used as a surface is a
            // different mistake, not a way out of this one.
            for candidate in [visuals.extreme_bg_color, visuals.faint_bg_color, visuals.panel_fill, visuals.window_fill]
            {
                let worst = fills.iter().map(|fill| contrast_ratio(candidate, *fill)).fold(f32::INFINITY, f32::min);
                assert!(
                    worst < SURFACE_CONTRAST_FLOOR,
                    "{theme} {candidate:?} clears every pill fill at {worst:.3}; a fill is viable after all"
                );
            }
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

    /// The middle width (4.0) is below `segment_rects_cfg`'s
    /// `min_segment_width` (16.0), so every test built on this fixture
    /// exercises widening together with translation, not translation alone.
    const SEGMENT_WIDTHS: [f32; 3] = [30.0, 4.0, 40.0];

    #[test]
    fn one_rect_per_segment_in_left_to_right_order() {
        let cfg = segment_rects_cfg();
        let rects = segment_rects(pill_rect_for(&SEGMENT_WIDTHS, &cfg), &SEGMENT_WIDTHS, &cfg);
        assert_eq!(rects.len(), SEGMENT_WIDTHS.len(), "got {rects:?}");
        for pair in rects.windows(2) {
            assert!(pair[1].left() > pair[0].left(), "segments out of order: {rects:?}");
        }
    }

    #[test]
    fn every_segment_rect_is_contained_in_the_pill_rect() {
        let cfg = segment_rects_cfg();
        let pill_rect = pill_rect_for(&SEGMENT_WIDTHS, &cfg);
        let rects = segment_rects(pill_rect, &SEGMENT_WIDTHS, &cfg);
        for r in &rects {
            assert!(pill_rect.contains_rect(*r), "segment {r:?} escapes the pill {pill_rect:?}");
        }
    }

    #[test]
    fn segment_rects_do_not_overlap() {
        let cfg = segment_rects_cfg();
        let rects = segment_rects(pill_rect_for(&SEGMENT_WIDTHS, &cfg), &SEGMENT_WIDTHS, &cfg);
        for pair in rects.windows(2) {
            assert!(pair[1].left() + f32::EPSILON >= pair[0].right(), "segments overlap: {rects:?}");
        }
    }

    #[test]
    fn segment_rects_span_the_pill_edge_to_edge() {
        let cfg = segment_rects_cfg();
        let pill_rect = pill_rect_for(&SEGMENT_WIDTHS, &cfg);
        let rects = segment_rects(pill_rect, &SEGMENT_WIDTHS, &cfg);
        let first = rects.first().expect("at least one segment");
        let last = rects.last().expect("at least one segment");
        assert!((first.left() - pill_rect.left()).abs() < f32::EPSILON, "first segment {first:?} vs {pill_rect:?}");
        assert!((last.right() - pill_rect.right()).abs() < f32::EPSILON, "last segment {last:?} vs {pill_rect:?}");
    }

    /// The hover wash rounds only where a segment sits on the pill's own outer
    /// boundary. Pure arithmetic, but only ever exercised through the render
    /// loop otherwise, which needs pointer-hover context to reach.
    #[test]
    fn a_segments_corners_round_only_on_the_pills_own_boundary() {
        let first = segment_corner_radius(0, 2);
        assert_eq!((first.nw, first.sw), (RADIUS, RADIUS), "the first segment carries the pill's left edge");
        assert_eq!((first.ne, first.se), (0, 0), "and none of its right");

        let middle = segment_corner_radius(1, 2);
        assert_eq!((middle.nw, middle.sw, middle.ne, middle.se), (0, 0, 0, 0), "an interior segment is a slice");

        let last = segment_corner_radius(2, 2);
        assert_eq!((last.ne, last.se), (RADIUS, RADIUS), "the last segment carries the pill's right edge");
        assert_eq!((last.nw, last.sw), (0, 0), "and none of its left");

        // `MatchTerm::FreeText` ships a one-segment pill, which is both first
        // and last and so matches the pill's own outline all the way round.
        let only = segment_corner_radius(0, 0);
        assert_eq!((only.nw, only.sw, only.ne, only.se), (RADIUS, RADIUS, RADIUS, RADIUS));
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
