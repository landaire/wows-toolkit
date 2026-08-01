use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::Sender;

use notify::EventKind;
use notify::RecommendedWatcher;
use notify::RecursiveMode;
use notify::Watcher;
use notify::event::ModifyKind;
use notify::event::RenameMode;
use parking_lot::Mutex;
use parking_lot::RwLock;
use rootcause::prelude::ResultExt;
use serde::Deserialize;
use serde::Serialize;
use tracing::debug;
use tracing::warn;
use wows_replays::ReplayFile;
use wows_replays::types::GameParamId;
use wowsunpack::vfs::VfsPath;

use crate::data::session_stats::PerGameStat;
use crate::data::session_stats::SessionStats;
use crate::data::settings::AppSettings;
use crate::data::wows_data::ReplayDependencies;
use crate::data::wows_data::ReplayLoader;
use crate::data::wows_data::SharedWoWsData;
use crate::data::wows_data::WoWsDataMap;
use crate::db::index::rows::WorkspaceId;
use crate::task::BackgroundParserThread;
use crate::task::BackgroundTask;
use crate::task::BackgroundTaskKind;
use crate::task::DataExportSettings;
use crate::task::NetworkJob;
use crate::task::ReplayBackgroundParserThreadMessage;
use crate::task::ReplaySource;
use crate::twitch::TwitchState;
use crate::ui::file_unpacker::ResourceBrowserState;
use crate::ui::file_unpacker::UnpackerProgress;
use crate::ui::mod_manager::ModInfo;
use crate::ui::mod_manager::ModManagerInfo;
use crate::ui::plaintext_viewer::PlaintextFileViewer;
use crate::ui::player_tracker::PlayerTrackerSubTab;
use crate::ui::replay_parser::Replay;
use crate::ui::replay_parser::ReplayWorkspace;
use crate::ui::replay_parser::SortOrder;
use crate::update_background_task;
use crate::util::personal_rating::PersonalRatingData;

pub type SharedToasts = Arc<parking_lot::Mutex<egui_notify::Toasts>>;

pub use wows_toolkit_config::WindowKind;
pub use wows_toolkit_config::WindowSettings;

/// egui-specific constructors/appliers for the persisted [`WindowSettings`].
///
/// These live here rather than on the type itself because `WindowSettings` is
/// defined in `wows-toolkit-config`, which has no egui dependency.
pub trait WindowSettingsEguiExt {
    fn from_viewport_info(info: &egui::ViewportInfo, zoom_compensation: Option<f32>) -> Self;
    fn apply_to_builder(&self, builder: egui::ViewportBuilder, default_size: [f32; 2]) -> egui::ViewportBuilder;
}

impl WindowSettingsEguiExt for WindowSettings {
    /// Capture current viewport state from [`egui::ViewportInfo`].
    ///
    /// Pass `Some(ctx.zoom_factor())` for the main window to compensate for
    /// eframe applying the zoom again on restore. Secondary/deferred viewports
    /// should pass `None` since they already report sizes at the correct scale.
    fn from_viewport_info(info: &egui::ViewportInfo, zoom_compensation: Option<f32>) -> Self {
        let zoom = zoom_compensation.unwrap_or(1.0);
        Self {
            inner_size_points: info.inner_rect.map(|r| [r.width() * zoom, r.height() * zoom]),
            outer_position_pixels: info.outer_rect.map(|r| [r.left(), r.top()]),
            fullscreen: info.fullscreen.unwrap_or(false),
            maximized: info.maximized.unwrap_or(false),
        }
    }

    /// Apply these settings to a [`egui::ViewportBuilder`], falling back to
    /// `default_size` when no stored size is available.
    fn apply_to_builder(&self, builder: egui::ViewportBuilder, default_size: [f32; 2]) -> egui::ViewportBuilder {
        let size = self.inner_size_points.unwrap_or(default_size);
        let mut builder = builder.with_inner_size(size);
        if let Some([x, y]) = self.outer_position_pixels {
            builder = builder.with_position(egui::pos2(x, y));
        }
        if self.fullscreen {
            builder.with_fullscreen(true)
        } else if self.maximized {
            builder.with_maximized(true)
        } else {
            builder
        }
    }
}

/// Tracks window settings for persistence.
#[derive(Default)]
pub struct WindowSettingsTracker {
    pub settings: HashMap<WindowKind, WindowSettings>,
}

pub type SharedWindowSettings = Arc<parking_lot::Mutex<WindowSettingsTracker>>;

// ---------------------------------------------------------------------------
// Persisted state — shared between UI thread and background save task
// ---------------------------------------------------------------------------

/// All state that gets persisted to SQLite, collected into a single struct
/// behind `Arc<RwLock<>>` so the background save task can read it without
/// blocking the UI thread.
pub struct PersistedState {
    pub settings: AppSettings,
    pub output_dir: String,
    pub auto_load_latest_replay: bool,
    pub mod_manager_info: ModManagerInfo,
    pub stats_dock_state: egui_dock::DockState<StatsSubTab>,
    pub player_tracker_dock_state: egui_dock::DockState<PlayerTrackerSubTab>,
    pub next_chart_tab_id: u64,
    pub chart_configs: HashMap<u64, SessionStatsChartConfig>,
    pub armor_viewer_defaults: crate::armor_viewer::state::ArmorViewerDefaults,
    pub session_stats: SessionStats,
}

impl Default for PersistedState {
    fn default() -> Self {
        Self {
            settings: Default::default(),
            output_dir: Default::default(),
            auto_load_latest_replay: true,
            mod_manager_info: Default::default(),
            stats_dock_state: default_stats_dock_state(),
            player_tracker_dock_state: default_player_tracker_dock_state(),
            next_chart_tab_id: 1,
            chart_configs: HashMap::new(),
            armor_viewer_defaults: Default::default(),
            session_stats: Default::default(),
        }
    }
}

/// A wrapper around `RwLock<PersistedState>` that tracks a generation counter.
/// Every call to `write()` increments the generation, allowing the save task
/// to detect changes without touching individual mutation sites.
pub struct TrackedPersistedState {
    inner: parking_lot::RwLock<PersistedState>,
    generation: std::sync::atomic::AtomicU64,
}

impl TrackedPersistedState {
    pub fn new(state: PersistedState) -> Self {
        Self { inner: parking_lot::RwLock::new(state), generation: std::sync::atomic::AtomicU64::new(0) }
    }

    pub fn read(&self) -> parking_lot::RwLockReadGuard<'_, PersistedState> {
        self.inner.read()
    }

    /// Acquire a write guard. Automatically increments the generation counter,
    /// so any write is treated as a potential change.
    pub fn write(&self) -> parking_lot::RwLockWriteGuard<'_, PersistedState> {
        self.generation.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.inner.write()
    }

    /// Acquire a write guard without marking the state dirty, for a mutation the
    /// caller knows leaves the persisted content as it found it: a tab moving a
    /// field out for the duration of a frame and putting it back, say.
    ///
    /// This defers a save rather than skipping one. The background save task in
    /// `db::save` re-serializes the whole persisted state on an unconditional
    /// five-second timer, so a change made under this guard reaches SQLite
    /// within a few seconds either way. Take [`Self::write`] when a change
    /// should be saved promptly rather than eventually, and take this one when
    /// the change is worth nothing to persist and the extra save is worth
    /// avoiding.
    pub fn write_untracked(&self) -> parking_lot::RwLockWriteGuard<'_, PersistedState> {
        self.inner.write()
    }

    /// Current generation counter. Compared by the app each frame to detect
    /// whether a save is needed.
    pub fn generation(&self) -> u64 {
        self.generation.load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl Default for TrackedPersistedState {
    fn default() -> Self {
        Self::new(PersistedState::default())
    }
}

pub type SharedPersistedState = Arc<TrackedPersistedState>;

/// A structural summary of a dock layout: which tabs sit in which node, which
/// of them is showing, and how the splits divide.
///
/// A tab that moves its dock state out of the persisted state for a frame and
/// puts it back compares this before and after, so an untouched layout is put
/// back without marking the state dirty. Excludes the rects egui_dock recomputes
/// on every show, which follow the window rather than the layout, and the
/// focused node, which follows the pointer.
pub fn dock_layout_fingerprint<Tab: std::hash::Hash>(dock: &egui_dock::DockState<Tab>) -> u64 {
    use std::hash::Hash;
    use std::hash::Hasher;

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for (path, node) in dock.iter_all_nodes() {
        (path.surface.0, path.node.0).hash(&mut hasher);
        match node {
            egui_dock::Node::Empty => 0u8.hash(&mut hasher),
            egui_dock::Node::Leaf(leaf) => {
                1u8.hash(&mut hasher);
                leaf.tabs.hash(&mut hasher);
                leaf.active.0.hash(&mut hasher);
                leaf.collapsed.hash(&mut hasher);
            }
            egui_dock::Node::Vertical(split) => {
                2u8.hash(&mut hasher);
                split.fraction.to_bits().hash(&mut hasher);
            }
            egui_dock::Node::Horizontal(split) => {
                3u8.hash(&mut hasher);
                split.fraction.to_bits().hash(&mut hasher);
            }
        }
    }
    hasher.finish()
}

/// Sub-tab selection for the Stats tab
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum StatsSubTab {
    Overview,
    Charts(u64),
}

/// Available statistics for charting
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ChartableStat {
    #[default]
    Damage,
    SpottingDamage,
    Frags,
    RawXp,
    BaseXp,
    WinRate,
    PersonalRating,
}

impl ChartableStat {
    pub fn name(&self) -> String {
        use rust_i18n::t;
        match self {
            ChartableStat::Damage => t!("stat.damage"),
            ChartableStat::SpottingDamage => t!("stat.spotting_damage"),
            ChartableStat::Frags => t!("stat.frags"),
            ChartableStat::RawXp => t!("stat.raw_xp"),
            ChartableStat::BaseXp => t!("stat.base_xp"),
            ChartableStat::WinRate => t!("stat.win_rate"),
            ChartableStat::PersonalRating => t!("stat.personal_rating"),
        }
        .into()
    }

    pub fn all() -> &'static [ChartableStat] {
        &[
            ChartableStat::BaseXp,
            ChartableStat::Damage,
            ChartableStat::Frags,
            ChartableStat::PersonalRating,
            ChartableStat::RawXp,
            ChartableStat::SpottingDamage,
            ChartableStat::WinRate,
        ]
    }
}

