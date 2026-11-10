//! Collaborative replay sessions: hosting one, joining one, and what the
//! session is doing while it runs.
//!
//! The session itself is `wt-collab-client`, shared with the egui and NSH
//! apps, so every front end runs one implementation of the mesh and of the
//! session controller. What is here is the part that is this front end's own:
//! waking it when the peer task changes something, the name and token fields,
//! and the reader-facing text.

use std::sync::Arc;

use gpui_kit::App;
use gpui_kit::AsyncApp;
use gpui_kit::Entity;
use rust_i18n::t;
use wt_collab_client::Permissions;
use wt_collab_client::SessionStatus;
use wt_collab_client::SessionWaker;
use wt_collab_client::controller::ConnectedPeer;
use wt_collab_client::controller::NoticeSeverity;
use wt_collab_client::controller::SessionController;
use wt_collab_client::controller::SessionFailure;
use wt_collab_client::controller::SharedWindow;
use wt_collab_client::controller::StartError;
use wt_collab_client::link::CollabLink;

use crate::runtime;

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
#[derive(Default)]
pub struct CollabState {
    controller: SessionController,
    /// The name this app appears under. Empty until one is entered, which is
    /// what refuses the start and join controls.
    pub display_name: String,
    /// Whether the token is shown rather than masked.
    pub token_revealed: bool,
    /// A start the runtime could not take, which the controller never saw.
    no_runtime: bool,
}

impl CollabState {
    /// Whether a session is running, hosted or joined.
    pub fn is_active(&self) -> bool {
        self.controller.is_active()
    }

    pub fn is_hosting(&self) -> bool {
        self.controller.is_hosting()
    }

    /// What the session is doing.
    pub fn status(&self) -> SessionStatus {
        self.controller.status()
    }

    /// The token peers join with. `None` until the host has one.
    pub fn token(&self) -> Option<String> {
        self.controller.token()
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
        self.controller.localhost_link()
    }

    /// What the session is showing, as the host has told everyone.
    ///
    /// A peer in this app plays a battle back from its own copy of the replay, so
    /// this is a list of what the session is on rather than a set of windows to
    /// open: a reader who has the same replay can open it themselves, and one who
    /// does not cannot be shown it (see `docs/gpui-port-gaps.md`, item 22).
    pub fn shared_windows(&self) -> Vec<SharedWindow> {
        self.controller.shared_windows()
    }

    /// A link a browser can join from.
    pub fn web_link(&self) -> Option<String> {
        self.controller.web_link()
    }

    /// Everyone in the session, this app included.
    pub fn connected(&self) -> Vec<ConnectedPeer> {
        self.controller.connected()
    }

    /// What a viewport needs of the session: where peers are pointing, and
    /// somewhere to say where this app is pointing.
    pub fn link(&self) -> CollabLink {
        self.controller.link()
    }

    pub fn permissions(&self) -> Permissions {
        self.controller.permissions()
    }

    /// Whether this app may change permissions and promote peers.
    pub fn may_steer(&self) -> bool {
        self.controller.may_steer()
    }

    /// What went wrong starting or joining, for the line under the controls.
    pub fn failure(&self) -> Option<String> {
        if self.no_runtime {
            return Some(t!("ui.collab.no_runtime").into_owned());
        }
        self.controller.failure().map(|failure| match failure {
            SessionFailure::InvalidToken { expected_prefix } => {
                t!("ui.collab.invalid_token", error = expected_prefix).into_owned()
            }
            SessionFailure::Error(reason) | SessionFailure::Rejected(reason) => reason.clone(),
        })
    }

    /// Installs the waker, once, so the peer task can reach this front end.
    pub fn bind<V: 'static>(&self, view: &Entity<V>, cx: &mut App) {
        let (nudge, mut nudges) = futures::channel::mpsc::unbounded::<()>();
        if !self.controller.install_waker(Arc::new(GpuiWaker { nudge })) {
            return;
        }

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

    /// Starts hosting. Refused without a name, which the controls already
    /// prevent.
    pub fn host(&mut self, toolkit_version: String, cx: &mut App) {
        let Some(runtime) = self.runtime(cx) else { return };
        let started = self.controller.host(runtime, toolkit_version, &self.display_name);
        Self::log_refusal(started);
    }

    /// Joins the session `token` names.
    pub fn join(&mut self, token: String, toolkit_version: String, cx: &mut App) {
        let Some(runtime) = self.runtime(cx) else { return };
        let started = self.controller.join(runtime, &token, toolkit_version, &self.display_name);
        Self::log_refusal(started);
    }

