use rust_i18n::t;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::TryRecvError;

use egui::Context;
use egui::KeyboardShortcut;
use egui::Modifiers;
use egui::OpenUrl;
use egui::RichText;
use egui::ScrollArea;
use egui::TextStyle;
use egui::Ui;
use egui::UiKind;
use egui::WidgetText;
use egui_commonmark::CommonMarkViewer;
use egui_dock::DockArea;
use egui_dock::DockState;
use egui_dock::TabStyle;
use egui_dock::TabViewer;

use octocrab::models::repos::Release;
use rootcause::Report;
use rootcause::hooks::builtin_hooks::report_formatter::DefaultReportFormatter;
use rootcause::prelude::ResultExt;
use tracing::debug;
use tracing::error;
use tracing::info;
use tracing::trace;
use tracing::warn;

use serde::Deserialize;
use serde::Serialize;

use tokio::runtime::Runtime;
use wows_data_mgr::download_repo::DownloadPlan;
use wows_data_mgr::download_repo::RemoteAvailability;
use wowsunpack::data::Version;

use crate::data::settings::DataSharingMode;
use crate::db::index::rows::SourceId;
use crate::db::index::rows::WorkspaceId;
use crate::icons;
use crate::tab_state::TabState;
use crate::task;
use crate::task::BackgroundTaskCompletion;
use crate::task::BackgroundTaskKind;
use crate::task::GameDataFollowUp;
use crate::task::NetworkJob;
use crate::task::NetworkResult;
use crate::task::PlanTicket;
use crate::task::ReplayBackgroundParserThreadMessage;
use crate::ui::file_unpacker::UNPACKER_STOP;
use crate::ui::theme::semantic::SemanticExt;
use crate::util::error::ToolkitError;

#[macro_export]
macro_rules! update_background_task {
    ($saved_tasks:expr, $background_task:expr) => {
        let task = $background_task;
        if let Some(task) = task {
            $saved_tasks.push(task);
        }
    };
}

#[allow(dead_code)]
#[derive(Clone, PartialEq, Eq)]
pub enum Tab {
    Unpacker,
    /// One replay listing, identified by which workspace it shows.
    Replays(WorkspaceId),
    Settings,
    PlayerTracker,
    ModManager,
    ArmorViewer,
    Stats,
    Search,
}

pub struct ToolkitTabViewer<'a> {
    pub tab_state: &'a mut TabState,
}

/// Which index source a `Tab::Replays` context-menu search covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TabSearchScope {
    /// Search this source, which holds exactly the tab's own replays.
    Source(SourceId),
    /// There is no source to scope by: an imported workspace's ingest has not
    /// reached `ensure_source`, or the indexer has not created the live source.
    /// The entry is drawn disabled, because searching the whole library when
    /// the user asked for one directory returns plausible-looking wrong results
    /// with nothing on screen to say so.
    Unresolved,
}

/// The scope a replay tab's search should use. The live workspace never
/// carries a source of its own -- the indexer owns the live source -- so it
/// reads `live`. Every other workspace reads only its own source, with no
/// fallback to `live`.
fn replay_tab_search_scope(
    id: WorkspaceId,
    workspace: Option<&crate::ui::replay_parser::ReplayWorkspace>,
    live: Option<SourceId>,
) -> TabSearchScope {
    let source = if id == WorkspaceId::LIVE { live } else { workspace.and_then(|workspace| workspace.source) };
    match source {
        Some(source) => TabSearchScope::Source(source),
        None => TabSearchScope::Unresolved,
    }
}

/// A query selecting every match indexed under `source` and nothing else.
fn source_scoped_query(source: SourceId) -> crate::db::index::query_model::Query {
    use crate::db::index::query_model::Chip;
    use crate::db::index::query_model::Connector;
    use crate::db::index::query_model::Field;
    use crate::db::index::query_model::Group;
    use crate::db::index::query_model::Op;
    use crate::db::index::query_model::Query;
    use crate::db::index::query_model::Value;

    Query {
        groups: vec![Group { chips: vec![Chip { field: Field::Group, op: Op::Is, value: Value::Source(source) }] }],
        connector: Connector::And,
    }
}

/// Draws the replay tab's search entry and returns its response, so both the
/// caller and a test can see whether it is enabled.
fn replay_search_menu_entry(ui: &mut Ui, scope: TabSearchScope) -> egui::Response {
    let enabled = matches!(scope, TabSearchScope::Source(_));
    let response = ui.add_enabled(enabled, egui::Button::new(t!("ui.tabs.search_these_replays")));
    if enabled {
        response
    } else {
        response.on_disabled_hover_text(t!("ui.tabs.search_these_replays_unavailable").into_owned())
    }
}

impl ToolkitTabViewer<'_> {
    /// The scope a search launched from the `Tab::Replays(id)` tab covers.
    fn resolve_tab_search_scope(&mut self, id: WorkspaceId) -> TabSearchScope {
        let live = self.tab_state.live_index_source();
        replay_tab_search_scope(id, self.tab_state.workspace(id), live)
    }

    /// Hand the Search tab a query covering `source` alone, and focus it. Both
    /// halves matter: the query without the focus leaves the user looking at
    /// the tab they right-clicked.
    fn search_replay_source(&mut self, source: SourceId) {
        self.tab_state.pending_search_query = Some(source_scoped_query(source));
        self.tab_state.pending_focus_search = true;
    }

    /// Builds a tab's title. A `Replays` tab needs `tab_state` to look up the
    /// workspace it names, so this lives on the viewer rather than on `Tab`
    /// itself. The live workspace's title is the same "Replay parser" label it
    /// has always had; a directory workspace is titled with its own shortened
    /// root, which is the only thing distinguishing two directory tabs from
    /// each other.
    fn tab_title(&self, tab: &Tab) -> String {
        use rust_i18n::t;
        if let Tab::Replays(id) = tab
            && *id != WorkspaceId::LIVE
        {
            // A tab outlives its workspace by a frame on the close path, and a
            // root can be empty, so neither the lookup nor the shortening is
            // allowed to be the only source of the title.
            let named = self
                .tab_state
                .workspace(*id)
                .and_then(|workspace| workspace.root.as_deref())
                .map(crate::ui::replay_parser::shorten_root)
                .filter(|title| !title.is_empty());
            let title = match named {
                Some(title) => title,
                None => t!("ui.tabs.replay_directory").into_owned(),
            };
            return wt_translations::icon_t(icons::FOLDER_OPEN, &title);
        }
        let (icon, key) = match tab {
            Tab::Unpacker => (icons::ARCHIVE, "ui.tabs.unpacker"),
            Tab::Settings => (icons::GEAR_FINE, "ui.tabs.settings"),
            Tab::Replays(_) => (icons::MAGNIFYING_GLASS, "ui.tabs.replay_parser"),
            Tab::PlayerTracker => (icons::DETECTIVE, "ui.tabs.player_tracker"),
            Tab::ModManager => (icons::WRENCH, "ui.tabs.mod_manager"),
            Tab::ArmorViewer => (icons::SHIELD, "ui.tabs.armor_viewer"),
            Tab::Stats => (icons::CHART_BAR, "ui.tabs.stats"),
            Tab::Search => (icons::MAGNIFYING_GLASS, "ui.tabs.search"),
        };
        wt_translations::icon_t(icon, &t!(key))
    }
}

impl TabViewer for ToolkitTabViewer<'_> {
    // This associated type is used to attach some data to each tab.
    type Tab = Tab;

    // Returns the current `tab`'s title.
    fn title(&mut self, tab: &mut Self::Tab) -> WidgetText {
        self.tab_title(tab).into()
    }

    // Defines the contents of a given `tab`.
    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        // The Settings tab caches its game-data cache-dir stats. The main dock
        // shows one tab per frame, so clearing here whenever another tab is
        // active makes reopening Settings recompute once instead of per frame.
        if !matches!(tab, Tab::Settings) {
            self.tab_state.game_data_cache_stats = None;
        }
        match tab {
            Tab::Unpacker => self.build_unpacker_tab(ui),
            Tab::Settings => self.build_settings_tab(ui),
            Tab::Replays(id) => self.build_replay_parser_tab(ui, *id),
            Tab::PlayerTracker => self.build_player_tracker_tab(ui),
            Tab::ModManager => self.build_mod_manager_tab(ui),
            Tab::ArmorViewer => self.build_armor_viewer_tab(ui),
            Tab::Stats => self.build_stats_tab(ui),
            Tab::Search => self.build_search_tab(ui),
        }
    }

    /// Right-clicking a replay tab offers a search over just that tab's own
    /// replays. egui_dock 0.20.1 calls this hook from its leaf tab-bar
    /// (`show/leaf.rs`) whenever `DockArea::tab_context_menus` is on, which it
    /// is by default and which this app leaves alone.
    fn context_menu(&mut self, ui: &mut Ui, tab: &mut Self::Tab, _path: egui_dock::NodePath) {
        let Tab::Replays(id) = *tab else {
            return;
        };
        let scope = self.resolve_tab_search_scope(id);
        if replay_search_menu_entry(ui, scope).clicked()
            && let TabSearchScope::Source(source) = scope
        {
            self.search_replay_source(source);
            ui.close();
        }
    }

    fn is_closeable(&self, tab: &Self::Tab) -> bool {
        match tab {
            Tab::Search => true,
            Tab::Replays(id) => *id != WorkspaceId::LIVE,
            Tab::Unpacker | Tab::Settings | Tab::PlayerTracker | Tab::ModManager | Tab::ArmorViewer | Tab::Stats => {
                false
            }
        }
    }

    // Closing a non-live `Replays` tab also drops its workspace. Runs twice
    // on egui_dock's context-menu close path (once from the menu action, once
    // from the deferred removal), so `close_workspace` must tolerate being
    // called again for an id it already removed.
    fn on_close(&mut self, tab: &mut Self::Tab) -> egui_dock::tab_viewer::OnCloseResponse {
        if let Tab::Replays(id) = tab
            && *id != WorkspaceId::LIVE
        {
            self.tab_state.close_workspace(*id);
        }
        egui_dock::tab_viewer::OnCloseResponse::Close
    }

    fn tab_style_override(&self, tab: &Self::Tab, global_style: &TabStyle) -> Option<TabStyle> {
        if matches!(tab, Tab::Settings) && self.tab_state.settings_needs_attention {
            let mut style = global_style.clone();
            let error = match self.tab_state.active_theme {
                egui::Theme::Dark => crate::ui::theme::semantic::DARK.error,
                egui::Theme::Light => crate::ui::theme::semantic::LIGHT.error,
            };
            let label = crate::ui::theme::contrast::label_on(error);
            style.active.bg_fill = error;
            style.active.text_color = label;
            style.inactive.bg_fill = error;
            style.inactive.text_color = label;
            style.focused.bg_fill = error;
            style.focused.text_color = label;
            style.hovered.bg_fill = error;
            style.hovered.text_color = label;
            style.active_with_kb_focus.bg_fill = error;
            style.active_with_kb_focus.text_color = label;
            style.inactive_with_kb_focus.bg_fill = error;
            style.inactive_with_kb_focus.text_color = label;
            style.focused_with_kb_focus.bg_fill = error;
            style.focused_with_kb_focus.text_color = label;
            Some(style)
        } else {
            None
        }
    }

    fn scroll_bars(&self, _tab: &Self::Tab) -> [bool; 2] {
        // egui_dock wraps every tab body in an outer ScrollArea by default. All
        // top-level tabs in this app build their own panels and add scroll
        // areas only where genuinely needed, so the outer wrapper just doubles
        // up and forces bars to appear when nothing actually overflows.
        [false, false]
    }
}

/// We derive Deserialize/Serialize so we can persist app state on shutdown.
#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct WowsToolkitApp {
    #[serde(skip)]
    checked_for_updates: bool,
    #[serde(skip)]
    manual_update_requested: bool,
    #[serde(skip)]
    update_window_open: bool,
    #[serde(skip)]
    panic_window_open: bool,
    #[serde(skip)]
    panic_info: Option<String>,
    #[serde(skip)]
    build_consent_window_open: bool,
    #[serde(skip)]
    replay_migration_window_open: bool,
    #[serde(skip)]
    language_selection_open: bool,
    #[serde(skip)]
    latest_release: Option<Release>,
    #[serde(skip)]
    show_about_window: bool,
    #[serde(skip)]
    show_error_window: bool,
    #[serde(skip)]
    error_to_show: Option<String>,

    #[serde(skip)]
    pub(crate) tab_state: TabState,
    #[serde(skip)]
    dock_state: DockState<Tab>,

    #[serde(skip)]
    pub(crate) runtime: Arc<Runtime>,

    /// Whether a constants/game version mismatch has been detected.
    #[serde(skip)]
    constants_version_mismatch: bool,
    /// Whether we've already shown a network error for constants updates
    /// (to avoid spamming the user on repeated failures).
    #[serde(skip)]
    constants_update_error_shown: bool,

    /// Whether we've already shown a toast for an invalid twitch token.
    #[serde(skip)]
    shown_twitch_token_error: bool,

    /// Receiver for results from the background networking thread.
    #[serde(skip)]
    pub(crate) network_result_rx: Option<std::sync::mpsc::Receiver<NetworkResult>>,

    /// Guard for the non-blocking log writer. Dropping this flushes remaining logs.
    #[cfg(feature = "logging")]
    #[serde(skip)]
    _log_guard: Option<tracing_appender::non_blocking::WorkerGuard>,

    /// Active realtime armor viewer windows spawned from replay renderers.
    #[serde(skip)]
    realtime_armor_viewers: Vec<Arc<parking_lot::Mutex<crate::replay::realtime_armor_viewer::RealtimeArmorViewer>>>,

    /// SQLite connection pool for persisting app state.
    #[serde(skip)]
    db_pool: Option<sqlx::SqlitePool>,

    /// Last observed `PersistedState::generation`, used to detect changes
    /// and notify the background save task.
    #[serde(skip)]
    last_persisted_generation: u64,

    /// Shutdown signal for the background save task. Dropping or sending
    /// triggers a final save before the task exits.
    #[serde(skip)]
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,

    /// Join handle for the background save task, used to await completion on exit.
    #[serde(skip)]
    save_task_handle: Option<tokio::task::JoinHandle<()>>,

    /// Constants data fetched from the network before game data was loaded.
    /// Flushed to disk once we know the build number (in `DataLoaded`).
    #[serde(skip)]
    pending_constants_data: Option<Vec<u8>>,

    /// Prompt asking whether to download game data for a build the user opened
    /// a replay for but does not have locally.
    #[serde(skip)]
    download_prompt: Option<GameDataDownloadPrompt>,

    /// Directory workspaces being brought up to date after a download, keyed
    /// by the workspace whose listing is short.
    #[serde(skip)]
    directory_reingest: BTreeMap<WorkspaceId, DirectoryReingest>,

    /// Scans whose read stage has not started, keyed by the workspace they
    /// describe. Retaining the scan is what lets a directory be walked once
    /// even when a download happens between the scan and the read.
    #[serde(skip)]
    pending_scans: HashMap<WorkspaceId, Box<crate::task::scan::DirectoryScan>>,

    /// Replay to reopen, released by the download task that was fetching its
    /// build. See [`WowsToolkitApp::finished_reingest_offer`] for why both of
    /// these are handed over rather than read from the task's own completion.
    #[serde(skip)]
    finished_download_replay: Option<PathBuf>,

    /// The builds the offer that started a re-walk showed, released by the walk
    /// that has just finished.
    ///
    /// Both handovers are written in the drain loop's kind dispatch, which runs
    /// for every finished task whatever its result, and read back by the
    /// completion arm in the same iteration. `handle_task_completion` returns
    /// early on a disconnected channel, so anything released only on the
    /// success path is stranded by a failure -- and a stranded `Walking` record
    /// silences the next explicit open's offer. Each is assigned on every task
    /// of its kind, so a failure leaves nothing behind for the next task's
    /// completion to pick up.
    #[serde(skip)]
    finished_reingest_offer: Option<(WorkspaceId, BTreeSet<u32>)>,
}

/// A directory workspace whose listing is waiting on downloaded game data.
enum DirectoryReingest {
    /// The download is still running. The whole selection goes out as a
    /// single task, so the walk is owed as soon as that one task finishes.
    AwaitingDownload { offered: BTreeSet<u32> },
    /// The download has been tried and the walk is owed, but has not started
    /// yet: the workspace can be mid-walk from a deliberate reopen, and a walk
    /// that could not be started must not be dropped.
    Owed { offered: BTreeSet<u32> },
    /// The walk is running.
    Walking { offered: BTreeSet<u32> },
}

impl DirectoryReingest {
    /// The builds the user was shown in the offer that led to this walk, so
    /// the walk does not re-raise the offer it came from.
    fn offered(&self) -> &BTreeSet<u32> {
        match self {
            Self::AwaitingDownload { offered } | Self::Owed { offered } | Self::Walking { offered } => offered,
        }
    }
}

/// One build the user is being offered a download of.
struct DownloadCandidate {
    build: u32,
    /// The replay's `major.minor.patch` version, used as a fallback hint when
    /// no exact build match is published.
    version: String,
    /// How many replays in the opened directory need this build. `None` when
    /// the offer came from a single replay, where a count says nothing.
    replays_needing: Option<usize>,
    /// What the remote has for this build, once the planner has been asked.
    /// `None` until then: nothing about a build can be claimed before the
    /// index and its metadata have actually been read.
    availability: Option<RemoteAvailability>,
    selected: bool,
}

impl DownloadCandidate {
    /// A candidate whose remote availability is known. Only an exact match is
    /// pre-selected: a nearest match may not load the replay that asked for
    /// it, and the other two states cannot be downloaded at all.
    fn new(build: u32, version: String, replays_needing: Option<usize>, availability: RemoteAvailability) -> Self {
        let selected = matches!(availability, RemoteAvailability::Exact);
        Self { build, version, replays_needing, availability: Some(availability), selected }
    }

    /// A candidate whose remote availability has not been resolved yet. Ticked
    /// so the first plan covers every build the directory asked for, and not
    /// selectable, so nothing can be downloaded on an unread claim.
    fn unresolved(build: u32, version: String, replays_needing: Option<usize>) -> Self {
        Self { build, version, replays_needing, availability: None, selected: true }
    }

    /// Whether the user can act on this row. An unresolved, unpublished or
    /// unreachable build has nothing that can be fetched for it.
    fn is_selectable(&self) -> bool {
        matches!(self.availability, Some(RemoteAvailability::Exact) | Some(RemoteAvailability::Nearest { .. }))
    }

    /// Whether this row is both ticked and something the remote can serve.
    fn is_downloadable(&self) -> bool {
        self.selected && self.is_selectable()
    }
}

/// Translation key describing what the remote has for a build. `Unreachable`
/// and `Unpublished` map to different keys on purpose: a fetch that failed and
/// data that was never published are different facts about the user's replays,
/// and rendering them alike states the wrong one.
const fn availability_key(availability: &RemoteAvailability) -> &'static str {
    match availability {
        RemoteAvailability::Exact => "ui.dialogs.download_availability_exact",
        RemoteAvailability::Nearest { .. } => "ui.dialogs.download_availability_nearest",
        RemoteAvailability::Unpublished => "ui.dialogs.download_availability_unpublished",
        RemoteAvailability::Unreachable => "ui.dialogs.download_availability_unreachable",
    }
}

/// The text shown in a candidate row's availability column.
fn availability_label(availability: &RemoteAvailability) -> String {
    let key = availability_key(availability);
    match availability {
        RemoteAvailability::Nearest { version, build } => t!(key, version = version, build = build).into_owned(),
        RemoteAvailability::Exact | RemoteAvailability::Unpublished | RemoteAvailability::Unreachable => {
            t!(key).into_owned()
        }
    }
}

/// Whether a walk's leftovers are just the offer that caused the walk coming
/// back around.
///
/// This is the only thing that ever suppresses the offer. `just_offered` is
/// `Some` only for the one automatic walk a download starts; every explicit
/// open -- the palette, a file dialog, a replay by name -- passes `None` and
/// always gets its offer, however many times the user has dismissed it before.
/// Dismissing is an answer to being asked, not a standing instruction to hide
/// data the user has just gone back and asked for again.
///
/// A subset rather than equality: the downloads that ran removed some builds
/// from `missing`, and what is left is the part of the same offer the user
/// chose not to fetch. A build that was not in the offer at all is new and
/// must be raised.
fn offer_was_just_made(just_offered: Option<&BTreeSet<u32>>, missing: &BTreeSet<u32>) -> bool {
    just_offered.is_some_and(|offered| missing.is_subset(offered))
}

/// How far along the planner is for the selection currently ticked.
enum DownloadPlanState {
    /// No plan requested yet for the current selection.
    Idle,
    /// A planning task is in flight for this selection.
    Planning,
    Ready(DownloadPlan),
    Failed(String),
}

/// A pending offer to download game data the toolkit does not have. Raised
/// either by opening one replay whose build is missing or by opening a
/// directory spanning builds; both land on this one dialog.
struct GameDataDownloadPrompt {
    candidates: Vec<DownloadCandidate>,
    /// What to redo once the downloads finish. `None` when there is nothing to
    /// redo, which is the case for a replay whose path the error did not carry.
    trigger: Option<GameDataFollowUp>,
    plan: DownloadPlanState,
    /// The selection `plan` describes. A plan is dispatched once per distinct
    /// selection rather than every frame, and a failed plan is not retried
    /// until the user changes what is ticked.
    planned_selection: Option<BTreeSet<u32>>,
    /// The planner run `plan` is waiting on. Selection equality cannot tell two
    /// offers over the same builds apart -- dismissing a directory's offer and
    /// reopening the same directory raises exactly that pair -- so the answer
    /// and the reaping are both matched on the ticket instead.
    planned_ticket: Option<PlanTicket>,
}

impl GameDataDownloadPrompt {
    fn new(candidates: Vec<DownloadCandidate>, trigger: Option<GameDataFollowUp>) -> Self {
        Self { candidates, trigger, plan: DownloadPlanState::Idle, planned_selection: None, planned_ticket: None }
    }

    /// Every build this offer covers, downloadable or not.
    fn offered_builds(&self) -> BTreeSet<u32> {
        self.candidates.iter().map(|c| c.build).collect()
    }

    /// The builds currently ticked, whether or not the planner has resolved
    /// them yet. This is what the object count is asked for.
    fn selected_builds(&self) -> BTreeSet<u32> {
        self.candidates.iter().filter(|c| c.selected).map(|c| c.build).collect()
    }

    /// The ticked builds the remote can actually serve, as download requests.
    fn downloadable(&self) -> Vec<(u32, String)> {
        self.candidates.iter().filter(|c| c.is_downloadable()).map(|c| (c.build, c.version.clone())).collect()
    }

    /// The planner's input for the current selection.
    fn plan_request(&self) -> Vec<(u32, Option<String>)> {
        self.candidates.iter().filter(|c| c.selected).map(|c| (c.build, Some(c.version.clone()))).collect()
    }

    /// Whether the plan on hand still describes what is ticked. A failed plan
    /// counts as describing its selection, so a planner that cannot reach the
    /// remote is not retried every frame.
    fn needs_plan(&self) -> bool {
        !matches!(self.plan, DownloadPlanState::Planning)
            && self.planned_selection.as_ref() != Some(&self.selected_builds())
    }

    /// Mark a plan as in flight for what is currently ticked, and return the
    /// planner's input alongside the ticket the answer must come back on.
    fn begin_planning(&mut self) -> (PlanTicket, Vec<(u32, Option<String>)>) {
        let ticket = PlanTicket::next();
        self.planned_selection = Some(self.selected_builds());
        self.planned_ticket = Some(ticket);
        self.plan = DownloadPlanState::Planning;
        (ticket, self.plan_request())
    }

    /// Fold a finished plan back into the rows.
    ///
    /// A row that already has an answer keeps it, along with its tick: that
    /// tick is the user's, and re-deriving it from a later plan would undo
    /// their choice. Only a row with no answer yet is filled in. A row whose
    /// answer the user cannot act on gets another chance through
    /// [`Self::retry`], which blanks it back to unresolved first.
    ///
    /// A plan from some other run is dropped. One offer can be dismissed and
    /// another raised while a planner is still running, and showing that
    /// planner's object count against the new selection would put a number on
    /// screen that answers a question nobody asked.
    ///
    /// Runs after [`Self::plan_task_finished`] on the completion path, so a
    /// delivered plan is what the dialog ends the frame showing.
    fn apply_plan(&mut self, ticket: PlanTicket, plan: DownloadPlan) {
        if self.planned_ticket != Some(ticket) {
            return;
        }
        for resolved in &plan.resolved {
            let matching = self.candidates.iter_mut().find(|c| {
                c.availability.is_none()
                    && c.build == resolved.requested_build
                    && resolved.requested_version.as_deref() == Some(c.version.as_str())
            });
            if let Some(candidate) = matching {
                *candidate = DownloadCandidate::new(
                    candidate.build,
                    candidate.version.clone(),
                    candidate.replays_needing,
                    resolved.availability.clone(),
                );
            }
        }
        self.plan = DownloadPlanState::Ready(plan);
    }

    /// Called when the planning task on `ticket` ends for any reason. A task
    /// that disconnected without sending a plan would otherwise leave the
    /// dialog showing a spinner and its Download button disabled forever.
    ///
    /// The ticket identifies the run: a planner started for an offer that has
    /// since been answered must not report failure against the offer now open.
    fn plan_task_finished(&mut self, ticket: PlanTicket) {
        if self.planned_ticket != Some(ticket) {
            return;
        }
        if matches!(self.plan, DownloadPlanState::Planning) {
            self.plan = DownloadPlanState::Failed(t!("ui.dialogs.download_plan_failed").into_owned());
        }
    }

    /// Whether there is anything a retry could improve: a plan that failed
    /// outright, or a row whose build the remote could not be asked about.
    fn can_retry(&self) -> bool {
        matches!(self.plan, DownloadPlanState::Failed(_))
            || self.candidates.iter().any(|c| matches!(c.availability, Some(RemoteAvailability::Unreachable)))
    }

    /// Ask the planner again. Rows the user cannot act on go back to
    /// unresolved, which re-ticks them so the retry covers them and the count
    /// it comes back with describes exactly the selection it was asked about.
    /// Rows the user can act on keep their answer and their tick.
    fn retry(&mut self) {
        for candidate in &mut self.candidates {
            if !candidate.is_selectable() {
                *candidate = DownloadCandidate::unresolved(
                    candidate.build,
                    candidate.version.clone(),
                    candidate.replays_needing,
                );
            }
        }
        self.plan = DownloadPlanState::Idle;
        self.planned_selection = None;
        self.planned_ticket = None;
    }
}

/// Targets whose events are written to the log file and the debug console.
///
/// `Targets` is an allowlist: any target not named here is dropped entirely, so
/// a crate whose diagnostics matter to a bug report has to be listed or the user
/// has nothing to send.
#[cfg(feature = "logging")]
fn log_targets() -> tracing_subscriber::filter::Targets {
    tracing_subscriber::filter::Targets::new()
        .with_target("wows_toolkit", tracing::Level::DEBUG)
        .with_target("wows_replay_insights", tracing::Level::DEBUG)
        .with_target("wows_replays", tracing::Level::INFO)
        .with_target(wows_data_mgr::LOG_TARGET, tracing::Level::INFO)
}

