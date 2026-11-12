//! The Model Viewer sidebar's model listing: the catalogue grouped by nation,
//! kind and sub-kind, with every row's search key lowercased once so a search
//! only runs substring tests.

use std::collections::BTreeMap;
use std::collections::HashMap;

use wowsunpack::export::model_catalog::ModelCatalog;
use wowsunpack::export::model_catalog::ModelEntry;
use wowsunpack::export::model_catalog::ModelKind;
use wowsunpack::export::model_catalog::ModelNation;
use wowsunpack::export::model_catalog::ModelPath;
use wowsunpack::export::model_catalog::ModelSubKind;
use wowsunpack::export::model_catalog::ModelVariantKind;
use wowsunpack::export::model_library::ModelLibrary;
use wowsunpack::export::part_group::PartGroup;
use wowsunpack::export::ship_parts::ListedMisc;
use wowsunpack::export::ship_parts::MiscParent;
use wowsunpack::export::ship_parts::ShipPartListing;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::GameParamProvider;

use super::catalog::ShipCatalog;
use super::model_refs::ShipPartKey;
use super::model_refs::VehicleParamIndex;

/// Every listed catalogue model, grouped for the tree.
pub struct ModelTreeIndex {
    pub nations: Vec<ModelNationGroup>,
}

pub struct ModelNationGroup {
    pub nation: ModelNation,
    /// The GameParams nation of the same name, whose flag and translated name
    /// the row shows.
    pub ship_nation: Option<String>,
    pub display_name: String,
    pub kinds: Vec<ModelKindGroup>,
}

pub struct ModelKindGroup {
    pub kind: ModelKind,
    /// Sub-kind folders in sub-kind order, then the models filed directly
    /// under the kind (`sub_kind: None`).
    pub sub_kinds: Vec<ModelSubKindGroup>,
}

pub struct ModelSubKindGroup {
    pub sub_kind: Option<ModelSubKind>,
    pub models: Vec<ModelRow>,
}

pub struct ModelRow {
    pub path: ModelPath,
    /// Lowercased path, which holds the name.
    pub search_key: String,
    pub variants: Vec<VariantRow>,
}

pub struct VariantRow {
    pub kind: ModelVariantKind,
    pub path: ModelPath,
    pub search_key: String,
    /// A skin's own dead and section variants; empty for the other kinds.
    pub variants: Vec<VariantRow>,
}

impl ModelRow {
    pub fn matches(&self, query: &str) -> bool {
        self.search_key.contains(query)
    }

    pub fn any_variant_matches(&self, query: &str) -> bool {
        self.variants.iter().any(|variant| variant.matches(query) || variant.any_variant_matches(query))
    }
}

impl VariantRow {
    pub fn matches(&self, query: &str) -> bool {
        self.search_key.contains(query)
    }

    pub fn any_variant_matches(&self, query: &str) -> bool {
        self.variants.iter().any(|variant| variant.matches(query))
    }
}

/// The folders a ship's mounts are listed under, in display order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MountGroup {
    MainBattery,
    Secondaries,
    AntiAircraft,
    Torpedoes,
    Directors,
    Finders,
    Radars,
    DepthCharges,
    Aircraft,
    Other,
}

impl MountGroup {
    /// The folder for a mount of `group` showing a model of `kind`. The model
    /// kind splits what GameParams files together as fire control.
    pub fn classify(group: PartGroup, kind: Option<ModelKind>) -> Self {
        match (kind, group) {
            (Some(ModelKind::Director), _) => Self::Directors,
            (Some(ModelKind::Finder), _) => Self::Finders,
            (Some(ModelKind::Radar), _) => Self::Radars,
            (Some(ModelKind::Catapult | ModelKind::Aircraft), _) => Self::Aircraft,
            (_, PartGroup::MainBattery) => Self::MainBattery,
            (_, PartGroup::SecondaryBattery) => Self::Secondaries,
            (_, PartGroup::AaGuns) => Self::AntiAircraft,
            (_, PartGroup::Torpedoes) => Self::Torpedoes,
            (_, PartGroup::DepthCharges) => Self::DepthCharges,
            (_, PartGroup::FireControl) => Self::Directors,
            (_, PartGroup::Radar) => Self::Radars,
            (_, _) => Self::Other,
        }
    }
}