    fn runtime(&mut self, cx: &App) -> Option<Arc<tokio::runtime::Runtime>> {
        let runtime = runtime::runtime(cx);
        self.no_runtime = runtime.is_none();

        runtime
    }

    /// A refused start is shown through `failure` where the reader can act on
    /// it; the rest are states the controls do not offer.
    fn log_refusal(started: Result<(), StartError>) {
        if let Err(err) = started {
            tracing::debug!("collab: session not started: {err}");
        }
    }

    /// Leaves or stops the session.
    pub fn leave(&mut self) {
        self.controller.leave();
        self.token_revealed = false;
    }

    /// Locks or unlocks what peers may change.
    pub fn set_permissions(&self, permissions: Permissions) {
        self.controller.set_permissions(permissions);
    }

    /// Returns every peer's display settings to the host's.
    pub fn reset_overrides(&self) {
        self.controller.reset_overrides();
    }

    /// Raises a peer to co-host.
    pub fn promote(&self, user_id: wt_collab_client::ids::UserId) {
        self.controller.promote(user_id);
    }

    /// Takes what the peer task has raised since the last look and acts on
    /// it.
    ///
    /// Called from the header's own draw, as the egui app polls its inbox
    /// every frame. Returns what the reader should be told, which the caller
    /// says once it has a window.
    pub fn poll(&mut self) -> Vec<SessionNotice> {
        let was_active = self.controller.is_active();
        let notices = self.controller.poll();
        if was_active && !self.controller.is_active() {
            self.token_revealed = false;
        }

        notices.into_iter().map(SessionNotice::from_controller).collect()
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
    level: NoticeSeverity,
}

impl SessionNotice {
    fn from_controller(notice: wt_collab_client::controller::SessionNotice) -> Self {
        use wt_collab_client::controller::SessionNotice as Raised;

        let level = notice.severity();
        let said = match notice {
            Raised::HostingStarted => t!("ui.messages.session_started"),
            Raised::Connected => t!("ui.messages.connected_to_session"),
            Raised::UserJoined { name } => t!("ui.messages.user_joined", name = name),
            Raised::UserLeft { name } => t!("ui.messages.user_left", name = name),
            Raised::UserTimedOut { name } => t!("ui.messages.user_timeout", name = name),
            Raised::ReplayOpened { replay_name } => t!("ui.messages.host_opened_replay", name = replay_name),
            Raised::ReplayClosed => t!("ui.messages.host_closed_replay"),
            Raised::Ended => t!("ui.messages.session_ended"),
            Raised::Failed { reason } => t!("ui.messages.session_error", msg = reason),
            Raised::Rejected { reason } => t!("ui.messages.session_rejected", reason = reason),
        }
        .into_owned();

        Self { said, level }
    }

    /// Puts it on screen at its own level.
    pub fn say(self, window: &mut gpui_kit::Window, cx: &mut gpui_kit::App) {
        match self.level {
            NoticeSeverity::Info => crate::toast::info(self.said, window, cx),
            NoticeSeverity::Warning => crate::toast::warn(self.said, window, cx),
            NoticeSeverity::Error => crate::toast::failed(self.said, window, cx),
        }
    }
}

/// Puts a ping dropped `age` ago into `state`.
///
/// Here rather than beside the test that wants it: a ping's clock is
/// `web_time`, which is this module's dependency rather than a viewport's.
#[cfg(test)]
pub(crate) fn push_ping_aged(
    state: &Arc<parking_lot::Mutex<wt_collab_client::SessionState>>,
    pos: [f32; 2],
    age: std::time::Duration,
) {
    state.lock().pings.push(wt_collab_client::PeerPing {
        user_id: 7,
        color: [255, 0, 0],
        pos,
        time: web_time::Instant::now() - age,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_is_masked_until_it_is_revealed() {
        let mut collab = CollabState::default();
        collab.controller.state().lock().token = Some("toolkit-abcdef".to_string());

        let masked = collab.token_display().expect("there is a token");
        assert!(!masked.contains("abcdef"), "a token on a shared screen is not readable by default");

        collab.token_revealed = true;
        assert_eq!(collab.token_display().as_deref(), Some("toolkit-abcdef"));
    }

    #[test]
    fn leaving_hides_the_token_again() {
        let mut collab = CollabState::default();
        collab.token_revealed = true;

        collab.leave();

        assert!(!collab.token_revealed);
    }
}
