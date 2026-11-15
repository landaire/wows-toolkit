use std::collections::HashMap;

use serde::Serialize;
use wowsunpack::data::Version;

use super::analyzer::Analyzer;
use crate::analyzer::decoder::DecodedPacketPayload;
use crate::analyzer::decoder::PacketDecoder;
use crate::analyzer::decoder::VoiceLine;
use crate::packet2::Packet;
use crate::types::AccountId;
use crate::types::GameClock;

/// One chat message or voiceline, in the order the replay recorded it.
#[derive(Debug, Clone, Serialize)]
pub struct ChatEvent {
    pub clock: GameClock,

    /// `None` when no arena state named the sender before the message.
    pub sender: Option<String>,
    pub body: ChatBody,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatBody {
    Chat {
        /// The channel as the game names it, such as `battle_team`.
        audience: String,
        message: String,
    },
    VoiceLine(VoiceLine),
}

/// Collects every chat message and voiceline with its sender's name.
pub struct ChatLogger {
    usernames: HashMap<AccountId, String>,
    packet_decoder: PacketDecoder<'static>,
    events: Vec<ChatEvent>,
}

impl ChatLogger {
    pub fn new(version: Version) -> Self {
        Self {
            usernames: HashMap::new(),
            packet_decoder: PacketDecoder::builder().version(version).build(),
            events: Vec::new(),
        }
    }

    pub fn into_events(self) -> Vec<ChatEvent> {
        self.events
    }

    fn sender(&self, sender_id: &AccountId) -> Option<String> {
        self.usernames.get(sender_id).cloned()
    }
}

impl Analyzer for ChatLogger {
    fn finish(&mut self) {}

    fn process(&mut self, packet: &Packet<'_, '_>) {
        let decoded = self.packet_decoder.decode(packet);
        match decoded.payload {
            DecodedPacketPayload::Chat { sender_id, audience, message, .. } => {
                let event = ChatEvent {
                    clock: decoded.clock,
                    sender: self.sender(&sender_id),
                    body: ChatBody::Chat { audience: audience.to_owned(), message: message.to_owned() },
                };
                self.events.push(event);
            }
            DecodedPacketPayload::VoiceLine { sender_id, message, .. } => {
                let event = ChatEvent {
                    clock: decoded.clock,
                    sender: self.sender(&sender_id),
                    body: ChatBody::VoiceLine(message),
                };
                self.events.push(event);
            }
            DecodedPacketPayload::OnArenaStateReceived { player_states: players, .. } => {
                // A sender id is either the account id (PLAYER_ID: chat from
                // 0.11.4 on, and voicelines in every version) or the avatar
                // entity id (ENTITY_ID: chat before 0.11.4). Key by both so this
                // one map resolves chat and voiceline across versions; the two
                // id spaces are disjoint, so there's no ambiguity.
                for player in players.iter() {
                    if let Some(player_id) = player.player_id() {
                        self.usernames.insert(AccountId::from(player_id.raw()), player.username().to_owned());
                    }
                    if let Some(avatar) = player.avatar_id() {
                        self.usernames.insert(AccountId::from(avatar.raw()), player.username().to_owned());
                    }
                }
            }
            _ => {}
        }
    }
}
