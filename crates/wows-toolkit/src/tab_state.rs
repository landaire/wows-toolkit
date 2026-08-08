use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;
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
use wowsunpack::data::Version;
use wowsunpack::vfs::VfsPath;

use crate::data::session_stats::PerGameStat;
use crate::data::session_stats::SessionStats;
use crate::data::settings::AppSettings;
use crate::data::wows_data::BuildDataCache;
use crate::data::wows_data::ReplayDependencies;
use crate::data::wows_data::ReplayLoader;
use crate::data::wows_data::SharedBuildData;
use crate::db::index::rows::WorkspaceId;
use crate::task::BackgroundParserThread;
use crate::task::BackgroundTask;
use crate::task::BackgroundTaskKind;
use crate::task::DataExportSettings;
use crate::task::FlushState;
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
    SetAsSessionStats { replays: Vec<PathBuf> },
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

/// Where a replay opened from search is going, as resolved by
/// [`TabState::workspace_to_open_replay_in`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchOpenTarget {
    /// A workspace that was already open lists the replay.
    Existing(WorkspaceId),
    /// Nothing open listed the replay, so `root` was opened as a workspace of
    /// its own. Its listing still has to be scanned, and the tab that appeared
    /// has to be accounted for to the user.
    Opened { id: WorkspaceId, root: PathBuf },
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
    pub shipbuilds_client: crate::data::shipbuilds::ShipBuildsClient,

    // ─── Transient / runtime-only state ──────────────────────────────────
    pub world_of_warships_data: Option<SharedBuildData>,
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
    pub pending_search_query: Option<crate::db::index::query_ast::MatchExpr>,
    /// When true, the app focuses the Search tab next frame (from palette/tracker).
    pub pending_focus_search: bool,
    /// A replay a search result asked to open. Consumed by the app loop after
    /// the dock has drawn, which is the only place the outer dock and the
    /// workspace list can both be written.
    pub pending_search_open: Option<crate::ui::search_tab::SearchOpenRequest>,
    /// Cached ship catalog for palette ship entries; built lazily on first palette open.
    pub ship_catalog: Option<crate::armor_viewer::ship_selector::ShipCatalog>,
    pub file_viewer: Mutex<Vec<PlaintextFileViewer>>,
    pub replay_renderers: Mutex<Vec<crate::replay::renderer::ReplayRendererViewer>>,
    pub renderer_asset_cache: Arc<parking_lot::Mutex<crate::replay::renderer::RendererAssetCache>>,
    pub renderer_texture_cache: Arc<parking_lot::Mutex<crate::replay::renderer::RendererTextureCache>>,
    pub preview_cache: Arc<parking_lot::Mutex<crate::replay::renderer::preview::PreviewCache>>,
    pub tactics_boards: Mutex<Vec<crate::replay::minimap_view::tactics::TacticsBoardViewer>>,
    /// Board IDs we've already auto-opened (prevents re-open after user closes them).
    pub tactics_auto_opened_board_ids: std::collections::HashSet<u64>,
    /// Shared tokio runtime for collab sessions and async tasks.
    pub tokio_runtime: Option<Arc<tokio::runtime::Runtime>>,
    /// SQLite connection pool for persistence.
    pub db_pool: Option<sqlx::SqlitePool>,
    /// Id of the live replay-index source, remembered once a lookup finds it.
    /// The indexer creates that source, never a reader, so it stays `None`
    /// until the indexer has written it. Private so every reader goes through
    /// [`Self::live_index_source`] and shares the one lookup.
    live_index_source: Option<crate::db::index::rows::SourceId>,
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
    /// Named by path so a queued batch retains nothing: each is read and parsed
    /// when its turn comes.
    pub replays_for_session_reset: Option<Vec<PathBuf>>,
    pub clear_before_session_reset: bool,
    /// Pending action awaiting user confirmation.
    pub pending_confirmation: Option<ConfirmableAction>,
    /// All loaded version data, keyed by build number.
    pub build_cache: Option<BuildDataCache>,
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
    /// Screen rects of the dock's tab buttons, refreshed every frame by
    /// `ToolkitTabViewer::on_tab_button`. `TabViewer::ui` runs only for the
    /// active tab and is handed no rect of its own, so this is how the active
    /// tab's marker finds the button that opened it.
    pub dock_tab_rects: Vec<(crate::app::Tab, egui::Rect)>,
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
            shipbuilds_client: crate::data::shipbuilds::ShipBuildsClient::new()
                .expect("failed to build ShipBuilds HTTP client"),
            world_of_warships_data: None,
            items_to_extract: Default::default(),
            translations: Default::default(),
            unpacker_progress: Default::default(),
            last_progress: Default::default(),
            search_tab: Default::default(),
            command_palette: Default::default(),
            pending_search_query: None,
            pending_focus_search: false,
            pending_search_open: None,
            ship_catalog: None,
            file_viewer: Default::default(),
            replay_renderers: Default::default(),
            renderer_asset_cache: Default::default(),
            renderer_texture_cache: Default::default(),
            preview_cache: Default::default(),
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
            build_cache: None,
            available_builds: Vec::new(),
            selected_browser_build: 0,
            browser_state: Default::default(),
            suppress_gpu_encoder_warning: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            network_job_tx: None,
            settings_needs_attention: false,
            active_theme: egui::Theme::Dark,
            dock_tab_rects: Vec::new(),
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
            live_index_source: None,
            window_settings: Default::default(),
            active_viewports: Arc::new(parking_lot::Mutex::new(Vec::new())),
            save_notify: Arc::new(tokio::sync::Notify::new()),
        }
    }
}

