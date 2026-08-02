//! Shell ballistics: trajectory simulation, penetration, and the per-plate
//! armor interaction chain.
//!
//! Trajectory physics match the game client (see `docs/BALLISTICS.md`); the
//! penetration formulas are the community-derived ones from wows_shell and
//! cannot be verified against the client binary (armor interaction is
//! server-side).
//!
//! Formulas ported from <https://github.com/jcw780/wows_shell>
//! by jcw780, licensed under the MIT License.
//! Copyright (c) 2020 jcw780

use std::f64::consts::PI;

use crate::game_params::types::Meters;
use crate::game_params::types::ShellInfo;
use crate::game_params::types::ShipModelDistance;

// Physical constants (ISA atmospheric model)
const G: f64 = 9.8; // gravitational acceleration (m/s^2)
const T0: f64 = 288.15; // sea-level temperature (K)
const L: f64 = 0.0065; // temperature lapse rate (K/m)
const P0: f64 = 101325.0; // sea-level pressure (Pa)
const R_GAS: f64 = 8.31447; // ideal gas constant (J/(mol*K))
const M_AIR: f64 = 0.0289644; // molar mass of air (kg/mol)

// Derived constant for barometric formula exponent: (g * M) / (R * L)
const GM_RL: f64 = (G * M_AIR) / (R_GAS * L);

// Game-specific constants
// Shell flight-time multiplier (jcw780's calibrated value), used only to convert
// real flight seconds into in-game time-to-target. Distinct from the game's ship
// time scale (SHIP_TIME_SCALE) and not interchangeable with it.
const TIME_MULTIPLIER: f64 = 2.75;
const VELOCITY_POWER: f64 = 1.38; // 2 * 0.69, penetration velocity exponent

// Simulation parameters
const DT: f64 = 0.02; // time step (seconds)
const MAX_TIME: f64 = 200.0; // max simulation time (seconds)
const BISECT_TOLERANCE_M: f64 = 1.0; // range solver tolerance (meters)
const BISECT_MAX_ITER: u32 = 60; // max bisection iterations

/// Preprocessed shell parameters for ballistic simulation.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct ShellParams {
    pub caliber: f64,
    pub mass: f64,
    pub v0: f64,
    pub krupp: f64,
    pub cd: f64,
    pub normalization: f64, // radians
    pub ricochet0: f64,     // radians
    pub ricochet1: f64,     // radians
    pub fuse_time: f64,
    pub threshold: f64, // mm
    /// Combined air drag coefficient: 0.5 * cD * (caliber/2)^2 * pi / mass
    pub k: f64,
    /// Combined penetration coefficient: 1e-7 * krupp * mass^0.69 * caliber^(-1.07)
    pub p_ppc: f64,
    /// Whether the shell is capped. Uncapped shells receive no normalization.
    pub cap: bool,
}

impl ShellParams {
    /// Build ballistic parameters from a shell.
    ///
    /// Returns `None` (with a logged reason) when the shell lacks data that has no
    /// safe default: `normalization` or `fuse_threshold`. Substituting 0.0 for
    /// either would silently change penetration outcomes, so we skip the shell
    /// rather than guess. Every real gun shell in GameParams carries both fields.
    pub fn from_shell_info(shell: &ShellInfo) -> Option<Self> {
        let caliber = shell.caliber.value() as f64 / 1000.0; // mm -> m
        let mass = shell.mass_kg as f64;
        let v0 = shell.muzzle_velocity as f64;
        let krupp = shell.krupp as f64;
        let cd = shell.air_drag as f64;
        let Some(normalization_deg) = shell.normalization else {
            tracing::warn!("shell '{}' has no normalization angle; skipping ballistic sim", shell.name);
            return None;
        };
        let Some(threshold_mm) = shell.fuse_threshold else {
            tracing::warn!("shell '{}' has no fuse threshold; skipping ballistic sim", shell.name);
            return None;
        };
        let normalization = (normalization_deg as f64).to_radians();
        let ricochet0 = (shell.ricochet_angle as f64).to_radians();
        let ricochet1 = (shell.always_ricochet_angle as f64).to_radians();
        let fuse_time = shell.fuse_time as f64;
        let threshold = threshold_mm as f64;

        let r = caliber / 2.0;
        let k = 0.5 * cd * r * r * PI / mass;
        let p_ppc = 1e-7 * krupp * mass.powf(0.69) * caliber.powf(-1.07);

        Some(ShellParams {
            caliber,
            mass,
            v0,
            krupp,
            cd,
            normalization,
            ricochet0,
            ricochet1,
            fuse_time,
            threshold,
            k,
            p_ppc,
            cap: shell.cap,
        })
    }
}

