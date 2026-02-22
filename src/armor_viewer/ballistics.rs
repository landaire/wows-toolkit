//! Shell ballistics simulation for World of Warships.
//!
//! Formulas ported from [wows_shell](https://github.com/jcw780/wows_shell)
//! by jcw780, licensed under the MIT License.
//! Copyright (c) 2020 jcw780

use std::f64::consts::PI;

use wowsunpack::game_params::types::{Meters, ShellInfo};

// Physical constants (ISA atmospheric model)
const G: f64 = 9.8; // gravitational acceleration (m/s²)
const T0: f64 = 288.15; // sea-level temperature (K)
const L: f64 = 0.0065; // temperature lapse rate (K/m)
const P0: f64 = 101325.0; // sea-level pressure (Pa)
const R_GAS: f64 = 8.31447; // ideal gas constant (J/(mol·K))
const M_AIR: f64 = 0.0289644; // molar mass of air (kg/mol)

// Derived constant for barometric formula exponent: (g * M) / (R * L)
const GM_RL: f64 = (G * M_AIR) / (R_GAS * L);

// Game-specific constants
const TIME_MULTIPLIER: f64 = 2.75; // shell time multiplier
const VELOCITY_POWER: f64 = 1.38; // 2 * 0.69, penetration velocity exponent

// Simulation parameters
const DT: f64 = 0.02; // time step (seconds)
const MAX_TIME: f64 = 200.0; // max simulation time (seconds)
const BISECT_TOLERANCE_M: f64 = 1.0; // range solver tolerance (meters)
const BISECT_MAX_ITER: u32 = 60; // max bisection iterations

/// Preprocessed shell parameters for ballistic simulation.
#[derive(Clone, Debug)]
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
    /// Combined air drag coefficient: 0.5 * cD * (caliber/2)² * π / mass
    pub k: f64,
    /// Combined penetration coefficient: 1e-7 * krupp * mass^0.69 * caliber^(-1.07)
    pub p_ppc: f64,
}

impl ShellParams {
    pub fn from_shell_info(shell: &ShellInfo) -> Self {
        let caliber = shell.caliber.value() as f64 / 1000.0; // mm -> m
        let mass = shell.mass_kg as f64;
        let v0 = shell.muzzle_velocity as f64;
        let krupp = shell.krupp as f64;
        let cd = shell.air_drag as f64;
        let normalization = (shell.normalization as f64).to_radians();
        let ricochet0 = (shell.ricochet_angle as f64).to_radians();
        let ricochet1 = (shell.always_ricochet_angle as f64).to_radians();
        let fuse_time = shell.fuse_time as f64;
        let threshold = shell.fuse_threshold as f64;

        let r = caliber / 2.0;
        let k = 0.5 * cd * r * r * PI / mass;
        let p_ppc = 1e-7 * krupp * mass.powf(0.69) * caliber.powf(-1.07);

        ShellParams {
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
        }
    }
}

/// Result of a trajectory simulation at impact.
#[derive(Clone, Debug)]
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

impl ImpactResult {
    /// Construct an ImpactResult from actual terminal velocity components (m/s).
    ///
    /// Used when server-authoritative data (e.g. from `TerminalBallisticsInfo`) is
    /// available, bypassing trajectory simulation entirely.
    /// `vx` and `vz` are horizontal velocity components, `vy` is vertical (negative = falling).
    pub fn from_terminal_velocity(params: &ShellParams, vx: f64, vy: f64, vz: f64) -> Self {
        let vx_horiz = (vx * vx + vz * vz).sqrt();
        let impact_velocity = (vx_horiz * vx_horiz + vy * vy).sqrt();

        // Impact angle from horizontal (positive = falling, vy negative when descending)
        let ia_horizontal = if vx_horiz > 0.001 { (vy / vx_horiz).atan().abs() } else { PI / 2.0 };
        let ia_deck = PI / 2.0 - ia_horizontal;

        let raw_pen = params.p_ppc * impact_velocity.powf(VELOCITY_POWER);
        let eff_belt = raw_pen * ia_horizontal.cos();
        let eff_belt_norm = raw_pen * calc_normalization(ia_horizontal, params.normalization).cos();
        let eff_deck = raw_pen * ia_deck.cos();
        let eff_deck_norm = raw_pen * calc_normalization(ia_deck, params.normalization).cos();

        // Estimate launch angle from impact angle (rough; only used for arc drawing)
        let launch_angle = ia_horizontal * 0.6;

        ImpactResult {
            distance: 0.0,
            impact_velocity,
            impact_angle_horizontal: ia_horizontal,
            impact_angle_deck: ia_deck,
            time_to_target: 0.0,
            raw_pen_mm: raw_pen,
            effective_pen_belt_mm: eff_belt,
            effective_pen_belt_normalized_mm: eff_belt_norm,
            effective_pen_deck_mm: eff_deck,
            effective_pen_deck_normalized_mm: eff_deck_norm,
            launch_angle,
        }
    }
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
    // Impact angle from deck = π/2 - horizontal angle
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
    // Scan from 5° to 60° in 1° steps — high drag shells peak below 30°
    for deg in 5..=60 {
        let angle = (deg as f64).to_radians();
        if let Some((dist, _, _, _)) = simulate_trajectory(params, angle) {
            if dist > best_range {
                best_range = dist;
            }
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

    // Bisection: find angle in [low, high] where simulated range ≈ target range
    let mut low: f64 = 0.001_f64.to_radians(); // near 0°
    let mut high: f64 = 45.0_f64.to_radians(); // up to 45°

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
            // Didn't land — reduce angle
            high = mid;
        }
    }

    // Return best result if we have one
    best_result.map(|(angle, dist, vx, vy, t)| build_impact_result(params, dist, vx, vy, t, angle))
}

/// Compute impact data at regular range intervals.
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
/// - `points`: list of `(x_frac, y_norm)` — x goes 0→1, y goes 0→1 at apex
/// - `height_ratio`: `max_height / total_range` — the real aspect ratio of the arc
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

    // Normalize: x_frac = x/total_x (0→1), y_norm = y/max_height (0→1 at apex)
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
