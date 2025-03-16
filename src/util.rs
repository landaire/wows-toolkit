use crate::icons;
use egui::Color32;
use egui::RichText;
use flate2::Compression;
use flate2::write::DeflateEncoder;
use language_tags::LanguageTag;
use serde_json::json;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use thousands::Separable;
use tracing::debug;
use wows_replays::analyzer::battle_controller::VehicleEntity;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::CrewSkill;
use wowsunpack::game_params::types::GameParamProvider;

pub fn separate_number<T: Separable>(num: T, locale: Option<&str>) -> String {
    let language: LanguageTag = locale.and_then(|locale| locale.parse().ok()).unwrap_or_else(|| LanguageTag::parse("en-US").unwrap());

    match language.primary_language() {
        "fr" => num.separate_with_spaces(),
        _ => num.separate_with_commas(),
    }
}

pub fn player_color_for_team_relation(relation: u32) -> Color32 {
    match relation {
        0 => Color32::WHITE,
        1 => Color32::LIGHT_GREEN,
        _ => Color32::LIGHT_RED,
    }
}

pub fn build_tomato_gg_url(entity: &VehicleEntity) -> Option<String> {
    let player = entity.player()?;
    Some(format!("https://tomato.gg/wows/stats/{}/{}-{}", player.realm(), player.name(), player.db_id()))
}

pub fn build_wows_numbers_url(entity: &VehicleEntity) -> Option<String> {
    let player = entity.player()?;
    Some(format!("https://{}.wows-numbers.com/player/{},{}", player.realm(), player.db_id(), player.name()))
}

pub fn build_ship_config_url(entity: &VehicleEntity, metadata_provider: &GameMetadataProvider) -> String {
    let config = entity.props().ship_config();
    let player = entity.player().expect("entity has no player?");
    let ship = player.vehicle();

    eprintln!("{:#?}", entity.commander_skills());

    let json = json!({
        "BuildName": format!("replay_{}", player.name()),

        "ShipIndex": ship.index(),

        "Nation": ship.nation().replace('_', ""),

        "Modules": config.units().iter().filter_map(|id| {
            Some(metadata_provider.game_param_by_id(*id)?.index().to_owned())
        }).collect::<Vec<_>>(),

        "Upgrades": config.modernization().iter().filter_map(|id| {
            Some(metadata_provider.game_param_by_id(*id)?.index().to_owned())
        }).collect::<Vec<_>>(),

        // If no captain is present, we use the default captain (wowssb does not allow for no captain to be used)
        "Captain": entity.captain().map(|captain| captain.index()).unwrap_or("PCW001"),

        "Skills": entity.commander_skills_raw(),

        "Consumables": config.abilities().iter().filter_map(|id| {
            Some(metadata_provider.game_param_by_id(*id)?.index().to_owned())
        }).collect::<Vec<_>>(),

        "Signals": config.signals().iter().filter_map(|id| {
            Some(metadata_provider.game_param_by_id(*id)?.index().to_owned())
        }).collect::<Vec<_>>(),

        "BuildVersion": 2
    });

    let json_blob = serde_json::to_string(&json).expect("failed to serialize ship config");
    let mut deflated_json = Vec::new();
    {
        let mut encoder = DeflateEncoder::new(&mut deflated_json, Compression::best());
        encoder.write_all(json_blob.as_bytes()).expect("failed to deflate JSON blob");
    }
    let encoded_data = data_encoding::BASE64.encode(&deflated_json);
    let encoded_data = encoded_data.replace('/', "%2F").replace('+', "%2B");
    let url = format!("https://app.wowssb.com/ship?shipIndexes={}&build={}&ref=landaire", ship.index(), encoded_data);

    url
}

pub fn build_short_ship_config_url(entity: &VehicleEntity, metadata_provider: &GameMetadataProvider) -> String {
    let config = entity.props().ship_config();
    let player = entity.player().expect("entity has no player?");
    let ship = player.vehicle();
    let mut parts: Vec<String> = vec![String::new(); 9];

    // Ship
    parts[0] = ship.index().to_string();

    // Modules
    parts[1] = config.units().iter().filter_map(|id| Some(metadata_provider.game_param_by_id(*id)?.index().to_owned())).collect::<Vec<_>>().join(",");

    // Upgrades
    parts[2] = config.modernization().iter().filter_map(|id| Some(metadata_provider.game_param_by_id(*id)?.index().to_owned())).collect::<Vec<_>>().join(",");
    // Captain
    parts[3] = entity.captain().map(|captain| captain.index()).unwrap_or("PCW001").to_string();

    // Skills
    parts[4] = entity.commander_skills_raw().iter().map(|x| x.to_string()).collect::<Vec<_>>().join(",");

    // Consumables
    parts[5] = config.abilities().iter().filter_map(|id| Some(metadata_provider.game_param_by_id(*id)?.index().to_owned())).collect::<Vec<_>>().join(",");

    // Signals
    parts[6] = config.signals().iter().filter_map(|id| Some(metadata_provider.game_param_by_id(*id)?.index().to_owned())).collect::<Vec<_>>().join(",");

    // Build Version
    parts[7] = "2".to_string();

    // Build Name
    parts[8] = format!("replay_{}", player.name());

    debug!("{:?}", parts.join(";"));

    let url = format!("https://app.wowssb.com/ship?shipIndexes={}&build={}&ref=landaire", ship.index(), parts.join(";"));

    url
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
        if raw_skills.iter().any(|skill| skill.skill_type() == crate::consts::DAZZLE_SKILL_ID) {
            has_dazzle = true;
        }
        if raw_skills.iter().any(|skill| skill.skill_type() == crate::consts::IFA_SKILL_ID) {
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
            RichText::new(format!("{}{} {}pts ({} skills)", extra_icons, crate::icons::CASTLE_TURRET, points, skills)).color(color),
            if extra_hover_text.is_empty() { Some(default_text.to_string()) } else { Some(format!("{} and has {}", default_text, extra_hover_text.join(", "))) },
        );
    } else if highest_skill_tier <= 2 && points >= 6 {
        color = Color32::LIGHT_RED;
        let default_text = "Player has no skills above tier 2";
        return (
            RichText::new(format!("{}{} {}pts ({} skills)", extra_icons, crate::icons::WARNING, points, skills)).color(color),
            if extra_hover_text.is_empty() { Some(default_text.to_string()) } else { Some(format!("{} and has {}", default_text, extra_hover_text.join(", "))) },
        );
    }

    (
        RichText::new(format!("{}{}pts ({} skills)", extra_icons, points, skills)).color(color),
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
