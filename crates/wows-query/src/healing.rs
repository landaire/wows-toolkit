//! Every way a ship can restore HP: its own consumables, consumables allied
//! ships and carrier squadrons use on it, unique commander talents and game
//! mode buffs.

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use wowsunpack::game_params::types::AbilityCategory;
use wowsunpack::game_params::types::CrewTalent;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::KnownTalentEffect;
use wowsunpack::game_params::types::Species;

use crate::buffs::BuffDuration;
use crate::buffs::buff_heal_rate;
use crate::commanders::Talent;
use crate::consumables::ConsumableStats;
use crate::consumables::consumable_stats;
use crate::data::GameData;
use crate::error::QueryError;
use crate::game_modes::GameModeBuffs;
use crate::names::Interpretations;
use crate::ships::Ship;
use crate::ships::ShipClass;
use crate::ships::ShipStatus;
use crate::ships::ShipSummary;

/// Arguments for `healing_sources`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HealingSourcesArgs {
    /// Ship to list every way it can restore HP, e.g. "Akizuki". Omit to list the consumables that heal allied ships and the ships that carry them.
    pub ship: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HealingSourcesOutput {
    #[serde(skip_serializing_if = "Interpretations::is_empty")]
    pub interpretations: Interpretations,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub ship: Option<ShipSummary>,

    /// The ship's own consumables that restore its HP, including those its
    /// squadrons drop on it.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub own_consumables: Vec<OwnHeal>,

    /// Consumables that allied ships or their squadrons use on another ship
    /// to restore its HP, with the ships that carry them.
    pub ally_heals: Vec<AllyHeal>,

    /// Unique commanders who can command the ship, with their talents that
    /// restore HP. Talents activate in battle on the condition given.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub commander_talents: Vec<CommanderHeals>,

    /// Game mode buffs that restore the ship's HP.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub game_modes: Vec<GameModeBuffs>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OwnHeal {
    /// The squadrons that drop it, for carrier plane consumables. Absent when
    /// the ship uses it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub squadrons: Option<Vec<String>>,

    #[serde(flatten)]
    pub consumable: ConsumableStats,
}

#[derive(Debug, Clone, Serialize)]
pub struct AllyHeal {
    pub consumable: String,

    /// The ships that carry it, grouped by the values they share.
    pub carriers: Vec<AllyHealCarriers>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AllyHealCarriers {
    pub ships: Vec<String>,

    pub delivered_by: DeliveredBy,

    #[serde(flatten)]
    pub consumable: ConsumableStats,
}

/// Whether the ship uses a consumable, or its squadrons drop it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveredBy {
    Ship,
    Squadron,
}

#[derive(Debug, Clone, Serialize)]
pub struct CommanderHeals {
    pub commander: String,
    pub talents: Vec<Talent>,
}

/// One consumable choice of a ship or of one of its squadrons.
struct Mounted {
    ability: String,
    variant: String,
    carrier: Carrier,
}

/// What carries a mounted consumable.
enum Carrier {
    Ship,

    /// English names of the squadrons that carry it.
    Squadrons(Vec<String>),
}

impl Carrier {
    fn squadrons(self) -> Option<Vec<String>> {
        match self {
            Self::Ship => None,
            Self::Squadrons(names) => Some(names),
        }
    }

    fn delivered_by(&self) -> DeliveredBy {
        match self {
            Self::Ship => DeliveredBy::Ship,
            Self::Squadrons(_) => DeliveredBy::Squadron,
        }
    }
}

