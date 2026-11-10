use jiff::ToSpan as _;
use nsh_core::prelude::*;
use rust_i18n::Backend as _;
use wows_toolkit_viewmodel::settings::keys;

/// One top-level application tab.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct AppTab {
    /// Stable tab identifier.
    pub id: String,
    /// Text shown in the tab strip.
    pub label: String,
    /// Short description shown in navigation search.
    pub description: String,
}

/// Localized text used by the application shell.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct ShellText {
    /// Label for the tactics board command.
    pub tactics_board: String,
    /// Label for the update command.
    pub check_updates: String,
    /// Label for the about command.
    pub about: String,
    /// Label for the quit command.
    pub quit: String,
    /// Label for opening a replay.
    pub open_replay: String,
    /// Message shown after an update check finds nothing.
    pub app_up_to_date: String,
}

/// State required before the first application frame.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct StartupState {
    /// Window and application title.
    pub title: String,
    /// Application version.
    pub version: String,
    /// Selected locale code.
    pub locale: String,
    /// Localized shell text.
    pub text: ShellText,
    /// Top-level tabs in display order.
    pub tabs: Vec<AppTab>,
}

/// Aggregate battle statistics from the shared session database.
#[derive(Debug, Clone, PartialEq, JsObject)]
#[non_exhaustive]
pub struct StatisticsState {
    pub games: u32,
    pub wins: u32,
    pub losses: u32,
    pub draws: u32,
    pub win_rate: f64,
    pub average_damage: f64,
    pub average_base_xp: f64,
    pub average_frags: f64,
    pub ships: Vec<ShipStatistics>,
}

/// Statistics for one ship.
#[derive(Debug, Clone, PartialEq, JsObject)]
#[non_exhaustive]
pub struct ShipStatistics {
    pub ship_name: String,
    pub ship_id: String,
    pub games: u32,
    pub win_rate: f64,
    pub average_damage: f64,
    pub average_base_xp: f64,
}

/// Historical players found in the shared replay index.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct PlayerTrackerState {
    pub players: Vec<TrackedPlayer>,
    pub total_players: u32,
}

/// One player shown by the historical player tracker.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct TrackedPlayer {
    pub account_id: String,
    pub name: String,
    pub clan: String,
    pub encounters: u32,
}

/// Bounded results from the shared replay index.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct SearchState {
    pub results: Vec<SearchResult>,
    pub truncated: bool,
}

/// One indexed replay search result.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct SearchResult {
    pub arena_id: String,
    pub timestamp: String,
    pub map: String,
    pub mode: String,
    pub game_type: String,
    pub outcome: String,
    pub ship: String,
    pub damage: String,
    pub kills: String,
    pub pr: String,
    pub replay_path: String,
}

#[derive(Debug)]
struct ShipAccumulator {
    ship_name: String,
    ship_id: u64,
    games: u32,
    wins: u32,
    damage: u64,
    base_xp: i64,
}

/// Native application services exposed to the TSX interface.
#[module]
pub struct Toolkit;

#[module]
impl Toolkit {
    /// Returns the state required to construct the application shell.
    pub fn startup(locale: Option<String>) -> StartupState {
        let persisted = locale.or_else(|| {
            wows_toolkit_config::load_startup_setting::<Option<String>>(keys::LOCALE).ok().flatten().flatten()
        });
        let locale = selected_locale(persisted);
        let backend = wt_translations::TranslationsBackend::load();
        let text = |key: &str, fallback: &str| backend.translate(&locale, key).unwrap_or(fallback).to_owned();
        StartupState {
            title: "WoWs Toolkit".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            locale: locale.clone(),
            text: ShellText {
                tactics_board: text("ui.windows.tactics_board", "Tactics Board"),
                check_updates: text("ui.menu.check_updates", "Check for Updates"),
                about: text("ui.menu.about", "About"),
                quit: text("ui.menu.quit", "Quit"),
                open_replay: text("ui.replay.open_file", "Open Replay File..."),
                app_up_to_date: text("ui.messages.app_up_to_date", "Application up-to-date"),
            },
            tabs: vec![
                tab(
                    "replay-inspector",
                    &text("ui.tabs.replay_parser", "Replay Inspector"),
                    "Open and inspect a replay",
                ),
                tab("launcher", &text("ui.tabs.launcher", "Launcher"), "Manage and start game installations"),
                tab("statistics", &text("ui.tabs.stats", "Stats"), "Analyze replay statistics"),
                tab("player-tracker", &text("ui.tabs.player_tracker", "Player Tracker"), "Track player battle results"),
                tab("search", &text("ui.tabs.search", "Search"), "Search game data"),
                tab("armor-viewer", &text("ui.tabs.armor_viewer", "Armor Viewer"), "Inspect ship armor models"),
                tab("unpacker", &text("ui.tabs.unpacker", "Resource Unpacker"), "Browse and extract game files"),
                tab("settings", &text("ui.tabs.settings", "Settings"), "Configure WoWs Toolkit"),
            ],
        }
    }

