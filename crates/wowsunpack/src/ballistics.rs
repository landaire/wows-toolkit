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

use crate::game_params::types::Degrees;
use crate::game_params::types::Kilograms;
use crate::game_params::types::Meters;
use crate::game_params::types::MetersPerSecond;
use crate::game_params::types::Millimeters;
use crate::game_params::types::Radians;
use crate::game_params::types::Seconds;
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

/// Floor on the cosine of a strike angle, so a plate struck edge-on presents a
/// large but finite effective thickness instead of an infinite one.
const MIN_STRIKE_COSINE: f32 = 0.001;

/// Floor on effective thickness when dividing penetration by it.
const MIN_EFFECTIVE_THICKNESS: Millimeters = Millimeters::new(0.001);

/// Below this the shell has spent itself and stops in the plate it just crossed.
const MIN_CARRY_VELOCITY: MetersPerSecond = MetersPerSecond::new(1.0);

/// Combined air-drag coefficient `0.5 * air_drag * (caliber / 2)^2 * pi / mass`.
/// Meaningful only to the trajectory integrator; a distinct type so it cannot be
/// swapped with [`PenetrationFactor`].
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct DragFactor(f64);

/// Combined penetration coefficient `1e-7 * krupp * mass^0.69 * caliber^-1.07`,
/// which multiplied by `velocity^VELOCITY_POWER` gives millimeters of armor.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct PenetrationFactor(f64);

impl DragFactor {
    pub fn value(self) -> f64 {
        self.0
    }
}

impl PenetrationFactor {
    pub fn value(self) -> f64 {
        self.0
    }
}

/// Preprocessed shell parameters for ballistic simulation.
#[derive(Clone, Debug)]
pub struct ShellParams {
    pub caliber: Millimeters,
    pub mass: Kilograms,
    pub muzzle_velocity: MetersPerSecond,
    pub krupp: f32,
    pub air_drag: f32,
    /// Angle by which a shell's cap straightens out an angled strike.
    pub normalization: Radians,
    /// Strike angle from the plate normal past which a ricochet becomes possible.
    pub ricochet_angle: Radians,
    /// Strike angle from the plate normal past which a ricochet is certain.
    pub always_ricochet_angle: Radians,
    pub fuse_time: Seconds,
    /// Plate thickness that arms the fuse.
    pub fuse_threshold: Millimeters,
    pub drag_factor: DragFactor,
    pub penetration_factor: PenetrationFactor,
    /// Whether the shell is capped. Uncapped shells receive no normalization.
    pub capped: bool,
}

impl ShellParams {
    /// Build ballistic parameters from a shell.
    ///
    /// Returns `None` (with a logged reason) when the shell lacks data that has no
    /// safe default: `normalization` or `fuse_threshold`. Substituting 0.0 for
    /// either would silently change penetration outcomes, so we skip the shell
    /// rather than guess. Every real gun shell in GameParams carries both fields.
    pub fn from_shell_info(shell: &ShellInfo) -> Option<Self> {
        let Some(normalization_deg) = shell.normalization else {
            tracing::warn!("shell '{}' has no normalization angle; skipping ballistic sim", shell.name);
            return None;
        };
        let Some(fuse_threshold_mm) = shell.fuse_threshold else {
            tracing::warn!("shell '{}' has no fuse threshold; skipping ballistic sim", shell.name);
            return None;
        };

        let caliber_m = f64::from(shell.caliber.to_meters().value());
        let mass_kg = f64::from(shell.mass_kg);
        let radius_m = caliber_m / 2.0;

        Some(ShellParams {
            caliber: shell.caliber,
            mass: Kilograms::from(shell.mass_kg),
            muzzle_velocity: MetersPerSecond::from(shell.muzzle_velocity),
            krupp: shell.krupp,
            air_drag: shell.air_drag,
            normalization: Degrees::from(normalization_deg).to_radians(),
            ricochet_angle: Degrees::from(shell.ricochet_angle).to_radians(),
            always_ricochet_angle: Degrees::from(shell.always_ricochet_angle).to_radians(),
            fuse_time: Seconds::from(shell.fuse_time),
            fuse_threshold: Millimeters::from(fuse_threshold_mm),
            drag_factor: DragFactor(0.5 * f64::from(shell.air_drag) * radius_m * radius_m * PI / mass_kg),
            penetration_factor: PenetrationFactor(
                1e-7 * f64::from(shell.krupp) * mass_kg.powf(0.69) * caliber_m.powf(-1.07),
            ),
            capped: shell.cap,
        })
    }

