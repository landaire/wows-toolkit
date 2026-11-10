use nsh_core::prelude::*;
use rust_i18n::Backend as _;
use wows_toolkit_viewmodel::settings::{DataSharingMode, keys, normalize_proxy_url};

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

/// Settings shared with the egui and GPUI front ends.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct SettingsState {
    pub wows_dir: String,
    pub locale: String,
    pub check_for_updates: bool,
    pub enable_logging: bool,
    pub debug_mode: bool,
    pub auto_load_latest_replay: bool,
    pub output_dir: String,
    pub auto_dump_game_data: bool,
    pub game_data_cache_dir: String,
    pub data_sharing: String,
    pub proxy_url: String,
    pub collab_display_name: String,
    pub disable_auto_open_session_windows: bool,
    pub storage_dir: String,
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

    /// Reads settings from the shared WoWs Toolkit database.
    pub async fn load_settings() -> Result<SettingsState, String> {
        blocking(load_settings).await.map_err(|error| format!("settings reader task failed: {error}"))?
    }

    /// Writes only the edited settings as one transaction.
    pub async fn save_settings(settings: SettingsState, changed: Vec<String>) -> Result<SettingsState, String> {
        blocking(move || save_settings(settings, changed))
            .await
            .map_err(|error| format!("settings writer task failed: {error}"))?
    }

    /// Searches the shared replay index using the common query language.
    pub async fn search_index(query: String, sort_column: String, descending: bool) -> Result<SearchState, String> {
        blocking(move || search_index(&query, &sort_column, descending))
            .await
            .map_err(|error| format!("search task failed: {error}"))?
    }
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

fn load_settings() -> Result<SettingsState, String> {
    database_runtime()?.block_on(async {
        let pool = wows_toolkit_config::open_db()
            .await
            .map_err(|error| format!("could not open the settings database: {error}"))?;
        load_settings_from_pool(&pool).await
    })
}

async fn load_settings_from_pool(pool: &sqlx::SqlitePool) -> Result<SettingsState, String> {
    let mode = optional_setting::<DataSharingMode>(pool, keys::DATA_SHARING_MODE).await?;
    let compatibility = optional_setting::<bool>(pool, keys::SEND_REPLAY_DATA).await?;
    let data_sharing = match (mode, compatibility) {
        (Some(mode), Some(shared)) => mode.reconcile_with_compat_bool(shared),
        (Some(mode), None) => mode,
        (None, Some(shared)) => DataSharingMode::from_send_replay_data_bool(shared),
        (None, None) => DataSharingMode::default(),
    };
    let locale = optional_setting::<Option<String>>(pool, keys::LOCALE).await?.flatten().unwrap_or_default();
    let proxy_url = optional_setting::<Option<String>>(pool, keys::PROXY_URL).await?.flatten().unwrap_or_default();

    Ok(SettingsState {
        wows_dir: setting_or_default(pool, keys::WOWS_DIR).await?,
        locale,
        check_for_updates: setting_or(pool, keys::CHECK_FOR_UPDATES, true).await?,
        enable_logging: setting_or(pool, keys::ENABLE_LOGGING, true).await?,
        debug_mode: setting_or(pool, keys::DEBUG_MODE, false).await?,
        auto_load_latest_replay: setting_or(pool, keys::AUTO_LOAD_LATEST_REPLAY, true).await?,
        output_dir: setting_or_default(pool, keys::OUTPUT_DIR).await?,
        auto_dump_game_data: setting_or(pool, keys::AUTO_DUMP_GAME_DATA, false).await?,
        game_data_cache_dir: setting_or_default(pool, keys::GAME_DATA_CACHE_DIR).await?,
        data_sharing: data_sharing_name(data_sharing).to_owned(),
        proxy_url,
        collab_display_name: setting_or_default(pool, keys::COLLAB_DISPLAY_NAME).await?,
        disable_auto_open_session_windows: setting_or(pool, keys::DISABLE_AUTO_OPEN_SESSION_WINDOWS, false).await?,
        storage_dir: wows_toolkit_config::storage_dir().map(|path| path.display().to_string()).unwrap_or_default(),
    })
}

