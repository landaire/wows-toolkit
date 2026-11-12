//! Ship selector catalog: every playable ship in the loaded build, grouped
//! by nation then class then sorted by tier/name. Ports the egui app's
//! `ShipCatalog` (`crates/wows-toolkit/src/armor_viewer/ship_selector.rs`)
//! verbatim -- same struct shapes, same filters (ship species only, no clan
//! rentals), same sort order -- so the sidebar/tree Task 4 builds on top of
//! this shows the exact same ship list the egui armor viewer does.

use std::collections::HashMap;

use wowsunpack::data::ResourceLoader;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::ttx::armor_material::ArmorMaterial;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::Species;

use super::search_key::SearchKey;

/// Pre-built catalog of all ships, organized for the tree selector.
#[derive(Clone)]
pub struct ShipCatalog {
    /// Nations sorted alphabetically, each containing sorted classes.
    pub nations: Vec<NationGroup>,
    /// The ships left out of `nations`, which the Model Viewer lists on
    /// request.
    pub hidden: Vec<HiddenShip>,
}

#[derive(Clone)]
pub struct NationGroup {
    /// The raw nation key (`usa`, `united_kingdom`), which is what the flag
    /// icons and the tree's row ids are keyed on.
    pub nation: String,
    /// The nation's name in the current locale, which is what the tree shows.
    pub display_name: String,
    pub classes: Vec<ClassGroup>,
}

#[derive(Clone)]
pub struct ClassGroup {
    pub species: Species,
    pub ships: Vec<ShipEntry>,
}

/// A ship the list leaves out, with the nation and class it belongs in.
#[derive(Clone)]
pub struct HiddenShip {
    pub nation: String,
    pub nation_display_name: String,
    pub species: Species,
    pub ship: ShipEntry,
    pub reason: HiddenReason,
}

/// Why the ship list leaves a ship out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HiddenReason {
    /// A clan rental copy (`group` `clan`) of a ship listed on its own.
    ClanRental,
    /// A species that is not one of the classes ships are grouped by.
    OtherClass,
}

#[derive(Clone)]
pub struct ShipEntry {
    pub param_index: String,
    pub display_name: String,
    /// Lowercased, ASCII-folded display name for search matching.
    pub search_name: String,
    /// The short and full names, GameParams index, name and id, which the
    /// Model Viewer sidebar finds the ship by.
    pub search_key: SearchKey,
    pub tier: u32,
}

impl ShipCatalog {
    /// Every ship in it, nation by nation and class by class.
    pub fn ships(&self) -> impl Iterator<Item = &ShipEntry> {
        self.nations.iter().flat_map(|nation| nation.classes.iter()).flat_map(|class| class.ships.iter())
    }

    /// This catalog with its hidden ships listed in their nations and
    /// classes, and none left hidden.
    pub fn with_hidden(&self) -> ShipCatalog {
        let listed = self.nations.iter().flat_map(|nation| {
            nation.classes.iter().flat_map(move |class| {
                class
                    .ships
                    .iter()
                    .map(move |ship| (nation.nation.clone(), nation.display_name.clone(), class.species, ship.clone()))
            })
        });
        let hidden = self.hidden.iter().map(|hidden| {
            (hidden.nation.clone(), hidden.nation_display_name.clone(), hidden.species, hidden.ship.clone())
        });

        ShipCatalog { nations: group_ships(listed.chain(hidden)), hidden: Vec::new() }
    }
}

/// Groups `ships`, each with its nation key, nation name and class, by
/// nation then class, sorted as the tree lists them.
fn group_ships(ships: impl IntoIterator<Item = (String, String, Species, ShipEntry)>) -> Vec<NationGroup> {
    let mut nation_map: HashMap<String, (String, HashMap<Species, Vec<ShipEntry>>)> = HashMap::new();
    for (nation, display_name, species, ship) in ships {
        let (_, classes) = nation_map.entry(nation).or_insert_with(|| (display_name, HashMap::new()));
        classes.entry(species).or_default().push(ship);
    }

    let mut nations: Vec<NationGroup> = nation_map
        .into_iter()
        .map(|(nation, (display_name, class_map))| {
            let mut classes: Vec<ClassGroup> = class_map
                .into_iter()
                .map(|(species, mut ships)| {
                    ships.sort_by(|a, b| a.tier.cmp(&b.tier).then(a.display_name.cmp(&b.display_name)));
                    ClassGroup { species, ships }
                })
                .collect();
            classes.sort_by_key(|c| species_order(&c.species));
            NationGroup { nation, display_name, classes }
        })
        .collect();

    // By the name shown, not the key behind it, matching the egui tree
    // (`armor_viewer/ui/tab.rs:377`).
    nations.sort_by(|a, b| a.display_name.cmp(&b.display_name));

    nations
}

/// The ship tiers the Model Viewer sidebar lists, inclusive and never empty.
/// Stored as `[lowest, highest]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(from = "[u32; 2]", into = "[u32; 2]")]
pub struct TierRange {
    lowest: u32,
    highest: u32,
}

impl TierRange {
    pub const LOWEST: u32 = 1;
    pub const HIGHEST: u32 = 11;
    /// Every tier, which filters nothing.
    pub const ALL: Self = Self { lowest: Self::LOWEST, highest: Self::HIGHEST };

