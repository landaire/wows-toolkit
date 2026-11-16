//! The armor of a ship without its render geometry.
//!
//! A ship's armor triangles sit inside its `.geometry` files next to the render
//! buffers, and placing its mounts needs `assets.bin`. Together those are several
//! gigabytes per build. This module keeps only what armor queries read: the
//! armor triangles of each hull model and mount model, the hull's hardpoint
//! nodes, and where every mount of every hull upgrade sits.
//!
//! Thickness is not stored. [`VehicleArmor::interactive_meshes`] resolves it
//! from GameParams the same way [`super::ship::ShipModelContext::interactive_armor_meshes`]
//! does, so an extract only changes when the models change.
//!
//! Each object is a versioned header followed by an rkyv payload. Readers
//! refuse any other [`FORMAT_VERSION`] instead of misreading it.

use std::collections::BTreeMap;
use std::collections::HashMap;

use rootcause::prelude::*;
use thiserror::Error;
use wows_core::units::Degrees;
use wows_core::units::ShipModelDistance;

use crate::game_params::types::ArmorMap;
use crate::game_params::types::MountPoint;
use crate::game_params::types::Vehicle;
use crate::models::geometry;
use crate::models::geometry::ArmorModel;
use crate::models::geometry::ArmorTriangle;

use super::gltf_export::InteractiveArmorMesh;
use super::ship::ModelLeaves;
use super::ship::ShipAssets;
use super::ship::hull_hardpoint_transforms;

/// Bump on any change to the encoded types or to what the extractor writes.
pub const FORMAT_VERSION: u32 = 1;

const HEADER_LEN: usize = 4 + std::mem::size_of::<u32>();

/// What kind of object a byte sequence holds, by its magic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectKind {
    Vehicle,
    MountModel,
}

impl ObjectKind {
    const fn magic(self) -> [u8; 4] {
        match self {
            Self::Vehicle => *b"WUAV",
            Self::MountModel => *b"WUAM",
        }
    }
}

#[derive(Debug, Error)]
pub enum ArmorExtractError {
    #[error("armor extract is {len} bytes, shorter than its header")]
    Truncated { len: usize },

    #[error("not a {expected:?} armor extract")]
    WrongKind { expected: ObjectKind },

    #[error("armor extract format version {found} is not the supported version {supported}")]
    UnsupportedVersion { found: u32, supported: u32 },

    #[error("armor extract payload is malformed")]
    Payload {
        #[source]
        source: rkyv::rancor::Error,
    },

    #[error("hull upgrade {upgrade} names hull model {index:?}, which the extract does not hold")]
    MissingHullModel { upgrade: String, index: HullModelIndex },
}

/// One armor triangle: its corners in the model's own (left-handed) space and
/// the GameParams key parts that give its thickness.
#[derive(Debug, Clone, Copy, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct ArmorFace {
    pub vertices: [[f32; 3]; 3],
    pub material_id: u8,
    pub layer_index: u8,
}

/// One armor model of a `.geometry` file.
#[derive(Debug, Clone, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct ArmorModelFaces {
    pub name: String,
    pub faces: Vec<ArmorFace>,
}

impl ArmorModelFaces {
    pub fn from_armor_model(model: &ArmorModel) -> Self {
        let faces = model
            .triangles
            .iter()
            .map(|triangle| ArmorFace {
                vertices: triangle.vertices,
                material_id: triangle.material_id,
                layer_index: triangle.layer_index,
            })
            .collect();

        Self { name: model.name.clone(), faces }
    }

    /// The model as [`crate::models::geometry::parse_geometry`] would return it.
    /// Every vertex normal is the face normal: the source's vertex normals are
    /// not kept, and armor queries use the face normal.
    pub fn to_armor_model(&self) -> ArmorModel {
        let triangles = self
            .faces
            .iter()
            .map(|face| {
                let normal = face_normal(&face.vertices);
                ArmorTriangle {
                    vertices: face.vertices,
                    normals: [normal; 3],
                    material_id: face.material_id,
                    layer_index: face.layer_index,
                }
            })
            .collect();

        ArmorModel { name: self.name.clone(), triangles }
    }
}

