//! The parts a ship is assembled from: hull sections, GameParams mounts and
//! the misc parts their skeleton extenders place. A listing reads no geometry,
//! so a browser can list every ship's parts up front; a loaded ship reports
//! the same parts under the same keys with placements and hardpoints.

use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;

use rootcause::Report;
use rootcause::prelude::ResultExt;
use thiserror::Error;

use crate::game_params::keys;
use crate::game_params::keys::ComponentType;
use crate::game_params::types::HullUpgradeConfig;
use crate::game_params::types::MountPoint;
use crate::game_params::types::MountSpecies;
use crate::game_params::types::Vehicle;
use crate::models::assets_bin::PrototypeDatabase;
use crate::models::assets_bin::StringsSection;
use crate::models::skeleton_extender::SkeletonExtender;
use crate::models::visual::VisualPrototype;

use super::hardpoint::Hardpoint;
use super::hardpoint::HardpointName;
use super::hardpoint::skeleton_extender_at;
use super::model_catalog::ModelEntry;
use super::model_catalog::ModelPath;
use super::model_library::ModelLibrary;
use super::part_group::PartGroup;

/// Skeleton node-name prefix marking a misc part (propeller, boat, deck fitting).
/// Style-variant miscs use `SP_` and are out of scope.
pub(super) const MISC_NODE_PREFIX: &str = "MP_";

/// The `customMiscs` preset key the game shows in battle (`MiscPresets.battle`);
/// the viewer renders the battle configuration, excluding `dock`-only extras.
pub(super) const MISC_PRESET_BATTLE: &str = keys::BATTLE_PRESET;

/// Names one part of a ship, alike in a [`ShipPartListing`] and in
/// [`super::ship::ShipModelContext::parts`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ShipPartKey {
    /// A hull model: the ship model itself or one of its sections.
    HullSection(ModelPath),
    /// The mount GameParams places at this hardpoint.
    Mount(HardpointName),
    /// A misc part at a skeleton `MP_` node. Hull sections reuse node names
    /// (each end of the hull has its own `MP_AM001_Bollard_2`), and every
    /// mount sharing a model repeats its nodes, so the parent is part of the key.
    Misc { node: HardpointName, parent: MiscParent },
}

impl ShipPartKey {
    /// A display and export name: the section stem, the mount hardpoint, or
    /// the misc node followed by its parent in brackets.
    pub fn label(&self) -> String {
        match self {
            Self::HullSection(section) => section.stem().to_string(),
            Self::Mount(hp) => hp.to_string(),
            Self::Misc { node, parent: MiscParent::HullSection(section) } => format!("{node} [{}]", section.stem()),
            Self::Misc { node, parent: MiscParent::Mount(hp) } => format!("{node} [{hp}]"),
        }
    }
}

/// The part whose skeleton carries a misc node.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MiscParent {
    HullSection(ModelPath),
    Mount(HardpointName),
}

/// Why a ship's parts could not be listed.
#[derive(Debug, Error)]
pub enum ShipPartListingError {
    #[error("the vehicle names no hull model")]
    MissingModel,
    #[error("the vehicle hull model {path:?} is not a model path")]
    InvalidModel { path: String },
}

/// A mount as GameParams places it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ListedMount {
    pub hp: HardpointName,
    pub model: ModelPath,
    pub species: Option<MountSpecies>,
    pub group: PartGroup,
}

/// A misc part the game shows on a ship in battle.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ListedMisc {
    pub node: HardpointName,
    pub parent: MiscParent,
    pub model: ModelPath,
}

impl ListedMisc {
    pub fn key(&self) -> ShipPartKey {
        ShipPartKey::Misc { node: self.node.clone(), parent: self.parent.clone() }
    }
}

/// A ship's parts, read from GameParams and `assets.bin` records without any
/// geometry.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ShipPartListing {
    /// The ship model, then its sections in path order.
    pub hull_sections: Vec<ModelPath>,
    /// Sorted by group, then hardpoint.
    pub mounts: Vec<ListedMount>,
    /// Hull miscs by section, then mount miscs by mount; each by node.
    pub miscs: Vec<ListedMisc>,
}

