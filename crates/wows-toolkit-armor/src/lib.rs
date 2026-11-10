//! Armor Viewer logic shared by the desktop front ends: the offscreen wgpu
//! viewport, ship armor loading, mesh upload, visibility state, and the
//! analysis overlays (splash boxes, gaps, shell trajectories, hits, camera
//! rings). None of it depends on a UI toolkit.

pub mod assets;
pub mod camera_rings;
pub mod camo;
pub mod gaps;
pub mod highlight;
pub mod hits;
pub mod load_ship;
pub mod realtime;
pub mod scene;
pub mod splash;
pub mod trajectory;
pub mod upload;
pub mod upload_hull;
pub mod viewport;
pub mod visibility;
