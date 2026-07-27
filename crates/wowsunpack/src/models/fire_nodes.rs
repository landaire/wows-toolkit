//! Fire-section (burn node) geometry for a ship hull.
//!
//! A hull's fire sections are `burningFlags` bits `0..N-1`, where `N` is
//! `hull.burnNodes.len()`. GameParams names each section's emitter
//! `HP_FX_Fire_{i+1}` in `A_Hull.effects.fire{i+1}`, but the model calls the same
//! node `EP_Fire_{i+1}` and does not carry it in the `.visual` record at all: the
//! nodes live in the per-section skeleton extenders `<stem>_<Section>_ep.skel_ext`.
//! Each names `Scene Root` as its parent, so every section shares one hull-local
//! space and the node translation is read directly with no mount composition.
//!
//! Node translations are in ship-model units; [`ShipModelDistance`] converts them
//! to meters. See `docs/FIRE_CHANCE.md` section 5.1 for the recovery.

#[cfg(feature = "models")]
use std::collections::HashMap;

use crate::game_params::types::Meters;
#[cfg(feature = "models")]
use crate::game_params::types::ShipModelDistance;
#[cfg(feature = "models")]
use crate::models::assets_bin::PathEntry;
#[cfg(feature = "models")]
use crate::models::assets_bin::PrototypeDatabase;
#[cfg(feature = "models")]
use crate::models::skeleton_extender;

/// Skeleton extenders that carry the effect-point nodes.
#[cfg(feature = "models")]
const EFFECT_EXTENDER_SUFFIX: &str = "_ep.skel_ext";

/// An index that is not a fire section: past the burn bits of `burningFlags`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("a fire section is 0..{max}, got {index}")]
pub struct InvalidBurnNodeIndex {
    pub index: u8,
    pub max: u8,
}

/// The identity of one fire section: its `burningFlags` bit index, which is also
/// its `hull.burnNodes` index and its `fire{i+1}` effect-group ordinal minus one.
///
/// Every construction path is checked, including deserialization, so an
/// out-of-range index cannot exist and [`BurnNodeIndex::bit_mask`] always names a
/// real burn bit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(try_from = "u8", into = "u8"))]
pub struct BurnNodeIndex(u8);

impl TryFrom<u8> for BurnNodeIndex {
    type Error = InvalidBurnNodeIndex;

    fn try_from(index: u8) -> Result<BurnNodeIndex, InvalidBurnNodeIndex> {
        if index >= BurnNodeIndex::MAX_NODES {
            return Err(InvalidBurnNodeIndex { index, max: BurnNodeIndex::MAX_NODES });
        }
        Ok(BurnNodeIndex(index))
    }
}

impl From<BurnNodeIndex> for u8 {
    fn from(index: BurnNodeIndex) -> u8 {
        index.0
    }
}

impl BurnNodeIndex {
    /// Upper bound on fire sections per hull: `burningFlags`' burn mask is `0x000F`.
    pub const MAX_NODES: u8 = 4;

    /// `None` when `index` is outside `0..MAX_NODES`.
    pub fn new(index: u8) -> Option<BurnNodeIndex> {
        BurnNodeIndex::try_from(index).ok()
    }

    pub fn get(self) -> u8 {
        self.0
    }

    /// The `burningFlags` bit this section occupies.
    pub fn bit_mask(self) -> u16 {
        1u16 << self.0
    }
}

/// A node list that cannot describe a hull's fire sections: empty, or longer than
/// there are burn bits in `burningFlags`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("a hull has 1..={max} fire sections, got {count}")]
pub struct InvalidNodeCount {
    pub count: usize,
    pub max: u8,
}

/// Where a hull's fire sections sit along its length.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(try_from = "Vec<Meters>", into = "Vec<Meters>"))]
pub struct FireSectionGeometry {
    /// Longitudinal offset of each burn node from the hull origin, positive
    /// toward the bow. Index is the `burningFlags` bit index. Stored in meters,
    /// not model units, so consumers never need the model scale.
    longitudinal: Vec<Meters>,
}

impl TryFrom<Vec<Meters>> for FireSectionGeometry {
    type Error = InvalidNodeCount;

    fn try_from(longitudinal: Vec<Meters>) -> Result<FireSectionGeometry, InvalidNodeCount> {
        let count = longitudinal.len();
        if count == 0 || count > BurnNodeIndex::MAX_NODES as usize {
            return Err(InvalidNodeCount { count, max: BurnNodeIndex::MAX_NODES });
        }
        Ok(FireSectionGeometry { longitudinal })
    }
}

