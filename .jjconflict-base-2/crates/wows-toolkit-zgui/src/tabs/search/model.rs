//! The Search tab's state: the query bar's committed pills and typed term, its history, undo and
//! completions, and the results of the last run.
//!
//! The bar keeps two texts apart: `committed`, the finished terms drawn as pills, and `pending`,
//! the term still being typed. Enter, or a completion that leaves a query that parses, moves the
//! typed term into the pills.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use sqlx::SqlitePool;
use wows_replays::types::GameParamId;
use wows_toolkit_config::index::query;
use wows_toolkit_config::index::query::SortColumn;
use wows_toolkit_config::index::query::SortSpec;
use wows_toolkit_config::index::query_ast::MatchExpr;
use wows_toolkit_config::index::query_ast::OperatorPreferences;
use wows_toolkit_config::index::query_ast::ValueKind;
use wows_toolkit_config::index::query_sql::CompileCtx;
use wows_toolkit_config::index::query_text;
use wows_toolkit_config::index::rows::MatchHit;
use wows_toolkit_viewmodel::query_bar::label::NameCache;
use wows_toolkit_viewmodel::query_bar::picker;
use wows_toolkit_viewmodel::query_bar::picker::Completion;
use wows_toolkit_viewmodel::query_bar::picker::StructuralEdit;
use wows_toolkit_viewmodel::query_bar::select;
use wows_toolkit_viewmodel::query_bar::select::Selection;
use wows_toolkit_viewmodel::query_bar::suggest;
use wows_toolkit_viewmodel::query_bar::suggest::ValueOption;
use wows_toolkit_viewmodel::query_bar::suggest::ValueRequest;
use wows_toolkit_viewmodel::query_bar::tokens;
use wows_toolkit_viewmodel::query_bar::tokens::NodePath;
use wows_toolkit_viewmodel::search::try_resolve_ship_name;
use wows_toolkit_viewmodel::search_results::NameList;
use wows_toolkit_viewmodel::search_results::RESULT_LIMIT;
use wows_toolkit_viewmodel::search_results::ValueNames;
use wows_toolkit_viewmodel::search_results::look_up_maps;
use wows_toolkit_viewmodel::search_results::look_up_sources;
use wows_toolkit_viewmodel::search_settings;
use wows_toolkit_viewmodel::search_settings::SEARCH_SETTINGS_KEY;
use wows_toolkit_viewmodel::search_settings::SavedSearch;
use zgui::reactive::LocalStorage;
use zgui::reactive::Owner;
use zgui::reactive::RwSignal;
use zgui::reactive::StoredValue;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::UpdateValue;
use zgui::reactive::prelude::WithUntracked;
use zgui::reactive::prelude::WithValue;
use zgui::task::spawn_local;
use zgui::view::TimeoutHandle;
use zgui::view::Timers;

use crate::services::Services;
use crate::services::TokioError;

/// How many structural edits can be stepped back through.
const UNDO_DEPTH: usize = 64;

/// How long a typed edit waits before the results follow it.
const RERUN_DELAY: Duration = Duration::from_millis(250);

/// How long the caret sits still before its value lookup is sent.
const VALUE_DEBOUNCE: Duration = Duration::from_millis(150);

/// Whether replacing the box's text is something the reader may want completions for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Offer {
    /// A fragment was taken and may be continued, so what can follow it is offered.
    WhatFollows,
    /// The text is an answer rather than a fragment: a committed query, a step through the history,
    /// a picked date.
    Nothing,
}

/// Where the tab is in running a query.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Status {
    /// Nothing asked for yet.
    Idle,
    /// The query text did not parse: what the parser objected to, the part of the text it
    /// objected at, and the text.
    Invalid {
        reason: String,
        span: std::ops::Range<usize>,
        text: String,
    },
    Running,
    Failed(String),
    Done,
}

/// A free value being retyped in its pill.
#[derive(Clone)]
pub(crate) struct ValueEdit {
    pub path: NodePath,
    leaf: NodePath,
    seed: MatchExpr,
    tail: NodePath,
    kind: ValueKind,
}

/// One run's results.
#[derive(Clone, Default)]
pub(crate) struct Results {
    pub hits: Rc<Vec<MatchHit>>,
    /// Whether each hit's replay is still on disk, checked once per run rather than per draw.
    pub on_disk: Rc<Vec<bool>>,
    /// Whether the run had more matches than it asked for.
    pub truncated: bool,
    /// Which run these came from, so a row from another run is told apart.
    pub generation: u64,
}

/// Which saved fields the reader changed before the saved row was read back, so the read does not
/// overwrite them.
#[derive(Clone, Copy, Default)]
struct Dirty {
    query: bool,
    history: bool,
    sort: bool,
    operator_preferences: bool,
}

impl Dirty {
    fn any(self) -> bool {
        self.query || self.history || self.sort || self.operator_preferences
    }
}

/// A field of the `search` row whose write failed, kept for the retry.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FailedWrite {
    pub field: &'static str,
    pub value: serde_json::Value,
    pub message: String,
}

