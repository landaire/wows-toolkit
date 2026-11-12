//! Collaborative replay sessions: hosting one, joining one, and what the
//! session is doing while it runs.
//!
//! The session itself is `wt-collab-client`, shared with the egui app, so
//! both front ends run one implementation of the mesh. What is here is the
//! part that is this front end's own: waking it when the peer task changes
//! something, and the controls that start, join and steer a session.

use std::sync::Arc;

use gpui_kit::App;
use gpui_kit::AsyncApp;
use gpui_kit::Entity;
use parking_lot::Mutex;
use wt_collab_client::PeerPing;
use wt_collab_client::PeerRole;
use wt_collab_client::Permissions;
use wt_collab_client::SessionCommand;
use wt_collab_client::SessionEvent;
use wt_collab_client::SessionState;
use wt_collab_client::SessionStatus;
use wt_collab_client::SessionWaker;
use wt_collab_client::UserCursor;
use wt_collab_client::peer::HostParams;
use wt_collab_client::peer::JoinParams;
use wt_collab_client::peer::LocalEvent;
use wt_collab_client::peer::PeerMode;
use wt_collab_client::peer::PeerSessionHandle;

use crate::runtime;

/// Where the web client is served. The session token rides in the fragment.
const WEB_CLIENT_URL: &str = wt_collab_client::WEB_CLIENT_URL;

/// What a session token is prefixed with, so a paste can be checked before it
/// is sent anywhere.
const TOKEN_PREFIX: &str = "toolkit-";

/// Wakes this front end when the peer task changes the session.
///
/// The task runs off the UI thread, so a change it makes reaches the screen
/// only when something asks for a redraw. Holding the entity weakly: the
/// session outlives a closed window, and waking a view that has gone is not
/// an error, just nothing to do.
/// Wakes this front end when the peer task changes the session.
///
/// A wake arrives on a tokio worker, and neither a GPUI entity nor an
/// `AsyncApp` may cross threads, so the wake is a nudge down a channel that a
/// GPUI task reads and turns into a notify. Unbounded and non-blocking: the
/// peer task must never wait on a redraw.
struct GpuiWaker {
    nudge: futures::channel::mpsc::UnboundedSender<()>,
}

impl SessionWaker for GpuiWaker {
    fn wake(&self) {
        let _ = self.nudge.unbounded_send(());
    }
}

/// A session this app is running, and what it is doing.
pub struct CollabState {
    /// The live session, if one is running. `None` means neither hosting nor
    /// joined, which is the resting state rather than a failure.
    handle: Option<PeerSessionHandle>,
    /// What the peer task and this side both read. Held even with no session
    /// so a waker can be installed once and kept.
    pub state: Arc<Mutex<SessionState>>,
    /// Whether this app started the session or joined someone else's.
    hosting: bool,
    /// The name this app appears under. Empty until one is entered, which is
    /// what refuses the start and join controls.
    pub display_name: String,
    /// Whether the token is shown rather than masked.
    pub token_revealed: bool,
    /// What went wrong starting or joining, for the line under the controls.
    pub failure: Option<String>,
    /// Where the asset bundle for web clients is kept once built. The port
    /// builds none yet, so a web client joining sees no map art.
    bundle: Arc<Mutex<Option<Vec<u8>>>>,
}

impl Default for CollabState {
    fn default() -> Self {
        Self {
            handle: None,
            state: Arc::new(Mutex::new(SessionState::default())),
            hosting: false,
            display_name: String::new(),
            token_revealed: false,
            failure: None,
            bundle: Arc::new(Mutex::new(None)),
        }
    }
}

impl CollabState {
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

    /// The token peers join with. `None` until the host has one, which is
    /// after the endpoint is published.
    pub fn token(&self) -> Option<String> {
        self.state.lock().token.clone()
    }

    /// The token as it should be shown: masked unless revealed, because it
    /// grants entry to the session and a shared screen is where it is read.
    pub fn token_display(&self) -> Option<String> {
        let token = self.token()?;
        if self.token_revealed { Some(token) } else { Some("*".repeat(token.chars().count().min(32))) }
    }

    /// The link to the session on this machine, for a web client being worked on
    /// beside the app. Debug builds only, as the egui popover offers it.
    #[cfg(debug_assertions)]
    pub fn localhost_link(&self) -> Option<String> {
        self.token().map(|token| format!("http://localhost:8080/#{token}"))
    }

    /// What the session is showing, as the host has told everyone.
    ///
    /// A peer in this app plays a battle back from its own copy of the replay, so
    /// this is a list of what the session is on rather than a set of windows to
    /// open: a reader who has the same replay can open it themselves, and one who
    /// does not cannot be shown it (see `docs/gpui-port-gaps.md`, item 22).
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

    /// A link a browser can join from.
    pub fn web_link(&self) -> Option<String> {
        self.token().map(|token| format!("{WEB_CLIENT_URL}#{token}"))
    }

    /// Everyone in the session, this app included.
    pub fn connected(&self) -> Vec<ConnectedPeer> {
        let held = self.state.lock();
        let me = held.my_user_id;
        held.connected_users
            .iter()
            .map(|user| ConnectedPeer {
                user_id: user.id,
                name: user.name.clone(),
                role: user.role,
                is_me: user.id == me,
            })
            .collect()
    }

    /// What the host has locked.
    /// What a viewport needs of the session: where peers are pointing, and
    /// somewhere to say where this app is pointing.
    pub fn link(&self) -> CollabLink {
        CollabLink {
            state: Some(Arc::clone(&self.state)),
            local_tx: self.handle.as_ref().map(|handle| handle.local_tx.clone()),
            board: None,
            alone: Arc::new(Mutex::new(wt_collab_client::AnnotationSyncState::default())),
            next_alone_id: Arc::new(std::sync::atomic::AtomicU64::new(1)),
            // Only a host or a co-host has a mesh to send frames into.
            frames: self.handle.as_ref().filter(|_| self.may_steer()).map(|handle| handle.frame_tx.clone()),
            commands: self.handle.as_ref().map(|handle| handle.command_tx.clone()),
        }
    }

    pub fn permissions(&self) -> Permissions {
        self.state.lock().permissions.clone()
    }

    /// Whether this app may change permissions and promote peers.
    pub fn may_steer(&self) -> bool {
        let role = self.state.lock().role;
        role.is_host() || role.is_co_host()
    }

    /// Installs the waker, once, so the peer task can reach this front end.
    pub fn bind<V: 'static>(&self, view: &Entity<V>, cx: &mut App) {
        {
            let held = self.state.lock();
            if held.waker.is_some() {
                return;
            }
        }
        let (nudge, mut nudges) = futures::channel::mpsc::unbounded::<()>();
        self.state.lock().waker = Some(Arc::new(GpuiWaker { nudge }));

