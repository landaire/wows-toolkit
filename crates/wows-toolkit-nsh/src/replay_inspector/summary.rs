//! What the TSX draws above and beside an open replay's table: the header,
//! the match-context line and the chat log, read from its document.
//!
//! Colors are resolved for one appearance when the summary is built; the TSX
//! asks for a new summary when it opens the replay again.

use nsh_core::prelude::JsObject;
use nsh_core::prelude::PropEnum;
use wows_replays::analyzer::battle_controller::BattleResult;
use wows_replays::analyzer::battle_controller::ChatChannel;
use wows_toolkit_replay_data::chat;
use wows_toolkit_replay_data::model::ChatMessage;
use wows_toolkit_replay_data::model::separate_number;
use wows_toolkit_replay_data::summary;
use wows_toolkit_viewmodel::personal_rating;
use wows_toolkit_viewmodel::theme::Semantic;
use wows_toolkit_viewmodel::theme::semantic_for;

use crate::replay_inspector::constants::BuildNumber;
use crate::replay_inspector::document::ReplayDocId;
use crate::replay_inspector::document::ReplayDocument;

/// How the battle ended for the recording player.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PropEnum)]
pub enum Outcome {
    Win,
    Loss,
    Draw,
}

/// The channel a chat message was sent in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PropEnum)]
pub enum ChatChannelKind {
    Division,
    Global,
    Team,
    System,
    Unknown,
}

/// The single-battle PR badge.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct PersonalRatingBadge {
    /// `PR: 1150 (Average)`.
    pub label: String,
    /// The band's text color, `#rrggbb`.
    pub color: String,
    /// The band's hue at the chip tint, `#rrggbbaa`.
    pub tint: String,
}

/// Each side's damage, grouped as every other number in the app.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct TeamDamageText {
    pub friendly: String,
    pub enemy: String,
    pub total: String,
}

/// One chat message, colored as the chat panel draws it.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct ChatLine {
    /// The clan tag without brackets.
    pub clan: Option<String>,
    pub clan_color: Option<String>,
    pub sender: String,
    /// By team relation, dim without one.
    pub sender_color: String,
    pub channel: ChatChannelKind,
    pub message: String,
    /// By channel.
    pub message_color: String,
    /// `[clan] sender (Channel): message`, what the copy button writes.
    pub copy_text: String,
}

/// The build a replay's results were read with a mapping that does not fit.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct ConstantsMismatchInfo {
    pub build: BuildNumber,
    pub version: String,
}

/// An open replay as its tab shows it.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct ReplaySummary {
    pub document: ReplayDocId,
    pub path: String,
    pub file_name: String,
    /// `<self ship> - <map>`, or the map alone.
    pub title: String,
    /// The recording player's nation flag as a PNG `data:` URL.
    pub nation_flag: Option<String>,
    /// The recording player's ship class icon as an SVG `data:` URL.
    pub class_icon: Option<String>,
    /// `None` while the replay has no battle result yet.
    pub outcome: Option<Outcome>,
    /// `None` without a rating table, a self row or known damage.
    pub personal_rating: Option<PersonalRatingBadge>,
    pub incomplete_results: bool,
    pub match_context: Vec<String>,
    pub team_damage: TeamDamageText,
    pub chat: Vec<ChatLine>,
    pub has_raw_results: bool,
    pub has_mapped_results: bool,
    pub is_test_ship: bool,
    pub alts: Vec<String>,
    pub constants_mismatch: Option<ConstantsMismatchInfo>,
    /// Why the automatic export of this battle failed, when it did.
    pub export_status: Option<String>,
}

