//! The dockable Search tab: a query bar over a results table backed by the
//! replay index.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::Sender;
use std::sync::mpsc::TryRecvError;
use std::sync::mpsc::channel;
use std::time::Duration;
use std::time::Instant;

use rust_i18n::t;
use tokio::runtime::Runtime;
use wows_replays::types::AccountId;
use wows_replays::types::GameParamId;

use crate::app::ToolkitTabViewer;
use crate::data::settings::ResultColumn;
use crate::db::index::query;
use crate::db::index::query::SortColumn;
use crate::db::index::query::SortDirection;
use crate::db::index::query::SortSpec;
use crate::db::index::query_ast::Expr;
use crate::db::index::query_ast::MapCatalog;
use crate::db::index::query_ast::MatchExpr;
use crate::db::index::query_ast::MatchField;
use crate::db::index::query_ast::MatchTerm;
use crate::db::index::query_ast::RosterExpr;
use crate::db::index::query_ast::Value;
use crate::db::index::query_sql::CompileCtx;
use crate::db::index::query_text::print_query;
use crate::db::index::query_text::quote_if_needed;
use crate::db::index::rows::IndexSource;
use crate::db::index::rows::MatchHit;
use crate::db::index::rows::MatchOutcome;
use crate::icons;
use crate::replay::renderer::preview::PreviewKey;
use crate::ui::query_bar::Deps;
use crate::ui::query_bar::QueryBar;
use crate::ui::query_bar::select::prune_empty;
use crate::ui::query_bar::suggest::ValueOption;
use crate::ui::query_bar::suggest::ValueRequest;
use crate::ui::replay_parser::preview_popup;
use crate::ui::theme::contrast::label_on;
use crate::ui::theme::semantic::SemanticExt;
use crate::ui::widgets::pr_chip;
use crate::util::personal_rating::PersonalRatingCategory;

/// Rows a single search reads. `search_by_ast` fetches one more so the count can
/// say "at least this many" rather than reporting a truncated total.
const RESULT_LIMIT: i64 = 500;
/// Rows a value lookup returns to the dropdown.
const VALUE_LIMIT: i64 = 50;
/// How long the caret must sit still before its value lookup is dispatched, so
/// typing a ship name does not issue one query per keystroke.
const VALUE_DEBOUNCE: Duration = Duration::from_millis(150);
/// How often the tab wakes itself while it is waiting on the runtime. egui only
/// repaints on input, so a reply arriving between frames would otherwise sit in
/// the channel until the pointer moved.
const POLL_INTERVAL: Duration = Duration::from_millis(50);
/// How long the pointer has to sit continuously on one result row's map cell
/// before the shared preview machinery is asked to do anything for it. A
/// table is scrolled fast, and scrolling drags the pointer across every row
/// in between; this gate is what stops that from queuing a bake per row the
/// pointer merely crossed.
const PREVIEW_DWELL: Duration = Duration::from_millis(300);

/// Tracks how long the pointer has continuously dwelled on one result row's
/// map cell, independent of egui: the table calls `hover`/`leave` once per
/// frame, and `pending_request` says whether a preview should actually be
/// asked for. Kept as pure logic so the dwell and cancellation behaviour can
/// be tested without rendering a frame.
#[derive(Default)]
struct PreviewState {
    /// The row currently under the pointer and how long it has been watched
    /// there.
    watched: Option<(PreviewKey, Duration)>,
}

impl PreviewState {
    /// Record that `key` was under the pointer for another `elapsed` of wall
    /// time. A key different from the one already being watched replaces it
    /// rather than adding to it: the dwell belongs to whichever row is
    /// currently under the pointer, not to a sum across rows visited earlier.
    fn hover(&mut self, key: PreviewKey, elapsed: Duration) {
        match &mut self.watched {
            Some((watched_key, dwelled)) if *watched_key == key => *dwelled += elapsed,
            _ => self.watched = Some((key, elapsed)),
        }
    }

    /// The row a preview should be requested for, once the pointer has
    /// dwelled on it for at least `PREVIEW_DWELL`.
    fn pending_request(&self) -> Option<PreviewKey> {
        let (key, dwelled) = self.watched.as_ref()?;
        (*dwelled >= PREVIEW_DWELL).then(|| key.clone())
    }

    /// The pointer left the table this frame, so nothing is dwelling
    /// anymore; whatever was pending is cancelled.
    fn leave(&mut self) {
        self.watched = None;
    }
}

/// A reply from the tokio runtime. Every database read the tab makes comes back
/// through one channel, so the UI thread never blocks on the runtime.
enum SearchMsg {
    /// `seq` identifies the search that asked, so a slower earlier query cannot
    /// overwrite the results of a later one.
    Results {
        seq: u64,
        hits: Vec<MatchHit>,
        truncated: bool,
    },
    Values {
        request: ValueRequest,
        options: Vec<ValueOption>,
    },
    Sources(Vec<IndexSource>),
    Maps(Vec<String>),
    Names {
        ships: Vec<(GameParamId, String)>,
        players: Vec<(AccountId, String)>,
    },
    /// How many indexed matches have `game_mode_id IS NULL`, refreshed
    /// alongside every search so it reads zero again once the user re-indexes.
    GameModeGap(i64),
}

pub struct SearchTabState {
    pub bar: QueryBar,
    pub results: Vec<MatchHit>,
    pub dirty: bool,
    /// True when the last query returned `limit + 1` rows.
    pub truncated: bool,
    pub sources: Vec<IndexSource>,
    /// How many indexed matches have no numeric game mode recorded, or `None`
    /// before the first search has returned one. A fetched zero and an
    /// unfetched count both have to render as "say nothing", but they are not
    /// the same fact, so this is never defaulted to `0`.
    pub game_mode_gap: Option<i64>,
    /// Display-name to raw-space-name resolution for a `map` term.
    ///
    /// Empty, and correct empty: `indexed_match.map` already stores the
    /// localized name the replay reported (see `BattleReport::map_name`), so a
    /// `map` term compares what the user typed against what the user reads with
    /// nothing to translate. The catalogue is what `query_sql` consults for a
    /// column holding raw space names.
    pub maps: MapCatalog,

    /// Every spawned task sends exactly once, `None` when it produced nothing,
    /// so `in_flight` balances however the task ended.
    tx: Sender<Option<SearchMsg>>,
    rx: Receiver<Option<SearchMsg>>,
    /// The value lookup waiting out its debounce, and when it was asked for.
    debounced: Option<(ValueRequest, Instant)>,
    /// The value lookup whose reply the bar is still waiting for, so a reply to
    /// a superseded one is discarded rather than shown under the new caret.
    awaiting_values: Option<ValueRequest>,
    /// Rises with each dispatched search; a reply carrying an older number is
    /// stale and dropped.
    query_seq: u64,
    /// Replies still outstanding, so the tab only keeps waking itself while
    /// there is something to wake for.
    in_flight: usize,
    /// Ids a name lookup has already been issued for, whether or not the index
    /// knew them.
    asked_ships: HashSet<GameParamId>,
    asked_players: HashSet<AccountId>,
    /// Map names the index has seen, offered when the caret is typing a `map`
    /// value.
    map_names: Vec<String>,
    /// Set once the source and map lists have been asked for. Both are fetched
    /// when the tab first draws rather than when a value editor needs them: an
    /// index with no groups and no maps answers with nothing, and a flag that
    /// only rose on a non-empty answer would re-ask every frame forever.
    catalogues_requested: bool,
    /// The dwell gate for the results table's per-row map preview.
    preview_state: PreviewState,
}

impl Default for SearchTabState {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self {
            bar: QueryBar::default(),
            results: Vec::new(),
            dirty: true,
            truncated: false,
            sources: Vec::new(),
            game_mode_gap: None,
            maps: MapCatalog::default(),
            tx,
            rx,
            debounced: None,
            awaiting_values: None,
            query_seq: 0,
            in_flight: 0,
            asked_ships: HashSet::new(),
            asked_players: HashSet::new(),
            map_names: Vec::new(),
            catalogues_requested: false,
            preview_state: PreviewState::default(),
        }
    }
}

impl SearchTabState {
    /// Replaces the query outright, as a seeded search or a restored setting
    /// does, and re-queries.
    pub(crate) fn set_query(&mut self, expr: MatchExpr) {
        self.bar.set_expr(expr);
        self.dirty = true;
    }

    /// Called when a background re-index finishes with `indexed` rows
    /// actually written. `dispatch_search` only ever runs again when
    /// something else marks the tab dirty (a typed edit, a seeded query, a
    /// restored setting) -- none of which fire on their own when indexing
    /// completes elsewhere in the app. Without this, the results table and
    /// the game-mode gap count both go stale the moment a re-index the search
    /// UI itself told the user to run actually finishes: the same query
    /// keeps returning the same rows, and `game_mode_gap` keeps reporting the
    /// pre-reindex count, against a table that has already changed.
    ///
    /// `indexed == 0` means nothing was written (every file was already
    /// indexed and a plain, non-forced pass skipped them all), so there is
    /// nothing for a re-query to find that the last one did not already see.
    pub(crate) fn note_reindex_completed(&mut self, indexed: usize) {
        if indexed > 0 {
            self.dirty = true;
        }
    }

    /// Applies every reply that has arrived since the last frame.
    fn drain_replies(&mut self) {
        loop {
            let reply = match self.rx.try_recv() {
                Ok(reply) => reply,
                Err(TryRecvError::Empty) => return,
                // The tab owns both ends, so the sender outlives every receive.
                Err(TryRecvError::Disconnected) => return,
            };
            // Counted before the payload is examined, because a task that failed
            // still sends: the count is of tasks, not of results.
            self.in_flight = self.in_flight.saturating_sub(1);
            let Some(msg) = reply else {
                continue;
            };
            match msg {
                SearchMsg::Results { seq, hits, truncated } => {
                    if seq == self.query_seq {
                        self.results = hits;
                        self.truncated = truncated;
                    }
                }
                SearchMsg::Values { request, options } => {
                    if self.awaiting_values.as_ref() == Some(&request) {
                        self.bar.value_options = options;
                        self.awaiting_values = None;
                    }
                }
                SearchMsg::Sources(sources) => {
                    self.bar.names.sources.clone_from(&sources);
                    self.sources = sources;
                }
                SearchMsg::Maps(maps) => self.map_names = maps,
                SearchMsg::Names { ships, players } => {
                    self.bar.names.ships.extend(ships);
                    self.bar.names.players.extend(players);
                }
                SearchMsg::GameModeGap(count) => self.game_mode_gap = Some(count),
            }
        }
    }

    /// Runs `work` on the runtime and delivers what it produces over the tab's
    /// own channel.
    ///
    /// The send is unconditional and carries the `Option` itself. `in_flight`
    /// is what keeps the tab waking itself until every reply is in hand, so a
    /// task that produced nothing still has to say it finished; a work future
    /// that returned without sending would leave the count above zero for the
    /// life of the tab and repaint at `POLL_INTERVAL` forever. Putting the send
    /// here rather than in each future is what stops a later error path from
    /// reintroducing that.
    fn spawn<F>(&mut self, rt: &Runtime, work: F)
    where
        F: std::future::Future<Output = Option<SearchMsg>> + Send + 'static,
    {
        let tx = self.tx.clone();
        self.in_flight += 1;
        rt.spawn(async move {
            let _ = tx.send(work.await);
        });
    }

