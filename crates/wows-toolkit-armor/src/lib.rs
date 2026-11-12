//! Armor Viewer logic shared by the desktop front ends: the wgpu viewport
//! engine and the scene logic it draws. None of it depends on a UI toolkit.

// `rust_i18n`'s locale is process-wide, so the host's `set_locale` also
// reaches this crate's strings.
rust_i18n::i18n!("i18n_no_compiled_locales", fallback = "en", backend = wt_translations::TranslationsBackend::load());

pub mod assets;
pub mod camera_rings;
pub mod camo;
pub mod catalog;
pub mod filter_fields;
pub mod gaps;
pub mod hardpoint_overlay;
pub mod hits;
pub mod hull_picking;
pub mod length_marks;
pub mod load_model;
pub mod load_ship;
pub mod model_refs;
pub mod model_tree;
pub mod plate_picking;
pub mod scene_load;
pub mod sidebar_tree;
pub mod splash;
pub mod surface_maps;
pub mod trajectory;
pub mod tree_filter;
pub mod upload;
pub mod upload_hull;
pub mod viewport;
pub mod visibility;
pub mod waterline_grid;