/// An Up walk through the history: how far back it is, and the text it started from.
#[derive(Clone)]
struct HistoryWalk {
    at: usize,
    started_from: String,
}

/// The names a player or ship field ranks, read for one list.
struct FieldNames {
    list: NameList,
    names: Arc<ValueNames>,
}

/// The Search tab's state, as one `Copy` handle.
#[derive(Clone, Copy)]
pub(crate) struct Search {
    services: Services,
    pub committed: RwSignal<String, LocalStorage>,
    /// The term being typed, which the bar's text box is bound to.
    pub pending: RwSignal<String, LocalStorage>,
    /// The committed query, which is what the pills read back.
    pub reading: RwSignal<Option<MatchExpr>, LocalStorage>,
    pub names: RwSignal<NameCache, LocalStorage>,
    pub selection: RwSignal<Selection, LocalStorage>,
    selection_anchor: StoredValue<Option<NodePath>, LocalStorage>,
    selection_focus: StoredValue<Option<NodePath>, LocalStorage>,
    undo: StoredValue<Vec<String>, LocalStorage>,
    redo: StoredValue<Vec<String>, LocalStorage>,
    pub history: RwSignal<Vec<String>, LocalStorage>,
    history_walk: StoredValue<Option<HistoryWalk>, LocalStorage>,
    pub completions: RwSignal<Vec<Completion>, LocalStorage>,
    /// The text the completions were last built for. Text the tab sets itself is recorded here so
    /// it is not read back as typing.
    completion_source: RwSignal<String, LocalStorage>,
    pub completions_open: RwSignal<bool, LocalStorage>,
    /// The dropdown row the keyboard is on. `None` while the caret is being typed at, so Enter
    /// runs the query rather than taking whatever row happened to be first.
    pub cursor: RwSignal<Option<usize>, LocalStorage>,
    value_request: StoredValue<Option<ValueRequest>, LocalStorage>,
    pub value_options: RwSignal<Vec<ValueOption>, LocalStorage>,
    pub value_lookup_running: RwSignal<bool, LocalStorage>,
    value_asked: StoredValue<u64>,
    /// The names the caret's player or ship field ranks, read once while it stays in that field.
    value_names: StoredValue<Option<FieldNames>, LocalStorage>,
    /// Counts visits to a player or ship field, with the list the current one ranks.
    value_field: StoredValue<(u64, Option<NameList>)>,
    pub value_edit: RwSignal<Option<ValueEdit>, LocalStorage>,
    pub value_text: RwSignal<String, LocalStorage>,
    pub value_error: RwSignal<bool, LocalStorage>,
    pub sort: RwSignal<SortSpec, LocalStorage>,
    pub operator_preferences: RwSignal<OperatorPreferences, LocalStorage>,
    pub results: RwSignal<Results, LocalStorage>,
    pub status: RwSignal<Status, LocalStorage>,
    /// The query the results on screen came from.
    pub ran: RwSignal<Option<MatchExpr>, LocalStorage>,
    /// How many indexed matches carry no game mode. `None` until asked.
    pub game_mode_gap: RwSignal<Option<i64>, LocalStorage>,
    gap_lookup_running: StoredValue<bool>,
    generation: StoredValue<u64>,
    last_run: StoredValue<String, LocalStorage>,
    dirty: StoredValue<Dirty>,
    opened: StoredValue<bool>,
    pub load_error: RwSignal<Option<String>, LocalStorage>,
    pub save_error: RwSignal<Option<FailedWrite>, LocalStorage>,
    write_lock: StoredValue<Arc<futures::lock::Mutex<()>>>,
    /// Ship names resolved against each result's own build, keyed by that build.
    pub resolved_ships: RwSignal<HashMap<(u32, GameParamId), String>, LocalStorage>,
    timers: StoredValue<Option<Timers>, LocalStorage>,
    /// The tab's scope. Work started from a timer runs outside it, so every task is spawned in it
    /// to be cancelled with the tab.
    owner: StoredValue<Option<Owner>, LocalStorage>,
    rerun: StoredValue<Option<TimeoutHandle>, LocalStorage>,
    value_pause: StoredValue<Option<TimeoutHandle>, LocalStorage>,
}