        let view = view.downgrade();
        cx.spawn(async move |cx: &mut AsyncApp| {
            use futures::StreamExt as _;
            while nudges.next().await.is_some() {
                // A view that has gone ends the loop: nothing is drawing the
                // session, so there is nothing to wake.
                if view.update(cx, |_view, cx| cx.notify()).is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    /// Starts hosting.
    ///
    /// Refused without a name: every other peer sees it, and an empty one
    /// makes a roster nobody can read.
    pub fn host(&mut self, toolkit_version: String, cx: &mut App) {
        if self.handle.is_some() || self.display_name.trim().is_empty() {
            return;
        }
        let params = HostParams {
            toolkit_version,
            display_name: self.display_name.trim().to_string(),
            initial_render_options: Default::default(),
            web_asset_bundle: Arc::clone(&self.bundle),
        };
        self.start(PeerMode::Host(params), true, cx);
    }

    /// Joins the session `token` names.
    ///
    /// The token is checked here rather than at the far end, so a mis-paste
    /// says so at once instead of timing out against nothing.
    pub fn join(&mut self, token: String, toolkit_version: String, cx: &mut App) {
        if self.handle.is_some() || self.display_name.trim().is_empty() {
            return;
        }
        let token = token.trim().to_string();
        if !token.starts_with(TOKEN_PREFIX) {
            self.failure = Some(t!("ui.collab.invalid_token", error = TOKEN_PREFIX).into_owned());
            return;
        }
        let params = JoinParams { token, display_name: self.display_name.trim().to_string(), toolkit_version };
        self.start(PeerMode::Join(params), false, cx);
    }

    fn start(&mut self, mode: PeerMode, hosting: bool, cx: &mut App) {
        let Some(runtime) = runtime::runtime(cx) else {
            self.failure = Some(t!("ui.collab.no_runtime").into_owned());
            return;
        };
        self.failure = None;
        self.hosting = hosting;
        // What the last session was left holding is not this one's: its own list
        // arrives with the handshake, and until then there is nothing drawn in
        // it. Kept past `leave` so a viewport could carry it onto its own end.
        {
            let mut held = self.state.lock();
            held.current_annotation_sync = None;
            held.tactics_boards.clear();
        }
        self.handle = Some(wt_collab_client::peer::start_peer_session(runtime, mode, Arc::clone(&self.state)));
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
        self.token_revealed = false;
        let mut held = self.state.lock();
        // Held back from the clearing: a session ending does not rub out what
        // was drawn in it, and each viewport reads it once more as it is handed
        // an inert link. `start` clears it when a new session begins.
        let drawn = held.current_annotation_sync.take();
        let boards = std::mem::take(&mut held.tactics_boards);
        held.clear_session_data();
        held.current_annotation_sync = drawn;
        held.tactics_boards = boards;
    }

    /// Locks or unlocks what peers may change.
    pub fn set_permissions(&self, permissions: Permissions) {
        if let Some(handle) = &self.handle {
            let _ = handle.command_tx.send(SessionCommand::SetPermissions(permissions));
        }
    }

    /// Returns every peer's display settings to the host's.
    pub fn reset_overrides(&self) {
        if let Some(handle) = &self.handle {
            let _ = handle.command_tx.send(SessionCommand::ResetClientOverrides);
        }
    }

    /// Raises a peer to co-host.
    pub fn promote(&self, user_id: u64) {
        if let Some(handle) = &self.handle {
            let _ = handle.command_tx.send(SessionCommand::PromoteToCoHost { user_id });
        }
    }

    /// Takes what the peer task has raised since the last look and acts on
    /// it.
    ///
    /// Called from the header's own draw, as the egui app polls its inbox
    /// every frame. It has to be called: the inbox is unbounded, so an
    /// undrained session's events would accumulate for as long as it runs.
    ///
    /// Returns what the reader should be told, which the caller says once it has
    /// a window: in a running session nothing else reports who joined, who left,
    /// or that the host has opened something.
    pub fn poll(&mut self) -> Vec<SessionNotice> {
        let Some(handle) = &self.handle else { return Vec::new() };
        let events = handle.event_inbox.drain();
        let mut notices = Vec::new();
        for event in events {
            match event {
                // Hosting announces a session; joining announces a connection,
                // which is the split the egui app makes between its two pollers.
                SessionEvent::Started => notices.push(if self.hosting {
                    SessionNotice::info(t!("ui.messages.session_started").into_owned())
                } else {
                    SessionNotice::info(t!("ui.messages.connected_to_session").into_owned())
                }),
                SessionEvent::UserJoined(user) => {
                    notices.push(SessionNotice::info(t!("ui.messages.user_joined", name = user.name).into_owned()));
                }
                SessionEvent::UserLeft { name, timed_out, .. } => notices.push(if timed_out {
                    SessionNotice::warn(t!("ui.messages.user_timeout", name = name).into_owned())
                } else {
                    SessionNotice::info(t!("ui.messages.user_left", name = name).into_owned())
                }),
                // The roster and the token are read from the shared state, so
                // these need nothing beyond the redraw.
                SessionEvent::PeerPromoted { .. }
                | SessionEvent::FrameSourceChanged { .. }
                | SessionEvent::SessionInfoReceived { .. } => {}
                // A replay opened on the host needs a viewport this port does
                // not build yet, so the reader is told rather than left to
                // wonder why nothing appeared.
                SessionEvent::ReplayOpened { replay_name, .. } => {
                    notices.push(SessionNotice::info(
                        t!("ui.messages.host_opened_replay", name = replay_name).into_owned(),
                    ));
                }
                SessionEvent::ReplayClosed { .. } => {
                    notices.push(SessionNotice::info(t!("ui.messages.host_closed_replay").into_owned()));
                }
                SessionEvent::Ended => {
                    notices.push(SessionNotice::info(t!("ui.messages.session_ended").into_owned()));
                    self.leave();
                }
                SessionEvent::Error(reason) => {
                    notices.push(SessionNotice::failed(t!("ui.messages.session_error", msg = reason).into_owned()));
                    self.failure = Some(reason);
                    self.leave();
                }
                SessionEvent::Rejected(reason) => {
                    notices
                        .push(SessionNotice::failed(t!("ui.messages.session_rejected", reason = reason).into_owned()));
                    self.failure = Some(reason);
                    self.leave();
                }
            }
        }
        notices
    }
}

/// Something that happened in the session, for the reader.
///
/// Carried out of `poll` rather than shown there: the poll runs inside the
/// header's draw, and a message queued during a draw is one the frame being
/// drawn cannot show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionNotice {
    said: String,
    level: NoticeLevel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NoticeLevel {
    /// Something happened that nobody asked for.
    Info,
    /// Something is wrong but the session goes on.
    Warn,
    /// The session is over.
    Failed,
}

impl SessionNotice {
    fn info(said: String) -> Self {
        Self { said, level: NoticeLevel::Info }
    }

    fn warn(said: String) -> Self {
        Self { said, level: NoticeLevel::Warn }
    }

    fn failed(said: String) -> Self {
        Self { said, level: NoticeLevel::Failed }
    }

    /// Puts it on screen at its own level.
    pub fn say(self, window: &mut gpui_kit::Window, cx: &mut gpui_kit::App) {
        match self.level {
            NoticeLevel::Info => crate::toast::info(self.said, window, cx),
            NoticeLevel::Warn => crate::toast::warn(self.said, window, cx),
            NoticeLevel::Failed => crate::toast::failed(self.said, window, cx),
        }
    }
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
pub struct ConnectedPeer {
    pub user_id: u64,
    pub name: String,
    pub role: PeerRole,
    /// Whether this is the reader, which the roster marks.
    pub is_me: bool,
}

use rust_i18n::t;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_that_has_not_started_is_not_active() {
        let collab = CollabState::default();

        assert!(!collab.is_active());
        assert!(!collab.is_hosting());
        assert!(collab.token().is_none());
        assert!(collab.connected().is_empty());
    }

    #[test]
    fn a_token_is_masked_until_it_is_revealed() {
        let mut collab = CollabState::default();
        collab.state.lock().token = Some("toolkit-abcdef".to_string());

        let masked = collab.token_display().expect("there is a token");
        assert!(!masked.contains("abcdef"), "a token on a shared screen is not readable by default");

        collab.token_revealed = true;
        assert_eq!(collab.token_display().as_deref(), Some("toolkit-abcdef"));
    }

    #[test]
    fn a_web_link_carries_the_token_in_its_fragment() {
        let collab = CollabState::default();
        collab.state.lock().token = Some("toolkit-abcdef".to_string());

        let link = collab.web_link().expect("there is a token");

        assert!(link.starts_with(WEB_CLIENT_URL));
        assert!(link.ends_with("#toolkit-abcdef"));
    }

    #[test]
    fn there_is_no_link_without_a_token() {
        assert!(CollabState::default().web_link().is_none());
    }

    #[test]
    fn the_roster_marks_which_peer_is_the_reader() {
        let collab = CollabState::default();
        {
            let mut held = collab.state.lock();
            held.my_user_id = 7;
            held.connected_users = vec![
                wt_collab_client::ConnectedUser {
                    id: 7,
                    name: "me".into(),
                    color: [0, 0, 0],
                    role: PeerRole::Host,
                    client_type: wt_collab_client::protocol::ClientType::Desktop { toolkit_version: "test".into() },
                },
                wt_collab_client::ConnectedUser {
                    id: 9,
                    name: "them".into(),
                    color: [0, 0, 0],
                    role: PeerRole::Peer,
                    client_type: wt_collab_client::protocol::ClientType::Desktop { toolkit_version: "test".into() },
                },
            ];
        }

        let roster = collab.connected();

        assert_eq!(roster.len(), 2);
        assert!(roster[0].is_me);
        assert!(!roster[1].is_me);
    }

    #[test]
    fn leaving_clears_what_the_session_held() {
        let mut collab = CollabState::default();
        {
            let mut held = collab.state.lock();
            held.token = Some("toolkit-abcdef".into());
            held.status = SessionStatus::Active;
        }
        collab.token_revealed = true;

        collab.leave();

        // Carrying a token across sessions would offer entry to one that has
        // ended.
        assert!(collab.token().is_none());
        assert!(!collab.token_revealed);
        assert_eq!(collab.status(), SessionStatus::Idle);
    }

    /// Leaving does not rub out what was drawn in the session: each viewport is
    /// handed an inert link and reads the shapes onto its own end as it takes
    /// one, which it cannot do if they have already been cleared.
    #[test]
    fn leaving_leaves_what_was_drawn_where_a_viewport_can_carry_it() {
        let mut collab = CollabState::default();
        {
            let mut held = collab.state.lock();
            held.status = SessionStatus::Active;
            held.current_annotation_sync = Some(wt_collab_client::AnnotationSyncState {
                annotations: vec![wt_collab_client::types::Annotation::Circle {
                    center: [1.0, 2.0],
                    radius: 3.0,
                    color: [255, 0, 0, 255],
                    width: 2.0,
                    filled: false,
                }],
                owners: vec![1],
                ids: vec![77],
            });
            held.tactics_boards.insert(42, wt_collab_client::TacticsBoardSessionState::default());
        }

        collab.leave();

        let held = collab.state.lock();
        assert!(held.current_annotation_sync.is_some(), "still readable for the viewport that draws it");
        assert!(held.tactics_boards.contains_key(&42), "and so is the board's own");
    }
}

/// A viewport's end of a collab session.
///
/// Cloneable and inert without a session, so a viewport holds one whether or
/// not anyone is connected and does not have to be told when that changes.
#[derive(Clone, Default)]
pub struct CollabLink {
    state: Option<Arc<Mutex<SessionState>>>,
    local_tx: Option<std::sync::mpsc::Sender<LocalEvent>>,
    /// Which tactics board this link speaks for. `None` is the replay context,
    /// which is the one window a session has without anyone opening one.
    board: Option<u64>,
    /// What this end holds while there is no session to hold it.
    ///
    /// Drawing on a map is not a collab feature: a reader alone draws on their
    /// own replay and nobody else sees it. Without this a shape would be sent to
    /// a session that is not there and never drawn.
    alone: Arc<Mutex<wt_collab_client::AnnotationSyncState>>,
    /// Ids for the shapes added while alone. They only have to tell this end's
    /// own shapes apart; a session assigns its own.
    next_alone_id: Arc<std::sync::atomic::AtomicU64>,
    /// Where a frame of playback goes for the rest of the session to watch.
    /// `None` for a peer, which is sent frames rather than sending them.
    frames: Option<std::sync::mpsc::SyncSender<wt_collab_client::peer::FrameBroadcast>>,
    /// Where a message about the session itself goes: which replays are open,
    /// and which end is the one being watched.
    commands: Option<std::sync::mpsc::Sender<wt_collab_client::SessionCommand>>,
}

impl CollabLink {
    /// Whether there is a session to talk to.
    pub fn is_active(&self) -> bool {
        self.local_tx.is_some()
    }

    /// Whether this end may steer the shared session, or is running alone.
    pub fn may_steer(&self) -> bool {
        self.state.as_ref().is_none_or(|state| {
            let role = state.lock().role;
            role.is_host() || role.is_co_host()
        })
    }

    /// Whether this end is prevented from changing annotations in the session.
    pub fn annotations_locked(&self) -> bool {
        self.state.as_ref().is_some_and(|state| state.lock().permissions.annotations_locked)
    }

    /// The same link, speaking for one tactics board.
    ///
    /// A board's shapes and capture points are its own: two boards open on
    /// different maps in one session do not share them, and neither shares the
    /// replay's. Its own store too, for the same reason.
    pub fn on_board(&self, board_id: BoardId) -> Self {
        Self {
            state: self.state.clone(),
            local_tx: self.local_tx.clone(),
            board: Some(board_id.raw()),
            alone: Arc::new(Mutex::new(wt_collab_client::AnnotationSyncState::default())),
            next_alone_id: Arc::new(std::sync::atomic::AtomicU64::new(1)),
            // A board draws no battle, so it sends no frames.
            frames: None,
            commands: self.commands.clone(),
        }
    }

    /// Puts `held` on this link, whichever end is holding.
    ///
    /// For a session starting or ending under an open viewport: what was drawn
    /// before stays on the map, and a session hears about it as though it had
    /// just been drawn, which is how the egui host pushes what it already had.
    pub fn adopt(&self, held: Vec<wt_collab_client::drawing::Held>) {
        if let Some(tx) = &self.local_tx {
            // Under the ids they already carry, so a step remembered for an undo
            // still names these shapes afterwards, and in order, so the session
            // holds them the way the reader drew them.
            for one in held {
                let event = wt_collab_client::peer::LocalAnnotationEvent::Set {
                    board_id: self.board,
                    id: one.id,
                    annotation: one.annotation,
                    owner: one.owner,
                };
                self.apply_local_annotation(&event);
                let _ = tx.send(LocalEvent::Annotation(event));
            }
            return;
        }
        let mut alone = self.alone.lock();
        *alone = wt_collab_client::AnnotationSyncState {
            annotations: held.iter().map(|one| one.annotation.clone()).collect(),
            owners: held.iter().map(|one| one.owner).collect(),
            ids: held.iter().map(|one| one.id).collect(),
        };
        let highest = alone.ids.iter().copied().max().unwrap_or_default();
        self.next_alone_id.store(highest.saturating_add(1), std::sync::atomic::Ordering::Relaxed);
    }

    /// Who this app is in the session, where it is in one.
    ///
    /// `None` outside a session: a user id is assigned by the server, and a
    /// reader alone has not been given one.
    pub fn my_user_id(&self) -> Option<UserId> {
        let state = self.state.as_ref()?;
        self.is_active().then(|| UserId(state.lock().my_user_id))
    }

    /// The shapes this link speaks for, as the session holds them.
    fn sync_of<'a>(&self, held: &'a SessionState) -> Option<&'a wt_collab_client::AnnotationSyncState> {
        match self.board {
            Some(board_id) => held.tactics_boards.get(&board_id).map(|board| &board.annotation_sync),
            None => held.current_annotation_sync.as_ref(),
        }
    }

    /// The shapes this link speaks for, wherever they are held.
    fn held(&self) -> wt_collab_client::AnnotationSyncState {
        if !self.is_active() {
            return self.alone.lock().clone();
        }
        let Some(state) = &self.state else { return wt_collab_client::AnnotationSyncState::default() };
        let held = state.lock();
        self.sync_of(&held).cloned().unwrap_or_default()
    }

    /// Where every other peer's pointer is, in minimap space.
    ///
    /// Our own is left out: the reader can already see their own pointer.
    pub fn peer_cursors(&self) -> Vec<UserCursor> {
        let Some(state) = &self.state else { return Vec::new() };
        let state = state.lock();
        let mine = state.my_user_id;
        state.cursors.iter().filter(|cursor| cursor.user_id != mine && cursor.pos.is_some()).cloned().collect()
    }

    /// The pings peers have dropped on the map recently.
    pub fn peer_pings(&self) -> Vec<PeerPing> {
        let Some(state) = &self.state else { return Vec::new() };
        state.lock().pings.clone()
    }

    /// Says where this app's pointer is, or that it has left the map.
    pub fn report_cursor(&self, pos: Option<[f32; 2]>) {
        if let Some(tx) = &self.local_tx {
            let _ = tx.send(LocalEvent::CursorPosition(pos));
        }
    }

    /// Drops a ping on the map for everyone in the session.
    pub fn send_ping(&self, pos: [f32; 2]) {
        if let Some(tx) = &self.local_tx {
            let _ = tx.send(LocalEvent::Ping(pos));
        }
    }

    /// What everyone in the session has drawn on the map.
    pub fn annotations(&self) -> Vec<wt_collab_client::types::Annotation> {
        self.held().annotations
    }

    /// How many shapes there are, without copying them to count them.
    pub fn annotation_count(&self) -> usize {
        if !self.is_active() {
            return self.alone.lock().annotations.len();
        }
        let Some(state) = &self.state else { return 0 };
        let held = state.lock();
        self.sync_of(&held).map(|sync| sync.annotations.len()).unwrap_or_default()
    }

    /// The shape at `index`, without copying the rest.
    pub fn annotation_at(&self, index: usize) -> Option<wt_collab_client::types::Annotation> {
        if !self.is_active() {
            return self.alone.lock().annotations.get(index).cloned();
        }
        let state = self.state.as_ref()?;
        let held = state.lock();
        self.sync_of(&held)?.annotations.get(index).cloned()
    }

    /// Puts `annotation` on the map for everyone in the session.
    pub fn add_annotation(&self, annotation: wt_collab_client::types::Annotation) {
        let owner = self.my_user_id().map(UserId::raw).unwrap_or_default();
        let Some(tx) = &self.local_tx else {
            let id = self.next_alone_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let mut alone = self.alone.lock();
            alone.annotations.push(annotation);
            alone.owners.push(owner);
            alone.ids.push(id);
            return;
        };
        let mut event = wt_collab_client::peer::LocalAnnotationEvent::new_annotation(annotation, owner);
        if let wt_collab_client::peer::LocalAnnotationEvent::Set { board_id, .. } = &mut event {
            *board_id = self.board;
        }
        self.apply_local_annotation(&event);
        let _ = tx.send(LocalEvent::Annotation(event));
    }

    /// Everything the session holds, with the ids and owners it keys them
    /// by. What a snapshot for undo is taken of.
    pub fn annotations_held(&self) -> Vec<wt_collab_client::drawing::Held> {
        let sync = self.held();
        sync.annotations
            .iter()
            .enumerate()
            .map(|(at, annotation)| wt_collab_client::drawing::Held {
                id: sync.ids.get(at).copied().unwrap_or_default(),
                owner: sync.owners.get(at).copied().unwrap_or_default(),
                annotation: annotation.clone(),
            })
            .collect()
    }

    /// Puts the session back the way `was` had it.
    pub fn restore(&self, was: &[wt_collab_client::drawing::Held]) {
        use wt_collab_client::drawing::Change;
        use wt_collab_client::peer::LocalAnnotationEvent;

        let Some(tx) = &self.local_tx else {
            self.adopt(was.to_vec());
            return;
        };
        let Some(state) = &self.state else { return };
        // The board reads this snapshot while the worker drains the ordered
        // events, so publish the target before sending its deltas.
        let changes = {
            let mut state = state.lock();
            let changes = {
                let sync = match self.board {
                    Some(board_id) => state.tactics_boards.get_mut(&board_id).map(|board| &mut board.annotation_sync),
                    None => Some(state.current_annotation_sync.get_or_insert_with(Default::default)),
                };
                let Some(sync) = sync else { return };
                let current = sync
                    .annotations
                    .iter()
                    .enumerate()
                    .map(|(at, annotation)| wt_collab_client::drawing::Held {
                        id: sync.ids.get(at).copied().unwrap_or_default(),
                        owner: sync.owners.get(at).copied().unwrap_or_default(),
                        annotation: annotation.clone(),
                    })
                    .collect::<Vec<_>>();
                let changes = wt_collab_client::drawing::undo_plan(was, &current);
                if !changes.is_empty() {
                    *sync = wt_collab_client::AnnotationSyncState {
                        annotations: was.iter().map(|held| held.annotation.clone()).collect(),
                        owners: was.iter().map(|held| held.owner).collect(),
                        ids: was.iter().map(|held| held.id).collect(),
                    };
                }
                changes
            };
            if !changes.is_empty() {
                match self.board {
                    Some(board_id) => {
                        if let Some(board) = state.tactics_boards.get_mut(&board_id) {
                            board.annotation_sync_version += 1;
                            state.tactics_boards_version += 1;
                        }
                    }
                    None => state.annotation_sync_version += 1,
                }
            }
            changes
        };
        for change in changes {
            let event = match change {
                Change::Set(held) => LocalAnnotationEvent::Set {
                    board_id: self.board,
                    id: held.id,
                    annotation: held.annotation,
                    owner: held.owner,
                },
                Change::Remove(id) => LocalAnnotationEvent::Remove { board_id: self.board, id },
            };
            let _ = tx.send(LocalEvent::Annotation(event));
        }
    }

    /// Replaces every annotation with the supplied set.
    pub fn replace_annotations(&self, annotations: Vec<wt_collab_client::types::Annotation>) {
        let owner = self.my_user_id().map(UserId::raw).unwrap_or_default();
        let held = annotations
            .into_iter()
            .map(|annotation| wt_collab_client::drawing::Held {
                id: if self.is_active() {
                    wt_collab_client::peer::fresh_id()
                } else {
                    self.next_alone_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                },
                owner,
                annotation,
            })
            .collect::<Vec<_>>();
        self.restore(&held);
    }

    fn apply_local_annotation(&self, event: &wt_collab_client::peer::LocalAnnotationEvent) {
        use wt_collab_client::peer::LocalAnnotationEvent;

        let Some(state) = &self.state else { return };
        // Apply before enqueueing so a following edit observes this one.
        let mut state = state.lock();
        let event_board = match event {
            LocalAnnotationEvent::Set { board_id, .. }
            | LocalAnnotationEvent::Remove { board_id, .. }
            | LocalAnnotationEvent::Clear { board_id } => *board_id,
        };
        if event_board != self.board {
            return;
        }
        let changed = {
            let sync = match self.board {
                Some(board_id) => state.tactics_boards.get_mut(&board_id).map(|board| &mut board.annotation_sync),
                None => Some(state.current_annotation_sync.get_or_insert_with(Default::default)),
            };
            let Some(sync) = sync else { return };
            match event {
                LocalAnnotationEvent::Set { id, annotation, owner, .. } => {
                    if let Some(at) = sync.ids.iter().position(|held| held == id) {
                        if sync.annotations[at] == *annotation && sync.owners[at] == *owner {
                            false
                        } else {
                            sync.annotations[at] = annotation.clone();
                            sync.owners[at] = *owner;
                            true
                        }
                    } else {
                        sync.annotations.push(annotation.clone());
                        sync.owners.push(*owner);
                        sync.ids.push(*id);
                        true
                    }
                }
                LocalAnnotationEvent::Remove { id, .. } => {
                    if let Some(at) = sync.ids.iter().position(|held| held == id) {
                        sync.annotations.remove(at);
                        sync.owners.remove(at);
                        sync.ids.remove(at);
                        true
                    } else {
                        false
                    }
                }
                LocalAnnotationEvent::Clear { .. } => {
                    let changed = !sync.ids.is_empty();
                    sync.annotations.clear();
                    sync.owners.clear();
                    sync.ids.clear();
                    changed
                }
            }
        };
        if changed {
            match self.board {
                Some(board_id) => {
                    if let Some(board) = state.tactics_boards.get_mut(&board_id) {
                        board.annotation_sync_version += 1;
                        state.tactics_boards_version += 1;
                    }
                }
                None => state.annotation_sync_version += 1,
            }
        }
    }

    /// Replaces the annotation at `index` of [`Self::annotations`], keeping
    /// the id the session knows it by.
    ///
    /// For a shape the reader has moved or turned: sending a new id would
    /// leave the old one on everyone else's map beside the new one.
    pub fn update_annotation(&self, index: usize, annotation: wt_collab_client::types::Annotation) {
        let Some(tx) = &self.local_tx else {
            let mut alone = self.alone.lock();
            if let Some(held) = alone.annotations.get_mut(index) {
                *held = annotation;
            }
            return;
        };
        let sync = self.held();
        let Some(id) = sync.ids.get(index).copied() else { return };
        let owner = sync.owners.get(index).copied().unwrap_or_default();
        let event = wt_collab_client::peer::LocalAnnotationEvent::Set { board_id: self.board, id, annotation, owner };
        self.apply_local_annotation(&event);
        let _ = tx.send(LocalEvent::Annotation(event));
    }

    /// Takes the annotation at `index` of [`Self::annotations`] off the map.
    ///
    /// By index because that is what a hit test answers, and by id on the
    /// wire because the session is keyed that way and another peer may have
    /// added one in between.
    pub fn erase_annotation(&self, index: usize) {
        let Some(tx) = &self.local_tx else {
            let mut alone = self.alone.lock();
            if index < alone.annotations.len() {
                alone.annotations.remove(index);
                alone.owners.remove(index);
                alone.ids.remove(index);
            }
            return;
        };
        let Some(id) = self.held().ids.get(index).copied() else { return };
        let event = wt_collab_client::peer::LocalAnnotationEvent::Remove { board_id: self.board, id };
        self.apply_local_annotation(&event);
        let _ = tx.send(LocalEvent::Annotation(event));
    }

    /// Puts this board's map in front of the session, so a peer can open the
    /// same board rather than being told one exists.
    ///
    /// The map art travels with it: a peer may not have the build the map came
    /// from, and a board nobody can draw is not a shared board.
    /// The session drops anything sent for a board it does not hold, so this has
    /// to reach the peer task before the board's zones and shapes do.
    pub fn announce_board(&self, map: BoardMap) {
        let (Some(tx), Some(board_id)) = (&self.local_tx, self.board) else { return };
        let owner_user_id = self.my_user_id().map(UserId::raw).unwrap_or_default();
        let map_image_png = map.art_png.unwrap_or_default();
        let map_info = map.info;
        if let Some(state) = &self.state {
            let mut state = state.lock();
            let owner = if owner_user_id == 0 { state.my_user_id } else { owner_user_id };
            let board = state.tactics_boards.entry(board_id).or_default();
            board.owner_user_id = owner;
            board.tactics_map = wt_collab_client::TacticsMapInfo {
                map_name: map.space.clone(),
                display_name: map.label.clone(),
                map_id: map.map_id,
                map_image_png: map_image_png.clone(),
                map_info: map_info.clone(),
            };
            state.tactics_boards_version += 1;
        }
        let _ = tx.send(LocalEvent::TacticsMapOpened {
            board_id,
            owner_user_id,
            map_name: map.space,
            display_name: map.label,
            map_id: map.map_id,
            map_image_png,
            map_info,
        });
    }

    /// Takes this board out of the session, so a peer stops being offered a board
    /// nobody is looking at.
    pub fn close_board(&self) {
        let (Some(tx), Some(board_id)) = (&self.local_tx, self.board) else { return };
        let _ = tx.send(LocalEvent::TacticsMapClosed { board_id });
    }

    /// Puts a capture point on this board for everyone in the session.
    pub fn set_cap(&self, cap: wt_collab_client::protocol::WireCapPoint) {
        let (Some(tx), Some(board_id)) = (&self.local_tx, self.board) else { return };
        if let Some(state) = &self.state {
            // Board handoff can read this state before the worker drains the event.
            let mut state = state.lock();
            if let Some(board) = state.tactics_boards.get_mut(&board_id) {
                if let Some(existing) = board.cap_point_sync.cap_points.iter_mut().find(|held| held.id == cap.id) {
                    *existing = cap.clone();
                } else {
                    board.cap_point_sync.cap_points.push(cap.clone());
                }
                board.cap_point_sync_version += 1;
                state.tactics_boards_version += 1;
            }
        }
        let _ = tx.send(LocalEvent::CapPoint { board_id, event: wt_collab_client::peer::LocalCapPointEvent::Set(cap) });
    }

    /// Takes one off it.
    pub fn remove_cap(&self, id: CapPointId) {
        let (Some(tx), Some(board_id)) = (&self.local_tx, self.board) else { return };
        if let Some(state) = &self.state {
            let mut state = state.lock();
            if let Some(board) = state.tactics_boards.get_mut(&board_id) {
                let before = board.cap_point_sync.cap_points.len();
                board.cap_point_sync.cap_points.retain(|cap| cap.id != id.raw());
                if before != board.cap_point_sync.cap_points.len() {
                    board.cap_point_sync_version += 1;
                    state.tactics_boards_version += 1;
                }
            }
        }
        let _ = tx.send(LocalEvent::CapPoint {
            board_id,
            event: wt_collab_client::peer::LocalCapPointEvent::Remove { id: id.raw() },
        });
    }

    /// This board's capture points as the session holds them.
    pub fn board_caps(&self) -> Vec<wt_collab_client::protocol::WireCapPoint> {
        let (Some(state), Some(board_id)) = (&self.state, self.board) else { return Vec::new() };
        let held = state.lock();
        held.tactics_boards.get(&board_id).map(|board| board.cap_point_sync.cap_points.clone()).unwrap_or_default()
    }

    /// Which version of this board's capture points and shapes the session is
    /// on, so a board can tell that nothing has changed without comparing lists.
    pub fn board_versions(&self) -> Option<BoardVersions> {
        let (state, board_id) = (self.state.as_ref()?, self.board?);
        let held = state.lock();
        let board = held.tactics_boards.get(&board_id)?;
        Some(BoardVersions { caps: board.cap_point_sync_version, shapes: board.annotation_sync_version })
    }

    /// The boards open in the session, with who opened each. What a peer reads
    /// to open the same boards the host has.
    pub fn session_boards(&self) -> Vec<SessionBoard> {
        let Some(state) = &self.state else { return Vec::new() };
        let held = state.lock();
        held.tactics_boards
            .iter()
            .map(|(board_id, board)| SessionBoard {
                board_id: BoardId::new(*board_id),
                owner_user_id: UserId::new(board.owner_user_id),
                map: BoardMap {
                    space: board.tactics_map.map_name.clone(),
                    label: board.tactics_map.display_name.clone(),
                    map_id: board.tactics_map.map_id,
                    art_png: (!board.tactics_map.map_image_png.is_empty())
                        .then(|| board.tactics_map.map_image_png.clone()),
                    info: board.tactics_map.map_info.clone(),
                },
            })
            .collect()
    }

    /// Whether this end is the one the rest of the session watches.
    pub fn broadcasts_frames(&self) -> bool {
        self.frames.is_some()
    }

    /// Says a replay is open here, so the session lists it and a peer can ask to
    /// watch it.
    ///
    /// The map art travels with it, as a board's does: a peer may not have the
    /// build the replay was recorded on, and it still has to draw the battle.
    pub fn announce_replay(&self, replay: SharedReplay) {
        let Some(commands) = &self.commands else { return };
        let _ = commands.send(wt_collab_client::SessionCommand::ReplayOpened {
            replay_id: replay.replay_id.raw(),
            replay_name: replay.replay_name,
            map_image_png: replay.art_png.unwrap_or_default(),
            game_version: replay.game_version,
            map_name: replay.map_name,
            display_name: replay.map_label,
        });
        // Said straight after: a session with nobody claiming to be the one
        // being watched shows a peer an empty window.
        if self.broadcasts_frames() {
            let _ = commands.send(wt_collab_client::SessionCommand::BecomeFrameSource);
        }
    }

    /// Says a replay here has been closed, so the session stops listing it.
    pub fn close_replay(&self, replay_id: ReplayId) {
        let Some(commands) = &self.commands else { return };
        let _ = commands.send(wt_collab_client::SessionCommand::ReplayClosed { replay_id: replay_id.raw() });
    }

    /// Asks the session to send this end the frames of one of its windows.
    ///
    /// No waker is registered with the sink: the shared one is a plain callback,
    /// and reaching a gpui entity from the peer task would need a channel into
    /// the UI thread that the viewport is already draining. It looks for frames
    /// on its own timer instead.
    pub fn watch_window(
        &self,
        replay_id: ReplayId,
        frames: std::sync::mpsc::SyncSender<wt_collab_client::PlaybackFrame>,
    ) {
        let Some(state) = &self.state else { return };
        state.lock().register_viewport_sink(
            replay_id.raw(),
            wt_collab_client::ViewportSink { frame_tx: Some(frames), wake: None },
        );
    }

    /// Says this end has stopped drawing one of the session's windows.
    pub fn stop_watching(&self, replay_id: ReplayId) {
        let Some(state) = &self.state else { return };
        state.lock().viewport_sinks.remove(&replay_id.raw());
    }

    /// The windows the host has asked every peer to open, taken as they are read:
    /// an ask that has been answered is not one to answer again.
    pub fn take_forced_windows(&self) -> Vec<u64> {
        let Some(state) = &self.state else { return Vec::new() };
        let mut held = state.lock();
        held.force_open_window_ids.drain().collect()
    }

    /// Asks every peer to open one of the session's windows.
    pub fn open_for_everyone(&self, window_id: u64) {
        let Some(commands) = &self.commands else { return };
        let _ = commands.send(wt_collab_client::SessionCommand::OpenWindowForEveryone { window_id });
    }

    /// Puts one frame of playback in front of the session.
    ///
    /// Draw commands rather than pixels, so each peer draws the battle with its
    /// own art at its own size. Dropped rather than queued when the mesh is
    /// behind: a frame nobody has read yet is already stale, and a reader
    /// scrubbing would otherwise build a backlog the session plays out
    /// afterwards.
    pub fn broadcast_frame(&self, frame: wt_collab_client::peer::FrameBroadcast) {
        if let Some(frames) = &self.frames {
            let _ = frames.try_send(frame);
        }
    }

    /// Forgets the pings whose ripple has finished.
    ///
    /// The session collects them and nothing else takes them out, so a
    /// session left running carries every ping anyone has dropped in it. The
    /// egui renderer sheds them in the same place, as it draws.
    pub fn drop_stale_pings(&self, life: std::time::Duration) {
        let Some(state) = &self.state else { return };
        state.lock().pings.retain(|ping| ping.time.elapsed() < life);
    }

    /// A link onto `state` that reports as active with no peer task behind
    /// it. The receiver is what the viewport's own messages arrive on.
    #[cfg(test)]
    pub(crate) fn for_test(state: Arc<Mutex<SessionState>>) -> (Self, std::sync::mpsc::Receiver<LocalEvent>) {
        let (tx, rx) = std::sync::mpsc::channel();
        (
            Self {
                state: Some(state),
                local_tx: Some(tx),
                board: None,
                alone: Arc::new(Mutex::new(wt_collab_client::AnnotationSyncState::default())),
                next_alone_id: Arc::new(std::sync::atomic::AtomicU64::new(1)),
                frames: None,
                commands: None,
            },
            rx,
        )
    }
}

/// Who someone is in a session. Assigned by the server, so a reader alone has
/// none.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct UserId(u64);

impl UserId {
    pub fn new(raw: u64) -> Self {
        Self(raw)
    }