fn save_settings(mut settings: SettingsState, changed: Vec<String>) -> Result<SettingsState, String> {
    database_runtime()?.block_on(async {
        let pool = wows_toolkit_config::open_db()
            .await
            .map_err(|error| format!("could not open the settings database: {error}"))?;
        let mut values = Vec::with_capacity(changed.len() + 1);
        for field in changed {
            match field.as_str() {
                "wowsDir" => {
                    if !settings.wows_dir.is_empty() && !std::path::Path::new(&settings.wows_dir).join("bin").is_dir() {
                        return Err("the World of Warships directory does not contain a bin directory".to_owned());
                    }
                    values.push(encoded_setting(keys::WOWS_DIR, &settings.wows_dir)?);
                }
                "locale" => {
                    settings.locale = settings.locale.trim().to_owned();
                    if !settings.locale.is_empty()
                        && !wt_translations::SUPPORTED_LANGUAGES.iter().any(|language| language.code == settings.locale)
                    {
                        return Err(format!("unsupported locale {:?}", settings.locale));
                    }
                    let locale = (!settings.locale.is_empty()).then(|| settings.locale.clone());
                    values.push(encoded_setting(keys::LOCALE, &locale)?);
                }
                "checkForUpdates" => {
                    values.push(encoded_setting(keys::CHECK_FOR_UPDATES, &settings.check_for_updates)?);
                }
                "enableLogging" => {
                    values.push(encoded_setting(keys::ENABLE_LOGGING, &settings.enable_logging)?);
                }
                "debugMode" => {
                    values.push(encoded_setting(keys::DEBUG_MODE, &settings.debug_mode)?);
                }
                "autoLoadLatestReplay" => {
                    values.push(encoded_setting(keys::AUTO_LOAD_LATEST_REPLAY, &settings.auto_load_latest_replay)?);
                }
                "outputDir" => values.push(encoded_setting(keys::OUTPUT_DIR, &settings.output_dir)?),
                "autoDumpGameData" => {
                    values.push(encoded_setting(keys::AUTO_DUMP_GAME_DATA, &settings.auto_dump_game_data)?);
                }
                "gameDataCacheDir" => {
                    if !settings.game_data_cache_dir.is_empty()
                        && !std::path::Path::new(&settings.game_data_cache_dir).is_absolute()
                    {
                        return Err("the game data cache directory must be an absolute path".to_owned());
                    }
                    values.push(encoded_setting(keys::GAME_DATA_CACHE_DIR, &settings.game_data_cache_dir)?);
                }
                "dataSharing" => {
                    let mode = parse_data_sharing(&settings.data_sharing)?;
                    settings.data_sharing = data_sharing_name(mode).to_owned();
                    values.push(encoded_setting(keys::DATA_SHARING_MODE, &mode)?);
                    values.push(encoded_setting(keys::SEND_REPLAY_DATA, &mode.shares_anything())?);
                }
                "proxyUrl" => {
                    settings.proxy_url = normalize_proxy_url(&settings.proxy_url).unwrap_or_default();
                    if !settings.proxy_url.is_empty() {
                        let parsed = url::Url::parse(&settings.proxy_url)
                            .map_err(|error| format!("invalid proxy URL: {error}"))?;
                        if !parsed.has_host() {
                            return Err("invalid proxy URL: a host is required".to_owned());
                        }
                        if !matches!(parsed.scheme(), "http" | "https") {
                            return Err("invalid proxy URL: only http and https are supported".to_owned());
                        }
                    }
                    let proxy_url = (!settings.proxy_url.is_empty()).then(|| settings.proxy_url.clone());
                    values.push(encoded_setting(keys::PROXY_URL, &proxy_url)?);
                }
                "collabDisplayName" => {
                    values.push(encoded_setting(keys::COLLAB_DISPLAY_NAME, &settings.collab_display_name)?);
                }
                "disableAutoOpenSessionWindows" => {
                    values.push(encoded_setting(
                        keys::DISABLE_AUTO_OPEN_SESSION_WINDOWS,
                        &settings.disable_auto_open_session_windows,
                    )?);
                }
                _ => return Err(format!("unknown settings field {field:?}")),
            }
        }
        wows_toolkit_config::queries::set_settings_raw(&pool, &values)
            .await
            .map_err(|error| format!("could not save settings: {error}"))?;
        Ok(settings)
    })
}

pub(crate) fn database_runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("could not start the settings database runtime: {error}"))
}

async fn optional_setting<T>(pool: &sqlx::SqlitePool, key: &str) -> Result<Option<T>, String>
where
    T: serde::de::DeserializeOwned,
{
    wows_toolkit_config::queries::try_get_setting(pool, key)
        .await
        .map_err(|error| format!("could not read setting {key:?}: {error}"))
}

async fn setting_or_default<T>(pool: &sqlx::SqlitePool, key: &str) -> Result<T, String>
where
    T: serde::de::DeserializeOwned + Default,
{
    Ok(optional_setting(pool, key).await?.unwrap_or_default())
}