/// Chart display mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ChartMode {
    /// Line chart showing stat over each game played
    #[default]
    Line,
    /// Bar chart showing average stat comparison between ships
    Bar,
}

/// Deserialize `selected_ships` from either `Vec<GameParamId>` (new format) or
/// `Vec<String>` (old format).  Old string-based selections cannot be mapped back
/// to IDs, so they are silently dropped — the user simply re-selects ships.
fn deserialize_selected_ships<'de, D>(deserializer: D) -> Result<Vec<GameParamId>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;

    struct ShipVisitor;

    impl<'de> de::Visitor<'de> for ShipVisitor {
        type Value = Vec<GameParamId>;

        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a sequence of ship IDs (u64) or ship names (string)")
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: de::SeqAccess<'de>,
        {
            let mut ids = Vec::new();
            // Try each element — keep u64 values, skip strings.
            while let Some(value) = seq.next_element::<serde_json::Value>()? {
                if let Some(n) = value.as_u64() {
                    ids.push(GameParamId::from(n));
                }
                // Old string entries are silently dropped
            }
            Ok(ids)
        }
    }

    deserializer.deserialize_seq(ShipVisitor)
}

/// Configuration for the session stats chart (one per Charts tab)
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct SessionStatsChartConfig {
    /// Selected stat to display
    pub selected_stat: ChartableStat,
    /// Chart display mode (line or bar)
    pub mode: ChartMode,
    /// Selected ships to show (empty = all ships).
    /// Uses a custom deserializer so that old configs with `Vec<String>` ship names
    /// gracefully degrade to an empty selection instead of failing entirely.
    #[serde(default, deserialize_with = "deserialize_selected_ships")]
    pub selected_ships: Vec<GameParamId>,
    pub selected_ships_manually_changed: bool,
    /// Whether to show rolling average instead of per-game values (line chart only)
    pub rolling_average: bool,
    /// Whether to combine all ships into a single rolling series
    #[serde(default)]
    pub combined: bool,
    /// Whether to show value labels on data points
    pub show_labels: bool,
    /// Whether a screenshot has been requested (waiting for the event)
    #[serde(skip)]
    pub screenshot_requested: bool,
    /// The plot rectangle from the last frame (used to crop the screenshot)
    #[serde(skip)]
    pub plot_rect: Option<egui::Rect>,
    /// Whether the plot should be reset (e.g. after stat/mode change)
    #[serde(skip)]
    pub reset_plot: bool,
}

/// Default stats dock: Overview on the left, Charts(0) on the right, 50/50 split.
pub(crate) fn default_stats_dock_state() -> egui_dock::DockState<StatsSubTab> {
    let mut dock = egui_dock::DockState::new(vec![StatsSubTab::Overview]);
    dock.split(
        egui_dock::NodePath::MAIN_ROOT,
        egui_dock::Split::Right,
        0.5,
        egui_dock::Node::leaf(StatsSubTab::Charts(0)),
    );
    dock
}

/// Default player-tracker dock: every sub-tab in one leaf. Historical is first,
/// which makes it the active tab on a fresh install.
pub(crate) fn default_player_tracker_dock_state() -> egui_dock::DockState<PlayerTrackerSubTab> {
    let mut dock = egui_dock::DockState::new(vec![
        PlayerTrackerSubTab::Historical,
        PlayerTrackerSubTab::Clans,
        PlayerTrackerSubTab::CurrentMatch,
    ]);
    // `DockState::new` leaves nothing focused; focus the only leaf so
    // `find_active_focused` resolves without waiting for a first render.
    dock.set_focused_node_and_surface(egui_dock::NodePath::MAIN_ROOT);
    dock
}

/// File system events for replay monitoring
#[derive(Debug)]
pub enum NotifyFileEvent {
    Added(PathBuf),
    Modified(PathBuf),
    Removed(PathBuf),
    PreferencesChanged,
    TempArenaInfoCreated(PathBuf),
}

/// An action that requires user confirmation before executing.
#[derive(Clone)]
pub enum ConfirmableAction {
    /// Launch WorldOfWarships.exe with the given replay path.
    OpenInGame { replay_path: PathBuf },
    /// Clear all session stats.
    ClearSessionStats,
    /// Clear session stats for a specific ship.
    ClearShipSessionStats { ship_id: GameParamId },
    /// Replace session stats with the given replays.
    SetAsSessionStats { replays: Vec<std::sync::Weak<RwLock<Replay>>> },
}

impl ConfirmableAction {
    pub fn confirmation_message(&self) -> String {
        use rust_i18n::t;
        match self {
            ConfirmableAction::OpenInGame { .. } => t!("confirm.open_in_game"),
            ConfirmableAction::ClearSessionStats => t!("confirm.clear_all_session_stats"),
            ConfirmableAction::ClearShipSessionStats { .. } => t!("confirm.clear_ship_session_stats"),
            ConfirmableAction::SetAsSessionStats { .. } => t!("confirm.set_as_session_stats"),
        }
        .into()
    }
}

/// Real disk usage and version count for the game-data cache directory,
/// cached so the Settings tab does not re-walk the directory every frame.
#[derive(Debug, Clone, Copy)]
pub struct GameDataCacheStats {
    /// Total size of regular files (symlinks excluded), in bytes.
    pub total_bytes: u64,
    /// Number of build versions (directories containing a `metadata.toml`).
    pub version_count: usize,
}

/// Main application state container.
///
/// Persisted state lives in `self.persisted` (shared with background save task).
/// Data stores (`player_tracker`, `sent_replays`) are separate `Arc<RwLock<>>`
/// fields for independent concurrent access.
pub struct TabState {
    // ─── Shared persisted state ──────────────────────────────────────────
    pub persisted: SharedPersistedState,

    // ─── Data stores (separate from settings, independently locked) ──────
    pub player_tracker: Arc<RwLock<crate::ui::player_tracker::PlayerTracker>>,
    pub sent_replays: Arc<RwLock<std::collections::HashSet<String>>>,
    pub replay_sort: Arc<Mutex<SortOrder>>,

