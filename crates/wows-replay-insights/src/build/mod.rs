//! Resolve a player's loadout into a single [`ResolvedBuild`].

mod consumables;
mod modifiers;
mod resolver;

#[cfg(feature = "wowssb")]
pub mod wowssb;

pub use consumables::ChargeCount;
pub use consumables::ConsumableSlot;
pub use modifiers::ModifierSet;
pub use resolver::ResolvedBuild;
