//! The tracker's state: the period, sorts and filter it remembers, the players and notes it reads
//! from the config database, and which rows are opened.

use std::collections::HashMap;
use std::collections::HashSet;

use futures::StreamExt;
use futures::channel::mpsc;
use jiff::Timestamp;
use rust_i18n::t;
use sqlx::sqlite::SqlitePool;
use wows_replays::types::AccountId;
use wows_toolkit_config::index::query;
use wows_toolkit_config::index::query_text;
use wows_toolkit_config::index::rows::ClanCorrection;
use wows_toolkit_config::index::rows::IndexError;
use wows_toolkit_config::index::rows::MatchFilter;
use wows_toolkit_config::index::rows::PlayerFacet;
use wows_toolkit_viewmodel::player_tracker::ClanSort;
use wows_toolkit_viewmodel::player_tracker::ClanSortColumn;
use wows_toolkit_viewmodel::player_tracker::PlayerRow;
use wows_toolkit_viewmodel::player_tracker::Sort;
use wows_toolkit_viewmodel::player_tracker::SortColumn;
use wows_toolkit_viewmodel::player_tracker::TRACKER_SETTINGS_KEY;
use wows_toolkit_viewmodel::player_tracker::TimePeriod;
use wows_toolkit_viewmodel::player_tracker::TrackerSettings;
use wows_toolkit_viewmodel::player_tracker::clans::ClanRow;
use wows_toolkit_viewmodel::player_tracker::clans::build_clan_breakdown;
use wows_toolkit_viewmodel::player_tracker::clans::visible_clans;
use wows_toolkit_viewmodel::player_tracker::live::CurrentMatchViewMode;
use wows_toolkit_viewmodel::player_tracker::live::WinRateMode;
use wows_toolkit_viewmodel::player_tracker::store;
use wows_toolkit_viewmodel::player_tracker::tracked;
use wows_toolkit_viewmodel::player_tracker::tracked::TrackedPlayer;
use wows_toolkit_viewmodel::player_tracker::visible_player_rows;
use wows_toolkit_viewmodel::query_bar::seed;
use wows_toolkit_viewmodel::text_filter::FilterQuery;
use zgui::reactive::LocalStorage;
use zgui::reactive::Memo;
use zgui::reactive::Owner;
use zgui::reactive::RwSignal;
use zgui::reactive::StoredValue;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::UpdateValue;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::reactive::prelude::WithValue;
use zgui::task::spawn_local;

use crate::services::Services;
use crate::shell::Shell;
use crate::shell::palette::TabRequest;
use crate::tabs::player_tracker::live::LiveModel;

/// Where a table is in reading the index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadState {
    /// Before the config database is available, or before the table is first wanted.
    Idle,
    Loading,
    Failed(String),
    Loaded,
}

/// Bumped per index query, so a slower earlier answer cannot overwrite a later one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Generation(u64);

/// One note to write, in the order the notes were kept.
struct NoteWrite {
    pool: SqlitePool,
    pending: tracked::Pending,
    written: HashMap<AccountId, TrackedPlayer>,
}

