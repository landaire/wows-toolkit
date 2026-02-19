use std::collections::HashSet;
use std::sync::Arc;

use wowsunpack::data::ResourceLoader;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::{GameParamProvider, Param, Species};

/// Shell info extracted from GameParams for display and penetration checks.
#[derive(Clone, Debug)]
pub struct ShellInfo {
    pub name: String,
    pub ammo_type: String,
    pub caliber_mm: f32,
    pub he_pen_mm: Option<f32>,
    pub sap_pen_mm: Option<f32>,
    pub alpha_damage: f32,
    pub muzzle_velocity: f32,
    pub mass_kg: f32,
    pub krupp: f32,
    pub ricochet_angle: f32,
    pub always_ricochet_angle: f32,
    pub fuse_time: f32,
    pub fuse_threshold: f32,
    pub burn_prob: f32,
    pub air_drag: f32,
    pub normalization: f32,
}

/// A ship added to the comparison list.
#[derive(Clone, Debug)]
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
    /// Angle-dependent (AP without overmatch — can't determine at point-blank without angle).
    AngleDependent,
}

/// Check if a shell penetrates a given armor thickness at point-blank (no angle consideration).
pub fn check_penetration(shell: &ShellInfo, thickness_mm: f32, ifhe: bool) -> PenResult {
    match shell.ammo_type.as_str() {
        "HE" => {
            let pen = if ifhe { shell.he_pen_mm.unwrap_or(0.0) * 1.25 } else { shell.he_pen_mm.unwrap_or(0.0) };
            if pen >= thickness_mm { PenResult::Penetrates } else { PenResult::Bounces }
        }
        "CS" => {
            let pen = shell.sap_pen_mm.unwrap_or(0.0);
            if pen >= thickness_mm { PenResult::Penetrates } else { PenResult::Bounces }
        }
        "AP" => {
            // Overmatch: caliber > armor * 14.3
            if shell.caliber_mm > thickness_mm * 14.3 { PenResult::Penetrates } else { PenResult::AngleDependent }
        }
        _ => PenResult::Bounces,
    }
}

/// Resolve all unique shells for a ship by param_index.
///
/// Chain: ship param → vehicle → ShipConfigData.main_battery_ammo → Projectile lookup.
pub fn resolve_ship_shells(metadata: &GameMetadataProvider, param_index: &str) -> Option<ComparisonShip> {
    let param: Arc<Param> = metadata.game_param_by_index(param_index)?;

    let species = param.species()?.known().copied()?;
    let vehicle = param.vehicle()?;
    let tier = vehicle.level();
    let nation = param.nation().to_string();

    let display_name =
        metadata.localized_name_from_param(&param).map(|s| s.to_string()).unwrap_or_else(|| param.name().to_string());

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

        let caliber_m = projectile.bullet_diametr().unwrap_or(0.0);
        let caliber_mm = caliber_m * 1000.0;

        shells.push(ShellInfo {
            name: ammo_name.clone(),
            ammo_type: projectile.ammo_type().to_string(),
            caliber_mm,
            he_pen_mm: projectile.alpha_piercing_he(),
            sap_pen_mm: projectile.alpha_piercing_cs(),
            alpha_damage: projectile.alpha_damage().unwrap_or(0.0),
            muzzle_velocity: projectile.bullet_speed().unwrap_or(0.0),
            mass_kg: projectile.bullet_mass().unwrap_or(0.0),
            krupp: projectile.bullet_krupp().unwrap_or(0.0),
            ricochet_angle: projectile.bullet_ricochet_at().unwrap_or(45.0),
            always_ricochet_angle: projectile.bullet_always_ricochet_at().unwrap_or(60.0),
            fuse_time: projectile.bullet_detonator().unwrap_or(0.033),
            fuse_threshold: projectile.bullet_detonator_threshold().unwrap_or(0.0),
            burn_prob: projectile.burn_prob().unwrap_or(-0.5),
            air_drag: projectile.bullet_air_drag().unwrap_or(0.0),
            normalization: projectile.bullet_cap_normalize_max_angle().unwrap_or(0.0),
        });
    }

    // Sort shells: AP first, then HE, then SAP (CS)
    shells.sort_by(|a, b| {
        fn ammo_order(t: &str) -> u8 {
            match t {
                "AP" => 0,
                "HE" => 1,
                "CS" => 2,
                _ => 3,
            }
        }
        ammo_order(&a.ammo_type)
            .cmp(&ammo_order(&b.ammo_type))
            .then(a.caliber_mm.partial_cmp(&b.caliber_mm).unwrap_or(std::cmp::Ordering::Equal))
    });

    Some(ComparisonShip { param_index: param_index.to_string(), display_name, tier, nation, species, shells })
}

