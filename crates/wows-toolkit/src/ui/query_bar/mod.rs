//! The Search tab's query bar: a wrapping multi-line editor whose committed
//! filters render as pills over a `MatchExpr`.
//!
//! This file is the egui layer and holds as little judgement as it can. Every
//! rule about what an edit means lives in `select`, every rule about where a
//! token goes lives in `layout`, and every rule about what the dropdown offers
//! lives in `suggest`, because none of those can be reached by a test from
//! here.

pub mod label;
pub mod layout;
pub mod paint;
pub mod seed;
pub mod select;
pub mod suggest;
pub mod tokens;

use std::sync::Arc;

use egui::Galley;
use egui::Key;
use egui::Modifiers;
use egui::Pos2;
use egui::Rect;
use egui::Sense;
use egui::Ui;
use egui::UiBuilder;
use egui::text::CCursor;
use egui::text::CCursorRange;
use egui::text_edit::TextEditState;
use rust_i18n::t;

use crate::db::index::query_ast::MatchExpr;
use crate::db::index::query_ast::Op;
use crate::db::index::query_text;
use crate::db::index::query_text::ParseErrorKind;
use crate::db::index::query_text::QueryParseError;
use crate::db::index::query_text::parse_query;
use crate::db::index::query_text::parse_roster_value;
use crate::db::index::query_text::print_value;
use crate::ui::query_bar::label::NameCache;
use crate::ui::query_bar::label::SegmentRole;
use crate::ui::query_bar::layout::LaidOut;
use crate::ui::query_bar::layout::LayoutCfg;
use crate::ui::query_bar::layout::lay_out;
use crate::ui::query_bar::paint::TokenState;
use crate::ui::query_bar::select::Selection;
use crate::ui::query_bar::suggest::FieldOption;
use crate::ui::query_bar::suggest::OperatorOption;
use crate::ui::query_bar::suggest::PRESETS;
use crate::ui::query_bar::suggest::Scope;
use crate::ui::query_bar::suggest::Suggestion;
use crate::ui::query_bar::suggest::SuggestionCategory;
use crate::ui::query_bar::suggest::SuggestionKind;
use crate::ui::query_bar::suggest::TermField;
use crate::ui::query_bar::suggest::ValueEditor;
use crate::ui::query_bar::suggest::ValueOption;
use crate::ui::query_bar::suggest::ValueRequest;
use crate::ui::query_bar::suggest::field_options;
use crate::ui::query_bar::suggest::filter_options;
use crate::ui::query_bar::suggest::operator_options;
use crate::ui::query_bar::suggest::rank;
use crate::ui::query_bar::suggest::value_request_for;
use crate::ui::query_bar::tokens::NodePath;
use crate::ui::query_bar::tokens::Token;
use crate::ui::query_bar::tokens::TokenKind;
use crate::ui::query_bar::tokens::tokenize;
use crate::ui::theme::semantic::SemanticExt;

/// Rows the bar shows before it scrolls internally instead of growing.
const MAX_ROWS: usize = 6;
const ROW_GAP: f32 = 4.0;
const DEPTH_INDENT: f32 = 10.0;
/// Gap painted between two adjacent segments of one pill. Tighter than
/// `ROW_GAP` because it divides one term rather than separating two.
const SEGMENT_GAP: f32 = 2.0;
/// Vertical inset applied to a token inside its layout row, so pills on
/// adjacent rows do not touch.
const ROW_PAD_Y: f32 = 2.0;
/// The caret never narrows below this, so a full last row pushes it onto a row
/// of its own rather than leaving a sliver to type into.
const MIN_CARET_WIDTH: f32 = 90.0;
/// Width held back for the scrollbar once the bar is tall enough to need one.
const SCROLLBAR_ALLOWANCE: f32 = 14.0;
const MAX_DROPDOWN_HEIGHT: f32 = 260.0;

/// The bar's editing state. `expr` is the source of truth; everything else is
/// either in-flight text or a view of it.
#[derive(Default)]
pub struct QueryBar {
    /// The committed tree.
    pub expr: MatchExpr,
    /// Text currently in the caret, not yet committed.
    pub pending: String,
    /// Set when `pending` fails to parse, so the bar can underline the span.
    pub pending_error: Option<QueryParseError>,
    pub selection: Selection,
    pub dropdown_open: bool,
    /// Which dropdown row the keyboard is on. `None` until an arrow key moves
    /// into the list, which is what leaves Enter free to commit typed text.
    /// An index with a "nothing here" value reserved would be the sentinel the
    /// model avoids everywhere else.
    pub highlighted: Option<usize>,
    pub names: NameCache,
    /// Value rows the Search tab fetched for the request the bar last made.
    pub value_options: Vec<ValueOption>,

    /// The pill segment the dropdown is editing, when it was opened by clicking
    /// one rather than by typing. While it is set the caret's text belongs to
    /// that editor -- a needle for its list, or the literal for a plain value --
    /// and is not a query fragment.
    editing: Option<SegmentEdit>,
    /// Where the popup anchors, refreshed while painting: the rect of the
    /// segment being edited, or the start of the fragment being typed.
    popup_anchor: Option<Rect>,
    /// Caret position captured from the previous frame's `TextEdit` output, so
    /// navigation keys can be consumed before the `TextEdit` renders.
    caret_at_start: bool,
    caret_at_end: bool,
    /// The fixed end of a multi-pill selection, and the end an arrow key moves.
    anchor: Option<NodePath>,
    focus: Option<NodePath>,
    /// `pending` as it stood when the grammar last ran over it, so a parse
    /// happens on a keystroke rather than on every frame.
    parsed_text: String,
    /// The request the caret's current text calls for, tracked so the same one
    /// is not re-issued every frame.
    active_request: Option<ValueRequest>,
    pending_request: Option<ValueRequest>,
    /// How far back into the caller's history Up has walked.
    history_cursor: Option<usize>,
    suggestions: Vec<Suggestion>,
    suggestions_locale: String,
}

pub struct QueryBarOutput {
    /// True when `expr` changed this frame and the caller should re-query.
    pub changed: bool,
    /// A request the caller should service on the runtime and hand back through
    /// `value_options`.
    pub request: Option<ValueRequest>,
}

/// What the bar needs from the Search tab that it does not own.
pub struct Deps<'a> {
    /// Previously run queries in canonical text form, most recent first.
    pub history: &'a [String],
}

/// One row of the dropdown, so the keyboard and the mouse commit through one
/// path.
///
/// The caret's own two sources are indices into state the bar keeps between
/// frames; a segment's three are rebuilt from the term each frame and carry
/// their choice directly, since there is no stable list for an index to name.
#[derive(Debug, Clone, PartialEq)]
enum Row {
    /// An index into `QueryBar::suggestions`, as `rank` returns them.
    Suggestion(usize),
    /// An index into `QueryBar::value_options`.
    Value(usize),
    /// A field the pill's filter segment can be retargeted to.
    Field(FieldOption),
    /// An operator the pill's field allows.
    Operator(OperatorOption),
    /// A grammar literal for the pill's value segment.
    Literal(ValueOption),
}

/// Which segment of which pill the dropdown is editing.
#[derive(Debug, Clone, PartialEq)]
struct SegmentEdit {
    /// The pill the segment belongs to, by its own path in the token stream.
    /// A value edit draws its box inside that pill, and the stream addresses
    /// pills rather than the terms their segments resolve to.
    pill: NodePath,
    /// Already resolved through `select::segment_path`, so it names the term
    /// the segment renders rather than the pill's own node.
    path: NodePath,
    role: SegmentRole,
    /// What dismissal has to hand back, carried only by an editor that a minted
    /// placeholder forced open. `None` for an editor the user opened by clicking
    /// a segment: the value already on the term was a real choice, so dismissal
    /// keeps it.
    restore: Option<Restore>,
    /// Set for a value edit, which is the one role that types into the pill
    /// rather than picking from a list alone.
    inline: Option<InlineValue>,
}

/// The box drawn in a pill's value segment, and what its write back is gated on.
///
/// The seed is taken when the box opens and the write is a whole-leaf
/// replacement against it, so a `canonicalise` between the two either leaves the
/// leaf where it was or moves something else under `leaf` and the write refuses.
/// Nothing here re-resolves a path a second time.
#[derive(Debug, Clone, PartialEq)]
struct InlineValue {
    leaf: NodePath,
    seed: MatchExpr,
    /// The remainder of the term path, inside the seed's roster predicate.
    tail: NodePath,
    /// The text the box opened holding, so a placeholder nobody chose can be
    /// told from one the user has typed over.
    opened_with: String,
}

impl SegmentEdit {
    /// Opens on the segment `role` of the term at `path`, inside the pill at
    /// `pill`, with the caret already holding `opened_with`. A value edit
    /// captures the leaf it opened over; the other two roles commit through
    /// their own list and capture nothing.
    fn open(
        expr: &MatchExpr,
        pill: NodePath,
        path: NodePath,
        role: SegmentRole,
        restore: Option<Restore>,
        opened_with: &str,
    ) -> SegmentEdit {
        let inline = (role == SegmentRole::Value)
            .then(|| select::leaf_seed(expr, &path))
            .flatten()
            .map(|(leaf, seed, tail)| InlineValue { leaf, seed, tail, opened_with: opened_with.to_owned() });
        SegmentEdit { pill, path, role, restore, inline }
    }
}

/// The undo a forced editor carries. Dismissing it puts `before` back, because
/// the placeholder it was opened on is a value nothing chose: for `outcome` it
/// is `win`, which reads as a filter the user set on purpose and silently
/// narrows the search.
#[derive(Debug, Clone, PartialEq)]
struct Restore {
    before: MatchExpr,
    /// The pill the mint added, when it added one, by the path it has in the
    /// tree the mint left behind. `None` when the placeholder replaced a value
    /// on a pill that already existed, which takes no pill away.
    minted: Option<NodePath>,
}

/// What dismissing an edit did, so a caller can both re-run the query and tell
/// whether the pill a click names survived.
#[derive(Debug, Default, PartialEq)]
struct Dismissal {
    /// The tree changed, so the query has to run again and the saved one has to
    /// be rewritten.
    restored: bool,
    /// The pill the restore took away, by the path it had while it existed.
    withdrew: Option<NodePath>,
}

/// A navigation key, read before the caret's `TextEdit` renders and applied
/// after. Splitting the two is what makes the ordering possible at all: once
/// the `TextEdit` has run it has already eaten the key.
#[derive(Debug, Clone, Copy)]
enum Nav {
    CloseDropdown,
    ReleaseFocus,
    CommitRow(usize),
    CommitTyped,
    HighlightNext,
    HighlightPrev,
    SelectPrev,
    DeleteSelection,
    Step { back: bool, extend: bool },
    HistoryBack,
}

/// A pointer interaction, collected while painting and applied afterwards so a
/// handler can take the tree mutably without fighting the paint loop's borrows.
#[derive(Debug, Clone)]
enum Command {
    SelectOnly(NodePath),
    SelectToggle(NodePath),
    SelectRange(NodePath),
    /// Open one segment's editor. Carries the pill's own path rather than a
    /// `segment_path`-resolved one, because the command is applied after a
    /// dismissal that can rewrite the tree the resolution has to hold against.
    EditSegment(NodePath, SegmentRole),
    Negate(NodePath),
    Delete(NodePath),
    SetConnector(NodePath, bool),
    Ungroup(NodePath),
}

/// What the bar's body reported back to `show` after painting itself.
struct Body {
    edited: bool,
    caret_changed: bool,
    caret_gained_focus: bool,
    /// A click landed somewhere in the bar this frame, the caret included. The
    /// caret is only granted focus on the frame after, so this is what tells
    /// `show` the bar is engaged before `has_focus` agrees.
    clicked_inside: bool,
    /// A click landed on a token: a pill, its chrome, or the background between
    /// them. The caret is deliberately not one, since a click there is a click
    /// inside whatever is being edited rather than away from it.
    clicked_token: bool,
    /// That click was on a pill segment, which is the one case that must not
    /// close the segment editor it just opened.
    clicked_segment: bool,
}

/// What a pass over one pill's segments found.
struct SegmentHit {
    clicked: bool,
    /// The rect of the segment the dropdown is editing, when this pill holds it.
    anchor: Option<Rect>,
}

/// The bar's own id and its caret's, bundled so `rows` -- which also needs
/// `cfg` for segment painting -- stays under clippy's argument-count lint
/// without dropping either id.
struct BarIds {
    id: egui::Id,
    caret_id: egui::Id,
}

/// What the measure and layout passes produced, bundled for the same reason
/// `BarIds` is: `rows` needs all of it and clippy counts arguments.
#[derive(Clone, Copy)]
struct Measured<'a> {
    galleys: &'a [Vec<Arc<Galley>>],
    laid: &'a LaidOut,
    cfg: &'a LayoutCfg,
    /// The pill the caret is standing inside. `None` while the caret is the
    /// trailing one.
    slot: Option<Slot<'a>>,
}

/// The pill an inline box is being typed into. It has left the drawn stream, so
/// everything a pill normally gets from the token loop -- its click target, its
/// segments', its menu -- is registered against this instead.
#[derive(Clone, Copy)]
struct Slot<'a> {
    pill: &'a Token,
    /// Which of the pill's segments the box occupies.
    segment: usize,
}

impl QueryBar {
    /// Replaces the query outright, as a seeded search or a restored setting
    /// does. Clears everything that described the old tree, and canonicalises
    /// on the way in: a seeded tree that carries a one-condition group would
    /// otherwise print as a bare term and reparse without it, and nothing else
    /// would notice until the first edit.
    pub fn set_expr(&mut self, expr: MatchExpr) {
        self.expr = expr;
        select::canonicalise(&mut self.expr);
        self.selection.clear();
        self.anchor = None;
        self.focus = None;
        self.clear_pending();
    }

    pub fn show(&mut self, ui: &mut Ui, deps: &Deps<'_>) -> QueryBarOutput {
        let id = ui.id().with("query_bar");
        let caret_id = id.with("caret");
        let focused = ui.memory(|m| m.has_focus(caret_id));
        let escaped = ui.input(|i| i.key_pressed(Key::Escape));

        self.refresh_suggestions();
        self.reparse_pending();

        // Only a focused bar reads keys, and the rows exist to answer them, so
        // an unfocused bar skips ranking the whole suggestion set every frame.
        let rows = if focused { self.dropdown_rows() } else { Vec::new() };
        let nav = if focused { self.consume_navigation(ui) } else { None };

        let (mut tokens, mut replaced) = self.stream();
        self.selection.retain_present(&tokens);

        let mut edited = false;
        if let Some(nav) = nav {
            edited |= self.apply_nav(nav, &rows, deps, ui, caret_id);
        }
        if edited {
            self.finish_edit();
            (tokens, replaced) = self.stream();
        }

        let frame = paint::bar_frame(ui, focused);
        let framed = frame.show(ui, |ui| self.body(ui, id, caret_id, &tokens, replaced.as_ref()));
        let body = framed.inner;
        edited |= body.edited;

        // A click counts as engagement even on the frame the caret has not been
        // granted focus yet, which is the frame every first click lands on.
        // Reading `focused` alone there would close a dropdown that same click
        // had just opened.
        if focused || body.clicked_inside {
            // A click on a pill or on the background leaves whatever segment
            // was being edited: the pointer has moved on. A click in the caret
            // has not moved on -- for a plain value editor the caret *is* the
            // editor, holding the literal it was seeded with, and for the other
            // two it holds the needle -- so clicking into that text to correct
            // it must not throw it away.
            if body.clicked_token && !body.clicked_segment {
                edited |= self.commit_open_box();
                edited |= self.end_segment_edit().restored;
            }
            if body.clicked_inside || body.caret_changed || body.caret_gained_focus || self.editing.is_some() {
                self.dropdown_open = true;
            }
        } else {
            // Escape reaches the bar as a focus loss and nothing else: egui
            // clears the focused widget while reading the pass's events, before
            // any of this runs. Telling that apart from the pointer moving on is
            // what keeps the one gesture that reverts from committing instead.
            if !escaped {
                edited |= self.commit_open_box();
            }
            edited |= self.end_segment_edit().restored;
            self.dropdown_open = false;
            self.highlighted = None;
        }

        edited |= self.toolbar(ui);
        edited |= self.dropdown(ui, id, caret_id, framed.response.rect);

        if edited {
            self.finish_edit();
        }
        QueryBarOutput { changed: edited, request: self.pending_request.take() }
    }

    /// The token stream as it is drawn: the committed tree, tokenized, with the
    /// caret moved into the slot of the pill whose value is being typed. Hands
    /// that pill back, since it is no longer in the stream and the caret's cell
    /// still has to be measured and painted as it.
    fn stream(&self) -> (Vec<Token>, Option<Token>) {
        let mut tokens = tokenize(&self.expr, &self.names);
        let Some((edit, _)) = self.inline_edit() else {
            return (tokens, None);
        };
        // Only a pill that draws a value segment has a slot to type into.
        // Nothing opens a value box on one that does not, and checking first is
        // what keeps that from taking a pill out of the stream with nowhere to
        // put the box back.
        if !tokens.iter().any(|token| token.path == edit.pill && boxed_segment(token).is_some()) {
            return (tokens, None);
        }
        let replaced = tokens::move_caret_to(&mut tokens, &edit.pill);
        (tokens, replaced)
    }

    /// The open value edit and its inline box, when one is open.
    fn inline_edit(&self) -> Option<(&SegmentEdit, &InlineValue)> {
        let edit = self.editing.as_ref()?;
        Some((edit, edit.inline.as_ref()?))
    }

    /// Every pill the selection can address, over the full stream rather than
    /// the drawn one. A pill the caret is standing in for is still part of the
    /// query and missing from what is painted, so a range taken off the drawn
    /// stream would silently omit a term the user highlighted and leave a later
    /// group or delete acting on fewer than they chose.
    fn selectable_paths(&self) -> Vec<NodePath> {
        select::selectable_paths(&self.expr, &tokenize(&self.expr, &self.names))
    }

    /// Steps 4 through 9: measure, lay out, paint, run the caret, and collect
    /// what the pointer did.
    fn body(
        &mut self,
        ui: &mut Ui,
        id: egui::Id,
        caret_id: egui::Id,
        tokens: &[Token],
        replaced: Option<&Token>,
    ) -> Body {
        let font = egui::TextStyle::Body.resolve(ui.style());
        let cfg = LayoutCfg {
            row_height: ui.spacing().interact_size.y + 2.0 * ROW_PAD_Y,
            gap: ROW_GAP,
            indent: DEPTH_INDENT,
            max_rows: MAX_ROWS,
            // `row_height` above already derives from `interact_size.y`; reusing
            // it here makes the smallest possible segment roughly square, a
            // natural click-target floor rather than borrowing the whole
            // widget's minimum width for a sub-region of an already-wide pill.
            min_segment_width: ui.spacing().interact_size.y,
            segment_gap: SEGMENT_GAP,
        };
        // The caret standing in a pill's slot is measured as that pill, with the
        // segment it is typing into measured from the text in it. That makes the
        // cell exactly as wide as the pill needs, so the query neither shifts on
        // open nor shifts back on commit, and it grows with what is typed.
        let slot = replaced.and_then(|pill| boxed_segment(pill).map(|segment| Slot { pill, segment }));
        let galleys: Vec<Vec<Arc<Galley>>> = tokens
            .iter()
            .map(|token| match (&token.kind, slot) {
                (TokenKind::Caret, Some(slot)) => inline_galleys(ui, slot.pill, &font, slot.segment, &self.pending),
                _ => token_galleys(ui, token, &font),
            })
            .collect();
        let widths: Vec<f32> = tokens
            .iter()
            .zip(&galleys)
            .map(|(token, runs)| match (&token.kind, slot) {
                (TokenKind::Caret, Some(_)) => layout::pill_width(&paint::segment_widths(runs), &cfg),
                _ => token_width(token, runs, &cfg),
            })
            .collect();

        let full_width = ui.available_width();
        let mut laid = lay_out(tokens, &widths, full_width, &cfg);
        if laid.needs_scroll {
            laid = lay_out(tokens, &widths, (full_width - SCROLLBAR_ALLOWANCE).max(1.0), &cfg);
        }

        let ids = BarIds { id, caret_id };
        let measured = Measured { galleys: &galleys, laid: &laid, cfg: &cfg, slot };
        if laid.needs_scroll {
            egui::ScrollArea::vertical()
                .id_salt(id.with("scroll"))
                .max_height(cfg.max_rows as f32 * cfg.row_height)
                .auto_shrink([false, false])
                .show(ui, |ui| self.rows(ui, &ids, tokens, &measured))
                .inner
        } else {
            self.rows(ui, &ids, tokens, &measured)
        }
    }

