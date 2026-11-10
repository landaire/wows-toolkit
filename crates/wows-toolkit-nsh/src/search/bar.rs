//! The Search tab's query bar: the finished terms as pills, the term being
//! typed, the pill selection, structural undo and the run history.
//!
//! The rules are `wows_toolkit_viewmodel::query_bar`'s, shared with the egui
//! and GPUI bars; this is the state the TSX bar drives through them.

use nsh_core::JsonValue;
use nsh_core::element::JsHost;
use nsh_core::element::JsIn;
use nsh_core::element::JsOwned;
use nsh_core::element::JsValue;
use nsh_core::element::PropError;
use nsh_core::element::TsTypes;
use nsh_core::prelude::*;
use wows_toolkit_config::index::query_ast::MatchExpr;
use wows_toolkit_config::index::query_ast::OperatorPreferences;
use wows_toolkit_config::index::query_ast::ValueKind;
use wows_toolkit_config::index::query_text;
use wows_toolkit_viewmodel::query_bar::label::NameCache;
use wows_toolkit_viewmodel::query_bar::label::SegmentRole;
use wows_toolkit_viewmodel::query_bar::pills;
use wows_toolkit_viewmodel::query_bar::pills::EditablePart;
use wows_toolkit_viewmodel::query_bar::pills::StructuralEdit;
use wows_toolkit_viewmodel::query_bar::pills::UNDO_DEPTH;
use wows_toolkit_viewmodel::query_bar::select;
use wows_toolkit_viewmodel::query_bar::select::Selection;
use wows_toolkit_viewmodel::query_bar::suggest;
use wows_toolkit_viewmodel::query_bar::suggest::ValueRequest;
use wows_toolkit_viewmodel::query_bar::tokens;
use wows_toolkit_viewmodel::query_bar::tokens::NodePath;
use wows_toolkit_viewmodel::query_bar::tokens::TokenKind;
use wows_toolkit_viewmodel::search_settings::HISTORY_DEPTH;

use super::error::OperatorPreferencesError;
use crate::js_newtype::js_newtype;

/// Where a drawn token sits in the query, as child indices from the root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenPath(NodePath);

js_newtype!(TokenPath, NodePath);

/// Which way a history walk steps. Crosses to JS as `1` for older and `-1`
/// for newer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryStep {
    Older,
    Newer,
}

impl JsValue for HistoryStep {
    fn ts_type(_types: &mut TsTypes) -> String {
        "number".into()
    }

    fn from_js(value: JsIn<'_>) -> Result<Self, PropError> {
        match value.as_number() {
            Some(1.0) => Ok(Self::Older),
            Some(-1.0) => Ok(Self::Newer),
            _ => Err(value.expected("1 or -1")),
        }
    }

    fn to_js<'a>(&self, host: &'a dyn JsHost) -> Result<JsOwned<'a>, PropError> {
        let step = match self {
            Self::Older => 1.0,
            Self::Newer => -1.0,
        };

        Ok(JsOwned::new(host, host.new_number(step)))
    }
}

/// What a drawn token of the bar is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PropEnum)]
pub enum BarTokenKind {
    Pill,
    Connector,
    Not,
    GroupOpen,
    GroupClose,
    QuantOpen,
    QuantClose,
    /// Where the text box sits among the pills.
    Caret,
}

/// Which part of a term a pill segment shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PropEnum)]
pub enum SegmentKind {
    Field,
    Operator,
    Value,
}

/// An edit a pill's own menu makes to the query's shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PropEnum)]
pub enum PillEdit {
    GroupAll,
    GroupAny,
    Ungroup,
    Negate,
    Delete,
    FlipConnector,
}

/// How committing an edited pill value went.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PropEnum)]
pub enum ValueEditOutcome {
    /// The value replaced the pill's own and the query changed.
    Committed,
    /// The text is not a value of the field; the edit stays open.
    Invalid,
    /// The edit no longer applies; it closed without changing the query.
    Closed,
}

/// Which catalogue the caret's field takes its values from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PropEnum)]
pub enum ValueLookupKind {
    Players,
    Ships,
    Maps,
    Sources,
}

/// One pick a pill segment offers.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct ChoiceView {
    pub label: String,
    /// The whole query taking it produces.
    pub taken: String,
    pub current: bool,
}

