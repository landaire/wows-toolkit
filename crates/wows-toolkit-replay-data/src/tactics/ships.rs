//! The ships a board can place, looked up by name.

use std::collections::HashMap;

use wowsunpack::data::ResourceLoader;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::Species;

/// How many ship matches the search offers at once.
pub const SHIP_MATCHES: usize = 10;

/// A ship the search can offer.
#[derive(Clone, Debug, PartialEq)]
pub struct ShipChoice {
    pub species: Species,
    /// The param the ship is looked up by.
    pub param_index: String,
    pub display_name: String,
    /// Lowercased, ASCII-folded display name, which is what a search is matched against.
    search_name: String,
    pub tier: u32,
}

/// Every playable ship in a build, in the order the armor viewer's tree lists them: nation by
/// nation in the order their names read, class by class, then by tier and name.
pub struct ShipList {
    ships: Vec<ShipChoice>,
}

/// The ship classes a board places, which leaves out buildings and other params with a species.
const SHIP_SPECIES: &[Species] = &[
    Species::AirCarrier,
    Species::Battleship,
    Species::Cruiser,
    Species::Destroyer,
    Species::Submarine,
    Species::Auxiliary,
];

/// The armor viewer's order of classes.
fn species_order(species: &Species) -> u32 {
    match species {
        Species::Destroyer => 0,
        Species::Cruiser => 1,
        Species::Battleship => 2,
        Species::AirCarrier => 3,
        Species::Submarine => 4,
        _ => 5,
    }
}

impl ShipList {
    pub fn build(metadata: &GameMetadataProvider) -> Self {
        let mut nation_names: HashMap<String, String> = HashMap::new();
        let mut ships: Vec<(String, ShipChoice)> = Vec::new();
        for param in metadata.params() {
            let Some(species) = param.species().and_then(|species| species.known().copied()) else { continue };
            if !SHIP_SPECIES.contains(&species) {
                continue;
            }
            let Some(vehicle) = param.vehicle() else { continue };
            // Clan rentals duplicate a real ship.
            if vehicle.group() == "clan" {
                continue;
            }
            let nation = param.nation().to_string();
            nation_names.entry(nation.clone()).or_insert_with(|| {
                let key = format!("IDS_{}", nation.to_uppercase());
                metadata
                    .localized_name_from_id(&wowsunpack::data::TranslationKey::new(key))
                    .unwrap_or_else(|| nation.clone())
            });
            let display_name = metadata.localized_name_from_param(param).unwrap_or_else(|| param.name().to_string());
            let search_name = unidecode::unidecode(&display_name).to_lowercase();
            ships.push((
                nation,
                ShipChoice {
                    species,
                    param_index: param.index().to_string(),
                    display_name,
                    search_name,
                    tier: vehicle.level(),
                },
            ));
        }
        ships.sort_by(|(a_nation, a), (b_nation, b)| {
            nation_names[a_nation]
                .cmp(&nation_names[b_nation])
                .then(a_nation.cmp(b_nation))
                .then(species_order(&a.species).cmp(&species_order(&b.species)))
                .then(a.tier.cmp(&b.tier))
                .then(a.display_name.cmp(&b.display_name))
        });
        Self { ships: ships.into_iter().map(|(_, ship)| ship).collect() }
    }

    /// The first [`SHIP_MATCHES`] ships whose names contain what has been typed.
    ///
    /// Empty until something is typed: every ship at once is not a choice.
    pub fn matching(&self, typed: &str) -> Vec<ShipChoice> {
        let query = unidecode::unidecode(typed.trim()).to_lowercase();
        if query.is_empty() {
            return Vec::new();
        }
        self.ships.iter().filter(|ship| ship.search_name.contains(&query)).take(SHIP_MATCHES).cloned().collect()
    }
}

/// Roman numeral for a tier.
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
