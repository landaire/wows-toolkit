//! Buffs: the `Modifier` params that consumables, buff drops and game modes
//! apply to a ship for a time or for the rest of the battle.
//!
//! The client's heal modifiers are percent of maximum HP per second:
//! Repair Party feeds `healthRegenPercent` as `100 * regenerationHPSpeed`
//! (`CommonConsumables/Logic/Ship.py` `RegenCrew.__setPassiveRegenModifier`),
//! and ModifierSettings shows it as `regenerationHPSpeed_percent_sec`.
//! `healthRegenPercent` adds up; `healthRegenPercentAbsolute` is in the
//! `ABSOLUTE` group (`Modifiers/ModifierValueType.py`), so of two such buffs
//! only the stronger counts.

use serde::Serialize;
use wows_core::units::Km;
use wows_core::units::Seconds;
use wowsunpack::game_params::modifier_settings_data::describe_modifiers;
use wowsunpack::game_params::modifier_settings_data::modifier_setting;
use wowsunpack::game_params::types::AbilityCategory;
use wowsunpack::game_params::types::CrewSkillModifier;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::Species;

use crate::builds::round;
use crate::commanders::RegenRate;
use crate::commanders::regen_line;
use crate::data::GameData;
use crate::text::clean;

/// Heal modifiers, percent of maximum HP per second.
const HEAL_MODIFIERS: &[&str] = &["healthRegenPercent", "healthRegenPercentAbsolute"];

/// What a consumable does to the ship it is used on, for consumables that
/// apply a buff.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ConsumableBuff {
    /// Who the consumable can be used on.
    pub target: &'static str,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub range_km: Option<Km>,

    /// Effects on the allied ship it is used on.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub on_ally: Vec<String>,

    /// Effects when used on your own ship.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub on_self: Vec<String>,
}

/// How long a buff lasts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum BuffDuration {
    For(Seconds),

    /// The buff of a TIME_BASED consumable, which has no fixed action time.
    WhileActive,

    RestOfBattle,
}

impl BuffDuration {
    /// The buff duration of a consumable with action time `action_time`.
    /// Only TIME_BASED consumables have none.
    pub(crate) fn of_consumable(action_time: Option<Seconds>) -> Self {
        action_time.map_or(Self::WhileActive, Self::For)
    }
}

impl GameData {
    /// The modifiers of level `level` (1-based) of the `Modifier` param
    /// `name` for `species`; `None` when the param is missing, is not a
    /// modifier set, lacks the level or does not apply to `species`.
    pub(crate) fn buff_modifiers(&self, name: &str, level: usize, species: Species) -> Option<Vec<CrewSkillModifier>> {
        let param = self.provider().game_param_by_name(name)?;
        let modifiers = param.modifier_param()?;
        if !modifiers.applies_to(species) {
            return None;
        }

        modifiers.levels().get(level.checked_sub(1)?).cloned()
    }

    /// The buff a consumable applies, `None` for consumables without one.
    pub(crate) fn consumable_buff(
        &self,
        category: &AbilityCategory,
        species: Species,
        duration: BuffDuration,
    ) -> Option<ConsumableBuff> {
        let lines = |name: Option<&str>| {
            name.and_then(|name| self.buff_modifiers(name, 1, species))
                .map(|modifiers| self.buff_lines(&modifiers, species, duration))
                .unwrap_or_default()
        };
        let on_allies = category.target_team() == Some("ally");
        let (on_ally, on_self) = if on_allies {
            (lines(category.buff()), lines(category.buff_on_self()))
        } else {
            (Vec::new(), lines(category.buff()))
        };
        if on_ally.is_empty() && on_self.is_empty() {
            return None;
        }

        Some(ConsumableBuff {
            target: if on_allies { "an allied ship in range or your own ship" } else { "your own ship" },
            // A buff for your own ship only has no range that matters.
            range_km: category
                .effect_fields()
                .get("workRange")
                .filter(|_| on_allies)
                .map(|meters| Km::new(round(meters / 1000.0))),
            on_ally,
            on_self,
        })
    }

    /// A buff's effects for `species`: heals first, then the other modifiers
    /// as the client describes them.
    pub(crate) fn buff_lines(
        &self,
        modifiers: &[CrewSkillModifier],
        species: Species,
        duration: BuffDuration,
    ) -> Vec<String> {
        let (duration_text, seconds) = match duration {
            BuffDuration::For(seconds) => (format!("{} s", round(seconds.value())), Some(seconds.value())),
            BuffDuration::WhileActive => ("as long as the consumable is active".to_owned(), None),
            BuffDuration::RestOfBattle => ("the rest of the battle".to_owned(), None),
        };
        let mut lines: Vec<String> = HEAL_MODIFIERS
            .iter()
            .filter_map(|name| heal_rate(modifiers, name, species))
            .map(|percent| regen_line(RegenRate::FractionPerSecond(percent / 100.0), false, &duration_text, seconds))
            .collect();
        for modifier in modifiers.iter().filter(|modifier| !HEAL_MODIFIERS.contains(&modifier.name())) {
            let value = modifier.get_for_species(&species);
            let mut described =
                describe_modifiers(self.version(), [(modifier.name(), value)], species, self.provider());
            // An `...Absolute` modifier is the ABSOLUTE-group form of the
            // modifier without the suffix (`Modifiers/ModifierValueType.py`),
            // shown with that modifier's settings when it has none of its own.
            if described.is_empty()
                && modifier_setting(self.version(), modifier.name()).is_none()
                && let Some(base) = modifier.name().strip_suffix("Absolute")
            {
                described = describe_modifiers(self.version(), [(base, value)], species, self.provider());
            }
            lines.extend(described.iter().map(|line| clean(line)));
        }

        lines
    }
}

/// The heal rate `name` gives, percent of maximum HP per second.
pub(crate) fn heal_rate(modifiers: &[CrewSkillModifier], name: &str, species: Species) -> Option<f32> {
    modifiers
        .iter()
        .find(|modifier| modifier.name() == name)
        .map(|modifier| modifier.get_for_species(&species))
        .filter(|rate| *rate > 0.0)
}

/// The total heal rate of a buff, percent of maximum HP per second.
pub(crate) fn buff_heal_rate(modifiers: &[CrewSkillModifier], species: Species) -> Option<f32> {
    let total: f32 = HEAL_MODIFIERS.iter().filter_map(|name| heal_rate(modifiers, name, species)).sum();

    (total > 0.0).then_some(total)
}
