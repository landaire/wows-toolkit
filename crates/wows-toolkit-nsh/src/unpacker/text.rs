//! The Unpacker's UI text, read from the shared translation catalog.

use std::rc::Rc;

use rust_i18n::Backend as _;
use wt_translations::TranslationsBackend;

/// The locale every key is written in, used when the selected locale lacks
/// a key.
const FALLBACK_LOCALE: &str = "en";

/// Looks up `ui.*` keys in one locale.
#[derive(Clone)]
pub struct Text {
    backend: Rc<TranslationsBackend>,
    locale: Rc<str>,
}

impl Text {
    pub fn new(locale: &str) -> Self {
        Self { backend: Rc::new(TranslationsBackend::load()), locale: Rc::from(locale) }
    }

    /// The text of `key`. A key missing from every locale shows as the key
    /// itself, as `rust_i18n` does, so the gap is visible rather than blank.
    pub fn get(&self, key: &str) -> String {
        self.backend
            .translate(&self.locale, key)
            .or_else(|| self.backend.translate(FALLBACK_LOCALE, key))
            .unwrap_or(key)
            .to_owned()
    }

    /// The text of `key` with each `%{name}` replaced by its value.
    pub fn with(&self, key: &str, args: &[(&str, &dyn std::fmt::Display)]) -> String {
        let mut text = self.get(key);
        for (name, value) in args {
            text = text.replace(&format!("%{{{name}}}"), &value.to_string());
        }

        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_are_filled_and_unknown_locales_fall_back_to_english() {
        let text = Text::new("xx");

        assert_eq!(text.get("ui.unpacker.packages"), "Packages");
        assert_eq!(text.with("ui.unpacker.item_count", &[("count", &3)]), "3 items");
        assert_eq!(text.get("ui.unpacker.no_such_key"), "ui.unpacker.no_such_key");
    }
}