    /// Armor a shell arriving at `velocity` defeats when it strikes head-on.
    pub fn raw_penetration(&self, velocity: MetersPerSecond) -> Millimeters {
        let pen = self.penetration_factor.0 * f64::from(velocity.value()).powf(VELOCITY_POWER);
        Millimeters::from(pen as f32)
    }

    /// Normalization the shell actually gets: uncapped shells get none.
    fn effective_normalization(&self) -> Radians {
        if self.capped { self.normalization } else { Radians::new(0.0) }
    }
}

/// Result of a trajectory simulation at impact.
#[derive(Clone, Debug)]
pub struct ImpactResult {
    /// Horizontal range flown.
    pub distance: Meters,
    pub impact_velocity: MetersPerSecond,
    /// Impact angle from horizontal; positive means falling.
    pub impact_angle_horizontal: Radians,
    /// Impact angle from the deck plane.
    pub impact_angle_deck: Radians,
    /// Time to target on the in-game clock (real flight time / [`TIME_MULTIPLIER`]).
    pub time_to_target: Seconds,
    /// Penetration before any strike angle is accounted for.
    pub raw_penetration: Millimeters,
    /// Penetration against a vertical surface.
    pub effective_penetration_belt: Millimeters,
    /// Penetration against a vertical surface, after normalization.
    pub effective_penetration_belt_normalized: Millimeters,
    /// Penetration against a horizontal surface.
    pub effective_penetration_deck: Millimeters,
    /// Penetration against a horizontal surface, after normalization.
    pub effective_penetration_deck_normalized: Millimeters,
    pub launch_angle: Radians,
}

/// Planar flight state during integration.
///
/// Kept in `f64` and in plain SI units: the integrator runs thousands of steps
/// and accumulates visible error at `f32`. Only the results that leave this
/// module carry unit types.
#[derive(Clone, Copy, Debug)]
struct FlightState {
    /// Horizontal distance from the muzzle (m).
    range: f64,
    /// Height above the firing plane (m).
    altitude: f64,
    /// Horizontal velocity (m/s).
    velocity_horizontal: f64,
    /// Vertical velocity (m/s), negative while descending.
    velocity_vertical: f64,
    /// Time since firing (s).
    time: f64,
}

impl FlightState {
    fn launch(muzzle_velocity: MetersPerSecond, launch_angle: Radians) -> Self {
        let speed = f64::from(muzzle_velocity.value());
        let angle = f64::from(launch_angle.value());
        FlightState {
            range: 0.0,
            altitude: 0.0,
            velocity_horizontal: speed * angle.cos(),
            velocity_vertical: speed * angle.sin(),
            time: 0.0,
        }
    }

    fn speed(&self) -> f64 {
        (self.velocity_horizontal * self.velocity_horizontal + self.velocity_vertical * self.velocity_vertical).sqrt()
    }
}

/// Acceleration acting on the shell: gravity plus air drag (m/s^2).
#[derive(Clone, Copy, Debug)]
struct Acceleration {
    horizontal: f64,
    vertical: f64,
}

/// Change a single integration step applies to a [`FlightState`].
#[derive(Clone, Copy, Debug)]
struct FlightStep {
    range: f64,
    altitude: f64,
    velocity_horizontal: f64,
    velocity_vertical: f64,
}

impl FlightStep {
    /// State reached after `fraction` of this step; 1.0 is the whole step.
    fn applied_to(&self, state: FlightState, fraction: f64) -> FlightState {
        FlightState {
            range: state.range + self.range * fraction,
            altitude: state.altitude + self.altitude * fraction,
            velocity_horizontal: state.velocity_horizontal + self.velocity_horizontal * fraction,
            velocity_vertical: state.velocity_vertical + self.velocity_vertical * fraction,
            time: state.time + DT * fraction,
        }
    }
}

/// Air density at a given altitude, from the ISA atmospheric model.
fn air_density(altitude: f64) -> f64 {
    let t = T0 - L * altitude;
    if t <= 0.0 {
        return 0.0;
    }
    let p = P0 * (t / T0).powf(GM_RL);
    (M_AIR * p) / (R_GAS * t)
}