/// One segment of a pill.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct PillSegmentView {
    pub kind: SegmentKind,
    pub text: String,
    /// What a press on it offers; empty for a segment with nothing to pick.
    pub choices: Vec<ChoiceView>,
    /// A free value that is retyped in place rather than picked.
    pub editable: bool,
}

/// Which of a pill menu's items apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct PillMenuView {
    pub selected: bool,
    pub can_group: bool,
    pub can_ungroup: bool,
    pub can_delete: bool,
}

/// One drawn token of the bar.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct BarToken {
    pub kind: BarTokenKind,
    pub path: TokenPath,
    /// A connector's word, a quantifier's prefix.
    pub text: String,
    pub segments: Vec<PillSegmentView>,
    pub menu: Option<PillMenuView>,
    /// The pill's value is open for retyping.
    pub editing: bool,
}

/// One completion the dropdown offers.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct CompletionView {
    pub label: String,
    pub context: String,
    pub replacement: String,
}

/// The value lookup the caret's field calls for.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct ValueLookup {
    pub kind: ValueLookupKind,
    pub needle: String,
}

/// The pill whose value is open for retyping.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct ValueEditView {
    pub path: TokenPath,
    pub literal: String,
}

/// What the bar shows.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct QueryBarView {
    /// The term being typed.
    pub pending: String,
    /// What the pills hold and what is being typed after them.
    pub full_query: String,
    /// Empty while nothing finished parses.
    pub tokens: Vec<BarToken>,
    /// The vocabulary the typed fragment may continue with.
    pub completions: Vec<CompletionView>,
    pub value_lookup: Option<ValueLookup>,
    /// The caret's field takes a date, which is picked from a calendar.
    pub wants_calendar: bool,
    /// The date half typed under the caret, when it is a whole one.
    pub calendar_date: Option<String>,
    pub can_undo: bool,
    pub can_redo: bool,
    pub has_selection: bool,
    pub value_edit: Option<ValueEditView>,
}

/// The part of a query the parser objected to.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct MarkedSpan {
    pub before: String,
    pub marked: String,
    pub after: String,
}

/// Why a query did not parse.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct ParseFailure {
    pub reason: String,
    /// `None` for an empty or out-of-range span, which the parser reports for
    /// text that simply stops early.
    pub span: Option<MarkedSpan>,
}

/// What a run is about to search for.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct RunPlan {
    pub query: String,
    pub failure: Option<ParseFailure>,
    /// The run taught the bar an operator to remember; save them.
    pub preferences_changed: bool,
}

#[derive(Debug, Clone)]
struct ValueEdit {
    path: TokenPath,
    leaf: NodePath,
    seed: MatchExpr,
    tail: NodePath,
    kind: ValueKind,
    literal: String,
}

#[derive(Debug, Clone)]
struct HistoryWalk {
    /// The recalled entry's position in `QueryBar::history`.
    at: usize,
    started_from: String,
}

/// The query bar's state.
#[class]
pub struct QueryBar {
    /// The part of the query turned into pills.
    committed: String,
    /// `committed`, parsed; what the pills read back.
    reading: Option<MatchExpr>,
    /// The term still being typed in the box.
    pending: String,

    selection: Selection,
    /// The fixed end of a multi-pill selection, and the end an arrow moves.
    selection_anchor: Option<NodePath>,
    selection_focus: Option<NodePath>,

    /// Queries a structural edit replaced, newest last.
    undo: Vec<String>,
    redo: Vec<String>,

    /// Queries that were run, newest first.
    history: Vec<String>,
    history_walk: Option<HistoryWalk>,

    operator_preferences: OperatorPreferences,
    name_cache: NameCache,
    value_edit: Option<ValueEdit>,
}

#[class]
impl QueryBar {
    pub fn new() -> Self {
        Self {
            committed: String::new(),
            reading: None,
            pending: String::new(),
            selection: Selection::default(),
            selection_anchor: None,
            selection_focus: None,
            undo: Vec::new(),
            redo: Vec::new(),
            history: Vec::new(),
            history_walk: None,
            operator_preferences: OperatorPreferences::default(),
            name_cache: NameCache::default(),
            value_edit: None,
        }
    }

