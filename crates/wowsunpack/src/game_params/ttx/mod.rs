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
pub mod consumables;
pub mod effects;
pub mod factories;
pub mod labels;
pub mod model;
pub mod modifiers;
pub mod module_options;
pub mod orchestration;
pub mod provenance;
pub mod render;
pub mod selection;
pub mod weapon_tables;

pub use effects::Effect;
pub use effects::EffectActivation;
pub use effects::EffectId;
pub use effects::EffectKind;
pub use effects::EffectiveModifiers;
pub use effects::Effects;
pub use effects::EffectsState;
pub use effects::HealthFraction;
pub use effects::Loadout;
pub use effects::ReloadCoeffs;
pub use module_options::ModuleOption;
pub use module_options::ModuleOptions;
pub use module_options::ModuleSlot;
pub use module_options::SlotOptions;
pub use module_options::module_options;
pub use orchestration::ship_stats;
pub use orchestration::ship_stats_stock;
pub use provenance::Contribution;
pub use provenance::InputId;
pub use provenance::Op;
pub use provenance::ShipStatsProvenance;
pub use provenance::StatAttribution;
pub use render::StatDelta;
pub use render::StatLine;
pub use render::diff_stat_rows;
pub use render::render_stat_rows;
pub use selection::ShipUpgradeSelection;