/// Summarizes `document`, registered as `id`, in the colors of the dark or
/// light appearance.
pub fn summarize(
    id: ReplayDocId,
    document: &ReplayDocument,
    dark: bool,
    export_status: Option<String>,
) -> ReplaySummary {
    let model = &document.model;
    let semantic = semantic_for(dark);
    let self_row = summary::self_row(model);

    let outcome = model.battle_result.map(|result| match result {
        BattleResult::Win(_) => Outcome::Win,
        BattleResult::Loss(_) => Outcome::Loss,
        BattleResult::Draw => Outcome::Draw,
    });
    let personal_rating = self_row.and_then(|row| row.personal_rating.as_ref()).map(|rating| PersonalRatingBadge {
        label: summary::personal_rating_label(rating),
        color: hex(personal_rating::chip_text(rating.category, dark)),
        tint: hex_with_alpha(personal_rating::chip_hue(rating.category), personal_rating::CHIP_TINT_ALPHA),
    });
    let damage = summary::team_damage(model);

    ReplaySummary {
        document: id,
        path: document.path.display().to_string(),
        // A document is read from a file, so its path always ends in a name.
        file_name: document.path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default(),
        title: summary::title(model),
        nation_flag: document.title_icons.nation_flag.clone(),
        class_icon: document.title_icons.class_icon.clone(),
        outcome,
        personal_rating,
        incomplete_results: document.raw_results_json.is_none(),
        match_context: summary::match_context_fields(&model.context),
        team_damage: TeamDamageText {
            friendly: separate_number(damage.friendly),
            enemy: separate_number(damage.enemy),
            total: separate_number(damage.total()),
        },
        chat: model.chat.iter().map(|message| chat_line(message, semantic)).collect(),
        has_raw_results: document.raw_results_json.is_some(),
        has_mapped_results: document.mapped_results_json.is_some(),
        is_test_ship: self_row.is_some_and(|row| row.is_test_ship),
        alts: document.alts.iter().map(|alt| alt.display().to_string()).collect(),
        constants_mismatch: document
            .constants_mismatch
            .as_ref()
            .map(|mismatch| ConstantsMismatchInfo { build: mismatch.build, version: mismatch.version.clone() }),
        export_status,
    }
}

fn chat_line(message: &ChatMessage, semantic: &Semantic) -> ChatLine {
    let channel = match message.channel {
        ChatChannel::Division => ChatChannelKind::Division,
        ChatChannel::Global => ChatChannelKind::Global,
        ChatChannel::Team => ChatChannelKind::Team,
        ChatChannel::System => ChatChannelKind::System,
        ChatChannel::Unknown(_) => ChatChannelKind::Unknown,
    };
    // The tag is drawn only with its color, which the model gives exactly
    // when it gives the tag.
    let (clan, clan_color) = match (&message.clan_tag, message.clan_color) {
        (Some(tag), Some(color)) => (Some(tag.clone()), Some(hex(chat::clan_color(color, semantic)))),
        _ => (None, None),
    };

    ChatLine {
        clan,
        clan_color,
        sender: message.sender_name.clone(),
        sender_color: hex(chat::sender_color(message.sender_relation, semantic)),
        channel,
        message: message.message.clone(),
        message_color: hex(chat::channel_color(&message.channel, semantic)),
        copy_text: chat::copy_text(message),
    }
}

fn hex(rgb: u32) -> String {
    format!("#{rgb:06x}")
}

fn hex_with_alpha(rgb: u32, alpha: u8) -> String {
    format!("#{rgb:06x}{alpha:02x}")
}

#[cfg(all(test, feature = "test-support"))]
mod tests {
    use std::path::Path;
    use std::path::PathBuf;

    use wows_toolkit_viewmodel::personal_rating::PersonalRatingResult;
    use wows_toolkit_viewmodel::theme::DARK_SEMANTIC;

    use super::*;
    use crate::replay_inspector::test_support::sample_document;

    fn summarize_sample(edit: impl FnOnce(&mut ReplayDocument)) -> ReplaySummary {
        let mut document = sample_document(Path::new("/replays/sample.wowsreplay"), &[]);
        edit(&mut document);

        summarize(ReplayDocId::new(7), &document, true, None)
    }

    #[test]
    fn the_header_names_the_ship_the_map_and_the_outcome() {
        let summary = summarize_sample(|document| document.model.battle_result = Some(BattleResult::Win(0)));

        assert_eq!(summary.document, ReplayDocId::new(7));
        assert_eq!(summary.file_name, "sample.wowsreplay");
        assert_eq!(summary.title, "Shimakaze - Fault Line");
        assert_eq!(summary.outcome, Some(Outcome::Win));
        assert!(!summary.incomplete_results);
        assert!(summary.has_raw_results && summary.has_mapped_results);
        assert!(!summary.is_test_ship);
    }