/// A ship's parts as its tree row lists them.
#[derive(Default)]
pub struct ShipPartRows {
    /// The hull model and its sections.
    pub hull: Vec<ModelPath>,
    /// Non-empty groups in [`MountGroup`] order.
    pub mounts: Vec<MountGroupRows>,
    pub misc: Vec<PartRow>,
}

pub struct MountGroupRows {
    pub group: MountGroup,
    pub parts: Vec<PartRow>,
}

/// One mount or misc part.
pub struct PartRow {
    pub key: ShipPartKey,
    /// The node it sits at, as shown: `HP_x`, `MP_y`, or the misc node behind
    /// its parent mount or hull section.
    pub node: String,
    pub model: ModelPath,
}

impl ShipPartRows {
    pub fn from_listing(listing: &ShipPartListing, catalog: &ModelCatalog) -> Self {
        let mut groups: BTreeMap<MountGroup, Vec<PartRow>> = BTreeMap::new();
        for mount in &listing.mounts {
            let kind = catalog.owning_entry(&mount.model).map(ModelEntry::kind);
            groups.entry(MountGroup::classify(mount.group, kind)).or_default().push(PartRow {
                key: ShipPartKey::Mount(mount.hp.clone()),
                node: mount.hp.to_string(),
                model: mount.model.clone(),
            });
        }

        let misc = listing
            .miscs
            .iter()
            .map(|misc| PartRow {
                key: misc.key(),
                node: misc_label(misc, listing.hull_sections.first()),
                model: misc.model.clone(),
            })
            .collect();

        Self {
            hull: listing.hull_sections.clone(),
            mounts: groups.into_iter().map(|(group, parts)| MountGroupRows { group, parts }).collect(),
            misc,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.hull.is_empty() && self.mounts.is_empty() && self.misc.is_empty()
    }
}

impl ModelTreeIndex {
    /// Groups `catalog`'s listed entries. `ships` names the nations whose
    /// directory matches a GameParams nation.
    pub fn build(catalog: &ModelCatalog, ships: &ShipCatalog) -> Self {
        let mut nations: Vec<ModelNationGroup> = Vec::new();
        for entry in catalog.listed_entries() {
            let nation_idx = match nations.iter().position(|group| group.nation == *entry.nation()) {
                Some(idx) => idx,
                None => {
                    nations.push(new_nation_group(entry.nation(), ships));
                    nations.len() - 1
                }
            };
            insert_entry(&mut nations[nation_idx], entry, catalog);
        }

        for nation in &mut nations {
            nation.kinds.sort_by_key(|group| group.kind);
            for kind in &mut nation.kinds {
                // `None` sorts first; models filed directly under the kind go last.
                kind.sub_kinds.sort_by(|a, b| match (&a.sub_kind, &b.sub_kind) {
                    (None, None) => std::cmp::Ordering::Equal,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (Some(a), Some(b)) => a.cmp(b),
                });
                for sub_kind in &mut kind.sub_kinds {
                    sub_kind.models.sort_by_cached_key(|model| model.path.stem().to_lowercase());
                }
            }
        }
        nations.sort_by_cached_key(|group| group.display_name.to_lowercase());

        Self { nations }
    }

