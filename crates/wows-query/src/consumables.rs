use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use serde::Serializer;
use wows_core::units::Km;
use wows_core::units::Meters;
use wows_core::units::Percent;
use wows_core::units::Seconds;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::translations::translate_consumable;
use wowsunpack::game_params::ttx::consumables::EffectiveConsumable;
use wowsunpack::game_params::ttx::consumables::effective_consumable;
use wowsunpack::game_params::ttx::model::AmmoCount;
use wowsunpack::game_params::ttx::model::Hp;
use wowsunpack::game_params::ttx::modifiers::ModifierBundle;
use wowsunpack::game_params::types::AbilityCategory;
use wowsunpack::game_params::types::ConsumableCapacity;
use wowsunpack::game_params::types::ConsumableLifecycle;
use wowsunpack::game_params::types::GameParamProvider;

use crate::buffs::BuffDuration;
use crate::buffs::ConsumableBuff;
use crate::builds::Better;
use crate::builds::BuildStat;
use crate::builds::Charges;
use crate::builds::Exclusion;
use crate::builds::Extract;
use crate::builds::Item;
use crate::builds::Outcome;
use crate::builds::Planner;
use crate::builds::StatAmount;
use crate::builds::StatPlan;
use crate::builds::StatUnit;
use crate::builds::round;
use crate::data::GameData;
use crate::error::QueryError;
use crate::loadout::AppliedSkill;
use crate::loadout::AppliedUpgrade;
use crate::loadout::LoadoutRequest;
use crate::names::Interpretations;
use crate::names::Lookup;
use crate::names::NameIndex;
use crate::names::NameKind;
use crate::names::ResolutionMethod;
use crate::ships::Ship;
use crate::ships::ShipClass;
use crate::ships::ShipSummary;
use crate::text::clean;

/// Community names of consumables, with the GameParams consumable type.
/// Initialisms of English names ("DCP", "DFAA", "MBRB") need no entry. Every
/// target is checked by the `alias_tables_resolve_to_their_targets` test.
pub(crate) const CONSUMABLE_ALIASES: &[(&str, &str)] = &[
    ("Radar", "rls"),
    ("Hydro", "sonar"),
    ("Heal", "regenCrew"),
    ("DC", "crashCrew"),
    ("Smoke", "smokeGenerator"),
    ("Speed Boost", "speedBoosters"),
    ("Spotter", "scout"),
    ("Sub Surveillance", "submarineLocator"),
];

/// Length of a Random Battle, for whether an extra charge can be used.
const MATCH: Seconds = Seconds::new(1200.0);

/// GameParams consumable type of the Main Battery Reload Booster.
pub(crate) const MAIN_BATTERY_RELOAD_BOOSTER: &str = "artilleryBoosters";

/// GameParams consumable type of Engine Boost and the other speed boosters.
pub(crate) const SPEED_BOOSTER: &str = "speedBoosters";

/// GameParams consumable type of the Repair Party.
const REPAIR_PARTY: &str = "regenCrew";

const HEAL_NOTE: &str = "Repair Party heal ceilings: per_charge is heal_percent_per_s x action time x max_hp, per_battle every charge used for its whole action time (or the whole capacity of a time-based Repair Party). The Repair Party restores only repairable damage: each damage type leaves a share of its damage that can never be healed, so a charge restores its full amount only when the ship has taken at least that much repairable damage. max_hp is that of the hull the values use: the stock hull at base and in the top-level values, the best-fit build's hull in builds.best, and it includes skills and upgrades that add HP.";

const SLOT_NOTE: &str = "Each slot carries one consumable in battle. A slot with a choice carries only the option the player picks in port, so options of one slot are never carried together.";

