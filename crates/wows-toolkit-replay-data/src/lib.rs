//! Game-data loading, replay parsing into the presentation model and the
//! replay index pass, shared by the desktop front ends. Blocking functions and
//! plain data only: each front end schedules them on its own executor.

rust_i18n::i18n!("i18n_no_compiled_locales", fallback = "en", backend = wt_translations::TranslationsBackend::load());

/// Sets the language every `t!` in this crate reads.
///
/// `rust_i18n` keeps its locale per crate, so a front end setting its own
/// does not reach these strings.
pub fn set_locale(code: &str) {
    rust_i18n::set_locale(code);
}

pub mod browser_tree;
pub mod columns;
pub mod damage_mix;
pub mod error;
pub mod force_reindex;
pub mod heap_profile;
pub mod install;
pub mod links;
pub mod load;
pub mod model;
pub mod paths;
pub mod replay_index;
#[cfg(any(test, feature = "test-support"))]
pub mod sample;
pub mod shareable;
pub mod sort;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