/// The tracker tab's state, as one `Copy` handle.
#[derive(Clone, Copy)]
pub struct TrackerModel {
    pub services: Services,
    pub period: RwSignal<TimePeriod, LocalStorage>,
    pub sort: RwSignal<Sort, LocalStorage>,
    pub clan_sort: RwSignal<ClanSort, LocalStorage>,
    pub filter: RwSignal<String, LocalStorage>,
    /// `filter`, folded once for every row it is tried on.
    pub query: Memo<FilterQuery>,
    /// Everyone the index returned for the period, unfiltered.
    pub players: RwSignal<Vec<PlayerFacet>, LocalStorage>,
    /// Notes and encounters kept against the players met, shared with the other front ends.
    pub tracked: RwSignal<HashMap<AccountId, TrackedPlayer>, LocalStorage>,
    /// The index's latest clan per account, and the encounters whose clan at the time differed.
    clan_latest: RwSignal<HashMap<AccountId, String>, LocalStorage>,
    clan_corrections: RwSignal<Vec<ClanCorrection>, LocalStorage>,
    pub show_division_mates: RwSignal<bool, LocalStorage>,
    pub view_mode: RwSignal<CurrentMatchViewMode, LocalStorage>,
    pub win_rate_mode: RwSignal<WinRateMode, LocalStorage>,
    pub load: RwSignal<LoadState, LocalStorage>,
    pub clan_load: RwSignal<LoadState, LocalStorage>,
    generation: StoredValue<Generation>,
    /// Opened rows, keyed by identity so a re-sort carries the open state with the row.
    pub expanded_players: RwSignal<HashSet<AccountId>, LocalStorage>,
    pub expanded_clans: RwSignal<HashSet<String>, LocalStorage>,
    /// The account whose note is open for editing.
    pub editing_note: RwSignal<Option<AccountId>, LocalStorage>,
    /// Why the last note did not save; `None` after a successful write.
    pub note_error: RwSignal<Option<String>, LocalStorage>,
    /// Feeds the one task that writes notes, so two saves of one note land in order.
    note_writes: StoredValue<mpsc::UnboundedSender<NoteWrite>, LocalStorage>,
    /// The tab's scope. Work started from a menu or a dialog runs here, so it is not cancelled
    /// when the menu or dialog closes.
    owner: StoredValue<Owner, LocalStorage>,
    pub rows: Memo<Vec<PlayerRow>>,
    pub clans: Memo<Vec<ClanRow>>,
    pub live: LiveModel,
}

impl TrackerModel {
    /// Called from the tab's component body: the model's work lives as long as the tab.
    pub fn new(services: Services) -> Self {
        let owner = Owner::current().expect("the tracker model is made inside a component");
        let period = RwSignal::new_local(TimePeriod::default());
        let sort = RwSignal::new_local(Sort::default());
        let clan_sort = RwSignal::new_local(ClanSort::default());
        let filter = RwSignal::new_local(String::new());
        let query = Memo::new(move |_| FilterQuery::new(&filter.get()));
        let players = RwSignal::new_local(Vec::new());
        let tracked = RwSignal::new_local(HashMap::new());
        let clan_latest = RwSignal::new_local(HashMap::new());
        let clan_corrections = RwSignal::new_local(Vec::new());
        let show_division_mates = RwSignal::new_local(false);

        // The rows have no equality, so a recompute always notifies; the inputs change rarely.
        let rows = Memo::new_with_compare(
            move |_| {
                let since = period.get().earliest(Timestamp::now());
                players.with(|players| {
                    tracked.with(|tracked| {
                        query.with(|query| {
                            visible_player_rows(players, tracked, query, since, show_division_mates.get(), sort.get())
                        })
                    })
                })
            },
            always_changed,
        );
        let clans = Memo::new_with_compare(
            move |_| {
                let since = period.get().earliest(Timestamp::now());
                let rows = tracked.with(|tracked| {
                    clan_latest.with(|latest| {
                        clan_corrections.with(|corrections| {
                            build_clan_breakdown(tracked, latest, corrections, since, show_division_mates.get())
                        })
                    })
                });
                query.with(|query| visible_clans(rows, query, clan_sort.get()))
            },
            always_changed,
        );

        let note_error = RwSignal::new_local(None);
        let (note_writes, mut writes) = mpsc::unbounded::<NoteWrite>();
        spawn_local(async move {
            while let Some(write) = writes.next().await {
                let NoteWrite { pool, pending, written } = write;
                let saved = services.on_tokio(async move { store::save(&pool, &pending, &written).await }).await;
                let failed = match saved {
                    Ok(Ok(())) => None,
                    Ok(Err(err)) => Some(err.to_string()),
                    Err(err) => Some(err.to_string()),
                };
                if let Some(reason) = &failed {
                    tracing::warn!("player tracker: the note was not saved: {reason}");
                }
                note_error.set(failed);
            }
        });

        Self {
            services,
            period,
            sort,
            clan_sort,
            filter,
            query,
            players,
            tracked,
            clan_latest,
            clan_corrections,
            show_division_mates,
            view_mode: RwSignal::new_local(CurrentMatchViewMode::default()),
            win_rate_mode: RwSignal::new_local(WinRateMode::default()),
            load: RwSignal::new_local(LoadState::Idle),
            clan_load: RwSignal::new_local(LoadState::Idle),
            generation: StoredValue::new(Generation::default()),
            expanded_players: RwSignal::new_local(HashSet::new()),
            expanded_clans: RwSignal::new_local(HashSet::new()),
            editing_note: RwSignal::new_local(None),
            note_error,
            note_writes: StoredValue::new_local(note_writes),
            owner: StoredValue::new_local(owner),
            rows,
            clans,
            live: LiveModel::new(),
        }
    }