/// Arguments for `ship_consumables`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShipConsumablesArgs {
    /// Ship name, e.g. "Des Moines".
    pub ship: String,

    /// Only this consumable, e.g. "radar", "hydro", "Repair Party", "DCP".
    pub consumable: Option<String>,

    /// Captain skills by name or abbreviation, e.g. ["SI"], applied to the consumable stats. Without skills and upgrades, each consumable lists its base and best-fit build values.
    pub skills: Option<Vec<String>>,

    /// Upgrades by name, e.g. ["Engine Boost Modification 1"], applied to the consumable stats.
    pub upgrades: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShipConsumables {
    /// Names read as something they do not spell exactly, or as one of several ships sharing a name.
    #[serde(skip_serializing_if = "Interpretations::is_empty")]
    pub interpretations: Interpretations,

    pub ship: ShipSummary,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<AppliedSkill>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub upgrades: Vec<AppliedUpgrade>,

    /// Consumable slots in port order. A slot with several options mounts one of them.
    pub slots: Vec<ConsumableSlot>,

    /// Present when a slot offers a choice.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConsumableSlot {
    /// 1-based.
    pub slot: usize,

    /// "choose one of: A, B" when the slot offers several consumables,
    /// including those `consumable` leaves out of `options`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub choice: Option<String>,

    pub options: Vec<ConsumableStats>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConsumableStats {
    pub name: String,

    /// GameParams consumable type, e.g. "rls" (radar), "sonar" (hydro), "crashCrew".
    #[serde(rename = "type")]
    pub kind: String,

    /// Count-based consumables only. Absent for time-based consumables,
    /// which draw on capacity_s instead.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub charges: Option<Charges>,

    /// Cooldown after each use. A time-based consumable is ready again this
    /// long after an activation ends.
    pub reload_s: Seconds,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub action_time_s: Option<Seconds>,

    /// Activation delay. Absent when zero or not listed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preparation_s: Option<Seconds>,

    /// No skill or upgrade changes it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ship_detection_m: Option<Meters>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub torpedo_detection_m: Option<Meters>,

    /// Repair Party heal rate, percent of maximum HP per second.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heal_percent_per_s: Option<Percent>,

    /// Repair Party: HP restored by one charge and per battle.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heal: Option<RepairHeal>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub smoke_radius_km: Option<Km>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub smoke_lifetime_s: Option<Seconds>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub fighters: Option<f32>,

    /// Time-based consumables: seconds of use per battle, spent in
    /// activations of at least min_activation_s with reload_s between them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capacity_s: Option<Capacity>,

    /// Time-based consumables: the shortest activation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_activation_s: Option<Seconds>,

    /// Time-based consumables whose capacity refills: seconds of capacity
    /// regained per second after an activation ends.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capacity_regen_per_s: Option<f32>,

    /// Main Battery Reload Booster: multiplier of the main battery reload
    /// time while active, e.g. 0.5 halves the reload.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub main_battery_reload_factor: Option<ReloadMultiplier>,

    /// The buff it applies, for consumables used on an allied ship or that buff your own.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buff: Option<ConsumableBuff>,

    /// Base and best-fit build values, given when no skills or upgrades are.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub builds: Option<ConsumableBuilds>,
}

/// Consumable stats on the best-fit build: consumable upgrades and captain
/// skills such as Superintendent and Consumable Enhancements.
#[derive(Debug, Clone, Serialize)]
pub struct ConsumableBuilds {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action_time: Option<BuildStat<Seconds>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub charges: Option<BuildStat<Charges>>,

    /// Time-based consumables with a limited capacity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capacity: Option<BuildStat<Seconds>>,

    pub reload: BuildStat<Seconds>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub smoke_lifetime: Option<BuildStat<Seconds>>,

    /// Repair Party: HP one charge restores.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heal_per_charge: Option<BuildStat<HealAmount>>,

    /// Repair Party: HP every charge restores together, leaving out charges
    /// the battle is too short to use.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heal_per_battle: Option<BuildStat<HealAmount>>,
}

/// Seconds of use of a time-based consumable, or unlimited.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Capacity {
    Limited(Seconds),
    Unlimited,
}

impl Serialize for Capacity {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Limited(seconds) => seconds.serialize(serializer),
            Self::Unlimited => serializer.serialize_str("unlimited"),
        }
    }
}

/// Multiplier of the main battery reload time.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ReloadMultiplier(pub f32);

/// HP a Repair Party restores, and as a share of the maximum HP it is for.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct HealAmount {
    pub hp: Hp,
    pub percent_of_max_hp: Percent,
    pub max_hp: Hp,
}

