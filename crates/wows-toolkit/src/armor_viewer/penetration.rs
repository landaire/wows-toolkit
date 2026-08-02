use crate::viewport_3d::Vec3;

use std::collections::HashSet;
use std::sync::Arc;

use wowsunpack::data::ResourceLoader;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::AmmoType;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::Km;
use wowsunpack::game_params::types::Millimeters;
use wowsunpack::game_params::types::Param;
use wowsunpack::game_params::types::ShellInfo;
use wowsunpack::game_params::types::Species;

pub use wowsunpack::ballistics::OVERMATCH_RATIO;

/// A ship added to the comparison list.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct ComparisonShip {
    pub param_index: String,
    pub display_name: String,
    pub tier: u32,
    pub nation: String,
    pub species: Species,
    pub shells: Vec<ShellInfo>,
}

/// Check result for a single shell vs a single armor thickness.
#[derive(Clone, Debug, PartialEq)]
pub enum PenResult {
    /// Shell penetrates (HE/SAP pen >= thickness, or AP overmatch).
    Penetrates,
    /// Shell does not penetrate.
    Bounces,
    /// Angle-dependent (AP without overmatch; can't determine at point-blank without angle).
    AngleDependent,
}

/// Check if a shell penetrates a given armor thickness at point-blank (no angle consideration).
///
/// Returns `None` for unknown ammo types (logged as a warning).
pub fn check_penetration(shell: &ShellInfo, thickness_mm: f32, ifhe: bool) -> Option<PenResult> {
    match &shell.ammo_type {
        AmmoType::HE => {
            let pen = if ifhe { shell.he_pen_mm.unwrap_or(0.0) * 1.25 } else { shell.he_pen_mm.unwrap_or(0.0) };
            Some(if pen >= thickness_mm { PenResult::Penetrates } else { PenResult::Bounces })
        }
        AmmoType::SAP => {
            let pen = shell.sap_pen_mm.unwrap_or(0.0);
            Some(if pen >= thickness_mm { PenResult::Penetrates } else { PenResult::Bounces })
        }
        AmmoType::AP => {
            // Overmatch: caliber > armor * OVERMATCH_RATIO
            Some(if shell.caliber.value() > thickness_mm * OVERMATCH_RATIO {
                PenResult::Penetrates
            } else {
                PenResult::AngleDependent
            })
        }
        AmmoType::Unknown(t) => {
            tracing::warn!("Unknown ammo type '{}' for shell '{}', cannot check penetration", t, shell.name);
            None
        }
    }
}

/// Resolve all unique shells for a ship by param_index.
///
/// Chain: ship param -> vehicle -> ShipConfigData.main_battery_ammo -> Projectile lookup.
pub fn resolve_ship_shells(metadata: &GameMetadataProvider, param_index: &str) -> Option<ComparisonShip> {
    let param: Arc<Param> = metadata.game_param_by_index(param_index)?;

    let species = param.species()?.known().copied()?;
    let vehicle = param.vehicle()?;
    let tier = vehicle.level();
    let nation = param.nation().to_string();

    let display_name = metadata.localized_name_from_param(&param).unwrap_or_else(|| param.name().to_string());

    // Get main battery ammo names from the config data
    let config = vehicle.config_data()?;
    let ammo_names: &HashSet<String> = &config.main_battery_ammo;

    let mut shells: Vec<ShellInfo> = Vec::new();
    let mut seen_names: HashSet<&String> = HashSet::new();

    for ammo_name in ammo_names {
        if !seen_names.insert(ammo_name) {
            continue;
        }
        let ammo_param = metadata.game_param_by_name(ammo_name)?;
        let projectile = ammo_param.projectile()?;
        shells.push(projectile.to_shell_info(ammo_name.clone()));
    }

    // Sort shells: AP first, then HE, then SAP
    shells.sort_by(|a, b| {
        a.ammo_type
            .sort_order()
            .cmp(&b.ammo_type.sort_order())
            .then(a.caliber.partial_cmp(&b.caliber).unwrap_or(std::cmp::Ordering::Equal))
    });

    Some(ComparisonShip { param_index: param_index.to_string(), display_name, tier, nation, species, shells })
}