impl Default for WowsToolkitApp {
    fn default() -> Self {
        Self {
            checked_for_updates: false,
            manual_update_requested: false,
            update_window_open: false,
            panic_info: None,
            panic_window_open: false,
            build_consent_window_open: false,
            replay_migration_window_open: false,
            language_selection_open: false,
            latest_release: None,
            show_about_window: false,
            tab_state: Default::default(),
            dock_state: DockState::new(
                [
                    Tab::Replays(WorkspaceId::LIVE),
                    Tab::Stats,
                    Tab::PlayerTracker,
                    Tab::Search,
                    Tab::ArmorViewer,
                    Tab::Unpacker,
                    Tab::Settings,
                ]
                .to_vec(),
            ),
            show_error_window: false,
            error_to_show: None,
            constants_version_mismatch: false,
            constants_update_error_shown: false,
            shown_twitch_token_error: false,
            network_result_rx: None,
            runtime: Arc::new(Runtime::new().expect("failed to create tokio runtime")),
            #[cfg(feature = "logging")]
            _log_guard: None,
            realtime_armor_viewers: Vec::new(),
            last_persisted_generation: 0,
            db_pool: None,
            shutdown_tx: None,
            save_task_handle: None,
            download_prompt: None,
            directory_reingest: BTreeMap::new(),
            pending_scans: HashMap::new(),
            finished_download_replay: None,
            finished_reingest_offer: None,
            pending_constants_data: None,
        }
    }
}

impl WowsToolkitApp {
    /// Called once before the first frame.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // This is also where you can customize the look and feel of egui using
        // `cc.egui_ctx.set_visuals` and `cc.egui_ctx.set_fonts`.

        // Ensure the app data directory exists before anything tries to write to it.
        if let Some(dir) = crate::storage_dir() {
            let _ = std::fs::create_dir_all(&dir);
        }

        // Install the ring crypto provider for rustls before any networking happens.
        let _ = rustls::crypto::ring::default_provider().install_default();

        // Include phosphor icons
        let mut fonts = egui::FontDefinitions::default();
        egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
        egui_extras::install_image_loaders(&cc.egui_ctx);

        // TODO: Maybe at some point I want to use Berkeley Mono?
        // fonts.font_data.insert("bm".into(), egui::FontData::from_static(include_bytes!("../assets/BerkeleyMono-Regular.otf")).into());

        // if let Some(font_keys) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        //     font_keys.insert(0, "bm".into());
        // }
        // if let Some(font_keys) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
        //     font_keys.insert(0, "bm".into());
        // }

        // fonts.add_font(FontInsert::new(
        //     "bm",
        //     egui::FontData::from_static(include_bytes!("")),
        //     vec![
        //         InsertFontFamily { family: egui::FontFamily::Proportional, priority: egui::epaint::text::FontPriority::Highest },
        //         InsertFontFamily { family: egui::FontFamily::Monospace, priority: egui::epaint::text::FontPriority::Lowest },
        //     ],
        // ));

        // Add system font fallbacks for CJK/Thai characters that egui's default
        // fonts don't cover.
        add_system_font_fallbacks(&mut fonts);

        // Register "GameFont" as a proportional fallback so game_font() never panics.
        // Upgraded to real game fonts once WoWs data is loaded.
        crate::replay::minimap_view::shapes::register_game_fonts(&mut fonts, None);

        cc.egui_ctx.set_fonts(fonts);
        crate::ui::theme::install(&cc.egui_ctx);

        // Open SQLite database for persisting app state.
        let default_state: Self = Default::default();
        let db_pool = match default_state.runtime.block_on(crate::db::open_db()) {
            Ok(pool) => Some(pool),
            Err(e) => {
                error!("Failed to open database: {e}");
                None
            }
        };

        // Load previous app state.
        //
        // Priority:
        // 1. SQLite (if migration has been completed)
        // 2. app.ron via eframe (legacy) — then migrate to SQLite
        // 3. Fresh defaults
        let mut had_saved_state = false;
        let mut state = if let Some(ref pool) = db_pool
            && default_state.runtime.block_on(crate::db::is_migrated(pool))
        {
            // Load from SQLite.
            info!("Loading app state from SQLite");
            let mut saved_state: Self = Default::default();
            if let Err(e) =
                saved_state.runtime.block_on(crate::db::load::load_tab_state_from_db(pool, &mut saved_state.tab_state))
            {
                error!("Failed to load state from SQLite: {e}");
            } else {
                had_saved_state = true;
            }
            saved_state
        } else if let Some(legacy_app) = load_from_app_ron() {
            // Legacy: loaded from app.ron on disk — convert to new structure.
            had_saved_state = true;

            let (persisted, player_tracker, sent_replays, replay_sort) = legacy_app.into_new_state();
            let mut saved_state: Self = Default::default();
            *saved_state.tab_state.persisted.write() = persisted;
            saved_state.tab_state.player_tracker = player_tracker;
            saved_state.tab_state.sent_replays = sent_replays;
            saved_state.tab_state.replay_sort = replay_sort;

            // Migrate converted data to SQLite.
            if let Some(ref pool) = db_pool {
                info!("Migrating app.ron data to SQLite...");
                if let Err(e) = saved_state
                    .runtime
                    .block_on(crate::db::migrate_ron::migrate_tab_state_to_db(pool, &saved_state.tab_state))
                {
                    error!("Failed to migrate app.ron to SQLite: {e}");
                }
            }

            // Rename app.ron → app.ron.migrated as a backup.
            if let Some(dir) = crate::storage_dir() {
                let ron_path = dir.join("app.ron");
                let migrated_path = dir.join("app.ron.migrated");
                if ron_path.exists() && !migrated_path.exists() {
                    if let Err(e) = std::fs::rename(&ron_path, &migrated_path) {
                        warn!("Failed to rename app.ron to app.ron.migrated: {e}");
                    } else {
                        info!("Renamed app.ron to app.ron.migrated");
                    }
                }
            }

            saved_state
        } else {
            warn!("Creating new default app settings");
            Default::default()
        };

        // Store the DB pool in the app state.
        state.db_pool = db_pool;

        if had_saved_state {
            {
                let mut p = state.tab_state.persisted.write();
                if !p.settings.game.has_052_game_params_fix {
                    p.settings.game.has_052_game_params_fix = true;
                    crate::util::game_params::clear_all_game_params_caches();
                }

                // Apply persisted armor viewer defaults to the initial pane
                // (ArmorViewerState is #[serde(skip)] so it gets Default on load)
                state.tab_state.armor_viewer.apply_defaults(&p.armor_viewer_defaults);

                // Sync the GPU encoder warning flag from persisted settings
                state
                    .tab_state
                    .suppress_gpu_encoder_warning
                    .store(p.settings.app.suppress_gpu_encoder_warning, std::sync::atomic::Ordering::Relaxed);

                // Ensure session stats are sorted correctly (backfills sort_key for legacy data)
                p.session_stats.sort_games();
            }

            let wows_dir = state.tab_state.persisted.read().settings.game.wows_dir.clone();
            if !wows_dir.is_empty() {
                let task = Some(state.tab_state.load_game_data(PathBuf::from(wows_dir)));
                update_background_task!(state.tab_state.background_tasks, task);
            }
        }

        if !had_saved_state {
            let detected = sys_locale::get_locale()
                .and_then(|sys| wt_translations::system_locale_to_wows(&sys).map(String::from))
                .unwrap_or_else(|| "en".into());
            state.tab_state.persisted.write().settings.app.locale = Some(detected);

            let default_wows_dir = "C:\\Games\\World_of_Warships";
            let default_wows_path = Path::new(default_wows_dir);
            if default_wows_path.exists() {
                state.tab_state.persisted.write().settings.game.wows_dir = default_wows_dir.to_string();

                let task = state.tab_state.load_game_data(default_wows_path.to_path_buf());
                update_background_task!(state.tab_state.background_tasks, Some(task));
            }
        }

        // Restore zoom factor from persisted settings.
        cc.egui_ctx.set_zoom_factor(state.tab_state.persisted.read().settings.app.zoom_factor);

        // Restore theme choice from persisted settings.
        crate::ui::theme::apply(&cc.egui_ctx, state.tab_state.persisted.read().settings.app.theme);
        state.tab_state.active_theme = cc.egui_ctx.theme();

        // Apply locale to rust-i18n
        if let Some(locale) = &state.tab_state.persisted.read().settings.app.locale {
            rust_i18n::set_locale(locale);
        }

        // Check if the application panicked
        let panic_log_path = Self::panic_log_path();
        if panic_log_path.exists() {
            let mut file = File::open(panic_log_path).expect("failed to open panic log");
            let mut contents = String::new();
            file.read_to_string(&mut contents).expect("failed to read panic log");
            state.panic_info = Some(contents);
            state.panic_window_open = true;
        }

        {
            let p = state.tab_state.persisted.read();
            if !p.settings.app.build_consent_window_shown {
                state.build_consent_window_open = true;
            } else if !p.settings.app.replay_consent_prompt_shown {
                state.replay_migration_window_open = true;
            }

            // Show language selection dialog on first launch if a non-English locale was detected
            if !p.settings.app.language_selection_shown {
                let locale = p.settings.app.locale.as_deref().unwrap_or("en");
                if locale != "en" {
                    state.language_selection_open = true;
                } else {
                    drop(p);
                    // English detected or default — no need to ask
                    state.tab_state.persisted.write().settings.app.language_selection_shown = true;
                }
            }
        }

        // Initialize logging if the feature is enabled and the user hasn't disabled it
        #[cfg(feature = "logging")]
        if state.tab_state.persisted.read().settings.app.enable_logging {
            state._log_guard = Self::init_logging();
        }

        // Capture wgpu render state for 3D viewport rendering
        state.tab_state.wgpu_render_state = cc.wgpu_render_state.clone();

        // Share the tokio runtime and DB pool with tab_state for collab sessions and persistence.
        state.tab_state.tokio_runtime = Some(Arc::clone(&state.runtime));
        state.tab_state.db_pool = state.db_pool.clone();

        // Main window geometry is now restored via the ViewportBuilder in main.rs,
        // which is the only way to set window position. Size, fullscreen, and
        // maximized state are also applied there.

        // Load persisted cap layout cache.
        {
            let mut loaded = false;

            // Try SQLite first.
            if let Some(ref pool) = state.db_pool {
                let mut db = state.runtime.block_on(crate::data::cap_layout::CapLayoutDb::load_from_db(pool));
                if !db.is_empty() {
                    let removed = db.dedup();
                    if removed > 0 {
                        tracing::info!("removed {removed} duplicate cap layouts from SQLite");
                        let pool = pool.clone();
                        let _ = state.runtime.block_on(db.save_to_db(&pool));
                    }
                    *state.tab_state.cap_layout_db.lock() = db;
                    loaded = true;
                }
            }

            // Fall back to cap_layouts.bin file.
            if !loaded
                && let Some(cache_path) = crate::data::cap_layout::cache_path()
                && let Some(mut db) = crate::data::cap_layout::CapLayoutDb::load(&cache_path)
            {
                let removed = db.dedup();
                if removed > 0 {
                    tracing::info!("removed {removed} duplicate cap layouts from cache");
                    let _ = db.save(&cache_path);
                }
                tracing::info!("loaded {} cap layouts from cache", db.len());

                // Migrate file-based cap layouts to SQLite.
                if let Some(ref pool) = state.db_pool {
                    let pool = pool.clone();
                    if let Err(e) = state.runtime.block_on(db.save_to_db(&pool)) {
                        error!("Failed to migrate cap layouts to SQLite: {e}");
                    }
                }

                *state.tab_state.cap_layout_db.lock() = db;
            }
        }

        state.tab_state.revalidate_wows_dir();

        // Spawn the background save task (runs on a 30s timer, independent of painting).
        if let Some(ref pool) = state.db_pool {
            let save_ctx = crate::db::save::SaveContext {
                persisted: state.tab_state.persisted.clone(),
                player_tracker: state.tab_state.player_tracker.clone(),
                sent_replays: state.tab_state.sent_replays.clone(),
                replay_sort: state.tab_state.replay_sort.clone(),
                window_settings: state.tab_state.window_settings.clone(),
                active_viewports: state.tab_state.active_viewports.clone(),
                save_notify: state.tab_state.save_notify.clone(),
            };
            let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
            let handle = crate::db::save::spawn_save_task(
                &state.runtime,
                pool.clone(),
                save_ctx,
                cc.egui_ctx.clone(),
                shutdown_rx,
            );
            state.shutdown_tx = Some(shutdown_tx);
            state.save_task_handle = Some(handle);
        }

        let (tx, rx) = tokio::sync::mpsc::channel(1);
        state.tab_state.twitch_update_sender = Some(tx);
        state.begin_startup_tasks(rx);

