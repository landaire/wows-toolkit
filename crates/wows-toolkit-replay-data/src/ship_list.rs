//! The ships a placed ship can be given, looked up by name, as the tactics board and the replay
//! viewport offer them.

use std::cmp::Ordering;
use std::collections::HashMap;

use wows_toolkit_viewmodel::text_filter::FilterKey;
use wows_toolkit_viewmodel::text_filter::FilterQuery;
use wowsunpack::data::ResourceLoader;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::Species;
use wowsunpack::game_params::types::tier_roman;

use crate::load::GameDataCache;

/// How many ship matches a search offers at once.
pub const SHIP_MATCHES: usize = 10;

/// A ship a search can offer.
#[derive(Clone, Debug, PartialEq)]
pub struct ShipChoice {
    pub species: Species,
    /// The param the ship is looked up by.
    pub param_index: String,
    pub display_name: String,
    /// The display name folded, which is what a search is matched against.
    key: FilterKey,
    pub tier: u32,
}

impl ShipChoice {
    pub fn new(species: Species, param_index: String, display_name: String, tier: u32) -> Self {
        let key = FilterKey::new(&display_name);

        Self { species, param_index, display_name, key, tier }
    }

    /// What a chooser's row says: the tier in roman numerals, then the name.
    pub fn label(&self) -> String {
        format!("{} {}", tier_roman(self.tier), self.display_name)
    }
}

/// Every playable ship in a build, clan rentals left out since they duplicate a real ship.
///
/// In the order the egui armor viewer's tree lists them, which egui's ship search walks: nation by
/// nation in the order their names read, class by class, then by tier and name. A search ranks its
/// matches by how well they match, so this order decides only between equal matches.
pub struct ShipList {
    ships: Vec<ShipChoice>,
}

impl ShipList {
    pub fn build(metadata: &GameMetadataProvider) -> Self {
        let mut nation_names: HashMap<String, String> = HashMap::new();
        let mut listed: Vec<ListedShip> = Vec::new();
        for param in metadata.params() {
            let Some(species) = param.species().and_then(|species| species.known().copied()) else { continue };
            if !Species::SHIP_CLASSES.contains(&species) {
                continue;
            }
            let Some(vehicle) = param.vehicle() else { continue };
            if vehicle.group() == "clan" {
                continue;
            }

            let nation = param.nation().to_string();
            let nation_name = nation_names
                .entry(nation.clone())
                .or_insert_with(|| {
                    let key = format!("IDS_{}", nation.to_uppercase());
                    metadata
                        .localized_name_from_id(&wowsunpack::data::TranslationKey::new(key))
                        .unwrap_or_else(|| nation.clone())
                })
                .clone();
            // A ship the build has no name for is still one that can be chosen, by its param name.
            let display_name = metadata.localized_name_from_param(param).unwrap_or_else(|| param.name().to_string());
            let ship = ShipChoice::new(species, param.index().to_string(), display_name, vehicle.level());
            listed.push(ListedShip { nation_name, nation, ship });
        }

        Self::from_listed(listed)
    }

    /// The ships of the newest build `game_data` has loaded, or `None` before one has.
    pub fn from_game_data(game_data: &GameDataCache) -> Option<Self> {
        let loaded = game_data.newest_loaded()?;

        Some(Self::build(loaded.provider()))
    }

    fn from_listed(mut listed: Vec<ListedShip>) -> Self {
        listed.sort_by(ListedShip::listing_order);

        Self { ships: listed.into_iter().map(|listed| listed.ship).collect() }
    }

    /// The ships whose names match `query`, best first and at most [`SHIP_MATCHES`] of them.
    ///
    /// Empty until something is typed: every ship at once is not a choice.
    pub fn matching(&self, query: &FilterQuery) -> Vec<ShipChoice> {
        if !query.is_active() {
            return Vec::new();
        }

        query.rank(&self.ships, |ship| &ship.key).into_iter().take(SHIP_MATCHES).cloned().collect()
    }
}

/// A ship with the nation it is listed under.
struct ListedShip {
    nation_name: String,
    nation: String,
    ship: ShipChoice,
}

impl ListedShip {
    fn listing_order(a: &Self, b: &Self) -> Ordering {
        a.nation_name
            .cmp(&b.nation_name)
            .then(a.nation.cmp(&b.nation))
            .then(a.ship.species.class_display_rank().cmp(&b.ship.species.class_display_rank()))
            .then(a.ship.tier.cmp(&b.ship.tier))
            .then(a.ship.display_name.cmp(&b.ship.display_name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed(nation: &str, species: Species, tier: u32, name: &str) -> ListedShip {
        let ship = ShipChoice::new(species, name.to_uppercase(), name.to_owned(), tier);

        ListedShip { nation_name: nation.to_owned(), nation: nation.to_lowercase(), ship }
    }

    /// Nation first, then class in display order, then tier and name, as the egui tree lists
    /// them; tier alone does not lead.
    #[test]
    fn ships_list_by_nation_class_tier_and_name() {
        let list = ShipList::from_listed(vec![
            listed("USA", Species::Destroyer, 10, "Gearing"),
            listed("Japan", Species::Battleship, 10, "Yamato"),
            listed("Japan", Species::Destroyer, 10, "Shimakaze"),
            listed("USA", Species::Cruiser, 2, "Chester"),
            listed("Japan", Species::Destroyer, 2, "Umikaze"),
            listed("Japan", Species::Battleship, 10, "Shikishima"),
        ]);

        let labels: Vec<String> = list.ships.iter().map(ShipChoice::label).collect();
        insta::assert_debug_snapshot!(labels);
    }
}