/// Format ammo type for display.
pub fn ammo_type_display(ammo_type: &str) -> &str {
    match ammo_type {
        "AP" => "AP",
        "HE" => "HE",
        "CS" => "SAP",
        _ => ammo_type,
    }
}

/// A single hit along a trajectory ray through the armor model.
#[derive(Clone, Debug)]
pub struct TrajectoryHit {
    pub position: [f32; 3],
    pub thickness_mm: f32,
    pub zone: String,
    pub material: String,
    pub angle_deg: f32,
    pub distance_from_start: f32,
}

/// Result of casting a trajectory ray through the armor model.
#[derive(Clone, Debug)]
pub struct TrajectoryResult {
    pub origin: [f32; 3],
    pub direction: [f32; 3],
    pub hits: Vec<TrajectoryHit>,
    pub total_armor_mm: f32,
    /// 3D arc points from firing position to first hit (empty if range=0 / point-blank).
    pub arc_points_3d: Vec<[f32; 3]>,
    /// Ballistic impact data at the selected range (None if range=0).
    pub ballistic_impact: Option<crate::armor_viewer::ballistics::ImpactResult>,
    /// Where AP shells detonate (one per comparison shell that has a fuse event).
    pub detonation_points: Vec<DetonationMarker>,
}

/// Compute the impact angle between a ray direction and a triangle normal (in degrees).
/// Returns angle from normal: 0° = head-on (perpendicular to plate), 90° = glancing (parallel).
/// This matches the WoWs convention where ricochet angles (45°/60°) are from normal.
pub fn impact_angle_deg(ray_dir: &[f32; 3], normal: &[f32; 3]) -> f32 {
    let dot = ray_dir[0] * normal[0] + ray_dir[1] * normal[1] + ray_dir[2] * normal[2];
    let cos_angle = dot.abs().min(1.0);
    cos_angle.acos().to_degrees()
}

// ---------------------------------------------------------------------------
// Per-plate ballistic simulation
// ---------------------------------------------------------------------------

use crate::armor_viewer::ballistics::{ImpactResult, ShellParams};

/// Outcome of a shell hitting a single plate.
#[derive(Clone, Debug, PartialEq)]
pub enum PlateOutcome {
    /// Caliber > 14.3 * thickness — always penetrates, ignores ricochet.
    Overmatch,
    /// Shell penetrates (raw_pen >= effective_thickness).
    Penetrate,
    /// Angle >= always_ricochet — guaranteed ricochet, shell stopped.
    Ricochet,
    /// Shell shatters (raw_pen < effective_thickness).
    Shatter,
}

/// Per-plate simulation result.
#[derive(Clone, Debug)]
pub struct PlateResult {
    pub outcome: PlateOutcome,
    /// Effective thickness after normalization (mm).
    pub effective_thickness_mm: f32,
    /// Shell's raw penetration arriving at this plate (mm).
    pub raw_pen_before_mm: f32,
    /// Shell velocity arriving at this plate (m/s).
    pub velocity_before: f32,
    /// Shell velocity after penetrating this plate (m/s). 0 if stopped.
    pub velocity_after: f32,
    /// Whether this plate armed the fuse.
    pub fuse_armed_here: bool,
}

/// A detonation point in 3D space, tagged with which comparison ship produced it.
#[derive(Clone, Debug)]
pub struct DetonationMarker {
    pub position: [f32; 3],
    pub ship_index: usize,
}

/// Where the AP shell detonates (fuse activation + travel).
#[derive(Clone, Debug)]
pub struct FuseDetonation {
    /// 3D world position of detonation.
    pub position: [f32; 3],
    /// Which hit index armed the fuse.
    pub armed_at_hit: usize,
    /// Distance traveled after arming (in real meters).
    pub travel_distance: f32,
}