impl From<FireSectionGeometry> for Vec<Meters> {
    fn from(geometry: FireSectionGeometry) -> Vec<Meters> {
        geometry.longitudinal
    }
}

impl FireSectionGeometry {
    /// `None` when the list is empty or longer than [`BurnNodeIndex::MAX_NODES`].
    pub fn from_longitudinal(longitudinal: Vec<Meters>) -> Option<FireSectionGeometry> {
        FireSectionGeometry::try_from(longitudinal).ok()
    }

    pub fn node_count(&self) -> usize {
        self.longitudinal.len()
    }

    pub fn longitudinal(&self) -> &[Meters] {
        &self.longitudinal
    }

    /// Burn node whose longitudinal position is closest to `offset`, an offset
    /// from the hull origin toward the bow. Ties go to the bow-ward node.
    pub fn nearest_node(&self, offset: Meters) -> BurnNodeIndex {
        let mut best = 0usize;
        let mut best_distance = f32::INFINITY;
        for (index, position) in self.longitudinal.iter().enumerate() {
            let distance = (position.value() - offset.value()).abs();
            if distance < best_distance {
                best_distance = distance;
                best = index;
            }
        }
        // `longitudinal` is never empty (the constructor rejects a zero count) and
        // its length is bounded by MAX_NODES, so this index is always valid.
        BurnNodeIndex::new(best as u8).expect("index bounded by node count")
    }
}

#[cfg(feature = "models")]
#[derive(Debug, thiserror::Error)]
pub enum FireNodeError {
    #[error("hull {hull} has no burn nodes")]
    NoBurnNodes { hull: String },
    #[error("hull {hull}: {expected} burn nodes exceeds the {max} burningFlags bits")]
    TooManyBurnNodes { hull: String, expected: usize, max: u8 },
    #[error("hull {hull} is not in assets.bin")]
    NoModelEntry { hull: String },
    #[error("hull {hull}: skeleton extender {extender} could not be read: {detail}")]
    ExtenderUnreadable { hull: String, extender: String, detail: String },
    #[error("hull {hull}: fire node EP_Fire_{ordinal} is defined by more than one skeleton extender")]
    DuplicateNode { hull: String, ordinal: u8 },
    #[error("hull {hull}: fire node EP_Fire_{ordinal} is missing")]
    MissingNode { hull: String, ordinal: u8 },
    #[error("hull {hull}: expected {expected} fire nodes, found {found}")]
    NodeCountMismatch { hull: String, expected: usize, found: usize },
}

