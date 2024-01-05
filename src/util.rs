use std::io::Write;
use log::{debug};
use egui::Color32;
use flate2::{write::DeflateEncoder, Compression};
use language_tags::LanguageTag;
use serde_json::json;
use thousands::Separable;
use wows_replays::{analyzer::battle_controller::VehicleEntity, game_params::GameParamProvider};

use crate::game_params::GameMetadataProvider;

pub fn separate_number<T: Separable>(num: T, locale: Option<&str>) -> String {
    let language: LanguageTag = locale.and_then(|locale| locale.parse().ok()).unwrap_or_else(|| LanguageTag::parse("en-US").unwrap());

    match language.primary_language() {
        "fr" => num.separate_with_spaces(),
        _ => num.separate_with_commas(),
    }
}

pub fn player_color_for_team_relation(relation: u32, is_dark_mode: bool) -> Color32 {
    match relation {
        0 => Color32::GOLD,
        1 => {
            if is_dark_mode {
                Color32::LIGHT_GREEN
            } else {
                Color32::DARK_GREEN
            }
        }
        _ => {
            if is_dark_mode {
                Color32::LIGHT_RED
            } else {
                Color32::DARK_RED
            }
        }
    }
}

pub fn build_wows_numbers_url(entity: &VehicleEntity) -> Option<String> {
    let player = entity.player()?;
    Some(format!("https://{}.wows-numbers.com/player/{},{}", player.realm(), player.db_id(), player.name()))
}

pub fn build_ship_config_url(entity: &VehicleEntity, metadata_provider: &GameMetadataProvider) -> String {
    let config = entity.props().ship_config();
    let player = entity.player().expect("entity has no player?");
    let ship = player.vehicle();

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
    parts[1] = config.units().iter().filter_map(|id| {
                Some(metadata_provider.game_param_by_id(*id)?.name().to_owned())
            }).collect::<Vec<_>>().join(",");

    // Upgrades
    parts[2] = config.modernization().iter().filter_map(|id| {
                Some(metadata_provider.game_param_by_id(*id)?.index().to_owned())
            }).collect::<Vec<_>>().join(",");
    // Captain
    parts[3] = entity.captain().map(|captain| captain.index()).unwrap_or("PCW001").to_string();

    // Skills
    parts[4] = entity.commander_skills_raw().iter().map(|x| x.to_string()).collect::<Vec<_>>().join(",");
    
    // Consumables
    parts[5] = config.abilities().iter().filter_map(|id| {
                Some(metadata_provider.game_param_by_id(*id as u32)?.index().to_owned())
            }).collect::<Vec<_>>().join(",");

    // Signals
    parts[6] = config.signals().iter().filter_map(|id| {
                Some(metadata_provider.game_param_by_id(*id as u32)?.name().to_owned())
            }).collect::<Vec<_>>().join(",");

    // Build Version
    parts[7] = "2".to_string();

    // Build Name
    parts[8] = format!("replay_{}", player.name());

    debug!("{:?}",parts.join(";"));

    let url = format!("https://app.wowssb.com/ship?shipIndexes={}&build={}&ref=landaire", ship.index(), parts.join(";"));

    url
}