/// A single hit along a trajectory ray through the armor model.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct TrajectoryHit {
    pub position: Vec3,
    pub thickness_mm: f32,
    pub zone: String,
    pub material: String,
    pub angle_deg: f32,
    pub distance_from_start: f32,
}

/// Result of casting a trajectory ray through the armor model.
#[derive(Clone, Debug)]
pub struct TrajectoryResult {
    pub origin: Vec3,
    pub direction: Vec3,
    pub hits: Vec<TrajectoryHit>,
    pub total_armor_mm: f32,
    /// Per-ship ballistic arcs (each ship gets its own arc shape + impact data).
    pub ship_arcs: Vec<ShipArc>,
    /// Where AP shells detonate (one per comparison shell that has a fuse event).
    pub detonation_points: Vec<DetonationMarker>,
}

/// Determine which zone volume the shell is inside after passing through plates `0..=last_plate_idx`.
///
/// Each plate crossing toggles whether the shell is inside that plate's zone. Zones with an
/// odd crossing count are "entered". The innermost (most recently entered) zone is returned.
pub fn enclosing_zone(hits: &[TrajectoryHit], last_plate_idx: usize) -> String {
    use std::collections::HashMap;
    let mut zone_crossings: HashMap<&str, usize> = HashMap::new();
    let mut last_entered = None;
    for (i, hit) in hits.iter().enumerate() {
        if i > last_plate_idx {
            break;
        }
        let count = zone_crossings.entry(&hit.zone).or_insert(0);
        *count += 1;
        if *count % 2 == 1 {
            // Odd crossing = just entered this zone
            last_entered = Some(hit.zone.as_str());
        }
    }
    // Return the innermost zone (last one entered with odd count), or fall back
    if let Some(zone) = last_entered
        && *zone_crossings.get(zone).unwrap_or(&0) % 2 == 1
    {
        return zone.to_string();
    }
    // Fallback: any zone with odd crossing count (last one in iteration order)
    for (i, hit) in hits.iter().enumerate().rev() {
        if i > last_plate_idx {
            continue;
        }
        if *zone_crossings.get(hit.zone.as_str()).unwrap_or(&0) % 2 == 1 {
            return hit.zone.clone();
        }
    }
    "past all armor".to_string()
}

/// Compute the impact angle between a ray direction and a triangle normal (in degrees).
/// Returns angle from normal: 0 deg = head-on (perpendicular to plate), 90 deg = glancing (parallel).
/// This matches the WoWs convention where ricochet angles (45/60 deg) are from normal.
pub fn impact_angle_deg(ray_dir: &Vec3, normal: &Vec3) -> f32 {
    let cos_angle = ray_dir.dot(normal).abs().min(1.0);
    cos_angle.acos().to_degrees()
}

// The per-plate simulation lives in wowsunpack::ballistics; re-exported here
// so armor-viewer call sites keep their existing paths.

use crate::armor_viewer::ballistics::ImpactResult;
use crate::armor_viewer::ballistics::ShellParams;
pub use wowsunpack::ballistics::FuseDetonation;
pub use wowsunpack::ballistics::PlateHit;
pub use wowsunpack::ballistics::PlateOutcome;
pub use wowsunpack::ballistics::ShellSimResult;
pub use wowsunpack::ballistics::simulate_shell_through_plates;

/// A detonation point in 3D space, tagged with which comparison ship produced it.
#[derive(Clone, Debug)]
pub struct DetonationMarker {
    pub position: Vec3,
    pub ship_index: usize,
}