    // ─── Transient / runtime-only state ──────────────────────────────────
    pub world_of_warships_data: Option<SharedWoWsData>,
    pub items_to_extract: Mutex<Vec<VfsPath>>,
    #[allow(dead_code)]
    pub translations: Option<gettext::Catalog>,
    pub unpacker_progress: Option<mpsc::Receiver<UnpackerProgress>>,
    pub last_progress: Option<UnpackerProgress>,
    pub search_tab: crate::ui::search_tab::SearchTabState,
    /// Command palette state (Ctrl+K / Ctrl+P): cascade mode plus on-demand,
    /// bounded sub-search results.
    pub command_palette: crate::ui::command_palette::CommandPalette,
    /// When set, the Search tab adopts this query on next show (from palette/tracker).
    pub pending_search_query: Option<crate::db::index::query_model::Query>,
    /// When true, the app focuses the Search tab next frame (from palette/tracker).
    pub pending_focus_search: bool,
    /// Cached ship catalog for palette ship entries; built lazily on first palette open.
    pub ship_catalog: Option<crate::armor_viewer::ship_selector::ShipCatalog>,
    pub file_viewer: Mutex<Vec<PlaintextFileViewer>>,
    pub replay_renderers: Mutex<Vec<crate::replay::renderer::ReplayRendererViewer>>,
    pub renderer_asset_cache: Arc<parking_lot::Mutex<crate::replay::renderer::RendererAssetCache>>,
    pub tactics_boards: Mutex<Vec<crate::replay::minimap_view::tactics::TacticsBoardViewer>>,
    /// Board IDs we've already auto-opened (prevents re-open after user closes them).
    pub tactics_auto_opened_board_ids: std::collections::HashSet<u64>,
    /// Shared tokio runtime for collab sessions and async tasks.
    pub tokio_runtime: Option<Arc<tokio::runtime::Runtime>>,
    /// SQLite connection pool for persistence.
    pub db_pool: Option<sqlx::SqlitePool>,
    pub window_settings: SharedWindowSettings,
    pub file_watcher: Option<RecommendedWatcher>,
    pub file_receiver: Option<mpsc::Receiver<NotifyFileEvent>>,
    pub background_tasks: Vec<BackgroundTask>,
    pub toasts: SharedToasts,
    pub can_change_wows_dir: bool,
    /// The live workspace always exists, so it is a field rather than a map
    /// entry: "at least one workspace exists" is a property of the type.
    pub live_workspace: ReplayWorkspace,
    /// Additional workspaces opened this session, keyed by runtime handle.
    /// `WorkspaceId`s are handed out monotonically, so iterating the map in
    /// key order is also the order the workspaces were opened -- which a
    /// later phase needs to restore tabs.
    pub workspaces: std::collections::BTreeMap<WorkspaceId, ReplayWorkspace>,
    /// The next id [`Self::open_directory_workspace`] will hand out. Starts at
    /// one because zero is [`WorkspaceId::LIVE`], and only ever increases, so a
    /// closed workspace's id is never reused by a later one.
    next_workspace_id: u64,
    /// Which workspace the replay inspector is currently showing. Private so
    /// it can only be set through [`Self::set_active_workspace`], which
    /// refuses to point it at a workspace that is not open.
    active_workspace_id: WorkspaceId,
    pub twitch_update_sender: Option<tokio::sync::mpsc::Sender<crate::twitch::TwitchUpdate>>,
    pub twitch_state: Arc<RwLock<TwitchState>>,
    pub markdown_cache: egui_commonmark::CommonMarkCache,
    pub mod_action_sender: Sender<ModInfo>,
    /// Used temporarily to store the mod action receiver until the mod manager thread is started.
    /// Consumed via `.take()` in `app.rs` — clippy false positive for "never read".
    #[allow(dead_code)]
    pub mod_action_receiver: Option<Receiver<ModInfo>>,
    pub background_task_receiver: Receiver<BackgroundTask>,
    pub background_task_sender: Sender<BackgroundTask>,
    pub background_parser_tx: Option<Sender<ReplayBackgroundParserThreadMessage>>,
    pub parser_lock: Arc<parking_lot::Mutex<()>>,
    pub personal_rating_data: Arc<RwLock<PersonalRatingData>>,
    /// Replays selected for session stats update. When Some, they will be
    /// processed and added to session stats. If `clear_before_session_reset` is true,
    /// existing stats are cleared first.
    /// Uses Weak references to avoid retaining stale replays if they're removed from the listing.
    pub replays_for_session_reset: Option<Vec<std::sync::Weak<RwLock<Replay>>>>,
    pub clear_before_session_reset: bool,
    /// Pending action awaiting user confirmation.
    pub pending_confirmation: Option<ConfirmableAction>,
    /// All loaded version data, keyed by build number.
    pub wows_data_map: Option<WoWsDataMap>,
    /// All build numbers available in the game's bin/ directory.
    pub available_builds: Vec<u32>,
    /// Currently selected build in the Resource Browser.
    pub selected_browser_build: u32,
    /// Explorer-style resource browser state (selected dir, filter, queue popover).
    pub browser_state: ResourceBrowserState,
    /// Shared flag for "suppress GPU encoder warning" — synced from Settings on startup.
    pub suppress_gpu_encoder_warning: Arc<std::sync::atomic::AtomicBool>,
    /// Sender for submitting jobs to the background networking thread.
    pub network_job_tx: Option<Sender<NetworkJob>>,
    /// Whether the Settings tab needs attention (e.g. invalid WoWs directory, invalid twitch token).
    pub settings_needs_attention: bool,
    /// The resolved dark/light theme, refreshed from `Context::theme()` each
    /// frame before the dock area is built. `TabViewer::tab_style_override`
    /// has no `Context` of its own, so it reads this instead.
    pub active_theme: egui::Theme,
    /// Cached builds found to have newer data upstream by the last update check.
    pub game_data_updates: Vec<wows_data_mgr::download_repo::BuildUpdateStatus>,
    /// Whether a game data update check is currently running.
    pub checking_game_data_updates: bool,
    /// Cached builds the last validation found missing, corrupt, or stale.
    pub game_data_repair: Vec<wows_data_mgr::download_repo::BuildUpdateStatus>,
    /// Whether a game data cache validation is currently running.
    pub validating_game_data_cache: bool,
    /// Cached real disk usage and version count for the game-data cache
    /// directory. Computed lazily while the Settings tab is shown and reused
    /// until another tab is shown, the cache dir changes, or a cache operation
    /// runs. Never recomputed per frame (the size walk is expensive).
    pub game_data_cache_stats: Option<GameDataCacheStats>,
    /// Cached result of WoWs directory validation. Updated by `revalidate_wows_dir()`
    /// on startup and whenever `settings.wows_dir` changes — NOT every frame.
    pub wows_dir_invalid: bool,
    /// wgpu render state for 3D viewport rendering (captured at app init).
    pub wgpu_render_state: Option<eframe::egui_wgpu::RenderState>,
    /// State for the Armor Viewer tab.
    pub armor_viewer: crate::armor_viewer::ArmorViewerState,
    /// Whether the standalone replay controls reference window is open.
    pub show_replay_controls: bool,
    /// Cached parsed replay/spectator keybindings from `commands.scheme.xml`.
    pub replay_controls_cache: Option<Vec<crate::util::controls::CommandGroup>>,

    // ─── Collaborative session ─────────────────────────────────────────────
    /// Session token text input for joining.
    pub join_session_token: String,
    /// Whether the IP disclosure warning dialog is showing.
    pub show_ip_warning: bool,
    /// Set by the session popover to trigger `do_join_session()` in the app update loop.
    pub pending_join: bool,
    /// Set by the session popover to trigger `do_host_session()` in the app update loop.
    pub pending_host: bool,
    /// Active client session handle (when joined as a peer).
    pub client_session: Option<crate::collab::peer::PeerSessionHandle>,
    /// Active host session handle.
    pub host_session: Option<crate::collab::peer::PeerSessionHandle>,
    /// Shared asset bundle reference (host only). The UI thread can lazily populate
    /// this once game data is loaded, and the host task reads it on `RequestAssets`.
    pub web_asset_bundle: Option<Arc<Mutex<Option<Vec<u8>>>>>,
    /// Shared session state for both host and client sessions.
    pub session_state: Arc<Mutex<crate::collab::SessionState>>,
    /// Whether the session token is visible (unmasked) in the popover.
    pub session_token_visible: bool,
    /// Show red error on the display name field (cleared on next edit).
    pub show_display_name_error: bool,
    /// Counter for assigning unique replay IDs to host renderers.
    pub next_replay_id: u64,
    /// Rolling timestamps of ReplayOpened events for spam protection (client-side).
    pub replay_open_timestamps: std::collections::VecDeque<std::time::Instant>,

    // ─── Tactics Board ────────────────────────────────────────────────────
    /// Local cache of cap layouts extracted from replays. Persisted to disk
    /// via rkyv, loaded on startup, and updated incrementally when new
    /// `(mapId, scenarioConfigId)` combinations are encountered.
    pub cap_layout_db: Arc<Mutex<crate::data::cap_layout::CapLayoutDb>>,

    /// Active viewport IDs for secondary windows, updated each frame.
    /// The background save task reads this to capture window geometry.
    pub active_viewports: Arc<parking_lot::Mutex<Vec<(WindowKind, egui::ViewportId)>>>,

    /// Notify handle to wake the background save task when settings change.
    pub save_notify: Arc<tokio::sync::Notify>,
}