    fn rows(&mut self, ui: &mut Ui, ids: &BarIds, tokens: &[Token], measured: &Measured<'_>) -> Body {
        let Measured { galleys, laid, cfg, slot } = *measured;
        let (_, area) = ui.allocate_space(egui::vec2(ui.available_width(), laid.height));
        let origin = area.min.to_vec2();

        // Registered before the tokens so a click on a pill wins the hit test
        // and only a click on the gaps falls through to focusing the caret.
        let background = ui.interact(area, ids.id.with("background"), Sense::click());

        // A nested group's row rect shares its top and bottom edge with the
        // group containing it, since every token on a row spans the full row
        // height. Whichever bracket is drawn last owns that shared edge, so the
        // deepest goes first and the outermost paints over it: the heavier,
        // brighter stroke is the one that should survive, and the inner group
        // is still delimited by its own left and right edges.
        let mut spans: Vec<&layout::GroupSpan> = laid.group_spans.iter().collect();
        spans.sort_by_key(|span| std::cmp::Reverse(tokens[span.open_index].depth));
        for span in spans {
            let depth = tokens[span.open_index].depth;
            let last = span.rows.len().saturating_sub(1);
            for (n, (_, row_rect)) in span.rows.iter().enumerate() {
                paint::group_row(ui, row_rect.translate(origin), depth, n == 0, n == last);
            }
        }

        let mut commands: Vec<Command> = Vec::new();
        // A click anywhere in the bar puts the caret back in focus, but the
        // request cannot be made until the caret widget itself has been built.
        // See the token loop below and the caret call after it.
        let mut refocus = background.clicked();
        let mut clicked_token = false;
        let mut clicked_segment = false;
        let mut anchor = None;
        for placed in &laid.placed {
            let token = &tokens[placed.index];
            if matches!(token.kind, TokenKind::Caret) {
                continue;
            }
            let rect = placed.rect.translate(origin).shrink2(egui::vec2(0.0, ROW_PAD_Y));
            let response = ui.interact(rect, ids.id.with(placed.index), Sense::click());
            // Only recorded here. egui runs its surrender check as each widget
            // is created, so focus requested at this point is taken straight
            // back when the caret is built below; the request has to happen
            // after it.
            let hit = response.clicked() || response.double_clicked();
            refocus |= hit;
            clicked_token |= hit;
            let state = if self.selection.contains(&token.path) {
                TokenState::Selected
            } else if response.hovered() {
                TokenState::Hovered
            } else {
                TokenState::Idle
            };
            // Registered after the pill so a segment wins the hit test over it.
            // What that leaves the pill is the gap between two segments, which
            // belongs to neither and is where the separator is drawn: a click
            // there selects the pill rather than being attributed to whichever
            // neighbour happens to be closer.
            let seg_rects = paint::segment_rects(rect, &paint::segment_widths(&galleys[placed.index]), cfg);
            let hit = self.segment_interaction(
                ui,
                ids.id.with("segment").with(placed.index),
                token,
                &seg_rects,
                None,
                &mut commands,
            );
            refocus |= hit.clicked;
            clicked_token |= hit.clicked;
            clicked_segment |= hit.clicked;
            anchor = anchor.or(hit.anchor);
            paint::token(ui, rect, &galleys[placed.index], &token.kind, state, cfg);
            self.token_interaction(&response, token, &mut commands);
        }
        let caret_cell = caret_rect(tokens, laid, origin);
        // The cell holds the whole pill while a value is being typed into it, so
        // the box is the sub-rect of the segment it stands in and the rest of the
        // pill is drawn and hit-tested around it.
        let caret_box = match slot {
            Some(slot) => {
                let index = caret_index(tokens, laid);
                let rects = paint::segment_rects(caret_cell, &paint::segment_widths(&galleys[index]), cfg);
                // Every target the token loop would have registered for this
                // pill, registered here because the pill is no longer in the
                // stream it walks. Without them the segments beside the box are
                // painted, washed on hover, and dead: a click on one falls
                // through to the bar's background and opens the caret's own
                // list rather than that segment's.
                let response = ui.interact(paint::pill_surface(caret_cell, &rects), ids.id.with(index), Sense::click());
                let state = if self.selection.contains(&slot.pill.path) {
                    TokenState::Selected
                } else if response.hovered() {
                    TokenState::Hovered
                } else {
                    TokenState::Idle
                };
                let hit = self.segment_interaction(
                    ui,
                    ids.id.with("segment").with(index),
                    slot.pill,
                    &rects,
                    Some(slot.segment),
                    &mut commands,
                );
                refocus |= hit.clicked || response.clicked();
                clicked_token |= hit.clicked || response.clicked();
                clicked_segment |= hit.clicked;
                paint::inline_pill(ui, caret_cell, &galleys[index], slot.segment, state, cfg);
                self.token_interaction(&response, slot.pill, &mut commands);
                let boxed = rects.get(slot.segment).copied().unwrap_or(caret_cell);
                paint::inline_box(ui, boxed);
                boxed
            }
            None => caret_cell,
        };
        // The pill holding the box is out of the stream, so no segment of it
        // reported an anchor above; the box is what the list hangs under.
        self.popup_anchor = anchor.or(slot.map(|_| caret_box));

        let caret = self.caret(ui, ids.caret_id, caret_box);
        // After the caret, never before: egui surrenders focus from any focused
        // widget the pointer is not over as that widget is created, so a
        // request made earlier in the frame is undone by the caret's own
        // creation and the click reads as unfocused on the very next frame.
        // That in turn skips `consume_navigation` entirely, which is what makes
        // Backspace-deletes-the-selection and Shift+Arrow-extends unreachable
        // from a selection made with the mouse.
        if refocus {
            ui.memory_mut(|m| m.request_focus(ids.caret_id));
        }

        // The background is registered over the whole bar, the caret included,
        // so a click in the caret reaches it too and `background.clicked()`
        // alone cannot tell a click on the gaps from a click on the text being
        // edited. The pointer position can, and that distinction is what keeps
        // clicking into a seeded value from discarding it.
        //
        // Measured against the caret's laid-out rect rather than the
        // `TextEdit`'s own response rect: the widget sizes itself to its
        // content and can be shorter than the row it sits in, which would leave
        // part of the caret reading as background. The box's rect, not the whole
        // cell, so the field and operator segments drawn beside it stay outside
        // what the caret owns and a click on one of them still ends the edit.
        //
        // Both axes, and the full row band rather than the inset cell.
        // Testing x alone would hand the caret every row above it, since
        // `lay_out` stretches a trailing caret to the bar's right edge and a
        // wrapped caret starts at x = 0, making its x-range the whole bar. The
        // band undoes `caret_rect`'s own `ROW_PAD_Y` inset, so the padding
        // between rows belongs to the row it separates rather than to neither.
        let band = caret_box.expand2(egui::vec2(0.0, ROW_PAD_Y));
        let on_caret = background.interact_pointer_pos().is_some_and(|pos| band.contains(pos));
        clicked_token |= background.clicked() && !on_caret;

        let mut edited = false;
        for command in commands {
            edited |= self.apply_command(command, ui, ids.caret_id);
        }
        Body {
            edited,
            caret_changed: caret.response.response.changed(),
            caret_gained_focus: caret.response.response.gained_focus(),
            clicked_inside: refocus || caret.response.response.clicked(),
            clicked_token,
            clicked_segment,
        }
    }

    /// One click target per segment of one pill, over exactly the rects
    /// `paint::pill` draws them at.
    ///
    /// A pill whose segments name no editable term registers nothing at all --
    /// free text carries no field or operator, and a roster quantifier too
    /// complex to collapse carries several. A click on one of those reaches the
    /// pill and selects it, rather than landing on a target that could not act.
    ///
    /// `boxed` names the segment an inline box has taken over, which registers
    /// nothing. Insurance rather than a live guard: the box's `TextEdit` is
    /// built after this and wins the hit test either way, so removing the skip
    /// changes nothing the suite can see. It is kept so the box's target stays
    /// the box's alone rather than resting on widget ordering elsewhere.
    fn segment_interaction(
        &self,
        ui: &Ui,
        id: egui::Id,
        token: &Token,
        rects: &[Rect],
        boxed: Option<usize>,
        commands: &mut Vec<Command>,
    ) -> SegmentHit {
        let mut hit = SegmentHit { clicked: false, anchor: None };
        let TokenKind::Pill { segments } = &token.kind else {
            return hit;
        };
        let Some(path) = select::segment_path(&self.expr, &token.path) else {
            return hit;
        };
        for (i, (rect, segment)) in rects.iter().zip(segments).enumerate() {
            if boxed == Some(i) {
                continue;
            }
            let response = ui.interact(*rect, id.with(i), Sense::click());
            // Also on the segment, not only on the pill: the segment sits on
            // top of it, so a right-click never reaches the pill's own menu.
            let expr = &self.expr;
            let pill_path = &token.path;
            response.context_menu(|ui| pill_menu(ui, expr, pill_path, commands));
            if response.clicked() {
                hit.clicked = true;
                commands.push(Command::EditSegment(token.path.clone(), segment.role));
            }
            if self.editing.as_ref().is_some_and(|edit| edit.path == path && edit.role == segment.role) {
                hit.anchor = Some(*rect);
            }
        }
        hit
    }

    /// Step 8: the caret is a real `TextEdit` sized to the rect the layout pass
    /// gave it, so IME, selection, and the clipboard behave the way they do
    /// anywhere else. That rect is its whole laid-out cell, or -- while a value
    /// is being typed into a pill -- the segment of that pill it stands in.
    fn caret(&mut self, ui: &mut Ui, caret_id: egui::Id, rect: Rect) -> egui::text_edit::TextEditOutput {
        let width = rect.width();
        // The hint describes an empty bar; beside committed pills it would read
        // as another filter rather than as placeholder text.
        let hint = if self.expr.is_empty_all() { t!("ui.search.bar.hint").into_owned() } else { String::new() };
        let output = ui
            .scope_builder(UiBuilder::new().max_rect(rect), |ui| {
                egui::TextEdit::singleline(&mut self.pending)
                    .id(caret_id)
                    .frame(egui::Frame::NONE)
                    .desired_width(width)
                    .hint_text(hint)
                    .show(ui)
            })
            .inner;

        let offset = output.cursor_range.map(|range| range.primary.index.0);
        let length = self.pending.chars().count();
        self.caret_at_start = offset.is_none_or(|i| i == 0);
        self.caret_at_end = offset.is_none_or(|i| i >= length);

        if self.editing.is_none() {
            // Anchored to the start of the fragment being typed, deliberately
            // not to the moving caret, so the list does not slide sideways with
            // every keystroke. `active_fragment_start` is a byte offset and
            // `CCursor` takes a character index, which `char_index` converts
            // without slicing the string.
            let start = paint::char_index(&self.pending, suggest::active_fragment_start(&self.pending));
            let x = output.galley_pos.x + output.galley.pos_from_cursor(CCursor::new(start)).left();
            self.popup_anchor =
                Some(Rect::from_min_max(Pos2::new(x, rect.top()), Pos2::new(rect.right().max(x), rect.bottom())));
        }

        if let Some(error) = &self.pending_error
            && !self.pending.is_empty()
        {
            paint::error_underline(ui, output.galley_pos, &output.galley, &self.pending, &error.span);
        }
        output
    }

    fn token_interaction(&self, response: &egui::Response, token: &Token, commands: &mut Vec<Command>) {
        match &token.kind {
            TokenKind::Pill { .. } => {
                let expr = &self.expr;
                let path = &token.path;
                response.context_menu(|ui| pill_menu(ui, expr, path, commands));
                // A pill inside a roster predicate draws inside its
                // quantifier's bracket but names no match-level node, so
                // selecting it would offer a toolbar whose edits cannot
                // address it. The keyboard already steps past those pills
                // (`selectable_paths`); the pointer has to agree.
                if !select::addresses_match_node(expr, path) {
                    return;
                }
                if response.clicked() {
                    let modifiers = response.ctx.input(|i| i.modifiers);
                    commands.push(if modifiers.shift {
                        Command::SelectRange(token.path.clone())
                    } else if modifiers.command {
                        Command::SelectToggle(token.path.clone())
                    } else {
                        Command::SelectOnly(token.path.clone())
                    });
                }
            }
            TokenKind::Connector { is_or } | TokenKind::GroupOpen { is_or } => {
                response.clone().on_hover_text(t!("ui.search.bar.flip_connector").into_owned());
                if response.clicked() {
                    commands.push(Command::SetConnector(token.path.clone(), !is_or));
                }
            }
            TokenKind::GroupClose => {
                response.clone().on_hover_text(t!("ui.search.bar.ungroup").into_owned());
                if response.clicked() {
                    commands.push(Command::Ungroup(token.path.clone()));
                }
            }
            // A quantifier bracket is neither an AND/OR to flip nor a group to
            // dissolve; its quantifier changes through negation.
            TokenKind::QuantOpen { .. } | TokenKind::QuantClose | TokenKind::NotPrefix | TokenKind::Caret => {}
        }
    }

    /// Step 10: the selection toolbar. Group is offered only when `can_group`
    /// allows it, which is the one rule here that is not a rendering detail and
    /// so is not decided here.
    fn toolbar(&mut self, ui: &mut Ui) -> bool {
        if self.selection.is_empty() {
            return false;
        }
        let mut edited = false;
        ui.horizontal_wrapped(|ui| {
            let groupable = select::can_group(&self.expr, &self.selection);
            if ui.add_enabled(groupable, egui::Button::new(t!("ui.search.bar.group_and").into_owned())).clicked() {
                select::group(&mut self.expr, &self.selection, false);
                edited = true;
            }
            if ui.add_enabled(groupable, egui::Button::new(t!("ui.search.bar.group_or").into_owned())).clicked() {
                select::group(&mut self.expr, &self.selection, true);
                edited = true;
            }
            if ui.button(t!("ui.search.bar.negate").into_owned()).clicked() {
                for path in self.selection.nodes.clone() {
                    select::negate(&mut self.expr, &path);
                }
                edited = true;
            }
            if ui.button(t!("ui.search.bar.delete").into_owned()).clicked() {
                select::delete(&mut self.expr, &self.selection);
                edited = true;
            }
            if ui.button(t!("ui.search.bar.clear_selection").into_owned()).clicked() {
                self.selection.clear();
                self.anchor = None;
                self.focus = None;
            }
        });
        edited
    }

    /// Step 4 of the dropdown: the suggestion list, anchored under whatever is
    /// being edited.
    fn dropdown(&mut self, ui: &mut Ui, id: egui::Id, caret_id: egui::Id, bar_rect: Rect) -> bool {
        if !self.dropdown_open {
            return false;
        }
        let rows = self.dropdown_rows();
        let prompt = self.plain_value_field().map(TermField::label);
        // A bar with nothing typed into its active fragment says nothing by
        // staying shut, rather than by reporting that nothing matches an empty
        // needle. An open segment edit always has something to say -- its own
        // list, or the name of the field whose value the caret is holding -- so
        // it never falls into that case.
        if rows.is_empty()
            && prompt.is_none()
            && self.editing.is_none()
            && suggest::active_fragment(&self.pending).trim().is_empty()
        {
            return false;
        }
        // Typing narrows the list under a highlight that was set against the
        // longer one, so an index left dangling would swallow the next Enter.
        if self.highlighted.is_some_and(|i| i >= rows.len()) {
            self.highlighted = None;
        }

        let mut picked = None;
        // Anchored to the segment being edited, or to the start of the fragment
        // being typed; the bar is the fallback for a frame where neither was
        // drawn. The alternative alignments are left at egui's defaults, which
        // already flip and shift a popup that would otherwise leave the screen.
        let anchor = self.popup_anchor.unwrap_or(bar_rect);
        egui::Popup::new(id.with("dropdown"), ui.ctx().clone(), anchor, ui.layer_id())
            .open(true)
            .align(egui::RectAlign::BOTTOM_START)
            .show(|ui| {
                // The width range is imposed here, every frame, rather than
                // through `Popup::width`. That one reaches `Area::default_size`,
                // which the area reads inside `state.size.get_or_insert_with(..)`
                // and then only as the sizing pass's *maximum*, so it caps the
                // popup without ever widening it: measured at 117.5 for the
                // filter list against a 790 bar, then 53.6 once an operator
                // list of `is` and `is not` had been shown, with no way back.
                // Bounded on both sides instead of pinned to one width: the
                // bar's width is the ceiling, so the list never reads as wider
                // than the control it belongs to, and the anchor's own width is
                // the floor, so the popup reads as belonging to the segment or
                // fragment it hangs under rather than to the bar as a whole.
                // `MIN_CARET_WIDTH` covers the anchor collapsing to a point, the
                // caret's own floor for the same reason.
                // `ui` here is already the frame's own inner content area, so
                // capping it at the bar's raw width would still leave the
                // popup's own frame -- margin and stroke both -- poking past
                // the bar on the outside. `Frame::popup` is the frame this
                // popup falls back to (no `.frame()` override is set above),
                // and `total_margin` is what it actually adds around the
                // content, so subtracting it is what makes the popup's
                // painted edge, frame included, land on the bar's.
                let overhead = egui::Frame::popup(ui.style()).total_margin().sum().x;
                let max_width = (bar_rect.width() - overhead).max(0.0);
                let min_width = anchor.width().max(MIN_CARET_WIDTH).min(max_width);
                ui.set_width_range(min_width..=max_width);
                if let Some(prompt) = &prompt {
                    ui.label(egui::RichText::new(prompt.clone()).color(ui.sem().text_dim));
                }
                if rows.is_empty() {
                    if prompt.is_none() {
                        ui.label(
                            egui::RichText::new(t!("ui.search.bar.no_suggestions").into_owned())
                                .color(ui.sem().text_dim),
                        );
                    }
                    return;
                }
                egui::ScrollArea::vertical().max_height(MAX_DROPDOWN_HEIGHT).show(ui, |ui| {
                    for (index, row) in rows.iter().enumerate() {
                        let selected = self.highlighted == Some(index);
                        let clicked = ui
                            .add_enabled_ui(self.row_enabled(row), |ui| {
                                let response = ui.selectable_label(selected, self.row_text(ui, row));
                                if selected {
                                    response.scroll_to_me(None);
                                }
                                response.clicked()
                            })
                            .inner;
                        if clicked {
                            picked = Some(row.clone());
                        }
                    }
                });
            });

        let Some(row) = picked else {
            return false;
        };
        let edited = self.commit_row(row, ui, caret_id);
        ui.memory_mut(|m| m.request_focus(caret_id));
        edited
    }

    /// Whether picking a row would take. Only the operator list has rows that
    /// would not: `set_op` refuses moving off a nullary operator onto one that
    /// needs a right-hand side the term no longer carries. Offered greyed out
    /// rather than hidden, so the field's full operator set stays visible.
    fn row_enabled(&self, row: &Row) -> bool {
        let Row::Operator(option) = row else {
            return true;
        };
        let Some(edit) = self.editing.as_ref() else {
            return false;
        };
        select::can_set_op(&self.expr, &edit.path, option.op)
    }

    /// Where the keyboard highlight moves, skipping every row the pointer is
    /// not allowed to click. A greyed row is offered so the field's full
    /// operator set stays readable, not so Enter can land on it and quietly do
    /// nothing.
    ///
    /// Stepping forward off the last enabled row stays put, the way a clamped
    /// index always has. Stepping back off the first leaves the list, which is
    /// what returns the caret to plain typing.
    fn step_highlight(&self, rows: &[Row], back: bool) -> Option<usize> {
        let enabled = |i: &usize| rows.get(*i).is_some_and(|row| self.row_enabled(row));
        match (self.highlighted, back) {
            (None, false) => (0..rows.len()).find(enabled),
            (None, true) => (0..rows.len()).rfind(enabled),
            (Some(current), false) => (current + 1..rows.len()).find(enabled).or(Some(current)),
            (Some(current), true) => (0..current).rfind(enabled),
        }
    }

    /// The row Enter takes with nothing highlighted: the first that would
    /// actually take, for the same reason `step_highlight` skips the rest.
    fn default_row(&self, rows: &[Row]) -> Option<Row> {
        rows.iter().find(|row| self.row_enabled(row)).cloned()
    }

    fn row_text(&self, ui: &Ui, row: &Row) -> egui::text::LayoutJob {
        let font = egui::TextStyle::Body.resolve(ui.style());
        let mut job = egui::text::LayoutJob::default();
        let (label, context) = match row {
            Row::Suggestion(i) => match self.suggestions.get(*i) {
                Some(s) => (s.label.clone(), Some(category_label(s.context))),
                None => (String::new(), None),
            },
            Row::Value(i) => (self.value_options.get(*i).map(|v| v.label.clone()).unwrap_or_default(), None),
            Row::Field(option) => (option.label.clone(), None),
            Row::Operator(option) => (option.label.clone(), None),
            Row::Literal(option) => (option.label.clone(), None),
        };
        job.append(&label, 0.0, egui::TextFormat::simple(font.clone(), ui.visuals().text_color()));
        if let Some(context) = context {
            job.append(&context, 8.0, egui::TextFormat::simple(font, ui.sem().text_dim));
        }
        job
    }

    /// The rows the dropdown shows: the open segment's own source, or the
    /// caret's.
    fn dropdown_rows(&self) -> Vec<Row> {
        match &self.editing {
            Some(edit) => self.segment_rows(edit),
            None => self.caret_rows(),
        }
    }

    /// The rows the caret offers. Value rows displace the static suggestions
    /// entirely: once the caret is typing a value, the field list is no longer
    /// what the user is choosing from.
    ///
    /// Ranking is against the active fragment, not the whole caret. `rank`
    /// matches its needle against suggestion labels, so the whole string stops
    /// matching anything the moment a second term is typed, and every
    /// suggestion becomes unreachable part way through a query.
    fn caret_rows(&self) -> Vec<Row> {
        if self.active_request.is_some() && !self.value_options.is_empty() {
            return (0..self.value_options.len()).map(Row::Value).collect();
        }
        let needle = suggest::active_fragment(&self.pending);
        rank(needle, &self.suggestions).into_iter().map(Row::Suggestion).collect()
    }