/// Per-ship ballistic arc data for a trajectory.
#[derive(Clone, Debug)]
pub struct ShipArc {
    pub ship_index: usize,
    pub arc_points_3d: Vec<Vec3>,
    pub ballistic_impact: Option<crate::armor_viewer::ballistics::ImpactResult>,
}

/// Simulate a shell through ray-cast armor hits.
///
/// Thin adapter over [`simulate_shell_through_plates`]: extracts the scalar
/// along-ray plate list from the 3D hits. Positions and along-ray distances are
/// in ship-model units (15 m per unit).
pub fn simulate_shell_through_hits(
    params: &ShellParams,
    impact: &ImpactResult,
    hits: &[TrajectoryHit],
    continue_on_ricochet: bool,
) -> ShellSimResult {
    let plates: Vec<PlateHit> = hits
        .iter()
        .map(|h| PlateHit {
            thickness_mm: h.thickness_mm,
            angle_deg: h.angle_deg,
            distance_along_ray: wowsunpack::game_params::types::ShipModelDistance::from(h.distance_from_start),
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

/// Euclidean distance between two 3D points.
pub fn distance_3d(a: &Vec3, b: &Vec3) -> f32 {
    (b - a).norm()
}

// Server vs Simulation Comparison

use wowsunpack::game_types::ShellHitType;
use wowsunpack::recognized::Recognized;

/// Server-authoritative shell outcome (mapped from ShellHitType).
#[derive(Clone, Debug, PartialEq)]
pub enum ServerOutcome {
    Penetration,
    Citadel,
    Ricochet,
    Shatter,
    Overpenetration,
    Underwater,
    Unknown(String),
}

impl ServerOutcome {
    pub fn from_shell_hit_type(hit: &Recognized<ShellHitType>) -> Self {
        match hit {
            Recognized::Known(ShellHitType::Normal) => Self::Penetration,
            Recognized::Known(ShellHitType::MajorHit) => Self::Citadel,
            Recognized::Known(ShellHitType::Ricochet) => Self::Ricochet,
            Recognized::Known(ShellHitType::NoPenetration) => Self::Shatter,
            Recognized::Known(ShellHitType::Overpenetration) => Self::Overpenetration,
            Recognized::Known(ShellHitType::ExitOverpenetration) => Self::Overpenetration,
            Recognized::Known(ShellHitType::Underwater) => Self::Underwater,
            Recognized::Known(ShellHitType::None) => Self::Unknown("None".into()),
            Recognized::Unknown(s) => Self::Unknown(s.clone()),
        }
    }

    pub fn display_name(&self) -> &str {
        match self {
            Self::Penetration => "Penetration",
            Self::Citadel => "Citadel",
            Self::Ricochet => "Ricochet",
            Self::Shatter => "Shatter",
            Self::Overpenetration => "Overpenetration",
            Self::Underwater => "Underwater",
            Self::Unknown(s) => s.as_str(),
        }
    }
}

/// How our simulation compares to the server.
#[derive(Clone, Debug)]
pub enum ComparisonVerdict {
    /// Simulation matches server.
    Match,
    /// Angle is in the ricochet RNG zone; server's call is valid either way.
    RicochetRngDefer { angle_deg: f32, range_start_deg: f32, range_end_deg: f32 },
    /// Simulation disagrees with server.
    Mismatch { sim_desc: String, server_desc: String },
}

/// Overpen exit point comparison.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct ExitDivergence {
    /// Server exit position (model space).
    pub server_exit_pos: Vec3,
    /// Simulated exit position (model space). None if sim didn't produce an exit.
    pub sim_exit_pos: Option<Vec3>,
    /// Distance between them in model units. None if sim exit unavailable.
    pub distance: Option<f32>,
}

/// Full comparison for one shell.
#[derive(Clone, Debug)]
pub struct ServerVsSimComparison {
    pub server_outcome: ServerOutcome,
    pub sim: ShellSimResult,
    pub verdict: ComparisonVerdict,
    pub exit_divergence: Option<ExitDivergence>,
}

/// Describe the simulation outcome in human-readable form.
fn describe_sim_outcome(sim: &ShellSimResult, hits: &[TrajectoryHit]) -> &'static str {
    // If fuse was armed and shell detonates, the detonation outcome takes priority
    // even if the shell shattered/ricocheted on a later plate (the fragments still explode).
    if sim.detonation.is_some() {
        if let Some(det_idx) = sim.detonated_at {
            let zone = enclosing_zone(hits, det_idx);
            if zone.to_lowercase().contains("citadel") {
                return "Citadel";
            }
            return "Penetration";
        }
        return "Overpenetration";
    }
    if let Some(stop_idx) = sim.stopped_at {
        if let Some(plate) = sim.plates.get(stop_idx) {
            return match plate.outcome {
                PlateOutcome::Ricochet => "Ricochet",
                PlateOutcome::Shatter => "Shatter",
                _ => "Stopped",
            };
        }
        return "Stopped";
    }
    "Overpenetration"
}

/// Compare a shell simulation result against the server's authoritative outcome.
///
/// `first_hit_angle_deg`: impact angle from normal of the first plate (0 deg = head-on, 90 deg = parallel).
/// `first_hit_thickness_mm`: thickness of the first plate hit.
pub fn compare_with_server(
    sim: &ShellSimResult,
    hits: &[TrajectoryHit],
    server_outcome: &ServerOutcome,
    params: &ShellParams,
    first_hit_angle_deg: f32,
    first_hit_thickness: Millimeters,
) -> ComparisonVerdict {
    let caliber = Millimeters::new((params.caliber * 1000.0) as f32);
    let is_overmatch = caliber > first_hit_thickness * OVERMATCH_RATIO;
    let ricochet0_deg = params.ricochet0.to_degrees() as f32;
    let ricochet1_deg = params.ricochet1.to_degrees() as f32;

    let server_is_ricochet = *server_outcome == ServerOutcome::Ricochet;

    // Handle ricochet logic first
    if server_is_ricochet {
        if is_overmatch {
            return ComparisonVerdict::Mismatch {
                sim_desc: "Overmatch (can't ricochet)".into(),
                server_desc: "Ricochet".into(),
            };
        }
        if first_hit_angle_deg >= ricochet1_deg {
            return ComparisonVerdict::Match;
        }
        if first_hit_angle_deg >= ricochet0_deg {
            return ComparisonVerdict::RicochetRngDefer {
                angle_deg: first_hit_angle_deg,
                range_start_deg: ricochet0_deg,
                range_end_deg: ricochet1_deg,
            };
        }
        return ComparisonVerdict::Mismatch {
            sim_desc: "No ricochet possible (angle too low)".into(),
            server_desc: "Ricochet".into(),
        };
    }

    // Server didn't ricochet. Check if we think it should have.
    if !is_overmatch && first_hit_angle_deg >= ricochet1_deg {
        return ComparisonVerdict::Mismatch {
            sim_desc: "Ricochet (always-ricochet zone)".into(),
            server_desc: server_outcome.display_name().into(),
        };
    }

    // Compare pen/shatter/overpen outcomes
    let sim_desc = describe_sim_outcome(sim, hits);

    let matches = match server_outcome {
        ServerOutcome::Penetration => sim_desc == "Penetration",
        ServerOutcome::Citadel => sim_desc == "Citadel",
        ServerOutcome::Shatter => sim_desc == "Shatter",
        ServerOutcome::Overpenetration => sim_desc == "Overpenetration",
        ServerOutcome::Underwater => true,
        ServerOutcome::Unknown(_) => true,
        ServerOutcome::Ricochet => false,
    };

    if matches {
        ComparisonVerdict::Match
    } else {
        ComparisonVerdict::Mismatch { sim_desc: sim_desc.into(), server_desc: server_outcome.display_name().into() }
    }
}
