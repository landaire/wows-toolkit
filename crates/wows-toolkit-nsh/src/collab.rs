//! Collaborative sessions: the app's one session controller, the `Collab`
//! module the session popover drives, and the Rust API viewports use to share
//! cursors, annotations, boards and playback.
//!
//! The session is `wt_collab_client::controller::SessionController`, shared
//! with the GPUI and egui apps. This module owns the app's single instance,
//! wakes readers when the peer task changes something, and turns what the
//! session raises into notices.
//!
//! # Rust API for viewports
//!
//! A replay renderer or a tactics board holds a [`CollabLink`] and talks to
//! the session only through it:
//!
//! - [`link`] is the current session's link, inert when there is none. An
//!   inert link still keeps what is drawn on this end.
//! - [`subscribe`] is a watch whose value is replaced with a fresh link each
//!   time a session starts or ends. On a change, carry what was drawn onto
//!   the new link, as the GPUI viewports do:
//!   `let carried = old.annotations_held(); new.adopt(carried);`. A tactics
//!   board first narrows the link with `link.on_board(board_id)`.
//! - [`revisions`] is a watch bumped on every change the peer task makes
//!   (cursors, pings, shapes, boards, frames). A frame producer redraws when
//!   it has changed.
//!
//! Through the link:
//!
//! - Cursors and pings: `report_cursor`, `send_ping`, `peer_cursors`,
//!   `peer_pings`, `drop_stale_pings`.
//! - Annotations: `annotations`, `annotations_held`, `add_annotation`,
//!   `update_annotation`, `erase_annotation`, `restore`, `replace_annotations`,
//!   `annotations_locked`.
//! - Tactics boards: `on_board`, `announce_board`, `close_board`, `set_cap`,
//!   `remove_cap`, `board_caps`, `board_versions`, `session_boards`.
//! - Playback: `announce_replay`, `close_replay`, `broadcasts_frames`,
//!   `broadcast_frame`, `watch_window`, `stop_watching`,
//!   `take_forced_windows`, `open_for_everyone`.
//!
//! Ids are `wt_collab_client::ids::{UserId, BoardId, ReplayId, CapPointId}`.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::Mutex;
use std::sync::MutexGuard;

use nsh_core::prelude::*;
use rust_i18n::t;
use wt_collab_client::PeerRole;
use wt_collab_client::Permissions;
use wt_collab_client::SessionStatus;
use wt_collab_client::SessionWaker;
use wt_collab_client::controller::NoticeSeverity;
use wt_collab_client::controller::SessionController;
use wt_collab_client::controller::SessionFailure;
use wt_collab_client::controller::SessionNotice;
use wt_collab_client::controller::StartError;
use wt_collab_client::link::CollabLink;

use crate::runtime;

static SERVICE: LazyLock<Service> = LazyLock::new(Service::new);

/// How many changes the peer task has made. Compared, never read for its
/// value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionRevision(u64);

/// The current session's link, inert when there is none.
pub fn link() -> CollabLink {
    SERVICE.lock().controller.link()
}

/// The battles the session is on, with the art the end that owns each sent.
pub fn shared_windows() -> Vec<wt_collab_client::controller::SharedWindow> {
    SERVICE.lock().controller.shared_windows()
}

/// A watch on the link: replaced each time a session starts or ends.
pub fn subscribe() -> tokio::sync::watch::Receiver<CollabLink> {
    SERVICE.links.subscribe()
}

/// A watch bumped on every change the session makes.
pub fn revisions() -> tokio::sync::watch::Receiver<SessionRevision> {
    SERVICE.revisions.subscribe()
}

/// Where the session stands, as the popover shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PropEnum)]
pub enum CollabPhase {
    Idle,
    /// Hosting or joining.
    Starting,
    Active,
    Error,
}

/// How loudly a notice is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PropEnum)]
pub enum CollabNoticeLevel {
    Info,
    Warning,
    Error,
}