/// A hull visual node whose name starts with `HP_`.
#[derive(Debug, Clone, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct Hardpoint {
    pub name: String,

    /// Column-major node matrix in the model's own space: +X starboard, +Y up,
    /// +Z toward the bow, in ship-model units.
    pub transform: [f32; 16],
}

impl Hardpoint {
    pub fn position(&self) -> ModelPosition {
        let matrix = &self.transform;

        ModelPosition {
            starboard: ShipModelDistance::new(matrix[12]),
            up: ShipModelDistance::new(matrix[13]),
            forward: ShipModelDistance::new(matrix[14]),
        }
    }

    /// Rotation of the node's forward (+Z) axis about +Y. Zero faces the bow,
    /// positive turns toward starboard (clockwise seen from above), in the
    /// range (-180, 180].
    pub fn yaw(&self) -> Degrees {
        let matrix = &self.transform;
        let degrees = matrix[8].atan2(matrix[10]).to_degrees();

        // atan2 returns -180 for a node facing straight aft; the range is
        // half-open at -180.
        if degrees <= -180.0 { Degrees::from(180.0) } else { Degrees::from(degrees) }
    }
}

/// A position in a hull model's own space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelPosition {
    pub starboard: ShipModelDistance,
    pub up: ShipModelDistance,
    pub forward: ShipModelDistance,
}

/// One mount of a hull upgrade, placed on the hull.
#[derive(Debug, Clone, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct Mount {
    pub hp_name: String,

    /// The mount's `.model` path as GameParams names it, which is also the
    /// key of its [`MountModelArmor`].
    pub model_path: String,

    /// Column-major hardpoint transform in the hull's own space, composed with
    /// the parent mount's for a hardpoint that sits on another mount.
    pub transform: [f32; 16],
}

/// The hull model and mounts one hull upgrade fits.
#[derive(Debug, Clone, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct HullUpgradeArmor {
    /// The upgrade's GameParams name, e.g. "PJUH911_Yamato_1944".
    pub upgrade: String,

    pub hull_model: HullModelIndex,

    /// Mounts whose model carries armor. A mount without armor cannot be hit by
    /// an armor query and is left out.
    pub mounts: Vec<Mount>,
}

/// A position in [`VehicleArmor::hull_models`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct HullModelIndex(pub usize);

/// The armor of one hull model: every armor model in every `.visual` under the
/// model's directory, and the hardpoint nodes of those visuals.
#[derive(Debug, Clone, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct HullModelArmor {
    /// The hull `.model` path, e.g. "content/gameplay/japan/ship/battleship/JSB018_Yamato_1944/JSB018_Yamato_1944.model".
    pub model_path: String,
    pub armor: Vec<ArmorModelFaces>,
    pub hardpoints: Vec<Hardpoint>,
}

/// Everything about one ship's armor except its mount models, which ships
/// share and which are stored once each as [`MountModelArmor`].
#[derive(Debug, Clone, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct VehicleArmor {
    /// The ship's GameParams index, e.g. "PJSB018".
    pub param_index: String,
    pub hull_models: Vec<HullModelArmor>,

    /// Sorted by upgrade name.
    pub hull_upgrades: Vec<HullUpgradeArmor>,
}

impl VehicleArmor {
    /// The hull upgrade named `upgrade`, or the first by name when `None`, which
    /// is the stock hull the toolkit's armor viewer opens.
    pub fn hull_upgrade(&self, upgrade: Option<&str>) -> Option<&HullUpgradeArmor> {
        match upgrade {
            Some(name) => self.hull_upgrades.iter().find(|hull| hull.upgrade == name),
            None => self.hull_upgrades.first(),
        }
    }

