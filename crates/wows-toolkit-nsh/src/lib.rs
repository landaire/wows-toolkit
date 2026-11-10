pub mod frames;
mod native;
mod replay;
mod search;

pub use native::{AppTab, ShellText, StartupState, Toolkit};
pub use replay::{ReplayInspector, ReplayInspectorProps};
pub use search::{Palette, QueryBar, ReplaySearch, ShipSearch};

use nsh_core::prelude::{Registry, native_module};

/// The native services imported by the TSX application.
#[native_module("wows-toolkit/native")]
fn native_module(registry: &mut Registry) {
    registry.module::<Toolkit>();
    registry.register::<ReplayInspector>();
    registry.module::<ReplaySearch>().module::<Palette>().class::<QueryBar>().class::<ShipSearch>();
}