    /// Aggregates recorded battles from the shared session database.
    pub async fn load_statistics() -> Result<StatisticsState, String> {
        blocking(load_statistics).await.map_err(|error| format!("statistics reader task failed: {error}"))?
    }
    /// Reads historical encounters from the shared replay index.
    pub async fn load_player_tracker(period: String, query: String) -> Result<PlayerTrackerState, String> {
        blocking(move || load_player_tracker(&period, &query))
            .await
            .map_err(|error| format!("player tracker reader task failed: {error}"))?
    }
    /// Searches the shared replay index using the common query language.
    pub async fn search_index(query: String, sort_column: String, descending: bool) -> Result<SearchState, String> {
        blocking(move || search_index(&query, &sort_column, descending))
            .await
            .map_err(|error| format!("search task failed: {error}"))?
    }
}

fn load_statistics() -> Result<StatisticsState, String> {
    database_runtime()?.block_on(async {
        let pool = wows_toolkit_config::open_db()
            .await
            .map_err(|error| format!("could not open the statistics database: {error}"))?;
        let rows = wows_toolkit_config::queries::get_all_session_stats(&pool)
            .await
            .map_err(|error| format!("could not read session statistics: {error}"))?;
        let games = u32::try_from(rows.len()).map_err(|_| "too many session statistics rows".to_owned())?;
        let wins = u32::try_from(rows.iter().filter(|row| row.is_win).count())
            .map_err(|_| "too many winning session statistics rows".to_owned())?;
        let losses = u32::try_from(rows.iter().filter(|row| row.is_loss).count())
            .map_err(|_| "too many losing session statistics rows".to_owned())?;
        let draws = u32::try_from(rows.iter().filter(|row| row.is_draw).count())
            .map_err(|_| "too many drawn session statistics rows".to_owned())?;
        let total_damage = rows.iter().try_fold(0u64, |total, row| {
            total
                .checked_add(row.damage as u64)
                .ok_or_else(|| "total session damage exceeds the supported range".to_owned())
        })?;
        let total_base_xp = rows.iter().try_fold(0i64, |total, row| {
            total.checked_add(row.base_xp).ok_or_else(|| "total session base XP exceeds the supported range".to_owned())
        })?;
        let total_frags = rows.iter().try_fold(0i64, |total, row| {
            total.checked_add(row.frags).ok_or_else(|| "total session frags exceeds the supported range".to_owned())
        })?;
        let mut by_ship = std::collections::BTreeMap::<u64, ShipAccumulator>::new();
        for row in &rows {
            let ship_id = row.ship_id as u64;
            let ship = by_ship.entry(ship_id).or_insert_with(|| ShipAccumulator {
                ship_name: row.ship_name.clone(),
                ship_id,
                games: 0,
                wins: 0,
                damage: 0,
                base_xp: 0,
            });
            ship.games += 1;
            ship.wins += u32::from(row.is_win);
            ship.damage = ship
                .damage
                .checked_add(row.damage as u64)
                .ok_or_else(|| format!("damage for ship {ship_id} exceeds the supported range"))?;
            ship.base_xp = ship
                .base_xp
                .checked_add(row.base_xp)
                .ok_or_else(|| format!("base XP for ship {ship_id} exceeds the supported range"))?;
        }
        let mut ships = by_ship
            .into_values()
            .map(|ship| ShipStatistics {
                ship_name: ship.ship_name,
                ship_id: ship.ship_id.to_string(),
                games: ship.games,
                win_rate: percentage(ship.wins, ship.games),
                average_damage: average_u64(ship.damage, ship.games),
                average_base_xp: average(ship.base_xp, ship.games),
            })
            .collect::<Vec<_>>();
        ships.sort_by(|left, right| {
            right
                .games
                .cmp(&left.games)
                .then_with(|| left.ship_name.cmp(&right.ship_name))
                .then_with(|| left.ship_id.cmp(&right.ship_id))
        });

        Ok(StatisticsState {
            games,
            wins,
            losses,
            draws,
            win_rate: percentage(wins, games),
            average_damage: average_u64(total_damage, games),
            average_base_xp: average(total_base_xp, games),
            average_frags: average(total_frags, games),
            ships,
        })
    })
}

