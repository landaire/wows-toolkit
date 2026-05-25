use crate::icons;
use egui::Color32;
use egui::RichText;
use jiff::Timestamp;
use jiff::civil::DateTime;
use jiff::tz::TimeZone;
use language_tags::LanguageTag;
use std::path::Path;
use std::process::Command;
use thousands::Separable;
use tracing::debug;
use wows_replay_insights::ResolvedBuild;
use wows_replay_insights::build::wowssb;
use wows_replays::ReplayMeta;
use wows_replays::analyzer::battle_controller::Player;
use wows_replays::types::Relation;
use wowsunpack::data::Version;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::CrewSkill;

const TOOLKIT_REFERRER: &str = "landaire";

pub fn replay_timestamp(replay_meta: &ReplayMeta) -> Timestamp {
    const REPLAY_DATE_FORMAT: &str = "%d.%m.%Y %H:%M:%S";

    DateTime::strptime(REPLAY_DATE_FORMAT, &replay_meta.dateTime)
        .expect("failed to parse replay timestamp")
        .to_zoned(TimeZone::system())
        .expect("failed to convert DateTime to zoned time")
        .into()
}

pub fn separate_number<T: Separable>(num: T, locale: Option<&str>) -> String {
    let language: LanguageTag = locale
        .and_then(|locale| locale.replace('_', "-").parse().ok())
        .unwrap_or_else(|| LanguageTag::parse("en-US").unwrap());

    match language.primary_language() {
        "fr" => num.separate_with_spaces(),
        _ => num.separate_with_commas(),
    }
}

pub fn player_color_for_team_relation(relation: Relation) -> Color32 {
    if relation.is_self() {
        Color32::WHITE
    } else if relation.is_ally() {
        Color32::LIGHT_GREEN
    } else {
        Color32::LIGHT_RED
    }
}

pub fn build_wows_numbers_url(player: &Player) -> Option<String> {
    let state = player.initial_state();
    let realm = state.realm()?;
    Some(format!("https://{}.wows-numbers.com/player/{},{}", realm, state.db_id(), state.username()))
}

pub fn build_ship_config_url(player: &Player, metadata_provider: &GameMetadataProvider) -> Option<String> {
    let build = ResolvedBuild::from_player(player, metadata_provider, Version::default())?;
    let build_name = format!("replay_{}", player.initial_state().username());
    let url = wowssb::build_url(&build, &build_name, Some(TOOLKIT_REFERRER));
    Some(url)
}

pub fn build_short_ship_config_url(player: &Player, metadata_provider: &GameMetadataProvider) -> Option<String> {
    let build = ResolvedBuild::from_player(player, metadata_provider, Version::default())?;
    let build_name = format!("replay_{}", player.initial_state().username());
    let url = wowssb::build_short_url(&build, &build_name, Some(TOOLKIT_REFERRER));
    debug!("{}", url);
    Some(url)
}

pub fn colorize_captain_points(
    points: usize,
    skills: usize,
    highest_skill_tier: usize,
    num_tier_1_skills: usize,
    raw_skills: Option<Vec<&CrewSkill>>,
) -> (RichText, Option<String>) {
    let mut color = match points {
        0..=9 => Color32::LIGHT_RED,
        10..=12 => Color32::from_rgb(0xfc, 0xae, 0x1e), // orange
        13..=16 => Color32::YELLOW,
        _ => Color32::LIGHT_GREEN,
    };
    const NUM_SKILLS_IN_TIER: usize = 6;

    let mut has_dazzle = false;
    let mut has_ifa = false;
    if let Some(raw_skills) = &raw_skills {
        if raw_skills.iter().any(|skill| skill.skill_type() == crate::util::consts::DAZZLE_SKILL_ID) {
            has_dazzle = true;
        }
        if raw_skills.iter().any(|skill| skill.skill_type() == crate::util::consts::IFA_SKILL_ID) {
            has_ifa = true;
        }
    }

    let mut extra_icons = Vec::new();
    let mut extra_hover_text = Vec::new();
    if has_dazzle {
        extra_icons.push(icons::STAR);
        extra_hover_text.push("Dazzle");
    }
    if has_ifa {
        extra_icons.push(icons::SIREN);
        extra_hover_text.push("IFA");
    }

    let extra_icons = if !extra_icons.is_empty() { extra_icons.join("") } else { String::new() };

    if num_tier_1_skills == NUM_SKILLS_IN_TIER {
        color = Color32::LIGHT_RED;
        let default_text = "Player is playing tower defense with their skills";
        return (
            RichText::new(format!("{}{} {}pts ({} skills)", extra_icons, crate::icons::CASTLE_TURRET, points, skills))
                .color(color),
            if extra_hover_text.is_empty() {
                Some(default_text.to_string())
            } else {
                Some(format!("{} and has {}", default_text, extra_hover_text.join(", ")))
            },
        );
    } else if highest_skill_tier <= 2 && points >= 6 {
        color = Color32::LIGHT_RED;
        let default_text = "Player has no skills above tier 2";
        return (
            RichText::new(format!("{}{} {}pts ({} skills)", extra_icons, crate::icons::WARNING, points, skills))
                .color(color),
            if extra_hover_text.is_empty() {
                Some(default_text.to_string())
            } else {
                Some(format!("{} and has {}", default_text, extra_hover_text.join(", ")))
            },
        );
    }

    (
        RichText::new(format!("{extra_icons}{points}pts ({skills} skills)")).color(color),
        if extra_hover_text.is_empty() { None } else { Some(format!("Player has {}", extra_hover_text.join(", "))) },
    )
}

pub fn open_file_explorer(path: &Path) {
    #[allow(clippy::zombie_processes)]
    {
        #[cfg(target_os = "linux")]
        {
            Command::new("xdg-open").arg(path.parent().expect("failed to get replayparent directory")).spawn().unwrap();
        }

        #[cfg(target_os = "macos")]
        {
            Command::new("open").arg("--reveal").arg(path).spawn().unwrap();
        }

        #[cfg(target_os = "windows")]
        {
            Command::new("explorer.exe").arg("/select,").arg(path).spawn().unwrap();
        }
    }
}