impl Default for TabState {
    fn default() -> Self {
        let (mod_action_sender, mod_action_receiver) = mpsc::channel();
        let (background_task_sender, background_task_receiver) = mpsc::channel();
        Self {
            persisted: Arc::new(TrackedPersistedState::default()),
            player_tracker: Default::default(),
            sent_replays: Default::default(),
            replay_sort: Arc::new(Mutex::new(SortOrder::default())),
            world_of_warships_data: None,
            items_to_extract: Default::default(),
            translations: Default::default(),
            unpacker_progress: Default::default(),
            last_progress: Default::default(),
            search_tab: Default::default(),
            command_palette: Default::default(),
            pending_search_query: None,
            pending_focus_search: false,
            ship_catalog: None,
            file_viewer: Default::default(),
            replay_renderers: Default::default(),
            renderer_asset_cache: Default::default(),
            file_watcher: None,
            file_receiver: None,
            background_tasks: Vec::new(),
            can_change_wows_dir: true,
            toasts: Arc::new(parking_lot::Mutex::new(egui_notify::Toasts::default())),
            live_workspace: ReplayWorkspace::new(None),
            workspaces: std::collections::BTreeMap::new(),
            next_workspace_id: 1,
            active_workspace_id: WorkspaceId::LIVE,
            twitch_update_sender: Default::default(),
            twitch_state: Default::default(),
            markdown_cache: Default::default(),
            mod_action_sender,
            mod_action_receiver: Some(mod_action_receiver),
            background_task_receiver,
            background_task_sender,
            background_parser_tx: None,
            parser_lock: Arc::new(parking_lot::Mutex::new(())),
            personal_rating_data: Arc::new(RwLock::new(PersonalRatingData::new())),
            replays_for_session_reset: None,
            clear_before_session_reset: true,
            pending_confirmation: None,
            wows_data_map: None,
            available_builds: Vec::new(),
            selected_browser_build: 0,
            browser_state: Default::default(),
            suppress_gpu_encoder_warning: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            network_job_tx: None,
            settings_needs_attention: false,
            active_theme: egui::Theme::Dark,
            game_data_updates: Vec::new(),
            checking_game_data_updates: false,
            game_data_repair: Vec::new(),
            validating_game_data_cache: false,
            game_data_cache_stats: None,
            wows_dir_invalid: false,
            wgpu_render_state: None,
            armor_viewer: Default::default(),
            show_replay_controls: false,
            replay_controls_cache: None,
            tokio_runtime: None,
            join_session_token: String::new(),
            show_ip_warning: false,
            pending_join: false,
            pending_host: false,
            client_session: None,
            host_session: None,
            web_asset_bundle: None,
            session_state: Arc::new(Mutex::new(crate::collab::SessionState::default())),
            session_token_visible: false,
            show_display_name_error: false,
            next_replay_id: 1,
            replay_open_timestamps: std::collections::VecDeque::new(),
            cap_layout_db: Default::default(),
            tactics_boards: Default::default(),
            tactics_auto_opened_board_ids: Default::default(),
            db_pool: None,
            window_settings: Default::default(),
            active_viewports: Arc::new(parking_lot::Mutex::new(Vec::new())),
            save_notify: Arc::new(tokio::sync::Notify::new()),
        }
    }
}

impl TabState {
    /// Notify the background save task that state has changed and should be
    /// persisted. The save task debounces rapid calls (1 second).
    pub fn request_save(&self) {
        self.save_notify.notify_one();
    }

    /// Switch the inspector to `id`. A workspace that is not open resolves to
    /// the live workspace rather than leaving the UI pointing at nothing.
    pub fn set_active_workspace(&mut self, id: WorkspaceId) {
        self.active_workspace_id =
            if id == WorkspaceId::LIVE || self.workspaces.contains_key(&id) { id } else { WorkspaceId::LIVE };
    }

    pub fn active_workspace_id(&self) -> WorkspaceId {
        self.active_workspace_id
    }

    /// Opens `root` as a replay workspace and returns the id of the workspace
    /// that lists it.
    ///
    /// A root that is already open resolves to the workspace already listing
    /// it rather than a second one, so the caller focuses the tab that is
    /// already there instead of stacking duplicates. The live workspace is
    /// never a candidate: its root is the game's own replays directory, which
    /// has its own permanent tab.
    pub fn open_directory_workspace(&mut self, root: PathBuf) -> WorkspaceId {
        if let Some((&id, _)) =
            self.workspaces.iter().find(|(_, workspace)| workspace.root.as_deref() == Some(root.as_path()))
        {
            return id;
        }
        let id = WorkspaceId(self.next_workspace_id);
        self.next_workspace_id += 1;
        self.workspaces.insert(id, ReplayWorkspace::new(Some(root)));
        id
    }

    /// Closes a non-live workspace: drops it from `workspaces`, and if it was
    /// the active one, returns the inspector to the live workspace. Removing
    /// an id that is not open is a no-op, so this is safe to call twice for
    /// the same id -- the tab-close path egui_dock drives this from does
    /// exactly that.
    pub fn close_workspace(&mut self, id: WorkspaceId) {
        self.workspaces.remove(&id);
        if self.active_workspace_id == id {
            self.set_active_workspace(WorkspaceId::LIVE);
        }
    }

    /// Looks up an open workspace by id. `WorkspaceId::LIVE` always resolves
    /// to `live_workspace`, since it is not stored in `workspaces`.
    pub fn workspace(&self, id: WorkspaceId) -> Option<&ReplayWorkspace> {
        if id == WorkspaceId::LIVE { Some(&self.live_workspace) } else { self.workspaces.get(&id) }
    }

    /// Looks up an open workspace by id, mutably. `WorkspaceId::LIVE` always
    /// resolves to `live_workspace`, since it is not stored in `workspaces`.
    pub fn workspace_mut(&mut self, id: WorkspaceId) -> Option<&mut ReplayWorkspace> {
        if id == WorkspaceId::LIVE { Some(&mut self.live_workspace) } else { self.workspaces.get_mut(&id) }
    }

    /// The workspace a `Tab::Replays(id)` tab should draw. Resolves strictly by
    /// the id the tab itself carries, with no fallback: unlike
    /// [`Self::active_workspace`], a closed workspace resolves to `None`
    /// rather than `live_workspace`, since drawing a different workspace
    /// under this tab's own title would misrepresent that workspace's data.
    pub fn workspace_for_tab(&self, id: WorkspaceId) -> Option<&ReplayWorkspace> {
        self.workspace(id)
    }

    /// The workspace the replay inspector is currently showing. Delegates to
    /// [`Self::workspace`] so both share one resolution rule, falling back to
    /// `live_workspace` when `active_workspace_id` names an id that is neither
    /// `LIVE` nor present in `workspaces`.
    pub fn active_workspace(&self) -> &ReplayWorkspace {
        self.workspace(self.active_workspace_id).unwrap_or(&self.live_workspace)
    }

    /// Mutable form of [`Self::active_workspace`]. Resolves identically and
    /// never inserts into `workspaces`.
    pub fn active_workspace_mut(&mut self) -> &mut ReplayWorkspace {
        let id = self.active_workspace_id;
        if id == WorkspaceId::LIVE {
            return &mut self.live_workspace;
        }
        match self.workspaces.get_mut(&id) {
            Some(workspace) => workspace,
            None => &mut self.live_workspace,
        }
    }

    /// Returns the replay shown in the currently focused (or first) replay dock tab, if any.
    pub fn focused_replay(&self) -> Option<Arc<RwLock<Replay>>> {
        self.active_workspace().focused_replay()
    }

    /// Replace the focused tab's replay, or open a new tab if none exists.
    pub fn open_replay_in_focused_tab(&mut self, replay: Arc<RwLock<Replay>>) {
        self.active_workspace_mut().open_replay_in_focused_tab(replay);
    }

    /// Returns the shared dependencies needed for loading replays, if wows_data is available.
    pub fn replay_dependencies(&self) -> Option<ReplayDependencies> {
        let wows_data_map = self.wows_data_map.as_ref()?;
        Some(ReplayDependencies {
            wows_data_map: wows_data_map.clone(),
            twitch_state: Arc::clone(&self.twitch_state),
            replay_sort: Arc::clone(&self.replay_sort),
            background_task_sender: self.background_task_sender.clone(),
            is_debug_mode: self.persisted.read().settings.app.debug_mode,
            personal_rating_data: Arc::clone(&self.personal_rating_data),
        })
    }

    /// Send a job to the background networking thread.
    pub fn send_network_job(&self, job: NetworkJob) {
        if let Some(tx) = &self.network_job_tx {
            let _ = tx.send(job);
        }
    }

    /// Start a background check for updates to cached builds. No-op if a check
    /// is already running or no cache directory is configured.
    pub fn check_game_data_updates(&mut self) {
        if self.checking_game_data_updates {
            return;
        }
        let cache_dir = self.persisted.read().settings.game.game_data_cache_dir.clone();
        let Some(output_base) = crate::task::replays::game_data_dump_base_with_override(&cache_dir) else {
            return;
        };
        let known_tip = self.persisted.read().settings.game.game_data_repo_commit.clone();
        self.checking_game_data_updates = true;
        update_background_task!(
            self.background_tasks,
            Some(crate::task::start_game_data_update_check_task(output_base, known_tip))
        );
    }

