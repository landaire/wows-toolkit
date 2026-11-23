//! The Fluent bundles every crate translates through.
//!
//! Each bundle holds the locale bundled into the binary plus any
//! `translations/<code>.ftl` sitting next to the executable, whose messages
//! override the bundled ones by id so a user can correct a string without a
//! rebuild.

use std::borrow::Cow;
use std::path::Path;
use std::sync::LazyLock;

use fluent_bundle::FluentArgs;
use fluent_bundle::FluentResource;
use fluent_bundle::concurrent::FluentBundle;
use unic_langid::LanguageIdentifier;

use crate::Language;
use crate::SUPPORTED_LANGUAGES;

type Bundle = FluentBundle<FluentResource>;

/// One bundle per entry of [`SUPPORTED_LANGUAGES`], in the same order.
struct Catalog {
    bundles: Vec<Bundle>,
}

impl Catalog {
    fn load(override_dir: Option<&Path>) -> Self {
        let bundles = SUPPORTED_LANGUAGES
            .iter()
            .map(|info| {
                let embedded = crate::embedded::EMBEDDED.iter().find(|(code, _)| *code == info.code).map(|(_, s)| *s);
                let overriding = override_dir.and_then(|dir| read_override(&dir.join(format!("{}.ftl", info.code))));
                build_bundle(info.code, info.langid, embedded, overriding)
            })
            .collect();
        Self { bundles }
    }

    fn bundle(&self, language: Language) -> &Bundle {
        &self.bundles[language.index()]
    }

    fn translate(&self, language: Language, id: &str, args: Option<&FluentArgs<'_>>) -> Cow<'static, str> {
        for language in [language, Language::ENGLISH] {
            if let Some(text) = format(self.bundle(language), language, id, args) {
                return Cow::Owned(text);
            }
        }

        report_missing(id);
        Cow::Owned(id.to_owned())
    }
}

static CATALOG: LazyLock<Catalog> = LazyLock::new(|| {
    let override_dir = std::env::current_exe().ok().and_then(|exe| exe.parent().map(|dir| dir.join("translations")));
    Catalog::load(override_dir.as_deref())
});

fn read_override(path: &Path) -> Option<String> {
    match std::fs::read_to_string(path) {
        Ok(source) => Some(source),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "could not read translation override");
            None
        }
    }
}

fn build_bundle(code: &str, langid: &str, embedded: Option<&str>, overriding: Option<String>) -> Bundle {
    let langid: LanguageIdentifier = langid.parse().expect("SUPPORTED_LANGUAGES holds valid BCP 47 tags");
    let mut bundle = Bundle::new_concurrent(vec![langid]);
    // The text renderers draw U+2068/U+2069 as boxes, and no catalog mixes
    // text direction.
    bundle.set_use_isolating(false);

    if let Some(source) = embedded {
        let resource = parse(code, "embedded", source.to_owned());
        if let Err(errors) = bundle.add_resource(resource) {
            tracing::warn!(locale = code, file = "embedded", ?errors, "duplicate translation messages");
        }
    }
    if let Some(source) = overriding {
        bundle.add_resource_overriding(parse(code, "override", source));
    }

    bundle
}

/// Parses `source`, keeping every message that parsed when some did not.
fn parse(code: &str, file: &str, source: String) -> FluentResource {
    FluentResource::try_new(source).unwrap_or_else(|(resource, errors)| {
        tracing::warn!(locale = code, file, ?errors, "translation file has syntax errors");
        resource
    })
}

