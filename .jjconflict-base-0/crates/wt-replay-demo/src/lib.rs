//! Replay playback demo for the WoWs Toolkit website.
//!
//! Plays a clip written by `export_demo_track` through the egui renderer the
//! desktop viewer uses, with its transport and layer switches.

// The app runs only in the browser; a host build compiles the pure playback
// logic so its tests run under `cargo test`.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

#[cfg(target_arch = "wasm32")]
mod app;
mod error;
mod page;
mod playback;
#[cfg(target_arch = "wasm32")]
mod textures;
mod tour;
#[cfg(target_arch = "wasm32")]
mod web;
