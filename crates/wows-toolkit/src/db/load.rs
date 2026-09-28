//! Load persisted state from SQLite back into `TabState`.
//!
//! This is the reverse of `migrate_ron`: it reads tables and populates the
//! in-memory app state that was previously loaded from `app.ron`.

use std::collections::BTreeSet;
use std::path::PathBuf;

use rootcause::prelude::ResultExt;
use sqlx::SqlitePool;
use tracing::error;
use tracing::info;
use tracing::warn;
use wows_toolkit_viewmodel::settings::keys;

use crate::data::session_stats::PerGameStat;
use crate::data::session_stats::SerializableAchievement;
use crate::data::session_stats::SessionStats;
use crate::db::index::query_text::parse_query;
use crate::tab_state::TabState;
use crate::tab_state::WindowKind;
use crate::tab_state::WindowSettings;

use super::queries;

/// Load all persisted state from SQLite into `tab_state`.
///
/// Fields that are not found in the database keep their current (default) values.
pub async fn load_tab_state_from_db(pool: &SqlitePool, tab_state: &mut TabState) -> Result<(), sqlx::Error> {
    info!("Loading state from SQLite...");

    load_settings(pool, tab_state).await?;
    load_session_stats(pool, tab_state).await?;
    load_tracked_players(pool, tab_state).await?;
    load_sent_replays(pool, tab_state).await?;
    load_chart_configs(pool, tab_state).await?;
    load_armor_viewer_defaults(pool, tab_state).await?;
    load_render_options(pool, tab_state).await?;
    load_dock_layout(pool, tab_state).await?;
    load_mod_manager(pool, tab_state).await?;

    info!("State loaded from SQLite");
    Ok(())
}