fn load_player_tracker(period: &str, query: &str) -> Result<PlayerTrackerState, String> {
    let now = jiff::Timestamp::now();
    let date_from = match period {
        "hour" => Some(now - 1.hour()),
        "six_hours" => Some(now - 6.hours()),
        "day" => Some(now - 24.hours()),
        "week" => Some(now - (24 * 7).hours()),
        "month" => Some(now - (24 * 30).hours()),
        "all" => None,
        _ => return Err(format!("unsupported player tracker period: {period}")),
    };
    database_runtime()?.block_on(async {
        let pool = wows_toolkit_config::open_db()
            .await
            .map_err(|error| format!("could not open the replay index: {error}"))?;
        let filter = wows_toolkit_config::index::rows::MatchFilter { date_from, ..Default::default() };
        let limit = std::num::NonZeroU32::new(500).expect("the player tracker page limit is nonzero");
        let page = wows_toolkit_config::index::query::distinct_players_page(&pool, &filter, query, limit)
            .await
            .map_err(|error| format!("could not read indexed players: {error}"))?;
        let total_players = u32::try_from(page.total_count)
            .map_err(|_| "the filtered player count exceeds the supported range".to_owned())?;
        let players = page
            .players
            .into_iter()
            .map(|player| {
                let encounters = u32::try_from(player.match_count).map_err(|_| {
                    format!("encounter count for account {} exceeds the supported range", player.account_id.raw())
                })?;
                Ok(TrackedPlayer {
                    account_id: player.account_id.raw().to_string(),
                    name: player.latest_name,
                    clan: player.clan,
                    encounters,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(PlayerTrackerState { players, total_players })
    })
}

fn search_index(query: &str, sort_column: &str, descending: bool) -> Result<SearchState, String> {
    let expression = wows_toolkit_config::index::query_text::parse_query(query)
        .map_err(|error| format!("invalid query at {}..{}: {}", error.span.start, error.span.end, error.kind))?;
    let column = match sort_column {
        "date" => wows_toolkit_config::index::query::SortColumn::Date,
        "map" => wows_toolkit_config::index::query::SortColumn::Map,
        "mode" => wows_toolkit_config::index::query::SortColumn::Mode,
        "outcome" => wows_toolkit_config::index::query::SortColumn::Outcome,
        "damage" => wows_toolkit_config::index::query::SortColumn::Damage,
        "kills" => wows_toolkit_config::index::query::SortColumn::Kills,
        "pr" => wows_toolkit_config::index::query::SortColumn::Pr,
        _ => return Err(format!("unsupported search sort column: {sort_column}")),
    };
    let direction = if descending {
        wows_toolkit_config::index::query::SortDirection::Descending
    } else {
        wows_toolkit_config::index::query::SortDirection::Ascending
    };
    let sort = wows_toolkit_config::index::query::SortSpec { column, direction };
    database_runtime()?.block_on(async {
        let pool = wows_toolkit_config::open_db()
            .await
            .map_err(|error| format!("could not open the replay index: {error}"))?;
        let mut hits = wows_toolkit_config::index::query::search_by_ast(
            &pool,
            &expression,
            &wows_toolkit_config::index::query_sql::CompileCtx::default(),
            500,
            sort,
        )
        .await
        .map_err(|error| format!("could not search indexed replays: {error}"))?;
        let truncated = hits.len() > 500;
        hits.truncate(500);
        let results = hits
            .into_iter()
            .map(|hit| {
                let ship = wows_toolkit_viewmodel::search::ship_display_name_from_parts(
                    hit.self_ship_id,
                    hit.self_ship_name.as_deref(),
                    None,
                )
                .unwrap_or_else(|| "Spectator".to_owned());
                SearchResult {
                    arena_id: hit.arena_id.raw().to_string(),
                    timestamp: hit.timestamp.to_string(),
                    map: hit.map,
                    mode: hit.game_mode,
                    game_type: hit.game_type,
                    outcome: hit.outcome.as_db_str().to_owned(),
                    ship,
                    damage: hit.self_damage.map(|damage| damage.to_string()).unwrap_or_else(|| "-".to_owned()),
                    kills: hit.self_kills.map(|kills| kills.to_string()).unwrap_or_else(|| "-".to_owned()),
                    pr: hit.self_pr.map(|pr| format!("{pr:.0}")).unwrap_or_else(|| "-".to_owned()),
                    replay_path: hit.replay_path.to_string_lossy().into_owned(),
                }
            })
            .collect();
        Ok(SearchState { results, truncated })
    })
}

fn percentage(part: u32, total: u32) -> f64 {
    if total == 0 { 0.0 } else { f64::from(part) * 100.0 / f64::from(total) }
}

fn average_u64(total: u64, count: u32) -> f64 {
    if count == 0 { 0.0 } else { total as f64 / f64::from(count) }
}

fn average(total: i64, count: u32) -> f64 {
    if count == 0 { 0.0 } else { total as f64 / f64::from(count) }
}

fn database_runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("could not start the settings database runtime: {error}"))
}

fn selected_locale(requested: Option<String>) -> String {
    requested
        .filter(|locale| wt_translations::SUPPORTED_LANGUAGES.iter().any(|language| language.code == locale))
        .or_else(|| {
            sys_locale::get_locale()
                .and_then(|locale| wt_translations::system_locale_to_wows(&locale).map(str::to_owned))
        })
        .unwrap_or_else(|| "en".to_owned())
}

fn tab(id: &str, label: &str, description: &str) -> AppTab {
    AppTab { id: id.to_owned(), label: label.to_owned(), description: description.to_owned() }
}
