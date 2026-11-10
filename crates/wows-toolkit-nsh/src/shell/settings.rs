//! The settings every front end shares, read and written under the keys the
//! egui and GPUI apps use.
//!
//! Edits are written as they are made, one key at a time, as the GPUI settings
//! tab writes them: there is no apply step to forget, and a key this app does
//! not edit is never rewritten from a stale copy.

use std::collections::HashMap;
use std::sync::Mutex;

use nsh_core::JsonValue;
use nsh_core::prelude::JsObject;
use nsh_core::prelude::PropEnum;
use nsh_core::prelude::background;
use nsh_core::prelude::module;
use serde::Serialize;
use serde::de::DeserializeOwned;
use sqlx::SqlitePool;
use wows_toolkit_config::ReplayExportFormat;
use wows_toolkit_config::ReplaySettings;
use wows_toolkit_viewmodel::settings::DataSharingMode;
use wows_toolkit_viewmodel::settings::ThemeChoice;
use wows_toolkit_viewmodel::settings::keys;
use wows_toolkit_viewmodel::twitch::Token as TwitchToken;
use wows_toolkit_viewmodel::twitch::keys as twitch_keys;

use crate::shell::error::SettingsError;

/// Zoom factor applied when `zoom_factor` has never been saved, the egui
/// app's documented default.
pub const DEFAULT_ZOOM: f32 = 1.15;
/// Bounds of the zoom slider, as the egui and GPUI settings tabs bound it.
pub const MIN_ZOOM: f32 = 0.5;
pub const MAX_ZOOM: f32 = 2.0;

/// The replay columns and export the Settings tab edits.
///
/// A subset of [`ReplaySettings`], which is one stored row: the rest of the
/// row (grouping, the listing's collapse) belongs to the Replay Inspector and
/// is kept as it is when this part is written.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct ReplayPreferences {
    pub show_raw_xp: bool,
    pub show_observed_damage: bool,
    pub show_entity_id: bool,
    pub show_heals: bool,
    pub enable_replay_previews: bool,
    pub auto_export_data: bool,
    /// `json`, `csv` or `cbor`.
    pub auto_export_format: String,
    /// Empty until a directory is chosen.
    pub auto_export_path: String,
}

/// One data-sharing mode as the Settings tab and the first-run question offer
/// it, worded as the other front ends word it.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct SharingMode {
    /// `off`, `build_data` or `replays`.
    pub id: String,
    pub label: String,
    pub description: String,
}

/// What the armor viewer last saved as its defaults. The viewer writes these
/// itself; the Settings tab only reports them.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct ArmorDefaults {
    pub show_plate_edges: bool,
    pub show_waterline: bool,
    pub hull_opaque: bool,
}

/// Every setting the shell and its Settings tab read.
#[derive(Debug, Clone, PartialEq, JsObject)]
#[non_exhaustive]
pub struct AppSettings {
    pub zoom: f64,
    /// A `ThemeChoice` variant name (`System`, `Dracula`, ...).
    pub theme: String,
    pub wows_dir: String,
    /// `None` until a language is chosen; the shell then follows the system's.
    pub locale: Option<String>,
    pub current_replay_path: String,
    pub replay: ReplayPreferences,
    pub debug_mode: bool,
    pub check_for_updates: bool,
    pub enable_logging: bool,
    /// `off`, `build_data` or `replays`.
    pub data_sharing: String,
    /// Empty means no proxy, the absence this setting has always used.
    pub proxy_url: String,
    pub auto_load_latest_replay: bool,
    pub output_dir: String,
    /// `None` until the armor viewer has saved defaults.
    pub armor_defaults: Option<ArmorDefaults>,
    pub auto_dump_game_data: bool,
    /// Empty means the default location under the storage directory.
    pub game_data_cache_dir: String,
    pub game_data_repo_commit: Option<String>,
    pub collab_display_name: String,
    pub suppress_p2p_ip_warning: bool,
    pub disable_auto_open_session_windows: bool,
    pub build_consent_shown: bool,
    pub replay_consent_shown: bool,
    pub language_selection_shown: bool,
    pub suppress_gpu_encoder_warning: bool,
    pub constants_commit: Option<String>,
    /// The account the stored Twitch credential belongs to, if one is stored.
    pub twitch_user: Option<String>,
    pub twitch_channel: String,
    /// The toolkit's storage directory, empty when the platform has none.
    pub storage_dir: String,
    /// Where the game-data cache goes when `game_data_cache_dir` is empty.
    pub default_cache_dir: String,
}