impl Search {
    /// Called from the tab's body.
    pub fn new(services: Services) -> Self {
        Self {
            services,
            committed: RwSignal::new_local(String::new()),
            pending: RwSignal::new_local(String::new()),
            reading: RwSignal::new_local(None),
            names: RwSignal::new_local(NameCache::default()),
            selection: RwSignal::new_local(Selection::default()),
            selection_anchor: StoredValue::new_local(None),
            selection_focus: StoredValue::new_local(None),
            undo: StoredValue::new_local(Vec::new()),
            redo: StoredValue::new_local(Vec::new()),
            history: RwSignal::new_local(Vec::new()),
            history_walk: StoredValue::new_local(None),
            completions: RwSignal::new_local(Vec::new()),
            completion_source: RwSignal::new_local(String::new()),
            completions_open: RwSignal::new_local(false),
            cursor: RwSignal::new_local(None),
            value_request: StoredValue::new_local(None),
            value_options: RwSignal::new_local(Vec::new()),
            value_lookup_running: RwSignal::new_local(false),
            value_asked: StoredValue::new(0),
            value_names: StoredValue::new_local(None),
            value_field: StoredValue::new((0, None)),
            value_edit: RwSignal::new_local(None),
            value_text: RwSignal::new_local(String::new()),
            value_error: RwSignal::new_local(false),
            sort: RwSignal::new_local(SortSpec::default()),
            operator_preferences: RwSignal::new_local(OperatorPreferences::default()),
            results: RwSignal::new_local(Results::default()),
            status: RwSignal::new_local(Status::Idle),
            ran: RwSignal::new_local(None),
            game_mode_gap: RwSignal::new_local(None),
            gap_lookup_running: StoredValue::new(false),
            generation: StoredValue::new(0),
            last_run: StoredValue::new_local(String::new()),
            dirty: StoredValue::new(Dirty::default()),
            opened: StoredValue::new(false),
            load_error: RwSignal::new_local(None),
            save_error: RwSignal::new_local(None),
            write_lock: StoredValue::new(Arc::new(futures::lock::Mutex::new(()))),
            resolved_ships: RwSignal::new_local(HashMap::new()),
            timers: StoredValue::new_local(Timers::current()),
            owner: StoredValue::new_local(Owner::current()),
            rerun: StoredValue::new_local(None),
            value_pause: StoredValue::new_local(None),
        }
    }

    /// The whole query: what the pills hold and what is being typed after them.
    pub fn full_query(self) -> String {
        let pending = self.pending.get_untracked();
        let pending = pending.trim();
        self.committed.with_untracked(|committed| match (committed.trim(), pending) {
            ("", rest) => rest.to_string(),
            (pills, "") => pills.to_string(),
            (pills, rest) => format!("{pills} {rest}"),
        })
    }

    /// What the dropdown offers: the values the caret's own field takes once the index has
    /// answered, and otherwise the vocabulary a new term may start with. Reactive.
    pub fn offered(self) -> Vec<Completion> {
        let options = self.value_options.get();
        if options.is_empty() {
            return self.completions.get();
        }
        self.value_request
            .with_value(|request| picker::value_completions(&self.completion_source.get(), request.as_ref(), &options))
    }

    /// Runs `work` in the tab's scope, so it is cancelled with the tab wherever it was started.
    fn spawn(self, work: impl Future<Output = ()> + 'static) {
        let owner = self.owner.with_value(Clone::clone);
        match owner {
            Some(owner) => owner.with(|| spawn_local(work)),
            None => spawn_local(work),
        };
    }

    /// [`Self::offered`], for a listener.
    pub fn offered_untracked(self) -> Vec<Completion> {
        let options = self.value_options.get_untracked();
        if options.is_empty() {
            return self.completions.get_untracked();
        }
        self.value_request.with_value(|request| {
            picker::value_completions(&self.completion_source.get_untracked(), request.as_ref(), &options)
        })
    }

    /// Replaces the box's text. `Offer::Nothing` also records it as what the completions were last
    /// built for, so it is not read back as typing.
    pub fn set_bar_text(self, text: &str, offer: Offer) {
        if offer == Offer::Nothing {
            self.completions.set(Vec::new());
            self.completion_source.set(text.to_string());
            self.cursor.set(None);
            self.completions_open.set(false);
        }
        self.pending.set(text.to_string());
    }

    /// Follows the box's text after an edit. Text the tab set itself is skipped: only typing
    /// offers completions and queues a re-run. `focused` is whether the box has the caret.
    pub fn typed(self, text: &str, focused: bool) {
        if self.completion_source.with_untracked(|source| source == text) {
            return;
        }
        self.completions.set(picker::completions(text));
        self.completion_source.set(text.to_string());
        self.cursor.set(None);
        self.completions_open.set(focused);
        self.refresh_value_options(text);

        self.dirty.update_value(|dirty| dirty.query = true);
        let whole = self.full_query();
        if whole == self.last_run.get_value() {
            return;
        }
        // Typing is a new query, not a step in the walk.
        let left_walk = self.history_walk.with_value(|walk| {
            walk.as_ref()
                .is_some_and(|walk| self.history.with_untracked(|history| history.get(walk.at) != Some(&whole)))
        });
        if left_walk {
            self.history_walk.set_value(None);
        }
        let timer = self
            .timers
            .with_value(|timers| timers.as_ref().map(|timers| timers.set_timeout(RERUN_DELAY, move || self.run())));
        match timer {
            Some(timer) => self.rerun.set_value(Some(timer)),
            None => self.run(),
        }
    }

