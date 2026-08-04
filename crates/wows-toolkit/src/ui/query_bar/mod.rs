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
use egui::Rect;
use egui::Sense;
use egui::Ui;
use egui::UiBuilder;
use rust_i18n::t;

use crate::db::index::query_ast::MatchExpr;
use crate::db::index::query_ast::Op;
use crate::db::index::query_text::QueryParseError;
use crate::db::index::query_text::parse_query;
use crate::db::index::query_text::print_query;
use crate::ui::query_bar::label::NameCache;
use crate::ui::query_bar::layout::LaidOut;
use crate::ui::query_bar::layout::LayoutCfg;
use crate::ui::query_bar::layout::lay_out;
use crate::ui::query_bar::paint::TokenState;
use crate::ui::query_bar::select::Selection;
use crate::ui::query_bar::suggest::PRESETS;
use crate::ui::query_bar::suggest::Scope;
use crate::ui::query_bar::suggest::Suggestion;
use crate::ui::query_bar::suggest::SuggestionCategory;
use crate::ui::query_bar::suggest::SuggestionKind;
use crate::ui::query_bar::suggest::ValueOption;
use crate::ui::query_bar::suggest::ValueRequest;
use crate::ui::query_bar::suggest::rank;
use crate::ui::query_bar::suggest::static_suggestions;
use crate::ui::query_bar::suggest::value_request_for;
use crate::ui::query_bar::tokens::NodePath;
use crate::ui::query_bar::tokens::Token;
use crate::ui::query_bar::tokens::TokenKind;
use crate::ui::query_bar::tokens::node_at;
use crate::ui::query_bar::tokens::tokenize;
use crate::ui::theme::semantic::SemanticExt;

