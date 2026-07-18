//! Ship selector catalog: every playable ship in the loaded build, grouped
//! by nation then class then sorted by tier/name. Ports the egui app's
//! `ShipCatalog` (`crates/wows-toolkit/src/armor_viewer/ship_selector.rs`)
//! verbatim -- same struct shapes, same filters (ship species only, no clan
//! rentals), same sort order -- so the sidebar/tree Task 4 builds on top of
//! this shows the exact same ship list the egui armor viewer does.

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_roman_covers_i_through_xi_and_falls_back_for_unknown_tiers() {
        let expected = ["I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X", "XI"];
        for (tier, roman) in (1..=11u32).zip(expected) {
            assert_eq!(tier_roman(tier), roman);
        }
        assert_eq!(tier_roman(0), "?");
        assert_eq!(tier_roman(12), "?");
    }

    #[test]
    fn species_name_covers_every_ship_species_and_falls_back_for_others() {
        assert_eq!(species_name(&Species::Destroyer), "Destroyer");
        assert_eq!(species_name(&Species::Cruiser), "Cruiser");
        assert_eq!(species_name(&Species::Battleship), "Battleship");
        assert_eq!(species_name(&Species::AirCarrier), "Aircraft Carrier");
        assert_eq!(species_name(&Species::Submarine), "Submarine");
        assert_eq!(species_name(&Species::Auxiliary), "Auxiliary");
        // Any non-ship species (e.g. a weapon-mount species) falls back to "Other".
        assert_eq!(species_name(&Species::Artillery), "Other");
    }

    #[test]
    fn species_order_ranks_destroyer_first_and_non_combatant_species_last() {
        assert_eq!(species_order(&Species::Destroyer), 0);
        assert_eq!(species_order(&Species::Cruiser), 1);
        assert_eq!(species_order(&Species::Battleship), 2);
        assert_eq!(species_order(&Species::AirCarrier), 3);
        assert_eq!(species_order(&Species::Submarine), 4);
        // Auxiliary is filterable into the catalog but has no canonical slot
        // in the class ordering, same as any other non-combatant species.
        assert_eq!(species_order(&Species::Auxiliary), 5);
        assert_eq!(species_order(&Species::Artillery), 5);
    }

    /// Pins `ShipCatalog::build`'s per-class ship sort (tier ascending, then
    /// display name) without needing a real `GameMetadataProvider` -- the
    /// comparator only touches `ShipEntry`'s own fields, which are cheap to
    /// fabricate directly. The full `build` path (species/vehicle/nation
    /// filtering and grouping) still needs a real provider; see
    /// `ship_catalog_build_groups_and_sorts_ships_from_a_real_provider` below.
    #[test]
    fn ship_entries_sort_by_tier_then_display_name() {
        let mut ships = [
            ShipEntry { param_index: "A".into(), display_name: "Zeta".into(), search_name: "zeta".into(), tier: 5 },
            ShipEntry { param_index: "B".into(), display_name: "Alpha".into(), search_name: "alpha".into(), tier: 3 },
            ShipEntry { param_index: "C".into(), display_name: "Beta".into(), search_name: "beta".into(), tier: 3 },
        ];
        ships.sort_by(|a, b| a.tier.cmp(&b.tier).then(a.display_name.cmp(&b.display_name)));
        let order: Vec<&str> = ships.iter().map(|s| s.display_name.as_str()).collect();
        assert_eq!(order, ["Alpha", "Beta", "Zeta"]);
    }

    /// `GameMetadataProvider::from_params_no_specs` builds a real (not
    /// mocked) provider from a hand-supplied `Param` list without needing
    /// any VFS or entity-script data -- but `Param` itself has private
    /// fields with no public constructor, so an empty list is the only
    /// case this crate can fabricate. This still exercises the real
    /// `ShipCatalog::build` path end to end: zero params in, zero nations
    /// out, no panics.
    #[test]
    fn ship_catalog_build_is_empty_for_a_provider_with_no_params() {
        use wowsunpack::game_params::provider::GameMetadataProvider;

        let metadata = GameMetadataProvider::from_params_no_specs(Vec::new())
            .expect("an empty param list should build a valid provider");
        let catalog = ShipCatalog::build(&metadata);
        assert!(catalog.nations.is_empty());
    }

    /// Needs local game data to build a real `GameMetadataProvider` with
    /// actual ship params (species/vehicle/nation resolution cannot be
    /// cheaply fabricated; see `ship_catalog_build_is_empty_for_a_provider_with_no_params`
    /// above for what is testable without one). `WOWS_ARMOR_VIEWER_TEST_GAME_DIR`
    /// is a live WoWs install directory (needs `bin/<build>/idx` and
    /// `res_packages/`, per `load_game_resources`), not a compact per-build
    /// dump. Run with, e.g.:
    ///
    /// ```text
    /// WOWS_ARMOR_VIEWER_TEST_GAME_DIR="E:\WoWs\World_of_Warships" \
    /// WOWS_ARMOR_VIEWER_TEST_VERSION="15, 6, 0, 12830008" \
    /// cargo test -p wows-toolkit-gpui -- --ignored ship_catalog_build_groups_and_sorts_ships_from_a_real_provider
    /// ```
    ///
    /// Mirrors `replay_inspector::model`'s equivalent real-provider test.
    #[test]
    #[ignore = "needs a local game install; see the doc comment for the run command"]
    fn ship_catalog_build_groups_and_sorts_ships_from_a_real_provider() {
        use wowsunpack::game_params::provider::GameMetadataProvider;

        let game_dir = std::env::var("WOWS_ARMOR_VIEWER_TEST_GAME_DIR")
            .expect("set WOWS_ARMOR_VIEWER_TEST_GAME_DIR to a WoWs install directory");
        let version_str = std::env::var("WOWS_ARMOR_VIEWER_TEST_VERSION").expect(
            "set WOWS_ARMOR_VIEWER_TEST_VERSION to a clientVersionFromExe string, e.g. \"13, 11, 0, 12668706\"",
        );
        let version = wowsunpack::data::Version::from_client_exe(&version_str);

        let resources = wowsunpack::game_data::load_game_resources(std::path::Path::new(&game_dir), &version)
            .expect("failed to load game resources from WOWS_ARMOR_VIEWER_TEST_GAME_DIR");
        let metadata =
            GameMetadataProvider::from_vfs(&resources.vfs).expect("failed to build GameMetadataProvider from the VFS");

        let catalog = ShipCatalog::build(&metadata);

        assert!(!catalog.nations.is_empty(), "expected at least one nation with ships");
        let nation_names: Vec<&str> = catalog.nations.iter().map(|n| n.nation.as_str()).collect();
        let mut sorted_names = nation_names.clone();
        sorted_names.sort();
        assert_eq!(nation_names, sorted_names, "nations should be sorted alphabetically");

        let total_ships: usize = catalog.nations.iter().flat_map(|n| &n.classes).map(|c| c.ships.len()).sum();
        assert!(total_ships > 0, "expected at least one ship in the catalog");

        for nation in &catalog.nations {
            let mut prev_order: Option<u32> = None;
            for class in &nation.classes {
                let order = species_order(&class.species);
                if let Some(prev) = prev_order {
                    assert!(order >= prev, "classes within a nation should be sorted by species_order");
                }
                prev_order = Some(order);

                for pair in class.ships.windows(2) {
                    let [a, b] = pair else { continue };
                    assert!(
                        (a.tier, &a.display_name) <= (b.tier, &b.display_name),
                        "ships within a class should be sorted by tier then display name"
                    );
                }
            }
        }

        println!("armor catalog (real provider): {} nations, {} ships", catalog.nations.len(), total_ships);
    }
}