fn acceleration(drag: DragFactor, state: FlightState) -> Acceleration {
    let drag_rho = drag.0 * air_density(state.altitude);
    let speed = state.speed();
    Acceleration {
        horizontal: -drag_rho * state.velocity_horizontal * speed,
        vertical: -G - drag_rho * state.velocity_vertical * speed,
    }
}

/// Simpson weighting of the four RK4 stage derivatives over one step.
fn rk4_weighted(k1: f64, k2: f64, k3: f64, k4: f64) -> f64 {
    (k1 + 2.0 * k2 + 2.0 * k3 + k4) / 6.0 * DT
}

/// One RK4 step of the trajectory ODE.
fn rk4_step(drag: DragFactor, state: FlightState) -> FlightStep {
    let half = DT * 0.5;

    let a1 = acceleration(drag, state);
    let stage2 = FlightState {
        altitude: state.altitude + state.velocity_vertical * half,
        velocity_horizontal: state.velocity_horizontal + a1.horizontal * half,
        velocity_vertical: state.velocity_vertical + a1.vertical * half,
        ..state
    };

    let a2 = acceleration(drag, stage2);
    let stage3 = FlightState {
        altitude: state.altitude + stage2.velocity_vertical * half,
        velocity_horizontal: state.velocity_horizontal + a2.horizontal * half,
        velocity_vertical: state.velocity_vertical + a2.vertical * half,
        ..state
    };

    let a3 = acceleration(drag, stage3);
    let stage4 = FlightState {
        altitude: state.altitude + stage3.velocity_vertical * DT,
        velocity_horizontal: state.velocity_horizontal + a3.horizontal * DT,
        velocity_vertical: state.velocity_vertical + a3.vertical * DT,
        ..state
    };

    let a4 = acceleration(drag, stage4);

    FlightStep {
        range: rk4_weighted(
            state.velocity_horizontal,
            stage2.velocity_horizontal,
            stage3.velocity_horizontal,
            stage4.velocity_horizontal,
        ),
        altitude: rk4_weighted(
            state.velocity_vertical,
            stage2.velocity_vertical,
            stage3.velocity_vertical,
            stage4.velocity_vertical,
        ),
        velocity_horizontal: rk4_weighted(a1.horizontal, a2.horizontal, a3.horizontal, a4.horizontal),
        velocity_vertical: rk4_weighted(a1.vertical, a2.vertical, a3.vertical, a4.vertical),
    }
}

/// Integrate a trajectory until the shell returns to the firing plane, and
/// return the state it lands in. `None` if it is still airborne after
/// [`MAX_TIME`].
fn simulate_trajectory(params: &ShellParams, launch_angle: Radians) -> Option<FlightState> {
    let mut state = FlightState::launch(params.muzzle_velocity, launch_angle);

    while state.time < MAX_TIME {
        let step = rk4_step(params.drag_factor, state);
        let next = step.applied_to(state, 1.0);

        if next.altitude < 0.0 && state.time > DT {
            // Interpolate linearly back to the exact ground crossing.
            let fraction = state.altitude / (state.altitude - next.altitude);
            return Some(step.applied_to(state, fraction));
        }

        state = next;
    }

    None
}

/// Angle left over after the shell's cap straightens the strike out. Zero once
/// the strike is shallower than the normalization the cap provides.
fn normalized_strike_angle(angle: Radians, normalization: Radians) -> Radians {
    (angle.abs() - normalization).max_zero()
}

fn build_impact_result(params: &ShellParams, landing: FlightState, launch_angle: Radians) -> ImpactResult {
    let impact_velocity = MetersPerSecond::from(landing.speed() as f32);

    // Vertical velocity is negative on descent; the fall angle is its magnitude.
    let from_horizontal = Radians::from((landing.velocity_vertical / landing.velocity_horizontal).atan().abs() as f32);
    let from_deck = Radians::new(std::f32::consts::FRAC_PI_2) - from_horizontal;

    let raw = params.raw_penetration(impact_velocity);
    let normalization = params.effective_normalization();

    ImpactResult {
        distance: Meters::from(landing.range as f32),
        impact_velocity,
        impact_angle_horizontal: from_horizontal,
        impact_angle_deck: from_deck,
        time_to_target: Seconds::from((landing.time / TIME_MULTIPLIER) as f32),
        raw_penetration: raw,
        effective_penetration_belt: raw * from_horizontal.cos(),
        effective_penetration_belt_normalized: raw * normalized_strike_angle(from_horizontal, normalization).cos(),
        effective_penetration_deck: raw * from_deck.cos(),
        effective_penetration_deck_normalized: raw * normalized_strike_angle(from_deck, normalization).cos(),
        launch_angle,
    }
}

