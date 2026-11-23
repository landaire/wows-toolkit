//! Translation catalogs, the `t!` macro, and language metadata for WoWs Toolkit.
//!
//! The catalogs are Fluent files, one per entry of [`SUPPORTED_LANGUAGES`],
//! formatted in one process-wide interface language set by [`set_locale`].

mod args;
mod catalog;
pub mod embedded;
mod error;
pub mod keys;
mod language;
mod text_resolver;

pub use catalog::has_message;
pub use catalog::translate;
pub use catalog::translate_in;
pub use error::UnknownLocale;
pub use language::Language;
pub use language::LanguageInfo;
pub use language::SUPPORTED_LANGUAGES;
pub use language::available_locales;
pub use language::language;
pub use language::language_name;
pub use language::locale;
pub use language::set_language;
pub use language::set_locale;
pub use language::system_locale_to_wows;
pub use text_resolver::DefaultTextResolver;
pub use text_resolver::TextResolver;
pub use text_resolver::TranslatableText;

#[doc(hidden)]
pub mod __private {
    pub use crate::args::Arg;
    pub use crate::args::ArgFromDisplay;
    pub use crate::args::ArgFromNumber;
    pub use fluent_bundle::FluentArgs;
}

/// Formats a catalog message in the interface language.
///
/// `t!(id)`, `t!(id, name = value, ...)`, or with a leading
/// `locale = language` (a [`Language`]) to format in that language instead.
/// Numeric arguments are passed as Fluent numbers so plural selectors apply;
/// anything else is passed as its `Display` text. Returns
/// `Cow<'static, str>`, always owned.
#[macro_export]
macro_rules! t {
    ($id:expr, locale = $language:expr $(, $name:ident = $value:expr)* $(,)?) => {
        $crate::translate_in($language, ::core::convert::AsRef::<str>::as_ref(&$id), $crate::t!(@args $($name = $value),*).as_ref())
    };
    ($id:expr $(, $name:ident = $value:expr)* $(,)?) => {
        $crate::translate(::core::convert::AsRef::<str>::as_ref(&$id), $crate::t!(@args $($name = $value),*).as_ref())
    };
    (@args) => {
        ::core::option::Option::<$crate::__private::FluentArgs<'static>>::None
    };
    (@args $($name:ident = $value:expr),+) => {{
        #[allow(unused_imports)]
        use $crate::__private::ArgFromDisplay as _;
        #[allow(unused_imports)]
        use $crate::__private::ArgFromNumber as _;

        let mut args = $crate::__private::FluentArgs::new();
        $(args.set(::core::stringify!($name), (&$crate::__private::Arg(&$value)).fluent_value());)+
        ::core::option::Option::Some(args)
    }};
}

/// Runtime icon + translated text concatenation.
///
/// The caller passes the already-translated string (from a `t!()` call).
pub fn icon_t(icon: &str, translated: &str) -> String {
    format!("{icon} {translated}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_icon_t() {
        let result = icon_t("\u{e900}", "Settings");
        assert_eq!(result, "\u{e900} Settings");
    }

    #[test]
    fn test_key_mappings() {
        use wows_core::game_types::AdvantageLevel;
        use wows_core::game_types::BattleResult;
        use wows_core::game_types::ExclusionReason;
        use wows_core::game_types::FinishType;

        assert_eq!(keys::battle_result_key(BattleResult::Victory), "battle-victory");
        assert_eq!(keys::battle_result_key(BattleResult::Defeat), "battle-defeat");
        assert_eq!(keys::battle_result_key(BattleResult::Draw), "battle-draw");

        assert_eq!(keys::finish_type_key(FinishType::Extermination), "finish_type-extermination");
        assert_eq!(keys::finish_type_key(FinishType::BaseCaptured), "finish_type-base_captured");

        assert_eq!(keys::advantage_key(AdvantageLevel::Strong), "advantage-strong");
        assert_eq!(keys::advantage_key(AdvantageLevel::Moderate), "advantage-moderate");

        assert_eq!(
            keys::exclusion_reason_key(ExclusionReason::SectionAlreadyBurning),
            "ui-replay-sections-fire_chance_exclusion_already_burning"
        );
        assert_eq!(
            keys::exclusion_reason_key(ExclusionReason::DamageControlActive),
            "ui-replay-sections-fire_chance_exclusion_damage_control_active"
        );
    }

    #[test]
    fn test_translatable_text_key() {
        use wows_core::game_types::BattleResult;
        use wows_core::recognized::Recognized;

        let t = TranslatableText::BattleResult(BattleResult::Victory);
        assert_eq!(t.key(), "battle-victory");

        let t = TranslatableText::PreBattleLabel;
        assert_eq!(t.key(), "battle-battle_starts_in");

        let t = TranslatableText::FinishType(Recognized::Unknown("CUSTOM".to_string()));
        assert_eq!(t.key(), "finish_type-unknown");
    }

    #[test]
    fn test_default_text_resolver() {
        use wows_core::game_types::BattleResult;
        use wows_core::game_types::FinishType;
        use wows_core::recognized::Recognized;

        let resolver = DefaultTextResolver;

        assert_eq!(resolver.resolve(&TranslatableText::BattleResult(BattleResult::Victory)), "VICTORY");
        assert_eq!(
            resolver.resolve(&TranslatableText::FinishType(Recognized::Known(FinishType::Extermination))),
            "All enemy ships destroyed"
        );
        assert_eq!(resolver.resolve(&TranslatableText::PreBattleLabel), "BATTLE STARTS IN");
    }

    #[test]
    fn english_messages_format_with_their_arguments() {
        assert_eq!(t!("stat-damage", locale = Language::ENGLISH), "Damage");
        assert_eq!(t!("ui-tabs-directory_stats", locale = Language::ENGLISH, dir = "Replays"), "Replays Stats");
    }

    #[test]
    fn a_missing_id_formats_as_the_id() {
        let german = Language::from_code("de").expect("de is supported");
        assert_eq!(t!("no-such-message", locale = german), "no-such-message");
        assert_eq!(t!("no-such-message", locale = Language::ENGLISH, count = 3), "no-such-message");
    }
}
