//! Trajectory simulation through an armor model, and the comparison against
//! what the server reported.
//!
//! The flat-penetration half lives in
//! `wows_toolkit_viewmodel::armor::penetration`, which both front ends read;
//! what is left here is tied to this crate's own `Vec3`.

use crate::viewport_3d::Vec3;

use wowsunpack::game_params::types::Degrees;
use wowsunpack::game_params::types::Km;
use wowsunpack::game_params::types::Millimeters;
use wowsunpack::game_params::types::ShipModelDistance;

use wowsunpack::ballistics::is_overmatch;

pub use wows_toolkit_viewmodel::armor::penetration::ComparisonShip;
pub use wows_toolkit_viewmodel::armor::penetration::ComparisonShipIndex;
pub use wows_toolkit_viewmodel::armor::penetration::Ifhe;
pub use wows_toolkit_viewmodel::armor::penetration::PenResult;
pub use wows_toolkit_viewmodel::armor::penetration::check_penetration;
pub use wows_toolkit_viewmodel::armor::penetration::he_penetration;
pub use wows_toolkit_viewmodel::armor::penetration::resolve_ship_shells;
pub use wows_toolkit_viewmodel::armor::penetration::sap_penetration;

/// A single hit along a trajectory ray through the armor model.
///
/// Positions and along-ray distances are in ship-model space (15 m per unit).
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct TrajectoryHit {
    pub position: Vec3,
    pub thickness: Millimeters,
    pub zone: String,
    pub material: String,
    /// Strike angle from the plate normal: 0 is head-on, 90 is glancing.
    pub angle_from_normal: Degrees,
    pub distance_from_start: ShipModelDistance,
}

/// Result of casting a trajectory ray through the armor model.
#[derive(Clone, Debug)]
pub struct TrajectoryResult {
    pub origin: Vec3,
    pub direction: Vec3,
    pub hits: Vec<TrajectoryHit>,
    /// Sum of every plate thickness along the ray, ignoring angle.
    pub total_armor: Millimeters,
    /// Per-ship ballistic arcs (each ship gets its own arc shape + impact data).
    pub ship_arcs: Vec<ShipArc>,
    /// Where AP shells detonate (one per comparison shell that has a fuse event).
    pub detonation_points: Vec<DetonationMarker>,
}

/// Which zone volume the shell is inside after crossing plates up to and
/// including `last_plate`.
///
/// Shared with the GPUI port, which reads the same answer out of the same cast
/// (`wows_toolkit_viewmodel::armor::arc`).
pub fn enclosing_zone(hits: &[TrajectoryHit], last_plate: PlateIndex) -> Option<&str> {
    let zones: Vec<&str> = hits.iter().map(|hit| hit.zone.as_str()).collect();
    wows_toolkit_viewmodel::armor::arc::enclosing_zone(&zones, last_plate.number())
}

/// Strike angle between a ray direction and a triangle normal.
///
/// Measured from the normal: 0 is head-on (perpendicular to the plate), 90 is
/// glancing (parallel to it). This is the convention the game's ricochet angles
/// (45/60 deg) are stated in.
pub fn impact_angle_from_normal(ray_dir: &Vec3, normal: &Vec3) -> Degrees {
    let cos_angle = ray_dir.dot(normal).abs().min(1.0);
    Degrees::from(cos_angle.acos().to_degrees())
}

// The per-plate simulation lives in wowsunpack::ballistics; re-exported here
// so armor-viewer call sites keep their existing paths.

use crate::armor_viewer::ballistics::ImpactResult;
use crate::armor_viewer::ballistics::ShellParams;
pub use wowsunpack::ballistics::FuseDetonation;
pub use wowsunpack::ballistics::PlateHit;
pub use wowsunpack::ballistics::PlateIndex;
pub use wowsunpack::ballistics::PlateOutcome;
pub use wowsunpack::ballistics::ShellSimResult;
pub use wowsunpack::ballistics::simulate_shell_through_plates;

/// A detonation point in 3D space, tagged with which comparison ship produced it.
#[derive(Clone, Debug)]
pub struct DetonationMarker {
    pub position: Vec3,
    pub ship: ComparisonShipIndex,
}