    fn pool(self) -> Option<SqlitePool> {
        self.services.settings.pool()
    }

    fn spawn(self, work: impl Future<Output = ()> + 'static) {
        self.owner.with_value(|owner| owner.with(|| spawn_local(work)));
    }

    /// Reads the saved view back, then the notes and the index. Called once the database is open.
    pub fn start(self, pool: SqlitePool) {
        let services = self.services;
        let read = pool.clone();
        self.spawn(async move {
            let stored = services
                .on_tokio(async move {
                    wows_toolkit_config::queries::get_setting::<TrackerSettings>(&read, TRACKER_SETTINGS_KEY).await
                })
                .await;
            if let Ok(Some(settings)) = stored {
                self.period.set(settings.period);
                self.sort.set(settings.sort);
                self.clan_sort.set(settings.clan_sort);
                self.filter.set(settings.filter);
            }
            self.refresh(pool);
        });
    }

    /// Reads the notes and view modes the front ends share.
    fn load_tracked(self, pool: SqlitePool) {
        let services = self.services;
        self.spawn(async move {
            let read = services
                .on_tokio(async move {
                    // Imports the tracker's settings blob into its tables, once.
                    if let Err(err) = store::import_blob_once(&pool).await {
                        tracing::warn!("player tracker: the stored blob was not imported: {err}");
                    }
                    let players = store::load(&pool).await;
                    let modes = store::load_view_modes(&pool).await;
                    players.map(|players| (players, modes))
                })
                .await;
            match read {
                Ok(Ok((players, modes))) => {
                    self.tracked.set(players);
                    self.win_rate_mode.set(modes.win_rate_mode);
                    self.view_mode.set(modes.current_match_view_mode);
                    self.show_division_mates.set(modes.show_division_mates);
                }
                // Left unloaded rather than shown as empty: a note written from here would then
                // be the only row an account has.
                Ok(Err(err)) => {
                    tracing::warn!("player tracker: the stored players could not be read: {err}");
                    self.note_error.set(Some(err.to_string()));
                }
                Err(err) => tracing::warn!("player tracker: the read did not run: {err}"),
            }
        });
    }

    fn next_generation(self) -> Generation {
        self.generation.update_value(|generation| generation.0 = generation.0.wrapping_add(1));
        self.generation.get_value()
    }

    /// Reads the players met in the period, and the notes, again.
    pub fn refresh(self, pool: SqlitePool) {
        let generation = self.next_generation();
        self.load.set(LoadState::Loading);
        self.clan_load.set(LoadState::Idle);
        self.load_tracked(pool.clone());

        let filter = self.period.get_untracked().match_filter(Timestamp::now());
        let services = self.services;
        self.spawn(async move {
            let found = services
                .on_tokio(async move {
                    let players = query::distinct_players(&pool, &filter).await?;
                    // The perspective player of a replay is not someone you met; left in they
                    // lead the table, since they were in every battle their own replay recorded.
                    let own = query::self_account_ids(&pool, &filter).await?;
                    Ok::<_, IndexError>((players, own))
                })
                .await;
            if self.generation.get_value() != generation {
                return;
            }
            match found {
                Ok(Ok((players, own))) => {
                    self.players.set(players.into_iter().filter(|facet| !own.contains(&facet.account_id)).collect());
                    self.load.set(LoadState::Loaded);
                }
                Ok(Err(err)) => self.load.set(LoadState::Failed(err.to_string())),
                Err(err) => self.load.set(LoadState::Failed(err.to_string())),
            }
        });
    }

