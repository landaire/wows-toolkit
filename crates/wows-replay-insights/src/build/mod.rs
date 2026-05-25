//! Resolve a player's loadout into a single [`ResolvedBuild`].

mod consumables;
mod modifiers;
mod resolver;
mod seed;

#[cfg(feature = "wowssb")]
pub mod wowssb;

pub use consumables::ChargeCount;
pub use consumables::ConsumableSlot;
pub use modifiers::ModifierSet;
pub use resolver::ResolvedBuild;
pub use seed::build_inventory_for_player;
pub use seed::seed_consumable_inventories;