/// One setting the TSX side writes, named as the TS field it edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PropEnum)]
pub enum SettingKey {
    Zoom,
    Theme,
    WowsDir,
    Locale,
    CheckForUpdates,
    EnableLogging,
    DebugMode,
    AutoLoadLatestReplay,
    DataSharing,
    ProxyUrl,
    OutputDir,
    AutoDumpGameData,
    GameDataCacheDir,
    CollabDisplayName,
    SuppressP2pIpWarning,
    DisableAutoOpenSessionWindows,
    BuildConsentShown,
    ReplayConsentShown,
    LanguageSelectionShown,
    SuppressGpuEncoderWarning,
    TwitchChannel,
}

impl SettingKey {
    /// The row this setting is stored in.
    fn row(self) -> &'static str {
        match self {
            Self::Zoom => keys::ZOOM_FACTOR,
            Self::Theme => keys::THEME,
            Self::WowsDir => keys::WOWS_DIR,
            Self::Locale => keys::LOCALE,
            Self::CheckForUpdates => keys::CHECK_FOR_UPDATES,
            Self::EnableLogging => keys::ENABLE_LOGGING,
            Self::DebugMode => keys::DEBUG_MODE,
            Self::AutoLoadLatestReplay => keys::AUTO_LOAD_LATEST_REPLAY,
            Self::DataSharing => keys::DATA_SHARING_MODE,
            Self::ProxyUrl => keys::PROXY_URL,
            Self::OutputDir => keys::OUTPUT_DIR,
            Self::AutoDumpGameData => keys::AUTO_DUMP_GAME_DATA,
            Self::GameDataCacheDir => keys::GAME_DATA_CACHE_DIR,
            Self::CollabDisplayName => keys::COLLAB_DISPLAY_NAME,
            Self::SuppressP2pIpWarning => keys::SUPPRESS_P2P_IP_WARNING,
            Self::DisableAutoOpenSessionWindows => keys::DISABLE_AUTO_OPEN_SESSION_WINDOWS,
            Self::BuildConsentShown => keys::BUILD_CONSENT_SHOWN,
            Self::ReplayConsentShown => keys::REPLAY_CONSENT_SHOWN,
            Self::LanguageSelectionShown => keys::LANGUAGE_SELECTION_SHOWN,
            Self::SuppressGpuEncoderWarning => keys::SUPPRESS_GPU_ENCODER_WARNING,
            Self::TwitchChannel => twitch_keys::MONITORED_CHANNEL,
        }
    }

    /// The rows `value` is written as, checked against the type the row holds.
    fn encode(self, value: serde_json::Value) -> Result<Vec<(&'static str, String)>, SettingsError> {
        let row = self.row();
        let encoded = match self {
            Self::Zoom => {
                let zoom: f32 = decode(row, "number", value)?;
                if !(MIN_ZOOM..=MAX_ZOOM).contains(&zoom) {
                    return Err(SettingsError::ZoomOutOfRange { value: f64::from(zoom), min: MIN_ZOOM, max: MAX_ZOOM });
                }
                vec![(row, json(row, &zoom)?)]
            }
            Self::Theme => {
                let name: String = decode(row, "string", value)?;
                let choice = parse_theme(&name)?;
                vec![(row, json(row, &choice)?)]
            }
            Self::Locale => {
                let code: String = decode(row, "string", value)?;
                if !wt_translations::SUPPORTED_LANGUAGES.iter().any(|language| language.code == code) {
                    return Err(SettingsError::UnknownLanguage { value: code });
                }
                vec![(row, json(row, &code)?)]
            }
            Self::DataSharing => {
                let name: String = decode(row, "string", value)?;
                let mode = parse_sharing(&name)?;
                // The older bool beside the mode, which a build that knows only
                // it still reads: writing the mode alone would have the load
                // reconcile undo this choice.
                vec![(row, json(row, &mode)?), (keys::SEND_REPLAY_DATA, json(row, &mode.shares_anything())?)]
            }
            Self::WowsDir
            | Self::ProxyUrl
            | Self::OutputDir
            | Self::GameDataCacheDir
            | Self::CollabDisplayName
            | Self::TwitchChannel => {
                let text: String = decode(row, "string", value)?;
                vec![(row, json(row, &text.trim())?)]
            }
            Self::CheckForUpdates
            | Self::EnableLogging
            | Self::DebugMode
            | Self::AutoLoadLatestReplay
            | Self::AutoDumpGameData
            | Self::SuppressP2pIpWarning
            | Self::DisableAutoOpenSessionWindows
            | Self::BuildConsentShown
            | Self::ReplayConsentShown
            | Self::LanguageSelectionShown
            | Self::SuppressGpuEncoderWarning => {
                let flag: bool = decode(row, "boolean", value)?;
                vec![(row, json(row, &flag)?)]
            }
        };

        Ok(encoded)
    }
}