/// Result of a trajectory simulation at impact.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct ImpactResult {
    /// Horizontal range (m)
    pub distance: f64,
    /// Impact velocity magnitude (m/s)
    pub impact_velocity: f64,
    /// Impact angle from horizontal (radians, positive = falling)
    pub impact_angle_horizontal: f64,
    /// Impact angle from deck plane (radians)
    pub impact_angle_deck: f64,
    /// Time to target in game seconds (real_time / TIME_MULTIPLIER)
    pub time_to_target: f64,
    /// Raw penetration (mm): pPPC * IV^1.38
    pub raw_pen_mm: f64,
    /// Effective belt penetration (mm): raw * cos(horizontal_angle)
    pub effective_pen_belt_mm: f64,
    /// Effective belt penetration with normalization (mm)
    pub effective_pen_belt_normalized_mm: f64,
    /// Effective deck penetration (mm): raw * cos(deck_angle)
    pub effective_pen_deck_mm: f64,
    /// Effective deck penetration with normalization (mm)
    pub effective_pen_deck_normalized_mm: f64,
    /// Launch angle used (radians)
    pub launch_angle: f64,
}

/// Compute air density at a given altitude using ISA atmospheric model.
fn air_density(altitude: f64) -> f64 {
    let t = T0 - L * altitude;
    if t <= 0.0 {
        return 0.0;
    }
    let p = P0 * (t / T0).powf(GM_RL);
    (M_AIR * p) / (R_GAS * t)
}

/// Compute acceleration components given current state.
/// Returns (ax, ay) where:
///   ax = -k * rho * vx * speed
///   ay = -g - k * rho * vy * speed
fn acceleration(k: f64, vx: f64, vy: f64, y: f64) -> (f64, f64) {
    let rho = air_density(y);
    let speed = (vx * vx + vy * vy).sqrt();
    let k_rho = k * rho;
    let ax = -k_rho * vx * speed;
    let ay = -G - k_rho * vy * speed;
    (ax, ay)
}

/// Simulate a shell trajectory using RK4 integration.
/// Returns (final_x, final_vx, final_vy, final_time) at the point the shell returns to y=0.
/// Returns None if the shell never comes back down within MAX_TIME.
fn simulate_trajectory(params: &ShellParams, launch_angle: f64) -> Option<(f64, f64, f64, f64)> {
    let mut x: f64 = 0.0;
    let mut y: f64 = 0.0;
    let mut vx = params.v0 * launch_angle.cos();
    let mut vy = params.v0 * launch_angle.sin();
    let mut t: f64 = 0.0;

    let k = params.k;

    while t < MAX_TIME {
        // RK4 integration
        let (ax1, ay1) = acceleration(k, vx, vy, y);

        let vx2 = vx + ax1 * DT * 0.5;
        let vy2 = vy + ay1 * DT * 0.5;
        let y2 = y + vy * DT * 0.5;
        let (ax2, ay2) = acceleration(k, vx2, vy2, y2);

        let vx3 = vx + ax2 * DT * 0.5;
        let vy3 = vy + ay2 * DT * 0.5;
        let y3 = y + vy2 * DT * 0.5;
        let (ax3, ay3) = acceleration(k, vx3, vy3, y3);

        let vx4 = vx + ax3 * DT;
        let vy4 = vy + ay3 * DT;
        let y4 = y + vy3 * DT;
        let (ax4, ay4) = acceleration(k, vx4, vy4, y4);

        let dx = (vx + 2.0 * vx2 + 2.0 * vx3 + vx4) / 6.0 * DT;
        let dy = (vy + 2.0 * vy2 + 2.0 * vy3 + vy4) / 6.0 * DT;
        let dvx = (ax1 + 2.0 * ax2 + 2.0 * ax3 + ax4) / 6.0 * DT;
        let dvy = (ay1 + 2.0 * ay2 + 2.0 * ay3 + ay4) / 6.0 * DT;

        let new_y = y + dy;

        // Check for ground crossing (shell descending past y=0)
        if new_y < 0.0 && t > DT {
            // Linear interpolation to find exact ground crossing
            let frac = y / (y - new_y);
            let final_x = x + dx * frac;
            let final_vx = vx + dvx * frac;
            let final_vy = vy + dvy * frac;
            let final_t = t + DT * frac;
            return Some((final_x, final_vx, final_vy, final_t));
        }

        x += dx;
        y = new_y;
        vx += dvx;
        vy += dvy;
        t += DT;
    }

    None
}

