#![warn(clippy::all, rust_2018_idioms)]
#![allow(clippy::blocks_in_conditions)]
mod app;
mod armor_viewer;
pub mod collab;
pub(crate) mod data;
#[cfg(feature = "mod_manager")]
mod mod_manager;
pub(crate) mod replay;
mod tab_state;
mod task;
mod twitch;
mod ui;
pub(crate) mod util;
pub mod viewport_3d;
pub use app::WowsToolkitApp;
pub const APP_NAME: &str = "WoWs Toolkit";
pub(crate) use egui_phosphor::regular as icons;

/// Concatenate an icon const with a string literal at compile time (zero allocation).
/// Usage: `icon_str!(icons::GEAR_FINE, "Settings")` => `&'static str`
macro_rules! icon_str {
    ($icon:expr, $text:expr) => {
        const_format::concatcp!($icon, " ", $text)
    };
}
pub(crate) use icon_str;
