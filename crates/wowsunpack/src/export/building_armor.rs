//! The GameParams `Building` params whose models carry armor plates, with the
//! armor maps that give those plates thickness. Building hulls in current
//! installs carry no plates of their own; a fort's armor is its gun mounts,
//! whose thickness the building's `HP_` entries list as on a ship.

use std::collections::HashMap;

use crate::game_params::provider::GameMetadataProvider;
use crate::game_params::types::ArmorMap;
use crate::game_params::types::Building;
use crate::game_params::types::GameParamProvider;
use crate::game_types::GameParamId;

use super::hardpoint::HardpointName;
use super::model_catalog::BuildingKind;
use super::model_catalog::BuildingParamRef;
use super::model_catalog::ModelPath;
use super::model_library::ModelArmorMaps;
use super::model_library::ModelLibrary;

/// A gun or other mount of a building.
#[derive(Debug, Clone)]
pub struct BuildingMount {
    hardpoint: HardpointName,
    model: ModelPath,
    armor: Option<ArmorMap>,
    has_armor: bool,
}

impl BuildingMount {
    /// The building model's node the mount sits on, e.g. `HP_CGM_1`.
    pub fn hardpoint(&self) -> &HardpointName {
        &self.hardpoint
    }

    pub fn model(&self) -> &ModelPath {
        &self.model
    }

    /// The mount's own `armor` map. `None` when the entry lists none.
    pub fn armor(&self) -> Option<&ArmorMap> {
        self.armor.as_ref()
    }

    /// Whether the mount model's geometry carries armor plates.
    pub fn has_armor(&self) -> bool {
        self.has_armor
    }
}

/// A `Building` param with armor on its hull model or on a mount.
#[derive(Debug, Clone)]
pub struct ArmoredBuilding {
    id: GameParamId,
    param: BuildingParamRef,
    model: Option<ModelPath>,
    armor: Option<ArmorMap>,
    hull_has_armor: bool,
    mounts: Vec<BuildingMount>,
}

impl ArmoredBuilding {
    pub fn id(&self) -> GameParamId {
        self.id
    }

    /// The param's name and kind, e.g. `PCBX001`, a fort.
    pub fn param(&self) -> &BuildingParamRef {
        &self.param
    }

    pub fn kind(&self) -> &BuildingKind {
        self.param.kind()
    }

    /// `hull.model`. `None` when the param names none.
    pub fn model(&self) -> Option<&ModelPath> {
        self.model.as_ref()
    }

    /// `hull.armor`. `None` when the param lists none.
    pub fn armor(&self) -> Option<&ArmorMap> {
        self.armor.as_ref()
    }

    /// Whether the hull model's geometry carries armor plates.
    pub fn hull_has_armor(&self) -> bool {
        self.hull_has_armor
    }

    /// Every mount, sorted by hardpoint.
    pub fn mounts(&self) -> &[BuildingMount] {
        &self.mounts
    }

    /// The maps that give the hull model's plates thickness.
    pub fn hull_armor_maps(&self) -> ModelArmorMaps<'_> {
        ModelArmorMaps { hull: self.armor.as_ref(), mount: None }
    }

    /// The maps for `mount`'s plates: its own map, then the hull's.
    pub fn mount_armor_maps<'a>(&'a self, mount: &'a BuildingMount) -> ModelArmorMaps<'a> {
        ModelArmorMaps { hull: self.armor.as_ref(), mount: mount.armor.as_ref() }
    }
}

/// Every armored building, by kind then name.
#[derive(Debug, Clone, Default)]
pub struct BuildingArmorIndex {
    buildings: Vec<ArmoredBuilding>,
    by_id: HashMap<GameParamId, usize>,
}

impl BuildingArmorIndex {
    /// Reads the armor count of each building and mount model once. A model
    /// whose geometry cannot be read, such as one the install does not ship,
    /// counts as unarmored: there is no plate to show.
    pub fn build(metadata: &GameMetadataProvider, library: &ModelLibrary) -> Self {
        let mut armored_models: HashMap<ModelPath, bool> = HashMap::new();
        let mut has_armor = |path: &ModelPath| {
            *armored_models
                .entry(path.clone())
                .or_insert_with(|| library.model_summary(path).is_ok_and(|summary| summary.has_armor))
        };

        let mut buildings = Vec::new();
        for param in metadata.params() {
            let Some(building) = param.building() else {
                continue;
            };
            let kind = param.species().map_or(BuildingKind::Unspecified, BuildingKind::from_species);
            let param_ref = BuildingParamRef::new(param.name().to_string(), kind);
            let Some(armored) = armored_building(param.id(), param_ref, building, &mut has_armor) else {
                continue;
            };
            buildings.push(armored);
        }
        buildings.sort_by(|a, b| (a.kind(), a.param.name()).cmp(&(b.kind(), b.param.name())));
        let by_id = buildings.iter().enumerate().map(|(idx, building)| (building.id, idx)).collect();

        Self { buildings, by_id }
    }

    pub fn buildings(&self) -> &[ArmoredBuilding] {
        &self.buildings
    }

    pub fn building(&self, id: GameParamId) -> Option<&ArmoredBuilding> {
        self.by_id.get(&id).map(|&idx| &self.buildings[idx])
    }
}

/// The armored view of `building`, or `None` when neither its hull model nor
/// a mount model carries plates.
fn armored_building(
    id: GameParamId,
    param: BuildingParamRef,
    building: &Building,
    has_armor: &mut impl FnMut(&ModelPath) -> bool,
) -> Option<ArmoredBuilding> {
    let model = building.model().and_then(|raw| ModelPath::parse(raw).ok());
    let hull_has_armor = model.as_ref().is_some_and(&mut *has_armor);
    let mounts: Vec<BuildingMount> = building
        .mounts()
        .iter()
        .filter_map(|mount| {
            let model = ModelPath::parse(mount.model_path()).ok()?;
            let has_armor = has_armor(&model);

            Some(BuildingMount {
                hardpoint: HardpointName::new(mount.hp_name()),
                model,
                armor: mount.mount_armor().cloned(),
                has_armor,
            })
        })
        .collect();
    if !hull_has_armor && !mounts.iter().any(BuildingMount::has_armor) {
        return None;
    }

    Some(ArmoredBuilding { id, param, model, armor: building.armor().cloned(), hull_has_armor, mounts })
}