/// Resolve the fire-section geometry for one hull.
///
/// `hull_model_path` is the hull's `.model` VFS path and `expected_nodes` is its
/// `burnNodes` length; a hull that yields any other node count is an error, never
/// a partial result.
///
/// Model space is a fixed 15 m per unit ([`ShipModelDistance`]), so the hull's
/// length is not needed to place the nodes.
#[cfg(feature = "models")]
pub fn resolve_fire_sections(
    db: &PrototypeDatabase<'_>,
    self_id_index: &HashMap<u64, usize>,
    hull_model_path: &str,
    expected_nodes: usize,
) -> Result<FireSectionGeometry, FireNodeError> {
    let hull = || hull_model_path.to_string();
    if expected_nodes == 0 {
        return Err(FireNodeError::NoBurnNodes { hull: hull() });
    }
    if expected_nodes > BurnNodeIndex::MAX_NODES as usize {
        return Err(FireNodeError::TooManyBurnNodes {
            hull: hull(),
            expected: expected_nodes,
            max: BurnNodeIndex::MAX_NODES,
        });
    }

    let leaf = hull_model_path.rsplit('/').next().unwrap_or(hull_model_path);
    let stem = leaf.strip_suffix(".model").unwrap_or(leaf);
    let (entry_index, _) = db
        .find_path_by_suffix(hull_model_path, self_id_index)
        .ok_or_else(|| FireNodeError::NoModelEntry { hull: hull() })?;
    // The sections are the model's siblings. Scoping to its directory keeps a
    // longer-named hull elsewhere in the store from matching the stem prefix.
    let directory_id = db.paths_storage[entry_index].parent_id;
    let prefix = format!("{stem}_");

    // Indexed by ordinal minus one, over every ordinal a hull could use, not just
    // the ones this hull claims: a model carrying more bare-ordinal nodes than the
    // hull declares is a disagreement, and taking the bow-most subset of it would
    // silently place the sections on a different hull's geometry.
    let mut longitudinal: Vec<Option<Meters>> = vec![None; BurnNodeIndex::MAX_NODES as usize];
    for entry in &db.paths_storage {
        if entry.parent_id != directory_id
            || !entry.name.starts_with(&prefix)
            || !entry.name.ends_with(EFFECT_EXTENDER_SUFFIX)
        {
            continue;
        }
        let extender = read_extender(db, entry).map_err(|detail| FireNodeError::ExtenderUnreadable {
            hull: hull(),
            extender: entry.name.clone(),
            detail,
        })?;

        for (node, &name_id) in extender.name_ids.iter().enumerate() {
            let Some(name) = db.strings.get_string_by_id(name_id) else { continue };
            let Some(ordinal) = burn_node_ordinal(name) else { continue };
            // `burningFlags` has four burn bits, so EP_Fire_5 and above cannot be a
            // burn node whatever the hull declares; they are the fireResistance
            // effect group's extra emitters.
            let Some(slot) = (ordinal as usize).checked_sub(1).filter(|slot| *slot < longitudinal.len()) else {
                continue;
            };
            let Some(matrix) = extender.matrices.get(node) else { continue };
            if longitudinal[slot].is_some() {
                return Err(FireNodeError::DuplicateNode { hull: hull(), ordinal });
            }
            longitudinal[slot] = Some(ShipModelDistance::from(matrix.0[14]).to_meters());
        }
    }

    let found = longitudinal.iter().filter(|node| node.is_some()).count();
    // A shortfall and a surplus are both disagreements, with one exception a
    // single-section hull earns from GameParams: such a hull has one `fire1`
    // effect group that owns every emitter it uses, and 38 of the 39 one-section
    // hulls in the live build list both `HP_FX_Fire_1` and `HP_FX_Fire_2` under
    // it. The extra ordinal is a second emitter for the same section, and with
    // one section `nearest_node` has a single answer, so it cannot pick wrong.
    let surplus_is_extra_emitters = expected_nodes == 1 && found > expected_nodes;
    if found != expected_nodes && !surplus_is_extra_emitters {
        return Err(FireNodeError::NodeCountMismatch { hull: hull(), expected: expected_nodes, found });
    }
    // The counts agree, so any gap below `expected_nodes` means an ordinal above it
    // stood in for a missing one and the sections would be misnumbered.
    longitudinal.truncate(expected_nodes);
    let longitudinal = longitudinal
        .iter()
        .enumerate()
        .map(|(slot, node)| node.ok_or(slot))
        .collect::<Result<Vec<Meters>, usize>>()
        .map_err(|slot| FireNodeError::MissingNode { hull: hull(), ordinal: slot as u8 + 1 })?;
    Ok(FireSectionGeometry { longitudinal })
}

/// Read and parse one skeleton-extender record, describing any failure in a way
/// the caller can attach to the extender's name.
#[cfg(feature = "models")]
fn read_extender(db: &PrototypeDatabase<'_>, entry: &PathEntry) -> Result<skeleton_extender::SkeletonExtender, String> {
    let value = db.lookup_r2p(entry.self_id).ok_or_else(|| "not in the resource-to-prototype map".to_string())?;
    let location = db.decode_r2p_value(value).map_err(|e| e.to_string())?;
    let data =
        db.get_prototype_data(location, skeleton_extender::SKELETON_EXTENDER_ITEM_SIZE).map_err(|e| e.to_string())?;
    skeleton_extender::parse_skeleton_extender(data).map_err(|e| e.to_string())
}

