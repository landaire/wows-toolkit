//! Fire starts per minute of every armament on the best-fit build, against
//! one target at its best bearing.

use std::rc::Rc;

use serde::Serialize;
use wows_core::units::FiresPerMinute;
use wows_core::units::Percent;
use wows_core::units::ProjectilesPerMinute;
use wows_ship_stats::Bearing;
use wows_ship_stats::FireArmament;
use wows_ship_stats::FireInputs;
use wows_ship_stats::FireSource;
use wows_ship_stats::MountCoverage;
use wows_ship_stats::PerBearing;
use wows_ship_stats::bearing::BearingRow;
use wows_ship_stats::secondary_fires_by_bearing;
use wows_ship_stats::total_fires_all_mounts;
use wows_ship_stats::total_fires_by_bearing;
use wowsunpack::game_params::ttx::ShipUpgradeSelection;

use crate::builds::Better;
use crate::builds::BuildStat;
use crate::builds::Exclusion;
use crate::builds::Item;
use crate::builds::Outcome;
use crate::builds::Planner;
use crate::builds::StatAmount;
use crate::builds::StatPlan;
use crate::builds::StatUnit;
use crate::builds::round;
use crate::data::GameData;
use crate::error::QueryError;
use crate::firing_arcs::round_to;

const FIRE_NOTE: &str = "Fire starts are theoretical: fires_per_minute = projectiles_per_minute x fire_chance, every projectile hitting a target that can burn. The headline is against one distant target at the bearing (degrees off the bow, clockwise) where the most fires start: the server gives each secondary mount its own target, and a gun fires at a target only when its mount turns to it outside its dead zones, so one target draws the mounts whose firing sectors cover its bearing, usually one side and the centerline. all_mounts_ceiling is every gun mount firing at once, which no single target sees. by_bearing gives the figure at standard bearings, starboard and its port mirror, on the base and best builds. salvo_fire_chance_pct = 1 - (1 - p)^n over the n bearing shells of one salvo. Not applied: the target's fire resistance and hull fire-node probability, the limit of fires burning at once, fire duration, dispersion and signal flags. Guns fire at their reload. A carrier squadron, which makes several attacks per sortie, attacks with attackerSize planes every attackCooldown, the shortest time between two of its attacks, while it has attacks left; flight to the target, returns and lost planes are left out, so this is its rate over the target. A squadron that attacks once per sortie, as hybrid ships launch them, attacks again when the hangar has restored its planes. An air strike releases every plane's ordnance once per consumable reload; its charges are not counted. Ordnance per attack is attackerSize x attackCount x projectilesPerAttack, the client's bombs per plane (PreprocessedPlanes). total adds the main battery's best ammo type, every secondary group, the best squadron of the stock plane modules (a carrier flies one at a time) and the best air strike aircraft (one air strike's aircraft share its charges).";

/// Fire starts per minute on the best-fit build.
#[derive(Debug, Clone, Serialize)]
pub struct FireRates {
    /// Every armament together.
    pub total: BuildStat<FireTotal>,

    /// Every secondary group together.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secondaries: Option<BuildStat<FireTotal>>,

    pub sources: Vec<FireSourceBuilds>,
    pub note: &'static str,
}

/// Fires against one target at its best bearing, and with every gun mount
/// firing at once.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct FireTotal {
    /// Against one target at `bearing`; every mount when the mount facings
    /// are unknown.
    pub fires_per_minute: FiresPerMinute,

    /// Degrees off the bow, clockwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bearing: Option<Bearing>,

    /// Every gun mount firing at once, a ceiling no single target sees.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub all_mounts_ceiling: Option<FiresPerMinute>,

    /// The figure at standard bearings; on the base and best builds only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub by_bearing: Option<[BearingRow<FiresPerMinute>; 9]>,
}