    #[test]
    fn a_replay_without_results_has_no_outcome_and_says_so() {
        let summary = summarize_sample(|document| {
            document.model.battle_result = None;
            document.raw_results_json = None;
        });

        assert_eq!(summary.outcome, None);
        assert!(summary.incomplete_results);
        assert!(!summary.has_raw_results);
    }

    #[test]
    fn the_pr_badge_uses_the_chip_wording_and_colors() {
        let summary = summarize_sample(|document| {
            let row = document.model.rows.iter_mut().find(|row| row.is_self).expect("the sample has a self row");
            row.personal_rating = Some(PersonalRatingResult::new(1150.0));
        });

        let badge = summary.personal_rating.expect("a rated self row gets a badge");
        let category = PersonalRatingResult::new(1150.0).category;
        assert_eq!(badge.label, "PR: 1150 (Average)");
        assert_eq!(badge.color, hex(personal_rating::chip_text(category, true)));
        assert_eq!(
            badge.tint,
            format!("{}{:02x}", hex(personal_rating::chip_hue(category)), personal_rating::CHIP_TINT_ALPHA)
        );
    }

    #[test]
    fn an_unrated_self_row_shows_no_badge() {
        let summary = summarize_sample(|document| {
            for row in &mut document.model.rows {
                row.personal_rating = None;
            }
        });

        assert_eq!(summary.personal_rating, None);
    }

    #[test]
    fn team_damage_is_grouped() {
        let summary = summarize_sample(|_| {});

        assert_eq!(summary.team_damage.friendly, "344,800");
        assert_eq!(summary.team_damage.enemy, "337,700");
        assert_eq!(summary.team_damage.total, "682,500");
    }

    #[test]
    fn chat_lines_carry_their_copy_text_and_colors() {
        let summary = summarize_sample(|_| {});

        let clanned = &summary.chat[0];
        assert_eq!(clanned.copy_text, "[WTK] You (Division): focus the Yamato");
        assert_eq!(clanned.clan.as_deref(), Some("WTK"));
        assert_eq!(clanned.clan_color.as_deref(), Some("#3399ff"));
        assert_eq!(clanned.channel, ChatChannelKind::Division);
        assert_eq!(clanned.message_color, hex(DARK_SEMANTIC.chat_division));
        assert_eq!(clanned.sender_color, hex(DARK_SEMANTIC.text_strong));

        let clanless = &summary.chat[1];
        assert_eq!(clanless.copy_text, "Ally_Bravo (Team): cap is contested");
        assert_eq!(clanless.clan, None);
        assert_eq!(clanless.sender_color, hex(DARK_SEMANTIC.win));

        let system = &summary.chat[3];
        assert_eq!(system.channel, ChatChannelKind::System);
        assert_eq!(system.sender_color, hex(DARK_SEMANTIC.text_dim));
    }

    #[test]
    fn an_unknown_channel_keeps_its_payload_in_the_copy_text() {
        let summary = summarize_sample(|document| {
            document.model.chat[1].channel = ChatChannel::Unknown("weird".to_owned());
        });

        assert_eq!(summary.chat[1].channel, ChatChannelKind::Unknown);
        assert_eq!(summary.chat[1].copy_text, "Ally_Bravo (Unknown(\"weird\")): cap is contested");
    }

    #[test]
    fn alts_and_a_mapping_mismatch_are_reported() {
        let summary = summarize_sample(|document| {
            document.alts = vec![PathBuf::from("/replays/other.wowsreplay")];
            document.constants_mismatch = Some(crate::replay_inspector::document::ConstantsMismatch {
                build: BuildNumber(9_000_000),
                version: "14.1.0".to_owned(),
            });
        });

        assert_eq!(summary.alts, ["/replays/other.wowsreplay"]);
        assert_eq!(
            summary.constants_mismatch,
            Some(ConstantsMismatchInfo { build: BuildNumber(9_000_000), version: "14.1.0".to_owned() })
        );
    }
}