/// Compute the normalization reduction: if |angle| > normalization, reduce by normalization.
fn calc_normalization(angle: f64, normalization: f64) -> f64 {
    if angle.abs() > normalization { angle.abs() - normalization } else { 0.0 }
}

/// Build an ImpactResult from simulation output.
fn build_impact_result(
    params: &ShellParams,
    distance: f64,
    vx: f64,
    vy: f64,
    time: f64,
    launch_angle: f64,
) -> ImpactResult {
    let impact_velocity = (vx * vx + vy * vy).sqrt();

    // Impact angle from horizontal (positive = falling, vy is negative when descending)
    let ia_horizontal = (vy / vx).atan().abs();
    // Impact angle from deck = pi/2 - horizontal angle
    let ia_deck = PI / 2.0 - ia_horizontal;

    let raw_pen = params.p_ppc * impact_velocity.powf(VELOCITY_POWER);

    // Belt penetration: shell hitting a vertical surface
    let eff_belt = raw_pen * ia_horizontal.cos();
    let eff_belt_norm = raw_pen * calc_normalization(ia_horizontal, params.normalization).cos();

    // Deck penetration: shell hitting a horizontal surface
    let eff_deck = raw_pen * ia_deck.cos();
    let eff_deck_norm = raw_pen * calc_normalization(ia_deck, params.normalization).cos();

    ImpactResult {
        distance,
        impact_velocity,
        impact_angle_horizontal: ia_horizontal,
        impact_angle_deck: ia_deck,
        time_to_target: time / TIME_MULTIPLIER,
        raw_pen_mm: raw_pen,
        effective_pen_belt_mm: eff_belt,
        effective_pen_belt_normalized_mm: eff_belt_norm,
        effective_pen_deck_mm: eff_deck,
        effective_pen_deck_normalized_mm: eff_deck_norm,
        launch_angle,
    }
}

/// Find the maximum range of the shell.
fn max_range(params: &ShellParams) -> Option<f64> {
    let mut best_range = 0.0f64;
    // Scan from 5 to 60 deg in 1 deg steps; high drag shells peak below 30 deg
    for deg in 5..=60 {
        let angle = (deg as f64).to_radians();
        if let Some((dist, _, _, _)) = simulate_trajectory(params, angle)
            && dist > best_range
        {
            best_range = dist;
        }
    }
    if best_range > 0.0 { Some(best_range) } else { None }
}

