//! Which ships mount each model, with the GameParams armor maps that give its
//! plates thickness. A mount model carries its armor geometry, but thickness
//! is set per ship and hardpoint, so a mount opened on its own borrows it from
//! a ship that mounts it.

use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;

use crate::game_params::keys::ComponentType;
use crate::game_params::provider::GameMetadataProvider;
use crate::game_params::types::ArmorMap;
use crate::game_params::types::GameParamProvider;
use crate::game_params::types::HullUpgradeConfig;
use crate::game_params::types::MountPoint;
use crate::game_types::GameParamId;

use super::hardpoint::HardpointName;
use super::model_catalog::ModelCatalog;
use super::model_catalog::ModelPath;

/// A vehicle's GameParams index, e.g. `PASB018`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VehicleParamIndex(String);

impl VehicleParamIndex {
    pub fn new(index: impl Into<String>) -> Self {
        Self(index.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for VehicleParamIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Every model GameParams mounts on a ship, with each ship and hardpoint that
/// mounts it. Built once; owns its data.
#[derive(Debug, Clone, Default)]
pub struct MountArmorIndex {
    by_model: HashMap<ModelPath, Vec<MountArmorRef>>,
}

impl MountArmorIndex {
    /// Indexes the mounts of every hull upgrade and every alternative
    /// component of every vehicle. A skin no ship mounts directly takes the
    /// mounts of the stock models `catalog` links it to, as an exterior swaps
    /// it in at those hardpoints.
    pub fn build(metadata: &GameMetadataProvider, catalog: &ModelCatalog) -> Self {
        let mut by_model: HashMap<ModelPath, Vec<MountArmorRef>> = HashMap::new();

        for param in metadata.params() {
            let Some(vehicle) = param.vehicle() else {
                continue;
            };
            let Some(upgrades) = vehicle.hull_upgrades() else {
                continue;
            };
            let hull_armor = vehicle.armor().cloned().map(Arc::new);

            let mut upgrade_names: Vec<&String> = upgrades.keys().collect();
            upgrade_names.sort();

            // A mount repeats across upgrades; the first upgrade by name wins, as
            // the stock hull is the one the loader picks without a selection.
            let mut seen: HashSet<(HardpointName, ModelPath)> = HashSet::new();
            for config in upgrade_names.into_iter().filter_map(|name| upgrades.get(name)) {
                for mount in upgrade_mounts(config) {
                    // An empty or malformed model path names nothing to look up.
                    let Ok(model) = ModelPath::parse(mount.model_path()) else {
                        continue;
                    };
                    let hp = HardpointName::new(mount.hp_name());
                    if !seen.insert((hp.clone(), model.clone())) {
                        continue;
                    }

                    by_model.entry(model).or_default().push(MountArmorRef {
                        vehicle: param.id(),
                        vehicle_index: VehicleParamIndex::new(param.index()),
                        tier: vehicle.level(),
                        hp,
                        mount_armor: mount.mount_armor().cloned().map(Arc::new),
                        hull_armor: hull_armor.clone(),
                    });
                }
            }
        }

        for refs in by_model.values_mut() {
            refs.sort_by(|a, b| (a.tier, &a.vehicle_index, &a.hp).cmp(&(b.tier, &b.vehicle_index, &b.hp)));
        }

        let mut skin_refs: Vec<(ModelPath, Vec<MountArmorRef>)> = Vec::new();
        for entry in catalog.entries() {
            if entry.skin_of().is_empty() || by_model.contains_key(entry.path()) {
                continue;
            }
            let mut refs: Vec<MountArmorRef> =
                entry.skin_of().iter().filter_map(|stock| by_model.get(stock)).flatten().cloned().collect();
            if refs.is_empty() {
                continue;
            }
            refs.sort_by(|a, b| (a.tier, &a.vehicle_index, &a.hp).cmp(&(b.tier, &b.vehicle_index, &b.hp)));
            skin_refs.push((entry.path().clone(), refs));
        }
        by_model.extend(skin_refs);

        Self { by_model }
    }

    /// Every ship hardpoint that mounts `model`, by tier, then GameParams
    /// index, then hardpoint. Empty for a model no ship mounts.
    pub fn referencing(&self, model: &ModelPath) -> &[MountArmorRef] {
        self.by_model.get(model).map_or(&[], Vec::as_slice)
    }

    /// The mount of `model` at `hp` on the vehicle with GameParams index
    /// `vehicle_index`.
    pub fn find(
        &self,
        model: &ModelPath,
        vehicle_index: &VehicleParamIndex,
        hp: &HardpointName,
    ) -> Option<&MountArmorRef> {
        self.referencing(model).iter().find(|mount| &mount.vehicle_index == vehicle_index && &mount.hp == hp)
    }

    /// Number of distinct mounted models.
    pub fn len(&self) -> usize {
        self.by_model.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_model.is_empty()
    }
}

/// One ship hardpoint that mounts a model.
#[derive(Debug, Clone)]
pub struct MountArmorRef {
    vehicle: GameParamId,
    vehicle_index: VehicleParamIndex,
    tier: u32,
    hp: HardpointName,
    mount_armor: Option<Arc<ArmorMap>>,
    hull_armor: Option<Arc<ArmorMap>>,
}

impl MountArmorRef {
    pub fn vehicle(&self) -> GameParamId {
        self.vehicle
    }

    pub fn vehicle_index(&self) -> &VehicleParamIndex {
        &self.vehicle_index
    }

    pub fn hp(&self) -> &HardpointName {
        &self.hp
    }

    /// The mount's own `armor` map, checked first for each plate.
    pub fn mount_armor(&self) -> Option<&ArmorMap> {
        self.mount_armor.as_deref()
    }

    /// The vehicle's hull `armor` map, the fallback for plates the mount map
    /// does not list.
    pub fn hull_armor(&self) -> Option<&ArmorMap> {
        self.hull_armor.as_deref()
    }
}

/// The mounts of an upgrade's default components, then of its alternative
/// components, in a fixed order.
fn upgrade_mounts(config: &HullUpgradeConfig) -> impl Iterator<Item = &MountPoint> {
    let defaults = ComponentType::ALL.iter().flat_map(|&ct| config.mounts(ct).into_iter().flatten());
    let alternatives = ComponentType::ALL
        .iter()
        .flat_map(|&ct| config.alternatives(ct))
        .filter_map(|name| config.alternative_mounts.get(name))
        .flat_map(|mounts| mounts.mounts());

    defaults.chain(alternatives)
}