    /// What the bar shows now.
    pub fn view(&self) -> QueryBarView {
        // Nothing finished parses, so no tokens are drawn.
        let tokens = self.reading.as_ref().map(|expr| self.tokens(expr)).unwrap_or_default();
        let completions = pills::completions(&self.pending)
            .into_iter()
            .map(|completion| CompletionView {
                label: completion.label,
                context: completion.context,
                replacement: completion.replacement,
            })
            .collect();
        let date = suggest::date_value_at_caret(&self.pending);
        let calendar_date = date
            .filter(|text| jiff::civil::Date::strptime("%Y-%m-%d", text).is_ok())
            .map(str::to_owned);

        QueryBarView {
            pending: self.pending.clone(),
            full_query: self.full_query(),
            tokens,
            completions,
            value_lookup: value_lookup(&self.pending),
            wants_calendar: date.is_some(),
            calendar_date,
            can_undo: !self.undo.is_empty(),
            can_redo: !self.redo.is_empty(),
            has_selection: !self.selection.is_empty(),
            value_edit: self
                .value_edit
                .as_ref()
                .map(|edit| ValueEditView { path: edit.path.clone(), literal: edit.literal.clone() }),
        }
    }

    /// The whole query: what the pills hold and what is being typed after
    /// them.
    pub fn full_query(&self) -> String {
        match (self.committed.trim(), self.pending.trim()) {
            ("", rest) => rest.to_owned(),
            (pills, "") => pills.to_owned(),
            (pills, rest) => format!("{pills} {rest}"),
        }
    }

    /// Queries that were run, newest first.
    #[getter]
    pub fn history(&self) -> Vec<String> {
        self.history.clone()
    }

    /// Takes the saved history in place of what the bar holds.
    pub fn set_history(&mut self, history: Vec<String>) {
        self.history = history;
        self.history_walk = None;
    }

    /// The operators each field was last given, as the settings row keeps
    /// them.
    pub fn operator_preferences(&self) -> Result<JsonValue, OperatorPreferencesError> {
        serde_json::to_value(&self.operator_preferences).map_err(|source| OperatorPreferencesError::Encode { source })
    }

    /// Takes saved operator preferences in place of what the bar holds.
    pub fn set_operator_preferences(&mut self, saved: JsonValue) -> Result<(), OperatorPreferencesError> {
        self.operator_preferences =
            serde_json::from_value(saved).map_err(|source| OperatorPreferencesError::Decode { source })?;

        Ok(())
    }

    /// Puts `text` in the bar without running it: as pills when it parses,
    /// and in the box otherwise, so an invalid query stays there to correct.
    pub fn restore(&mut self, text: String) {
        self.value_edit = None;
        self.reading = query_text::parse_query(&text).ok();
        if self.reading.is_some() {
            self.committed = text;
            self.pending.clear();
        } else {
            self.committed.clear();
            self.pending = text;
        }
    }

    /// Replaces the whole query with `text`, which the caller runs.
    pub fn set_query(&mut self, text: String) {
        self.clear_pill_selection();
        self.restore(text);
    }

    /// The box was typed in.
    pub fn set_pending(&mut self, text: String) {
        self.pending = text;

        // Typing is a new query, not a step in the walk. The walk's own
        // edits carry the entry it recalled, so only other text leaves it.
        let full = self.full_query();
        if let Some(walk) = self.history_walk.as_ref()
            && self.history.get(walk.at) != Some(&full)
        {
            self.history_walk = None;
        }
    }

    /// Turns what has been typed into pills. `false` when the whole query
    /// does not parse: it stays in the box with its error showing.
    pub fn commit_typed(&mut self) -> bool {
        let whole = self.full_query();
        let Ok(parsed) = query_text::parse_query(&whole) else {
            return false;
        };

        self.remember_for_undo();
        self.committed = whole;
        self.pending.clear();
        self.reading = Some(parsed);

        true
    }

    /// Takes a completion: `replacement` is the box's new text. A finished
    /// query becomes pills at once; `true` when it did.
    pub fn take_completion(&mut self, replacement: String) -> bool {
        self.set_pending(replacement);
        if query_text::parse_query(&self.full_query()).is_err() {
            return false;
        }

        self.commit_typed()
    }

    /// A day picked from the calendar, `YYYY-MM-DD`, replacing the value
    /// half typed under the caret.
    pub fn take_date(&mut self, day: String) -> bool {
        let replacement = suggest::replace_active_value(&self.pending, &day);

        self.take_completion(replacement)
    }