/// Load scalar settings from the k/v table.
#[allow(clippy::await_holding_lock)]
async fn load_settings(pool: &SqlitePool, ts: &mut TabState) -> Result<(), sqlx::Error> {
    let mut p = ts.persisted.write();
    let s = &mut p.settings;

    if let Some(v) = queries::get_setting::<PathBuf>(pool, "current_replay_path").await {
        s.game.current_replay_path = v;
    }
    if let Some(v) = queries::get_setting::<String>(pool, "wows_dir").await {
        s.game.wows_dir = v;
    }
    if let Some(v) = queries::get_setting::<Option<String>>(pool, "locale").await {
        s.app.locale = v;
    }
    if let Some(v) = queries::get_setting::<bool>(pool, "check_for_updates").await {
        s.app.check_for_updates = v;
    }
    if let Some(v) = queries::get_setting::<crate::data::settings::DataSharingMode>(pool, keys::DATA_SHARING_MODE).await
    {
        s.integrations.data_sharing_mode = v;
        if let Some(b) = queries::get_setting::<bool>(pool, keys::SEND_REPLAY_DATA).await {
            s.integrations.data_sharing_mode = s.integrations.data_sharing_mode.reconcile_with_compat_bool(b);
        }
    } else if let Some(v) = queries::get_setting::<bool>(pool, keys::SEND_REPLAY_DATA).await {
        s.integrations.data_sharing_mode = crate::data::settings::DataSharingMode::from_send_replay_data_bool(v);
    }
    if let Some(v) = queries::get_setting::<bool>(pool, "has_052_game_params_fix").await {
        s.game.has_052_game_params_fix = v;
    }
    if let Some(v) = queries::get_setting::<bool>(pool, "auto_dump_game_data").await {
        s.game.auto_dump_game_data = v;
    }
    if let Some(v) = queries::get_setting::<String>(pool, "game_data_cache_dir").await {
        s.game.game_data_cache_dir = v;
    }
    if let Some(v) = queries::get_setting::<Option<String>>(pool, "game_data_repo_commit").await {
        s.game.game_data_repo_commit = v;
    }
    // twitch_token: Option<Token> — stored as JSON
    if let Some(v) = queries::get_setting(pool, "twitch_token").await {
        s.integrations.twitch_token = v;
    }
    if let Some(v) = queries::get_setting::<String>(pool, "twitch_monitored_channel").await {
        s.integrations.twitch_monitored_channel = v;
    }
    if let Some(v) = queries::get_setting::<Option<String>>(pool, "constants_file_commit").await {
        s.game.constants_file_commit = v;
    }
    if let Some(v) = queries::get_setting::<Option<String>>(pool, "proxy_url").await {
        s.app.proxy_url = v;
    }
    if let Some(v) = queries::get_setting::<bool>(pool, "debug_mode").await {
        s.app.debug_mode = v;
    }
    if let Some(v) = queries::get_setting::<bool>(pool, keys::BUILD_CONSENT_SHOWN).await {
        s.app.build_consent_window_shown = v;
    }
    if let Some(v) = queries::get_setting::<bool>(pool, keys::REPLAY_CONSENT_SHOWN).await {
        s.app.replay_consent_prompt_shown = v;
    }
    if let Some(v) = queries::get_setting::<bool>(pool, keys::LANGUAGE_SELECTION_SHOWN).await {
        s.app.language_selection_shown = v;
    }
    if let Some(v) = queries::get_setting::<bool>(pool, "session_stats_limit_enabled").await {
        s.stats_filters.limit_enabled = v;
    }
    if let Some(v) = queries::get_setting::<usize>(pool, "session_stats_game_count").await {
        s.stats_filters.game_count = v;
    }
    if let Some(v) = queries::get_setting(pool, "session_stats_division_filter").await {
        s.stats_filters.division_filter = v;
    }
    if let Some(v) = queries::get_setting::<BTreeSet<String>>(pool, "session_stats_game_mode_filter").await {
        s.stats_filters.game_mode_filter = v;
    }
    if let Some(v) = queries::get_setting::<bool>(pool, "suppress_gpu_encoder_warning").await {
        s.app.suppress_gpu_encoder_warning = v;
    }
    if let Some(v) = queries::get_setting::<bool>(pool, "enable_logging").await {
        s.app.enable_logging = v;
    }
    if let Some(v) = queries::get_setting::<f32>(pool, "zoom_factor").await {
        s.app.zoom_factor = v;
    }
    if let Some(v) = queries::get_setting::<crate::data::settings::ThemeChoice>(pool, "theme").await {
        s.app.theme = v;
    }
    // Read here only so the settings UI shows what is in force. The value that
    // decides the policy is read in `main`, before this pool exists.
    if let Some(v) =
        queries::get_setting::<crate::hardening::CodeIntegrityPreference>(pool, crate::CODE_INTEGRITY_SETTING).await
    {
        s.app.code_integrity = v;
    }
    if let Some(v) = queries::get_setting::<String>(pool, "collab_display_name").await {
        s.collab.display_name = v;
    }
    if let Some(v) = queries::get_setting::<bool>(pool, "suppress_p2p_ip_warning").await {
        s.collab.suppress_p2p_ip_warning = v;
    }
    if let Some(v) = queries::get_setting::<bool>(pool, "disable_auto_open_session_windows").await {
        s.collab.disable_auto_open_session_windows = v;
    }

    // Nested struct: replay_settings
    if let Some(v) = queries::get_setting(pool, "replay_settings").await {
        s.replay = v;
    }
    // Nested struct: search. Also restored into the Search tab itself below,
    // once the write guard on `settings` is dropped. A stored value that does
    // not deserialize is logged by `get_setting` and read as `None`, which
    // leaves the defaults in place rather than failing the whole load.
    let search: Option<crate::data::settings::SearchSettings> = queries::get_setting(pool, "search").await;
    if let Some(v) = search.clone() {
        s.search = v;
    }

    // Fields that moved to PersistedState.
    if let Some(v) = queries::get_setting::<String>(pool, "output_dir").await {
        p.output_dir = v;
    }
    if let Some(v) = queries::get_setting::<bool>(pool, "auto_load_latest_replay").await {
        p.auto_load_latest_replay = v;
    }
    if let Some(v) = queries::get_setting::<u64>(pool, "next_chart_tab_id").await {
        p.next_chart_tab_id = v;
    }

    // Drop the write guard before accessing ts fields directly.
    drop(p);

    // Restore the Search tab's query. A missing, empty, or unparseable one
    // leaves the bar empty, which is what a fresh install shows.
    if let Some(text) = search.map(|v| v.query).filter(|text| !text.trim().is_empty()) {
        match parse_query(&text).attach("while restoring the persisted search query").attach(text.clone()) {
            Ok(expr) => ts.search_tab.set_query(expr),
            Err(report) => warn!("{report:?}"),
        }
    }

    if let Some(v) = queries::get_setting(pool, "replay_sort").await {
        *ts.replay_sort.lock() = v;
    }

    // Window sizes/geometry.
    if let Some(sizes) =
        queries::get_setting::<std::collections::HashMap<WindowKind, WindowSettings>>(pool, "window_sizes").await
    {
        ts.window_settings.lock().settings = sizes;
    }

    info!("  loaded settings");
    Ok(())
}