    /// Asks the index for the values the caret's field takes, when it takes any, once the caret
    /// has sat still for a moment. A player or ship field's names are read once and ranked for
    /// each value typed into it.
    fn refresh_value_options(self, text: &str) {
        let request = suggest::value_request_for(text);
        if self.value_request.with_value(|asked| *asked == request) {
            return;
        }
        self.value_request.set_value(request.clone());
        self.value_options.set(Vec::new());
        self.value_pause.set_value(None);
        self.value_asked.update_value(|asked| *asked += 1);
        // Leaving a player or ship field ends its visit, and the names read for it.
        let list = request.as_ref().and_then(name_list);
        if self.value_field.with_value(|(_, held)| *held != list) {
            self.value_names.set_value(None);
            self.value_field.update_value(|(visit, held)| {
                *visit += 1;
                *held = list;
            });
        }
        let Some(request) = request else {
            self.value_lookup_running.set(false);
            return;
        };
        let Some(pool) = self.services.settings.pool() else { return };

        self.value_lookup_running.set(true);
        let asked = self.value_asked.get_value();
        let send = move || {
            self.spawn(async move {
                let options = self.look_up_values(pool, request).await;
                // The caret has moved on to another field since.
                if self.value_asked.get_value() != asked {
                    return;
                }
                self.value_lookup_running.set(false);
                self.value_options.set(options);
            });
        };
        let timer = self
            .timers
            .with_value(|timers| timers.as_ref().map(|timers| timers.set_timeout(VALUE_DEBOUNCE, send.clone())));
        match timer {
            Some(timer) => self.value_pause.set_value(Some(timer)),
            None => send(),
        }
    }

    /// The values `request` offers. A failed lookup offers nothing and is logged: the bar still
    /// works without completions.
    async fn look_up_values(self, pool: SqlitePool, request: ValueRequest) -> Vec<ValueOption> {
        let services = self.services;
        let (list, needle) = match request {
            ValueRequest::Players { needle } => (NameList::Players, needle),
            ValueRequest::Ships { needle } => (NameList::Ships, needle),
            ValueRequest::Maps => return finished(services.on_tokio(async move { look_up_maps(&pool).await }).await),
            ValueRequest::Sources => {
                return finished(services.on_tokio(async move { look_up_sources(&pool).await }).await);
            }
        };
        let Some(names) = self.field_names(list, pool).await else { return Vec::new() };

        finished(services.on_tokio(async move { names.options(&needle) }).await)
    }

    /// The names `list` ranks: those held for the caret's field, or a read of them made now and
    /// held while the caret stays in a field of that list.
    async fn field_names(self, list: NameList, pool: SqlitePool) -> Option<Arc<ValueNames>> {
        let held = self
            .value_names
            .with_value(|held| held.as_ref().filter(|held| held.list == list).map(|held| Arc::clone(&held.names)));
        if held.is_some() {
            return held;
        }

        let started = self.value_field.with_value(|(visit, _)| *visit);
        let names = match self.services.on_tokio(async move { list.load(&pool).await }).await {
            Ok(Ok(names)) => Arc::new(names),
            Ok(Err(err)) => {
                tracing::warn!(?list, %err, "search: the name lookup failed");
                return None;
            }
            Err(err) => {
                tracing::warn!(?list, %err, "search: the name lookup did not finish");
                return None;
            }
        };
        // A read that outlived its field visit is not kept for a later one.
        if self.value_field.with_value(|(visit, _)| *visit == started) {
            self.value_names.set_value(Some(FieldNames { list, names: Arc::clone(&names) }));
        }

        Some(names)
    }

    /// Enter in the box: the query runs, is remembered, and the typed term becomes a pill.
    pub fn press_enter(self) {
        self.dirty.update_value(|dirty| dirty.query = true);
        self.rerun.set_value(None);
        self.completions_open.set(false);
        self.cursor.set(None);
        self.remember_query();
        self.commit_typed();
    }

    /// Turns what has been typed into a pill. A term half typed is not a filter, so this waits for
    /// Enter rather than following each keystroke.
    pub fn commit_typed(self) {
        let whole = self.full_query();
        if query_text::parse_query(&whole).is_err() {
            // Not a query yet: it stays in the box with its error showing.
            self.run();
            return;
        }
        self.remember_for_undo();
        self.committed.set(whole.clone());
        self.set_bar_text("", Offer::Nothing);
        self.reading.set(query_text::parse_query(&whole).ok());
        self.run();
    }

    /// Puts a taken completion in the box, committing it when it leaves a query that parses.
    pub fn take_completion(self, replacement: String, offer: Offer) {
        self.set_bar_text(&replacement, offer);
        self.cursor.set(None);
        if query_text::parse_query(&self.full_query()).is_ok() {
            self.commit_typed();
        }
    }

    /// A day taken from the calendar replaces the half-typed value under the caret.
    pub fn take_date(self, day: &str) {
        let text = self.pending.get_untracked();
        self.take_completion(suggest::replace_active_value(&text, day), Offer::Nothing);
    }

    /// Puts `query` in the bar as pills and runs it, for a caller outside the bar such as the
    /// palette's seeded searches.
    pub fn set_query_text(self, text: String) {
        self.dirty.update_value(|dirty| dirty.query = true);
        self.restore_query_text(text);
        self.run();
    }

