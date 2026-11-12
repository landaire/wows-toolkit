//! Armor Viewer logic shared by the desktop front ends: the wgpu viewport
//! engine and the scene logic it draws. None of it depends on a UI toolkit.

// `rust_i18n`'s locale is process-wide, so the host's `set_locale` also
// reaches this crate's strings.
rust_i18n::i18n!("i18n_no_compiled_locales", fallback = "en", backend = wt_translations::TranslationsBackend::load());

pub mod camera_rings;
pub mod catalog;
pub mod filter_fields;
pub mod hits;
pub mod model_refs;
pub mod model_tree;
pub mod realtime;
pub mod splash;
pub mod viewport;
