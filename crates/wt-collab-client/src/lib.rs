//! Collaborative replay session support via iroh peer-to-peer networking.
//!
//! # Architecture
//!
//! Sessions use a mesh topology: every participant maintains a direct QUIC
//! connection to every other participant. The host is the initial rendezvous
//! point and assigns identities; all other messaging flows peer-to-peer.
//!
//! Permission enforcement is client-side: each receiver decides what to drop
//! based on the sender's role and the current permission state.
//!
//! The iroh networking runs in a background tokio task. It talks to whatever
//! is drawing through `std::sync::mpsc` channels and an
//! `Arc<Mutex<SessionState>>`, and wakes it through a [`SessionWaker`] the
//! front end supplies: the task cannot know whether a repaint means an egui
//! viewport or a GPUI entity.

pub mod peer;

/// The wire protocol and its validation, re-exported so a caller needs one
/// dependency rather than two.
pub use wt_collab_protocol::protocol;
pub use wt_collab_protocol::types;
pub use wt_collab_protocol::validation;

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use parking_lot::Mutex;
use wows_minimap_renderer::DrawCommand;
use wows_replays::types::GameClock;

pub use wt_collab_protocol::types::Permissions;
pub use wt_collab_protocol::types::UserCursor;

/// Wakes whatever is drawing the session when the peer task changes it.
///
/// The task runs off the drawing thread, so a change it makes reaches the
/// screen only when that thread comes round again. Each front end wakes
/// itself its own way -- egui repaints a viewport, GPUI notifies an entity --
/// so the session is handed one of these rather than knowing either.
pub trait SessionWaker: Send + Sync {
    /// Wakes whatever shows the session as a whole: the roster, the token,
    /// the status line.
    ///
    /// A single window is woken through its own [`ViewportSink`] instead: a
    /// front end names its windows its own way, so the session holds the wake
    /// it was handed rather than deriving one from a window id.
    fn wake(&self);
}

/// Wakes the one window a sink belongs to.
pub type WindowWake = Arc<dyn Fn() + Send + Sync>;

/// Sends events from the peer task and wakes whatever reads them.
///
/// The waker is bound when the session starts, so an event raised before a
/// front end registered one still arrives; it is simply read on the next pass
/// rather than waking anything.
#[derive(Clone)]
pub struct EventSender {
    tx: std::sync::mpsc::Sender<SessionEvent>,
    waker: Option<Arc<dyn SessionWaker>>,
}

impl EventSender {
    /// Raises an event. A closed inbox is not an error: it means whatever was
    /// reading has gone, and the peer task keeps running until it is told to
    /// stop.
    pub fn send(&self, event: SessionEvent) {
        if self.tx.send(event).is_ok()
            && let Some(waker) = &self.waker
        {
            waker.wake();
        }
    }
}

/// Receives what [`EventSender`] raises.
pub struct EventInbox {
    rx: std::sync::mpsc::Receiver<SessionEvent>,
    tx: std::sync::mpsc::Sender<SessionEvent>,
    waker: Option<Arc<dyn SessionWaker>>,
}

impl EventInbox {
    /// Opens an inbox that wakes `waker` on every event.
    pub fn new(waker: Option<Arc<dyn SessionWaker>>) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        Self { rx, tx, waker }
    }

    pub fn sender(&self) -> EventSender {
        EventSender { tx: self.tx.clone(), waker: self.waker.clone() }
    }

    /// Everything raised since the last read. Draining rather than blocking:
    /// the caller is a draw pass, which must not wait on the network.
    pub fn drain(&self) -> Vec<SessionEvent> {
        self.rx.try_iter().collect()
    }
}

/// A single frame's drawing data, handed from the peer task to whatever is
/// showing that window.
#[derive(Debug)]
pub struct PlaybackFrame {
    pub replay_id: u64,
    pub commands: Vec<DrawCommand>,
    pub clock: GameClock,
    pub frame_index: usize,
    pub total_frames: usize,
    pub game_duration: f32,
}

