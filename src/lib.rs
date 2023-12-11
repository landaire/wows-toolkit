#![warn(clippy::all, rust_2018_idioms)]

mod app;
mod error;
mod file_unpacker;
mod game_params;
mod plaintext_viewer;
mod replay_parser;
pub use app::WowsToolkitApp;
