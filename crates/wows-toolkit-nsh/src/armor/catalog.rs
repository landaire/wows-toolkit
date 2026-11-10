//! The ship catalog as the sidebar tree and the ship searches read it.

use std::collections::HashMap;

use base64::Engine as _;
use nsh_core::prelude::JsObject;
use wows_toolkit_viewmodel::ship_catalog::ShipCatalog;
use wows_toolkit_viewmodel::ship_catalog::ShipEntry;
use wows_toolkit_viewmodel::ship_catalog::species_name;
use wows_toolkit_viewmodel::ship_catalog::tier_roman;
use wowsunpack::game_assets::GuiAsset;
use wowsunpack::game_params::types::Species;
use wowsunpack::vfs::VfsPath;

use crate::class_icon::ClassIcon;

/// One ship row.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct CatalogShip {
    pub param_index: String,
    pub name: String,
    pub tier: u32,
    pub roman: String,
    /// `"{roman} {name}"`, as the row reads.
    pub label: String,
}

/// One class within a nation.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct CatalogClass {
    /// The class key (`"Destroyer"`), which the class icon is keyed on.
    pub species: String,
    pub label: String,
    /// The class icon, when the build has one.
    #[prop(skip_none)]
    pub icon: Option<ClassIcon>,
    pub ships: Vec<CatalogShip>,
}

/// One nation.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct CatalogNation {
    /// The raw nation key (`"usa"`), which the flag icon is keyed on.
    pub nation: String,
    pub label: String,
    /// The nation flag as a PNG `data:` URL, when the build has one.
    #[prop(skip_none)]
    pub flag: Option<String>,
    pub classes: Vec<CatalogClass>,
}

/// The ships tree.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct CatalogView {
    pub nations: Vec<CatalogNation>,
}

/// The tree's nation flags as `data:` URLs and class icons, read once per
/// build.
#[derive(Debug, Default)]
pub(super) struct CatalogIcons {
    flags: HashMap<String, String>,
    classes: HashMap<Species, ClassIcon>,
}

impl CatalogIcons {
    pub(super) fn read(vfs: &VfsPath, catalog: &ShipCatalog) -> Self {
        let mut icons = Self::default();
        for nation in &catalog.nations {
            if let Some(bytes) = GuiAsset::NationFlag(&nation.nation).read(vfs, None) {
                icons.flags.insert(nation.nation.clone(), data_url("image/png", &bytes));
            }
            for class in &nation.classes {
                if icons.classes.contains_key(&class.species) {
                    continue;
                }
                if let Some(icon) = ClassIcon::read(class.species, vfs) {
                    icons.classes.insert(class.species, icon);
                }
            }
        }
        icons
    }
}

fn data_url(mime: &str, bytes: &[u8]) -> String {
    format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))
}

/// A search as the catalog's names are folded: accents removed, lowercased,
/// trimmed. `None` when nothing is searched for.
pub(super) fn fold_query(query: &str) -> Option<String> {
    let folded = unidecode::unidecode(query.trim()).to_lowercase();

    (!folded.is_empty()).then_some(folded)
}

/// Whether `ship` is found by the folded `needle`.
pub(super) fn ship_matches(ship: &ShipEntry, needle: &str) -> bool {
    ship.search_name.contains(needle)
}

/// The tree, keeping only the branches a search for `query` finds. An empty
/// query keeps everything.
pub(super) fn catalog_view(catalog: &ShipCatalog, icons: &CatalogIcons, query: &str) -> CatalogView {
    let needle = fold_query(query);
    let mut nations = Vec::new();
    for nation in &catalog.nations {
        let mut classes = Vec::new();
        for class in &nation.classes {
            let ships: Vec<CatalogShip> = class
                .ships
                .iter()
                .filter(|ship| needle.as_deref().is_none_or(|needle| ship_matches(ship, needle)))
                .map(ship_row)
                .collect();
            if ships.is_empty() {
                continue;
            }
            classes.push(CatalogClass {
                species: format!("{:?}", class.species),
                label: species_name(&class.species).to_owned(),
                icon: icons.classes.get(&class.species).cloned(),
                ships,
            });
        }
        if classes.is_empty() {
            continue;
        }
        nations.push(CatalogNation {
            nation: nation.nation.clone(),
            label: nation.display_name.clone(),
            flag: icons.flags.get(&nation.nation).cloned(),
            classes,
        });
    }

    CatalogView { nations }
}

fn ship_row(ship: &ShipEntry) -> CatalogShip {
    let roman = tier_roman(ship.tier);
    CatalogShip {
        param_index: ship.param_index.clone(),
        name: ship.display_name.clone(),
        tier: ship.tier,
        roman: roman.to_owned(),
        label: format!("{roman} {}", ship.display_name),
    }
}

#[cfg(test)]
mod tests {
    use wows_toolkit_viewmodel::ship_catalog::ClassGroup;
    use wows_toolkit_viewmodel::ship_catalog::NationGroup;

    use super::*;

    fn entry(param_index: &str, display_name: &str, tier: u32) -> ShipEntry {
        ShipEntry {
            param_index: param_index.to_owned(),
            display_name: display_name.to_owned(),
            search_name: unidecode::unidecode(display_name).to_lowercase(),
            tier,
        }
    }

    fn catalog() -> ShipCatalog {
        ShipCatalog {
            nations: vec![
                NationGroup {
                    nation: "europe".to_owned(),
                    display_name: "Europe".to_owned(),
                    classes: vec![ClassGroup {
                        species: Species::Destroyer,
                        ships: vec![entry("PWSD110", "Sm\u{e5}land", 10), entry("PWSD108", "Skane", 8)],
                    }],
                },
                NationGroup {
                    nation: "japan".to_owned(),
                    display_name: "Japan".to_owned(),
                    classes: vec![ClassGroup {
                        species: Species::Battleship,
                        ships: vec![entry("PJSB018", "Yamato", 10)],
                    }],
                },
            ],
        }
    }

    fn param_indices(view: &CatalogView) -> Vec<&str> {
        view.nations
            .iter()
            .flat_map(|nation| &nation.classes)
            .flat_map(|class| &class.ships)
            .map(|ship| ship.param_index.as_str())
            .collect()
    }

    #[test]
    fn a_query_is_folded_like_the_names() {
        assert_eq!(fold_query("  Sm\u{e5}LAND ").as_deref(), Some("smaland"));
        assert_eq!(fold_query(" "), None);
    }

    #[test]
    fn an_unaccented_search_finds_an_accented_ship() {
        let view = catalog_view(&catalog(), &CatalogIcons::default(), "smal");

        assert_eq!(param_indices(&view), ["PWSD110"]);
        assert_eq!(view.nations.len(), 1);
        assert_eq!(view.nations[0].classes[0].label, "Destroyer");
    }

    #[test]
    fn an_accented_search_finds_it_too() {
        assert_eq!(param_indices(&catalog_view(&catalog(), &CatalogIcons::default(), "Sm\u{e5}")), ["PWSD110"]);
    }

    #[test]
    fn an_empty_search_keeps_the_whole_tree() {
        let view = catalog_view(&catalog(), &CatalogIcons::default(), " ");

        assert_eq!(param_indices(&view), ["PWSD110", "PWSD108", "PJSB018"]);
        assert_eq!(view.nations[1].classes[0].ships[0].label, "X Yamato");
    }

    #[test]
    fn a_search_with_no_match_leaves_no_branch() {
        assert!(catalog_view(&catalog(), &CatalogIcons::default(), "iowa").nations.is_empty());
    }
}