    pub fn raw(self) -> u64 {
        self.0
    }
}

/// What a tactics board is called in a session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BoardId(u64);

impl BoardId {
    pub fn new(raw: u64) -> Self {
        Self(raw)
    }

    pub fn raw(self) -> u64 {
        self.0
    }

    /// A name nothing else in the session holds.
    pub fn fresh() -> Self {
        Self(wt_collab_client::peer::fresh_id())
    }
}

/// What a replay window is called in a session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ReplayId(u64);

impl ReplayId {
    /// A name the session already holds, read back off the wire.
    pub fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    pub fn raw(self) -> u64 {
        self.0
    }

    /// A name nothing else in the session holds.
    pub fn fresh() -> Self {
        Self(wt_collab_client::peer::fresh_id())
    }
}

/// A replay this app has open, as the session lists it.
#[derive(Clone, Debug)]
pub struct SharedReplay {
    pub replay_id: ReplayId,
    /// What the replay is called, which is what the session's list reads.
    pub replay_name: String,
    /// The map's space name, and what it is called to a reader.
    pub map_name: String,
    pub map_label: String,
    /// The build it was recorded on, which some of a ship's ranges are read at.
    pub game_version: String,
    /// The drawn map as a PNG, for a peer whose build ships none. `None` where
    /// there was no art to send, which the wire states as an empty one.
    pub art_png: Option<Vec<u8>>,
}

/// What a capture point is called in a session. Its place in a list is not
/// that: every peer's list is its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CapPointId(u64);

impl CapPointId {
    pub fn new(raw: u64) -> Self {
        Self(raw)
    }