impl GameData {
    /// The ways `ship` can restore HP, or without a ship, the consumables
    /// that heal allied ships and the ships that carry them.
    pub fn healing_sources(&self, args: &HealingSourcesArgs) -> Result<HealingSourcesOutput, QueryError> {
        let mut interpretations = Interpretations::default();
        let ship = match args.ship.as_deref() {
            Some(name) => Some(self.resolve_ship(name, &mut interpretations)?),
            None => None,
        };

        let mut own_consumables = Vec::new();
        let mut commander_talents = Vec::new();
        let mut game_modes = Vec::new();
        if let Some(ship) = ship {
            let species = ship.class.species();
            for mounted in self.mounted_consumables(ship) {
                let Some(param) = self.provider().game_param_by_name(&mounted.ability) else {
                    continue;
                };
                let Some(category) = param.ability().and_then(|ability| ability.get_category(&mounted.variant)) else {
                    continue;
                };
                let name = self.consumable_name(&mounted.ability, category);
                if !self.heals_self(category, species) {
                    continue;
                }
                own_consumables.push(OwnHeal {
                    squadrons: mounted.carrier.squadrons(),
                    consumable: self.healing_consumable(ship, name, category)?,
                });
            }

            for commander in self.commanders.iter().filter(|commander| commander.serves(ship)) {
                let talents = self.talents_where(commander, Some(ship.class), heals);
                if !talents.is_empty() {
                    commander_talents.push(CommanderHeals { commander: commander.name.clone(), talents });
                }
            }

            game_modes = self.mode_heals(ship.class)?;
        }

        Ok(HealingSourcesOutput {
            interpretations,
            ship: ship.map(Ship::summary),
            own_consumables,
            ally_heals: self.ally_heals(ship.map(|ship| ship.class.species()))?,
            commander_talents,
            game_modes,
        })
    }

    /// Consumables that heal an allied ship of `target`, or of any class,
    /// with the ships that carry them.
    fn ally_heals(&self, target: Option<Species>) -> Result<Vec<AllyHeal>, QueryError> {
        let mut heals: Vec<AllyHeal> = Vec::new();
        let listed = self.ships.iter().filter(|ship| {
            !ship.is_replica() && (ship.is_regular() || (ship.status() == ShipStatus::Test && !ship.copies_released))
        });
        for ship in listed {
            for mounted in self.mounted_consumables(ship) {
                let Some(param) = self.provider().game_param_by_name(&mounted.ability) else {
                    continue;
                };
                let Some(category) = param.ability().and_then(|ability| ability.get_category(&mounted.variant)) else {
                    continue;
                };
                let name = self.consumable_name(&mounted.ability, category);
                if !self.heals_ally(category, target) {
                    continue;
                }

                let consumable = self.healing_consumable(ship, name.clone(), category)?;
                let delivered_by = mounted.carrier.delivered_by();
                let heal = match heals.iter_mut().find(|heal| heal.consumable == name) {
                    Some(heal) => heal,
                    None => {
                        heals.push(AllyHeal { consumable: name, carriers: Vec::new() });
                        heals.last_mut().expect("just pushed")
                    }
                };
                match heal.carriers.iter_mut().find(|carriers| {
                    carriers.delivered_by == delivered_by && same_values(&carriers.consumable, &consumable)
                }) {
                    Some(carriers) => {
                        let label = ship.label();
                        if !carriers.ships.contains(&label) {
                            carriers.ships.push(label);
                        }
                    }
                    None => {
                        heal.carriers.push(AllyHealCarriers { ships: vec![ship.label()], delivered_by, consumable })
                    }
                }
            }
        }

        Ok(heals)
    }

