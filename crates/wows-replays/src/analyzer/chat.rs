use wowsunpack::data::Version;

use crate::analyzer::decoder::DecodedPacketPayload;
use crate::analyzer::decoder::PacketDecoder;
use crate::packet2::Packet;
use crate::types::AccountId;
use std::collections::HashMap;

use super::analyzer::Analyzer;

pub struct ChatLoggerBuilder;

impl Default for ChatLoggerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ChatLoggerBuilder {
    pub fn new() -> ChatLoggerBuilder {
        ChatLoggerBuilder
    }

    pub fn build(self, meta: &crate::ReplayMeta) -> Box<dyn Analyzer> {
        let version = Version::from_client_exe(&meta.clientVersionFromExe);
        Box::new(ChatLogger {
            usernames: HashMap::new(),
            packet_decoder: PacketDecoder::builder().version(version).build(),
        })
    }
}

pub struct ChatLogger {
    usernames: HashMap<AccountId, String>,
    packet_decoder: PacketDecoder<'static>,
}

impl Analyzer for ChatLogger {
    fn finish(&mut self) {}

    fn process(&mut self, packet: &Packet<'_, '_>) {
        let decoded = self.packet_decoder.decode(packet);
        match decoded.payload {
            DecodedPacketPayload::Chat { sender_id, audience, message, .. } => {
                println!(
                    "{}: {}: {} {}",
                    decoded.clock,
                    self.usernames.get(&sender_id).map(String::as_str).unwrap_or("<UNKNOWN_USERNAME>"),
                    audience,
                    message
                );
            }
            DecodedPacketPayload::VoiceLine { sender_id, message, .. } => {
                println!(
                    "{}: {}: voiceline {:#?}",
                    decoded.clock,
                    self.usernames.get(&sender_id).map(String::as_str).unwrap_or("<UNKNOWN_USERNAME>"),
                    message
                );
            }
            DecodedPacketPayload::OnArenaStateReceived { player_states: players, .. } => {
                // A sender id is either the account id (PLAYER_ID: chat from
                // 0.11.4 on, and voicelines in every version) or the avatar
                // entity id (ENTITY_ID: chat before 0.11.4). Key by both so this
                // one map resolves chat and voiceline across versions; the two
                // id spaces are disjoint, so there's no ambiguity.
                for player in players.iter() {
                    if player.meta_ship_id().raw() != 0 {
                        self.usernames.insert(player.meta_ship_id(), player.username().to_owned());
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