        state
    }

    #[tracing::instrument(skip_all)]
    fn begin_startup_tasks(&mut self, token_rx: tokio::sync::mpsc::Receiver<crate::twitch::TwitchUpdate>) {
        use std::sync::Arc;

        // Start the networking thread
        let (network_job_tx, network_result_rx) = task::start_networking_thread();
        self.tab_state.network_job_tx = Some(network_job_tx);
        self.network_result_rx = Some(network_result_rx);

        let (twitch_channel, twitch_token) = {
            let p = self.tab_state.persisted.read();
            (p.settings.integrations.twitch_monitored_channel.clone(), p.settings.integrations.twitch_token.clone())
        };
        task::start_twitch_task(
            &self.runtime,
            Arc::clone(&self.tab_state.twitch_state),
            twitch_channel,
            twitch_token,
            token_rx,
            self.db_pool.clone(),
        );

        #[cfg(feature = "mod_manager")]
        update_background_task!(self.tab_state.background_tasks, Some(crate::mod_manager::load_mods_db()));

        // Migrate any pre-dedup game data caches and garbage-collect orphaned
        // content objects (e.g. left behind when a build directory was deleted
        // by hand). Runs off the UI thread so startup is never blocked.
        {
            let cache_dir = self.tab_state.persisted.read().settings.game.game_data_cache_dir.clone();
            if let Some(dump_base) = crate::task::replays::game_data_dump_base_with_override(&cache_dir) {
                crate::util::thread::spawn_logged("game-data-cache-maintenance", move || {
                    if !dump_base.exists() {
                        return;
                    }
                    match wows_data_mgr::dump::migrate_cas_dir_name(&dump_base) {
                        Ok(true) => tracing::info!("migrated game data cache from vfs_common/ to common/"),
                        Ok(false) => {}
                        Err(e) => tracing::warn!("game data cache rename migration failed: {e}"),
                    }
                    match wows_data_mgr::dump::migrate_to_cas(&dump_base) {
                        Ok(n) if n > 0 => tracing::info!("migrated {n} cached build(s) to deduplicated storage"),
                        Ok(_) => {}
                        Err(e) => tracing::warn!("game data cache migration failed: {e}"),
                    }
                    match wows_data_mgr::dump::gc_unreferenced(&dump_base) {
                        Ok(n) if n > 0 => tracing::info!("removed {n} orphaned game data object(s)"),
                        Ok(_) => {}
                        Err(e) => tracing::warn!("game data cache GC skipped: {e}"),
                    }
                });
            }
        }

        // Load PR expected values from disk if available
        let pr_path = crate::util::personal_rating::get_expected_values_path();
        if pr_path.exists() {
            if let Ok(pr_data) = std::fs::read(&pr_path) {
                update_background_task!(
                    self.tab_state.background_tasks,
                    Some(task::load_personal_rating_data(pr_data))
                );
            } else {
                tracing::error!("failed to read PR expected values file");
            }
        }
    }

    /// Initialize the tracing subscriber with file logging.
    #[cfg(feature = "logging")]
    fn init_logging() -> Option<tracing_appender::non_blocking::WorkerGuard> {
        use tracing_appender::rolling::Rotation;
        use tracing_subscriber::Layer;
        use tracing_subscriber::fmt;
        use tracing_subscriber::fmt::time::LocalTime;
        use tracing_subscriber::layer::SubscriberExt;

        let log_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| ".".into());
        let file_appender = tracing_appender::rolling::Builder::new()
            .rotation(Rotation::HOURLY)
            .max_log_files(3)
            .filename_prefix("wows_toolkit.log")
            .build(&log_dir)
            .ok()?;
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

        let subscriber = tracing_subscriber::registry().with(
            fmt::Layer::new()
                .with_writer(non_blocking)
                .with_timer(LocalTime::rfc_3339())
                .with_ansi(false)
                .with_target(true)
                .with_filter(log_targets()),
        );

        // In debug builds, also log to the console
        #[cfg(debug_assertions)]
        let subscriber =
            subscriber.with(fmt::Layer::new().with_ansi(true).with_target(true).with_filter(log_targets()));

        let _ = tracing::subscriber::set_global_default(subscriber);

        Some(guard)
    }

    pub fn build_bottom_panel(&mut self, ui: &mut Ui) {
        // Try to update mod update tasks
        if let Ok(new_task) = self.tab_state.background_task_receiver.try_recv() {
            self.tab_state.background_tasks.push(new_task);
        }

        if self.tab_state.persisted.read().settings.app.debug_mode {
            ui.label(RichText::new("⚠ Debug build ⚠").heading().color(ui.visuals().warn_fg_color));
        }

        ui.horizontal(|ui| {
            let mut remove_tasks = Vec::new();

            // Count pending LoadingReplay tasks so we can show a single consolidated indicator
            let pending_replay_count = self
                .tab_state
                .background_tasks
                .iter()
                .filter(|t| matches!(t.kind, BackgroundTaskKind::LoadingReplay) && t.receiver.is_some())
                .count();
            let mut shown_replay_spinner = false;

            // Consolidate concurrent game-data downloads (Update-all or Repair
            // across many builds) into a single status-bar entry with aggregate
            // progress, rather than one spinner per build.
            let mut downloading_count = 0usize;
            let mut download_done = 0u64;
            let mut download_total = 0u64;
            // Collected rather than applied in place: the workspaces these land
            // on live beside the tasks being borrowed here.
            let mut directories_downloading: Vec<(WorkspaceId, task::DownloadProgress)> = Vec::new();
            for task in &mut self.tab_state.background_tasks {
                if task.receiver.is_none() {
                    continue;
                }
                if let BackgroundTaskKind::DownloadingGameData { rx, last_progress, follow_up } = &mut task.kind {
                    while let Ok(progress) = rx.try_recv() {
                        *last_progress = Some(progress);
                    }
                    if let Some(progress) = last_progress {
                        download_done += progress.downloaded;
                        download_total += progress.total;
                        if let Some(GameDataFollowUp::Directory(workspace)) = follow_up {
                            directories_downloading.push((*workspace, *progress));
                        }
                    }
                    downloading_count += 1;
                }
            }
            // A directory whose read is waiting on this download reports the
            // download above its listing: the listing is short until it lands,
            // and the status bar does not say which directory is short.
            for (workspace, progress) in directories_downloading {
                self.mark_directory_downloading(workspace, progress);
            }
            let mut shown_download_progress = false;

            self.drain_ingest_updates();

            for i in 0..self.tab_state.background_tasks.len() {
                let task = &mut self.tab_state.background_tasks[i];

                let remove_task = {
                    // For LoadingReplay tasks, show one consolidated spinner instead of many
                    let desc = if matches!(task.kind, BackgroundTaskKind::LoadingReplay) && pending_replay_count > 1 {
                        if !shown_replay_spinner {
                            shown_replay_spinner = true;
                            ui.spinner();
                            ui.label(t!("ui.labels.loading_replays", count = pending_replay_count));
                        }
                        task.check_completion()
                    } else if matches!(task.kind, BackgroundTaskKind::DownloadingGameData { .. })
                        && downloading_count > 1
                    {
                        if !shown_download_progress {
                            shown_download_progress = true;
                            if download_total > 0 {
                                ui.add(
                                    egui::ProgressBar::new(download_done as f32 / download_total as f32)
                                        .text(t!("ui.messages.downloading_game_data")),
                                );
                            } else {
                                ui.spinner();
                                ui.label(t!("ui.messages.downloading_game_data"));
                            }
                        }
                        task.check_completion()
                    } else {
                        task.build_description(ui)
                    };
                    trace!("Task description: {:?}", desc);
                    if let Some(result) = desc {
                        match &task.kind {
                            BackgroundTaskKind::LoadingData => {
                                self.tab_state.allow_changing_wows_dir();
                            }
                            BackgroundTaskKind::LoadingBuildData(_) => {}
                            BackgroundTaskKind::LoadingReplay => {}
                            BackgroundTaskKind::Updating { rx: _rx, last_progress: _last_progress } => {}
                            BackgroundTaskKind::PopulatePlayerInspectorFromReplays => {}
                            #[cfg(feature = "mod_manager")]
                            BackgroundTaskKind::ModTask(_task_info) => {}
                            BackgroundTaskKind::LoadingPersonalRatingData => {}
                            BackgroundTaskKind::UpdateTimedMessage(toast) => {
                                let mut toasts = self.tab_state.toasts.lock();
                                match &toast.level {
                                    task::ToastLevel::Success => {
                                        toasts.success(toast.message.clone());
                                    }
                                    task::ToastLevel::Info => {
                                        toasts.info(toast.message.clone());
                                    }
                                    task::ToastLevel::Warning => {
                                        toasts.warning(toast.message.clone());
                                    }
                                    task::ToastLevel::Error => {
                                        toasts.error(toast.message.clone());
                                    }
                                };
                            }
                            BackgroundTaskKind::OpenFileViewer(plaintext_file_viewer) => {
                                self.tab_state.file_viewer.lock().push(plaintext_file_viewer.clone());
                            }
                            BackgroundTaskKind::BatchVideoExport { .. } => {}
                            // Released here rather than in the completion arm
                            // so a failed or disconnected download still frees
                            // the directory that was waiting on it, and leaves
                            // no replay behind for an unrelated download's
                            // completion to reopen.
                            BackgroundTaskKind::DownloadingGameData { follow_up, .. } => {
                                let follow_up = follow_up.clone();
                                self.finished_download_replay = None;
                                match follow_up {
                                    Some(GameDataFollowUp::Directory(workspace)) => {
                                        // The download owned the stage while it
                                        // ran; the read stage this owes takes
                                        // it back when it starts.
                                        self.tab_state.set_ingest_finished(workspace);
                                        self.note_reingest_download_finished(workspace);
                                    }
                                    Some(GameDataFollowUp::Replay(path)) => {
                                        self.finished_download_replay = Some(path);
                                    }
                                    None => {}
                                }
                            }
                            // Cleared here rather than in the completion arm so
                            // a disconnected planner cannot leave the dialog
                            // stuck showing a plan that will never arrive. A
                            // plan that did arrive is applied by the completion
                            // arm, which runs after this, and wins.
                            BackgroundTaskKind::PlanningGameDataDownload { ticket } => {
                                let ticket = *ticket;
                                if let Some(prompt) = &mut self.download_prompt {
                                    prompt.plan_task_finished(ticket);
                                }
                            }
                            BackgroundTaskKind::CheckingGameDataUpdates => {}
                            BackgroundTaskKind::ValidatingGameData { .. } => {}
                            BackgroundTaskKind::ReconcilingIndex { .. } => {}
                            BackgroundTaskKind::LoadingRowSummaries { workspace } => {
                                let workspace_id = *workspace;
                                if let Some(target) = self.tab_state.workspace_mut(workspace_id) {
                                    target.replay_row_summaries_loading = false;
                                }
                            }
                            // Released here rather than in the completion arm so
                            // an errored or disconnected read also frees the
                            // workspace for another attempt.
                            BackgroundTaskKind::IngestingDirectory { workspace, rx } => {
                                let workspace_id = *workspace;
                                // Whatever the walk sent between the drain above
                                // and now, before the receiver goes away with the
                                // task.
                                let tail: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
                                for update in tail {
                                    self.tab_state.apply_ingest_update(update);
                                }
                                self.tab_state.set_ingest_finished(workspace_id);
                            }
                            // The re-walk record is dropped here rather than in
                            // the completion arm so an errored or disconnected
                            // scan also releases it, which would otherwise mark
                            // the next explicit open as automatic and silence
                            // its offer. The scan is the stage that raises the
                            // offer, so it is the stage the record answers.
                            BackgroundTaskKind::ScanningDirectory { workspace, rx } => {
                                let workspace_id = *workspace;
                                let tail: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
                                for update in tail {
                                    self.tab_state.apply_ingest_update(update);
                                }
                                // The read stage that follows sets this again.
                                // Clearing it here is what lets a scan that
                                // failed be retried at all.
                                self.tab_state.set_ingest_finished(workspace_id);
                                let offered = self.finish_directory_reingest(workspace_id);
                                self.finished_reingest_offer = offered.map(|offered| (workspace_id, offered));
                            }
                        }

                        self.handle_task_completion(ui.ctx(), result);
                        true
                    } else {
                        false
                    }
                };

                if remove_task {
                    remove_tasks.push(i);
                }
            }

            // Remove whatever background tasks have yielded a result
            self.tab_state.background_tasks = self
                .tab_state
                .background_tasks
                .drain(..)
                .enumerate()
                .filter_map(|(i, task)| if remove_tasks.contains(&i) { None } else { Some(task) })
                .collect();

            if let Some(rx) = &self.tab_state.unpacker_progress {
                if ui.button(t!("ui.buttons.stop")).clicked() {
                    UNPACKER_STOP.store(true, Ordering::Relaxed);
                }
                let mut done = false;
                loop {
                    match rx.try_recv() {
                        Ok(progress) => {
                            self.tab_state.last_progress = Some(progress);
                        }
                        Err(TryRecvError::Empty) => {
                            if let Some(last_progress) = self.tab_state.last_progress.as_ref() {
                                ui.add(
                                    egui::ProgressBar::new(last_progress.progress)
                                        .text(last_progress.file_name.as_str()),
                                );
                            }
                            break;
                        }
                        Err(TryRecvError::Disconnected) => {
                            done = true;
                            break;
                        }
                    }
                }

                if done {
                    self.tab_state.unpacker_progress.take();
                    self.tab_state.last_progress.take();
                }
            }
        });
    }

    /// Move whatever every running directory walk has sent since the last frame
    /// into the listings those walks belong to.
    ///
    /// Collected before anything is applied so the tasks are done being borrowed
    /// by the time the workspaces they name are touched.
    fn drain_ingest_updates(&mut self) {
        let mut updates = Vec::new();
        for task in &self.tab_state.background_tasks {
            match &task.kind {
                BackgroundTaskKind::IngestingDirectory { rx, .. }
                | BackgroundTaskKind::ScanningDirectory { rx, .. } => {
                    updates.extend(std::iter::from_fn(|| rx.try_recv().ok()));
                }
                _ => {}
            }
        }
        for update in updates {
            self.tab_state.apply_ingest_update(update);
        }
    }

    /// Send all startup network checks to the background networking thread (non-blocking).
    fn request_update_checks(&mut self) {
        self.tab_state.send_network_job(NetworkJob::CheckForAppUpdates);
        self.tab_state.send_network_job(NetworkJob::FetchLatestConstants {
            current_commit: self.tab_state.persisted.read().settings.game.constants_file_commit.clone(),
        });
        if crate::util::personal_rating::needs_update() {
            self.tab_state.send_network_job(NetworkJob::FetchPersonalRatingData);
        }
        self.checked_for_updates = true;
    }

    /// Poll the networking thread for results and handle them.
    fn poll_network_results(&mut self) {
        let mut check_version_mismatch = false;

        let Some(rx) = &self.network_result_rx else {
            return;
        };
        while let Ok(result) = rx.try_recv() {
            match result {
                NetworkResult::AppUpdateAvailable(release) => {
                    self.update_window_open = true;
                    self.latest_release = Some(*release);
                }
                NetworkResult::AppUpToDate => {
                    self.tab_state.toasts.lock().success(t!("ui.messages.app_up_to_date"));
                }
                NetworkResult::AppUpdateCheckFailed(msg) => {
                    warn!("App update check failed: {}", msg);
                    self.tab_state.toasts.lock().error(t!("ui.messages.update_check_failed"));
                }
                NetworkResult::ConstantsFetched { data, commit } => {
                    // Save under the current build number so the versioned system finds it.
                    // If game data hasn't loaded yet, stash for later (DataLoaded will flush it).
                    if let Some(wows_data) = &self.tab_state.world_of_warships_data {
                        let build = wows_data.read().build_number;
                        if let Some(storage_dir) = crate::storage_dir() {
                            let path = storage_dir.join(format!("constants_{build}.json"));
                            let _ = std::fs::write(path, data.as_slice());
                        }
                        // Rebuild loaded data with the new constants from disk.
                        if wows_data.write().rebuild_with_new_constants() {
                            for replay in self.tab_state.all_open_replays() {
                                replay.write().ui_report = None;
                            }
                        }
                    } else {
                        self.pending_constants_data = Some(data);
                    }
                    self.tab_state.persisted.write().settings.game.constants_file_commit = commit;
                    check_version_mismatch = true;
                }
                NetworkResult::ConstantsUpToDate => {}
                NetworkResult::ConstantsFetchFailed(msg) => {
                    warn!("Constants fetch failed: {}", msg);
                    if !self.constants_update_error_shown {
                        self.constants_update_error_shown = true;
                        self.tab_state.toasts.lock().error(t!("ui.messages.constants_fetch_failed")).duration(None);
                    }
                }
                NetworkResult::PersonalRatingDataFetched(data) => {
                    if crate::util::personal_rating::save_expected_values(&data).is_ok() {
                        update_background_task!(
                            self.tab_state.background_tasks,
                            Some(task::load_personal_rating_data(data))
                        );
                    }
                }
                NetworkResult::PersonalRatingDataFetchFailed(msg) => {
                    warn!("PR data fetch failed: {}", msg);
                }
                NetworkResult::VersionedConstantsFetched { build } => {
                    // Versioned constants were downloaded and saved to disk.
                    // If we have this build loaded with inexact constants, rebuild it.
                    if let Some(wows_data_map) = self.tab_state.wows_data_map.as_ref()
                        && let Some(data) = wows_data_map.get(build)
                        && !data.read().replay_constants_exact_match
                    {
                        debug!("Rebuilding build {} with newly fetched versioned constants", build);
                        if data.write().rebuild_with_new_constants() {
                            // Invalidate cached reports so they rebuild with correct constants
                            for replay in self.tab_state.all_open_replays() {
                                replay.write().ui_report = None;
                            }
                        }
                    }

                    // Copy fetched constants into the dump directory if it exists
                    if let Some(constants) = crate::task::networking::load_versioned_constants_from_disk(build) {
                        let cache_dir = self.tab_state.persisted.read().settings.game.game_data_cache_dir.clone();
                        if let Some(dump_base) = crate::task::replays::game_data_dump_base_with_override(&cache_dir) {
                            let builds_index = wows_data_mgr::builds::BuildsIndex::load(&dump_base.join("builds.toml"));
                            if let Some(entry) = builds_index.find_by_build(build) {
                                let dest = dump_base.join(&entry.dir).join("constants.json");
                                if !dest.exists()
                                    && let Ok(bytes) = serde_json::to_vec_pretty(&constants)
                                {
                                    let _ = std::fs::write(&dest, bytes);
                                    debug!("Copied constants.json to dump for build {build}");
                                }
                            }
                        }
                    }
                }
                NetworkResult::VersionedConstantsFetchFailed { build, msg } => {
                    warn!("Versioned constants fetch failed for build {}: {}", build, msg);
                }
            }
        }

        if check_version_mismatch {
            self.check_constants_version_mismatch();
        }
    }

    /// Handle a completed background task result.
    fn handle_task_completion(&mut self, ctx: &egui::Context, result: Result<BackgroundTaskCompletion, Report>) {
        match result {
            Ok(data) => match data {
                BackgroundTaskCompletion::NoReceiver => {}
                BackgroundTaskCompletion::DataLoaded { new_dir, wows_data, replays, available_builds } => {
                    let replays_dir = wows_data.replays_dir.clone();
                    let build_number = wows_data.build_number;

                    // Detect if the WoWs directory changed
                    let dir_changed =
                        self.tab_state.persisted.read().settings.game.wows_dir != new_dir.to_str().unwrap_or_default();

                    // Clear all stale game state when directory changes
                    if dir_changed {
                        self.tab_state.reset_game_state();
                    }

                    if let Some(old_wows_data) = &self.tab_state.world_of_warships_data {
                        *old_wows_data.write() = wows_data;
                    } else {
                        let wows_data = Arc::new(parking_lot::RwLock::new(wows_data));
                        self.tab_state.world_of_warships_data = Some(Arc::clone(&wows_data));

                        #[cfg(feature = "mod_manager")]
                        crate::mod_manager::start_mod_manager_thread(
                            Arc::clone(&self.runtime),
                            wows_data,
                            self.tab_state.mod_action_receiver.take().unwrap(),
                            self.tab_state.background_task_sender.clone(),
                        );
                    }

                    // Register real game fonts from VFS now that data is available.
                    {
                        let wdata = self.tab_state.world_of_warships_data.as_ref().unwrap().read();
                        let gf = self.tab_state.renderer_asset_cache.lock().get_or_load_game_fonts(
                            &wdata.vfs,
                            wdata.version(),
                            wdata.dump_dir.as_deref(),
                        );
                        let mut font_defs = ctx.fonts(|r| r.definitions().clone());
                        crate::replay::minimap_view::shapes::register_game_fonts(&mut font_defs, Some(&gf));
                        ctx.set_fonts(font_defs);
                    }

                    // Initialize or update the version data map.
                    // Always create a new map when the directory changed
                    // (reset_game_state sets wows_data_map to None).
                    let wows_data_ref = self.tab_state.world_of_warships_data.as_ref().unwrap();
                    if let Some(map) = &self.tab_state.wows_data_map {
                        map.insert_main(build_number, Arc::clone(wows_data_ref));
                    } else {
                        let (locale, cache_dir) = {
                            let persisted = self.tab_state.persisted.read();
                            (
                                persisted.settings.app.locale.clone().unwrap_or_else(|| "en".to_string()),
                                persisted.settings.game.game_data_cache_dir.clone(),
                            )
                        };
                        let mut map =
                            crate::data::wows_data::WoWsDataMap::new(PathBuf::from(&new_dir), locale, cache_dir);
                        if let Some(tx) = self.tab_state.network_job_tx.clone() {
                            map.set_network_job_tx(tx);
                        }
                        map.insert_main(build_number, Arc::clone(wows_data_ref));
                        self.tab_state.wows_data_map = Some(map);
                    }

                    // If the initial build used fallback constants, request the correct version
                    if !wows_data_ref.read().replay_constants_exact_match {
                        let version = wows_data_ref
                            .read()
                            .full_version
                            .as_ref()
                            .map(|v| format!("{}.{}.{}", v.major, v.minor, v.patch));
                        self.tab_state
                            .send_network_job(NetworkJob::FetchVersionedConstants { build: build_number, version });
                    }

                    // Flush any constants data that arrived from the network before
                    // we knew the build number.
                    if let Some(data) = self.pending_constants_data.take()
                        && let Some(storage_dir) = crate::storage_dir()
                    {
                        let path = storage_dir.join(format!("constants_{build_number}.json"));
                        let _ = std::fs::write(path, &data);
                    }

                    self.tab_state.available_builds = available_builds;
                    self.tab_state.selected_browser_build = build_number;

                    self.tab_state.update_wows_dir(&new_dir, &replays_dir);
                    let no_replays = replays.as_ref().is_none_or(|r| r.is_empty());
                    // `replays` was loaded from the game's own replays directory,
                    // so it belongs to the live workspace regardless of which one
                    // is active.
                    self.tab_state.live_workspace.replay_files = replays;
                    self.tab_state.browser_state.reset_filters();

                    self.tab_state.toasts.lock().success(t!("ui.messages.game_data_loaded"));

                    if no_replays {
                        self.tab_state.toasts.lock().warning(t!("ui.messages.no_replays_detected"));
                    }

                    self.check_constants_version_mismatch();
                }
                BackgroundTaskCompletion::BuildDataLoaded { build } => {
                    self.tab_state.selected_browser_build = build;
                    self.tab_state.browser_state.reset_filters();
                    self.tab_state.toasts.lock().success(t!("ui.messages.build_loaded", build = build));
                }
                BackgroundTaskCompletion::GameDataDownloaded { downloaded, failures } => {
                    for (requested_build, build) in &downloaded {
                        self.tab_state.toasts.lock().success(t!("ui.messages.game_data_downloaded", build = build));

                        // The data that was missing is now on disk, so an
                        // earlier failure to find it says nothing about this
                        // build any more. The requested build is cleared too: a
                        // fallback build can be what makes it resolvable.
                        if let Some(map) = &self.tab_state.wows_data_map {
                            map.forget_unresolvable_build(*build);
                            map.forget_unresolvable_build(*requested_build);
                        }
                        crate::ui::replay_parser::forget_fire_section_failures(*build);
                        crate::ui::replay_parser::forget_fire_section_failures(*requested_build);

                        if requested_build != build {
                            debug!("downloaded build {build} as a fallback for requested build {requested_build}");
                        }
                    }

                    for build in &failures {
                        self.tab_state
                            .toasts
                            .lock()
                            .error(t!("ui.messages.game_data_build_download_failed", build = build));
                    }

                    // Reopen the replay that triggered the download. Attempted
                    // whenever anything landed: the replay's own build may have
                    // been served by a fallback under another request.
                    if !downloaded.is_empty()
                        && let Some(path) = self.finished_download_replay.take()
                        && let Some(deps) = self.tab_state.replay_dependencies()
                    {
                        update_background_task!(
                            self.tab_state.background_tasks,
                            deps.parse_replay_from_path(path, crate::task::ReplaySource::ManualOpen)
                        );
                    }
                }
                BackgroundTaskCompletion::GameDataDownloadPlanned { ticket, plan } => {
                    if let Some(prompt) = &mut self.download_prompt {
                        prompt.apply_plan(ticket, plan);
                    }
                }
                BackgroundTaskCompletion::GameDataUpdatesChecked { tip, updates } => {
                    self.tab_state.checking_game_data_updates = false;
                    if updates.is_empty() {
                        // Everything is current; record the tip so the next check
                        // can short-circuit without per-build requests.
                        self.tab_state.persisted.write().settings.game.game_data_repo_commit = Some(tip);
                        self.tab_state.game_data_updates.clear();
                        self.tab_state.toasts.lock().success(t!("ui.messages.game_data_up_to_date"));
                    } else {
                        // Leave the stored commit untouched until the user updates,
                        // so a later check re-detects these builds.
                        let count = updates.len();
                        self.tab_state.game_data_updates = updates;
                        self.tab_state.toasts.lock().info(t!("ui.messages.game_data_updates_available", count = count));
                    }
                }
                BackgroundTaskCompletion::GameDataValidated { tip, builds } => {
                    use wows_data_mgr::download_repo::ValidationOutcome;

                    self.tab_state.validating_game_data_cache = false;
                    let repair: Vec<wows_data_mgr::download_repo::BuildUpdateStatus> = builds
                        .iter()
                        .filter(|b| matches!(b.outcome, ValidationOutcome::NeedsRepair(_)))
                        .map(|b| wows_data_mgr::download_repo::BuildUpdateStatus {
                            build: b.build,
                            version: b.version.clone(),
                        })
                        .collect();

                    if repair.is_empty() {
                        // Every cached build matches the remote repo; record the
                        // tip so a later update check can short-circuit.
                        self.tab_state.persisted.write().settings.game.game_data_repo_commit = Some(tip);
                        self.tab_state.game_data_repair.clear();
                        self.tab_state.toasts.lock().success(t!("ui.messages.game_data_cache_valid"));
                    } else {
                        let count = repair.len();
                        self.tab_state.game_data_repair = repair;
                        self.tab_state.toasts.lock().warning(t!("ui.messages.game_data_cache_invalid", count = count));
                    }
                }
                BackgroundTaskCompletion::ReplayLoaded { replay, source } => {
                    use crate::task::ReplaySource;

                    let track_session_stats = matches!(
                        source,
                        ReplaySource::FileListing
                            | ReplaySource::AutoLoad
                            | ReplaySource::Reload
                            | ReplaySource::SessionStatsOnly
                    );
                    let update_ui = !matches!(source, ReplaySource::SessionStatsOnly);
                    let open_tab =
                        matches!(source, ReplaySource::ManualOpen | ReplaySource::AutoLoad | ReplaySource::Reload);
                    // Jump the outer dock to the replay tab only on an explicit open
                    // (search result, palette, file open), not on passive auto-loads.
                    let focus_replay_tab = matches!(source, ReplaySource::ManualOpen);

                    // A replay newly written by the game (the watcher's Added
                    // path) is read and built on the background thread; add it to
                    // the listing here so it appears without a full rescan. The
                    // most recently completed read wins, so a re-read triggered by
                    // a later modification replaces an earlier, staler parse. The
                    // watcher only observes the live replays directory, so this
                    // always belongs to the live workspace.
                    if matches!(source, ReplaySource::AutoLoad | ReplaySource::SessionStatsOnly) {
                        let listed = {
                            let guard = replay.read();
                            guard.source_path.clone().map(|path| {
                                (path, crate::ui::replay_parser::ListedReplay::from_meta(&guard.replay_file.meta))
                            })
                        };
                        if let Some((path, listed)) = listed
                            && let Some(replay_files) = &mut self.tab_state.live_workspace.replay_files
                        {
                            replay_files.insert(path, Arc::new(listed));
                        }
                    }

                    if track_session_stats {
                        let replay_guard = replay.read();
                        if let Some(stat) = crate::data::session_stats::PerGameStat::from_replay(
                            &replay_guard,
                            &replay_guard.resource_loader,
                        ) {
                            self.tab_state.persisted.write().session_stats.add_game(stat);
                        }
                        drop(replay_guard);
                    }
                    if update_ui {
                        self.tab_state.player_tracker.write().update_from_replay(&replay.read());
                        if open_tab {
                            self.tab_state.open_replay_in_focused_tab(replay);
                            if focus_replay_tab {
                                self.focus_tab(&Tab::Replays(self.tab_state.active_workspace_id()));
                            }
                        }
                        self.tab_state.toasts.lock().success(t!("ui.messages.replay_loaded"));
                        self.try_update_constants();
                    }
                }
                BackgroundTaskCompletion::UpdateDownloaded(new_exe) => {
                    let current_process = std::env::current_exe().expect("current process has no path?");
                    let mut current_process_new_path = current_process.as_os_str().to_owned();
                    current_process_new_path.push(".old");
                    let current_process_new_path = PathBuf::from(current_process_new_path);
                    let rename_process = move || {
                        std::fs::rename(current_process.clone(), &current_process_new_path)
                            .context("failed to rename current process")?;
                        std::fs::rename(new_exe, &current_process).context("failed to rename new process")?;

                        std::process::Command::new(current_process)
                            .arg(current_process_new_path)
                            .spawn()
                            .context("failed to execute updated process")
                    };

                    match rename_process() {
                        Ok(_) => {
                            std::process::exit(0);
                        }
                        Err(e) => {
                            error!("Update rename failed: {e:?}");
                            self.show_err_window(e.into());
                        }
                    }
                }
                BackgroundTaskCompletion::PopulatePlayerInspectorFromReplays => {
                    // Switch to "All Time" so historical data is visible
                    self.tab_state.player_tracker.write().filter_time_period =
                        crate::ui::player_tracker::TimePeriod::AllTime;
                }
                BackgroundTaskCompletion::PersonalRatingDataLoaded(pr_data) => {
                    self.tab_state.personal_rating_data.write().load(pr_data);
                }
                BackgroundTaskCompletion::ReconcileIndexComplete { indexed, total } => {
                    if indexed > 0 {
                        self.tab_state.toasts.lock().success(t!(
                            "ui.messages.replays_indexed",
                            indexed = indexed,
                            total = total
                        ));
                    } else {
                        self.tab_state.toasts.lock().info(t!("ui.messages.replays_already_indexed", total = total));
                    }
                }
                BackgroundTaskCompletion::DirectoryIngested { workspace, source, failures } => {
                    self.tab_state.set_ingest_finished(workspace);

                    // A workspace that is gone was closed while the walk ran,
                    // and the result belongs to nothing else: carrying the id
                    // is what lets this be dropped instead of landing on
                    // whichever listing happens to be showing.
                    if let Some(target) = self.tab_state.workspace_mut(workspace) {
                        target.source = Some(source);
                        // A directory with nothing in it sends no batches, so
                        // the listing is started here rather than left unset,
                        // which reads as "not loaded yet".
                        target.replay_files.get_or_insert_with(std::collections::HashMap::new);
                        // Any summaries already loaded were read against the
                        // live source. Drop the stamp so the next frame reads
                        // them against the source just resolved.
                        target.replay_row_summaries_generation = None;

                        // No offer is raised here. The scan that preceded this
                        // read already offered every build it found missing; a
                        // build still missing now is one whose download failed
                        // or was declined, and asking again is the loop the
                        // retained scan exists to break.
                        //
                        // Nothing else is going to account for these replays,
                        // so the toast names all of them.
                        let skipped = failures.total();
                        if skipped > 0 {
                            self.tab_state
                                .toasts
                                .lock()
                                .warning(t!("ui.messages.directory_replays_skipped", skipped = skipped));
                        }

                        // Listed, but with no stats behind their rows, so the
                        // listing looks half-loaded with no explanation unless
                        // this is said out loud.
                        if failures.not_indexed > 0 {
                            self.tab_state
                                .toasts
                                .lock()
                                .warning(t!("ui.messages.directory_replays_not_indexed", count = failures.not_indexed));
                        }
                    }
                }
                BackgroundTaskCompletion::DirectoryScanned { workspace, scan } => {
                    // A workspace that is gone was closed while the scan ran,
                    // and the scan belongs to nothing else.
                    if self.tab_state.workspace(workspace).is_some() {
                        let missing: BTreeSet<u32> = scan.missing_builds.iter().map(|build| build.get()).collect();
                        let just_offered = self.take_finished_reingest_offer(workspace);
                        let offer = !missing.is_empty()
                            && self.download_prompt.is_none()
                            && !offer_was_just_made(just_offered.as_ref(), &missing);

                        // Built before the scan is retained, since retaining it
                        // hands the scan over. The map is ordered, so the rows
                        // do not depend on the order the walk found files in.
                        let candidates: Vec<DownloadCandidate> = if offer {
                            scan.by_build
                                .iter()
                                .filter(|(build, _)| scan.missing_builds.contains(*build))
                                .map(|(build, group)| {
                                    DownloadCandidate::unresolved(
                                        build.get(),
                                        group.request.friendly_version(),
                                        Some(group.paths.len()),
                                    )
                                })
                                .collect()
                        } else {
                            Vec::new()
                        };

                        self.pending_scans.insert(workspace, scan);
                        if offer {
                            // The read waits on the answer: fetching the data
                            // first is what keeps these replays from being
                            // read, missed, and read again after the download.
                            self.download_prompt = Some(GameDataDownloadPrompt::new(
                                candidates,
                                Some(GameDataFollowUp::Directory(workspace)),
                            ));
                        } else {
                            self.start_directory_read(workspace);
                        }
                    }
                }
                BackgroundTaskCompletion::RowSummariesLoaded { summaries, workspace, .. } => {
                    if let Some(target) = self.tab_state.workspace_mut(workspace) {
                        target.replay_row_summaries = summaries;
                        target.replay_row_summaries_loaded = true;
                        target.replay_rows_need_reindex_scan = true;
                    }
                }
                #[cfg(feature = "mod_manager")]
                BackgroundTaskCompletion::ModManager(mod_manager_info) => match *mod_manager_info {
                    crate::mod_manager::ModTaskCompletion::DatabaseLoaded(index) => {
                        self.tab_state.persisted.write().mod_manager_info.update_index("test".to_string(), index);
                    }
                    crate::mod_manager::ModTaskCompletion::ModInstalled(mod_info) => {
                        self.tab_state
                            .toasts
                            .lock()
                            .success(format!("Successfully installed mod: {}", mod_info.meta.name()));
                    }
                    crate::mod_manager::ModTaskCompletion::ModUninstalled(mod_info) => {
                        self.tab_state
                            .toasts
                            .lock()
                            .success(format!("Successfully uninstalled mod: {}", mod_info.meta.name()));
                    }
                    crate::mod_manager::ModTaskCompletion::ModDownloaded(_) => {}
                },
            },
            Err(e)
                if e.downcast_current_context::<ToolkitError>()
                    .is_some_and(|e| matches!(e, ToolkitError::BackgroundTaskCompleted)) => {}
            Err(e) => {
                error!("Background task error: {e:?}");

                if let Some(ToolkitError::ReplayBuildUnavailable { build, version, replay_path }) =
                    e.downcast_current_context::<ToolkitError>()
                {
                    // Offer to download the missing data unless a prompt is
                    // already open.
                    if self.download_prompt.is_none() {
                        let candidate = DownloadCandidate::unresolved(*build, version.clone(), None);
                        let trigger = replay_path.clone().map(GameDataFollowUp::Replay);
                        self.download_prompt = Some(GameDataDownloadPrompt::new(vec![candidate], trigger));
                    }
                } else {
                    if e.downcast_current_context::<ToolkitError>()
                        .is_some_and(|e| matches!(e, ToolkitError::InvalidWowsDirectory(_)))
                    {
                        self.tab_state.settings_needs_attention = true;
                    }

                    self.tab_state.toasts.lock().error(format!("{e}"));
                }
            }
        }
    }

    /// Draw replay renderer viewports, auto-wire collab sessions, and clean up closed renderers.
    fn sync_replay_renderers(&mut self, ctx: &egui::Context) {
        let mut replay_renderers = self.tab_state.replay_renderers.lock();
        let mut remove_renderers = Vec::new();
        for (idx, renderer) in replay_renderers.iter().enumerate() {
            if !renderer.open.load(Ordering::Relaxed) {
                // Keep hidden client renderers alive so they can be reopened
                // from the session popover without showing a loading spinner.
                let is_hidden_client = renderer.shared_state().lock().collab_replay_id.is_some()
                    && self.tab_state.client_session.is_some();
                if is_hidden_client {
                    continue; // Skip draw + settings sync for hidden viewers.
                }
                remove_renderers.push(idx);
                continue;
            }
            renderer.draw(ctx);
            // Check if renderer wants to save default options
            if let Some(saved) = renderer.pending_defaults_save.lock().take() {
                self.tab_state.persisted.write().settings.renderer = saved;
            }
            // Sync GPU warning suppress flag back to settings
            let suppress = renderer.suppress_gpu_warning.load(Ordering::Relaxed);
            if suppress != self.tab_state.persisted.read().settings.app.suppress_gpu_encoder_warning {
                self.tab_state.persisted.write().settings.app.suppress_gpu_encoder_warning = suppress;
            }

            // Auto-wire renderer to host session if active.
            if let Some(ref host_handle) = self.tab_state.host_session {
                let mut state = renderer.shared_state().lock();
                // Assign replay_id if not yet assigned.
                if state.collab_replay_id.is_none() {
                    let id = self.tab_state.next_replay_id;
                    self.tab_state.next_replay_id += 1;
                    state.collab_replay_id = Some(id);
                    state.session_frame_tx = Some(host_handle.frame_tx.clone());
                    state.collab_session_state = Some(std::sync::Arc::clone(&self.tab_state.session_state));
                    state.collab_local_tx = Some(host_handle.local_tx.clone());
                    state.collab_command_tx = Some(host_handle.command_tx.clone());
                    // Send the current frame (if any) so clients get it immediately.
                    if let Some(ref frame) = state.frame {
                        tracing::debug!("Auto-wire: first frame already available, broadcasting (replay_id={id})");
                        let _ = host_handle.frame_tx.try_send(crate::collab::peer::FrameBroadcast {
                            replay_id: id,
                            clock: frame.clock.0,
                            frame_index: frame.frame_index as u32,
                            total_frames: frame.total_frames as u32,
                            game_duration: frame.game_duration,
                            commands: frame.commands.clone(),
                        });
                    }
                }
                // ReplayOpened is normally sent by the background thread once
                // assets load. But if assets loaded before auto-wire set
                // collab_command_tx, the background thread missed its chance.
                // Handle that race here.
                if !state.session_announced
                    && state.assets.is_some()
                    && let Some(replay_id) = state.collab_replay_id
                {
                    let map_png = state
                        .assets
                        .as_ref()
                        .and_then(|a| {
                            a.map_image.as_ref().map(|img| {
                                let mut buf = Vec::new();
                                if let Some(image) = image::RgbaImage::from_raw(img.width, img.height, img.data.clone())
                                {
                                    let mut cursor = std::io::Cursor::new(&mut buf);
                                    let _ = image.write_to(&mut cursor, image::ImageFormat::Png);
                                }
                                buf
                            })
                        })
                        .unwrap_or_default();
                    let game_version = state.game_version.clone().unwrap_or_default();
                    let replay_name = state.collab_replay_name.clone().unwrap_or_else(|| {
                        renderer.title.strip_prefix("Replay Renderer - ").unwrap_or(&renderer.title).to_string()
                    });
                    let collab_map_name = state.collab_map_name.clone().unwrap_or_default();
                    let display_name =
                        translate_map_display_name(&collab_map_name, &self.tab_state.world_of_warships_data);
                    let _ = host_handle.command_tx.send(crate::collab::SessionCommand::ReplayOpened {
                        replay_id,
                        replay_name,
                        map_image_png: map_png,
                        game_version,
                        map_name: collab_map_name,
                        display_name,
                    });
                    state.session_announced = true;
                }
            }
        }

        // Send ReplayClosed for renderers being removed while a host session is active.
        // Also poison session_announced + collab_command_tx so the background playback
        // thread can't send a late ReplayOpened after the renderer is already gone.
        for &idx in &remove_renderers {
            let mut state = replay_renderers[idx].shared_state().lock();
            state.session_announced = true;
            state.collab_command_tx = None;
            if let Some(replay_id) = state.collab_replay_id
                && let Some(ref handle) = self.tab_state.host_session
            {
                let _ = handle.command_tx.send(crate::collab::SessionCommand::ReplayClosed { replay_id });
            }
        }

        *replay_renderers = replay_renderers
            .drain(..)
            .enumerate()
            .filter_map(|(idx, r)| if !remove_renderers.contains(&idx) { Some(r) } else { None })
            .collect();
    }

    fn sync_tactics_boards(&mut self, ctx: &egui::Context) {
        let is_host = self.tab_state.host_session.is_some();
        let is_client = self.tab_state.client_session.is_some();
        let mut boards = self.tab_state.tactics_boards.lock();

        // Auto-wire existing tactics boards to session when one starts.
        let session_handle = self.tab_state.host_session.as_ref().or(self.tab_state.client_session.as_ref());
        if let Some(handle) = session_handle {
            for board in boards.iter_mut() {
                if board.collab_local_tx.is_none() {
                    board.collab_local_tx = Some(handle.local_tx.clone());
                    board.collab_session_state = Some(std::sync::Arc::clone(&self.tab_state.session_state));
                    board.collab_command_tx = Some(handle.command_tx.clone());
                    if is_host {
                        board.is_session_board = true;
                        // Send current map + caps + annotations to peers so they can catch up.
                        let state = board.state_arc().lock();
                        if let Some((map_id, map_name)) = state.selected_map() {
                            let map_name = map_name.to_string();
                            let map_image_png = state
                                .map_image_raw()
                                .map(|img| {
                                    let mut buf = Vec::new();
                                    if let Some(image) =
                                        image::RgbaImage::from_raw(img.width, img.height, img.data.clone())
                                    {
                                        let mut cursor = std::io::Cursor::new(&mut buf);
                                        let _ = image.write_to(&mut cursor, image::ImageFormat::Png);
                                    }
                                    buf
                                })
                                .unwrap_or_default();
                            let map_info = state.map_info().cloned();
                            let wire_caps: Vec<crate::collab::protocol::WireCapPoint> = state
                                .cap_points()
                                .iter()
                                .map(|c| crate::collab::protocol::WireCapPoint {
                                    id: c.id,
                                    index: c.index as u32,
                                    world_x: c.world_x,
                                    world_z: c.world_z,
                                    radius: c.radius,
                                    team_id: c.team_id,
                                    frozen: c.frozen,
                                })
                                .collect();
                            drop(state);
                            let display_name =
                                translate_map_display_name(&map_name, &self.tab_state.world_of_warships_data);
                            let _ = handle.local_tx.send(crate::collab::peer::LocalEvent::TacticsMapOpened {
                                board_id: board.board_id,
                                owner_user_id: board.owner_user_id,
                                map_name,
                                display_name,
                                map_id,
                                map_image_png,
                                map_info,
                            });
                            let _ = handle.command_tx.send(crate::collab::SessionCommand::SyncCapPoints {
                                board_id: board.board_id,
                                cap_points: wire_caps,
                            });
                            // Push pre-existing annotations into the session.
                            let ann = board.annotation_state_arc().lock();
                            if !ann.annotations.is_empty() {
                                crate::replay::minimap_view::send_annotation_full_sync(
                                    &Some(handle.command_tx.clone()),
                                    &ann,
                                    Some(board.board_id),
                                );
                            }
                        }
                    }
                }
            }
        }

        // Promotion: when a peer becomes co-host, flip their local boards to session boards
        // and announce them so they become visible to everyone.
        if let Some(handle) = session_handle {
            let is_authority = {
                let s = self.tab_state.session_state.lock();
                s.role.is_host() || s.role.is_co_host()
            };
            if is_authority {
                for board in boards.iter_mut() {
                    if !board.is_session_board && board.collab_local_tx.is_some() {
                        board.is_session_board = true;
                        // Announce this board to the session.
                        let state = board.state_arc().lock();
                        if let Some((map_id, map_name)) = state.selected_map() {
                            let map_name = map_name.to_string();
                            let map_image_png = state
                                .map_image_raw()
                                .map(|img| {
                                    let mut buf = Vec::new();
                                    if let Some(image) =
                                        image::RgbaImage::from_raw(img.width, img.height, img.data.clone())
                                    {
                                        let mut cursor = std::io::Cursor::new(&mut buf);
                                        let _ = image.write_to(&mut cursor, image::ImageFormat::Png);
                                    }
                                    buf
                                })
                                .unwrap_or_default();
                            let map_info = state.map_info().cloned();
                            let wire_caps: Vec<crate::collab::protocol::WireCapPoint> = state
                                .cap_points()
                                .iter()
                                .map(|c| crate::collab::protocol::WireCapPoint {
                                    id: c.id,
                                    index: c.index as u32,
                                    world_x: c.world_x,
                                    world_z: c.world_z,
                                    radius: c.radius,
                                    team_id: c.team_id,
                                    frozen: c.frozen,
                                })
                                .collect();
                            drop(state);
                            let display_name =
                                translate_map_display_name(&map_name, &self.tab_state.world_of_warships_data);
                            let _ = handle.local_tx.send(crate::collab::peer::LocalEvent::TacticsMapOpened {
                                board_id: board.board_id,
                                owner_user_id: board.owner_user_id,
                                map_name,
                                display_name,
                                map_id,
                                map_image_png,
                                map_info,
                            });
                            let _ = handle.command_tx.send(crate::collab::SessionCommand::SyncCapPoints {
                                board_id: board.board_id,
                                cap_points: wire_caps,
                            });
                            let ann = board.annotation_state_arc().lock();
                            if !ann.annotations.is_empty() {
                                crate::replay::minimap_view::send_annotation_full_sync(
                                    &Some(handle.command_tx.clone()),
                                    &ann,
                                    Some(board.board_id),
                                );
                            }
                        }
                    }
                }
            }
        }

        // Peer-only: auto-open tactics boards that appear in session state but aren't
        // open locally.  Each board_id is tracked in `tactics_auto_opened_board_ids`
        // so we don't re-open after the user closes one.
        if is_client
            && !self.tab_state.persisted.read().settings.collab.disable_auto_open_session_windows
            && let Some(handle) = self.tab_state.client_session.as_ref()
            && let Some(ref wows_data) = self.tab_state.world_of_warships_data
        {
            let ss = self.tab_state.session_state.lock();
            let new_boards: Vec<(u64, u64)> = ss
                .tactics_boards
                .iter()
                .filter(|(bid, _)| {
                    !boards.iter().any(|b| b.board_id == **bid)
                        && !self.tab_state.tactics_auto_opened_board_ids.contains(bid)
                })
                .map(|(&bid, bs)| (bid, bs.owner_user_id))
                .collect();
            drop(ss);
            for (bid, owner) in new_boards {
                if boards.len() >= crate::collab::protocol::MAX_TACTICS_BOARDS {
                    break;
                }
                self.tab_state.tactics_auto_opened_board_ids.insert(bid);
                let mut board = crate::replay::minimap_view::tactics::TacticsBoardViewer::new(
                    bid,
                    owner,
                    std::sync::Arc::clone(&self.tab_state.cap_layout_db),
                    std::sync::Arc::clone(&self.tab_state.renderer_asset_cache),
                    std::sync::Arc::clone(wows_data),
                    self.tab_state.db_pool.clone(),
                    self.tab_state.tokio_runtime.clone(),
                    self.tab_state.window_settings.clone(),
                    self.tab_state.save_notify.clone(),
                );
                board.is_session_board = true;
                board.collab_local_tx = Some(handle.local_tx.clone());
                board.collab_session_state = Some(std::sync::Arc::clone(&self.tab_state.session_state));
                board.collab_command_tx = Some(handle.command_tx.clone());
                boards.push(board);
            }
        }

        // Drain force_open_window_ids — the host asked everyone to open these windows.
        // For tactics boards, force-open even if the user previously closed them.
        if let Some(handle) = self.tab_state.host_session.as_ref().or(self.tab_state.client_session.as_ref())
            && let Some(ref wows_data) = self.tab_state.world_of_warships_data
        {
            let mut ss = self.tab_state.session_state.lock();
            let force_ids: Vec<u64> = ss.force_open_window_ids.drain().collect();
            // Collect board info while we have the lock.
            let force_boards: Vec<(u64, u64)> = force_ids
                .iter()
                .filter_map(|id| ss.tactics_boards.get(id).map(|bs| (*id, bs.owner_user_id)))
                .filter(|(bid, _)| !boards.iter().any(|b| b.board_id == *bid))
                .collect();
            drop(ss);
            for (bid, owner) in force_boards {
                if boards.len() >= crate::collab::protocol::MAX_TACTICS_BOARDS {
                    break;
                }
                self.tab_state.tactics_auto_opened_board_ids.insert(bid);
                let mut board = crate::replay::minimap_view::tactics::TacticsBoardViewer::new(
                    bid,
                    owner,
                    std::sync::Arc::clone(&self.tab_state.cap_layout_db),
                    std::sync::Arc::clone(&self.tab_state.renderer_asset_cache),
                    std::sync::Arc::clone(wows_data),
                    self.tab_state.db_pool.clone(),
                    self.tab_state.tokio_runtime.clone(),
                    self.tab_state.window_settings.clone(),
                    self.tab_state.save_notify.clone(),
                );
                board.is_session_board = true;
                board.collab_local_tx = Some(handle.local_tx.clone());
                board.collab_session_state = Some(std::sync::Arc::clone(&self.tab_state.session_state));
                board.collab_command_tx = Some(handle.command_tx.clone());
                boards.push(board);
            }
        }

        // Peer-only: close local session boards whose board_id is no longer in session state.
        if is_client && !boards.is_empty() {
            let session = self.tab_state.session_state.lock();
            for board in boards.iter() {
                if board.is_session_board && !session.tactics_boards.contains_key(&board.board_id) {
                    board.open.store(false, Ordering::Relaxed);
                }
            }
        }

        let mut remove = Vec::new();
        for (idx, board) in boards.iter().enumerate() {
            if !board.open.load(Ordering::Relaxed) {
                remove.push(idx);
            } else {
                board.draw(ctx);
            }
        }
        if !remove.is_empty() {
            // Host/co-host closing a session board — clear annotations and notify peers per board.
            let close_handle = self.tab_state.host_session.as_ref().or(self.tab_state.client_session.as_ref());
            if let Some(handle) = close_handle {
                for &idx in &remove {
                    if boards[idx].is_session_board && boards[idx].collab_local_tx.is_some() {
                        let bid = boards[idx].board_id;
                        let _ = handle.local_tx.send(crate::collab::peer::LocalEvent::Annotation(
                            crate::collab::peer::LocalAnnotationEvent::Clear { board_id: Some(bid) },
                        ));
                        let _ =
                            handle.local_tx.send(crate::collab::peer::LocalEvent::TacticsMapClosed { board_id: bid });
                    }
                }
            }
            *boards = boards
                .drain(..)
                .enumerate()
                .filter_map(|(idx, b)| if !remove.contains(&idx) { Some(b) } else { None })
                .collect();
        }
    }

    /// Poll pending armor viewer requests from replay renderers and spawn viewers.
    fn poll_armor_viewer_requests(&mut self) {
        // Poll ship assets loading (so it works without the Armor Viewer tab open)
        if let crate::armor_viewer::state::ShipAssetsState::Loading(ref rx) = self.tab_state.armor_viewer.ship_assets
            && let Ok(result) = rx.try_recv()
        {
            match result {
                Ok(assets) => {
                    // Build ship catalog if not already built (same logic as build_armor_viewer_tab)
                    if self.tab_state.armor_viewer.ship_catalog.is_none()
                        && let Some(ref wows_data) = self.tab_state.world_of_warships_data
                    {
                        let wd = wows_data.read();
                        if let Some(metadata) = wd.game_metadata.as_ref() {
                            let catalog = crate::armor_viewer::ship_selector::ShipCatalog::build(metadata);
                            for nation_group in &catalog.nations {
                                if !self.tab_state.armor_viewer.nation_flag_textures.contains_key(&nation_group.nation)
                                    && let Some(asset) =
                                        crate::task::load_nation_flag(&wd.vfs, &nation_group.nation, wd.version())
                                {
                                    self.tab_state
                                        .armor_viewer
                                        .nation_flag_textures
                                        .insert(nation_group.nation.clone(), asset);
                                }
                            }
                            self.tab_state.armor_viewer.ship_catalog = Some(std::sync::Arc::new(catalog));
                        }
                    }
                    self.tab_state.armor_viewer.ship_assets =
                        crate::armor_viewer::state::ShipAssetsState::Loaded(assets);
                }
                Err(e) => {
                    tracing::error!("Failed to load ship assets: {e}");
                    self.tab_state.armor_viewer.ship_assets = crate::armor_viewer::state::ShipAssetsState::Failed(e);
                }
            }
        }

        let replay_renderers = self.tab_state.replay_renderers.lock();
        for renderer in replay_renderers.iter() {
            let mut state = renderer.shared_state().lock();
            let requests: Vec<crate::replay::renderer::ArmorViewerRequest> =
                state.pending_armor_viewers.drain(..).collect();
            drop(state);

            for request in requests {
                // Ensure ship assets and GPU pipeline are available
                let ship_assets = match &self.tab_state.armor_viewer.ship_assets {
                    crate::armor_viewer::state::ShipAssetsState::Loaded(assets) => Some(assets.clone()),
                    _ => None,
                };
                let gpu_pipeline = self.tab_state.armor_viewer.gpu_pipeline.clone();
                let render_state = self.tab_state.wgpu_render_state.clone();

                if let (Some(ship_assets), Some(gpu_pipeline), Some(render_state)) =
                    (ship_assets, gpu_pipeline, render_state)
                {
                    // Find the target player info from the bridge
                    let bridge = request.bridge.lock();
                    let target_player = bridge.players.iter().find(|p| p.entity_id == request.target_entity_id);
                    if let Some(player) = target_player {
                        let viewer = crate::replay::realtime_armor_viewer::RealtimeArmorViewer::new(
                            player,
                            request.bridge.clone(),
                            ship_assets,
                            gpu_pipeline,
                            render_state,
                            Some(request.command_tx.clone()),
                            self.tab_state.window_settings.clone(),
                            self.tab_state.save_notify.clone(),
                        );
                        drop(bridge);
                        self.realtime_armor_viewers.push(Arc::new(parking_lot::Mutex::new(viewer)));
                    } else {
                        // Bridge players not populated yet — re-queue for next frame
                        drop(bridge);
                        let mut state = renderer.shared_state().lock();
                        state.pending_armor_viewers.push(request);
                    }
                } else {
                    // Assets not ready — trigger loading if needed
                    if matches!(
                        &self.tab_state.armor_viewer.ship_assets,
                        crate::armor_viewer::state::ShipAssetsState::NotLoaded
                    ) && let Some(ref wows_data) = self.tab_state.world_of_warships_data
                    {
                        let wd = wows_data.read();
                        let vfs = wd.vfs.clone();
                        let game_metadata = wd.game_metadata.clone();
                        drop(wd);
                        let (tx, rx) = std::sync::mpsc::channel();
                        crate::util::thread::spawn_logged("load-ship-assets", move || {
                            let result = (|| -> Result<Arc<wowsunpack::export::ship::ShipAssets>, String> {
                                let metadata =
                                    game_metadata.ok_or_else(|| "GameMetadataProvider not loaded".to_string())?;
                                let assets =
                                    wowsunpack::export::ship::ShipAssets::from_vfs_with_metadata(&vfs, metadata)
                                        .map_err(|e| format!("{e:?}"))?;
                                Ok(Arc::new(assets))
                            })();
                            let _ = tx.send(result);
                        });
                        self.tab_state.armor_viewer.ship_assets =
                            crate::armor_viewer::state::ShipAssetsState::Loading(rx);
                    }
                    if self.tab_state.armor_viewer.gpu_pipeline.is_none()
                        && let Some(ref rs) = self.tab_state.wgpu_render_state
                    {
                        self.tab_state.armor_viewer.gpu_pipeline =
                            Some(Arc::new(crate::viewport_3d::GpuPipeline::new(&rs.device, &rs.queue)));
                    }
                    // Re-queue the request for next frame
                    let mut state = renderer.shared_state().lock();
                    state.pending_armor_viewers.push(request);
                }
            }
        }
        drop(replay_renderers);
    }

    fn ui_file_drag_and_drop(&mut self, ctx: &Context) {
        use egui::Align2;
        use egui::Color32;
        use egui::Id;
        use egui::LayerId;
        use egui::Order;
        use egui::TextStyle;

        // Preview hovering files:
        if !ctx.input(|i| i.raw.hovered_files.is_empty()) {
            let text = ctx.input(|i| {
                if i.raw.hovered_files.len() > 1 {
                    return Some("Only one file at a time, please.".to_owned());
                }

                if let Some(file) = i.raw.hovered_files.first()
                    && let Some(path) = &file.path
                    && path.is_file()
                {
                    return Some(format!("Drop to load\n{}", path.file_name()?.to_str()?));
                }

                None
            });

            if let Some(text) = text {
                let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("file_drop_target")));

                let screen_rect = ctx.content_rect();
                painter.rect_filled(screen_rect, 0.0, Color32::from_black_alpha(192));
                painter.text(
                    screen_rect.center(),
                    Align2::CENTER_CENTER,
                    text,
                    TextStyle::Heading.resolve(&ctx.global_style()),
                    Color32::WHITE, // theme-exempt: sits on this overlay's own opaque scrim
                );
            }
        }

        let mut dropped_files = Vec::new();

        ctx.input(|i| {
            if !i.raw.dropped_files.is_empty() {
                dropped_files.clone_from(&i.raw.dropped_files);
            }
        });

        if dropped_files.len() == 1
            && let Some(path) = &dropped_files[0].path
            && let Some(deps) = self.tab_state.replay_dependencies()
        {
            self.tab_state.persisted.write().settings.game.current_replay_path = path.clone();
            update_background_task!(
                self.tab_state.background_tasks,
                deps.parse_replay_from_path(path.clone(), crate::task::ReplaySource::ManualOpen)
            );
        }
    }

    fn update_impl(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if mitigate_wgpu_mem_leak(ctx) {
            return;
        }

        // Update active viewport list for the background save task's window geometry capture.
        {
            use crate::tab_state::WindowKind;
            let mut viewports: Vec<(WindowKind, egui::ViewportId)> = Vec::new();

            for r in self.tab_state.replay_renderers.lock().iter() {
                viewports.push((WindowKind::ReplayRenderer, r.viewport_id()));
            }
            for t in self.tab_state.tactics_boards.lock().iter() {
                viewports.push((WindowKind::TacticsBoard, t.viewport_id()));
            }
            for v in &self.realtime_armor_viewers {
                viewports.push((WindowKind::ArmorViewer, v.lock().viewport_id()));
            }

            *self.tab_state.active_viewports.lock() = viewports;
        }

        // Register main window context so the peer task can wake us.
        {
            let mut s = self.tab_state.session_state.lock();
            if s.egui_ctx.is_none() {
                s.egui_ctx = Some(ctx.clone());
            }
        }
        // Draw realtime armor viewer windows
        self.realtime_armor_viewers.retain(|v| v.lock().open.load(Ordering::Relaxed));
        for viewer in &self.realtime_armor_viewers {
            crate::replay::realtime_armor_viewer::draw_realtime_armor_viewer(viewer, ctx);
        }

        if ctx
            .input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::CTRL | Modifiers::SHIFT, egui::Key::D)))
        {
            {
                let mut p = self.tab_state.persisted.write();
                p.settings.app.debug_mode = !p.settings.app.debug_mode;
            }
            let debug_mode = self.tab_state.persisted.read().settings.app.debug_mode;
            if let Some(sender) = self.tab_state.background_parser_tx.as_ref() {
                let _ = sender.send(ReplayBackgroundParserThreadMessage::DebugStateChange(debug_mode));
            }
        }

        if ctx.input_mut(|i| {
            i.consume_shortcut(&KeyboardShortcut::new(Modifiers::CTRL, egui::Key::K))
                || i.consume_shortcut(&KeyboardShortcut::new(Modifiers::CTRL, egui::Key::P))
        }) {
            if self.tab_state.command_palette.state.open {
                self.tab_state.command_palette.state.close();
            } else {
                if self.tab_state.ship_catalog.is_none()
                    && let Some(wows_data) = self.tab_state.world_of_warships_data.as_ref()
                {
                    let wd = wows_data.read();
                    if let Some(metadata) = wd.game_metadata.as_ref() {
                        self.tab_state.ship_catalog =
                            Some(crate::armor_viewer::ship_selector::ShipCatalog::build(metadata));
                    }
                }
                self.tab_state.command_palette.mode = crate::ui::command_palette::PaletteMode::Root;
                self.tab_state.command_palette.state.open();
            }
        }

        self.tab_state.try_update_replays();

        // Pick up "Add to Session Stats" requests (no confirmation needed).
        // App-wide: feeds the one global session-stats total, not a per-workspace one.
        if let Some(replays) =
            ctx.data_mut(|data| data.remove_temp::<Vec<PathBuf>>(egui::Id::new("add_to_session_stats_request")))
        {
            self.tab_state.clear_before_session_reset = false;
            self.tab_state.replays_for_session_reset = Some(replays);
        }

        self.tab_state.process_session_stats_reset();

        if self.manual_update_requested
            || (!self.checked_for_updates && self.tab_state.persisted.read().settings.app.check_for_updates)
        {
            self.manual_update_requested = false;
            self.request_update_checks();
        }

        self.poll_network_results();

        // Update settings_needs_attention based on cached WoWs directory validity and twitch token state
        {
            let twitch_token_failed = self.tab_state.persisted.read().settings.integrations.twitch_token.is_some()
                && self.tab_state.twitch_state.read().token_validation_failed;

            if twitch_token_failed && !self.shown_twitch_token_error {
                self.shown_twitch_token_error = true;
                error!("Twitch token is invalid or expired");
                self.tab_state.toasts.lock().error(t!("ui.messages.twitch_token_invalid"));
            } else if !twitch_token_failed {
                self.shown_twitch_token_error = false;
            }

            self.tab_state.settings_needs_attention = self.tab_state.wows_dir_invalid || twitch_token_failed;
        }

        if self.build_consent_window_open {
            egui::Window::new(t!("ui.windows.build_consent")).collapsible(false).show(ctx, |ui| {
                ui.label(t!("ui.dialogs.build_consent_message"));
                ui.horizontal(|ui| {
                    if ui.button(t!("ui.buttons.send_replays")).clicked() {
                        self.set_data_sharing_choice(DataSharingMode::Replays);
                    }
                    if ui.button(t!("ui.buttons.send_build_data")).clicked() {
                        self.set_data_sharing_choice(DataSharingMode::BuildData);
                    }
                    if ui.button(t!("ui.buttons.send_nothing")).clicked() {
                        self.set_data_sharing_choice(DataSharingMode::Off);
                    }
                });
            });
        }

        if self.replay_migration_window_open {
            egui::Window::new(t!("ui.windows.replay_migration")).collapsible(false).show(ctx, |ui| {
                ui.label(t!("ui.dialogs.replay_migration_message"));
                ui.horizontal(|ui| {
                    if ui.button(t!("ui.buttons.switch_to_replays")).clicked() {
                        self.replay_migration_window_open = false;
                        {
                            let mut p = self.tab_state.persisted.write();
                            p.settings.app.replay_consent_prompt_shown = true;
                            p.settings.integrations.data_sharing_mode = DataSharingMode::Replays;
                        }
                        self.tab_state.send_replay_consent_changed();
                    }
                    if ui.button(t!("ui.buttons.keep_current")).clicked() {
                        self.replay_migration_window_open = false;
                        self.tab_state.persisted.write().settings.app.replay_consent_prompt_shown = true;
                    }
                });
            });
        }

        self.service_directory_reingests();
        self.show_download_prompt(ctx);

        if self.language_selection_open {
            let detected_locale =
                self.tab_state.persisted.read().settings.app.locale.clone().unwrap_or_else(|| "en".into());
            let native_name = wt_translations::language_name(&detected_locale).unwrap_or("English");

            egui::Window::new(t!("dialog.select_language"))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(t!("dialog.machine_translation_warning"));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        // "Continue in English" button
                        if ui.button(t!("dialog.continue_in_english")).clicked() {
                            let mut p = self.tab_state.persisted.write();
                            p.settings.app.locale = Some("en".into());
                            rust_i18n::set_locale("en");
                            p.settings.app.language_selection_shown = true;
                            drop(p);
                            self.language_selection_open = false;
                        }
                        // "Continue in <detected language>" button
                        let continue_label = t!("dialog.continue_in_language");
                        // For English TOML the label is the same, but for translated TOMLs
                        // it will be in the detected language. Show native name as fallback.
                        let label = if continue_label == "Continue in English" {
                            format!("Continue in {}", native_name)
                        } else {
                            continue_label.into()
                        };
                        if ui.button(label).clicked() {
                            // Keep the detected locale
                            self.tab_state.persisted.write().settings.app.language_selection_shown = true;
                            self.language_selection_open = false;
                        }
                    });
                });
        }

        if self.panic_window_open {
            self.show_panic_window(ctx);
        }

        if self.update_window_open {
            self.show_update_window(ctx);
        }

        if let Some(error) = self.error_to_show.as_ref() {
            if self.show_error_window {
                egui::Window::new(t!("ui.windows.error")).open(&mut self.show_error_window).show(ctx, |ui| {
                    build_error_window(ui, error);
                });
            } else {
                self.error_to_show = None;
            }
        }

        if self.show_about_window {
            egui::Window::new(t!("ui.windows.about")).open(&mut self.show_about_window).show(ctx, |ui| {
                build_about_window(ui);
            });
        }

        // Panels are drawn in ui() via draw_panels()

        self.show_confirmation_dialog(ctx);
        self.show_ip_warning_dialog(ctx);
        if self.tab_state.pending_join && !self.tab_state.show_ip_warning {
            self.tab_state.pending_join = false;
            self.do_join_session();
        }
        if self.tab_state.pending_host && !self.tab_state.show_ip_warning {
            self.tab_state.pending_host = false;
            self.do_host_session();
        }
        self.poll_host_session_events(ctx);
        self.poll_client_session_events(ctx);

        // Pop open something to view the clicked file from the unpacker tab
        let mut file_viewer = self.tab_state.file_viewer.lock();
        let mut remove_viewers = Vec::new();
        for (idx, file_viewer) in file_viewer.iter_mut().enumerate() {
            file_viewer.draw(ctx);
            if !file_viewer.open.load(Ordering::Relaxed) {
                remove_viewers.push(idx);
            }
        }

        *file_viewer = file_viewer
            .drain(..)
            .enumerate()
            .filter_map(|(idx, viewer)| if !remove_viewers.contains(&idx) { Some(viewer) } else { None })
            .collect();
        drop(file_viewer);

        self.sync_replay_renderers(ctx);
        self.sync_tactics_boards(ctx);

        self.poll_armor_viewer_requests();

        self.ui_file_drag_and_drop(ctx);

        self.tab_state.toasts.lock().show(ctx);

        // If persisted state was written to this frame, wake the background save task.
        {
            let current_gen = self.tab_state.persisted.generation();
            if current_gen != self.last_persisted_generation {
                self.last_persisted_generation = current_gen;
                self.tab_state.request_save();
            }
        }

        // When any replay renderer is playing locally, repaint continuously so
        // deferred viewports stay in sync. Client sessions are event-driven:
        // the peer task repaints registered viewports when state changes.
        let any_playing = self.tab_state.replay_renderers.lock().iter().any(|r| r.shared_state().lock().playing);
        if any_playing || !self.realtime_armor_viewers.is_empty() {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after_secs(1.0);
        }
    }

    fn show_panic_window(&mut self, ctx: &Context) {
        if let Some(panic_info) = self.panic_info.as_mut() {
            egui::Window::new(t!("ui.windows.crash_detected")).open(&mut self.panic_window_open).show(ctx, |ui| {
                ui.vertical(|ui| {
                    ui.label(t!("ui.dialogs.crash_message"));
                    ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                        ui.scope(|ui| {
                            let style = ui.style_mut();
                            style.override_text_style = Some(TextStyle::Monospace);
                            let widget = egui::TextEdit::multiline(panic_info).desired_width(f32::INFINITY);
                            ui.add_enabled(false, widget);
                        });
                    });
                    ui.horizontal(|ui| {
                        if ui.button(t!("ui.buttons.copy")).clicked() {
                            Context::copy_text(ctx, panic_info.clone());
                        }
                        if ui.button(wt_translations::icon_t(icons::GITHUB_LOGO, &t!("ui.buttons.github"))).clicked() {
                            ui.ctx().open_url(OpenUrl::new_tab(
                                "https://github.com/landaire/wows-toolkit/issues/new/choose",
                            ));
                        }
                        if ui.button(wt_translations::icon_t(icons::DISCORD_LOGO, &t!("ui.buttons.discord"))).clicked()
                        {
                            ui.ctx().open_url(OpenUrl::new_tab(rot13("uggcf://qvfpbeq.tt/EWXjXHw7eu")));
                        }
                    });
                    ui.collapsing(t!("ui.buttons.more_options"), |ui| {
                        ui.label(t!("ui.dialogs.crash_clear_settings"));
                        ui.scope(|ui| {
                            let error = ui.sem().error;
                            let label = crate::ui::theme::contrast::label_on(error);
                            let visuals = &mut ui.style_mut().visuals;

                            visuals.widgets.inactive.bg_fill = error;
                            visuals.widgets.hovered.bg_fill = error;
                            visuals.widgets.active.bg_fill = error;

                            visuals.widgets.inactive.weak_bg_fill = error;
                            visuals.widgets.hovered.weak_bg_fill = error;
                            visuals.widgets.active.weak_bg_fill = error;

                            visuals.widgets.inactive.fg_stroke.color = label;
                            visuals.widgets.hovered.fg_stroke.color = label;
                            visuals.widgets.active.fg_stroke.color = label;

                            if ui.button(t!("ui.buttons.clear_settings")).clicked() {
                                *self.tab_state.persisted.write() = Default::default();
                            }
                        });
                    });
                });
            });
        }

        if !self.panic_window_open {
            let _ = std::fs::remove_file(Self::panic_log_path());
            self.panic_info = None;
        }
    }

    fn show_update_window(&mut self, ctx: &Context) {
        if let Some(latest_release) = self.latest_release.as_ref() {
            let url = latest_release.html_url.clone();
            let mut notes = latest_release.body.clone();
            let tag = latest_release.tag_name.clone();
            let asset = latest_release
                .assets
                .iter()
                // Match the app archive only. The release also carries a
                // `wows_toolkit_tools_*` CLI bundle; without the `tools` guard the
                // updater could grab it and relaunch a console tool instead of the app.
                .find(|asset| {
                    asset.name.contains("windows") && asset.name.ends_with(".zip") && !asset.name.contains("tools")
                });
            if let Some(asset) = asset {
                egui::Window::new(t!("ui.windows.update_available")).open(&mut self.update_window_open).show(
                    ctx,
                    |ui| {
                        ui.vertical(|ui| {
                            ui.label(t!("ui.dialogs.update_message", tag = tag));
                            if let Some(notes) = notes.as_mut() {
                                ScrollArea::vertical().max_height(500.0).show(ui, |ui| {
                                    CommonMarkViewer::new().show(ui, &mut self.tab_state.markdown_cache, notes);
                                });
                            }
                            ui.horizontal(|ui| {
                                #[cfg(target_os = "windows")]
                                {
                                    if ui.button(t!("ui.buttons.install_update")).clicked() {
                                        let task = Some(crate::task::start_download_update_task(&self.runtime, asset));
                                        update_background_task!(self.tab_state.background_tasks, task);
                                    }
                                }
                                #[cfg(not(target_os = "windows"))]
                                {
                                    let _ = asset;
                                    ui.label(t!("ui.dialogs.update_windows_only"));
                                }
                                if ui.button(t!("ui.buttons.view_release")).clicked() {
                                    ui.ctx().open_url(OpenUrl::new_tab(url));
                                }
                            });
                        });
                    },
                );
            } else {
                self.update_window_open = false;
            }
        }
    }

    pub fn panic_log_path() -> PathBuf {
        let mut panic_log_path = PathBuf::from("wows_toolkit_panic.log");
        if let Some(storage_dir) = crate::storage_dir() {
            panic_log_path = storage_dir.join(panic_log_path)
        }
        panic_log_path
    }

    /// If a constants/game version mismatch was detected, request updated
    /// constants from the networking thread. The thread handles throttling internally.
    fn try_update_constants(&mut self) {
        if !self.constants_version_mismatch {
            return;
        }

        self.tab_state.send_network_job(NetworkJob::FetchLatestConstants {
            current_commit: self.tab_state.persisted.read().settings.game.constants_file_commit.clone(),
        });
    }

    fn check_constants_version_mismatch(&mut self) {
        // Determine mismatch status under locks, then drop them before acting.
        // Read the version from the loaded WorldOfWarshipsData's replay constants
        // rather than a separate copy.
        let mismatch_status = {
            let Some(wows_data) = &self.tab_state.world_of_warships_data else { return };
            let wows_data = wows_data.read();
            let Some(full_version) = &wows_data.full_version else { return };

            let replay_constants = wows_data.replay_constants.read();
            let constants_version =
                replay_constants.get("VERSION").and_then(|v| v.get("VERSION")).and_then(|v| v.as_str());
            let Some(constants_version) = constants_version else { return };
            let game_version = format!("{}.{}", full_version.major, full_version.minor);

            if constants_version != game_version {
                Some(true) // mismatch
            } else if self.constants_version_mismatch {
                Some(false) // mismatch just resolved
            } else {
                None // no change
            }
        };

        match mismatch_status {
            Some(true) => {
                self.constants_version_mismatch = true;
                self.tab_state.toasts.lock().warning(t!("ui.messages.constants_version_mismatch")).duration(None);

                // The on-disk constants file is stale — delete it so the versioned
                // system doesn't treat it as an exact match, then request a fresh fetch.
                if let Some(wows_data) = &self.tab_state.world_of_warships_data {
                    let (build, version) = {
                        let guard = wows_data.read();
                        let version =
                            guard.full_version.as_ref().map(|v| format!("{}.{}.{}", v.major, v.minor, v.patch));
                        (guard.build_number, version)
                    };
                    if let Some(storage_dir) = crate::storage_dir() {
                        let path = storage_dir.join(format!("constants_{build}.json"));
                        let _ = std::fs::remove_file(path);
                    }
                    // Mark as inexact so the fetch/rebuild path works
                    wows_data.write().replay_constants_exact_match = false;
                    self.tab_state.send_network_job(NetworkJob::FetchVersionedConstants { build, version });
                }
                // Also clear the saved commit so FetchLatestConstants re-downloads
                self.tab_state.persisted.write().settings.game.constants_file_commit = None;
                self.tab_state.send_network_job(NetworkJob::FetchLatestConstants { current_commit: None });
            }
            Some(false) => {
                self.constants_version_mismatch = false;
                self.tab_state.toasts.lock().dismiss_all_toasts();

                // Rebuild all loaded WorldOfWarshipsData with fresh constants
                let rebuild_ok = self
                    .tab_state
                    .wows_data_map
                    .as_ref()
                    .map(|map| map.rebuild_all_with_new_constants())
                    .unwrap_or(true);

                if rebuild_ok {
                    self.constants_update_error_shown = false;

                    // Invalidate ui_report on all loaded replays so they re-build
                    // with the new constants on next access
                    for replay in self.tab_state.all_open_replays() {
                        replay.write().ui_report = None;
                    }

                    // Re-load the focused replay to rebuild its ui_report
                    if let Some(focused) = self.tab_state.focused_replay()
                        && let Some(deps) = self.tab_state.replay_dependencies()
                    {
                        update_background_task!(
                            self.tab_state.background_tasks,
                            deps.load_replay(focused, crate::task::ReplaySource::Reload)
                        );
                    }

                    self.tab_state.toasts.lock().success(t!("ui.messages.constants_updated"));
                } else if !self.constants_update_error_shown {
                    self.constants_update_error_shown = true;
                    warn!("Failed to fetch versioned constants during rebuild");
                    self.tab_state
                        .toasts
                        .lock()
                        .error(t!("ui.messages.versioned_constants_rebuild_failed"))
                        .duration(None);
                }
            }
            None => {}
        }
    }

    fn show_err_window(&mut self, err: Report) {
        self.show_error_window = true;
        let formatted = err.format_with(&DefaultReportFormatter::ASCII);
        self.error_to_show = Some(format!("{formatted}"));
    }

    /// Start the walks owed to directories whose downloads have finished.
    ///
    /// Done here rather than at the moment the last download landed: the
    /// workspace can be mid-walk from a deliberate reopen just then, and a walk
    /// that could not start would be dropped, leaving the listing permanently
    /// short of the replays the download was for.
    fn service_directory_reingests(&mut self) {
        // A scan whose tab closed before its read started describes a directory
        // nothing is listing any more.
        self.pending_scans.retain(|workspace, _| self.tab_state.workspace(*workspace).is_some());

        let owed: Vec<WorkspaceId> = self
            .directory_reingest
            .iter()
            .filter(|(_, state)| matches!(state, DirectoryReingest::Owed { .. }))
            .map(|(workspace, _)| *workspace)
            .collect();

        for workspace in owed {
            // A workspace closed while its downloads ran has no listing left to
            // fill, and reopening one the user closed would be a surprise.
            let Some(root) = self.tab_state.workspace(workspace).and_then(|w| w.root.clone()) else {
                self.directory_reingest.remove(&workspace);
                continue;
            };

            // The scan taken before the download still describes this
            // directory, so the read starts from it rather than walking the
            // directory a second time. That read raises no offer of its own,
            // which leaves the record with nothing to suppress.
            if self.pending_scans.contains_key(&workspace) {
                if self.start_directory_read(workspace) {
                    self.directory_reingest.remove(&workspace);
                }
                continue;
            }

            // A download that finished after its scan was dropped still owes
            // the user a listing, and the scan it needs has to be taken again.
            if !self.start_directory_scan(workspace, root) {
                continue;
            }
            if let Some(state) = self.directory_reingest.get_mut(&workspace)
                && let DirectoryReingest::Owed { offered } = state
            {
                *state = DirectoryReingest::Walking { offered: std::mem::take(offered) };
            }
        }
    }

    /// Take the record of a finished re-walk, returning the builds the offer
    /// that caused it showed. `None` when this walk was not one: a deliberate
    /// reopen is a fresh question and its offer is not suppressed.
    ///
    /// Called from the drain loop's kind dispatch, so a walk that errored or
    /// whose channel disconnected clears its record too. A record left in
    /// `Walking` marks every later walk of that workspace as automatic.
    fn finish_directory_reingest(&mut self, workspace: WorkspaceId) -> Option<BTreeSet<u32>> {
        if !matches!(self.directory_reingest.get(&workspace), Some(DirectoryReingest::Walking { .. })) {
            return None;
        }
        self.directory_reingest.remove(&workspace).map(|state| state.offered().clone())
    }

    /// Read back the offer released by the walk that has just finished for
    /// `workspace`. A release for some other workspace is dropped rather than
    /// answering for this one.
    fn take_finished_reingest_offer(&mut self, workspace: WorkspaceId) -> Option<BTreeSet<u32>> {
        match self.finished_reingest_offer.take() {
            Some((released, offered)) if released == workspace => Some(offered),
            _ => None,
        }
    }

    /// Keep the plan in step with what is ticked. Planning reads the remote
    /// index and per-build metadata, so it runs as a background task and only
    /// when the selection it describes has actually changed.
    fn sync_download_plan(&mut self) {
        if !self.download_prompt.as_ref().is_some_and(GameDataDownloadPrompt::needs_plan) {
            return;
        }

        let cache_dir = self.tab_state.persisted.read().settings.game.game_data_cache_dir.clone();
        let output_base = crate::task::replays::game_data_dump_base_with_override(&cache_dir);

        let Some(prompt) = &mut self.download_prompt else {
            return;
        };
        let (ticket, request) = prompt.begin_planning();
        let Some(output_base) = output_base else {
            // Naming the cause, because Retry cannot fix it: no amount of
            // pressing it resolves a cache directory that is not configured.
            prompt.plan = DownloadPlanState::Failed(t!("ui.dialogs.download_plan_no_cache_dir").into_owned());
            return;
        };
        update_background_task!(
            self.tab_state.background_tasks,
            Some(crate::task::start_game_data_plan_task(output_base, request, ticket))
        );
    }

    /// Draw the offer to download missing game data, if one is open.
    fn show_download_prompt(&mut self, ctx: &egui::Context) {
        self.sync_download_plan();

        let Some(prompt) = &mut self.download_prompt else {
            return;
        };

        let mut start_download = false;
        let mut retry = false;
        let mut dismiss = false;
        egui::Window::new(t!("ui.windows.download_game_data"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(t!("ui.dialogs.download_game_data_intro"));
                ui.add_space(8.0);

                for candidate in &mut prompt.candidates {
                    ui.horizontal(|ui| {
                        let selectable = candidate.is_selectable();
                        ui.add_enabled(selectable, egui::Checkbox::without_text(&mut candidate.selected));
                        ui.label(t!(
                            "ui.dialogs.download_build_row",
                            version = &candidate.version,
                            build = candidate.build
                        ));
                        if let Some(count) = candidate.replays_needing {
                            ui.label(RichText::new(t!("ui.dialogs.download_replays_needing", count = count)).weak());
                        }
                        let availability = match &candidate.availability {
                            Some(availability) => availability_label(availability),
                            None => t!("ui.dialogs.download_availability_resolving").into_owned(),
                        };
                        ui.label(RichText::new(availability).weak());
                    });
                }

                ui.add_space(8.0);
                match &prompt.plan {
                    DownloadPlanState::Idle | DownloadPlanState::Planning => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(t!("ui.dialogs.download_plan_pending"));
                        });
                    }
                    DownloadPlanState::Ready(plan) => {
                        ui.label(t!("ui.dialogs.download_objects_to_fetch", count = plan.unique_missing_objects));
                    }
                    DownloadPlanState::Failed(message) => {
                        ui.label(RichText::new(message.as_str()).color(ui.sem().error));
                    }
                }

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let ready = matches!(prompt.plan, DownloadPlanState::Ready(_));
                    let can_download = ready && prompt.candidates.iter().any(|c| c.is_downloadable());
                    if ui.add_enabled(can_download, egui::Button::new(t!("ui.buttons.download"))).clicked() {
                        start_download = true;
                    }
                    // Without this the dialog is a dead end after a network
                    // blip: nothing is resolved, so every checkbox is disabled
                    // and no selection change can ask the planner again.
                    if prompt.can_retry() && ui.button(t!("ui.buttons.retry")).clicked() {
                        retry = true;
                    }
                    if ui.button(t!("ui.buttons.dismiss")).clicked() {
                        dismiss = true;
                    }
                });
            });

        if start_download {
            if let Some(prompt) = self.download_prompt.take() {
                self.start_game_data_download(prompt);
            }
        } else if retry {
            if let Some(prompt) = &mut self.download_prompt {
                prompt.retry();
            }
        } else if dismiss {
            // Dismissing closes this offer and nothing more. Going back and
            // opening those replays again is a fresh request and gets a fresh
            // offer; the only offer ever suppressed is the automatic one the
            // walk after a download would otherwise repeat.
            let trigger = self.download_prompt.take().and_then(|prompt| prompt.trigger);
            self.resume_unanswered_directory(trigger.as_ref());
        }
    }

    /// Start downloading every build ticked in `prompt`. Each build's data is
    /// checked against the remote repository and, if published, fetched into
    /// the local cache directory.
    fn start_game_data_download(&mut self, prompt: GameDataDownloadPrompt) {
        if !self.spawn_game_data_download(&prompt) {
            // Nothing is going to fetch these builds, so nothing is going to
            // start the read the offer was holding back either.
            self.resume_unanswered_directory(prompt.trigger.as_ref());
        }
    }

    /// Spawn the download task for what `prompt` has ticked, reporting whether
    /// one was actually started.
    fn spawn_game_data_download(&mut self, prompt: &GameDataDownloadPrompt) -> bool {
        let cache_dir = self.tab_state.persisted.read().settings.game.game_data_cache_dir.clone();
        let Some(output_base) = crate::task::replays::game_data_dump_base_with_override(&cache_dir) else {
            self.tab_state.toasts.lock().error(t!("ui.messages.game_data_download_failed"));
            return false;
        };

        let builds = prompt.downloadable();
        if builds.is_empty() {
            return false;
        }

        let Some(runtime) = self.tab_state.tokio_runtime.as_ref().map(Arc::clone) else {
            warn!("cannot download game data: tokio runtime is not available");
            return false;
        };

        let requests: Vec<crate::task::BuildRequest> = builds
            .into_iter()
            .filter_map(|(build, version)| {
                let mut parts = version.split('.').filter_map(|p| p.trim().parse::<u32>().ok());
                let version = Version {
                    major: parts.next()?,
                    minor: parts.next().unwrap_or(0),
                    patch: parts.next().unwrap_or(0),
                    build: std::num::NonZeroU32::new(build),
                };
                crate::task::BuildRequest::new(version)
            })
            .collect();
        if requests.is_empty() {
            return false;
        }

        // Recorded only once a task is actually about to be spawned: an
        // earlier return here would leave a record nothing clears, since
        // `service_directory_reingests` only ever advances an `Owed` state and
        // `finish_directory_reingest` only ever consumes a `Walking` one.
        if let Some(GameDataFollowUp::Directory(workspace)) = &prompt.trigger {
            let workspace = *workspace;
            self.directory_reingest
                .insert(workspace, DirectoryReingest::AwaitingDownload { offered: prompt.offered_builds() });
            self.mark_directory_downloading(workspace, task::DownloadProgress { downloaded: 0, total: 0 });
        }

        update_background_task!(
            self.tab_state.background_tasks,
            Some(crate::task::start_game_data_download_task(
                output_base,
                requests,
                runtime,
                false,
                prompt.trigger.clone(),
            ))
        );
        true
    }

    /// Report a download above `workspace`'s listing.
    ///
    /// Called when the download task is spawned as well as on every progress
    /// message it sends: the first message arrives only after planning and a
    /// metadata fetch per build, and a listing reporting no stage across that
    /// window draws as a finished, empty directory.
    fn mark_directory_downloading(&mut self, workspace: WorkspaceId, progress: task::DownloadProgress) {
        if let Some(target) = self.tab_state.workspace_mut(workspace) {
            target.ingest_in_flight = true;
            target.ingest_stage = Some(crate::task::replays::IngestStage::Downloading(progress));
        }
    }

    /// Start the read a directory's offer was holding back, for an offer that
    /// ended without a download.
    ///
    /// The scan is retained across the offer, and the read that consumes it is
    /// started by the download that answers the offer. An offer answered with
    /// anything else has to start the read itself, or the directory is left
    /// with a listing that never fills.
    fn resume_unanswered_directory(&mut self, trigger: Option<&GameDataFollowUp>) {
        if let Some(GameDataFollowUp::Directory(workspace)) = trigger {
            self.start_directory_read(*workspace);
        }
    }

    /// The download a directory was waiting on has finished. The walk is now
    /// owed; `service_directory_reingests` starts it, so the replays skipped
    /// for want of game data appear without the user reopening the directory.
    fn note_reingest_download_finished(&mut self, workspace: WorkspaceId) {
        let Some(DirectoryReingest::AwaitingDownload { offered }) = self.directory_reingest.get_mut(&workspace) else {
            return;
        };
        let offered = std::mem::take(offered);
        self.directory_reingest.insert(workspace, DirectoryReingest::Owed { offered });
    }

    fn pick_up_confirmation_request(&mut self, ctx: &egui::Context) {
        if self.tab_state.pending_confirmation.is_none() {
            let request: Option<Option<crate::tab_state::ConfirmableAction>> =
                ctx.data_mut(|data| data.remove_temp(egui::Id::new("pending_confirmation_request")));
            if let Some(Some(action)) = request {
                self.tab_state.pending_confirmation = Some(action);
            }
        }
    }

    fn show_confirmation_dialog(&mut self, ctx: &egui::Context) {
        self.pick_up_confirmation_request(ctx);

        let Some(action) = self.tab_state.pending_confirmation.clone() else {
            return;
        };

        let message = action.confirmation_message();

        let mut confirmed = false;
        let mut dismissed = false;

        egui::Window::new(t!("ui.windows.confirm"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(message);
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button(t!("ui.buttons.yes")).clicked() {
                        confirmed = true;
                    }
                    if ui.button(t!("ui.buttons.no")).clicked() {
                        dismissed = true;
                    }
                });
            });

        if confirmed {
            let action = self.tab_state.pending_confirmation.take().unwrap();
            self.execute_confirmed_action(action, ctx);
        } else if dismissed {
            self.tab_state.pending_confirmation = None;
        }
    }

    fn execute_confirmed_action(&mut self, action: crate::tab_state::ConfirmableAction, ctx: &egui::Context) {
        match action {
            crate::tab_state::ConfirmableAction::OpenInGame { replay_path } => {
                let wows_dir = self.tab_state.persisted.read().settings.game.wows_dir.clone();
                let exe = std::path::Path::new(&wows_dir).join("WorldOfWarships.exe");
                let _ = std::process::Command::new(exe).arg(&replay_path).spawn();
                // Signal the replay parser to open the controls window.
                // App-wide: opens the single reference window regardless of workspace.
                ctx.data_mut(|data| {
                    data.insert_temp(egui::Id::new("open_replay_controls_window"), true);
                });
            }
            crate::tab_state::ConfirmableAction::ClearSessionStats => {
                self.tab_state.persisted.write().session_stats.clear();
            }
            crate::tab_state::ConfirmableAction::ClearShipSessionStats { ship_id } => {
                self.tab_state.persisted.write().session_stats.clear_ship(ship_id);
            }
            crate::tab_state::ConfirmableAction::SetAsSessionStats { replays } => {
                self.tab_state.clear_before_session_reset = true;
                self.tab_state.replays_for_session_reset = Some(replays);
            }
        }
    }

    fn show_ip_warning_dialog(&mut self, ctx: &egui::Context) {
        if !self.tab_state.show_ip_warning {
            return;
        }

        let mut proceed = false;
        let mut cancel = false;

        egui::Window::new(t!("ui.windows.network_warning"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(t!("ui.dialogs.p2p_warning"));
                ui.add_space(4.0);
                ui.hyperlink_to(t!("ui.labels.more_info"), "https://landaire.github.io/wows-toolkit/networking");
                ui.add_space(8.0);
                {
                    let mut p = self.tab_state.persisted.write();
                    ui.checkbox(&mut p.settings.collab.suppress_p2p_ip_warning, t!("ui.labels.suppress_warning"));
                }
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button(t!("ui.buttons.continue_")).clicked() {
                        proceed = true;
                    }
                    if ui.button(t!("ui.buttons.cancel")).clicked() {
                        cancel = true;
                    }
                });
            });

        if proceed {
            self.tab_state.show_ip_warning = false;
            // pending_join / pending_host were set before showing the dialog;
            // they will execute on the next frame now that the gate is lifted.
        }
        if cancel {
            self.tab_state.show_ip_warning = false;
            self.tab_state.pending_join = false;
            self.tab_state.pending_host = false;
        }
    }

    fn do_join_session(&mut self) {
        let params = crate::collab::peer::JoinParams {
            token: self.tab_state.join_session_token.trim().to_string(),
            display_name: self.tab_state.persisted.read().settings.collab.display_name.trim().to_string(),
            toolkit_version: env!("CARGO_PKG_VERSION").to_string(),
        };

        let state = Arc::clone(&self.tab_state.session_state);
        let handle = crate::collab::peer::start_peer_session(
            Arc::clone(&self.runtime),
            crate::collab::peer::PeerMode::Join(params),
            state,
        );

        self.tab_state.client_session = Some(handle);
        self.tab_state.join_session_token.clear();
    }

    fn do_host_session(&mut self) {
        let web_asset_bundle = Arc::new(parking_lot::Mutex::new(self.build_web_asset_bundle()));
        self.tab_state.web_asset_bundle = Some(Arc::clone(&web_asset_bundle));

        let params = crate::collab::peer::HostParams {
            toolkit_version: env!("CARGO_PKG_VERSION").to_string(),
            display_name: self.tab_state.persisted.read().settings.collab.display_name.clone(),
            initial_render_options: crate::collab::protocol::collab_render_options_from_saved(
                &crate::data::settings::SavedRenderOptions::default(),
            ),
            web_asset_bundle,
        };

        let session_state = Arc::clone(&self.tab_state.session_state);
        let handle = crate::collab::peer::start_peer_session(
            Arc::clone(&self.runtime),
            crate::collab::peer::PeerMode::Host(params),
            session_state,
        );

        self.tab_state.host_session = Some(handle);
    }

    /// Build a pre-serialized `PeerMessage::AssetBundle` for web clients.
    /// Returns `None` if game data isn't loaded yet.
    fn build_web_asset_bundle(&self) -> Option<Vec<u8>> {
        use crate::collab::protocol::GameFontsWire;
        use crate::collab::protocol::PeerMessage;
        use crate::collab::protocol::RgbaAssetWire;
        use crate::collab::protocol::frame_peer_message;

        let wows_data = self.tab_state.world_of_warships_data.as_ref()?;
        let wd = wows_data.read();
        let mut cache = self.tab_state.renderer_asset_cache.lock();

        let convert_icons = |icons: &std::collections::HashMap<String, crate::replay::renderer::RgbaAsset>| -> Vec<(String, RgbaAssetWire)> {
            icons.iter().map(|(k, a)| {
                (k.clone(), RgbaAssetWire { data: a.data.clone(), width: a.width, height: a.height })
            }).collect()
        };

        let version = wd.version();
        let dump_dir = wd.dump_dir.as_deref();
        let ship_icons = convert_icons(&cache.get_or_load_ship_icons(&wd.vfs, version, dump_dir));
        let plane_icons = convert_icons(&cache.get_or_load_plane_icons(&wd.vfs, version, dump_dir));
        let consumable_icons = convert_icons(&cache.get_or_load_consumable_icons(&wd.vfs, version, dump_dir));
        let ribbon_icons = convert_icons(&cache.get_or_load_ribbon_icons(&wd.vfs, version, dump_dir));
        let subribbon_icons = convert_icons(&cache.get_or_load_subribbon_icons(&wd.vfs, version, dump_dir));
        let death_cause_icons = convert_icons(&cache.get_or_load_death_cause_icons(&wd.vfs, version, dump_dir));
        let powerup_icons = convert_icons(&cache.get_or_load_powerup_icons(&wd.vfs, version, dump_dir));

        let fonts = cache.get_or_load_game_fonts(&wd.vfs, version, dump_dir);
        let game_fonts = Some(GameFontsWire {
            primary: fonts.primary_bytes.clone(),
            fallback_ko: fonts.fallback_bytes.first().cloned(),
            fallback_ja: fonts.fallback_bytes.get(1).cloned(),
            fallback_zh: fonts.fallback_bytes.get(2).cloned(),
        });

        let msg = PeerMessage::AssetBundle {
            ship_icons,
            plane_icons,
            consumable_icons,
            ribbon_icons,
            subribbon_icons,
            death_cause_icons,
            powerup_icons,
            game_fonts,
        };

        match frame_peer_message(&msg) {
            Ok(bytes) => Some(bytes),
            Err(e) => {
                tracing::warn!("Failed to serialize AssetBundle: {e}");
                None
            }
        }
    }

    fn poll_host_session_events(&mut self, ctx: &egui::Context) {
        if self.tab_state.host_session.is_none() {
            return;
        }

        // Lazily build the asset bundle once game data becomes available.
        if let Some(ref bundle_slot) = self.tab_state.web_asset_bundle
            && bundle_slot.lock().is_none()
            && let Some(bundle) = self.build_web_asset_bundle()
        {
            *bundle_slot.lock() = Some(bundle);
        }

        // Drain queued session events; the inbox sender wakes the UI on send.
        let events: Vec<crate::collab::SessionEvent> = match self.tab_state.host_session {
            Some(ref session) => session.event_inbox.read(ctx).collect(),
            None => return,
        };

        let mut session_ended = false;
        for event in events {
            match event {
                crate::collab::SessionEvent::Started => {
                    self.tab_state.toasts.lock().info(t!("ui.messages.session_started"));
                }
                crate::collab::SessionEvent::UserJoined(user) => {
                    self.tab_state.toasts.lock().info(t!("ui.messages.user_joined", name = &user.name));
                }
                crate::collab::SessionEvent::UserLeft { name, timed_out, .. } => {
                    if timed_out {
                        self.tab_state.toasts.lock().warning(t!("ui.messages.user_timeout", name = name));
                    } else {
                        self.tab_state.toasts.lock().info(t!("ui.messages.user_left", name = name));
                    }
                }
                crate::collab::SessionEvent::Ended => {
                    self.tab_state.toasts.lock().info(t!("ui.messages.session_ended"));
                    session_ended = true;
                }
                crate::collab::SessionEvent::Error(msg) => {
                    self.tab_state.toasts.lock().error(t!("ui.messages.session_error", msg = msg));
                    session_ended = true;
                }
                _ => {}
            }
        }

        if session_ended {
            // Unwire all renderers and reset their applied sync versions.
            for r in self.tab_state.replay_renderers.lock().iter() {
                let mut s = r.shared_state().lock();
                s.session_frame_tx = None;
                s.collab_replay_id = None;
                s.session_announced = false;
                s.collab_session_state = None;
                s.collab_local_tx = None;
                s.applied_render_options_version = 0;
                s.applied_annotation_sync_version = 0;
                s.applied_range_override_version = 0;
                s.applied_trail_override_version = 0;
            }
            // Unwire all tactics boards and reset their applied sync versions.
            for b in self.tab_state.tactics_boards.lock().iter_mut() {
                b.collab_local_tx = None;
                b.collab_session_state = None;
                b.collab_command_tx = None;
                b.state_arc().lock().reset_applied_sync_versions();
            }
            self.tab_state.host_session = None;
            self.tab_state.web_asset_bundle = None;
            self.tab_state.session_state.lock().clear_session_data();
        }
    }

    fn cleanup_client_session(&mut self) {
        // Remove hidden client renderers (kept alive for quick reopen)
        // and unwire visible ones.
        let mut renderers = self.tab_state.replay_renderers.lock();
        renderers.retain(|r| {
            let is_hidden_client =
                !r.open.load(Ordering::Relaxed) && r.shared_state().lock().collab_replay_id.is_some();
            !is_hidden_client
        });
        for r in renderers.iter() {
            let mut s = r.shared_state().lock();
            s.session_frame_tx = None;
            s.collab_replay_id = None;
            s.session_announced = false;
            s.collab_session_state = None;
            s.collab_local_tx = None;
            s.applied_render_options_version = 0;
            s.applied_annotation_sync_version = 0;
            s.applied_range_override_version = 0;
            s.applied_trail_override_version = 0;
        }
        drop(renderers);
        // Unwire tactics boards and reset applied sync versions.
        for b in self.tab_state.tactics_boards.lock().iter_mut() {
            b.collab_local_tx = None;
            b.collab_session_state = None;
            b.collab_command_tx = None;
            b.state_arc().lock().reset_applied_sync_versions();
        }
        self.tab_state.client_session = None;
        self.tab_state.session_state.lock().clear_session_data();
    }

    fn poll_client_session_events(&mut self, ctx: &egui::Context) {
        // Drain queued session events; the inbox sender wakes the UI on send.
        let events: Vec<crate::collab::SessionEvent> = match self.tab_state.client_session {
            Some(ref session) => session.event_inbox.read(ctx).collect(),
            None => return,
        };

        for event in events {
            match event {
                crate::collab::SessionEvent::Started => {
                    self.tab_state.toasts.lock().info(t!("ui.messages.connected_to_session"));
                }
                crate::collab::SessionEvent::SessionInfoReceived { open_replays } => {
                    tracing::debug!("SessionInfoReceived: {} open replay(s)", open_replays.len());
                    // Launch client viewer windows for each open replay (up to 2).
                    let saved_options = self.tab_state.persisted.read().settings.renderer.clone();
                    let suppress = Arc::clone(&self.tab_state.suppress_gpu_encoder_warning);
                    let is_debug_mode = self.tab_state.persisted.read().settings.app.debug_mode;
                    for replay in open_replays.into_iter().take(2) {
                        self.tab_state.toasts.lock().info(t!("ui.messages.joined_session", name = &replay.replay_name));
                        let viewer = crate::replay::renderer::launch_client_renderer(
                            replay.replay_name,
                            replay.map_image_png,
                            replay.game_version,
                            &saved_options,
                            Arc::clone(&suppress),
                            self.tab_state.world_of_warships_data.as_ref(),
                            &self.tab_state.renderer_asset_cache,
                            self.tab_state.window_settings.clone(),
                            self.tab_state.save_notify.clone(),
                            is_debug_mode,
                        );
                        if let Some(ref client_handle) = self.tab_state.client_session {
                            let (frame_tx, frame_rx) = std::sync::mpsc::sync_channel(2);
                            let viewport_id = egui::ViewportId::from_hash_of(&*viewer.title);
                            let mut state = viewer.shared_state().lock();
                            state.collab_replay_id = Some(replay.replay_id);
                            state.collab_session_state = Some(std::sync::Arc::clone(&self.tab_state.session_state));
                            state.collab_local_tx = Some(client_handle.local_tx.clone());
                            state.collab_frame_rx = Some(frame_rx);
                            self.tab_state.session_state.lock().register_viewport_sink(
                                replay.replay_id,
                                crate::collab::ViewportSink { frame_tx: Some(frame_tx), viewport_id },
                            );
                        }
                        self.tab_state.replay_renderers.lock().push(viewer);
                    }
                }
                crate::collab::SessionEvent::ReplayOpened {
                    replay_id,
                    replay_name,
                    map_image_png,
                    game_version,
                    ..
                } => {
                    // Spam protection: track timestamps of ReplayOpened events.
                    let now = std::time::Instant::now();
                    self.tab_state.replay_open_timestamps.push_back(now);
                    while self
                        .tab_state
                        .replay_open_timestamps
                        .front()
                        .is_some_and(|t| now.duration_since(*t).as_secs() >= 10)
                    {
                        self.tab_state.replay_open_timestamps.pop_front();
                    }
                    if self.tab_state.replay_open_timestamps.len() >= 5 {
                        self.tab_state.toasts.lock().error(t!("ui.messages.replay_spam_protection"));
                        if let Some(ref handle) = self.tab_state.client_session {
                            let _ = handle.command_tx.send(crate::collab::SessionCommand::Stop);
                        }
                        self.tab_state.client_session = None;
                        self.tab_state.replay_open_timestamps.clear();
                        return;
                    }

                    // Cap at 2 client viewer windows — close oldest if needed.
                    let mut renderers = self.tab_state.replay_renderers.lock();
                    let client_count =
                        renderers.iter().filter(|r| r.shared_state().lock().collab_replay_id.is_some()).count();
                    if client_count >= 2 {
                        // Close the oldest client viewer.
                        if let Some(pos) =
                            renderers.iter().position(|r| r.shared_state().lock().collab_replay_id.is_some())
                        {
                            renderers[pos].open.store(false, Ordering::Relaxed);
                            renderers.remove(pos);
                        }
                    }
                    drop(renderers);

                    let saved_options = self.tab_state.persisted.read().settings.renderer.clone();
                    let suppress = Arc::clone(&self.tab_state.suppress_gpu_encoder_warning);
                    let is_debug_mode = self.tab_state.persisted.read().settings.app.debug_mode;
                    self.tab_state.toasts.lock().info(t!("ui.messages.host_opened_replay", name = replay_name));
                    let viewer = crate::replay::renderer::launch_client_renderer(
                        replay_name,
                        map_image_png,
                        game_version,
                        &saved_options,
                        suppress,
                        self.tab_state.world_of_warships_data.as_ref(),
                        &self.tab_state.renderer_asset_cache,
                        self.tab_state.window_settings.clone(),
                        self.tab_state.save_notify.clone(),
                        is_debug_mode,
                    );
                    // Wire the client viewer to the session.
                    if let Some(ref client_handle) = self.tab_state.client_session {
                        let (frame_tx, frame_rx) = std::sync::mpsc::sync_channel(2);
                        let viewport_id = egui::ViewportId::from_hash_of(&*viewer.title);
                        let mut state = viewer.shared_state().lock();
                        state.collab_replay_id = Some(replay_id);
                        state.collab_session_state = Some(std::sync::Arc::clone(&self.tab_state.session_state));
                        state.collab_local_tx = Some(client_handle.local_tx.clone());
                        state.collab_frame_rx = Some(frame_rx);
                        self.tab_state.session_state.lock().register_viewport_sink(
                            replay_id,
                            crate::collab::ViewportSink { frame_tx: Some(frame_tx), viewport_id },
                        );
                    }
                    self.tab_state.replay_renderers.lock().push(viewer);
                }
                crate::collab::SessionEvent::ReplayClosed { replay_id } => {
                    // Close the matching client viewer.
                    let mut renderers = self.tab_state.replay_renderers.lock();
                    if let Some(pos) =
                        renderers.iter().position(|r| r.shared_state().lock().collab_replay_id == Some(replay_id))
                    {
                        renderers[pos].open.store(false, Ordering::Relaxed);
                        renderers.remove(pos);
                    }
                    self.tab_state.session_state.lock().viewport_sinks.remove(&replay_id);
                    self.tab_state.toasts.lock().info(t!("ui.messages.host_closed_replay"));
                }
                crate::collab::SessionEvent::Error(msg) => {
                    self.tab_state.toasts.lock().error(t!("ui.messages.session_error_generic", msg = msg));
                    self.cleanup_client_session();
                    return;
                }
                crate::collab::SessionEvent::Rejected(reason) => {
                    self.tab_state.toasts.lock().error(t!("ui.messages.session_rejected", reason = reason));
                    self.tab_state.client_session = None;
                    return;
                }
                crate::collab::SessionEvent::Ended => {
                    self.tab_state.toasts.lock().info(t!("ui.messages.session_ended"));
                    self.cleanup_client_session();
                    return;
                }
                _ => {}
            }
        }
    }

    fn set_data_sharing_choice(&mut self, mode: DataSharingMode) {
        self.build_consent_window_open = false;
        {
            let mut p = self.tab_state.persisted.write();
            p.settings.app.build_consent_window_shown = true;
            p.settings.app.replay_consent_prompt_shown = true;
            p.settings.integrations.data_sharing_mode = mode;
        }
        self.tab_state.send_replay_consent_changed();
    }

    /// Focus the given dock tab, opening it first if it isn't currently docked.
    /// Only closeable tabs (`Tab::Search`, non-live `Tab::Replays`) need a
    /// fallback push: every other variant is always docked.
    fn focus_tab(&mut self, tab: &Tab) {
        if let Some(loc) = self.dock_state.find_tab(tab) {
            let _ = self.dock_state.set_active_tab(loc);
            return;
        }
        match tab {
            Tab::Search => self.dock_state.push_to_focused_leaf(Tab::Search),
            Tab::Replays(id) => self.dock_state.push_to_focused_leaf(Tab::Replays(*id)),
            _ => return,
        }
        if let Some(loc) = self.dock_state.find_tab(tab) {
            let _ = self.dock_state.set_active_tab(loc);
        }
    }

    /// Parses and loads a replay file picked manually (via the palette or a file dialog).
    fn open_replay_path(&mut self, path: PathBuf) {
        if let Some(deps) = self.tab_state.replay_dependencies() {
            update_background_task!(
                self.tab_state.background_tasks,
                deps.parse_replay_from_path(path, crate::task::ReplaySource::ManualOpen)
            );
        }
    }

    /// Open `root` as a replay workspace: focus its tab, opening one if this
    /// directory is not already listed, and start the walk that fills it.
    ///
    /// The ingest needs game data, a database and a runtime; without all three
    /// the tab still opens, showing an empty listing, rather than the pick
    /// silently doing nothing.
    fn open_replay_directory(&mut self, root: PathBuf) {
        // `build_replay_parser_tab` makes the tab it draws the active
        // workspace, so focusing the tab is all this needs to do.
        let id = self.tab_state.open_directory_workspace(root.clone());
        self.focus_tab(&Tab::Replays(id));
        self.start_directory_scan(id, root);
    }

    /// Whether `id` already has a scan, either running or taken and waiting on
    /// the download offer.
    ///
    /// The offer's window is the part `ingest_in_flight` does not cover: it is
    /// cleared when the scan finishes and set again only when the read starts.
    /// The offer is not modal, and re-picking the directory resolves to this
    /// same workspace, so a second scan there would overwrite the retained one
    /// and its read would start without the offer ever being answered.
    fn scan_already_taken(&self, id: WorkspaceId) -> bool {
        self.tab_state.workspace(id).is_some_and(|workspace| workspace.ingest_in_flight)
            || self.pending_scans.contains_key(&id)
    }

    /// Start the scan that counts `root`'s replays and groups them by build,
    /// without touching which tab is focused. Returns whether it started: a
    /// scan this workspace already has, or a missing prerequisite, leaves it
    /// for the caller to retry rather than losing it.
    ///
    /// The three-way prerequisite check covers the read stage that follows, not
    /// the scan, which needs only `deps`: refusing at the point the user picked
    /// a directory beats scanning a thousand files and then discovering there is
    /// nowhere to put them.
    fn start_directory_scan(&mut self, id: WorkspaceId, root: PathBuf) -> bool {
        if self.scan_already_taken(id) {
            return false;
        }

        // One match over the three options, so the name of the missing
        // prerequisite comes from the same evaluation that found it absent.
        let ingest_deps = match (
            self.tab_state.replay_dependencies(),
            self.tab_state.db_pool.as_ref(),
            self.tab_state.tokio_runtime.as_ref(),
        ) {
            (Some(deps), Some(_pool), Some(_rt)) => Ok(deps),
            (None, _, _) => Err("game data"),
            (_, None, _) => Err("database pool"),
            (_, _, None) => Err("tokio runtime"),
        };
        let deps = match ingest_deps {
            Ok(resolved) => resolved,
            Err(missing) => {
                warn!("cannot ingest replay directory {}: {missing} is not available", root.display());
                return false;
            }
        };

        if let Some(workspace) = self.tab_state.workspace_mut(id) {
            workspace.ingest_in_flight = true;
        }
        update_background_task!(
            self.tab_state.background_tasks,
            Some(crate::task::scan::start_scan_directory(deps, id, root))
        );
        true
    }

    /// Start the read stage over a scan already taken for `workspace`.
    ///
    /// The scan is taken rather than borrowed: a read consumes it, and a scan
    /// left behind would start a second read of the same directory. It is taken
    /// only once the read is going to start, so a refusal keeps it: the offer
    /// this is called from is answered once, and a scan dropped here is a
    /// listing that never fills.
    fn start_directory_read(&mut self, workspace: WorkspaceId) -> bool {
        // Symmetric with `start_directory_scan`, and refused before the scan is
        // taken so a run that cannot start now keeps it: a re-opened directory
        // resolves to the workspace it is already open as, so an open the user
        // repeats while a run is going would otherwise put two on one listing.
        let already_running = self.tab_state.workspace(workspace).is_some_and(|workspace| workspace.ingest_in_flight);
        if already_running {
            return false;
        }
        let Some(root) = self.pending_scans.get(&workspace).map(|scan| scan.root.clone()) else {
            return false;
        };
        let ingest_deps = match (
            self.tab_state.replay_dependencies(),
            self.tab_state.db_pool.as_ref(),
            self.tab_state.tokio_runtime.as_ref(),
        ) {
            (Some(deps), Some(pool), Some(rt)) => Ok((deps, pool.clone(), Arc::clone(rt))),
            (None, _, _) => Err("game data"),
            (_, None, _) => Err("database pool"),
            (_, _, None) => Err("tokio runtime"),
        };
        let (deps, pool, rt) = match ingest_deps {
            Ok(resolved) => resolved,
            Err(missing) => {
                warn!("cannot read replay directory {}: {missing} is not available", root.display());
                self.tab_state.set_ingest_finished(workspace);
                return false;
            }
        };
        let Some(scan) = self.pending_scans.remove(&workspace) else {
            return false;
        };

        if let Some(target) = self.tab_state.workspace_mut(workspace) {
            target.ingest_in_flight = true;
        }
        update_background_task!(
            self.tab_state.background_tasks,
            Some(crate::task::start_read_directory(deps, pool, rt, workspace, scan))
        );
        true
    }

    /// Dispatch a palette-picked action against app state.
    fn dispatch_palette_action(&mut self, ctx: &egui::Context, action: crate::ui::command_palette::PaletteAction) {
        use crate::db::index::query_model::Chip;
        use crate::db::index::query_model::Connector;
        use crate::db::index::query_model::Field;
        use crate::db::index::query_model::Group;
        use crate::db::index::query_model::Op;
        use crate::db::index::query_model::Query;
        use crate::db::index::query_model::Value;
        use crate::ui::command_palette::PaletteAction;

        match action {
            PaletteAction::ViewArmor { ship_index } => {
                self.tab_state.armor_viewer.pending_ship_selection = Some(ship_index);
                self.focus_tab(&Tab::ArmorViewer);
            }
            PaletteAction::MyMatchesInShip { ship_id } => {
                self.tab_state.pending_search_query = Some(Query {
                    groups: vec![Group {
                        chips: vec![Chip { field: Field::SelfShip, op: Op::Is, value: Value::Ship(ship_id) }],
                    }],
                    connector: Connector::And,
                });
                self.focus_tab(&Tab::Search);
            }
            PaletteAction::FindMatchesWithPlayer { account_id } => {
                self.tab_state.pending_search_query = Some(Query {
                    groups: vec![Group {
                        chips: vec![Chip {
                            field: Field::PlayerPresent,
                            op: Op::Present,
                            value: Value::Account(account_id),
                        }],
                    }],
                    connector: Connector::And,
                });
                self.focus_tab(&Tab::Search);
            }
            PaletteAction::OpenSearchTab => self.focus_tab(&Tab::Search),
            PaletteAction::OpenReplay { path } => self.open_replay_path(path),
            PaletteAction::OpenReplayFile => {
                if let Some(path) = rfd::FileDialog::new().add_filter("WoWs Replays", &["wowsreplay"]).pick_file() {
                    self.open_replay_path(path);
                }
            }
            PaletteAction::OpenReplayDirectory => {
                if let Some(root) = rfd::FileDialog::new().pick_folder() {
                    self.open_replay_directory(root);
                }
            }
            PaletteAction::IndexAllReplays => {
                let reindex_deps = match (
                    self.tab_state.db_pool.as_ref(),
                    self.tab_state.tokio_runtime.as_ref(),
                    self.tab_state.wows_data_map.as_ref(),
                ) {
                    (Some(pool), Some(rt), Some(wows_data_map)) => {
                        Some((pool.clone(), Arc::clone(rt), wows_data_map.clone()))
                    }
                    _ => None,
                };
                if let Some((pool, rt, wows_data_map)) = reindex_deps {
                    update_background_task!(
                        self.tab_state.background_tasks,
                        Some(crate::task::start_reconcile_index(
                            wows_data_map,
                            Arc::clone(&self.tab_state.twitch_state),
                            pool,
                            rt,
                            Arc::clone(&self.tab_state.personal_rating_data),
                            false,
                        ))
                    );
                }
            }
            PaletteAction::GoToTab(tab) => self.focus_tab(&tab),
            PaletteAction::OpenSearchWith(query) => {
                self.tab_state.pending_search_query = Some(query);
                self.focus_tab(&Tab::Search);
            }
            PaletteAction::SetTheme(choice) => {
                self.tab_state.persisted.write().settings.app.theme = choice;
                crate::ui::theme::apply(ctx, choice);
            }
            // Handled by the render loop before it reaches dispatch: entering a
            // sub-mode keeps the palette open instead of running an action.
            PaletteAction::EnterSub(_) => {}
        }
    }
}

