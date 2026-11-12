//! Names for what the Model Viewer sidebar can open or toggle, shared by the
//! sidebar and the pane that acts on its events.

use std::fmt;

use wowsunpack::export::hardpoint::HardpointName;
use wowsunpack::export::model_catalog::ModelPath;
use wowsunpack::export::ship_parts::MiscParent;

pub use wowsunpack::export::ship_parts::ShipPartKey;

/// A ship's GameParams index, e.g. `PASB018`.
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

/// The ship and part a part model was opened from.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ShipPartRef {
    pub vehicle_param_index: VehicleParamIndex,
    pub part: ShipPartKey,
}

impl ShipPartRef {
    /// The hull hardpoint the part sits at: the mount's own for a mount, the
    /// carrying mount's for a misc part on a mount, `None` on the hull.
    pub fn hp(&self) -> Option<&HardpointName> {
        match &self.part {
            ShipPartKey::HullSection(_) => None,
            ShipPartKey::Mount(hp) => Some(hp),
            ShipPartKey::Misc { parent: MiscParent::Mount(hp), .. } => Some(hp),
            ShipPartKey::Misc { parent: MiscParent::HullSection(_), .. } => None,
        }
    }
}

/// What a hardpoint checkbox controls.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum HardpointTarget {
    /// A catalogue model opened on its own.
    Model(ModelPath),
    /// One part of a ship.
    ShipPart { vehicle_param_index: VehicleParamIndex, part: ShipPartKey },
}

/// What the active pane shows, for the sidebar to highlight.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum OpenTarget {
    Ship(VehicleParamIndex),
    Model { model: ModelPath, from_ship: Option<ShipPartRef> },
}