/// Per-ship ballistic arc data for a trajectory.
#[derive(Clone, Debug)]
pub struct ShipArc {
    pub ship: ComparisonShipIndex,
    pub arc_points_3d: Vec<Vec3>,
    pub ballistic_impact: Option<ImpactResult>,
}

/// Simulate a shell through ray-cast armor hits.
///
/// Thin adapter over [`simulate_shell_through_plates`]: extracts the scalar
/// along-ray plate list from the 3D hits.
pub fn simulate_shell_through_hits(
    params: &ShellParams,
    impact: &ImpactResult,
    hits: &[TrajectoryHit],
    continue_on_ricochet: bool,
) -> ShellSimResult {
    let plates: Vec<PlateHit> = hits
        .iter()
        .map(|hit| PlateHit {
            thickness: hit.thickness,
            angle_from_normal: hit.angle_from_normal,
            distance_along_ray: hit.distance_from_start,
        })
        .collect();
    simulate_shell_through_plates(params, impact, &plates, continue_on_ricochet)
}

/// 3D position of a fuse detonation along the trajectory ray.
///
/// `None` when there are no hits (a detonation only exists after a hit armed
/// the fuse, so this is unreachable in practice).
pub fn detonation_position(hits: &[TrajectoryHit], det: &FuseDetonation, shell_dir: &Vec3) -> Option<Vec3> {
    let first = hits.first()?;
    let dir = shell_dir / shell_dir.norm().max(1e-9);
    Some(first.position + dir * det.distance_along_ray.value())
}

/// Metadata for a stored trajectory (non-simulation display data).
#[derive(Clone, Debug)]
pub struct TrajectoryMeta {
    /// Unique monotonically increasing ID for stable UI references.
    pub id: u64,
    /// Index into the trajectory color palette.
    pub color_index: usize,
    /// Per-trajectory ballistic range.
    pub range: Km,
}

/// Distance between two points in ship-model space.
pub fn model_distance(a: &Vec3, b: &Vec3) -> ShipModelDistance {
    ShipModelDistance::from((b - a).norm())
}

// Server vs Simulation Comparison

/// Shared with the GPUI port, whose incoming-fire log names the same
/// outcomes.
pub use wows_toolkit_viewmodel::armor::incoming::ServerOutcome;

/// What the simulation says became of the shell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimOutcome {
    Citadel,
    Penetration,
    Overpenetration,
    Ricochet,
    Shatter,
    /// Stopped in the armor without an identified ricochet or shatter.
    Stopped,
}

impl SimOutcome {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Citadel => "Citadel",
            Self::Penetration => "Penetration",
            Self::Overpenetration => "Overpenetration",
            Self::Ricochet => "Ricochet",
            Self::Shatter => "Shatter",
            Self::Stopped => "Stopped",
        }
    }
}

/// The simulation's side of a disagreement with the server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimVerdict {
    /// The simulation ran to this outcome.
    Outcome(SimOutcome),
    /// The plate is overmatched, which rules a ricochet out entirely.
    OvermatchRulesOutRicochet,
    /// The strike sits in the band where a ricochet is certain.
    AlwaysRicochetBand,
    /// The strike is too shallow for a ricochet to be possible at all.
    RicochetAngleTooLow,
}

impl SimVerdict {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Outcome(outcome) => outcome.display_name(),
            Self::OvermatchRulesOutRicochet => "Overmatch (can't ricochet)",
            Self::AlwaysRicochetBand => "Ricochet (always-ricochet zone)",
            Self::RicochetAngleTooLow => "No ricochet possible (angle too low)",
        }
    }
}

/// How our simulation compares to the server.
#[derive(Clone, Debug)]
pub enum ComparisonVerdict {
    /// Simulation matches server.
    Match,
    /// Angle is in the ricochet RNG zone; server's call is valid either way.
    RicochetRngDefer { angle: Degrees, ricochet_start: Degrees, always_ricochet: Degrees },
    /// Simulation disagrees with server.
    Mismatch { sim: SimVerdict, server: ServerOutcome },
}