/// Rows the bar shows before it scrolls internally instead of growing.
const MAX_ROWS: usize = 6;
const ROW_GAP: f32 = 4.0;
const DEPTH_INDENT: f32 = 10.0;
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    /// An index into `QueryBar::suggestions`, as `rank` returns them.
    Suggestion(usize),
    /// An index into `QueryBar::value_options`.
    Value(usize),
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
    EditAsText(NodePath),
    SetOp(NodePath, Op),
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

        if body.caret_changed || body.caret_gained_focus {
            self.dropdown_open = true;
        }
        if !focused {
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
        let galleys: Vec<Arc<Galley>> = tokens
            .iter()
            .map(|token| ui.painter().layout_no_wrap(token_text(token), font.clone(), egui::Color32::PLACEHOLDER))
            .collect();
        let widths: Vec<f32> = tokens.iter().zip(&galleys).map(|(t, g)| token_width(t, g)).collect();

        let cfg = LayoutCfg {
            row_height: ui.spacing().interact_size.y + 2.0 * ROW_PAD_Y,
            gap: ROW_GAP,
            indent: DEPTH_INDENT,
            max_rows: MAX_ROWS,
        };
        let full_width = ui.available_width();
        let mut laid = lay_out(tokens, &widths, full_width, &cfg);
        if laid.needs_scroll {
            laid = lay_out(tokens, &widths, (full_width - SCROLLBAR_ALLOWANCE).max(1.0), &cfg);
        }

        if laid.needs_scroll {
            egui::ScrollArea::vertical()
                .id_salt(id.with("scroll"))
                .max_height(cfg.max_rows as f32 * cfg.row_height)
                .auto_shrink([false, false])
                .show(ui, |ui| self.rows(ui, id, caret_id, tokens, &galleys, &laid))
                .inner
        } else {
            self.rows(ui, id, caret_id, tokens, &galleys, &laid)
        }
    }

    fn rows(
        &mut self,
        ui: &mut Ui,
        id: egui::Id,
        caret_id: egui::Id,
        tokens: &[Token],
        galleys: &[Arc<Galley>],
        laid: &LaidOut,
    ) -> Body {
        let (_, area) = ui.allocate_space(egui::vec2(ui.available_width(), laid.height));
        let origin = area.min.to_vec2();

        // Registered before the tokens so a click on a pill wins the hit test
        // and only a click on the gaps falls through to focusing the caret.
        let background = ui.interact(area, id.with("background"), Sense::click());

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
        for placed in &laid.placed {
            let token = &tokens[placed.index];
            if matches!(token.kind, TokenKind::Caret) {
                continue;
            }
            let rect = placed.rect.translate(origin).shrink2(egui::vec2(0.0, ROW_PAD_Y));
            let response = ui.interact(rect, id.with(placed.index), Sense::click());
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
            paint::token(ui, rect, galleys[placed.index].clone(), &token.kind, state);
            self.token_interaction(&response, token, &mut commands);
        }

        let caret = self.caret(ui, caret_id, laid, origin);
        // After the caret, never before: egui surrenders focus from any focused
        // widget the pointer is not over as that widget is created, so a
        // request made earlier in the frame is undone by the caret's own
        // creation and the click reads as unfocused on the very next frame.
        // That in turn skips `consume_navigation` entirely, which is what makes
        // Backspace-deletes-the-selection and Shift+Arrow-extends unreachable
        // from a selection made with the mouse.
        if refocus {
            ui.memory_mut(|m| m.request_focus(caret_id));
        }

        let mut edited = false;
        for command in commands {
            edited |= self.apply_command(command, tokens);
        }
        Body {
            edited,
            caret_changed: caret.response.response.changed(),
            caret_gained_focus: caret.response.response.gained_focus(),
        }
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
                if response.double_clicked() {
                    commands.push(Command::EditAsText(token.path.clone()));
                } else if response.clicked() {
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
            // dissolve; its quantifier changes through negation or by editing
            // the pill as text.
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

    /// Step 4 of the dropdown: the suggestion list, anchored under the bar.
    fn dropdown(&mut self, ui: &mut Ui, id: egui::Id, caret_id: egui::Id, bar_rect: Rect) -> bool {
        if !self.dropdown_open {
            return false;
        }
        let rows = self.dropdown_rows();
        // A bar with nothing typed into its active fragment says nothing by
        // staying shut, rather than by reporting that nothing matches an empty
        // needle.
        if rows.is_empty() && suggest::active_fragment(&self.pending).trim().is_empty() {
            return false;
        }
        // Typing narrows the list under a highlight that was set against the
        // longer one, so an index left dangling would swallow the next Enter.
        if self.highlighted.is_some_and(|i| i >= rows.len()) {
            self.highlighted = None;
        }

        let mut picked = None;
        egui::Popup::new(id.with("dropdown"), ui.ctx().clone(), bar_rect, ui.layer_id())
            .open(true)
            .align(egui::RectAlign::BOTTOM_START)
            .width(bar_rect.width())
            .show(|ui| {
                if rows.is_empty() {
                    ui.label(
                        egui::RichText::new(t!("ui.search.bar.no_suggestions").into_owned()).color(ui.sem().text_dim),
                    );
                    return;
                }
                egui::ScrollArea::vertical().max_height(MAX_DROPDOWN_HEIGHT).show(ui, |ui| {
                    for (index, row) in rows.iter().enumerate() {
                        let selected = self.highlighted == Some(index);
                        let response = ui.selectable_label(selected, self.row_text(ui, *row));
                        if selected {
                            response.scroll_to_me(None);
                        }
                        if response.clicked() {
                            picked = Some(*row);
                        }
                    }
                });
            });

        let Some(row) = picked else {
            return false;
        };
        let edited = self.commit_row(row);
        ui.memory_mut(|m| m.request_focus(caret_id));
        edited
    }

    fn row_text(&self, ui: &Ui, row: Row) -> egui::text::LayoutJob {
        let font = egui::TextStyle::Body.resolve(ui.style());
        let mut job = egui::text::LayoutJob::default();
        let (label, context) = match row {
            Row::Suggestion(i) => match self.suggestions.get(i) {
                Some(s) => (s.label.clone(), Some(category_label(s.context))),
                None => (String::new(), None),
            },
            Row::Value(i) => (self.value_options.get(i).map(|v| v.label.clone()).unwrap_or_default(), None),
        };
        job.append(&label, 0.0, egui::TextFormat::simple(font.clone(), ui.visuals().text_color()));
        if let Some(context) = context {
            job.append(&context, 8.0, egui::TextFormat::simple(font, ui.sem().text_dim));
        }
        job
    }

    /// The rows the dropdown shows. Value rows displace the static suggestions
    /// entirely: once the caret is typing a value, the field list is no longer
    /// what the user is choosing from.
    ///
    /// Ranking is against the active fragment, not the whole caret. `rank`
    /// matches its needle against suggestion labels, so the whole string stops
    /// matching anything the moment a second term is typed, and every
    /// suggestion becomes unreachable part way through a query.
    fn dropdown_rows(&self) -> Vec<Row> {
        if self.active_request.is_some() && !self.value_options.is_empty() {
            return (0..self.value_options.len()).map(Row::Value).collect();
        }
        let needle = suggest::active_fragment(&self.pending);
        rank(needle, &self.suggestions).into_iter().map(Row::Suggestion).collect()
    }

    /// Steps 1 and 2: read the caret position captured last frame, then take
    /// the navigation keys before the `TextEdit` can.
    fn consume_navigation(&self, ui: &mut Ui) -> Option<Nav> {
        let take = |modifiers, key| ui.input_mut(|i| i.consume_key(modifiers, key));

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
            let in_list = self.highlighted.is_some() || !self.pending.is_empty();
            return Some(if in_list { Nav::HighlightPrev } else { Nav::HistoryBack });
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
                self.dropdown_open = false;
                self.highlighted = None;
                false
            }
            Nav::ReleaseFocus => {
                ui.memory_mut(|m| m.surrender_focus(caret_id));
                self.selection.clear();
                false
            }
            Nav::CommitRow(index) => rows.get(index).copied().is_some_and(|row| self.commit_row(row)),
            Nav::CommitTyped => self.commit_pending(),
            Nav::HighlightNext => {
                if !rows.is_empty() {
                    self.highlighted = Some(self.highlighted.map_or(0, |i| (i + 1).min(rows.len() - 1)));
                    self.dropdown_open = true;
                }
                false
            }
            Nav::HighlightPrev => {
                self.highlighted = match self.highlighted {
                    None => rows.len().checked_sub(1),
                    Some(0) => None,
                    Some(i) => Some(i - 1),
                };
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

    fn apply_command(&mut self, command: Command, tokens: &[Token]) -> bool {
        match command {
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
            Command::EditAsText(path) => self.edit_as_text(&path),
            Command::SetOp(path, op) => select::set_op(&mut self.expr, &path, op),
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

    /// Lifts one pill back into the caret as the text it prints as, so its
    /// value or operator can be retyped. Refused for a pill that renders part
    /// of a roster predicate: the printed text would be the whole quantifier
    /// while the removal would address a node the match tree does not have.
    fn edit_as_text(&mut self, path: &NodePath) -> bool {
        if !select::addresses_match_node(&self.expr, path) {
            return false;
        }
        let Some(node) = node_at(&self.expr, path) else {
            return false;
        };
        let text = print_query(node);
        select::delete(&mut self.expr, &Selection { nodes: vec![path.clone()] });
        self.pending = if self.pending.trim().is_empty() { text } else { format!("{} {text}", self.pending) };
        self.parsed_text.clear();
        self.selection.clear();
        true
    }

    fn commit_row(&mut self, row: Row) -> bool {
        self.highlighted = None;
        match row {
            Row::Value(index) => {
                let Some(token) = self.value_options.get(index).map(|option| option.token.clone()) else {
                    return false;
                };
                // Replaces the half-typed value the row was picked against;
                // appending would give `enemy.ship:yamYamato`.
                self.pending = suggest::replace_active_value(&self.pending, &token);
                self.commit_pending()
            }
            Row::Suggestion(index) => {
                let Some(kind) = self.suggestions.get(index).map(|s| s.kind.clone()) else {
                    return false;
                };
                self.commit_suggestion(kind)
            }
        }
    }

    fn commit_suggestion(&mut self, kind: SuggestionKind) -> bool {
        match kind {
            SuggestionKind::Preset(key) => {
                let Some(preset) = PRESETS.iter().find(|preset| preset.key == key) else {
                    return false;
                };
                select::append_query(&mut self.expr, (preset.build)());
                self.clear_pending();
                true
            }
            SuggestionKind::MatchField(field) => {
                self.begin_field(suggest::match_field_prefix(field));
                false
            }
            SuggestionKind::RosterField { field, scope } => {
                // A roster field name is only reachable through a scope prefix
                // or a quantifier, so an unscoped one takes the widest scope
                // rather than emitting text the grammar cannot read back.
                self.begin_field(suggest::roster_field_prefix(field, scope.unwrap_or(Scope::Anyone)));
                false
            }
        }
    }

    /// Puts a field's grammar prefix in the caret and leaves the user on the
    /// value. Only the fragment being typed is replaced, so committing a
    /// suggestion part way through a query keeps the terms already there.
    fn begin_field(&mut self, prefix: Option<String>) {
        let Some(prefix) = prefix else {
            return;
        };
        self.pending = suggest::replace_active_fragment(&self.pending, &prefix);
        self.parsed_text.clear();
        self.dropdown_open = true;
        self.highlighted = None;
    }

    /// Parses the caret's text and adds it to the tree. Blank text commits
    /// nothing; text that does not parse stays put with its span underlined.
    fn commit_pending(&mut self) -> bool {
        if self.pending.trim().is_empty() {
            return false;
        }
        match parse_query(&self.pending) {
            Ok(parsed) if !parsed.is_empty_all() => {
                select::append_query(&mut self.expr, parsed);
                self.clear_pending();
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
    }

    /// Re-runs the grammar over the caret's text, but only when it changed:
    /// parsing is per keystroke, not per frame.
    fn reparse_pending(&mut self) {
        if self.pending == self.parsed_text {
            return;
        }
        self.parsed_text.clone_from(&self.pending);
        self.pending_error = parse_query(&self.pending).err();
        self.history_cursor = None;

        let request = value_request_for(&self.pending);
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

    /// The static suggestion list is rebuilt only when the locale moves under
    /// it; its labels are translated and there are enough of them that
    /// rebuilding per frame would be wasted work.
    fn refresh_suggestions(&mut self) {
        let locale = rust_i18n::locale();
        if self.suggestions.is_empty() || self.suggestions_locale != *locale {
            self.suggestions_locale = locale.to_string();
            self.suggestions = static_suggestions();
        }
    }
}

/// The right-click menu for one pill: the operators its field allows, then the
/// edits that need no operator.
///
/// A pill inside a roster predicate keeps its operator list, which `set_op`
/// does reach, and loses the rest: negate, delete, and edit-as-text all address
/// a match-level node the path does not name.
fn pill_menu(ui: &mut Ui, expr: &MatchExpr, path: &NodePath, commands: &mut Vec<Command>) {
    if let Some((allowed, current)) = select::term_op_at(expr, path) {
        ui.label(t!("ui.search.bar.operator_menu").into_owned());
        for &op in allowed {
            let enabled = select::can_set_op(expr, path, op);
            let clicked = ui
                .add_enabled_ui(enabled, |ui| ui.selectable_label(op == current, label::op_label(op)).clicked())
                .inner;
            if clicked {
                commands.push(Command::SetOp(path.clone(), op));
                ui.close();
            }
        }
        ui.separator();
    }
    if !select::addresses_match_node(expr, path) {
        return;
    }
    if ui.button(t!("ui.search.bar.edit_as_text").into_owned()).clicked() {
        commands.push(Command::EditAsText(path.clone()));
        ui.close();
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

fn token_text(token: &Token) -> String {
    match &token.kind {
        // Segments 5-7 give each part its own click target and paint; for now
        // this joins them the way `label::join_segments` builds `pill_text`.
        TokenKind::Pill { segments } => segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" "),
        TokenKind::Connector { is_or } => {
            if *is_or { t!("ui.search.bar.or_word") } else { t!("ui.search.bar.and_word") }.into_owned()
        }
        TokenKind::NotPrefix => t!("ui.search.bar.not_word").into_owned(),
        TokenKind::GroupOpen { .. } => "(".to_owned(),
        // The closing bracket of an editable group is its dissolve control, so
        // it reads as a close button rather than as punctuation.
        TokenKind::GroupClose => crate::icons::X.to_owned(),
        TokenKind::QuantClose => ")".to_owned(),
        TokenKind::QuantOpen { prefix } => format!("{prefix} ("),
        TokenKind::Caret => String::new(),
    }
}

fn token_width(token: &Token, galley: &Galley) -> f32 {
    match &token.kind {
        TokenKind::Caret => MIN_CARET_WIDTH,
        TokenKind::Pill { .. }
        | TokenKind::Connector { .. }
        | TokenKind::NotPrefix
        | TokenKind::GroupOpen { .. }
        | TokenKind::GroupClose
        | TokenKind::QuantOpen { .. }
        | TokenKind::QuantClose => galley.size().x + 2.0 * paint::PAD_X,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::index::query_ast::Expr;
    use crate::db::index::query_ast::MatchField;
    use crate::db::index::query_ast::MatchTerm;
    use crate::db::index::query_ast::Value;
    use crate::db::index::rows::MatchOutcome;

    fn win() -> MatchExpr {
        Expr::Leaf(MatchTerm::Field(MatchField::Outcome, Op::Is, Value::Outcome(MatchOutcome::Win)))
    }

    /// Double-clicking a pill lifts it into the caret so its value can be
    /// retyped, which means the tree and the caret must not both hold it. A
    /// removal that quietly did nothing would leave the term in place while
    /// its text sat in the caret, and the next Enter would add a second copy
    /// of it -- to the query, to the settings file, and to whatever text the
    /// user shares.
    #[test]
    fn editing_the_only_pill_as_text_takes_it_out_of_the_tree() {
        let mut bar = QueryBar::default();
        bar.set_expr(win());
        assert!(bar.edit_as_text(&vec![]));
        select::canonicalise(&mut bar.expr);
        assert!(bar.expr.is_empty_all(), "the pill must leave the tree: got {:?}", bar.expr);
        assert!(!bar.pending.trim().is_empty(), "the pill's text must land in the caret");

        assert!(bar.commit_pending());
        select::canonicalise(&mut bar.expr);
        assert_eq!(bar.expr, win(), "retyping the pill gives one term back, not two");
    }

    /// `-outcome:win` canonicalises to a root `Not`, whose only pill is at
    /// `[0]` rather than under any conjunction.
    #[test]
    fn editing_the_only_pill_of_a_negated_query_as_text_takes_it_out_of_the_tree() {
        let mut bar = QueryBar::default();
        bar.set_expr(Expr::Not(Box::new(win())));
        assert!(bar.edit_as_text(&vec![0]));
        select::canonicalise(&mut bar.expr);
        assert!(bar.expr.is_empty_all(), "the pill must leave the tree: got {:?}", bar.expr);
    }

    /// A pill inside a roster predicate prints as its whole quantifier while
    /// the removal would address a node the match tree does not have, so the
    /// caret must not be given text for a term that stays behind.
    #[test]
    fn editing_a_roster_internal_pill_as_text_is_refused() {
        use crate::db::index::query_ast::Op;
        use crate::db::index::query_ast::Quant;
        use crate::db::index::query_ast::RosterField;
        use crate::db::index::query_ast::RosterTerm;
        let pred = Expr::All(vec![
            Expr::Leaf(RosterTerm { field: RosterField::Tier, op: Op::Eq, value: Value::Int(10) }),
            Expr::Leaf(RosterTerm { field: RosterField::Kills, op: Op::Ge, value: Value::Int(2) }),
        ]);
        let mut bar = QueryBar::default();
        bar.set_expr(Expr::All(vec![win(), Expr::Leaf(MatchTerm::Roster { quant: Quant::None, pred })]));
        let before = bar.expr.clone();
        assert!(!bar.edit_as_text(&vec![1, 0]));
        assert!(bar.pending.is_empty());
        assert_eq!(bar.expr, before);
    }
}
