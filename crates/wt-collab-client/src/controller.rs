//! A session this app is running, and what it is doing: hosting one, joining
//! one, leaving, steering it, and what happened in it since the last look.
//!
//! The front end owns the controls, the display name and the waking; this is
//! the part every front end runs the same way.

use std::sync::Arc;

use parking_lot::Mutex;
use thiserror::Error;

use crate::PeerRole;
use crate::Permissions;
use crate::SessionCommand;
use crate::SessionEvent;
use crate::SessionState;
use crate::SessionStatus;
use crate::SessionWaker;
use crate::WEB_CLIENT_URL;
use crate::link::CollabLink;
use crate::peer::HostParams;
use crate::peer::JoinParams;
use crate::peer::PeerMode;
use crate::peer::PeerSessionHandle;

/// What a session token is prefixed with, so a paste can be checked before it
/// is sent anywhere.
pub const TOKEN_PREFIX: &str = "toolkit-";

/// Why a session could not be started.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum StartError {
    #[error("a session is already running")]
    AlreadyRunning,
    /// Every other peer reads the name, and an empty one makes a roster
    /// nobody can read.
    #[error("a display name is required")]
    NoDisplayName,
    #[error("the token does not start with {expected_prefix}")]
    InvalidToken { expected_prefix: &'static str },
}

/// Why the last session ended badly or never started, for the line under the
/// controls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionFailure {
    /// A pasted token is not one this app issues.
    InvalidToken { expected_prefix: &'static str },
    /// The session task gave up, with its reason.
    Error(String),
    /// The host refused the join, with its reason.
    Rejected(String),
}

/// Something that happened in the session that the reader should be told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionNotice {
    /// This app's hosted session is up.
    HostingStarted,
    /// This app has joined someone else's session.
    Connected,
    UserJoined { name: String },
    UserLeft { name: String },
    UserTimedOut { name: String },
    /// The host opened a replay.
    ReplayOpened { replay_name: String },
    /// The host closed a replay.
    ReplayClosed,
    Ended,
    Failed { reason: String },
    Rejected { reason: String },
}

impl SessionNotice {
    /// Whether the notice is about something going wrong, for a front end
    /// that shows those differently.
    pub fn severity(&self) -> NoticeSeverity {
        match self {
            Self::UserTimedOut { .. } => NoticeSeverity::Warning,
            Self::Failed { .. } | Self::Rejected { .. } => NoticeSeverity::Error,
            _ => NoticeSeverity::Info,
        }
    }
}

/// How a [`SessionNotice`] is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeSeverity {
    Info,
    Warning,
    Error,
}

/// One battle the session is on.
#[derive(Clone, Debug)]
pub struct SharedWindow {
    pub replay_id: u64,
    pub replay_name: String,
    /// The map it is played on, translated where the host said so.
    pub map: String,
    /// The map's space name, for a viewport that has this build's own art.
    pub map_name: String,
    /// The art the end that owns the replay sent, for a map this build ships
    /// none of. `None` where it sent none, which the wire states as empty.
    pub art_png: Option<Vec<u8>>,
}

/// One participant, as the roster shows them.
#[derive(Clone, Debug)]
pub struct ConnectedPeer {
    pub user_id: u64,
    pub name: String,
    pub role: PeerRole,
    /// Whether this is the reader, which the roster marks.
    pub is_me: bool,
}

/// A session this app is running, and what it is doing.
pub struct SessionController {
    /// The live session, if one is running. `None` means neither hosting nor
    /// joined, which is the resting state rather than a failure.
    handle: Option<PeerSessionHandle>,
    /// What the peer task and this side both read. Held even with no session
    /// so a waker can be installed once and kept.
    state: Arc<Mutex<SessionState>>,
    /// Whether this app started the session or joined someone else's.
    hosting: bool,
    failure: Option<SessionFailure>,
    /// Where the asset bundle for web clients is kept once built.
    bundle: Arc<Mutex<Option<Vec<u8>>>>,
}

impl SessionController {
    pub fn new() -> Self {
        Self {
            handle: None,
            state: Arc::new(Mutex::new(SessionState::default())),
            hosting: false,
            failure: None,
            bundle: Arc::new(Mutex::new(None)),
        }
    }

    /// The state the peer task writes, for a front end that reads more of it
    /// than this type summarizes.
    pub fn state(&self) -> &Arc<Mutex<SessionState>> {
        &self.state
    }

    /// Installs the waker the peer task calls on every change, unless one is
    /// already installed. Returns whether this one was.
    pub fn install_waker(&self, waker: Arc<dyn SessionWaker>) -> bool {
        let mut held = self.state.lock();
        if held.waker.is_some() {
            return false;
        }
        held.waker = Some(waker);

        true
    }