/// Longest range the shell can reach.
fn max_range(params: &ShellParams) -> Option<Meters> {
    // Scan from 5 to 60 deg in 1 deg steps; high drag shells peak below 30 deg.
    let mut best = 0.0f64;
    for degrees in 5..=60 {
        if let Some(landing) = simulate_trajectory(params, Degrees::from(degrees as f32).to_radians())
            && landing.range > best
        {
            best = landing.range;
        }
    }
    (best > 0.0).then(|| Meters::from(best as f32))
}

/// Solve for the launch angle that produces a given horizontal range.
/// Bisects the low-angle (flat) branch of the range curve.
/// Returns `None` if the range exceeds the shell's maximum range.
pub fn solve_for_range(params: &ShellParams, range: Meters) -> Option<ImpactResult> {
    let target_range = f64::from(range.value());
    if target_range <= 0.0 {
        // Point blank: the shell arrives at muzzle velocity, flat.
        let muzzle = FlightState::launch(params.muzzle_velocity, Radians::new(0.0));
        return Some(build_impact_result(params, muzzle, Radians::new(0.0)));
    }

    if range > max_range(params)? {
        return None;
    }

    let mut low = Degrees::from(0.001).to_radians();
    let mut high = Degrees::from(45.0).to_radians();
    let mut last_candidate: Option<(Radians, FlightState)> = None;

    for _ in 0..BISECT_MAX_ITER {
        let mid = Radians::from((low.value() + high.value()) / 2.0);
        let Some(landing) = simulate_trajectory(params, mid) else {
            // Never came down; a shallower angle will.
            high = mid;
            continue;
        };

        let error = landing.range - target_range;
        if error.abs() < BISECT_TOLERANCE_M {
            return Some(build_impact_result(params, landing, mid));
        }
        last_candidate = Some((mid, landing));

        // Overshot means aim flatter, undershot means aim higher.
        if error > 0.0 {
            high = mid;
        } else {
            low = mid;
        }
    }

    last_candidate.map(|(launch_angle, landing)| build_impact_result(params, landing, launch_angle))
}

/// Compute impact data at regular range intervals.
pub fn compute_range_table(params: &ShellParams, max_range: Meters, step: Meters) -> Vec<ImpactResult> {
    let mut results = Vec::new();
    let mut range = step;
    while range <= max_range {
        let Some(impact) = solve_for_range(params, range) else {
            break; // Exceeded max range
        };
        results.push(impact);
        range = range + step;
    }
    results
}

/// A point on a normalized trajectory arc. Both components run 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArcPoint {
    /// Fraction of the total horizontal range covered.
    pub along_range: f32,
    /// Height as a fraction of the arc's apex.
    pub height: f32,
}

/// A trajectory arc reduced to its shape, for drawing.
#[derive(Clone, Debug)]
pub struct ArcProfile {
    pub points: Vec<ArcPoint>,
    /// `apex_height / total_range`: the arc's real aspect ratio. Scale
    /// [`ArcPoint::height`] by this and by the drawn horizontal extent to keep
    /// the arc in proportion, or apply an additional visual multiplier.
    pub height_ratio: f32,
}

impl ArcProfile {
    /// A flat two-point arc, for trajectories that never leave the ground.
    fn flat() -> Self {
        ArcProfile {
            points: vec![ArcPoint { along_range: 0.0, height: 0.0 }, ArcPoint { along_range: 1.0, height: 0.0 }],
            height_ratio: 0.0,
        }
    }
}