impl HealAmount {
    fn new(hp: f32, max_hp: Hp) -> Self {
        Self { hp: Hp::from(hp.round()), percent_of_max_hp: Percent::new(round(hp / max_hp.value() * 100.0)), max_hp }
    }
}

impl StatAmount for HealAmount {
    fn amount(self) -> f32 {
        self.hp.value()
    }
}

impl StatUnit for HealAmount {
    const UNIT: &'static str = "HP";
}

/// HP ceilings of a Repair Party on one maximum HP.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RepairHeal {
    /// One charge used for its whole action time. Absent for a time-based
    /// Repair Party.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub per_charge: Option<HealAmount>,

    /// Every charge used for its whole action time, or the whole capacity of
    /// a time-based Repair Party. Absent when unlimited.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub per_battle: Option<HealAmount>,

    pub note: &'static str,
}

impl GameData {
    /// The consumables a ship can mount, with skills and upgrades applied or
    /// with their best-fit build values.
    pub fn ship_consumables(&self, args: &ShipConsumablesArgs) -> Result<ShipConsumables, QueryError> {
        let mut interpretations = Interpretations::default();
        let ship = self.resolve_ship(&args.ship, &mut interpretations)?;
        let wanted = match args.consumable.as_deref() {
            Some(query) => Some(self.resolve_consumable(query, &mut interpretations)?),
            None => None,
        };
        let request = LoadoutRequest {
            skills: args.skills.as_deref().unwrap_or_default(),
            upgrades: args.upgrades.as_deref().unwrap_or_default(),
            conditions_met: false,
            hp_percent: None,
            commander: None,
        };
        let loadout = self.resolve_loadout(ship, &request, &mut interpretations)?;
        let effects = self.effective_modifiers(ship, &loadout)?;
        let bundle = effects.build.modifiers.bundle();
        let planner = loadout.is_empty().then(|| Planner::new(self, ship));
        let mut card_max_hp = None;

        let mut slots = Vec::new();
        let ability_slots = ship.param.vehicle().and_then(|vehicle| vehicle.abilities()).unwrap_or_default();
        for (index, slot) in ability_slots.iter().enumerate() {
            let mut names = Vec::new();
            let mut options = Vec::new();
            for (ability, variant) in slot {
                let Some(param) = self.provider().game_param_by_name(ability) else {
                    continue;
                };
                let Some(category) = param.ability().and_then(|ability| ability.get_category(variant)) else {
                    continue;
                };
                let name = self.consumable_name(ability, category);
                names.push(name.clone());
                if wanted.as_ref().is_some_and(|types| !types.iter().any(|kind| kind == category.consumable_type_raw()))
                {
                    continue;
                }
                let mut stats = consumable_stats(name, category, bundle);
                stats.buff = self.consumable_buff(
                    category,
                    ship.class.species(),
                    BuffDuration::of_consumable(stats.action_time_s),
                );
                if category.consumable_type_raw() == REPAIR_PARTY {
                    let max_hp = match card_max_hp {
                        Some(max_hp) => max_hp,
                        None => {
                            let card = self.card(ship, &loadout)?;
                            let max_hp = card.stats.durability.as_ref().and_then(|durability| durability.health);
                            card_max_hp = Some(max_hp);
                            max_hp
                        }
                    };
                    stats.heal = max_hp.and_then(|max_hp| repair_heal(category, bundle, max_hp));
                }
                if let Some(planner) = &planner {
                    stats.builds = Some(consumable_builds(planner, &stats.name, category)?);
                }
                options.push(stats);
            }
            if !options.is_empty() {
                slots.push(ConsumableSlot { slot: index + 1, choice: slot_choice(&names), options });
            }
        }
        if slots.is_empty()
            && let Some(query) = args.consumable.as_deref()
        {
            return Err(QueryError::ConsumableNotOnShip { consumable: query.to_owned(), ship: ship.name.clone() });
        }

        let note = slots.iter().any(|slot| slot.choice.is_some()).then_some(SLOT_NOTE);

        Ok(ShipConsumables {
            interpretations,
            ship: ship.summary(),
            skills: loadout.applied_skills,
            upgrades: loadout.applied_upgrades,
            slots,
            note,
        })
    }