/// Load session stats from the database.
async fn load_session_stats(pool: &SqlitePool, ts: &mut TabState) -> Result<(), sqlx::Error> {
    let rows = queries::get_all_session_stats(pool).await?;
    let mut games = Vec::with_capacity(rows.len());

    for row in rows {
        let achievements: Vec<SerializableAchievement> = serde_json::from_str(&row.achievements).unwrap_or_default();

        games.push(PerGameStat {
            ship_name: row.ship_name,
            ship_id: (row.ship_id as u64).into(),
            game_time: row.game_time,
            sort_key: row.sort_key,
            player_id: row.player_id,
            damage: row.damage as u64,
            spotting_damage: row.spotting_damage as u64,
            frags: row.frags,
            raw_xp: row.raw_xp,
            base_xp: row.base_xp,
            is_win: row.is_win,
            is_loss: row.is_loss,
            is_draw: row.is_draw,
            is_div: row.is_div,
            match_group: row.match_group,
            achievements,
        });
    }

    let mut p = ts.persisted.write();
    p.session_stats = SessionStats { games, ..Default::default() };

    info!("  loaded {} session stats", p.session_stats.games.len());
    Ok(())
}

/// Load tracked players from the database.
async fn load_tracked_players(pool: &SqlitePool, ts: &mut TabState) -> Result<(), sqlx::Error> {
    use wows_toolkit_viewmodel::player_tracker::store;

    // A database written before the tracker had tables still holds it as one
    // settings blob. Importing it is seconds of writing for a reader with years
    // of history, so it runs behind the window rather than in front of it; see
    // [`spawn_tracker_import`].
    match store::load(pool).await {
        Ok(players) => ts.player_tracker.write().tracked_players = players,
        Err(e) => error!("Failed to read the tracked players: {e}"),
    }

    // Every read happens before the guard is taken: the tracker's lock must not
    // be held across an await.
    let modes = store::load_view_modes(pool).await;
    let prefs = load_tracker_prefs(pool).await;
    let period = queries::get_setting(pool, "player_tracker.filter_time_period").await;
    {
        let mut tracker = ts.player_tracker.write();
        tracker.win_rate_mode = modes.win_rate_mode;
        tracker.current_match_view_mode = modes.current_match_view_mode;
        tracker.show_division_mates = modes.show_division_mates;
        tracker.sort_order = prefs.sort_order;
        tracker.clan_sort_order = prefs.clan_sort_order;
        tracker.player_filter = prefs.player_filter;
        if let Some(period) = period {
            tracker.filter_time_period = period;
        }
    }

    info!("  loaded {} tracked players", ts.player_tracker.read().tracked_players.len());
    Ok(())
}