/// Simulate a trajectory and reduce it to a normalized arc for visualization.
pub fn simulate_arc(params: &ShellParams, launch_angle: Radians, num_points: usize) -> ArcProfile {
    let mut state = FlightState::launch(params.muzzle_velocity, launch_angle);
    let mut path = vec![state];

    while state.time < MAX_TIME {
        let step = rk4_step(params.drag_factor, state);
        let next = step.applied_to(state, 1.0);

        if next.altitude < 0.0 && state.time > DT {
            let fraction = state.altitude / (state.altitude - next.altitude);
            let mut landing = step.applied_to(state, fraction);
            landing.altitude = 0.0;
            path.push(landing);
            break;
        }

        state = next;
        path.push(state);
    }

    let total_range = path.last().map(|s| s.range).unwrap_or(0.0);
    let apex = path.iter().map(|s| s.altitude).fold(0.0f64, f64::max);
    if path.len() < 2 || total_range <= 0.0 || apex <= 0.0 {
        return ArcProfile::flat();
    }

    let height_ratio = (apex / total_range) as f32;
    let normalized: Vec<ArcPoint> = path
        .iter()
        .map(|s| ArcPoint { along_range: (s.range / total_range) as f32, height: (s.altitude / apex) as f32 })
        .collect();

    if num_points <= 2 || normalized.len() <= num_points {
        return ArcProfile { points: normalized, height_ratio };
    }

    // Resample evenly along the horizontal axis, keeping both end points.
    let mut points = Vec::with_capacity(num_points);
    points.push(normalized[0]);
    for i in 1..num_points - 1 {
        let target = i as f32 / (num_points - 1) as f32;
        let idx = normalized.partition_point(|p| p.along_range < target).clamp(1, normalized.len() - 1);
        let before = normalized[idx - 1];
        let after = normalized[idx];
        let span = after.along_range - before.along_range;
        let fraction = if span.abs() > f32::EPSILON { (target - before.along_range) / span } else { 0.0 };
        points
            .push(ArcPoint { along_range: target, height: before.height + fraction * (after.height - before.height) });
    }
    points.push(normalized[normalized.len() - 1]);

    ArcProfile { points, height_ratio }
}

/// AP overmatch ratio: a plate is overmatched when `caliber > thickness * OVERMATCH_RATIO`.
/// This is the community-established value; the real check is engine-side and is not
/// present in GameParams, so it cannot be data-validated and must be kept in sync by hand.
pub const OVERMATCH_RATIO: f32 = 14.3;

/// Whether a shell this wide defeats a plate this thin regardless of angle.
pub fn is_overmatch(caliber: Millimeters, thickness: Millimeters) -> bool {
    caliber > thickness * OVERMATCH_RATIO
}

/// Thickness a strike at `angle` from the plate normal has to defeat.
fn effective_thickness(thickness: Millimeters, angle: Radians) -> Millimeters {
    thickness / angle.cos().max(MIN_STRIKE_COSINE)
}

/// Position of a plate along a shell's ray, counted in strike order from the
/// first plate the shell reaches.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PlateIndex(usize);

impl PlateIndex {
    /// The first plate the shell reaches along its ray.
    pub const FIRST: PlateIndex = PlateIndex(0);

    pub fn new(index: usize) -> Self {
        PlateIndex(index)
    }

    pub fn value(self) -> usize {
        self.0
    }

    /// 1-based position, for display alongside a plate list.
    pub fn number(self) -> usize {
        self.0 + 1
    }

    /// The plate struck immediately before this one, if there was one.
    pub fn previous(self) -> Option<PlateIndex> {
        self.0.checked_sub(1).map(PlateIndex)
    }
}

/// One armor plate along a shell's ray, in strike order.
#[derive(Clone, Copy, Debug)]
pub struct PlateHit {
    pub thickness: Millimeters,
    /// Strike angle from the plate normal: 0 is head-on, 90 is glancing.
    pub angle_from_normal: Degrees,
    /// Distance from the first hit along the ray.
    pub distance_along_ray: ShipModelDistance,
}

/// Outcome of a shell hitting a single plate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlateOutcome {
    /// Caliber beats [`OVERMATCH_RATIO`] times thickness: always penetrates, ignores ricochet.
    Overmatch,
    /// Shell penetrates (raw penetration at least the effective thickness).
    Penetrate,
    /// Strike at or past [`ShellParams::always_ricochet_angle`]: guaranteed ricochet.
    Ricochet,
    /// Shell shatters (raw penetration below the effective thickness).
    Shatter,
}

/// Per-plate simulation result.
#[derive(Clone, Copy, Debug)]
pub struct PlateResult {
    pub outcome: PlateOutcome,
    /// Thickness this plate presents once the strike angle is accounted for.
    pub effective_thickness: Millimeters,
    /// Penetration the shell brings to this plate.
    pub raw_penetration: Millimeters,
    pub velocity_before: MetersPerSecond,
    /// Velocity leaving this plate. `None` when the plate stopped the shell.
    pub velocity_after: Option<MetersPerSecond>,
    /// Whether this plate armed the fuse.
    pub fuse_armed_here: bool,
}