impl eframe::App for WowsToolkitApp {
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.update_impl(ctx, frame);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        egui::Panel::top("top_panel").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                let is_web = cfg!(target_arch = "wasm32");
                if !is_web {
                    ui.menu_button(t!("ui.menu.file"), |ui| {
                        if ui.button(t!("ui.menu.check_updates")).clicked() {
                            self.manual_update_requested = true;
                            ui.close_kind(UiKind::Menu);
                        }
                        if ui.button(t!("ui.menu.about")).clicked() {
                            self.show_about_window = true;
                            ui.close_kind(UiKind::Menu);
                        }
                        if ui.button(t!("ui.menu.quit")).clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                    ui.add_space(16.0);
                }

                if ui.button(wt_translations::icon_t(icons::BUG, &t!("ui.buttons.create_issue"))).clicked() {
                    ui.ctx().open_url(OpenUrl::new_tab("https://github.com/landaire/wows-toolkit/issues/new/choose"));
                }

                if ui.button(wt_translations::icon_t(icons::DISCORD_LOGO, &t!("ui.buttons.discord"))).clicked() {
                    ui.ctx().open_url(OpenUrl::new_tab(rot13("uggcf://qvfpbeq.tt/EWXjXHw7eu")));
                }
            });
        });

        egui::Panel::bottom("status_panel").show(ui, |ui| {
            self.build_bottom_panel(ui);
        });

        self.tab_state.active_theme = ctx.theme();

        egui::CentralPanel::default().show(ui, |ui| {
            DockArea::new(&mut self.dock_state)
                .style(crate::ui::theme::style::dock_style(ui.style().as_ref()))
                .allowed_splits(egui_dock::AllowedSplits::None)
                .show_leaf_collapse_buttons(false)
                .show_leaf_close_all_buttons(false)
                .show_close_buttons(true)
                .show_inside(ui, &mut ToolkitTabViewer { tab_state: &mut self.tab_state });
        });

        if self.tab_state.command_palette.state.open {
            use crate::ui::command_palette::PaletteAction;
            use crate::ui::command_palette::PaletteMode;
            use crate::ui::command_palette::SubKind;

            // Split borrows: `palette` needs `&mut`, the others just `&`, all
            // disjoint fields of `self`/`self.tab_state` -- take the shared
            // borrows first so they don't overlap the palette's `&mut`.
            let db_pool = self.tab_state.db_pool.as_ref();
            let ship_catalog = self.tab_state.ship_catalog.as_ref();
            let rt = self.runtime.as_ref();
            let palette = &mut self.tab_state.command_palette;
            let (entries, hint): (Vec<egui_palette::Entry<'static, PaletteAction>>, &str) = match palette.mode {
                PaletteMode::Root => {
                    palette.state.bypass_filter = false;
                    (palette.root_entries(), "Search ships, players, replays, commands")
                }
                PaletteMode::Sub(kind) => {
                    palette.state.bypass_filter = true;
                    let hint = match kind {
                        SubKind::Players => "Search players",
                        SubKind::MyShips => "Search ships you've played",
                        SubKind::ArmorShips => "Search ships to view armor",
                    };
                    (palette.sub_entries(kind, db_pool, rt, ship_catalog), hint)
                }
            };

            if let Some(outcome) = egui_palette::show(&ctx, &mut self.tab_state.command_palette.state, &entries, hint) {
                match outcome {
                    egui_palette::Outcome::Picked { data, .. } => match data {
                        PaletteAction::EnterSub(kind) => {
                            self.tab_state.command_palette.enter_sub(kind);
                        }
                        other => {
                            self.tab_state.command_palette.state.close();
                            self.tab_state.command_palette.mode = PaletteMode::Root;
                            self.dispatch_palette_action(&ctx, other);
                        }
                    },
                    egui_palette::Outcome::Dismissed(_) => {
                        // Esc: back out of a submenu first, else close.
                        let cp = &mut self.tab_state.command_palette;
                        if matches!(cp.mode, PaletteMode::Sub(_)) {
                            cp.back_to_root();
                        } else {
                            cp.state.close();
                        }
                    }
                    egui_palette::Outcome::SubAction { .. } => {
                        self.tab_state.command_palette.state.close();
                    }
                }
            }
        }

        if std::mem::take(&mut self.tab_state.pending_focus_search) {
            self.focus_tab(&Tab::Search);
        }
    }

    fn on_exit(&mut self) {
        // Signal the background save task to do a final save, then await completion.
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.save_task_handle.take() {
            self.runtime.block_on(async {
                match tokio::time::timeout(std::time::Duration::from_secs(5), handle).await {
                    Ok(Ok(())) => info!("Final save completed"),
                    Ok(Err(e)) => error!("Save task panicked: {e}"),
                    Err(_) => error!("Final save timed out after 5 seconds"),
                }
            });
        }
    }
}

