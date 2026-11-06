use std::collections::HashMap;

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use wows_core::units::Km;
use wows_core::units::Percent;
use wows_core::units::Seconds;
use wows_ship_stats::Derivation;
use wows_ship_stats::DerivedRow;
use wows_ship_stats::DerivedStat;
use wows_ship_stats::StockRanges;
use wowsunpack::game_params::ttx::AttributionLine;
use wowsunpack::game_params::ttx::ContributorLine;
use wowsunpack::game_params::ttx::HEALTH_SCALED_RELOAD_STEP;
use wowsunpack::game_params::ttx::ShipUpgradeSelection;
use wowsunpack::game_params::ttx::diff_stat_rows;
use wowsunpack::game_params::ttx::labels::TtxStat;
use wowsunpack::game_params::ttx::model::ShipStats;
use wowsunpack::game_params::ttx::provenance::ShipStatsProvenance;
use wowsunpack::game_params::ttx::render_attributions;
use wowsunpack::game_params::ttx::render_stat_rows;
use wowsunpack::game_params::ttx::special_abilities::BurstShots;

use crate::builds::Better;
use crate::builds::BuildStat;
use crate::builds::Outcome;
use crate::builds::Planner;
use crate::builds::StatPlan;
use crate::builds::round;
use crate::data::GameData;
use crate::error::QueryError;
use crate::fires::FireRates;
use crate::loadout::AppliedSkill;
use crate::loadout::AppliedUpgrade;
use crate::loadout::LoadoutRequest;
use crate::loadout::ResolvedLoadout;
use crate::loadout::build_failed;
use crate::names::Interpretations;
use crate::rates::Rates;
use crate::ships::Ship;
use crate::ships::ShipSummary;
use crate::special_abilities::ActiveAbility;
use crate::text::clean;

/// Arguments for `ship_stats`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShipStatsArgs {
    /// Ship name, e.g. "Des Moines".
    pub ship: String,

    /// Captain skills by name or acronym, e.g. ["IFHE", "Concealment Expert"]. Resolved in the ship's class tree.
    pub skills: Option<Vec<String>>,

    /// Upgrades by name, e.g. ["Main Battery Modification 2"]. One per slot.
    pub upgrades: Option<Vec<String>>,

    /// Treat conditional skill triggers as met, e.g. an enemy in secondary range for Close Quarters Combat. Defaults to false.
    pub conditions_met: Option<bool>,

    /// Remaining health in percent for health-scaled effects such as Adrenaline Rush. Defaults to full health.
    pub hp_percent: Option<Percent>,

    /// Switch on the ship's special ability (its F key): Combat Instructions apply their effects; an alternative fire mode such as Burst Fire switches the main battery to it, with its reload, shells and effects. Defaults to false.
    pub special_ability_active: Option<bool>,

    /// Include the full stat card. Defaults to true without skills or upgrades and false with them, when only the changes are listed.
    pub full_card: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShipStatsOutput {
    /// Names read as something they do not spell exactly, or as one of several ships sharing a name.
    #[serde(skip_serializing_if = "Interpretations::is_empty")]
    pub interpretations: Interpretations,

    pub ship: ShipSummary,

    /// Abilities switched on by special_ability_active.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub special_ability: Vec<String>,

    /// Without skills and upgrades: weapon stats on the best-fit build.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_stats: Option<WeaponBuilds>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<AppliedSkill>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_points: Option<u32>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub upgrades: Vec<AppliedUpgrade>,

    /// Conditional skills counted as active.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub active_conditions: Vec<String>,

    /// Stats that differ from the stock card, with what changed them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub changes: Vec<StatChange>,

    /// "Label (qualifier): value" lines of the port stat card, stock modules.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stats: Vec<String>,
}

/// Weapon stats on the best-fit build.
#[derive(Debug, Clone, Serialize)]
pub struct WeaponBuilds {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub main_battery_reload: Option<BuildStat<Seconds>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub main_battery_range: Option<BuildStat<Km>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub secondary_range: Option<BuildStat<Km>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub torpedo_reload: Option<BuildStat<Seconds>>,

    /// With Burst Fire switched on: the salvos of one burst and its timing.
    /// main_battery_reload is then the reload after a burst.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub burst_fire: Option<BurstFireBuilds>,

    #[serde(flatten)]
    pub rates: Rates,

    /// Theoretical fire starts per minute of every armament.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fires_per_minute: Option<FireRates>,
}

/// Burst timing on the best-fit build. The client scales the interval and
/// the reload by the same reload modifiers.
#[derive(Debug, Clone, Serialize)]
pub struct BurstFireBuilds {
    pub shots_per_burst: BurstShots,

