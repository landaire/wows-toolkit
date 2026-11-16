//! The language the interface reads in.

use rust_i18n::t;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::Track;

/// A language code from `wt_translations::SUPPORTED_LANGUAGES`, such as `en` or `ja`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LanguageCode(String);

impl LanguageCode {
    pub fn new(code: impl Into<String>) -> Self {
        Self(code.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The interface language, as a signal every translated string can follow.
///
/// `rust_i18n` keeps one locale per crate, so a change is applied to this crate's strings and to
/// the shared crates' strings at once.
#[derive(Clone, Copy)]
pub struct Locale {
    /// `None` until a language is chosen, which reads as English.
    chosen: RwSignal<Option<LanguageCode>, LocalStorage>,
}

impl Locale {
    pub fn new() -> Self {
        Self { chosen: RwSignal::new_local(None) }
    }

    /// Subscribes the calling reactive scope to a change of language.
    pub fn track(self) {
        self.chosen.track();
    }

    /// Switches every catalog to `code`.
    pub fn choose(self, code: LanguageCode) {
        apply_to_catalogs(&code);
        self.chosen.set(Some(code));
    }

    /// The string under `key`, following the language. For a reactive hole or an attribute.
    pub fn text(self, key: &'static str) -> impl Fn() -> String + Copy + 'static {
        move || {
            self.track();
            t!(key).into_owned()
        }
    }
}

impl Default for Locale {
    fn default() -> Self {
        Self::new()
    }
}

/// Points this crate's catalog and the shared crates' catalogs at `code`.
pub fn apply_to_catalogs(code: &LanguageCode) {
    rust_i18n::set_locale(code.as_str());
    wows_toolkit_viewmodel::set_locale(code.as_str());
    wows_toolkit_replay_data::set_locale(code.as_str());
}
