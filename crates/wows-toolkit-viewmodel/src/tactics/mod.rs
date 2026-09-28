//! A tactics board: what is on it, what it is saved as, and what it calls the
//! maps and modes it offers.

pub mod naming;
pub mod preset;

pub use preset::PresetAnnotation;
pub use preset::PresetCapPoint;
pub use preset::PresetError;
pub use preset::PresetRangeFilter;
pub use preset::PresetShipConfig;
pub use preset::TacticsPreset;
pub use preset::delete_preset;
pub use preset::list_preset_names;
pub use preset::load_preset;
pub use preset::presets_dir;
pub use preset::save_preset;
