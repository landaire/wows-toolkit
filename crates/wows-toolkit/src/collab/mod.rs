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
//! The iroh networking runs in a background tokio task. Communication with the
//! egui UI thread uses `std::sync::mpsc` channels and `Arc<Mutex<SessionState>>`,
//! following the same pattern as the existing `PlaybackCommand` channel.

pub mod peer;
pub mod protocol;
pub mod types;
pub mod validation;

use std::sync::Arc;
use std::sync::Mutex;
use std::time::Instant;

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
}

/// Shared session state visible to the UI thread.
///
/// Stored behind `Arc<Mutex<>>` and read each frame by the renderer UI.
/// Written by the background collab task.
#[derive(Debug)]
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
    pub current_annotation_sync: Option<(Vec<types::Annotation>, Vec<u64>)>,
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
}

/// A user's cursor position with metadata for rendering.
#[derive(Debug, Clone)]
pub struct UserCursor {
    pub user_id: u64,
    pub name: String,
    pub color: [u8; 3],
    /// Minimap-space position. None = cursor not on the minimap.
    pub pos: Option<[f32; 2]>,
    /// When the cursor position was last updated (for fade-out).
    pub last_update: Instant,
}

/// Permission flags controlled by the host/co-host.
#[derive(Debug, Clone, Default)]
pub struct Permissions {
    /// When true, peers cannot add or undo annotations.
    pub annotations_locked: bool,
    /// When true, peers cannot toggle display options.
    pub settings_locked: bool,
}

// ─── UI ↔ Session task communication ────────────────────────────────────────

/// Events sent from the background session task to the UI thread.
#[derive(Debug)]
pub enum SessionEvent {
    /// Session is now active (host: token ready; joiner: connected + mesh established).
    Started,
    /// A user joined the session.
    UserJoined(ConnectedUser),
    /// A user left the session.
    UserLeft { user_id: u64 },
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
    ReplayOpened { replay_id: u64, replay_name: String, map_image_png: Vec<u8>, game_version: String },
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
    SyncAnnotations { annotations: Vec<types::Annotation>, owners: Vec<u64> },
    /// Promote a peer to co-host (host only).
    PromoteToCoHost { user_id: u64 },
    /// Declare self as frame source (host/co-host only).
    BecomeFrameSource,
    /// Notify peers that a replay was opened on the host.
    ReplayOpened { replay_id: u64, replay_name: String, map_image_png: Vec<u8>, game_version: String },
    /// Notify peers that a replay was closed on the host.
    ReplayClosed { replay_id: u64 },
}

/// Create a shared session state wrapped in Arc<Mutex>.
pub fn shared_session_state() -> Arc<Mutex<SessionState>> {
    Arc::new(Mutex::new(SessionState::default()))
}