async fn setting_or<T>(pool: &sqlx::SqlitePool, key: &str, default: T) -> Result<T, String>
where
    T: serde::de::DeserializeOwned,
{
    Ok(optional_setting(pool, key).await?.unwrap_or(default))
}

fn encoded_setting<T: serde::Serialize>(key: &str, value: &T) -> Result<(String, String), String> {
    serde_json::to_string(value)
        .map(|json| (key.to_owned(), json))
        .map_err(|error| format!("could not encode setting {key:?}: {error}"))
}

fn parse_data_sharing(value: &str) -> Result<DataSharingMode, String> {
    match value {
        "off" => Ok(DataSharingMode::Off),
        "build_data" => Ok(DataSharingMode::BuildData),
        "replays" => Ok(DataSharingMode::Replays),
        _ => Err(format!("unknown data sharing mode {value:?}")),
    }
}

fn data_sharing_name(value: DataSharingMode) -> &'static str {
    match value {
        DataSharingMode::Off => "off",
        DataSharingMode::BuildData => "build_data",
        DataSharingMode::Replays => "replays",
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_through_the_shared_database() {
        let storage = tempfile::tempdir().expect("a settings directory is created");
        wows_toolkit_config::storage_override::set(storage.path().to_owned());
        let install = storage.path().join("World_of_Warships");
        std::fs::create_dir_all(install.join("bin")).expect("a game bin directory is created");
        let settings = SettingsState {
            wows_dir: install.display().to_string(),
            locale: "fr".to_owned(),
            check_for_updates: false,
            enable_logging: false,
            debug_mode: true,
            auto_load_latest_replay: false,
            output_dir: "/tmp/unpacked".to_owned(),
            auto_dump_game_data: true,
            game_data_cache_dir: "/tmp/game-data".to_owned(),
            data_sharing: "replays".to_owned(),
            proxy_url: "proxy.example:8080".to_owned(),
            collab_display_name: "Tester".to_owned(),
            disable_auto_open_session_windows: true,
            storage_dir: storage.path().display().to_string(),
        };

        let fields = vec![
            "wowsDir",
            "locale",
            "checkForUpdates",
            "enableLogging",
            "debugMode",
            "autoLoadLatestReplay",
            "outputDir",
            "autoDumpGameData",
            "gameDataCacheDir",
            "dataSharing",
            "proxyUrl",
            "collabDisplayName",
            "disableAutoOpenSessionWindows",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        let saved = save_settings(settings, fields).expect("settings save");
        assert_eq!(saved.proxy_url, "http://proxy.example:8080");
        assert_eq!(load_settings().expect("settings load"), saved);
        assert_eq!(Toolkit::startup(None).locale, "fr");

        database_runtime().expect("database runtime").block_on(async {
            let pool = wows_toolkit_config::open_db().await.expect("database opens");
            wows_toolkit_config::queries::set_setting(&pool, keys::ENABLE_LOGGING, &true)
                .await
                .expect("external setting change");
        });
        let mut edited = saved;
        edited.collab_display_name = "Changed".to_owned();
        save_settings(edited, vec!["collabDisplayName".to_owned()]).expect("one setting saves");
        let after_concurrent_change = load_settings().expect("settings reload");
        assert!(after_concurrent_change.enable_logging, "an unedited external change is preserved");

        let mut invalid_proxy = after_concurrent_change;
        invalid_proxy.proxy_url = "http://[::1".to_owned();
        assert!(save_settings(invalid_proxy, vec!["proxyUrl".to_owned()]).is_err());

        let mut unsupported_proxy = load_settings().expect("settings reload");
        unsupported_proxy.proxy_url = "file://proxy.example".to_owned();
        assert!(save_settings(unsupported_proxy, vec!["proxyUrl".to_owned()]).is_err());

        let mut relative_cache = load_settings().expect("settings reload");
        relative_cache.game_data_cache_dir = "relative/cache".to_owned();
        assert!(save_settings(relative_cache, vec!["gameDataCacheDir".to_owned()]).is_err());

        database_runtime().expect("database runtime").block_on(async {
            let pool = wows_toolkit_config::open_db().await.expect("database opens");
            wows_toolkit_config::queries::set_setting_raw(&pool, keys::DEBUG_MODE, "not-json")
                .await
                .expect("malformed setting is stored");
        });
        assert!(load_settings().is_err(), "a malformed setting must not become a writable default");
    }
}