/// Why a session request was refused.
#[derive(Debug, Clone, thiserror::Error)]
pub enum CollabError {
    #[error(transparent)]
    Runtime(#[from] runtime::RuntimeError),
    #[error("{}", start_error_text(.0))]
    Start(#[source] StartError),
    #[error("not a session id: {raw:?}")]
    InvalidId {
        raw: String,
        #[source]
        source: std::num::ParseIntError,
    },
}

/// The session as the popover shows it.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct CollabState {
    pub status: CollabPhase,
    /// What went wrong, while `status` is `error`.
    pub error: Option<String>,
    /// Whether a session runs, hosted or joined.
    pub active: bool,
    /// The heading of a running session: hosting or connected.
    pub heading: String,
    /// The name this app appears under.
    pub display_name: String,
    /// The token peers join with, for the copy button. Only a host has one.
    pub token: Option<String>,
    /// The token as shown: masked unless revealed.
    pub token_shown: Option<String>,
    pub token_revealed: bool,
    pub web_link: Option<String>,
    /// The link to a web client served on this machine, in a debug build.
    pub localhost_link: Option<String>,
    /// How many are connected, as the popover says it.
    pub connected_count: String,
    pub roster: Vec<RosterEntry>,
    pub shared_windows: Vec<SharedWindowEntry>,
    /// Whether this app may lock, reset and promote.
    pub may_steer: bool,
    pub annotations_locked: bool,
    pub settings_locked: bool,
    /// Why the last start or session failed, for the line under the controls.
    pub failure: Option<String>,
}

/// One participant.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct RosterEntry {
    /// The session's id for them, in decimal.
    pub user_id: String,
    pub name: String,
    /// `Host` or `Co-host`, translated; none for a peer.
    pub role: Option<String>,
    pub is_me: bool,
    /// Whether this app may raise them to co-host.
    pub promotable: bool,
}

/// One battle the session is on.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct SharedWindowEntry {
    /// The session's id for the window, in decimal.
    pub replay_id: String,
    pub replay_name: String,
    pub map: String,
}

/// Something that happened in the session.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct CollabNotice {
    pub text: String,
    pub level: CollabNoticeLevel,
}

/// What a listener is told after each change.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct CollabUpdate {
    pub state: CollabState,
    /// What happened since the listener was last told.
    pub notices: Vec<CollabNotice>,
}

/// Collaborative sessions: host, join, steer and leave.
#[module]
pub struct Collab;

#[module]
impl Collab {
    /// The session now.
    pub fn state() -> CollabState {
        SERVICE.lock().state()
    }

    /// Calls `listener` with the session after every change, starting now.
    /// Returns a function that stops it.
    pub fn on(#[args(update)] listener: Callback<CollabUpdate>, cx: CallCx<'_>) -> Stop {
        let stopped = Rc::new(Cell::new(false));
        let mut changes = SERVICE.revisions.subscribe();
        let mut notices = SERVICE.notices.subscribe();
        listener.post(CollabUpdate { state: SERVICE.lock().state(), notices: Vec::new() });

        let running = Rc::clone(&stopped);
        cx.start(async move {
            while changes.changed().await.is_ok() {
                if running.get() {
                    return;
                }
                let state = SERVICE.refresh();
                let mut told = Vec::new();
                loop {
                    match notices.try_recv() {
                        Ok(notice) => told.push(notice),
                        Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => {}
                        Err(_) => break,
                    }
                }
                listener.post(CollabUpdate { state, notices: told });
            }
        });

        Stop::new(move || stopped.set(true))
    }

    /// Sets the name this app appears under. The settings store saves it.
    pub fn set_display_name(name: String) {
        SERVICE.lock().display_name = name;
        SERVICE.changed();
    }

    /// Starts hosting a session.
    pub fn host() -> Result<(), CollabError> {
        SERVICE.start(|controller, runtime, name| controller.host(runtime, toolkit_version(), name))
    }

    /// Joins the session `token` names.
    pub fn join(token: String) -> Result<(), CollabError> {
        SERVICE.start(|controller, runtime, name| controller.join(runtime, &token, toolkit_version(), name))
    }

    /// Leaves or stops the session.
    pub fn leave() {
        {
            let mut service = SERVICE.lock();
            service.controller.leave();
            service.token_revealed = false;
        }
        SERVICE.publish_link();
        SERVICE.changed();
    }

    /// Shows or masks the token.
    pub fn toggle_token() {
        {
            let mut service = SERVICE.lock();
            service.token_revealed = !service.token_revealed;
        }
        SERVICE.changed();
    }

    /// Locks or unlocks what peers may change.
    pub fn set_permissions(annotations_locked: bool, settings_locked: bool) {
        SERVICE.lock().controller.set_permissions(Permissions { annotations_locked, settings_locked });
        SERVICE.changed();
    }

    /// Returns every peer's display settings to the host's.
    pub fn reset_overrides() {
        SERVICE.lock().controller.reset_overrides();
    }

    /// Raises a peer to co-host.
    pub fn promote(user_id: String) -> Result<(), CollabError> {
        let user_id = parse_id(&user_id)?;
        SERVICE.lock().controller.promote(user_id);
        SERVICE.changed();

        Ok(())
    }

    /// Asks every peer to open one of the session's windows.
    pub fn open_for_everyone(replay_id: String) -> Result<(), CollabError> {
        let replay_id = parse_id(&replay_id)?;
        link().open_for_everyone(replay_id);

        Ok(())
    }
}

