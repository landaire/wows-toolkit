//! The dockable Search tab: a query bar over a results table backed by the
//! replay index.

use std::collections::HashSet;
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
use crate::db::index::query;
use crate::db::index::query_ast::Expr;
use crate::db::index::query_ast::MapCatalog;
use crate::db::index::query_ast::MatchExpr;
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
use crate::ui::query_bar::Deps;
use crate::ui::query_bar::QueryBar;
use crate::ui::query_bar::select::prune_empty;
use crate::ui::query_bar::suggest::ValueOption;
use crate::ui::query_bar::suggest::ValueRequest;

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
}

pub struct SearchTabState {
    pub bar: QueryBar,
    pub results: Vec<MatchHit>,
    pub dirty: bool,
    /// True when the last query returned `limit + 1` rows.
    pub truncated: bool,
    pub sources: Vec<IndexSource>,
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

    fn dispatch_search(&mut self, pool: &sqlx::SqlitePool, rt: &Runtime) {
        self.dirty = false;
        self.query_seq += 1;
        let seq = self.query_seq;
        // The pruned tree, not the one on screen: an empty-text term compiles to
        // `LIKE '%'` and would widen the search instead of narrowing it.
        let expr = prune_empty(&self.bar.expr);
        let maps = self.maps.clone();
        let pool = pool.clone();
        self.spawn(rt, async move {
            let ctx = CompileCtx { maps: &maps };
            match query::search_by_ast(&pool, &expr, &ctx, RESULT_LIMIT).await {
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

        let (locale, history) = {
            let persisted = self.tab_state.persisted.read();
            let search = &persisted.settings.search;
            (persisted.settings.app.locale.clone(), search.history.iter().cloned().collect::<Vec<_>>())
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
                search_tab.dispatch_search(pool, rt);
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

        let count = self.tab_state.search_tab.results.len();
        ui.label(if self.tab_state.search_tab.truncated {
            t!("ui.search.match_count_truncated", count = count)
        } else {
            t!("ui.search.match_count", count = count)
        });

        let mut open_path: Option<std::path::PathBuf> = None;
        egui::ScrollArea::horizontal().id_salt("search_results").show(ui, |ui| {
            use egui_extras::Column;
            use egui_extras::TableBuilder;
            TableBuilder::new(ui)
                .striped(true)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(Column::initial(150.0)) // date
                .column(Column::initial(120.0)) // map
                .column(Column::initial(90.0)) // mode
                .column(Column::initial(140.0)) // ship
                .column(Column::initial(60.0)) // result
                .column(Column::initial(80.0)) // dmg
                .column(Column::initial(50.0)) // kills
                .column(Column::initial(60.0)) // pr
                .column(Column::remainder()) // open
                .header(20.0, |mut h| {
                    for label in [
                        t!("ui.search.column.date"),
                        t!("ui.search.column.map"),
                        t!("ui.search.column.mode"),
                        t!("ui.search.column.ship"),
                        t!("ui.search.column.result"),
                        t!("ui.search.column.damage"),
                        t!("ui.search.column.kills"),
                        t!("ui.search.column.pr"),
                    ] {
                        h.col(|ui| {
                            ui.strong(label);
                        });
                    }
                    h.col(|_ui| {});
                })
                .body(|mut body| {
                    for hit in &self.tab_state.search_tab.results {
                        let ship_name = self.search_ship_display_name(hit);
                        body.row(24.0, |mut row| {
                            row.col(|ui| {
                                ui.label(hit.timestamp.strftime("%Y-%m-%d %H:%M").to_string());
                            });
                            row.col(|ui| {
                                ui.label(&hit.map);
                            });
                            row.col(|ui| {
                                ui.label(&hit.game_type);
                            });
                            row.col(|ui| {
                                ui.label(ship_name.clone().unwrap_or_default());
                            });
                            row.col(|ui| {
                                ui.label(match hit.outcome {
                                    MatchOutcome::Win => "W",
                                    MatchOutcome::Loss => "L",
                                    MatchOutcome::Draw => "D",
                                    MatchOutcome::Unknown => "-",
                                });
                            });
                            row.col(|ui| {
                                ui.label(
                                    hit.self_damage
                                        .map(|d| crate::util::formatting::separate_number(d, locale.as_deref()))
                                        .unwrap_or_default(),
                                );
                            });
                            row.col(|ui| {
                                ui.label(hit.self_kills.map(|k| k.to_string()).unwrap_or_default());
                            });
                            row.col(|ui| {
                                ui.label(hit.self_pr.map(|pr| format!("{pr:.0}")).unwrap_or_default());
                            });
                            row.col(|ui| {
                                let exists = hit.replay_path.exists();
                                let btn = ui.add_enabled(
                                    exists,
                                    egui::Button::new(wt_translations::icon_t(
                                        icons::FOLDER_OPEN,
                                        &t!("ui.search.open"),
                                    )),
                                );
                                if !exists {
                                    btn.on_hover_text(t!("ui.search.open_missing"));
                                } else if btn.clicked() {
                                    open_path = Some(hit.replay_path.clone());
                                }
                            });
                        });
                    }
                });
        });

        if let Some(path) = open_path
            && let Some(deps) = self.tab_state.replay_dependencies()
        {
            crate::update_background_task!(
                self.tab_state.background_tasks,
                deps.parse_replay_from_path(path, crate::task::ReplaySource::ManualOpen)
            );
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
}
