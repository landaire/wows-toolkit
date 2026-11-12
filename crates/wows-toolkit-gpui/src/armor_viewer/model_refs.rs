//! Names for what the Model Viewer sidebar can open or toggle, shared by the
//! sidebar and the pane that acts on its events.

use wowsunpack::export::hardpoint::HardpointName;
use wowsunpack::export::model_catalog::ModelPath;
use wowsunpack::export::ship_parts::MiscParent;
use wowsunpack::game_types::GameParamId;

pub use wowsunpack::export::mount_armor::VehicleParamIndex;
pub use wowsunpack::export::ship_parts::ShipPartKey;

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
    Model {
        model: ModelPath,
        from_ship: Option<ShipPartRef>,
    },
    /// A building opened with its gun mounts, by its GameParams id.
    Building(GameParamId),
}
