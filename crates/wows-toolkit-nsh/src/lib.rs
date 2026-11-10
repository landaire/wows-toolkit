pub mod frames;
pub mod game_data;
mod native;
mod player_tracker;
mod replay;
mod stats;

pub use native::{AppTab, ShellText, StartupState, Toolkit};
pub use player_tracker::PlayerTracker;
pub use replay::{ReplayInspector, ReplayInspectorProps};
pub use stats::Stats;

use nsh_core::prelude::{Registry, native_module};

/// The native services imported by the TSX application.
#[native_module("wows-toolkit/native")]
fn native_module(registry: &mut Registry) {
    registry.module::<Toolkit>();
    registry.module::<game_data::GameData>();
    registry.module::<Stats>();
    registry.module::<PlayerTracker>();
    registry.register::<ReplayInspector>();
}