    pub fn raw(self) -> u64 {
        self.0
    }

    /// A name nothing else in the session holds.
    pub fn fresh() -> Self {
        Self(wt_collab_client::peer::fresh_id())
    }
}

/// How far the session has moved on for one board. Named rather than a pair of
/// counters: the two are the same type, and swapping them would compile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoardVersions {
    pub caps: u64,
    pub shapes: u64,
}

/// The map a tactics board is set on, as a session carries it.
#[derive(Clone, Debug, Default)]
pub struct BoardMap {
    /// The map's space name, which is what the renderer loads art by.
    pub space: String,
    /// What the map is called to a reader.
    pub label: String,
    pub map_id: u32,
    /// The drawn map as a PNG, for a peer whose build ships none for this map.
    /// `None` where there was no art to send, which the wire states as an empty
    /// one.
    pub art_png: Option<Vec<u8>>,
    /// What the map measures, for placing a world position on it.
    pub info: Option<wows_minimap_renderer::map_data::MapInfo>,
}

/// One board open in the session.
#[derive(Clone, Debug)]
pub struct SessionBoard {
    pub board_id: BoardId,
    pub owner_user_id: UserId,
    pub map: BoardMap,
}

#[cfg(test)]
mod annotation_tests {
    use super::*;
    use wt_collab_client::AnnotationSyncState;
    use wt_collab_client::types::Annotation;

