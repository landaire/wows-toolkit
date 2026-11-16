//! The `rust_i18n` backend both front ends translate through.
//!
//! The whole translation catalog: the locales bundled into the binary, plus
//! any `translations/*.toml` sitting next to the executable, which override
//! them key by key so a user can correct a string without a rebuild.
//!
//! `rust_i18n::i18n!` generates its lookup per crate, so a workspace where
//! more than one crate translates ends up invoking it more than once. The
//! catalog itself is parsed once per process regardless (see [`catalog`]) and
//! every backend handed out borrows it, so the second caller costs a pointer
//! rather than another megabyte of TOML.

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

type Catalog = HashMap<String, HashMap<String, String>>;

static CATALOG: OnceLock<Catalog> = OnceLock::new();

/// The parsed catalog, loaded on first use and shared by every backend.
fn catalog() -> &'static Catalog {
    CATALOG.get_or_init(|| {
        let mut all = Catalog::new();
        for (locale, source) in crate::embedded::EMBEDDED {
            if let Some(flat) = parse_locale(source) {
                all.insert((*locale).to_owned(), flat);
            }
        }

        // Applied second so an on-disk file wins, and per key rather than per
        // file, so an override listing one string does not blank the rest.
        let mut on_disk = HashMap::new();
        if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("translations"))) {
            load_toml_dir(&dir, &mut on_disk);
        }
        for (locale, overrides) in on_disk {
            all.entry(locale).or_default().extend(overrides);
        }

        all
    })
}

/// A handle to the shared catalog.
///
/// Cheap to create: [`load`](Self::load) parses nothing after the first call
/// in the process.
pub struct TranslationsBackend {
    translations: &'static Catalog,
}

impl TranslationsBackend {
    pub fn load() -> Self {
        Self { translations: catalog() }
    }
}

impl rust_i18n::Backend for TranslationsBackend {
    fn available_locales(&self) -> Vec<&str> {
        self.translations.keys().map(|s| s.as_str()).collect()
    }

    fn translate(&self, locale: &str, key: &str) -> Option<&str> {
        self.translations.get(locale).and_then(|m| m.get(key).map(|s| s.as_str()))
    }
}

fn load_toml_dir(dir: &Path, out: &mut Catalog) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "toml")
            && let Some(locale) = path.file_stem().and_then(|s| s.to_str())
            && let Some(flat) = load_locale_file(&path)
        {
            out.insert(locale.to_string(), flat);
        }
    }
}

/// Load and flatten a single locale TOML file. Kept as a separate non-inlined
/// function so the large `toml::Table` intermediate lives in its own stack frame
/// (reused across files) rather than accumulating in the caller's frame.
#[inline(never)]
fn load_locale_file(path: &Path) -> Option<HashMap<String, String>> {
    parse_locale(&std::fs::read_to_string(path).ok()?)
}

/// Flatten one locale's TOML source. Non-inlined for the same reason as
/// [`load_locale_file`]: the `toml::Table` intermediate is large, and inlining
/// would accumulate one per locale in the caller's frame.
#[inline(never)]
fn parse_locale(source: &str) -> Option<HashMap<String, String>> {
    let table: Box<toml::Table> = Box::new(source.parse().ok()?);
    let mut flat = HashMap::new();
    flatten_toml("", &table, &mut flat);
    Some(flat)
}

fn flatten_toml(prefix: &str, table: &toml::Table, out: &mut HashMap<String, String>) {
    for (k, v) in table {
        let key = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
        match v {
            toml::Value::String(s) => {
                out.insert(key, s.clone());
            }
            toml::Value::Table(t) => {
                flatten_toml(&key, t, out);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_i18n::Backend as _;

    /// Two callers share one parsed catalog, so a second crate translating
    /// costs a pointer rather than another copy of every locale.
    #[test]
    fn every_backend_borrows_the_same_catalog() {
        let first = TranslationsBackend::load();
        let second = TranslationsBackend::load();
        assert!(std::ptr::eq(first.translations, second.translations));
    }

    #[test]
    fn the_bundled_locales_are_available_and_translate() {
        let backend = TranslationsBackend::load();
        let locales = backend.available_locales();
        assert!(locales.contains(&"en"), "English is bundled");
        assert!(locales.len() > 1, "so are the other locales");

        // A key the search bar and the stats table both use.
        assert_eq!(backend.translate("en", "stat.damage"), Some("Damage"));
        assert_eq!(backend.translate("en", "no.such.key"), None);
    }

    /// Nested TOML tables flatten to the dotted keys `t!` looks up.
    #[test]
    fn nested_tables_flatten_to_dotted_keys() {
        let flat = parse_locale("[stat]\ndamage = \"Damage\"\n\n[ui.stats]\ntitle = \"Stats\"\n").expect("it parses");
        assert_eq!(flat.get("stat.damage").map(String::as_str), Some("Damage"));
        assert_eq!(flat.get("ui.stats.title").map(String::as_str), Some("Stats"));
    }
}
