use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::Sender;
use std::time::Duration;

use notify::EventKind;
use notify::RecommendedWatcher;
use notify::RecursiveMode;
use notify::Watcher;
use notify::event::ModifyKind;
use notify::event::RenameMode;
use parking_lot::Mutex;
use parking_lot::RwLock;
use serde::Deserialize;
use serde::Serialize;
use tracing::debug;
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
use crate::ui::replay_parser::Replay;
use crate::ui::replay_parser::ReplayTab;
use crate::ui::replay_parser::SharedReplayParserTabState;
use crate::ui::replay_parser::SortOrder;
use crate::update_background_task;
use crate::util::personal_rating::PersonalRatingData;

pub type SharedToasts = Arc<parking_lot::Mutex<egui_notify::Toasts>>;

// ---------------------------------------------------------------------------
// Window settings persistence (modeled after egui_winit::WindowSettings)
// ---------------------------------------------------------------------------

/// Identifies which type of window settings to persist.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WindowKind {
    Main,
    ReplayRenderer,
    TacticsBoard,
    ArmorViewer,
}

/// Persisted window geometry, modeled after `egui_winit::WindowSettings`.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowSettings {
    pub inner_size_points: Option<[f32; 2]>,
    pub outer_position_pixels: Option<[f32; 2]>,
    pub fullscreen: bool,
    pub maximized: bool,
}

impl WindowSettings {
    /// Capture current viewport state from [`egui::ViewportInfo`].
    pub fn from_viewport_info(info: &egui::ViewportInfo) -> Self {
        Self {
            inner_size_points: info.inner_rect.map(|r| [r.width(), r.height()]),
            outer_position_pixels: info.outer_rect.map(|r| [r.left(), r.top()]),
            fullscreen: info.fullscreen.unwrap_or(false),
            maximized: info.maximized.unwrap_or(false),
        }
    }

    /// Apply these settings to a [`egui::ViewportBuilder`], falling back to
    /// `default_size` when no stored size is available.
    pub fn apply_to_builder(&self, builder: egui::ViewportBuilder, default_size: [f32; 2]) -> egui::ViewportBuilder {
        let size = self.inner_size_points.unwrap_or(default_size);
        let builder = builder.with_inner_size(size);
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

/// Sub-tab selection for the Stats tab
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
        (egui_dock::SurfaceIndex::main(), egui_dock::NodeIndex::root()),
        egui_dock::Split::Right,
        0.5,
        egui_dock::Node::leaf(StatsSubTab::Charts(0)),
    );
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
    /// Derived from `settings.game.wows_dir` — not persisted.
    pub replays_dir: Option<PathBuf>,
    pub unpacker_progress: Option<mpsc::Receiver<UnpackerProgress>>,
    pub last_progress: Option<UnpackerProgress>,
    pub replay_parser_tab: SharedReplayParserTabState,
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
    pub replay_files: Option<HashMap<PathBuf, Arc<RwLock<Replay>>>>,
    pub background_tasks: Vec<BackgroundTask>,
    pub toasts: SharedToasts,
    pub can_change_wows_dir: bool,
    pub replay_dock_state: egui_dock::DockState<ReplayTab>,
    pub next_replay_tab_id: u64,
    /// Whether the replay listing panel has been auto-sized to fit content.
    /// Reset when game state is cleared so the panel re-auto-sizes on next load.
    pub replay_listing_auto_sized: bool,
    pub twitch_update_sender: Option<tokio::sync::mpsc::Sender<crate::twitch::TwitchUpdate>>,
    pub twitch_state: Arc<RwLock<TwitchState>>,
    pub markdown_cache: egui_commonmark::CommonMarkCache,
    pub game_constants: Arc<RwLock<serde_json::Value>>,
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
        let default_constants = serde_json::from_str(include_str!("../../../embedded_resources/constants.json"))
            .expect("failed to parse constants JSON");
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
            replays_dir: None,
            unpacker_progress: Default::default(),
            last_progress: Default::default(),
            replay_parser_tab: Default::default(),
            file_viewer: Default::default(),
            replay_renderers: Default::default(),
            renderer_asset_cache: Default::default(),
            file_watcher: None,
            replay_files: None,
            file_receiver: None,
            background_tasks: Vec::new(),
            can_change_wows_dir: true,
            toasts: Arc::new(parking_lot::Mutex::new(egui_notify::Toasts::default())),
            replay_dock_state: egui_dock::DockState::new(vec![]),
            next_replay_tab_id: 0,
            replay_listing_auto_sized: false,
            twitch_update_sender: Default::default(),
            twitch_state: Default::default(),
            markdown_cache: Default::default(),
            game_constants: Arc::new(parking_lot::RwLock::new(default_constants)),
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