impl FireTotal {
    /// `ceiling` is every mount firing; `by_bearing` one target per bearing,
    /// when the mount facings are known.
    fn new(ceiling: FiresPerMinute, by_bearing: Option<PerBearing<FiresPerMinute>>) -> Self {
        let rounded = |fires: FiresPerMinute| FiresPerMinute::new(round(fires.value()));
        let Some(by_bearing) = by_bearing else {
            return Self {
                fires_per_minute: rounded(ceiling),
                bearing: None,
                all_mounts_ceiling: None,
                by_bearing: None,
            };
        };
        let bearing = by_bearing.best();

        Self {
            fires_per_minute: rounded(by_bearing.at(bearing)),
            bearing: Some(bearing),
            all_mounts_ceiling: Some(rounded(ceiling)),
            by_bearing: Some(by_bearing.table().map(|row| BearingRow {
                bearing: row.bearing,
                starboard: rounded(row.starboard),
                port: rounded(row.port),
            })),
        }
    }
}

impl StatAmount for FireTotal {
    fn amount(self) -> f32 {
        self.fires_per_minute.value()
    }
}

impl StatUnit for FireTotal {
    const UNIT: &'static str = "fires/min";
}

/// One armament's fire output.
#[derive(Debug, Clone, Serialize)]
pub struct FireSourceBuilds {
    pub armament: FireArmament,

    /// Ammo type of the main battery, secondary gun group, or aircraft name.
    pub source: String,

    /// GameParams name of the aircraft.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aircraft_param: Option<String>,

    /// Carried by a researched plane module rather than the stock one; the
    /// total leaves it out.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub module_upgrade: bool,

    /// Modules other than stock that carry the source. Empty for stock.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub modules: Vec<String>,

    pub rate: BuildStat<FireRate>,
}

/// One armament against one target at its best bearing: projectiles per
/// minute, the fire chance of one and the fires they start.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct FireRate {
    pub fires_per_minute: FiresPerMinute,

    /// Degrees off the bow, clockwise; absent for aircraft and when the mount
    /// facings are unknown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bearing: Option<Bearing>,

    pub fire_chance_pct: Percent,
    pub projectiles_per_minute: ProjectilesPerMinute,

    /// Chance one salvo of the bearing guns starts at least one fire.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub salvo_fire_chance_pct: Option<Percent>,

    /// Every mount firing at once, a ceiling no single target sees.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub all_mounts_ceiling: Option<FiresPerMinute>,
}

impl FireRate {
    fn new(source: &FireSource, coverage: Option<&MountCoverage>) -> Self {
        let rounded = |fires: FiresPerMinute| FiresPerMinute::new(round(fires.value()));
        let fire_chance_pct = Percent::new(round_to(source.fire_chance.value(), 100.0));
        let projectiles =
            |projectiles: ProjectilesPerMinute| ProjectilesPerMinute::new(round_to(projectiles.value(), 10.0));
        let gun = !source.mounts.is_empty();
        let Some(coverage) = coverage.filter(|_| gun) else {
            return Self {
                fires_per_minute: rounded(source.fires_per_minute),
                bearing: None,
                fire_chance_pct,
                projectiles_per_minute: projectiles(source.projectiles_per_minute),
                salvo_fire_chance_pct: None,
                all_mounts_ceiling: None,
            };
        };
        let by_bearing = source.by_bearing(coverage);
        let bearing = by_bearing.best();

        Self {
            fires_per_minute: rounded(by_bearing.at(bearing)),
            bearing: Some(bearing),
            fire_chance_pct,
            projectiles_per_minute: projectiles(source.projectiles_at(coverage, bearing)),
            salvo_fire_chance_pct: source
                .salvo_fire_chance(coverage, bearing)
                .map(|chance| Percent::new(round_to(chance.value(), 10.0))),
            all_mounts_ceiling: Some(rounded(source.fires_per_minute)),
        }
    }
}

impl StatAmount for FireRate {
    fn amount(self) -> f32 {
        self.fires_per_minute.value()
    }
}

impl StatUnit for FireRate {
    const UNIT: &'static str = "fires/min";
}