/// Load app state from the legacy `app.ron` file on disk.
///
/// The file is a RON-serialized `HashMap<String, String>` (eframe's key-value
/// storage). The app state lives under the `"app"` key as a nested RON string.
///
/// Returns a `LegacyWowsToolkitApp` which must be converted via
/// [`into_new_state()`](crate::data::legacy_settings::LegacyWowsToolkitApp::into_new_state).
fn load_from_app_ron() -> Option<crate::data::legacy_settings::LegacyWowsToolkitApp> {
    let dir = crate::storage_dir()?;
    let ron_path = dir.join("app.ron");
    let contents = std::fs::read_to_string(&ron_path).ok()?;
    let kv: std::collections::HashMap<String, String> = ron::from_str(&contents).ok()?;
    let app_str = kv.get("app")?;
    if app_str.is_empty() {
        return None;
    }
    match ron::from_str::<crate::data::legacy_settings::LegacyWowsToolkitApp>(app_str) {
        Ok(app) => {
            info!("Loaded legacy app state from {}", ron_path.display());
            Some(app)
        }
        Err(e) => {
            error!("Failed to deserialize app.ron: {e}");
            None
        }
    }
}

/// Translate a map name to a human-readable display name using game metadata.
///
/// Falls back to a prettified version of the raw name if game data is unavailable.
fn translate_map_display_name(map_name: &str, wows_data: &Option<crate::data::wows_data::SharedWoWsData>) -> String {
    if let Some(wd) = wows_data {
        let wd = wd.read();
        if let Some(ref gm) = wd.game_metadata {
            return wowsunpack::game_params::translations::translate_map_name(map_name, gm.as_ref());
        }
    }
    // Fallback: strip "spaces/" prefix and leading number prefix, replace underscores.
    let bare = map_name.strip_prefix("spaces/").unwrap_or(map_name);
    let stripped = bare.find('_').map(|i| &bare[i + 1..]).unwrap_or(bare);
    stripped.replace('_', " ")
}

