//! The Twitch chat poll: who is in the watched channel's chat, recorded to the shared database
//! where the roster's possible-stream-sniper chip reads it, and whether Twitch took the stored
//! credential.

use std::sync::Arc;

use futures::future::AbortHandle;
use landaire_ui::Notice;
use landaire_ui::ToastKey;
use rust_i18n::t;
use wows_toolkit_services::twitch::Session;
use wows_toolkit_viewmodel::settings_drawer::Setting;
use wows_toolkit_viewmodel::twitch::Token;
use wows_toolkit_viewmodel::twitch::keys as twitch_keys;
use zgui::prelude::ClipboardKind;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::StoredValue;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::UpdateValue;
use zgui::reactive::prelude::WithValue;
use zgui::task::spawn_detached;

use crate::shell::Shell;

/// The message key a credential Twitch refused stays up under until it is replaced.
const STUCK_TWITCH_TOKEN: &str = "twitch-token";

/// What Twitch made of the stored credential. Only Twitch can answer this: a credential that is
/// present is not therefore working.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TwitchStatus {
    /// Nothing stored to check.
    Unset,
    /// Stored, and the reply has not come back.
    Checking,
    Accepted {
        who: String,
    },
    /// Expired, revoked, or never valid.
    Refused,
}

/// What the last paste of a credential made of the clipboard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TwitchPaste {
    /// Stored, for the account it names.
    Stored { who: String },
    /// Not a credential, or nothing on the clipboard.
    Refused { reason: String },
}

/// The poll's state.
#[derive(Clone, Copy)]
pub struct TwitchLink {
    status: RwSignal<TwitchStatus, LocalStorage>,
    paste: RwSignal<Option<TwitchPaste>, LocalStorage>,
    /// Stops the running poll, so one started for an older credential or channel records nothing
    /// more.
    poll: StoredValue<Option<AbortHandle>>,
}

impl TwitchLink {
    pub fn new() -> Self {
        Self {
            status: RwSignal::new_local(TwitchStatus::Unset),
            paste: RwSignal::new_local(None),
            poll: StoredValue::new(None),
        }
    }

    pub fn status(self) -> TwitchStatus {
        self.status.get()
    }

    pub fn paste(self) -> Option<TwitchPaste> {
        self.paste.get()
    }

    /// Stops the running poll, dropping the request it has in flight.
    fn stop(self) {
        self.poll.try_update_value(|poll| {
            if let Some(running) = poll.take() {
                running.abort();
            }
        });
    }
}

impl Default for TwitchLink {
    fn default() -> Self {
        Self::new()
    }
}

/// What a poll is started with, read from the settings at once.
struct PollRequest {
    token: Option<Token>,
    /// Empty watches the credential's own channel.
    channel: String,
    proxy_url: String,
}