/// Where the AP shell detonates (fuse activation + travel).
#[derive(Clone, Copy, Debug)]
pub struct FuseDetonation {
    /// Detonation point measured from the first hit along the ray.
    pub distance_along_ray: ShipModelDistance,
    /// Plate that armed the fuse.
    pub armed_at: PlateIndex,
    /// Real-world distance travelled after arming.
    pub travel_distance: Meters,
}

/// Complete shell simulation through all hit plates.
#[derive(Clone, Debug)]
pub struct ShellSimResult {
    /// Per-plate results, one for each hit the shell actually reached.
    pub plates: Vec<PlateResult>,
    /// Where the fuse detonates (None if fuse never armed or HE/SAP).
    pub detonation: Option<FuseDetonation>,
    /// Plate where the shell stopped due to ricochet/shatter/spent velocity.
    pub stopped_at: Option<PlateIndex>,
    /// Last plate the shell reached before fuse detonation. The shell explodes
    /// between this plate and the next. Distinct from `stopped_at`.
    pub detonated_at: Option<PlateIndex>,
}

impl ShellSimResult {
    /// Plate the shell's run ends at: whichever of detonation and being stopped
    /// comes first. `None` when the shell passed through everything intact.
    pub fn last_reached_plate(&self) -> Option<PlateIndex> {
        match (self.detonated_at, self.stopped_at) {
            (Some(detonated), Some(stopped)) => Some(detonated.min(stopped)),
            (detonated, stopped) => detonated.or(stopped),
        }
    }
}

/// A fuse burning down while the shell works through the plate stack.
#[derive(Clone, Copy, Debug)]
struct ArmedFuse {
    /// Plate that armed it.
    armed_at: PlateIndex,
    /// Velocity the shell left the arming plate at, which sets the burn distance.
    velocity_at_arming: MetersPerSecond,
    /// How far along the ray the shell may travel before the fuse fires.
    travel_budget: ShipModelDistance,
    /// How far along the ray it has travelled since arming.
    travelled: ShipModelDistance,
}

impl ArmedFuse {
    fn remaining(&self) -> ShipModelDistance {
        self.travel_budget - self.travelled
    }

    /// Real-world distance the shell covers between arming and detonating.
    fn travel_distance(&self, fuse_time: Seconds) -> Meters {
        self.velocity_at_arming.travel_over(fuse_time)
    }
}