/// `EP_Fire_{n}` for a bare integer `n`. Rejects `EP_Fire_2_side_1` and
/// `EP_Fire_5_1`, which are extra emitters rather than burn nodes.
fn burn_node_ordinal(node_name: &str) -> Option<u8> {
    let rest = node_name.strip_prefix("EP_Fire_")?;
    if rest.is_empty() || !rest.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    rest.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Iowa's four nodes as the resolver returns them, in meters from the hull
    /// origin: `EP_Fire_1..4` at model z 6.489, 1.317, -2.480, -6.912.
    fn iowa_like() -> FireSectionGeometry {
        FireSectionGeometry::from_longitudinal(vec![
            Meters::from(97.34),
            Meters::from(19.76),
            Meters::from(-37.20),
            Meters::from(-103.68),
        ])
        .expect("four nodes")
    }

    /// Nearest-node is a 1D partition along the hull. The boundary between the two
    /// bow-most sections is their midpoint, 58.55 m.
    #[test]
    fn nearest_node_partitions_the_hull() {
        let g = iowa_like();
        assert_eq!(g.nearest_node(Meters::from(120.0)).get(), 0);
        assert_eq!(g.nearest_node(Meters::from(93.0)).get(), 0);
        assert_eq!(g.nearest_node(Meters::from(59.0)).get(), 0);
        assert_eq!(g.nearest_node(Meters::from(58.0)).get(), 1);
        assert_eq!(g.nearest_node(Meters::from(0.0)).get(), 1);
        assert_eq!(g.nearest_node(Meters::from(-40.0)).get(), 2);
        assert_eq!(g.nearest_node(Meters::from(-200.0)).get(), 3);
    }

    /// An offset exactly between two nodes ties toward the bow-ward one. This only
    /// matters for reproducibility, not correctness.
    #[test]
    fn nearest_node_ties_toward_the_bow() {
        let g = FireSectionGeometry::from_longitudinal(vec![Meters::from(10.0), Meters::from(-10.0)]).expect("two");
        assert_eq!(g.nearest_node(Meters::from(0.0)).get(), 0);
    }

    #[test]
    fn from_longitudinal_rejects_impossible_node_counts() {
        assert!(FireSectionGeometry::from_longitudinal(Vec::new()).is_none());
        assert!(FireSectionGeometry::from_longitudinal(vec![Meters::from(0.0); 5]).is_none());
        assert!(FireSectionGeometry::from_longitudinal(vec![Meters::from(0.0); 4]).is_some());
    }

    /// Deserializing must not route around either invariant. A cached geometry with
    /// five sections would make `nearest_node` hand out an index `BurnNodeIndex`
    /// cannot represent, and a cached index past the burn bits would make
    /// `bit_mask` name a different section's bit.
    #[cfg(feature = "serde")]
    #[test]
    fn deserializing_rejects_an_impossible_node_count() {
        assert!(serde_json::from_str::<FireSectionGeometry>("[1.0,2.0,3.0,4.0,5.0]").is_err());
        assert!(serde_json::from_str::<FireSectionGeometry>("[]").is_err());
        let round_tripped: FireSectionGeometry = serde_json::from_str("[1.0,2.0]").expect("two nodes");
        assert_eq!(round_tripped.node_count(), 2);

        assert!(serde_json::from_str::<BurnNodeIndex>("200").is_err());
        assert!(serde_json::from_str::<BurnNodeIndex>("4").is_err());
        let index: BurnNodeIndex = serde_json::from_str("3").expect("last section");
        assert_eq!(index.get(), 3);
        assert_eq!(serde_json::to_string(&index).expect("serialize"), "3");
    }

    #[test]
    fn burn_node_index_rejects_out_of_range() {
        assert!(BurnNodeIndex::new(3).is_some());
        assert!(BurnNodeIndex::new(4).is_none());
    }

    /// Bit i of burningFlags is burn node i (BURN_MASK is 0x000F).
    #[test]
    fn burn_node_index_maps_to_its_flag_bit() {
        assert_eq!(BurnNodeIndex::new(0).unwrap().bit_mask(), 0b0001);
        assert_eq!(BurnNodeIndex::new(3).unwrap().bit_mask(), 0b1000);
    }

    /// Only a bare ordinal is a burn node. `EP_Fire_5_1` and the side emitters are
    /// fireResistance decoration, and GameParams' own `HP_FX_` name is not a model
    /// node at all.
    #[test]
    fn burn_node_ordinal_accepts_only_bare_ordinals() {
        assert_eq!(burn_node_ordinal("EP_Fire_1"), Some(1));
        assert_eq!(burn_node_ordinal("EP_Fire_12"), Some(12));
        assert_eq!(burn_node_ordinal("EP_Fire_5_1"), None);
        assert_eq!(burn_node_ordinal("EP_Fire_2_side_1"), None);
        assert_eq!(burn_node_ordinal("EP_Fire_"), None);
        assert_eq!(burn_node_ordinal("HP_FX_Fire_1"), None);
    }
}