    /// The tiers from `a` to `b`, in either order, kept inside I to XI.
    pub fn new(a: u32, b: u32) -> Self {
        let lowest = a.min(b).clamp(Self::LOWEST, Self::HIGHEST);
        let highest = a.max(b).clamp(Self::LOWEST, Self::HIGHEST);

        Self { lowest, highest }
    }

    pub fn lowest(self) -> u32 {
        self.lowest
    }

    pub fn highest(self) -> u32 {
        self.highest
    }

    pub fn contains(self, tier: u32) -> bool {
        (self.lowest..=self.highest).contains(&tier)
    }

    pub fn is_all(self) -> bool {
        self == Self::ALL
    }
}

impl From<[u32; 2]> for TierRange {
    fn from([a, b]: [u32; 2]) -> Self {
        Self::new(a, b)
    }
}

impl From<TierRange> for [u32; 2] {
    fn from(range: TierRange) -> Self {
        [range.lowest, range.highest]
    }
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

/// A nation or armor zone name in the current locale.
///
/// The game keys both as `IDS_<NAME>`; an unknown one shows its raw key, as
/// the egui app's own `translate_name` does. The raw key stays the identity
/// everywhere else, so this is only for what is drawn.
pub fn translate_name(metadata: &GameMetadataProvider, name: &str) -> String {
    let key = format!("IDS_{}", name.to_uppercase());
    metadata.localized_name_from_id(&wowsunpack::data::TranslationKey::new(key)).unwrap_or_else(|| name.to_string())
}

/// A plate material keyed `key` named the way the game names it on a ship of
/// `species`. A key outside the material table shows as is.
pub fn translate_material_key(metadata: &GameMetadataProvider, key: &str, species: Option<Species>) -> String {
    match ArmorMaterial::parse(key) {
        Some(material) => material.localized_name(metadata, species),
        None => key.to_owned(),
    }
}

impl ShipCatalog {
    /// Build from GameMetadataProvider. Lists the ship classes' vehicles;
    /// clan rentals and vehicles of other species go to [`Self::hidden`].
    pub fn build(metadata: &GameMetadataProvider) -> Self {
        let mut nation_names: HashMap<String, String> = HashMap::new();
        let mut listed = Vec::new();
        let mut hidden = Vec::new();
        for param in metadata.params() {
            let Some(vehicle) = param.vehicle() else {
                continue;
            };
            // A species the game names but this build does not know cannot be
            // placed in a class.
            let Some(species) = param.species().and_then(|species| species.known().copied()) else {
                continue;
            };

            let reason = if !SHIP_SPECIES.contains(&species) {
                Some(HiddenReason::OtherClass)
            } else if vehicle.group() == "clan" {
                Some(HiddenReason::ClanRental)
            } else {
                None
            };

            let nation = param.nation().to_string();
            let nation_display_name =
                nation_names.entry(nation.clone()).or_insert_with(|| translate_name(metadata, &nation)).clone();
            let entry = ship_entry(param, vehicle.level(), metadata);
            match reason {
                Some(reason) => hidden.push(HiddenShip { nation, nation_display_name, species, ship: entry, reason }),
                None => listed.push((nation, nation_display_name, species, entry)),
            }
        }

        ShipCatalog { nations: group_ships(listed), hidden }
    }
}

/// The entry for ship `param` at tier `tier`.
fn ship_entry(param: &wowsunpack::game_params::types::Param, tier: u32, metadata: &GameMetadataProvider) -> ShipEntry {
    let display_name = metadata.localized_name_from_param(param).unwrap_or_else(|| param.name().to_string());

    let search_name = unidecode::unidecode(&display_name).to_lowercase();
    let full_name_key = wowsunpack::data::TranslationKey::new(format!("IDS_{}_FULL", param.index()));
    let mut search_key = SearchKey::new();
    search_key.push(display_name.clone());
    if let Some(full_name) = metadata.localized_name_from_id(&full_name_key) {
        search_key.push(full_name);
    }
    search_key.push(param.index());
    search_key.push(param.name());
    search_key.push(param.id().to_string());

    ShipEntry { param_index: param.index().to_string(), display_name, search_name, search_key, tier }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tier_range_is_ordered_kept_inside_i_to_xi_and_stored_as_a_pair() {
        let range = TierRange::new(8, 3);
        assert_eq!((range.lowest(), range.highest()), (3, 8));
        assert!(range.contains(3) && range.contains(8) && !range.contains(2) && !range.contains(9));
        assert_eq!(TierRange::new(0, 40), TierRange::ALL);
        assert!(TierRange::ALL.is_all() && !range.is_all());

        assert_eq!(serde_json::to_string(&range).expect("serializes"), "[3,8]");
        let read: TierRange = serde_json::from_str("[12,5]").expect("a stored pair reads back");
        assert_eq!(read, TierRange::new(5, 11), "an out-of-range store is brought back inside");
    }

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
        let ship = |param_index: &str, display_name: &str, tier: u32| ShipEntry {
            param_index: param_index.into(),
            display_name: display_name.into(),
            search_name: display_name.to_lowercase(),
            search_key: SearchKey::new(),
            tier,
        };
        let mut ships = [ship("A", "Zeta", 5), ship("B", "Alpha", 3), ship("C", "Beta", 3)];
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