/// Base URL of the deployed web client. The session token is appended as a
/// URL hash fragment: `{WEB_CLIENT_URL}#{token}`.
pub const WEB_CLIENT_URL: &str = "https://landaire.github.io/wows-toolkit/tactics/";

/// Peer's role in the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerRole {
    /// The original session creator. Can promote co-hosts, manage roster.
    Host,
    /// Promoted by the host. Can change settings, open replays as frame source.
    CoHost,
    /// Regular participant. Can send cursors, annotations (if unlocked),
    /// and toggle display options (if unlocked).
    Peer,
}

impl PeerRole {
    pub fn is_host(self) -> bool {
        self == PeerRole::Host
    }

    pub fn is_co_host(self) -> bool {
        self == PeerRole::CoHost
    }

    pub fn is_peer(self) -> bool {
        self == PeerRole::Peer
    }
}

/// Metadata for a replay that is currently open in the host session.
#[derive(Debug, Clone)]
pub struct OpenReplay {
    pub replay_id: u64,
    pub replay_name: String,
    pub map_image_png: Vec<u8>,
    pub game_version: String,
    /// Raw map name (e.g. "spaces/16_OC_bees_to_honey").
    pub map_name: String,
    /// Human-readable translated map name for display.
    pub display_name: String,
}

/// Info about the currently open tactics board map, received from a peer.
#[derive(Debug, Clone, Default)]
pub struct TacticsMapInfo {
    pub map_name: String,
    /// Human-readable map name for display.
    pub display_name: String,
    pub map_id: u32,
    pub map_image_png: Vec<u8>,
    /// Map metadata for coordinate transforms.
    pub map_info: Option<wows_minimap_renderer::map_data::MapInfo>,
}

/// A map ping from a peer, rendered as an expanding ripple effect.
#[derive(Debug, Clone)]
pub struct PeerPing {
    pub user_id: u64,
    pub color: [u8; 3],
    pub pos: [f32; 2],
    pub time: web_time::Instant,
}

/// Authoritative annotation state maintained by the session.
///
/// Uses parallel vecs for annotations, their unique IDs, and owner user_ids.
#[derive(Debug, Clone, Default)]
pub struct AnnotationSyncState {
    pub annotations: Vec<types::Annotation>,
    pub owners: Vec<u64>,
    pub ids: Vec<u64>,
}

/// Authoritative cap point state maintained by the session for tactics board sync.
#[derive(Debug, Clone, Default)]
pub struct CapPointSyncState {
    pub cap_points: Vec<protocol::WireCapPoint>,
}

/// Per-board session state for multi-window tactics boards.
#[derive(Debug, Clone, Default)]
pub struct TacticsBoardSessionState {
    pub owner_user_id: u64,
    pub tactics_map: TacticsMapInfo,
    /// Human-readable window title, set by the local viewer each frame.
    pub window_title: String,
    pub cap_point_sync: CapPointSyncState,
    pub cap_point_sync_version: u64,
    pub annotation_sync: AnnotationSyncState,
    pub annotation_sync_version: u64,
}

/// Shared session state visible to the UI thread.
///
/// A registered window that receives targeted wake-ups and optionally
/// playback frames from the peer task.
pub struct ViewportSink {
    /// Where frames for this window go. `None` for a window that receives
    /// none, such as a tactics board.
    pub frame_tx: Option<std::sync::mpsc::SyncSender<PlaybackFrame>>,
    /// How to wake this window. `None` for one that needs no waking, which
    /// is not the same as one that has gone: a sink is removed when that
    /// happens.
    pub wake: Option<WindowWake>,
}

impl std::fmt::Debug for ViewportSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ViewportSink")
            .field("takes_frames", &self.frame_tx.is_some())
            .field("wakes", &self.wake.is_some())
            .finish()
    }
}