    /// Re-download every cached build the last check flagged as out of date.
    pub fn update_all_game_data(&mut self) {
        let cache_dir = self.persisted.read().settings.game.game_data_cache_dir.clone();
        let Some(output_base) = crate::task::replays::game_data_dump_base_with_override(&cache_dir) else {
            return;
        };
        for update in std::mem::take(&mut self.game_data_updates) {
            update_background_task!(
                self.background_tasks,
                Some(crate::task::start_game_data_download_task(
                    output_base.clone(),
                    update.build,
                    Some(update.version),
                    true,
                    None,
                ))
            );
        }
    }

    /// Validate the cache against the remote repo. No-op if a validation is
    /// already running or no cache directory is configured.
    pub fn validate_game_data_cache(&mut self) {
        if self.validating_game_data_cache {
            return;
        }
        let cache_dir = self.persisted.read().settings.game.game_data_cache_dir.clone();
        let Some(output_base) = crate::task::replays::game_data_dump_base_with_override(&cache_dir) else {
            return;
        };
        self.validating_game_data_cache = true;
        update_background_task!(self.background_tasks, Some(crate::task::start_game_data_validation_task(output_base)));
    }

    /// Re-download every cached build the last validation flagged for repair.
    pub fn repair_game_data_cache(&mut self) {
        let cache_dir = self.persisted.read().settings.game.game_data_cache_dir.clone();
        let Some(output_base) = crate::task::replays::game_data_dump_base_with_override(&cache_dir) else {
            return;
        };
        for build in std::mem::take(&mut self.game_data_repair) {
            update_background_task!(
                self.background_tasks,
                Some(crate::task::start_game_data_download_task(
                    output_base.clone(),
                    build.build,
                    Some(build.version),
                    true,
                    None,
                ))
            );
        }
    }

    pub(crate) fn send_replay_consent_changed(&self) {
        let mode = self.persisted.read().settings.integrations.data_sharing_mode;
        let _ = self
            .background_parser_tx
            .as_ref()
            .map(|tx| tx.send(ReplayBackgroundParserThreadMessage::DataSharingModeChanged(mode)));
    }

    pub(crate) fn try_update_replays(&mut self) {
        // Sometimes we parse the replay too early. Let's try to parse it a couple times
        let parser_lock_arc = Arc::clone(&self.parser_lock);
        let parser_lock = parser_lock_arc.try_lock();
        if parser_lock.is_none() {
            // don't make the UI hang
            return;
        }

        let events: Vec<_> = self
            .file_receiver
            .as_ref()
            .map(|file| std::iter::from_fn(|| file.try_recv().ok()).collect())
            .unwrap_or_default();

        for file_event in events {
            match file_event {
                NotifyFileEvent::Added(new_file) => {
                    let source = if self.persisted.read().auto_load_latest_replay {
                        ReplaySource::AutoLoad
                    } else {
                        ReplaySource::SessionStatsOnly
                    };

                    // The game may still be flushing the freshly written replay
                    // when the watcher fires. Read, retry, and build on the
                    // background thread so the UI never stalls; the loaded replay
                    // is inserted into the listing when the task completes.
                    if let Some(deps) = self.replay_dependencies() {
                        update_background_task!(self.background_tasks, deps.parse_replay_from_path(new_file, source));
                    }
                }
                NotifyFileEvent::Modified(modified_file) => {
                    let source = if self.persisted.read().auto_load_latest_replay {
                        ReplaySource::AutoLoad
                    } else {
                        ReplaySource::SessionStatsOnly
                    };

                    // The watcher only observes the live replays directory, so a
                    // change it reports always belongs to the live workspace.
                    let replay_clone = self
                        .live_workspace
                        .replay_files
                        .as_ref()
                        .and_then(|files| files.get(&modified_file))
                        .map(Arc::clone);

                    if let Some(replay) = replay_clone {
                        // Invalidate cached data so the reload re-parses the file.
                        let mut replay_inner = replay.write();
                        replay_inner.battle_report = None;
                        replay_inner.ui_report = None;
                        drop(replay_inner);

                        if let Some(deps) = self.replay_dependencies() {
                            update_background_task!(self.background_tasks, deps.load_replay(replay, source));
                        }
                    } else if let Some(deps) = self.replay_dependencies() {
                        // The Added task for this replay may still be parsing, so
                        // the listing entry does not exist yet. Read from the path
                        // so the modification is not dropped; the freshest read
                        // wins on completion.
                        update_background_task!(
                            self.background_tasks,
                            deps.parse_replay_from_path(modified_file, source)
                        );
                    }
                }
                NotifyFileEvent::Removed(old_file) => {
                    // The watcher only observes the live replays directory, so a
                    // removal it reports always belongs to the live workspace.
                    if let Some(replay_files) = &mut self.live_workspace.replay_files {
                        replay_files.remove(&old_file);
                    }
                }
                NotifyFileEvent::PreferencesChanged => {
                    // debug!("Preferences file changed -- reloading game data");
                    // self.background_task = Some(self.load_game_data(self.settings.wows_dir.clone().into()));
                }
                NotifyFileEvent::TempArenaInfoCreated(path) => {
                    let parsed = std::fs::read(&path)
                        .context("failed to read tempArenaInfo.json")
                        .and_then(|meta_data| {
                            ReplayFile::from_decrypted_parts(meta_data, Vec::new())
                                .context("failed to parse tempArenaInfo.json")
                        })
                        .attach_with(|| format!("path: {}", path.display()));

                    match parsed {
                        Ok(replay_file) => {
                            self.player_tracker.write().update_from_live_arena_info(&replay_file.meta);
                        }
                        // The game writes this file at match start and may not
                        // have finished flushing it yet; skip this event and let
                        // a later write re-trigger the update.
                        Err(e) => warn!("live arena info update skipped: {e:?}"),
                    }
                }
            }
        }
    }

    /// Add one slice of a running directory walk to the listing it names.
    ///
    /// Batches are merged, never assigned: every batch after the first lands on
    /// a listing that already holds the replays before it. A batch whose
    /// workspace has closed is dropped, which is the whole reason the batch
    /// carries its own id rather than being applied to whatever is active.
    pub fn apply_ingest_batch(&mut self, batch: crate::task::replays::IngestBatch) {
        let Some(workspace) = self.workspace_mut(batch.workspace) else {
            return;
        };

        workspace.replay_files.get_or_insert_with(HashMap::new).extend(batch.replays);
        workspace.ingest_progress = Some(batch.progress);

        if workspace.source != Some(batch.source) {
            workspace.source = Some(batch.source);
            // Summaries already loaded were read against a different source.
            // Drop the stamp so the next frame reads them against this one.
            workspace.replay_row_summaries_generation = None;
        }
    }

    pub(crate) fn prevent_changing_wows_dir(&mut self) {
        self.can_change_wows_dir = false;
    }

    pub(crate) fn allow_changing_wows_dir(&mut self) {
        self.can_change_wows_dir = true;
    }

    /// Remove the chart config for a closed tab.
    pub fn remove_chart_config(&self, id: u64) {
        self.persisted.write().chart_configs.remove(&id);
    }

    /// Point the app at a different game data cache. The live data map reads
    /// the directory it was constructed with and keeps it until the app is
    /// restarted, so this only moves where new work looks.
    pub(crate) fn set_game_data_cache_dir(&mut self, dir: String) {
        self.persisted.write().settings.game.game_data_cache_dir = dir;
        self.game_data_cache_stats = None;
        crate::ui::replay_parser::clear_fire_section_failures();
    }

    /// Clears all game-related state. Called when the WoWs directory changes
    /// to ensure no stale data from the previous directory persists.
    pub(crate) fn reset_game_state(&mut self) {
        self.live_workspace.reset();
        for workspace in self.workspaces.values_mut() {
            workspace.reset();
        }
        self.browser_state = Default::default();
        {
            let mut p = self.persisted.write();
            p.session_stats.clear();
            p.chart_configs.clear();
        }
        self.replays_for_session_reset = None;
        self.clear_before_session_reset = true;
        self.file_viewer.lock().clear();
        self.replay_renderers.lock().clear();
        self.available_builds.clear();
        self.selected_browser_build = 0;
        // Dropping the map drops which builds it could not resolve; the
        // fire-section record lives outside it and is cleared on its own.
        self.wows_data_map = None;
        crate::ui::replay_parser::clear_fire_section_failures();
    }

