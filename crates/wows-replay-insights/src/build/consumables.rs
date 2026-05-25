use std::time::Duration;

use wowsunpack::Rc;
use wowsunpack::game_params::types::Param;
use wowsunpack::game_types::Consumable;
use wowsunpack::recognized::Recognized;

pub use wowsunpack::game_types::ChargeCount;

#[derive(Debug, Clone)]
pub struct ConsumableSlot {
    pub slot_index: u8,
    pub ability: Rc<Param>,
    /// Variant key within the Ability (e.g. `"Default"`, `"D_Gold"`). Selects
    /// which `AbilityCategory` is in effect for this slot.
    pub variant_name: String,
    pub consumable_type: Recognized<Consumable>,
    /// Raw GameParams `consumableType` string (e.g. `"crashCrew"`). Used as a
    /// stable key for matching activation events to slots.
    pub consumable_type_raw: String,
    pub base_charges: ChargeCount,
    /// Additive bonus from build modifiers (currently only `additionalConsumables`).
    pub bonus_charges: u32,
    pub total_charges: ChargeCount,
    pub work_time: Duration,
    pub reload_time: Duration,
    /// Lookup key for the consumable icon map. Equals `ability.index()`.
    pub icon_key: String,
}