/// Solve for the launch angle that produces a given horizontal range.
/// Uses bisection on the low-angle (flat) trajectory.
/// Returns None if the range exceeds the shell's maximum range.
pub fn solve_for_range(params: &ShellParams, range: Meters) -> Option<ImpactResult> {
    let range_m = range.value() as f64;
    if range_m <= 0.0 {
        // At zero range, return muzzle velocity impact
        return Some(build_impact_result(params, 0.0, params.v0, 0.0, 0.0, 0.0));
    }

    // Check max range first
    let max_r = max_range(params)?;
    if range_m > max_r {
        return None;
    }

    // Bisection: find angle in [low, high] where simulated range ~= target range
    let mut low: f64 = 0.001_f64.to_radians(); // near 0 deg
    let mut high: f64 = 45.0_f64.to_radians(); // up to 45 deg

    let mut best_result: Option<(f64, f64, f64, f64, f64)> = None; // (angle, x, vx, vy, t)

    for _ in 0..BISECT_MAX_ITER {
        let mid = (low + high) / 2.0;
        if let Some((dist, vx, vy, t)) = simulate_trajectory(params, mid) {
            let err = dist - range_m;
            if err.abs() < BISECT_TOLERANCE_M {
                return Some(build_impact_result(params, dist, vx, vy, t, mid));
            }
            best_result = Some((mid, dist, vx, vy, t));
            if err > 0.0 {
                // Overshot: reduce angle
                high = mid;
            } else {
                // Undershot: increase angle
                low = mid;
            }
        } else {
            // Didn't land; reduce angle
            high = mid;
        }
    }

    // Return best result if we have one
    best_result.map(|(angle, dist, vx, vy, t)| build_impact_result(params, dist, vx, vy, t, angle))
}

/// Compute impact data at regular range intervals.
#[allow(dead_code)]
pub fn compute_range_table(params: &ShellParams, max_range: Meters, step: Meters) -> Vec<ImpactResult> {
    let mut results = Vec::new();
    let mut range = step;
    while range <= max_range {
        if let Some(impact) = solve_for_range(params, range) {
            results.push(impact);
        } else {
            break; // Exceeded max range
        }
        range = range + step;
    }
    results
}

/// Simulate a trajectory and return normalized arc points for visualization.
///
/// Returns `(points, height_ratio)` where:
/// - `points`: list of `(x_frac, y_norm)`: x goes 0->1, y goes 0->1 at apex
/// - `height_ratio`: `max_height / total_range`: the real aspect ratio of the arc
///
/// The caller should scale: `y_model = y_norm * height_ratio * horiz_extent`
/// to get physically correct proportions, or apply an additional visual multiplier.
pub fn simulate_arc_points(params: &ShellParams, launch_angle: f64, num_points: usize) -> (Vec<(f64, f64)>, f64) {
    // First pass: collect all raw (x, y) points
    let mut raw_points: Vec<(f64, f64)> = Vec::new();
    let mut x: f64 = 0.0;
    let mut y: f64 = 0.0;
    let mut vx = params.v0 * launch_angle.cos();
    let mut vy = params.v0 * launch_angle.sin();
    let mut t: f64 = 0.0;
    let k = params.k;

    raw_points.push((0.0, 0.0));

    while t < MAX_TIME {
        let (ax1, ay1) = acceleration(k, vx, vy, y);
        let vx2 = vx + ax1 * DT * 0.5;
        let vy2 = vy + ay1 * DT * 0.5;
        let y2 = y + vy * DT * 0.5;
        let (ax2, ay2) = acceleration(k, vx2, vy2, y2);
        let vx3 = vx + ax2 * DT * 0.5;
        let vy3 = vy + ay2 * DT * 0.5;
        let y3 = y + vy2 * DT * 0.5;
        let (ax3, ay3) = acceleration(k, vx3, vy3, y3);
        let vx4 = vx + ax3 * DT;
        let vy4 = vy + ay3 * DT;
        let (ax4, ay4) = acceleration(k, vx4, vy4, y + vy3 * DT);

        let dx = (vx + 2.0 * vx2 + 2.0 * vx3 + vx4) / 6.0 * DT;
        let dy = (vy + 2.0 * vy2 + 2.0 * vy3 + vy4) / 6.0 * DT;
        let dvx = (ax1 + 2.0 * ax2 + 2.0 * ax3 + ax4) / 6.0 * DT;
        let dvy = (ay1 + 2.0 * ay2 + 2.0 * ay3 + ay4) / 6.0 * DT;

        let new_y = y + dy;

        if new_y < 0.0 && t > DT {
            // Interpolate to ground
            let frac = y / (y - new_y);
            raw_points.push((x + dx * frac, 0.0));
            break;
        }

        x += dx;
        y = new_y;
        vx += dvx;
        vy += dvy;
        t += DT;

        raw_points.push((x, y));
    }

    if raw_points.len() < 2 {
        return (vec![(0.0, 0.0), (1.0, 0.0)], 0.0);
    }

    let total_x = raw_points.last().unwrap().0;
    if total_x <= 0.0 {
        return (vec![(0.0, 0.0), (1.0, 0.0)], 0.0);
    }

    let max_y = raw_points.iter().map(|(_, py)| *py).fold(0.0f64, f64::max);
    let height_ratio = max_y / total_x;
    if max_y <= 0.0 {
        return (vec![(0.0, 0.0), (1.0, 0.0)], 0.0);
    }

    // Normalize: x_frac = x/total_x (0->1), y_norm = y/max_height (0->1 at apex)
    let normalized: Vec<(f64, f64)> = raw_points.iter().map(|(px, py)| (px / total_x, py / max_y)).collect();

    // Downsample to num_points evenly spaced along x_frac
    if num_points <= 2 || normalized.len() <= num_points {
        return (normalized, height_ratio);
    }

    let mut result = Vec::with_capacity(num_points);
    result.push(normalized[0]);

    for i in 1..num_points - 1 {
        let target_x = i as f64 / (num_points - 1) as f64;
        // Binary search for the segment containing target_x
        let idx = normalized.partition_point(|(nx, _)| *nx < target_x).min(normalized.len() - 1).max(1);
        let (x0, y0) = normalized[idx - 1];
        let (x1, y1) = normalized[idx];
        let frac = if (x1 - x0).abs() > 1e-12 { (target_x - x0) / (x1 - x0) } else { 0.0 };
        result.push((target_x, y0 + frac * (y1 - y0)));
    }

    result.push(*normalized.last().unwrap());
    (result, height_ratio)
}