impl ShipPartListing {
    /// Every part's key: hull sections, mounts, then miscs.
    pub fn keys(&self) -> impl Iterator<Item = ShipPartKey> + '_ {
        let hull = self.hull_sections.iter().cloned().map(ShipPartKey::HullSection);
        let mounts = self.mounts.iter().map(|mount| ShipPartKey::Mount(mount.hp.clone()));
        let miscs = self.miscs.iter().map(ListedMisc::key);

        hull.chain(mounts).chain(miscs)
    }
}

/// One part of a loaded ship.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ShipPart {
    pub key: ShipPartKey,
    /// The `.model` the part shows, absent when the store holds only its visual.
    pub model: Option<ModelPath>,
    pub group: PartGroup,
    /// Where the part's node space sits in ship space, right-handed (Z negated)
    /// like mesh transforms. `None` for hull sections, which are drawn in ship
    /// space. A mount's meshes additionally carry its bone correction, so use
    /// their own transforms to draw them.
    pub placement: Option<[f32; 16]>,
    /// The part's `HP_`, `MP_` and `EP_` nodes in ship space, right-handed.
    pub hardpoints: Vec<Hardpoint>,
}

impl ModelLibrary {
    /// The parts a ship loaded with `hull` and `overrides` is assembled from,
    /// as [`super::ship::ShipExportOptions::hull`] and
    /// [`super::ship::ShipExportOptions::module_overrides`] select them.
    /// Reads no geometry. A misc whose model the store lacks is left out, as
    /// the loader leaves it out.
    pub fn list_ship_parts(
        &self,
        vehicle: &Vehicle,
        hull: Option<&str>,
        overrides: &HashMap<ComponentType, String>,
    ) -> Result<ShipPartListing, Report<ShipPartListingError>> {
        let raw_path = vehicle.model_path().ok_or_else(|| Report::new(ShipPartListingError::MissingModel))?;
        let ship = ModelPath::parse(raw_path)
            .context_with(|| ShipPartListingError::InvalidModel { path: raw_path.to_string() })?;

        let sections = self.catalog().entry(&ship).into_iter().flat_map(ModelEntry::sections).cloned();
        let hull_sections: Vec<ModelPath> = std::iter::once(ship.clone()).chain(sections).collect();

        let db = self.db();
        let upgrade = select_hull_upgrade(vehicle, hull);
        let mut miscs = BTreeSet::new();
        for section in &hull_sections {
            let mut extenders = self.model_extenders(&db, section);
            extenders.extend(battle_section_extenders(&db, self.self_id_index(), upgrade, ship.stem(), section.stem()));
            for node in misc_nodes(&extenders, &db.strings) {
                let Some(model) = self.model_by_stem(&misc_name_from_node(node)) else {
                    continue;
                };
                miscs.insert((MiscParent::HullSection(section.clone()), HardpointName::new(node), model.clone()));
            }
        }

        let hull_visuals: Vec<VisualPrototype> =
            hull_sections.iter().filter_map(|section| self.model_visual(&db, section)).collect();
        let hull_hardpoints: HashSet<&str> =
            hull_visuals.iter().flat_map(|visual| mount_node_names(visual, &db.strings)).collect();

        // A vehicle without hull upgrades mounts nothing; the loader reads it the same way.
        let mount_points = select_hull_mount_points(vehicle, hull, overrides).unwrap_or_default();
        let hull_mount_models: HashMap<&str, &str> = mount_points
            .iter()
            .filter(|mount| hull_hardpoints.contains(mount.hp_name()))
            .map(|mount| (mount.hp_name(), mount.model_path()))
            .collect();

        let mut mounts = Vec::with_capacity(mount_points.len());
        let mut visuals_by_model: HashMap<ModelPath, Option<VisualPrototype>> = HashMap::new();
        let mut extenders_by_model: HashMap<ModelPath, Vec<SkeletonExtender>> = HashMap::new();
        for mount in &mount_points {
            // An empty or malformed model path places nothing; the loader skips it too.
            let Ok(model) = ModelPath::parse(mount.model_path()) else {
                continue;
            };

            // A hardpoint the hull lacks sits on another mount's model; the loader
            // drops one that resolves on neither.
            if !hull_hardpoints.contains(mount.hp_name()) {
                let Some((parent_hp, child_hp)) = split_compound_hp(mount.hp_name(), hull_hardpoints.iter().copied())
                else {
                    continue;
                };
                let Some(parent_model) = hull_mount_models.get(parent_hp).and_then(|path| ModelPath::parse(path).ok())
                else {
                    continue;
                };
                let parent_visual =
                    visuals_by_model.entry(parent_model).or_insert_with_key(|path| self.model_visual(&db, path));
                let resolves = parent_visual
                    .as_ref()
                    .is_some_and(|visual| visual.find_hardpoint_transform(child_hp, &db.strings).is_some());
                if !resolves {
                    continue;
                }
            }

            let hp = HardpointName::new(mount.hp_name());
            if mount_has_misc_selection(mount.misc_filter(), mount.custom_miscs()) {
                let extenders =
                    extenders_by_model.entry(model.clone()).or_insert_with(|| self.model_extenders(&db, &model));
                for node in misc_nodes(extenders, &db.strings) {
                    let misc_name = misc_name_from_node(node);
                    if !mount_shows_misc(mount.misc_filter(), mount.custom_miscs(), node, &misc_name) {
                        continue;
                    }
                    let Some(misc_model) = self.model_by_stem(&misc_name) else {
                        continue;
                    };
                    miscs.insert((MiscParent::Mount(hp.clone()), HardpointName::new(node), misc_model.clone()));
                }
            }

            mounts.push(ListedMount {
                hp,
                model,
                species: mount.species(),
                group: PartGroup::from_mount_species(mount.species()),
            });
        }
        mounts.sort_by(|a, b| (a.group, &a.hp).cmp(&(b.group, &b.hp)));

        let miscs = miscs.into_iter().map(|(parent, node, model)| ListedMisc { node, parent, model }).collect();

        Ok(ShipPartListing { hull_sections, mounts, miscs })
    }
}

