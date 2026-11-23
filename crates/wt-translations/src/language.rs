//! Supported languages and the process-wide interface language.

use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use crate::UnknownLocale;

/// Metadata for a supported language.
pub struct LanguageInfo {
    /// The World of Warships locale code, which also names the catalog file.
    pub code: &'static str,
    /// BCP 47 tag the catalog's plural rules are chosen by.
    pub langid: &'static str,
    pub native_name: &'static str,
}

/// All languages supported by World of Warships (and this toolkit).
/// Sorted by native name (Latin-script languages first alphabetically,
/// then non-Latin scripts). English is pinned at the top as the fallback.
pub const SUPPORTED_LANGUAGES: &[LanguageInfo] = &[
    LanguageInfo { code: "en", langid: "en", native_name: "English" },
    LanguageInfo { code: "cs", langid: "cs", native_name: "Čeština" },
    LanguageInfo { code: "de", langid: "de", native_name: "Deutsch" },
    LanguageInfo { code: "es", langid: "es", native_name: "Español" },
    LanguageInfo { code: "es_mx", langid: "es-MX", native_name: "Español (LA)" },
    LanguageInfo { code: "fr", langid: "fr", native_name: "Français" },
    LanguageInfo { code: "it", langid: "it", native_name: "Italiano" },
    LanguageInfo { code: "nl", langid: "nl", native_name: "Nederlands" },
    LanguageInfo { code: "pl", langid: "pl", native_name: "Polski" },
    LanguageInfo { code: "pt", langid: "pt", native_name: "Português" },
    LanguageInfo { code: "pt_br", langid: "pt-BR", native_name: "Português (Brasil)" },
    LanguageInfo { code: "tr", langid: "tr", native_name: "Türkçe" },
    LanguageInfo { code: "uk", langid: "uk", native_name: "Українська" },
    LanguageInfo { code: "ru", langid: "ru", native_name: "Русский" },
    LanguageInfo { code: "th", langid: "th", native_name: "ไทย" },
    LanguageInfo { code: "ko", langid: "ko", native_name: "한국어" },
    LanguageInfo { code: "ja", langid: "ja", native_name: "日本語" },
    LanguageInfo { code: "zh", langid: "zh", native_name: "简体中文" },
    LanguageInfo { code: "zh_sg", langid: "zh-SG", native_name: "简体中文 (SG)" },
    LanguageInfo { code: "zh_tw", langid: "zh-TW", native_name: "繁體中文" },
];

/// A supported language, as its position in [`SUPPORTED_LANGUAGES`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Language(usize);

impl Language {
    pub const ENGLISH: Self = Self(0);

    pub fn from_code(code: &str) -> Result<Self, UnknownLocale> {
        SUPPORTED_LANGUAGES
            .iter()
            .position(|info| info.code == code)
            .map(Self)
            .ok_or_else(|| UnknownLocale { code: code.to_owned() })
    }

    pub fn info(self) -> &'static LanguageInfo {
        &SUPPORTED_LANGUAGES[self.0]
    }

    pub fn code(self) -> &'static str {
        self.info().code
    }

    pub(crate) fn index(self) -> usize {
        self.0
    }
}

static CURRENT: AtomicUsize = AtomicUsize::new(Language::ENGLISH.0);

/// Switches every translated string in the process to `code`.
pub fn set_locale(code: &str) -> Result<(), UnknownLocale> {
    let language = Language::from_code(code)?;
    set_language(language);

    Ok(())
}

pub fn set_language(language: Language) {
    CURRENT.store(language.0, Ordering::Relaxed);
}

/// The interface language. English until [`set_locale`] picks another.
pub fn language() -> Language {
    Language(CURRENT.load(Ordering::Relaxed))
}

/// The code of the interface language, such as `en` or `pt_br`.
pub fn locale() -> &'static str {
    language().code()
}

