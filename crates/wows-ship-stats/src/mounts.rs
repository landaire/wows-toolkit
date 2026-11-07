//! The weapon mounts a ship carries with a module selection.

use serde::Serialize;
use wowsunpack::game_params::keys::ComponentType;
use wowsunpack::game_params::ttx::ShipUpgradeSelection;
use wowsunpack::game_params::types::MountPoint;
use wowsunpack::game_params::types::Param;

/// Number of gun mounts or torpedo launchers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct MountCount(pub u32);

/// Number of barrels or tubes, on one mount or on several.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct BarrelCount(pub u32);

/// Every mount of one component type for the selected hull and modules. Guns
/// and torpedoes are modules of their own; the hull lists the components
/// they may mount.
pub fn selected_mounts<'a>(
    ship: &'a Param,
    selection: &ShipUpgradeSelection,
    component_type: ComponentType,
) -> &'a [MountPoint] {
    let Some(vehicle) = ship.vehicle() else {
        return &[];
    };
    let Some(hull) = selection.hull.as_deref().and_then(|name| vehicle.hull_upgrade(name)) else {
        return &[];
    };
    let Some(component) = selected_component(ship, selection, component_type) else {
        return &[];
    };

    if hull.component_name(component_type) == Some(component) {
        hull.mounts(component_type).unwrap_or_default()
    } else {
        hull.alternative_mounts.get(component).map(|mounts| mounts.mounts()).unwrap_or_default()
    }
}

/// Name of the component of one type the selected hull and modules mount,
/// e.g. `B_Torpedoes`.
pub fn selected_component<'a>(
    ship: &'a Param,
    selection: &ShipUpgradeSelection,
    component_type: ComponentType,
) -> Option<&'a str> {
    let vehicle = ship.vehicle()?;
    let hull = vehicle.hull_upgrade(selection.hull.as_deref()?)?;
    let upgrade = match component_type {
        ComponentType::Artillery => selection.artillery.as_deref(),
        ComponentType::Torpedoes => selection.torpedoes.as_deref(),
        _ => None,
    };

    upgrade
        .and_then(|name| vehicle.ttx_components()?.component_on_hull(name, hull, component_type))
        .or_else(|| hull.component_name(component_type))
}

/// The aiming mounts of one component type for the selected hull and
/// modules, with their barrel counts, by hardpoint name.
pub fn aiming_mounts<'a>(
    ship: &'a Param,
    selection: &ShipUpgradeSelection,
    component_type: ComponentType,
) -> Vec<(&'a MountPoint, u32)> {
    let mut mounts: Vec<(&MountPoint, u32)> = selected_mounts(ship, selection, component_type)
        .iter()
        .filter(|mount| mount.traverse().is_some())
        .filter_map(|mount| Some((mount, mount.num_barrels()?)))
        .collect();
    mounts.sort_by(|(left, _), (right, _)| left.hp_name().cmp(right.hp_name()));

    mounts
}
