//! A replay's chat log as the chat panel shows and copies it: the sender
//! coloured by team relation, the clan tag by its league colour, and the
//! message by channel, as the egui app's `build_replay_chat_content`
//! (`ui/replay_parser/mod.rs`) colours them.

use wows_replays::analyzer::battle_controller::ChatChannel;
use wows_replays::types::Relation;
use wows_toolkit_viewmodel::theme::Semantic;

use crate::model::ChatMessage;
use crate::model::ClanColor;
use crate::palette::relation_color;

/// The clipboard text for one message: `"[clan] sender (Channel): message"`,
/// or without the clan segment when the sender is clanless. The channel is
/// its `Debug` form, which is what the egui copy buttons write.
pub fn copy_text(message: &ChatMessage) -> String {
    match &message.clan_tag {
        Some(clan) => format!("[{clan}] {} ({:?}): {}", message.sender_name, message.channel, message.message),
        None => format!("{} ({:?}): {}", message.sender_name, message.channel, message.message),
    }
}

/// The whole log, one message per line, in the per-message copy format.
pub fn transcript(messages: &[ChatMessage]) -> String {
    messages.iter().map(copy_text).collect::<Vec<_>>().join("\n")
}

/// Packed `0xRRGGBB` for a sender: by relation, or the dim tone (egui's
/// `Color32::GRAY`) for a message with no resolvable relation.
pub fn sender_color(relation: Option<Relation>, semantic: &Semantic) -> u32 {
    match relation {
        Some(relation) => relation_color(relation, semantic),
        None => semantic.text_dim,
    }
}

/// Packed `0xRRGGBB` for a clan tag: the league colour the replay records, or
/// the player's own relation colour for an older replay that records none.
pub fn clan_color(color: ClanColor, semantic: &Semantic) -> u32 {
    match color {
        ClanColor::League(rgb) => rgb,
        ClanColor::Relation(relation) => relation_color(relation, semantic),
    }
}

/// Packed `0xRRGGBB` for a message body, by channel: egui's `Division =>
/// GOLD, Global => WHITE, Team => LIGHT_GREEN, _ => ORANGE`.
pub fn channel_color(channel: &ChatChannel, semantic: &Semantic) -> u32 {
    match channel {
        ChatChannel::Division => semantic.chat_division,
        ChatChannel::Global => semantic.text_strong,
        ChatChannel::Team => semantic.chat_team,
        ChatChannel::System | ChatChannel::Unknown(_) => semantic.chat_other,
    }
}

#[cfg(test)]
mod tests {
    use wows_replays::types::GameClock;
    use wows_toolkit_viewmodel::theme::DARK_SEMANTIC;
    use wows_toolkit_viewmodel::theme::LIGHT_SEMANTIC;

    use super::*;

    fn message(sender_relation: Option<Relation>, channel: ChatChannel, clan_tag: Option<&str>) -> ChatMessage {
        ChatMessage {
            clock: GameClock(0.0),
            sender_relation,
            sender_name: "Player".to_owned(),
            channel,
            message: "hello".to_owned(),
            clan_tag: clan_tag.map(str::to_owned),
            clan_color: clan_tag.map(|_| ClanColor::League(0x3399ff)),
        }
    }

    #[test]
    fn sender_color_matches_relation_palette() {
        for semantic in [&DARK_SEMANTIC, &LIGHT_SEMANTIC] {
            assert_eq!(sender_color(None, semantic), semantic.text_dim);
            for relation in [0, 1, 2] {
                let relation = Relation::new(relation);
                assert_eq!(sender_color(Some(relation), semantic), relation_color(relation, semantic));
            }
        }
    }

    #[test]
    fn clan_color_prefers_the_league_color() {
        let semantic = &DARK_SEMANTIC;

        assert_eq!(clan_color(ClanColor::League(0x3399ff), semantic), 0x3399ff);
        assert_eq!(clan_color(ClanColor::Relation(Relation::new(2)), semantic), semantic.loss);
    }

    #[test]
    fn channel_color_matches_palette() {
        let semantic = &DARK_SEMANTIC;

        assert_eq!(channel_color(&ChatChannel::Division, semantic), semantic.chat_division);
        assert_eq!(channel_color(&ChatChannel::Global, semantic), semantic.text_strong);
        assert_eq!(channel_color(&ChatChannel::Team, semantic), semantic.chat_team);
        assert_eq!(channel_color(&ChatChannel::System, semantic), semantic.chat_other);
        assert_eq!(channel_color(&ChatChannel::Unknown("x".to_owned()), semantic), semantic.chat_other);
    }

    #[test]
    fn copy_text_includes_clan_tag_when_present() {
        let message = message(Some(Relation::new(1)), ChatChannel::Team, Some("WTK"));

        assert_eq!(copy_text(&message), "[WTK] Player (Team): hello");
    }

    #[test]
    fn copy_text_omits_clan_segment_when_clanless() {
        let message = message(Some(Relation::new(1)), ChatChannel::Team, None);

        assert_eq!(copy_text(&message), "Player (Team): hello");
    }

    #[test]
    fn copy_text_formats_unknown_channel_with_its_payload() {
        let message = message(None, ChatChannel::Unknown("weird".to_owned()), None);

        assert_eq!(copy_text(&message), "Player (Unknown(\"weird\")): hello");
    }

    #[test]
    fn the_transcript_is_one_line_per_message() {
        let messages =
            [message(Some(Relation::new(1)), ChatChannel::Team, Some("WTK")), message(None, ChatChannel::Global, None)];

        assert_eq!(transcript(&messages), "[WTK] Player (Team): hello\nPlayer (Global): hello");
    }
}