impl GameData {
    pub(crate) fn fire_rates(&self, planner: &Planner<'_>) -> Result<Option<FireRates>, QueryError> {
        let selections: Vec<&ShipUpgradeSelection> = planner.selections().collect();
        let coverages: Vec<Option<Rc<MountCoverage>>> = self.mount_coverages(planner.ship(), planner.selections())?;
        let sources = |outcome: &Outcome| -> Vec<FireSource> {
            FireInputs {
                ship: &planner.ship().param,
                selection: selections[outcome.selection],
                stats: &outcome.stats,
                modifiers: &outcome.bundle,
                provider: self.provider(),
            }
            .sources()
        };
        let coverage = |outcome: &Outcome| coverages[outcome.selection].as_deref();

        let total_extract = |outcome: &Outcome| {
            let sources = sources(outcome);
            let ceiling = total_fires_all_mounts(&sources)?;
            let by_bearing = coverage(outcome).and_then(|coverage| total_fires_by_bearing(&sources, coverage));
            Some(FireTotal::new(ceiling, by_bearing))
        };
        let total_exclude = |planner: &Planner<'_>, _: &[(Item, FireTotal)]| lowering_items(planner, 0, &total_extract);
        let mut total_plan =
            StatPlan::new("fire starts per minute, all armament".to_owned(), Better::Higher, &total_extract);
        total_plan.exclude = Some(&total_exclude);
        let Some(mut total) = planner.plan(&total_plan)? else {
            return Ok(None);
        };
        tables_on_base_and_best(&mut total);

        let secondaries_extract = |outcome: &Outcome| {
            let sources = sources(outcome);
            let ceiling: Vec<FiresPerMinute> = sources
                .iter()
                .filter(|source| source.armament == FireArmament::Secondary)
                .map(|source| source.fires_per_minute)
                .collect();
            if ceiling.is_empty() {
                return None;
            }
            let by_bearing = coverage(outcome).and_then(|coverage| secondary_fires_by_bearing(&sources, coverage));
            Some(FireTotal::new(ceiling.into_iter().sum(), by_bearing))
        };
        let secondaries_exclude =
            |planner: &Planner<'_>, _: &[(Item, FireTotal)]| lowering_items(planner, 0, &secondaries_extract);
        let mut secondaries_plan = StatPlan::new(
            "secondary fire starts per minute, every group".to_owned(),
            Better::Higher,
            &secondaries_extract,
        );
        secondaries_plan.exclude = Some(&secondaries_exclude);
        let mut secondaries = planner.plan(&secondaries_plan)?;
        if let Some(secondaries) = &mut secondaries {
            tables_on_base_and_best(secondaries);
        }

        // Each source with the first module combination that carries it.
        let mut found: Vec<(FireSource, usize)> = Vec::new();
        for selection in 0..selections.len() {
            for source in sources(&*planner.bare(selection)?) {
                let known = found
                    .iter()
                    .any(|(other, _)| other.armament == source.armament && other.qualifier == source.qualifier);
                if !known {
                    found.push((source, selection));
                }
            }
        }

        let mut builds = Vec::new();
        for (source, selection) in &found {
            let extract = |outcome: &Outcome| {
                sources(outcome)
                    .iter()
                    .find(|other| other.armament == source.armament && other.qualifier == source.qualifier)
                    .map(|other| FireRate::new(other, coverage(outcome)))
            };
            let exclude = |planner: &Planner<'_>, _: &[(Item, FireRate)]| lowering_items(planner, *selection, &extract);
            let label = format!("{} fire starts per minute", self.fire_source_name(source));
            let mut plan = StatPlan::new(label, Better::Higher, &extract).on_selection(*selection);
            plan.exclude = Some(&exclude);
            let Some(rate) = planner.plan(&plan)? else {
                continue;
            };

            let aircraft = matches!(source.armament, FireArmament::Squadron | FireArmament::AirStrike);
            builds.push(FireSourceBuilds {
                armament: source.armament,
                source: self.fire_source_name(source),
                aircraft_param: aircraft.then(|| source.qualifier.clone()),
                module_upgrade: source.module_upgrade,
                modules: planner.selection_modules(*selection).to_vec(),
                rate,
            });
        }

        Ok(Some(FireRates { total, secondaries, sources: builds, note: FIRE_NOTE }))
    }

    fn fire_source_name(&self, source: &FireSource) -> String {
        match source.armament {
            FireArmament::MainBattery | FireArmament::Secondary => {
                format!("{} {}", source.armament.label(), source.qualifier)
            }
            FireArmament::Squadron | FireArmament::AirStrike => {
                let name = self.unit_name(&source.qualifier);
                format!("{} {name}", source.armament.label())
            }
        }
    }
}

