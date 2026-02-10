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