    /// English name of a consumable, or its GameParams type when it has none.
    pub(crate) fn consumable_name(&self, ability: &str, category: &AbilityCategory) -> String {
        translate_consumable(ability, self.provider())
            .map_or_else(|| category.consumable_type_raw().to_owned(), |name| clean(&name))
    }

    /// English name of the consumable a stat card row of `ship` is qualified
    /// by. Card rows name the first consumable of each type in slot order by
    /// its type. `None` when no consumable of the ship has that type.
    pub(crate) fn consumable_row_name(&self, ship: &Ship, qualifier: &str) -> Option<String> {
        let slots = ship.param.vehicle()?.abilities()?;

        for (ability, variant) in slots.iter().flatten() {
            let Some(param) = self.provider().game_param_by_name(ability) else {
                continue;
            };
            let Some(category) = param.ability().and_then(|ability| ability.get_category(variant)) else {
                continue;
            };
            if category.consumable_type_raw() == qualifier {
                return Some(self.consumable_name(ability, category));
            }
        }

        None
    }

    /// The consumable types a player-written consumable name refers to.
    fn resolve_consumable(
        &self,
        query: &str,
        interpretations: &mut Interpretations,
    ) -> Result<Vec<String>, QueryError> {
        match self.consumable_names.lookup(query) {
            Lookup::Found(resolved, types) => {
                interpretations.note(query, &resolved, NameKind::Consumable);
                Ok(types)
            }
            Lookup::Ambiguous(candidates) => {
                Err(QueryError::AmbiguousConsumable { query: query.to_owned(), candidates })
            }
            Lookup::NotFound(suggestions) => {
                Err(QueryError::ConsumableNotFound { query: query.to_owned(), suggestions })
            }
        }
    }
}

/// Action time, charges, capacity, reload, smoke lifetime and Repair Party
/// heal of one consumable on its best-fit build.
pub(crate) fn consumable_builds(
    planner: &Planner<'_>,
    name: &str,
    category: &AbilityCategory,
) -> Result<ConsumableBuilds, QueryError> {
    let effective = |bundle: &ModifierBundle| effective_consumable(category, bundle).0;
    let action_time = planner.plan(&consumable_plan(name, "action time", Better::Higher, &|outcome| {
        effective(&outcome.bundle).work_time.map(|time| Seconds::new(round(time.value())))
    }))?;
    let reload = planner
        .plan(&consumable_plan(name, "reload", Better::Lower, &|outcome| {
            Some(Seconds::new(round(effective(&outcome.bundle).reload_time.value())))
        }))?
        .expect("every consumable has a reload");
    let smoke_lifetime = planner.plan(&consumable_plan(name, "smoke lifetime", Better::Higher, &|outcome| {
        effective(&outcome.bundle).smoke_lifetime.map(|time| Seconds::new(round(time.value())))
    }))?;
    let capacity = planner.plan(&consumable_plan(name, "capacity", Better::Higher, &|outcome| {
        match effective(&outcome.bundle).max_capacity? {
            ConsumableCapacity::Seconds(seconds) => Some(Seconds::new(round(seconds))),
            // Modifiers never limit an unlimited pool.
            ConsumableCapacity::Unlimited => None,
        }
    }))?;

    let exclude_charges = |planner: &Planner<'_>, singles: &[(Item, Charges)]| {
        unusable_charges(planner, category, singles.iter().map(|(item, _)| *item))
    };
    let extract = |outcome: &Outcome| effective(&outcome.bundle).charges.map(charges);
    let charge_plan =
        StatPlan { exclude: Some(&exclude_charges), ..consumable_plan(name, "charges", Better::Higher, &extract) };
    let charges = planner.plan(&charge_plan)?;

    let (heal_per_charge, heal_per_battle) = if category.consumable_type_raw() == REPAIR_PARTY {
        let heal = |outcome: &Outcome| {
            let max_hp = outcome.stats.durability.as_ref()?.health?;
            repair_heal(category, &outcome.bundle, max_hp)
        };
        let per_charge = |outcome: &Outcome| heal(outcome).and_then(|heal| heal.per_charge);
        let per_battle = |outcome: &Outcome| heal(outcome).and_then(|heal| heal.per_battle);
        let exclude_heal = |planner: &Planner<'_>, singles: &[(Item, HealAmount)]| {
            unusable_charges(planner, category, singles.iter().map(|(item, _)| *item))
        };
        // The hull module sets the maximum HP the heal is a share of.
        let per_charge_plan =
            StatPlan { modules: true, ..consumable_plan(name, "heal per charge", Better::Higher, &per_charge) };
        let per_battle_plan = StatPlan {
            modules: true,
            exclude: Some(&exclude_heal),
            ..consumable_plan(name, "heal per battle", Better::Higher, &per_battle)
        };

        (planner.plan(&per_charge_plan)?, planner.plan(&per_battle_plan)?)
    } else {
        (None, None)
    };

    Ok(ConsumableBuilds { action_time, charges, capacity, reload, smoke_lifetime, heal_per_charge, heal_per_battle })
}