    /// `sort` becomes the query's own `ORDER BY`. It cannot be applied to the
    /// rows this brings back: the query fetches `RESULT_LIMIT + 1` rows and the
    /// tab keeps the first `RESULT_LIMIT`, so re-ordering them would show the
    /// first 500 matches by date rearranged rather than the 500 highest by the
    /// column the user clicked.
    fn dispatch_search(&mut self, pool: &sqlx::SqlitePool, rt: &Runtime, sort: SortSpec) {
        self.dirty = false;
        self.query_seq += 1;
        let seq = self.query_seq;
        // The pruned tree, not the one on screen: an empty-text term compiles to
        // `LIKE '%'` and would widen the search instead of narrowing it.
        let expr = prune_empty(&self.bar.expr);
        let maps = self.maps.clone();
        let search_pool = pool.clone();
        self.spawn(rt, async move {
            let ctx = CompileCtx { maps: &maps };
            match query::search_by_ast(&search_pool, &expr, &ctx, RESULT_LIMIT, sort).await {
                Ok(mut hits) => {
                    let truncated = hits.len() as i64 > RESULT_LIMIT;
                    hits.truncate(RESULT_LIMIT as usize);
                    Some(SearchMsg::Results { seq, hits, truncated })
                }
                Err(e) => {
                    tracing::warn!("search: query failed: {e}");
                    None
                }
            }
        });

        // Refreshed on every search rather than fetched once, so the hint
        // below actually clears once the user re-indexes: a "requested once"
        // flag like the source/map catalogues' would leave a stale nonzero
        // count on screen for the rest of the session.
        let gap_pool = pool.clone();
        self.spawn(rt, async move {
            match query::matches_missing_game_mode_count(&gap_pool).await {
                Ok(count) => Some(SearchMsg::GameModeGap(count)),
                Err(e) => {
                    tracing::warn!("search: matches_missing_game_mode_count failed: {e}");
                    None
                }
            }
        });
    }

    /// Resolves the ids a seeded or restored query carries, so its pills read as
    /// names rather than as `#<id>`.
    ///
    /// Filtered by what has already been asked rather than by what came back:
    /// an id the index does not know answers with nothing, so filtering on the
    /// name cache alone would re-ask for it on every edit for as long as its
    /// pill was in the bar.
    fn resolve_names(&mut self, pool: &sqlx::SqlitePool, rt: &Runtime) {
        let (mut ships, mut players) = (Vec::new(), Vec::new());
        collect_ids(&self.bar.expr, &mut ships, &mut players);
        ships.retain(|id| self.asked_ships.insert(*id));
        players.retain(|id| self.asked_players.insert(*id));
        if ships.is_empty() && players.is_empty() {
            return;
        }
        let pool = pool.clone();
        self.spawn(rt, async move {
            let mut ship_names = Vec::new();
            for id in ships {
                match query::ship_name(&pool, id).await {
                    Ok(Some(name)) => ship_names.push((id, name)),
                    Ok(None) => {}
                    Err(e) => tracing::warn!("search: ship_name lookup failed for {id:?}: {e}"),
                }
            }
            let mut player_names = Vec::new();
            for id in players {
                match query::player_name(&pool, id).await {
                    Ok(Some(name)) => player_names.push((id, name)),
                    Ok(None) => {}
                    Err(e) => tracing::warn!("search: player_name lookup failed for {id:?}: {e}"),
                }
            }
            Some(SearchMsg::Names { ships: ship_names, players: player_names })
        });
    }

    /// Starts the debounce for a value lookup the bar asked for. A new request
    /// restarts it, so a lookup only runs once the caret has settled.
    ///
    /// Whatever lookup was already in flight is abandoned here as well. It was
    /// issued for a field the caret has since left, and a reply that still
    /// matched `awaiting_values` would fill the dropdown with rows from the old
    /// field: a ship row committed into an account term parses as an integer
    /// and searches for an account that does not exist.
    fn queue_value_request(&mut self, request: ValueRequest) {
        self.awaiting_values = None;
        self.debounced = Some((request, Instant::now()));
    }

    /// The rows for a request the tab can answer from a catalogue it already
    /// holds, or `None` for one that has to reach the index.
    fn local_value_options(&self, request: &ValueRequest) -> Option<Vec<ValueOption>> {
        match request {
            ValueRequest::Sources => Some(self.sources.iter().map(source_option).collect()),
            ValueRequest::Maps => Some(self.map_names.iter().map(|name| map_option(name)).collect()),
            ValueRequest::Players { .. } | ValueRequest::Ships { .. } => None,
        }
    }

    /// Dispatches the debounced lookup once it has sat still long enough, and
    /// reports how long is left otherwise so the tab can wake itself in time.
    fn service_value_request(&mut self, pool: &sqlx::SqlitePool, rt: &Runtime) -> Option<Duration> {
        let (_, asked_at) = self.debounced.as_ref()?;
        let waited = asked_at.elapsed();
        if waited < VALUE_DEBOUNCE {
            return Some(VALUE_DEBOUNCE - waited);
        }
        let (request, _) = self.debounced.take()?;
        // Both catalogues are already in hand, so these answer with no runtime
        // round trip. Answering locally still has to abandon whatever lookup was
        // outstanding: a player or ship reply that arrived afterwards would
        // otherwise still match `awaiting_values` and replace these rows,
        // leaving the caret typing a source or a map while the dropdown offered
        // accounts to commit.
        //
        // The rows are filled in after the bar has already drawn this frame, and
        // nothing is in flight to wake the tab afterwards, so the wake has to be
        // asked for here or the dropdown stays empty until the next input event.
        if let Some(options) = self.local_value_options(&request) {
            self.awaiting_values = None;
            self.bar.value_options = options;
            return Some(Duration::ZERO);
        }

        self.awaiting_values = Some(request.clone());
        let pool = pool.clone();
        self.spawn(rt, async move {
            let options = match &request {
                ValueRequest::Players { needle } => match query::search_players(&pool, needle, VALUE_LIMIT).await {
                    Ok(rows) => rows.iter().map(player_option).collect(),
                    Err(e) => {
                        tracing::warn!("search: search_players failed: {e}");
                        return None;
                    }
                },
                ValueRequest::Ships { needle } => match query::search_ships(&pool, needle, VALUE_LIMIT).await {
                    Ok(rows) => rows.iter().map(ship_option).collect(),
                    Err(e) => {
                        tracing::warn!("search: search_ships failed: {e}");
                        return None;
                    }
                },
                // Both are answered above without reaching the runtime.
                ValueRequest::Sources | ValueRequest::Maps => Vec::new(),
            };
            Some(SearchMsg::Values { request, options })
        });
        None
    }
}

fn player_option(facet: &crate::db::index::rows::PlayerFacet) -> ValueOption {
    let label = if facet.clan.is_empty() {
        facet.latest_name.clone()
    } else {
        format!("[{}] {}", facet.clan, facet.latest_name)
    };
    ValueOption { label, token: facet.account_id.raw().to_string() }
}

fn ship_option(facet: &crate::db::index::rows::ShipFacet) -> ValueOption {
    ValueOption { label: facet.ship_name.clone(), token: facet.ship_id.raw().to_string() }
}

fn source_option(source: &IndexSource) -> ValueOption {
    ValueOption { label: source.name.clone(), token: source.id.0.to_string() }
}

fn map_option(name: &str) -> ValueOption {
    ValueOption { label: name.to_owned(), token: quote_if_needed(name) }
}

/// The ship and account ids a query names. Only roster terms can carry one: the
/// match level has no ship or account field, because the perspective player is
/// the roster row whose relation is `self`.
fn collect_ids(expr: &MatchExpr, ships: &mut Vec<GameParamId>, players: &mut Vec<AccountId>) {
    match expr {
        Expr::Leaf(MatchTerm::Roster { pred, .. }) => collect_roster_ids(pred, ships, players),
        Expr::Leaf(MatchTerm::Field(..) | MatchTerm::FreeText(_)) => {}
        other => {
            for child in other.children() {
                collect_ids(child, ships, players);
            }
        }
    }
}

/// True when the tree names `MatchField::GameMode` anywhere, structurally --
/// a walk over the AST, not a substring search over the printed query -- so a
/// free-text term that happens to contain the literal word "game-mode" does
/// not count. `GameMode` is a match-level field, not a roster one, so this
/// only has to descend through `MatchTerm::Roster` far enough to say it holds
/// none, never into the `RosterExpr` itself.
fn references_game_mode(expr: &MatchExpr) -> bool {
    match expr {
        Expr::Leaf(MatchTerm::Field(MatchField::GameMode, ..)) => true,
        Expr::Leaf(_) => false,
        other => other.children().iter().any(references_game_mode),
    }
}

/// Whether the re-index hint belongs on screen: some indexed match is
/// missing its game mode, *and* the query on screen actually filters on it.
/// A nonzero gap the current query never asked about is not this user's
/// problem right now, so it stays quiet.
fn game_mode_gap_hint_relevant(missing_count: i64, expr: &MatchExpr) -> bool {
    missing_count > 0 && references_game_mode(expr)
}

/// The hint's text, singular at exactly one so it never reads "1 indexed
/// matches". Follows the same `count == 1` split the rest of the app uses
/// (see `set_session_stats_one` / `set_session_stats_many` in
/// `ui/replay_parser/mod.rs`); `rust_i18n`'s own key lookup has no built-in
/// pluralisation, so the split has to happen here.
fn game_mode_gap_hint_text(missing_count: i64) -> std::borrow::Cow<'static, str> {
    if missing_count == 1 {
        t!("ui.search.game_mode_gap_hint_one")
    } else {
        t!("ui.search.game_mode_gap_hint", count = missing_count)
    }
}

/// The results table's columns, left to right. Mirrors the order the body rows
/// are emitted in below; the action button at the far right is not one of them,
/// since it shows no data and has no header.
const RESULT_COLUMNS: [ResultColumn; 8] = [
    ResultColumn::Date,
    ResultColumn::Map,
    ResultColumn::Mode,
    ResultColumn::Ship,
    ResultColumn::Outcome,
    ResultColumn::Damage,
    ResultColumn::Kills,
    ResultColumn::Pr,
];

const fn column_label_key(column: ResultColumn) -> &'static str {
    match column {
        ResultColumn::Date => "ui.search.column.date",
        ResultColumn::Map => "ui.search.column.map",
        ResultColumn::Mode => "ui.search.column.mode",
        ResultColumn::Ship => "ui.search.column.ship",
        ResultColumn::Outcome => "ui.search.column.result",
        ResultColumn::Damage => "ui.search.column.damage",
        ResultColumn::Kills => "ui.search.column.kills",
        ResultColumn::Pr => "ui.search.column.pr",
    }
}

/// Phosphor's own naming is backwards from what the glyphs draw:
/// `icons::SORT_ASCENDING` (`\u{E444}`) is the arrow that points down (a
/// descending sort), and `icons::SORT_DESCENDING` (`\u{E446}`) is the arrow
/// that points up (an ascending sort). These constants are named for what
/// they draw on screen, not for phosphor's constant names, so the mapping
/// below reads as deliberate rather than as a swap waiting to be "fixed"
/// back to the wrong glyphs.
const ASCENDING_SORT_GLYPH: &str = icons::SORT_DESCENDING;
const DESCENDING_SORT_GLYPH: &str = icons::SORT_ASCENDING;

const fn direction_icon(direction: SortDirection) -> &'static str {
    match direction {
        SortDirection::Ascending => ASCENDING_SORT_GLYPH,
        SortDirection::Descending => DESCENDING_SORT_GLYPH,
    }
}