/// Formats message `id` in `language`, then in English, then returns `id`.
pub fn translate_in(language: Language, id: &str, args: Option<&FluentArgs<'_>>) -> Cow<'static, str> {
    CATALOG.translate(language, id, args)
}

/// Whether `language`'s own catalog has message `id`, without the English fallback.
pub fn has_message(language: Language, id: &str) -> bool {
    CATALOG.bundle(language).has_message(id)
}

/// Formats message `id` in the current locale. See [`translate_in`].
pub fn translate(id: &str, args: Option<&FluentArgs<'_>>) -> Cow<'static, str> {
    translate_in(crate::language(), id, args)
}

fn format(bundle: &Bundle, language: Language, id: &str, args: Option<&FluentArgs<'_>>) -> Option<String> {
    let pattern = bundle.get_message(id)?.value()?;

    let mut errors = Vec::new();
    let text = bundle.format_pattern(pattern, args, &mut errors).into_owned();
    if !errors.is_empty() {
        tracing::warn!(locale = language.code(), id, ?errors, "translation formatted with errors");
    }

    Some(text)
}

#[cfg(debug_assertions)]
fn report_missing(id: &str) {
    use std::collections::HashSet;
    use std::sync::Mutex;

    static REPORTED: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Mutex::default);

    let mut reported = REPORTED.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if reported.insert(id.to_owned()) {
        tracing::warn!(id, "no translation message with this id");
    }
}

#[cfg(not(debug_assertions))]
fn report_missing(_id: &str) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundle(source: &str) -> Bundle {
        build_bundle("cs", "cs", Some(source), None)
    }

    fn render(bundle: &Bundle, id: &str, args: Option<&FluentArgs<'_>>) -> Option<String> {
        format(bundle, Language::ENGLISH, id, args)
    }

    #[test]
    fn an_override_replaces_only_the_messages_it_names() {
        let bundle = build_bundle(
            "en",
            "en",
            Some("greeting = Hello\nfarewell = Goodbye\n"),
            Some("greeting = Howdy\n".to_owned()),
        );
        assert_eq!(render(&bundle, "greeting", None).as_deref(), Some("Howdy"));
        assert_eq!(render(&bundle, "farewell", None).as_deref(), Some("Goodbye"));
    }

    #[test]
    fn an_override_with_a_syntax_error_keeps_its_valid_messages() {
        let bundle = build_bundle("en", "en", Some("a = A\nb = B\n"), Some("a = Override\nb = { $\n".to_owned()));
        assert_eq!(render(&bundle, "a", None).as_deref(), Some("Override"));
        assert_eq!(render(&bundle, "b", None).as_deref(), Some("B"));
    }

    #[test]
    fn czech_plural_categories_follow_cldr() {
        let bundle = bundle(
            "files = { $count ->\n    [one] { $count } soubor\n    [few] { $count } soubory\n    [many] { $count } souboru\n   *[other] { $count } souborů\n}\n",
        );
        let cases: [(&str, FluentArgs<'_>); 5] = [
            ("1 soubor", FluentArgs::from_iter([("count", 1)])),
            ("3 soubory", FluentArgs::from_iter([("count", 3)])),
            ("5 souborů", FluentArgs::from_iter([("count", 5)])),
            ("1.5 souboru", FluentArgs::from_iter([("count", 1.5)])),
            ("22 souborů", FluentArgs::from_iter([("count", 22)])),
        ];
        for (expected, args) in cases {
            assert_eq!(render(&bundle, "files", Some(&args)).as_deref(), Some(expected));
        }
    }

    #[test]
    fn placeables_are_not_wrapped_in_isolation_marks() {
        let bundle = bundle("hi = Hi { $name }!\n");
        let args = FluentArgs::from_iter([("name", "Ann")]);
        assert_eq!(render(&bundle, "hi", Some(&args)).as_deref(), Some("Hi Ann!"));
    }

    fn catalog(english: &str, czech: &str) -> Catalog {
        let bundles = SUPPORTED_LANGUAGES
            .iter()
            .map(|info| {
                let source = match info.code {
                    "en" => english,
                    "cs" => czech,
                    _ => "",
                };
                build_bundle(info.code, info.langid, Some(source), None)
            })
            .collect();
        Catalog { bundles }
    }

    #[test]
    fn a_message_missing_from_a_locale_is_taken_from_english() {
        let catalog = catalog("a = A\nb = B\n", "a = Á\n");
        let czech = Language::from_code("cs").expect("cs is supported");
        assert_eq!(catalog.translate(czech, "a", None), "Á");
        assert_eq!(catalog.translate(czech, "b", None), "B");
    }

    #[test]
    fn a_message_missing_everywhere_is_its_id() {
        let catalog = catalog("a = A\n", "a = Á\n");
        let czech = Language::from_code("cs").expect("cs is supported");
        assert_eq!(catalog.translate(czech, "ui-missing", None), "ui-missing");
    }

    #[test]
    fn every_embedded_catalog_parses_cleanly() {
        for (code, source) in crate::embedded::EMBEDDED {
            if let Err((_, errors)) = FluentResource::try_new((*source).to_owned()) {
                panic!("{code}: {errors:?}");
            }
        }
    }
}