fn build_about_window(ui: &mut egui::Ui) {
    ui.vertical(|ui| {
        ui.label(t!("ui.labels.made_by"));
        ui.label(t!("ui.labels.credits"));
        ui.horizontal(|ui| {
            ui.label(t!("ui.labels.pr_credits"));
            ui.hyperlink_to(t!("ui.labels.more_info"), "https://wows-numbers.com/personal/rating");
        });
        if ui.button(t!("ui.buttons.view_github")).clicked() {
            ui.ctx().open_url(OpenUrl::new_tab("https://github.com/landaire/wows-toolkit"));
        }

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.label(t!("ui.labels.powered_by"));
            ui.hyperlink_to("egui", "https://github.com/emilk/egui");
            ui.label(t!("ui.labels.and"));
            ui.hyperlink_to("eframe", "https://github.com/emilk/egui/tree/master/crates/eframe");
            ui.label(".");
        });
    });
}

fn build_error_window(ui: &mut egui::Ui, error: &str) {
    ui.vertical(|ui| {
        ui.label(wt_translations::icon_t(icons::WARNING, &t!("ui.labels.error_occurred")));
        ui.label(error);
    });
}

/// Helper function to mitigate https://github.com/emilk/egui/issues/7434.
///
/// Load system fonts that cover scripts egui's built-in fonts lack (CJK, Thai,
/// Cyrillic, etc.) and append them as low-priority fallbacks in the Proportional
/// family. Fonts that don't exist on the current system are silently skipped.
#[cfg(not(target_arch = "wasm32"))]
fn add_system_font_fallbacks(fonts: &mut egui::FontDefinitions) {
    // (logical name, file path) — tried in order per platform.
    #[cfg(target_os = "windows")]
    let candidates: &[(&str, &str)] = &[
        ("sys_cjk_sc", r"C:\Windows\Fonts\msyh.ttc"), // Microsoft YaHei — Simplified Chinese + Latin
        ("sys_cjk_tc", r"C:\Windows\Fonts\msjh.ttc"), // Microsoft JhengHei — Traditional Chinese
        ("sys_cjk_jp", r"C:\Windows\Fonts\YuGothR.ttc"), // Yu Gothic — Japanese
        ("sys_cjk_kr", r"C:\Windows\Fonts\malgun.ttf"), // Malgun Gothic — Korean
        ("sys_thai", r"C:\Windows\Fonts\leelawui.ttf"), // Leelawadee UI — Thai
    ];

    #[cfg(target_os = "macos")]
    let candidates: &[(&str, &str)] = &[
        ("sys_cjk_sc", "/System/Library/Fonts/PingFang.ttc"), // PingFang — CJK
        ("sys_cjk_jp", "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc"), // Hiragino Sans
        ("sys_thai", "/System/Library/Fonts/Supplemental/Ayuthaya.ttf"), // Thai
    ];

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let candidates: &[(&str, &str)] = &[
        ("sys_cjk", "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc"),
        ("sys_cjk", "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc"),
        ("sys_cjk", "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc"),
        ("sys_thai", "/usr/share/fonts/truetype/noto/NotoSansThai-Regular.ttf"),
        ("sys_thai", "/usr/share/fonts/noto/NotoSansThai-Regular.ttf"),
    ];

    for (name, path) in candidates {
        // Skip if we already loaded a font under this logical name (e.g. multiple
        // candidate paths for the same script on Linux).
        if fonts.font_data.contains_key(*name) {
            continue;
        }
        // Memory-map the fallback fonts instead of reading them into the heap.
        // These CJK/Thai files total ~65 MiB, but only the glyph pages actually
        // rendered (a few player names) ever fault into RAM. skrifa borrows the
        // bytes for rasterization, so from_static avoids a copy. The mapping is
        // intentionally leaked: the fonts live for the whole process and egui
        // requires a 'static slice.
        let Ok(file) = std::fs::File::open(path) else {
            continue;
        };
        // SAFETY: these are read-only system font files, not mutated while the
        // app runs.
        let Ok(mmap) = (unsafe { memmap2::Mmap::map(&file) }) else {
            continue;
        };
        let leaked: &'static memmap2::Mmap = Box::leak(Box::new(mmap));
        let bytes: &'static [u8] = &leaked[..];
        fonts.font_data.insert(name.to_string(), egui::FontData::from_static(bytes).into());
        if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            family.push(name.to_string());
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn add_system_font_fallbacks(_fonts: &mut egui::FontDefinitions) {
    // No filesystem access on WASM — nothing to load.
}

/// If this returns true, the app should early return in the `update()` function
/// or call `wgpu::Device::poll()`
/// ROT13 transform (its own inverse). The community Discord invite is stored
/// ROT13-encoded so scrapers can't harvest the raw `discord.gg/...` invite from the
/// source or the compiled binary; it is decoded only when the user clicks to open it.
fn rot13(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A'..='Z' => (((c as u8 - b'A' + 13) % 26) + b'A') as char,
            'a'..='z' => (((c as u8 - b'a' + 13) % 26) + b'a') as char,
            _ => c,
        })
        .collect()
}