    /// Process replays selected for session stats update.
    /// If `clear_before_session_reset` is true, clears existing stats first.
    /// If any replays haven't been parsed yet, they will be queued for parsing.
    pub(crate) fn process_session_stats_reset(&mut self) {
        let Some(weak_replays) = self.replays_for_session_reset.take() else {
            return;
        };

        if self.clear_before_session_reset {
            self.persisted.write().session_stats.clear();
        }

        // Upgrade weak references and add to session stats
        for weak_replay in weak_replays {
            if let Some(replay) = weak_replay.upgrade() {
                let replay_guard = replay.read();

                // Check if the replay needs parsing (no ui_report means not parsed)
                let needs_parsing = replay_guard.ui_report.is_none();

                // If already parsed, extract stats and add immediately
                if !needs_parsing
                    && let Some(stat) = PerGameStat::from_replay(&replay_guard, &replay_guard.resource_loader)
                {
                    self.persisted.write().session_stats.add_game(stat);
                }

                drop(replay_guard);

                if needs_parsing {
                    // Queue the replay for parsing (skip UI update since this is batch loading)
                    if let Some(deps) = self.replay_dependencies() {
                        update_background_task!(
                            self.background_tasks,
                            ReplayLoader::from_replay(deps, replay.clone())
                                .source(ReplaySource::SessionStatsOnly)
                                .load()
                        );
                    }
                }
            }
        }

        // Focus the Overview sub-tab automatically
        let mut p = self.persisted.write();
        if let Some(tab_path) = p.stats_dock_state.find_tab(&StatsSubTab::Overview) {
            let _ = p.stats_dock_state.set_active_tab(tab_path);
        }
    }

    pub(crate) fn update_wows_dir(&mut self, wows_dir: &Path, replay_dir: &Path) {
        // Persist the directory immediately — before anything that might early-return.
        self.persisted.write().settings.game.wows_dir = wows_dir.to_str().unwrap().to_string();
        // `replay_dir` is always the game's own replays directory, so this
        // belongs to the live workspace regardless of which one is active.
        self.live_workspace.root = Some(replay_dir.to_owned());
        self.revalidate_wows_dir();

        // Drop old watcher and background parser thread (if any).
        // Dropping background_parser_tx closes the channel, causing the old
        // parser thread to exit when its recv() returns Err.
        self.file_watcher = None;
        self.file_receiver = None;
        self.background_parser_tx = None;

        debug!("creating filesystem watcher");
        let (tx, rx) = mpsc::channel();
        let (background_tx, background_rx) = mpsc::channel();

        self.background_parser_tx = Some(background_tx.clone());

        if let Some(wows_data_map) = self.wows_data_map.clone() {
            let p = self.persisted.read();
            let background_thread_data = BackgroundParserThread {
                rx: background_rx,
                sent_replays: Arc::clone(&self.sent_replays),
                wows_data_map,
                twitch_state: Arc::clone(&self.twitch_state),
                data_sharing_mode: p.settings.integrations.data_sharing_mode,
                data_export_settings: DataExportSettings {
                    should_auto_export: p.settings.replay.auto_export_data,
                    export_path: PathBuf::from(p.settings.replay.auto_export_path.clone()),
                    export_format: p.settings.replay.auto_export_format,
                },

                player_tracker: Arc::clone(&self.player_tracker),
                is_debug: p.settings.app.debug_mode,
                parser_lock: Arc::clone(&self.parser_lock),
                cap_layout_db: Arc::clone(&self.cap_layout_db),
                db_pool: self.db_pool.clone(),
                tokio_runtime: self.tokio_runtime.clone(),
                personal_rating_data: Arc::clone(&self.personal_rating_data),
                index_source_id: None,
                unindexable: crate::data::replay_reconcile::Unindexable::default(),
            };
            drop(p);
            crate::task::start_background_parsing_thread(background_thread_data);
        }

        let mut watcher =
            match notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| match res {
                Ok(event) => {
                    // TODO: maybe properly handle moves?
                    debug!("filesytem event: {:?}", event);

                    // The receiver is dropped when the WoWs directory changes or
                    // the app shuts down. Log and drop the event rather than
                    // panicking, which runs on notify's own thread and would kill
                    // the watcher for the rest of the session.
                    let send_ui = |file_event: NotifyFileEvent| {
                        if let Err(e) = tx.send(file_event) {
                            debug!("file watcher receiver disconnected, dropping event: {e}");
                        }
                    };

                    match event.kind {
                        EventKind::Modify(ModifyKind::Name(RenameMode::To)) | EventKind::Create(_) => {
                            for path in event.paths {
                                if !path.is_file() {
                                    continue;
                                }
                                let Some(file_name) = path.file_name() else {
                                    continue;
                                };
                                let is_replay = path.extension().map(|ext| ext == "wowsreplay").unwrap_or(false);
                                if is_replay && file_name != "temp.wowsreplay" {
                                    send_ui(NotifyFileEvent::Added(path.clone()));
                                    // Send this path to the thread watching for replays in background
                                    let _ = background_tx
                                        .send(crate::task::ReplayBackgroundParserThreadMessage::NewReplay(path));
                                } else if file_name == "tempArenaInfo.json" {
                                    send_ui(NotifyFileEvent::TempArenaInfoCreated(path.clone()));
                                }
                            }
                        }
                        EventKind::Modify(ModifyKind::Data(_)) => {
                            for path in event.paths {
                                if let Some(filename) = path.file_name()
                                    && filename == "preferences.xml"
                                {
                                    debug!("Sending preferences changed event");
                                    send_ui(NotifyFileEvent::PreferencesChanged);
                                }
                                if path.extension().map(|ext| ext == "wowsreplay").unwrap_or(false) {
                                    send_ui(NotifyFileEvent::Modified(path.clone()));
                                    let _ = background_tx
                                        .send(crate::task::ReplayBackgroundParserThreadMessage::ModifiedReplay(path));
                                }
                            }
                        }
                        EventKind::Remove(_) => {
                            for path in event.paths {
                                send_ui(NotifyFileEvent::Removed(path));
                            }
                        }
                        _ => {
                            // TODO: handle RenameMode::From for proper file moves
                        }
                    }
                }
                Err(e) => debug!("watch error: {:?}", e),
            }) {
                Ok(w) => w,
                Err(e) => {
                    self.toasts.lock().error(rust_i18n::t!("error.file_watcher_creation", error = e));
                    return;
                }
            };

        if let Err(e) = watcher.watch(replay_dir, RecursiveMode::NonRecursive) {
            self.toasts.lock().error(rust_i18n::t!("error.replay_dir_watch", error = e));
            return;
        }

