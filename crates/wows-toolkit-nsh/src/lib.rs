pub mod frames;
mod native;
mod renderer;
mod replay;

rust_i18n::i18n!("i18n_no_compiled_locales", fallback = "en", backend = wt_translations::TranslationsBackend::load());

pub use native::{AppTab, ShellText, StartupState, Toolkit};
pub use renderer::ReplayRenderer;
pub use replay::{ReplayInspector, ReplayInspectorProps};

use nsh_core::prelude::{Registry, native_module};

/// The native services imported by the TSX application.
#[native_module("wows-toolkit/native")]
fn native_module(registry: &mut Registry) {
    registry.module::<Toolkit>();
    registry.module::<ReplayRenderer>();
    registry.register::<ReplayInspector>();
}