    /// Records the query as one that was run, newest first; `true` when the
    /// history changed and wants saving.
    pub fn remember_query(&mut self) -> bool {
        let text = self.full_query();
        if text.is_empty() || self.history.first() == Some(&text) {
            return false;
        }

        self.history.retain(|entry| entry != &text);
        self.history.insert(0, text);
        self.history.truncate(HISTORY_DEPTH);

        true
    }

    /// Puts an older or newer query in the bar; `true` when the bar changed
    /// and wants running. Walking past the newest restores the text the walk
    /// started from.
    pub fn walk_history(&mut self, step: HistoryStep) -> bool {
        if self.history.is_empty() {
            return false;
        }

        let walk = self.history_walk.take();
        // `None` is past the newest entry.
        let at = match (&walk, step) {
            (Some(walk), HistoryStep::Older) => Some(walk.at + 1),
            (Some(walk), HistoryStep::Newer) => walk.at.checked_sub(1),
            (None, HistoryStep::Older) => Some(0),
            // Down with no walk in progress is not a recall.
            (None, HistoryStep::Newer) => return false,
        };

        let recalled = match at.map(|at| self.history.get(at).cloned()) {
            Some(Some(entry)) => Some(entry),
            Some(None) => {
                // Past the oldest: stay where the walk already is.
                self.history_walk = walk;
                return false;
            }
            None => None,
        };
        let started_from = match walk {
            Some(walk) => walk.started_from,
            None => self.full_query(),
        };

        self.committed = recalled.unwrap_or_else(|| started_from.clone());
        self.pending.clear();
        self.reading = query_text::parse_query(&self.committed).ok();
        self.history_walk = at.map(|at| HistoryWalk { at, started_from });

        true
    }

    /// Adds the pill at `path` to the selection, or takes it out again.
    pub fn toggle_selected(&mut self, path: TokenPath) {
        let path = path.0;

        // An arrow step afterwards carries on from the pill just pressed.
        if self.selection.contains(&path) {
            self.selection_anchor = None;
            self.selection_focus = None;
        } else {
            self.selection_anchor = Some(path.clone());
            self.selection_focus = Some(path.clone());
        }
        self.selection.toggle(path);
    }

    /// Applies a pill menu's `edit`, asked for from the pill at `path`;
    /// `true` when the query changed and wants running.
    pub fn edit_pill(&mut self, path: TokenPath, edit: PillEdit) -> bool {
        let edit = match edit {
            PillEdit::GroupAll => StructuralEdit::Group { is_or: false },
            PillEdit::GroupAny => StructuralEdit::Group { is_or: true },
            PillEdit::Ungroup => StructuralEdit::Ungroup,
            PillEdit::Negate => StructuralEdit::Negate,
            PillEdit::Delete => StructuralEdit::Delete,
            PillEdit::FlipConnector => StructuralEdit::FlipConnector,
        };
        let Some(reading) = self.reading.as_ref() else { return false };
        let Some(edited) = pills::apply_structural_edit(reading, &path.0, edit, &self.selection) else {
            return false;
        };

        self.rewrite(query_text::print_query(&edited));

        true
    }

    /// Moves the selection onto the pill beside the one it is on, or onto
    /// the last pill when the caret is still in the text. `extend` grows the
    /// selection from its anchor instead of replacing it.
    pub fn step_selection(&mut self, back: bool, extend: bool) {
        let Some(expr) = self.reading.as_ref() else { return };
        let stream = tokens::tokenize(expr, &self.name_cache);
        let paths = select::selectable_paths(expr, &stream);
        let Some(target) = select::step(&paths, self.selection_focus.as_deref(), back) else {
            // Stepping forward off the last pill puts the caret back in the
            // text, which is where the reader was heading.
            if !back {
                self.clear_pill_selection();
            }
            return;
        };

        if extend {
            // An anchorless extension starts from the pill it reaches.
            let anchor = self.selection_anchor.clone().unwrap_or_else(|| target.clone());
            self.selection.set_many(select::range(&paths, &anchor, &target));
            self.selection_anchor = Some(anchor);
        } else {
            self.selection.set_one(target.clone());
            self.selection_anchor = Some(target.clone());
        }
        self.selection_focus = Some(target);
    }

    /// Puts the caret back in the text, with no pill under it.
    pub fn clear_pill_selection(&mut self) {
        self.selection.clear();
        self.selection_anchor = None;
        self.selection_focus = None;
    }

