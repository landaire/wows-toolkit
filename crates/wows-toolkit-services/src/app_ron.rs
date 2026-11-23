//! One-time move of the settings in `app.ron`, which releases before the SQLite
//! database kept their state in, into that database.
//!
//! The legacy types below mirror the fields of the old `app.ron` layout that the
//! app still reads. Fields it no longer reads are left out, and serde skips them.
//! Their defaults are the ones the app that wrote the file used, so a field the
//! file omits migrates as the value that app ran with.

use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;
use sqlx::SqlitePool;
use wows_minimap_renderer::SavedRenderOptions;
use wows_replays::types::AccountId;
use wows_replays::types::GameParamId;
use wows_toolkit_config::ReplaySettings;
use wows_toolkit_config::queries;
use wows_toolkit_config::queries::ArmorViewerDefaultsRow;
use wows_toolkit_viewmodel::player_tracker::TimePeriod;
use wows_toolkit_viewmodel::player_tracker::live::CurrentMatchViewMode;
use wows_toolkit_viewmodel::player_tracker::live::WinRateMode;
use wows_toolkit_viewmodel::player_tracker::saved_view;
use wows_toolkit_viewmodel::player_tracker::store;
use wows_toolkit_viewmodel::player_tracker::tracked::Pending;
use wows_toolkit_viewmodel::player_tracker::tracked::TrackedPlayer;
use wows_toolkit_viewmodel::player_tracker::tracked::ViewModes;
use wows_toolkit_viewmodel::replay_sort::SortOrder;
use wows_toolkit_viewmodel::settings::DataSharingMode;
use wows_toolkit_viewmodel::settings::keys;
use wows_toolkit_viewmodel::stats::BattleDetails;
use wows_toolkit_viewmodel::stats::DivisionFilter;
use wows_toolkit_viewmodel::stats::PerGameStat;
use wows_toolkit_viewmodel::stats::SerializableAchievement;
use wows_toolkit_viewmodel::stats::chart::ChartMode;
use wows_toolkit_viewmodel::stats::chart::ChartableStat;
use wows_toolkit_viewmodel::stats::setting_keys as stats_keys;
use wows_toolkit_viewmodel::twitch::Token as TwitchToken;
use wows_toolkit_viewmodel::twitch::keys as twitch_keys;

/// The legacy settings file, in the storage directory.
pub const FILE_NAME: &str = "app.ron";

/// What the file is renamed to once its contents are in the database.
pub const MIGRATED_FILE_NAME: &str = "app.ron.migrated";

