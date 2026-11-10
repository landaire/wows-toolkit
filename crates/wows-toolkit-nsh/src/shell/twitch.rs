//! The Twitch chat poll that feeds the player tracker's stream-sniper marks.
//!
//! The observations go to the shared database, which is where the tracker
//! reads them from and where the other front ends write their own. Without a
//! stored credential nothing is polled.

use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use nsh_core::prelude::JsObject;
use nsh_core::prelude::PropEnum;
use nsh_core::prelude::background;
use nsh_core::prelude::module;
use wows_toolkit_viewmodel::twitch::Token;
use wows_toolkit_viewmodel::twitch::keys as twitch_keys;
use wows_toolkit_viewmodel::twitch_chat::Session;
use wows_toolkit_viewmodel::twitch_chat::poll_interval;

use crate::shell::error::PasteError;
use crate::shell::error::SettingsError;
use crate::shell::update::USER_AGENT;

/// What Twitch made of the stored credential. Only Twitch can say whether a
/// stored credential still works.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PropEnum)]
pub enum TwitchState {
    /// Nothing stored to check.
    Unset,
    /// Twitch took it, and the channel is being polled.
    Accepted,
    /// Twitch refused it: expired, revoked, or never valid.
    Refused,
}

/// The poll's state, and whose credential it runs under.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct TwitchStatus {
    pub state: TwitchState,
    /// The account the credential belongs to, while one is stored.
    pub who: Option<String>,
}

/// The poll that is current. Each restart takes the next number, and a poll
/// whose number is no longer current stops at its next wake.
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// One run of the poll, numbered from [`GENERATION`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PollGeneration(u64);

impl PollGeneration {
    /// Takes the next number, which stops every earlier poll.
    fn next() -> Self {
        Self(GENERATION.fetch_add(1, Ordering::SeqCst) + 1)
    }

    fn is_current(self) -> bool {
        GENERATION.load(Ordering::SeqCst) == self.0
    }
}

/// The Twitch credential and the poll it drives.
#[module]
pub struct Twitch;

#[module]
impl Twitch {
    /// Stores the credential on the clipboard. Resolves to the account it
    /// belongs to, or `None` when the clipboard holds no text.
    pub async fn paste_token() -> Result<Option<String>, PasteError> {
        let pasted = nsh_core::prelude::clipboard_string().await;
        if pasted.trim().is_empty() {
            return Ok(None);
        }

        let token: Token = pasted.trim().parse()?;
        let who = token.username().to_owned();
        let encoded = serde_json::to_string(&token).map_err(|source| SettingsError::Value {
            key: twitch_keys::TOKEN,
            expected: "credential",
            source,
        })?;
        background(async move {
            let pool = crate::shell::settings::open().await?;
            wows_toolkit_config::queries::set_setting_raw(&pool, twitch_keys::TOKEN, &encoded)
                .await
                .map_err(|source| SettingsError::Write { key: twitch_keys::TOKEN, source })
        })
        .await
        .map_err(SettingsError::Task)??;

        Ok(Some(who))
    }

    /// Stops the running poll and starts one under the stored credential and
    /// channel. Resolves once Twitch has answered for the credential.
    pub async fn restart() -> Result<TwitchStatus, SettingsError> {
        let generation = PollGeneration::next();
        background(start(generation)).await.map_err(SettingsError::Task)?
    }
}

async fn start(generation: PollGeneration) -> Result<TwitchStatus, SettingsError> {
    let pool = crate::shell::settings::open().await?;
    let proxy_url = crate::shell::settings::manual_proxy(&pool).await?;
    let channel = crate::shell::settings::twitch_channel(&pool).await?;
    // The other front ends store the credential as an `Option`, so a cleared
    // one is the row `null`.
    let token = wows_toolkit_config::queries::try_get_setting::<Option<Token>>(&pool, twitch_keys::TOKEN)
        .await
        .map_err(|source| SettingsError::Read { key: twitch_keys::TOKEN, source })?
        .flatten();
    let Some(token) = token else {
        return Ok(TwitchStatus { state: TwitchState::Unset, who: None });
    };

    let who = Some(token.username().to_owned());
    let client = match wows_toolkit_viewmodel::http::client(
        USER_AGENT,
        proxy_url.as_deref().unwrap_or_default(),
        reqwest::redirect::Policy::default(),
    ) {
        Ok(client) => client,
        Err(err) => {
            tracing::warn!("twitch: no client to poll with: {err}");
            return Ok(TwitchStatus { state: TwitchState::Refused, who });
        }
    };
    let session = match Session::open(&token, channel.as_deref().unwrap_or_default(), client).await {
        Ok(session) => session,
        Err(err) => {
            tracing::warn!("twitch: the stored credential is not usable: {err}");
            return Ok(TwitchStatus { state: TwitchState::Refused, who });
        }
    };

    tokio::spawn(async move {
        while generation.is_current() {
            match session.poll_once(&pool, jiff::Timestamp::now()).await {
                Ok(seen) => tracing::debug!("twitch: recorded {seen} chat observation(s)"),
                // A poll that fails says nothing about the next one: the
                // channel may simply have gone offline.
                Err(err) => tracing::warn!("twitch: a chat poll failed: {err}"),
            }
            tokio::time::sleep(poll_interval()).await;
        }
    });

    Ok(TwitchStatus { state: TwitchState::Accepted, who })
}