    /// Takes the selection off the query, or the pill before the caret when
    /// nothing is selected; `true` when the query changed.
    pub fn delete_at_caret(&mut self) -> bool {
        let Some(expr) = self.reading.as_ref() else { return false };
        let target = if self.selection.is_empty() {
            let stream = tokens::tokenize(expr, &self.name_cache);
            let Some(last) = select::step(&select::pill_paths(&stream), None, true) else { return false };
            if !select::addresses_node(expr, &last) {
                return false;
            }
            let mut one = Selection::default();
            one.set_one(last);
            one
        } else {
            self.selection.clone()
        };

        let mut edited = expr.clone();
        select::delete(&mut edited, &target);
        select::canonicalise(&mut edited);
        self.rewrite(query_text::print_query(&edited));

        true
    }

    /// Steps back to the query before the last structural edit; `true` when
    /// there was one.
    pub fn undo(&mut self) -> bool {
        let Some(previous) = self.undo.pop() else { return false };

        self.redo.push(self.full_query());
        self.set_query(previous);

        true
    }

    /// Steps forward again to a query that was undone.
    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else { return false };

        self.undo.push(self.full_query());
        self.set_query(next);

        true
    }

    /// Opens the value of the pill at `path` for retyping; `false` when the
    /// pill has no free value.
    pub fn begin_value_edit(&mut self, path: TokenPath) -> bool {
        let Some(expr) = self.reading.as_ref() else { return false };
        let Some(term_path) = select::segment_path(expr, &path.0) else { return false };
        let Some((field, _, value)) = select::term_at(expr, &term_path) else { return false };
        let literal = query_text::print_value(value);
        let Some((leaf, seed, tail)) = select::leaf_seed(expr, &term_path) else { return false };

        self.value_edit = Some(ValueEdit { path, leaf, seed, tail, kind: field.value_kind(), literal });

        true
    }

    /// Commits the open value edit with `literal`.
    pub fn commit_value_edit(&mut self, literal: String) -> ValueEditOutcome {
        let Some(edit) = self.value_edit.as_ref() else { return ValueEditOutcome::Closed };
        let Some(value) = query_text::parse_roster_value(edit.kind, &literal) else {
            return ValueEditOutcome::Invalid;
        };
        let Some(mut expr) = self.reading.clone() else {
            self.value_edit = None;
            return ValueEditOutcome::Closed;
        };
        if !select::commit_value(&mut expr, &edit.leaf, &edit.seed, &edit.tail, value) {
            self.value_edit = None;
            return ValueEditOutcome::Closed;
        }

        self.remember_for_undo();
        self.value_edit = None;
        self.committed = query_text::print_query(&expr);
        self.reading = Some(expr);

        ValueEditOutcome::Committed
    }

    /// Closes the open value edit without changing the query.
    pub fn cancel_value_edit(&mut self) {
        self.value_edit = None;
    }

    /// What a run searches for, and why it cannot when the query does not
    /// parse. Records the operators the query uses as the ones each field
    /// was last given.
    pub fn prepare_run(&mut self) -> RunPlan {
        let query = self.full_query();
        let expr = match query_text::parse_query(&query) {
            Ok(expr) => expr,
            Err(error) => {
                let failure = ParseFailure { reason: error.kind.to_string(), span: marked_span(&query, error.span) };
                return RunPlan { query, failure: Some(failure), preferences_changed: false };
            }
        };

        let prior = self.operator_preferences.clone();
        select::record_operators(&expr, &mut self.operator_preferences);

        RunPlan { query, failure: None, preferences_changed: self.operator_preferences != prior }
    }
}

impl QueryBar {
    /// Records the query before a structural rewrite, so undo restores it.
    fn remember_for_undo(&mut self) {
        let current = self.full_query();
        if self.undo.last() == Some(&current) {
            return;
        }

        self.undo.push(current);
        if self.undo.len() > UNDO_DEPTH {
            self.undo.remove(0);
        }
        // A new edit is a new branch; what was undone no longer follows.
        self.redo.clear();
    }

    /// Replaces the query after a structural edit, recording what it was.
    fn rewrite(&mut self, text: String) {
        self.remember_for_undo();
        self.set_query(text);
    }

