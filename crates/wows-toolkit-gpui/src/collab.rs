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

    /// A link a browser can join from.
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
                map: match replay.display_name.as_str() {
                    "" => replay.map_name.clone(),
                    named => named.to_owned(),
                },
            })
            .collect()
    }

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
        self.state.lock().clear_session_data();
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

/// One participant, as the roster shows them.
/// One battle the session is on.
pub struct SharedWindow {
    pub replay_id: u64,
    pub replay_name: String,
    /// The map it is played on, translated where the host said so.
    pub map: String,
}

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
}

/// A viewport's end of a collab session.
///
/// Cloneable and inert without a session, so a viewport holds one whether or
/// not anyone is connected and does not have to be told when that changes.
#[derive(Clone, Default)]
pub struct CollabLink {
    state: Option<Arc<Mutex<SessionState>>>,
    local_tx: Option<std::sync::mpsc::Sender<LocalEvent>>,
}

impl CollabLink {
    /// Whether there is a session to talk to.
    pub fn is_active(&self) -> bool {
        self.local_tx.is_some()
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
        let Some(state) = &self.state else { return Vec::new() };
        state.lock().current_annotation_sync.as_ref().map(|sync| sync.annotations.clone()).unwrap_or_default()
    }

    /// Puts `annotation` on the map for everyone in the session.
    pub fn add_annotation(&self, annotation: wt_collab_client::types::Annotation) {
        let Some(tx) = &self.local_tx else { return };
        let owner = self.state.as_ref().map(|state| state.lock().my_user_id).unwrap_or_default();
        let _ = tx.send(LocalEvent::Annotation(wt_collab_client::peer::LocalAnnotationEvent::new_annotation(
            annotation, owner,
        )));
    }

    /// Everything the session holds, with the ids and owners it keys them
    /// by. What a snapshot for undo is taken of.
    pub fn annotations_held(&self) -> Vec<wt_collab_client::drawing::Held> {
        let Some(state) = &self.state else { return Vec::new() };
        let held = state.lock();
        let Some(sync) = held.current_annotation_sync.as_ref() else { return Vec::new() };
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

        let Some(tx) = &self.local_tx else { return };
        for change in wt_collab_client::drawing::undo_plan(was, &self.annotations_held()) {
            let event = match change {
                Change::Set(held) => LocalAnnotationEvent::Set {
                    board_id: None,
                    id: held.id,
                    annotation: held.annotation,
                    owner: held.owner,
                },
                Change::Remove(id) => LocalAnnotationEvent::Remove { board_id: None, id },
            };
            let _ = tx.send(LocalEvent::Annotation(event));
        }
    }

    /// Replaces the annotation at `index` of [`Self::annotations`], keeping
    /// the id the session knows it by.
    ///
    /// For a shape the reader has moved or turned: sending a new id would
    /// leave the old one on everyone else's map beside the new one.
    pub fn update_annotation(&self, index: usize, annotation: wt_collab_client::types::Annotation) {
        let Some(tx) = &self.local_tx else { return };
        let Some(state) = &self.state else { return };
        let (id, owner) = {
            let held = state.lock();
            let Some(sync) = held.current_annotation_sync.as_ref() else { return };
            let Some(id) = sync.ids.get(index).copied() else { return };
            (id, sync.owners.get(index).copied().unwrap_or_default())
        };
        let _ = tx.send(LocalEvent::Annotation(wt_collab_client::peer::LocalAnnotationEvent::Set {
            board_id: None,
            id,
            annotation,
            owner,
        }));
    }

    /// Takes the annotation at `index` of [`Self::annotations`] off the map.
    ///
    /// By index because that is what a hit test answers, and by id on the
    /// wire because the session is keyed that way and another peer may have
    /// added one in between.
    pub fn erase_annotation(&self, index: usize) {
        let Some(tx) = &self.local_tx else { return };
        let Some(state) = &self.state else { return };
        let id = {
            let held = state.lock();
            let Some(sync) = held.current_annotation_sync.as_ref() else { return };
            let Some(id) = sync.ids.get(index).copied() else { return };
            id
        };
        let _ = tx
            .send(LocalEvent::Annotation(wt_collab_client::peer::LocalAnnotationEvent::Remove { board_id: None, id }));
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
        (Self { state: Some(state), local_tx: Some(tx) }, rx)
    }
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
            ..Default::default()
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

    /// An index past what the session holds sends nothing, rather than
    /// rubbing out whatever happens to be last.
    #[test]
    fn an_index_the_session_does_not_have_rubs_nothing_out() {
        let state = Arc::new(Mutex::new(SessionState::default()));
        state.lock().current_annotation_sync =
            Some(AnnotationSyncState { annotations: vec![circle(5.0)], ids: vec![77], ..Default::default() });
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
