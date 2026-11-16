//! Display grouping for the parts that make up an exported or rendered ship.

use crate::game_params::types::MountSpecies;

/// The category a ship part is listed under, shared by the glTF scene hierarchy
/// and the in-app part-visibility controls so both name and order parts alike.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PartGroup {
    #[default]
    Hull,
    MainBattery,
    SecondaryBattery,
    AaGuns,
    Torpedoes,
    DepthCharges,
    FireControl,
    Radar,
    Missiles,
    Decorations,
    /// Propellers, boats, deck fittings: the `MP_` skeleton nodes.
    Misc,
    /// Armor plating geometry, which exists only on the export path.
    Armor,
    /// A mount whose GameParams species is missing or unrecognized.
    Other,
}

impl PartGroup {
    /// Every group, in display order.
    pub const ALL: [PartGroup; 13] = [
        Self::Hull,
        Self::MainBattery,
        Self::SecondaryBattery,
        Self::AaGuns,
        Self::Torpedoes,
        Self::DepthCharges,
        Self::FireControl,
        Self::Radar,
        Self::Missiles,
        Self::Decorations,
        Self::Misc,
        Self::Armor,
        Self::Other,
    ];

    /// The group a mount belongs to, from its GameParams `typeinfo.species`.
    pub fn from_mount_species(species: Option<MountSpecies>) -> Self {
        match species {
            Some(MountSpecies::Main) => Self::MainBattery,
            Some(MountSpecies::Secondary) => Self::SecondaryBattery,
            Some(MountSpecies::AAircraft) => Self::AaGuns,
            Some(MountSpecies::Torpedo) => Self::Torpedoes,
            Some(MountSpecies::DCharge) => Self::DepthCharges,
            Some(MountSpecies::FireControl) => Self::FireControl,
            Some(MountSpecies::Search) => Self::Radar,
            Some(MountSpecies::MissileGun) => Self::Missiles,
            Some(MountSpecies::Decoration) => Self::Decorations,
            None => Self::Other,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Hull => "Hull",
            Self::MainBattery => "Main Battery",
            Self::SecondaryBattery => "Secondary Battery",
            Self::AaGuns => "AA Guns",
            Self::Torpedoes => "Torpedoes",
            Self::DepthCharges => "Depth Charges",
            Self::FireControl => "Fire Control",
            Self::Radar => "Radar",
            Self::Missiles => "Missiles",
            Self::Decorations => "Decorations",
            Self::Misc => "Misc",
            Self::Armor => "Armor",
            Self::Other => "Other",
        }
    }

    /// Rank within [`ALL`](Self::ALL), for sorting collected groups.
    pub fn order(self) -> usize {
        Self::ALL.iter().position(|g| *g == self).unwrap_or(Self::ALL.len())
    }
}