    fn tokens(&self, expr: &MatchExpr) -> Vec<BarToken> {
        tokens::tokenize(expr, &self.name_cache)
            .into_iter()
            .map(|token| {
                let path = token.path;
                let (kind, text, segments, menu) = match token.kind {
                    TokenKind::Pill { segments } => {
                        let segments = segments
                            .into_iter()
                            .map(|segment| self.segment(expr, &path, segment.role, segment.text))
                            .collect();
                        let menu = pills::pill_menu(expr, &path, &self.selection);
                        let menu = PillMenuView {
                            selected: menu.selected,
                            can_group: menu.can_group,
                            can_ungroup: menu.can_ungroup,
                            can_delete: menu.can_delete,
                        };
                        (BarTokenKind::Pill, String::new(), segments, Some(menu))
                    }
                    TokenKind::Connector { is_or } => {
                        (BarTokenKind::Connector, if is_or { "or" } else { "and" }.to_owned(), Vec::new(), None)
                    }
                    TokenKind::NotPrefix => (BarTokenKind::Not, "not".to_owned(), Vec::new(), None),
                    TokenKind::GroupOpen { .. } => (BarTokenKind::GroupOpen, "(".to_owned(), Vec::new(), None),
                    TokenKind::GroupClose => (BarTokenKind::GroupClose, ")".to_owned(), Vec::new(), None),
                    TokenKind::QuantOpen { prefix } => (BarTokenKind::QuantOpen, prefix, Vec::new(), None),
                    TokenKind::QuantClose => (BarTokenKind::QuantClose, "]".to_owned(), Vec::new(), None),
                    TokenKind::Caret => (BarTokenKind::Caret, String::new(), Vec::new(), None),
                };
                let path = TokenPath(path);
                let editing = self.value_edit.as_ref().is_some_and(|edit| edit.path == path);

                BarToken { kind, path, text, segments, menu, editing }
            })
            .collect()
    }

    fn segment(&self, expr: &MatchExpr, path: &NodePath, role: SegmentRole, text: String) -> PillSegmentView {
        let choices: Vec<ChoiceView> =
            pills::choices(expr, path, EditablePart::of(role), &self.operator_preferences)
                .into_iter()
                .map(|choice| ChoiceView { label: choice.label, taken: choice.taken, current: choice.current })
                .collect();
        let kind = match role {
            SegmentRole::Filter => SegmentKind::Field,
            SegmentRole::Operator => SegmentKind::Operator,
            SegmentRole::Value => SegmentKind::Value,
        };
        // A free value is retyped in place; one that enumerates is picked.
        let editable = kind == SegmentKind::Value && choices.is_empty() && select::segment_path(expr, path).is_some();

        PillSegmentView { kind, text, choices, editable }
    }
}

impl Default for QueryBar {
    fn default() -> Self {
        Self::new()
    }
}

/// The value lookup `pending`'s caret field calls for.
pub fn value_lookup(pending: &str) -> Option<ValueLookup> {
    let lookup = match suggest::value_request_for(pending)? {
        ValueRequest::Players { needle } => ValueLookup { kind: ValueLookupKind::Players, needle },
        ValueRequest::Ships { needle } => ValueLookup { kind: ValueLookupKind::Ships, needle },
        ValueRequest::Maps => ValueLookup { kind: ValueLookupKind::Maps, needle: String::new() },
        ValueRequest::Sources => ValueLookup { kind: ValueLookupKind::Sources, needle: String::new() },
    };

    Some(lookup)
}