    /// Reads the clans table's own all-time aggregates, once per refresh. They are whole-index
    /// scans, so they wait until the table is shown.
    pub fn load_clan_inputs_if_idle(self) {
        if self.clan_load.with_untracked(|state| *state != LoadState::Idle) {
            return;
        }
        let Some(pool) = self.pool() else { return };
        let generation = self.generation.get_value();
        self.clan_load.set(LoadState::Loading);
        let services = self.services;
        self.spawn(async move {
            let found = services
                .on_tokio(async move {
                    // Scoped to the live source, so a directory opened once does not cost every
                    // later read a scan of its rows.
                    let filter = MatchFilter {
                        source_ids: query::live_source_id(&pool).await?.map(|source| vec![source]),
                        ..Default::default()
                    };
                    let latest = query::distinct_players(&pool, &filter).await?;
                    let corrections = query::clan_history_corrections(&pool, &filter).await?;
                    Ok::<_, IndexError>((latest, corrections))
                })
                .await;
            if self.generation.get_value() != generation {
                return;
            }
            match found {
                Ok(Ok((latest, corrections))) => {
                    self.clan_latest.set(latest.into_iter().map(|facet| (facet.account_id, facet.clan)).collect());
                    self.clan_corrections.set(corrections);
                    self.clan_load.set(LoadState::Loaded);
                }
                Ok(Err(err)) => {
                    tracing::warn!("player tracker: the clan inputs could not be read: {err}");
                    self.clan_load.set(LoadState::Failed(err.to_string()));
                }
                Err(err) => {
                    tracing::warn!("player tracker: the clan inputs did not load: {err}");
                    self.clan_load.set(LoadState::Failed(err.to_string()));
                }
            }
        });
    }

    fn save_view(self) {
        let settings = TrackerSettings {
            period: self.period.get_untracked(),
            sort: self.sort.get_untracked(),
            clan_sort: self.clan_sort.get_untracked(),
            filter: self.filter.get_untracked(),
        };
        self.services.settings.save(TRACKER_SETTINGS_KEY, &settings);
    }

    pub fn set_period(self, period: TimePeriod) {
        if self.period.get_untracked() == period {
            return;
        }
        self.period.set(period);
        self.save_view();
        // A different window of time is a different query, not a filter over what is loaded.
        if let Some(pool) = self.pool() {
            self.refresh(pool);
        }
    }

    pub fn set_filter(self, text: String) {
        if self.filter.with_untracked(|filter| *filter == text) {
            return;
        }
        self.filter.set(text);
        self.save_view();
    }

    pub fn sort_by(self, column: SortColumn) {
        self.sort.update(|sort| *sort = sort.toggled(column));
        self.save_view();
    }

    pub fn sort_clans_by(self, column: ClanSortColumn) {
        self.clan_sort.update(|sort| *sort = sort.toggled(column));
        self.save_view();
    }

    pub fn set_show_division_mates(self, show: bool) {
        if self.show_division_mates.get_untracked() == show {
            return;
        }
        self.show_division_mates.set(show);
        self.store_view_modes();
    }

    pub fn set_view_mode(self, mode: CurrentMatchViewMode) {
        self.view_mode.set(mode);
        self.store_view_modes();
    }

    pub fn set_win_rate_mode(self, mode: WinRateMode) {
        self.win_rate_mode.set(mode);
        self.store_view_modes();
    }

    /// Saves the roster's view settings where the egui app keeps its own.
    fn store_view_modes(self) {
        let Some(pool) = self.pool() else { return };
        let modes = tracked::ViewModes {
            win_rate_mode: self.win_rate_mode.get_untracked(),
            current_match_view_mode: self.view_mode.get_untracked(),
            show_division_mates: self.show_division_mates.get_untracked(),
        };
        let services = self.services;
        self.spawn(async move {
            if let Ok(Err(err)) = services.on_tokio(async move { store::store_view_modes(&pool, modes).await }).await {
                tracing::warn!("player tracker: the view settings were not saved: {err}");
            }
        });
    }

    pub fn toggle_player_expanded(self, account: AccountId) {
        let mut closed = false;
        self.expanded_players.update(|expanded| {
            if !expanded.remove(&account) {
                expanded.insert(account);
            } else {
                closed = true;
            }
        });
        if closed && self.editing_note.get_untracked() == Some(account) {
            self.editing_note.set(None);
        }
    }

    pub fn toggle_clan_expanded(self, clan: String) {
        self.expanded_clans.update(|expanded| {
            if !expanded.remove(&clan) {
                expanded.insert(clan);
            }
        });
    }