/// The hull upgrade `hull_selection` names: an upgrade, a substring of one,
/// or a hull component prefix (`"B"`); `None` takes the first upgrade by name.
/// `None` when the vehicle has no upgrades or the selection matches none.
pub(super) fn select_hull_upgrade<'v>(
    vehicle: &'v Vehicle,
    hull_selection: Option<&str>,
) -> Option<&'v HullUpgradeConfig> {
    let upgrades = vehicle.hull_upgrades()?;
    let mut sorted: Vec<_> = upgrades.iter().collect();
    sorted.sort_by_key(|(k, _)| (*k).clone());

    let selected = if let Some(sel) = hull_selection {
        sorted
            .iter()
            .find(|(name, _)| *name == sel || name.to_lowercase().contains(&sel.to_lowercase()))
            .or_else(|| {
                let prefix = format!("{sel}_");
                sorted.iter().find(|(_, config)| {
                    config.component_name(ComponentType::Hull).map(|n| n.starts_with(&prefix)).unwrap_or(false)
                })
            })
            .copied()
    } else {
        sorted.first().copied()
    };

    selected.map(|(_, config)| config)
}

/// Mount points for the hull upgrade [`select_hull_upgrade`] picks, with
/// optional module overrides. A hardpoint two components name is kept once.
pub(super) fn select_hull_mount_points(
    vehicle: &Vehicle,
    hull_selection: Option<&str>,
    module_overrides: &HashMap<ComponentType, String>,
) -> Option<Vec<MountPoint>> {
    let config = select_hull_upgrade(vehicle, hull_selection)?;
    let mount_points: Vec<MountPoint> = if module_overrides.is_empty() {
        config.all_mount_points().cloned().collect()
    } else {
        config.mount_points_with_overrides(module_overrides).cloned().collect()
    };

    let mut seen = HashSet::new();
    Some(mount_points.into_iter().filter(|mount| seen.insert(mount.hp_name().to_string())).collect())
}

/// The skeleton extenders `upgrade`'s hull component adds in battle to the
/// hull section `section_stem` of the ship model `ship_stem`. Empty for the
/// ship model itself, a section the component names none for, or a path
/// that does not resolve.
pub(super) fn battle_section_extenders(
    db: &PrototypeDatabase<'_>,
    self_id_index: &HashMap<u64, usize>,
    upgrade: Option<&HullUpgradeConfig>,
    ship_stem: &str,
    section_stem: &str,
) -> Vec<SkeletonExtender> {
    let Some(section) = section_stem.strip_prefix(ship_stem).and_then(|rest| rest.strip_prefix('_')) else {
        return Vec::new();
    };
    let paths = upgrade.and_then(|upgrade| upgrade.battle_skeleton_extenders.get(section));

    paths.into_iter().flatten().filter_map(|path| skeleton_extender_at(db, self_id_index, path)).collect()
}