    fn restore_query_text(self, text: String) {
        self.value_edit.set(None);
        self.value_error.set(false);
        let reading = query_text::parse_query(&text).ok();
        if reading.is_some() {
            self.committed.set(text);
            self.set_bar_text("", Offer::Nothing);
        } else {
            // A query that does not parse stays in the box for correction.
            self.committed.set(String::new());
            self.set_bar_text(&text, Offer::Nothing);
        }
        self.reading.set(reading);
    }

    /// Puts the query as it stands on the undo stack.
    fn remember_for_undo(self) {
        let current = self.full_query();
        if self.undo.with_value(|undo| undo.last() == Some(&current)) {
            return;
        }
        self.undo.update_value(|undo| {
            undo.push(current);
            if undo.len() > UNDO_DEPTH {
                undo.remove(0);
            }
        });
        // A new edit is a new branch; what was undone no longer follows.
        self.redo.update_value(Vec::clear);
    }

    #[cfg(test)]
    pub fn can_undo(self) -> bool {
        self.undo.with_value(|undo| !undo.is_empty())
    }

    #[cfg(test)]
    pub fn can_redo(self) -> bool {
        self.redo.with_value(|redo| !redo.is_empty())
    }

    /// Steps back to the query before the last structural edit.
    pub fn undo_edit(self) {
        let Some(previous) = self.undo.try_update_value(Vec::pop).flatten() else { return };
        let current = self.full_query();
        self.redo.update_value(|redo| redo.push(current));
        self.clear_pill_selection();
        self.set_query_text(previous);
    }

    /// Steps forward again to a query that was undone.
    pub fn redo_edit(self) {
        let Some(next) = self.redo.try_update_value(Vec::pop).flatten() else { return };
        let current = self.full_query();
        self.undo.update_value(|undo| undo.push(current));
        self.clear_pill_selection();
        self.set_query_text(next);
    }

    /// Applies one of a pill menu's edits.
    pub fn apply_structural_edit(self, path: NodePath, edit: StructuralEdit) {
        if edit == StructuralEdit::ToggleSelected {
            // An arrow step afterwards carries on from the pill just clicked.
            let selected = self.selection.with_untracked(|selection| selection.contains(&path));
            let end = (!selected).then(|| path.clone());
            self.selection_anchor.set_value(end.clone());
            self.selection_focus.set_value(end);
            self.selection.update(|selection| selection.toggle(path));
            return;
        }

        let Some(expr) = self.reading.get_untracked() else { return };
        let expr =
            self.selection.with_untracked(|selection| picker::apply_structural_edit(expr, &path, edit, selection));
        self.remember_for_undo();
        self.clear_pill_selection();
        self.set_query_text(query_text::print_query(&expr));
    }

    /// Takes a choice from a pill segment's picker.
    pub fn take_choice(self, taken: String) {
        self.remember_for_undo();
        self.set_query_text(taken);
    }

    /// Puts the caret back in the text, with no pill under it.
    fn clear_pill_selection(self) {
        self.selection.set(Selection::default());
        self.selection_anchor.set_value(None);
        self.selection_focus.set_value(None);
    }

    /// Moves the selection onto the pill beside the one it is on, or onto the last pill when the
    /// caret is still in the text. `extend` grows the selection from its anchor.
    pub fn step_pill_selection(self, back: bool, extend: bool) {
        let Some(expr) = self.reading.get_untracked() else { return };
        let stream = self.names.with_untracked(|names| tokens::tokenize(&expr, names));
        let paths = select::selectable_paths(&expr, &stream);
        let from = self.selection_focus.get_value();
        let Some(target) = select::step(&paths, from.as_deref(), back) else {
            // Stepping forward off the last pill puts the caret back in the text.
            if !back {
                self.clear_pill_selection();
            }
            return;
        };

        if extend {
            let anchor = self.selection_anchor.get_value().unwrap_or_else(|| target.clone());
            self.selection.update(|selection| selection.set_many(select::range(&paths, &anchor, &target)));
            self.selection_anchor.set_value(Some(anchor));
        } else {
            self.selection.update(|selection| selection.set_one(target.clone()));
            self.selection_anchor.set_value(Some(target.clone()));
        }
        self.selection_focus.set_value(Some(target));
    }

    /// Takes the pill before the caret, or the selection where there is one.
    pub fn delete_at_caret(self) {
        let Some(mut expr) = self.reading.get_untracked() else { return };
        if self.selection.with_untracked(Selection::is_empty) {
            let stream = self.names.with_untracked(|names| tokens::tokenize(&expr, names));
            let Some(target) = select::step(&select::pill_paths(&stream), None, true) else { return };
            if !select::addresses_node(&expr, &target) {
                return;
            }
            let mut one = Selection::default();
            one.set_one(target);
            select::delete(&mut expr, &one);
        } else {
            self.selection.with_untracked(|selection| select::delete(&mut expr, selection));
        }
        select::canonicalise(&mut expr);
        self.remember_for_undo();
        self.clear_pill_selection();
        self.set_query_text(query_text::print_query(&expr));
    }

