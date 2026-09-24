//! Settings snapshot loaded from the shared SQLite config DB at startup.
//!
//! An edit is written back through [`crate::settings_store`], to the same row
//! the egui app reads, so a setting changed in either app holds in both.

use std::path::PathBuf;

use sqlx::sqlite::SqlitePool;
use wows_toolkit_config::ReplaySettings;
use wows_toolkit_config::queries;
use wows_toolkit_config::queries::ArmorViewerDefaultsRow;
use wows_toolkit_viewmodel::settings::DataSharingMode;
use wows_toolkit_viewmodel::settings::ThemeChoice;
use wows_toolkit_viewmodel::settings::keys;
use wows_toolkit_viewmodel::twitch::Token as TwitchToken;
use wows_toolkit_viewmodel::twitch::keys as twitch_keys;

/// Zoom factor applied when the `zoom_factor` setting has never been saved,
/// matching the egui app's documented default.
pub const DEFAULT_ZOOM: f32 = 1.15;

/// Bounds for the zoom slider, mirroring the egui settings tab.
pub const MIN_ZOOM: f32 = 0.5;
pub const MAX_ZOOM: f32 = 2.0;

/// Settings read from the shared config DB, applied once at startup.
pub struct GpuiSettings {
    pub zoom: f32,
    /// Which palette to render in. The egui app writes the same setting, so
    /// the two open in the theme the user last chose in either.
    pub theme: ThemeChoice,
    /// The Twitch credential the chat poll runs under, and the channel it
    /// watches. Both written by the egui app under the same keys; an empty
    /// channel watches the credential's own.
    pub twitch_token: Option<TwitchToken>,
    pub twitch_channel: String,
    pub wows_dir: String,
    /// The locale the app reads numbers and dates in. `None` until one is
    /// chosen, which formats as `en-US` (see `formatting::separate_number`).
    pub locale: Option<String>,
    pub current_replay_path: PathBuf,
    pub replay: ReplaySettings,
    /// `AppPreferences.debug_mode` in the egui app: unhides NDA-hidden stats
    /// in the Replay Inspector and reveals its raw-metadata/raw-results
    /// viewers. Read-only seed for the RI's session debug toggle (see
    /// `replay_inspector::view::ReplayInspectorView::set_debug_mode`); this
    /// crate never writes it back.
    pub debug_mode: bool,
    /// `check_for_updates` in the shared database: whether the app looks for a
    /// new release at startup. Defaults on, as the egui app does.
    pub check_for_updates: bool,
    pub enable_logging: bool,
    pub data_sharing: DataSharingMode,
    /// Empty means no proxy, which is the absence this setting has always
    /// used rather than a separate enabled flag.
    pub proxy_url: String,
    /// `TabState.persisted.auto_load_latest_replay` in the egui app: a
    /// top-level setting, not part of `ReplaySettings`. Read-only seed for the
    /// RI header's "Autoload Latest Replay" checkbox; this crate never writes
    /// it back, and does not yet act on it (see the checkbox's own doc
    /// comment in `replay_inspector::view`).
    pub auto_load_latest_replay: bool,
    /// Where the unpacker writes extracted files. Empty until the user picks
    /// one, which is the absence this setting has always used; the Extract
    /// control stays disabled while it is.
    pub output_dir: String,
    /// `None` when the `armor_viewer_defaults` table has no row yet (fresh DB),
    /// or when the read failed (logged via `tracing::warn!` in `load`).
    pub armor_defaults: Option<ArmorViewerDefaultsRow>,
    /// Whether game data is cached on load so old replays still open after a
    /// game update. The port does not dump; it reports and manages the cache
    /// the egui app writes.
    pub auto_dump_game_data: bool,
    /// Where the game-data cache is kept. Empty means the default location,
    /// which is the absence this setting has always used.
    pub game_data_cache_dir: String,
    /// The repository commit the cache was last checked against. `None` until
    /// a check has run, which is what makes the first check a full one.
    pub game_data_repo_commit: Option<String>,
    /// The name this app appears under to the peers in a session. Empty until
    /// one is chosen, which is what refuses hosting and joining.
    pub collab_display_name: String,
    /// Whether the warning that hosting reveals an address is suppressed.
    pub suppress_p2p_ip_warning: bool,
    /// Whether windows a session opens are left for the reader to open.
    pub disable_auto_open_session_windows: bool,
}