pub fn mitigate_wgpu_mem_leak(ctx: &egui::Context) -> bool {
    let mut is_minimized = false;
    ctx.input(|reader| {
        is_minimized = reader.viewport().minimized.unwrap_or_default();
    });

    is_minimized
}

#[cfg(test)]
mod tab_viewer_tests {
    use super::*;

    /// Named `is_closeable`, not `closeable`, on purpose: `TabViewer::closeable`
    /// is dead code in egui_dock 0.20.1 and is never called, so an impl under
    /// that name would compile, pass every other test, and (because the real
    /// `is_closeable` default is `true`) make every top-level tab closeable,
    /// Settings included. This test only exercises `is_closeable`, so renaming
    /// the outer viewer's impl back to `closeable` makes it fail.
    #[test]
    fn only_search_and_non_live_replays_tabs_are_closeable() {
        let mut tab_state = TabState::default();
        let viewer = ToolkitTabViewer { tab_state: &mut tab_state };

        for tab in [
            Tab::Unpacker,
            Tab::Settings,
            Tab::PlayerTracker,
            Tab::ModManager,
            Tab::ArmorViewer,
            Tab::Stats,
            Tab::Replays(WorkspaceId::LIVE),
        ] {
            assert!(!viewer.is_closeable(&tab), "must not be closeable: {}", viewer.tab_title(&tab));
        }

        for tab in [Tab::Search, Tab::Replays(WorkspaceId(1))] {
            assert!(viewer.is_closeable(&tab), "must be closeable: {}", viewer.tab_title(&tab));
        }
    }

    /// Two directory tabs would otherwise carry the same static label and be
    /// indistinguishable, so the title has to come from the workspace's own
    /// root. Asserts the full string, not a substring, because the icon and
    /// the shortening are both part of what the tab strip shows.
    #[test]
    fn a_directory_tab_is_titled_with_its_shortened_root() {
        let mut tab_state = TabState::default();
        let id = tab_state.open_directory_workspace(PathBuf::from("G:/dev/wows/replays"));
        let viewer = ToolkitTabViewer { tab_state: &mut tab_state };

        let title = viewer.tab_title(&Tab::Replays(id));

        assert_eq!(title, wt_translations::icon_t(icons::FOLDER_OPEN, "G:/d/w/replays"));
        assert_ne!(
            title,
            viewer.tab_title(&Tab::Replays(WorkspaceId::LIVE)),
            "a directory tab must not share the live tab's label"
        );
    }

    /// Two directories whose names differ only deep in the path still have to
    /// produce different tab strip entries.
    #[test]
    fn two_directory_tabs_get_different_titles() {
        let mut tab_state = TabState::default();
        let first = tab_state.open_directory_workspace(PathBuf::from("D:/archive/2025"));
        let second = tab_state.open_directory_workspace(PathBuf::from("D:/archive/2026"));
        let viewer = ToolkitTabViewer { tab_state: &mut tab_state };

        assert_eq!(viewer.tab_title(&Tab::Replays(first)), wt_translations::icon_t(icons::FOLDER_OPEN, "D:/a/2025"));
        assert_eq!(viewer.tab_title(&Tab::Replays(second)), wt_translations::icon_t(icons::FOLDER_OPEN, "D:/a/2026"));
    }

    /// The live tab keeps the label it has always had, whatever its root is.
    #[test]
    fn the_live_replays_tab_keeps_its_translated_label() {
        let mut tab_state = TabState::default();
        tab_state.live_workspace.root = Some(PathBuf::from("G:/dev/wows/replays"));
        let viewer = ToolkitTabViewer { tab_state: &mut tab_state };

        assert_eq!(
            viewer.tab_title(&Tab::Replays(WorkspaceId::LIVE)),
            wt_translations::icon_t(icons::MAGNIFYING_GLASS, "Replay Inspector")
        );
    }

    /// A tab can outlive its workspace by a frame (egui_dock removes the tab
    /// after `on_close` has already dropped the workspace), and a workspace
    /// can exist before its root is known. Neither may panic or render a
    /// blank tab.
    #[test]
    fn a_directory_tab_without_a_resolvable_root_falls_back_to_a_generic_label() {
        let mut tab_state = TabState::default();
        let rootless = WorkspaceId(7);
        tab_state.workspaces.insert(rootless, crate::ui::replay_parser::ReplayWorkspace::new(None));
        let empty_root = WorkspaceId(8);
        tab_state.workspaces.insert(empty_root, crate::ui::replay_parser::ReplayWorkspace::new(Some(PathBuf::new())));
        let viewer = ToolkitTabViewer { tab_state: &mut tab_state };

        let expected = wt_translations::icon_t(icons::FOLDER_OPEN, "Replay Directory");
        assert_eq!(viewer.tab_title(&Tab::Replays(rootless)), expected);
        assert_eq!(viewer.tab_title(&Tab::Replays(empty_root)), expected);
        assert_eq!(
            viewer.tab_title(&Tab::Replays(WorkspaceId(9999))),
            expected,
            "a closed workspace still needs a title"
        );
        assert!(!expected.trim().is_empty());
    }
}

#[cfg(test)]
mod replay_tab_search_tests {
    use std::path::Path;

    use jiff::Timestamp;
    use sqlx::sqlite::SqlitePoolOptions;
    use wows_replays::types::ArenaId;

    use super::*;
    use crate::db::index::query;
    use crate::db::index::query_model::Chip;
    use crate::db::index::query_model::Field;
    use crate::db::index::query_model::Op;
    use crate::db::index::query_model::Value;
    use crate::db::index::rows::MatchOutcome;
    use crate::db::index::rows::ObjectiveMatch;
    use crate::db::index::rows::ReplayRecord;
    use crate::db::index::rows::SourceKind;
    use crate::ui::replay_parser::ReplayWorkspace;

    fn now() -> Timestamp {
        Timestamp::from_second(1_700_000_000).expect("a fixed valid timestamp")
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a current-thread runtime")
    }

    fn mem_pool(rt: &tokio::runtime::Runtime) -> sqlx::SqlitePool {
        rt.block_on(async {
            let pool = SqlitePoolOptions::new()
                .max_connections(1)
                .connect("sqlite::memory:")
                .await
                .expect("an in-memory sqlite pool");
            sqlx::migrate!("../wows-toolkit-config/migrations").run(&pool).await.expect("migrations apply");
            pool
        })
    }

    fn sample_match(arena: i64) -> ObjectiveMatch {
        ObjectiveMatch {
            arena_id: ArenaId::new(arena),
            timestamp: now(),
            map: "Ocean".into(),
            game_mode: "Domination".into(),
            game_type: "pvp".into(),
            match_group: "pvp".into(),
            version_build: Some(1234),
        }
    }

    fn sample_record(arena: i64, source: SourceId, path: &str) -> ReplayRecord {
        ReplayRecord {
            arena_id: ArenaId::new(arena),
            source_id: source,
            replay_path: PathBuf::from(path),
            file_mtime: Some(42),
            outcome: MatchOutcome::Win,
            self_account_id: None,
            self_ship_id: None,
            self_survived: Some(true),
            self_damage: Some(10),
            self_kills: Some(1),
            self_pr: None,
            results_available: true,
            indexed_at: now(),
        }
    }

    /// Seed one match per source so a query that ignores the source scope
    /// returns both arenas, and one that honours it returns exactly one.
    fn seed(rt: &tokio::runtime::Runtime, pool: &sqlx::SqlitePool, arena: i64, source: SourceId, path: &str) {
        rt.block_on(async {
            query::upsert_match(pool, &sample_match(arena)).await.expect("the match inserts");
            query::upsert_record(pool, &sample_record(arena, source, path)).await.expect("the record inserts");
        });
    }

    /// Two open directory tabs with distinct sources: the entry on tab B must
    /// carry B's source, not A's and not the live source.
    #[test]
    fn searching_from_a_directory_tab_scopes_to_that_directory_source() {
        let mut tab_state = TabState::default();
        let a = tab_state.open_directory_workspace(PathBuf::from("D:/archive/a"));
        let b = tab_state.open_directory_workspace(PathBuf::from("D:/archive/b"));
        tab_state.workspace_mut(a).expect("a is open").source = Some(SourceId(11));
        tab_state.workspace_mut(b).expect("b is open").source = Some(SourceId(22));

        let mut viewer = ToolkitTabViewer { tab_state: &mut tab_state };
        assert_eq!(viewer.resolve_tab_search_scope(a), TabSearchScope::Source(SourceId(11)));
        let scope = viewer.resolve_tab_search_scope(b);
        assert_eq!(scope, TabSearchScope::Source(SourceId(22)), "tab B must scope to B's own source");

        let TabSearchScope::Source(source) = scope else {
            panic!("b has a source, so its scope must be resolved");
        };
        viewer.search_replay_source(source);

        let query = tab_state.pending_search_query.take().expect("the Search tab must be handed a query");
        assert_eq!(
            query.groups.iter().map(|group| group.chips.as_slice()).collect::<Vec<_>>(),
            vec![[Chip { field: Field::Group, op: Op::Is, value: Value::Source(SourceId(22)) }].as_slice()],
            "the query must hold exactly one source chip, naming B"
        );
        assert!(tab_state.pending_focus_search, "the Search tab must also be focused");
    }

    /// The scope has to survive the whole way into the SQL, not just into the
    /// query struct: run the dispatched query against a seeded index and check
    /// only the tab's own directory comes back.
    #[test]
    fn the_dispatched_query_returns_only_that_directorys_matches() {
        let rt = runtime();
        let pool = mem_pool(&rt);
        let mine = rt
            .block_on(query::ensure_source(&pool, "Mine", SourceKind::ImportedDir, Path::new("D:/mine"), now()))
            .expect("the source is created");
        let other = rt
            .block_on(query::ensure_source(&pool, "Other", SourceKind::ImportedDir, Path::new("D:/other"), now()))
            .expect("the source is created");
        seed(&rt, &pool, 100, mine, "D:/mine/a.wowsreplay");
        seed(&rt, &pool, 200, other, "D:/other/b.wowsreplay");

        let mut tab_state = TabState::default();
        let id = tab_state.open_directory_workspace(PathBuf::from("D:/mine"));
        tab_state.workspace_mut(id).expect("the workspace is open").source = Some(mine);

        let mut viewer = ToolkitTabViewer { tab_state: &mut tab_state };
        let TabSearchScope::Source(source) = viewer.resolve_tab_search_scope(id) else {
            panic!("the workspace has a source, so its scope must be resolved");
        };
        viewer.search_replay_source(source);
        let query = tab_state.pending_search_query.take().expect("the Search tab must be handed a query");

        let hits = rt.block_on(query::search_by_query(&pool, &query, 500)).expect("the search runs");
        let arenas: Vec<i64> = hits.iter().map(|hit| hit.arena_id.raw()).collect();
        assert_eq!(arenas, vec![100], "only the tab's own directory may match");

        // The same seed with no scope returns both, so the assertion above is
        // about the scope rather than about the index holding one row.
        let unscoped = crate::db::index::query_model::Query::default();
        let all = rt.block_on(query::search_by_query(&pool, &unscoped, 500)).expect("the search runs");
        assert_eq!(all.len(), 2, "both directories are indexed");
    }

    /// A directory whose ingest has not reached `ensure_source` has nothing to
    /// scope by. The entry stays visible but disabled, and nothing is
    /// dispatched: quietly searching the whole library instead would look like
    /// a working search returning wrong results.
    #[test]
    fn a_workspace_without_a_source_yet_cannot_be_searched() {
        let mut tab_state = TabState::default();
        let pending = tab_state.open_directory_workspace(PathBuf::from("D:/archive/pending"));
        let ready = tab_state.open_directory_workspace(PathBuf::from("D:/archive/ready"));
        tab_state.workspace_mut(ready).expect("ready is open").source = Some(SourceId(5));

        let mut viewer = ToolkitTabViewer { tab_state: &mut tab_state };
        let unresolved = viewer.resolve_tab_search_scope(pending);
        let resolved = viewer.resolve_tab_search_scope(ready);
        assert_eq!(unresolved, TabSearchScope::Unresolved);

        let mut states = Vec::new();
        egui::__run_test_ui(|ui| {
            states = vec![
                replay_search_menu_entry(ui, unresolved).enabled(),
                replay_search_menu_entry(ui, resolved).enabled(),
            ];
        });
        assert_eq!(states, vec![false, true], "the entry must be shown disabled, not hidden, and not always disabled");
    }

    /// The live workspace never carries a source of its own, so its tab reads
    /// the live source the indexer created. A directory workspace that has not
    /// resolved its own source must NOT borrow that fallback.
    #[test]
    fn the_live_tab_scopes_to_the_live_source_and_a_pending_directory_does_not() {
        let rt = runtime();
        let pool = mem_pool(&rt);
        let live = rt
            .block_on(query::ensure_default_source(&pool, Path::new("C:/wows/replays"), now()))
            .expect("the live source is created");

        let mut tab_state = TabState::default();
        tab_state.db_pool = Some(pool);
        tab_state.tokio_runtime = Some(Arc::new(rt));
        let pending = tab_state.open_directory_workspace(PathBuf::from("D:/archive/pending"));

        let mut viewer = ToolkitTabViewer { tab_state: &mut tab_state };
        assert_eq!(viewer.resolve_tab_search_scope(WorkspaceId::LIVE), TabSearchScope::Source(live));
        assert_eq!(
            viewer.resolve_tab_search_scope(pending),
            TabSearchScope::Unresolved,
            "a directory with no source of its own must not fall back to the live source"
        );
    }

    /// Exercises the trait hook egui_dock calls, not just the helper it draws
    /// with: a replay tab gets the entry, and a tab that owns no replays gets
    /// an empty menu rather than an entry that would search someone else's
    /// directory.
    #[test]
    fn the_context_menu_hook_adds_the_entry_only_for_replay_tabs() {
        let mut tab_state = TabState::default();
        let id = tab_state.open_directory_workspace(PathBuf::from("D:/archive/a"));
        tab_state.workspace_mut(id).expect("the workspace is open").source = Some(SourceId(4));

        let mut drawn = Vec::new();
        for mut tab in [Tab::Replays(id), Tab::Settings] {
            let mut viewer = ToolkitTabViewer { tab_state: &mut tab_state };
            egui::__run_test_ui(|ui| {
                let before = ui.min_rect().height();
                viewer.context_menu(ui, &mut tab, egui_dock::NodePath::MAIN_ROOT);
                drawn.push(ui.min_rect().height() > before);
            });
        }

        assert_eq!(drawn, vec![true, false], "only a replay tab may add the entry");
    }

    /// A tab outlives its workspace by a frame on the close path, so the menu
    /// has to answer for an id that no longer resolves.
    #[test]
    fn a_closed_workspace_has_no_search_scope() {
        assert_eq!(replay_tab_search_scope(WorkspaceId(42), None, Some(SourceId(9))), TabSearchScope::Unresolved);
    }

    /// The entry's label is the translated string, not the key: rust-i18n
    /// returns the key itself when the catalog has no entry, so comparing it
    /// against a literal is the only check that proves the text was written.
    #[test]
    fn the_menu_entry_is_labelled_from_the_catalog() {
        assert_eq!(t!("ui.tabs.search_these_replays"), "Search these replays");
        assert_eq!(t!("ui.tabs.search_these_replays_unavailable"), "These replays have not been indexed yet");
    }

    /// A workspace with no source that is also not open behaves the same as a
    /// present-but-unresolved one, and a resolved source is never confused with
    /// the live one.
    #[test]
    fn only_the_live_id_reads_the_live_source() {
        let workspace = ReplayWorkspace::new(Some(PathBuf::from("D:/archive")));
        assert_eq!(
            replay_tab_search_scope(WorkspaceId::LIVE, Some(&workspace), Some(SourceId(3))),
            TabSearchScope::Source(SourceId(3))
        );
        assert_eq!(replay_tab_search_scope(WorkspaceId::LIVE, Some(&workspace), None), TabSearchScope::Unresolved);
        assert_eq!(
            replay_tab_search_scope(WorkspaceId(1), Some(&workspace), Some(SourceId(3))),
            TabSearchScope::Unresolved
        );
    }
}

#[cfg(test)]
mod download_prompt_tests {
    use std::collections::BTreeSet;

    use wows_data_mgr::download_repo::RemoteAvailability;
    use wows_data_mgr::download_repo::ResolvedBuild;

    use super::*;

    #[test]
    fn an_exact_candidate_starts_selected() {
        let c = DownloadCandidate::new(9_876, "13.5.0".into(), Some(2), RemoteAvailability::Exact);
        assert!(c.selected);
    }

    #[test]
    fn a_nearest_match_candidate_starts_unselected() {
        let availability = RemoteAvailability::Nearest { version: "13.4.0".into(), build: 9_800 };
        let c = DownloadCandidate::new(9_876, "13.5.0".into(), Some(2), availability);
        assert!(!c.selected, "a nearest match may not fix the replay, so it must not be pre-selected");
    }

    #[test]
    fn an_unreachable_candidate_starts_unselected_and_is_not_selectable() {
        let c = DownloadCandidate::new(9_876, "13.5.0".into(), Some(2), RemoteAvailability::Unreachable);
        assert!(!c.selected);
        assert!(!c.is_selectable());
    }

    /// A fetch failure and a genuine absence must not render alike: telling a
    /// user their data was never published, when the network merely failed, is
    /// a false statement about their data.
    #[test]
    fn unreachable_and_unpublished_do_not_share_a_label() {
        let unreachable = availability_label(&RemoteAvailability::Unreachable);
        let unpublished = availability_label(&RemoteAvailability::Unpublished);
        assert_ne!(unreachable, unpublished);
        assert!(!unreachable.is_empty());
        assert!(!unpublished.is_empty());
    }

    #[test]
    fn an_unpublished_candidate_starts_unselected_and_is_not_selectable() {
        let c = DownloadCandidate::new(9_876, "13.5.0".into(), Some(2), RemoteAvailability::Unpublished);
        assert!(!c.selected);
        assert!(!c.is_selectable());
    }

    /// `t!` returns the key itself when a catalog entry is missing, so the
    /// mandated label test above would still pass with no strings written at
    /// all. This one fails in that case.
    #[test]
    fn availability_labels_are_translated_text_not_raw_keys() {
        for availability in [
            RemoteAvailability::Exact,
            RemoteAvailability::Unpublished,
            RemoteAvailability::Unreachable,
            RemoteAvailability::Nearest { version: "13.4.0".into(), build: 9_800 },
        ] {
            let label = availability_label(&availability);
            assert!(
                !label.contains("ui.dialogs."),
                "no catalog entry for {}: got {label}",
                availability_key(&availability)
            );
        }
    }

    /// Every string this dialog puts on screen, not just the availability
    /// labels. A missing catalog entry renders as the key itself, which is
    /// non-empty, so an "is not empty" assertion cannot catch it.
    #[test]
    fn every_dialog_string_has_a_catalog_entry() {
        for key in [
            "ui.dialogs.download_game_data_intro",
            "ui.dialogs.download_build_row",
            "ui.dialogs.download_replays_needing",
            "ui.dialogs.download_availability_resolving",
            "ui.dialogs.download_plan_pending",
            "ui.dialogs.download_objects_to_fetch",
            "ui.dialogs.download_plan_failed",
            "ui.dialogs.download_plan_no_cache_dir",
            "ui.windows.download_game_data",
            "ui.buttons.download",
            "ui.buttons.dismiss",
            "ui.buttons.retry",
        ] {
            let rendered = t!(key).into_owned();
            assert_ne!(rendered, key, "no catalog entry for {key}");
            assert!(!rendered.trim().is_empty(), "empty catalog entry for {key}");
        }
    }

    /// Retry re-asks the planner, which cannot help when the cache directory it
    /// would write into is not configured. Reporting that as the generic
    /// planning failure leaves the user pressing Retry with nothing changing
    /// and no hint of the cause.
    #[test]
    fn a_missing_cache_directory_is_not_reported_as_a_planning_failure() {
        let generic = t!("ui.dialogs.download_plan_failed").into_owned();
        let no_cache_dir = t!("ui.dialogs.download_plan_no_cache_dir").into_owned();

        assert_ne!(generic, no_cache_dir, "the cause the user can act on must not read as the generic failure");
    }

    /// A nearest match is the one label carrying data, and the whole point of
    /// it is naming the build that would actually be fetched instead.
    #[test]
    fn the_nearest_match_label_names_the_build_it_would_fetch() {
        let label = availability_label(&RemoteAvailability::Nearest { version: "13.4.0".into(), build: 9_800 });
        assert!(label.contains("13.4.0"), "label must name the version: {label}");
        assert!(label.contains("9800"), "label must name the build: {label}");
    }