/// Stored behind `Arc<Mutex<>>` and read each frame by the renderer UI.
/// Written by the background collab task.
pub struct SessionState {
    /// Our role in the session.
    pub role: PeerRole,
    /// Our own user_id assigned by the host.
    pub my_user_id: u64,
    /// The host's user_id (immutable after SessionInfo). Used to verify
    /// authority messages like PromoteToCoHost.
    pub host_user_id: u64,
    /// Who is currently the frame source (user_id). Receivers only accept
    /// Frame messages from this user.
    pub frame_source_id: u64,
    /// All connected users (including self).
    pub connected_users: Vec<ConnectedUser>,
    /// Current permission state set by the host/co-host.
    pub permissions: Permissions,
    /// All users' cursor positions (including self).
    pub cursors: Vec<UserCursor>,
    /// Host-only: the session token (base64-encoded EndpointAddr JSON).
    pub token: Option<String>,
    /// Status message for display in the UI.
    pub status: SessionStatus,
    /// Currently open replays in the session (host tracks authoritative list,
    /// clients receive via SessionInfo + ReplayOpened/ReplayClosed).
    pub open_replays: Vec<OpenReplay>,
    /// Monotonically increasing version for render options updates.
    /// Each renderer tracks its own applied version to detect new updates.
    pub render_options_version: u64,
    /// Current render options from the authority peer (None = no update yet).
    pub current_render_options: Option<protocol::CollabRenderOptions>,
    /// Monotonically increasing version for annotation sync updates.
    pub annotation_sync_version: u64,
    /// Current annotation sync from the authority peer.
    pub current_annotation_sync: Option<AnnotationSyncState>,
    /// Monotonically increasing version for per-ship range override updates.
    pub range_override_version: u64,
    /// Current per-ship range overrides from peers.
    pub current_range_overrides:
        Option<Vec<(wows_replays::types::EntityId, wows_minimap_renderer::draw_command::ShipConfigFilter)>>,
    /// Monotonically increasing version for per-ship trail override updates.
    pub trail_override_version: u64,
    /// Current set of player names whose trails are hidden.
    pub current_trail_hidden: Option<Vec<String>>,
    /// Active map pings from peers (rendered as ripple effects).
    pub pings: Vec<PeerPing>,
    /// Per-board tactics session state, keyed by `board_id`.
    pub tactics_boards: HashMap<u64, TacticsBoardSessionState>,
    /// Monotonically increasing version bumped on any tactics board add/remove/update.
    pub tactics_boards_version: u64,
    /// Per-viewport sinks, keyed by window ID (replay_id or board_id).
    /// Each entry holds the frame channel sender and viewport ID so the
    /// peer task can push a frame and repaint the exact viewport.
    pub viewport_sinks: HashMap<u64, ViewportSink>,
    /// Frames that arrived before the viewport sink was registered.
    /// Drained when the sink is inserted via `register_viewport_sink`.
    pending_first_frames: HashMap<u64, PlaybackFrame>,
    /// Window IDs the host has requested all peers to open (consumed by UI thread).
    pub force_open_window_ids: HashSet<u64>,
    /// How the peer task wakes whatever is drawing. `None` before a front
    /// end has registered one, in which case the task still runs and the
    /// screen simply catches up on its next pass.
    ///
    /// Not part of this state's `Debug`: a waker is a callback into the front
    /// end, and printing it says nothing about the session.
    pub waker: Option<Arc<dyn SessionWaker>>,
}

impl std::fmt::Debug for SessionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionState")
            .field("role", &self.role)
            .field("my_user_id", &self.my_user_id)
            .field("host_user_id", &self.host_user_id)
            .field("frame_source_id", &self.frame_source_id)
            .field("connected_users", &self.connected_users)
            .field("permissions", &self.permissions)
            .field("status", &self.status)
            .field("open_replays", &self.open_replays)
            .field("tactics_boards", &self.tactics_boards)
            .field("viewport_sinks", &self.viewport_sinks)
            .field("woken", &self.waker.is_some())
            .finish_non_exhaustive()
    }
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            role: PeerRole::Host,
            my_user_id: 0,
            host_user_id: 0,
            frame_source_id: 0,
            connected_users: Vec::new(),
            permissions: Permissions::default(),
            cursors: Vec::new(),
            token: None,
            status: SessionStatus::Idle,
            open_replays: Vec::new(),
            render_options_version: 0,
            current_render_options: None,
            annotation_sync_version: 0,
            current_annotation_sync: None,
            range_override_version: 0,
            current_range_overrides: None,
            trail_override_version: 0,
            current_trail_hidden: None,
            pings: Vec::new(),
            tactics_boards: HashMap::new(),
            tactics_boards_version: 0,
            viewport_sinks: HashMap::new(),
            pending_first_frames: HashMap::new(),
            force_open_window_ids: HashSet::new(),
            waker: None,
        }
    }
}