    /// Opens `account`'s note for editing, in the row's own detail block.
    pub fn edit_note(self, account: AccountId) {
        self.editing_note.set(Some(account));
        self.expanded_players.update(|expanded| {
            expanded.insert(account);
        });
    }

    pub fn note_for(self, account: AccountId) -> String {
        self.tracked
            .with_untracked(|tracked| tracked.get(&account).map(|player| player.notes.clone()).unwrap_or_default())
    }

    /// Keeps `note` against `account` and writes that one row.
    pub fn store_note(self, account: AccountId, note: String) {
        let unchanged = self.tracked.with_untracked(|tracked| match tracked.get(&account) {
            Some(player) => player.notes == note,
            // An empty note about a player nothing else is recorded for is not worth an entry.
            None => note.is_empty(),
        });
        if unchanged {
            return;
        }

        // The name the index knows, so the note is legible in the egui tab's list, which keys
        // its rows on the stored name.
        let facet = self.players.with_untracked(|players| {
            players
                .iter()
                .find(|facet| facet.account_id == account)
                .map(|facet| (facet.latest_name.clone(), facet.clan.clone()))
        });
        let mut written = HashMap::new();
        self.tracked.update(|tracked| {
            let entry = tracked.entry(account).or_insert_with(|| {
                let (last_name, clan) = facet.unwrap_or_default();
                TrackedPlayer { last_name, clan, ..TrackedPlayer::new(account) }
            });
            entry.notes = note;
            written.insert(account, entry.clone());
        });

        let Some(pool) = self.pool() else { return };
        let mut pending = tracked::Pending::default();
        pending.note_changed(account);
        self.note_writes.with_value(|writes| {
            if writes.unbounded_send(NoteWrite { pool, pending, written }).is_err() {
                tracing::warn!("player tracker: the note writer has stopped; the note was not saved");
            }
        });
    }

    /// Forgets every tracked player once the delete has landed, so a failed write does not leave
    /// an empty table that fills again on the next load.
    pub fn clear_tracked(self) {
        let Some(pool) = self.pool() else { return };
        let services = self.services;
        self.spawn(async move {
            match services.on_tokio(async move { wows_toolkit_config::tracker::clear_tracker(&pool).await }).await {
                Ok(Ok(())) => self.tracked.set(HashMap::new()),
                Ok(Err(err)) => tracing::error!("player tracker: the tracker was not cleared: {err}"),
                Err(err) => tracing::error!("player tracker: the clear did not complete: {err}"),
            }
        });
    }

    /// Shows the Search tab with a query for every match `account` was in.
    pub fn find_player_matches(shell: Shell, account: AccountId) {
        let query = query_text::print_query(&seed::matches_with_player(account));
        shell.palette.send(shell, TabRequest::Search(query));
    }

    /// The same for a clan tag, which also matches the tag inside a name.
    pub fn find_clan_matches(shell: Shell, clan: &str) {
        let query = query_text::print_query(&seed::matches_mentioning_clan(clan));
        shell.palette.send(shell, TabRequest::Search(query));
    }

    /// Seeds what the database reads would have produced.
    #[cfg(test)]
    pub fn seed(self, players: Vec<PlayerFacet>, tracked: HashMap<AccountId, TrackedPlayer>) {
        self.players.set(players);
        self.tracked.set(tracked);
        self.load.set(LoadState::Loaded);
        self.clan_load.set(LoadState::Loaded);
    }
}

/// What a table says in place of its rows, or `None` when it has rows to show.
pub fn table_status(state: &LoadState, rows: usize, loading_key: &str, empty_key: &str) -> Option<String> {
    match state {
        LoadState::Idle => Some(t!("ui.player_tracker.waiting_for_index").into_owned()),
        LoadState::Loading => Some(t!(loading_key).into_owned()),
        LoadState::Failed(reason) => Some(t!("ui.player_tracker.index_failed", reason = reason).into_owned()),
        LoadState::Loaded if rows == 0 => Some(t!(empty_key).into_owned()),
        LoadState::Loaded => None,
    }
}

/// A memo comparison for values without equality.
fn always_changed<T>(_: Option<&T>, _: Option<&T>) -> bool {
    true
}