    pub fn has_pill_selection(self) -> bool {
        !self.selection.with_untracked(Selection::is_empty)
    }

    /// Moves the dropdown highlight `delta` rows, out of `offered`. The first press lands on the
    /// first row whichever way it went: Down opens at the top and Up at the bottom.
    pub fn move_cursor(self, delta: isize, offered: usize) {
        if offered == 0 {
            return;
        }
        let next = match self.cursor.get_untracked() {
            None if delta > 0 => 0,
            None => offered - 1,
            Some(at) => (at as isize + delta).rem_euclid(offered as isize) as usize,
        };
        self.cursor.set(Some(next));
    }

    pub fn close_completions(self) {
        if self.completions_open.get_untracked() {
            self.completions_open.set(false);
            self.cursor.set(None);
        }
    }

    /// Puts an older (positive `delta`) or newer query in the bar. Walking past the newest
    /// restores the text the walk started from.
    pub fn walk_history(self, delta: isize) {
        if self.history.with_untracked(Vec::is_empty) {
            return;
        }
        let (at, started_from) = match self.history_walk.get_value() {
            Some(walk) => (walk.at as isize + delta, walk.started_from),
            None if delta > 0 => (0, self.full_query()),
            // Down with no walk in progress is not a recall.
            None => return,
        };

        let text = if at < 0 {
            started_from.clone()
        } else {
            match self.history.with_untracked(|history| history.get(at as usize).cloned()) {
                Some(entry) => entry,
                // Past the oldest: stay where the walk already is.
                None => return,
            }
        };

        self.committed.set(text.clone());
        self.set_bar_text("", Offer::Nothing);
        self.reading.set(query_text::parse_query(&text).ok());
        self.history_walk.set_value((at >= 0).then_some(HistoryWalk { at: at as usize, started_from }));
        self.run();
    }

    /// Records the bar's text as a query that was run. Only an explicit run is remembered, or the
    /// history would fill with every prefix of a typed query.
    fn remember_query(self) {
        let text = self.full_query();
        let changed =
            self.history.try_update(|history| search_settings::remember_query(history, &text)).unwrap_or(false);
        if !changed {
            return;
        }
        self.dirty.update_value(|dirty| dirty.history = true);
        let history = self.history.get_untracked();
        self.save_field("history", serde_json::json!(history));
    }

    /// Starts retyping the free value of the pill at `path`.
    pub fn begin_value_edit(self, path: NodePath) {
        let Some(expr) = self.reading.get_untracked() else { return };
        let Some(term_path) = select::segment_path(&expr, &path) else { return };
        let Some((field, _, value)) = select::term_at(&expr, &term_path) else { return };
        let kind = field.value_kind();
        let literal = query_text::print_value(value);
        let Some((leaf, seed, tail)) = select::leaf_seed(&expr, &term_path) else { return };
        self.value_text.set(literal);
        self.value_error.set(false);
        self.close_completions();
        self.value_edit.set(Some(ValueEdit { path, leaf, seed, tail, kind }));
    }

    /// Writes the retyped value into its pill. A value the field cannot read stays in the box,
    /// marked, until it is corrected or the edit is cancelled.
    pub fn commit_value_edit(self) {
        let Some(edit) = self.value_edit.get_untracked() else { return };
        let literal = self.value_text.get_untracked();
        let Some(value) = query_text::parse_roster_value(edit.kind, &literal) else {
            self.value_error.set(true);
            return;
        };
        let Some(mut expr) = self.reading.get_untracked() else { return };
        if !select::commit_value(&mut expr, &edit.leaf, &edit.seed, &edit.tail, value) {
            self.value_edit.set(None);
            return;
        }
        self.remember_for_undo();
        self.value_edit.set(None);
        self.value_error.set(false);
        self.committed.set(query_text::print_query(&expr));
        self.reading.set(Some(expr));
        self.dirty.update_value(|dirty| dirty.query = true);
        self.run();
    }

    pub fn cancel_value_edit(self) {
        self.value_edit.set(None);
        self.value_error.set(false);
    }

    /// A header press re-sorts, which re-runs: the ordering is the query's, not the page's.
    pub fn sort_by(self, column: SortColumn) {
        self.dirty.update_value(|dirty| dirty.sort = true);
        let sort = self.sort.get_untracked().after_click(column);
        self.sort.set(sort);
        self.save_field("sort", search_settings::sort_value(sort));
        if matches!(self.status.get_untracked(), Status::Done | Status::Running) {
            self.run();
        }
    }

