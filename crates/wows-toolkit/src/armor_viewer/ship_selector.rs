use std::collections::HashMap;

use wowsunpack::data::ResourceLoader;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::Species;

/// Pre-built catalog of all ships, organized for the tree selector.
pub struct ShipCatalog {
    /// Nations sorted alphabetically, each containing sorted classes.
    pub nations: Vec<NationGroup>,
}

pub struct NationGroup {
    pub nation: String,
    pub classes: Vec<ClassGroup>,
}

pub struct ClassGroup {
    pub species: Species,
    pub ships: Vec<ShipEntry>,
}

#[derive(Clone)]
pub struct ShipEntry {
    pub param_index: String,
    pub display_name: String,
    /// Lowercased, ASCII-folded display name for search matching.
    pub search_name: String,
    pub tier: u32,
}

/// Canonical display order for ship classes.
fn species_order(s: &Species) -> u32 {
    match s {
        Species::Destroyer => 0,
        Species::Cruiser => 1,
        Species::Battleship => 2,
        Species::AirCarrier => 3,
        Species::Submarine => 4,
        _ => 5,
    }
}

/// Roman numeral for tier display.
pub fn tier_roman(tier: u32) -> &'static str {
    match tier {
        1 => "I",
        2 => "II",
        3 => "III",
        4 => "IV",
        5 => "V",
        6 => "VI",
        7 => "VII",
        8 => "VIII",
        9 => "IX",
        10 => "X",
        11 => "XI",
        _ => "?",
    }
}

/// Species display name.
pub fn species_name(s: &Species) -> &'static str {
    match s {
        Species::Destroyer => "Destroyer",
        Species::Cruiser => "Cruiser",
        Species::Battleship => "Battleship",
        Species::AirCarrier => "Aircraft Carrier",
        Species::Submarine => "Submarine",
        Species::Auxiliary => "Auxiliary",
        _ => "Other",
    }
}

const SHIP_SPECIES: &[Species] = &[
    Species::AirCarrier,
    Species::Battleship,
    Species::Cruiser,
    Species::Destroyer,
    Species::Submarine,
    Species::Auxiliary,
];

impl ShipCatalog {
    /// Build from GameMetadataProvider. Filters to only ship species.
    pub fn build(metadata: &GameMetadataProvider) -> Self {
        let mut nation_map: HashMap<String, HashMap<Species, Vec<ShipEntry>>> = HashMap::new();

        for param in metadata.params() {
            let species = match param.species() {
                Some(r) => match r.known() {
                    Some(s) if SHIP_SPECIES.contains(s) => *s,
                    _ => continue,
                },
                None => continue,
            };

            let vehicle = match param.vehicle() {
                Some(v) => v,
                None => continue,
            };

            // Skip clan rental ships (duplicates of real ships).
            if vehicle.group() == "clan" {
                continue;
            }

            let tier = vehicle.level();
            let nation = param.nation().to_string();

            let display_name = metadata.localized_name_from_param(param).unwrap_or_else(|| param.name().to_string());

            let search_name = unidecode::unidecode(&display_name).to_lowercase();
            let entry = ShipEntry { param_index: param.index().to_string(), display_name, search_name, tier };

            nation_map.entry(nation).or_default().entry(species).or_default().push(entry);
        }

        let mut nations: Vec<NationGroup> = nation_map
            .into_iter()
            .map(|(nation, class_map)| {
                let mut classes: Vec<ClassGroup> = class_map
                    .into_iter()
                    .map(|(species, mut ships)| {
                        ships.sort_by(|a, b| a.tier.cmp(&b.tier).then(a.display_name.cmp(&b.display_name)));
                        ClassGroup { species, ships }
                    })
                    .collect();
                classes.sort_by_key(|c| species_order(&c.species));
                NationGroup { nation, classes }
            })
            .collect();

        nations.sort_by(|a, b| a.nation.cmp(&b.nation));

        ShipCatalog { nations }
    }
}