/// Moves a tracker stored as the old settings blob into its tables, off the
/// startup path.
///
/// On a connection of its own: the app's pool holds a single connection, and a
/// write of a million rows takes it for long enough that every other query --
/// the index reads the tracker tab runs on the UI thread among them -- would
/// wait behind it. WAL lets those reads carry on against the pool's connection
/// while this one writes.
///
/// Imported players are merged rather than swapped in: a battle that ended while
/// the import was running is in memory and not in the blob.
pub fn spawn_tracker_import(
    runtime: &std::sync::Arc<tokio::runtime::Runtime>,
    db_path: std::path::PathBuf,
    tracker: std::sync::Arc<parking_lot::RwLock<crate::ui::player_tracker::PlayerTracker>>,
    egui_ctx: egui::Context,
) {
    use wows_toolkit_viewmodel::player_tracker::store;

    runtime.spawn(async move {
        let pool = match wows_toolkit_config::open_db_at(&db_path).await {
            Ok(pool) => pool,
            Err(e) => {
                error!("Failed to open the database for the tracker import: {e}");
                return;
            }
        };

        match store::import_blob_once(&pool).await {
            Ok(store::Imported::Migrated { players, arenas, timestamps }) => {
                info!(
                    "imported {players} tracked players ({arenas} arenas, {timestamps} timestamps) out of the old blob"
                );
            }
            // Nothing to import, and nothing to load that startup did not.
            Ok(_) => return,
            // The blob is kept when its import fails, so the next launch tries
            // again; the tracker holds whatever the tables already had.
            Err(e) => {
                error!("Failed to import the stored player tracker: {e}");
                return;
            }
        }

        let imported = match store::load(&pool).await {
            Ok(imported) => imported,
            Err(e) => {
                error!("Failed to read the imported players: {e}");
                return;
            }
        };
        // Held no longer than the import: a second connection to the same file
        // is worth a startup, not a session.
        pool.close().await;
        drop(pool);

        {
            let mut tracker = tracker.write();
            // A battle that ended while the import ran left a player holding
            // only what that battle reported: no note, no aliases, one
            // encounter. The imported row is the historical one, so it is the
            // base and the live one is folded onto it.
            let mut met_meanwhile = Vec::new();
            for (account, player) in imported {
                if let Some(live) = tracker.tracked_players.insert(account, player) {
                    met_meanwhile.push((account, live));
                }
            }

            for (account, live) in met_meanwhile {
                let Some(player) = tracker.tracked_players.get_mut(&account) else { continue };
                player.merge_encounters_from(&live);

                // The import replaced the rows that battle had written, so they
                // are stated again. The two key families are walked apart: they
                // are unpaired, and zipping them would drop whichever is longer.
                let arenas: Vec<_> = player.arena_ids.iter().collect();
                let timestamps: Vec<_> = player.timestamps.iter().collect();
                tracker.pending.identity_changed(account);
                for arena_id in arenas {
                    tracker.pending.arena_changed(account, arena_id);
                }
                for timestamp in timestamps {
                    tracker.pending.timestamp_changed(account, timestamp);
                }
            }
            tracker.note_encounters_changed();
        }
        egui_ctx.request_repaint();
    });
}

/// The Historical and Clans table settings, each under its own key.
///
/// Only this app writes them: the tracker's own view modes are shared with the
/// port and live in `store`.
async fn load_tracker_prefs(pool: &SqlitePool) -> crate::ui::player_tracker::TrackerPrefs {
    use crate::ui::player_tracker::TrackerPrefs;

    TrackerPrefs {
        sort_order: queries::get_setting(pool, TrackerPrefs::SORT_ORDER_KEY).await.unwrap_or_default(),
        clan_sort_order: queries::get_setting(pool, TrackerPrefs::CLAN_SORT_ORDER_KEY).await.unwrap_or_default(),
        player_filter: queries::get_setting(pool, TrackerPrefs::PLAYER_FILTER_KEY).await.unwrap_or_default(),
    }
}

/// Load sent replays.
async fn load_sent_replays(pool: &SqlitePool, ts: &mut TabState) -> Result<(), sqlx::Error> {
    let paths = queries::get_all_sent_replays(pool).await?;
    let mut set = ts.sent_replays.write();
    set.clear();
    for p in &paths {
        set.insert(p.clone());
    }
    info!("  loaded {} sent replays", set.len());
    Ok(())
}