/// `span` of `text` split for marking, when it marks anything.
fn marked_span(text: &str, span: std::ops::Range<usize>) -> Option<MarkedSpan> {
    let marked = text.get(span.clone()).filter(|marked| !marked.is_empty())?;

    Some(MarkedSpan {
        before: text[..span.start].to_owned(),
        marked: marked.to_owned(),
        after: text[span.end..].to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar_with(text: &str) -> QueryBar {
        let mut bar = QueryBar::new();
        bar.set_pending(text.to_owned());
        bar
    }

    fn first_pill(bar: &QueryBar) -> TokenPath {
        bar.view()
            .tokens
            .into_iter()
            .find(|token| token.kind == BarTokenKind::Pill)
            .map(|token| token.path)
            .expect("the bar draws a pill")
    }

    #[test]
    fn a_finished_term_becomes_a_pill_and_backspace_takes_it_back() {
        let mut bar = bar_with("outcome=win");
        assert!(bar.view().tokens.is_empty(), "a term being typed is text, not a pill");

        assert!(bar.commit_typed());
        let view = bar.view();
        assert_eq!(view.pending, "");
        assert!(view.tokens.iter().any(|token| token.kind == BarTokenKind::Pill));

        assert!(bar.delete_at_caret());
        assert_eq!(bar.full_query(), "");
    }

    #[test]
    fn a_query_that_does_not_parse_stays_in_the_box_and_names_its_error() {
        let mut bar = bar_with("outcome=win and");

        assert!(!bar.commit_typed());
        assert_eq!(bar.view().pending, "outcome=win and");
        let plan = bar.prepare_run();
        assert!(plan.failure.is_some(), "the run reports the parse error");
    }

    #[test]
    fn undo_and_redo_step_through_structural_edits() {
        let mut bar = bar_with("outcome=win");
        bar.commit_typed();
        assert!(!bar.view().can_redo);

        assert!(bar.edit_pill(first_pill(&bar), PillEdit::Negate));
        let negated = bar.full_query();
        assert_ne!(negated, "outcome=win");
        assert!(bar.view().can_undo);

        assert!(bar.undo());
        assert_eq!(bar.full_query(), "outcome=win");
        assert!(bar.redo());
        assert_eq!(bar.full_query(), negated);
    }

    #[test]
    fn up_recalls_what_was_run_and_down_walks_back_out() {
        let mut bar = bar_with("outcome:win");
        bar.commit_typed();
        assert!(bar.remember_query());
        bar.set_query("survived:false".to_owned());
        assert!(bar.remember_query());
        bar.set_query(String::new());

        assert!(bar.walk_history(HistoryStep::Older));
        assert_eq!(bar.full_query(), "survived:false");
        assert!(bar.walk_history(HistoryStep::Older));
        assert_eq!(bar.full_query(), "outcome:win");
        assert!(!bar.walk_history(HistoryStep::Older), "past the oldest stays put");
        assert!(bar.walk_history(HistoryStep::Newer));
        assert!(bar.walk_history(HistoryStep::Newer));
        assert_eq!(bar.full_query(), "", "walking back out restores the text the walk started from");
    }

    #[test]
    fn a_completion_that_finishes_a_query_becomes_pills() {
        let mut bar = bar_with("outcome=w");
        let taken = suggest::replace_active_value(&bar.pending, "win");

        assert!(bar.take_completion(taken));
        assert_eq!(bar.view().pending, "");
        assert_eq!(bar.full_query(), "outcome=win");
    }

    #[test]
    fn a_date_field_asks_for_a_calendar_and_takes_a_day() {
        let mut bar = bar_with("map:ocean date>");
        assert!(bar.view().wants_calendar);
        assert!(!bar_with("map:").view().wants_calendar, "a map is not a date");

        bar.take_date("2026-10-28".to_owned());
        assert!(bar.full_query().contains("2026-10-28"), "the day landed: {}", bar.full_query());
    }

    #[test]
    fn a_free_value_is_retyped_in_place() {
        let mut bar = bar_with("build>9000000");
        bar.commit_typed();
        let path = first_pill(&bar);
        let token = bar.view().tokens.into_iter().find(|token| token.path == path).expect("the pill");
        assert!(token.segments.iter().any(|segment| segment.editable), "a number is typed, not picked");

        assert!(bar.begin_value_edit(path));
        assert_eq!(bar.commit_value_edit("not a number".to_owned()), ValueEditOutcome::Invalid);
        assert_eq!(bar.commit_value_edit("9100000".to_owned()), ValueEditOutcome::Committed);
        assert!(bar.full_query().contains("9100000"));
    }

    #[test]
    fn arrows_step_and_extend_the_pill_selection() {
        let mut bar = bar_with("outcome=win map:ocean");
        bar.commit_typed();

        bar.step_selection(true, false);
        assert!(bar.view().has_selection);
        bar.step_selection(true, true);
        assert!(bar.delete_at_caret(), "the selection is deleted");
        assert_eq!(bar.full_query(), "");
    }

    #[test]
    fn a_parse_error_marks_the_text_it_objects_to() {
        assert_eq!(
            marked_span("map:ocean )", 10..11),
            Some(MarkedSpan { before: "map:ocean ".into(), marked: ")".into(), after: String::new() })
        );
        assert_eq!(marked_span("map:", 4..5), None);
    }
}
