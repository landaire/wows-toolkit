//! The translation catalogs, embedded in the binary.
//!
//! `rust_i18n::i18n!()` is deliberately *not* pointed at this directory.
//! Its generated initializer emits one `HashMap::from([(k, v)])` temporary
//! per key inside a single closure; across 20 locales that is ~20,000
//! temporaries, and an unoptimized build gives each its own stack slot,
//! producing a frame larger than the 2 MiB a thread gets by default. The
//! catalogs are carried as source text instead and parsed at startup, which
//! keeps the binary self-contained without the stack cliff.

/// Every bundled locale, as `(locale, TOML source)`.
pub static EMBEDDED: &[(&str, &str)] = &[
    ("cs", include_str!("../translations/cs.toml")),
    ("de", include_str!("../translations/de.toml")),
    ("en", include_str!("../translations/en.toml")),
    ("es", include_str!("../translations/es.toml")),
    ("es_mx", include_str!("../translations/es_mx.toml")),
    ("fr", include_str!("../translations/fr.toml")),
    ("it", include_str!("../translations/it.toml")),
    ("ja", include_str!("../translations/ja.toml")),
    ("ko", include_str!("../translations/ko.toml")),
    ("nl", include_str!("../translations/nl.toml")),
    ("pl", include_str!("../translations/pl.toml")),
    ("pt", include_str!("../translations/pt.toml")),
    ("pt_br", include_str!("../translations/pt_br.toml")),
    ("ru", include_str!("../translations/ru.toml")),
    ("th", include_str!("../translations/th.toml")),
    ("tr", include_str!("../translations/tr.toml")),
    ("uk", include_str!("../translations/uk.toml")),
    ("zh", include_str!("../translations/zh.toml")),
    ("zh_sg", include_str!("../translations/zh_sg.toml")),
    ("zh_tw", include_str!("../translations/zh_tw.toml")),
];

#[cfg(test)]
mod tests {
    use super::EMBEDDED;

    /// A locale file added to the directory but not to [`EMBEDDED`] would
    /// simply never load, with no error anywhere. This is the only thing
    /// keeping the two in step.
    #[test]
    fn every_locale_file_on_disk_is_embedded() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("translations");
        let mut on_disk: Vec<String> = std::fs::read_dir(&dir)
            .expect("translations directory")
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "toml"))
            .filter_map(|e| e.path().file_stem().and_then(|s| s.to_str()).map(str::to_owned))
            .collect();
        on_disk.sort();

        let mut embedded: Vec<String> = EMBEDDED.iter().map(|(locale, _)| (*locale).to_owned()).collect();
        embedded.sort();

        assert_eq!(embedded, on_disk);
    }

    #[test]
    fn every_embedded_catalog_has_content() {
        for (locale, source) in EMBEDDED {
            assert!(!source.trim().is_empty(), "{locale} embedded empty");
        }
    }
}
