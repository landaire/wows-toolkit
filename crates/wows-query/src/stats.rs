use std::collections::HashMap;

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use wows_core::units::Km;
use wows_core::units::Percent;
use wows_core::units::Seconds;
use wowsunpack::game_params::ttx::AttributionLine;
use wowsunpack::game_params::ttx::ContributorLine;
use wowsunpack::game_params::ttx::ShipUpgradeSelection;
use wowsunpack::game_params::ttx::diff_stat_rows;
use wowsunpack::game_params::ttx::labels::TtxStat;
use wowsunpack::game_params::ttx::model::ShipStats;
use wowsunpack::game_params::ttx::provenance::ShipStatsProvenance;
use wowsunpack::game_params::ttx::render_attributions;
use wowsunpack::game_params::ttx::render_stat_rows;

use crate::builds::Better;
use crate::builds::BuildStat;
use crate::builds::Planner;
use crate::builds::StatPlan;
use crate::builds::round;
use crate::data::GameData;
use crate::error::QueryError;
use crate::loadout::AppliedSkill;
use crate::loadout::AppliedUpgrade;
use crate::loadout::LoadoutRequest;
use crate::loadout::ResolvedLoadout;
use crate::names::Interpretations;
use crate::ships::Ship;
use crate::ships::ShipSummary;
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

    /// Include the full stat card. Defaults to true without skills or upgrades and false with them, when only the changes are listed.
    pub full_card: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShipStatsOutput {
    /// Names read as something they do not spell exactly.
    #[serde(skip_serializing_if = "Interpretations::is_empty")]
    pub interpretations: Interpretations,

    pub ship: ShipSummary,

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

/// A computed stat card and its provenance.
pub(crate) struct Card {
    pub(crate) stats: ShipStats,
    pub(crate) provenance: ShipStatsProvenance,
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
        let loadout = self.resolve_loadout(ship, &request, &mut interpretations)?;
        let modified = !loadout.is_empty() || args.hp_percent.is_some();

        let card = self.card(ship, &loadout)?;
        let rows = card.stats.rows();

        let changes = if modified {
            let baseline = self.card(ship, &self.stock_loadout())?;
            let attributions = render_attributions(&card.provenance, &rows, self.provider(), self.provider());
            changes(&baseline.stats, &card.stats, &attributions, &loadout, self, |_| true)
        } else {
            Vec::new()
        };

        let stats = if args.full_card.unwrap_or(!modified) { stat_lines(&card.stats, self) } else { Vec::new() };

        let build_stats = if loadout.is_empty() { Some(self.weapon_builds(ship)?) } else { None };

        Ok(ShipStatsOutput {
            interpretations,
            ship: ship.summary(),
            build_stats,
            skill_points: (!loadout.applied_skills.is_empty()).then(|| loadout.skill_points()),
            skills: loadout.applied_skills,
            upgrades: loadout.applied_upgrades,
            active_conditions: card.active_conditions,
            changes,
            stats,
        })
    }

    fn weapon_builds(&self, ship: &Ship) -> Result<WeaponBuilds, QueryError> {
        let planner = Planner::new(self, ship);
        let seconds = |value: Option<wowsunpack::game_params::ttx::model::Seconds>| {
            value.map(|value| Seconds::new(round(value.value())))
        };
        let km = |value: Option<wowsunpack::game_params::types::Km>| value.map(|value| Km::new(round(value.value())));

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
        let (stats, provenance) =
            effects.modifiers.stats_explained(&ship.param, selection, ship.tier.0, self.provider());

        Ok(Card { stats, provenance, active_conditions: effects.active_conditions })
    }
}

pub(crate) fn stat_label(label: &str, qualifier: Option<&str>) -> String {
    match qualifier {
        Some(qualifier) => format!("{} ({qualifier})", clean(label)),
        None => clean(label),
    }
}

fn stat_lines(stats: &ShipStats, game: &GameData) -> Vec<String> {
    render_stat_rows(&stats.rows(), game.provider())
        .into_iter()
        .map(|line| format!("{}: {}", stat_label(&line.label, line.qualifier.as_deref()), line.value))
        .collect()
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
                .map(|line| causes(line, loadout))
                .unwrap_or_default();

            StatChange {
                stat: stat_label(&delta.label, delta.qualifier.as_deref()),
                from: delta.from,
                to: delta.to,
                because,
            }
        })
        .collect()
}

/// The loadout inputs behind one stat, direct ones first, then those that act
/// through an upstream stat.
fn causes(line: &AttributionLine, loadout: &ResolvedLoadout) -> Vec<String> {
    let mut causes: Vec<String> = line
        .contributors
        .iter()
        .filter_map(|contributor| loadout_label(contributor, loadout).map(|name| describe(&name, contributor)))
        .collect();

    for inherited in &line.inherited {
        let Some(name) = loadout_label(&inherited.line, loadout) else {
            continue;
        };
        let via = stat_label(&inherited.via_stat, inherited.via_qualifier.as_deref());
        let cause = format!("{} via {via}", describe(&name, &inherited.line));
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

fn describe(name: &str, contributor: &ContributorLine) -> String {
    format!("{name} {} ({})", contributor.effect, contributor.delta)
}
