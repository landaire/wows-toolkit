//! Target bearings, and which gun mounts and torpedo launchers bear on one
//! distant target at each.
//!
//! The server gives every secondary mount a target of its own
//! (`state.atba.atbaTargets`, one entity per gun), and a mount fires only
//! while it is aimed and outside its dead zones (`ATBAGun.canShootRightNow`).
//! Against one target only the mounts whose yaw limits cover its bearing
//! fire, so a broadside target draws one side's mounts and the centerline
//! mounts that turn to it.

use std::collections::HashMap;
use std::ops::Add;

use serde::Serialize;
use serde::Serializer;
use wows_core::units::Degrees;
use wowsunpack::game_params::keys::ComponentType;
use wowsunpack::game_params::ttx::ShipUpgradeSelection;
use wowsunpack::game_params::types::MountPoint;
use wowsunpack::game_params::types::Param;
use wowsunpack::game_params::types::Species;

use crate::mounts::aiming_mounts;

/// A target bearing off the bow in whole degrees, clockwise, 0 to 359.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Bearing(u16);

impl Bearing {
    pub const fn new(degrees: u16) -> Self {
        Self(degrees % 360)
    }

    pub fn degrees(self) -> u16 {
        self.0
    }

    pub fn all() -> impl Iterator<Item = Bearing> {
        (0..360).map(Bearing)
    }

    /// The same bearing on the other side, mirrored across the keel.
    pub fn mirrored(self) -> Self {
        Self::new(360 - self.0)
    }

    /// Degrees to the nearer beam, 90 or 270.
    fn off_beam(self) -> u16 {
        let starboard = self.0.abs_diff(90);
        let port = self.0.abs_diff(270);

        starboard.min(port)
    }

    fn index(self) -> usize {
        usize::from(self.0)
    }
}

impl Serialize for Bearing {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u16(self.0)
    }
}

/// Bearings a per-bearing table lists, on the starboard side: bow-on, the
/// bow quarter, broadside, the stern quarter and stern-on.
pub const TABLE_BEARINGS: [Bearing; 9] = [
    Bearing::new(0),
    Bearing::new(30),
    Bearing::new(45),
    Bearing::new(60),
    Bearing::new(90),
    Bearing::new(120),
    Bearing::new(135),
    Bearing::new(150),
    Bearing::new(180),
];

/// Bearings at which each aiming main battery and secondary mount and each
/// torpedo launcher bears, by hardpoint, for one hull and module selection.
#[derive(Debug, Clone, Default)]
pub struct MountCoverage {
    by_hardpoint: HashMap<String, [bool; 360]>,

    /// Whether the torpedo launchers are covered. Submarine tubes aim by
    /// firing sectors this model leaves out.
    launchers: bool,
}

impl MountCoverage {
    /// `None` when the hull model gives no facing for some aiming mount or
    /// launcher.
    pub fn new(ship: &Param, selection: &ShipUpgradeSelection, facings: &HashMap<String, Degrees>) -> Option<Self> {
        let submarine = ship.species().and_then(|species| species.known()) == Some(&Species::Submarine);
        let component_types: &[ComponentType] = if submarine {
            &[ComponentType::Artillery, ComponentType::Atba]
        } else {
            &[ComponentType::Artillery, ComponentType::Atba, ComponentType::Torpedoes]
        };

        let mut by_hardpoint = HashMap::new();
        for component_type in component_types {
            for (mount, _) in aiming_mounts(ship, selection, *component_type) {
                let facing = *facings.get(mount.hp_name())?;
                let mut bears = [false; 360];
                for bearing in Bearing::all() {
                    bears[bearing.index()] = mount_bears(mount, facing, bearing);
                }
                by_hardpoint.insert(mount.hp_name().to_owned(), bears);
            }
        }

        Some(Self { by_hardpoint, launchers: !submarine })
    }

    /// The bearings at which the mount on `hardpoint` bears, by bearing
    /// index; `None` for a hardpoint with no aiming mount.
    fn row(&self, hardpoint: &str) -> Option<&[bool; 360]> {
        self.by_hardpoint.get(hardpoint)
    }

