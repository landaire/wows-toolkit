//! Armor Viewer logic shared by the desktop front ends: the wgpu viewport
//! engine and the scene logic it draws. None of it depends on a UI toolkit.

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
pub mod textures;
pub mod trajectory;
pub mod upload;
pub mod upload_hull;
pub mod viewport;
pub mod visibility;
pub mod water_plane;
pub mod waterline_grid;
