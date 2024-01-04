#![warn(clippy::all, rust_2018_idioms)]
#![feature(iter_intersperse)]
mod app;
mod error;
mod file_unpacker;
mod game_params;
mod plaintext_viewer;
mod replay_parser;
mod util;
pub use app::WowsToolkitApp;