impl SessionState {
    /// Reset all session-specific sync state so stale data does not leak
    /// into the next session.
    pub fn clear_session_data(&mut self) {
        self.status = SessionStatus::Idle;
        self.token = None;
        self.connected_users.clear();
        self.cursors.clear();
        self.open_replays.clear();
        self.render_options_version = 0;
        self.current_render_options = None;
        self.annotation_sync_version = 0;
        self.current_annotation_sync = None;
        self.range_override_version = 0;
        self.current_range_overrides = None;
        self.trail_override_version = 0;
        self.current_trail_hidden = None;
        self.pings.clear();
        self.tactics_boards.clear();
        self.tactics_boards_version = 0;
        self.force_open_window_ids.clear();
        self.viewport_sinks.clear();
        self.pending_first_frames.clear();
        self.permissions = Permissions::default();
    }

    /// Push a playback frame to the renderer for the given window ID
    /// (replay_id) and request a repaint of that specific viewport.
    /// If no sink is registered yet, buffers the frame so it can be
    /// delivered when the sink is created via `register_viewport_sink`.
    pub fn push_frame(&mut self, window_id: u64, frame: PlaybackFrame) {
        if let Some(sink) = self.viewport_sinks.get(&window_id) {
            if let Some(ref tx) = sink.frame_tx {
                let _ = tx.try_send(frame);
            }
            self.wake_window(window_id);
        } else {
            // Sink not yet registered: buffer so the first frame is not lost.
            self.pending_first_frames.insert(window_id, frame);
        }
    }

    /// Register a viewport sink and flush any pending first frame into it.
    pub fn register_viewport_sink(&mut self, window_id: u64, sink: ViewportSink) {
        let pending = self.pending_first_frames.remove(&window_id);
        if let Some(frame) = pending {
            if let Some(ref tx) = sink.frame_tx {
                let _ = tx.try_send(frame);
            }
            self.viewport_sinks.insert(window_id, sink);
            self.wake_window(window_id);
            return;
        }
        self.viewport_sinks.insert(window_id, sink);
    }

    /// Wakes whatever is showing one window, by replay or board id.
    ///
    /// A window with no sink registered is not woken: nothing is drawing it.
    pub fn repaint_viewport(&self, window_id: u64) {
        self.wake_window(window_id);
    }

    fn wake_window(&self, window_id: u64) {
        if let Some(sink) = self.viewport_sinks.get(&window_id)
            && let Some(wake) = &sink.wake
        {
            wake();
        }
    }

    /// Wakes whatever is showing the session as a whole.
    pub fn wake(&self) {
        if let Some(waker) = &self.waker {
            waker.wake();
        }
    }
}

/// Connection status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionStatus {
    Idle,
    Starting,
    Active,
    Connecting,
    Error(String),
}