        self.file_watcher = Some(watcher);
        self.file_receiver = Some(rx);
    }

    /// Re-check whether `settings.wows_dir` points to a valid WoWs installation.
    /// Call this on startup and whenever the directory setting changes.
    pub(crate) fn revalidate_wows_dir(&mut self) {
        let dir = self.persisted.read().settings.game.wows_dir.clone();
        self.wows_dir_invalid = if dir.is_empty() {
            false
        } else {
            let wows_dir = std::path::Path::new(&dir);
            if !wows_dir.exists() {
                true
            } else {
                let has_exe = wows_dir.join("WorldOfWarships.exe").exists();
                let has_bin = wows_dir.join("bin").exists();
                let has_replays = wows_dir.join("replays").exists();
                !has_exe && !has_bin && !has_replays
            }
        };
    }

    #[must_use]
    pub fn load_game_data(&self, wows_directory: PathBuf) -> BackgroundTask {
        let (tx, rx) = mpsc::channel();
        let settings = self.persisted.read();
        let locale = settings.settings.app.locale.clone().unwrap();
        let auto_dump = settings.settings.game.auto_dump_game_data;
        let cache_dir = settings.settings.game.game_data_cache_dir.clone();
        drop(settings);
        let fallback_constants: serde_json::Value =
            serde_json::from_str(include_str!("../../../embedded_resources/constants.json"))
                .expect("failed to parse embedded constants JSON");
        let _join_handle = crate::util::thread::spawn_logged("load-game-data", move || {
            let _ = tx.send(crate::task::load_wows_files(
                wows_directory,
                locale.as_str(),
                &fallback_constants,
                auto_dump,
                cache_dir,
            ));
        });

        BackgroundTask { receiver: Some(rx), kind: BackgroundTaskKind::LoadingData }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The save task wakes on any generation change and then re-serializes the
    /// whole persisted state, so a per-frame mutation that changes nothing has
    /// to leave the counter alone.
    #[test]
    fn only_a_tracked_write_marks_the_state_dirty() {
        let state = TrackedPersistedState::default();
        let start = state.generation();

        state.write_untracked().output_dir = "untracked".to_string();
        assert_eq!(state.generation(), start, "an untracked write must not wake the save task");

        state.write().output_dir = "tracked".to_string();
        assert_ne!(state.generation(), start, "a tracked write must wake the save task");
    }

    #[test]
    fn workspace_live_and_active_workspace_name_the_same_workspace() {
        let state = TabState::default();
        let via_workspace = state.workspace(WorkspaceId::LIVE).expect("LIVE always resolves");
        let via_active = state.active_workspace();
        assert!(
            std::ptr::eq(via_workspace, via_active),
            "workspace(LIVE) and active_workspace() must resolve to the same ReplayWorkspace by default"
        );
    }

    #[test]
    fn active_workspace_mut_does_not_grow_workspaces() {
        let mut state = TabState::default();
        let before = state.workspaces.len();
        let _ = state.active_workspace_mut();
        assert_eq!(state.workspaces.len(), before, "active_workspace_mut must never insert into workspaces");
    }

    #[test]
    fn unknown_id_misses_workspace_but_active_workspace_still_resolves() {
        let mut state = TabState::default();
        state.active_workspace_id = WorkspaceId(9999);
        assert!(state.workspace(WorkspaceId(9999)).is_none());
        let via_active = state.active_workspace();
        assert!(
            std::ptr::eq(via_active, &state.live_workspace),
            "an unknown active_workspace_id must fall back to live_workspace"
        );
    }

    /// `workspaces` is `pub`, so nothing stops a `LIVE` key from ending up in
    /// it. All four accessors must still resolve to the `live_workspace`
    /// field, not the map entry -- a map-first implementation would return
    /// the decoy here instead.
    #[test]
    fn a_live_entry_in_the_map_is_ignored_by_every_accessor() {
        let mut state = TabState::default();
        state.live_workspace.root = Some(PathBuf::from("live"));
        state.workspaces.insert(WorkspaceId::LIVE, ReplayWorkspace::new(Some(PathBuf::from("decoy"))));
        let live_addr = &state.live_workspace as *const ReplayWorkspace as usize;

        assert_eq!(
            state.workspace(WorkspaceId::LIVE).expect("LIVE always resolves") as *const ReplayWorkspace as usize,
            live_addr,
            "workspace(LIVE) must ignore a LIVE entry in the map"
        );
        assert_eq!(
            state.active_workspace() as *const ReplayWorkspace as usize,
            live_addr,
            "active_workspace() must ignore a LIVE entry in the map"
        );
        assert_eq!(
            state.workspace_mut(WorkspaceId::LIVE).expect("LIVE always resolves") as *mut ReplayWorkspace as usize,
            live_addr,
            "workspace_mut(LIVE) must ignore a LIVE entry in the map"
        );
        assert_eq!(
            state.active_workspace_mut() as *mut ReplayWorkspace as usize,
            live_addr,
            "active_workspace_mut() must ignore a LIVE entry in the map"
        );

        assert_eq!(
            state.workspaces.get(&WorkspaceId::LIVE).and_then(|w| w.root.clone()),
            Some(PathBuf::from("decoy")),
            "the decoy map entry itself must be untouched"
        );
    }

    /// A minimal but real `Replay`: an empty-params `GameMetadataProvider`
    /// (no VFS needed) backing a hand-built `ReplayMeta` round-tripped
    /// through `ReplayFile::from_decrypted_parts`, the same entry point the
    /// app uses for a loaded replay's raw JSON.
    fn test_replay() -> Arc<RwLock<Replay>> {
        let meta = wows_replays::ReplayMeta {
            matchGroup: None,
            gameMode: 0,
            gameType: None,
            clientVersionFromExe: "0,0,0,0".to_string(),
            scenarioUiCategoryId: None,
            mapDisplayName: String::new(),
            mapId: 0,
            clientVersionFromXml: String::new(),
            weatherParams: None,
            duration: 0,
            gameLogic: None,
            name: String::new(),
            scenario: String::new(),
            playerID: wows_replays::types::AccountId(0),
            vehicles: Vec::new(),
            playersPerTeam: 0,
            dateTime: String::new(),
            mapName: String::new(),
            playerName: String::new(),
            scenarioConfigId: 0,
            teamsCount: 0,
            logic: None,
            playerVehicle: String::new(),
            battleDuration: None,
        };
        let meta_json = serde_json::to_vec(&meta).expect("ReplayMeta serializes");
        let replay_file = ReplayFile::from_decrypted_parts(meta_json, Vec::new())
            .expect("a ReplayMeta we just serialized parses back");
        let resource_loader = Arc::new(
            wowsunpack::game_params::provider::GameMetadataProvider::from_params_no_specs(Vec::new())
                .expect("an empty param list is always valid"),
        );
        Arc::new(RwLock::new(Replay::new(replay_file, resource_loader)))
    }

    /// A watcher `Removed` event names a path in the live replays directory
    /// (the only directory watched), so it must always be applied to
    /// `live_workspace` -- never to whichever workspace the inspector happens
    /// to be showing. Both workspaces hold an entry at the same path so a
    /// routing mistake is observable in both directions: routing to the
    /// active workspace would leave the live entry in place and delete the
    /// active one instead of the reverse.
    #[test]
    fn removed_watcher_event_mutates_the_live_workspace_not_the_active_one() {
        let mut state = TabState::default();
        let path = PathBuf::from("replay.wowsreplay");

        let mut live_files = HashMap::new();
        live_files.insert(path.clone(), test_replay());
        state.live_workspace.replay_files = Some(live_files);

        let other_id = WorkspaceId(1);
        let mut other_workspace = ReplayWorkspace::new(None);
        let mut other_files = HashMap::new();
        other_files.insert(path.clone(), test_replay());
        other_workspace.replay_files = Some(other_files);
        state.workspaces.insert(other_id, other_workspace);
        state.set_active_workspace(other_id);
        assert_eq!(state.active_workspace_id(), other_id, "the other workspace must actually be active");

        let (tx, rx) = mpsc::channel();
        state.file_receiver = Some(rx);
        tx.send(NotifyFileEvent::Removed(path.clone())).expect("receiver is held by state, not dropped");

        state.try_update_replays();

        assert!(
            !state.live_workspace.replay_files.as_ref().expect("set above").contains_key(&path),
            "the live workspace's entry must be removed"
        );
        assert!(
            state
                .workspace(other_id)
                .expect("inserted above")
                .replay_files
                .as_ref()
                .expect("set above")
                .contains_key(&path),
            "the active (non-live) workspace's entry must be untouched"
        );
    }

    fn ingest_batch(
        workspace: WorkspaceId,
        paths: &[&str],
        done: usize,
        total: usize,
    ) -> crate::task::replays::IngestBatch {
        crate::task::replays::IngestBatch {
            workspace,
            source: crate::db::index::rows::SourceId(3),
            replays: paths.iter().map(|path| (PathBuf::from(path), test_replay())).collect(),
            progress: crate::task::replays::IngestProgress { done, total },
        }
    }

    /// The listing already holds a replay when the next batch lands, which is
    /// what every batch after the first sees. An implementation that assigns
    /// `replay_files` instead of merging drops `a.wowsreplay` here.
    #[test]
    fn an_ingest_batch_merges_into_a_listing_that_already_lists_a_replay() {
        let mut state = TabState::default();
        let id = WorkspaceId(1);
        let mut workspace = ReplayWorkspace::new(None);
        workspace.replay_files = Some(HashMap::from([(PathBuf::from("a.wowsreplay"), test_replay())]));
        state.workspaces.insert(id, workspace);

        state.apply_ingest_batch(ingest_batch(id, &["b.wowsreplay"], 2, 2));

        let files = state.workspace(id).expect("inserted above").replay_files.as_ref().expect("set above");
        assert!(files.contains_key(Path::new("a.wowsreplay")), "the replay already listed must survive the batch");
        assert!(files.contains_key(Path::new("b.wowsreplay")), "the batch's replay must be added");
    }

    /// The very first batch of a walk arrives before anything is listed, so it
    /// has to start the listing rather than being dropped for want of one.
    #[test]
    fn the_first_ingest_batch_starts_the_listing() {
        let mut state = TabState::default();
        let id = WorkspaceId(1);
        state.workspaces.insert(id, ReplayWorkspace::new(None));

        state.apply_ingest_batch(ingest_batch(id, &["a.wowsreplay"], 1, 4));

        let workspace = state.workspace(id).expect("inserted above");
        assert_eq!(workspace.replay_files.as_ref().map(|files| files.len()), Some(1));
        assert_eq!(workspace.source, Some(crate::db::index::rows::SourceId(3)), "the batch's source must be adopted");
        assert_eq!(
            workspace.ingest_progress.map(|progress| (progress.done, progress.total)),
            Some((1, 4)),
            "the listing needs the walk's progress to report it"
        );
    }

    /// Batches are routed by the id they carry, exactly like watcher events are
    /// routed to the live workspace. Both workspaces are listed so a routing
    /// mistake is observable in both directions.
    #[test]
    fn an_ingest_batch_lands_on_the_workspace_it_names_not_the_active_one() {
        let mut state = TabState::default();
        let target = WorkspaceId(1);
        let active = WorkspaceId(2);
        state.workspaces.insert(target, ReplayWorkspace::new(None));
        state.workspaces.insert(active, ReplayWorkspace::new(None));
        state.set_active_workspace(active);
        assert_eq!(state.active_workspace_id(), active, "the other workspace must actually be active");

        state.apply_ingest_batch(ingest_batch(target, &["a.wowsreplay"], 1, 1));

        assert_eq!(
            state.workspace(target).expect("inserted above").replay_files.as_ref().map(|files| files.len()),
            Some(1),
            "the named workspace must receive the batch"
        );
        assert!(
            state.workspace(active).expect("inserted above").replay_files.is_none(),
            "the active workspace must not receive another workspace's batch"
        );
    }

    /// Closing a directory tab while its walk runs leaves batches in flight
    /// with nowhere to land. Dropping them is the intended outcome, and must
    /// not fall back to the live workspace.
    #[test]
    fn an_ingest_batch_for_a_closed_workspace_is_dropped() {
        let mut state = TabState::default();
        state.apply_ingest_batch(ingest_batch(WorkspaceId(99), &["a.wowsreplay"], 1, 1));

        assert!(state.live_workspace.replay_files.is_none(), "a departed workspace's batch must not land on live");
        assert!(state.workspaces.is_empty(), "a departed workspace must not be recreated by its own batch");
    }

    #[test]
    fn set_active_workspace_falls_back_to_live_for_an_unknown_id() {
        let mut state = TabState::default();
        state.set_active_workspace(WorkspaceId(1234));
        assert_eq!(
            state.active_workspace_id(),
            WorkspaceId::LIVE,
            "a workspace that is not open must not become active"
        );
    }

    #[test]
    fn set_active_workspace_accepts_a_present_non_live_id() {
        let mut state = TabState::default();
        let id = WorkspaceId(1);
        state.workspaces.insert(id, ReplayWorkspace::new(None));
        state.set_active_workspace(id);
        assert_eq!(state.active_workspace_id(), id, "a workspace that is open must become active");
    }

    #[test]
    fn close_workspace_removes_the_entry_and_resets_the_active_id_when_it_was_active() {
        let mut state = TabState::default();
        let id = WorkspaceId(1);
        state.workspaces.insert(id, ReplayWorkspace::new(None));
        state.set_active_workspace(id);

        state.close_workspace(id);

        assert!(state.workspaces.get(&id).is_none(), "the closed workspace must be dropped from the map");
        assert_eq!(
            state.active_workspace_id(),
            WorkspaceId::LIVE,
            "closing the active workspace must fall back to live"
        );
    }

    /// egui_dock calls `on_close` from the context-menu close action and then
    /// again from the deferred tab removal, so a second call for the same
    /// already-closed id must not misbehave (e.g. by finding the id absent
    /// and somehow un-resetting the active id it already reset).
    #[test]
    fn close_workspace_is_idempotent_on_a_second_call() {
        let mut state = TabState::default();
        let id = WorkspaceId(1);
        state.workspaces.insert(id, ReplayWorkspace::new(None));
        state.set_active_workspace(id);

        state.close_workspace(id);
        state.close_workspace(id);

        assert!(state.workspaces.get(&id).is_none());
        assert_eq!(state.active_workspace_id(), WorkspaceId::LIVE);
    }

    #[test]
    fn workspace_for_tab_resolves_live_to_the_live_workspace() {
        let state = TabState::default();
        let live_addr = &state.live_workspace as *const ReplayWorkspace as usize;
        let resolved = state.workspace_for_tab(WorkspaceId::LIVE).expect("LIVE always resolves");
        assert_eq!(
            resolved as *const ReplayWorkspace as usize, live_addr,
            "workspace_for_tab(LIVE) must resolve to live_workspace"
        );
    }

    /// Three distinct workspaces (live, the tab's own, and whatever happens to be
    /// active) so "resolves by the id the tab carries" is distinguishable from
    /// both "resolves to live" and "resolves to whatever is active" -- a
    /// regression that fell back to `active_workspace()` would return the
    /// "active" root here instead of the "tab" root.
    #[test]
    fn workspace_for_tab_resolves_a_present_non_live_id_to_that_workspace_not_active_or_live() {
        let mut state = TabState::default();
        state.live_workspace.root = Some(PathBuf::from("live"));

        let tab_id = WorkspaceId(1);
        state.workspaces.insert(tab_id, ReplayWorkspace::new(Some(PathBuf::from("tab"))));

        let active_id = WorkspaceId(2);
        state.workspaces.insert(active_id, ReplayWorkspace::new(Some(PathBuf::from("active"))));
        state.set_active_workspace(active_id);
        assert_eq!(state.active_workspace_id(), active_id, "the active workspace must differ from the tab's id");

        let resolved = state.workspace_for_tab(tab_id).expect("tab_id is open");
        assert_eq!(
            resolved.root,
            Some(PathBuf::from("tab")),
            "must resolve to the tab's own workspace, not active or live"
        );
    }

    #[test]
    fn workspace_for_tab_does_not_resolve_an_absent_id_to_the_live_workspace() {
        let state = TabState::default();
        assert!(
            state.workspace_for_tab(WorkspaceId(42)).is_none(),
            "a tab naming a closed workspace must not resolve to live_workspace"
        );
    }

    /// The map length matters as much as the id: an implementation that
    /// returned the existing id but *also* inserted a second entry for the
    /// same root would satisfy an id-only assertion and still leave the user
    /// with two tabs listing one directory.
    #[test]
    fn opening_the_same_root_twice_returns_the_same_id_without_growing_the_map() {
        let mut state = TabState::default();
        let first = state.open_directory_workspace(PathBuf::from("D:/replays"));
        assert_eq!(state.workspaces.len(), 1, "the first open must create exactly one workspace");

        let second = state.open_directory_workspace(PathBuf::from("D:/replays"));

        assert_eq!(second, first, "a root that is already open must resolve to its existing workspace");
        assert_eq!(state.workspaces.len(), 1, "re-opening a root must not insert a second workspace");
        assert_eq!(
            state.workspace(first).expect("just opened").root,
            Some(PathBuf::from("D:/replays")),
            "the surviving workspace must still list the root it was opened for"
        );
    }

    #[test]
    fn opening_two_different_roots_gives_two_workspaces_with_different_ids() {
        let mut state = TabState::default();
        let first = state.open_directory_workspace(PathBuf::from("D:/replays"));
        let second = state.open_directory_workspace(PathBuf::from("D:/other"));

        assert_ne!(first, second, "two directories must not share one workspace");
        assert_eq!(state.workspaces.len(), 2);
        assert_eq!(state.workspace(first).expect("just opened").root, Some(PathBuf::from("D:/replays")));
        assert_eq!(state.workspace(second).expect("just opened").root, Some(PathBuf::from("D:/other")));
    }

    /// `WorkspaceId::LIVE` names the game's own replays directory and has its
    /// own permanent tab. Handing it out here would make a directory tab draw
    /// (and closing it destroy) the live listing instead.
    #[test]
    fn opening_a_directory_never_returns_the_live_id() {
        let mut state = TabState::default();
        state.live_workspace.root = Some(PathBuf::from("D:/replays"));

        let id = state.open_directory_workspace(PathBuf::from("D:/replays"));

        assert_ne!(id, WorkspaceId::LIVE, "even the live root must open as its own workspace, not as LIVE");
        assert_eq!(state.workspaces.len(), 1);
    }

    /// Ids are handed out monotonically and never reused, so a completion that
    /// arrives for a workspace the user already closed cannot land on whichever
    /// directory was opened next.
    #[test]
    fn a_closed_workspaces_id_is_not_reused_by_the_next_directory() {
        let mut state = TabState::default();
        let first = state.open_directory_workspace(PathBuf::from("D:/replays"));
        state.close_workspace(first);

        let second = state.open_directory_workspace(PathBuf::from("D:/other"));

        assert_ne!(second, first, "a closed workspace's id must not be recycled");
    }

    #[test]
    fn close_workspace_leaves_the_active_id_untouched_when_closing_a_different_workspace() {
        let mut state = TabState::default();
        let active_id = WorkspaceId(1);
        let other_id = WorkspaceId(2);
        state.workspaces.insert(active_id, ReplayWorkspace::new(None));
        state.workspaces.insert(other_id, ReplayWorkspace::new(None));
        state.set_active_workspace(active_id);

        state.close_workspace(other_id);

        assert!(state.workspaces.get(&other_id).is_none(), "the closed workspace must be dropped from the map");
        assert_eq!(
            state.active_workspace_id(),
            active_id,
            "closing a workspace that is not active must not disturb the active id"
        );
    }
}
