//! Line breaking over the token stream.
//!
//! Takes measured widths rather than measuring, so the algorithm is pure and
//! testable; the egui layer measures with `layout_no_wrap` and passes them in.

use egui::Pos2;
use egui::Rect;

use crate::ui::query_bar::tokens::Token;
use crate::ui::query_bar::tokens::TokenKind;

#[derive(Debug, Clone, Copy)]
pub struct LayoutCfg {
    pub row_height: f32,
    pub gap: f32,
    /// Horizontal indent added per nesting level.
    pub indent: f32,
    /// Rows shown before the bar scrolls internally.
    pub max_rows: usize,
    /// A segment narrower than this widens to it, so a short label like "="
    /// is still a clickable target.
    pub min_segment_width: f32,
    /// Horizontal gap painted between two adjacent segments of one pill.
    pub segment_gap: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    pub index: usize,
    pub row: usize,
    pub rect: Rect,
}

/// One row-slice of a bracketed run, so a group that wraps can be painted as an
/// open-ended bracket on each row it touches.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupSpan {
    pub open_index: usize,
    pub close_index: usize,
    pub rows: Vec<(usize, Rect)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LaidOut {
    pub placed: Vec<Placed>,
    pub group_spans: Vec<GroupSpan>,
    pub rows: usize,
    pub height: f32,
    pub needs_scroll: bool,
}

fn is_open_bracket(kind: &TokenKind) -> bool {
    matches!(kind, TokenKind::GroupOpen { .. } | TokenKind::QuantOpen { .. })
}

fn is_close_bracket(kind: &TokenKind) -> bool {
    matches!(kind, TokenKind::GroupClose | TokenKind::QuantClose)
}

/// An open bracket's accumulated per-row rects, built up while its contents
/// are being placed and finalized into a `GroupSpan` at the matching close.
struct OpenBracket {
    open_index: usize,
    rows: Vec<(usize, Rect)>,
}

pub fn lay_out(tokens: &[Token], widths: &[f32], avail: f32, cfg: &LayoutCfg) -> LaidOut {
    debug_assert_eq!(widths.len(), tokens.len());

    let mut placed = Vec::with_capacity(tokens.len());
    let mut group_spans = Vec::new();
    let mut open_brackets: Vec<OpenBracket> = Vec::new();

    let mut row = 0usize;
    // True only for the very first token: after that, a wrap is decided by
    // whether the candidate position fits, not by this flag. It exists solely
    // to let token 0 skip the wrap check unconditionally, which is the guard
    // against looping on a token wider than `avail`.
    let mut is_first_token = true;
    let mut cursor = 0.0f32;

    for (i, token) in tokens.iter().enumerate() {
        let width = widths[i];
        let start_x = if is_first_token {
            token.depth as f32 * cfg.indent
        } else {
            let candidate = cursor + cfg.gap;
            if candidate + width > avail {
                row += 1;
                token.depth as f32 * cfg.indent
            } else {
                candidate
            }
        };

        let mut end_x = start_x + width;
        if matches!(token.kind, TokenKind::Caret) {
            // The caret takes the rest of its row only where nothing follows it.
            // Stretching one that sits mid-stream -- which is where a term being
            // edited as text puts it -- would push every later token onto a row
            // of its own.
            if i + 1 == tokens.len() {
                end_x = end_x.max(avail);
            }
            // Pinned in the other direction too, and for a reason no other token
            // has: the caret is a `TextEdit`, which scrolls its own text
            // internally but only within the cell it is given. A cell past the
            // bar's right edge is clipped by the panel instead, so the text and
            // the typing cursor leave the bar with no way to scroll them back --
            // the user types blind.
            end_x = end_x.min(avail).max(start_x);
        }

        let top = row as f32 * cfg.row_height;
        let rect = Rect::from_min_max(Pos2::new(start_x, top), Pos2::new(end_x, top + cfg.row_height));

        if is_open_bracket(&token.kind) {
            open_brackets.push(OpenBracket { open_index: i, rows: Vec::new() });
        }
        for bracket in &mut open_brackets {
            match bracket.rows.last_mut() {
                Some((r, acc)) if *r == row => *acc = acc.union(rect),
                _ => bracket.rows.push((row, rect)),
            }
        }
        // `tokenize` emits brackets in matched pairs, so there is always an open
        // waiting here; an unmatched close is dropped rather than taken down to
        // a panic, since this runs on every keystroke.
        if is_close_bracket(&token.kind)
            && let Some(bracket) = open_brackets.pop()
        {
            group_spans.push(GroupSpan { open_index: bracket.open_index, close_index: i, rows: bracket.rows });
        }

        placed.push(Placed { index: i, row, rect });
        cursor = end_x;
        is_first_token = false;
    }

    let rows = row + 1;
    LaidOut { placed, group_spans, rows, height: rows as f32 * cfg.row_height, needs_scroll: rows > cfg.max_rows }
}