/// Splits a compound hardpoint such as `HP_AGM_3_HP_AGA_1` at the longest
/// hull hardpoint it extends, into that parent and the child node on the
/// parent's mount model.
pub(super) fn split_compound_hp<'n, 'h>(
    hp_name: &'n str,
    hull_hardpoints: impl IntoIterator<Item = &'h str>,
) -> Option<(&'h str, &'n str)> {
    let parent = hull_hardpoints
        .into_iter()
        .filter(|hull_hp| hp_name.strip_prefix(hull_hp).is_some_and(|rest| rest.len() > 1 && rest.starts_with('_')))
        .max_by_key(|hull_hp| hull_hp.len())?;

    Some((parent, &hp_name[parent.len() + 1..]))
}

/// The `HP_` node names of `visual`.
fn mount_node_names<'s>(visual: &VisualPrototype, strings: &'s StringsSection<'_>) -> Vec<&'s str> {
    visual
        .nodes
        .name_map_name_ids
        .iter()
        .filter_map(|&name_id| strings.get_string_by_id(name_id))
        .filter(|name| name.starts_with("HP_"))
        .collect()
}

/// Whether a mount names any misc to show: a `miscFilter` whitelist or a
/// battle-preset `customMiscs` list.
pub(super) fn mount_has_misc_selection(misc_filter: &[String], custom_miscs: &HashMap<String, Vec<String>>) -> bool {
    !misc_filter.is_empty() || custom_miscs.contains_key(MISC_PRESET_BATTLE)
}

/// Whether a mount shows the misc at `node`, as the game's `MiscsController`
/// decides: the node is whitelisted by name, or its misc is in the battle preset.
pub(super) fn mount_shows_misc(
    misc_filter: &[String],
    custom_miscs: &HashMap<String, Vec<String>>,
    node: &str,
    misc_name: &str,
) -> bool {
    misc_filter.iter().any(|name| name == node)
        || custom_miscs.get(MISC_PRESET_BATTLE).is_some_and(|names| names.iter().any(|name| name == misc_name))
}

/// The `MP_` node names `extenders` add.
fn misc_nodes<'s>(extenders: &[SkeletonExtender], strings: &'s StringsSection<'_>) -> Vec<&'s str> {
    extenders
        .iter()
        .flat_map(|extender| extender.name_ids.iter())
        .filter_map(|&name_id| strings.get_string_by_id(name_id))
        .filter(|name| name.starts_with(MISC_NODE_PREFIX))
        .collect()
}

/// Derive the misc model name from a `MP_` skeleton node name.
///
/// Mirrors the game's `MiscFinder.__findMiscsForNodes`: split on `_` and drop
/// the first segment (the `MP` prefix), then remove a trailing instance suffix.
/// A `.NNN` suffix (a dot followed by three digits, matched anywhere) drops the
/// last dot-segment; otherwise a legacy `INDEX_<n>` ending drops the last two
/// `_` segments. E.g. both `MP_BM509_Liferaft_type_20_10ft.001` and
/// `MP_BM509_Liferaft_type_20_10ft_INDEX_1` yield `BM509_Liferaft_type_20_10ft`.
pub(super) fn misc_name_from_node(node_name: &str) -> String {
    let mut segments = node_name.split('_');
    segments.next();
    let rest: String = segments.collect::<Vec<_>>().join("_");

    if contains_dot_three_digits(&rest) {
        if let Some(pos) = rest.rfind('.') {
            return rest[..pos].to_string();
        }
    } else if ends_with_legacy_index(&rest) {
        let segs: Vec<&str> = rest.split('_').collect();
        if segs.len() >= 2 {
            return segs[..segs.len() - 2].join("_");
        }
    }

    rest
}