/// Load chart configs.
async fn load_chart_configs(pool: &SqlitePool, ts: &mut TabState) -> Result<(), sqlx::Error> {
    let rows = queries::get_all_chart_configs(pool).await?;
    let mut p = ts.persisted.write();
    p.chart_configs.clear();
    for (chart_id, json) in rows {
        match serde_json::from_str(&json) {
            Ok(config) => {
                p.chart_configs.insert(chart_id as u64, config);
            }
            Err(e) => {
                warn!("Failed to deserialize chart config {chart_id}: {e}");
            }
        }
    }
    info!("  loaded {} chart configs", p.chart_configs.len());
    Ok(())
}

/// Load armor viewer defaults.
async fn load_armor_viewer_defaults(pool: &SqlitePool, ts: &mut TabState) -> Result<(), sqlx::Error> {
    if let Some(row) = queries::get_armor_viewer_defaults(pool).await? {
        let mut p = ts.persisted.write();
        p.armor_viewer_defaults.show_plate_edges = row.show_plate_edges;
        p.armor_viewer_defaults.show_waterline = row.show_waterline;
        p.armor_viewer_defaults.show_zero_mm = row.show_zero_mm;
        p.armor_viewer_defaults.armor_opacity = row.armor_opacity as f32;
        p.armor_viewer_defaults.waterline_opacity = row.waterline_opacity as f32;
        p.armor_viewer_defaults.hull_opaque = row.hull_opaque;
        p.armor_viewer_defaults.hull_all_visible = row.hull_all_visible;
        p.armor_viewer_defaults.armor_all_visible = row.armor_all_visible;
        p.armor_viewer_defaults.show_splash_boxes = row.show_splash_boxes;
        p.armor_viewer_defaults.show_legend = row.show_legend;
        p.armor_viewer_defaults.legend_collapsed = row.legend_collapsed;
        p.armor_viewer_defaults.legend_pos = match (row.legend_pos_x, row.legend_pos_y) {
            (Some(x), Some(y)) => Some([x as f32, y as f32]),
            _ => None,
        };
    }
    info!("  loaded armor viewer defaults");
    Ok(())
}

/// Load render options.
async fn load_render_options(pool: &SqlitePool, ts: &mut TabState) -> Result<(), sqlx::Error> {
    if let Some(json) = queries::get_render_options(pool).await? {
        match serde_json::from_str(&json) {
            Ok(opts) => ts.persisted.write().settings.renderer = opts,
            Err(e) => warn!("Failed to deserialize render options: {e}"),
        }
    }
    info!("  loaded render options");
    Ok(())
}

/// Load dock layouts.
async fn load_dock_layout(pool: &SqlitePool, ts: &mut TabState) -> Result<(), sqlx::Error> {
    if let Some(json) = queries::get_dock_layout(pool, "stats").await? {
        match serde_json::from_str(&json) {
            Ok(layout) => ts.persisted.write().stats_dock_state = layout,
            Err(e) => warn!("Failed to deserialize stats dock layout: {e}"),
        }
    }
    if let Some(json) = queries::get_dock_layout(pool, "player_tracker").await? {
        match serde_json::from_str(&json) {
            Ok(layout) => ts.persisted.write().player_tracker_dock_state = layout,
            Err(e) => warn!("Failed to deserialize player tracker dock layout: {e}"),
        }
    }
    info!("  loaded dock layouts");
    Ok(())
}

