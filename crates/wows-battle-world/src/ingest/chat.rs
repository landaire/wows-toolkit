//! Ingestion handler for onChatMessage packets.

use bevy_ecs::world::World;
use tracing::debug;
use wows_replays::analyzer::battle_controller::ChatChannel;
use wows_replays::analyzer::battle_controller::GameMessage;
use wows_replays::analyzer::decoder::ChatMessageExtra;
use wows_replays::analyzer::decoder::chat_sender_is_account_id;
use wows_replays::types::AccountId;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wows_replays::types::Relation;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::Version;

use crate::resources::ChatLog;
use crate::resources::PlayerIndex;
use crate::resources::ReplayVehicles;

/// Handle one onChatMessage RPC, mirroring BattleController::handle_chat_message.
pub fn handle_chat_message<G: ResourceLoader>(
    entity_id: EntityId,
    sender_id: AccountId,
    audience: &str,
    message: &str,
    _extra_data: Option<ChatMessageExtra>,
    clock: GameClock,
    world: &mut World,
    resources: &G,
    version: Version,
) {
    // System messages have sender_id 0.
    if sender_id.raw() == 0 {
        return;
    }

    let channel = match audience {
        "battle_common" => ChatChannel::Global,
        "battle_team" => ChatChannel::Team,
        "battle_prebattle" => ChatChannel::Division,
        other => ChatChannel::Unknown(other.to_string()),
    };

    let by_account = chat_sender_is_account_id(version);

    // Resolve the player from PlayerIndex using the same two-era lookup as the original.
    let player = world
        .resource::<PlayerIndex>()
        .0
        .values()
        .find(|p| {
            let state = p.initial_state();
            if by_account {
                state.meta_ship_id() == sender_id
            } else {
                state.avatar_id().is_some_and(|avatar| AccountId::from(avatar.raw()) == sender_id)
            }
        })
        .cloned();

    // Metadata vehicles are keyed by account id; only useful in the PLAYER_ID era.
    let meta_vehicle = if by_account && player.is_none() {
        world.resource::<ReplayVehicles>().0.iter().find(|v| v.id == sender_id).cloned()
    } else {
        None
    };

    if player.is_none() && meta_vehicle.is_none() {
        debug!(
            sender = sender_id.raw(),
            by_account,
            "chat sender did not match any player"
        );
    }

    let resolved_name = player
        .as_ref()
        .and_then(|p| {
            let name = p.initial_state().username();
            if name.is_empty() { None } else { Some(name.to_owned()) }
        })
        .or_else(|| {
            meta_vehicle.as_ref().and_then(|v| {
                if v.name.is_empty() { None } else { Some(v.name.clone()) }
            })
        });
    let sender_relation: Option<Relation> = player
        .as_ref()
        .map(|p| p.relation())
        .or_else(|| meta_vehicle.as_ref().map(|v| Relation::new(v.relation)));

    let is_bot = player.as_ref().map(|p| p.is_bot()).unwrap_or(true);

    let sender_name = match resolved_name {
        Some(name) if is_bot => translate_ids(resources, &name),
        Some(name) => name,
        None => "Unknown".to_owned(),
    };
    let message_text = if is_bot { translate_ids(resources, message) } else { message.to_string() };

    debug!("chat message from {sender_name} in {channel:?}: {message_text}");

    world.resource_mut::<ChatLog>().0.push(GameMessage {
        clock,
        sender_relation,
        sender_name,
        channel,
        message: message_text,
        entity_id,
        player,
    });
}

fn translate_ids<G: ResourceLoader>(resources: &G, text: &str) -> String {
    if text.starts_with("IDS_") {
        resources.localized_name_from_id(text).unwrap_or_else(|| text.to_string())
    } else {
        text.to_string()
    }
}
