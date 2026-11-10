//! Text in the reader's language, from the catalog the other front ends
//! translate with.

use std::sync::LazyLock;

use rust_i18n::Backend as _;

static CATALOG: LazyLock<wt_translations::TranslationsBackend> = LazyLock::new(wt_translations::TranslationsBackend::load);

/// The catalog's text for `key` in the selected locale, falling back to
/// English as `rust_i18n::t!` does, and to the key itself when neither has
/// it, which is also what `t!` shows.
pub(crate) fn text(key: &str) -> String {
    let locale = wows_toolkit_viewmodel::locale();
    CATALOG.translate(&locale, key).or_else(|| CATALOG.translate("en", key)).unwrap_or(key).to_owned()
}

/// [`text`] with each `%{name}` replaced by its value.
pub(crate) fn text_with(key: &str, values: &[(&str, &str)]) -> String {
    let mut out = text(key);
    for (name, value) in values {
        out = out.replace(&format!("%{{{name}}}"), value);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_are_filled_in() {
        wows_toolkit_viewmodel::set_locale("en");

        assert_eq!(text_with("ui.stats.best_frags", &[("ship", "Yamato"), ("count", "4")]), "Best: Yamato (4 kills)");
    }

    #[test]
    fn an_unknown_key_reads_as_itself() {
        assert_eq!(text("ui.stats.no_such_key"), "ui.stats.no_such_key");
    }
}