    fn resolved(build: u32, version: &str, availability: RemoteAvailability) -> ResolvedBuild {
        ResolvedBuild { requested_build: build, requested_version: Some(version.to_string()), availability }
    }

    /// The first plan is what turns unresolved rows into rows the user can act
    /// on, and it is where the pre-selection rule is actually applied.
    #[test]
    fn the_first_plan_resolves_every_row_and_ticks_only_the_exact_ones() {
        let mut prompt = GameDataDownloadPrompt::new(
            vec![
                DownloadCandidate::unresolved(9_876, "13.5.0".into(), Some(2)),
                DownloadCandidate::unresolved(9_900, "13.6.0".into(), Some(1)),
            ],
            None,
        );

        assert!(prompt.needs_plan(), "a fresh offer has nothing planned yet");
        let (ticket, _) = prompt.begin_planning();
        prompt.apply_plan(
            ticket,
            DownloadPlan {
                unique_missing_objects: 12,
                resolved: vec![
                    resolved(9_876, "13.5.0", RemoteAvailability::Exact),
                    resolved(9_900, "13.6.0", RemoteAvailability::Nearest { version: "13.5.0".into(), build: 9_876 }),
                ],
            },
        );

        assert!(prompt.candidates[0].selected);
        assert!(!prompt.candidates[1].selected, "a nearest match must not stay ticked from the unresolved default");
        assert_eq!(prompt.selected_builds(), BTreeSet::from([9_876]));
        assert_eq!(prompt.downloadable(), vec![(9_876, "13.5.0".to_string())]);
    }

    /// Unticking a row narrows what the next plan is asked about, and the
    /// object count the dialog shows is only honest if the request matches what
    /// is on screen. This is what the request carries, not what a later plan
    /// does to the rows: an unticked row is never in a plan at all, so nothing
    /// a plan does could re-tick it.
    #[test]
    fn a_plan_request_covers_only_the_rows_still_ticked() {
        let mut prompt = GameDataDownloadPrompt::new(
            vec![
                DownloadCandidate::unresolved(9_876, "13.5.0".into(), Some(2)),
                DownloadCandidate::unresolved(9_900, "13.6.0".into(), Some(1)),
            ],
            None,
        );
        let (ticket, request) = prompt.begin_planning();
        assert_eq!(
            request,
            vec![(9_876, Some("13.5.0".to_string())), (9_900, Some("13.6.0".to_string()))],
            "the first plan has to cover every build the offer names"
        );
        prompt.apply_plan(
            ticket,
            DownloadPlan {
                unique_missing_objects: 12,
                resolved: vec![
                    resolved(9_876, "13.5.0", RemoteAvailability::Exact),
                    resolved(9_900, "13.6.0", RemoteAvailability::Exact),
                ],
            },
        );
        prompt.candidates[0].selected = false;

        let (ticket, request) = prompt.begin_planning();
        assert_eq!(request, vec![(9_900, Some("13.6.0".to_string()))], "the unticked build must not be asked about");
        assert_eq!(prompt.planned_selection, Some(BTreeSet::from([9_900])));

        prompt.apply_plan(
            ticket,
            DownloadPlan {
                unique_missing_objects: 5,
                resolved: vec![resolved(9_900, "13.6.0", RemoteAvailability::Exact)],
            },
        );

        assert!(!prompt.candidates[0].selected);
        assert!(prompt.candidates[1].selected);
        assert_eq!(prompt.downloadable(), vec![(9_900, "13.6.0".to_string())]);
    }

    /// The reachable half of the same rule. A nearest match starts unticked but
    /// is selectable, so the user can tick it; that puts it in the next plan's
    /// request, and the answer that comes back is still `Nearest`. Re-deriving
    /// the row from that answer would silently untick it under the user's
    /// hands, and the object count would then describe a different selection
    /// from the one on screen.
    #[test]
    fn a_later_plan_does_not_undo_a_tick_the_user_added() {
        let mut prompt = GameDataDownloadPrompt::new(
            vec![
                DownloadCandidate::unresolved(9_876, "13.5.0".into(), Some(2)),
                DownloadCandidate::unresolved(9_900, "13.6.0".into(), Some(1)),
            ],
            None,
        );
        let nearest = RemoteAvailability::Nearest { version: "13.5.0".into(), build: 9_876 };
        let (ticket, _) = prompt.begin_planning();
        prompt.apply_plan(
            ticket,
            DownloadPlan {
                unique_missing_objects: 12,
                resolved: vec![
                    resolved(9_876, "13.5.0", RemoteAvailability::Exact),
                    resolved(9_900, "13.6.0", nearest.clone()),
                ],
            },
        );
        assert!(!prompt.candidates[1].selected, "a nearest match starts unticked");

        // Unticking itself narrows the selection, so the corrected count for
        // the ticked rows alone is fetched first, exactly as production does.
        assert!(prompt.needs_plan());
        let (ticket, _) = prompt.begin_planning();
        prompt.apply_plan(
            ticket,
            DownloadPlan {
                unique_missing_objects: 12,
                resolved: vec![resolved(9_876, "13.5.0", RemoteAvailability::Exact)],
            },
        );

        prompt.candidates[1].selected = true;
        assert!(prompt.needs_plan(), "ticking a row must ask what it costs");
        let (ticket, _) = prompt.begin_planning();
        prompt.apply_plan(
            ticket,
            DownloadPlan {
                unique_missing_objects: 20,
                resolved: vec![
                    resolved(9_876, "13.5.0", RemoteAvailability::Exact),
                    resolved(9_900, "13.6.0", nearest),
                ],
            },
        );

        assert!(prompt.candidates[1].selected, "the user ticked this row; a later answer must not untick it");
        assert_eq!(prompt.selected_builds(), BTreeSet::from([9_876, 9_900]));
    }

    /// A row keeps whatever answer it has, so the only thing that can get a
    /// build out of an unusable answer is [`GameDataDownloadPrompt::retry`]
    /// blanking it first. A `retry` that left the answer in place would leave a
    /// build whose metadata fetch merely blipped unselectable for the life of
    /// the dialog, and that is what this discriminates.
    #[test]
    fn a_retry_lets_a_later_plan_re_resolve_a_row_the_user_could_not_act_on() {
        let mut prompt =
            GameDataDownloadPrompt::new(vec![DownloadCandidate::unresolved(9_876, "13.5.0".into(), Some(2))], None);
        let (ticket, _) = prompt.begin_planning();
        prompt.apply_plan(
            ticket,
            DownloadPlan {
                unique_missing_objects: 0,
                resolved: vec![resolved(9_876, "13.5.0", RemoteAvailability::Unreachable)],
            },
        );
        assert!(!prompt.candidates[0].is_selectable());

        prompt.retry();
        let (ticket, _) = prompt.begin_planning();
        prompt.apply_plan(
            ticket,
            DownloadPlan {
                unique_missing_objects: 9,
                resolved: vec![resolved(9_876, "13.5.0", RemoteAvailability::Exact)],
            },
        );

        assert!(prompt.candidates[0].is_selectable(), "a blip must not make a build unselectable forever");
        assert!(prompt.candidates[0].selected);
        assert_eq!(prompt.downloadable(), vec![(9_876, "13.5.0".to_string())]);
    }

    /// The dead end this closes: on a fresh offer nothing is resolved, so every
    /// checkbox is disabled, Download needs a Ready plan, and no selection
    /// change can ask again. Without a retry the only live control is Dismiss,
    /// and a transient network blip hides the user's replays for the session.
    #[test]
    fn a_failed_first_plan_can_be_retried() {
        let mut prompt =
            GameDataDownloadPrompt::new(vec![DownloadCandidate::unresolved(9_876, "13.5.0".into(), Some(2))], None);
        let (ticket, _) = prompt.begin_planning();
        prompt.plan_task_finished(ticket);

        assert!(matches!(prompt.plan, DownloadPlanState::Failed(_)));
        assert!(!prompt.needs_plan(), "nothing about the selection changed, so nothing re-asks on its own");
        assert!(prompt.can_retry(), "a failed plan must offer a way out");

        prompt.retry();

        assert!(prompt.needs_plan(), "a retry must actually ask the planner again");
        let (ticket, _) = prompt.begin_planning();
        prompt.apply_plan(
            ticket,
            DownloadPlan {
                unique_missing_objects: 9,
                resolved: vec![resolved(9_876, "13.5.0", RemoteAvailability::Exact)],
            },
        );
        assert!(prompt.candidates[0].is_downloadable());
    }

    /// A resolved offer with nothing wrong has nothing to retry, so the button
    /// is not there to be pressed.
    #[test]
    fn a_healthy_offer_offers_no_retry() {
        let mut prompt =
            GameDataDownloadPrompt::new(vec![DownloadCandidate::unresolved(9_876, "13.5.0".into(), Some(2))], None);
        let (ticket, _) = prompt.begin_planning();
        prompt.apply_plan(
            ticket,
            DownloadPlan {
                unique_missing_objects: 9,
                resolved: vec![resolved(9_876, "13.5.0", RemoteAvailability::Exact)],
            },
        );

        assert!(!prompt.can_retry());
    }

    /// An unreachable row is a fetch that failed, not an absence, so it is
    /// worth asking again even though the plan as a whole succeeded.
    #[test]
    fn an_unreachable_row_offers_a_retry_even_when_the_plan_succeeded() {
        let mut prompt = GameDataDownloadPrompt::new(
            vec![
                DownloadCandidate::unresolved(9_876, "13.5.0".into(), Some(2)),
                DownloadCandidate::unresolved(9_900, "13.6.0".into(), Some(1)),
            ],
            None,
        );
        let (ticket, _) = prompt.begin_planning();
        prompt.apply_plan(
            ticket,
            DownloadPlan {
                unique_missing_objects: 9,
                resolved: vec![
                    resolved(9_876, "13.5.0", RemoteAvailability::Exact),
                    resolved(9_900, "13.6.0", RemoteAvailability::Unreachable),
                ],
            },
        );

        assert!(matches!(prompt.plan, DownloadPlanState::Ready(_)));
        assert!(prompt.can_retry());
    }

    /// A retry must not throw away answers the user has already acted on. The
    /// exact row keeps its resolution and its tick; only the row that could not
    /// be answered goes back to being asked about.
    #[test]
    fn a_retry_keeps_the_rows_the_user_can_already_act_on() {
        let mut prompt = GameDataDownloadPrompt::new(
            vec![
                DownloadCandidate::unresolved(9_876, "13.5.0".into(), Some(2)),
                DownloadCandidate::unresolved(9_900, "13.6.0".into(), Some(1)),
            ],
            None,
        );
        let (ticket, _) = prompt.begin_planning();
        prompt.apply_plan(
            ticket,
            DownloadPlan {
                unique_missing_objects: 9,
                resolved: vec![
                    resolved(9_876, "13.5.0", RemoteAvailability::Exact),
                    resolved(9_900, "13.6.0", RemoteAvailability::Unreachable),
                ],
            },
        );

        prompt.retry();

        assert_eq!(prompt.candidates[0].availability, Some(RemoteAvailability::Exact));
        assert!(prompt.candidates[0].selected);
        assert!(prompt.candidates[1].availability.is_none(), "the unanswerable row must be asked about again");
        assert!(prompt.candidates[1].selected, "and must be in the selection the retry asks about");
    }

    /// Untick a row and the object count on screen no longer describes what is
    /// ticked, so a fresh plan has to be asked for.
    #[test]
    fn changing_the_selection_asks_for_a_new_plan() {
        let mut prompt = GameDataDownloadPrompt::new(
            vec![
                DownloadCandidate::unresolved(9_876, "13.5.0".into(), Some(2)),
                DownloadCandidate::unresolved(9_900, "13.6.0".into(), Some(1)),
            ],
            None,
        );
        let (ticket, _) = prompt.begin_planning();
        prompt.apply_plan(
            ticket,
            DownloadPlan {
                unique_missing_objects: 12,
                resolved: vec![
                    resolved(9_876, "13.5.0", RemoteAvailability::Exact),
                    resolved(9_900, "13.6.0", RemoteAvailability::Exact),
                ],
            },
        );
        assert!(!prompt.needs_plan(), "the plan on hand describes exactly what is ticked");

        prompt.candidates[1].selected = false;

        assert!(prompt.needs_plan());
    }

    /// A plan that fails must not be re-requested on the next frame: the
    /// dialog would dispatch a network task per frame for as long as it is
    /// open. Getting out of it is the Retry button's job, not a spin loop's.
    #[test]
    fn a_failed_plan_is_not_retried_until_the_user_asks() {
        let mut prompt =
            GameDataDownloadPrompt::new(vec![DownloadCandidate::unresolved(9_876, "13.5.0".into(), None)], None);
        let (ticket, _) = prompt.begin_planning();
        prompt.plan_task_finished(ticket);

        assert!(matches!(prompt.plan, DownloadPlanState::Failed(_)));
        assert!(!prompt.needs_plan(), "a failed plan still describes the selection it was asked for");
    }

    /// A planner started for an offer that has since been answered must not
    /// report failure against the offer now open, which would flip a healthy
    /// dialog to Failed for no reason.
    ///
    /// Both offers ask about the same build, which is what dismissing a
    /// directory's offer and immediately reopening that directory produces.
    /// Matching on the builds asked about cannot tell the two runs apart.
    #[test]
    fn a_stale_planner_reaping_does_not_fail_the_current_offer() {
        let mut answered =
            GameDataDownloadPrompt::new(vec![DownloadCandidate::unresolved(9_876, "13.5.0".into(), None)], None);
        let (stale, _) = answered.begin_planning();

        let mut current =
            GameDataDownloadPrompt::new(vec![DownloadCandidate::unresolved(9_876, "13.5.0".into(), None)], None);
        let (ticket, _) = current.begin_planning();
        assert_ne!(stale, ticket, "two runs over the same builds must still be distinguishable");

        current.plan_task_finished(stale);

        assert!(
            matches!(current.plan, DownloadPlanState::Planning),
            "another offer's planner ending says nothing about this one"
        );

        current.plan_task_finished(ticket);
        assert!(matches!(current.plan, DownloadPlanState::Failed(_)), "its own planner ending does report");
    }

    /// The same collision on the answering side: the stale planner delivering a
    /// plan for the identical build set must not put its object count on screen
    /// for the offer now open.
    #[test]
    fn a_stale_plan_over_the_same_builds_does_not_answer_the_current_offer() {
        let mut answered =
            GameDataDownloadPrompt::new(vec![DownloadCandidate::unresolved(9_876, "13.5.0".into(), None)], None);
        let (stale, _) = answered.begin_planning();

        let mut current =
            GameDataDownloadPrompt::new(vec![DownloadCandidate::unresolved(9_876, "13.5.0".into(), None)], None);
        current.begin_planning();

        current.apply_plan(
            stale,
            DownloadPlan {
                unique_missing_objects: 4_000,
                resolved: vec![resolved(9_876, "13.5.0", RemoteAvailability::Exact)],
            },
        );

        assert!(matches!(current.plan, DownloadPlanState::Planning), "a plan from another run must not land");
        assert!(current.candidates[0].availability.is_none());
    }

    /// An unresolved row is ticked so the first plan covers it, but nothing is
    /// known about it yet, so it must not be downloadable.
    #[test]
    fn an_unresolved_row_is_ticked_for_planning_but_never_downloadable() {
        let candidate = DownloadCandidate::unresolved(9_876, "13.5.0".into(), Some(2));
        assert!(candidate.selected, "the first plan has to cover every build the directory asked for");
        assert!(!candidate.is_selectable());
        assert!(!candidate.is_downloadable());
    }

    /// The planning task can end without ever sending a plan (a panicked or
    /// dropped worker disconnects the channel). The dialog must not be left
    /// waiting on it.
    #[test]
    fn a_planning_task_that_ends_without_a_plan_leaves_the_dialog_usable() {
        let mut prompt =
            GameDataDownloadPrompt::new(vec![DownloadCandidate::unresolved(9_876, "13.5.0".into(), None)], None);
        let (ticket, _) = prompt.begin_planning();

        prompt.plan_task_finished(ticket);

        match &prompt.plan {
            DownloadPlanState::Failed(message) => assert!(!message.is_empty()),
            _ => panic!("a planner that ended without a plan must not leave the dialog Planning"),
        }
    }

    /// In the drain loop the `match &task.kind` dispatch runs before
    /// `handle_task_completion`, so a successful plan really does arrive as
    /// Planning -> plan_task_finished -> Failed -> apply_plan -> Ready. Run in
    /// the other order this passes while a defensive `if !Planning { return }`
    /// in `apply_plan` would discard every successful plan in production.
    #[test]
    fn a_plan_delivered_after_its_task_was_reaped_still_lands() {
        let mut prompt =
            GameDataDownloadPrompt::new(vec![DownloadCandidate::unresolved(9_876, "13.5.0".into(), None)], None);
        let (ticket, _) = prompt.begin_planning();

        prompt.plan_task_finished(ticket);
        prompt.apply_plan(
            ticket,
            DownloadPlan {
                unique_missing_objects: 7,
                resolved: vec![resolved(9_876, "13.5.0", RemoteAvailability::Exact)],
            },
        );

        match &prompt.plan {
            DownloadPlanState::Ready(plan) => assert_eq!(plan.unique_missing_objects, 7),
            other => panic!(
                "a delivered plan must win over its own task being reaped, got {}",
                match other {
                    DownloadPlanState::Failed(message) => message.as_str(),
                    _ => "a non-Ready state",
                }
            ),
        }
        assert!(prompt.candidates[0].is_downloadable());
    }

    /// The walk a download itself triggers is not a question the user asked.
    /// Its leftovers are the part of the offer they chose not to fetch, so
    /// re-raising the identical offer is a loop.
    #[test]
    fn the_walk_a_download_triggers_does_not_repeat_the_offer_it_came_from() {
        let offered = BTreeSet::from([9_876_u32, 9_877]);
        assert!(offer_was_just_made(Some(&offered), &BTreeSet::from([9_876, 9_877])));
    }

    /// The downloads that ran remove builds from `missing`, so what comes back
    /// is a subset of the offer, not the same set. Comparing for equality would
    /// re-raise the offer for exactly the common case: the user fetched some of
    /// what they were shown.
    #[test]
    fn a_partly_satisfied_offer_is_still_the_same_offer() {
        let offered = BTreeSet::from([9_876_u32, 9_877]);
        assert!(offer_was_just_made(Some(&offered), &BTreeSet::from([9_877])));
    }

    /// A build that was not in the offer is new data the user has never been
    /// told about, and must be raised even though it arrives on the automatic
    /// walk alongside builds that were.
    #[test]
    fn a_build_the_offer_did_not_cover_is_still_raised() {
        let offered = BTreeSet::from([9_876_u32]);
        assert!(!offer_was_just_made(Some(&offered), &BTreeSet::from([9_876, 9_999])));
    }

    /// The user going back and opening those replays again is an explicit
    /// request, and gets an answer however many times they have dismissed the
    /// offer before. Suppressing it is what made the feature look like it had
    /// swallowed their replays.
    #[test]
    fn an_explicit_open_is_never_suppressed() {
        assert!(!offer_was_just_made(None, &BTreeSet::from([9_876_u32])));
        assert!(!offer_was_just_made(None, &BTreeSet::from([9_876_u32, 9_877])));
    }

    /// Only the walk a download started carries a suppression record, and only
    /// while it is the walk in flight. Every other completed walk is one the
    /// user asked for.
    #[test]
    fn only_the_walk_a_download_started_is_marked_as_automatic() {
        let mut app = WowsToolkitApp::default();
        let workspace = WorkspaceId(3);
        let offered = BTreeSet::from([9_876_u32]);

        assert_eq!(app.finish_directory_reingest(workspace), None, "a walk nobody queued is an explicit open");

        app.directory_reingest.insert(workspace, DirectoryReingest::AwaitingDownload { offered: offered.clone() });
        assert_eq!(
            app.finish_directory_reingest(workspace),
            None,
            "a walk finishing while the download is still running is not the queued one"
        );

        app.note_reingest_download_finished(workspace);
        assert!(
            matches!(app.directory_reingest.get(&workspace), Some(DirectoryReingest::Owed { .. })),
            "the walk is owed once the single download task has been tried"
        );

        // The owed walk has not started, so a walk finishing now is some other
        // walk -- a reopen the user asked for while the downloads ran -- and
        // must get its offer. It must also leave the owed record alone, or the
        // walk the downloads paid for is dropped.
        assert_eq!(
            app.finish_directory_reingest(workspace),
            None,
            "a walk that is owed but not started is not the one that just finished"
        );
        assert!(
            matches!(app.directory_reingest.get(&workspace), Some(DirectoryReingest::Owed { .. })),
            "an unrelated walk finishing must not consume the owed record"
        );

        // `service_directory_reingests` makes this transition once the
        // workspace is free; it needs a live workspace with a root, so the
        // state it produces is set directly here.
        app.directory_reingest.insert(workspace, DirectoryReingest::Walking { offered: offered.clone() });
        assert_eq!(app.finish_directory_reingest(workspace), Some(offered));
        assert!(app.directory_reingest.is_empty(), "the record must not outlive the walk it describes");
    }

    /// The record is released by the drain loop's kind dispatch, which runs for
    /// every finished walk whatever its result, and read back by the completion
    /// arm through this. A release for another workspace is not an answer for
    /// this one.
    #[test]
    fn a_released_offer_is_read_back_only_by_the_workspace_it_belongs_to() {
        let mut app = WowsToolkitApp::default();
        let offered = BTreeSet::from([9_876_u32]);

        app.finished_reingest_offer = Some((WorkspaceId(3), offered.clone()));
        assert_eq!(app.take_finished_reingest_offer(WorkspaceId(4)), None);
        assert!(app.finished_reingest_offer.is_none(), "a release read by the wrong workspace is dropped, not kept");

        app.finished_reingest_offer = Some((WorkspaceId(3), offered.clone()));
        assert_eq!(app.take_finished_reingest_offer(WorkspaceId(3)), Some(offered));
        assert!(app.finished_reingest_offer.is_none(), "a release must not answer for a second walk");
    }

    /// A scan of a directory nothing was found in: enough to stand in for one
    /// retained across the download offer, which is the only property these
    /// tests read off it.
    fn empty_scan(root: PathBuf) -> crate::task::scan::DirectoryScan {
        crate::task::scan::DirectoryScan {
            root,
            by_build: BTreeMap::new(),
            unreadable: Vec::new(),
            missing_builds: BTreeSet::new(),
            total: 0,
        }
    }

    /// The download task sends its first progress only after planning and a
    /// metadata fetch per build, which is seconds to tens of seconds on a slow
    /// link. A listing reporting no stage across that window draws as a
    /// finished, empty directory, which is the failure the staging removes.
    #[test]
    fn a_directory_download_is_reported_before_its_first_progress() {
        let mut app = WowsToolkitApp::default();
        let workspace = app.tab_state.open_directory_workspace(PathBuf::from("replays"));

        app.mark_directory_downloading(workspace, task::DownloadProgress { downloaded: 0, total: 0 });

        let listed = app.tab_state.workspace(workspace).expect("the workspace was just opened");
        assert!(listed.ingest_in_flight, "the listing must not draw as finished while the download runs");
        let stage = listed.ingest_stage.clone().expect("the progress line returns early without a stage");
        assert!(matches!(stage, crate::task::replays::IngestStage::Downloading(_)));
        assert_eq!(stage.fraction(), Some(0.0), "a download with nothing planned yet is not a full bar");
    }

    /// The workspace can close between the download starting and its progress
    /// arriving. Neither its stage nor a workspace to hold it may be created.
    #[test]
    fn a_download_stage_for_a_closed_workspace_lands_nowhere() {
        let mut app = WowsToolkitApp::default();
        let closed = WorkspaceId(7);

        app.mark_directory_downloading(closed, task::DownloadProgress { downloaded: 0, total: 0 });

        assert!(app.tab_state.workspace(closed).is_none(), "a departed workspace must not be reopened by its download");
    }

    /// The offer is not modal, and re-picking the same directory while it is
    /// open resolves to the same workspace. `ingest_in_flight` is clear across
    /// that window, so the retained scan is what has to refuse the second scan:
    /// otherwise it overwrites the first and its read starts unanswered.
    #[test]
    fn a_scan_held_across_the_offer_refuses_a_second_one() {
        let mut app = WowsToolkitApp::default();
        let root = PathBuf::from("replays");
        let workspace = app.tab_state.open_directory_workspace(root.clone());

        assert!(!app.scan_already_taken(workspace), "a directory nothing has scanned is scannable");

        app.pending_scans.insert(workspace, Box::new(empty_scan(root)));
        assert!(app.scan_already_taken(workspace), "a scan waiting on the offer is still this workspace's scan");

        // The re-scan `service_directory_reingests` falls back to is reached
        // only when no scan is retained, which is this state.
        app.pending_scans.remove(&workspace);
        assert!(!app.scan_already_taken(workspace), "a download that outlived its scan must still be able to re-scan");
    }

    /// Dismissing the offer is the only thing that starts this read, and it
    /// leaves no reingest record to retry from. A scan dropped by a read that
    /// then refused is a listing that never fills and never says why.
    #[test]
    fn a_read_that_cannot_start_keeps_the_scan_it_would_have_consumed() {
        let mut app = WowsToolkitApp::default();
        let root = PathBuf::from("replays");
        let workspace = app.tab_state.open_directory_workspace(root.clone());
        app.pending_scans.insert(workspace, Box::new(empty_scan(root)));

        // No game data is loaded, so the prerequisite check is what refuses.
        assert!(!app.start_directory_read(workspace), "a read without game data cannot start");

        assert!(app.pending_scans.contains_key(&workspace), "the scan must outlive a read that never started");
    }
}

#[cfg(all(test, feature = "logging"))]
mod logging_target_tests {
    use super::log_targets;

    /// The download crate's diagnostics are the only record of a corrupt or
    /// missing content object, and an allowlist that omits its target discards
    /// them before they reach the file.
    #[test]
    fn game_data_download_diagnostics_reach_the_log() {
        let targets = log_targets();

        assert!(
            targets.would_enable(wows_data_mgr::LOG_TARGET, &tracing::Level::ERROR),
            "wows-data-mgr errors are filtered out of the log file"
        );
        assert!(
            targets.would_enable(wows_data_mgr::LOG_TARGET, &tracing::Level::INFO),
            "wows-data-mgr progress is filtered out of the log file"
        );
        // Every event actually emitted carries a module target, never the bare
        // crate name, so the assertions above pass on prefix matching alone and
        // would keep passing if that stopped working.
        assert!(
            targets.would_enable("wows_data_mgr::download_repo", &tracing::Level::ERROR),
            "the corrupt-object evidence is filtered out of the log file"
        );
        assert!(
            targets.would_enable("wows_data_mgr::dump", &tracing::Level::WARN),
            "unreadable content objects are filtered out of the log file"
        );
        assert!(
            !targets.would_enable("hyper_util", &tracing::Level::ERROR),
            "the filter must stay an allowlist, not turn into a catch-all"
        );
    }
}