    /// Whether a session is running, hosted or joined.
    pub fn is_active(&self) -> bool {
        self.handle.is_some()
    }

    pub fn is_hosting(&self) -> bool {
        self.hosting && self.handle.is_some()
    }

    /// What the session is doing.
    pub fn status(&self) -> SessionStatus {
        self.state.lock().status.clone()
    }

    /// What went wrong starting, joining or running the last session.
    pub fn failure(&self) -> Option<&SessionFailure> {
        self.failure.as_ref()
    }

    /// The token peers join with. `None` until the host has one, which is
    /// after the endpoint is published.
    pub fn token(&self) -> Option<String> {
        self.state.lock().token.clone()
    }

    /// A link a browser can join from.
    pub fn web_link(&self) -> Option<String> {
        self.token().map(|token| format!("{WEB_CLIENT_URL}#{token}"))
    }

    /// The link to the session on this machine, for a web client being worked
    /// on beside the app.
    pub fn localhost_link(&self) -> Option<String> {
        self.token().map(|token| format!("http://localhost:8080/#{token}"))
    }

    /// What the session is showing, as the host has told everyone.
    pub fn shared_windows(&self) -> Vec<SharedWindow> {
        let state = self.state.lock();
        state
            .open_replays
            .iter()
            .map(|replay| SharedWindow {
                replay_id: replay.replay_id,
                replay_name: replay.replay_name.clone(),
                map_name: replay.map_name.clone(),
                art_png: (!replay.map_image_png.is_empty()).then(|| replay.map_image_png.clone()),
                // The protocol carries no display name as an empty string,
                // which is the map's own name here rather than a blank label.
                map: match replay.display_name.as_str() {
                    "" => replay.map_name.clone(),
                    named => named.to_owned(),
                },
            })
            .collect()
    }

    /// Everyone in the session, this app included.
    pub fn connected(&self) -> Vec<ConnectedPeer> {
        let held = self.state.lock();
        let me = held.my_user_id;
        held.connected_users
            .iter()
            .map(|user| ConnectedPeer { user_id: user.id, name: user.name.clone(), role: user.role, is_me: user.id == me })
            .collect()
    }

    pub fn permissions(&self) -> Permissions {
        self.state.lock().permissions.clone()
    }

    /// Whether this app may change permissions and promote peers.
    pub fn may_steer(&self) -> bool {
        let role = self.state.lock().role;
        role.is_host() || role.is_co_host()
    }

    /// What a viewport needs of the session: where peers are pointing, and
    /// somewhere to say where this app is pointing. Inert without a session.
    pub fn link(&self) -> CollabLink {
        let Some(handle) = &self.handle else {
            return CollabLink::inert(Arc::clone(&self.state));
        };
        // Only a host or a co-host has a mesh to send frames into.
        let frames = self.may_steer().then(|| handle.frame_tx.clone());

        CollabLink::connected(
            Arc::clone(&self.state),
            handle.local_tx.clone(),
            frames,
            handle.command_tx.clone(),
        )
    }

    /// Starts hosting on `runtime`.
    pub fn host(
        &mut self,
        runtime: Arc<tokio::runtime::Runtime>,
        toolkit_version: String,
        display_name: &str,
    ) -> Result<(), StartError> {
        let display_name = self.check_start(display_name)?;
        let params = HostParams {
            toolkit_version,
            display_name,
            initial_render_options: Default::default(),
            web_asset_bundle: Arc::clone(&self.bundle),
        };
        self.start(runtime, PeerMode::Host(params), true);

        Ok(())
    }

    /// Joins the session `token` names.
    ///
    /// The token is checked here rather than at the far end, so a mis-paste
    /// says so at once instead of timing out against nothing.
    pub fn join(
        &mut self,
        runtime: Arc<tokio::runtime::Runtime>,
        token: &str,
        toolkit_version: String,
        display_name: &str,
    ) -> Result<(), StartError> {
        let display_name = self.check_start(display_name)?;
        let token = token.trim().to_owned();
        if !token.starts_with(TOKEN_PREFIX) {
            self.failure = Some(SessionFailure::InvalidToken { expected_prefix: TOKEN_PREFIX });
            return Err(StartError::InvalidToken { expected_prefix: TOKEN_PREFIX });
        }
        let params = JoinParams { token, display_name, toolkit_version };
        self.start(runtime, PeerMode::Join(params), false);

        Ok(())
    }

    fn check_start(&self, display_name: &str) -> Result<String, StartError> {
        if self.handle.is_some() {
            return Err(StartError::AlreadyRunning);
        }
        let display_name = display_name.trim();
        if display_name.is_empty() {
            return Err(StartError::NoDisplayName);
        }

        Ok(display_name.to_owned())
    }

