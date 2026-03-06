//! TextResolver trait for resolving game enums to display text.
//!
//! The minimap renderer and other consumers use this trait to convert
//! translatable items into display strings without depending on i18n directly.

use wowsunpack::game_types::{AdvantageLevel, BattleResult, FinishType};
use wowsunpack::recognized::Recognized;

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
///
/// Consumers (wows-toolkit, wt-web) implement this with their own `t!()` macro.
/// [`DefaultTextResolver`] provides a no-dependency English fallback.
pub trait TextResolver: Send + Sync {
    fn resolve(&self, text: &TranslatableText) -> String;
}

/// English-only fallback that requires no i18n infrastructure.
pub struct DefaultTextResolver;

impl TextResolver for DefaultTextResolver {
    fn resolve(&self, text: &TranslatableText) -> String {
        match text {
            TranslatableText::BattleResult(r) => match r {
                BattleResult::Victory => "VICTORY",
                BattleResult::Defeat => "DEFEAT",
                BattleResult::Draw => "DRAW",
            }
            .into(),
            TranslatableText::FinishType(ft) => match ft {
                Recognized::Known(ft) => default_finish_type_text(*ft).into(),
                Recognized::Unknown(raw) => raw.clone(),
            },
            TranslatableText::PreBattleLabel => "BATTLE STARTS IN".into(),
            TranslatableText::Advantage(level) => level.label().into(),
        }
    }
}

/// English finish type descriptions (matches the TOML en.toml values).
const fn default_finish_type_text(ft: FinishType) -> &'static str {
    match ft {
        FinishType::Unknown => "Battle ended",
        FinishType::Extermination => "All enemy ships destroyed",
        FinishType::BaseCaptured => "Base captured",
        FinishType::Timeout => "Time expired",
        FinishType::Failure => "Mission failed",
        FinishType::Technical => "Technical finish",
        FinishType::Score => "Score limit reached",
        FinishType::ScoreOnTimeout => "Score lead at time limit",
        FinishType::PveMainTaskSucceeded => "Mission accomplished",
        FinishType::PveMainTaskFailed => "Mission failed",
        FinishType::ScoreZero => "Score depleted",
        FinishType::ScoreExcess => "Score limit exceeded",
    }
}

impl TranslatableText {
    /// Get the translation key for this item.
    pub fn key(&self) -> &'static str {
        match self {
            TranslatableText::BattleResult(r) => keys::battle_result_key(*r),
            TranslatableText::FinishType(ft) => match ft {
                Recognized::Known(ft) => keys::finish_type_key(*ft),
                Recognized::Unknown(_) => "finish_type.unknown",
            },
            TranslatableText::PreBattleLabel => keys::PRE_BATTLE_KEY,
            TranslatableText::Advantage(level) => keys::advantage_key(*level),
        }
    }
}