    pub fn model_count(&self) -> usize {
        self.nations
            .iter()
            .flat_map(|nation| &nation.kinds)
            .flat_map(|kind| &kind.sub_kinds)
            .map(|sub_kind| sub_kind.models.len())
            .sum()
    }
}

/// Lists the stock parts of every ship in `ships`. A ship whose parts cannot
/// be listed has no entry, and its row stays a plain ship row.
pub fn list_all_ship_parts(
    ships: &ShipCatalog,
    metadata: &GameMetadataProvider,
    library: &ModelLibrary,
) -> HashMap<VehicleParamIndex, ShipPartRows> {
    let no_overrides = HashMap::new();
    let mut parts = HashMap::new();
    for ship in ships.ships() {
        let Some(param) = metadata.game_param_by_index(&ship.param_index) else {
            continue;
        };
        let Some(vehicle) = param.vehicle() else {
            continue;
        };

        match library.list_ship_parts(vehicle, None, &no_overrides) {
            Ok(listing) => {
                let rows = ShipPartRows::from_listing(&listing, library.catalog());
                parts.insert(VehicleParamIndex::new(ship.param_index.clone()), rows);
            }
            Err(report) => {
                tracing::debug!("model viewer: the parts of {} could not be listed: {report:?}", ship.param_index);
            }
        }
    }

    parts
}

/// A misc node behind the mount or hull section carrying it: `HP_x MP_y`,
/// `Bow MP_y`, or `MP_y` on the ship model itself. Sections and mounts repeat
/// node names, so the bare node does not tell the rows apart.
fn misc_label(misc: &ListedMisc, ship_model: Option<&ModelPath>) -> String {
    match &misc.parent {
        MiscParent::Mount(hp) => format!("{hp} {}", misc.node),
        MiscParent::HullSection(section) if Some(section) == ship_model => misc.node.to_string(),
        MiscParent::HullSection(section) => {
            let suffix = ship_model
                .and_then(|ship| section.stem().strip_prefix(ship.stem()))
                .and_then(|rest| rest.strip_prefix('_'))
                .unwrap_or(section.stem());
            format!("{suffix} {}", misc.node)
        }
    }
}

fn new_nation_group(nation: &ModelNation, ships: &ShipCatalog) -> ModelNationGroup {
    let ship_nation = ships.nations.iter().find(|group| same_nation(nation.as_str(), &group.nation));

    ModelNationGroup {
        nation: nation.clone(),
        ship_nation: ship_nation.map(|group| group.nation.clone()),
        display_name: ship_nation.map_or_else(|| nation.as_str().to_string(), |group| group.display_name.clone()),
        kinds: Vec::new(),
    }
}

fn insert_entry(nation: &mut ModelNationGroup, entry: &ModelEntry, catalog: &ModelCatalog) {
    let kind_idx = match nation.kinds.iter().position(|group| group.kind == entry.kind()) {
        Some(idx) => idx,
        None => {
            nation.kinds.push(ModelKindGroup { kind: entry.kind(), sub_kinds: Vec::new() });
            nation.kinds.len() - 1
        }
    };
    let kind = &mut nation.kinds[kind_idx];

    let sub_kind_idx = match kind.sub_kinds.iter().position(|group| group.sub_kind.as_ref() == entry.sub_kind()) {
        Some(idx) => idx,
        None => {
            kind.sub_kinds.push(ModelSubKindGroup { sub_kind: entry.sub_kind().cloned(), models: Vec::new() });
            kind.sub_kinds.len() - 1
        }
    };

    let variants = entry
        .variants()
        .iter()
        .map(|variant| {
            // A skin is its own catalogue entry with its own wreck and hull
            // sections; its skins are left out so the nesting ends here.
            let variants = match variant.kind {
                ModelVariantKind::Skin => catalog
                    .entry(&variant.path)
                    .into_iter()
                    .flat_map(ModelEntry::variants)
                    .filter(|nested| nested.kind != ModelVariantKind::Skin)
                    .map(|nested| variant_row(&nested.kind, &nested.path, Vec::new()))
                    .collect(),
                ModelVariantKind::Dead | ModelVariantKind::Section(_) => Vec::new(),
            };
            variant_row(&variant.kind, &variant.path, variants)
        })
        .collect();
    kind.sub_kinds[sub_kind_idx].models.push(ModelRow {
        path: entry.path().clone(),
        search_key: entry.path().as_str().to_lowercase(),
        variants,
    });
}

fn variant_row(kind: &ModelVariantKind, path: &ModelPath, variants: Vec<VariantRow>) -> VariantRow {
    VariantRow { kind: kind.clone(), path: path.clone(), search_key: path.as_str().to_lowercase(), variants }
}

/// Whether model directory `directory` (`usa`, `uk`, `panasia`) names
/// GameParams nation `ship_nation` (`USA`, `United_Kingdom`, `Pan_Asia`).
fn same_nation(directory: &str, ship_nation: &str) -> bool {
    let folded: String = ship_nation.chars().filter(|c| *c != '_').map(|c| c.to_ascii_lowercase()).collect();

    match directory {
        "uk" => folded == "unitedkingdom",
        _ => folded == directory,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_directories_match_game_params_nations() {
        assert!(same_nation("usa", "USA"));
        assert!(same_nation("uk", "United_Kingdom"));
        assert!(same_nation("panasia", "Pan_Asia"));
        assert!(same_nation("panamerica", "Pan_America"));
        assert!(same_nation("commonwealth", "Commonwealth"));
        assert!(!same_nation("common", "Commonwealth"));
        assert!(!same_nation("events", "USA"));
    }
}
