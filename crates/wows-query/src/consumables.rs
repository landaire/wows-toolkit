use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use wows_core::units::Km;
use wows_core::units::Meters;
use wows_core::units::Percent;
use wows_core::units::Seconds;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::translations::translate_consumable;
use wowsunpack::game_params::ttx::consumables::EffectiveConsumable;
use wowsunpack::game_params::ttx::consumables::effective_consumable;
use wowsunpack::game_params::ttx::model::AmmoCount;
use wowsunpack::game_params::ttx::modifiers::ModifierBundle;
use wowsunpack::game_params::types::AbilityCategory;
use wowsunpack::game_params::types::GameParamProvider;

use crate::buffs::ConsumableBuff;
use crate::builds::Better;
use crate::builds::BuildStat;
use crate::builds::Charges;
use crate::builds::Exclusion;
use crate::builds::Extract;
use crate::builds::Item;
use crate::builds::Outcome;
use crate::builds::Planner;
use crate::builds::StatPlan;
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

    pub charges: Charges,
    pub reload_s: Seconds,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub action_time_s: Option<Seconds>,

    /// Activation delay. Absent when zero.
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

    #[serde(skip_serializing_if = "Option::is_none")]
    pub smoke_radius_km: Option<Km>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub smoke_lifetime_s: Option<Seconds>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub fighters: Option<f32>,

    /// Usage pool of time-based consumables.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capacity_s: Option<Seconds>,

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

    pub reload: BuildStat<Seconds>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub smoke_lifetime: Option<BuildStat<Seconds>>,
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
                stats.buff =
                    self.consumable_buff(category, ship.class.species(), stats.action_time_s.map(|time| time.value()));
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

/// Action time, charges, reload and smoke lifetime of one consumable on its
/// best-fit build.
fn consumable_builds(
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

    let exclude = |planner: &Planner<'_>, singles: &[(Item, Charges)]| unusable_charges(planner, category, singles);
    let extract = |outcome: &Outcome| Some(charges(effective(&outcome.bundle).charges));
    let charge_plan =
        StatPlan { exclude: Some(&exclude), ..consumable_plan(name, "charges", Better::Higher, &extract) };
    let charges = planner.plan(&charge_plan)?;

    Ok(ConsumableBuilds { action_time, charges, reload, smoke_lifetime })
}

fn consumable_plan<'f, V>(name: &str, stat: &str, better: Better, extract: Extract<'f, V>) -> StatPlan<'f, V> {
    StatPlan::new(format!("{name} {stat}"), better, extract).for_consumable()
}

/// Extra charges the best-fit build leaves out because using every charge
/// back to back would outlast a match (shipbuilds `unburnable_consumable_charge`).
/// A cruiser's or destroyer's Repair Party is used as damage comes in, not
/// back to back, so its charges always count.
fn unusable_charges(
    planner: &Planner<'_>,
    category: &AbilityCategory,
    singles: &[(Item, Charges)],
) -> Result<Vec<Exclusion>, QueryError> {
    let reactive = category.consumable_type_raw() == "regenCrew"
        && matches!(planner.ship().class, ShipClass::Cruiser | ShipClass::Destroyer);
    if reactive {
        return Ok(Vec::new());
    }

    let mut exclusions = Vec::new();
    for (item, total) in singles {
        let Charges::Limited(count) = *total else {
            continue;
        };
        let outcome = planner.with_items(&[*item])?;
        let EffectiveConsumable { work_time, reload_time, .. } = effective_consumable(category, &outcome.bundle).0;
        let cycle = work_time.map_or(0.0, |time| time.value()) + reload_time.value();
        // Charge counts are far below f32's exact integer range.
        #[allow(clippy::cast_precision_loss)]
        let needed = count as f32 * cycle;
        if needed > MATCH.value() {
            exclusions.push(Exclusion {
                item: *item,
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
        charges: charges(stats.charges),
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
        buff: None,
        builds: None,
    }
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
