#![warn(clippy::all, rust_2018_idioms)]
#![allow(clippy::blocks_in_conditions)]
mod app;
mod build_tracker;
mod consts;
mod error;
mod game_params;
#[cfg(feature = "mod_manager")]
mod mod_manager;
mod personal_rating;
mod plaintext_viewer;
mod replay_export;
pub mod replay_renderer;
mod session_stats;
mod settings;
mod tab_state;
mod task;
mod twitch;
mod ui;
mod util;
mod wows_data;
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
