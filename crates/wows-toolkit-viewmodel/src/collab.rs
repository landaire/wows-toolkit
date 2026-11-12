//! What a collaborative session tells the reader.

use rust_i18n::t;

/// What a session token is prefixed with, so a paste can be checked before it
/// is sent anywhere.
pub const TOKEN_PREFIX: &str = "toolkit-";

/// Something that happened in the session, for the reader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionNotice {
    /// This app's hosted session is up.
    Started,
    /// This app joined someone else's session.
    Connected,
    UserJoined {
        name: String,
    },
    UserLeft {
        name: String,
    },
    /// A peer stopped answering and was dropped.
    UserTimedOut {
        name: String,
    },
    /// The host opened a replay this front end may not be able to show.
    HostOpenedReplay {
        name: String,
    },
    HostClosedReplay,
    Ended,
    Failed {
        reason: String,
    },
    Rejected {
        reason: String,
    },
}

/// How loudly a [`SessionNotice`] is said.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeLevel {
    /// Something happened that nobody asked for.
    Info,
    /// Something is wrong but the session goes on.
    Warn,
    /// The session is over.
    Failed,
}

impl SessionNotice {
    pub fn level(&self) -> NoticeLevel {
        match self {
            Self::UserTimedOut { .. } => NoticeLevel::Warn,
            Self::Failed { .. } | Self::Rejected { .. } => NoticeLevel::Failed,
            Self::Started
            | Self::Connected
            | Self::UserJoined { .. }
            | Self::UserLeft { .. }
            | Self::HostOpenedReplay { .. }
            | Self::HostClosedReplay
            | Self::Ended => NoticeLevel::Info,
        }
    }

    pub fn text(&self) -> String {
        match self {
            Self::Started => t!("ui.messages.session_started").into_owned(),
            Self::Connected => t!("ui.messages.connected_to_session").into_owned(),
            Self::UserJoined { name } => t!("ui.messages.user_joined", name = name).into_owned(),
            Self::UserLeft { name } => t!("ui.messages.user_left", name = name).into_owned(),
            Self::UserTimedOut { name } => t!("ui.messages.user_timeout", name = name).into_owned(),
            Self::HostOpenedReplay { name } => t!("ui.messages.host_opened_replay", name = name).into_owned(),
            Self::HostClosedReplay => t!("ui.messages.host_closed_replay").into_owned(),
            Self::Ended => t!("ui.messages.session_ended").into_owned(),
            Self::Failed { reason } => t!("ui.messages.session_error", msg = reason).into_owned(),
            Self::Rejected { reason } => t!("ui.messages.session_rejected", reason = reason).into_owned(),
        }
    }
}

/// Why a session could not be started or joined, or why it ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionFailure {
    /// The pasted token does not start with [`TOKEN_PREFIX`].
    InvalidToken,
    /// There is no async runtime to run the session on.
    NoRuntime,
    /// The session reported an error or refused this app.
    Session(String),
}

impl SessionFailure {
    pub fn text(&self) -> String {
        match self {
            Self::InvalidToken => t!("ui.collab.invalid_token", error = TOKEN_PREFIX).into_owned(),
            Self::NoRuntime => t!("ui.collab.no_runtime").into_owned(),
            Self::Session(reason) => reason.clone(),
        }
    }
}
