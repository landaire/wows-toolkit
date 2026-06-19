//! TTX (tactical-technical characteristics) ship-stats engine: computes a ship's
//! as-shown-in-port module characteristics card from GameParams.

pub mod armor_materials;
pub mod components;
pub mod constants;
pub mod factories;
pub mod model;
pub mod modifiers;
pub mod orchestration;
pub mod selection;
pub mod weapon_tables;

pub use orchestration::ship_stats;
pub use orchestration::ship_stats_stock;
pub use selection::ShipUpgradeSelection;