    /// Returns the replay shown in the currently focused (or first) replay dock tab, if any.
    pub fn focused_replay(&self) -> Option<Arc<RwLock<Replay>>> {
        // Try focused leaf first
        if let Some((si, ni)) = self.replay_dock_state.focused_leaf()
            && let Some(leaf) = self.replay_dock_state[si][ni].get_leaf()
            && let Some(tab) = leaf.tabs.get(leaf.active.0)
        {
            return Some(Arc::clone(&tab.replay));
        }
        // Fall back to the first tab in any leaf
        let (_, tab) = self.replay_dock_state.iter_all_tabs().next()?;
        Some(Arc::clone(&tab.replay))
    }

    /// Replace the focused tab's replay, or open a new tab if none exists.
    pub fn open_replay_in_focused_tab(&mut self, replay: Arc<RwLock<Replay>>) {
        // Try focused tab first
        if let Some((_rect, tab)) = self.replay_dock_state.find_active_focused() {
            tab.replay = replay;
            return;
        }
        // Fall back to the first tab in any leaf
        if let Some((_, tab)) = self.replay_dock_state.iter_all_tabs_mut().next() {
            tab.replay = replay;
            return;
        }
        self.open_replay_in_new_tab(replay);
    }

    /// Open a replay in a new dock tab.
    pub fn open_replay_in_new_tab(&mut self, replay: Arc<RwLock<Replay>>) {
        let id = self.next_replay_tab_id;
        self.next_replay_tab_id += 1;
        self.replay_dock_state.push_to_focused_leaf(ReplayTab { replay, id });
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
        })
    }

    /// Send a job to the background networking thread.
    pub fn send_network_job(&self, job: NetworkJob) {
        if let Some(tx) = &self.network_job_tx {
            let _ = tx.send(job);
        }
    }

    pub(crate) fn send_replay_consent_changed(&self) {
        let send = self.persisted.read().settings.integrations.send_replay_data;
        let _ = self
            .background_parser_tx
            .as_ref()
            .map(|tx| tx.send(ReplayBackgroundParserThreadMessage::ShouldSendReplaysToServer(send)));
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
                    // Build the replay while holding the read guard, then drop it
                    // before calling &mut self methods.
                    let new_replay = self.world_of_warships_data.as_ref().and_then(|wd| {
                        let wows_data = wd.read();
                        let game_metadata = wows_data.game_metadata.as_ref()?;
                        for _ in 0..3 {
                            if let Ok(replay_file) = ReplayFile::from_file(&new_file) {
                                let mut replay = Replay::new(replay_file, game_metadata.clone());
                                replay.game_constants = Some(Arc::clone(&wows_data.game_constants));
                                replay.source_path = Some(new_file.clone());
                                return Some(Arc::new(RwLock::new(replay)));
                            } else {
                                // oops our framerate
                                std::thread::sleep(Duration::from_secs(1));
                            }
                        }
                        None
                    });

                    if let Some(replay) = new_replay {
                        if let Some(replay_files) = &mut self.replay_files {
                            replay_files.insert(new_file.clone(), Arc::clone(&replay));
                        }

                        let source = if self.persisted.read().auto_load_latest_replay {
                            ReplaySource::AutoLoad
                        } else {
                            ReplaySource::SessionStatsOnly
                        };
                        if let Some(deps) = self.replay_dependencies() {
                            update_background_task!(self.background_tasks, deps.load_replay(replay, source));
                        }
                    }
                }
                NotifyFileEvent::Modified(modified_file) => {
                    // Invalidate cached data when file is modified
                    let replay_clone =
                        self.replay_files.as_ref().and_then(|files| files.get(&modified_file)).map(Arc::clone);

                    if let Some(replay) = replay_clone {
                        let mut replay_inner = replay.write();
                        replay_inner.battle_report = None;
                        replay_inner.ui_report = None;
                        drop(replay_inner);

                        let source = if self.persisted.read().auto_load_latest_replay {
                            ReplaySource::AutoLoad
                        } else {
                            ReplaySource::SessionStatsOnly
                        };
                        if let Some(deps) = self.replay_dependencies() {
                            update_background_task!(
                                self.background_tasks,
                                deps.load_replay(Arc::clone(&replay), source)
                            );
                        }
                    }
                }
                NotifyFileEvent::Removed(old_file) => {
                    if let Some(replay_files) = &mut self.replay_files {
                        replay_files.remove(&old_file);
                    }
                }
                NotifyFileEvent::PreferencesChanged => {
                    // debug!("Preferences file changed -- reloading game data");
                    // self.background_task = Some(self.load_game_data(self.settings.wows_dir.clone().into()));
                }
                NotifyFileEvent::TempArenaInfoCreated(path) => {
                    // Parse the metadata
                    let meta_data = std::fs::read(path);

                    if meta_data.is_err() {
                        return;
                    }

                    if let Ok(replay_file) = ReplayFile::from_decrypted_parts(meta_data.unwrap(), Vec::with_capacity(0))
                    {
                        self.player_tracker.write().update_from_live_arena_info(&replay_file.meta);
                    }
                }
            }
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

    /// Clears all game-related state. Called when the WoWs directory changes
    /// to ensure no stale data from the previous directory persists.
    pub(crate) fn reset_game_state(&mut self) {
        self.replay_dock_state = egui_dock::DockState::new(vec![]);
        self.next_replay_tab_id = 0;
        self.replay_files = None;
        self.replay_listing_auto_sized = false;
        self.browser_state = Default::default();
        {
            let mut p = self.persisted.write();
            p.session_stats.clear();
            p.chart_configs.clear();
        }
        self.replays_for_session_reset = None;
        self.clear_before_session_reset = true;
        self.replay_parser_tab.lock().game_chat.clear();
        self.file_viewer.lock().clear();
        self.replay_renderers.lock().clear();
        self.available_builds.clear();
        self.selected_browser_build = 0;
        self.wows_data_map = None;
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
                            ReplayLoader::new(deps, replay.clone()).source(ReplaySource::SessionStatsOnly).load()
                        );
                    }
                }
            }
        }

        // Focus the Overview sub-tab automatically
        let mut p = self.persisted.write();
        if let Some((surface, node, tab_idx)) = p.stats_dock_state.find_tab(&StatsSubTab::Overview) {
            p.stats_dock_state.set_active_tab((surface, node, tab_idx));
        }
    }

    pub(crate) fn update_wows_dir(&mut self, wows_dir: &Path, replay_dir: &Path) {
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
                should_send_replays: p.settings.integrations.send_replay_data,
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
            };
            drop(p);
            crate::task::start_background_parsing_thread(background_thread_data);
        }

        let mut watcher =
            match notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| match res {
                Ok(event) => {
                    // TODO: maybe properly handle moves?
                    debug!("filesytem event: {:?}", event);
                    match event.kind {
                        EventKind::Modify(ModifyKind::Name(RenameMode::To)) | EventKind::Create(_) => {
                            for path in event.paths {
                                if path.is_file() {
                                    if path.extension().map(|ext| ext == "wowsreplay").unwrap_or(false)
                                        && path.file_name().expect("path has no filename") != "temp.wowsreplay"
                                    {
                                        tx.send(NotifyFileEvent::Added(path.clone()))
                                            .expect("failed to send file creation event");
                                        // Send this path to the thread watching for replays in background
                                        let _ = background_tx
                                            .send(crate::task::ReplayBackgroundParserThreadMessage::NewReplay(path));
                                    } else if path.file_name().expect("path has no file name") == "tempArenaInfo.json" {
                                        tx.send(NotifyFileEvent::TempArenaInfoCreated(path.clone()))
                                            .expect("failed to send file creation event");
                                    }
                                }
                            }
                        }
                        EventKind::Modify(ModifyKind::Data(_)) => {
                            for path in event.paths {
                                if let Some(filename) = path.file_name()
                                    && filename == "preferences.xml"
                                {
                                    debug!("Sending preferences changed event");
                                    tx.send(NotifyFileEvent::PreferencesChanged)
                                        .expect("failed to send file creation event");
                                }
                                if path.extension().map(|ext| ext == "wowsreplay").unwrap_or(false) {
                                    tx.send(NotifyFileEvent::Modified(path.clone()))
                                        .expect("failed to send file modification event");
                                    let _ = background_tx
                                        .send(crate::task::ReplayBackgroundParserThreadMessage::ModifiedReplay(path));
                                }
                            }
                        }
                        EventKind::Remove(_) => {
                            for path in event.paths {
                                tx.send(NotifyFileEvent::Removed(path)).expect("failed to send file removal event");
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

        self.persisted.write().settings.game.wows_dir = wows_dir.to_str().unwrap().to_string();
        self.replays_dir = Some(replay_dir.to_owned());
        self.revalidate_wows_dir();
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
        let locale = self.persisted.read().settings.app.locale.clone().unwrap();
        let fallback_constants = self.game_constants.read().clone();
        let _join_handle = crate::util::thread::spawn_logged("load-game-data", move || {
            let _ = tx.send(crate::task::load_wows_files(wows_directory, locale.as_str(), &fallback_constants));
        });

        BackgroundTask { receiver: Some(rx), kind: BackgroundTaskKind::LoadingData }
    }
}