#[derive(Debug, thiserror::Error)]
pub enum AppRonError {
    #[error("{path} could not be read")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path} is not a settings file this app can read")]
    Parse {
        path: PathBuf,
        #[source]
        source: Box<ron::error::SpannedError>,
    },
    #[error("the migrated settings could not be stored")]
    Store(#[source] sqlx::Error),
    #[error("{path} could not be renamed after its settings were migrated")]
    Rename {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// What [`migrate`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The database records an earlier migration, so the file was not read.
    AlreadyMigrated,
    /// There is no `app.ron`, or it holds no app state.
    NothingToMigrate,
    Migrated,
}

/// The top level of the legacy app state. Only `tab_state` was ever saved.
#[derive(Deserialize, Default)]
#[serde(default)]
struct LegacyApp {
    tab_state: LegacyTabState,
}

#[derive(Deserialize)]
#[serde(default)]
struct LegacyTabState {
    settings: LegacySettings,
    output_dir: String,
    auto_load_latest_replay: bool,
    replay_sort: SortOrder,
    chart_configs: HashMap<u64, LegacyChartConfig>,
    armor_viewer_defaults: LegacyArmorViewerDefaults,
}

impl Default for LegacyTabState {
    fn default() -> Self {
        Self {
            settings: LegacySettings::default(),
            output_dir: String::new(),
            auto_load_latest_replay: true,
            replay_sort: SortOrder::default(),
            chart_configs: HashMap::new(),
            armor_viewer_defaults: LegacyArmorViewerDefaults::default(),
        }
    }
}

#[derive(Deserialize)]
#[serde(default)]
struct LegacySettings {
    current_replay_path: PathBuf,
    wows_dir: String,
    locale: Option<String>,
    replay_settings: ReplaySettings,
    check_for_updates: bool,
    send_replay_data: bool,
    sent_replays: HashSet<String>,
    player_tracker: LegacyPlayerTracker,
    twitch_token: Option<TwitchToken>,
    twitch_monitored_channel: String,
    constants_file_commit: Option<String>,
    debug_mode: bool,
    build_consent_window_shown: bool,
    language_selection_shown: bool,
    renderer_options: SavedRenderOptions,
    session_stats_limit_enabled: bool,
    session_stats_game_count: usize,
    session_stats_division_filter: DivisionFilter,
    session_stats_game_mode_filter: BTreeSet<String>,
    session_stats: LegacySessionStats,
    suppress_gpu_encoder_warning: bool,
    enable_logging: bool,
    collab_display_name: String,
    suppress_p2p_ip_warning: bool,
    disable_auto_open_session_windows: bool,
}

impl Default for LegacySettings {
    fn default() -> Self {
        Self {
            current_replay_path: PathBuf::new(),
            wows_dir: "C:\\Games\\World_of_Warships".to_string(),
            locale: Some("en".to_string()),
            replay_settings: ReplaySettings::default(),
            check_for_updates: true,
            send_replay_data: false,
            sent_replays: HashSet::new(),
            player_tracker: LegacyPlayerTracker::default(),
            twitch_token: None,
            twitch_monitored_channel: String::new(),
            constants_file_commit: None,
            debug_mode: false,
            build_consent_window_shown: false,
            language_selection_shown: false,
            renderer_options: SavedRenderOptions::default(),
            session_stats_limit_enabled: false,
            session_stats_game_count: 20,
            session_stats_division_filter: DivisionFilter::default(),
            session_stats_game_mode_filter: BTreeSet::new(),
            session_stats: LegacySessionStats::default(),
            suppress_gpu_encoder_warning: false,
            enable_logging: true,
            collab_display_name: String::new(),
            suppress_p2p_ip_warning: false,
            disable_auto_open_session_windows: false,
        }
    }
}

/// The tracker's saved fields. Its sort orders are not migrated: they name the
/// old tracker's columns, and a tracker without them opens on its default sort.
#[derive(Deserialize, Default)]
#[serde(default)]
struct LegacyPlayerTracker {
    tracked_players: HashMap<AccountId, TrackedPlayer>,
    filter_time_period: TimePeriod,
    player_filter: String,
    show_division_mates: bool,
    win_rate_mode: WinRateMode,
    current_match_view_mode: CurrentMatchViewMode,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct LegacySessionStats {
    games: Vec<LegacyGame>,
}

#[derive(Deserialize)]
struct LegacyGame {
    ship_name: String,
    ship_id: GameParamId,
    game_time: String,
    #[serde(default)]
    sort_key: String,
    player_id: i64,
    damage: u64,
    spotting_damage: u64,
    frags: i64,
    raw_xp: i64,
    base_xp: i64,
    is_win: bool,
    is_loss: bool,
    is_draw: bool,
    is_div: bool,
    #[serde(default)]
    match_group: String,
    #[serde(default)]
    achievements: Vec<SerializableAchievement>,
    #[serde(default)]
    match_contribution: Option<i64>,
}

impl LegacyGame {
    fn into_game(self) -> PerGameStat {
        PerGameStat {
            ship_name: self.ship_name,
            ship_id: self.ship_id,
            game_time: self.game_time,
            sort_key: self.sort_key,
            player_id: self.player_id,
            damage: self.damage,
            spotting_damage: self.spotting_damage,
            frags: self.frags,
            raw_xp: self.raw_xp,
            base_xp: self.base_xp,
            is_win: self.is_win,
            is_loss: self.is_loss,
            is_draw: self.is_draw,
            is_div: self.is_div,
            match_group: self.match_group,
            achievements: self.achievements,
            match_contribution: self.match_contribution,
            // The legacy record never carried these, so they read as not recorded.
            details: BattleDetails::default(),
        }
    }
}

/// One saved chart, written to the `chart_configs` table in the shape the
/// stats tab imports from it. Ship selections named ships by name, which the
/// import drops, so they are not carried.
#[derive(Deserialize, Serialize)]
struct LegacyChartConfig {
    selected_stat: ChartableStat,
    mode: ChartMode,
    selected_ships_manually_changed: bool,
    rolling_average: bool,
    #[serde(default)]
    combined: bool,
    show_labels: bool,
}

/// The armor viewer's display defaults. Lighting postdates `app.ron`.
#[derive(Deserialize)]
#[serde(default)]
struct LegacyArmorViewerDefaults {
    show_plate_edges: bool,
    show_waterline: bool,
    show_zero_mm: bool,
    armor_opacity: f32,
    waterline_opacity: f32,
    hull_opaque: bool,
    hull_all_visible: bool,
    armor_all_visible: bool,
    show_splash_boxes: bool,
    show_legend: bool,
    legend_collapsed: bool,
    legend_pos: Option<[f32; 2]>,
}

impl Default for LegacyArmorViewerDefaults {
    fn default() -> Self {
        Self {
            show_plate_edges: true,
            show_waterline: true,
            show_zero_mm: false,
            armor_opacity: 1.0,
            waterline_opacity: 0.3,
            hull_opaque: false,
            hull_all_visible: false,
            armor_all_visible: true,
            show_splash_boxes: false,
            show_legend: true,
            legend_collapsed: false,
            legend_pos: None,
        }
    }
}

impl LegacyArmorViewerDefaults {
    fn to_row(&self) -> ArmorViewerDefaultsRow {
        ArmorViewerDefaultsRow {
            show_plate_edges: self.show_plate_edges,
            show_waterline: self.show_waterline,
            show_zero_mm: self.show_zero_mm,
            armor_opacity: f64::from(self.armor_opacity),
            waterline_opacity: f64::from(self.waterline_opacity),
            hull_opaque: self.hull_opaque,
            hull_all_visible: self.hull_all_visible,
            armor_all_visible: self.armor_all_visible,
            show_splash_boxes: self.show_splash_boxes,
            show_legend: self.show_legend,
            legend_collapsed: self.legend_collapsed,
            legend_pos_x: self.legend_pos.map(|pos| f64::from(pos[0])),
            legend_pos_y: self.legend_pos.map(|pos| f64::from(pos[1])),
            lighting: None,
        }
    }
}

/// Moves `app.ron` in `storage_dir` into `pool`, once.
///
/// Runs before the settings are read, so a first launch after an upgrade from a
/// release that kept its state in `app.ron` opens on that state. The file is
/// renamed to [`MIGRATED_FILE_NAME`] afterwards and kept as a backup.
pub async fn migrate(pool: &SqlitePool, storage_dir: &Path) -> Result<Outcome, AppRonError> {
    if wows_toolkit_config::is_migrated(pool).await {
        return Ok(Outcome::AlreadyMigrated);
    }

    let path = storage_dir.join(FILE_NAME);
    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Outcome::NothingToMigrate),
        Err(source) => return Err(AppRonError::Read { path, source }),
    };

    let Some(app) =
        parse(&contents).map_err(|source| AppRonError::Parse { path: path.clone(), source: Box::new(source) })?
    else {
        return Ok(Outcome::NothingToMigrate);
    };

    tracing::info!("Migrating {} to SQLite", path.display());
    store_app(pool, app).await.map_err(AppRonError::Store)?;
    wows_toolkit_config::set_migrated(pool).await.map_err(AppRonError::Store)?;

    let migrated_path = storage_dir.join(MIGRATED_FILE_NAME);
    if !migrated_path.exists() {
        std::fs::rename(&path, &migrated_path).map_err(|source| AppRonError::Rename { path, source })?;
    }

    Ok(Outcome::Migrated)
}

/// The app state in an `app.ron` file, or `None` when it holds none.
///
/// The file is eframe's key-value storage: a RON map of strings, with the app
/// state under `"app"` as a nested RON string.
fn parse(contents: &str) -> Result<Option<LegacyApp>, ron::error::SpannedError> {
    let storage: HashMap<String, String> = ron::from_str(contents)?;
    let Some(app) = storage.get("app").filter(|app| !app.is_empty()) else {
        return Ok(None);
    };

    ron::from_str(app).map(Some)
}

async fn store_app(pool: &SqlitePool, app: LegacyApp) -> Result<(), sqlx::Error> {
    let tab_state = app.tab_state;
    let settings = tab_state.settings;

    queries::set_setting(pool, keys::CURRENT_REPLAY_PATH, &settings.current_replay_path).await?;
    queries::set_setting(pool, keys::WOWS_DIR, &settings.wows_dir).await?;
    queries::set_setting(pool, keys::LOCALE, &settings.locale).await?;
    queries::set_setting(pool, keys::REPLAY_SETTINGS, &settings.replay_settings).await?;
    queries::set_setting(pool, keys::CHECK_FOR_UPDATES, &settings.check_for_updates).await?;
    queries::set_setting(pool, keys::DEBUG_MODE, &settings.debug_mode).await?;
    queries::set_setting(pool, keys::ENABLE_LOGGING, &settings.enable_logging).await?;
    queries::set_setting(pool, keys::BUILD_CONSENT_SHOWN, &settings.build_consent_window_shown).await?;
    queries::set_setting(pool, keys::LANGUAGE_SELECTION_SHOWN, &settings.language_selection_shown).await?;
    queries::set_setting(pool, keys::SUPPRESS_GPU_ENCODER_WARNING, &settings.suppress_gpu_encoder_warning).await?;
    queries::set_setting(pool, keys::CONSTANTS_FILE_COMMIT, &settings.constants_file_commit).await?;
    let data_sharing = DataSharingMode::from_send_replay_data_bool(settings.send_replay_data);
    queries::set_setting(pool, keys::DATA_SHARING_MODE, &data_sharing).await?;
    queries::set_setting(pool, keys::SEND_REPLAY_DATA, &settings.send_replay_data).await?;
    if let Some(token) = &settings.twitch_token {
        queries::set_setting(pool, twitch_keys::TOKEN, token).await?;
    }
    queries::set_setting(pool, twitch_keys::MONITORED_CHANNEL, &settings.twitch_monitored_channel).await?;
    queries::set_setting(pool, keys::COLLAB_DISPLAY_NAME, &settings.collab_display_name).await?;
    queries::set_setting(pool, keys::SUPPRESS_P2P_IP_WARNING, &settings.suppress_p2p_ip_warning).await?;
    queries::set_setting(pool, keys::DISABLE_AUTO_OPEN_SESSION_WINDOWS, &settings.disable_auto_open_session_windows)
        .await?;
    queries::set_setting(pool, keys::OUTPUT_DIR, &tab_state.output_dir).await?;
    queries::set_setting(pool, keys::AUTO_LOAD_LATEST_REPLAY, &tab_state.auto_load_latest_replay).await?;
    queries::set_setting(pool, keys::REPLAY_SORT, &tab_state.replay_sort).await?;

    queries::set_setting(pool, stats_keys::LIMIT_ENABLED, &settings.session_stats_limit_enabled).await?;
    queries::set_setting(pool, stats_keys::GAME_COUNT, &settings.session_stats_game_count).await?;
    queries::set_setting(pool, stats_keys::DIVISION_FILTER, &settings.session_stats_division_filter).await?;
    queries::set_setting(pool, stats_keys::GAME_MODE_FILTER, &settings.session_stats_game_mode_filter).await?;

    let renderer_options = serde_json::to_string(&settings.renderer_options).map_err(encode_error)?;
    queries::save_render_options(pool, &renderer_options).await?;
    queries::save_armor_viewer_defaults(pool, &tab_state.armor_viewer_defaults.to_row()).await?;

    // Every other write here is an upsert, so a run that failed partway is repeated in full on
    // the next start. The session rows are the one plain insert: each replaces the row stored
    // under its key, so a repeated run leaves one copy of each game.
    let mut transaction = pool.begin().await?;
    for game in settings.session_stats.games {
        let row = game.into_game().to_row();
        sqlx::query("DELETE FROM session_stats WHERE game_time = ?1 AND player_id = ?2")
            .bind(&row.game_time)
            .bind(row.player_id)
            .execute(&mut *transaction)
            .await?;
        queries::insert_session_stat(&mut *transaction, &row).await?;
    }
    transaction.commit().await?;

    for path in &settings.sent_replays {
        queries::insert_sent_replay(pool, path).await?;
    }

    for (chart_id, config) in &tab_state.chart_configs {
        let json = serde_json::to_string(config).map_err(encode_error)?;
        queries::upsert_chart_config(pool, *chart_id as i64, &json).await?;
    }

    store_tracker(pool, settings.player_tracker).await
}

async fn store_tracker(pool: &SqlitePool, tracker: LegacyPlayerTracker) -> Result<(), sqlx::Error> {
    // Every player is new to the database.
    let mut pending = Pending::default();
    pending.everything_changed();
    let write = store::write_for(&pending, &tracker.tracked_players);
    wows_toolkit_config::tracker::apply_tracker_write(pool, &write).await?;

    let modes = ViewModes {
        win_rate_mode: tracker.win_rate_mode,
        current_match_view_mode: tracker.current_match_view_mode,
        show_division_mates: tracker.show_division_mates,
    };
    store::store_view_modes(pool, modes).await?;

    // The tracker's view imports these keys on its first load.
    queries::set_setting(pool, saved_view::EGUI_TIME_PERIOD_KEY, &tracker.filter_time_period).await?;
    queries::set_setting(pool, saved_view::EGUI_PLAYER_FILTER_KEY, &tracker.player_filter).await
}

fn encode_error(err: serde_json::Error) -> sqlx::Error {
    sqlx::Error::Encode(Box::new(err))
}

#[cfg(test)]
mod tests {
    use wows_toolkit_viewmodel::replay_sort::SortColumn;