/// Draws one header cell, returning the column a click asked to sort by.
///
/// A column the index can order by senses clicks, so egui tints it on hover and
/// gives it the pointing-hand cursor, and it carries the current direction's
/// arrow while it is the sorted one. A column the index cannot order by is
/// drawn as a plain label: no explicit sense, so the text keeps its resting
/// colour under the pointer and the cursor stays the text caret. Nothing about
/// it reads as an offer, which is the point -- a header that looks clickable
/// and then sorts by something adjacent, or by nothing, is worse than one that
/// never offered.
fn header_cell(ui: &mut egui::Ui, column: ResultColumn, sort: SortSpec) -> Option<SortColumn> {
    let label = t!(column_label_key(column));
    let Some(sortable) = column.sort_column() else {
        ui.strong(label);
        return None;
    };
    let text = if sort.column == sortable {
        format!("{label} {}", direction_icon(sort.direction))
    } else {
        label.into_owned()
    };
    let clicked = ui
        .add(egui::Label::new(egui::RichText::new(text).strong()).sense(egui::Sense::click()))
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked();
    clicked.then_some(sortable)
}

/// Height of one results row.
const RESULT_ROW_HEIGHT: f32 = 24.0;
/// Height of the single header row above them.
const RESULT_HEADER_HEIGHT: f32 = 20.0;
/// Padding either side of a cell's content, so text in adjacent columns does
/// not run together across the separator.
const RESULT_CELL_MARGIN: i8 = 4;
/// What the table stores its column widths under. Held across frames by
/// `egui_table`, so a width the user drags to survives a scroll, a re-query and
/// an app restart.
const RESULT_TABLE_SALT: &str = "search_results";

/// The column at `col_nr`, or `None` for the trailing action column, which
/// shows no data and has no header.
fn column_at(col_nr: usize) -> Option<ResultColumn> {
    RESULT_COLUMNS.get(col_nr).copied()
}

/// The id `egui_table` files a column's stored width under. Keyed on the column
/// itself rather than on its position, so a width follows its own column if the
/// order ever changes instead of being inherited by whatever lands at that
/// index.
fn column_id(column: Option<ResultColumn>) -> egui::Id {
    match column {
        Some(column) => egui::Id::new(column_label_key(column)),
        None => egui::Id::new("search_results.actions"),
    }
}

/// Narrowest any column may be dragged.
///
/// Auto-sizing never lands here: a column's own header label is measured along
/// with its cells, so the header is the floor content sizing actually reaches.
/// This only stops a drag from collapsing a column to nothing.
const RESULT_COLUMN_MIN_WIDTH: f32 = 24.0;

/// The width a column first lays out at, and the widest it may be auto-sized or
/// dragged to, as `(initial, max)`.
///
/// The initial width only decides the very first layout. `egui_table` runs a
/// sizing pass the first time the table is shown and every column lands on its
/// own widest cell; afterwards a column grows to fit content that no longer
/// fits. The maxima are therefore ceilings on how far one outlier row may push
/// a column, not target widths.
const fn column_extent(column: ResultColumn) -> (f32, f32) {
    match column {
        ResultColumn::Date => (150.0, 260.0),
        ResultColumn::Map => (120.0, 400.0),
        ResultColumn::Mode => (90.0, 200.0),
        ResultColumn::Ship => (140.0, 320.0),
        ResultColumn::Outcome => (60.0, 120.0),
        ResultColumn::Damage => (80.0, 180.0),
        ResultColumn::Kills => (50.0, 120.0),
        ResultColumn::Pr => (60.0, 140.0),
    }
}

/// The action column holds three buttons whose combined width depends on the
/// locale's word for "Open", so it is sized from its content like every other
/// column rather than from a constant that only holds in English.
const ACTION_COLUMN_EXTENT: (f32, f32) = (120.0, 320.0);

/// Every column is resizable. That is what the user asked for, and it is also
/// what makes auto-sizing work at all: `egui_table` only records a measured
/// content width for a column it can resize, so a fixed column would sit at its
/// declared width and clip whatever did not fit.
fn result_columns() -> Vec<egui_table::Column> {
    RESULT_COLUMNS
        .iter()
        .map(|column| (column_id(Some(*column)), column_extent(*column)))
        .chain(std::iter::once((column_id(None), ACTION_COLUMN_EXTENT)))
        .map(|(id, (initial, max))| {
            egui_table::Column::new(initial).range(RESULT_COLUMN_MIN_WIDTH..=max).id(id).resizable(true)
        })
        .collect()
}

/// Draws the results table.
///
/// Everything the rows need is resolved before this is built, so a cell body
/// neither borrows the tab mutably nor reaches back into `TabState`. What the
/// pass asked for is collected into the trailing fields and read back once the
/// table has drawn, which is also what keeps the sort out of the widget: a
/// header click records a column here and the next frame's SQL applies it.
struct ResultsTable<'a> {
    hits: &'a [MatchHit],
    /// Per-hit ship name, resolved against whichever build's game data is
    /// loaded. Parallel to `hits`.
    ship_names: &'a [Option<String>],
    sort: SortSpec,
    locale: Option<&'a str>,
    /// Consulted only for a row whose dwell has already elapsed: resolving the
    /// dependencies builds the preview assets and uploads icon textures, which
    /// is far too much to do for every row the pointer crosses.
    preview_deps: &'a dyn Fn(&egui::Ui) -> Option<Arc<preview_popup::PreviewDeps>>,
    preview_state: &'a mut PreviewState,
    /// Wall time since the last frame, which is what the dwell accumulates.
    dwell_step: Duration,
    sort_clicked: Option<SortColumn>,
    open_path: Option<std::path::PathBuf>,
    copy_path: Option<std::path::PathBuf>,
    /// Whether any row's preview icon was under the pointer this frame.
    preview_icon_hovered: bool,
}

impl ResultsTable<'_> {
    fn data_cell_ui(&self, ui: &mut egui::Ui, column: ResultColumn, row: usize) {
        let hit = &self.hits[row];
        match column {
            ResultColumn::Date => {
                ui.label(hit.timestamp.strftime("%Y-%m-%d %H:%M").to_string());
            }
            ResultColumn::Map => {
                ui.label(&hit.map);
            }
            ResultColumn::Mode => {
                ui.label(&hit.game_type);
            }
            ResultColumn::Ship => {
                ui.label(self.ship_names[row].clone().unwrap_or_default());
            }
            ResultColumn::Outcome => {
                let letter = match hit.outcome {
                    MatchOutcome::Win => "W",
                    MatchOutcome::Loss => "L",
                    MatchOutcome::Draw => "D",
                    MatchOutcome::Unknown => "-",
                };
                let fill = outcome_colour(hit.outcome, ui.style());
                if hit.outcome == MatchOutcome::Unknown {
                    ui.colored_label(fill, letter);
                } else {
                    let text = label_on(fill);
                    ui.label(egui::RichText::new(letter).color(text).background_color(fill).strong());
                }
            }
            ResultColumn::Damage => {
                ui.label(
                    hit.self_damage
                        .map(|d| crate::util::formatting::separate_number(d, self.locale))
                        .unwrap_or_default(),
                );
            }
            ResultColumn::Kills => {
                ui.label(hit.self_kills.map(|k| k.to_string()).unwrap_or_default());
            }
            ResultColumn::Pr => {
                if let Some(pr) = hit.self_pr {
                    pr_chip(ui, PersonalRatingCategory::from_pr(pr), &format!("{pr:.0}"), false);
                }
            }
        }
    }

    /// Open, copy path, and the dwell-gated map preview, each acting on this
    /// row's own replay.
    fn actions_ui(&mut self, ui: &mut egui::Ui, row: usize) {
        let hit = &self.hits[row];
        let exists = hit.replay_path.exists();
        let btn = ui
            .add_enabled(exists, egui::Button::new(wt_translations::icon_t(icons::FOLDER_OPEN, &t!("ui.search.open"))));
        if !exists {
            btn.on_hover_text(t!("ui.search.open_missing"));
        } else if btn.clicked() {
            self.open_path = Some(hit.replay_path.clone());
        }

        let copy_btn = ui.add(egui::Button::new(icons::COPY)).on_hover_text(t!("ui.replay.context.copy_path"));
        if copy_btn.clicked() {
            self.copy_path = Some(copy_target(self.hits, row));
        }

        let Some(key) = hit_preview_key(hit) else {
            return;
        };
        let mut preview_response = ui.add(egui::Button::new(icons::FILM_STRIP));
        if !preview_response.hovered() {
            let _ = preview_response.on_hover_text(t!("ui.search.preview_map"));
            return;
        }

        self.preview_icon_hovered = true;
        self.preview_state.hover(key.clone(), self.dwell_step);
        if self.preview_state.pending_request().as_ref() == Some(&key)
            && let Some(deps) = (self.preview_deps)(ui)
        {
            let hover_text = preview_hover_text(hit, self.ship_names[row].as_deref());
            preview_response = preview_response.on_hover_ui(|ui| {
                preview_popup::preview_tooltip(ui, &deps, key.clone(), &hit.map, &hover_text);
            });
        } else {
            preview_response = preview_response.on_hover_text(t!("ui.search.preview_map"));
        }
        let _ = preview_response;
    }
}

impl egui_table::TableDelegate for ResultsTable<'_> {
    fn header_cell_ui(&mut self, ui: &mut egui::Ui, cell: &egui_table::HeaderCellInfo) {
        // The action column has no header, so nothing is drawn over it and
        // there is nothing there to click.
        let Some(column) = column_at(cell.group_index) else {
            return;
        };
        let sort = self.sort;
        egui::Frame::new().inner_margin(egui::Margin::symmetric(RESULT_CELL_MARGIN, 0)).show(ui, |ui| {
            if let Some(clicked) = header_cell(ui, column, sort) {
                self.sort_clicked = Some(clicked);
            }
        });
    }

    fn row_ui(&mut self, ui: &mut egui::Ui, row_nr: u64) {
        // Striping belongs here rather than in `cell_ui`, which runs afterwards
        // and would paint over the cell contents.
        if row_nr % 2 == 1 {
            ui.painter().rect_filled(ui.max_rect(), 0.0, ui.visuals().faint_bg_color);
        }
    }

    fn cell_ui(&mut self, ui: &mut egui::Ui, cell: &egui_table::CellInfo) {
        let row = cell.row_nr as usize;
        let column = column_at(cell.col_nr);
        // The frame allocates in the parent, so the margin and the content are
        // both part of the cell's `min_size`, which is what egui_table measures
        // a column's width from.
        egui::Frame::new().inner_margin(egui::Margin::symmetric(RESULT_CELL_MARGIN, 0)).show(ui, |ui| match column {
            Some(column) => self.data_cell_ui(ui, column, row),
            None => self.actions_ui(ui, row),
        });
    }

    fn default_row_height(&self) -> f32 {
        RESULT_ROW_HEIGHT
    }
}

/// Draws the table, returning the id its per-column widths are stored under.
///
/// `AutoSizeMode::Never` leaves the columns' total width alone rather than
/// redistributing it across the parent every frame; it does not disable the
/// per-column sizing this move is for, which runs off each column's own widest
/// measured cell.
fn show_results_table(ui: &mut egui::Ui, delegate: &mut ResultsTable<'_>) -> egui::Id {
    let id = egui_table::TableState::id(ui, egui::IdSalt::new(RESULT_TABLE_SALT));
    egui_table::Table::new()
        .id_salt(RESULT_TABLE_SALT)
        .num_rows(delegate.hits.len() as u64)
        .columns(result_columns())
        .headers([egui_table::HeaderRow { height: RESULT_HEADER_HEIGHT, groups: Default::default() }])
        .auto_size_mode(egui_table::AutoSizeMode::Never)
        .show(ui, delegate);
    id
}

fn collect_roster_ids(expr: &RosterExpr, ships: &mut Vec<GameParamId>, players: &mut Vec<AccountId>) {
    match expr {
        Expr::Leaf(term) => match &term.value {
            Value::Ship(id) => ships.push(*id),
            Value::Account(id) => players.push(*id),
            _ => {}
        },
        other => {
            for child in other.children() {
                collect_roster_ids(child, ships, players);
            }
        }
    }
}