    /// The rows one segment offers, from the source its role names. The caret's
    /// text is this list's needle, so a long list narrows by typing the same way
    /// the caret's own does.
    fn segment_rows(&self, edit: &SegmentEdit) -> Vec<Row> {
        let Some((field, _, _)) = select::term_at(&self.expr, &edit.path) else {
            return Vec::new();
        };
        let needle = self.pending.trim();
        match edit.role {
            SegmentRole::Filter => field_options(field)
                .into_iter()
                .filter(|option| suggest::matches(&option.label, needle))
                .map(Row::Field)
                .collect(),
            SegmentRole::Operator => operator_options(field).into_iter().map(Row::Operator).collect(),
            SegmentRole::Value => match suggest::value_editor(field) {
                ValueEditor::Enum(options) => options
                    .into_iter()
                    .filter(|option| suggest::matches(&option.label, needle))
                    .map(Row::Literal)
                    .collect(),
                ValueEditor::Lookup => (0..self.value_options.len()).map(Row::Value).collect(),
                ValueEditor::Plain => Vec::new(),
            },
        }
    }

    /// The field whose value the caret is holding as a literal, when a plain
    /// value editor is the one open. `None` for every other state, including
    /// the two value editors that offer rows.
    fn plain_value_field(&self) -> Option<TermField> {
        let edit = self.editing.as_ref()?;
        if edit.role != SegmentRole::Value {
            return None;
        }
        let (field, _, _) = select::term_at(&self.expr, &edit.path)?;
        matches!(suggest::value_editor(field), ValueEditor::Plain).then_some(field)
    }

    /// Steps 1 and 2: read the caret position captured last frame, then take
    /// the navigation keys before the `TextEdit` can.
    fn consume_navigation(&self, ui: &mut Ui) -> Option<Nav> {
        let take = |modifiers, key| ui.input_mut(|i| i.consume_key(modifiers, key));
        // While an editor is open the caret is that editor's buffer, so the keys
        // that navigate the pill stream instead -- history, selection, and
        // stepping off the ends -- would act on a query the user is not typing.
        let editing = self.editing.is_some();

        if take(Modifiers::NONE, Key::Escape) {
            return Some(if self.dropdown_open { Nav::CloseDropdown } else { Nav::ReleaseFocus });
        }
        if take(Modifiers::NONE, Key::Enter) {
            return Some(self.highlighted.map_or(Nav::CommitTyped, Nav::CommitRow));
        }
        if take(Modifiers::NONE, Key::ArrowDown) {
            return Some(Nav::HighlightNext);
        }
        if take(Modifiers::NONE, Key::ArrowUp) {
            let in_list = editing || self.highlighted.is_some() || !self.pending.is_empty();
            return Some(if in_list { Nav::HighlightPrev } else { Nav::HistoryBack });
        }
        if editing {
            return None;
        }
        if self.pending.is_empty() && take(Modifiers::NONE, Key::Backspace) {
            return Some(if self.selection.is_empty() { Nav::SelectPrev } else { Nav::DeleteSelection });
        }
        if self.caret_at_start {
            if take(Modifiers::SHIFT, Key::ArrowLeft) {
                return Some(Nav::Step { back: true, extend: true });
            }
            if take(Modifiers::NONE, Key::ArrowLeft) {
                return Some(Nav::Step { back: true, extend: false });
            }
        }
        if self.caret_at_end && !self.selection.is_empty() {
            if take(Modifiers::SHIFT, Key::ArrowRight) {
                return Some(Nav::Step { back: false, extend: true });
            }
            if take(Modifiers::NONE, Key::ArrowRight) {
                return Some(Nav::Step { back: false, extend: false });
            }
        }
        None
    }

    fn apply_nav(&mut self, nav: Nav, rows: &[Row], deps: &Deps<'_>, ui: &mut Ui, caret_id: egui::Id) -> bool {
        let paths = self.selectable_paths();
        match nav {
            Nav::CloseDropdown => {
                let dismissed = self.end_segment_edit();
                self.dropdown_open = false;
                self.highlighted = None;
                dismissed.restored
            }
            Nav::ReleaseFocus => {
                ui.memory_mut(|m| m.surrender_focus(caret_id));
                let dismissed = self.end_segment_edit();
                self.selection.clear();
                dismissed.restored
            }
            Nav::CommitRow(index) => rows.get(index).cloned().is_some_and(|row| self.commit_row(row, ui, caret_id)),
            Nav::CommitTyped => self.commit_typed(rows, ui, caret_id),
            Nav::HighlightNext => {
                if !rows.is_empty() {
                    if let Some(next) = self.step_highlight(rows, false) {
                        self.highlighted = Some(next);
                    }
                    self.dropdown_open = true;
                }
                false
            }
            Nav::HighlightPrev => {
                self.highlighted = self.step_highlight(rows, true);
                self.dropdown_open = true;
                false
            }
            Nav::SelectPrev => {
                if let Some(path) = select::step(&paths, None, true) {
                    self.select_single(path);
                }
                false
            }
            Nav::DeleteSelection => {
                select::delete(&mut self.expr, &self.selection);
                true
            }
            Nav::Step { back, extend } => {
                self.step_selection(&paths, back, extend);
                false
            }
            Nav::HistoryBack => self.walk_history(deps),
        }
    }

    fn step_selection(&mut self, paths: &[NodePath], back: bool, extend: bool) {
        let from = self.focus.clone();
        let Some(target) = select::step(paths, from.as_deref(), back) else {
            // Stepping forward off the last pill puts the caret back in the
            // text, which is where the user was heading.
            if !back {
                self.selection.clear();
                self.anchor = None;
                self.focus = None;
            }
            return;
        };
        if !extend {
            self.select_single(target);
            return;
        }
        let anchor = self.anchor.clone().unwrap_or_else(|| target.clone());
        self.selection.set_many(select::range(paths, &anchor, &target));
        self.anchor = Some(anchor);
        self.focus = Some(target);
    }

    fn select_single(&mut self, path: NodePath) {
        self.anchor = Some(path.clone());
        self.focus = Some(path.clone());
        self.selection.set_one(path);
    }

    fn walk_history(&mut self, deps: &Deps<'_>) -> bool {
        let next = self.history_cursor.map_or(0, |i| i + 1);
        let Some(entry) = deps.history.get(next) else {
            return false;
        };
        let Ok(expr) = parse_query(entry) else {
            return false;
        };
        self.expr = expr;
        self.clear_pending();
        self.selection.clear();
        // After `clear_pending`, which resets the cursor it would otherwise
        // undo: the walk has to survive to the next Up.
        self.history_cursor = Some(next);
        true
    }

    /// Applies one pointer command. A command that rewrites the tree dismisses
    /// whatever editor was open first: the pointer has moved on to another pill,
    /// and a forced editor's snapshot predates this command, so restoring it
    /// afterwards would undo the very edit that was just asked for.
    fn apply_command(&mut self, command: Command, ui: &Ui, caret_id: egui::Id) -> bool {
        match command {
            Command::EditSegment(pill, role) => {
                // Moving from a value box to another segment of the same pill is
                // still moving on, so the box commits first, for the same reason
                // a click on the bar's background does. Swapping one value for
                // another moves no path, so the click's own pill is still where
                // it was resolved.
                let committed = self.commit_open_box();
                let dismissed = self.end_segment_edit();
                if dismissed.withdrew.as_deref() != Some(pill.as_slice()) {
                    self.open_clicked_segment(&pill, role, &dismissed, ui, caret_id);
                }
                committed || dismissed.restored
            }
            Command::SelectOnly(path) => {
                self.select_single(path);
                false
            }
            Command::SelectToggle(path) => {
                self.anchor = Some(path.clone());
                self.focus = Some(path.clone());
                self.selection.toggle(path);
                false
            }
            Command::SelectRange(path) => {
                let paths = self.selectable_paths();
                let anchor = self.anchor.clone().unwrap_or_else(|| path.clone());
                self.selection.set_many(select::range(&paths, &anchor, &path));
                self.anchor = Some(anchor);
                self.focus = Some(path);
                false
            }
            Command::Negate(path) => {
                self.end_segment_edit();
                select::negate(&mut self.expr, &path);
                true
            }
            Command::Delete(path) => {
                self.end_segment_edit();
                select::delete(&mut self.expr, &Selection { nodes: vec![path] });
                true
            }
            Command::SetConnector(path, is_or) => {
                self.end_segment_edit();
                select::set_connector(&mut self.expr, &path, is_or);
                true
            }
            Command::Ungroup(path) => {
                let dismissed = self.end_segment_edit();
                select::ungroup(&mut self.expr, &path) || dismissed.restored
            }
        }
    }

    /// Opens the segment a click named, against the tree the click's own
    /// dismissal left rather than the one it was painted over.
    ///
    /// Withdrawing a minted pill can leave the query a single condition again,
    /// which dissolves the root's group and moves every path under it up one
    /// level. The pill the user aimed at is then one level shallower, and
    /// resolving the click as painted would open nothing.
    ///
    /// Only ever one level, and only for a withdrawal: `mint_term` canonicalises
    /// before snapshotting, so the tree the editor opened over differs from the
    /// snapshot by exactly one appended top-level node. Taking that node away
    /// either leaves the root's arity alone or collapses a one-child root into
    /// its child. A retarget's snapshot carries no `minted` path at all, so this
    /// never runs against one -- which is what makes the claim above hold for
    /// every snapshot it does run against.
    ///
    /// The retry is a path fix, not an identity test: a numeric path names a
    /// position rather than a pill, so a different pill can inherit the one that
    /// was clicked. `apply_command` refuses the withdrawn pill's own path before
    /// calling this, which is the case a mint creates. Other shapes, where a
    /// deeper sibling path stays legal after a restore, resolve onto a pill the
    /// user did not click, and are recorded as a follow-up.
    fn open_clicked_segment(
        &mut self,
        pill: &NodePath,
        role: SegmentRole,
        dismissed: &Dismissal,
        ui: &Ui,
        caret_id: egui::Id,
    ) {
        let mut target = select::segment_path(&self.expr, pill).map(|path| (pill.clone(), path));
        if target.is_none() && dismissed.withdrew.is_some() {
            target = pill.split_first().and_then(|(_, tail)| {
                let shifted = tail.to_vec();
                select::segment_path(&self.expr, &shifted).map(|path| (shifted, path))
            });
        }
        if let Some((pill, path)) = target {
            self.begin_segment_edit(pill, path, role, ui, caret_id);
        }
    }

    /// Opens one segment's editor because the user clicked that segment. The
    /// value it holds is one they chose, so dismissing the editor keeps it.
    fn begin_segment_edit(
        &mut self,
        pill: NodePath,
        path: NodePath,
        role: SegmentRole,
        ui: &Ui,
        caret_id: egui::Id,
    ) -> bool {
        self.begin_edit(pill, path, role, None, ui, caret_id)
    }

    /// Opens a value editor that a minted placeholder forced open, carrying what
    /// dismissing it has to hand back.
    fn begin_forced_value_edit(
        &mut self,
        pill: NodePath,
        path: NodePath,
        restore: Restore,
        ui: &Ui,
        caret_id: egui::Id,
    ) -> bool {
        self.begin_edit(pill, path, SegmentRole::Value, Some(restore), ui, caret_id)
    }

    /// Opens one segment's editor, reporting whether it opened. A value segment
    /// its term does not draw -- a nullary operator has no right-hand side --
    /// has nothing to edit and is refused rather than opened on nothing.
    ///
    /// Whatever editor was already open ends first, so a forced one hands its
    /// placeholder back rather than losing the snapshot to the new edit. The
    /// path is resolved after that, against the tree the restore left; a path
    /// that tree does not name opens nothing. `apply_command` is what keeps a
    /// click from landing there, by re-resolving a pill the restore only
    /// renumbered.
    fn begin_edit(
        &mut self,
        pill: NodePath,
        path: NodePath,
        role: SegmentRole,
        restore: Option<Restore>,
        ui: &Ui,
        caret_id: egui::Id,
    ) -> bool {
        self.end_segment_edit();
        let Some((field, op, value)) = select::term_at(&self.expr, &path).map(|(f, o, v)| (f, o, v.clone())) else {
            return false;
        };
        if role == SegmentRole::Value && op.is_nullary() {
            return false;
        }
        self.selection.clear();
        self.anchor = None;
        self.focus = None;
        self.value_options.clear();
        self.active_request = None;
        self.highlighted = None;
        self.history_cursor = None;
        self.pending_error = None;
        // A plain editor is the value itself, so the caret opens holding the
        // literal already on the term rather than making the user retype it.
        // Every other editor picks from rows, and the caret is only its needle.
        self.pending = match (role, suggest::value_editor(field)) {
            (SegmentRole::Value, ValueEditor::Plain) => print_value(&value),
            _ => String::new(),
        };
        self.parsed_text.clone_from(&self.pending);
        let opened_with = self.pending.clone();
        self.editing = Some(SegmentEdit::open(&self.expr, pill, path, role, restore, &opened_with));
        self.dropdown_open = true;
        let request = self.segment_request();
        self.refresh_request(request);
        park_caret(ui.ctx(), caret_id, self.pending.chars().count());
        true
    }

    /// Dismisses the open segment edit and discards the caret buffer it owned.
    /// A no-op when nothing is being edited, so it never throws away typed query
    /// text.
    ///
    /// An editor a minted placeholder forced open puts the tree back as it stood
    /// before the mint, reporting the change so the caller re-runs the query.
    /// The placeholder is in-kind and arbitrary, never a marker, so left behind
    /// it reads as a filter the user chose: `enemy.ship=0` at least renders as
    /// an unresolvable pill, but `outcome=win` is indistinguishable from a real
    /// choice and quietly narrows the search.
    fn end_segment_edit(&mut self) -> Dismissal {
        let Some(edit) = self.editing.take() else {
            return Dismissal::default();
        };
        self.clear_pending();
        let Some(restore) = edit.restore else {
            return Dismissal::default();
        };
        if self.expr == restore.before {
            return Dismissal::default();
        }
        self.expr = restore.before;
        Dismissal { restored: true, withdrew: restore.minted }
    }

    /// Commits the inline box on the way out, which is what a click elsewhere in
    /// the bar and a focus loss both are: the text in the box was typed on
    /// purpose, and the pointer moving on is not a reason to throw it away.
    /// Escape is the one dismissal that is not a commit.
    ///
    /// Only a plain box, whose caret holds a literal. An enum or lookup box
    /// holds a needle for its list, which is not a value and cannot be
    /// committed as one.
    ///
    /// A box a minted placeholder forced open commits only once its text has
    /// moved off that placeholder. Untouched, the snapshot it carries is right:
    /// the user picked a field, never chose a value, and the pill is withdrawn
    /// rather than left as a filter nobody set. Typed into, the snapshot is
    /// wrong, because the literal in the box is a choice -- and Enter already
    /// commits it, so refusing here would make the same keystrokes mean two
    /// different things depending on how the user left the box.
    fn commit_open_box(&mut self) -> bool {
        let Some((edit, inline)) = self.inline_edit() else {
            return false;
        };
        let untouched = edit.restore.is_some() && self.pending == inline.opened_with;
        if untouched || self.plain_value_field().is_none() {
            return false;
        }
        let literal = self.pending.clone();
        self.commit_value_literal(&literal)
    }

    /// Ends the open segment edit keeping what it committed, so a value the user
    /// did choose is not rolled back by the snapshot a forced editor carries.
    fn commit_segment_edit(&mut self) {
        if let Some(edit) = self.editing.as_mut() {
            edit.restore = None;
        }
        self.end_segment_edit();
    }

    fn commit_row(&mut self, row: Row, ui: &Ui, caret_id: egui::Id) -> bool {
        self.highlighted = None;
        match row {
            Row::Field(option) => self.commit_field_change(option.field, ui, caret_id),
            Row::Operator(option) => self.commit_operator(option.op),
            Row::Literal(option) => self.commit_value_literal(&option.token),
            Row::Value(index) => {
                let Some(token) = self.value_options.get(index).map(|option| option.token.clone()) else {
                    return false;
                };
                if self.editing.is_some() {
                    return self.commit_value_literal(&token);
                }
                self.commit_caret_value(&token, ui, caret_id)
            }
            Row::Suggestion(index) => {
                let Some(kind) = self.suggestions.get(index).map(|s| s.kind.clone()) else {
                    return false;
                };
                self.commit_suggestion(kind, ui, caret_id)
            }
        }
    }

    /// Retargets the pill being edited at another field.
    ///
    /// `set_field` reports that the path addressed a field term, not that
    /// anything changed, so something else has to say whether the old value
    /// survived. That something is the value itself, before and after: a value
    /// the new field cannot carry is replaced by an arbitrary in-kind
    /// placeholder, which is not a choice the user made, so the value editor
    /// opens on it here rather than leaving it to be committed unseen -- for
    /// `Ship` and `Account` it is a zero id that renders as an unresolvable
    /// pill.
    ///
    /// Comparing the two fields' declared `value_kind`s instead would miss the
    /// case that has nothing to do with kinds: a nullary operator carries
    /// `Value::NoOperand`, which belongs to no kind at all, so retargeting
    /// `realm is set` at `name` -- both `Text` -- still mints `Text("")` and
    /// still needs the editor.
    fn commit_field_change(&mut self, field: TermField, ui: &Ui, caret_id: egui::Id) -> bool {
        let Some(edit) = self.editing.clone() else {
            return false;
        };
        let Some((_, _, current)) = select::term_at(&self.expr, &edit.path) else {
            return false;
        };
        let previous = current.clone();
        let before = self.expr.clone();
        if !select::set_field(&mut self.expr, &edit.path, field) {
            return false;
        }
        let kept = select::term_at(&self.expr, &edit.path).is_some_and(|(_, _, value)| *value == previous);
        // No `minted` path: a retarget replaces a value on a pill that already
        // exists, so the restore puts a term back rather than taking one away and
        // no path moves.
        let restore = Restore { before, minted: None };
        if !kept && self.begin_forced_value_edit(edit.pill, edit.path, restore, ui, caret_id) {
            return true;
        }
        self.commit_segment_edit();
        self.dropdown_open = true;
        park_caret(ui.ctx(), caret_id, 0);
        true
    }

    fn commit_operator(&mut self, op: Op) -> bool {
        let Some(edit) = self.editing.clone() else {
            return false;
        };
        if !select::set_op(&mut self.expr, &edit.path, op) {
            return false;
        }
        self.commit_segment_edit();
        true
    }

    /// Applies a grammar literal to the pill's value, whether it came from a
    /// row or from the caret. The literal goes through the grammar's own parser
    /// rather than a second reader of the same spellings, so a clicked value
    /// and a typed one become the same `Value`.
    ///
    /// A literal the field cannot read is reported the way the typed path
    /// reports one, through `pending_error` and the underline it drives, and
    /// leaves the box open over the value it could not replace.
    /// `reparse_pending` clears it on the next keystroke, so the mark follows
    /// the text rather than outlasting it.
    ///
    /// The write goes through the leaf the box opened over rather than through
    /// the path it was opened at, so a `canonicalise` between the two either
    /// leaves that leaf where it was or moves something else under the path and
    /// the write refuses. A refusal closes the box: there is nowhere left for the
    /// value to land, so leaving it open would leave text with no destination.
    fn commit_value_literal(&mut self, literal: &str) -> bool {
        let Some(edit) = self.editing.clone() else {
            return false;
        };
        let Some((field, _, _)) = select::term_at(&self.expr, &edit.path) else {
            return false;
        };
        let value = match parse_roster_value(field.value_kind(), literal.trim()) {
            Some(value) => value,
            None => {
                self.report_bad_value(field, literal);
                return false;
            }
        };
        let Some(inline) = edit.inline else {
            return false;
        };
        // A box the user left spelled the way they found it changed nothing, and
        // a reported change re-runs the search, rewrites the saved query, and
        // clears whatever the same click selected.
        let same = select::term_at(&self.expr, &edit.path).is_some_and(|(_, _, current)| *current == value);
        if !select::commit_value(&mut self.expr, &inline.leaf, &inline.seed, &inline.tail, value) {
            return self.end_segment_edit().restored;
        }
        self.commit_segment_edit();
        !same
    }

    /// Marks a literal the field cannot read, spanning that literal rather than
    /// whatever the caret happens to hold.
    ///
    /// A literal picked from a row is not the caret's text, so it is moved there
    /// first: the underline is drawn over the caret's galley and is suppressed
    /// outright while the caret is empty, so reporting without moving it would
    /// leave the refusal invisible -- the silent failure this reporting exists
    /// to remove. `parsed_text` moves with it so the next keystroke clears the
    /// mark, rather than the next frame's `reparse_pending` erasing it before it
    /// is ever seen.
    fn report_bad_value(&mut self, field: TermField, literal: &str) {
        if self.pending != literal {
            self.pending = literal.to_owned();
        }
        self.parsed_text.clone_from(&self.pending);
        self.pending_error = Some(bad_value(field, literal));
    }