/// The app's session and the channels its readers watch.
struct Service {
    held: Mutex<Held>,
    links: tokio::sync::watch::Sender<CollabLink>,
    revisions: tokio::sync::watch::Sender<SessionRevision>,
    notices: tokio::sync::broadcast::Sender<CollabNotice>,
}

/// What the lock guards.
struct Held {
    controller: SessionController,
    display_name: String,
    token_revealed: bool,
    /// A start the background runtime could not take, which the controller
    /// never saw.
    runtime_failure: Option<runtime::RuntimeError>,
    /// Whether the last published link was a running session's.
    published_active: bool,
}

/// Bumps the revision on every change the peer task makes. Touches only the
/// watch: the peer task may hold the session state's lock when it wakes.
struct RevisionWaker {
    revisions: tokio::sync::watch::Sender<SessionRevision>,
}

impl SessionWaker for RevisionWaker {
    fn wake(&self) {
        self.revisions.send_modify(|revision| revision.0 = revision.0.wrapping_add(1));
    }
}

impl Service {
    fn new() -> Self {
        let controller = SessionController::new();
        let (revisions, _) = tokio::sync::watch::channel(SessionRevision::default());
        controller.install_waker(Arc::new(RevisionWaker { revisions: revisions.clone() }));
        let (links, _) = tokio::sync::watch::channel(controller.link());
        let (notices, _) = tokio::sync::broadcast::channel(64);

        Self {
            held: Mutex::new(Held {
                controller,
                display_name: String::new(),
                token_revealed: false,
                runtime_failure: None,
                published_active: false,
            }),
            links,
            revisions,
            notices,
        }
    }

    fn lock(&self) -> MutexGuard<'_, Held> {
        self.held.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Tells every reader something changed.
    fn changed(&self) {
        self.revisions.send_modify(|revision| revision.0 = revision.0.wrapping_add(1));
    }

    /// Hands viewports a fresh link when a session has started or ended.
    fn publish_link(&self) {
        let link = {
            let mut held = self.lock();
            let active = held.controller.is_active();
            if active == held.published_active {
                return;
            }
            held.published_active = active;
            held.controller.link()
        };
        self.links.send_replace(link);
    }

    fn start(
        &self,
        start: impl FnOnce(&mut SessionController, Arc<tokio::runtime::Runtime>, &str) -> Result<(), StartError>,
    ) -> Result<(), CollabError> {
        let runtime = match runtime::shared() {
            Ok(runtime) => runtime,
            Err(error) => {
                self.lock().runtime_failure = Some(error.clone());
                self.changed();
                return Err(error.into());
            }
        };
        let started = {
            let mut held = self.lock();
            held.runtime_failure = None;
            let name = held.display_name.clone();
            start(&mut held.controller, runtime, &name)
        };
        self.publish_link();
        self.changed();

        started.map_err(CollabError::Start)
    }

    /// Takes what the session raised, says it to every listener, and reads
    /// the session as it is now.
    fn refresh(&self) -> CollabState {
        let (notices, state) = {
            let mut held = self.lock();
            let notices = held.controller.poll();
            if !held.controller.is_active() {
                held.token_revealed = false;
            }
            (notices, held.state())
        };
        for notice in notices {
            // No receiver is no listener, which has nobody to tell.
            let _ = self.notices.send(notice_of(notice));
        }
        self.publish_link();

        state
    }
}

impl Held {
    fn state(&self) -> CollabState {
        let controller = &self.controller;
        let (status, error) = match controller.status() {
            SessionStatus::Idle => (CollabPhase::Idle, None),
            SessionStatus::Starting | SessionStatus::Connecting => (CollabPhase::Starting, None),
            SessionStatus::Active => (CollabPhase::Active, None),
            SessionStatus::Error(reason) => (CollabPhase::Error, Some(reason)),
        };
        let token = controller.token();
        let token_shown = token
            .as_ref()
            .map(|token| if self.token_revealed { token.clone() } else { "*".repeat(token.chars().count().min(32)) });
        let may_steer = controller.may_steer();
        let may_promote = controller.may_promote();
        let roster = controller.connected();
        let permissions = controller.permissions();
        let heading =
            if controller.is_hosting() { t!("ui.collab.session_active") } else { t!("ui.collab.connected_to_session") };

        CollabState {
            status,
            error,
            active: controller.is_active(),
            heading: heading.into_owned(),
            display_name: self.display_name.clone(),
            token,
            token_shown,
            token_revealed: self.token_revealed,
            web_link: controller.web_link(),
            localhost_link: cfg!(debug_assertions).then(|| controller.localhost_link()).flatten(),
            connected_count: t!("ui.collab.connected_count", count = roster.len()).into_owned(),
            roster: roster
                .into_iter()
                .map(|peer| RosterEntry {
                    user_id: peer.user_id.to_string(),
                    role: match peer.role {
                        PeerRole::Host => Some(t!("ui.collab.role_host").into_owned()),
                        PeerRole::CoHost => Some(t!("ui.collab.role_cohost").into_owned()),
                        PeerRole::Peer => None,
                    },
                    // Only the original host raises a peer, never itself or another host.
                    promotable: may_promote && peer.role == PeerRole::Peer && !peer.is_me,
                    is_me: peer.is_me,
                    name: peer.name,
                })
                .collect(),
            shared_windows: controller
                .shared_windows()
                .into_iter()
                .map(|window| SharedWindowEntry {
                    replay_id: window.replay_id.to_string(),
                    replay_name: window.replay_name,
                    map: window.map,
                })
                .collect(),
            may_steer,
            annotations_locked: permissions.annotations_locked,
            settings_locked: permissions.settings_locked,
            failure: self
                .runtime_failure
                .as_ref()
                .map(ToString::to_string)
                .or_else(|| controller.failure().map(failure_text)),
        }
    }
}