    /// Reload between the salvos of one burst.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shot_interval: Option<BuildStat<Seconds>>,

    /// Reload after the last salvo of a burst.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reload_after_burst: Option<BuildStat<Seconds>>,

    /// One burst and the reload after it: (shots - 1) x interval + reload.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cycle: Option<BuildStat<Seconds>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StatChange {
    pub stat: String,

    /// Absent when the stat only exists with the loadout.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,

    /// Absent when the loadout removes the stat.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,

    /// Each chosen skill or upgrade that moved the value, with its effect and
    /// the amount, e.g. "Inertia Fuse for HE Shells x1.25 (+9)".
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub because: Vec<String>,
}

/// A computed stat card, its provenance and the stats derived from it.
pub(crate) struct Card {
    pub(crate) stats: ShipStats,
    pub(crate) provenance: ShipStatsProvenance,
    pub(crate) derived: Vec<DerivedRow>,
    pub(crate) active_conditions: Vec<String>,
}

impl GameData {
    /// The port stat card of one ship, optionally with captain skills and
    /// upgrades applied and the changes they make explained.
    pub fn ship_stats(&self, args: &ShipStatsArgs) -> Result<ShipStatsOutput, QueryError> {
        let mut interpretations = Interpretations::default();
        let ship = self.resolve_ship(&args.ship, &mut interpretations)?;
        let request = LoadoutRequest {
            skills: args.skills.as_deref().unwrap_or_default(),
            upgrades: args.upgrades.as_deref().unwrap_or_default(),
            conditions_met: args.conditions_met.unwrap_or(false),
            hp_percent: args.hp_percent,
        };
        let mut loadout = self.resolve_loadout(ship, &request, &mut interpretations)?;

        let ability =
            if args.special_ability_active.unwrap_or(false) { Some(self.active_ability(ship)?) } else { None };
        let stock_ship = ship;
        let ship = ability.as_ref().and_then(|ability| ability.switched.as_ref()).unwrap_or(ship);
        if let Some(ability) = &ability {
            loadout.activate(&ability.modifiers);
        }
        let modified = !loadout.is_empty() || args.hp_percent.is_some() || ability.is_some();

        let mut card = self.card(ship, &loadout)?;
        if let Some(burst) = ability.as_ref().and_then(|ability| ability.burst)
            && let Some(reload) = card.stats.artillery.as_ref().and_then(|artillery| artillery.reload_time)
        {
            burst.average_damage_rates(&mut card.derived, Seconds::new(reload.value()));
        }
        let rows = card.stats.rows();

        // The baseline is the stock card in the standard fire mode, so an
        // ability's changes show against it.
        let changes = if modified {
            let baseline = self.card(stock_ship, &self.stock_loadout())?;
            let attributions = render_attributions(&card.provenance, &rows, self.provider(), self.provider());
            let mut changes = changes(&baseline.stats, &card.stats, &attributions, &loadout, self, |_| true);
            changes.extend(derived_changes(&baseline.derived, &card.derived, &attributions, &loadout, self));
            changes
        } else {
            Vec::new()
        };

        let stats = if args.full_card.unwrap_or(!modified) { stat_lines(&card, self) } else { Vec::new() };

        let build_stats = if loadout.is_empty() { Some(self.weapon_builds(ship, ability.as_ref())?) } else { None };

        Ok(ShipStatsOutput {
            interpretations,
            ship: ship.summary(),
            special_ability: ability.map(|ability| ability.names).unwrap_or_default(),
            build_stats,
            skill_points: (!loadout.applied_skills.is_empty()).then(|| loadout.skill_points()),
            skills: loadout.applied_skills,
            upgrades: loadout.applied_upgrades,
            active_conditions: card.active_conditions,
            changes,
            stats,
        })
    }