/// Overpen exit point comparison.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct ExitDivergence {
    /// Server exit position (model space).
    pub server_exit_pos: Vec3,
    /// Simulated exit position (model space). None if sim didn't produce an exit.
    pub sim_exit_pos: Option<Vec3>,
    /// Distance between them. None if sim exit unavailable.
    pub distance: Option<ShipModelDistance>,
}

/// Full comparison for one shell.
#[derive(Clone, Debug)]
pub struct ServerVsSimComparison {
    pub server_outcome: ServerOutcome,
    pub sim: ShellSimResult,
    pub verdict: ComparisonVerdict,
    pub exit_divergence: Option<ExitDivergence>,
}

/// Classify the simulation's outcome for the shell.
pub fn sim_outcome(sim: &ShellSimResult, hits: &[TrajectoryHit]) -> SimOutcome {
    // A detonation takes priority even if the shell shattered or ricocheted on a
    // later plate: the fragments still explode.
    if sim.detonation.is_some() {
        let Some(detonated_at) = sim.detonated_at else {
            return SimOutcome::Overpenetration;
        };
        let inside_citadel =
            enclosing_zone(hits, detonated_at).is_some_and(|zone| zone.to_lowercase().contains("citadel"));
        return if inside_citadel { SimOutcome::Citadel } else { SimOutcome::Penetration };
    }

    let Some(stopped_at) = sim.stopped_at else {
        return SimOutcome::Overpenetration;
    };
    match sim.plates.get(stopped_at.value()).map(|plate| plate.outcome) {
        Some(PlateOutcome::Ricochet) => SimOutcome::Ricochet,
        Some(PlateOutcome::Shatter) => SimOutcome::Shatter,
        _ => SimOutcome::Stopped,
    }
}

/// Compare a shell simulation result against the server's authoritative outcome.
///
/// Ricochet reasoning needs the first plate the shell struck, so this returns
/// `None` when the ray produced no hits at all.
pub fn compare_with_server(
    sim: &ShellSimResult,
    hits: &[TrajectoryHit],
    server_outcome: &ServerOutcome,
    params: &ShellParams,
) -> Option<ComparisonVerdict> {
    let first_hit = hits.first()?;
    let strike_angle = first_hit.angle_from_normal;
    let overmatched = is_overmatch(params.caliber, first_hit.thickness);
    let ricochet_start = params.ricochet_angle.to_degrees();
    let always_ricochet = params.always_ricochet_angle.to_degrees();

    let mismatch = |sim_verdict| ComparisonVerdict::Mismatch { sim: sim_verdict, server: server_outcome.clone() };

    if *server_outcome == ServerOutcome::Ricochet {
        if overmatched {
            return Some(mismatch(SimVerdict::OvermatchRulesOutRicochet));
        }
        if strike_angle >= always_ricochet {
            return Some(ComparisonVerdict::Match);
        }
        if strike_angle >= ricochet_start {
            return Some(ComparisonVerdict::RicochetRngDefer { angle: strike_angle, ricochet_start, always_ricochet });
        }
        return Some(mismatch(SimVerdict::RicochetAngleTooLow));
    }

    // Server didn't ricochet. Check if we think it should have.
    if !overmatched && strike_angle >= always_ricochet {
        return Some(mismatch(SimVerdict::AlwaysRicochetBand));
    }

    let outcome = sim_outcome(sim, hits);
    let agrees = match server_outcome {
        ServerOutcome::Penetration => outcome == SimOutcome::Penetration,
        ServerOutcome::Citadel => outcome == SimOutcome::Citadel,
        ServerOutcome::Shatter => outcome == SimOutcome::Shatter,
        ServerOutcome::Overpenetration => outcome == SimOutcome::Overpenetration,
        // The simulation models neither of these, so it cannot disagree.
        ServerOutcome::Underwater | ServerOutcome::Unknown(_) => true,
        ServerOutcome::Ricochet => false,
    };

    Some(if agrees { ComparisonVerdict::Match } else { mismatch(SimVerdict::Outcome(outcome)) })
}