/// What a refused start says, in the reader's language where the catalog has it.
fn start_error_text(error: &StartError) -> String {
    match error {
        StartError::NoDisplayName => t!("ui.collab.display_name_error").into_owned(),
        StartError::InvalidToken { expected_prefix } => {
            t!("ui.collab.invalid_token", error = expected_prefix).into_owned()
        }
        StartError::AlreadyRunning => error.to_string(),
    }
}

fn failure_text(failure: &SessionFailure) -> String {
    match failure {
        SessionFailure::InvalidToken { expected_prefix } => {
            t!("ui.collab.invalid_token", error = expected_prefix).into_owned()
        }
        SessionFailure::Error(reason) | SessionFailure::Rejected(reason) => reason.clone(),
    }
}

fn notice_of(notice: SessionNotice) -> CollabNotice {
    let level = match notice.severity() {
        NoticeSeverity::Info => CollabNoticeLevel::Info,
        NoticeSeverity::Warning => CollabNoticeLevel::Warning,
        NoticeSeverity::Error => CollabNoticeLevel::Error,
    };
    let text = match notice {
        SessionNotice::HostingStarted => t!("ui.messages.session_started"),
        SessionNotice::Connected => t!("ui.messages.connected_to_session"),
        SessionNotice::UserJoined { name } => t!("ui.messages.user_joined", name = name),
        SessionNotice::UserLeft { name } => t!("ui.messages.user_left", name = name),
        SessionNotice::UserTimedOut { name } => t!("ui.messages.user_timeout", name = name),
        SessionNotice::ReplayOpened { replay_name } => t!("ui.messages.host_opened_replay", name = replay_name),
        SessionNotice::ReplayClosed => t!("ui.messages.host_closed_replay"),
        SessionNotice::Ended => t!("ui.messages.session_ended"),
        SessionNotice::Failed { reason } => t!("ui.messages.session_error", msg = reason),
        SessionNotice::Rejected { reason } => t!("ui.messages.session_rejected", reason = reason),
    };

    CollabNotice { text: text.into_owned(), level }
}

fn parse_id(raw: &str) -> Result<u64, CollabError> {
    raw.parse().map_err(|source| CollabError::InvalidId { raw: raw.to_owned(), source })
}

/// What this build calls itself to its peers.
fn toolkit_version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_handed_out_without_a_session_is_inert() {
        assert!(!link().is_active());
        assert!(!subscribe().borrow().is_active());
    }

    #[test]
    fn notices_are_told_in_the_readers_words() {
        let notice = notice_of(SessionNotice::UserTimedOut { name: "Mira".into() });

        assert_eq!(notice.level, CollabNoticeLevel::Warning);
        assert!(notice.text.contains("Mira"), "{}", notice.text);
    }

    #[test]
    fn a_peer_id_round_trips_through_its_decimal_form() {
        assert_eq!(parse_id(&u64::MAX.to_string()).ok(), Some(u64::MAX));
        assert!(parse_id("seven").is_err());
    }

    #[test]
    fn a_token_is_masked_until_revealed() {
        let held = Held {
            controller: SessionController::new(),
            display_name: "me".into(),
            token_revealed: false,
            runtime_failure: None,
            published_active: false,
        };
        held.controller.state().lock().token = Some("toolkit-abcdef".into());

        let masked = held.state();
        assert_eq!(masked.token.as_deref(), Some("toolkit-abcdef"));
        assert!(masked.token_shown.is_some_and(|shown| !shown.contains("abcdef")));

        let revealed = Held { token_revealed: true, ..held };
        assert_eq!(revealed.state().token_shown.as_deref(), Some("toolkit-abcdef"));
    }
}
