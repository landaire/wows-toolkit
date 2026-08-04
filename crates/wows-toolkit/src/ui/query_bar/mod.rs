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
#[derive(Debug, Clone, PartialEq, Eq)]
struct SegmentEdit {
    /// Already resolved through `select::segment_path`, so it names the term
    /// the segment renders rather than the pill's own node.
    path: NodePath,
    role: SegmentRole,
}

/// Text a dropdown row puts into the caret, and how it ends.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Completion {
    /// A field's grammar prefix. No trailing space, because a value comes next
    /// and the caret has to stay inside the fragment it just started.
    Filter(String),
    /// A value literal, which finishes the term. The trailing space leaves
    /// `active_fragment` empty so the full list returns, and makes
    /// `value_request_for` yield `None` so no stale lookup fires.
    Value(String),
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
    /// Open one segment's editor. The path is already `segment_path`-resolved.
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
    /// A click landed somewhere in the bar this frame. The caret is only
    /// granted focus on the frame after, so this is what tells `show` the bar
    /// is engaged before `has_focus` agrees.
    clicked_inside: bool,
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

        self.refresh_suggestions();
        self.reparse_pending();

        // Only a focused bar reads keys, and the rows exist to answer them, so
        // an unfocused bar skips ranking the whole suggestion set every frame.
        let rows = if focused { self.dropdown_rows() } else { Vec::new() };
        let nav = if focused { self.consume_navigation(ui) } else { None };

        let mut tokens = tokenize(&self.expr, &self.names);
        self.selection.retain_present(&tokens);

        let mut edited = false;
        if let Some(nav) = nav {
            edited |= self.apply_nav(nav, &tokens, &rows, deps, ui, caret_id);
        }
        if edited {
            self.finish_edit();
            tokens = tokenize(&self.expr, &self.names);
        }

        let frame = paint::bar_frame(ui, focused);
        let framed = frame.show(ui, |ui| self.body(ui, id, caret_id, &tokens));
        let body = framed.inner;
        edited |= body.edited;

        // A click counts as engagement even on the frame the caret has not been
        // granted focus yet, which is the frame every first click lands on.
        // Reading `focused` alone there would close a dropdown that same click
        // had just opened.
        if focused || body.clicked_inside {
            // Any click that is not on a segment leaves whatever segment was
            // being edited: the pointer has moved on to the pill, the
            // background, or the caret.
            if body.clicked_inside && !body.clicked_segment {
                self.end_segment_edit();
            }
            if body.clicked_inside || body.caret_changed || body.caret_gained_focus || self.editing.is_some() {
                self.dropdown_open = true;
            }
        } else {
            self.end_segment_edit();
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

    /// Steps 4 through 9: measure, lay out, paint, run the caret, and collect
    /// what the pointer did.
    fn body(&mut self, ui: &mut Ui, id: egui::Id, caret_id: egui::Id, tokens: &[Token]) -> Body {
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
        let galleys: Vec<Vec<Arc<Galley>>> = tokens.iter().map(|token| token_galleys(ui, token, &font)).collect();
        let widths: Vec<f32> = tokens.iter().zip(&galleys).map(|(t, g)| token_width(t, g, &cfg)).collect();

        let full_width = ui.available_width();
        let mut laid = lay_out(tokens, &widths, full_width, &cfg);
        if laid.needs_scroll {
            laid = lay_out(tokens, &widths, (full_width - SCROLLBAR_ALLOWANCE).max(1.0), &cfg);
        }

        let ids = BarIds { id, caret_id };
        if laid.needs_scroll {
            egui::ScrollArea::vertical()
                .id_salt(id.with("scroll"))
                .max_height(cfg.max_rows as f32 * cfg.row_height)
                .auto_shrink([false, false])
                .show(ui, |ui| self.rows(ui, &ids, tokens, &galleys, &laid, &cfg))
                .inner
        } else {
            self.rows(ui, &ids, tokens, &galleys, &laid, &cfg)
        }
    }

    fn rows(
        &mut self,
        ui: &mut Ui,
        ids: &BarIds,
        tokens: &[Token],
        galleys: &[Vec<Arc<Galley>>],
        laid: &LaidOut,
        cfg: &LayoutCfg,
    ) -> Body {
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
            refocus |= response.clicked() || response.double_clicked();
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
                &mut commands,
            );
            refocus |= hit.clicked;
            clicked_segment |= hit.clicked;
            anchor = anchor.or(hit.anchor);
            paint::token(ui, rect, &galleys[placed.index], &token.kind, state, cfg);
            self.token_interaction(&response, token, &mut commands);
        }
        self.popup_anchor = anchor;

        let caret = self.caret(ui, ids.caret_id, laid, origin);
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

        let mut edited = false;
        for command in commands {
            edited |= self.apply_command(command, tokens, ui, ids.caret_id);
        }
        Body {
            edited,
            caret_changed: caret.response.response.changed(),
            caret_gained_focus: caret.response.response.gained_focus(),
            clicked_inside: refocus || caret.response.response.clicked(),
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
    fn segment_interaction(
        &self,
        ui: &Ui,
        id: egui::Id,
        token: &Token,
        rects: &[Rect],
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
            let response = ui.interact(*rect, id.with(i), Sense::click());
            // Also on the segment, not only on the pill: the segment sits on
            // top of it, so a right-click never reaches the pill's own menu.
            let expr = &self.expr;
            let pill_path = &token.path;
            response.context_menu(|ui| pill_menu(ui, expr, pill_path, commands));
            if response.clicked() {
                hit.clicked = true;
                commands.push(Command::EditSegment(path.clone(), segment.role));
            }
            if self.editing.as_ref().is_some_and(|edit| edit.path == path && edit.role == segment.role) {
                hit.anchor = Some(*rect);
            }
        }
        hit
    }

    /// Step 8: the caret is a real `TextEdit` sized to its `Placed` rect, so
    /// IME, selection, and the clipboard behave the way they do anywhere else.
    fn caret(
        &mut self,
        ui: &mut Ui,
        caret_id: egui::Id,
        laid: &LaidOut,
        origin: egui::Vec2,
    ) -> egui::text_edit::TextEditOutput {
        // `tokenize` always emits the caret last, and `lay_out` places tokens
        // in order.
        let placed = laid.placed.last().expect("lay_out places every token, and the caret is always one of them");
        let rect = placed.rect.translate(origin).shrink2(egui::vec2(0.0, ROW_PAD_Y));
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
            .width(bar_rect.width())
            .show(|ui| {
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
        // While a segment editor is open the caret is that editor's buffer, so
        // the keys that navigate the pill stream instead -- history, selection,
        // and stepping off the ends -- would act on a query the user is not
        // typing.
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

    fn apply_nav(
        &mut self,
        nav: Nav,
        tokens: &[Token],
        rows: &[Row],
        deps: &Deps<'_>,
        ui: &mut Ui,
        caret_id: egui::Id,
    ) -> bool {
        let paths = select::selectable_paths(&self.expr, tokens);
        match nav {
            Nav::CloseDropdown => {
                self.end_segment_edit();
                self.dropdown_open = false;
                self.highlighted = None;
                false
            }
            Nav::ReleaseFocus => {
                ui.memory_mut(|m| m.surrender_focus(caret_id));
                self.end_segment_edit();
                self.selection.clear();
                false
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

    fn apply_command(&mut self, command: Command, tokens: &[Token], ui: &Ui, caret_id: egui::Id) -> bool {
        match command {
            Command::EditSegment(path, role) => {
                self.begin_segment_edit(path, role, ui, caret_id);
                false
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
                let paths = select::selectable_paths(&self.expr, tokens);
                let anchor = self.anchor.clone().unwrap_or_else(|| path.clone());
                self.selection.set_many(select::range(&paths, &anchor, &path));
                self.anchor = Some(anchor);
                self.focus = Some(path);
                false
            }
            Command::Negate(path) => {
                select::negate(&mut self.expr, &path);
                true
            }
            Command::Delete(path) => {
                select::delete(&mut self.expr, &Selection { nodes: vec![path] });
                true
            }
            Command::SetConnector(path, is_or) => {
                select::set_connector(&mut self.expr, &path, is_or);
                true
            }
            Command::Ungroup(path) => select::ungroup(&mut self.expr, &path),
        }
    }

    /// Opens one segment's editor, reporting whether it opened. A value segment
    /// its term does not draw -- a nullary operator has no right-hand side --
    /// has nothing to edit and is refused rather than opened on nothing.
    fn begin_segment_edit(&mut self, path: NodePath, role: SegmentRole, ui: &Ui, caret_id: egui::Id) -> bool {
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
        self.editing = Some(SegmentEdit { path, role });
        self.dropdown_open = true;
        let request = self.segment_request();
        self.refresh_request(request);
        park_caret(ui, caret_id, self.pending.chars().count());
        true
    }

    /// Ends the open segment edit and discards the caret buffer it owned. A
    /// no-op when nothing is being edited, so it never throws away typed query
    /// text.
    fn end_segment_edit(&mut self) {
        if self.editing.is_some() {
            self.clear_pending();
        }
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
                // Replaces the half-typed value the row was picked against;
                // appending would give `enemy.ship:yamYamato`.
                self.complete(Completion::Value(token), ui, caret_id);
                false
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
    /// anything changed, so the `value_kind` comparison is what says the old
    /// value did not survive and was replaced by an arbitrary in-kind
    /// placeholder. That placeholder is not a choice the user made, so the
    /// value editor opens on it here rather than leaving it as something that
    /// could be committed unseen -- for `Ship` and `Account` it is a zero id
    /// that would render as an unresolvable pill.
    fn commit_field_change(&mut self, field: TermField, ui: &Ui, caret_id: egui::Id) -> bool {
        let Some(edit) = self.editing.clone() else {
            return false;
        };
        let Some((current, _, _)) = select::term_at(&self.expr, &edit.path) else {
            return false;
        };
        let previous_kind = current.value_kind();
        if !select::set_field(&mut self.expr, &edit.path, field) {
            return false;
        }
        if field.value_kind() != previous_kind && self.begin_segment_edit(edit.path, SegmentRole::Value, ui, caret_id) {
            return true;
        }
        self.end_segment_edit();
        self.dropdown_open = true;
        park_caret(ui, caret_id, 0);
        true
    }

    fn commit_operator(&mut self, op: Op) -> bool {
        let Some(edit) = self.editing.clone() else {
            return false;
        };
        if !select::set_op(&mut self.expr, &edit.path, op) {
            return false;
        }
        self.end_segment_edit();
        true
    }

    /// Applies a grammar literal to the pill's value, whether it came from a
    /// row or from the caret. The literal goes through the grammar's own parser
    /// rather than a second reader of the same spellings, so a clicked value
    /// and a typed one become the same `Value`.
    ///
    /// A literal the field cannot read is reported the way the typed path
    /// reports one, through `pending_error` and the underline it drives.
    /// `reparse_pending` clears it on the next keystroke, so the mark follows
    /// the text rather than outlasting it.
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
        if !select::set_value(&mut self.expr, &edit.path, value) {
            return false;
        }
        self.end_segment_edit();
        true
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
            self.end_segment_edit();
            return false;
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
                park_caret(ui, caret_id, 0);
                true
            }
            SuggestionKind::MatchField(field) => {
                self.begin_field(suggest::match_field_prefix(field), ui, caret_id);
                false
            }
            SuggestionKind::RosterField { field, scope } => {
                // A roster field name is only reachable through a scope prefix
                // or a quantifier, so an unscoped one takes the widest scope
                // rather than emitting text the grammar cannot read back.
                self.begin_field(suggest::roster_field_prefix(field, scope.unwrap_or(Scope::Anyone)), ui, caret_id);
                false
            }
        }
    }

    /// Puts a field's grammar prefix in the caret and leaves the user on the
    /// value. Only the fragment being typed is replaced, so committing a
    /// suggestion part way through a query keeps the terms already there.
    fn begin_field(&mut self, prefix: Option<String>, ui: &Ui, caret_id: egui::Id) {
        let Some(prefix) = prefix else {
            return;
        };
        self.complete(Completion::Filter(prefix), ui, caret_id);
    }

    /// Writes a completion into the caret and parks the cursor after it.
    fn complete(&mut self, completion: Completion, ui: &Ui, caret_id: egui::Id) {
        let (text, cursor) = completed_text(&self.pending, &completion);
        self.pending = text;
        self.parsed_text.clear();
        self.dropdown_open = true;
        self.highlighted = None;
        park_caret(ui, caret_id, cursor);
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
                park_caret(ui, caret_id, 0);
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
        let request = value_request_for(&self.pending);
        self.refresh_request(request);
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

/// The caret text a completion produces, and the character index the caret
/// parks at.
///
/// A character index, never a byte offset: `CCursor::index` is a `CharIndex`,
/// and the text counted here is arbitrary user input that a byte length would
/// misplace the caret in the moment it contains anything outside ASCII.
fn completed_text(pending: &str, completion: &Completion) -> (String, usize) {
    let text = match completion {
        Completion::Filter(prefix) => suggest::replace_active_fragment(pending, prefix),
        Completion::Value(literal) => format!("{} ", suggest::replace_active_value(pending, literal)),
    };
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

/// Parks the caret at `cursor`, a character index. egui otherwise keeps
/// whatever offset the widget had, which after a programmatic replacement of
/// the text is arbitrary.
fn park_caret(ui: &Ui, caret_id: egui::Id, cursor: usize) {
    let mut state = TextEditState::load(ui.ctx(), caret_id).unwrap_or_default();
    state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(cursor))));
    state.store(ui.ctx(), caret_id);
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
        TokenKind::Caret => run(String::new()),
    }
}

fn token_width(token: &Token, galleys: &[Arc<Galley>], cfg: &LayoutCfg) -> f32 {
    match &token.kind {
        TokenKind::Caret => MIN_CARET_WIDTH,
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
    use super::*;
    use crate::db::index::query_ast::Expr;
    use crate::db::index::query_ast::MatchField;
    use crate::db::index::query_ast::MatchTerm;
    use crate::db::index::query_ast::Quant;
    use crate::db::index::query_ast::RosterField;
    use crate::db::index::query_ast::RosterTerm;
    use crate::db::index::query_ast::Value;
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
        bar.editing = Some(SegmentEdit { path, role });
        bar
    }

    fn roster_pill(field: RosterField, op: Op, value: Value) -> MatchExpr {
        Expr::Leaf(MatchTerm::Roster { quant: Quant::Any, pred: Expr::Leaf(RosterTerm { field, op, value }) })
    }

    #[test]
    fn a_filter_completion_leaves_the_caret_tight_after_the_prefix() {
        let (text, cursor) = completed_text("outcome:win bui", &Completion::Filter("build:".to_owned()));
        assert_eq!(text, "outcome:win build:");
        assert!(!text.ends_with(' '), "a value comes next, so the term must not be closed: {text:?}");
        assert_eq!(cursor, text.chars().count());
    }

    #[test]
    fn a_value_completion_closes_the_term_and_puts_the_caret_after_the_space() {
        let (text, cursor) = completed_text("outcome:wi", &Completion::Value("win".to_owned()));
        assert_eq!(text, "outcome:win ");
        assert_eq!(cursor, text.chars().count());
        assert!(suggest::active_fragment(&text).is_empty(), "the next fragment must start empty: {text:?}");
        assert!(value_request_for(&text).is_none(), "a closed term must not leave a value lookup pending");
    }

    /// The brief's own case, arranged so the multi-byte characters survive into
    /// the completed text. `CCursor::index` is a character index, so a caret
    /// derived from `str::len` overshoots the end of the string here.
    #[test]
    fn a_completion_over_multi_byte_text_places_the_caret_by_character() {
        let (text, cursor) = completed_text("map:caf\u{e9} aa\u{e9}b", &Completion::Filter("build:".to_owned()));
        assert_eq!(text, "map:caf\u{e9} build:");
        assert_eq!(cursor, text.chars().count());
        assert!(cursor < text.len(), "a byte offset would overshoot: {cursor} against {} bytes", text.len());
    }

    /// The inserted literal is itself multi-byte, so a caret counted in bytes
    /// overshoots even when everything before it is ASCII.
    #[test]
    fn a_multi_byte_completion_value_places_the_caret_by_character() {
        let (text, cursor) = completed_text("map:oc", &Completion::Value("\"nord\u{e9}\"".to_owned()));
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
    /// alters the value kind replaces the value with an arbitrary in-kind
    /// placeholder, which for `Ship` and `Account` is a zero id that renders as
    /// an unresolvable pill. The editor has to open on it, so zero is never a
    /// state the user can rest on.
    #[test]
    fn a_field_change_that_resets_the_value_opens_the_value_editor() {
        let mut bar = bar_editing(roster_pill(RosterField::Tier, Op::Eq, Value::Int(10)), SegmentRole::Filter);
        let previous = bar.expr.clone();
        assert!(select::set_field(&mut bar.expr, &vec![], TermField::Roster(RosterField::Ship)));
        assert_ne!(bar.expr, previous);
        // What the routing does with that, minus the egui half: the kind moved,
        // so the value segment is what must be open next.
        assert_eq!(TermField::Roster(RosterField::Ship).value_kind(), crate::db::index::query_ast::ValueKind::Ship);
        assert_ne!(
            TermField::Roster(RosterField::Tier).value_kind(),
            TermField::Roster(RosterField::Ship).value_kind()
        );

        bar.editing = Some(SegmentEdit { path: vec![], role: SegmentRole::Value });
        assert_eq!(bar.segment_request(), Some(ValueRequest::Ships { needle: String::new() }));
    }

    /// A field change that keeps the value kind has nothing to re-choose, so
    /// the value editor must not be forced open on a value the user still has.
    #[test]
    fn a_field_change_that_keeps_the_value_kind_leaves_the_value_alone() {
        let mut expr = roster_pill(RosterField::Tier, Op::Eq, Value::Int(10));
        assert!(select::set_field(&mut expr, &vec![], TermField::Roster(RosterField::Kills)));
        let (field, _, value) = select::term_at(&expr, &[]).expect("the retargeted term");
        assert_eq!(field, TermField::Roster(RosterField::Kills));
        assert_eq!(*value, Value::Int(10), "a value the new field can carry is kept");
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
}