    fn start(&mut self, runtime: Arc<tokio::runtime::Runtime>, mode: PeerMode, hosting: bool) {
        self.failure = None;
        self.hosting = hosting;

        // What the last session was left holding is not this one's: its own
        // list arrives with the handshake. Kept past `leave` so a viewport
        // could carry it onto its own end.
        {
            let mut held = self.state.lock();
            held.current_annotation_sync = None;
            held.tactics_boards.clear();
        }

        self.handle = Some(crate::peer::start_peer_session(runtime, mode, Arc::clone(&self.state)));
    }

    /// Leaves or stops the session.
    ///
    /// The peer task is told to stop and the shared state is cleared here
    /// rather than waited for, so the controls return at once even if the
    /// mesh takes a moment to wind down.
    pub fn leave(&mut self) {
        if let Some(handle) = self.handle.take() {
            let _ = handle.command_tx.send(SessionCommand::Stop);
        }
        self.hosting = false;

        let mut held = self.state.lock();
        // Held back from the clearing: a session ending does not rub out what
        // was drawn in it, and each viewport reads it once more as it is
        // handed an inert link. `start` clears it when a new session begins.
        let drawn = held.current_annotation_sync.take();
        let boards = std::mem::take(&mut held.tactics_boards);
        held.clear_session_data();
        held.current_annotation_sync = drawn;
        held.tactics_boards = boards;
    }

    /// Whether this app may raise peers to co-host: only the original host.
    pub fn may_promote(&self) -> bool {
        self.state.lock().role.is_host()
    }

    /// Locks or unlocks what peers may change. Held here at once, so a
    /// reader sees the change before the peer task applies it.
    pub fn set_permissions(&self, permissions: Permissions) {
        if self.handle.is_none() {
            return;
        }
        self.state.lock().permissions = permissions.clone();
        self.command(SessionCommand::SetPermissions(permissions));
    }

    /// Returns every peer's display settings to the host's.
    pub fn reset_overrides(&self) {
        self.command(SessionCommand::ResetClientOverrides);
    }

    /// Raises a peer to co-host.
    pub fn promote(&self, user_id: u64) {
        self.command(SessionCommand::PromoteToCoHost { user_id });
    }

    fn command(&self, command: SessionCommand) {
        if let Some(handle) = &self.handle {
            let _ = handle.command_tx.send(command);
        }
    }

    /// Takes what the peer task has raised since the last look and acts on
    /// it. Has to be called while a session runs: the inbox is unbounded.
    pub fn poll(&mut self) -> Vec<SessionNotice> {
        let Some(handle) = &self.handle else {
            return Vec::new();
        };
        let events = handle.event_inbox.drain();

        let mut notices = Vec::new();
        for event in events {
            match event {
                SessionEvent::Started => {
                    notices.push(if self.hosting { SessionNotice::HostingStarted } else { SessionNotice::Connected });
                }
                SessionEvent::UserJoined(user) => notices.push(SessionNotice::UserJoined { name: user.name }),
                SessionEvent::UserLeft { name, timed_out: true, .. } => {
                    notices.push(SessionNotice::UserTimedOut { name });
                }
                SessionEvent::UserLeft { name, timed_out: false, .. } => {
                    notices.push(SessionNotice::UserLeft { name });
                }
                // The roster and the token are read from the shared state, so
                // these need nothing beyond the redraw.
                SessionEvent::PeerPromoted { .. }
                | SessionEvent::FrameSourceChanged { .. }
                | SessionEvent::SessionInfoReceived { .. } => {}
                SessionEvent::ReplayOpened { replay_name, .. } => {
                    notices.push(SessionNotice::ReplayOpened { replay_name });
                }
                SessionEvent::ReplayClosed { .. } => notices.push(SessionNotice::ReplayClosed),
                SessionEvent::Ended => {
                    notices.push(SessionNotice::Ended);
                    self.leave();
                }
                SessionEvent::Error(reason) => {
                    notices.push(SessionNotice::Failed { reason: reason.clone() });
                    self.failure = Some(SessionFailure::Error(reason));
                    self.leave();
                }
                SessionEvent::Rejected(reason) => {
                    notices.push(SessionNotice::Rejected { reason: reason.clone() });
                    self.failure = Some(SessionFailure::Rejected(reason));
                    self.leave();
                }
            }
        }

        notices
    }
}

impl Default for SessionController {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(id: u64, name: &str, role: PeerRole) -> crate::ConnectedUser {
        crate::ConnectedUser {
            id,
            name: name.into(),
            color: [0, 0, 0],
            role,
            client_type: crate::protocol::ClientType::Desktop { toolkit_version: "test".into() },
        }
    }