/// A segmented pill's total width: each segment widened to at least
/// `cfg.min_segment_width`, plus `cfg.segment_gap` between every adjacent
/// pair (a single segment gets no gap, since there is no pair). `lay_out`
/// still treats the pill as one placeable unit; this only decides how wide
/// that unit is.
///
/// An empty slice cannot arise from `label::pill_segments` (every match on
/// `MatchTerm` returns at least one segment), but resolves to `0.0` rather
/// than panicking, since this runs on every keystroke.
pub fn pill_width(segment_widths: &[f32], cfg: &LayoutCfg) -> f32 {
    let Some((first, rest)) = segment_widths.split_first() else {
        return 0.0;
    };
    let widened: f32 = rest.iter().fold(first.max(cfg.min_segment_width), |acc, w| acc + w.max(cfg.min_segment_width));
    widened + rest.len() as f32 * cfg.segment_gap
}

/// (x offset within the pill, width) per segment, in order. Widens and gaps
/// the same way `pill_width` sums them, so the two never disagree about where
/// the pill ends; see `pill_width` for the empty-slice decision.
pub fn segment_offsets(segment_widths: &[f32], cfg: &LayoutCfg) -> Vec<(f32, f32)> {
    let mut offsets = Vec::with_capacity(segment_widths.len());
    let mut cursor = 0.0f32;
    for &w in segment_widths {
        let width = w.max(cfg.min_segment_width);
        offsets.push((cursor, width));
        cursor += width + cfg.segment_gap;
    }
    offsets
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::query_bar::tokens::Token;
    use crate::ui::query_bar::tokens::TokenKind;

    fn cfg() -> LayoutCfg {
        LayoutCfg { row_height: 20.0, gap: 4.0, indent: 8.0, max_rows: 6, min_segment_width: 0.0, segment_gap: 4.0 }
    }
    fn tok(kind: TokenKind, depth: usize) -> Token {
        Token { kind, path: vec![], depth }
    }
    fn pills(n: usize) -> Vec<Token> {
        let mut v: Vec<Token> = (0..n).map(|_| tok(TokenKind::Pill { segments: vec![] }, 0)).collect();
        v.push(tok(TokenKind::Caret, 0));
        v
    }

    #[test]
    fn tokens_that_fit_stay_on_one_row() {
        let toks = pills(3);
        let widths = vec![30.0; toks.len()];
        let out = lay_out(&toks, &widths, 500.0, &cfg());
        assert_eq!(out.rows, 1);
        assert!(out.placed.iter().all(|p| p.row == 0));
    }

    #[test]
    fn tokens_wrap_when_they_exceed_the_available_width() {
        let toks = pills(5);
        let widths = vec![100.0; toks.len()];
        let out = lay_out(&toks, &widths, 250.0, &cfg());
        assert!(out.rows > 1, "expected a wrap, got {out:#?}");
        assert!(out.placed.iter().all(|p| p.rect.max.x <= 250.0 + f32::EPSILON), "a token overflowed: {out:#?}");
    }

    #[test]
    fn a_token_wider_than_the_bar_still_gets_its_own_row_rather_than_looping() {
        let toks = pills(2);
        let widths = vec![9999.0, 9999.0, 10.0];
        let out = lay_out(&toks, &widths, 100.0, &cfg());
        assert_eq!(out.placed.len(), toks.len(), "every token must be placed exactly once");
        assert!(out.rows >= 2);
    }

    // Regression test beyond the pinned module: a wrap-guard bug can still
    // place every token exactly once (satisfying the pinned test above) while
    // silently skipping row 0, if it wraps on a too-wide token even when the
    // cursor is already at the row start. Verified to catch that: reinstating
    // the unconditional wrap check (dropping the `is_first_token` guard on
    // whether to check at all, while still using it to pick the indent)
    // leaves this failing with `out.placed[0].row == 1` while every pinned
    // test above still passes.
    #[test]
    fn a_token_wider_than_the_bar_still_lands_on_row_zero() {
        let toks = pills(2);
        let widths = vec![9999.0, 9999.0, 10.0];
        let out = lay_out(&toks, &widths, 100.0, &cfg());
        assert_eq!(out.placed[0].row, 0, "the first row must still be used, not skipped: {out:#?}");
    }

    #[test]
    fn total_height_matches_the_row_count() {
        let toks = pills(5);
        let widths = vec![100.0; toks.len()];
        let c = cfg();
        let out = lay_out(&toks, &widths, 250.0, &c);
        assert!((out.height - out.rows as f32 * c.row_height).abs() < f32::EPSILON, "got {out:#?}");
    }

    #[test]
    fn the_caret_is_placed_last_and_gets_the_remaining_width_on_its_row() {
        let toks = pills(2);
        let widths = vec![50.0, 50.0, 10.0];
        let out = lay_out(&toks, &widths, 500.0, &cfg());
        let caret = out.placed.last().expect("the caret");
        assert!(caret.rect.width() > 10.0, "the caret should stretch to the row end: {caret:?}");
    }

    /// A term being edited as text puts the caret in that pill's slot, with the
    /// rest of the query after it. Stretching it there would give one token the
    /// whole row and push everything following it onto rows of their own.
    #[test]
    fn a_caret_that_is_not_last_does_not_take_the_rest_of_its_row() {
        let toks = vec![
            tok(TokenKind::Pill { segments: vec![] }, 0),
            tok(TokenKind::Caret, 0),
            tok(TokenKind::Pill { segments: vec![] }, 0),
        ];
        let widths = vec![50.0, 60.0, 50.0];
        let out = lay_out(&toks, &widths, 500.0, &cfg());
        assert!((out.placed[1].rect.width() - 60.0).abs() < f32::EPSILON, "a mid-stream caret was stretched: {out:#?}");
        assert_eq!(out.rows, 1, "stretching it pushed the pill after it onto a row of its own: {out:#?}");
        assert!(out.placed[2].rect.min.x > out.placed[1].rect.max.x, "the last pill must follow the caret: {out:#?}");
    }

    /// The caret is a `TextEdit`, which scrolls its own text but only inside the
    /// cell it is given. A cell wider than the bar is clipped by the panel
    /// around it instead, so the text and the typing cursor leave the bar with
    /// no way to scroll them back. Both carets are pinned to the bar's right
    /// edge; only the trailing one is also stretched out to it.
    #[test]
    fn a_caret_wider_than_the_bar_is_pinned_to_its_right_edge() {
        const AVAIL: f32 = 250.0;
        for (label, toks, widths) in [
            (
                "mid-stream",
                vec![
                    tok(TokenKind::Pill { segments: vec![] }, 0),
                    tok(TokenKind::Caret, 0),
                    tok(TokenKind::Pill { segments: vec![] }, 0),
                ],
                vec![50.0, 9999.0, 50.0],
            ),
            (
                "trailing",
                vec![tok(TokenKind::Pill { segments: vec![] }, 0), tok(TokenKind::Caret, 0)],
                vec![50.0, 9999.0],
            ),
        ] {
            let out = lay_out(&toks, &widths, AVAIL, &cfg());
            let caret = out
                .placed
                .iter()
                .find(|p| matches!(toks[p.index].kind, TokenKind::Caret))
                .unwrap_or_else(|| panic!("{label}: the caret"));
            assert!(caret.rect.max.x <= AVAIL + f32::EPSILON, "{label}: the caret ran past the bar: {caret:?}");
            assert!(caret.rect.width() > 0.0, "{label}: the caret was clamped away entirely: {caret:?}");
        }
    }

    #[test]
    fn a_group_spanning_a_break_is_reported_as_open_ended_on_each_row() {
        let mut toks = vec![tok(TokenKind::GroupOpen { is_or: false }, 0)];
        toks.extend((0..4).map(|_| tok(TokenKind::Pill { segments: vec![] }, 1)));
        toks.push(tok(TokenKind::GroupClose, 0));
        toks.push(tok(TokenKind::Caret, 0));
        let widths = vec![8.0, 100.0, 100.0, 100.0, 100.0, 8.0, 10.0];
        let out = lay_out(&toks, &widths, 250.0, &cfg());
        assert!(out.rows > 1);
        let spans: Vec<_> = out.group_spans.iter().filter(|s| s.rows.len() > 1).collect();
        assert!(!spans.is_empty(), "a wrapped group must report one span per row: {out:#?}");
    }

    // Mirrors `a_group_spanning_a_break_is_reported_as_open_ended_on_each_row`
    // but with the other bracket-pair kind. Nothing else in the suite puts a
    // QuantOpen/QuantClose pair through `lay_out`, so without this a later
    // narrowing of `is_open_bracket`/`is_close_bracket` back to only the
    // Group variants would silently break Quant-bracket wrapping (a
    // non-sugar roster filter, e.g. `count(...)`, that wraps a line) with a
    // green suite. Verified to catch that: see the fix report.
    #[test]
    fn a_quant_bracket_spanning_a_break_is_reported_as_open_ended_on_each_row() {
        let mut toks = vec![tok(TokenKind::QuantOpen { prefix: "at least 2".into() }, 0)];
        toks.extend((0..4).map(|_| tok(TokenKind::Pill { segments: vec![] }, 1)));
        toks.push(tok(TokenKind::QuantClose, 0));
        toks.push(tok(TokenKind::Caret, 0));
        let widths = vec![8.0, 100.0, 100.0, 100.0, 100.0, 8.0, 10.0];
        let out = lay_out(&toks, &widths, 250.0, &cfg());
        assert!(out.rows > 1);
        let spans: Vec<_> = out.group_spans.iter().filter(|s| s.rows.len() > 1).collect();
        assert!(!spans.is_empty(), "a wrapped quantifier bracket must report one span per row: {out:#?}");
    }

    /// What the deepest-first paint order in `mod.rs::rows` rests on: an outer
    /// bracket may paint over an inner one only because it covers it. Paint
    /// order itself needs a rendered frame to observe; the containment it
    /// assumes does not, and is the half that can drift silently.
    #[test]
    fn an_outer_group_covers_the_one_inside_it_on_every_row_they_share() {
        let mut toks = vec![
            tok(TokenKind::GroupOpen { is_or: false }, 1),
            tok(TokenKind::Pill { segments: vec![] }, 1),
            tok(TokenKind::GroupOpen { is_or: true }, 2),
        ];
        toks.extend((0..3).map(|_| tok(TokenKind::Pill { segments: vec![] }, 2)));
        toks.push(tok(TokenKind::GroupClose, 2));
        toks.push(tok(TokenKind::Pill { segments: vec![] }, 1));
        toks.push(tok(TokenKind::GroupClose, 1));
        toks.push(tok(TokenKind::Caret, 0));
        let widths = vec![8.0, 100.0, 8.0, 100.0, 100.0, 100.0, 8.0, 100.0, 8.0, 10.0];
        let out = lay_out(&toks, &widths, 250.0, &cfg());

        let outer = out.group_spans.iter().find(|s| s.open_index == 0).expect("the outer span");
        let inner = out.group_spans.iter().find(|s| s.open_index == 2).expect("the inner span");
        assert!(inner.rows.len() > 1, "the fixture must wrap, or one row proves nothing: {out:#?}");
        for (row, inner_rect) in &inner.rows {
            let outer_rect = outer
                .rows
                .iter()
                .find(|(r, _)| r == row)
                .map(|(_, rect)| *rect)
                .unwrap_or_else(|| panic!("the outer group must touch row {row} too: {out:#?}"));
            assert!(
                outer_rect.contains_rect(*inner_rect),
                "row {row}: outer {outer_rect:?} does not cover inner {inner_rect:?}"
            );
        }
    }

    #[test]
    fn nested_depth_indents_the_rows_it_occupies() {
        let c = cfg();
        let shallow = lay_out(
            &[tok(TokenKind::Pill { segments: vec![] }, 0), tok(TokenKind::Caret, 0)],
            &[10.0, 10.0],
            500.0,
            &c,
        );
        let deep = lay_out(
            &[tok(TokenKind::Pill { segments: vec![] }, 2), tok(TokenKind::Caret, 0)],
            &[10.0, 10.0],
            500.0,
            &c,
        );
        assert!(deep.placed[0].rect.min.x > shallow.placed[0].rect.min.x, "depth must indent");
    }

    #[test]
    fn beyond_max_rows_the_bar_reports_that_it_needs_scrolling() {
        let toks = pills(40);
        let widths = vec![100.0; toks.len()];
        let out = lay_out(&toks, &widths, 200.0, &cfg());
        assert!(out.rows > cfg().max_rows);
        assert!(out.needs_scroll, "the caller must know to put this in a scroll area");
    }

    #[test]
    fn layout_is_deterministic_for_identical_input() {
        let toks = pills(7);
        let widths = vec![60.0; toks.len()];
        let a = lay_out(&toks, &widths, 200.0, &cfg());
        let b = lay_out(&toks, &widths, 200.0, &cfg());
        assert_eq!(a.placed, b.placed);
    }

    #[test]
    fn a_pills_width_is_its_segments_plus_the_gaps() {
        let c = LayoutCfg { min_segment_width: 0.0, segment_gap: 4.0, ..cfg() };
        assert!((pill_width(&[30.0, 20.0, 40.0], &c) - (30.0 + 20.0 + 40.0 + 8.0)).abs() < f32::EPSILON);
    }

    #[test]
    fn a_narrow_segment_is_widened_to_the_minimum() {
        // "=" must still be a clickable target.
        let c = LayoutCfg { min_segment_width: 16.0, segment_gap: 0.0, ..cfg() };
        assert!((pill_width(&[30.0, 4.0, 40.0], &c) - (30.0 + 16.0 + 40.0)).abs() < f32::EPSILON);
    }

    #[test]
    fn segment_offsets_do_not_overlap_and_fill_the_pill() {
        let c = LayoutCfg { min_segment_width: 16.0, segment_gap: 4.0, ..cfg() };
        let widths = [30.0, 4.0, 40.0];
        let offsets = segment_offsets(&widths, &c);
        assert_eq!(offsets.len(), widths.len());
        for pair in offsets.windows(2) {
            assert!(pair[1].0 >= pair[0].0 + pair[0].1, "segments overlap: {offsets:?}");
        }
        let last = offsets.last().unwrap();
        assert!((last.0 + last.1 - pill_width(&widths, &c)).abs() < f32::EPSILON, "offsets do not fill the pill");
    }

    #[test]
    fn a_two_segment_pill_is_narrower_than_a_three_segment_one() {
        let c = LayoutCfg { min_segment_width: 16.0, segment_gap: 4.0, ..cfg() };
        assert!(pill_width(&[30.0, 20.0], &c) < pill_width(&[30.0, 20.0, 40.0], &c));
    }

    #[test]
    fn an_oversized_pill_still_gets_its_own_row() {
        // The existing rule for any token wider than the bar must survive.
        let toks = pills(2);
        let widths = vec![9999.0, 9999.0, 10.0];
        let out = lay_out(&toks, &widths, 100.0, &cfg());
        assert_eq!(out.placed.len(), toks.len());
        assert_eq!(out.placed[0].row, 0);
    }

    #[test]
    fn a_lone_segment_has_no_gap_and_takes_its_own_width() {
        let c = LayoutCfg { min_segment_width: 0.0, segment_gap: 4.0, ..cfg() };
        assert!((pill_width(&[30.0], &c) - 30.0).abs() < f32::EPSILON);
        let offsets = segment_offsets(&[30.0], &c);
        assert_eq!(offsets.len(), 1);
        assert!((offsets[0].0 - 0.0).abs() < f32::EPSILON);
        assert!((offsets[0].1 - 30.0).abs() < f32::EPSILON);
    }

    #[test]
    fn an_empty_pill_has_zero_width_and_no_segments_rather_than_panicking() {
        let c = cfg();
        assert!((pill_width(&[], &c) - 0.0).abs() < f32::EPSILON);
        assert!(segment_offsets(&[], &c).is_empty());
    }

    /// The property `segment_offsets` and `pill_width` must never drift apart
    /// on: the last segment's right edge is exactly the pill's own width.
    /// Verified to fail if either is changed independently of the other --
    /// see task-5-report.md for the induced-failure transcript.
    #[test]
    fn segment_offsets_and_pill_width_agree_on_where_the_pill_ends() {
        let c = LayoutCfg { min_segment_width: 12.0, segment_gap: 5.0, ..cfg() };
        for widths in [vec![30.0], vec![30.0, 4.0], vec![30.0, 4.0, 40.0], vec![5.0, 5.0, 5.0, 5.0]] {
            let offsets = segment_offsets(&widths, &c);
            let last = offsets.last().unwrap();
            assert!(
                (last.0 + last.1 - pill_width(&widths, &c)).abs() < f32::EPSILON,
                "segment_offsets and pill_width disagree for {widths:?}: {offsets:?} vs {}",
                pill_width(&widths, &c)
            );
        }
    }
}