    fn circle(radius: f32) -> Annotation {
        Annotation::Circle { center: [10.0, 10.0], radius, color: [255, 0, 0, 255], width: 2.0, filled: false }
    }

    /// A drawn shape reaches the session under an id of its own, so two peers
    /// drawing at once do not overwrite each other.
    #[test]
    fn a_drawn_shape_goes_to_the_session_under_its_own_id() {
        let state = Arc::new(Mutex::new(SessionState::default()));
        let (link, events) = CollabLink::for_test(Arc::clone(&state));

        link.add_annotation(circle(5.0));
        link.add_annotation(circle(6.0));

        let ids: Vec<u64> = events
            .try_iter()
            .map(|event| match event {
                LocalEvent::Annotation(wt_collab_client::peer::LocalAnnotationEvent::Set { id, .. }) => id,
                _ => panic!("drawing a shape sends an annotation"),
            })
            .collect();
        assert_eq!(ids.len(), 2);
        assert_ne!(ids[0], ids[1], "each gets its own id");
    }

    /// Rubbing one out names the id the session knows it by, not where it
    /// happened to sit in the list.
    #[test]
    fn rubbing_one_out_names_the_id_the_session_knows() {
        let state = Arc::new(Mutex::new(SessionState::default()));
        state.lock().current_annotation_sync = Some(AnnotationSyncState {
            annotations: vec![circle(5.0), circle(6.0)],
            ids: vec![77, 88],
            owners: vec![1, 2],
        });
        let (link, events) = CollabLink::for_test(Arc::clone(&state));

        link.erase_annotation(1);
        let sent: Vec<u64> = events
            .try_iter()
            .map(|event| match event {
                LocalEvent::Annotation(wt_collab_client::peer::LocalAnnotationEvent::Remove { id, .. }) => id,
                _ => panic!("rubbing one out sends a removal"),
            })
            .collect();
        assert_eq!(sent, vec![88]);
    }