/// Whether `s` contains a `.` immediately followed by at least three ASCII digits
/// (the game's `.*?\.(\d\d\d)` instance-suffix pattern).
fn contains_dot_three_digits(s: &str) -> bool {
    let bytes = s.as_bytes();
    bytes
        .iter()
        .enumerate()
        .any(|(i, &b)| b == b'.' && bytes.get(i + 1..i + 4).is_some_and(|d| d.iter().all(u8::is_ascii_digit)))
}

/// Whether `s` ends with `INDEX_<digits>` (the game's `^.*INDEX_\d+$` legacy pattern).
fn ends_with_legacy_index(s: &str) -> bool {
    match s.rfind("INDEX_") {
        Some(pos) => {
            let tail = &s[pos + "INDEX_".len()..];
            !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit())
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn misc_name_strips_prefix_and_dot_index() {
        assert_eq!(misc_name_from_node("MP_BM509_Liferaft_type_20_10ft"), "BM509_Liferaft_type_20_10ft");
        assert_eq!(misc_name_from_node("MP_BM509_Liferaft_type_20_10ft.001"), "BM509_Liferaft_type_20_10ft");
        // Only the last dot-segment is dropped when a `.NNN` group is present.
        assert_eq!(misc_name_from_node("MP_Foo.001.999"), "Foo.001");
    }

    #[test]
    fn misc_name_strips_legacy_index() {
        assert_eq!(misc_name_from_node("MP_BM509_Liferaft_type_20_10ft_INDEX_1"), "BM509_Liferaft_type_20_10ft");
        assert_eq!(misc_name_from_node("MP_BM800_Ventilators_mushroom_INDEX_16"), "BM800_Ventilators_mushroom");
    }

    #[test]
    fn misc_name_keeps_non_instance_suffixes() {
        // A two-digit dotted tail is not the `.NNN` instance pattern.
        assert_eq!(misc_name_from_node("MP_Foo.01"), "Foo.01");
        // `INDEX_` with a non-numeric tail is not the legacy pattern.
        assert_eq!(misc_name_from_node("MP_Foo_INDEX_x"), "Foo_INDEX_x");
        // Propeller shares the same rule.
        assert_eq!(misc_name_from_node("MP_CM003_Propeller5_L"), "CM003_Propeller5_L");
    }

    #[test]
    fn contains_dot_three_digits_matches_game_pattern() {
        assert!(contains_dot_three_digits("Foo.001"));
        assert!(contains_dot_three_digits("Foo.0012")); // >=3 digits after the dot
        assert!(!contains_dot_three_digits("Foo.01"));
        assert!(!contains_dot_three_digits("Foo_1"));
    }

    #[test]
    fn a_mount_shows_whitelisted_nodes_and_battle_preset_miscs() {
        let filter = vec!["MP_AM100_Rangefinder_1".to_string()];
        let customs = HashMap::from([
            ("battle".to_string(), vec!["AM200_Light".to_string()]),
            ("dock".to_string(), vec!["AM300_Flag".to_string()]),
        ]);

        assert!(mount_has_misc_selection(&filter, &HashMap::new()));
        assert!(mount_has_misc_selection(&[], &customs));
        assert!(!mount_has_misc_selection(&[], &HashMap::new()));

        assert!(mount_shows_misc(&filter, &customs, "MP_AM100_Rangefinder_1", "AM100_Rangefinder"));
        assert!(mount_shows_misc(&filter, &customs, "MP_AM200_Light.001", "AM200_Light"));
        assert!(!mount_shows_misc(&filter, &customs, "MP_AM300_Flag", "AM300_Flag"), "dock-only extras stay hidden");
    }

    #[test]
    fn a_compound_hardpoint_splits_at_the_longest_hull_hardpoint() {
        let hull = ["HP_AGM_3", "HP_AG", "HP_AGM_3_HP", "HP_AGS_1"];

        assert_eq!(split_compound_hp("HP_AGM_3_HP_AGA_1", hull), Some(("HP_AGM_3_HP", "AGA_1")));
        assert_eq!(split_compound_hp("HP_AGM_3_HP_AGA_1", ["HP_AGM_3", "HP_AG"]), Some(("HP_AGM_3", "HP_AGA_1")));
        assert_eq!(split_compound_hp("HP_AGM_3", hull), None, "a hull hardpoint is not compound");
        assert_eq!(split_compound_hp("HP_AGMX_1", ["HP_AGM"]), None, "the parent ends at an underscore");
    }
}