    /// Parses the query and searches the index with it. An empty query matches everything; one
    /// that does not parse says where, and the results it replaces stay.
    pub fn run(self) {
        let text = self.full_query();
        self.last_run.set_value(text.clone());
        // Bumped first: every exit below changes what is on screen, and a search in flight must
        // not land over it.
        self.generation.update_value(|generation| *generation = generation.wrapping_add(1));
        let generation = self.generation.get_value();

        let expr = match query_text::parse_query(&text) {
            Ok(expr) => expr,
            Err(err) => {
                self.status.set(Status::Invalid { reason: err.kind.to_string(), span: err.span.clone(), text });
                return;
            }
        };
        self.ran.set(Some(expr.clone()));
        let mut preferences = self.operator_preferences.get_untracked();
        select::record_operators(&expr, &mut preferences);
        if self.operator_preferences.with_untracked(|prior| *prior != preferences) {
            self.dirty.update_value(|dirty| dirty.operator_preferences = true);
            match serde_json::to_value(&preferences) {
                Ok(value) => self.save_field("op_prefs", value),
                Err(error) => tracing::warn!("search: operator preferences could not be serialized: {error}"),
            }
            self.operator_preferences.set(preferences);
        }
        self.save_field("query", serde_json::Value::String(text));

        let Some(pool) = self.services.settings.pool() else {
            self.status.set(Status::Failed("the replay index is not open".to_string()));
            return;
        };

        self.status.set(Status::Running);
        self.look_up_game_mode_gap(pool.clone());

        let sort = self.sort.get_untracked();
        let services = self.services;
        self.spawn(async move {
            let found = services
                .on_tokio(async move {
                    // The query fetches one row past the limit, so a set of exactly the limit
                    // reads as complete. The extra row is dropped below.
                    query::search_by_ast(&pool, &expr, &CompileCtx::default(), RESULT_LIMIT, sort).await
                })
                .await;
            if self.generation.get_value() != generation {
                return;
            }
            let mut results = Results { generation, ..Results::default() };
            match found {
                Ok(Ok(mut hits)) => {
                    results.truncated = hits.len() as i64 > RESULT_LIMIT;
                    hits.truncate(RESULT_LIMIT as usize);
                    results.on_disk = Rc::new(hits.iter().map(|hit| hit.replay_path.is_file()).collect());
                    results.hits = Rc::new(hits);
                    self.status.set(Status::Done);
                }
                Ok(Err(err)) => self.status.set(Status::Failed(err.to_string())),
                Err(err) => self.status.set(Status::Failed(err.to_string())),
            }
            self.results.set(results);
            self.resolve_ship_names();
        });
    }

    /// Shows `hits` as a finished run's results, with no index behind them.
    #[cfg(test)]
    pub fn show_results_for_test(self, hits: Vec<MatchHit>, on_disk: Vec<bool>) {
        self.generation.update_value(|generation| *generation += 1);
        let generation = self.generation.get_value();
        self.results.set(Results { hits: Rc::new(hits), on_disk: Rc::new(on_disk), truncated: false, generation });
        self.status.set(Status::Done);
    }

    /// Asks how many matches carry no game mode, so a query on one can say what it cannot see.
    /// Asked per search, since ingest appends to the index while the app runs.
    fn look_up_game_mode_gap(self, pool: sqlx::SqlitePool) {
        if self.gap_lookup_running.get_value() {
            return;
        }
        self.gap_lookup_running.set_value(true);
        let services = self.services;
        self.spawn(async move {
            let found = services.on_tokio(async move { query::matches_missing_game_mode_count(&pool).await }).await;
            self.gap_lookup_running.set_value(false);
            match found {
                Ok(Ok(count)) => self.game_mode_gap.set(Some(count)),
                // The results still stand; only the hint is missing.
                Ok(Err(err)) => tracing::warn!("search: the game-mode gap lookup failed: {err}"),
                Err(err) => tracing::warn!("search: the game-mode gap lookup did not complete: {err}"),
            }
        });
    }

    /// Resolves each result's ship name against its own build, asking only builds already loaded:
    /// a search can span years of replays.
    fn resolve_ship_names(self) {
        let Some(cache) = self.services.game_data.cache_untracked() else { return };
        let wanted: Vec<(u32, GameParamId)> = self.results.with_untracked(|results| {
            self.resolved_ships.with_untracked(|resolved| {
                results
                    .hits
                    .iter()
                    .filter_map(|hit| Some((hit.version_build?, hit.self_ship_id?)))
                    .filter(|key| !resolved.contains_key(key))
                    .collect()
            })
        });
        if wanted.is_empty() {
            return;
        }
        self.spawn(async move {
            let resolved = zgui::task::blocking(move || {
                let mut resolved = HashMap::new();
                for (build, ship_id) in wanted {
                    let Some(loaded) = cache.loaded_build(build) else { continue };
                    if let Some(name) = try_resolve_ship_name(ship_id, Some(loaded.provider())) {
                        resolved.insert((build, ship_id), name);
                    }
                }
                resolved
            })
            .await;
            self.resolved_ships.update(|names| names.extend(resolved));
        });
    }