/// Load mod manager state.
async fn load_mod_manager(pool: &SqlitePool, ts: &mut TabState) -> Result<(), sqlx::Error> {
    if let Some(json) = queries::get_mod_manager(pool).await? {
        match serde_json::from_str(&json) {
            Ok(info) => ts.persisted.write().mod_manager_info = info,
            Err(e) => warn!("Failed to deserialize mod manager info: {e}"),
        }
    }
    info!("  loaded mod manager info");
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use parking_lot::RwLock;
    use wows_replays::types::AccountId;
    use wows_replays::types::ArenaId;
    use wows_toolkit_viewmodel::player_tracker::store;
    use wows_toolkit_viewmodel::player_tracker::tracked;
    use wows_toolkit_viewmodel::player_tracker::tracked::TrackedPlayer;

    use crate::ui::player_tracker::PlayerTracker;

    fn runtime() -> Arc<tokio::runtime::Runtime> {
        Arc::new(tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().expect("a runtime"))
    }

    fn at(second: i64) -> jiff::Timestamp {
        jiff::Timestamp::from_second(second).expect("a second in range")
    }

    /// A stored tracker with a note, an alias and two encounters, as an older
    /// build wrote it.
    const BLOB: &str = r#"{"tracked_players":{"501":{"last_name":"Enemy","db_id":501,"names":["OldHandle"],
        "clan_id":7,"clan":"RAIN","timestamps":["2023-11-14T22:13:20Z","2023-11-14T23:13:20Z"],
        "arena_ids":[100,101],"notes":"camps the corner",
        "division_encounters":{"arena_ids":[101],"timestamps":["2023-11-14T23:13:20Z"]}}},
        "show_division_mates":true}"#;

    /// The import brings the blob's players in, notes and aliases and marks
    /// included, and a battle that ended while it ran keeps its encounter.
    ///
    /// This is the path a reader upgrading takes, so it is the one worth
    /// driving: the merge it performs is what the tracker shows afterwards and
    /// what the next save writes.
    #[test]
    fn importing_keeps_both_the_stored_tracker_and_a_battle_met_meanwhile() {
        let runtime = runtime();
        let directory = tempfile::tempdir().expect("a temporary directory");
        let db_path = directory.path().join("import.db");

        // The blob an older build left, and nothing else.
        runtime.block_on(async {
            let pool = wows_toolkit_config::open_db_at(&db_path).await.expect("the database opens");
            wows_toolkit_config::queries::set_setting(&pool, tracked::SETTING_KEY, &BLOB.to_owned())
                .await
                .expect("the blob stores");
            pool.close().await;
        });

        // A battle that ended just after launch. One of its players is the
        // account the blob already knows, met again: `ingest_roster` records it
        // with the name that battle reported and nothing else, so the entry in
        // memory has no note and no aliases. That is the account an import can
        // overwrite with an empty note, so it is the one worth driving.
        let tracker = Arc::new(RwLock::new(PlayerTracker::default()));
        {
            let mut guard = tracker.write();
            let mut again =
                TrackedPlayer { db_id: AccountId(501), last_name: "EnemyRenamed".to_owned(), ..Default::default() };
            again.arena_ids.insert(ArenaId::new(900));
            again.timestamps.insert(at(1_700_900_000));
            guard.tracked_players.insert(AccountId(501), again);
            guard.pending.identity_changed(AccountId(501));
            guard.pending.encounter_changed(AccountId(501), ArenaId::new(900), at(1_700_900_000));

            let mut met =
                TrackedPlayer { db_id: AccountId(902), last_name: "MetJustNow".to_owned(), ..Default::default() };
            met.arena_ids.insert(ArenaId::new(900));
            met.timestamps.insert(at(1_700_900_000));
            guard.tracked_players.insert(AccountId(902), met);
            guard.pending.identity_changed(AccountId(902));
            guard.pending.encounter_changed(AccountId(902), ArenaId::new(900), at(1_700_900_000));
        }

        super::spawn_tracker_import(&runtime, db_path.clone(), Arc::clone(&tracker), egui::Context::default());
        // The import is the only work on this runtime, so it is finished once
        // the runtime has nothing left to run.
        runtime.block_on(async { tokio::time::sleep(std::time::Duration::from_millis(1500)).await });

        let guard = tracker.read();
        let stored = guard.tracked_players.get(&AccountId(501)).expect("the stored player was imported");
        assert_eq!(stored.notes, "camps the corner", "the note came across");
        assert!(stored.names.contains("OldHandle"), "and the alias");
        assert_eq!(stored.arena_ids.len(), 3, "both stored encounters and the one met meanwhile");
        assert!(stored.arena_in_division(ArenaId::new(101)), "and the division mark");
        assert!(!stored.arena_in_division(ArenaId::new(100)));
        assert_eq!(stored.last_name, "EnemyRenamed", "the newer name a battle reported wins");
        assert!(stored.names.contains("Enemy"), "and the one it displaced became an alias");

        let met = guard.tracked_players.get(&AccountId(902)).expect("the battle met meanwhile is still tracked");
        assert_eq!(met.arena_ids.len(), 1, "with its encounter");
        drop(guard);

        // What the next save writes has to put that battle back on disk: the
        // import wrote the blob's rows, not this one's.
        let pending = tracker.read().pending.clone();
        let players = tracker.read().tracked_players.clone();
        runtime.block_on(async {
            let pool = wows_toolkit_config::open_db_at(&db_path).await.expect("the database opens");
            store::save(&pool, &pending, &players).await.expect("the save lands");

            let read = store::load(&pool).await.expect("the tables read back");
            let stored = read.get(&AccountId(501)).expect("the imported player is on disk");
            assert_eq!(stored.notes, "camps the corner", "the imported note is still on disk");
            assert!(stored.names.contains("OldHandle"));
            assert_eq!(stored.arena_ids.len(), 3);
            assert!(stored.arena_in_division(ArenaId::new(101)));

            let met = read.get(&AccountId(902)).expect("and so is the battle met meanwhile");
            assert_eq!(met.arena_ids.iter().collect::<Vec<_>>(), vec![ArenaId::new(900)]);
            assert_eq!(met.last_name, "MetJustNow");

            // The blob is gone only because its rows are there.
            let left: Option<String> =
                wows_toolkit_config::queries::try_get_setting(&pool, tracked::SETTING_KEY).await.expect("the read");
            assert!(left.is_none(), "the blob was dropped once its rows were checked");
            pool.close().await;
        });
    }

    /// A battle written to the tables before the import runs does not stop it:
    /// the import folds in, and neither side loses rows.
    #[test]
    fn a_battle_already_on_disk_does_not_block_the_import() {
        let runtime = runtime();
        let directory = tempfile::tempdir().expect("a temporary directory");
        let db_path = directory.path().join("import.db");

        runtime.block_on(async {
            let pool = wows_toolkit_config::open_db_at(&db_path).await.expect("the database opens");
            wows_toolkit_config::queries::set_setting(&pool, tracked::SETTING_KEY, &BLOB.to_owned())
                .await
                .expect("the blob stores");

            // The save task got there first.
            let mut met =
                TrackedPlayer { db_id: AccountId(902), last_name: "MetJustNow".to_owned(), ..Default::default() };
            met.arena_ids.insert(ArenaId::new(900));
            met.timestamps.insert(at(1_700_900_000));
            let players = [(AccountId(902), met)].into_iter().collect();
            let mut pending = tracked::Pending::default();
            pending.identity_changed(AccountId(902));
            pending.encounter_changed(AccountId(902), ArenaId::new(900), at(1_700_900_000));
            store::save(&pool, &pending, &players).await.expect("the live save lands");
            pool.close().await;
        });

        let tracker = Arc::new(RwLock::new(PlayerTracker::default()));
        super::spawn_tracker_import(&runtime, db_path.clone(), Arc::clone(&tracker), egui::Context::default());
        runtime.block_on(async { tokio::time::sleep(std::time::Duration::from_millis(1500)).await });

        runtime.block_on(async {
            let pool = wows_toolkit_config::open_db_at(&db_path).await.expect("the database opens");
            let read = store::load(&pool).await.expect("the tables read back");

            assert!(read.contains_key(&AccountId(501)), "the blob was imported anyway");
            assert_eq!(read[&AccountId(501)].notes, "camps the corner");
            assert!(read.contains_key(&AccountId(902)), "and the battle already on disk survived");

            let left: Option<String> =
                wows_toolkit_config::queries::try_get_setting(&pool, tracked::SETTING_KEY).await.expect("the read");
            assert!(left.is_none(), "the blob is gone, so the next launch has nothing to import");
            pool.close().await;
        });
    }
}
