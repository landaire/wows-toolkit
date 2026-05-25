use std::time::Duration;

use wowsunpack::Rc;
use wowsunpack::game_params::types::Param;
use wowsunpack::game_types::Consumable;
use wowsunpack::recognized::Recognized;

/// Total available charges for a consumable slot.
///
/// `AbilityCategory::num_consumables` uses `-1` to mean "unlimited" (e.g. base
/// Damage Control). [`from_game_params`] converts at the boundary so the
/// sentinel never leaks past this type.
///
/// [`from_game_params`]: ChargeCount::from_game_params
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ChargeCount {
    Unlimited,
    Finite(u32),
}

impl ChargeCount {
    pub fn from_game_params(num_consumables: isize) -> Self {
        if num_consumables < 0 {
            ChargeCount::Unlimited
        } else {
            ChargeCount::Finite(num_consumables as u32)
        }
    }

    pub fn saturating_sub(self, used: u32) -> Self {
        match self {
            ChargeCount::Unlimited => ChargeCount::Unlimited,
            ChargeCount::Finite(n) => ChargeCount::Finite(n.saturating_sub(used)),
        }
    }

    pub fn saturating_add(self, extra: u32) -> Self {
        match self {
            ChargeCount::Unlimited => ChargeCount::Unlimited,
            ChargeCount::Finite(n) => ChargeCount::Finite(n.saturating_add(extra)),
        }
    }

    pub fn is_unlimited(self) -> bool {
        matches!(self, ChargeCount::Unlimited)
    }

    pub fn finite(self) -> Option<u32> {
        match self {
            ChargeCount::Finite(n) => Some(n),
            ChargeCount::Unlimited => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConsumableSlot {
    pub slot_index: u8,
    pub ability: Rc<Param>,
    /// Variant key within the Ability (e.g. `"Default"`, `"D_Gold"`). Selects
    /// which `AbilityCategory` is in effect for this slot.
    pub variant_name: String,
    pub consumable_type: Recognized<Consumable>,
    pub base_charges: ChargeCount,
    /// Additive bonus from build modifiers (currently only `additionalConsumables`).
    pub bonus_charges: u32,
    pub total_charges: ChargeCount,
    pub work_time: Duration,
    pub reload_time: Duration,
    /// Lookup key for the consumable icon map. Equals `ability.index()`.
    pub icon_key: String,
}