/// The shared settings: read in one pass, written one key at a time.
#[module]
pub struct Settings;

#[module]
impl Settings {
    /// Reads every setting the shell uses from the shared database.
    pub async fn load() -> Result<AppSettings, SettingsError> {
        background(async {
            let pool = open().await?;
            load_from(&pool).await
        })
        .await
        .map_err(SettingsError::Task)?
    }

    /// Writes one setting. A wrong type for the key, or a value the key does
    /// not take, is refused rather than written.
    pub async fn save(key: SettingKey, value: JsonValue) -> Result<(), SettingsError> {
        let rows = key.encode(value)?;
        background(write(rows)).await.map_err(SettingsError::Task)?
    }

    /// Writes the Settings tab's part of the replay row, keeping the rest of it.
    pub async fn save_replay(preferences: ReplayPreferences) -> Result<(), SettingsError> {
        let format = parse_export_format(&preferences.auto_export_format)?;
        background(update_replay_settings(move |replay| {
            replay.show_raw_xp = preferences.show_raw_xp;
            replay.show_observed_damage = preferences.show_observed_damage;
            replay.show_entity_id = preferences.show_entity_id;
            replay.show_heals = preferences.show_heals;
            replay.enable_replay_previews = preferences.enable_replay_previews;
            replay.auto_export_data = preferences.auto_export_data;
            replay.auto_export_format = format;
            replay.auto_export_path = preferences.auto_export_path;
        }))
        .await
        .map_err(SettingsError::Task)?
        .map(|_| ())
    }

    /// Whether `path` is a directory a game install could be in: one with a
    /// `bin` directory, which is what the egui settings tab checks.
    pub fn is_game_directory(path: String) -> bool {
        std::path::Path::new(path.trim()).join("bin").is_dir()
    }

    /// Whether `path` names an existing directory.
    pub fn is_directory(path: String) -> bool {
        std::path::Path::new(path.trim()).is_dir()
    }

    /// The data-sharing modes, in the order every front end lists them.
    pub fn sharing_modes() -> Vec<SharingMode> {
        DataSharingMode::ALL
            .into_iter()
            .map(|mode| SharingMode {
                id: sharing_name(mode).to_owned(),
                label: mode.label().to_owned(),
                description: mode.description().to_owned(),
            })
            .collect()
    }
}

/// Reads the replay row, applies `edit` and writes it back, ordered with every
/// other settings write so two editors of the row (this tab and the Replay
/// Inspector) cannot interleave and drop each other's fields. Resolves to the
/// row as written.
pub(crate) async fn update_replay_settings(
    edit: impl FnOnce(&mut ReplaySettings) + Send,
) -> Result<ReplaySettings, SettingsError> {
    let pool = open().await?;
    let _order = WRITE_ORDER.lock().await;
    let mut replay = read_or_default::<ReplaySettings>(&pool, keys::REPLAY_SETTINGS).await?;
    edit(&mut replay);
    let encoded = json(keys::REPLAY_SETTINGS, &replay)?;
    wows_toolkit_config::queries::set_setting_raw(&pool, keys::REPLAY_SETTINGS, &encoded)
        .await
        .map_err(|source| SettingsError::Write { key: keys::REPLAY_SETTINGS, source })?;

    Ok(replay)
}

