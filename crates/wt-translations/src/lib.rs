//! Translation key mappings, TextResolver trait, and language metadata for WoWs Toolkit.
//!
//! This crate does NOT own the translation machinery (that's `rust-i18n` in each
//! consumer crate). It provides:
//!
//! - **Const fn key mappings**: enum variants -> dotted translation key strings
//! - **`TextResolver` trait**: for consumers that need translated text without
//!   depending on `rust-i18n` (e.g. minimap renderer's `ImageTarget`)
//! - **Language metadata**: supported locales, native names, system locale mapping

pub mod keys;
mod text_resolver;

pub use text_resolver::{DefaultTextResolver, TextResolver, TranslatableText};

/// Runtime icon + translated text concatenation.
///
/// Replaces the compile-time `icon_str!` macro for use with runtime translations.
/// The caller passes the already-translated string (from their own `t!()` call).
pub fn icon_t(icon: &str, translated: &str) -> String {
    format!("{icon} {translated}")
}

/// Metadata for a supported language.
pub struct LanguageInfo {
    pub code: &'static str,
    pub native_name: &'static str,
}

/// All languages supported by World of Warships (and this toolkit).
/// Sorted by native name (Latin-script languages first alphabetically,
/// then non-Latin scripts). English is pinned at the top as the fallback.
pub const SUPPORTED_LANGUAGES: &[LanguageInfo] = &[
    LanguageInfo { code: "en", native_name: "English" },
    LanguageInfo { code: "cs", native_name: "Čeština" },
    LanguageInfo { code: "de", native_name: "Deutsch" },
    LanguageInfo { code: "es", native_name: "Español" },
    LanguageInfo { code: "es_mx", native_name: "Español (LA)" },
    LanguageInfo { code: "fr", native_name: "Français" },
    LanguageInfo { code: "it", native_name: "Italiano" },
    LanguageInfo { code: "nl", native_name: "Nederlands" },
    LanguageInfo { code: "pl", native_name: "Polski" },
    LanguageInfo { code: "pt", native_name: "Português" },
    LanguageInfo { code: "pt_br", native_name: "Português (Brasil)" },
    LanguageInfo { code: "tr", native_name: "Türkçe" },
    LanguageInfo { code: "uk", native_name: "Українська" },
    LanguageInfo { code: "ru", native_name: "Русский" },
    LanguageInfo { code: "th", native_name: "ไทย" },
    LanguageInfo { code: "ko", native_name: "한국어" },
    LanguageInfo { code: "ja", native_name: "日本語" },
    LanguageInfo { code: "zh", native_name: "简体中文" },
    LanguageInfo { code: "zh_sg", native_name: "简体中文 (SG)" },
    LanguageInfo { code: "zh_tw", native_name: "繁體中文" },
];

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
    fn test_icon_t() {
        let result = icon_t("\u{e900}", "Settings");
        assert_eq!(result, "\u{e900} Settings");
    }

    #[test]
    fn test_key_mappings() {
        use wowsunpack::game_types::{AdvantageLevel, BattleResult, FinishType};

        assert_eq!(keys::battle_result_key(BattleResult::Victory), "battle.victory");
        assert_eq!(keys::battle_result_key(BattleResult::Defeat), "battle.defeat");
        assert_eq!(keys::battle_result_key(BattleResult::Draw), "battle.draw");

        assert_eq!(keys::finish_type_key(FinishType::Extermination), "finish_type.extermination");
        assert_eq!(keys::finish_type_key(FinishType::BaseCaptured), "finish_type.base_captured");

        assert_eq!(keys::advantage_key(AdvantageLevel::Strong), "advantage.strong");
        assert_eq!(keys::advantage_key(AdvantageLevel::Moderate), "advantage.moderate");
    }

    #[test]
    fn test_translatable_text_key() {
        use wowsunpack::game_types::BattleResult;
        use wowsunpack::recognized::Recognized;

        let t = TranslatableText::BattleResult(BattleResult::Victory);
        assert_eq!(t.key(), "battle.victory");

        let t = TranslatableText::PreBattleLabel;
        assert_eq!(t.key(), "battle.battle_starts_in");

        let t = TranslatableText::FinishType(Recognized::Unknown("CUSTOM".to_string()));
        assert_eq!(t.key(), "finish_type.unknown");
    }

    #[test]
    fn test_default_text_resolver() {
        use wowsunpack::game_types::{BattleResult, FinishType};
        use wowsunpack::recognized::Recognized;

        let resolver = DefaultTextResolver;

        assert_eq!(
            resolver.resolve(&TranslatableText::BattleResult(BattleResult::Victory)),
            "VICTORY"
        );
        assert_eq!(
            resolver.resolve(&TranslatableText::FinishType(Recognized::Known(
                FinishType::Extermination
            ))),
            "All enemy ships destroyed"
        );
        assert_eq!(resolver.resolve(&TranslatableText::PreBattleLabel), "BATTLE STARTS IN");
    }
}