    /// Enter with nothing highlighted. With no segment open that commits the
    /// caret's query text; with one open it takes the list's best row, which is
    /// what a narrowed list is for, and an empty list has nothing to take.
    fn commit_typed(&mut self, rows: &[Row], ui: &Ui, caret_id: egui::Id) -> bool {
        if self.editing.is_none() {
            return self.commit_pending(ui, caret_id);
        }
        if self.plain_value_field().is_some() {
            let literal = self.pending.clone();
            return self.commit_value_literal(&literal);
        }
        let Some(row) = self.default_row(rows) else {
            return self.end_segment_edit().restored;
        };
        self.commit_row(row, ui, caret_id)
    }

    fn commit_suggestion(&mut self, kind: SuggestionKind, ui: &Ui, caret_id: egui::Id) -> bool {
        match kind {
            SuggestionKind::Preset(key) => {
                let Some(preset) = PRESETS.iter().find(|preset| preset.key == key) else {
                    return false;
                };
                select::append_query(&mut self.expr, (preset.build)());
                self.clear_pending();
                // A preset finishes a term, so the full list comes back for the
                // next one, and the caret parks at the end of a now-empty
                // buffer rather than keeping an offset into the text it had.
                self.dropdown_open = true;
                park_caret(ui.ctx(), caret_id, 0);
                true
            }
            SuggestionKind::MatchField(field) => self.mint_term(TermField::Match(field), None, ui, caret_id),
            SuggestionKind::RosterField { field, scope } => {
                // A roster field name is only reachable through a scope prefix
                // or a quantifier, so an unscoped one takes the widest scope
                // rather than building a term the grammar cannot read back.
                self.mint_term(TermField::Roster(field), Some(scope.unwrap_or(Scope::Anyone)), ui, caret_id)
            }
        }
    }

    /// Appends a term for a field picked from the caret's list and opens its
    /// value editor, so a picked row builds a pill rather than leaving grammar
    /// text in the caret for Enter to finish.
    ///
    /// The term arrives carrying `select::new_term`'s placeholder, which is an
    /// arbitrary in-kind value rather than one the user chose, so the editor has
    /// to open on it for the same reason `commit_field_change` opens one.
    ///
    /// Whole terms typed ahead of the fragment the row was picked against are
    /// committed rather than discarded. A head that does not parse cannot become
    /// a term, and the editor that opens next takes the caret, the way opening
    /// any other segment editor does.
    fn mint_term(&mut self, field: TermField, scope: Option<Scope>, ui: &Ui, caret_id: egui::Id) -> bool {
        let Some(node) = select::new_term(field, scope) else {
            return false;
        };
        self.pending = suggest::replace_active_fragment(&self.pending, "");
        self.parsed_text.clear();
        let typed = self.commit_pending(ui, caret_id);

        // Snapshotted after the caret's own text is committed, so dismissing the
        // editor withdraws the placeholder alone and not the terms the user
        // typed before picking the row. Canonical when taken, because writing it
        // back is an assignment rather than an edit.
        select::canonicalise(&mut self.expr);
        let before = self.expr.clone();
        select::append_top_level(&mut self.expr, node);
        select::canonicalise(&mut self.expr);
        let path = select::last_appended_path(&self.expr);
        let restore = Restore { before: before.clone(), minted: Some(path.clone()) };
        let opened = select::segment_path(&self.expr, &path)
            .is_some_and(|segment| self.begin_forced_value_edit(path.clone(), segment, restore, ui, caret_id));
        if opened {
            return true;
        }
        // No editor means nothing carries a snapshot, so nothing would ever
        // withdraw the placeholder and it would rest in the query as a value
        // nobody chose. Every field the caret offers does open one, so this
        // guards a field whose leading allowed operator is nullary rather than a
        // path anything reaches today.
        self.expr = before;
        self.clear_pending();
        self.dropdown_open = true;
        park_caret(ui.ctx(), caret_id, 0);
        typed
    }

    /// Applies a value row picked from the caret's own list. The row finishes
    /// the term the caret was typing, so that term is committed here rather than
    /// left as text waiting for Enter; text that does not parse stays in the
    /// caret with its span marked, which is what Enter leaves too.
    fn commit_caret_value(&mut self, literal: &str, ui: &Ui, caret_id: egui::Id) -> bool {
        let (text, cursor) = value_completed_text(&self.pending, literal);
        self.pending = text;
        self.parsed_text.clear();
        self.dropdown_open = true;
        self.highlighted = None;
        park_caret(ui.ctx(), caret_id, cursor);
        self.commit_pending(ui, caret_id)
    }

    /// Parses the caret's text and adds it to the tree. Blank text commits
    /// nothing; text that does not parse stays put with its span underlined.
    fn commit_pending(&mut self, ui: &Ui, caret_id: egui::Id) -> bool {
        if self.pending.trim().is_empty() {
            return false;
        }
        match parse_query(&self.pending) {
            Ok(parsed) if !parsed.is_empty_all() => {
                select::append_query(&mut self.expr, parsed);
                self.clear_pending();
                // Adding two filters in a row is the common case, so a
                // committed term leaves the full list showing.
                self.dropdown_open = true;
                park_caret(ui.ctx(), caret_id, 0);
                true
            }
            Ok(_) => false,
            Err(error) => {
                self.pending_error = Some(error);
                false
            }
        }
    }

    fn clear_pending(&mut self) {
        self.pending.clear();
        self.parsed_text.clear();
        self.pending_error = None;
        self.value_options.clear();
        self.active_request = None;
        self.editing = None;
        self.dropdown_open = false;
        self.highlighted = None;
        self.history_cursor = None;
    }

    /// Step 11. Canonicalising after every edit is what keeps a one-condition
    /// group from printing as a bare term and reparsing as a leaf, which would
    /// lose the group across save and reload.
    fn finish_edit(&mut self) {
        select::canonicalise(&mut self.expr);
        self.selection.clear();
        self.anchor = None;
        self.focus = None;
        // Canonicalising can dissolve the node an open segment edit addresses,
        // which would leave the editor pointing at a term that is gone.
        if self.editing.as_ref().is_some_and(|edit| select::term_at(&self.expr, &edit.path).is_none()) {
            self.end_segment_edit();
        }
    }

    /// Re-runs the grammar over the caret's text, but only when it changed:
    /// parsing is per keystroke, not per frame.
    fn reparse_pending(&mut self) {
        if self.pending == self.parsed_text {
            return;
        }
        self.parsed_text.clone_from(&self.pending);
        self.history_cursor = None;

        if self.editing.is_some() {
            // The caret is the open segment's own buffer here -- a needle for
            // its list, or a bare literal -- so the query grammar has nothing
            // to say about it and no span to underline.
            self.pending_error = None;
            let request = self.segment_request();
            self.refresh_request(request);
            return;
        }
        self.pending_error = parse_query(&self.pending).err();
        self.refresh_request(value_request_for(&self.pending));
    }

    /// The lookup the open value segment calls for, read off the field rather
    /// than off caret text.
    fn segment_request(&self) -> Option<ValueRequest> {
        let edit = self.editing.as_ref()?;
        if edit.role != SegmentRole::Value {
            return None;
        }
        let (field, _, _) = select::term_at(&self.expr, &edit.path)?;
        suggest::segment_value_request(field, self.pending.trim())
    }

    fn refresh_request(&mut self, request: Option<ValueRequest>) {
        // A new needle for the same source keeps the rows already on screen
        // until the caller answers, so the list does not blink on every key.
        if request_source(request.as_ref()) != request_source(self.active_request.as_ref()) {
            self.value_options.clear();
        }
        if request != self.active_request {
            self.active_request.clone_from(&request);
            self.pending_request = request;
        }
    }

    /// The caret's suggestion list is rebuilt only when the locale moves under
    /// it; its labels are translated and there are enough of them that
    /// rebuilding per frame would be wasted work.
    fn refresh_suggestions(&mut self) {
        let locale = rust_i18n::locale();
        if self.suggestions.is_empty() || self.suggestions_locale != *locale {
            self.suggestions_locale = locale.to_string();
            self.suggestions = filter_options();
        }
    }
}

/// The caret text a picked value row produces, and the character index the
/// caret parks at when that text stays there.
///
/// The literal replaces the half-typed value the row was picked against;
/// appending would give `enemy.ship:yamYamato`. The trailing space closes the
/// term, which leaves `active_fragment` empty so the full list returns and makes
/// `value_request_for` yield `None` so no stale lookup fires.
///
/// A character index, never a byte offset: `CCursor::index` is a `CharIndex`,
/// and the text counted here is arbitrary user input that a byte length would
/// misplace the caret in the moment it contains anything outside ASCII.
fn value_completed_text(pending: &str, literal: &str) -> (String, usize) {
    let text = format!("{} ", suggest::replace_active_value(pending, literal));
    let cursor = text.chars().count();
    (text, cursor)
}

/// The error a value the field cannot read reports, spanning `literal` so the
/// underline covers exactly what failed. Built from the grammar's own error
/// type rather than a second one, so the value editor and the typed path
/// underline through one mechanism.
///
/// A byte range, which is what `QueryParseError::span` is everywhere else and
/// what `paint::char_index` converts on the way to the galley.
fn bad_value(field: TermField, literal: &str) -> QueryParseError {
    let name = match field {
        TermField::Match(f) => f.name(),
        TermField::Roster(f) => f.name(),
    };
    QueryParseError {
        span: 0..literal.len(),
        kind: ParseErrorKind::BadValue {
            field: name,
            allowed: query_text::enumerable_roster_values(field.value_kind()),
        },
    }
}

/// The cell the caret is laid out in, found by kind rather than by position:
/// `move_caret_to` puts it in the slot of a pill whose value is being typed, so
/// it is only the last token while nothing is.
///
/// One derivation, read both by the widget that fills the cell and by the
/// hit test that asks whether a click landed in it: the `TextEdit` sizes
/// itself to its content and can be shorter than the cell, so the two are not
/// interchangeable.
fn caret_rect(tokens: &[Token], laid: &LaidOut, origin: egui::Vec2) -> Rect {
    laid.placed[caret_placement(tokens, laid)].rect.translate(origin).shrink2(egui::vec2(0.0, ROW_PAD_Y))
}

/// The token index of the caret, so its measured runs can be read back out of
/// the per-token galleys.
fn caret_index(tokens: &[Token], laid: &LaidOut) -> usize {
    laid.placed[caret_placement(tokens, laid)].index
}

/// Where in `laid.placed` the caret sits. By kind, for the reason `caret_rect`
/// gives.
fn caret_placement(tokens: &[Token], laid: &LaidOut) -> usize {
    laid.placed
        .iter()
        .position(|placed| tokens.get(placed.index).is_some_and(|token| matches!(token.kind, TokenKind::Caret)))
        .expect("lay_out places every token, and exactly one of them is always the caret")
}

/// The index of the segment an inline box occupies in the pill it stands in.
/// `None` for a pill that draws no value segment, which is a term whose operator
/// is nullary and has nothing to type into.
fn boxed_segment(pill: &Token) -> Option<usize> {
    let TokenKind::Pill { segments } = &pill.kind else {
        return None;
    };
    segments.iter().position(|segment| segment.role == SegmentRole::Value)
}

/// The runs a caret standing inside a pill is measured and painted from: that
/// pill's own segments, with the boxed one measured from the text being typed
/// into it rather than from the value already on the term.
fn inline_galleys(ui: &Ui, pill: &Token, font: &egui::FontId, boxed: usize, text: &str) -> Vec<Arc<Galley>> {
    let TokenKind::Pill { segments } = &pill.kind else {
        return Vec::new();
    };
    let lay = |run: &str| ui.painter().layout_no_wrap(run.to_owned(), font.clone(), egui::Color32::PLACEHOLDER);
    segments
        .iter()
        .enumerate()
        .map(|(i, segment)| {
            if i != boxed {
                return lay(&segment.text);
            }
            // Only this run's width is ever read -- `paint::inline_pill` leaves
            // the boxed segment to the `TextEdit` and never draws it -- and it
            // takes the wider of what is being typed and what the segment
            // already read as, so opening the box moves nothing after it while
            // it still grows with the text.
            let typed = lay(text);
            let was = lay(&segment.text);
            if typed.size().x >= was.size().x { typed } else { was }
        })
        .collect()
}

/// Parks the caret at `cursor`, a character index. egui otherwise keeps
/// whatever offset the widget had, which after a programmatic replacement of
/// the text is arbitrary.
fn park_caret(ctx: &egui::Context, caret_id: egui::Id, cursor: usize) {
    let mut state = TextEditState::load(ctx, caret_id).unwrap_or_default();
    state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(cursor))));
    state.store(ctx, caret_id);
}

/// The right-click menu for one pill: the whole-term edits, which have no
/// segment of their own to be clicked on.
///
/// A pill inside a roster predicate gets nothing: negate and delete both
/// address a match-level node its path does not name. Its field, operator, and
/// value are all reachable through its segments.
fn pill_menu(ui: &mut Ui, expr: &MatchExpr, path: &NodePath, commands: &mut Vec<Command>) {
    if !select::addresses_match_node(expr, path) {
        return;
    }
    if ui.button(t!("ui.search.bar.negate").into_owned()).clicked() {
        commands.push(Command::Negate(path.clone()));
        ui.close();
    }
    if ui.button(t!("ui.search.bar.delete").into_owned()).clicked() {
        commands.push(Command::Delete(path.clone()));
        ui.close();
    }
}

/// The breadcrumb a suggestion's category reads as. Matched exhaustively so a
/// new category cannot render untranslated.
fn category_label(category: SuggestionCategory) -> String {
    match category {
        SuggestionCategory::Preset => t!("ui.search.bar.context_preset"),
        SuggestionCategory::Match => t!("ui.search.bar.context_match"),
        SuggestionCategory::Roster => t!("ui.search.bar.context_roster"),
    }
    .into_owned()
}

/// Which table a request reads from, ignoring its needle, so a keystroke that
/// only narrows the search does not throw away the rows already fetched.
fn request_source(request: Option<&ValueRequest>) -> Option<std::mem::Discriminant<ValueRequest>> {
    request.map(std::mem::discriminant)
}

/// One measured run per segment for a `Pill`, one run for every other kind.
/// `paint::token` needs a galley per segment so it can lay each one out with
/// `paint::segment_rects`; nothing else here decides where a segment goes.
///
/// A single match over `token.kind`, rather than a `Pill` special case
/// falling back to a separate whole-token-text function: the old split left a
/// `Pill` arm in that other function that duplicated `label::join_segments`
/// byte for byte while never actually running, since `token_galleys` always
/// intercepted `Pill` first. One match makes that shape impossible to
/// reintroduce rather than merely unused.
fn token_galleys(ui: &Ui, token: &Token, font: &egui::FontId) -> Vec<Arc<Galley>> {
    let run = |text: String| vec![ui.painter().layout_no_wrap(text, font.clone(), egui::Color32::PLACEHOLDER)];
    match &token.kind {
        TokenKind::Pill { segments } => segments
            .iter()
            .map(|segment| ui.painter().layout_no_wrap(segment.text.clone(), font.clone(), egui::Color32::PLACEHOLDER))
            .collect(),
        TokenKind::Connector { is_or } => {
            run(if *is_or { t!("ui.search.bar.or_word") } else { t!("ui.search.bar.and_word") }.into_owned())
        }
        TokenKind::NotPrefix => run(t!("ui.search.bar.not_word").into_owned()),
        TokenKind::GroupOpen { .. } => run("(".to_owned()),
        // The closing bracket of an editable group is its dissolve control, so
        // it reads as a close button rather than as punctuation.
        TokenKind::GroupClose => run(crate::icons::X.to_owned()),
        TokenKind::QuantClose => run(")".to_owned()),
        TokenKind::QuantOpen { prefix } => run(format!("{prefix} (")),
        // The trailing caret is measured empty: `lay_out` gives it the rest of
        // its row whatever it holds. A caret standing inside a pill is measured
        // by `inline_galleys` instead.
        TokenKind::Caret => run(String::new()),
    }
}

fn token_width(token: &Token, galleys: &[Arc<Galley>], cfg: &LayoutCfg) -> f32 {
    match &token.kind {
        // A caret standing in for a pill is as wide as the term it holds;
        // `MIN_CARET_WIDTH` is the floor, and the whole width of one holding
        // nothing.
        TokenKind::Caret => galleys.first().map_or(0.0, |g| g.size().x + 2.0 * paint::PAD_X).max(MIN_CARET_WIDTH),
        // A pill's own width is its segments widened and gapped the same way
        // `paint::segment_rects` will place them, so the rect `lay_out` hands
        // back is exactly wide enough for every segment to land inside it.
        TokenKind::Pill { .. } => layout::pill_width(&paint::segment_widths(galleys), cfg),
        // `token_galleys` always hands back exactly one run for these kinds;
        // `first()` degrades to zero width instead of trusting that
        // invariant with an index that would panic if it were ever violated.
        TokenKind::Connector { .. }
        | TokenKind::NotPrefix
        | TokenKind::GroupOpen { .. }
        | TokenKind::GroupClose
        | TokenKind::QuantOpen { .. }
        | TokenKind::QuantClose => galleys.first().map_or(0.0, |g| g.size().x + 2.0 * paint::PAD_X),
    }
}

#[cfg(test)]
mod tests {
    use wows_replays::types::GameParamId;

    use super::*;
    use crate::db::index::query_ast::Expr;
    use crate::db::index::query_ast::MatchField;
    use crate::db::index::query_ast::MatchTerm;
    use crate::db::index::query_ast::Quant;
    use crate::db::index::query_ast::RosterField;
    use crate::db::index::query_ast::RosterTerm;
    use crate::db::index::query_ast::Value;
    use crate::db::index::query_text::ZoneGuard;
    use crate::db::index::rows::MatchOutcome;

    fn win() -> MatchExpr {
        Expr::Leaf(MatchTerm::Field(MatchField::Outcome, Op::Is, Value::Outcome(MatchOutcome::Win)))
    }

    /// A bar with one pill and one of its segments open, which is every state
    /// the segment routing decides anything in.
    fn bar_editing(expr: MatchExpr, role: SegmentRole) -> QueryBar {
        let mut bar = QueryBar::default();
        bar.set_expr(expr);
        let path = select::segment_path(&bar.expr, &vec![]).expect("the fixture pill addresses a term");
        bar.editing = Some(SegmentEdit::open(&bar.expr, vec![], path, role, None, ""));
        bar
    }

    fn roster_pill(field: RosterField, op: Op, value: Value) -> MatchExpr {
        Expr::Leaf(MatchTerm::Roster { quant: Quant::Any, pred: Expr::Leaf(RosterTerm { field, op, value }) })
    }

    /// Pins the zone the timestamp printer resolves against, so a fixture built
    /// on the epoch spells itself the same way on every machine.
    /// `print_timestamp` writes a bare date only for an instant that is local
    /// midnight, which the epoch is in UTC alone; west of it the same instant is
    /// the previous day and prints as a full RFC 3339 instant instead. The
    /// local-day behaviour itself is covered where the printer lives, against
    /// `America/New_York`.
    #[must_use]
    fn pinned_utc() -> ZoneGuard {
        ZoneGuard::set(jiff::tz::TimeZone::UTC)
    }

    /// A bar in the state the caret is in with nothing committed. The
    /// suggestion list is what `show` refreshes every frame and a default bar
    /// has not run one, so its rows would otherwise be empty.
    fn caret_bar() -> QueryBar {
        let mut bar = QueryBar::default();
        bar.refresh_suggestions();
        bar
    }

    /// The field a caret row names, or `None` for a preset row.
    fn row_field(bar: &QueryBar, row: &Row) -> Option<TermField> {
        let Row::Suggestion(index) = row else {
            return None;
        };
        match bar.suggestions.get(*index)?.kind {
            SuggestionKind::MatchField(field) => Some(TermField::Match(field)),
            SuggestionKind::RosterField { field, .. } => Some(TermField::Roster(field)),
            SuggestionKind::Preset(_) => None,
        }
    }

    /// The first caret row that names a field rather than a preset, with the
    /// field it names.
    fn first_field_row(bar: &QueryBar) -> (usize, TermField) {
        bar.dropdown_rows()
            .iter()
            .enumerate()
            .find_map(|(index, row)| row_field(bar, row).map(|field| (index, field)))
            .expect("the caret offers a field row")
    }

    /// The caret row that offers `field`.
    fn field_row(bar: &QueryBar, field: TermField) -> Row {
        bar.dropdown_rows()
            .into_iter()
            .find(|row| row_field(bar, row) == Some(field))
            .unwrap_or_else(|| panic!("{field:?} is not offered"))
    }

