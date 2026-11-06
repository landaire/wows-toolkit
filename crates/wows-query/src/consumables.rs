use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use serde::Serializer;
use wows_core::units::Km;
use wows_core::units::Meters;
use wows_core::units::Percent;
use wows_core::units::Seconds;
use wowsunpack::game_params::translations::translate_consumable;
use wowsunpack::game_params::ttx::consumables::effective_consumable;
use wowsunpack::game_params::ttx::model::AmmoCount;
use wowsunpack::game_params::types::AbilityCategory;
use wowsunpack::game_params::types::GameParamProvider;

use crate::data::GameData;
use crate::error::QueryError;
use crate::loadout::AppliedSkill;
use crate::loadout::AppliedUpgrade;
use crate::loadout::LoadoutRequest;
use crate::ships::ShipSummary;
use crate::text::clean;

/// Arguments for `ship_consumables`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShipConsumablesArgs {
    /// Ship name, e.g. "Des Moines".
    pub ship: String,

    /// Captain skills by name or acronym, e.g. ["SI"], applied to the consumable stats.
    pub skills: Option<Vec<String>>,

    /// Upgrades by name, e.g. ["Engine Boost Modification 1"], applied to the consumable stats.
    pub upgrades: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShipConsumables {
    pub ship: ShipSummary,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<AppliedSkill>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub upgrades: Vec<AppliedUpgrade>,

    /// Consumable slots in port order. A slot with several options mounts one of them.
    pub slots: Vec<ConsumableSlot>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConsumableSlot {
    /// 1-based.
    pub slot: usize,
    pub options: Vec<ConsumableStats>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConsumableStats {
    pub name: String,

    /// GameParams consumable type, e.g. "rls" (radar), "sonar" (hydro), "crashCrew".
    #[serde(rename = "type")]
    pub kind: String,

    pub charges: Charges,
    pub reload_s: Seconds,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub action_time_s: Option<Seconds>,

    /// Activation delay. Absent when zero.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preparation_s: Option<Seconds>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub ship_detection_m: Option<Meters>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub torpedo_detection_m: Option<Meters>,

    /// Repair Party heal rate, percent of maximum HP per second.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heal_percent_per_s: Option<Percent>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub smoke_radius_km: Option<Km>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub smoke_lifetime_s: Option<Seconds>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub fighters: Option<f32>,

    /// Usage pool of time-based consumables.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capacity_s: Option<Seconds>,
}

/// Number of charges, or unlimited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Charges {
    Limited(u32),
    Unlimited,
}

impl Serialize for Charges {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Limited(count) => serializer.serialize_u32(*count),
            Self::Unlimited => serializer.serialize_str("unlimited"),
        }
    }
}

impl GameData {
    /// The consumables a ship can mount, with skills and upgrades applied.
    pub fn ship_consumables(&self, args: &ShipConsumablesArgs) -> Result<ShipConsumables, QueryError> {
        let ship = self.resolve_ship(&args.ship)?;
        let request = LoadoutRequest {
            skills: args.skills.as_deref().unwrap_or_default(),
            upgrades: args.upgrades.as_deref().unwrap_or_default(),
            conditions_met: false,
            hp_percent: None,
        };
        let loadout = self.resolve_loadout(ship, &request)?;
        let effects = self.effective_modifiers(ship, &loadout)?;
        let bundle = effects.modifiers.bundle();

        let mut slots = Vec::new();
        let ability_slots = ship.param.vehicle().and_then(|vehicle| vehicle.abilities()).unwrap_or_default();
        for (index, slot) in ability_slots.iter().enumerate() {
            let options: Vec<ConsumableStats> = slot
                .iter()
                .filter_map(|(ability, variant)| {
                    let param = self.provider().game_param_by_name(ability)?;
                    let category = param.ability()?.get_category(variant)?;
                    let name = translate_consumable(ability, self.provider()).map(|name| clean(&name));
                    Some(consumable_stats(name, category, bundle))
                })
                .collect();
            if !options.is_empty() {
                slots.push(ConsumableSlot { slot: index + 1, options });
            }
        }

        Ok(ShipConsumables {
            ship: ship.summary(),
            skills: loadout.applied_skills,
            upgrades: loadout.applied_upgrades,
            slots,
        })
    }
}

fn consumable_stats(
    name: Option<String>,
    category: &AbilityCategory,
    bundle: &wowsunpack::game_params::ttx::modifiers::ModifierBundle,
) -> ConsumableStats {
    let (stats, _) = effective_consumable(category, bundle);
    let kind = category.consumable_type_raw().to_owned();

    ConsumableStats {
        name: name.unwrap_or_else(|| kind.clone()),
        kind,
        charges: match stats.charges {
            AmmoCount::Finite(count) => Charges::Limited(count),
            AmmoCount::Infinite => Charges::Unlimited,
        },
        reload_s: Seconds::new(round(stats.reload_time.value())),
        action_time_s: stats.work_time.map(|time| Seconds::new(round(time.value()))),
        preparation_s: Some(stats.preparation_time.value())
            .filter(|time| *time > 0.0)
            .map(|time| Seconds::new(round(time))),
        // BigWorld unit conversion leaves 9999.9 for a 10 km radar.
        ship_detection_m: stats.detection_radius.map(|radius| Meters::new(radius.value().round())),
        torpedo_detection_m: category.torpedo_detection_radius().map(|radius| Meters::new(radius.value().round())),
        heal_percent_per_s: stats.regeneration_hp_speed.map(|rate| Percent::new(round(rate * 100.0))),
        smoke_radius_km: stats.smoke_radius.map(|radius| Km::new(round(radius.value()))),
        smoke_lifetime_s: stats.smoke_lifetime.map(|time| Seconds::new(round(time.value()))),
        fighters: stats.fighters_count,
        capacity_s: stats.max_capacity.map(|capacity| Seconds::new(round(capacity))),
    }
}

/// Two decimals, enough for every port value.
fn round(value: f32) -> f32 {
    (value * 100.0).round() / 100.0
}