    /// The armor meshes of `hull`, with thickness from `vehicle`'s GameParams,
    /// as [`super::ship::ShipModelContext::interactive_armor_meshes`] builds them
    /// when the upgrade's hull model is the vehicle's model (the context always
    /// loads the vehicle's model). Mounts come in hardpoint name order.
    /// `mount_model` looks a mount model up by path. A mount whose model it does
    /// not return is skipped, as the context skips a turret it cannot load.
    pub fn interactive_meshes<'a>(
        &self,
        hull: &HullUpgradeArmor,
        vehicle: &Vehicle,
        mount_model: impl Fn(&str) -> Option<&'a MountModelArmor>,
    ) -> Result<Vec<InteractiveArmorMesh>, ArmorExtractError> {
        let Some(hull_model) = self.hull_models.get(hull.hull_model.0) else {
            return Err(ArmorExtractError::MissingHullModel { upgrade: hull.upgrade.clone(), index: hull.hull_model });
        };
        let armor_map = vehicle.armor();

        let mut meshes: Vec<InteractiveArmorMesh> = hull_model
            .armor
            .iter()
            .map(|model| InteractiveArmorMesh::from_armor_model(&model.to_armor_model(), armor_map, None))
            .collect();

        let mount_armor: HashMap<&str, &ArmorMap> = vehicle
            .hull_upgrade(&hull.upgrade)
            .into_iter()
            .flat_map(|config| config.all_mount_points())
            .filter_map(|point| point.mount_armor().map(|armor| (point.hp_name(), armor)))
            .collect();

        for mount in &hull.mounts {
            let Some(model) = mount_model(&mount.model_path) else {
                continue;
            };
            let mount_armor = mount_armor.get(mount.hp_name.as_str()).copied();
            for armor in &model.armor {
                let mesh = InteractiveArmorMesh::from_armor_model(&armor.to_armor_model(), armor_map, mount_armor);
                meshes.push(mesh.placed_at_mount(&mount.hp_name, mount.transform));
            }
        }

        Ok(meshes)
    }
}

/// The armor models of one mount `.model`.
#[derive(Debug, Clone, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct MountModelArmor {
    pub model_path: String,
    pub armor: Vec<ArmorModelFaces>,
}

/// A vehicle's armor and the mount models its hull upgrades place.
#[derive(Debug, Clone, PartialEq)]
pub struct VehicleArmorExtract {
    pub vehicle: VehicleArmor,

    /// Sorted by model path. Only models that carry armor.
    pub mount_models: Vec<MountModelArmor>,
}

impl ShipAssets {
    /// Extracts the armor of every hull upgrade of `vehicle`, placing mounts
    /// with the same code [`super::ship::ShipModelContext::interactive_armor_meshes`]
    /// uses. Each upgrade's hull is the model its hull component names, falling
    /// back to the vehicle's model.
    pub fn vehicle_armor(&self, param_index: &str, vehicle: &Vehicle) -> Result<VehicleArmorExtract, Report> {
        let Some(default_model) = vehicle.model_path() else {
            bail!("vehicle {param_index} has no model path");
        };
        let db = self.db()?;
        let self_id_index = db.build_self_id_index();
        let leaves = ModelLeaves::new(&db);

        let Some(upgrades) = vehicle.hull_upgrades().filter(|upgrades| !upgrades.is_empty()) else {
            bail!("vehicle {param_index} has no hull upgrades");
        };
        let mut upgrades: Vec<_> = upgrades.iter().collect();
        upgrades.sort_by(|a, b| a.0.cmp(b.0));

        let mut hull_models: Vec<HullModelArmor> = Vec::new();
        let mut hull_parts = Vec::new();
        let mut mount_models: BTreeMap<String, MountModelArmor> = BTreeMap::new();
        let mut hull_upgrades = Vec::new();
        for (upgrade, config) in upgrades {
            let model_path = config.hull_model_path().unwrap_or(default_model);
            let hull_model = match hull_models.iter().position(|hull| hull.model_path == model_path) {
                Some(index) => HullModelIndex(index),
                None => {
                    let model_dir = model_dir_of(model_path);
                    let visual_paths = self.find_visual_paths(&db, &self_id_index, model_dir);
                    if visual_paths.is_empty() {
                        bail!("no .visual files for hull model {model_path} of {param_index}");
                    }
                    let parts = self.load_sub_models(&db, &self_id_index, &leaves, &visual_paths)?;

                    let mut armor = Vec::new();
                    for part in &parts {
                        let geom = geometry::parse_geometry(&part.geom_bytes)
                            .context_with(|| format!("hull geometry of {model_path}"))?;
                        armor.extend(geom.armor_models.iter().map(ArmorModelFaces::from_armor_model));
                    }

                    let mut hardpoints: Vec<Hardpoint> = hull_hardpoint_transforms(&db, &parts)
                        .into_iter()
                        .map(|(name, transform)| Hardpoint { name, transform })
                        .collect();
                    hardpoints.sort_by(|a, b| a.name.cmp(&b.name));

                    hull_models.push(HullModelArmor { model_path: model_path.to_owned(), armor, hardpoints });
                    hull_parts.push(parts);
                    HullModelIndex(hull_models.len() - 1)
                }
            };

            let mount_points: Vec<MountPoint> = config.all_mount_points().cloned().collect();
            let loaded = self.load_mounts(&db, &self_id_index, &leaves, &mount_points, &hull_parts[hull_model.0])?;

            let mut model_paths: Vec<Option<&str>> = vec![None; loaded.turret_models.len()];
            for (path, &index) in &loaded.turret_model_index {
                model_paths[index] = Some(path);
                if mount_models.contains_key(path) {
                    continue;
                }
                let geom = geometry::parse_geometry(&loaded.turret_models[index].geom_bytes)
                    .context_with(|| format!("mount geometry of {path}"))?;
                let armor = geom.armor_models.iter().map(ArmorModelFaces::from_armor_model).collect();
                mount_models.insert(path.clone(), MountModelArmor { model_path: path.clone(), armor });
            }

            let mut mounts: Vec<Mount> = loaded
                .mounts
                .iter()
                .filter_map(|mount| {
                    let model_path = model_paths[mount.turret_model_index]?;
                    let has_armor = mount_models.get(model_path).is_some_and(|model| !model.armor.is_empty());
                    has_armor.then(|| Mount {
                        hp_name: mount.hp_name.clone(),
                        model_path: model_path.to_owned(),
                        transform: mount.armor_transform,
                    })
                })
                .collect();
            // Mount points come out of a hash map; sorting keeps an unchanged
            // ship's object byte-identical across runs.
            mounts.sort_by(|a, b| a.hp_name.cmp(&b.hp_name));

            hull_upgrades.push(HullUpgradeArmor { upgrade: upgrade.clone(), hull_model, mounts });
        }

        let vehicle = VehicleArmor { param_index: param_index.to_owned(), hull_models, hull_upgrades };
        let mount_models = mount_models.into_values().filter(|model| !model.armor.is_empty()).collect();

        Ok(VehicleArmorExtract { vehicle, mount_models })
    }
}

