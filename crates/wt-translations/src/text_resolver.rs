//! TextResolver trait for resolving game enums to display text.
//!
//! The minimap renderer and other consumers use this trait to convert
//! translatable items into display strings.

use wows_core::game_types::AdvantageLevel;
use wows_core::game_types::BattleResult;
use wows_core::game_types::FinishType;
use wows_core::recognized::Recognized;

use crate::keys;

/// Items that need translation for display.
#[derive(Debug, Clone)]
pub enum TranslatableText {
    /// Battle outcome (Victory, Defeat, Draw).
    BattleResult(BattleResult),
    /// How the battle ended.
    FinishType(Recognized<FinishType>),
    /// "BATTLE STARTS IN" label for pre-battle countdown.
    PreBattleLabel,
    /// Team advantage strength label.
    Advantage(AdvantageLevel),
}

/// Resolves [`TranslatableText`] items into display strings.
pub trait TextResolver: Send + Sync {
    fn resolve(&self, text: &TranslatableText) -> String;
}

/// Resolves through the catalogs in the interface language.
pub struct DefaultTextResolver;

impl TextResolver for DefaultTextResolver {
    fn resolve(&self, text: &TranslatableText) -> String {
        crate::t!(text.key()).into_owned()
    }
}

impl TranslatableText {
    /// Get the translation key for this item.
    pub fn key(&self) -> &'static str {
        match self {
            TranslatableText::BattleResult(r) => keys::battle_result_key(*r),
            TranslatableText::FinishType(ft) => match ft {
                Recognized::Known(ft) => keys::finish_type_key(*ft),
                Recognized::Unknown(_) => "finish_type-unknown",
            },
            TranslatableText::PreBattleLabel => keys::PRE_BATTLE_KEY,
            TranslatableText::Advantage(level) => keys::advantage_key(*level),
        }
    }
}