/// Simulate a shell passing through a sequence of armor plates along one ray.
///
/// Uses formulas from wows_shell (jcw780):
///   raw_pen = penetration_factor * velocity^1.38
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
    let normalization = params.effective_normalization();

    let mut velocity = impact.impact_velocity;
    let mut plates: Vec<PlateResult> = Vec::with_capacity(hits.len());
    let mut stopped_at: Option<PlateIndex> = None;
    let mut detonated_at: Option<PlateIndex> = None;
    let mut detonation: Option<FuseDetonation> = None;

    let mut fuse: Option<ArmedFuse> = None;
    // Along-ray position of the last plate the shell actually processed.
    let mut previous_plate = ShipModelDistance::ZERO;

    for (index, hit) in hits.iter().enumerate() {
        let index = PlateIndex::new(index);

        // A fuse that burns out before this plate detonates in the gap behind it.
        if let Some(armed) = fuse.as_mut() {
            let gap = hit.distance_along_ray - previous_plate;
            let remaining = armed.remaining();
            if gap >= remaining && remaining > ShipModelDistance::ZERO {
                detonation = Some(FuseDetonation {
                    distance_along_ray: previous_plate + remaining,
                    armed_at: armed.armed_at,
                    travel_distance: armed.travel_distance(params.fuse_time),
                });
                // The fuse armed on an earlier plate, so there is always one before this.
                detonated_at = index.previous();
                break;
            }
            armed.travelled = armed.travelled + gap;
        }

        let raw_penetration = params.raw_penetration(velocity);
        let strike_angle = hit.angle_from_normal.to_radians();
        let overmatched = is_overmatch(params.caliber, hit.thickness);

        if !overmatched && strike_angle >= params.always_ricochet_angle {
            plates.push(PlateResult {
                outcome: PlateOutcome::Ricochet,
                effective_thickness: effective_thickness(hit.thickness, strike_angle),
                raw_penetration,
                velocity_before: velocity,
                velocity_after: continue_on_ricochet.then_some(velocity),
                fuse_armed_here: false,
            });
            if !continue_on_ricochet {
                stopped_at = Some(index);
                break;
            }
            // The plate is recorded, but the shell carries on unslowed.
            previous_plate = hit.distance_along_ray;
            continue;
        }

        // Overmatched plates are defeated head-on, whatever the geometry says.
        let normalized_angle =
            if overmatched { Radians::new(0.0) } else { normalized_strike_angle(strike_angle, normalization) };
        let effective = effective_thickness(hit.thickness, normalized_angle);

        if !overmatched && raw_penetration < effective {
            plates.push(PlateResult {
                outcome: PlateOutcome::Shatter,
                effective_thickness: effective,
                raw_penetration,
                velocity_before: velocity,
                velocity_after: None,
                fuse_armed_here: false,
            });
            stopped_at = Some(index);
            break;
        }

        let penetration_ratio = raw_penetration.value() / effective.value().max(MIN_EFFECTIVE_THICKNESS.value());
        let velocity_after = MetersPerSecond::from(velocity.value() * (1.0 - (1.0 - penetration_ratio).exp()));

        let armed_here = fuse.is_none() && hit.thickness >= params.fuse_threshold;
        if armed_here {
            // Armor meshes are in ship-model space (15 m per unit); converting at
            // the 30 m BigWorld scale halves fuse travel and detonates shells
            // short of the citadel (issue #43).
            fuse = Some(ArmedFuse {
                armed_at: index,
                velocity_at_arming: velocity_after,
                travel_budget: velocity_after.travel_over(params.fuse_time).to_ship_model(),
                travelled: ShipModelDistance::ZERO,
            });
        }

        plates.push(PlateResult {
            outcome: if overmatched { PlateOutcome::Overmatch } else { PlateOutcome::Penetrate },
            effective_thickness: effective,
            raw_penetration,
            velocity_before: velocity,
            velocity_after: Some(velocity_after),
            fuse_armed_here: armed_here,
        });

        previous_plate = hit.distance_along_ray;
        velocity = velocity_after;

        if velocity < MIN_CARRY_VELOCITY {
            stopped_at = Some(index);
            break;
        }
    }

    // A fuse still burning when the plates ran out fires past the last one.
    if let Some(armed) = fuse
        && detonation.is_none()
    {
        detonation = Some(FuseDetonation {
            distance_along_ray: previous_plate + armed.remaining().max_zero(),
            armed_at: armed.armed_at,
            travel_distance: armed.travel_distance(params.fuse_time),
        });
        // A shell stopped by ricochet or shatter still detonates, on the plate
        // that stopped it. One that simply ran out of ship detonates outside it,
        // and `detonated_at` stays None.
        detonated_at = stopped_at;
    }

    ShellSimResult { plates, detonation, stopped_at, detonated_at }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Colombo 381mm AP (PIPA045_381MM_50_AP) from GameParams.
    fn colombo_ap() -> ShellParams {
        let caliber = Millimeters::from(381.0);
        let mass = Kilograms::from(884.8);
        let air_drag = 0.2954;
        let krupp = 2434.0;

        let caliber_m = f64::from(caliber.to_meters().value());
        let mass_kg = f64::from(mass.value());
        let radius_m = caliber_m / 2.0;

        ShellParams {
            caliber,
            mass,
            muzzle_velocity: MetersPerSecond::from(850.0),
            krupp,
            air_drag,
            normalization: Degrees::from(6.0).to_radians(),
            ricochet_angle: Degrees::from(45.0).to_radians(),
            always_ricochet_angle: Degrees::from(60.0).to_radians(),
            fuse_time: Seconds::from(0.033),
            fuse_threshold: Millimeters::from(64.0),
            drag_factor: DragFactor(0.5 * f64::from(air_drag) * radius_m * radius_m * PI / mass_kg),
            penetration_factor: PenetrationFactor(1e-7 * f64::from(krupp) * mass_kg.powf(0.69) * caliber_m.powf(-1.07)),
            capped: true,
        }
    }

    fn impact_at(velocity: MetersPerSecond) -> ImpactResult {
        let fall = Degrees::from(4.3).to_radians();
        ImpactResult {
            distance: Meters::from(8500.0),
            impact_velocity: velocity,
            impact_angle_horizontal: fall,
            impact_angle_deck: Radians::new(std::f32::consts::FRAC_PI_2) - fall,
            time_to_target: Seconds::from(0.0),
            raw_penetration: Millimeters::from(0.0),
            effective_penetration_belt: Millimeters::from(0.0),
            effective_penetration_belt_normalized: Millimeters::from(0.0),
            effective_penetration_deck: Millimeters::from(0.0),
            effective_penetration_deck_normalized: Millimeters::from(0.0),
            launch_angle: Radians::new(0.0),
        }
    }

    /// A plate struck at the 24.7 deg the issue #43 screenshot reports.
    fn plate(distance: ShipModelDistance, thickness: Millimeters) -> PlateHit {
        PlateHit { thickness, angle_from_normal: Degrees::from(24.7), distance_along_ray: distance }
    }

    fn units(value: f32) -> ShipModelDistance {
        ShipModelDistance::from(value)
    }

    fn mm(value: f32) -> Millimeters {
        Millimeters::from(value)
    }

    fn mps(value: f32) -> MetersPerSecond {
        MetersPerSecond::from(value)
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
        let impact = impact_at(mps(699.0));
        let hits = vec![plate(units(0.0), mm(425.0)), plate(units(0.42), mm(40.0)), plate(units(0.6), mm(375.0))];

        let sim = simulate_shell_through_plates(&params, &impact, &hits, false);

        let det = sim.detonation.expect("fuse armed on the belt, shell must detonate");
        let travel_m = det.travel_distance.value();
        assert!((travel_m - 7.4).abs() < 0.2, "fuse travel {travel_m} m, expected ~7.4 m");
        assert_eq!(sim.plates.len(), 2, "shell must reach and penetrate the plate 6.3 m behind the belt");
        assert_eq!(sim.plates[1].outcome, PlateOutcome::Penetrate);
        assert_eq!(
            sim.detonated_at,
            Some(PlateIndex::new(1)),
            "detonation happens between the second and third plates"
        );
        let expected = det.travel_distance.to_ship_model().value();
        let got = det.distance_along_ray.value();
        assert!((got - expected).abs() < 0.02, "detonation at {got} units along ray, expected ~{expected}");
    }

    /// The belt arms the fuse, so the detonation is attributed to the plate that
    /// armed it rather than the plate the shell was crossing when it fired.
    #[test]
    fn detonation_names_the_arming_plate() {
        let params = colombo_ap();
        let sim = simulate_shell_through_plates(
            &params,
            &impact_at(mps(699.0)),
            &[plate(units(0.0), mm(425.0)), plate(units(0.42), mm(40.0)), plate(units(0.6), mm(375.0))],
            false,
        );

        let det = sim.detonation.expect("fuse armed on the belt");
        assert_eq!(det.armed_at, PlateIndex::FIRST);
        assert_eq!(det.armed_at.number(), 1);
    }

    /// An uncapped shell gets no normalization, so the same strike has to defeat
    /// more armor than a capped shell would.
    #[test]
    fn uncapped_shells_lose_normalization() {
        let capped = colombo_ap();
        let uncapped = ShellParams { capped: false, ..colombo_ap() };
        let hits = [plate(units(0.0), mm(425.0))];

        let capped_sim = simulate_shell_through_plates(&capped, &impact_at(mps(699.0)), &hits, false);
        let uncapped_sim = simulate_shell_through_plates(&uncapped, &impact_at(mps(699.0)), &hits, false);

        assert!(uncapped_sim.plates[0].effective_thickness > capped_sim.plates[0].effective_thickness);
    }

    /// A shell that shatters is stopped, and a stopped shell reports no exit velocity.
    #[test]
    fn a_shattered_plate_reports_no_exit_velocity() {
        let params = colombo_ap();
        let sim =
            simulate_shell_through_plates(&params, &impact_at(mps(200.0)), &[plate(units(0.0), mm(425.0))], false);

        assert_eq!(sim.plates[0].outcome, PlateOutcome::Shatter);
        assert_eq!(sim.plates[0].velocity_after, None);
        assert_eq!(sim.stopped_at, Some(PlateIndex::FIRST));
    }

    /// Overmatch is a caliber-to-thickness ratio, and the ratio is what decides it.
    #[test]
    fn overmatch_follows_the_caliber_ratio() {
        let caliber = mm(457.0);
        assert!(is_overmatch(caliber, mm(31.0)));
        assert!(!is_overmatch(caliber, mm(32.0)));
    }
}