    /// The text one pill renders, joined the way `label::pill_text` joins it.
    fn pill_text_at(bar: &QueryBar, path: &NodePath) -> String {
        tokenize(&bar.expr, &bar.names)
            .into_iter()
            .find_map(|token| match token.kind {
                TokenKind::Pill { segments } if token.path == *path => {
                    Some(segments.iter().map(|segment| segment.text.as_str()).collect::<Vec<_>>().join(" "))
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("no pill at {path:?} in {:?}", bar.expr))
    }

    /// The caret part way through a ship filter, holding the rows the Search
    /// tab answered its lookup with. Hands back the row for `ship`.
    ///
    /// The caret text is the field's own grammar prefix, so this is the state a
    /// picked field suggestion used to leave behind rather than a hand-built one.
    fn seed_ship_value_editor(bar: &mut QueryBar, ship: GameParamId) -> Row {
        bar.pending = suggest::roster_field_prefix(RosterField::Ship, Scope::Enemy).expect("ship has a grammar prefix");
        bar.parsed_text.clone_from(&bar.pending);
        assert_eq!(
            value_request_for(&bar.pending),
            Some(ValueRequest::Ships { needle: String::new() }),
            "the fixture must be a caret that really would have fetched ships"
        );
        bar.active_request = value_request_for(&bar.pending);
        bar.value_options = vec![ValueOption { label: "Yamato".to_owned(), token: ship.raw().to_string() }];
        let rows = bar.dropdown_rows();
        assert_eq!(rows, vec![Row::Value(0)], "the caret must be offering the fetched ships");
        rows[0].clone()
    }

    /// Runs `f` against a real `Ui` and the caret id derived from it, for the
    /// routing that stores caret state through `TextEditState` and so cannot be
    /// driven without a `Context`.
    fn with_ui<R>(f: impl FnOnce(&mut Ui, egui::Id) -> R) -> R {
        let mut once = Some(f);
        let mut out = None;
        egui::__run_test_ui(|ui| {
            if let Some(f) = once.take() {
                let caret_id = ui.id().with("query_bar").with("caret");
                out = Some(f(ui, caret_id));
            }
        });
        out.expect("the test ui runs its contents exactly once")
    }

    #[test]
    fn a_value_completion_closes_the_term_and_puts_the_caret_after_the_space() {
        let (text, cursor) = value_completed_text("outcome:wi", "win");
        assert_eq!(text, "outcome:win ");
        assert_eq!(cursor, text.chars().count());
        assert!(suggest::active_fragment(&text).is_empty(), "the next fragment must start empty: {text:?}");
        assert!(value_request_for(&text).is_none(), "a closed term must not leave a value lookup pending");
    }

    /// Multi-byte text ahead of the fragment being completed. `CCursor::index`
    /// is a character index, so a caret derived from `str::len` overshoots the
    /// end of the string here.
    #[test]
    fn a_completion_over_multi_byte_text_places_the_caret_by_character() {
        let (text, cursor) = value_completed_text("map:caf\u{e9} outcome:wi", "win");
        assert_eq!(text, "map:caf\u{e9} outcome:win ");
        assert_eq!(cursor, text.chars().count());
        assert!(cursor < text.len(), "a byte offset would overshoot: {cursor} against {} bytes", text.len());
    }

    /// The inserted literal is itself multi-byte, so a caret counted in bytes
    /// overshoots even when everything before it is ASCII.
    #[test]
    fn a_multi_byte_completion_value_places_the_caret_by_character() {
        let (text, cursor) = value_completed_text("map:oc", "\"nord\u{e9}\"");
        assert_eq!(text, "map:\"nord\u{e9}\" ");
        assert_eq!(cursor, text.chars().count());
        assert!(cursor < text.len(), "a byte offset would overshoot: {cursor} against {} bytes", text.len());
    }

    /// The typing anchor's byte-to-character step, which is the same conversion
    /// the error underline needs and the one place this path could slice a
    /// string mid-character.
    #[test]
    fn the_typing_anchor_is_the_active_fragments_start_counted_in_characters() {
        let plain = "outcome:win aa\u{e9}b";
        let start = suggest::active_fragment_start(plain);
        assert_eq!(&plain[start..], "aa\u{e9}b");
        assert_eq!(paint::char_index(plain, start), 12);

        // A multi-byte fragment head moves the two apart, which is what makes
        // the conversion load-bearing rather than an identity.
        let accented = "map:caf\u{e9} aa\u{e9}b";
        let start = suggest::active_fragment_start(accented);
        assert_eq!(start, 10, "byte offset");
        assert_eq!(paint::char_index(accented, start), 9, "character index");
    }

    /// The routing itself: a clicked segment's role decides which source the
    /// dropdown reads, and nothing else does.
    #[test]
    fn each_segment_role_reads_its_own_source() {
        let bar = bar_editing(win(), SegmentRole::Filter);
        let fields: Vec<TermField> = bar
            .dropdown_rows()
            .into_iter()
            .map(|row| match row {
                Row::Field(option) => option.field,
                other => panic!("the filter segment offered {other:?}"),
            })
            .collect();
        assert_eq!(
            fields,
            field_options(TermField::Match(MatchField::Outcome)).into_iter().map(|o| o.field).collect::<Vec<_>>()
        );

        let bar = bar_editing(win(), SegmentRole::Operator);
        assert!(bar.dropdown_rows().iter().all(|row| matches!(row, Row::Operator(_))), "{:?}", bar.dropdown_rows());

        // Outcome is an enum kind, so its values come from the local source
        // with no database round trip.
        let bar = bar_editing(win(), SegmentRole::Value);
        let literals: Vec<String> = bar
            .dropdown_rows()
            .into_iter()
            .map(|row| match row {
                Row::Literal(option) => option.token,
                other => panic!("an enum value segment offered {other:?}"),
            })
            .collect();
        assert_eq!(literals, vec!["win", "loss", "draw", "unknown"]);
    }

    /// A field whose values live in the index offers the rows the Search tab
    /// fetched, and asks for them: the request has to be made from the field,
    /// since the caret holds no text naming it.
    #[test]
    fn a_lookup_value_segment_offers_fetched_rows_and_asks_for_them() {
        let bar = bar_editing(roster_pill(RosterField::Ship, Op::Is, Value::Ship(1u64.into())), SegmentRole::Value);
        assert_eq!(bar.segment_request(), Some(ValueRequest::Ships { needle: String::new() }));
        assert!(bar.dropdown_rows().is_empty(), "nothing fetched yet");

        let mut bar = bar;
        bar.value_options = vec![ValueOption { label: "Yamato".into(), token: "1".into() }];
        assert_eq!(bar.dropdown_rows(), vec![Row::Value(0)]);
    }

    /// A number has no closed set of values, so its segment offers no rows at
    /// all and the caret becomes plain entry for the literal.
    #[test]
    fn a_plain_value_segment_offers_no_rows_and_names_its_field() {
        let bar = bar_editing(roster_pill(RosterField::Damage, Op::Ge, Value::Int(50_000)), SegmentRole::Value);
        assert!(bar.dropdown_rows().is_empty(), "{:?}", bar.dropdown_rows());
        assert_eq!(bar.plain_value_field(), Some(TermField::Roster(RosterField::Damage)));
        assert_eq!(bar.segment_request(), None, "plain entry needs no lookup");
    }

    /// The invariant the whole plan exists to make structural: `Op` spells
    /// equality three ways, all printing the same token, so an operator outside
    /// the field's own set builds a tree that reparses into a different one.
    /// Task 3 sourced the list from `allowed_ops`; this pins that the routing
    /// did not reintroduce a second list on the way to the dropdown.
    #[test]
    fn no_operator_reachable_by_clicking_is_outside_its_fields_allowed_ops() {
        // The operator source reads the field and nothing else, so the term's
        // value is irrelevant here and is the same placeholder throughout.
        for field in MatchField::ALL {
            let expr = Expr::Leaf(MatchTerm::Field(field, field.allowed_ops()[0], Value::Int(0)));
            let bar = bar_editing(expr, SegmentRole::Operator);
            let offered: Vec<Op> = bar
                .dropdown_rows()
                .into_iter()
                .map(|row| match row {
                    Row::Operator(option) => option.op,
                    other => panic!("got {other:?}"),
                })
                .collect();
            assert_eq!(offered, field.allowed_ops().to_vec(), "{} offered the wrong operators", field.name());
        }
        for field in RosterField::ALL {
            let expr = roster_pill(field, field.allowed_ops()[0], Value::Int(0));
            let bar = bar_editing(expr, SegmentRole::Operator);
            let offered: Vec<Op> = bar
                .dropdown_rows()
                .into_iter()
                .map(|row| match row {
                    Row::Operator(option) => option.op,
                    other => panic!("got {other:?}"),
                })
                .collect();
            assert_eq!(offered, field.allowed_ops().to_vec(), "{} offered the wrong operators", field.name());
        }
    }

    /// The obligation `select::placeholder_value` names: a field change that
    /// loses the old value replaces it with an arbitrary in-kind placeholder,
    /// which for `Ship` and `Account` is a zero id that renders as an
    /// unresolvable pill. The editor has to open on it, so the placeholder is
    /// never something the user can commit without seeing.
    ///
    /// Driven through `commit_field_change` itself, not through `set_field`
    /// with the editor state set by hand: the decision under test lives in that
    /// function and a fixture that sets `editing` for it cannot exercise it.
    ///
    /// The last three pairs are the ones a declared-kind comparison misses. Both
    /// fields have the same `ValueKind` in each, and the value is replaced
    /// anyway, because the old operator was nullary and `Value::NoOperand`
    /// belongs to no kind at all.
    #[test]
    fn a_field_change_that_replaces_the_value_opens_the_value_editor() {
        let cases = [
            (RosterField::Tier, Op::Eq, Value::Int(10), RosterField::Ship),
            (RosterField::Realm, Op::IsSet, Value::NoOperand, RosterField::Name),
            (RosterField::Damage, Op::IsSet, Value::NoOperand, RosterField::Tier),
            (RosterField::Survived, Op::IsSet, Value::NoOperand, RosterField::TestShip),
        ];
        for (from, op, value, to) in cases {
            let mut bar = bar_editing(roster_pill(from, op, value.clone()), SegmentRole::Filter);
            let committed = with_ui(|ui, caret_id| bar.commit_field_change(TermField::Roster(to), ui, caret_id));
            assert!(committed, "{} -> {} must report the retarget", from.name(), to.name());

            let (landed_field, _, landed) = select::term_at(&bar.expr, &[]).expect("the retargeted term");
            assert_eq!(landed_field, TermField::Roster(to));
            assert_ne!(*landed, value, "{} -> {} must actually lose its value", from.name(), to.name());
            assert_eq!(
                bar.editing.as_ref().map(|edit| edit.role),
                Some(SegmentRole::Value),
                "{} -> {} committed a value the user never chose",
                from.name(),
                to.name()
            );
        }
    }

    /// The `Ship` case's whole point: the editor that opens is the one that can
    /// resolve the zero id into a real ship.
    #[test]
    fn the_forced_editor_is_the_one_that_can_replace_the_placeholder() {
        let mut bar = bar_editing(roster_pill(RosterField::Tier, Op::Eq, Value::Int(10)), SegmentRole::Filter);
        assert!(with_ui(|ui, caret_id| bar.commit_field_change(TermField::Roster(RosterField::Ship), ui, caret_id)));
        assert_eq!(bar.segment_request(), Some(ValueRequest::Ships { needle: String::new() }));
    }

    /// A field change the value survives has nothing to re-choose, so the value
    /// editor must not be forced open on a value the user still has.
    #[test]
    fn a_field_change_the_value_survives_leaves_the_editor_closed() {
        let mut bar = bar_editing(roster_pill(RosterField::Tier, Op::Eq, Value::Int(10)), SegmentRole::Filter);
        assert!(with_ui(|ui, caret_id| bar.commit_field_change(TermField::Roster(RosterField::Kills), ui, caret_id)));
        let (field, _, value) = select::term_at(&bar.expr, &[]).expect("the retargeted term");
        assert_eq!(field, TermField::Roster(RosterField::Kills));
        assert_eq!(*value, Value::Int(10), "a value the new field can carry is kept");
        assert!(bar.editing.is_none(), "nothing was replaced, so nothing needs re-choosing");
    }

    /// The retarget reaches the tree through the row the dropdown actually
    /// offers, not only through `commit_field_change` called directly.
    #[test]
    fn picking_a_field_row_retargets_the_pill() {
        let mut bar = bar_editing(win(), SegmentRole::Filter);
        let row = bar
            .dropdown_rows()
            .into_iter()
            .find(|row| matches!(row, Row::Field(option) if option.field == TermField::Match(MatchField::Build)))
            .expect("Build is offered on a match pill");
        assert!(with_ui(|ui, caret_id| bar.commit_row(row, ui, caret_id)));
        let (field, _, _) = select::term_at(&bar.expr, &[]).expect("the retargeted term");
        assert_eq!(field, TermField::Match(MatchField::Build));
    }

    /// A `build is set` pill: `Build` allows the six comparisons plus the two
    /// nullary operators, and `set_op` refuses moving off a nullary operator
    /// onto one that needs a right-hand side the term no longer carries. So the
    /// first six rows are offered for context and only the last two can take.
    fn nullary_build_bar() -> QueryBar {
        bar_editing(Expr::Leaf(MatchTerm::Field(MatchField::Build, Op::IsSet, Value::NoOperand)), SegmentRole::Operator)
    }

    #[test]
    fn only_the_operators_that_would_take_are_enabled() {
        let bar = nullary_build_bar();
        let rows = bar.dropdown_rows();
        let enabled: Vec<Op> = rows
            .iter()
            .filter(|row| bar.row_enabled(row))
            .map(|row| match row {
                Row::Operator(option) => option.op,
                other => panic!("got {other:?}"),
            })
            .collect();
        assert_eq!(enabled, vec![Op::IsSet, Op::IsNotSet], "of {rows:?}");
        assert!(rows.len() > enabled.len(), "the greyed rows must still be offered, not hidden");

        // Every other variant is always pickable; only the operator list has
        // rows the tree can refuse.
        let bar = bar_editing(win(), SegmentRole::Value);
        assert!(bar.dropdown_rows().iter().all(|row| bar.row_enabled(row)), "{:?}", bar.dropdown_rows());
    }

    /// The mouse cannot click a greyed row, so the keyboard must not land on
    /// one either: an offered option that is reachable and inert is the defect
    /// class this whole plan exists to close, and arriving at it by Down-arrow
    /// is no better than arriving by pointer.
    #[test]
    fn the_keyboard_skips_a_row_the_pointer_could_not_click() {
        let mut bar = nullary_build_bar();
        let rows = bar.dropdown_rows();
        let first_enabled = rows.iter().position(|row| bar.row_enabled(row)).expect("some row takes");
        assert!(first_enabled > 0, "the fixture must have a disabled row before the first enabled one");

        assert_eq!(
            bar.step_highlight(&rows, false),
            Some(first_enabled),
            "Down must skip straight past the greyed rows"
        );
        bar.highlighted = Some(first_enabled);
        assert_eq!(bar.step_highlight(&rows, false), Some(rows.len() - 1));
        bar.highlighted = Some(rows.len() - 1);
        assert_eq!(bar.step_highlight(&rows, false), Some(rows.len() - 1), "forward off the end stays put");

        assert_eq!(bar.step_highlight(&rows, true), Some(first_enabled), "Up must skip back past them too");
        bar.highlighted = Some(first_enabled);
        assert_eq!(bar.step_highlight(&rows, true), None, "back off the first enabled row leaves the list");
    }

    /// The case that makes `step_highlight`'s forward fallback do real work: a
    /// disabled row *after* the last enabled one, where staying put is a choice
    /// rather than the only place left to be. No field produces it -- both
    /// nullable operator sets put their nullary operators last -- so the rows
    /// are built directly, which `step_highlight` allows because it is given
    /// them rather than deriving them.
    #[test]
    fn forward_off_the_last_enabled_row_will_not_land_on_a_trailing_disabled_one() {
        let mut bar = nullary_build_bar();
        let row = |op: Op| Row::Operator(OperatorOption { op, label: label::op_label(op) });
        // On a nullary term `IsSet` takes and `Eq` does not, so this is a real
        // enabled/disabled pair for this bar, not a hand-set flag.
        let rows = vec![row(Op::IsSet), row(Op::Eq)];
        assert!(bar.row_enabled(&rows[0]), "the fixture's first row must take");
        assert!(!bar.row_enabled(&rows[1]), "and its last must not");

        assert_eq!(bar.step_highlight(&rows, false), Some(0), "Down enters on the enabled row");
        bar.highlighted = Some(0);
        assert_eq!(bar.step_highlight(&rows, false), Some(0), "forward must stay rather than take the trailing row");
        assert_eq!(bar.step_highlight(&rows, true), None, "back off the only enabled row still leaves the list");

        bar.highlighted = None;
        assert_eq!(bar.step_highlight(&rows, true), Some(0), "Up into the list skips the trailing disabled row");
        assert_eq!(bar.default_row(&rows), Some(rows[0].clone()), "and Enter takes the same row");
    }

    /// Enter with nothing highlighted took `rows.first()`, which for this pill
    /// is a greyed `=` and so did nothing at all, silently, with the editor
    /// left open.
    #[test]
    fn enter_with_nothing_highlighted_takes_the_first_row_that_would_take() {
        let mut bar = nullary_build_bar();
        let rows = bar.dropdown_rows();
        assert!(!bar.row_enabled(&rows[0]), "the fixture's first row must be one that cannot take");
        assert_eq!(
            bar.default_row(&rows),
            Some(rows[bar.step_highlight(&rows, false).expect("an enabled row")].clone())
        );

        let Some(Row::Operator(option)) = bar.default_row(&rows) else {
            panic!("got {:?}", bar.default_row(&rows));
        };
        assert!(bar.commit_operator(option.op), "the row Enter takes must actually take");
        assert_eq!(select::term_at(&bar.expr, &[]).expect("the term").1, option.op);
        assert!(bar.editing.is_none(), "a committed operator closes its editor");
    }

    #[test]
    fn committing_an_operator_the_field_refuses_changes_nothing() {
        let mut bar = nullary_build_bar();
        let before = bar.expr.clone();
        assert!(!bar.commit_operator(Op::Eq), "Eq needs a right-hand side this term does not have");
        assert_eq!(bar.expr, before);
        assert!(bar.editing.is_some(), "a refused operator leaves the editor open rather than closing on nothing");
    }

    /// Enter runs through the same routing the pointer does, so the fix has to
    /// hold at the real call site and not only in the helper it delegates to.
    #[test]
    fn enter_on_a_nullary_operator_segment_applies_a_legal_operator() {
        let mut bar = nullary_build_bar();
        let rows = bar.dropdown_rows();
        let mut applied = false;
        egui::__run_test_ui(|ui| {
            if !applied {
                applied = bar.commit_typed(&rows, ui, ui.id().with("caret"));
            }
        });
        assert!(applied, "Enter must apply something rather than failing silently");
        assert_eq!(select::term_at(&bar.expr, &[]).expect("the term").1, Op::IsSet);
    }

    /// A literal the field cannot read is reported the way the typed path
    /// reports one, rather than vanishing. `date` takes a plain editor, so the
    /// caret holds the literal itself.
    #[test]
    fn a_value_the_field_cannot_read_is_underlined_rather_than_ignored() {
        let mut bar = bar_editing(
            Expr::Leaf(MatchTerm::Field(
                MatchField::Date,
                Op::Ge,
                Value::Timestamp(jiff::Timestamp::from_second(0).expect("the epoch")),
            )),
            SegmentRole::Value,
        );
        assert_eq!(bar.plain_value_field(), Some(TermField::Match(MatchField::Date)));
        bar.pending = "not-a-date".to_owned();
        let before = bar.expr.clone();

        assert!(!bar.commit_value_literal("not-a-date"));
        assert_eq!(bar.expr, before, "an unreadable literal must not reach the tree");
        let error = bar.pending_error.as_ref().expect("the failure must be reported");
        assert_eq!(error.span, 0..bar.pending.len(), "the underline covers the literal that failed");
        assert!(matches!(error.kind, ParseErrorKind::BadValue { field: "date", .. }), "got {:?}", error.kind);

        // The next keystroke clears it, so the mark follows the text.
        bar.pending = "2026-01-01".to_owned();
        bar.reparse_pending();
        assert!(bar.pending_error.is_none());
        assert!(bar.commit_value_literal("2026-01-01"));
        assert!(bar.editing.is_none());
    }

    /// A literal from a dropdown row is not what the caret holds, so reporting
    /// against the caret would mark the wrong text -- and against an empty
    /// caret would mark nothing at all, since `caret` suppresses the underline
    /// while `pending` is empty. That is the silent commit this reporting
    /// exists to remove, reappearing on the one path it did not cover.
    ///
    /// Only reachable if a token the grammar printed fails to reparse, so this
    /// drives `report_bad_value` with a token the field genuinely cannot read
    /// rather than waiting for such a mismatch to exist.
    #[test]
    fn a_row_token_the_field_cannot_read_is_moved_into_the_caret_to_be_marked() {
        let mut bar = bar_editing(
            Expr::Leaf(MatchTerm::Field(
                MatchField::Date,
                Op::Ge,
                Value::Timestamp(jiff::Timestamp::from_second(0).expect("the epoch")),
            )),
            SegmentRole::Value,
        );
        // The caret opened holding the value already on the term, so clear it:
        // an empty caret is the case where the old span suppressed the mark.
        bar.pending.clear();
        bar.parsed_text.clear();

        assert!(!bar.commit_value_literal("not-a-date"));
        assert_eq!(bar.pending, "not-a-date", "the failing literal must reach the caret to be underlined");
        let error = bar.pending_error.as_ref().expect("the failure must be reported");
        assert_eq!(error.span, 0.."not-a-date".len(), "the span covers the literal, not the caret's old text");

        // `parsed_text` moved with it, so the next frame's reparse leaves the
        // mark alone rather than erasing it before it is ever drawn.
        bar.reparse_pending();
        assert!(bar.pending_error.is_some(), "a frame with no keystroke must not clear the mark");
    }

    /// Opening a plain value editor seeds the caret with the literal already on
    /// the term, so a date is corrected rather than retyped from nothing.
    #[test]
    fn opening_a_plain_value_editor_seeds_the_caret_with_the_current_literal() {
        let _zone = pinned_utc();
        let mut bar = QueryBar::default();
        bar.set_expr(Expr::Leaf(MatchTerm::Field(
            MatchField::Date,
            Op::Ge,
            Value::Timestamp(jiff::Timestamp::from_second(0).expect("the epoch")),
        )));
        let opened = with_ui(|ui, caret_id| bar.begin_segment_edit(vec![], vec![], SegmentRole::Value, ui, caret_id));
        assert!(opened);
        assert_eq!(bar.pending, "1970-01-01", "the caret opens holding the value already on the term");
        assert_eq!(bar.editing.as_ref().map(|edit| edit.role), Some(SegmentRole::Value));
    }

    /// A nullary operator draws no value segment, so there is nothing for a
    /// value editor to open on and it must refuse rather than open on nothing.
    #[test]
    fn a_value_editor_refuses_to_open_where_the_pill_draws_no_value() {
        let mut bar = QueryBar::default();
        bar.set_expr(Expr::Leaf(MatchTerm::Field(MatchField::Build, Op::IsSet, Value::NoOperand)));
        let opened = with_ui(|ui, caret_id| bar.begin_segment_edit(vec![], vec![], SegmentRole::Value, ui, caret_id));
        assert!(!opened);
        assert!(bar.editing.is_none());
    }

    fn frame_input(width: f32) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(width, 600.0))),
            ..Default::default()
        }
    }

    fn click_input(pos: Pos2, width: f32) -> egui::RawInput {
        let mut input = frame_input(width);
        input.events.push(egui::Event::PointerMoved(pos));
        for pressed in [true, false] {
            input.events.push(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            });
        }
        input
    }

    /// A bar driven through real frames, with the ids the widgets inside it are
    /// registered under.
    ///
    /// Fonts are left at egui's defaults rather than emptied. An empty font set
    /// lays every galley out at zero size, which gives the caret's `TextEdit` a
    /// zero-height response rect -- and a click point derived from that rect
    /// lands outside the row it is meant to be inside, so the test would be
    /// aiming somewhere it does not mean to and passing for the wrong reason.
    struct Harness {
        ctx: egui::Context,
        bar: QueryBar,
        id: egui::Id,
        width: f32,
        /// What the last frame painted, so a test can ask whether something was
        /// drawn rather than only whether it registered a click target.
        shapes: Vec<egui::epaint::ClippedShape>,
    }

    impl Harness {
        /// Builds the bar and runs one frame, which is what registers the
        /// widgets whose ids and rects the rest of a test reads.
        fn new(expr: MatchExpr, width: f32) -> Self {
            let mut bar = QueryBar::default();
            bar.set_expr(expr);
            let mut harness =
                Self { ctx: egui::Context::default(), bar, id: egui::Id::NULL, width, shapes: Vec::new() };
            harness.frame(frame_input(width));
            harness
        }

        fn frame(&mut self, input: egui::RawInput) {
            let bar = &mut self.bar;
            let deps = Deps { history: &[] };
            let mut id = None;
            let output = self.ctx.run_ui(input, |ui| {
                id = Some(ui.id().with("query_bar"));
                bar.show(ui, &deps);
            });
            self.shapes = output.shapes;
            if let Some(id) = id {
                self.id = id;
            }
        }

        /// Runs quiet frames until what `rect_of` and `shapes` report is what
        /// the bar draws in its current state.
        ///
        /// Two of them, for two separate lags. An input is applied at the end of
        /// the frame that carries it, so that frame still draws the state before
        /// it. And `Context::read_response` answers out of `this_pass` first,
        /// which `end_pass` *swaps* with `prev_pass` rather than clearing, so a
        /// rect read straight after a frame is the frame before it.
        fn settle(&mut self) {
            self.frame(frame_input(self.width));
            self.frame(frame_input(self.width));
        }

        fn caret_id(&self) -> egui::Id {
            self.id.with("caret")
        }

        fn rect_of(&self, id: egui::Id) -> Rect {
            self.ctx.read_response(id).unwrap_or_else(|| panic!("no widget registered under {id:?}")).rect
        }

        /// The size the dropdown's `Area` remembers, which is what it is drawn
        /// at on every frame after its first.
        fn popup_width(&self) -> Option<f32> {
            egui::AreaState::load(&self.ctx, self.id.with("dropdown"))?.size.map(|size| size.x)
        }

        /// The left edge the dropdown's `Area` is pinned to. `BOTTOM_START`
        /// pivots the area on its own top-left corner, so the pivot position
        /// the area remembers is that edge directly, with no size or alignment
        /// arithmetic to get wrong on the read side.
        fn popup_left(&self) -> Option<f32> {
            let state = egui::AreaState::load(&self.ctx, self.id.with("dropdown"))?;
            debug_assert_eq!(state.pivot, egui::Align2::LEFT_TOP, "BOTTOM_START pivots on the child's top-left");
            state.pivot_pos.map(|pos| pos.x)
        }
    }

    fn date_pill() -> MatchExpr {
        Expr::Leaf(MatchTerm::Field(
            MatchField::Date,
            Op::Ge,
            Value::Timestamp(jiff::Timestamp::from_second(0).expect("the epoch")),
        ))
    }

    /// Opens a value editor the way a segment click would, and hands the caret
    /// the focus that click would have given it.
    fn open_value_editor(harness: &mut Harness, pill: NodePath, width: f32) {
        let caret_id = harness.caret_id();
        let bar = &mut harness.bar;
        let path = select::segment_path(&bar.expr, &pill).expect("the fixture pill addresses a term");
        let opened = with_ui(|ui, _| bar.begin_segment_edit(pill, path, SegmentRole::Value, ui, caret_id));
        assert!(opened);
        // Parked here for the reason `mint_forced_box` gives: `begin_edit` put
        // the cursor in `with_ui`'s context, which is not the one under test.
        let cursor = harness.bar.pending.chars().count();
        park_caret(&harness.ctx, caret_id, cursor);
        harness.ctx.memory_mut(|m| m.request_focus(caret_id));
        harness.settle();
        assert_eq!(harness.width, width, "the fixture must drive the harness at the width it was built with");
        assert!(harness.bar.editing.is_some(), "a quiet frame must not close the editor");
    }

    /// A click inside the caret is not "the pointer moved on to something
    /// else": for a plain value editor the caret **is** the editor, holding the
    /// literal `begin_segment_edit` seeded it with, and for the other two it
    /// holds the needle. Clicking into it to correct the text must not throw
    /// that text away and close the editor.
    ///
    /// Two points, and the second is the one that needs the gate. The
    /// `TextEdit` fills its cell horizontally but is inset vertically by
    /// `ROW_PAD_Y`, so a click on the text lands on the widget and never
    /// reaches the background at all, while a click on the padding within the
    /// caret's own row does. Only the row band tells that second point apart
    /// from a click on another row.
    ///
    /// Driven through real frames rather than by calling the handshake's
    /// pieces, because the carrier is egui's own hit testing: which of two
    /// overlapping widgets receives a press is not something a unit-level
    /// fixture can answer.
    #[test]
    fn clicking_inside_the_carets_own_row_keeps_the_segment_editor_open() {
        let _zone = pinned_utc();
        let mut harness = Harness::new(date_pill(), 800.0);
        open_value_editor(&mut harness, vec![], 800.0);
        assert_eq!(harness.bar.pending, "1970-01-01");

        let caret = harness.rect_of(harness.caret_id());
        assert!(caret.height() > 0.0, "a degenerate caret rect would put the click points outside the row they name");

        harness.frame(click_input(caret.center(), 800.0));
        assert_eq!(harness.bar.pending, "1970-01-01", "the literal being edited must survive a click into the text");
        assert!(harness.bar.editing.is_some(), "clicking the text must not close the editor it belongs to");

        // Inside the caret's row, outside the widget: the strip `caret_rect`
        // insets and the band puts back.
        let padding = Pos2::new(caret.center().x, caret.top() - 1.0);
        assert!(!caret.contains(padding), "the second point must be off the widget: {padding:?} in {caret:?}");
        harness.frame(click_input(padding, 800.0));
        assert_eq!(harness.bar.pending, "1970-01-01", "the row's own padding is still the caret's row");
        assert!(harness.bar.editing.is_some(), "clicking beside the text within its row must not close the editor");
    }

    /// The other half of the same gate, and the one an x-only test cannot
    /// state. `lay_out` stretches the caret to the bar's right edge, and a
    /// wrapped caret starts at x = 0, so its x-range is the whole bar: ignoring
    /// y hands it every background click on every row above it, and the gesture
    /// that dismisses a segment editor stops working on any bar wide enough to
    /// wrap.
    #[test]
    fn a_background_click_on_a_row_the_caret_is_not_on_ends_the_segment_edit() {
        const WIDTH: f32 = 260.0;
        let pills = Expr::All(vec![
            date_pill(),
            Expr::Leaf(MatchTerm::Field(MatchField::Build, Op::Ge, Value::Int(1234))),
            Expr::Leaf(MatchTerm::Field(MatchField::Outcome, Op::Is, Value::Outcome(MatchOutcome::Win))),
        ]);
        let mut harness = Harness::new(pills, WIDTH);
        // The last pill, so the box it opens sits on a later row than the one
        // the click below aims at. A box opened on the first pill would share
        // that row and the fixture could not tell the two apart.
        open_value_editor(&mut harness, vec![2], WIDTH);

        let caret = harness.rect_of(harness.caret_id());
        let background = harness.rect_of(harness.id.with("background"));
        // Every token's rect, so the click can be aimed at a point the fixture
        // proves is empty rather than one that merely looks it.
        let tokens: Vec<Rect> =
            (0..8).filter_map(|i| harness.ctx.read_response(harness.id.with(i))).map(|r| r.rect).collect();
        let first = *tokens.first().expect("the fixture must draw pills");
        assert!(
            caret.top() >= first.bottom(),
            "the caret must not sit on the row being clicked: {caret:?} vs {first:?}"
        );

        let on_row: Vec<Rect> =
            tokens.iter().copied().filter(|r| r.top() < first.bottom() && r.bottom() > first.top()).collect();
        let row_end = on_row.iter().fold(f32::MIN, |acc, r| acc.max(r.right()));
        let target = Pos2::new(background.right() - 1.0, first.center().y);
        assert!(
            target.x > row_end,
            "the fixture must leave empty space at the end of the row: {target:?} past {row_end}"
        );
        assert!(background.contains(target), "the target must be inside the bar: {target:?} in {background:?}");
        assert!(!tokens.iter().any(|r| r.contains(target)), "the target must be empty space: {target:?} in {tokens:?}");
        assert!(!caret.contains(target), "and must not be the caret: {target:?} in {caret:?}");

        harness.frame(click_input(target, WIDTH));
        assert!(
            harness.bar.editing.is_none(),
            "a background click away from the caret must end the segment edit, not be swallowed by the caret's x-range"
        );
    }

    /// A narrow list -- the case that broke first -- must not read as though it
    /// belonged to the whole bar. `Outcome`'s operator segment is a couple of
    /// characters wide, and its list is just `is` and `is not`; against an
    /// 800-wide bar, a popup pinned to the bar's own width is unmistakably the
    /// bug this guards, not a rounding difference.
    #[test]
    fn a_narrow_list_produces_a_narrow_popup_not_a_bar_width_one() {
        const WIDTH: f32 = 800.0;
        let mut harness = Harness::new(win(), WIDTH);
        harness.ctx.memory_mut(|m| m.request_focus(harness.caret_id()));

        harness.bar.editing =
            Some(SegmentEdit::open(&harness.bar.expr, vec![], vec![], SegmentRole::Operator, None, ""));
        harness.bar.dropdown_open = true;
        harness.frame(frame_input(WIDTH));
        harness.frame(frame_input(WIDTH));

        let narrow = harness.popup_width().expect("the dropdown was built");
        let bar = harness.rect_of(harness.id.with("background")).width();
        assert!(narrow < 200.0, "an `is` / `is not` list must not balloon to the bar's width: {narrow} (bar is {bar})");
    }

    /// The dropdown must not shrink to whatever its narrowest list measured and
    /// stay there.
    ///
    /// `Popup::width` cannot deliver this: it feeds `Area::default_size`, which
    /// the area reads inside `state.size.get_or_insert_with(..)`, so it applies
    /// on the frame the popup is first built and never again. From then on the
    /// area keeps its own last content size, and a narrow list pins it narrow
    /// for good -- including for the alignment decision, which reads that same
    /// stored size when choosing whether to flip near a screen edge.
    ///
    /// The value list of an unresolved ship term is the fixture: its anchor is
    /// the `#<raw>` placeholder, a couple of characters wide either way, so the
    /// two widths below are content alone, not the anchor moving.
    ///
    /// Two lists and several frames, because the bug is invisible on the first
    /// one: the wide list is what the narrow list then has to fail to shrink.
    #[test]
    fn a_wide_list_shown_after_a_narrow_one_recovers_its_width() {
        const WIDTH: f32 = 800.0;
        let mut harness = Harness::new(roster_pill(RosterField::Ship, Op::Is, Value::Ship(1u64.into())), WIDTH);
        let path = select::segment_path(&harness.bar.expr, &vec![]).expect("the fixture pill addresses a term");
        harness.bar.editing = Some(SegmentEdit::open(&harness.bar.expr, vec![], path, SegmentRole::Value, None, ""));
        harness.bar.dropdown_open = true;
        harness.ctx.memory_mut(|m| m.request_focus(harness.caret_id()));

        harness.bar.value_options = vec![ValueOption { label: "A".to_owned(), token: "1".to_owned() }];
        harness.frame(frame_input(WIDTH));
        harness.frame(frame_input(WIDTH));
        let narrow = harness.popup_width().expect("the dropdown was built");

        harness.bar.value_options = vec![ValueOption { label: "A".repeat(80), token: "1".to_owned() }];
        harness.frame(frame_input(WIDTH));
        harness.frame(frame_input(WIDTH));
        let wide = harness.popup_width().expect("the dropdown is still built");

        assert!(
            wide > narrow * 2.0,
            "a wide list shown after a narrow one must recover its own width: {wide} against {narrow}"
        );
    }

    /// However wide its content wants to be, the dropdown must not read as
    /// wider than the control it belongs to. Swept across several bar widths
    /// against the caret's full suggestion list -- the widest content this
    /// bar ever shows, so the cap has something to actually resist.
    #[test]
    fn the_dropdown_never_grows_past_the_bars_width() {
        for width in [250.0f32, 300.0, 400.0, 500.0, 800.0] {
            let mut harness = Harness::new(Expr::default(), width);
            harness.bar.refresh_suggestions();
            harness.bar.dropdown_open = true;
            harness.ctx.memory_mut(|m| m.request_focus(harness.caret_id()));
            harness.frame(frame_input(width));
            harness.frame(frame_input(width));

            let popup = harness.popup_width().expect("the dropdown was built");
            assert!(popup <= width + 0.5, "the dropdown outgrew its {width}-wide bar: {popup}");
        }
    }

    /// The dropdown hangs under the segment being edited, not under the bar's
    /// own left edge -- otherwise a bar-width popup anchored under a mid-bar
    /// segment runs off the right edge and egui's flip and shift alternatives
    /// reposition it, which is the second symptom the over-wide popup caused.
    ///
    /// Three pills so the segment under test sits well clear of the bar's own
    /// left edge, which is what tells the two apart.
    #[test]
    fn the_dropdowns_left_edge_tracks_the_anchor_not_the_bar() {
        const WIDTH: f32 = 800.0;
        let pills = Expr::All(vec![
            date_pill(),
            Expr::Leaf(MatchTerm::Field(MatchField::Build, Op::Ge, Value::Int(1234))),
            Expr::Leaf(MatchTerm::Field(MatchField::Outcome, Op::Is, Value::Outcome(MatchOutcome::Win))),
        ]);
        let mut harness = Harness::new(pills, WIDTH);
        let path = select::segment_path(&harness.bar.expr, &vec![1]).expect("the fixture's second pill");
        harness.bar.editing =
            Some(SegmentEdit::open(&harness.bar.expr, vec![1], path, SegmentRole::Operator, None, ""));
        harness.bar.dropdown_open = true;
        harness.ctx.memory_mut(|m| m.request_focus(harness.caret_id()));
        harness.frame(frame_input(WIDTH));
        harness.frame(frame_input(WIDTH));

        let anchor = harness.bar.popup_anchor.expect("editing a segment anchors the dropdown to it");
        let bar_left = harness.rect_of(harness.id.with("background")).left();
        assert!(
            anchor.left() - bar_left > 50.0,
            "the fixture must anchor well clear of the bar's own left edge: {anchor:?} vs bar left {bar_left}"
        );

        let popup_left = harness.popup_left().expect("the dropdown was built");
        assert!(
            (popup_left - anchor.left()).abs() < 1.0,
            "the popup's left edge must track the anchor: {popup_left} vs anchor {anchor:?}"
        );
        assert!(
            (popup_left - bar_left).abs() > 50.0,
            "the popup must not have fallen back to the bar's own left edge: {popup_left} vs bar left {bar_left}"
        );
    }

    /// The report this change answers: a picked row builds the filter, rather
    /// than spelling it into the caret for Enter to finish. The segment path
    /// already mutated the tree at once, so the two paths only looked alike.
    #[test]
    fn picking_a_field_from_the_caret_appends_a_pill_rather_than_text() {
        let mut bar = caret_bar();
        let (index, field) = first_field_row(&bar);
        bar.highlighted = Some(index);
        let row = bar.dropdown_rows()[index].clone();

        assert!(with_ui(|ui, caret_id| bar.commit_row(row, ui, caret_id)), "picking a field must change the query");
        assert!(bar.pending.is_empty(), "the caret must not be left holding grammar text: {:?}", bar.pending);
        assert!(!bar.expr.is_empty_all(), "no term was appended");

        let path = select::segment_path(&bar.expr, &vec![]).expect("the new pill addresses a term");
        let (landed, op, _) = select::term_at(&bar.expr, &path).expect("the minted term");
        assert_eq!(landed, field, "the pill must carry the field that was picked");
        assert!(field.allowed_ops().contains(&op), "{op:?} is not one {field:?} allows");
    }

    /// The minted value is a placeholder, not a choice. If the editor does not
    /// open, the user commits a value they never picked.
    ///
    /// Minted into a query that already holds a term, and of a different field,
    /// so "the editor is open on *a* term" cannot pass for "the editor is open
    /// on the one just minted". Against an empty bar there is only one term and
    /// the assertion cannot discriminate even in principle.
    #[test]
    fn picking_a_field_opens_the_value_editor_on_the_new_pill() {
        let mut bar = caret_bar();
        bar.set_expr(parse_query("outcome:win").expect("parse"));
        let minted = TermField::Match(MatchField::Map);
        let row = field_row(&bar, minted);
        with_ui(|ui, caret_id| bar.commit_row(row, ui, caret_id));

        let edit = bar.editing.as_ref().expect("no value editor opened");
        assert_eq!(edit.role, SegmentRole::Value);
        let (addressed, _, _) = select::term_at(&bar.expr, &edit.path).expect("the editor must address a term");
        assert_eq!(addressed, minted, "the editor opened on a term other than the one just minted");
    }

    /// The caret path wrote `enemy.ship=1234` and waited. A pill resolves the
    /// name, which is the whole difference the user sees.
    #[test]
    fn picking_a_ship_shows_its_name_rather_than_its_id() {
        let mut bar = caret_bar();
        bar.names.ships.insert(GameParamId::from(1234u64), "Yamato".to_owned());
        let row = seed_ship_value_editor(&mut bar, GameParamId::from(1234u64));

        assert!(with_ui(|ui, caret_id| bar.commit_row(row, ui, caret_id)), "picking a ship must change the query");
        assert!(bar.pending.is_empty(), "the caret must not be left holding the term's text: {:?}", bar.pending);
        bar.finish_edit();

        let text = pill_text_at(&bar, &vec![]);
        assert!(text.contains("Yamato"), "got {text:?}");
        assert!(!text.contains("1234"), "raw id leaked into the pill: {text:?}");
    }

    /// The typed path is the share format and must not regress: text is still
    /// parsed and committed on Enter, and only what a picked row does changed.
    #[test]
    fn typing_a_whole_term_and_pressing_enter_still_commits_it() {
        let mut bar = caret_bar();
        bar.pending = "outcome:win".to_owned();
        let rows = bar.dropdown_rows();

        assert!(with_ui(|ui, caret_id| bar.commit_typed(&rows, ui, caret_id)), "Enter must commit the typed term");
        // `show` canonicalises after every edit that took; a one-term query is
        // a bare leaf only once it has.
        bar.finish_edit();

        assert_eq!(bar.expr, parse_query("outcome:win").expect("parse"));
        assert!(bar.pending.is_empty(), "a committed term leaves the caret empty");
        assert!(bar.editing.is_none(), "typing a whole term opens no editor");
    }

    /// Picking a field part way through a typed query must not throw away the
    /// terms already in the caret, which the text path kept by replacing only
    /// the active fragment.
    #[test]
    fn picking_a_field_keeps_the_terms_already_typed() {
        let mut bar = caret_bar();
        bar.pending = "outcome:win ma".to_owned();
        let row = field_row(&bar, TermField::Match(MatchField::Map));

        assert!(with_ui(|ui, caret_id| bar.commit_row(row, ui, caret_id)));
        bar.finish_edit();

        let Expr::All(children) = &bar.expr else {
            panic!("the typed term was dropped instead of committed: {:?}", bar.expr);
        };
        assert_eq!(children.len(), 2, "{:?}", bar.expr);
        assert_eq!(
            select::term_at(&bar.expr, &[0]).map(|(field, _, _)| field),
            Some(TermField::Match(MatchField::Outcome)),
            "the term typed ahead of the pick must survive as its own pill"
        );
        assert_eq!(
            select::term_at(&bar.expr, &[1]).map(|(field, _, _)| field),
            Some(TermField::Match(MatchField::Map))
        );
    }

    /// A scope is a relation constraint standing beside the term rather than a
    /// part of it, so a mint that dropped it would silently widen the filter to
    /// the whole roster. Pinned against the same term typed by hand, which is
    /// the only statement of the expansion that cannot drift from the parser.
    #[test]
    fn a_scoped_field_mints_the_scopes_own_constraint() {
        let minted = select::new_term(TermField::Roster(RosterField::Ship), Some(Scope::Enemy)).expect("a ship term");
        assert_eq!(minted, parse_query("enemy.ship=0").expect("the same term typed by hand"));

        let anyone = select::new_term(TermField::Roster(RosterField::Ship), Some(Scope::Anyone)).expect("a ship term");
        assert_ne!(anyone, minted, "the widest scope must not carry a relation constraint");
        assert_eq!(anyone, parse_query("anyone.ship=0").expect("the same term typed by hand"));
    }

    /// Every field the caret offers has to mint a term the grammar reads back
    /// as itself. The mint goes through the grammar's own text, so a field whose
    /// placeholder does not print as a literal that field accepts would fail
    /// here rather than in the app, where the row would be offered and picking
    /// it would do nothing at all.
    #[test]
    fn every_field_the_caret_offers_mints_a_term_the_grammar_reads_back() {
        let mut checked = 0;
        for suggestion in filter_options() {
            let (field, scope) = match suggestion.kind {
                SuggestionKind::MatchField(field) => (TermField::Match(field), None),
                SuggestionKind::RosterField { field, scope } => {
                    (TermField::Roster(field), Some(scope.unwrap_or(Scope::Anyone)))
                }
                SuggestionKind::Preset(_) => continue,
            };
            let label = &suggestion.label;
            let minted = select::new_term(field, scope).unwrap_or_else(|| panic!("{label} mints nothing"));

            let path = select::segment_path(&minted, &vec![])
                .unwrap_or_else(|| panic!("{label} mints a pill with no editable segment"));
            let (landed, op, _) =
                select::term_at(&minted, &path).unwrap_or_else(|| panic!("{label} mints no term at {path:?}"));
            assert_eq!(landed, field, "{label} minted the wrong field");
            assert!(field.allowed_ops().contains(&op), "{label} minted {op:?}, which its field refuses");

            let printed = query_text::print_query(&minted);
            let reparsed = parse_query(&printed).unwrap_or_else(|e| panic!("{label} printed {printed:?}: {e}"));
            assert_eq!(reparsed, minted, "{label} printed {printed:?}");
            checked += 1;
        }
        assert!(checked >= 50, "the caret offers 58 field rows, so {checked} means most of them vanished");
    }

    /// The obligation `mint_term` carries: a placeholder reaches the tree, so an
    /// editor that can withdraw it has to open on every field the caret offers.
    /// A field with no editor would leave one resting in the query, which is the
    /// state the snapshot exists to make unreachable.
    #[test]
    fn every_field_the_caret_offers_opens_an_editor_that_can_withdraw_it() {
        let mut checked = 0;
        with_ui(|ui, caret_id| {
            for suggestion in filter_options() {
                let (field, scope) = match suggestion.kind {
                    SuggestionKind::MatchField(field) => (TermField::Match(field), None),
                    SuggestionKind::RosterField { field, scope } => {
                        (TermField::Roster(field), Some(scope.unwrap_or(Scope::Anyone)))
                    }
                    SuggestionKind::Preset(_) => continue,
                };
                let label = &suggestion.label;
                let mut bar = QueryBar::default();
                assert!(bar.mint_term(field, scope, ui, caret_id), "{label} minted nothing");

                let edit = bar.editing.as_ref().unwrap_or_else(|| panic!("{label} opened no editor"));
                assert_eq!(edit.role, SegmentRole::Value, "{label}");
                let restore = edit.restore.as_ref().unwrap_or_else(|| panic!("{label} carries no snapshot"));
                assert!(
                    restore.before.is_empty_all(),
                    "{label} snapshotted {:?} rather than the empty query",
                    restore.before
                );
                assert_eq!(restore.minted.as_deref(), Some([].as_slice()), "{label} named the wrong minted pill");
                let (addressed, _, _) =
                    select::term_at(&bar.expr, &edit.path).unwrap_or_else(|| panic!("{label} addresses no term"));
                assert_eq!(addressed, field, "{label} opened its editor on the wrong term");
                checked += 1;
            }
        });
        assert!(checked >= 50, "the caret offers 58 field rows, so {checked} means most of them vanished");
    }

    /// Mints a term the way picking that field's row does, and hands back the
    /// query as it stood before, which is what dismissal has to restore.
    fn mint_from_caret(bar: &mut QueryBar, field: TermField, scope: Option<Scope>) -> MatchExpr {
        let before = bar.expr.clone();
        assert!(with_ui(|ui, caret_id| bar.mint_term(field, scope, ui, caret_id)), "the mint must take");
        assert!(bar.editing.is_some(), "the value editor must open on the placeholder");
        assert_ne!(bar.expr, before, "the placeholder is in the tree while its editor is open");
        before
    }

    /// Escape while the dropdown is open, through the same routing the key does.
    fn press_escape(bar: &mut QueryBar) -> bool {
        let deps = Deps { history: &[] };
        with_ui(|ui, caret_id| bar.apply_nav(Nav::CloseDropdown, &[], &deps, ui, caret_id))
    }

    /// The case with no visible defect at all, and the reason the earlier
    /// ruling was wrong. `outcome`'s placeholder is `win` -- a value a user
    /// picks on purpose -- so a dismissed editor that left it behind would
    /// narrow every later search to victories, with nothing on screen saying so.
    #[test]
    fn dismissing_a_minted_outcome_pill_restores_the_query() {
        let mut bar = caret_bar();
        let before = mint_from_caret(&mut bar, TermField::Match(MatchField::Outcome), None);
        assert_eq!(query_text::print_query(&bar.expr), "outcome=win", "the fixture must mint the silent placeholder");

        assert!(press_escape(&mut bar), "a restore has to be reported, or the results stay stale");
        assert_eq!(bar.expr, before, "the placeholder outlived its editor");
        assert!(bar.editing.is_none());
    }

    /// The visibly broken case: a zero id resolves to no ship, so the pill reads
    /// as nonsense and the compiled query matches nothing.
    #[test]
    fn dismissing_a_minted_ship_pill_restores_the_query() {
        let mut bar = caret_bar();
        let before = mint_from_caret(&mut bar, TermField::Roster(RosterField::Ship), Some(Scope::Enemy));
        assert_eq!(query_text::print_query(&bar.expr), "enemy.ship=0");
        assert!(pill_text_at(&bar, &vec![]).contains('0'), "the fixture must be the unresolvable pill");

        assert!(press_escape(&mut bar), "a restore has to be reported");
        assert_eq!(bar.expr, before);
    }

    /// The case the compiler already forgave: an empty `Contains` is vacuous, so
    /// `prune_empty` drops it before the query runs. The pill is still on screen
    /// though, so dismissal has to withdraw it like any other.
    #[test]
    fn dismissing_a_minted_map_pill_restores_the_query() {
        let mut bar = caret_bar();
        let before = mint_from_caret(&mut bar, TermField::Match(MatchField::Map), None);
        assert_eq!(query_text::print_query(&bar.expr), "map:\"\"");
        assert!(select::prune_empty(&bar.expr).is_empty_all(), "the fixture must be the benign placeholder");

        press_escape(&mut bar);
        assert_eq!(bar.expr, before, "a pill the query ignores is still a pill the user did not ask for");
    }

    /// Dismissal withdraws the placeholder and nothing else: the terms the caret
    /// held when the row was picked were committed on purpose.
    #[test]
    fn dismissing_a_minted_pill_keeps_the_terms_typed_before_it() {
        let mut bar = caret_bar();
        bar.pending = "outcome:win ".to_owned();
        let before = bar.expr.clone();
        assert!(with_ui(|ui, caret_id| bar.mint_term(
            TermField::Roster(RosterField::Ship),
            Some(Scope::Enemy),
            ui,
            caret_id
        )));
        assert_ne!(bar.expr, before);

        press_escape(&mut bar);
        assert_eq!(bar.expr, parse_query("outcome:win").expect("parse"), "the typed term was withdrawn with the pill");
    }

    /// The snapshot must not outlive the choice it was insurance against.
    #[test]
    fn choosing_a_value_for_a_minted_pill_keeps_it() {
        let mut bar = caret_bar();
        mint_from_caret(&mut bar, TermField::Match(MatchField::Outcome), None);
        assert!(bar.commit_value_literal("loss"), "the picked literal must land");
        assert!(bar.editing.is_none(), "a committed value closes its editor");

        assert!(!press_escape(&mut bar), "nothing is left to restore");
        assert_eq!(bar.expr, parse_query("outcome=loss").expect("parse"));
    }

    /// The scope of the whole mechanism. An editor the user opened by clicking a
    /// segment holds a value they chose earlier, so Escape leaves the term
    /// alone; only an editor a mint forced open carries a snapshot.
    #[test]
    fn dismissing_an_editor_the_user_opened_keeps_its_value() {
        let mut bar = QueryBar::default();
        bar.set_expr(win());
        let opened = with_ui(|ui, caret_id| bar.begin_segment_edit(vec![], vec![], SegmentRole::Value, ui, caret_id));
        assert!(opened);
        assert!(bar.editing.as_ref().expect("the editor").restore.is_none(), "a clicked segment carries no snapshot");

        assert!(!press_escape(&mut bar), "dismissing a deliberate edit changes nothing");
        assert_eq!(bar.expr, win());
    }

    /// The other path that mints a placeholder: retargeting a pill at a field
    /// its old value cannot serve. Dismissal has to put the whole term back,
    /// field included, since the new field is half of a term the user never
    /// finished.
    #[test]
    fn dismissing_a_retarget_that_minted_a_placeholder_restores_the_term() {
        let mut bar = bar_editing(roster_pill(RosterField::Tier, Op::Eq, Value::Int(10)), SegmentRole::Filter);
        let before = bar.expr.clone();
        assert!(with_ui(|ui, caret_id| bar.commit_field_change(TermField::Roster(RosterField::Ship), ui, caret_id)));
        assert_eq!(query_text::print_query(&bar.expr), "anyone.ship=0", "the retarget must mint a placeholder");

        assert!(press_escape(&mut bar), "a restore has to be reported");
        assert_eq!(bar.expr, before, "the pill was left carrying a field and a value the user never chose");
    }

    /// The path a click on the first pill carries. `segment_interaction` pushes
    /// the pill's own path, so this reads it off the token stream rather than
    /// assuming what it is.
    fn first_pill_path(bar: &QueryBar) -> NodePath {
        tokenize(&bar.expr, &bar.names)
            .into_iter()
            .find(|token| matches!(token.kind, TokenKind::Pill { .. }))
            .expect("the fixture draws a pill")
            .path
    }

    /// The dismissal that cannot self-correct. Escape and focus loss both leave
    /// the bar, and the frame that follows re-runs the query anyway; clicking
    /// another pill's segment stays inside it, so a restore that went unreported
    /// would leave the table showing the placeholder's empty result against a
    /// query that plainly matches -- and the Search tab mirrors the query into
    /// settings on that same flag, so the withdrawn placeholder would outlive a
    /// restart.
    #[test]
    fn clicking_another_pills_segment_reports_the_restore_it_performs() {
        let mut bar = caret_bar();
        bar.set_expr(parse_query("outcome:win and anyone.tier=10").expect("parse"));
        let before = mint_from_caret(&mut bar, TermField::Roster(RosterField::Ship), Some(Scope::Enemy));

        let click = Command::EditSegment(first_pill_path(&bar), SegmentRole::Value);
        let changed = with_ui(|ui, caret_id| bar.apply_command(click, ui, caret_id));

        assert!(changed, "the restore was silent, so nothing re-runs the query or rewrites the saved one");
        assert_eq!(bar.expr, before, "the placeholder outlived the click that dismissed it");
        let edit = bar.editing.as_ref().expect("the clicked segment must still open its editor");
        assert_eq!(
            select::term_at(&bar.expr, &edit.path).map(|(field, _, _)| field),
            Some(TermField::Match(MatchField::Outcome)),
            "the click opened an editor on the wrong term"
        );
    }

    /// The click a path-shaped retry makes dangerous: the placeholder's own
    /// segment, which is the natural "wrong field, let me fix it" move. The
    /// restore takes that pill away, so the path the click carries now names
    /// whichever pill inherited it -- and a retarget editor opened there would
    /// rewrite a filter the user never touched, while a value editor would offer
    /// that other pill's literals in a list opened expecting ships. A numeric
    /// path names a position, not a pill, so the gate is identity.
    #[test]
    fn clicking_the_withdrawn_placeholders_own_segment_opens_nothing() {
        for role in [SegmentRole::Filter, SegmentRole::Operator, SegmentRole::Value] {
            let mut bar = caret_bar();
            bar.set_expr(parse_query("outcome:win").expect("parse"));
            let before = mint_from_caret(&mut bar, TermField::Roster(RosterField::Ship), Some(Scope::Enemy));
            let placeholder = select::last_appended_path(&bar.expr);
            assert_eq!(placeholder, vec![1], "the fixture must sit the placeholder after the surviving pill");

            let click = Command::EditSegment(placeholder, role);
            let changed = with_ui(|ui, caret_id| bar.apply_command(click, ui, caret_id));

            assert!(changed, "{role:?}: the withdrawal still has to be reported");
            assert_eq!(bar.expr, before, "{role:?}");
            assert!(bar.editing.is_none(), "{role:?} opened an editor on a pill the user never clicked");
        }
    }

    /// Opening an editor is not a change to the query. A reported change re-runs
    /// the search and rewrites the saved query, so the report is pinned in both
    /// directions rather than only where it must fire.
    #[test]
    fn clicking_a_segment_with_nothing_to_restore_reports_no_change() {
        let mut bar = caret_bar();
        bar.set_expr(parse_query("outcome:win and anyone.tier=10").expect("parse"));
        let query = bar.expr.clone();

        for role in [SegmentRole::Value, SegmentRole::Operator, SegmentRole::Filter] {
            let click = Command::EditSegment(first_pill_path(&bar), role);
            let changed = with_ui(|ui, caret_id| bar.apply_command(click, ui, caret_id));

            assert!(!changed, "{role:?} reported a query change for opening an editor");
            assert_eq!(bar.expr, query, "{role:?}");
            assert!(bar.editing.is_some(), "{role:?} must still open its editor");
        }
    }

    /// The renumbering that same restore can cause. Withdrawing the placeholder
    /// leaves a single condition, which dissolves the root's group and moves
    /// every path under it up a level, so the pill the click was painted over is
    /// no longer where the click says it is.
    #[test]
    fn clicking_the_only_other_pill_still_opens_it_after_the_restore() {
        let mut bar = caret_bar();
        bar.set_expr(parse_query("outcome:win").expect("parse"));
        mint_from_caret(&mut bar, TermField::Roster(RosterField::Ship), Some(Scope::Enemy));

        let click = first_pill_path(&bar);
        assert_eq!(click, vec![0], "the fixture must be the case where the path moves");
        with_ui(|ui, caret_id| bar.apply_command(Command::EditSegment(click, SegmentRole::Value), ui, caret_id));

        assert_eq!(bar.expr, parse_query("outcome:win").expect("parse"));
        let edit = bar.editing.as_ref().expect("the click must not be eaten by the restore it caused");
        assert_eq!(edit.path, Vec::<usize>::new(), "the editor must address the pill where it now is");
        assert_eq!(
            select::term_at(&bar.expr, &edit.path).map(|(field, _, _)| field),
            Some(TermField::Match(MatchField::Outcome))
        );
    }

    /// The other dismissal the user reaches for: clicking away from the bar
    /// rather than pressing Escape. Driven through real frames, because the
    /// route is `show`'s own unfocused branch.
    #[test]
    fn a_minted_pill_is_withdrawn_when_focus_leaves_the_bar() {
        const WIDTH: f32 = 800.0;
        let mut harness = Harness::new(MatchExpr::default(), WIDTH);
        let caret_id = harness.caret_id();
        harness.ctx.memory_mut(|m| m.request_focus(caret_id));
        harness.frame(frame_input(WIDTH));

        let bar = &mut harness.bar;
        mint_from_caret(bar, TermField::Match(MatchField::Outcome), None);

        harness.ctx.memory_mut(egui::Memory::stop_text_input);
        harness.frame(frame_input(WIDTH));
        assert!(
            harness.bar.expr.is_empty_all(),
            "focus leaving the bar left the placeholder behind: {:?}",
            harness.bar.expr
        );
        assert!(harness.bar.editing.is_none());
    }

    /// The path a picked row lands on, which decides which term the value
    /// editor opens. Appending to an empty query must not build a one-condition
    /// group, and appending to a query that already has terms must address the
    /// new one rather than the first.
    #[test]
    fn the_appended_path_names_the_term_that_was_just_added() {
        let mut expr = MatchExpr::default();
        select::append_top_level(&mut expr, win());
        select::canonicalise(&mut expr);
        assert_eq!(select::last_appended_path(&expr), Vec::<usize>::new(), "{expr:?}");

        select::append_top_level(&mut expr, roster_pill(RosterField::Tier, Op::Eq, Value::Int(10)));
        select::canonicalise(&mut expr);
        assert_eq!(select::last_appended_path(&expr), vec![1], "{expr:?}");
        assert_eq!(
            select::term_at(&expr, &select::last_appended_path(&expr)).map(|(field, _, _)| field),
            Some(TermField::Roster(RosterField::Tier))
        );
    }

    /// The path of the sole pill in the bar, read off the token stream rather
    /// than assumed, so a fixture that stopped drawing one pill fails here rather
    /// than testing a path nothing points at.
    fn only_pill_path(bar: &QueryBar) -> NodePath {
        let mut pills = tokenize(&bar.expr, &bar.names)
            .into_iter()
            .filter(|token| matches!(token.kind, TokenKind::Pill { .. }))
            .map(|token| token.path);
        let path = pills.next().expect("the fixture draws a pill");
        assert_eq!(pills.next(), None, "the fixture must draw exactly one pill");
        path
    }

    fn bar_holding(query: &str) -> QueryBar {
        let mut bar = QueryBar::default();
        bar.set_expr(parse_query(query).expect("parse"));
        bar
    }

    /// A single click opens the clicked segment's own dropdown, over that
    /// segment's rows.
    #[test]
    fn a_single_click_opens_the_segment_dropdown() {
        let mut bar = bar_holding("outcome:win");
        let path = only_pill_path(&bar);
        let click = Command::EditSegment(path.clone(), SegmentRole::Value);
        with_ui(|ui, caret_id| bar.apply_command(click, ui, caret_id));

        let edit = bar.editing.as_ref().expect("a single click opens the segment editor");
        assert_eq!(edit.path, path);
        assert_eq!(edit.role, SegmentRole::Value);
        let rows = bar.dropdown_rows();
        assert!(!rows.is_empty(), "the segment editor opened over an empty list");
        assert!(
            rows.iter().all(|row| matches!(row, Row::Literal(_))),
            "the dropdown must be the value segment's own list: {rows:?}"
        );
    }

    /// `caret_rect` has to find the caret wherever it stands, not assume it is
    /// the last placed token: `move_caret_to` can put it in the middle of the
    /// stream, and a lookup that fell back to "whatever is last" would measure
    /// the wrong token's rect.
    #[test]
    fn caret_rect_finds_the_caret_by_kind_rather_than_by_last_placement() {
        use crate::ui::query_bar::tokens::move_caret_to;

        let expr = Expr::All(vec![win(), roster_pill(RosterField::Tier, Op::Eq, Value::Int(10))]);
        let mut toks = tokenize(&expr, &NameCache::default());
        move_caret_to(&mut toks, &vec![0]).expect("the first pill must be replaced");
        let caret_index =
            toks.iter().position(|t| matches!(t.kind, TokenKind::Caret)).expect("the stream must still hold a caret");
        assert_ne!(caret_index, toks.len() - 1, "the fixture must leave the caret somewhere other than last");

        let cfg = LayoutCfg {
            row_height: 20.0,
            gap: 4.0,
            indent: 8.0,
            max_rows: 6,
            min_segment_width: 0.0,
            segment_gap: 4.0,
        };
        let widths = vec![50.0; toks.len()];
        let laid = lay_out(&toks, &widths, 500.0, &cfg);

        let rect = caret_rect(&toks, &laid, egui::Vec2::ZERO);
        let expected = laid.placed[caret_index].rect.shrink2(egui::vec2(0.0, ROW_PAD_Y));
        assert_eq!(rect, expected, "caret_rect did not find the caret standing in the pill's slot");

        let last_rect = laid.placed.last().expect("a placed token").rect.shrink2(egui::vec2(0.0, ROW_PAD_Y));
        assert_ne!(rect, last_rect, "caret_rect fell back to whatever was placed last");
    }

    /// `end_segment_edit`'s third gate: a forced editor's snapshot is put back
    /// only when doing so would actually change the tree. A snapshot that
    /// already matches the current expression must close the editor without
    /// reporting a change or touching `expr`.
    #[test]
    fn dismissing_an_editor_whose_snapshot_already_matches_reports_no_change() {
        let mut bar = QueryBar::default();
        bar.set_expr(win());
        let restore = Restore { before: bar.expr.clone(), minted: None };
        bar.editing = Some(SegmentEdit::open(&bar.expr, vec![], vec![], SegmentRole::Value, Some(restore), ""));

        let dismissed = bar.end_segment_edit();

        assert_eq!(dismissed, Dismissal::default(), "a snapshot already matching the tree must report nothing");
        assert_eq!(bar.expr, win(), "the tree must be untouched");
        assert!(bar.editing.is_none(), "the editor still closes");
    }

    /// Width the inline-editing fixtures run at: wide enough that a mid-bar
    /// segment sits well clear of the bar's left edge and well clear of the end
    /// of the bar, which is where value entry used to land.
    const INLINE_WIDTH: f32 = 800.0;

    /// A bar over a parsed query, focused and settled, so its segment rects can
    /// be read back and clicked.
    fn bar_with(text: &str) -> Harness {
        let expr = parse_query(text).unwrap_or_else(|error| panic!("the fixture {text:?} must parse: {error:?}"));
        let mut harness = Harness::new(expr, INLINE_WIDTH);
        harness.ctx.memory_mut(|m| m.request_focus(harness.caret_id()));
        harness.settle();
        harness
    }

    impl Harness {
        /// The stream the bar is drawing right now, which is what the token
        /// indices its widget ids are salted with count over.
        fn tokens(&self) -> Vec<Token> {
            self.bar.stream().0
        }

        /// The index of the pill at `path` in that stream, and its segments.
        ///
        /// A pill holding an inline box has left the stream, replaced in place
        /// by the caret, so its index -- and with it the ids its segment targets
        /// are salted with -- is the caret's.
        fn pill(&self, path: &NodePath) -> (usize, Vec<label::PillSegment>) {
            let (tokens, replaced) = self.bar.stream();
            if let Some(TokenKind::Pill { segments }) = replaced.filter(|pill| pill.path == *path).map(|pill| pill.kind)
            {
                let index =
                    tokens.iter().position(|t| matches!(t.kind, TokenKind::Caret)).expect("the stream keeps a caret");
                return (index, segments);
            }
            tokens
                .iter()
                .enumerate()
                .find_map(|(i, token)| match &token.kind {
                    TokenKind::Pill { segments } if token.path == *path => Some((i, segments.clone())),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("no pill at {path:?} in {tokens:?}"))
        }

        /// The click target the bar registered for one segment of one pill.
        fn segment_rect(&self, path: &NodePath, role: SegmentRole) -> Rect {
            let (index, segments) = self.pill(path);
            let seg = segments
                .iter()
                .position(|segment| segment.role == role)
                .unwrap_or_else(|| panic!("the pill at {path:?} draws no {role:?} segment"));
            self.rect_of(self.id.with("segment").with(index).with(seg))
        }

        /// The value segment of the one pill a single-term query draws, whatever
        /// path that pill has under whatever wraps it.
        fn only_value_segment_rect(&self) -> Rect {
            let tokens = self.tokens();
            let paths: Vec<NodePath> = tokens
                .iter()
                .filter(|token| matches!(token.kind, TokenKind::Pill { .. }))
                .map(|token| token.path.clone())
                .collect();
            let [path] = paths.as_slice() else {
                panic!("the fixture must draw exactly one pill, got {paths:?}");
            };
            self.segment_rect(path, SegmentRole::Value)
        }

        /// Whether a segment's own text was painted inside the bar this frame.
        /// Restricted to the bar's own area, so the dropdown -- which hangs
        /// below it and can carry the same words -- cannot answer for it.
        fn segment_is_drawn(&self, path: &NodePath, role: SegmentRole) -> bool {
            // Off the full tokenization, not the drawn stream: a pill holding an
            // inline box has left the drawn stream, and its other segments being
            // painted anyway is the whole question.
            let tokens = tokenize(&self.bar.expr, &self.bar.names);
            let segments = tokens
                .iter()
                .find_map(|token| match &token.kind {
                    TokenKind::Pill { segments } if token.path == *path => Some(segments.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("no pill at {path:?} in {tokens:?}"));
            let Some(segment) = segments.iter().find(|segment| segment.role == role) else {
                return false;
            };
            let bar = self.rect_of(self.id.with("background"));
            self.shapes.iter().any(|clipped| match &clipped.shape {
                egui::Shape::Text(text) => text.galley.job.text == segment.text && bar.contains(text.pos),
                _ => false,
            })
        }

        fn caret_rect(&self) -> Rect {
            self.rect_of(self.caret_id())
        }

        /// Whether a frame in the engaged accent was painted around `rect` this
        /// frame. Matched on the horizontal span, which is what tells the box's
        /// own frame from the bar's focus border and from a selected pill's
        /// outline: all three carry the same stroke, and only this one spans one
        /// segment.
        ///
        /// The accent is read off the style actually in force, not off
        /// `dark_style`. They agree in a default `Context` and part company the
        /// moment the app installs its own theme, which would make this read a
        /// colour nothing on screen uses and quietly answer `false` forever.
        fn is_framed(&self, rect: Rect) -> bool {
            let theme = self.ctx.theme();
            let accent = self.ctx.style_of(theme).visuals.selection.stroke.color;
            self.shapes.iter().any(|clipped| match &clipped.shape {
                egui::Shape::Rect(shape) => {
                    shape.stroke.color == accent
                        && shape.stroke.width > 0.0
                        && (shape.rect.left() - rect.left()).abs() < 1.0
                        && (shape.rect.right() - rect.right()).abs() < 1.0
                }
                _ => false,
            })
        }

        fn caret_text(&self) -> String {
            self.bar.pending.clone()
        }

        fn query(&self) -> MatchExpr {
            self.bar.expr.clone()
        }

        fn inline_edit_open(&self) -> bool {
            self.bar.inline_edit().is_some()
        }

        fn dropdown_open(&self) -> bool {
            self.bar.dropdown_open && !self.bar.dropdown_rows().is_empty()
        }

        fn has_error_mark(&self) -> bool {
            self.bar.pending_error.is_some()
        }

        fn click(&mut self, pos: Pos2) {
            self.frame(click_input(pos, self.width));
            self.settle();
        }

        /// Replaces what the box holds with `text`. The box opens with its
        /// cursor at the end of the value it was seeded with, so the seed is
        /// erased key by key first, which is the gesture a user makes.
        fn type_text(&mut self, text: &str) {
            let mut input = frame_input(self.width);
            for _ in 0..self.caret_text().chars().count() {
                push_key(&mut input, Key::Backspace);
            }
            input.events.push(egui::Event::Text(text.to_owned()));
            self.frame(input);
            self.settle();
            assert_eq!(self.caret_text(), text, "the box did not take the typed text");
        }

        fn press_enter(&mut self) {
            self.press(Key::Enter);
        }

        fn press_escape(&mut self) {
            self.press(Key::Escape);
        }

        fn press(&mut self, key: Key) {
            let mut input = frame_input(self.width);
            push_key(&mut input, key);
            self.frame(input);
            self.settle();
        }
    }

    fn push_key(input: &mut egui::RawInput, key: Key) {
        for pressed in [true, false] {
            input.events.push(egui::Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: Modifiers::NONE,
            });
        }
    }

    #[test]
    fn clicking_a_value_segment_opens_an_inline_box_in_its_own_slot() {
        // The field and operator segments stay drawn; only the value is replaced.
        let mut h = bar_with("anyone.tier>=8");
        let value = h.segment_rect(&vec![], SegmentRole::Value);
        h.click(value.center());
        let caret = h.caret_rect();
        assert!((caret.left() - value.left()).abs() < 2.0, "caret {caret:?} not in the value slot {value:?}");
        assert!(h.segment_is_drawn(&vec![], SegmentRole::Filter), "the field segment stopped being drawn");
        assert!(h.segment_is_drawn(&vec![], SegmentRole::Operator), "the operator segment stopped being drawn");
    }

    #[test]
    fn the_inline_box_is_seeded_with_the_values_current_text() {
        let mut h = bar_with("anyone.tier>=8");
        h.click(h.segment_rect(&vec![], SegmentRole::Value).center());
        assert_eq!(h.caret_text(), "8");
    }

    #[test]
    fn committing_an_inline_edit_changes_only_that_value() {
        let mut h = bar_with("outcome:win and anyone.tier>=8");
        h.click(h.segment_rect(&vec![1], SegmentRole::Value).center());
        h.type_text("10");
        h.press_enter();
        assert_eq!(h.query(), parse_query("outcome:win and anyone.tier>=10").expect("parse"));
    }

    #[test]
    fn escape_reverts_an_inline_edit() {
        let mut h = bar_with("anyone.tier>=8");
        h.click(h.segment_rect(&vec![], SegmentRole::Value).center());
        h.type_text("999");
        h.press_escape();
        assert_eq!(h.query(), parse_query("anyone.tier>=8").expect("parse"));
    }

    #[test]
    fn an_unparseable_inline_edit_keeps_the_box_open_and_marks_it() {
        let _zone = pinned_utc();
        let mut h = bar_with("date>=2026-07-23");
        h.click(h.segment_rect(&vec![], SegmentRole::Value).center());
        h.type_text("not-a-date");
        h.press_enter();
        assert_eq!(h.query(), parse_query("date>=2026-07-23").expect("parse"), "the term was destroyed");
        assert!(h.has_error_mark(), "no error was shown");
        assert!(h.inline_edit_open(), "the box closed on an unparseable value");
    }

    #[test]
    fn an_inline_edit_under_a_not_keeps_the_negation() {
        // The retired whole-pill editor lost this. Editing in place must not.
        let mut h = bar_with("not anyone.tier>=8");
        h.click(h.only_value_segment_rect().center());
        h.type_text("10");
        h.press_enter();
        assert_eq!(h.query(), parse_query("not anyone.tier>=10").expect("parse"));
    }

    /// Only the value segment gains an inline box; the other two keep opening
    /// their own list.
    ///
    /// The list has to be identified by the rows in it, not by the dropdown
    /// merely being open. The caret's own suggestion list is sixty-five rows and
    /// is open on almost every frame, so "a dropdown appeared" is satisfied by a
    /// click that opened no editor at all -- which is exactly what a segment
    /// whose target went missing would do.
    #[test]
    fn the_operator_and_filter_segments_still_open_their_lists() {
        for role in [SegmentRole::Filter, SegmentRole::Operator] {
            let mut h = bar_with("anyone.tier>=8");
            h.click(h.segment_rect(&vec![], role).center());

            assert_eq!(
                h.bar.editing.as_ref().map(|edit| edit.role),
                Some(role),
                "clicking the {role:?} segment opened no editor of its own"
            );
            assert!(!h.inline_edit_open(), "the {role:?} segment opened a text box");
            assert!(h.dropdown_open(), "the {role:?} list did not open");

            let rows = h.bar.dropdown_rows();
            let own = rows.iter().all(|row| match role {
                SegmentRole::Filter => matches!(row, Row::Field(_)),
                SegmentRole::Operator => matches!(row, Row::Operator(_)),
                SegmentRole::Value => false,
            });
            assert!(own, "the {role:?} segment offered the caret's own list rather than its own: {rows:?}");
        }
    }

    /// The complaint the whole change answers, stated as geometry. Value entry
    /// used to seed the caret at the end of the bar -- some 340px from the pill
    /// on a wide window -- which is why the feature read as a query language
    /// rather than as pills, however correct its commit semantics were.
    ///
    /// Containment, not just a shared left edge: the box has to be *in* the
    /// pill, and a caret that merely started at the same x while running to the
    /// bar's right edge would satisfy an alignment check on its own.
    #[test]
    fn the_box_is_inside_the_pill_rather_than_at_the_end_of_the_bar() {
        let mut h = bar_with("anyone.tier>=8");
        let pill = h.rect_of(h.id.with(h.pill(&vec![]).0));
        let bar = h.rect_of(h.id.with("background"));
        assert!(
            bar.right() - pill.right() > 200.0,
            "the fixture must leave the end of the bar far from the pill: {pill:?} in {bar:?}"
        );

        h.click(h.segment_rect(&vec![], SegmentRole::Value).center());

        let caret = h.caret_rect();
        assert!(pill.contains(caret.center()), "the box is not inside the pill: {caret:?} against {pill:?}");
        assert!(
            caret.right() <= pill.right() + 1.0,
            "the box runs past the pill it belongs to: {caret:?} against {pill:?}"
        );
    }

    /// The mark that says a segment is open for typing has to reach the screen,
    /// not merely exist as a function: without it the box is the pill's own fill
    /// with a cursor on it -- `(FIELD|OP|text)` -- which is the state the user
    /// said was not what they asked for.
    /// Both themes, driven through the project's own styles rather than egui's
    /// defaults. The accent parts company between the two, so a reader that goes
    /// to `dark_style` instead of the style in force answers for a colour
    /// nothing on screen uses -- and answers `false` forever in the light theme
    /// without ever failing in the dark one.
    #[test]
    fn an_open_box_is_marked_and_a_settled_pill_is_not() {
        for theme in [egui::Theme::Dark, egui::Theme::Light] {
            let mut h = bar_with("anyone.tier>=8");
            crate::ui::theme::install(&h.ctx);
            h.ctx.set_theme(theme);
            h.settle();

            let value = h.segment_rect(&vec![], SegmentRole::Value);
            assert!(!h.is_framed(value), "{theme:?}: nothing is being typed into, so no segment must be framed");

            h.click(value.center());

            let caret = h.caret_rect();
            assert!(h.is_framed(caret), "{theme:?}: an open box must be framed: {caret:?}");
        }
    }

    /// A segment beside the box is painted and washed on hover, so it advertises
    /// a click. Clicking it has to open that segment's own editor -- the pill is
    /// out of the drawn stream while the box is open, so nothing registers those
    /// targets unless the box's own paint pass does, and the click falls through
    /// to the bar's background and opens the caret's sixty-five-row field list
    /// instead.
    #[test]
    fn a_segment_beside_the_box_still_opens_its_own_list() {
        let mut h = bar_with("anyone.tier>=8");
        h.click(h.segment_rect(&vec![], SegmentRole::Value).center());
        assert!(h.inline_edit_open(), "the fixture must start with the box open");

        h.click(h.segment_rect(&vec![], SegmentRole::Operator).center());

        assert_eq!(
            h.bar.editing.as_ref().map(|edit| edit.role),
            Some(SegmentRole::Operator),
            "the operator segment beside the box opened no editor"
        );
        assert!(h.bar.dropdown_rows().iter().all(|row| matches!(row, Row::Operator(_))), "not the operator's list");
    }

    /// The pill keeps its own controls while its box is open, which is the third
    /// of the three things registering the boxed pill's targets restores -- the
    /// segments beside the box, the pill's menu, and this.
    ///
    /// A click on the gap between two segments belongs to neither of them and is
    /// where the separator is drawn, so on a settled pill it selects the pill.
    /// It has to do the same while a box is open, or the pill silently loses its
    /// selection, and with it the toolbar's group, negate, and delete.
    #[test]
    fn the_pill_holding_the_box_is_still_selectable() {
        let mut h = bar_with("anyone.tier>=8");
        h.click(h.segment_rect(&vec![], SegmentRole::Value).center());
        assert!(h.inline_edit_open(), "the fixture must start with the box open");

        let filter = h.segment_rect(&vec![], SegmentRole::Filter);
        let operator = h.segment_rect(&vec![], SegmentRole::Operator);
        let gap = Pos2::new(filter.right().midpoint(operator.left()), filter.center().y);
        assert!(
            !filter.contains(gap) && !operator.contains(gap),
            "the click must land in the gap, which belongs to the pill: {gap:?} between {filter:?} and {operator:?}"
        );

        h.click(gap);

        assert_eq!(h.bar.selection.nodes, vec![Vec::<usize>::new()], "the gap click did not reach the pill");
    }

    /// Moving from the box to another segment of the same pill is still moving
    /// on, so it commits for the same reason a click on the background does.
    #[test]
    fn moving_from_the_box_to_another_segment_commits_what_was_typed() {
        let mut h = bar_with("anyone.tier>=8");
        h.click(h.segment_rect(&vec![], SegmentRole::Value).center());
        h.type_text("10");
        h.click(h.segment_rect(&vec![], SegmentRole::Operator).center());
        assert_eq!(h.query(), parse_query("anyone.tier>=10").expect("parse"));
    }

    /// Mints a term the way picking a field from the caret's list does, leaving
    /// its value box forced open on `new_term`'s placeholder.
    ///
    /// The mint itself runs off-frame, since it is state rather than geometry;
    /// everything the test then measures or clicks goes through real frames.
    fn mint_forced_box(harness: &mut Harness, field: TermField) {
        let caret_id = harness.caret_id();
        let bar = &mut harness.bar;
        assert!(with_ui(|ui, _| bar.mint_term(field, None, ui, caret_id)), "the fixture must mint a term");
        assert!(
            bar.editing.as_ref().is_some_and(|edit| edit.restore.is_some()),
            "the fixture must leave a forced box, which is the case under test"
        );
        // `mint_term` parked the caret in `with_ui`'s throwaway context, not in
        // this one, so the box would open with its cursor at the front and text
        // typed into it would land before the placeholder rather than after it.
        let cursor = harness.bar.pending.chars().count();
        park_caret(&harness.ctx, caret_id, cursor);
        harness.ctx.memory_mut(|m| m.request_focus(caret_id));
        harness.settle();
        assert!(harness.bar.inline_edit().is_some(), "a quiet frame must not close the box");
    }

    /// The value in a minted pill's box is a real choice once it has been typed,
    /// so leaving the box by clicking away must keep it -- Enter in the same
    /// state already does, and the two cannot disagree about the same
    /// keystrokes.
    ///
    /// The snapshot a forced box carries exists to withdraw a pill the user
    /// picked and never filled in; it is not licence to destroy one they did.
    #[test]
    fn a_click_away_from_a_typed_minted_box_keeps_the_pill_and_the_value() {
        let mut h = bar_with("outcome:win");
        mint_forced_box(&mut h, TermField::Match(MatchField::Build));
        h.type_text("1234");

        let bar = h.rect_of(h.id.with("background"));
        let box_rect = h.caret_rect();
        let away = Pos2::new(bar.right() - 1.0, box_rect.center().y);
        assert!(!box_rect.contains(away), "the click must land off the box: {away:?} in {box_rect:?}");
        h.click(away);

        let Expr::All(terms) = h.query() else {
            panic!("the minted pill was withdrawn along with what was typed into it: {:?}", h.query());
        };
        assert_eq!(terms.len(), 2, "the minted pill must survive: {terms:?}");
        assert_eq!(terms[0], win(), "the term that was already there must be untouched");
        assert_eq!(
            select::term_at(&h.query(), &[1]).map(|(field, _, value)| (field, value.clone())),
            Some((TermField::Match(MatchField::Build), Value::Int(1234))),
            "the typed literal must be what the minted pill carries"
        );
    }

    /// The other half of that rule, and the reason the exclusion existed: a box
    /// still holding the placeholder is a field the user picked and never filled
    /// in, so leaving it withdraws the pill rather than committing a filter
    /// nobody chose.
    #[test]
    fn a_click_away_from_an_untouched_minted_box_still_withdraws_the_pill() {
        let mut h = bar_with("outcome:win");
        mint_forced_box(&mut h, TermField::Match(MatchField::Build));

        let bar = h.rect_of(h.id.with("background"));
        let box_rect = h.caret_rect();
        h.click(Pos2::new(bar.right() - 1.0, box_rect.center().y));

        assert_eq!(h.query(), win(), "an untouched placeholder must not be left behind as a filter");
    }

    /// The other half of the same rule Escape states: a click that moves on
    /// keeps what was typed. Escape is the dismissal that reverts, and it is
    /// the only one -- both reach the bar as a focus loss, so a `show` that
    /// cannot tell them apart gets one of the two wrong.
    ///
    /// Aimed at empty space at the end of the bar, well past the pill, which is
    /// the same background click that ends a segment edit.
    #[test]
    fn a_click_away_from_the_box_commits_what_was_typed() {
        let mut h = bar_with("anyone.tier>=8");
        h.click(h.segment_rect(&vec![], SegmentRole::Value).center());
        h.type_text("10");

        let bar = h.rect_of(h.id.with("background"));
        let box_rect = h.caret_rect();
        let away = Pos2::new(bar.right() - 1.0, box_rect.center().y);
        assert!(!box_rect.contains(away), "the click must land off the box: {away:?} in {box_rect:?}");
        h.click(away);

        assert_eq!(h.query(), parse_query("anyone.tier>=10").expect("parse"));
        assert!(!h.inline_edit_open(), "the box must close on a click that moved on");
    }
}