/// A connected user in the session.
#[derive(Debug, Clone)]
pub struct ConnectedUser {
    pub id: u64,
    pub name: String,
    pub color: [u8; 3],
    pub role: PeerRole,
    pub client_type: protocol::ClientType,
}
/// Events sent from the background session task to the UI thread.
#[derive(Debug)]
pub enum SessionEvent {
    /// Session is now active (host: token ready; joiner: connected + mesh established).
    Started,
    /// A user joined the session.
    UserJoined(ConnectedUser),
    /// A user left the session.
    UserLeft { user_id: u64, name: String, timed_out: bool },
    /// Session ended (host stopped or disconnected).
    Ended,
    /// An error occurred.
    Error(String),
    /// Join-only: connection was rejected by the host.
    Rejected(String),
    /// Join-only: session info received with the list of currently open replays.
    SessionInfoReceived { open_replays: Vec<OpenReplay> },
    /// A peer was promoted to co-host.
    PeerPromoted { user_id: u64 },
    /// The frame source changed.
    FrameSourceChanged { source_user_id: u64 },
    /// A new replay was opened on the host.
    ReplayOpened {
        replay_id: u64,
        replay_name: String,
        map_image_png: Vec<u8>,
        game_version: String,
        map_name: String,
        display_name: String,
    },
    /// A replay was closed on the host.
    ReplayClosed { replay_id: u64 },
}

/// Commands sent from the UI thread to the background session task.
#[derive(Debug)]
pub enum SessionCommand {
    /// Stop the session.
    Stop,
    /// Update permission flags (host/co-host only).
    SetPermissions(Permissions),
    /// Reset all peer display overrides (host/co-host only).
    ResetClientOverrides,
    /// Broadcast current annotation state (host/co-host only).
    /// `board_id`: `None` = replay context, `Some(id)` = tactics board.
    SyncAnnotations { board_id: Option<u64>, annotations: Vec<types::Annotation>, owners: Vec<u64>, ids: Vec<u64> },
    /// Promote a peer to co-host (host only).
    PromoteToCoHost { user_id: u64 },
    /// Declare self as frame source (host/co-host only).
    BecomeFrameSource,
    /// Notify peers that a replay was opened on the host.
    ReplayOpened {
        replay_id: u64,
        replay_name: String,
        map_image_png: Vec<u8>,
        game_version: String,
        map_name: String,
        display_name: String,
    },
    /// Notify peers that a replay was closed on the host.
    ReplayClosed { replay_id: u64 },
    /// Broadcast full cap point state for a specific tactics board.
    SyncCapPoints { board_id: u64, cap_points: Vec<protocol::WireCapPoint> },
    /// Request all peers to open a specific window (replay or tactics board).
    OpenWindowForEveryone { window_id: u64 },
}