    use super::*;

    /// An `app.ron` as a release before the database wrote it, with the app
    /// state nested as a RON string.
    fn app_ron(app: &str) -> String {
        let storage = HashMap::from([("app".to_string(), app.to_string())]);

        ron::to_string(&storage).unwrap()
    }

    const LEGACY_APP: &str = r#"(
        tab_state: (
            settings: (
                wows_dir: "D:\\Games\\World_of_Warships",
                locale: Some("de"),
                check_for_updates: false,
                send_replay_data: true,
                sent_replays: ["a.wowsreplay"],
                debug_mode: true,
                twitch_monitored_channel: "channel",
                collab_display_name: "Host",
                session_stats_game_count: 7,
                session_stats_division_filter: SoloOnly,
                session_stats: (games: [(
                    ship_name: "Yamato",
                    ship_id: 4074649552,
                    game_time: "01.02.2024 12:00:00",
                    player_id: 1,
                    damage: 100000,
                    spotting_damage: 2000,
                    frags: 2,
                    raw_xp: 3000,
                    base_xp: 1500,
                    is_win: true,
                    is_loss: false,
                    is_draw: false,
                    is_div: false,
                )]),
                player_tracker: (
                    filter_time_period: LastWeek,
                    player_filter: "abc",
                    show_division_mates: true,
                ),
                has_052_game_params_fix: true,
            ),
            output_dir: "C:\\out",
            auto_load_latest_replay: false,
            replay_sort: Desc(ActualDamage),
            next_chart_tab_id: 3,
        ),
    )"#;