    #[test]
    fn a_session_that_has_not_started_is_not_active() {
        let collab = SessionController::new();

        assert!(!collab.is_active());
        assert!(!collab.is_hosting());
        assert!(collab.token().is_none());
        assert!(collab.connected().is_empty());
        assert!(!collab.link().is_active());
    }

    #[test]
    fn a_web_link_carries_the_token_in_its_fragment() {
        let collab = SessionController::new();
        collab.state.lock().token = Some("toolkit-abcdef".to_string());

        let link = collab.web_link().expect("there is a token");

        assert!(link.starts_with(WEB_CLIENT_URL));
        assert!(link.ends_with("#toolkit-abcdef"));
    }

    #[test]
    fn there_is_no_link_without_a_token() {
        assert!(SessionController::new().web_link().is_none());
    }

    #[test]
    fn the_roster_marks_which_peer_is_the_reader() {
        let collab = SessionController::new();
        {
            let mut held = collab.state.lock();
            held.my_user_id = 7;
            held.connected_users = vec![user(7, "me", PeerRole::Host), user(9, "them", PeerRole::Peer)];
        }

        let roster = collab.connected();

        assert_eq!(roster.len(), 2);
        assert!(roster[0].is_me);
        assert!(!roster[1].is_me);
    }

    #[test]
    fn leaving_clears_what_the_session_held() {
        let mut collab = SessionController::new();
        {
            let mut held = collab.state.lock();
            held.token = Some("toolkit-abcdef".into());
            held.status = SessionStatus::Active;
        }

        collab.leave();

        // Carrying a token across sessions would offer entry to one that has
        // ended.
        assert!(collab.token().is_none());
        assert_eq!(collab.status(), SessionStatus::Idle);
    }

    /// Leaving does not rub out what was drawn in the session: each viewport
    /// is handed an inert link and reads the shapes onto its own end as it
    /// takes one, which it cannot do if they have already been cleared.
    #[test]
    fn leaving_leaves_what_was_drawn_where_a_viewport_can_carry_it() {
        let mut collab = SessionController::new();
        {
            let mut held = collab.state.lock();
            held.status = SessionStatus::Active;
            held.current_annotation_sync = Some(crate::AnnotationSyncState {
                annotations: vec![crate::types::Annotation::Circle {
                    center: [1.0, 2.0],
                    radius: 3.0,
                    color: [255, 0, 0, 255],
                    width: 2.0,
                    filled: false,
                }],
                owners: vec![1],
                ids: vec![77],
            });
            held.tactics_boards.insert(42, crate::TacticsBoardSessionState::default());
        }

        collab.leave();

        let held = collab.state.lock();
        assert!(held.current_annotation_sync.is_some(), "still readable for the viewport that draws it");
        assert!(held.tactics_boards.contains_key(&42), "and so is the board's own");
    }

    #[test]
    fn nothing_starts_without_a_name() {
        let mut collab = SessionController::new();
        let runtime = Arc::new(tokio::runtime::Builder::new_current_thread().build().expect("a runtime"));

        assert_eq!(collab.host(Arc::clone(&runtime), "test".into(), "  "), Err(StartError::NoDisplayName));
        assert_eq!(collab.join(runtime, "toolkit-abc", "test".into(), ""), Err(StartError::NoDisplayName));
        assert!(!collab.is_active());
    }

    /// A mis-pasted token is refused here, and says so under the controls.
    #[test]
    fn a_token_from_elsewhere_is_refused_before_it_is_sent() {
        let mut collab = SessionController::new();
        let runtime = Arc::new(tokio::runtime::Builder::new_current_thread().build().expect("a runtime"));

        let joined = collab.join(runtime, "https://example.com", "test".into(), "me");

        assert_eq!(joined, Err(StartError::InvalidToken { expected_prefix: TOKEN_PREFIX }));
        assert_eq!(collab.failure(), Some(&SessionFailure::InvalidToken { expected_prefix: TOKEN_PREFIX }));
        assert!(!collab.is_active());
    }

    #[test]
    fn a_shared_window_without_a_display_name_is_called_by_its_map() {
        let collab = SessionController::new();
        collab.state.lock().open_replays = vec![crate::OpenReplay {
            replay_id: 5,
            replay_name: "battle.wowsreplay".into(),
            map_image_png: Vec::new(),
            game_version: "14.0".into(),
            map_name: "spaces/16_OC_bees_to_honey".into(),
            display_name: String::new(),
        }];

        let [window] = &collab.shared_windows()[..] else { panic!("one window is listed") };

        assert_eq!(window.map, "spaces/16_OC_bees_to_honey");
        assert!(window.art_png.is_none());
    }
}