    /// Alone, what is drawn is kept on this end: a reader with nobody connected
    /// draws on their own map and nobody else sees it.
    #[test]
    fn drawing_alone_keeps_the_shape_on_this_end() {
        let link = CollabLink::default();
        assert!(link.annotations().is_empty());

        link.add_annotation(circle(5.0));
        link.add_annotation(circle(6.0));
        assert_eq!(link.annotations().len(), 2);

        link.erase_annotation(0);
        assert_eq!(link.annotations(), vec![circle(6.0)], "and rubbing one out takes that one");

        link.update_annotation(0, circle(9.0));
        assert_eq!(link.annotations(), vec![circle(9.0)]);
    }

    /// Each shape drawn alone gets an id of its own, so an undo can name one.
    #[test]
    fn a_shape_drawn_alone_gets_its_own_id() {
        let link = CollabLink::default();
        link.add_annotation(circle(5.0));
        link.add_annotation(circle(6.0));
        let ids: Vec<u64> = link.annotations_held().iter().map(|held| held.id).collect();
        assert_eq!(ids.len(), 2);
        assert_ne!(ids[0], ids[1]);
    }

    /// A session starting under an open viewport is told what was already drawn,
    /// rather than the map going blank.
    #[test]
    fn what_was_drawn_alone_reaches_a_session_that_starts_after_it() {
        let alone = CollabLink::default();
        alone.add_annotation(circle(5.0));

        let state = Arc::new(Mutex::new(SessionState::default()));
        let (joined, events) = CollabLink::for_test(Arc::clone(&state));
        joined.adopt(alone.annotations_held());

        let sent: Vec<wt_collab_client::types::Annotation> = events
            .try_iter()
            .filter_map(|event| match event {
                LocalEvent::Annotation(wt_collab_client::peer::LocalAnnotationEvent::Set { annotation, .. }) => {
                    Some(annotation)
                }
                _ => None,
            })
            .collect();
        assert_eq!(sent, vec![circle(5.0)]);
    }