/// The name stored on the roster row, when it names anything.
///
/// Rejected when empty, and when it is the bare id `UiReport::refresh_translations`
/// writes for a ship its own provider could not name: taking that would render a
/// naked number in the cell for good, even once a provider that can name the ship
/// is loaded.
fn stored_ship_name(hit: &MatchHit, ship_id: GameParamId) -> Option<&str> {
    let stored = hit.self_ship_name.as_deref()?;
    if stored.is_empty() || stored == ship_id.to_string() {
        return None;
    }
    Some(stored)
}

/// The display name for a hit's self ship, given whatever name the match's own
/// build resolves for it (`live`) when that build's game data is loaded.
///
/// `live` wins. It is the same source every other surface names ships from, so
/// preferring it keeps the tab in the app's current locale, while a stored name
/// is frozen in whatever locale was active when the match was indexed. The
/// stored name is the fallback for the case the whole fix exists for: a match
/// whose build's game data is no longer installed. A bracketed id is the last
/// resort when neither can name the ship.
fn ship_display_name(hit: &MatchHit, live: Option<String>) -> Option<String> {
    let ship_id = hit.self_ship_id?;
    if let Some(live) = live {
        return Some(live);
    }
    if let Some(stored) = stored_ship_name(hit, ship_id) {
        return Some(stored.to_owned());
    }
    Some(format!("[{ship_id}]"))
}

/// The chip fill for a match outcome, read off `ui.sem()`. `Unknown` reads
/// `text_dim` rather than a fourth chip tone: plenty of indexed rows carry no
/// recorded outcome, and colouring that absence as if it were a result would
/// be actively misleading, so it deliberately falls back to the same
/// de-emphasised tone every other "nothing recorded" cell in this table uses.
fn outcome_colour(outcome: MatchOutcome, style: &egui::Style) -> egui::Color32 {
    let sem = style.visuals.sem();
    match outcome {
        MatchOutcome::Win => sem.outcome_chip.win,
        MatchOutcome::Loss => sem.outcome_chip.loss,
        MatchOutcome::Draw => sem.outcome_chip.draw,
        MatchOutcome::Unknown => sem.text_dim,
    }
}

/// `PreviewKey` for `hit`, or `None` when the match has no recorded mtime.
///
/// Mirrors `preview_popup::preview_key`, which builds the same key from a
/// replay-listing row's `RowSummary`; a search hit carries its own mtime
/// directly on `MatchHit` and has no `RowSummary` to build one from.
fn hit_preview_key(hit: &MatchHit) -> Option<PreviewKey> {
    Some(PreviewKey { path: hit.replay_path.clone(), mtime_secs: hit.file_mtime? })
}

/// The line the preview popup shows below the map: when the match was played
/// and, when it is known, which ship. The map name and the rest of the
/// match's facts already have their own columns in the row the popup is
/// anchored to, so this stays short.
fn preview_hover_text(hit: &MatchHit, ship_name: Option<&str>) -> String {
    let when = hit.timestamp.strftime("%Y-%m-%d %H:%M").to_string();
    match ship_name {
        Some(name) => format!("{when} - {name}"),
        None => when,
    }
}

/// The path a copy-path click on `hits[index]` copies: that row's own path,
/// never the selected row's or the first row's. There is no row selection in
/// this table, so the button's own row is the only source that can be right.
fn copy_target(hits: &[MatchHit], index: usize) -> std::path::PathBuf {
    hits[index].replay_path.clone()
}