/// Serializes writes, so two edits of one key land in the order they were made.
static WRITE_ORDER: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The newest generation of each row, so a write a newer one has overtaken is
/// dropped rather than landing after it.
static LATEST: Mutex<Option<HashMap<&'static str, u64>>> = Mutex::new(None);

fn next_generation(row: &'static str) -> u64 {
    let mut latest = LATEST.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let generation = latest.get_or_insert_with(HashMap::new).entry(row).or_insert(0);
    *generation += 1;

    *generation
}

fn is_latest(row: &'static str, generation: u64) -> bool {
    let latest = LATEST.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    latest.as_ref().and_then(|map| map.get(row)) == Some(&generation)
}

async fn write(rows: Vec<(&'static str, String)>) -> Result<(), SettingsError> {
    let generations: Vec<u64> = rows.iter().map(|(row, _)| next_generation(row)).collect();
    let pool = open().await?;
    let _order = WRITE_ORDER.lock().await;
    for ((row, encoded), generation) in rows.into_iter().zip(generations) {
        if !is_latest(row, generation) {
            continue;
        }
        wows_toolkit_config::queries::set_setting_raw(&pool, row, &encoded)
            .await
            .map_err(|source| SettingsError::Write { key: row, source })?;
    }

    Ok(())
}

pub(crate) async fn open() -> Result<SqlitePool, SettingsError> {
    wows_toolkit_config::open_db().await.map_err(SettingsError::Open)
}

pub(crate) async fn load_from(pool: &SqlitePool) -> Result<AppSettings, SettingsError> {
    // Two rows, as the egui app keeps them: the mode, and the older bool that
    // says only whether anything is shared. A disagreement means the bool is the
    // newer word on it.
    let mode = read::<DataSharingMode>(pool, keys::DATA_SHARING_MODE).await?;
    let compatibility = read::<bool>(pool, keys::SEND_REPLAY_DATA).await?;
    let data_sharing = match (mode, compatibility) {
        (Some(mode), Some(shared)) => mode.reconcile_with_compat_bool(shared),
        (Some(mode), None) => mode,
        (None, Some(shared)) => DataSharingMode::from_send_replay_data_bool(shared),
        // Never asked means nothing is shared.
        (None, None) => DataSharingMode::Off,
    };
    let replay = read_or_default::<ReplaySettings>(pool, keys::REPLAY_SETTINGS).await?;
    let armor_defaults = wows_toolkit_config::queries::get_armor_viewer_defaults(pool)
        .await
        .map_err(SettingsError::ArmorDefaults)?
        .map(|row| ArmorDefaults {
            show_plate_edges: row.show_plate_edges,
            show_waterline: row.show_waterline,
            hull_opaque: row.hull_opaque,
        });
    let twitch_user =
        read::<TwitchToken>(pool, twitch_keys::TOKEN).await?.map(|token| token.username().to_owned());
    let current_replay_path = read::<std::path::PathBuf>(pool, keys::CURRENT_REPLAY_PATH)
        .await?
        .map(|path| path.display().to_string())
        .unwrap_or_default();
    // A stored null and an absent row both mean no proxy and no language.
    let proxy_url = read::<Option<String>>(pool, keys::PROXY_URL).await?.flatten().unwrap_or_default();
    let locale = read::<Option<String>>(pool, keys::LOCALE).await?.flatten();
    // Never chosen means following the desktop, the egui default.
    let theme = read::<ThemeChoice>(pool, keys::THEME).await?.unwrap_or_default();

    Ok(AppSettings {
        zoom: f64::from(read::<f32>(pool, keys::ZOOM_FACTOR).await?.unwrap_or(DEFAULT_ZOOM)),
        theme: theme_name(theme).to_owned(),
        wows_dir: read_or_default(pool, keys::WOWS_DIR).await?,
        locale,
        current_replay_path,
        replay: ReplayPreferences {
            show_raw_xp: replay.show_raw_xp,
            show_observed_damage: replay.show_observed_damage,
            show_entity_id: replay.show_entity_id,
            show_heals: replay.show_heals,
            enable_replay_previews: replay.enable_replay_previews,
            auto_export_data: replay.auto_export_data,
            auto_export_format: export_format_name(replay.auto_export_format).to_owned(),
            auto_export_path: replay.auto_export_path,
        },
        // The defaults below are the egui app's documented ones for a row that
        // was never written.
        debug_mode: read(pool, keys::DEBUG_MODE).await?.unwrap_or(false),
        check_for_updates: read(pool, keys::CHECK_FOR_UPDATES).await?.unwrap_or(true),
        enable_logging: read(pool, keys::ENABLE_LOGGING).await?.unwrap_or(true),
        data_sharing: sharing_name(data_sharing).to_owned(),
        proxy_url,
        auto_load_latest_replay: read(pool, keys::AUTO_LOAD_LATEST_REPLAY).await?.unwrap_or(true),
        output_dir: read_or_default(pool, keys::OUTPUT_DIR).await?,
        armor_defaults,
        auto_dump_game_data: read(pool, keys::AUTO_DUMP_GAME_DATA).await?.unwrap_or(false),
        game_data_cache_dir: read_or_default(pool, keys::GAME_DATA_CACHE_DIR).await?,
        game_data_repo_commit: read::<Option<String>>(pool, keys::GAME_DATA_REPO_COMMIT).await?.flatten(),
        collab_display_name: read_or_default(pool, keys::COLLAB_DISPLAY_NAME).await?,
        suppress_p2p_ip_warning: read(pool, keys::SUPPRESS_P2P_IP_WARNING).await?.unwrap_or(false),
        disable_auto_open_session_windows: read(pool, keys::DISABLE_AUTO_OPEN_SESSION_WINDOWS).await?.unwrap_or(false),
        build_consent_shown: read(pool, keys::BUILD_CONSENT_SHOWN).await?.unwrap_or(false),
        replay_consent_shown: read(pool, keys::REPLAY_CONSENT_SHOWN).await?.unwrap_or(false),
        language_selection_shown: read(pool, keys::LANGUAGE_SELECTION_SHOWN).await?.unwrap_or(false),
        suppress_gpu_encoder_warning: read(pool, keys::SUPPRESS_GPU_ENCODER_WARNING).await?.unwrap_or(false),
        constants_commit: read::<Option<String>>(pool, keys::CONSTANTS_FILE_COMMIT).await?.flatten(),
        twitch_user,
        twitch_channel: read_or_default(pool, twitch_keys::MONITORED_CHANNEL).await?,
        storage_dir: wows_toolkit_config::storage_dir().map(|dir| dir.display().to_string()).unwrap_or_default(),
        default_cache_dir: wows_toolkit_config::game_data_dump_base()
            .map(|dir| dir.display().to_string())
            .unwrap_or_default(),
    })
}

async fn read<T: DeserializeOwned>(pool: &SqlitePool, key: &'static str) -> Result<Option<T>, SettingsError> {
    wows_toolkit_config::queries::try_get_setting(pool, key).await.map_err(|source| SettingsError::Read { key, source })
}

/// A row whose absence means the type's empty value: an unset path is an empty
/// string, and an unwritten replay row is the replay defaults.
async fn read_or_default<T: DeserializeOwned + Default>(
    pool: &SqlitePool,
    key: &'static str,
) -> Result<T, SettingsError> {
    Ok(read(pool, key).await?.unwrap_or_default())
}

fn decode<T: DeserializeOwned>(
    key: &'static str,
    expected: &'static str,
    value: serde_json::Value,
) -> Result<T, SettingsError> {
    serde_json::from_value(value).map_err(|source| SettingsError::Value { key, expected, source })
}

fn json<T: Serialize + ?Sized>(key: &'static str, value: &T) -> Result<String, SettingsError> {
    serde_json::to_string(value).map_err(|source| SettingsError::Value { key, expected: "serializable value", source })
}

pub(crate) fn parse_theme(name: &str) -> Result<ThemeChoice, SettingsError> {
    ThemeChoice::ALL
        .into_iter()
        .find(|choice| theme_name(*choice) == name)
        .ok_or_else(|| SettingsError::UnknownTheme { value: name.to_owned() })
}

/// The stored name of `choice`, which is its variant name.
pub(crate) fn theme_name(choice: ThemeChoice) -> &'static str {
    match choice {
        ThemeChoice::System => "System",
        ThemeChoice::Dark => "Dark",
        ThemeChoice::Light => "Light",
        ThemeChoice::Dracula => "Dracula",
        ThemeChoice::Nord => "Nord",
        ThemeChoice::Monokai => "Monokai",
        ThemeChoice::SolarizedDark => "SolarizedDark",
        ThemeChoice::SolarizedLight => "SolarizedLight",
        ThemeChoice::GruvboxDark => "GruvboxDark",
        ThemeChoice::GruvboxLight => "GruvboxLight",
        ThemeChoice::CatppuccinMocha => "CatppuccinMocha",
        ThemeChoice::TokyoNight => "TokyoNight",
    }
}