impl GpuiSettings {
    /// Load all leaf settings this tab displays. Each `get_setting` miss falls
    /// back to that field's documented default rather than a sentinel value.
    pub async fn load(pool: &SqlitePool) -> Self {
        let zoom = queries::get_setting::<f32>(pool, keys::ZOOM_FACTOR).await.unwrap_or(DEFAULT_ZOOM);
        // Never chosen means follow the desktop, which is the egui default.
        let theme = queries::get_setting::<ThemeChoice>(pool, keys::THEME).await.unwrap_or_default();
        let twitch_token = queries::get_setting::<TwitchToken>(pool, twitch_keys::TOKEN).await;
        let twitch_channel =
            queries::get_setting::<String>(pool, twitch_keys::MONITORED_CHANNEL).await.unwrap_or_default();
        let wows_dir = queries::get_setting::<String>(pool, keys::WOWS_DIR).await.unwrap_or_default();
        let locale = queries::get_setting::<Option<String>>(pool, keys::LOCALE).await.flatten();
        let current_replay_path =
            queries::get_setting::<PathBuf>(pool, keys::CURRENT_REPLAY_PATH).await.unwrap_or_default();
        let replay = queries::get_setting::<ReplaySettings>(pool, keys::REPLAY_SETTINGS).await.unwrap_or_default();
        let debug_mode = queries::get_setting::<bool>(pool, keys::DEBUG_MODE).await.unwrap_or(false);
        let check_for_updates = queries::get_setting::<bool>(pool, keys::CHECK_FOR_UPDATES).await.unwrap_or(true);
        let enable_logging = queries::get_setting::<bool>(pool, keys::ENABLE_LOGGING).await.unwrap_or(false);
        let data_sharing =
            queries::get_setting::<DataSharingMode>(pool, keys::DATA_SHARING_MODE).await.unwrap_or_default();
        // Stored as a nullable string: absent and empty both mean no proxy.
        let proxy_url =
            queries::get_setting::<Option<String>>(pool, keys::PROXY_URL).await.flatten().unwrap_or_default();
        let auto_load_latest_replay =
            queries::get_setting::<bool>(pool, keys::AUTO_LOAD_LATEST_REPLAY).await.unwrap_or(true);
        let output_dir = queries::get_setting::<String>(pool, keys::OUTPUT_DIR).await.unwrap_or_default();
        let armor_defaults = match queries::get_armor_viewer_defaults(pool).await {
            Ok(defaults) => defaults,
            Err(e) => {
                tracing::warn!("Failed to read armor viewer defaults from DB: {e}");
                None
            }
        };

        let auto_dump_game_data = queries::get_setting::<bool>(pool, keys::AUTO_DUMP_GAME_DATA).await.unwrap_or(false);
        let game_data_cache_dir =
            queries::get_setting::<String>(pool, keys::GAME_DATA_CACHE_DIR).await.unwrap_or_default();
        let game_data_repo_commit =
            queries::get_setting::<Option<String>>(pool, keys::GAME_DATA_REPO_COMMIT).await.flatten();

        let collab_display_name =
            queries::get_setting::<String>(pool, keys::COLLAB_DISPLAY_NAME).await.unwrap_or_default();
        let suppress_p2p_ip_warning =
            queries::get_setting::<bool>(pool, keys::SUPPRESS_P2P_IP_WARNING).await.unwrap_or(false);
        let disable_auto_open_session_windows =
            queries::get_setting::<bool>(pool, keys::DISABLE_AUTO_OPEN_SESSION_WINDOWS).await.unwrap_or(false);

        Self {
            zoom,
            theme,
            twitch_token,
            twitch_channel,
            wows_dir,
            locale,
            current_replay_path,
            replay,
            debug_mode,
            check_for_updates,
            enable_logging,
            data_sharing,
            proxy_url,
            auto_load_latest_replay,
            output_dir,
            armor_defaults,
            auto_dump_game_data,
            game_data_cache_dir,
            game_data_repo_commit,
            collab_display_name,
            suppress_p2p_ip_warning,
            disable_auto_open_session_windows,
        }
    }
}