    /// And a session ending leaves what it held where the reader can see it.
    #[test]
    fn what_a_session_held_stays_when_it_ends() {
        let state = Arc::new(Mutex::new(SessionState::default()));
        state.lock().current_annotation_sync = Some(AnnotationSyncState {
            annotations: vec![circle(5.0), circle(6.0)],
            ids: vec![77, 88],
            owners: vec![1, 2],
        });
        let (joined, _events) = CollabLink::for_test(Arc::clone(&state));

        let ended = CollabLink::default();
        ended.adopt(joined.annotations_held());
        assert_eq!(ended.annotations(), vec![circle(5.0), circle(6.0)]);
        // The next shape drawn does not take an id one of these already has.
        ended.add_annotation(circle(7.0));
        let ids: Vec<u64> = ended.annotations_held().iter().map(|held| held.id).collect();
        assert_eq!(ids.len(), 3);
        assert!(!ids[..2].contains(&ids[2]), "{ids:?}");
    }

    /// A board's shapes are its own, not the replay's.
    #[test]
    fn a_board_holds_its_own_shapes() {
        let replay = CollabLink::default();
        let board = replay.on_board(BoardId::new(12));
        replay.add_annotation(circle(5.0));
        board.add_annotation(circle(6.0));

        assert_eq!(replay.annotations(), vec![circle(5.0)]);
        assert_eq!(board.annotations(), vec![circle(6.0)]);
    }