    fn weapon_builds(&self, ship: &Ship, ability: Option<&ActiveAbility>) -> Result<WeaponBuilds, QueryError> {
        let planner = match ability {
            Some(ability) => Planner::new(self, ship).with_ability(ability.modifiers.clone(), ability.burst),
            None => Planner::new(self, ship),
        };
        let seconds = |value: Option<wowsunpack::game_params::ttx::model::Seconds>| {
            value.map(|value| Seconds::new(round(value.value())))
        };
        let km = |value: Option<wowsunpack::game_params::types::Km>| value.map(|value| Km::new(round(value.value())));

        let burst_fire = match planner.burst() {
            Some(burst) => {
                let reload = |outcome: &Outcome| {
                    outcome.stats.artillery.as_ref()?.reload_time.map(|reload| Seconds::new(reload.value()))
                };
                let rounded = |value: Seconds| Seconds::new(round(value.value()));

                Some(BurstFireBuilds {
                    shots_per_burst: burst.shots,
                    shot_interval: planner.plan(&StatPlan::new(
                        "burst shot interval".to_owned(),
                        Better::Lower,
                        &|outcome| reload(outcome).map(|reload| rounded(burst.interval(reload))),
                    ))?,
                    reload_after_burst: planner.plan(&StatPlan::new(
                        "reload after a burst".to_owned(),
                        Better::Lower,
                        &|outcome| reload(outcome).map(rounded),
                    ))?,
                    cycle: planner.plan(&StatPlan::new("burst cycle".to_owned(), Better::Lower, &|outcome| {
                        reload(outcome).map(|reload| rounded(burst.cycle(reload)))
                    }))?,
                })
            }
            None => None,
        };

        Ok(WeaponBuilds {
            main_battery_reload: planner.plan(&StatPlan::new(
                "main battery reload".to_owned(),
                Better::Lower,
                &|outcome| seconds(outcome.stats.artillery.as_ref()?.reload_time),
            ))?,
            main_battery_range: planner.plan(&StatPlan::new(
                "main battery range".to_owned(),
                Better::Higher,
                &|outcome| km(outcome.stats.artillery.as_ref()?.range),
            ))?,
            secondary_range: planner.plan(&StatPlan::new(
                "secondary battery range".to_owned(),
                Better::Higher,
                &|outcome| km(Some(outcome.stats.secondaries.as_ref()?.range)),
            ))?,
            torpedo_reload: planner.plan(&StatPlan::new("torpedo reload".to_owned(), Better::Lower, &|outcome| {
                seconds(outcome.stats.torpedoes.as_ref()?.reload_time)
            }))?,
            burst_fire,
            fires_per_minute: self.fire_rates(&planner)?,
            rates: self.rates(&planner)?,
        })
    }

    pub(crate) fn card(&self, ship: &Ship, loadout: &ResolvedLoadout) -> Result<Card, QueryError> {
        self.card_with(ship, loadout, &ShipUpgradeSelection::stock(&ship.param))
    }

    pub(crate) fn card_with(
        &self,
        ship: &Ship,
        loadout: &ResolvedLoadout,
        selection: &ShipUpgradeSelection,
    ) -> Result<Card, QueryError> {
        let effects = self.effective_modifiers(ship, loadout)?;
        let card = effects.build.card(&ship.param, selection, self.provider()).map_err(build_failed(ship))?;
        let derived = Derivation {
            ship: &ship.param,
            selection,
            stats: &card.stats,
            modifiers: effects.build.modifiers.bundle(),
            stock_ranges: self.stock_ranges(ship)?,
            provider: self.provider(),
        }
        .rows();

        Ok(Card {
            stats: card.stats,
            provenance: card.provenance,
            derived,
            active_conditions: effects.active_conditions,
        })
    }

    /// Firing ranges of the stock card, where the derived dispersions are
    /// measured.
    pub(crate) fn stock_ranges(&self, ship: &Ship) -> Result<StockRanges, QueryError> {
        let effects = self.effective_modifiers(ship, &self.stock_loadout())?;
        let stock = effects
            .build
            .stats(&ship.param, &ShipUpgradeSelection::stock(&ship.param), self.provider())
            .map_err(build_failed(ship))?;

        Ok(StockRanges::from_stock_card(&stock))
    }
}

/// "Label (qualifier)", with a projectile qualifier by its English name.
fn stat_label(label: &str, qualifier: Option<&str>, game: &GameData) -> String {
    match qualifier {
        Some(qualifier) => format!("{} ({})", clean(label), game.qualifier_name(qualifier)),
        None => clean(label),
    }
}

fn stat_lines(card: &Card, game: &GameData) -> Vec<String> {
    let mut lines: Vec<String> = render_stat_rows(&card.stats.rows(), game.provider())
        .into_iter()
        .map(|line| format!("{}: {}", stat_label(&line.label, line.qualifier.as_deref(), game), line.value))
        .collect();
    lines.extend(
        card.derived
            .iter()
            .map(|row| format!("{}: {}", stat_label(row.stat.label(), row.qualifier.as_deref(), game), row.value)),
    );

    lines
}

