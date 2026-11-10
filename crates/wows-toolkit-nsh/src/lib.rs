pub mod frames;
pub mod game_data;
mod native;
mod replay;

pub use native::{AppTab, ShellText, StartupState, Toolkit};
pub use replay::{ReplayInspector, ReplayInspectorProps};

use nsh_core::prelude::{Registry, native_module};

/// The native services imported by the TSX application.
#[native_module("wows-toolkit/native")]
fn native_module(registry: &mut Registry) {
    registry.module::<Toolkit>();
    registry.module::<game_data::GameData>();
    registry.register::<ReplayInspector>();
}