/// Map a system locale string (e.g. "de-DE", "ja-JP") to a WoWs locale code.
///
/// Returns `None` if the locale doesn't match any supported language.
pub fn system_locale_to_wows(sys_locale: &str) -> Option<&'static str> {
    // Normalize: lowercase, replace '-' with '_'
    let normalized = sys_locale.to_lowercase().replace('-', "_");

    // Try exact match first (e.g. "pt_br" -> "pt_br")
    for lang in SUPPORTED_LANGUAGES {
        if normalized == lang.code {
            return Some(lang.code);
        }
    }

    // Try language-only prefix (e.g. "de_de" -> "de", "ja_jp" -> "ja")
    let prefix = normalized.split('_').next()?;

    // Special cases: Chinese variants
    if prefix == "zh" {
        // zh_TW, zh_HK -> zh_tw (Traditional)
        if normalized.contains("tw") || normalized.contains("hk") {
            return Some("zh_tw");
        }
        // zh_SG -> zh_sg
        if normalized.contains("sg") {
            return Some("zh_sg");
        }
        // zh_CN or plain zh -> zh (Simplified)
        return Some("zh");
    }

    // Special cases: Portuguese variants
    if prefix == "pt" {
        if normalized.contains("br") {
            return Some("pt_br");
        }
        return Some("pt");
    }

    // Special cases: Spanish variants
    if prefix == "es" {
        if normalized.contains("mx") || normalized.contains("419") {
            return Some("es_mx");
        }
        return Some("es");
    }

    // Generic: match by language prefix
    for lang in SUPPORTED_LANGUAGES {
        if lang.code == prefix {
            return Some(lang.code);
        }
    }

    None
}

/// Get the native language name for a locale code.
pub fn language_name(code: &str) -> Option<&'static str> {
    SUPPORTED_LANGUAGES.iter().find(|l| l.code == code).map(|l| l.native_name)
}

/// Get all available locale codes.
pub fn available_locales() -> Vec<&'static str> {
    SUPPORTED_LANGUAGES.iter().map(|l| l.code).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_locale_mapping() {
        assert_eq!(system_locale_to_wows("en-US"), Some("en"));
        assert_eq!(system_locale_to_wows("de-DE"), Some("de"));
        assert_eq!(system_locale_to_wows("ja-JP"), Some("ja"));
        assert_eq!(system_locale_to_wows("zh-TW"), Some("zh_tw"));
        assert_eq!(system_locale_to_wows("zh-CN"), Some("zh"));
        assert_eq!(system_locale_to_wows("zh-SG"), Some("zh_sg"));
        assert_eq!(system_locale_to_wows("pt-BR"), Some("pt_br"));
        assert_eq!(system_locale_to_wows("pt-PT"), Some("pt"));
        assert_eq!(system_locale_to_wows("es-MX"), Some("es_mx"));
        assert_eq!(system_locale_to_wows("es-ES"), Some("es"));
        assert_eq!(system_locale_to_wows("ko"), Some("ko"));
        assert_eq!(system_locale_to_wows("xx-YY"), None);
    }

    #[test]
    fn test_language_name_lookup() {
        assert_eq!(language_name("en"), Some("English"));
        assert_eq!(language_name("de"), Some("Deutsch"));
        assert_eq!(language_name("nonexistent"), None);
    }

    #[test]
    fn test_available_locales() {
        let locales = available_locales();
        assert_eq!(locales.len(), 20);
        assert!(locales.contains(&"en"));
        assert!(locales.contains(&"ja"));
    }

    #[test]
    fn english_is_the_fallback_language() {
        assert_eq!(Language::ENGLISH.code(), "en");
    }

    #[test]
    fn an_unsupported_code_is_rejected_with_the_code() {
        assert_eq!(Language::from_code("xx"), Err(UnknownLocale { code: "xx".to_owned() }));
    }

    #[test]
    fn every_langid_is_a_valid_bcp47_tag() {
        for info in SUPPORTED_LANGUAGES {
            let parsed: Result<unic_langid::LanguageIdentifier, _> = info.langid.parse();
            assert!(parsed.is_ok(), "{}: {}", info.code, info.langid);
        }
    }
}
