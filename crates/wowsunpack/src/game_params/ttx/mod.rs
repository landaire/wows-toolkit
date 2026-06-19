//! TTX ship-stats engine: computes a ship's as-shown-in-port module
//! characteristics card from GameParams.
//!
//! "TTX" is the game's own term for the port stats panel. It is the Russian
//! abbreviation TTX (tactico-technical characteristics) -- the standard
//! Russian-military term for an equipment spec sheet -- which the game (from a
//! Russian-rooted developer) uses for the in-port stats panel; the name is kept
//! here so this module mirrors that vocabulary. The engine works in two layers:
//! preprocess (resolve ship -> component -> gun -> ammo and read base field
//! values) then factory (apply the formulas, unit conversions, and the
//! equipped-modifier pipeline). Public entry: [`ship_stats`] / [`ship_stats_stock`].

pub mod armor_materials;
pub mod components;
pub mod constants;
pub mod factories;
pub mod labels;
pub mod model;
pub mod modifiers;
pub mod orchestration;
pub mod selection;
pub mod weapon_tables;

pub use orchestration::ship_stats;
pub use orchestration::ship_stats_stock;
pub use selection::ShipUpgradeSelection;
