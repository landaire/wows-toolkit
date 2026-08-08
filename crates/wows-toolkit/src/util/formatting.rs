use crate::data::match_stats::Region;
use crate::icons;
use crate::ui::theme::semantic::semantic;
use egui::Color32;
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
use wows_replays::types::AccountId;
use wows_replays::types::Relation;
use wowsunpack::data::Version;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::CrewSkill;
use wowsunpack::game_params::types::KnownCrewSkill;

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

pub fn player_color_for_team_relation(relation: Relation, visuals: &egui::Visuals) -> Color32 {
    crate::ui::replay_parser::PlayerTint::from_relation(relation).color(visuals)
}

pub fn build_wows_numbers_url(player: &Player) -> Option<String> {
    let state = player.initial_state();
    let realm = state.realm()?;
    Some(format!("https://{}.wows-numbers.com/player/{},{}", realm, state.db_id(), state.username()))
}

/// A player's wows-numbers page. The realm is a subdomain there, so a region
/// this client does not support has no page and cannot be linked.
pub fn wows_numbers_player_url(region: Region, account_id: AccountId, name: &str) -> String {
    format!("https://{}.wows-numbers.com/player/{},{}", region.as_wire(), account_id.0, name)
}

/// A player's shipbuilds page. Only the `EU` form of the region segment is
/// confirmed against the live site; `NA` and `ASIA` follow the same shape.
pub fn shipbuilds_player_url(region: Region, account_id: AccountId, name: &str) -> String {
    format!("https://shipbuilds.com/player/{}/{}/{}", region.as_url_segment(), account_id.0, name)
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

/// How a player's captain-skill point count reads at a glance. Resolved to a
/// colour at draw time so the scoreboard follows the active theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub enum SkillTier {
    Poor,
    Mediocre,
    Good,
    Great,
}

impl SkillTier {
    pub fn color(self, visuals: &egui::Visuals) -> Color32 {
        let sem = semantic(visuals);
        match self {
            Self::Poor => sem.loss,
            Self::Mediocre => sem.warn,
            Self::Good => sem.notice,
            Self::Great => sem.win,
        }
    }
}

pub fn colorize_captain_points(
    points: usize,
    skills: usize,
    highest_skill_tier: usize,
    num_tier_1_skills: usize,
    raw_skills: Option<Vec<&CrewSkill>>,
) -> (SkillTier, String, Option<String>) {
    let mut tier = match points {
        0..=9 => SkillTier::Poor,
        10..=12 => SkillTier::Mediocre,
        13..=16 => SkillTier::Good,
        _ => SkillTier::Great,
    };
    const NUM_SKILLS_IN_TIER: usize = 6;

    let mut has_dazzle = false;
    let mut has_ifa = false;
    if let Some(raw_skills) = &raw_skills {
        for skill in raw_skills {
            match KnownCrewSkill::recognize(skill.internal_name(), skill.skill_type()).known() {
                Some(KnownCrewSkill::Dazzle) => has_dazzle = true,
                Some(KnownCrewSkill::IncomingFireAlert) => has_ifa = true,
                _ => {}
            }
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
        tier = SkillTier::Poor;
        let default_text = "Player is playing tower defense with their skills";
        return (
            tier,
            format!("{}{} {}pts ({} skills)", extra_icons, crate::icons::CASTLE_TURRET, points, skills),
            if extra_hover_text.is_empty() {
                Some(default_text.to_string())
            } else {
                Some(format!("{} and has {}", default_text, extra_hover_text.join(", ")))
            },
        );
    } else if highest_skill_tier <= 2 && points >= 6 {
        tier = SkillTier::Poor;
        let default_text = "Player has no skills above tier 2";
        return (
            tier,
            format!("{}{} {}pts ({} skills)", extra_icons, crate::icons::WARNING, points, skills),
            if extra_hover_text.is_empty() {
                Some(default_text.to_string())
            } else {
                Some(format!("{} and has {}", default_text, extra_hover_text.join(", ")))
            },
        );
    }

    (
        tier,
        format!("{extra_icons}{points}pts ({skills} skills)"),
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
            let mut command = Command::new("explorer.exe");
            command.arg("/select,").arg(path);
            crate::hardening::prepare_child(&mut command).spawn().unwrap();
        }
    }
}

/// Open a directory in the OS file manager, showing its contents.
pub fn open_directory(path: &Path) {
    #[allow(clippy::zombie_processes)]
    {
        #[cfg(target_os = "linux")]
        {
            let _ = Command::new("xdg-open").arg(path).spawn();
        }

        #[cfg(target_os = "macos")]
        {
            let _ = Command::new("open").arg(path).spawn();
        }

        #[cfg(target_os = "windows")]
        {
            let mut command = Command::new("explorer.exe");
            command.arg(path);
            let _ = crate::hardening::prepare_child(&mut command).spawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wows_numbers_url_is_scoped_by_its_regional_subdomain() {
        let url = wows_numbers_player_url(Region::Na, AccountId(1_003_924_023), "G4ngB4r3ng");

        assert_eq!(url, "https://na.wows-numbers.com/player/1003924023,G4ngB4r3ng");
    }

    #[test]
    fn a_shipbuilds_url_uppercases_its_region() {
        let url = shipbuilds_player_url(Region::Eu, AccountId(533_130_923), "G4ngB4r3ng");

        assert_eq!(url, "https://shipbuilds.com/player/EU/533130923/G4ngB4r3ng");
    }
}