pub(crate) fn consumable_plan<'f, V>(
    name: &str,
    stat: &str,
    better: Better,
    extract: Extract<'f, V>,
) -> StatPlan<'f, V> {
    StatPlan::new(format!("{name} {stat}"), better, extract).for_consumable()
}

/// Extra charges the best-fit build leaves out because using every charge
/// back to back would outlast a match (shipbuilds `unburnable_consumable_charge`).
/// A cruiser's or destroyer's Repair Party is used as damage comes in, not
/// back to back, so its charges always count. `items` are the items that
/// change the stat, each judged on a build with it alone.
fn unusable_charges(
    planner: &Planner<'_>,
    category: &AbilityCategory,
    items: impl Iterator<Item = Item>,
) -> Result<Vec<Exclusion>, QueryError> {
    let reactive = category.consumable_type_raw() == REPAIR_PARTY
        && matches!(planner.ship().class, ShipClass::Cruiser | ShipClass::Destroyer);
    if reactive {
        return Ok(Vec::new());
    }

    let mut exclusions = Vec::new();
    for item in items {
        let outcome = planner.with_items(&[item])?;
        let EffectiveConsumable { work_time, reload_time, charges, .. } =
            effective_consumable(category, &outcome.bundle).0;
        let Some(AmmoCount::Finite(count)) = charges else {
            continue;
        };
        let cycle = work_time.map_or(0.0, |time| time.value()) + reload_time.value();
        // Charge counts are far below f32's exact integer range.
        #[allow(clippy::cast_precision_loss)]
        let needed = count as f32 * cycle;
        if needed > MATCH.value() {
            exclusions.push(Exclusion {
                item,
                note: format!(
                    "{count} charges of {} s action and reload take {} s, longer than a {} s match: the extra charge cannot be used",
                    round(cycle),
                    round(needed),
                    MATCH.value()
                ),
            });
        }
    }
    Ok(exclusions)
}

fn charges(count: AmmoCount) -> Charges {
    match count {
        AmmoCount::Finite(count) => Charges::Limited(count),
        AmmoCount::Infinite => Charges::Unlimited,
    }
}

/// "choose one of: A, B" for a consumable slot with several options, `None`
/// for a slot with one.
pub(crate) fn slot_choice(names: &[String]) -> Option<String> {
    (names.len() > 1).then(|| format!("choose one of: {}", names.join(", ")))
}

pub(crate) fn consumable_stats(name: String, category: &AbilityCategory, bundle: &ModifierBundle) -> ConsumableStats {
    let (stats, _) = effective_consumable(category, bundle);
    let kind = category.consumable_type_raw().to_owned();

    ConsumableStats {
        name,
        kind,
        charges: stats.charges.map(charges),
        reload_s: Seconds::new(round(stats.reload_time.value())),
        action_time_s: stats.work_time.map(|time| Seconds::new(round(time.value()))),
        preparation_s: stats
            .preparation_time
            .map(|time| time.value())
            .filter(|time| *time > 0.0)
            .map(|time| Seconds::new(round(time))),
        // BigWorld unit conversion leaves 9999.9 for a 10 km radar.
        ship_detection_m: stats.detection_radius.map(|radius| Meters::new(radius.value().round())),
        torpedo_detection_m: category.torpedo_detection_radius().map(|radius| Meters::new(radius.value().round())),
        heal_percent_per_s: stats.regeneration_hp_speed.map(|rate| Percent::new(round(rate * 100.0))),
        smoke_radius_km: stats.smoke_radius.map(|radius| Km::new(round(radius.value()))),
        smoke_lifetime_s: stats.smoke_lifetime.map(|time| Seconds::new(round(time.value()))),
        fighters: stats.fighters_count,
        capacity_s: stats.max_capacity.map(|capacity| match capacity {
            ConsumableCapacity::Seconds(seconds) => Capacity::Limited(Seconds::new(round(seconds))),
            ConsumableCapacity::Unlimited => Capacity::Unlimited,
        }),
        min_activation_s: stats.min_work_time.map(|time| Seconds::new(round(time.value()))),
        capacity_regen_per_s: stats.capacity_regen.filter(|regen| *regen > 0.0).map(round),
        main_battery_reload_factor: main_battery_reload_factor(category),
        heal: None,
        buff: None,
        builds: None,
    }
}