impl Shell {
    /// Stops any running poll and starts one for the stored credential and channel.
    ///
    /// Without a credential there is nothing to poll and the chip shows whatever the other app
    /// collected.
    pub fn restart_twitch_poll(self) {
        let twitch = self.twitch;
        twitch.stop();

        let services = self.services();
        let Some(request) = services.settings.read_untracked(|settings| PollRequest {
            token: settings.twitch_token.clone(),
            channel: settings.twitch_channel.clone(),
            proxy_url: settings.proxy_url.clone(),
        }) else {
            return;
        };
        let Some(token) = request.token else {
            twitch.status.set(TwitchStatus::Unset);
            return;
        };
        let Some(pool) = services.settings.pool() else { return };

        let who = token.username().to_owned();
        twitch.status.set(TwitchStatus::Checking);

        let (polling, running) = futures::future::abortable(async move {
            let client = match wows_toolkit_services::http::client(
                crate::http::USER_AGENT,
                &request.proxy_url,
                reqwest::redirect::Policy::default(),
            ) {
                Ok(client) => client,
                Err(err) => {
                    tracing::warn!("twitch: no client to poll with: {err}");
                    return;
                }
            };

            let channel = request.channel;
            let opened = services.on_tokio(async move { Session::open(&token, &channel, client).await }).await;
            let session = match opened {
                Ok(Ok(session)) => Arc::new(session),
                Ok(Err(err)) => {
                    tracing::warn!("twitch: the stored credential is not usable: {err}");
                    self.twitch_refused();
                    return;
                }
                Err(err) => {
                    tracing::warn!("twitch: the credential check did not complete: {err}");
                    self.twitch_refused();
                    return;
                }
            };
            twitch.status.set(TwitchStatus::Accepted { who });
            self.toasts().withdraw(&ToastKey::new(STUCK_TWITCH_TOKEN));

            loop {
                let polling = Arc::clone(&session);
                let pool = pool.clone();
                let polled =
                    services.on_tokio(async move { polling.poll_once(&pool, jiff::Timestamp::now()).await }).await;
                match polled {
                    Ok(Ok(seen)) => tracing::debug!("twitch: recorded {seen} chat observation(s)"),
                    // A poll that fails says nothing about the next one: the channel may simply
                    // have gone offline.
                    Ok(Err(err)) => tracing::warn!("twitch: a chat poll failed: {err}"),
                    Err(err) => tracing::warn!("twitch: a chat poll did not complete: {err}"),
                }

                // Built inside the runtime: a tokio timer made outside one panics.
                let interval = wows_toolkit_services::twitch::poll_interval();
                if services.on_tokio(async move { tokio::time::sleep(interval).await }).await.is_err() {
                    return;
                }
            }
        });
        twitch.poll.try_update_value(|poll| *poll = Some(running));
        spawn_detached(async move {
            // Aborted by the next restart.
            let _ = polling.await;
        });
    }

    /// Reads a credential off the clipboard and stores it.
    ///
    /// The credential is pasted rather than obtained through a browser flow, as the egui app does;
    /// what comes back is which account it belongs to, or which part of the paste was wrong.
    pub fn paste_twitch_token(self) {
        let empty = move || {
            self.twitch.paste.set(Some(TwitchPaste::Refused { reason: t!("ui.collab.clipboard_empty").into_owned() }));
        };
        let reading = self.clipboard.with_value(|clipboard| {
            clipboard.as_ref().map(|clipboard| {
                clipboard.read_text(ClipboardKind::Standard, move |pasted| match pasted {
                    Some(pasted) => self.adopt_twitch_token(&pasted),
                    None => empty(),
                });
            })
        });
        if reading.is_none() {
            empty();
        }
    }

    /// Stores `pasted` as the credential when it is one, and polls with it.
    fn adopt_twitch_token(self, pasted: &str) {
        let token = match pasted.trim().parse::<Token>() {
            Ok(token) => token,
            Err(err) => {
                self.twitch.paste.set(Some(TwitchPaste::Refused { reason: err.to_string() }));
                return;
            }
        };

        let who = token.username().to_owned();
        self.services().settings.edit(twitch_keys::TOKEN, |settings| {
            settings.twitch_token = Some(token.clone());
            token
        });
        self.twitch.paste.set(Some(TwitchPaste::Stored { who }));
        self.restart_twitch_poll();
    }

    /// Adopts the channel to watch, and polls it: the running poll is reading the old one.
    pub fn set_twitch_channel(self, channel: String) {
        let unchanged = self.services().settings.read_untracked(|settings| settings.twitch_channel == channel);
        if unchanged != Some(false) {
            return;
        }

        self.services().settings.edit(twitch_keys::MONITORED_CHANNEL, |settings| {
            settings.twitch_channel = channel.clone();
            channel
        });
        self.restart_twitch_poll();
    }

    /// Twitch would not take the stored credential. Said out loud rather than only logged: the
    /// credential goes stale on its own schedule, and nothing else on screen would show it.
    fn twitch_refused(self) {
        self.twitch.status.set(TwitchStatus::Refused);
        let open = self.settings_drawer;
        self.toasts().show(
            Notice::warn(t!("ui.messages.twitch_token_invalid").into_owned())
                .keyed(ToastKey::new(STUCK_TWITCH_TOKEN))
                .sticky()
                .action(t!("ui.settings.panel.open").into_owned(), move || open.open_at(Setting::TwitchToken)),
        );
    }
}
