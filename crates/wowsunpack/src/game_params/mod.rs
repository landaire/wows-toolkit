#[cfg(all(feature = "parsing", feature = "rkyv"))]
pub mod cache;
pub mod convert;
pub mod describe;
pub mod keys;
/// Generated modifier value-formatting table (MODIFIER_SETTINGS). See
/// scripts/gen_modifier_settings.py.
pub mod modifier_settings_data;
pub mod provider;
/// Generated modern (>=0.10) captain skill grid layout. See
/// scripts/gen_skill_grid_rs.py.
pub mod skill_grid_data;
pub mod translations;
pub mod types;
