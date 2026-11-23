//! The language the interface reads in.

use wt_translations::t;
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

    /// Switches every translated string to `code`.
    pub fn choose(self, code: LanguageCode) {
        if let Err(error) = wt_translations::set_locale(code.as_str()) {
            tracing::warn!(%error, "keeping the current interface language");
            return;
        }

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