/// Turn flagged builds into download requests, dropping any whose version
/// string does not even parse a major component; nothing can be requested for
/// those. A build the remote resolves through a fallback still needs an entry
/// here to trigger the request, not to name the exact build fetched.
fn build_requests_from_updates(
    updates: Vec<wows_data_mgr::download_repo::BuildUpdateStatus>,
) -> Vec<crate::task::BuildRequest> {
    updates
        .into_iter()
        .filter_map(|update| {
            let mut parts = update.version.split('.').filter_map(|p| p.trim().parse::<u32>().ok());
            let version = Version {
                major: parts.next()?,
                minor: parts.next().unwrap_or(0),
                patch: parts.next().unwrap_or(0),
                build: std::num::NonZeroU32::new(update.build),
            };
            crate::task::BuildRequest::new(version)
        })
        .collect()
}

impl TabState {
    /// Stable id for the Settings tab's WoWs-directory `TextEdit`. The sticky
    /// error toast asks egui's own focus memory whether this id is focused,
    /// rather than caching a snapshot that only settings_tab.rs could update
    /// and that would go stale once the tab stops drawing (see the arming
    /// check in app.rs).
    pub(crate) fn wows_dir_field_id() -> egui::Id {
        egui::Id::new("settings.wows_dir_field")
    }

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