/// The directory a `.model` path sits in, which is what names a hull's visuals.
fn model_dir_of(model_path: &str) -> &str {
    let dir = model_path.rsplit_once('/').map_or(model_path, |(dir, _)| dir);

    dir.rsplit('/').next().unwrap_or(dir)
}

pub fn encode_vehicle(armor: &VehicleArmor) -> Vec<u8> {
    encode(ObjectKind::Vehicle, armor)
}

pub fn decode_vehicle(bytes: &[u8]) -> Result<VehicleArmor, ArmorExtractError> {
    let payload = payload(ObjectKind::Vehicle, bytes)?;

    rkyv::from_bytes::<VehicleArmor, rkyv::rancor::Error>(&payload)
        .map_err(|source| ArmorExtractError::Payload { source })
}

pub fn encode_mount_model(armor: &MountModelArmor) -> Vec<u8> {
    encode(ObjectKind::MountModel, armor)
}

pub fn decode_mount_model(bytes: &[u8]) -> Result<MountModelArmor, ArmorExtractError> {
    let payload = payload(ObjectKind::MountModel, bytes)?;

    rkyv::from_bytes::<MountModelArmor, rkyv::rancor::Error>(&payload)
        .map_err(|source| ArmorExtractError::Payload { source })
}

fn encode<T>(kind: ObjectKind, value: &T) -> Vec<u8>
where
    T: for<'a> rkyv::Serialize<
            rkyv::api::high::HighSerializer<
                rkyv::util::AlignedVec,
                rkyv::ser::allocator::ArenaHandle<'a>,
                rkyv::rancor::Error,
            >,
        >,
{
    // Serializing owned plain data into a growable buffer has no failure mode
    // short of allocation failure, which aborts anyway.
    let payload = rkyv::to_bytes::<rkyv::rancor::Error>(value).expect("armor extract serializes");

    let mut bytes = Vec::with_capacity(HEADER_LEN + payload.len());
    bytes.extend_from_slice(&kind.magic());
    bytes.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    bytes.extend_from_slice(&payload);

    bytes
}