/// Keeps the per-bearing table on the base and best builds and drops it
/// from the alternatives, which differ only in the headline.
fn tables_on_base_and_best(stat: &mut BuildStat<FireTotal>) {
    for alternative in &mut stat.builds.alternatives {
        alternative.value.by_bearing = None;
    }
}

/// Inertia Fuse for HE Shells, when it lowers the stat on module combination
/// `selection`: it halves fire chance, so the best-fit build leaves it out and
/// lists it as an alternative.
fn lowering_items<V: StatAmount>(
    planner: &Planner<'_>,
    selection: usize,
    extract: &dyn Fn(&Outcome) -> Option<V>,
) -> Result<Vec<Exclusion>, QueryError> {
    const IFHE: &str = "Inertia Fuse for HE Shells";

    let base = extract(&*planner.bare(selection)?);
    let mut exclusions = Vec::new();
    for item in planner.items_named(&[IFHE.to_owned()]) {
        let with = extract(&*planner.with_items_on(selection, &[item])?);
        let lowers = base.zip(with).is_some_and(|(base, with)| Better::Higher.improves(base.amount(), with.amount()));
        if lowers {
            exclusions.push(Exclusion {
                item,
                note: "Lowers fire chance: Inertia Fuse for HE Shells halves the fire chance of HE shells, main battery and secondaries alike.".to_owned(),
            });
        }
    }

    Ok(exclusions)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use wowsunpack::game_params::types::GameParamProvider;

    use super::*;

    /// Every squadron and stock air strike whose ordnance can start fires
    /// resolves its attack rate, so none drops out of the totals unseen.
    #[test]
    #[cfg_attr(not(has_build_13533378), ignore)]
    fn every_burning_aircraft_has_an_attack_rate() {
        let root = PathBuf::from(std::env::var_os("WOWS_GAME_DATA").expect("WOWS_GAME_DATA names the dump root"));
        let index = wows_data_mgr::builds::BuildsIndex::load(&root.join("builds.toml"))
            .expect("dump root has a readable builds.toml");
        let entry = index.find_by_build(13533378).expect("dump root lists build 13533378");
        let cache = std::env::temp_dir().join("wows-query-test-cache");
        let game = GameData::load_dump(&root.join(&entry.dir), Some(&cache)).expect("dump loads");

        let burns = |plane: &str| {
            let Some(aircraft) = game.provider().game_param_by_name(plane) else {
                return false;
            };
            aircraft
                .aircraft()
                .and_then(|aircraft| aircraft.ordnance())
                .and_then(|ordnance| game.provider().game_param_by_name(ordnance))
                .and_then(|ordnance| ordnance.projectile().and_then(|projectile| projectile.burn_prob()))
                .is_some_and(|burn_prob| burn_prob > 0.0)
        };

        let mut missing: Vec<String> = Vec::new();
        for ship in &game.ships {
            let Some(components) = ship.param.vehicle().and_then(|vehicle| vehicle.ttx_components()) else {
                continue;
            };
            let selection = ShipUpgradeSelection::stock(&ship.param);
            let mut planes: Vec<&String> = components.squadrons.values().flat_map(|upgrade| &upgrade.planes).collect();
            if let Some(strike) = selection.hull.as_deref().and_then(|hull| components.air_supports.get(hull)) {
                planes.extend(&strike.planes);
            }
            if !planes.iter().any(|plane| burns(plane)) {
                continue;
            }

            let planner = Planner::new(&game, ship);
            let stock = planner.stock().expect("stock card");
            let sources = FireInputs {
                ship: &ship.param,
                selection: &selection,
                stats: &stock.stats,
                modifiers: &stock.bundle,
                provider: game.provider(),
            }
            .sources();
            for plane in planes.into_iter().filter(|plane| burns(plane)) {
                if !sources.iter().any(|source| source.qualifier == *plane) {
                    missing.push(format!("{}: {plane}", ship.name));
                }
            }
        }

        assert!(missing.is_empty(), "{missing:#?}");
    }
}