    /// Whether the mount on `hardpoint` bears at `bearing`; false for a
    /// hardpoint with no aiming mount.
    pub fn bears(&self, hardpoint: &str, bearing: Bearing) -> bool {
        self.row(hardpoint).is_some_and(|bears| bears[bearing.index()])
    }

    /// Whether it covers the torpedo launchers.
    pub fn covers_launchers(&self) -> bool {
        self.launchers
    }
}

/// One value per bearing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PerBearing<V> {
    values: [V; 360],
}

impl<V> PerBearing<V>
where
    V: Copy + Default + PartialOrd + Add<Output = V>,
{
    /// `value` at every bearing.
    pub fn uniform(value: V) -> Self {
        Self { values: [value; 360] }
    }

    /// `value` at the bearings where the mount on `hardpoint` bears.
    pub fn mount(coverage: &MountCoverage, hardpoint: &str, value: V) -> Self {
        let mut values = [V::default(); 360];
        if let Some(bears) = coverage.row(hardpoint) {
            for (slot, bears) in values.iter_mut().zip(bears) {
                if *bears {
                    *slot = value;
                }
            }
        }

        Self { values }
    }

    pub fn at(&self, bearing: Bearing) -> V {
        self.values[bearing.index()]
    }

    /// The bearing with the highest value, the one nearest a beam on a tie,
    /// then the lowest.
    pub fn best(&self) -> Bearing {
        Bearing::all().fold(Bearing::new(0), |best, bearing| {
            let (value, current) = (self.at(bearing), self.at(best));
            let better = value > current || (value == current && bearing.off_beam() < best.off_beam());
            if better { bearing } else { best }
        })
    }

    /// The values at [`TABLE_BEARINGS`], each with its port mirror.
    pub fn table(&self) -> [BearingRow<V>; 9] {
        TABLE_BEARINGS.map(|bearing| BearingRow {
            bearing,
            starboard: self.at(bearing),
            port: self.at(bearing.mirrored()),
        })
    }

    /// The higher of the two values at every bearing.
    pub fn max(self, other: Self) -> Self {
        let mut values = self.values;
        for (value, other) in values.iter_mut().zip(other.values) {
            if other > *value {
                *value = other;
            }
        }

        Self { values }
    }
}

impl<V> PerBearing<V>
where
    V: Copy,
{
    /// `value(bearing)` at every bearing.
    pub fn from_fn(value: impl Fn(Bearing) -> V) -> Self {
        Self { values: std::array::from_fn(|idx| value(Bearing(idx as u16))) }
    }

    /// `convert` applied to the value at every bearing.
    pub fn map<U>(&self, convert: impl Fn(V) -> U) -> PerBearing<U> {
        PerBearing { values: self.values.map(convert) }
    }
}

impl<V> Add for PerBearing<V>
where
    V: Copy + Add<Output = V>,
{
    type Output = Self;

    fn add(mut self, rhs: Self) -> Self {
        for (value, other) in self.values.iter_mut().zip(rhs.values) {
            *value = *value + other;
        }

        self
    }
}

/// One row of a per-bearing table: the value at `bearing` off the starboard
/// bow and at its mirror off the port bow (the same at 0 and 180).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct BearingRow<V> {
    pub bearing: Bearing,
    pub starboard: V,
    pub port: V,
}

/// Whether a mount whose hull hardpoint faces `facing` fires at a distant
/// target on `bearing`. Mount yaw limits are measured in the hardpoint frame,
/// clockwise from above; a dead zone stops fire inside the limits.
pub fn mount_bears(mount: &MountPoint, facing: Degrees, bearing: Bearing) -> bool {
    let local = Degrees::new(f32::from(bearing.0) - facing.value());

    mount.can_fire_at(local).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use wows_core::units::FiresPerMinute;

    use super::*;

    #[test]
    fn ties_go_to_the_bearing_nearest_a_beam() {
        let flat = PerBearing::uniform(FiresPerMinute::new(1.0));

        assert_eq!(flat.best(), Bearing::new(90));
        assert_eq!(Bearing::new(0).mirrored(), Bearing::new(0));
        assert_eq!(Bearing::new(30).mirrored(), Bearing::new(330));
    }
}