impl ToolkitTabViewer<'_> {
    /// [`ship_display_name`] against the game data for the match's own build,
    /// when that build is loaded.
    fn search_ship_display_name(&self, hit: &MatchHit) -> Option<String> {
        let ship_id = hit.self_ship_id?;
        let data = hit.version_build.and_then(|build| self.tab_state.wows_data_map.as_ref()?.get(build));
        let guard = data.as_ref().map(|d| d.read());
        let provider = guard.as_ref().and_then(|g| g.game_metadata.as_deref());
        ship_display_name(hit, crate::data::session_stats::try_resolve_ship_name(ship_id, provider))
    }

    pub fn build_search_tab(&mut self, ui: &mut egui::Ui) {
        let pool = self.tab_state.db_pool.clone();
        let rt = self.tab_state.tokio_runtime.clone();
        let seeded = self.tab_state.pending_search_query.take();

        // The persisted settings are the sort's only home, read fresh each
        // frame and written back on a header click, so the choice survives a
        // restart without a separate restore step to keep in step with it.
        let (locale, history, sort) = {
            let persisted = self.tab_state.persisted.read();
            let search = &persisted.settings.search;
            (
                persisted.settings.app.locale.clone(),
                search.history.iter().cloned().collect::<Vec<_>>(),
                search.sort_spec(),
            )
        };

        let search_tab = &mut self.tab_state.search_tab;
        search_tab.bar.names.locale.clone_from(&locale);
        search_tab.drain_replies();
        let seeded_now = seeded.is_some();
        if let Some(expr) = seeded {
            search_tab.set_query(expr);
        }

        let output = search_tab.bar.show(ui, &Deps { history: &history });
        if let Some(request) = output.request {
            search_tab.queue_value_request(request);
        }
        if output.changed {
            search_tab.dirty = true;
        }

        let mut wake_in = None;
        if let (Some(pool), Some(rt)) = (pool.as_ref(), rt.as_ref()) {
            if !search_tab.catalogues_requested {
                search_tab.catalogues_requested = true;
                let sources_pool = pool.clone();
                search_tab.spawn(rt, async move {
                    match query::list_sources(&sources_pool).await {
                        Ok(sources) => Some(SearchMsg::Sources(sources)),
                        Err(e) => {
                            tracing::warn!("search: list_sources failed: {e}");
                            None
                        }
                    }
                });
                let maps_pool = pool.clone();
                search_tab.spawn(rt, async move {
                    match query::distinct_maps(&maps_pool, VALUE_LIMIT).await {
                        Ok(maps) => Some(SearchMsg::Maps(maps)),
                        Err(e) => {
                            tracing::warn!("search: distinct_maps failed: {e}");
                            None
                        }
                    }
                });
            }
            wake_in = search_tab.service_value_request(pool, rt);
            if search_tab.dirty {
                search_tab.resolve_names(pool, rt);
                search_tab.dispatch_search(pool, rt, sort);
            }
        }
        if search_tab.in_flight > 0 {
            wake_in = Some(wake_in.map_or(POLL_INTERVAL, |left| left.min(POLL_INTERVAL)));
        }
        if let Some(wake_in) = wake_in {
            ui.ctx().request_repaint_after(wake_in);
        }

        // Mirror the query into persisted settings so it survives an app
        // restart; the background save task picks it up from there. A seeded
        // query counts: the user asked for it as much as a typed one.
        if output.changed || seeded_now {
            let text = print_query(&self.tab_state.search_tab.bar.expr);
            self.tab_state.persisted.write().settings.search.query = text;
        }

        ui.separator();

        // Only shown when it is relevant to what the user is looking at: a
        // gap in a filter they are not using is noise, and it goes away on its
        // own once every affected match is re-indexed (the count reads 0).
        if let Some(missing) = self.tab_state.search_tab.game_mode_gap
            && game_mode_gap_hint_relevant(missing, &self.tab_state.search_tab.bar.expr)
        {
            ui.colored_label(ui.sem().warn, game_mode_gap_hint_text(missing));
        }

        let count = self.tab_state.search_tab.results.len();
        ui.label(if self.tab_state.search_tab.truncated {
            t!("ui.search.match_count_truncated", count = count)
        } else {
            t!("ui.search.match_count", count = count)
        });

        // Taken out for the duration of the table so the delegate can hold it
        // mutably alongside the immutable borrows of `self` it also makes
        // (`search_ship_display_name`, `preview_deps`); put back once the table
        // is done drawing.
        let mut preview_state = std::mem::take(&mut self.tab_state.search_tab.preview_state);
        let dwell_step = Duration::from_secs_f32(ui.input(|i| i.stable_dt).max(0.0));

        let results = &self.tab_state.search_tab.results;
        // Resolved before the delegate takes its borrows, since naming a ship
        // reads the loaded game data through `self`.
        let ship_names: Vec<Option<String>> = results.iter().map(|hit| self.search_ship_display_name(hit)).collect();
        let viewer: &ToolkitTabViewer<'_> = self;
        let preview_deps = |ui: &egui::Ui| viewer.preview_deps(ui, true);

        let mut delegate = ResultsTable {
            hits: results,
            ship_names: &ship_names,
            sort,
            locale: locale.as_deref(),
            preview_deps: &preview_deps,
            preview_state: &mut preview_state,
            dwell_step,
            sort_clicked: None,
            open_path: None,
            copy_path: None,
            preview_icon_hovered: false,
        };
        show_results_table(ui, &mut delegate);
        let ResultsTable { sort_clicked, open_path, copy_path, preview_icon_hovered, .. } = delegate;

        // No row's preview icon was hovered this frame, whether the pointer
        // left the table entirely or is sitting over a different column:
        // either way nothing is dwelling anymore, so a return to the same
        // row later starts the dwell over rather than resuming it.
        if !preview_icon_hovered {
            preview_state.leave();
        }
        self.tab_state.search_tab.preview_state = preview_state;

        // The re-query happens next frame, since this frame's search was
        // already dispatched under the old sort before the header drew.
        if let Some(column) = sort_clicked {
            self.tab_state.persisted.write().settings.search.set_sort_spec(sort.after_click(column));
            self.tab_state.search_tab.dirty = true;
        }

        if let Some(path) = open_path
            && let Some(deps) = self.tab_state.replay_dependencies()
        {
            crate::update_background_task!(
                self.tab_state.background_tasks,
                deps.parse_replay_from_path(path, crate::task::ReplaySource::ManualOpen)
            );
        }

        if let Some(path) = copy_path {
            ui.ctx().copy_text(path.to_string_lossy().into_owned());
            self.tab_state.toasts.lock().success(t!("ui.search.path_copied"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::index::query_ast::Quant;
    use crate::db::index::query_ast::RosterField;
    use crate::db::index::query_ast::RosterTerm;
    use crate::db::index::rows::VehicleRelation;
    use crate::ui::query_bar::seed;
    use crate::ui::theme::contrast::CONTRAST_FLOOR as TEXT_CONTRAST_FLOOR;
    use crate::ui::theme::contrast::SURFACE_CONTRAST_FLOOR;
    use crate::ui::theme::contrast::contrast_ratio;
    use crate::ui::theme::style::dark_style;
    use crate::ui::theme::style::light_style;
    use crate::util::personal_rating::PersonalRatingCategorySwatch;

    fn row_key(i: usize) -> PreviewKey {
        PreviewKey { path: std::path::PathBuf::from(format!("row-{i}")), mtime_secs: 0 }
    }

    #[test]
    fn a_preview_is_requested_only_after_the_dwell_gate() {
        let mut state = PreviewState::default();
        state.hover(row_key(0), Duration::from_millis(50));
        assert!(state.pending_request().is_none(), "requested before the dwell elapsed");
        state.hover(row_key(0), PREVIEW_DWELL);
        assert!(state.pending_request().is_some(), "no request after the dwell elapsed");
    }

    #[test]
    fn moving_across_rows_does_not_queue_a_request_per_row() {
        // Scrolling a long table drags the pointer over every row in between.
        let mut state = PreviewState::default();
        for row in 0..20 {
            state.hover(row_key(row), Duration::from_millis(10));
        }
        assert!(state.pending_request().is_none(), "a fast pass queued work");
    }

    #[test]
    fn leaving_a_row_cancels_its_pending_preview() {
        let mut state = PreviewState::default();
        state.hover(row_key(0), PREVIEW_DWELL);
        assert!(state.pending_request().is_some());
        state.leave();
        assert!(state.pending_request().is_none(), "the request outlived the hover");
    }

    /// A proof wrapper over `PersonalRatingCategory`, kept test-only: render
    /// code calls `PersonalRatingCategory::from_pr` and `widgets::pr_chip`
    /// directly. `colour` reads the exact tier-solved tone `pr_chip` paints
    /// the label in, off the same `swatch` call, so a passing test here is a
    /// property of what actually renders rather than a parallel computation.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct PersonalRatingBand(PersonalRatingCategory);

    impl PersonalRatingBand {
        const ALL: [PersonalRatingBand; 8] = [
            PersonalRatingBand(PersonalRatingCategory::Bad),
            PersonalRatingBand(PersonalRatingCategory::BelowAverage),
            PersonalRatingBand(PersonalRatingCategory::Average),
            PersonalRatingBand(PersonalRatingCategory::Good),
            PersonalRatingBand(PersonalRatingCategory::VeryGood),
            PersonalRatingBand(PersonalRatingCategory::Great),
            PersonalRatingBand(PersonalRatingCategory::Unicum),
            PersonalRatingBand(PersonalRatingCategory::SuperUnicum),
        ];

        fn colour(&self, style: &egui::Style) -> egui::Color32 {
            self.0.swatch(&style.visuals).text
        }
    }

    /// One `MatchHit` per name, distinguished only by `replay_path`; every
    /// other field is the fixture default from `a_hit`, since the tests this
    /// feeds only ever check which path a row's own button carries.
    fn sample_hits(names: &[&str]) -> Vec<MatchHit> {
        names.iter().map(|name| MatchHit { replay_path: std::path::PathBuf::from(name), ..a_hit(None, None) }).collect()
    }

    /// A PR colour that vanishes into the table is worse than no colour.
    #[test]
    fn every_pr_band_is_distinguishable_from_the_row_background() {
        for band in PersonalRatingBand::ALL {
            for theme in [dark_style(), light_style()] {
                let ratio = contrast_ratio(band.colour(&theme), theme.visuals.panel_fill);
                assert!(ratio >= TEXT_CONTRAST_FLOOR, "{band:?} reads at {ratio:.3} against the table");
            }
        }
    }

    #[test]
    fn win_loss_and_draw_are_mutually_distinguishable() {
        for theme in [dark_style(), light_style()] {
            let win = outcome_colour(MatchOutcome::Win, &theme);
            let loss = outcome_colour(MatchOutcome::Loss, &theme);
            let draw = outcome_colour(MatchOutcome::Draw, &theme);
            for (a, b, names) in [(win, loss, "win/loss"), (win, draw, "win/draw"), (loss, draw, "loss/draw")] {
                assert!(contrast_ratio(a, b) >= SURFACE_CONTRAST_FLOOR, "{names} are too close");
            }
        }
    }

    #[test]
    fn an_unknown_outcome_is_not_dressed_as_a_result() {
        for theme in [dark_style(), light_style()] {
            let unknown = outcome_colour(MatchOutcome::Unknown, &theme);
            for known in [MatchOutcome::Win, MatchOutcome::Loss, MatchOutcome::Draw] {
                assert!(
                    contrast_ratio(unknown, outcome_colour(known, &theme)) >= SURFACE_CONTRAST_FLOOR,
                    "unknown reads as {known:?}"
                );
            }
        }
    }

    #[test]
    fn the_copy_action_carries_the_rows_own_path() {
        // The row's path, not the selected row's or the first row's.
        let hits = sample_hits(&["a.wowsreplay", "b.wowsreplay", "c.wowsreplay"]);
        for (index, hit) in hits.iter().enumerate() {
            assert_eq!(copy_target(&hits, index), hit.replay_path);
        }
    }

    /// Seeded queries carry bare ids, so the pill reads as `#<id>` until these
    /// are found and looked up. A walk that stopped at the roster boundary
    /// would find neither, since every id lives inside a quantifier.
    #[test]
    fn id_collection_reaches_inside_roster_predicates() {
        let ship = GameParamId::from(4_179_530_192_u64);
        let account = AccountId(1_234_567);
        let expr = Expr::All(vec![seed::my_matches_in_ship(ship), seed::matches_with_player(account)]);

        let (mut ships, mut players) = (Vec::new(), Vec::new());
        collect_ids(&expr, &mut ships, &mut players);
        assert_eq!(ships, vec![ship]);
        assert_eq!(players, vec![account]);
    }

    /// A negated term still names an id whose pill has to read as a name.
    #[test]
    fn id_collection_descends_through_negation_and_disjunction() {
        let account = AccountId(42);
        let expr: MatchExpr = Expr::Any(vec![
            Expr::Not(Box::new(seed::matches_with_player(account))),
            Expr::Leaf(MatchTerm::FreeText("ocean".into())),
        ]);

        let (mut ships, mut players) = (Vec::new(), Vec::new());
        collect_ids(&expr, &mut ships, &mut players);
        assert!(ships.is_empty());
        assert_eq!(players, vec![account]);
    }

    /// A relation is not an id, and asking the index to resolve one would be a
    /// query per pill that can never answer.
    #[test]
    fn id_collection_ignores_roster_values_that_are_not_ids() {
        let expr: MatchExpr = Expr::Leaf(MatchTerm::Roster {
            quant: Quant::Any,
            pred: Expr::Leaf(RosterTerm {
                field: RosterField::Relation,
                op: crate::db::index::query_ast::Op::Is,
                value: Value::Relation(VehicleRelation::Enemy),
            }),
        });

        let (mut ships, mut players) = (Vec::new(), Vec::new());
        collect_ids(&expr, &mut ships, &mut players);
        assert!(ships.is_empty() && players.is_empty());
    }

    /// A value reply belongs to the field the caret was on when it was asked
    /// for. Moving to another field inside the debounce window has to abandon
    /// it: a ships reply that still matched would fill the dropdown while the
    /// caret typed an account, and the ship id a committed row writes parses
    /// cleanly as an account number, so nothing downstream would notice.
    #[test]
    fn a_reply_to_a_superseded_value_request_never_reaches_the_dropdown() {
        let mut tab = SearchTabState::default();
        let ships = ValueRequest::Ships { needle: "yam".into() };
        tab.awaiting_values = Some(ships.clone());

        tab.queue_value_request(ValueRequest::Players { needle: "x".into() });
        let options = vec![ValueOption { label: "Yamato".into(), token: "4179530192".into() }];
        tab.tx.send(Some(SearchMsg::Values { request: ships, options })).expect("the tab owns the receiver");
        tab.drain_replies();

        assert!(tab.bar.value_options.is_empty(), "got {:?}", tab.bar.value_options);
    }

    /// A picked map name goes back into the caret as grammar text, so one with a
    /// space in it has to come back quoted or the term ends at the space.
    #[test]
    fn a_map_option_quotes_a_name_the_grammar_would_split() {
        assert_eq!(map_option("Ocean").token, "Ocean");
        let two_words = map_option("New Dawn");
        assert_eq!(two_words.label, "New Dawn");
        assert_eq!(two_words.token, "\"New Dawn\"");
    }

    fn a_hit(self_ship_id: Option<GameParamId>, self_ship_name: Option<&str>) -> MatchHit {
        MatchHit {
            arena_id: wows_replays::types::ArenaId::new(1),
            timestamp: jiff::Timestamp::UNIX_EPOCH,
            map: "spaces/13_OC_new_dawn".into(),
            game_mode: "Domination".into(),
            game_mode_id: None,
            game_type: "pvp".into(),
            match_group: "pvp".into(),
            version_build: Some(11_189_791),
            source_id: crate::db::index::rows::SourceId(1),
            outcome: MatchOutcome::Win,
            self_account_id: Some(AccountId(7)),
            self_ship_id,
            self_ship_name: self_ship_name.map(str::to_owned),
            self_survived: Some(true),
            self_damage: Some(80_000),
            self_kills: Some(2),
            self_pr: Some(1500.0),
            results_available: true,
            replay_path: std::path::PathBuf::from("1.wowsreplay"),
            file_mtime: Some(42),
        }
    }

    /// No live name stands in for the case the bug is about: a January replay
    /// whose build's game data is no longer installed.
    #[test]
    fn a_stored_name_is_shown_when_the_matchs_build_is_not_loaded() {
        let hit = a_hit(Some(GameParamId::from(4_074_649_424_u64)), Some("Smaland"));
        assert_eq!(ship_display_name(&hit, None).as_deref(), Some("Smaland"));
    }

    /// The stored name is frozen in the locale that was active when the match
    /// was indexed. Every other surface re-localizes, so a loaded build has to
    /// win or the tab drifts out of step with the rest of the app.
    #[test]
    fn a_loaded_build_outranks_the_stored_name() {
        let hit = a_hit(Some(GameParamId::from(4_074_649_424_u64)), Some("Smaland"));
        assert_eq!(ship_display_name(&hit, Some("Smolandia".into())).as_deref(), Some("Smolandia"));
    }

    #[test]
    fn a_hit_with_no_stored_name_falls_back_to_the_bracketed_id() {
        let hit = a_hit(Some(GameParamId::from(4_074_649_424_u64)), None);
        assert_eq!(ship_display_name(&hit, None).as_deref(), Some("[4074649424]"));
    }

    /// An empty stored name is not a name; showing it would leave the cell blank
    /// with no way to tell which ship the match was played in.
    #[test]
    fn an_empty_stored_name_is_not_treated_as_a_name() {
        let hit = a_hit(Some(GameParamId::from(4_074_649_424_u64)), Some(""));
        assert_eq!(ship_display_name(&hit, None).as_deref(), Some("[4074649424]"));
    }

    /// `refresh_translations` stores a bare id when its own provider cannot name
    /// the ship. Taking it would print a naked number that no later provider
    /// could ever displace.
    #[test]
    fn a_stored_bare_id_is_not_treated_as_a_name() {
        let hit = a_hit(Some(GameParamId::from(4_074_649_424_u64)), Some("4074649424"));
        assert_eq!(ship_display_name(&hit, None).as_deref(), Some("[4074649424]"));
    }

    #[test]
    fn a_hit_with_no_self_ship_names_nothing() {
        let hit = a_hit(None, Some("Smaland"));
        assert_eq!(ship_display_name(&hit, None), None);
    }

    /// The structural walk, not a substring search: a query that filters on
    /// `game-mode` is found regardless of where in the tree it sits.
    #[test]
    fn references_game_mode_finds_a_direct_term() {
        let expr = crate::db::index::query_text::parse_query("game-mode=arms-race").expect("parse");
        assert!(references_game_mode(&expr));
    }

    /// A field the query never mentions must not trip the detector; otherwise
    /// every query would show the hint.
    #[test]
    fn references_game_mode_is_false_when_the_query_names_no_such_field() {
        let expr = crate::db::index::query_text::parse_query("outcome=win").expect("parse");
        assert!(!references_game_mode(&expr));
    }

    /// The literal word "game-mode" typed as free text (no operator, so it
    /// parses as `FreeText`) must not be confused with the typed filter. This
    /// is the exact case the brief calls out: string-matching the printed
    /// query would get this wrong.
    #[test]
    fn references_game_mode_ignores_the_words_in_free_text() {
        let expr = crate::db::index::query_text::parse_query("game-mode").expect("parse");
        assert!(matches!(&expr, Expr::Leaf(MatchTerm::FreeText(s)) if s == "game-mode"), "got {expr:?}");
        assert!(!references_game_mode(&expr));
    }

    /// Reached through every kind of node that is not itself the leaf: `And`,
    /// `Or`, and `Not` all have to keep walking rather than stop at the first
    /// level.
    #[test]
    fn references_game_mode_is_found_however_deep_it_is_nested() {
        let and = crate::db::index::query_text::parse_query("outcome=win and game-mode=domination").expect("parse");
        assert!(references_game_mode(&and));

        let or = crate::db::index::query_text::parse_query("map:ocean or game-mode=domination").expect("parse");
        assert!(references_game_mode(&or));

        let not = crate::db::index::query_text::parse_query("not game-mode=domination").expect("parse");
        assert!(references_game_mode(&not));
    }

    /// `GameMode` is a match-level field. A roster predicate naming an
    /// unrelated field must not be mistaken for it just because it sits inside
    /// a quantifier the walk also has to cross.
    #[test]
    fn references_game_mode_is_false_for_an_unrelated_roster_predicate() {
        let expr = crate::db::index::query_text::parse_query("anyone.tier>=8").expect("parse");
        assert!(!references_game_mode(&expr));
    }

    /// The three properties the hint's gate has to hold, proven directly
    /// against the function the render call site uses. `missing_count` and
    /// "the query uses game mode" are independent conditions; each must be
    /// able to veto the hint on its own.
    #[test]
    fn the_hint_is_relevant_only_with_both_a_gap_and_a_matching_query() {
        let uses_game_mode = crate::db::index::query_text::parse_query("game-mode=arms-race").expect("parse");
        let does_not = crate::db::index::query_text::parse_query("outcome=win").expect("parse");

        assert!(
            game_mode_gap_hint_relevant(3, &uses_game_mode),
            "a nonzero gap plus a query that filters on game mode must show the hint"
        );
        assert!(
            !game_mode_gap_hint_relevant(0, &uses_game_mode),
            "a count of exactly zero must not show the hint, even though the query filters on game mode"
        );
        assert!(
            !game_mode_gap_hint_relevant(3, &does_not),
            "a nonzero gap must not show the hint when the query never asked about game mode"
        );
    }

    /// The exact wording at exactly one, so it reads "1 ... match has", not
    /// "1 ... matches".
    #[test]
    fn the_gap_hint_is_singular_at_exactly_one() {
        assert_eq!(
            game_mode_gap_hint_text(1).as_ref(),
            "1 indexed match has no game mode recorded. Re-index All Replays in Settings to fill it in."
        );
    }

    /// The plural form above one, with the count substituted rather than
    /// hardcoded.
    #[test]
    fn the_gap_hint_is_plural_above_one() {
        assert_eq!(
            game_mode_gap_hint_text(2).as_ref(),
            "2 indexed matches have no game mode recorded. Re-index All Replays in Settings to fill them in."
        );
    }

    /// `dispatch_search` is what the previously-shipped code refreshed
    /// `game_mode_gap` from; nothing exercised the whole path from a
    /// dispatched query through to the field a re-render reads. Run against a
    /// real database, since the property under test is that the SQL result
    /// actually lands in `SearchTabState`, not merely that the query
    /// compiles (that is `matches_missing_game_mode_count`'s own job in
    /// `wows-toolkit-config`).
    #[test]
    fn dispatch_search_populates_the_game_mode_gap_from_the_database() {
        let rt = tokio::runtime::Runtime::new().expect("failed to build test runtime");
        let pool = rt.block_on(async {
            let pool = sqlx::sqlite::SqlitePoolOptions::new()
                .max_connections(1)
                .connect("sqlite::memory:")
                .await
                .expect("open in-memory db");
            sqlx::migrate!("../wows-toolkit-config/migrations").run(&pool).await.expect("run migrations");
            let objective = |arena: i64, game_mode_id: Option<i32>| crate::db::index::rows::ObjectiveMatch {
                arena_id: wows_replays::types::ArenaId::new(arena),
                timestamp: jiff::Timestamp::from_second(1_700_000_000 + arena).unwrap(),
                map: "spaces/13_OC_new_dawn".into(),
                game_mode: "Domination".into(),
                game_mode_id,
                game_type: "pvp".into(),
                match_group: "pvp".into(),
                version_build: Some(1234),
            };
            crate::db::index::query::upsert_match(&pool, &objective(1, None)).await.unwrap();
            crate::db::index::query::upsert_match(&pool, &objective(2, Some(15))).await.unwrap();
            crate::db::index::query::upsert_match(&pool, &objective(3, None)).await.unwrap();
            pool
        });

        let mut tab = SearchTabState::default();
        assert_eq!(tab.game_mode_gap, None, "unfetched until a search has actually returned one");
        tab.dispatch_search(&pool, &rt, SortSpec::default());

        // Mirrors how the UI thread waits: poll drain_replies until the
        // background task's reply lands, or give up rather than hang.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while tab.game_mode_gap.is_none() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(5));
            tab.drain_replies();
        }
        assert_eq!(tab.game_mode_gap, Some(2), "two of the three seeded rows have no recorded game mode");
    }

    /// The wiring gap the review found: nothing marked the tab dirty when a
    /// background re-index completed, so a user who did exactly what the
    /// hint told them to do saw the identical stale count and results until
    /// they happened to edit the query by hand.
    #[test]
    fn a_completed_reindex_that_wrote_rows_marks_the_tab_dirty() {
        let mut tab = SearchTabState { dirty: false, ..Default::default() };
        tab.note_reindex_completed(5);
        assert!(tab.dirty, "a re-index that wrote rows must force the results and the gap count to refresh");
    }

    /// `indexed == 0` means the pass wrote nothing (every file was already
    /// indexed and a non-forced pass skipped them all), so there is nothing a
    /// refresh would find that the last one did not already see.
    #[test]
    fn a_completed_reindex_that_wrote_nothing_does_not_force_a_refresh() {
        let mut tab = SearchTabState { dirty: false, ..Default::default() };
        tab.note_reindex_completed(0);
        assert!(!tab.dirty);
    }

    /// Every column the index can order by has to be reachable from a header,
    /// and every header that offers a sort has to name one the index knows. A
    /// variant added on one side and forgotten on the other is either an
    /// ordering nothing can ask for or a header offering one that does not
    /// exist.
    #[test]
    fn the_header_offers_exactly_the_columns_the_index_can_order_by() {
        let offered: Vec<SortColumn> = RESULT_COLUMNS.iter().filter_map(|c| c.sort_column()).collect();
        for column in SortColumn::ALL {
            assert!(offered.contains(&column), "{column:?} can be ordered by but no header offers it");
        }
        assert_eq!(offered.len(), SortColumn::ALL.len(), "a header offers a column twice: {offered:?}");
    }

    /// The one displayed column with no ordering behind it. Its cell is
    /// composed on the UI thread from whichever build's game data is loaded,
    /// and the index holds only the name frozen at index time, so an ORDER BY
    /// over that column would put rows in an order the names on screen do not
    /// read in.
    #[test]
    fn the_ship_column_offers_no_sort() {
        assert_eq!(ResultColumn::Ship.sort_column(), None);
    }

    /// Clicking the sorted column reverses it; clicking any other starts that
    /// column at its own natural direction, so a first click on Damage reads
    /// highest-first rather than lowest-first.
    #[test]
    fn a_click_reverses_the_sorted_column_and_starts_any_other_at_its_own_direction() {
        let date_desc = SortSpec::default();
        assert_eq!(
            date_desc.after_click(SortColumn::Date),
            SortSpec { column: SortColumn::Date, direction: SortDirection::Ascending }
        );
        assert_eq!(
            date_desc.after_click(SortColumn::Damage),
            SortSpec { column: SortColumn::Damage, direction: SortDirection::Descending }
        );
        assert_eq!(
            date_desc.after_click(SortColumn::Map),
            SortSpec { column: SortColumn::Map, direction: SortDirection::Ascending }
        );
    }

    fn frame_input() -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 400.0))),
            ..Default::default()
        }
    }

    fn click_input(pos: egui::Pos2) -> egui::RawInput {
        let mut input = frame_input();
        input.events.push(egui::Event::PointerMoved(pos));
        for pressed in [true, false] {
            input.events.push(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
        }
        input
    }

    /// The header row driven through real frames.
    ///
    /// Fonts are left at egui's defaults rather than emptied, and the geometry
    /// comes back from a real pass rather than from `__run_test_ui`: an empty
    /// font set lays every galley out at zero size, and a click point derived
    /// from a zero-size rect lands outside the cell it is meant to be inside,
    /// so the test would pass for the wrong reason.
    struct HeaderHarness {
        ctx: egui::Context,
        sort: SortSpec,
        cells: Vec<(ResultColumn, egui::Rect)>,
        clicked: Option<SortColumn>,
    }

    impl HeaderHarness {
        fn new(sort: SortSpec) -> Self {
            let mut harness = Self { ctx: egui::Context::default(), sort, cells: Vec::new(), clicked: None };
            // Two quiet passes first: an input is applied at the end of the
            // pass carrying it, and egui swaps `this_pass` with `prev_pass`
            // rather than clearing it, so a rect read straight after the first
            // pass is not yet the one the row settles at.
            harness.frame(frame_input());
            harness.frame(frame_input());
            harness
        }

        fn frame(&mut self, input: egui::RawInput) {
            let sort = self.sort;
            let mut cells = Vec::new();
            let mut clicked = None;
            let _ = self.ctx.run_ui(input, |ui| {
                ui.horizontal(|ui| {
                    for column in RESULT_COLUMNS {
                        let cell = ui.scope(|ui| header_cell(ui, column, sort));
                        cells.push((column, cell.response.rect));
                        if let Some(picked) = cell.inner {
                            clicked = Some(picked);
                        }
                    }
                });
            });
            self.cells = cells;
            self.clicked = clicked;
        }

        /// What a click in the middle of `column`'s header asked to sort by.
        fn click(&mut self, column: ResultColumn) -> Option<SortColumn> {
            let rect = self
                .cells
                .iter()
                .find(|(c, _)| *c == column)
                .map(|(_, rect)| *rect)
                .unwrap_or_else(|| panic!("{column:?} drew no header"));
            assert!(
                rect.width() > 1.0 && rect.height() > 1.0,
                "{column:?} laid out at {rect:?}; a click derived from that rect proves nothing"
            );
            self.frame(click_input(rect.center()));
            self.clicked
        }
    }

    /// Driven through real frames rather than by calling the pieces: the
    /// property is that the cell actually senses a pointer press where it is
    /// drawn, which nothing short of egui's own hit testing can answer.
    #[test]
    fn clicking_a_sortable_header_asks_the_index_to_order_by_it() {
        let mut harness = HeaderHarness::new(SortSpec::default());
        assert_eq!(harness.click(ResultColumn::Damage), Some(SortColumn::Damage));
        assert_eq!(harness.click(ResultColumn::Pr), Some(SortColumn::Pr));
    }

    /// The affordance the whole exclusion is about: the Ship header must not
    /// answer a click at all, rather than answer it by sorting on something
    /// the cell does not display.
    #[test]
    fn clicking_the_ship_header_asks_for_nothing() {
        let mut harness = HeaderHarness::new(SortSpec::default());
        assert_eq!(harness.click(ResultColumn::Ship), None, "the ship header must not be a click target");
    }

    /// A quiet frame must not look like a click, or the sort would cycle on
    /// its own every time the tab repainted.
    #[test]
    fn a_frame_with_no_pointer_press_asks_for_nothing() {
        let mut harness = HeaderHarness::new(SortSpec::default());
        harness.frame(frame_input());
        assert_eq!(harness.clicked, None);
    }

    /// What the header row actually paints, read back off the frame rather
    /// than rebuilt from the same expressions the code under test uses.
    fn painted_header_texts(sort: SortSpec) -> Vec<String> {
        let ctx = egui::Context::default();
        let output = ctx.run_ui(frame_input(), |ui| {
            ui.horizontal(|ui| {
                for column in RESULT_COLUMNS {
                    header_cell(ui, column, sort);
                }
            });
        });
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
                _ => None,
            })
            .collect()
    }

    /// Only the sorted column carries an arrow, or every header would claim to
    /// be the one in force.
    #[test]
    fn only_the_sorted_columns_header_carries_a_direction_arrow() {
        let sort = SortSpec { column: SortColumn::Damage, direction: SortDirection::Ascending };
        let painted = painted_header_texts(sort);
        assert_eq!(painted.len(), RESULT_COLUMNS.len(), "every header draws exactly once: {painted:?}");

        let arrowed: Vec<&String> = painted
            .iter()
            .filter(|text| text.contains(ASCENDING_SORT_GLYPH) || text.contains(DESCENDING_SORT_GLYPH))
            .collect();
        assert_eq!(arrowed.len(), 1, "exactly one header may claim the sort: {painted:?}");
        assert!(arrowed[0].starts_with("Damage"), "the arrow belongs to the sorted column: {:?}", arrowed[0]);
        // Ascending was requested, so the header must carry the glyph that
        // visually draws an ascending (upward) arrow -- not the phosphor
        // constant that happens to be named SORT_ASCENDING, which draws the
        // opposite.
        assert!(arrowed[0].contains(ASCENDING_SORT_GLYPH), "the arrow must read the way it sorts: {:?}", arrowed[0]);
    }

    /// The arrow is the only thing that says which way it sorts, so reversing
    /// the direction has to reverse it.
    #[test]
    fn the_arrow_follows_the_direction() {
        let descending =
            painted_header_texts(SortSpec { column: SortColumn::Damage, direction: SortDirection::Descending });
        let damage = descending.iter().find(|text| text.starts_with("Damage")).expect("the damage header draws");
        // Descending was requested, so the header must carry the glyph that
        // visually draws a descending (downward) arrow, and must not carry
        // the one that draws an ascending arrow.
        assert!(damage.contains(DESCENDING_SORT_GLYPH), "got {damage:?}");
        assert!(!damage.contains(ASCENDING_SORT_GLYPH), "got {damage:?}");
    }

    /// What one pass of the table asked the tab to do.
    #[derive(Default, Clone, PartialEq, Eq, Debug)]
    struct TablePass {
        sort_clicked: Option<SortColumn>,
        open_path: Option<std::path::PathBuf>,
        copy_path: Option<std::path::PathBuf>,
    }

    /// The whole results table driven through real frames.
    ///
    /// Geometry has to come from real passes. `egui::__run_test_ui`, which
    /// `with_ui` runs, lays every glyph out at zero size, so a width read
    /// through it comes back at the column's floor and an assertion about
    /// auto-sizing would hold without anything having been measured.
    struct TableHarness {
        ctx: egui::Context,
        hits: Vec<MatchHit>,
        ship_names: Vec<Option<String>>,
        sort: SortSpec,
        preview_state: PreviewState,
        /// Wall time each frame credits to whichever preview icon is hovered.
        /// Zero unless a test is about the dwell, so probing the action cell
        /// cannot accidentally arm a preview.
        dwell_step: Duration,
        table_id: egui::Id,
        asked: TablePass,
        /// What the last pass painted, so a test can aim at where a widget
        /// actually drew rather than at where column arithmetic says it should
        /// be. A point computed the second way is the classic vacuous UI test:
        /// it misses everything, and every "nothing was clicked" assertion
        /// holds for the wrong reason.
        shapes: Vec<egui::epaint::ClippedShape>,
    }

    impl TableHarness {
        /// A table over `hits` with no ship names resolved, since none of these
        /// tests turn on what the Ship cell reads.
        fn new(hits: Vec<MatchHit>) -> Self {
            let ship_names = vec![None; hits.len()];
            let mut harness = Self {
                ctx: egui::Context::default(),
                hits,
                ship_names,
                sort: SortSpec::default(),
                preview_state: PreviewState::default(),
                dwell_step: Duration::ZERO,
                table_id: egui::Id::NULL,
                asked: TablePass::default(),
                shapes: Vec::new(),
            };
            harness.settle();
            harness
        }

        fn frame(&mut self, input: egui::RawInput) {
            let hits = &self.hits;
            let ship_names = &self.ship_names;
            let sort = self.sort;
            let preview_state = &mut self.preview_state;
            let dwell_step = self.dwell_step;
            // A preview needs loaded game data and a texture cache, neither of
            // which exists here; nothing in these tests dwells long enough to
            // ask for one.
            let no_previews: &dyn Fn(&egui::Ui) -> Option<Arc<preview_popup::PreviewDeps>> = &|_ui| None;
            let mut table_id = None;
            let mut asked = TablePass::default();
            let output = self.ctx.run_ui(input, |ui| {
                let mut delegate = ResultsTable {
                    hits,
                    ship_names,
                    sort,
                    locale: None,
                    preview_deps: no_previews,
                    preview_state: &mut *preview_state,
                    dwell_step,
                    sort_clicked: None,
                    open_path: None,
                    copy_path: None,
                    preview_icon_hovered: false,
                };
                table_id = Some(show_results_table(ui, &mut delegate));
                asked = TablePass {
                    sort_clicked: delegate.sort_clicked,
                    open_path: delegate.open_path,
                    copy_path: delegate.copy_path,
                };
            });
            if let Some(id) = table_id {
                self.table_id = id;
            }
            self.asked = asked;
            self.shapes = output.shapes;
        }

        /// Runs quiet frames until the widths and rects report what the table
        /// has settled at.
        ///
        /// Several, for lags that stack. An input is applied at the end of the
        /// pass carrying it, so that pass still draws the state before it, and
        /// `read_response` answers out of `this_pass`, which `end_pass` *swaps*
        /// with `prev_pass` rather than clearing. On top of that egui_table
        /// measures a column on one pass and lays it out at the measured width
        /// on the next.
        fn settle(&mut self) {
            for _ in 0..4 {
                self.frame(frame_input());
            }
        }

        /// The width egui_table has stored for a column, which is what it lays
        /// the column out at on the following frame.
        fn column_width(&self, column: Option<ResultColumn>) -> f32 {
            let state = egui_table::TableState::load(&self.ctx, self.table_id)
                .unwrap_or_else(|| panic!("the table stored no state under {:?}", self.table_id));
            let id = column_id(column);
            *state.col_widths.get(&id).unwrap_or_else(|| {
                panic!("no stored width for {column:?} under {id:?}; stored: {:?}", state.col_widths)
            })
        }

        fn rect_of(&self, id: egui::Id) -> egui::Rect {
            self.ctx.read_response(id).unwrap_or_else(|| panic!("no widget registered under {id:?}")).rect
        }

        fn resizer_id(&self, col_nr: usize) -> egui::Id {
            self.table_id.with(column_id(column_at(col_nr))).with("resize")
        }

        /// The x of `col_nr`'s right edge, read off the resizer egui_table
        /// registered there rather than summed from the declared widths, so a
        /// probe aims where the column actually is.
        fn column_right(&self, col_nr: usize) -> f32 {
            self.rect_of(self.resizer_id(col_nr)).center().x
        }

        fn column_left(&self, col_nr: usize) -> f32 {
            self.column_right(col_nr) - self.column_width(column_at(col_nr))
        }

        /// Where every painted text containing `needle` was drawn, in paint
        /// order, which for the body is row order.
        fn text_rects(&self, needle: &str) -> Vec<egui::Rect> {
            self.shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) if text.galley.text().contains(needle) => {
                        Some(egui::Rect::from_min_size(text.pos, text.galley.size()))
                    }
                    _ => None,
                })
                .collect()
        }

        /// The one place `needle` was painted, as a click point. Panics unless
        /// it was drawn exactly once at a non-degenerate size, so a test can
        /// never end up aiming at a rect that is not there.
        fn only_text_at(&self, needle: &str) -> egui::Pos2 {
            let rects = self.text_rects(needle);
            assert_eq!(rects.len(), 1, "{needle:?} was painted {} times, wanted once", rects.len());
            let rect = rects[0];
            assert!(rect.width() > 1.0 && rect.height() > 1.0, "{needle:?} laid out at {rect:?}");
            rect.center()
        }

        /// Clicks `pos` and reports what that pass asked the tab to do.
        ///
        /// egui applies the input at the start of the pass carrying it and
        /// hit-tests against the rects the previous pass registered, so the
        /// answer belongs to that pass and has to be taken before the quiet
        /// frame that follows overwrites it.
        fn click(&mut self, pos: egui::Pos2) -> TablePass {
            self.frame(click_input(pos));
            let asked = self.asked.clone();
            // The pointer stays where it was left, so a quiet frame clears the
            // press before the next probe is aimed somewhere else.
            self.frame(frame_input());
            asked
        }
    }

    /// Where a column sits in the table, counting the action column as the one
    /// past the displayed ones.
    fn column_number(column: Option<ResultColumn>) -> usize {
        match column {
            Some(column) => RESULT_COLUMNS.iter().position(|c| *c == column).expect("a displayed column"),
            None => RESULT_COLUMNS.len(),
        }
    }

    /// Three real map names of clearly different lengths. The longest is longer
    /// than the Map column's declared 120 px so a fixed column would clip it,
    /// and still short of that column's own ceiling so what is measured is the
    /// content rather than the clamp.
    const SHORT_MAP: &str = "Ocean";
    const MEDIUM_MAP: &str = "Islands of Ice";
    const LONG_MAP: &str = "Northern Waters Approach";

    fn hit_on_map(map: &str) -> MatchHit {
        MatchHit { map: map.to_owned(), ..a_hit(None, None) }
    }

    fn map_column_width(map: &str) -> f32 {
        TableHarness::new(vec![hit_on_map(map)]).column_width(Some(ResultColumn::Map))
    }

    /// The property the whole move to `egui_table` exists for: a column is as
    /// wide as what is in it, not as wide as a number written next to it.
    ///
    /// Three lengths rather than two, so a column pinned at any one value --
    /// its declared width, its floor, or its ceiling -- fails rather than
    /// happening to satisfy a one-sided comparison.
    #[test]
    fn a_column_sizes_itself_to_the_widest_thing_in_it() {
        let short = map_column_width(SHORT_MAP);
        let medium = map_column_width(MEDIUM_MAP);
        let long = map_column_width(LONG_MAP);

        assert!(
            long > medium && medium > short,
            "the column must track its content: {SHORT_MAP} {short}, {MEDIUM_MAP} {medium}, {LONG_MAP} {long}"
        );

        let (declared, ceiling) = column_extent(ResultColumn::Map);
        assert!(long < ceiling, "the long name must be sized to, not clamped at, the ceiling: {long} vs {ceiling}");
        assert!(short > RESULT_COLUMN_MIN_WIDTH, "the short name sank to the drag floor: {short}");
        // The point of the move: a column is not stuck at the number written
        // next to it. One of these names has to be on each side of it.
        assert!(short < declared && long > declared, "nothing straddled the declared width {declared}");
    }

    /// The half a widening-only test cannot state: a short cell must leave the
    /// column narrower than a long one does, rather than every row reserving
    /// the widest width the column could ever take.
    #[test]
    fn a_short_cell_does_not_reserve_a_long_cells_width() {
        let short = map_column_width(SHORT_MAP);
        let long = map_column_width(LONG_MAP);
        assert!(short < long, "a short name must not claim a long name's width: {short} vs {long}");
    }

    /// Sizing is per column: a long map name must widen the Map column and
    /// leave its neighbours alone. Without this, a table that simply grew every
    /// column together would pass the tests above.
    #[test]
    fn one_columns_content_does_not_widen_another() {
        let short = TableHarness::new(vec![hit_on_map(SHORT_MAP)]);
        let long = TableHarness::new(vec![hit_on_map(LONG_MAP)]);
        assert_eq!(
            short.column_width(Some(ResultColumn::Date)),
            long.column_width(Some(ResultColumn::Date)),
            "the Date cells are identical in both, so the column may not differ"
        );
    }

    /// A column the pointer drags has to end up at the width it was dragged to
    /// and stay there once the pointer is gone: egui_table re-derives widths
    /// every frame, so a width that was not stored is a width that snaps back.
    #[test]
    fn a_dragged_column_keeps_the_width_it_was_dragged_to() {
        const DRAG: f32 = 60.0;
        let mut harness = TableHarness::new(vec![hit_on_map(SHORT_MAP)]);
        let before = harness.column_width(Some(ResultColumn::Map));

        let handle = harness.rect_of(harness.resizer_id(column_number(Some(ResultColumn::Map)))).center();
        let target = egui::Pos2::new(handle.x + DRAG, handle.y);

        let mut press = frame_input();
        press.events.push(egui::Event::PointerMoved(handle));
        press.events.push(egui::Event::PointerButton {
            pos: handle,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        });
        harness.frame(press);

        // Two frames holding the pointer at the target: the first is what egui
        // promotes the press into a drag on, the second is the one egui_table
        // reads `pointer_latest_pos` from to widen the column.
        for _ in 0..2 {
            let mut drag = frame_input();
            drag.events.push(egui::Event::PointerMoved(target));
            harness.frame(drag);
        }

        let mut release = frame_input();
        release.events.push(egui::Event::PointerButton {
            pos: target,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        });
        harness.frame(release);

        let dragged = harness.column_width(Some(ResultColumn::Map));
        assert!(dragged > before + DRAG / 2.0, "the drag did not widen the column: {before} to {dragged}");

        // Quiet frames with the pointer gone: what the drag settled on has to
        // survive them rather than being recomputed from the content.
        harness.settle();
        let after = harness.column_width(Some(ResultColumn::Map));
        assert!((after - dragged).abs() < 1.0, "the dragged width did not survive a frame: {dragged} became {after}");
    }

    /// Every sortable header still cycles its own column's sort once the table
    /// draws it. This is the part the move to a delegate could quietly break:
    /// header cells are now addressed by a column index rather than emitted in
    /// order, so an off-by-one would sort by a neighbour.
    fn column_label(column: ResultColumn) -> String {
        t!(column_label_key(column)).into_owned()
    }

    /// Every sort a sweep across `column`'s header cell asked for.
    ///
    /// The sweep runs at the height the header labels were actually painted at,
    /// taken from a label that is provably there. A y derived from column
    /// arithmetic instead can miss the header band entirely, at which point
    /// every "asked for nothing" assertion below holds for the wrong reason.
    fn sorts_asked_across_header(harness: &mut TableHarness, column: Option<ResultColumn>) -> Vec<SortColumn> {
        let y = harness.only_text_at(&column_label(ResultColumn::Date)).y;
        let col_nr = column_number(column);
        let (left, right) = (harness.column_left(col_nr), harness.column_right(col_nr));
        let mut asked = Vec::new();
        let mut x = left + 2.0;
        while x < right {
            if let Some(got) = harness.click(egui::Pos2::new(x, y)).sort_clicked {
                asked.push(got);
            }
            x += 4.0;
        }
        asked
    }

    /// Every sortable header still cycles its own column's sort once the table
    /// draws it. This is what the move to a delegate could quietly break:
    /// header cells are addressed by a column index rather than emitted in
    /// order, so an off-by-one would sort by a neighbour.
    #[test]
    fn every_sortable_header_asks_the_index_to_order_by_its_own_column() {
        let mut harness = TableHarness::new(vec![hit_on_map(SHORT_MAP)]);
        let mut exercised = 0;
        for column in RESULT_COLUMNS {
            let Some(expected) = column.sort_column() else {
                continue;
            };
            exercised += 1;
            // The label itself, which is where a user clicks.
            let label = harness.only_text_at(&column_label(column));
            let asked = harness.click(label).sort_clicked;
            assert_eq!(asked, Some(expected), "{column:?}'s header at {label:?} asked for {asked:?}");
        }
        assert_eq!(exercised, SortColumn::ALL.len(), "every orderable column must have been exercised");
    }

    /// The affordance the Ship exclusion is about: its header must answer a
    /// click with nothing rather than sort on something its cell does not
    /// display. Its label is clicked directly, and then the rest of its cell is
    /// swept, so this says nothing anywhere in the header is a click target.
    #[test]
    fn the_ship_header_asks_for_nothing_when_clicked_in_the_table() {
        let mut harness = TableHarness::new(vec![hit_on_map(SHORT_MAP)]);
        let label = harness.only_text_at(&column_label(ResultColumn::Ship));
        assert_eq!(harness.click(label).sort_clicked, None, "the ship label must not be a click target");

        let asked = sorts_asked_across_header(&mut harness, Some(ResultColumn::Ship));
        assert!(asked.is_empty(), "something in the ship header asked for {asked:?}");
    }

    /// The action column has no header at all, so nothing there may reach a
    /// sort either.
    #[test]
    fn the_action_header_asks_for_nothing_when_clicked_in_the_table() {
        let mut harness = TableHarness::new(vec![hit_on_map(SHORT_MAP)]);
        let asked = sorts_asked_across_header(&mut harness, None);
        assert!(asked.is_empty(), "the action header asked for {asked:?}");
    }

    /// The sweep the two tests above lean on has to be capable of finding a
    /// click target. Run over a header that does sort, it must find one; that
    /// is what makes their empty results mean something.
    #[test]
    fn the_header_sweep_finds_a_sortable_header() {
        let mut harness = TableHarness::new(vec![hit_on_map(SHORT_MAP)]);
        let asked = sorts_asked_across_header(&mut harness, Some(ResultColumn::Damage));
        assert!(!asked.is_empty(), "the sweep found nothing on a header that does sort");
        assert!(asked.iter().all(|got| *got == SortColumn::Damage), "the sweep strayed off Damage: {asked:?}");
    }

    /// A quiet frame must not look like a click, or the sort would cycle on its
    /// own every time the tab repainted.
    #[test]
    fn a_quiet_frame_over_the_table_asks_for_no_sort() {
        let mut harness = TableHarness::new(vec![hit_on_map(SHORT_MAP)]);
        harness.frame(frame_input());
        assert_eq!(harness.asked.sort_clicked, None);
    }

    /// Three real files, so the Open button is enabled: it is disabled for a
    /// path that does not exist, and a disabled button answers no click.
    fn hits_with_real_files(dir: &std::path::Path) -> Vec<MatchHit> {
        (0..3)
            .map(|i| {
                let path = dir.join(format!("row-{i}.wowsreplay"));
                std::fs::write(&path, b"").expect("write the fixture replay");
                MatchHit { replay_path: path, ..a_hit(None, None) }
            })
            .collect()
    }

    /// Where every row painted `needle`, top to bottom, which is row order.
    ///
    /// One per row, asserted, rather than assumed: this is what says the button
    /// exists on the row a click is about to be aimed at.
    fn button_per_row(harness: &TableHarness, needle: &str, rows: usize) -> Vec<egui::Pos2> {
        let mut rects = harness.text_rects(needle);
        rects.sort_by(|a, b| a.top().total_cmp(&b.top()));
        assert_eq!(rects.len(), rows, "{needle:?} was painted {} times over {rows} rows", rects.len());
        rects.into_iter().map(|rect| rect.center()).collect()
    }

    /// The row's own replay, not the first row's. Rows are drawn by index out
    /// of a slice rather than by an iterator carrying the hit with it, so an
    /// off-by-one here would open somebody else's replay.
    #[test]
    fn each_rows_action_buttons_carry_that_rows_own_replay() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let hits = hits_with_real_files(dir.path());
        let distinct: HashSet<&std::path::PathBuf> = hits.iter().map(|hit| &hit.replay_path).collect();
        assert_eq!(distinct.len(), hits.len(), "the fixture rows must be distinguishable: {hits:?}");

        let mut harness = TableHarness::new(hits.clone());
        let opens = button_per_row(&harness, &t!("ui.search.open"), hits.len());
        let copies = button_per_row(&harness, icons::COPY, hits.len());

        for (row, hit) in hits.iter().enumerate() {
            let opened = harness.click(opens[row]).open_path;
            assert_eq!(opened.as_deref(), Some(hit.replay_path.as_path()), "row {row}'s open button opened {opened:?}");

            let copied = harness.click(copies[row]).copy_path;
            assert_eq!(copied.as_deref(), Some(hit.replay_path.as_path()), "row {row}'s copy button copied {copied:?}");
        }
    }

    /// The film strip is the third action, and the only one that is not a
    /// one-shot: hovering it has to credit the dwell to the row under the
    /// pointer, since that gate is what stops a fast scroll from queuing a
    /// preview bake per row it crossed.
    #[test]
    fn hovering_a_rows_film_strip_arms_a_preview_for_that_row() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let hits = hits_with_real_files(dir.path());
        let mut harness = TableHarness::new(hits.clone());
        let strips = button_per_row(&harness, icons::FILM_STRIP, hits.len());

        // A whole dwell's worth per frame, so one hovered frame arms it.
        harness.dwell_step = PREVIEW_DWELL;
        let mut hover = frame_input();
        hover.events.push(egui::Event::PointerMoved(strips[1]));
        harness.frame(hover);

        let armed = harness.preview_state.pending_request();
        assert_eq!(armed, hit_preview_key(&hits[1]), "the dwell must arm the hovered row's own replay, not another's");
        assert_ne!(armed, hit_preview_key(&hits[0]), "the fixture rows must be distinguishable by preview key");
    }

    /// The same gate from the other side: a pointer that crosses a row without
    /// resting on it must arm nothing, which is the whole reason the dwell
    /// exists.
    #[test]
    fn crossing_a_film_strip_without_resting_arms_nothing() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let hits = hits_with_real_files(dir.path());
        let mut harness = TableHarness::new(hits.clone());
        let strips = button_per_row(&harness, icons::FILM_STRIP, hits.len());

        harness.dwell_step = PREVIEW_DWELL / 10;
        for strip in &strips {
            let mut hover = frame_input();
            hover.events.push(egui::Event::PointerMoved(*strip));
            harness.frame(hover);
        }
        assert_eq!(harness.preview_state.pending_request(), None, "a pass across the rows armed a preview");
    }

    /// The two buttons are separate actions on the same row. A cell that wired
    /// both to one response would pass the test above, since each assertion
    /// only reads the field it is about.
    #[test]
    fn the_open_and_copy_buttons_are_not_the_same_button() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let hits = hits_with_real_files(dir.path());
        let mut harness = TableHarness::new(hits.clone());
        let opens = button_per_row(&harness, &t!("ui.search.open"), hits.len());
        let copies = button_per_row(&harness, icons::COPY, hits.len());
        assert_ne!(opens[0], copies[0], "the two buttons drew on top of each other");

        let by_open = harness.click(opens[0]);
        assert!(by_open.copy_path.is_none(), "the open button also copied: {by_open:?}");
        let by_copy = harness.click(copies[0]);
        assert!(by_copy.open_path.is_none(), "the copy button also opened: {by_copy:?}");
    }
}