/// Complete shell simulation through all hit plates.
#[derive(Clone, Debug)]
pub struct ShellSimResult {
    /// Per-plate results, one for each hit the shell actually reached.
    pub plates: Vec<PlateResult>,
    /// Where the fuse detonates (None if fuse never armed or HE/SAP).
    pub detonation: Option<FuseDetonation>,
    /// Hit index where the shell stopped due to ricochet/shatter/zero velocity (None if not stopped).
    pub stopped_at: Option<usize>,
    /// Hit index of the last plate the shell reached before fuse detonation.
    /// The shell explodes between this hit and the next. Distinct from `stopped_at`.
    pub detonated_at: Option<usize>,
}

/// Simulate a shell passing through a sequence of armor plates.
///
/// Uses formulas from wows_shell (jcw780):
///   raw_pen = p_ppc * velocity^1.38
///   normalized_angle = max(0, angle_from_normal - normalization)
///   effective_thickness = thickness / cos(normalized_angle)
///   post_pen_velocity = velocity * (1 - exp(1 - raw_pen / effective_thickness))
///
/// Fuse detonation is tracked inline: once armed, the shell accumulates travel
/// distance and stops processing further plates when the fuse distance is exceeded.
pub fn simulate_shell_through_hits(
    params: &ShellParams,
    impact: &ImpactResult,
    hits: &[TrajectoryHit],
    shell_dir: &[f32; 3],
) -> ShellSimResult {
    use wowsunpack::game_params::types::Meters;

    let mut velocity = impact.impact_velocity as f32;
    let caliber_mm = (params.caliber * 1000.0) as f32;
    let normalization_rad = params.normalization as f32;
    let ricochet1_rad = params.ricochet1 as f32;
    let fuse_threshold_mm = params.threshold as f32;
    let fuse_time = params.fuse_time as f32;
    let p_ppc = params.p_ppc as f32;

    let mut plates = Vec::with_capacity(hits.len());
    let mut stopped_at: Option<usize> = None;
    let mut detonated_at: Option<usize> = None;

    // Fuse tracking
    let mut fuse_armed = false;
    let mut fuse_arm_velocity: f32 = 0.0;
    let mut fuse_distance_model: f32 = 0.0;
    let mut fuse_accumulated: f32 = 0.0; // distance traveled since arming (model units)
    let mut prev_position: [f32; 3] = [0.0; 3]; // last hit position (for distance accumulation)

    // Precompute shell direction unit vector for detonation fallback
    let dir_len =
        (shell_dir[0] * shell_dir[0] + shell_dir[1] * shell_dir[1] + shell_dir[2] * shell_dir[2]).sqrt().max(1e-9);
    let dir_norm = [shell_dir[0] / dir_len, shell_dir[1] / dir_len, shell_dir[2] / dir_len];

    let mut detonation: Option<FuseDetonation> = None;

    for (i, hit) in hits.iter().enumerate() {
        // If fuse is armed, check if detonation occurs before reaching this plate
        if fuse_armed && detonation.is_none() {
            let seg_dist = distance_3d(&prev_position, &hit.position);
            let remaining = fuse_distance_model - fuse_accumulated;
            if seg_dist >= remaining && remaining > 0.0 {
                // Shell detonates before reaching this plate
                let t = remaining / seg_dist.max(1e-9);
                let det_pos = [
                    prev_position[0] + (hit.position[0] - prev_position[0]) * t,
                    prev_position[1] + (hit.position[1] - prev_position[1]) * t,
                    prev_position[2] + (hit.position[2] - prev_position[2]) * t,
                ];
                let arm_idx = plates.iter().position(|p: &PlateResult| p.fuse_armed_here).unwrap_or(0);
                let fuse_real_m = fuse_arm_velocity * fuse_time;
                detonation =
                    Some(FuseDetonation { position: det_pos, armed_at_hit: arm_idx, travel_distance: fuse_real_m });
                detonated_at = Some(i.saturating_sub(1)); // last plate before detonation
                break;
            }
            fuse_accumulated += seg_dist;
        }

        let raw_pen = p_ppc * velocity.powf(1.38);
        let angle_from_normal_rad = hit.angle_deg.to_radians();
        let is_overmatch = caliber_mm > hit.thickness_mm * 14.3;

        // Check ricochet (only if not overmatch)
        if !is_overmatch && angle_from_normal_rad >= ricochet1_rad {
            plates.push(PlateResult {
                outcome: PlateOutcome::Ricochet,
                effective_thickness_mm: hit.thickness_mm / angle_from_normal_rad.cos().max(0.001),
                raw_pen_before_mm: raw_pen,
                velocity_before: velocity,
                velocity_after: 0.0,
                fuse_armed_here: false,
            });
            stopped_at = Some(i);
            break;
        }

        // Apply normalization
        let norm_angle = if is_overmatch { 0.0 } else { (angle_from_normal_rad - normalization_rad).max(0.0) };
        let effective_thickness = hit.thickness_mm / norm_angle.cos().max(0.001);

        // Check penetration
        if !is_overmatch && raw_pen < effective_thickness {
            plates.push(PlateResult {
                outcome: PlateOutcome::Shatter,
                effective_thickness_mm: effective_thickness,
                raw_pen_before_mm: raw_pen,
                velocity_before: velocity,
                velocity_after: 0.0,
                fuse_armed_here: false,
            });
            stopped_at = Some(i);
            break;
        }

        // Shell penetrates
        let outcome = if is_overmatch { PlateOutcome::Overmatch } else { PlateOutcome::Penetrate };
        let pen_ratio = raw_pen / effective_thickness.max(0.001);
        let post_pen_velocity = velocity * (1.0 - (1.0 - pen_ratio).exp());

        // Check fuse arming
        let armed_here = !fuse_armed && hit.thickness_mm >= fuse_threshold_mm;
        if armed_here {
            fuse_armed = true;

            fuse_arm_velocity = post_pen_velocity;
            let fuse_real_m = post_pen_velocity * fuse_time;
            fuse_distance_model = Meters::from(fuse_real_m).to_bigworld().value();
            fuse_accumulated = 0.0;
        }

        plates.push(PlateResult {
            outcome,
            effective_thickness_mm: effective_thickness,
            raw_pen_before_mm: raw_pen,
            velocity_before: velocity,
            velocity_after: post_pen_velocity,
            fuse_armed_here: armed_here,
        });

        prev_position = hit.position;
        velocity = post_pen_velocity;

        if velocity < 1.0 {
            stopped_at = Some(i);
            break;
        }
    }

    // If fuse armed but detonation didn't happen between hits, compute where it detonates.
    if fuse_armed && detonation.is_none() {
        let remaining = fuse_distance_model - fuse_accumulated;
        let det_pos = [
            prev_position[0] + dir_norm[0] * remaining.max(0.0),
            prev_position[1] + dir_norm[1] * remaining.max(0.0),
            prev_position[2] + dir_norm[2] * remaining.max(0.0),
        ];
        let arm_idx = plates.iter().position(|p| p.fuse_armed_here).unwrap_or(0);
        let fuse_real_m = fuse_arm_velocity * fuse_time;
        detonation = Some(FuseDetonation { position: det_pos, armed_at_hit: arm_idx, travel_distance: fuse_real_m });

        if stopped_at.is_some() {
            // Shell stopped (ricochet/shatter) but fuse was armed — it still detonates.
            // Mark the stop plate as the detonation plate so the outcome shows as detonation.
            detonated_at = stopped_at;
        }
        // else: shell exited before detonating — overpen with armed fuse (detonated_at stays None)
    }

    ShellSimResult { plates, detonation, stopped_at, detonated_at }
}

/// Metadata for a stored trajectory (non-simulation display data).
#[derive(Clone, Debug)]
pub struct TrajectoryMeta {
    /// Unique monotonically increasing ID for stable UI references.
    pub id: u64,
    /// Index into the trajectory color palette.
    pub color_index: usize,
    /// Per-trajectory ballistic range (km).
    pub range_km: f32,
    /// When true, this trajectory's range follows the shared slider.
    pub range_locked: bool,
}

/// Euclidean distance between two 3D points.
fn distance_3d(a: &[f32; 3], b: &[f32; 3]) -> f32 {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let dz = b[2] - a[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}