    /// The consumables of the ship and of every squadron it carries, each
    /// choice once.
    fn mounted_consumables(&self, ship: &Ship) -> Vec<Mounted> {
        let mut mounted: Vec<Mounted> = Vec::new();
        let mut add = |ability: &str, variant: &str, squadron: Option<String>| {
            let found = mounted.iter_mut().find(|other| other.ability == ability && other.variant == variant);
            match (found, squadron) {
                (Some(other), Some(squadron)) => match &mut other.carrier {
                    Carrier::Squadrons(names) => {
                        if !names.contains(&squadron) {
                            names.push(squadron);
                        }
                    }
                    // A squadron that drops a consumable the ship also uses
                    // reports it as dropped.
                    Carrier::Ship => other.carrier = Carrier::Squadrons(vec![squadron]),
                },
                (Some(_), None) => {
                    // Already listed.
                }
                (None, squadron) => mounted.push(Mounted {
                    ability: ability.to_owned(),
                    variant: variant.to_owned(),
                    carrier: squadron.map_or(Carrier::Ship, |name| Carrier::Squadrons(vec![name])),
                }),
            }
        };

        for slot in ship.param.vehicle().and_then(|vehicle| vehicle.abilities()).unwrap_or_default() {
            for (ability, variant) in slot {
                add(ability, variant, None);
            }
        }

        let Some(components) = ship.param.vehicle().and_then(|vehicle| vehicle.ttx_components()) else {
            return mounted;
        };
        let mut planes: Vec<&String> = components.squadrons.values().flat_map(|upgrade| &upgrade.planes).collect();
        planes.sort();
        planes.dedup();
        for plane in planes {
            let Some(param) = self.provider().game_param_by_name(plane) else {
                continue;
            };
            let Some(aircraft) = param.aircraft() else {
                continue;
            };
            let squadron = self.aircraft_name(&param);
            for slot in aircraft.abilities() {
                for (ability, variant) in slot {
                    add(ability, variant, Some(squadron.clone()));
                }
            }
        }

        mounted
    }

    /// Whether the consumable restores HP of the ship that uses it.
    fn heals_self(&self, category: &AbilityCategory, species: Species) -> bool {
        if category.regeneration_hp_speed().is_some_and(|rate| rate > 0.0)
            || category.regeneration_hp_speed_units().is_some_and(|rate| rate > 0.0)
        {
            return true;
        }
        let buff = if category.target_team() == Some("ally") { category.buff_on_self() } else { category.buff() };

        buff.and_then(|buff| self.buff_modifiers(buff, 1, species))
            .is_some_and(|modifiers| buff_heal_rate(&modifiers, species).is_some())
    }

    /// Whether the consumable restores HP of an allied ship of `target`, or
    /// of some class.
    fn heals_ally(&self, category: &AbilityCategory, target: Option<Species>) -> bool {
        if category.target_team() != Some("ally") {
            return false;
        }
        let Some(buff) = category.buff() else {
            return false;
        };
        let classes = target
            .map_or_else(|| ShipClass::ALL.iter().map(|class| class.species()).collect(), |species| vec![species]);

        classes.into_iter().any(|species| {
            self.buff_modifiers(buff, 1, species).is_some_and(|modifiers| buff_heal_rate(&modifiers, species).is_some())
        })
    }

    /// A consumable's stats on `ship` with stock modifiers, and the buff it
    /// applies.
    fn healing_consumable(
        &self,
        ship: &Ship,
        name: String,
        category: &AbilityCategory,
    ) -> Result<ConsumableStats, QueryError> {
        let effects = self.effective_modifiers(ship, &self.stock_loadout())?;
        let mut stats = consumable_stats(name, category, effects.build.modifiers.bundle());
        stats.buff =
            self.consumable_buff(category, ship.class.species(), BuffDuration::of_consumable(stats.action_time_s));

        Ok(stats)
    }
}

/// Whether a talent restores HP: a regeneration effect or a heal modifier.
fn heals(talent: &CrewTalent) -> bool {
    talent.effects().iter().any(|effect| {
        effect.effect_type().known() == Some(KnownTalentEffect::Regen)
            || effect
                .values()
                .iter()
                .chain(effect.levels().iter().flatten())
                .any(|value| value.name().starts_with("healthRegenPercent"))
    })
}

/// Whether two carriers of one consumable share every value.
fn same_values(left: &ConsumableStats, right: &ConsumableStats) -> bool {
    left.charges == right.charges
        && left.reload_s == right.reload_s
        && left.action_time_s == right.action_time_s
        && left.preparation_s == right.preparation_s
        && left.buff == right.buff
}
