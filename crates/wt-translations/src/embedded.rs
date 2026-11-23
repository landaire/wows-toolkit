//! The translation catalogs, embedded in the binary.

/// Every bundled locale, as `(locale, Fluent source)`.
pub static EMBEDDED: &[(&str, &str)] = &[
    ("cs", include_str!("../translations/cs.ftl")),
    ("de", include_str!("../translations/de.ftl")),
    ("en", include_str!("../translations/en.ftl")),
    ("es", include_str!("../translations/es.ftl")),
    ("es_mx", include_str!("../translations/es_mx.ftl")),
    ("fr", include_str!("../translations/fr.ftl")),
    ("it", include_str!("../translations/it.ftl")),
    ("ja", include_str!("../translations/ja.ftl")),
    ("ko", include_str!("../translations/ko.ftl")),
    ("nl", include_str!("../translations/nl.ftl")),
    ("pl", include_str!("../translations/pl.ftl")),
    ("pt", include_str!("../translations/pt.ftl")),
    ("pt_br", include_str!("../translations/pt_br.ftl")),
    ("ru", include_str!("../translations/ru.ftl")),
    ("th", include_str!("../translations/th.ftl")),
    ("tr", include_str!("../translations/tr.ftl")),
    ("uk", include_str!("../translations/uk.ftl")),
    ("zh", include_str!("../translations/zh.ftl")),
    ("zh_sg", include_str!("../translations/zh_sg.ftl")),
    ("zh_tw", include_str!("../translations/zh_tw.ftl")),
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
            .filter(|e| e.path().extension().is_some_and(|x| x == "ftl"))
            .filter_map(|e| e.path().file_stem().and_then(|s| s.to_str()).map(str::to_owned))
            .collect();
        on_disk.sort();

        let mut embedded: Vec<String> = EMBEDDED.iter().map(|(locale, _)| (*locale).to_owned()).collect();
        embedded.sort();

        assert_eq!(embedded, on_disk);
    }

    #[test]
    fn every_supported_language_is_embedded() {
        for info in crate::SUPPORTED_LANGUAGES {
            assert!(EMBEDDED.iter().any(|(code, _)| *code == info.code), "{} is not embedded", info.code);
        }
    }

    #[test]
    fn every_embedded_catalog_has_content() {
        for (locale, source) in EMBEDDED {
            assert!(!source.trim().is_empty(), "{locale} embedded empty");
        }
    }
}