    /// An index past what the session holds sends nothing, rather than
    /// rubbing out whatever happens to be last.
    #[test]
    fn an_index_the_session_does_not_have_rubs_nothing_out() {
        let state = Arc::new(Mutex::new(SessionState::default()));
        state.lock().current_annotation_sync =
            Some(AnnotationSyncState { annotations: vec![circle(5.0)], ids: vec![77], owners: vec![1] });
        let (link, events) = CollabLink::for_test(Arc::clone(&state));

        link.erase_annotation(9);
        assert!(events.try_iter().next().is_none());
    }
}

/// Puts a ping dropped `age` ago into `state`.
///
/// Here rather than beside the test that wants it: a ping's clock is
/// `web_time`, which is this module's dependency rather than a viewport's.
#[cfg(test)]
pub(crate) fn push_ping_aged(state: &Arc<Mutex<SessionState>>, pos: [f32; 2], age: std::time::Duration) {
    state.lock().pings.push(PeerPing { user_id: 7, color: [255, 0, 0], pos, time: web_time::Instant::now() - age });
}

#[cfg(test)]
mod board_tests {
    use super::BoardId;
    use super::CapPointId;
    use super::CollabLink;
    use super::LocalEvent;
    use super::SessionState;
    use super::UserId;
    use parking_lot::Mutex;
    use std::sync::Arc;
    use wt_collab_client::TacticsBoardSessionState;
    use wt_collab_client::protocol::WireCapPoint;

    fn circle(radius: f32) -> wt_collab_client::types::Annotation {
        wt_collab_client::types::Annotation::Circle {
            center: [10.0, 10.0],
            radius,
            color: [255, 0, 0, 255],
            width: 2.0,
            filled: false,
        }
    }

    fn zone(id: u64, index: u32) -> WireCapPoint {
        WireCapPoint { id, index, world_x: 1.0, world_z: 2.0, radius: 150.0, team_id: -1, frozen: false }
    }

    /// A zone placed on a board reaches the session under that board's name, not
    /// the replay's.
    #[test]
    fn a_zone_reaches_the_session_under_its_board() {
        let state = Arc::new(Mutex::new(SessionState::default()));
        let (link, events) = CollabLink::for_test(Arc::clone(&state));
        let board = link.on_board(BoardId::new(42));

        board.set_cap(zone(7, 0));
        board.remove_cap(CapPointId::new(7));

        let sent: Vec<(u64, bool)> = events
            .try_iter()
            .filter_map(|event| match event {
                LocalEvent::CapPoint { board_id, event } => {
                    Some((board_id, matches!(event, wt_collab_client::peer::LocalCapPointEvent::Set(_))))
                }
                _ => None,
            })
            .collect();
        assert_eq!(sent, vec![(42, true), (42, false)]);
    }

    /// A board with no session sends nothing and asks nothing: a reader alone
    /// still has capture points, held by the board itself.
    #[test]
    fn a_board_alone_reads_no_zones_from_a_session() {
        let board = CollabLink::default().on_board(BoardId::new(42));
        board.set_cap(zone(7, 0));
        assert!(board.board_caps().is_empty());
        assert!(board.board_versions().is_none());
    }

    /// What the session holds for one board is that board's, and the versions
    /// say whether it has moved on.
    #[test]
    fn a_board_reads_its_own_zones_and_versions() {
        let state = Arc::new(Mutex::new(SessionState::default()));
        state.lock().tactics_boards.insert(
            42,
            TacticsBoardSessionState {
                cap_point_sync: wt_collab_client::CapPointSyncState { cap_points: vec![zone(7, 0), zone(8, 1)] },
                cap_point_sync_version: 3,
                annotation_sync_version: 5,
                ..Default::default()
            },
        );
        let (link, _events) = CollabLink::for_test(Arc::clone(&state));

        let board = link.on_board(BoardId::new(42));
        assert_eq!(board.board_caps().len(), 2);
        assert_eq!(board.board_versions(), Some(super::BoardVersions { caps: 3, shapes: 5 }));

        let other = link.on_board(BoardId::new(43));
        assert!(other.board_caps().is_empty(), "another board's zones are not this one's");
        assert!(other.board_versions().is_none());
    }

    /// What a peer opened is listed with the art it sent, so a board can be
    /// opened on it without the build it came from.
    #[test]
    fn the_boards_a_session_is_on_carry_their_art() {
        let state = Arc::new(Mutex::new(SessionState::default()));
        state.lock().tactics_boards.insert(
            42,
            TacticsBoardSessionState {
                owner_user_id: 9,
                tactics_map: wt_collab_client::TacticsMapInfo {
                    map_name: "spaces/13_OC_new_dawn".into(),
                    display_name: "New Dawn".into(),
                    map_id: 13,
                    map_image_png: vec![1, 2, 3],
                    map_info: None,
                },
                ..Default::default()
            },
        );
        let (link, _events) = CollabLink::for_test(Arc::clone(&state));

        let [board] = &link.session_boards()[..] else { panic!("one board is open") };
        assert_eq!(board.board_id, BoardId::new(42));
        assert_eq!(board.owner_user_id, UserId::new(9));
        assert_eq!(board.map.space, "spaces/13_OC_new_dawn");
        assert_eq!(board.map.label, "New Dawn");
        assert_eq!(board.map.art_png, Some(vec![1, 2, 3]));
    }

    /// A shape carried into a session keeps the id it already had, so a step
    /// remembered for an undo still names it afterwards.
    #[test]
    fn a_carried_shape_keeps_the_id_it_had() {
        use wt_collab_client::drawing::Held;

        let state = Arc::new(Mutex::new(SessionState::default()));
        let (link, events) = CollabLink::for_test(Arc::clone(&state));
        let held = vec![
            Held { id: 77, owner: 3, annotation: circle(5.0) },
            Held { id: 88, owner: 4, annotation: circle(6.0) },
        ];

        link.adopt(held);

        let sent: Vec<(u64, u64)> = events
            .try_iter()
            .filter_map(|event| match event {
                LocalEvent::Annotation(wt_collab_client::peer::LocalAnnotationEvent::Set { id, owner, .. }) => {
                    Some((id, owner))
                }
                _ => None,
            })
            .collect();
        assert_eq!(sent, vec![(77, 3), (88, 4)], "in the order they were drawn, under their own ids and owners");
    }

    /// Announcing says which map, under this board's name.
    #[test]
    fn announcing_says_which_map_the_board_is_on() {
        let state = Arc::new(Mutex::new(SessionState::default()));
        let (link, events) = CollabLink::for_test(Arc::clone(&state));
        let board = link.on_board(BoardId::new(42));

        board.announce_board(super::BoardMap {
            space: "spaces/13_OC_new_dawn".into(),
            label: "New Dawn".into(),
            map_id: 13,
            art_png: Some(vec![9]),
            info: None,
        });
        board.close_board();

        let said: Vec<String> = events
            .try_iter()
            .filter_map(|event| match event {
                LocalEvent::TacticsMapOpened { board_id, map_name, map_id, .. } => {
                    Some(format!("opened {board_id} {map_name} {map_id}"))
                }
                LocalEvent::TacticsMapClosed { board_id } => Some(format!("closed {board_id}")),
                _ => None,
            })
            .collect();
        assert_eq!(said, vec!["opened 42 spaces/13_OC_new_dawn 13", "closed 42"]);
    }
}