    async fn open_pool(dir: &Path) -> SqlitePool {
        wows_toolkit_config::open_db_at(&dir.join("wows_toolkit.db")).await.expect("the database opens")
    }

    #[tokio::test]
    async fn settings_in_app_ron_reach_the_database() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE_NAME), app_ron(LEGACY_APP)).unwrap();
        let pool = open_pool(dir.path()).await;

        let outcome = migrate(&pool, dir.path()).await.expect("the file migrates");

        assert_eq!(outcome, Outcome::Migrated);
        let settings = crate::settings::StartupSettings::load(&pool).await;
        assert_eq!(settings.wows_dir, "D:\\Games\\World_of_Warships");
        assert_eq!(settings.locale.as_deref(), Some("de"));
        assert!(!settings.check_for_updates);
        assert!(settings.debug_mode);
        assert_eq!(settings.data_sharing, DataSharingMode::from_send_replay_data_bool(true));
        assert_eq!(settings.twitch_channel, "channel");
        assert_eq!(settings.collab_display_name, "Host");
        assert_eq!(settings.output_dir, "C:\\out");
        assert!(!settings.auto_load_latest_replay);
        assert_eq!(settings.replay_sort, SortOrder::Desc(SortColumn::ActualDamage));
        assert_eq!(queries::get_setting::<usize>(&pool, stats_keys::GAME_COUNT).await, Some(7));
        assert_eq!(
            queries::get_setting::<DivisionFilter>(&pool, stats_keys::DIVISION_FILTER).await,
            Some(DivisionFilter::SoloOnly)
        );
        assert_eq!(
            queries::get_setting::<TimePeriod>(&pool, saved_view::EGUI_TIME_PERIOD_KEY).await,
            Some(TimePeriod::LastWeek)
        );
        assert!(store::load_view_modes(&pool).await.show_division_mates);
        assert_eq!(queries::get_all_sent_replays(&pool).await.unwrap(), ["a.wowsreplay"]);
        let games = queries::get_all_session_stats(&pool).await.unwrap();
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].ship_name, "Yamato");
    }

    #[tokio::test]
    async fn a_run_that_fails_after_the_sessions_and_is_repeated_keeps_one_copy_of_each_game() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE_NAME), app_ron(LEGACY_APP)).unwrap();
        let pool = open_pool(dir.path()).await;
        // The sent replays are written after the sessions, so their missing table fails the run there.
        sqlx::query("ALTER TABLE sent_replays RENAME TO sent_replays_hidden").execute(&pool).await.unwrap();

        assert!(migrate(&pool, dir.path()).await.is_err());

        assert!(!wows_toolkit_config::is_migrated(&pool).await);
        assert_eq!(queries::get_all_session_stats(&pool).await.unwrap().len(), 1);
        sqlx::query("ALTER TABLE sent_replays_hidden RENAME TO sent_replays").execute(&pool).await.unwrap();

        assert_eq!(migrate(&pool, dir.path()).await.unwrap(), Outcome::Migrated);

        assert_eq!(queries::get_all_session_stats(&pool).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_migrated_file_is_kept_as_a_backup_and_not_read_again() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE_NAME), app_ron(LEGACY_APP)).unwrap();
        let pool = open_pool(dir.path()).await;

        migrate(&pool, dir.path()).await.expect("the file migrates");

        assert!(!dir.path().join(FILE_NAME).exists());
        assert!(dir.path().join(MIGRATED_FILE_NAME).exists());
        std::fs::write(dir.path().join(FILE_NAME), app_ron(LEGACY_APP)).unwrap();
        assert_eq!(migrate(&pool, dir.path()).await.unwrap(), Outcome::AlreadyMigrated);
    }

    #[tokio::test]
    async fn no_file_leaves_the_database_alone() {
        let dir = tempfile::tempdir().unwrap();
        let pool = open_pool(dir.path()).await;

        assert_eq!(migrate(&pool, dir.path()).await.unwrap(), Outcome::NothingToMigrate);
        assert!(!wows_toolkit_config::is_migrated(&pool).await);
    }

    #[test]
    fn storage_without_app_state_holds_nothing_to_migrate() {
        let storage = HashMap::from([("app".to_string(), String::new())]);

        assert!(parse(&ron::to_string(&storage).unwrap()).unwrap().is_none());
    }

    #[test]
    fn an_unreadable_file_is_an_error() {
        assert!(parse(&app_ron("(tab_state: 5)")).is_err());
    }
}
