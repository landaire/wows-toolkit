//! Polling a Twitch channel's chat for who is watching.
//!
//! The roster's possible-stream-sniper chip is drawn from observations in the
//! shared config database. The egui app writes them from its own poll; this is
//! the same poll, so the port fills the table rather than only reading what
//! the other app happened to collect.
//!
//! Who a login plausibly names, and how long an observation counts for, are
//! `wows_toolkit_viewmodel::twitch`; this module is only the fetching.

use std::time::Duration;

use sqlx::sqlite::SqlitePool;
use twitch_api::HelixClient;
use twitch_api::helix::chat::get_chatters;
use twitch_api::twitch_oauth2::AccessToken;
use twitch_api::twitch_oauth2::UserToken;
use twitch_api::types::UserId;
use wows_toolkit_config::index::query;
use wows_toolkit_viewmodel::twitch::POLL_INTERVAL;
use wows_toolkit_viewmodel::twitch::Token;

/// Why a poll produced nothing.
#[derive(Debug, thiserror::Error)]
pub enum PollError {
    #[error("the stored credential was refused by Twitch: {0}")]
    Credential(String),
    #[error("the channel {channel:?} is not one Twitch knows")]
    UnknownChannel { channel: String },
    #[error("the chatter list could not be read: {0}")]
    Chatters(String),
    #[error("the observations could not be recorded: {0}")]
    Record(#[from] wows_toolkit_config::index::rows::IndexError),
}

/// A validated credential and the channel it will poll.
pub struct Session {
    client: HelixClient<'static, reqwest::Client>,
    token: UserToken,
    channel: UserId,
}

impl Session {
    /// Validates `token` against Twitch and resolves which channel to watch.
    ///
    /// An empty `channel` watches the credential's own, which is what the
    /// egui app does: the common case is streaming under the account the
    /// credential belongs to.
    pub async fn open(token: &Token, channel: &str, client: reqwest::Client) -> Result<Self, PollError> {
        let client: HelixClient<'static, reqwest::Client> = HelixClient::with_client(client);
        let access = AccessToken::new(token.oauth_token().to_string());
        let token = UserToken::from_existing(&client, access, None, None)
            .await
            .map_err(|err| PollError::Credential(err.to_string()))?;

        let channel = if channel.is_empty() {
            token.user_id.clone()
        } else {
            client
                .get_user_from_login(channel, &token)
                .await
                .map_err(|err| PollError::Chatters(err.to_string()))?
                .ok_or_else(|| PollError::UnknownChannel { channel: channel.to_string() })?
                .id
        };

        Ok(Self { client, token, channel })
    }

    /// One poll: who is in chat now, recorded against this moment.
    ///
    /// Returns how many logins were seen. Observations are insert-or-ignore,
    /// so a viewer who stays in chat across polls is recorded once per
    /// distinct second rather than accumulating duplicates.
    pub async fn poll_once(&self, pool: &SqlitePool, now: jiff::Timestamp) -> Result<usize, PollError> {
        let request = get_chatters::GetChattersRequest::new(&self.channel, &self.token.user_id);
        let chatters =
            self.client.req_get(request, &self.token).await.map_err(|err| PollError::Chatters(err.to_string()))?.data;

        let seen_at = now.as_second();
        let observations: Vec<(String, i64)> =
            chatters.iter().map(|chatter| (chatter.user_login.to_string(), seen_at)).collect();
        if observations.is_empty() {
            return Ok(0);
        }

        query::record_twitch_observations(pool, &observations).await?;
        Ok(observations.len())
    }
}

/// How long to wait before polling again.
pub const fn poll_interval() -> Duration {
    POLL_INTERVAL
}
