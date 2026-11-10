// The path holds no locale files: `TranslationsBackend` supplies the catalogs,
// parsed at startup, as in the other front ends.
rust_i18n::i18n!("i18n_no_compiled_locales", fallback = "en", backend = wt_translations::TranslationsBackend::load());

pub mod armor;
pub mod clipboard;
pub mod collab;
#[cfg(target_os = "linux")]
pub mod desktop;
pub mod frames;
pub mod game_data;
mod js_newtype;
mod launcher;
mod native;
mod player_tracker;
mod renderer;
pub mod replay_inspector;
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
pub use native::AppTab;
pub use native::ShellText;
pub use native::StartupState;
pub use native::Toolkit;
pub use player_tracker::PlayerTracker;
pub use renderer::ReplayRenderer;
pub use search::Palette;
pub use search::QueryBar;
pub use search::ReplaySearch;
pub use search::ShipSearch;
pub use stats::Stats;
pub use unpacker::Unpacker;
pub use unpacker::UnpackerProps;

/// Points this test process's settings database at a temporary directory, so
/// a unit test never reads or writes the user's own data.
#[cfg(test)]
pub(crate) fn isolate_test_storage() {
    wows_toolkit_config::storage_override::set(
        std::env::temp_dir().join(format!("wows-toolkit-nsh-unit-{}", std::process::id())),
    );
}

/// The app's `nsh.json`: its name, id and the capabilities it asks for.
pub const APP_MANIFEST: &str = include_str!("../ui/nsh.json");

use nsh_core::prelude::Registry;
use nsh_core::prelude::native_module;

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
    registry.module::<armor::Armor>();
    registry.module::<replay_inspector::service::ReplayInspectorService>();
    registry.register::<replay_inspector::table::PlayerTable>();
    registry.register::<replay_inspector::browser::ReplayBrowser>();
    registry.register::<Unpacker>();
}