/// Create a shared session state wrapped in Arc<Mutex>.
pub fn shared_session_state() -> Arc<Mutex<SessionState>> {
    Arc::new(Mutex::new(SessionState::default()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── PeerRole ────────────────────────────────────────────────────────

    #[test]
    fn peer_role_is_host() {
        assert!(PeerRole::Host.is_host());
        assert!(!PeerRole::Host.is_co_host());
        assert!(!PeerRole::Host.is_peer());
    }

    #[test]
    fn peer_role_is_co_host() {
        assert!(!PeerRole::CoHost.is_host());
        assert!(PeerRole::CoHost.is_co_host());
        assert!(!PeerRole::CoHost.is_peer());
    }

    #[test]
    fn peer_role_is_peer() {
        assert!(!PeerRole::Peer.is_host());
        assert!(!PeerRole::Peer.is_co_host());
        assert!(PeerRole::Peer.is_peer());
    }

    #[test]
    fn peer_role_equality() {
        assert_eq!(PeerRole::Host, PeerRole::Host);
        assert_ne!(PeerRole::Host, PeerRole::CoHost);
        assert_ne!(PeerRole::Host, PeerRole::Peer);
        assert_ne!(PeerRole::CoHost, PeerRole::Peer);
    }

    // ─── Permissions ─────────────────────────────────────────────────────

    #[test]
    fn permissions_default_unlocked() {
        let perms = Permissions::default();
        assert!(!perms.annotations_locked);
        assert!(!perms.settings_locked);
    }

    // ─── AnnotationSyncState ─────────────────────────────────────────────

    #[test]
    fn annotation_sync_state_default_empty() {
        let state = AnnotationSyncState::default();
        assert!(state.annotations.is_empty());
        assert!(state.owners.is_empty());
        assert!(state.ids.is_empty());
    }

    #[test]
    fn annotation_sync_state_clone_is_independent() {
        let mut state = AnnotationSyncState::default();
        state.annotations.push(types::Annotation::Circle {
            center: [100.0, 200.0],
            radius: 50.0,
            color: [255, 0, 0, 255],
            width: 3.0,
            filled: false,
        });
        state.owners.push(0);
        state.ids.push(42);

        let cloned = state.clone();
        state.annotations.clear();
        assert!(state.annotations.is_empty());
        assert_eq!(cloned.annotations.len(), 1);
    }

    // ─── SessionState ────────────────────────────────────────────────────

    #[test]
    fn session_state_defaults() {
        let state = SessionState::default();
        assert_eq!(state.role, PeerRole::Host);
        assert_eq!(state.my_user_id, 0);
        assert_eq!(state.host_user_id, 0);
        assert_eq!(state.frame_source_id, 0);
        assert!(state.connected_users.is_empty());
        assert!(!state.permissions.annotations_locked);
        assert!(!state.permissions.settings_locked);
        assert!(state.cursors.is_empty());
        assert!(state.token.is_none());
        assert_eq!(state.status, SessionStatus::Idle);
        assert!(state.open_replays.is_empty());
        assert_eq!(state.render_options_version, 0);
        assert!(state.current_render_options.is_none());
        assert_eq!(state.annotation_sync_version, 0);
        assert!(state.current_annotation_sync.is_none());
        assert_eq!(state.range_override_version, 0);
        assert!(state.current_range_overrides.is_none());
        assert_eq!(state.trail_override_version, 0);
        assert!(state.current_trail_hidden.is_none());
        assert!(state.pings.is_empty());
        assert!(state.tactics_boards.is_empty());
        assert_eq!(state.tactics_boards_version, 0);
        assert!(state.force_open_window_ids.is_empty());
        assert!(state.viewport_sinks.is_empty());
    }

    #[test]
    fn session_status_equality() {
        assert_eq!(SessionStatus::Idle, SessionStatus::Idle);
        assert_eq!(SessionStatus::Active, SessionStatus::Active);
        assert_eq!(SessionStatus::Starting, SessionStatus::Starting);
        assert_eq!(SessionStatus::Connecting, SessionStatus::Connecting);
        assert_ne!(SessionStatus::Idle, SessionStatus::Active);
        assert_eq!(SessionStatus::Error("test".into()), SessionStatus::Error("test".into()));
        assert_ne!(SessionStatus::Error("a".into()), SessionStatus::Error("b".into()));
    }

    #[test]
    fn shared_session_state_is_arc_mutex() {
        let state = shared_session_state();
        // Should be accessible from multiple references.
        let state2 = Arc::clone(&state);
        state.lock().my_user_id = 42;
        assert_eq!(state2.lock().my_user_id, 42);
    }

    // ─── ConnectedUser ───────────────────────────────────────────────────

    #[test]
    fn connected_user_clone() {
        let user = ConnectedUser {
            id: 1,
            name: "Alice".into(),
            color: [255, 0, 0],
            role: PeerRole::Host,
            client_type: protocol::ClientType::Desktop { toolkit_version: "test".into() },
        };
        let cloned = user.clone();
        assert_eq!(cloned.id, 1);
        assert_eq!(cloned.name, "Alice");
        assert_eq!(cloned.color, [255, 0, 0]);
        assert_eq!(cloned.role, PeerRole::Host);
    }

    // ─── OpenReplay ──────────────────────────────────────────────────────

    #[test]
    fn open_replay_clone() {
        let replay = OpenReplay {
            replay_id: 1,
            replay_name: "test.wowsreplay".into(),
            map_image_png: vec![0u8; 100],
            game_version: "13.5.0".into(),
            map_name: "spaces/16_OC_bees_to_honey".into(),
            display_name: "Bees to Honey".into(),
        };
        let cloned = replay.clone();
        assert_eq!(cloned.replay_id, 1);
        assert_eq!(cloned.replay_name, "test.wowsreplay");
    }
}
