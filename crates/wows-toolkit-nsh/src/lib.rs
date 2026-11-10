// The path holds no locale files: `TranslationsBackend` supplies the catalogs,
// parsed at startup, as in the other front ends.
rust_i18n::i18n!("i18n_no_compiled_locales", fallback = "en", backend = wt_translations::TranslationsBackend::load());

pub mod frames;
pub mod game_data;
mod native;
mod replay;
pub mod shell;

pub use native::{AppTab, ShellText, StartupState, Toolkit};
pub use replay::{ReplayInspector, ReplayInspectorProps};

use nsh_core::prelude::{Registry, native_module};

/// The native services imported by the TSX application.
#[native_module("wows-toolkit/native")]
fn native_module(registry: &mut Registry) {
    registry.module::<Toolkit>();
    registry.module::<shell::Shell>();
    registry.module::<shell::Settings>();
    registry.module::<shell::Twitch>();
    registry.module::<shell::upload::Uploads>();
    registry.module::<game_data::GameData>();
    registry.register::<ReplayInspector>();
}