pub(crate) fn parse_sharing(name: &str) -> Result<DataSharingMode, SettingsError> {
    DataSharingMode::ALL
        .into_iter()
        .find(|mode| sharing_name(*mode) == name)
        .ok_or_else(|| SettingsError::UnknownSharingMode { value: name.to_owned() })
}

pub(crate) fn sharing_name(mode: DataSharingMode) -> &'static str {
    match mode {
        DataSharingMode::Off => "off",
        DataSharingMode::BuildData => "build_data",
        DataSharingMode::Replays => "replays",
    }
}

fn parse_export_format(name: &str) -> Result<ReplayExportFormat, SettingsError> {
    [ReplayExportFormat::Json, ReplayExportFormat::Csv, ReplayExportFormat::Cbor]
        .into_iter()
        .find(|format| export_format_name(*format) == name)
        .ok_or_else(|| SettingsError::UnknownExportFormat { value: name.to_owned() })
}

fn export_format_name(format: ReplayExportFormat) -> &'static str {
    match format {
        ReplayExportFormat::Json => "json",
        ReplayExportFormat::Csv => "csv",
        ReplayExportFormat::Cbor => "cbor",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_theme_name_round_trips_as_the_stored_variant_name() {
        for choice in ThemeChoice::ALL {
            let stored = serde_json::to_string(&choice).expect("a theme serializes");
            assert_eq!(stored, format!("\"{}\"", theme_name(choice)));
            assert_eq!(parse_theme(theme_name(choice)).expect("the name parses"), choice);
        }
    }

    #[test]
    fn data_sharing_writes_the_compatibility_row_beside_the_mode() {
        let rows = SettingKey::DataSharing.encode(serde_json::json!("replays")).expect("a mode encodes");
        assert_eq!(rows, vec![(keys::DATA_SHARING_MODE, "\"replays\"".to_owned()), (keys::SEND_REPLAY_DATA, "true".to_owned())]);
    }

    #[test]
    fn a_value_of_the_wrong_type_is_refused() {
        assert!(SettingKey::CheckForUpdates.encode(serde_json::json!("yes")).is_err());
        assert!(SettingKey::Zoom.encode(serde_json::json!(9.0)).is_err());
        assert!(SettingKey::Locale.encode(serde_json::json!("xx")).is_err());
        assert!(SettingKey::Theme.encode(serde_json::json!("Neon")).is_err());
    }

    #[test]
    fn text_settings_are_stored_trimmed() {
        let rows = SettingKey::WowsDir.encode(serde_json::json!("  /games/wows  ")).expect("a path encodes");
        assert_eq!(rows, vec![(keys::WOWS_DIR, "\"/games/wows\"".to_owned())]);
    }
}