// Per-plate armor interaction chain

/// AP overmatch ratio: a plate is overmatched when `caliber_mm > thickness_mm * OVERMATCH_RATIO`.
/// This is the community-established value; the real check is engine-side and is not
/// present in GameParams, so it cannot be data-validated and must be kept in sync by hand.
pub const OVERMATCH_RATIO: f32 = 14.3;

/// One armor plate along a shell's ray, in strike order.
#[derive(Clone, Debug)]
pub struct PlateHit {
    pub thickness_mm: f32,
    /// Impact angle from the plate normal in degrees (0 = head-on, 90 = glancing).
    pub angle_deg: f32,
    /// Distance from the first hit along the ray.
    pub distance_along_ray: ShipModelDistance,
}

/// Outcome of a shell hitting a single plate.
#[derive(Clone, Debug, PartialEq)]
pub enum PlateOutcome {
    /// Caliber > OVERMATCH_RATIO * thickness: always penetrates, ignores ricochet.
    Overmatch,
    /// Shell penetrates (raw_pen >= effective_thickness).
    Penetrate,
    /// Angle >= always_ricochet: guaranteed ricochet, shell stopped.
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

/// Where the AP shell detonates (fuse activation + travel).
#[derive(Clone, Debug)]
pub struct FuseDetonation {
    /// Detonation point measured from the first hit along the ray.
    pub distance_along_ray: ShipModelDistance,
    /// Which hit index armed the fuse.
    pub armed_at_hit: usize,
    /// Distance traveled after arming.
    pub travel_distance: Meters,
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

/// Simulate a shell passing through a sequence of armor plates along one ray.
///
/// Uses formulas from wows_shell (jcw780):
///   raw_pen = p_ppc * velocity^1.38
///   normalized_angle = max(0, angle_from_normal - normalization)
///   effective_thickness = thickness / cos(normalized_angle)
///   post_pen_velocity = velocity * (1 - exp(1 - raw_pen / effective_thickness))
///
/// Fuse detonation is tracked inline: once armed, the shell accumulates travel
/// distance and stops processing further plates when the fuse distance is exceeded.
pub fn simulate_shell_through_plates(
    params: &ShellParams,
    impact: &ImpactResult,
    hits: &[PlateHit],
    continue_on_ricochet: bool,
) -> ShellSimResult {
    let mut velocity = impact.impact_velocity as f32;
    let caliber_mm = (params.caliber * 1000.0) as f32;
    // Uncapped shells (bulletCap == false) receive no normalization.
    let normalization_rad = if params.cap { params.normalization as f32 } else { 0.0 };
    let ricochet1_rad = params.ricochet1 as f32;
    let fuse_threshold_mm = params.threshold as f32;
    let fuse_time = params.fuse_time as f32;
    let p_ppc = params.p_ppc as f32;

    let mut plates = Vec::with_capacity(hits.len());
    let mut stopped_at: Option<usize> = None;
    let mut detonated_at: Option<usize> = None;

    // Fuse tracking. Distances are along the ray in ship-model units.
    let mut fuse_armed = false;
    let mut fuse_arm_velocity: f32 = 0.0;
    let mut fuse_distance_model: f32 = 0.0;
    let mut fuse_accumulated: f32 = 0.0; // distance traveled since arming
    let mut prev_dist: f32 = 0.0; // last processed plate's distance_along_ray

    let mut detonation: Option<FuseDetonation> = None;

    for (i, hit) in hits.iter().enumerate() {
        let hit_dist = hit.distance_along_ray.value();

        // If fuse is armed, check if detonation occurs before reaching this plate
        if fuse_armed && detonation.is_none() {
            let seg_dist = hit_dist - prev_dist;
            let remaining = fuse_distance_model - fuse_accumulated;
            if seg_dist >= remaining && remaining > 0.0 {
                let arm_idx = plates.iter().position(|p: &PlateResult| p.fuse_armed_here).unwrap_or(0);
                detonation = Some(FuseDetonation {
                    distance_along_ray: ShipModelDistance::from(prev_dist + remaining),
                    armed_at_hit: arm_idx,
                    travel_distance: Meters::from(fuse_arm_velocity * fuse_time),
                });
                detonated_at = Some(i.saturating_sub(1)); // last plate before detonation
                break;
            }
            fuse_accumulated += seg_dist;
        }

        let raw_pen = p_ppc * velocity.powf(1.38);
        let angle_from_normal_rad = hit.angle_deg.to_radians();
        let is_overmatch = caliber_mm > hit.thickness_mm * OVERMATCH_RATIO;

        // Check ricochet (only if not overmatch)
        if !is_overmatch && angle_from_normal_rad >= ricochet1_rad {
            plates.push(PlateResult {
                outcome: PlateOutcome::Ricochet,
                effective_thickness_mm: hit.thickness_mm / angle_from_normal_rad.cos().max(0.001),
                raw_pen_before_mm: raw_pen,
                velocity_before: velocity,
                velocity_after: if continue_on_ricochet { velocity } else { 0.0 },
                fuse_armed_here: false,
            });
            if !continue_on_ricochet {
                stopped_at = Some(i);
                break;
            }
            // continue_on_ricochet: plate recorded as ricochet, shell continues with unchanged velocity
            prev_dist = hit_dist;
            continue;
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
            // Armor meshes are in ship-model space (15 m per unit); converting at
            // the 30 m BigWorld scale halves fuse travel and detonates shells
            // short of the citadel (issue #43).
            fuse_distance_model = Meters::from(fuse_real_m).to_ship_model().value();
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

        prev_dist = hit_dist;
        velocity = post_pen_velocity;

        if velocity < 1.0 {
            stopped_at = Some(i);
            break;
        }
    }

    // If fuse armed but detonation didn't happen between hits, compute where it detonates.
    if fuse_armed && detonation.is_none() {
        let remaining = fuse_distance_model - fuse_accumulated;
        let arm_idx = plates.iter().position(|p| p.fuse_armed_here).unwrap_or(0);
        detonation = Some(FuseDetonation {
            distance_along_ray: ShipModelDistance::from(prev_dist + remaining.max(0.0)),
            armed_at_hit: arm_idx,
            travel_distance: Meters::from(fuse_arm_velocity * fuse_time),
        });

        if stopped_at.is_some() {
            // Shell stopped (ricochet/shatter) but fuse was armed; it still detonates.
            // Mark the stop plate as the detonation plate so the outcome shows as detonation.
            detonated_at = stopped_at;
        }
        // else: shell exited before detonating, overpen with armed fuse (detonated_at stays None)
    }

    ShellSimResult { plates, detonation, stopped_at, detonated_at }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Colombo 381mm AP (PIPA045_381MM_50_AP) from GameParams.
    fn colombo_ap() -> ShellParams {
        let caliber = 0.381;
        let mass = 884.8;
        let cd = 0.2954;
        let krupp = 2434.0;
        let r: f64 = caliber / 2.0;
        ShellParams {
            caliber,
            mass,
            v0: 850.0,
            krupp,
            cd,
            normalization: 6.0_f64.to_radians(),
            ricochet0: 45.0_f64.to_radians(),
            ricochet1: 60.0_f64.to_radians(),
            fuse_time: 0.033,
            threshold: 64.0,
            k: 0.5 * cd * r * r * PI / mass,
            p_ppc: 1e-7 * krupp * mass.powf(0.69) * caliber.powf(-1.07),
            cap: true,
        }
    }

    fn impact_at(velocity: f64) -> ImpactResult {
        ImpactResult {
            distance: 8500.0,
            impact_velocity: velocity,
            impact_angle_horizontal: 4.3_f64.to_radians(),
            impact_angle_deck: PI / 2.0 - 4.3_f64.to_radians(),
            time_to_target: 0.0,
            raw_pen_mm: 0.0,
            effective_pen_belt_mm: 0.0,
            effective_pen_belt_normalized_mm: 0.0,
            effective_pen_deck_mm: 0.0,
            effective_pen_deck_normalized_mm: 0.0,
            launch_angle: 0.0,
        }
    }

    fn plate(dist: f32, thickness_mm: f32) -> PlateHit {
        PlateHit { thickness_mm, angle_deg: 24.7, distance_along_ray: ShipModelDistance::from(dist) }
    }

    /// Regression test for issue #43 (Colombo vs Ushakov citadel range).
    ///
    /// Armor mesh space is 15 m per unit (ShipModelDistance), not the 30 m
    /// GameParams BigWorld scale. Converting the fuse travel distance at 30
    /// halves it in mesh space, detonating shells short of the citadel.
    ///
    /// Numbers from the issue screenshot at 8.5 km: v=699 m/s into a 425 mm
    /// belt at 24.7 deg arms the fuse and exits at ~224 m/s, so the shell
    /// travels 224 * 0.033 = 7.4 real meters = 0.49 mesh units before
    /// detonating. A plate 0.42 units (6.3 m) behind the belt must be reached
    /// and penetrated before the detonation point.
    #[test]
    fn fuse_travel_uses_ship_model_scale() {
        let params = colombo_ap();
        let impact = impact_at(699.0);
        let hits = vec![plate(0.0, 425.0), plate(0.42, 40.0), plate(0.6, 375.0)];

        let sim = simulate_shell_through_plates(&params, &impact, &hits, false);

        let det = sim.detonation.as_ref().expect("fuse armed on the belt, shell must detonate");
        let travel_m = det.travel_distance.value();
        assert!((travel_m - 7.4).abs() < 0.2, "fuse travel {travel_m} m, expected ~7.4 m");
        assert_eq!(sim.plates.len(), 2, "shell must reach and penetrate the plate 6.3 m behind the belt");
        assert_eq!(sim.plates[1].outcome, PlateOutcome::Penetrate);
        assert_eq!(sim.detonated_at, Some(1), "detonation happens between the second and third plates");
        let expected = det.travel_distance.to_ship_model().value();
        let got = det.distance_along_ray.value();
        assert!((got - expected).abs() < 0.02, "detonation at {got} units along ray, expected ~{expected}");
    }
}