/// The main battery reload time multiplier of a Main Battery Reload Booster,
/// its `logic.boostCoeff` (`CommonConsumables/Logic/Ship.MainBatteryBooster`
/// sets it as the reload coefficient; no modifier changes it). `None` for
/// other consumables.
pub(crate) fn main_battery_reload_factor(category: &AbilityCategory) -> Option<ReloadMultiplier> {
    if category.consumable_type_raw() != MAIN_BATTERY_RELOAD_BOOSTER {
        return None;
    }

    category.effect_fields().get("boostCoeff").map(|coeff| ReloadMultiplier(*coeff))
}

/// Repair Party heal ceilings under `bundle` on `max_hp`. `None` for a
/// consumable without a heal rate.
pub(crate) fn repair_heal(category: &AbilityCategory, bundle: &ModifierBundle, max_hp: Hp) -> Option<RepairHeal> {
    let (stats, _) = effective_consumable(category, bundle);
    let fraction = stats.regeneration_hp_speed?;
    // The flat rate takes the same modifier as the fraction
    // (ConsumableUtils.py:139-140); a category without one heals only the
    // fraction.
    let flat = category.regeneration_hp_speed_units().map_or(0.0, |units| units * bundle.coef("regenerationHPSpeed"));
    let per_second = fraction * max_hp.value() + flat;

    let (per_charge, per_battle) = match category.lifecycle() {
        ConsumableLifecycle::CountBased { .. } => {
            let per_charge = stats.work_time.map(|time| per_second * time.value());
            let per_battle = match (per_charge, stats.charges) {
                // Charge counts are far below f32's exact integer range.
                #[allow(clippy::cast_precision_loss)]
                (Some(per_charge), Some(AmmoCount::Finite(count))) => Some(per_charge * count as f32),
                _ => None,
            };
            (per_charge, per_battle)
        }
        ConsumableLifecycle::TimeBased { .. } => {
            let per_battle = match stats.max_capacity {
                Some(ConsumableCapacity::Seconds(seconds)) => Some(per_second * seconds),
                Some(ConsumableCapacity::Unlimited) | None => None,
            };
            (None, per_battle)
        }
    };

    Some(RepairHeal {
        per_charge: per_charge.map(|hp| HealAmount::new(hp, max_hp)),
        per_battle: per_battle.map(|hp| HealAmount::new(hp, max_hp)),
        note: HEAL_NOTE,
    })
}

/// English consumable names, GameParams names and types, and
/// [`CONSUMABLE_ALIASES`], each resolving to the consumable types they name.
pub(crate) fn consumable_names(provider: &GameMetadataProvider) -> NameIndex<String> {
    let mut names = NameIndex::default();
    for param in provider.params() {
        let Some(ability) = param.ability() else {
            continue;
        };
        let Some(name) = translate_consumable(param.name(), provider).map(|name| clean(&name)) else {
            continue;
        };
        let mut types: Vec<&str> = ability.categories().values().map(AbilityCategory::consumable_type_raw).collect();
        types.sort_unstable();
        types.dedup();
        for kind in types {
            names.add_name(&name, kind.to_owned(), false);
            names.add_id(param.name(), &name, kind.to_owned());
            names.add_id(kind, &name, kind.to_owned());
        }
    }
    for (alias, kind) in CONSUMABLE_ALIASES {
        names.add_alias(alias, kind, ResolutionMethod::ExactAlias);
    }
    names
}
