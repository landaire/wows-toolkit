// The path holds no locale files: `TranslationsBackend` supplies the catalogs,
// parsed at startup, as in the other front ends.
rust_i18n::i18n!("i18n_no_compiled_locales", fallback = "en", backend = wt_translations::TranslationsBackend::load());

pub mod collab;
pub mod frames;
pub mod game_data;
mod launcher;
mod native;
mod player_tracker;
mod renderer;
mod replay;
pub mod runtime;
mod search;
mod settings_store;
pub mod shell;
mod stats;
mod tactics;
mod text;
mod unpacker;

pub use collab::Collab;
pub use launcher::LauncherSession;
pub use native::{AppTab, ShellText, StartupState, Toolkit};
pub use player_tracker::PlayerTracker;
pub use renderer::ReplayRenderer;
pub use replay::{ReplayInspector, ReplayInspectorProps};
pub use search::{Palette, QueryBar, ReplaySearch, ShipSearch};
pub use stats::Stats;
pub use unpacker::{Unpacker, UnpackerProps};

/// Points this test process's settings database at a temporary directory, so
/// a unit test never reads or writes the user's own data.
#[cfg(test)]
pub(crate) fn isolate_test_storage() {
    wows_toolkit_config::storage_override::set(
        std::env::temp_dir().join(format!("wows-toolkit-nsh-unit-{}", std::process::id())),
    );
}

use nsh_core::prelude::{Registry, native_module};

/// The native services imported by the TSX application.
#[native_module("wows-toolkit/native")]
fn native_module(registry: &mut Registry) {
    registry.module::<Toolkit>();
    registry.module::<game_data::GameData>();
    registry.module::<shell::Shell>();
    registry.module::<shell::Settings>();
    registry.module::<shell::Twitch>();
    registry.module::<shell::upload::Uploads>();
    registry.module::<Collab>();
    registry.class::<LauncherSession>();
    registry.module::<Stats>();
    registry.module::<PlayerTracker>();
    registry.module::<ReplayRenderer>();
    registry.module::<tactics::Tactics>();
    registry.module::<ReplaySearch>().module::<Palette>().class::<QueryBar>().class::<ShipSearch>();
    registry.register::<ReplayInspector>();
    registry.register::<Unpacker>();
}
