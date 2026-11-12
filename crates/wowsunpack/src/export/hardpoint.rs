//! Named attachment nodes of a model: the `HP_` nodes of its visual and the
//! `MP_` (misc part) and `EP_` (effect) nodes its skeleton extenders add.

use std::borrow::Borrow;
use std::collections::HashMap;
use std::fmt;

use crate::models::assets_bin::PrototypeDatabase;
use crate::models::assets_bin::StringsSection;
use crate::models::model::ModelPrototype;
use crate::models::skeleton_extender;
use crate::models::skeleton_extender::SkeletonExtender;
use crate::models::visual::VisualPrototype;

use super::gltf_export::negate_z_transform;

/// A model node name, e.g. `HP_AGM_1`, `MP_CM003_Propeller5_L`, `EP_Smoke_1`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct HardpointName(String);

impl HardpointName {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for HardpointName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Borrow<str> for HardpointName {
    fn borrow(&self) -> &str {
        &self.0
    }
}

/// Which node family a hardpoint belongs to, from its name prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum HardpointKind {
    /// `HP_`: where a mount, gun fire effect or child mount attaches.
    Mount,
    /// `MP_`: where a misc part (propeller, boat, fitting) attaches.
    Misc,
    /// `EP_`: where an effect (smoke, fire, wake) is emitted.
    Effect,
}

impl HardpointKind {
    pub fn from_name(name: &str) -> Option<Self> {
        if name.starts_with("HP_") {
            Some(Self::Mount)
        } else if name.starts_with("MP_") {
            Some(Self::Misc)
        } else if name.starts_with("EP_") {
            Some(Self::Effect)
        } else {
            None
        }
    }
}

/// One attachment node with its transform in the space the model's meshes
/// are drawn in: column-major and right-handed (Z negated), as
/// [`super::gltf_export::InteractiveHullMesh`] positions are.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Hardpoint {
    pub name: HardpointName,
    pub kind: HardpointKind,
    pub transform: [f32; 16],
}

impl Hardpoint {
    /// The node origin.
    pub fn position(&self) -> [f32; 3] {
        [self.transform[12], self.transform[13], self.transform[14]]
    }
}

/// Hardpoints in the model's own left-handed space, before placement.
#[derive(Debug, Clone)]
pub(crate) struct ModelSpaceHardpoints {
    hardpoints: Vec<(HardpointName, HardpointKind, [f32; 16])>,
    index: HashMap<HardpointName, usize>,
}

impl ModelSpaceHardpoints {
    pub(crate) fn new() -> Self {
        Self { hardpoints: Vec::new(), index: HashMap::new() }
    }

    /// Adds the `HP_` nodes of `visual` at their composed node transforms.
    pub(crate) fn add_visual(&mut self, visual: &VisualPrototype, strings: &StringsSection<'_>) {
        for &name_id in &visual.nodes.name_map_name_ids {
            let Some(name) = strings.get_string_by_id(name_id) else {
                continue;
            };
            if HardpointKind::from_name(name) != Some(HardpointKind::Mount) {
                continue;
            }
            if let Some(world) = visual.find_hardpoint_transform(name, strings) {
                self.insert(name, HardpointKind::Mount, world);
            }
        }
    }

    /// Adds the `MP_` and `EP_` nodes of `extenders`. A node's parent is named
    /// into the base skeleton, so it is looked up in `visuals`; a parent no
    /// visual holds leaves the local matrix as the model-space transform.
    pub(crate) fn add_extenders(
        &mut self,
        extenders: &[SkeletonExtender],
        visuals: &[&VisualPrototype],
        strings: &StringsSection<'_>,
    ) {
        for extender in extenders {
            let nodes = extender.name_ids.iter().zip(&extender.parent_name_ids).zip(&extender.matrices);
            for ((&name_id, &parent_id), local) in nodes {
                let Some(name) = strings.get_string_by_id(name_id) else {
                    continue;
                };
                let Some(kind) = HardpointKind::from_name(name).filter(|kind| *kind != HardpointKind::Mount) else {
                    continue;
                };

                let parent_world = strings
                    .get_string_by_id(parent_id)
                    .and_then(|parent| visuals.iter().find_map(|v| v.find_hardpoint_transform(parent, strings)));
                let world = match parent_world {
                    Some(parent_world) => mat4_mul(&parent_world, &local.0),
                    None => local.0,
                };
                self.insert(name, kind, world);
            }
        }
    }

