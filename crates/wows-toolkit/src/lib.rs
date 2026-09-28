#![warn(clippy::all, rust_2018_idioms)]
#![allow(clippy::blocks_in_conditions)]

// The path deliberately holds no locale files. Pointed at the real catalogs,
// `i18n!()` generates one `HashMap::from([(k, v)])` temporary per key inside a
// single closure; across 20 locales that is ~20,000 temporaries, and an
// unoptimized build gives each its own stack slot, so the initializer needs a
// frame larger than the 2 MiB a thread gets by default. Every test that reached
// `t!()` first therefore died with a stack overflow. `TranslationsBackend`
// supplies the same catalogs, parsed at startup, with no such frame.
rust_i18n::i18n!("i18n_no_compiled_locales", fallback = "en", backend = wt_translations::TranslationsBackend::load());

mod app;
mod armor_viewer;
pub mod boot;
pub mod cli;
pub mod collab;
pub mod data;
pub(crate) mod db;
pub mod gpu;
/// Windows process mitigations, shared with the GPUI port: both front ends apply
/// the same policies before they open a window.
pub use wows_toolkit_hardening as hardening;
#[cfg(feature = "mod_manager")]
mod mod_manager;
// Also under test: the export's equivalence check needs the same headless
// replay load this module already performs.
#[cfg(any(feature = "profile-bins", test))]
pub mod profiling;

/// Accumulate the wall time of an expression against a named sub-stage, for
/// the `profile_replay` binary. Compiles to the bare expression without the
/// `profile-bins` feature.
#[cfg(feature = "profile-bins")]
macro_rules! timed_stage {
    ($name:expr, $e:expr) => {{
        let start = std::time::Instant::now();
        let value = $e;
        $crate::profiling::record($name, start.elapsed());
        value
    }};
}

#[cfg(not(feature = "profile-bins"))]
macro_rules! timed_stage {
    ($name:expr, $e:expr) => {
        $e
    };
}

pub(crate) use timed_stage;
pub(crate) mod replay;
mod tab_state;
mod task;
#[cfg(test)]
mod test_utils;
mod twitch;
mod ui;
pub(crate) mod ui_channel;
pub(crate) mod util;
pub mod viewport_3d;
pub use app::WowsToolkitApp;
pub use data::replay_index;
pub use db::load_main_window_settings;

/// The Code Integrity Guard preference to launch with, or `None` when the
/// setting could not be read.
///
/// Read before the app exists, because the policy has to be applied before any
/// window is created and cannot be changed afterwards.
///
/// Absence and failure are kept apart deliberately. A key that was never
/// written means the user has not chosen, and `Automatic` is the right answer.
/// A database that could not be opened means the choice is unknown, and the
/// unknown value may have been the one turning the policy off; defaulting there
/// would apply an irreversible policy against a preference that said otherwise.
pub fn load_code_integrity_preference() -> Option<hardening::CodeIntegrityPreference> {
    match db::load_startup_setting(CODE_INTEGRITY_SETTING) {
        Ok(stored) => Some(stored.unwrap_or_default()),
        Err(_) => None,
    }
}

/// Settings key for the Code Integrity Guard preference, shared by the startup
/// read and the save path so the two cannot drift apart.
pub const CODE_INTEGRITY_SETTING: &str = "code_integrity";
pub use tab_state::WindowSettingsEguiExt;
// `util` is crate-private, but this type appears in the public `hardening` and
// `gpu` error enums, so it has to be nameable from outside.
pub use util::win32::Win32Status;
// Narrow re-export so the gated integration test in `tests/replay_index_mapper.rs`
// can construct a `Replay` and call `replay_index::map_rows` on it without making
// the whole (egui-coupled) `ui` module public.
pub use ui::replay_parser::Replay;
pub const APP_NAME: &str = "WoWs Toolkit";
pub(crate) use egui_phosphor::regular as icons;

/// Force the `i18n!()` lazy static to initialize. The generated initializer
/// requires a large stack frame in debug builds, so call this from a thread
/// with an explicit (larger) stack size.
pub fn init_i18n() {
    // Any `t!()` call triggers the lazy init.
    let _ = rust_i18n::t!("meta.language_name");
}

/// [`TextResolver`] implementation that uses `t!()` from rust-i18n.
pub(crate) struct LocalizedTextResolver;

impl wt_translations::TextResolver for LocalizedTextResolver {
    fn resolve(&self, text: &wt_translations::TranslatableText) -> String {
        let key = text.key();
        rust_i18n::t!(key).into()
    }
}

pub use wows_toolkit_config::storage_dir;