    /// Loads the hovered row's own build, if it is not open, and names its ship from it: its build
    /// is about to be loaded for the preview anyway.
    pub fn resolve_hovered_ship(self, hit: &MatchHit) {
        let Some(cache) = self.services.game_data.cache_untracked() else { return };
        let Some(key) = hit.version_build.zip(hit.self_ship_id) else { return };
        if self.resolved_ships.with_untracked(|resolved| resolved.contains_key(&key)) {
            return;
        }
        let (build, ship_id) = key;
        self.spawn(async move {
            let named = zgui::task::blocking(move || {
                let loaded = cache.get_or_load_build_for(build, None).ok()?;
                try_resolve_ship_name(ship_id, Some(loaded.provider()))
            })
            .await;
            if let Some(name) = named {
                self.resolved_ships.update(|names| {
                    names.insert(key, name);
                });
            }
        });
    }

    /// Reads the query the bar was last left holding and runs it, once the config database is
    /// open. The row is shared with the other front ends, so every one reopens on the same query.
    pub fn open_saved_query(self) {
        if self.opened.get_value() {
            return;
        }
        let Some(pool) = self.services.settings.pool() else { return };
        self.opened.set_value(true);

        let services = self.services;
        self.spawn(async move {
            let stored = services
                .on_tokio(async move {
                    wows_toolkit_config::queries::try_get_setting::<serde_json::Value>(&pool, SEARCH_SETTINGS_KEY).await
                })
                .await;
            let parsed = match stored {
                Ok(Ok(stored)) => search_settings::parse_saved_search(stored).map_err(|error| error.to_string()),
                Ok(Err(error)) => Err(format!("Could not read Search settings: {error}")),
                Err(error) => Err(format!("Search settings read did not complete: {error}")),
            };
            match parsed {
                Ok(saved) => self.adopt_saved(saved),
                Err(message) => {
                    self.load_error.set(Some(message.clone()));
                    if !self.dirty.get_value().any() {
                        self.status.set(Status::Failed(message));
                    }
                }
            }
        });
    }

    /// Reads the saved row again after a failed read.
    pub fn retry_load(self) {
        self.load_error.set(None);
        self.opened.set_value(false);
        self.open_saved_query();
    }

    fn adopt_saved(self, saved: SavedSearch) {
        let SavedSearch { query, history, sort, operator_preferences } = saved;
        let dirty = self.dirty.get_value();
        self.load_error.set(None);
        if !dirty.history {
            self.history.set(history);
        }
        if !dirty.sort {
            self.sort.set(sort);
        }
        if !dirty.operator_preferences {
            self.operator_preferences.set(operator_preferences);
        }
        if !dirty.query {
            self.restore_query_text(query);
            // An empty query matches everything, which is the page the tab opens on.
            self.completions_open.set(false);
            self.run();
        } else if !dirty.sort && self.last_run.get_value() == self.full_query() {
            self.run();
        }
    }

    /// Writes `field` of the shared row, keeping every other field another front end stores there.
    /// Writes are serialised so two fields saved together do not lose each other.
    fn save_field(self, field: &'static str, value: serde_json::Value) {
        let Some(pool) = self.services.settings.pool() else { return };
        let lock = self.write_lock.get_value();
        let retry = value.clone();
        let services = self.services;
        self.spawn(async move {
            let _guard = lock.lock().await;
            let written = services
                .on_tokio(async move {
                    let stored =
                        wows_toolkit_config::queries::try_get_setting::<serde_json::Value>(&pool, SEARCH_SETTINGS_KEY)
                            .await
                            .map_err(|error| {
                                format!("Could not read Search settings before saving {field}: {error}")
                            })?;
                    let row = search_settings::with_field(stored, field, value)
                        .ok_or_else(|| format!("Search settings are not an object; {field} was not saved"))?;
                    wows_toolkit_config::queries::set_setting_raw(&pool, SEARCH_SETTINGS_KEY, &row.to_string())
                        .await
                        .map_err(|error| format!("Search {field} could not be saved: {error}"))
                })
                .await;
            let message = match written {
                Ok(Ok(())) => None,
                Ok(Err(error)) => Some(error),
                Err(error) => Some(format!("Search settings update for {field} did not complete: {error}")),
            };
            if let Some(message) = &message {
                tracing::warn!("{message}");
            }
            self.save_error.set(message.map(|message| FailedWrite { field, value: retry, message }));
        });
    }

    /// Writes the failed field again.
    pub fn retry_save(self) {
        if let Some(failed) = self.save_error.get_untracked() {
            self.save_field(failed.field, failed.value);
        }
    }
}

/// Which names `request` ranks; none for a field whose values are offered entire.
fn name_list(request: &ValueRequest) -> Option<NameList> {
    match request {
        ValueRequest::Players { .. } => Some(NameList::Players),
        ValueRequest::Ships { .. } => Some(NameList::Ships),
        ValueRequest::Maps | ValueRequest::Sources => None,
    }
}

/// The values a lookup on the runtime found; none, logged, when it did not finish.
fn finished(read: Result<Vec<ValueOption>, TokioError>) -> Vec<ValueOption> {
    read.unwrap_or_else(|err| {
        tracing::warn!(%err, "search: the value lookup did not finish");
        Vec::new()
    })
}