    /// Places every hardpoint with `placement` (left-handed, `None` for the
    /// identity) and converts it to the right-handed mesh space. A name met
    /// twice keeps its last transform, as mounts are placed.
    pub(crate) fn placed(&self, placement: Option<&[f32; 16]>) -> Vec<Hardpoint> {
        self.hardpoints
            .iter()
            .map(|(name, kind, world)| {
                let world = match placement {
                    Some(placement) => mat4_mul(placement, world),
                    None => *world,
                };
                Hardpoint { name: name.clone(), kind: *kind, transform: negate_z_transform(world) }
            })
            .collect()
    }

    fn insert(&mut self, name: &str, kind: HardpointKind, world: [f32; 16]) {
        match self.index.get(name) {
            Some(&idx) => self.hardpoints[idx].2 = world,
            None => {
                let name = HardpointName::new(name);
                self.index.insert(name.clone(), self.hardpoints.len());
                self.hardpoints.push((name, kind, world));
            }
        }
    }
}

/// The skeleton extenders a model names. One that does not resolve or parse
/// is skipped: extenders only add hardpoints.
pub(crate) fn model_extenders(db: &PrototypeDatabase<'_>, model: &ModelPrototype) -> Vec<SkeletonExtender> {
    const SKELETON_EXTENDER_BLOB_INDEX: usize = 2;

    model
        .skel_ext_res_ids
        .iter()
        .filter_map(|&id| {
            let location = db.decode_r2p_value(db.lookup_r2p(id)?).ok()?;
            if location.blob_index != SKELETON_EXTENDER_BLOB_INDEX {
                return None;
            }
            let record = db.get_prototype_data(location, skeleton_extender::SKELETON_EXTENDER_ITEM_SIZE).ok()?;
            skeleton_extender::parse_skeleton_extender(record).ok()
        })
        .collect()
}

/// Transform from a model's mesh space into its node space. Mount meshes are
/// skinned to `Rotate_Y_BlendBone`, whose rest rotation (a Z reflection on
/// many turrets) the node tree does not carry, so the inverse of that
/// rotation lines the meshes up with the nodes and the armor. `None` when the
/// model has no such bone and its meshes already sit in node space.
pub(crate) fn mesh_to_node_space(visual: &VisualPrototype, strings: &StringsSection<'_>) -> Option<[f32; 16]> {
    visual.find_node_local_matrix("Rotate_Y_BlendBone", strings).map(|bone| mat4_rotation_inverse(&bone))
}

/// Column-major 4x4 product `a * b`.
pub(crate) fn mat4_mul(a: &[f32; 16], b: &[f32; 16]) -> [f32; 16] {
    let mut out = [0.0f32; 16];
    for col in 0..4 {
        for row in 0..4 {
            out[col * 4 + row] = (0..4).map(|k| a[k * 4 + row] * b[col * 4 + k]).sum();
        }
    }

    out
}

/// Inverse of the rotation part of a rigid column-major transform (its
/// transpose), with no translation.
pub(crate) fn mat4_rotation_inverse(m: &[f32; 16]) -> [f32; 16] {
    [m[0], m[4], m[8], 0.0, m[1], m[5], m[9], 0.0, m[2], m[6], m[10], 0.0, 0.0, 0.0, 0.0, 1.0]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn translation(x: f32, y: f32, z: f32) -> [f32; 16] {
        [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, x, y, z, 1.0]
    }

    #[test]
    fn kind_follows_the_name_prefix() {
        assert_eq!(HardpointKind::from_name("HP_AGM_1"), Some(HardpointKind::Mount));
        assert_eq!(HardpointKind::from_name("MP_CM003_Propeller5_L"), Some(HardpointKind::Misc));
        assert_eq!(HardpointKind::from_name("EP_Smoke_1"), Some(HardpointKind::Effect));
        assert_eq!(HardpointKind::from_name("Rotate_Y"), None);
    }

    #[test]
    fn placement_composes_then_negates_z() {
        let mut hardpoints = ModelSpaceHardpoints::new();
        hardpoints.insert("HP_AGA_1", HardpointKind::Mount, translation(0.25, 0.5, -0.5));
        // A repeated name keeps the later node, as mount placement does.
        hardpoints.insert("HP_AGA_2", HardpointKind::Mount, translation(9.0, 9.0, 9.0));
        hardpoints.insert("HP_AGA_2", HardpointKind::Mount, translation(0.0, 0.5, -0.5));

        let placed = hardpoints.placed(Some(&translation(0.0, 1.0, 10.0)));
        let positions: Vec<(&str, [f32; 3])> = placed.iter().map(|h| (h.name.as_str(), h.position())).collect();
        assert_eq!(positions, [("HP_AGA_1", [0.25, 1.5, -9.5]), ("HP_AGA_2", [0.0, 1.5, -9.5])]);
    }
}