/// The rkyv payload of `bytes`, copied to an aligned buffer: callers hand in
/// slices of any alignment.
fn payload(kind: ObjectKind, bytes: &[u8]) -> Result<rkyv::util::AlignedVec, ArmorExtractError> {
    let Some((header, payload)) = bytes.split_at_checked(HEADER_LEN) else {
        return Err(ArmorExtractError::Truncated { len: bytes.len() });
    };
    if header[..4] != kind.magic() {
        return Err(ArmorExtractError::WrongKind { expected: kind });
    }

    let version = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
    if version != FORMAT_VERSION {
        return Err(ArmorExtractError::UnsupportedVersion { found: version, supported: FORMAT_VERSION });
    }

    let mut aligned = rkyv::util::AlignedVec::with_capacity(payload.len());
    aligned.extend_from_slice(payload);

    Ok(aligned)
}

fn face_normal(vertices: &[[f32; 3]; 3]) -> [f32; 3] {
    let [a, b, c] = vertices;
    let edge1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let edge2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let cross = [
        edge1[1] * edge2[2] - edge1[2] * edge2[1],
        edge1[2] * edge2[0] - edge1[0] * edge2[2],
        edge1[0] * edge2[1] - edge1[1] * edge2[0],
    ];
    let length = (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt();

    // A degenerate triangle has no normal; zero stands in for one. Ray casts
    // never hit such a triangle.
    if length == 0.0 { [0.0; 3] } else { [cross[0] / length, cross[1] / length, cross[2] / length] }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mount_model() -> MountModelArmor {
        MountModelArmor {
            model_path: "content/gameplay/japan/gun/main/JGM178/JGM178.model".to_owned(),
            armor: vec![ArmorModelFaces {
                name: "CM_PA_turret".to_owned(),
                faces: vec![ArmorFace {
                    vertices: [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                    material_id: 42,
                    layer_index: 1,
                }],
            }],
        }
    }

    #[test]
    fn a_mount_model_round_trips() {
        let model = mount_model();

        let decoded = decode_mount_model(&encode_mount_model(&model)).expect("own encoding decodes");

        assert_eq!(decoded, model);
    }

    #[test]
    fn another_format_version_is_refused() {
        let mut bytes = encode_mount_model(&mount_model());
        bytes[4..8].copy_from_slice(&(FORMAT_VERSION + 1).to_le_bytes());

        let error = decode_mount_model(&bytes).expect_err("a newer version is refused");

        assert!(matches!(
            error,
            ArmorExtractError::UnsupportedVersion { found, supported } if found == FORMAT_VERSION + 1 && supported == FORMAT_VERSION
        ));
    }

    #[test]
    fn an_object_of_the_other_kind_is_refused() {
        let bytes = encode_mount_model(&mount_model());

        let error = decode_vehicle(&bytes).expect_err("a mount model is not a vehicle");

        assert!(matches!(error, ArmorExtractError::WrongKind { expected: ObjectKind::Vehicle }));
        assert!(matches!(decode_vehicle(&bytes[..3]), Err(ArmorExtractError::Truncated { len: 3 })));
    }

    #[test]
    fn hardpoint_yaw_measures_the_forward_axis_from_the_bow() {
        let facing = |forward_x: f32, forward_z: f32| Hardpoint {
            name: "HP_JGM_1".to_owned(),
            transform: [
                forward_z, 0.0, -forward_x, 0.0, 0.0, 1.0, 0.0, 0.0, forward_x, 0.0, forward_z, 0.0, 1.0, 2.0, 3.0, 1.0,
            ],
        };

        assert_eq!(facing(0.0, 1.0).yaw(), Degrees::from(0.0));
        assert_eq!(facing(1.0, 0.0).yaw(), Degrees::from(90.0));
        assert_eq!(facing(0.0, -1.0).yaw(), Degrees::from(180.0));
        assert_eq!(facing(-1.0, 0.0).yaw(), Degrees::from(-90.0));

        let position = facing(0.0, 1.0).position();
        assert_eq!((position.starboard.value(), position.up.value(), position.forward.value()), (1.0, 2.0, 3.0));
    }
}