    /// The live replay-index source, looked up through the pool the first time
    /// it is needed and remembered afterwards.
    ///
    /// `None` while the indexer has not created that source yet, and when
    /// there is no pool or runtime to ask through. A caller that needs to scope
    /// work to the live replays has to treat `None` as "cannot scope", never as
    /// "scope to everything".
    pub fn live_index_source(&mut self) -> Option<crate::db::index::rows::SourceId> {
        if self.live_index_source.is_some() {
            return self.live_index_source;
        }
        let (Some(pool), Some(rt)) = (self.db_pool.as_ref(), self.tokio_runtime.as_ref()) else {
            return None;
        };
        match rt.block_on(crate::db::index::query::live_source_id(pool)) {
            Ok(found) => {
                self.live_index_source = found;
                found
            }
            Err(e) => {
                warn!("failed to resolve the live replay index source: {e}");
                None
            }
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

    /// Every workspace, live one included.
    pub fn all_workspaces(&self) -> impl Iterator<Item = &ReplayWorkspace> {
        std::iter::once(&self.live_workspace).chain(self.workspaces.values())
    }

    /// Replay paths listed across every open workspace.
    pub fn open_replay_paths(&self) -> BTreeSet<PathBuf> {
        self.all_workspaces()
            .filter_map(|workspace| workspace.replay_files.as_ref())
            .flat_map(|replay_files| replay_files.keys().cloned())
            .collect()
    }

    /// Every workspace paired with the id that names it, live one first.
    pub fn all_workspaces_with_ids(&self) -> impl Iterator<Item = (WorkspaceId, &ReplayWorkspace)> {
        std::iter::once((WorkspaceId::LIVE, &self.live_workspace))
            .chain(self.workspaces.iter().map(|(id, workspace)| (*id, workspace)))
    }

    /// The open workspace whose listing covers `path`, if any.
    ///
    /// A workspace lists its root recursively -- [`crate::task::replays::walk_replay_files`]
    /// walks with `WalkDir` -- so ownership is "the root is an ancestor of the
    /// file", not "the root is the file's parent". Roots nest legitimately: a
    /// subdirectory of the live replays directory can be imported as its own
    /// workspace, and then both list the same file. The deepest matching root
    /// wins, because that is the listing showing the file most tightly. Two
    /// distinct roots cannot tie on depth while both being ancestors of one
    /// path, so the only tie is two workspaces on the same root, which the live
    /// workspace takes by drawing first.
    pub fn workspace_for_replay_path(&self, path: &Path) -> Option<WorkspaceId> {
        let mut best: Option<(WorkspaceId, usize)> = None;
        for (id, workspace) in self.all_workspaces_with_ids() {
            let Some(root) = workspace.root.as_deref() else {
                continue;
            };
            if !path.starts_with(root) {
                continue;
            }
            let depth = root.components().count();
            if best.is_none_or(|(_, best_depth)| depth > best_depth) {
                best = Some((id, depth));
            }
        }
        best.map(|(id, _)| id)
    }

    /// Every replay open in a dock tab of any workspace. What invalidates a
    /// cached report -- new constants, a locale change -- invalidates it in
    /// every workspace, and a tab the user has to switch tabs to see is still a
    /// tab they will look at.
    pub fn all_open_replays(&self) -> Vec<Arc<RwLock<Replay>>> {
        self.all_workspaces().flat_map(|workspace| workspace.open_replays()).collect()
    }

    /// The hydrated replay for `path` if any workspace has that file open in a
    /// dock tab. Opening is what hydrates a replay, so a file that is only
    /// listed has none.
    pub fn open_replay_at(&self, path: &Path) -> Option<Arc<RwLock<Replay>>> {
        self.all_workspaces().find_map(|workspace| workspace.hydrated_replay(path))
    }

    /// Every hydrated replay for `path`, across all workspaces. A directory
    /// imported as a workspace can be the live replays directory, so the same
    /// file can be open in more than one dock at once and all of them go stale
    /// together when it changes on disk.
    pub fn open_replays_at(&self, path: &Path) -> Vec<Arc<RwLock<Replay>>> {
        self.all_workspaces().filter_map(|workspace| workspace.hydrated_replay(path)).collect()
    }

    /// Replace the focused tab's replay, or open a new tab if none exists.
    pub fn open_replay_in_focused_tab(&mut self, replay: Arc<RwLock<Replay>>) {
        self.active_workspace_mut().open_replay_in_focused_tab(replay);
    }

    /// Which workspace an inspector sub-tab for `path` belongs in.
    ///
    /// When no open workspace lists `path`, its own directory is opened as one
    /// rather than the replay being adopted by whichever tab happens to be
    /// active: a search covers directories whose tabs were never opened this
    /// session, so refusing would make those results unopenable, while adopting
    /// would put a replay in a listing that does not contain it. Which of the
    /// two happened is reported rather than inferred, so the caller can start
    /// the new listing's scan and tell the user a tab appeared.
    ///
    /// `None` only when `path` names no directory to fall back on.
    pub fn workspace_to_open_replay_in(&mut self, path: &Path) -> Option<SearchOpenTarget> {
        if let Some(id) = self.workspace_for_replay_path(path) {
            return Some(SearchOpenTarget::Existing(id));
        }
        // An empty parent is what a bare filename yields. Opening it as a root
        // would produce a workspace that `starts_with` reports as containing
        // every path there is.
        let root = path.parent().filter(|parent| !parent.as_os_str().is_empty())?.to_path_buf();
        let id = self.open_directory_workspace(root.clone());
        Some(SearchOpenTarget::Opened { id, root })
    }

    /// Adds `replay` to `ws_id`'s dock as a new sub-tab, reporting whether it
    /// landed.
    ///
    /// Always a new sub-tab, never a focus of one already showing the same
    /// file: two views of one replay are what a dock is for, and search is how
    /// a replay open in one tab gets put beside another. A workspace closed
    /// between the click and this call has no dock to add to, and the replay is
    /// dropped rather than redirected into a listing that does not cover it.
    pub fn open_replay_in_new_workspace_tab(&mut self, ws_id: WorkspaceId, replay: Arc<RwLock<Replay>>) -> bool {
        let Some(workspace) = self.workspace_mut(ws_id) else {
            return false;
        };
        workspace.open_replay_in_new_tab(replay);
        true
    }

    /// Returns the shared dependencies needed for loading replays, if wows_data is available.
    pub fn replay_dependencies(&self) -> Option<ReplayDependencies> {
        let build_cache = self.build_cache.as_ref()?;
        Some(ReplayDependencies {
            build_cache: build_cache.clone(),
            shipbuilds_client: self.shipbuilds_client.clone(),
            twitch_state: Arc::clone(&self.twitch_state),
            replay_sort: Arc::clone(&self.replay_sort),
            background_task_sender: self.background_task_sender.clone(),
            is_debug_mode: self.persisted.read().settings.app.debug_mode,
            personal_rating_data: Arc::clone(&self.personal_rating_data),
        })
    }

    /// Read a listed replay off disk and wire it to its build's game data.
    ///
    /// The listing keeps only the metadata its rows draw, so this is what turns
    /// a row into something a replay tab can show. The packet parse that
    /// follows still happens in the background; only the file read is here.
    pub fn hydrate_replay(&self, path: &Path) -> Result<Arc<RwLock<Replay>>, rootcause::Report> {
        let Some(deps) = self.replay_dependencies() else {
            return Err(rootcause::report!("game data is not loaded"));
        };
        ReplayLoader::build_replay_from_existing_file(&deps, path.to_path_buf()).map(|(replay, _data)| replay)
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
        let Some(runtime) = self.tokio_runtime.as_ref().map(Arc::clone) else {
            warn!("cannot download game data: tokio runtime is not available");
            return;
        };
        let requests = build_requests_from_updates(std::mem::take(&mut self.game_data_updates));
        if requests.is_empty() {
            return;
        }
        update_background_task!(
            self.background_tasks,
            Some(crate::task::start_game_data_download_task(output_base, requests, runtime, true, None))
        );
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
        let Some(runtime) = self.tokio_runtime.as_ref().map(Arc::clone) else {
            warn!("cannot download game data: tokio runtime is not available");
            return;
        };
        let requests = build_requests_from_updates(std::mem::take(&mut self.game_data_repair));
        if requests.is_empty() {
            return;
        }
        update_background_task!(
            self.background_tasks,
            Some(crate::task::start_game_data_download_task(output_base, requests, runtime, true, None))
        );
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

                    // The watcher only observes the live replays directory, but
                    // that directory can also have been imported as a workspace,
                    // so every dock is asked. Only a replay open in a tab is
                    // hydrated; a merely listed one is re-read from the path
                    // below.
                    let open_replays = self.open_replays_at(&modified_file);

                    if !open_replays.is_empty() {
                        for replay in open_replays {
                            // Invalidate cached data so the reload re-parses the file.
                            let mut replay_inner = replay.write();
                            replay_inner.battle_report = None;
                            replay_inner.ui_report = None;
                            drop(replay_inner);

                            if let Some(deps) = self.replay_dependencies() {
                                update_background_task!(self.background_tasks, deps.load_replay(replay, source));
                            }
                        }
                    } else if let Some(deps) = self.replay_dependencies() {
                        // Nothing is open on this file. Read from the path so the
                        // modification is not dropped; the freshest read wins on
                        // completion, and its listing row is refreshed with it.
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

                            let build = Version::try_from_client_exe(&replay_file.meta.clientVersionFromExe)
                                .and_then(|v| v.build_number());
                            let started_at = crate::util::replay_timestamp(&replay_file.meta);
                            // tempArenaInfo.json and temp.wowsreplay are written as
                            // siblings by the game, so the roster lives next to it.
                            if let Some(replay) = path.parent().map(|dir| dir.join("temp.wowsreplay")) {
                                let _ = self.background_parser_tx.as_ref().map(|tx| {
                                    tx.send(ReplayBackgroundParserThreadMessage::LiveMatchStarted {
                                        replay,
                                        build,
                                        flush: FlushState::InProgress,
                                        started_at,
                                    })
                                });
                            }
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

    /// Apply one message from a running directory walk to the listing it names.
    pub fn apply_ingest_update(&mut self, update: crate::task::replays::IngestUpdate) {
        use crate::task::replays::IngestUpdate;

        match update {
            IngestUpdate::Walked { workspace, paths } => self.retain_listed_replays(workspace, &paths),
            IngestUpdate::Batch(batch) => self.apply_ingest_batch(batch),
            IngestUpdate::Stage { workspace, stage } => {
                if let Some(workspace) = self.workspace_mut(workspace) {
                    workspace.ingest_stage = Some(stage);
                }
            }
        }
    }

    /// Mark the run filling `workspace`'s listing as over, however it ended.
    ///
    /// The flag and the stage go together: a stage left behind draws progress
    /// for a run that is not happening, and a flag left behind refuses the next
    /// attempt at the directory.
    pub fn set_ingest_finished(&mut self, workspace: WorkspaceId) {
        if let Some(workspace) = self.workspace_mut(workspace) {
            workspace.ingest_in_flight = false;
            workspace.ingest_stage = None;
        }
    }

    /// Drop listing entries for replays a fresh walk of the same directory no
    /// longer finds on disk.
    ///
    /// Only a listing that already exists is touched: a walk that found nothing
    /// must not start one, or an empty directory would list as loaded before it
    /// has been read.
    fn retain_listed_replays(&mut self, workspace: WorkspaceId, present: &HashSet<PathBuf>) {
        let Some(workspace) = self.workspace_mut(workspace) else {
            return;
        };
        if let Some(files) = workspace.replay_files.as_mut() {
            files.retain(|path, _| present.contains(path));
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
        workspace.ingest_stage = Some(crate::task::replays::IngestStage::Reading(batch.progress));

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
        self.build_cache = None;
        crate::ui::replay_parser::clear_fire_section_failures();
    }

    /// Whether `path` is a replay the game itself wrote, i.e. one inside the
    /// configured replays directory.
    ///
    /// Session stats describe the user's own play. An imported directory is
    /// somebody's history being read, and the listing it draws reaches the same
    /// open path the live listing does, so the file's location is what
    /// separates the two. A replay with no path, or no configured directory to
    /// compare against, is not one of the user's own.
    pub(crate) fn is_primary_replay(&self, path: Option<&Path>) -> bool {
        let (Some(path), Some(root)) = (path, self.live_workspace.root.as_deref()) else {
            return false;
        };
        crate::util::paths::path_is_within(root, path)
    }

    /// Process replays selected for session stats update.
    /// If `clear_before_session_reset` is true, clears existing stats first.
    /// If any replays haven't been parsed yet, they will be queued for parsing.
    pub(crate) fn process_session_stats_reset(&mut self) {
        let Some(paths) = self.replays_for_session_reset.take() else {
            return;
        };

        if self.clear_before_session_reset {
            self.persisted.write().session_stats.clear();
        }

        for path in paths {
            // A replay already open in a tab carries its parse, so its stats
            // can be taken straight off it rather than paying for the file
            // again.
            let parsed = self.open_replay_at(&path).and_then(|replay| {
                let guard = replay.read();
                guard.ui_report.as_ref()?;
                PerGameStat::from_replay(&guard, &guard.resource_loader)
            });

            if let Some(stat) = parsed {
                self.persisted.write().session_stats.add_game(stat);
                continue;
            }

            // Read and parse in the background, skipping the UI update since
            // this is batch loading.
            if let Some(deps) = self.replay_dependencies() {
                update_background_task!(
                    self.background_tasks,
                    ReplayLoader::from_path(deps, path).source(ReplaySource::SessionStatsOnly).load()
                );
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

        if let Some(build_cache) = self.build_cache.clone() {
            let p = self.persisted.read();
            let background_thread_data = BackgroundParserThread {
                rx: background_rx,
                sent_replays: Arc::clone(&self.sent_replays),
                build_cache,
                shipbuilds_client: self.shipbuilds_client.clone(),
                twitch_state: Arc::clone(&self.twitch_state),
                persisted: Arc::clone(&self.persisted),
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
            let _ = crate::task::start_background_parsing_thread(background_thread_data);
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
    use std::collections::BTreeSet;

    fn insert_listed_paths<'a>(workspace: &mut ReplayWorkspace, paths: impl IntoIterator<Item = &'a PathBuf>) {
        workspace.replay_files = Some(paths.into_iter().map(|path| (path.clone(), listed_replay())).collect());
    }

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
    fn open_replay_paths_deduplicate_across_every_workspace() {
        let mut state = TabState::default();
        let shared = PathBuf::from("replays/shared.wowsreplay");
        let live_only = PathBuf::from("replays/live.wowsreplay");
        let ad_hoc_only = PathBuf::from("import/ad-hoc.wowsreplay");
        insert_listed_paths(&mut state.live_workspace, [&shared, &live_only]);
        let id = state.open_directory_workspace(PathBuf::from("import"));
        insert_listed_paths(state.workspace_mut(id).unwrap(), [&shared, &ad_hoc_only]);

        assert_eq!(state.open_replay_paths(), BTreeSet::from([shared, live_only, ad_hoc_only]));
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

    /// A minimal hydrated `Replay` opened from `path`, built the way
    /// `ReplayLoader` builds one: real replay metadata round-tripped through
    /// `ReplayFile::from_decrypted_parts`.
    fn open_replay(path: &str) -> Arc<RwLock<Replay>> {
        let meta = serde_json::json!({
            "gameMode": 0,
            "clientVersionFromExe": "0,0,0,0",
            "mapDisplayName": "",
            "mapId": 0,
            "clientVersionFromXml": "",
            "duration": 0,
            "name": "",
            "scenario": "",
            "playerID": 0,
            "vehicles": [],
            "playersPerTeam": 0,
            "dateTime": "28.07.2026 14:23:05",
            "mapName": "",
            "playerName": "",
            "scenarioConfigId": 0,
            "teamsCount": 0,
            "playerVehicle": "",
        });
        let replay_file =
            ReplayFile::from_decrypted_parts(serde_json::to_vec(&meta).expect("the fixture serializes"), Vec::new())
                .expect("hand-built replay metadata parses");
        let resource_loader = Arc::new(
            wowsunpack::game_params::provider::GameMetadataProvider::from_params_no_specs(Vec::new())
                .expect("an empty param list is always valid"),
        );
        let mut replay = Replay::new(replay_file, resource_loader);
        replay.source_path = Some(PathBuf::from(path));
        Arc::new(RwLock::new(replay))
    }

    /// New constants and a locale change invalidate every cached report, and a
    /// tab in a workspace the user is not currently looking at is still a tab
    /// they will look at. Both workspaces hold a tab so scoping the sweep to
    /// the active one is observable in either direction.
    #[test]
    fn every_workspaces_tabs_are_reachable_for_invalidation_not_just_the_active_ones() {
        let mut state = TabState::default();
        state.live_workspace.open_replay_in_new_tab(open_replay("live.wowsreplay"));

        let other = WorkspaceId(1);
        let mut workspace = ReplayWorkspace::new(None);
        workspace.open_replay_in_new_tab(open_replay("other.wowsreplay"));
        state.workspaces.insert(other, workspace);
        state.set_active_workspace(other);
        assert_eq!(state.active_workspace_id(), other, "the non-live workspace must actually be active");

        let paths: Vec<Option<PathBuf>> =
            state.all_open_replays().iter().map(|replay| replay.read().source_path.clone()).collect();

        assert_eq!(paths.len(), 2, "both workspaces' tabs must be swept");
        assert!(paths.contains(&Some(PathBuf::from("live.wowsreplay"))), "the inactive workspace's tab is included");
        assert!(paths.contains(&Some(PathBuf::from("other.wowsreplay"))), "the active workspace's tab is included");
    }

    /// The watcher only watches the live replays directory, but that directory
    /// can also have been imported as its own workspace, so one modified file
    /// can be open in two docks. Refreshing only one leaves the other stale.
    #[test]
    fn a_file_open_in_two_workspaces_is_reported_by_both() {
        let mut state = TabState::default();
        let path = PathBuf::from("replay.wowsreplay");
        state.live_workspace.open_replay_in_new_tab(open_replay("replay.wowsreplay"));

        let imported = WorkspaceId(1);
        let mut workspace = ReplayWorkspace::new(Some(PathBuf::from("live")));
        workspace.open_replay_in_new_tab(open_replay("replay.wowsreplay"));
        state.workspaces.insert(imported, workspace);

        assert_eq!(state.open_replays_at(&path).len(), 2, "both docks' copies of the file must be reported");
        assert!(state.open_replay_at(&path).is_some(), "the single-replay lookup still resolves the file");
        assert!(
            state.open_replays_at(Path::new("absent.wowsreplay")).is_empty(),
            "a file no dock has open reports nothing"
        );
    }

    /// One listing entry, with the shape a directory walk produces.
    fn listed_replay() -> Arc<crate::ui::replay_parser::ListedReplay> {
        Arc::new(crate::ui::replay_parser::ListedReplay {
            ship_id: None,
            map_name: "spaces".into(),
            game_type: "RandomBattle".into(),
            scenario: "Domination".into(),
            date_time: "28.07.2026 14:23:05".into(),
        })
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
        live_files.insert(path.clone(), listed_replay());
        state.live_workspace.replay_files = Some(live_files);

        let other_id = WorkspaceId(1);
        let mut other_workspace = ReplayWorkspace::new(None);
        let mut other_files = HashMap::new();
        other_files.insert(path.clone(), listed_replay());
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
            replays: paths.iter().map(|path| (PathBuf::from(path), listed_replay())).collect(),
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
        workspace.replay_files = Some(HashMap::from([(PathBuf::from("a.wowsreplay"), listed_replay())]));
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
            workspace.ingest_stage.clone(),
            Some(crate::task::replays::IngestStage::Reading(crate::task::replays::IngestProgress {
                done: 1,
                total: 4
            })),
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

    /// Batches merge into the listing rather than replacing it, so a replay
    /// deleted between two walks of the same directory would outlive its file.
    /// The list of what the walk found is what drops it, and only it.
    #[test]
    fn the_files_a_walk_found_drop_the_listing_entries_it_did_not_find() {
        let mut state = TabState::default();
        let id = WorkspaceId(1);
        let mut workspace = ReplayWorkspace::new(None);
        workspace.replay_files = Some(HashMap::from([
            (PathBuf::from("kept.wowsreplay"), listed_replay()),
            (PathBuf::from("deleted.wowsreplay"), listed_replay()),
        ]));
        state.workspaces.insert(id, workspace);

        state.apply_ingest_update(crate::task::replays::IngestUpdate::Walked {
            workspace: id,
            paths: HashSet::from([PathBuf::from("kept.wowsreplay")]),
        });

        let files = state.workspace(id).expect("inserted above").replay_files.as_ref().expect("set above");
        assert!(files.contains_key(Path::new("kept.wowsreplay")), "a file the walk found must stay listed");
        assert!(
            !files.contains_key(Path::new("deleted.wowsreplay")),
            "a file the walk no longer finds must leave the listing with it"
        );
    }

    /// The listing existing at all is what the UI reads as "this directory has
    /// been read", so the walk naming its files must not create one.
    #[test]
    fn the_files_a_walk_found_do_not_start_a_listing() {
        let mut state = TabState::default();
        let id = WorkspaceId(1);
        state.workspaces.insert(id, ReplayWorkspace::new(None));

        state.apply_ingest_update(crate::task::replays::IngestUpdate::Walked {
            workspace: id,
            paths: HashSet::from([PathBuf::from("a.wowsreplay")]),
        });

        assert!(
            state.workspace(id).expect("inserted above").replay_files.is_none(),
            "a walk that has listed nothing yet must not leave an empty listing behind"
        );
    }

    /// A run of files that all fail to read carries no replay, and the count
    /// still has to move: a frozen count next to a spinner is what a stalled
    /// walk looks like. It must move without disturbing what is listed.
    #[test]
    fn a_progress_update_moves_the_count_without_touching_the_listing() {
        let mut state = TabState::default();
        let id = WorkspaceId(1);
        let mut workspace = ReplayWorkspace::new(None);
        workspace.replay_files = Some(HashMap::from([(PathBuf::from("a.wowsreplay"), listed_replay())]));
        state.workspaces.insert(id, workspace);

        state.apply_ingest_update(crate::task::replays::IngestUpdate::Stage {
            workspace: id,
            stage: crate::task::replays::IngestStage::Reading(crate::task::replays::IngestProgress {
                done: 4_000,
                total: 5_000,
            }),
        });

        let workspace = state.workspace(id).expect("inserted above");
        assert_eq!(
            workspace.ingest_stage.clone(),
            Some(crate::task::replays::IngestStage::Reading(crate::task::replays::IngestProgress {
                done: 4_000,
                total: 5_000
            })),
            "progress with no replay to carry it must still reach the listing"
        );
        assert_eq!(
            workspace.replay_files.as_ref().map(|files| files.len()),
            Some(1),
            "progress must not disturb the replays already listed"
        );
    }

    /// A stage update names the workspace it belongs to, so one landing after its
    /// tab closed is dropped rather than reported on whichever listing is showing.
    #[test]
    fn a_stage_update_for_a_closed_workspace_is_dropped() {
        let mut state = TabState::default();
        let id = WorkspaceId(4242);

        state.apply_ingest_update(crate::task::replays::IngestUpdate::Stage {
            workspace: id,
            stage: crate::task::replays::IngestStage::Scanning(crate::task::replays::IngestProgress {
                done: 1,
                total: 2,
            }),
        });

        assert!(state.workspace(id).is_none(), "no workspace may be created by an update naming a closed one");
        assert!(state.live_workspace.ingest_stage.is_none(), "a departed workspace's stage must not land on live");
        assert!(state.workspaces.is_empty(), "a departed workspace must not be recreated by its own stage");
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

    /// The sticky wows-dir toast asks `ctx.memory(|m| m.focused())` about the
    /// field's stable id instead of caching a snapshot, precisely so that a
    /// frame in which the field never draws again cannot leave the gate
    /// engaged. This drives egui through the same `begin_pass`/closure/`end_pass`
    /// cycle eframe uses (`run_ui`) to prove the self-heal actually happens,
    /// rather than trusting the API docs describing it.
    #[test]
    fn focus_on_the_wows_dir_field_clears_once_it_stops_drawing() {
        let ctx = egui::Context::default();
        let id = TabState::wows_dir_field_id();

        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            let mut text = String::new();
            let response = ui.add(egui::TextEdit::singleline(&mut text).id(id));
            response.request_focus();
        });
        assert_eq!(ctx.memory(|m| m.focused()), Some(id), "the field must hold focus right after requesting it");

        // A later frame in which the Settings tab (and so this field) never draws,
        // e.g. because the user clicked another dock tab.
        let _ = ctx.run_ui(egui::RawInput::default(), |_ui| {});

        assert_eq!(
            ctx.memory(|m| m.focused()),
            None,
            "focus must clear on its own once the field stops drawing, with no help from settings_tab.rs"
        );
    }

    /// Three open workspaces over three unrelated directories, none of them the
    /// active one for the replay being opened. A lookup that returned the first
    /// workspace, the live workspace, or the active one would pick the wrong
    /// container for at least one of these, so all three are walked rather than
    /// one being spot-checked.
    fn three_directory_workspaces() -> (TabState, [WorkspaceId; 3]) {
        let mut state = TabState::default();
        state.live_workspace.root = Some(PathBuf::from("D:/live"));
        let a = state.open_directory_workspace(PathBuf::from("D:/archive/a"));
        let b = state.open_directory_workspace(PathBuf::from("D:/archive/b"));
        assert_ne!(a, b, "the fixture workspaces must be distinguishable");
        (state, [WorkspaceId::LIVE, a, b])
    }

    #[test]
    fn a_replay_resolves_to_the_workspace_whose_directory_contains_it() {
        let (state, [live, a, b]) = three_directory_workspaces();

        assert_eq!(state.workspace_for_replay_path(Path::new("D:/live/x.wowsreplay")), Some(live));
        assert_eq!(state.workspace_for_replay_path(Path::new("D:/archive/a/x.wowsreplay")), Some(a));
        assert_eq!(state.workspace_for_replay_path(Path::new("D:/archive/b/x.wowsreplay")), Some(b));
    }

    /// The containment is over the whole subtree, not just the immediate
    /// parent: a workspace's listing is built by a recursive walk of its root
    /// (`walk_replay_files` uses `WalkDir`), so a file two directories down is
    /// listed by it and belongs to it.
    #[test]
    fn a_replay_in_a_subdirectory_still_belongs_to_the_workspace_above_it() {
        let (state, [_, a, _]) = three_directory_workspaces();
        assert_eq!(state.workspace_for_replay_path(Path::new("D:/archive/a/2026/07/x.wowsreplay")), Some(a));
    }

    /// Roots nest: a subdirectory of the live replays directory can be imported
    /// as its own workspace, and then two listings both cover the file. The
    /// deeper one is the one actually showing it in context, so it wins.
    #[test]
    fn the_deepest_containing_root_wins_over_an_ancestor_root() {
        let mut state = TabState::default();
        state.live_workspace.root = Some(PathBuf::from("D:/replays"));
        let nested = state.open_directory_workspace(PathBuf::from("D:/replays/2026-07"));

        assert_eq!(
            state.workspace_for_replay_path(Path::new("D:/replays/2026-07/x.wowsreplay")),
            Some(nested),
            "the nested workspace lists the file more tightly than its ancestor"
        );
        assert_eq!(
            state.workspace_for_replay_path(Path::new("D:/replays/2026-06/x.wowsreplay")),
            Some(WorkspaceId::LIVE),
            "a sibling directory the nested workspace does not cover still belongs to the ancestor"
        );
    }

    /// A prefix match on the string would call `D:/archive/abc` a child of
    /// `D:/archive/a`. Path components are what nest, not characters.
    #[test]
    fn a_directory_that_merely_shares_a_name_prefix_does_not_own_the_replay() {
        let (mut state, [_, a, _]) = three_directory_workspaces();
        let sibling = state.open_directory_workspace(PathBuf::from("D:/archive/abc"));
        assert_ne!(sibling, a, "the fixture roots must be different workspaces");

        assert_eq!(state.workspace_for_replay_path(Path::new("D:/archive/abc/x.wowsreplay")), Some(sibling));
        assert_eq!(state.workspace_for_replay_path(Path::new("D:/archive/a/x.wowsreplay")), Some(a));
    }

    #[test]
    fn a_replay_under_no_open_root_resolves_to_no_workspace() {
        let (state, _) = three_directory_workspaces();
        assert_eq!(state.workspace_for_replay_path(Path::new("E:/elsewhere/x.wowsreplay")), None);
    }

    /// The choice for a replay nothing lists: its own directory becomes a
    /// workspace, and that fact is reported so the caller can scan it and say
    /// so. Adopting it into an already-open workspace would put it in a listing
    /// that does not contain it, and refusing would make an archive search
    /// unopenable on a fresh session.
    #[test]
    fn a_replay_under_no_open_root_opens_its_own_directory_as_a_workspace() {
        let (mut state, _) = three_directory_workspaces();
        let before = state.workspaces.len();

        let target = state.workspace_to_open_replay_in(Path::new("E:/elsewhere/x.wowsreplay"));

        let Some(SearchOpenTarget::Opened { id, root }) = target else {
            panic!("nothing listed the replay, so a workspace had to be opened: {target:?}");
        };
        assert_eq!(root, PathBuf::from("E:/elsewhere"), "the directory opened must be the replay's own");
        assert_eq!(state.workspaces.len(), before + 1, "exactly one workspace was opened");
        assert_eq!(
            state.workspace_for_replay_path(Path::new("E:/elsewhere/x.wowsreplay")),
            Some(id),
            "the workspace just opened must be the one that now owns the replay"
        );
    }

    /// The other side of that branch: a replay an open workspace already lists
    /// must not cause a second workspace to be opened for its directory.
    #[test]
    fn a_replay_an_open_workspace_lists_opens_nothing_new() {
        let (mut state, [_, a, _]) = three_directory_workspaces();
        let before = state.workspaces.len();

        let target = state.workspace_to_open_replay_in(Path::new("D:/archive/a/2026/x.wowsreplay"));

        assert_eq!(target, Some(SearchOpenTarget::Existing(a)));
        assert_eq!(state.workspaces.len(), before, "an already-listed replay must not open a workspace");
    }

    /// A bare filename's parent is the empty path, whose `starts_with` matches
    /// every path there is. Opening it as a root would produce a workspace that
    /// claims to contain every replay on the machine.
    #[test]
    fn a_bare_filename_opens_no_workspace_at_all() {
        let mut state = TabState::default();
        let before = state.workspaces.len();
        assert_eq!(state.workspace_to_open_replay_in(Path::new("x.wowsreplay")), None);
        assert_eq!(state.workspaces.len(), before, "a directoryless path must not open a workspace");
    }

    /// End to end over the two steps the search tab actually runs: resolve the
    /// owning workspace from the path, then put the replay in that workspace's
    /// dock. The tab has to land in the resolved workspace and nowhere else.
    #[test]
    fn opening_a_replay_from_search_puts_its_tab_in_the_workspace_that_owns_it() {
        let (mut state, [live, a, b]) = three_directory_workspaces();
        let path = Path::new("D:/archive/b/x.wowsreplay");

        let ws_id = state.workspace_for_replay_path(path).expect("an open workspace lists it");
        assert!(state.open_replay_in_new_workspace_tab(ws_id, open_replay("D:/archive/b/x.wowsreplay")));

        let tabs = |state: &TabState, id: WorkspaceId| {
            state.workspace(id).expect("open").replay_dock_state.iter_all_tabs().count()
        };
        assert_eq!(tabs(&state, b), 1, "the replay belongs to b's directory");
        assert_eq!(tabs(&state, a), 0, "no other workspace may receive it");
        assert_eq!(tabs(&state, live), 0, "the live workspace is not a fallback");
    }

    /// "A new sub-tab" was the request, and it holds even for a replay the
    /// workspace already has open: a second tab is how one replay gets put
    /// beside another, so this adds rather than focusing or replacing.
    #[test]
    fn opening_a_replay_that_is_already_open_adds_a_second_sub_tab() {
        let (mut state, [_, a, _]) = three_directory_workspaces();
        let path = "D:/archive/a/x.wowsreplay";

        state.open_replay_in_new_workspace_tab(a, open_replay(path));
        assert_eq!(state.workspace(a).expect("open").replay_dock_state.iter_all_tabs().count(), 1);

        state.open_replay_in_new_workspace_tab(a, open_replay(path));
        assert_eq!(
            state.workspace(a).expect("open").replay_dock_state.iter_all_tabs().count(),
            2,
            "the same replay opened twice must produce two sub-tabs, not replace the first"
        );
    }

    /// A workspace closed between the click and the load must not send the
    /// replay somewhere else. The live workspace is the tempting fallback, so
    /// it is the one checked.
    #[test]
    fn a_replay_for_a_closed_workspace_is_not_redirected_to_the_live_one() {
        let (mut state, _) = three_directory_workspaces();
        let closed = WorkspaceId(999);
        assert!(state.workspace(closed).is_none(), "the fixture id must actually be closed");

        assert!(!state.open_replay_in_new_workspace_tab(closed, open_replay("D:/gone/x.wowsreplay")));
        assert_eq!(
            state.live_workspace.replay_dock_state.iter_all_tabs().count(),
            0,
            "a replay for a closed workspace must not land in the live one"
        );
    }
}