/// Stats `keep` accepts that differ between the cards, with the loadout
/// inputs that moved them.
pub(crate) fn changes(
    baseline: &ShipStats,
    candidate: &ShipStats,
    attributions: &[AttributionLine],
    loadout: &ResolvedLoadout,
    game: &GameData,
    keep: impl Fn(TtxStat) -> bool,
) -> Vec<StatChange> {
    let by_key: HashMap<(TtxStat, Option<&str>), &AttributionLine> =
        attributions.iter().map(|line| ((line.stat, line.qualifier.as_deref()), line)).collect();

    diff_stat_rows(&baseline.rows(), &candidate.rows(), game.provider())
        .into_iter()
        .filter(|delta| keep(delta.stat))
        .map(|delta| {
            let because = by_key
                .get(&(delta.stat, delta.qualifier.as_deref()))
                .map(|line| causes(line, loadout, game))
                .unwrap_or_default();

            StatChange {
                stat: stat_label(&delta.label, delta.qualifier.as_deref(), game),
                from: delta.from,
                to: delta.to,
                because,
            }
        })
        .collect()
}

/// Derived stats that differ between the cards, with the loadout inputs that
/// moved the card stats they are computed from.
fn derived_changes(
    baseline: &[DerivedRow],
    candidate: &[DerivedRow],
    attributions: &[AttributionLine],
    loadout: &ResolvedLoadout,
    game: &GameData,
) -> Vec<StatChange> {
    let mut keys: Vec<(DerivedStat, Option<&str>)> = Vec::new();
    for row in baseline.iter().chain(candidate) {
        let key = (row.stat, row.qualifier.as_deref());
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    let value = |rows: &[DerivedRow], (stat, qualifier): (DerivedStat, Option<&str>)| {
        rows.iter().find(|row| row.stat == stat && row.qualifier.as_deref() == qualifier).map(|row| row.value)
    };

    keys.into_iter()
        .filter_map(|key| {
            let (from, to) = (value(baseline, key), value(candidate, key));
            let (from, to) = (from.map(|value| value.to_string()), to.map(|value| value.to_string()));
            if from == to {
                return None;
            }

            let (stat, qualifier) = key;
            let mut because: Vec<String> = Vec::new();
            // A whole-ship stat, such as total fire starts, draws on every
            // ammo type and gun group it adds up.
            for line in attributions.iter().filter(|line| {
                stat.sources().contains(&line.stat)
                    && (line.qualifier.is_none() || qualifier.is_none() || line.qualifier.as_deref() == qualifier)
            }) {
                for cause in causes(line, loadout, game) {
                    if !because.contains(&cause) {
                        because.push(cause);
                    }
                }
            }

            Some(StatChange { stat: stat_label(stat.label(), qualifier, game), from, to, because })
        })
        .collect()
}

/// The loadout inputs behind one stat, direct ones first, then those that act
/// through an upstream stat.
fn causes(line: &AttributionLine, loadout: &ResolvedLoadout, game: &GameData) -> Vec<String> {
    let mut causes: Vec<String> = line
        .contributors
        .iter()
        .filter_map(|contributor| loadout_label(contributor, loadout).map(|name| describe(&name, contributor, loadout)))
        .collect();

    for inherited in &line.inherited {
        let Some(name) = loadout_label(&inherited.line, loadout) else {
            continue;
        };
        let via = stat_label(&inherited.via_stat, inherited.via_qualifier.as_deref(), game);
        let cause = format!("{} via {via}", describe(&name, &inherited.line, loadout));
        if !causes.contains(&cause) {
            causes.push(cause);
        }
    }

    causes
}

/// English name of a contributor that belongs to the loadout. The renderer
/// labels skills by internal name and upgrades by GameParams name.
fn loadout_label(contributor: &ContributorLine, loadout: &ResolvedLoadout) -> Option<String> {
    if let Some(name) = loadout.labels.get(&contributor.label) {
        return Some(name.clone());
    }

    loadout.labels.values().any(|name| *name == clean(&contributor.label)).then(|| clean(&contributor.label))
}

/// "Inertia Fuse for HE Shells x1.25 (+9)", or for a health-scaled reload
/// "Adrenaline Rush at 1% HP: -19.8% reload (-0.84)".
fn describe(name: &str, contributor: &ContributorLine, loadout: &ResolvedLoadout) -> String {
    if let Some(hp) = loadout.hp_percent
        && contributor.modifier_name == HEALTH_SCALED_RELOAD_STEP
    {
        let change = round((contributor.operand - 1.0) * 100.0);
        return format!("{name} at {}% HP: {change:+}% reload ({})", round(hp.value()), contributor.delta);
    }

    format!("{name} {} ({})", contributor.effect, contributor.delta)
}
