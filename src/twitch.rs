use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fmt::Debug,
    str::FromStr,
    sync::{atomic::AtomicBool, Arc},
    time::Instant,
};

use anyhow::anyhow;
use chrono::{DateTime, Duration, Local};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use twitch_api::{
    helix::{
        self,
        chat::{get_chatters, Chatter},
    },
    twitch_oauth2::{AccessToken, UserToken},
    types::{UserId, UserIdRef},
    HelixClient,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Token {
    username: String,
    user_id: u64,
    client_id: String,
    oauth_token: String,
}

impl Token {
    pub fn username(&self) -> &str {
        &self.username
    }

    pub fn user_id(&self) -> u64 {
        self.user_id
    }

    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    pub fn oauth_token(&self) -> &str {
        &self.oauth_token
    }
}

impl FromStr for Token {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts = s.split(';');
        let mut username = None;
        let mut user_id = None;
        let mut client_id = None;
        let mut oauth_token = None;

        for part in parts {
            if part.is_empty() {
                continue;
            }

            let mut split = part.split('=');
            let key = split.next().ok_or(())?;
            let value = split.next().ok_or(())?;
            match key {
                "username" => {
                    username = Some(value.to_string());
                }
                "user_id" => {
                    user_id = Some(value.parse().map_err(|_| ())?);
                }
                "client_id" => {
                    client_id = Some(value.to_string());
                }
                "oauth_token" => {
                    oauth_token = Some(value.to_string());
                }
                _ => {
                    return Err(());
                }
            }
        }

        Ok(Token {
            username: username.ok_or(())?,
            user_id: user_id.ok_or(())?,
            client_id: client_id.ok_or(())?,
            oauth_token: oauth_token.ok_or(())?,
        })
    }
}

type ParticipantList = HashMap<String, BTreeSet<DateTime<Local>>>;

#[derive(Default)]
pub struct TwitchState {
    client: HelixClient<'static, reqwest::Client>,
    pub token: Option<UserToken>,
    pub participants: ParticipantList,
    last_participants_update: Option<Instant>,
    pub token_is_valid: bool,
    pub watched_channel: String,
}

impl TwitchState {
    pub fn token_is_valid(&self) -> bool {
        self.token_is_valid
    }

    pub fn client(&self) -> &HelixClient<'static, reqwest::Client> {
        &self.client
    }

    pub fn player_is_potential_stream_sniper(&self, name: &str, match_timestamp: DateTime<Local>) -> Option<HashMap<String, Vec<DateTime<Local>>>> {
        let mut results = HashMap::new();
        for (viewer_name, viewer_timestamps) in &self.participants {
            if levenshtein::levenshtein(&viewer_name, name) < 5 {
                let timestamps: Vec<_> = viewer_timestamps
                    .iter()
                    .cloned()
                    .filter(|timestamp| {
                        let delta = timestamp.signed_duration_since(match_timestamp);

                        delta.num_minutes() < 20 && delta.num_minutes() > -2
                    })
                    .collect();

                if !timestamps.is_empty() {
                    results.insert(viewer_name.to_string(), timestamps);
                }
            }
        }

        if results.is_empty() {
            None
        } else {
            Some(results)
        }
    }
}

pub async fn fetch_chatters(client: HelixClient<'static, reqwest::Client>, user_id: &UserId, token: &UserToken) -> anyhow::Result<Vec<String>> {
    let request = get_chatters::GetChattersRequest::new(user_id, &token.user_id);
    let response: Vec<helix::chat::Chatter> = client.req_get(request, token).await?.data;
    Ok(response.iter().map(|chatter| chatter.user_login.to_string()).collect())
}
