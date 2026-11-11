//! A ship's torpedo options and torpedo damage by the distance run
//! (`distanceOfDamage`), shared by ship_comparison and quantity_values.

use wows_core::units::Km;
use wowsunpack::game_params::ttx::constants::BW_TO_BALLISTIC;
use wowsunpack::game_params::ttx::constants::KM_TO_M;
use wowsunpack::game_params::ttx::factories::torpedo_arming_run;
use wowsunpack::game_params::ttx::model::Knots;
use wowsunpack::game_params::types::Projectile;

use crate::builds::Planner;
use crate::comparison::number;
use crate::error::QueryError;

/// A torpedo and the first module combination that carries it.
pub(crate) struct CarriedTorpedo {
    /// GameParams name of the torpedo.
    pub(crate) param: String,

    /// Modules of the combination that are not stock; empty for a torpedo
    /// the stock modules carry.
    pub(crate) modules: Vec<String>,

    /// Index into [`Planner::selections`].
    pub(crate) selection: usize,
}

/// A share of full torpedo damage from a distance on.
pub(crate) struct FalloffPoint {
    pub(crate) distance: Km,
    pub(crate) coefficient: f32,
}

/// Distance from launch a torpedo at `speed` runs before it arms, as the port
/// computes it (FactoryTorpedoes.py:136-139).
pub(crate) fn arming_distance(projectile: &Projectile, speed: Knots) -> Option<Km> {
    let run = torpedo_arming_run(projectile, speed)?;

    Some(Km::new(run * BW_TO_BALLISTIC / KM_TO_M))
}

/// The `distanceOfDamage` points of a torpedo as distances run from launch,
/// nearest first: each pair's distance after the arming run, as the client
/// places `distanceOfMaxDamage` (FactoryTorpedoes.py:131-147). `None` for a
/// torpedo whose damage does not depend on distance.
pub(crate) fn falloff_points(projectile: &Projectile, speed: Knots) -> Option<Vec<FalloffPoint>> {
    let pairs = projectile.distance_of_damage().filter(|pairs| !pairs.is_empty())?;
    let run = torpedo_arming_run(projectile, speed)?;

    let mut points: Vec<FalloffPoint> = pairs
        .iter()
        .map(|&(distance, coefficient)| FalloffPoint {
            distance: Km::new((run + distance) * BW_TO_BALLISTIC / KM_TO_M),
            coefficient,
        })
        .collect();
    points.sort_by(|left, right| left.distance.value().total_cmp(&right.distance.value()));

    Some(points)
}

/// Every torpedo the ship can carry, each with the first module combination
/// that carries it. Selections come stock first, so a torpedo the stock
/// modules carry is listed without modules.
pub(crate) fn torpedo_options(planner: &Planner<'_>) -> Result<Vec<CarriedTorpedo>, QueryError> {
    let mut options: Vec<CarriedTorpedo> = Vec::new();
    for (selection, _) in planner.selections().enumerate() {
        let outcome = planner.bare(selection)?;
        for torpedo in outcome.stats.torpedoes.iter().flat_map(|torpedoes| &torpedoes.torpedoes) {
            if !options.iter().any(|option| option.param == torpedo.name) {
                options.push(CarriedTorpedo {
                    param: torpedo.name.clone(),
                    modules: planner.selection_modules(selection).to_vec(),
                    selection,
                });
            }
        }
    }

    Ok(options)
}

/// "100% of full damage up to 8 km, 50% from 8.1 km".
pub(crate) fn describe_falloff(points: &[FalloffPoint]) -> String {
    let parts: Vec<String> = points
        .iter()
        .enumerate()
        .map(|(index, point)| {
            let share = number(point.coefficient * 100.0);
            let distance = number(point.distance.value());
            if index == 0 {
                format!("{share}% of full damage up to {distance} km")
            } else {
                format!("{share}% from {distance} km")
            }
        })
        .collect();

    parts.join(", ")
}

/// Share of full damage a hit deals after a run of `distance`: the share of
/// the nearest point before the first and after the last, and linear between
/// two points. Game data places points 0.1 km apart where the share steps,
/// which only makes a step under linear interpolation.
pub(crate) fn damage_share(points: &[FalloffPoint], distance: Km) -> Option<f32> {
    let first = points.first()?;
    let last = points.last()?;
    if distance.value() <= first.distance.value() {
        return Some(first.coefficient);
    }
    if distance.value() >= last.distance.value() {
        return Some(last.coefficient);
    }

    points.windows(2).find_map(|pair| {
        let [near, far] = pair else {
            return None;
        };
        if distance.value() > far.distance.value() {
            return None;
        }

        let span = far.distance.value() - near.distance.value();
        let along = if span > 0.0 { (distance.value() - near.distance.value()) / span } else { 1.0 };

        Some(near.coefficient + (far.coefficient - near.coefficient) * along)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(distance: f32, coefficient: f32) -> FalloffPoint {
        FalloffPoint { distance: Km::new(distance), coefficient }
    }

    #[test]
    fn share_steps_between_close_points_and_holds_past_the_ends() {
        let points = [point(8.0, 1.0), point(8.1, 0.5)];

        let shares: Vec<Option<f32>> =
            [1.0, 8.0, 8.1, 12.0].into_iter().map(|km| damage_share(&points, Km::new(km))).collect();
        assert_eq!(shares, [Some(1.0), Some(1.0), Some(0.5), Some(0.5)]);
        let between = damage_share(&points, Km::new(8.05)).expect("points exist");
        assert!((between - 0.75).abs() < 1e-3, "got {between}");
    }

    #[test]
    fn arming_and_falloff_keep_full_distance_precision() {
        use wows_core::units::BigWorldDistance;

        let projectile = Projectile::builder()
            .maneuver_dist(BigWorldDistance::from(12.345))
            .distance_of_damage(vec![(0.123, 1.0)])
            .build();
        let speed = Knots::from(60.0);
        let expected_arming = 12.345 * BW_TO_BALLISTIC / KM_TO_M;
        let expected_falloff = (12.345 + 0.123) * BW_TO_BALLISTIC / KM_TO_M;

        let arming = arming_distance(&projectile, speed).expect("arming distance");
        let falloff = falloff_points(&projectile, speed).expect("falloff points");

        assert_eq!(arming.value(), expected_arming);
        assert_eq!(falloff[0].distance.value(), expected_falloff);
        assert_ne!(arming.value(), (arming.value() * 100.0).round() / 100.0);
    }
}
