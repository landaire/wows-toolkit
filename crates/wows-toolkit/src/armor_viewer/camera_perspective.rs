use crate::viewport_3d::types::Vec3;
use wowsunpack::game_params::types::CameraRing;
use wowsunpack::game_params::types::CameraTrajectory;

/// How the locked camera projects: faithfully like the game (look along yaw,
/// off-center for elliptical orbits) or with the view axis through the ship
/// center.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LookMode {
    Game,
    ThroughCenter,
}

const PITCH_MIN: f32 = 0.03;
const PITCH_MAX: f32 = 1.4;

/// First-person preview state: where on the ring the eye sits and how it aims.
#[derive(Clone, Copy, Debug)]
pub struct CameraPerspective {
    /// Ring parameter / aim yaw, radians.
    pub yaw: f32,
    /// Downward look tilt, radians; positive aims toward the water.
    pub pitch: f32,
    /// Inner(0)..outer(1) orbit blend.
    pub zoom: f32,
    /// Vertical FOV in degrees.
    pub fov_deg: f32,
    /// Projection mode (game-faithful vs through-center).
    pub look_mode: LookMode,
}

impl Default for CameraPerspective {
    fn default() -> Self {
        Self { yaw: 0.0, pitch: 0.35, zoom: 0.0, fov_deg: 75.0, look_mode: LookMode::Game }
    }
}

/// Linear blend of two rings (inner -> outer) by `z`, clamped to 0..1.
pub(crate) fn lerp_ring(a: &CameraRing, b: &CameraRing, z: f32) -> CameraRing {
    let z = z.clamp(0.0, 1.0);
    CameraRing { pos_center: a.pos_center.lerp(b.pos_center, z), semi_axes: a.semi_axes.lerp(b.semi_axes, z) }
}

/// Point on the ring at parameter `yaw`, matching `camera_ellipse::sample_ring_points`.
pub(crate) fn eye_on_ring(ring: &CameraRing, waterline_dy: f32, yaw: f32) -> Vec3 {
    let center = Vec3::new(ring.pos_center.x, ring.pos_center.y + waterline_dy, ring.pos_center.z);
    center + Vec3::new(yaw.cos() * ring.semi_axes.x, 0.0, yaw.sin() * ring.semi_axes.y)
}

/// World-space point where the ray `eye + t*dir` meets the water plane Y=0.
/// The denominator is forced downward so a near-horizontal ray yields a far
/// (capped) point instead of diverging; `t` is clamped to `[0, max_dist]`.
pub(crate) fn water_aim_point(eye: Vec3, dir: Vec3, max_dist: f32) -> Vec3 {
    let denom = dir.y.min(-1e-4);
    let t = (-eye.y / denom).clamp(0.0, max_dist);
    eye + dir * t
}

impl CameraPerspective {
    /// Clamp the live state to its valid ranges.
    pub fn clamp(&mut self) {
        self.pitch = self.pitch.clamp(PITCH_MIN, PITCH_MAX);
        self.zoom = self.zoom.clamp(0.0, 1.0);
        self.fov_deg = self.fov_deg.clamp(30.0, 120.0);
    }

    /// Resolve the ring and return model-space eye, ring center, and the
    /// horizontal look unit `h` for the current projection mode.
    fn eye_center_h(
        &self,
        traj: &CameraTrajectory,
        fov_blend: f32,
        height: f32,
        waterline_dy: f32,
    ) -> (Vec3, Vec3, Vec3) {
        let inner = traj.resolve(fov_blend, height);
        let ring = match traj.resolve_outer(fov_blend, height) {
            Some(outer) => lerp_ring(&inner, &outer, self.zoom),
            None => inner,
        };
        let eye = eye_on_ring(&ring, waterline_dy, self.yaw);
        let center = Vec3::new(ring.pos_center.x, ring.pos_center.y + waterline_dy, ring.pos_center.z);
        let h = match self.look_mode {
            LookMode::Game => Vec3::new(-self.yaw.cos(), 0.0, -self.yaw.sin()),
            LookMode::ThroughCenter => {
                let raw = Vec3::new(center.x - eye.x, 0.0, center.z - eye.z);
                if raw.norm() > 1e-6 { raw.normalize() } else { Vec3::new(0.0, 0.0, -1.0) }
            }
        };
        (eye, center, h)
    }

    /// Model-space eye and unit look direction. The horizontal look is the yaw
    /// ray (game) or toward-center (through-center), tilted down by `pitch`.
    pub(crate) fn eye_and_look_dir(
        &self,
        traj: &CameraTrajectory,
        fov_blend: f32,
        height: f32,
        waterline_dy: f32,
    ) -> (Vec3, Vec3) {
        let (eye, _center, h) = self.eye_center_h(traj, fov_blend, height, waterline_dy);
        let up = Vec3::new(0.0, 1.0, 0.0);
        let dir = h * self.pitch.cos() - up * self.pitch.sin();
        (eye, dir)
    }

    /// Cap `pitch` per frame so the water aim point can never come nearer than
    /// the ship centerline along the look direction (never onto the camera's
    /// side of the hull). `.max(PITCH_MIN)` guards an inverted clamp range.
    pub(crate) fn clamp_pitch_to_far_side(
        &mut self,
        traj: &CameraTrajectory,
        fov_blend: f32,
        height: f32,
        waterline_dy: f32,
    ) {
        let (eye, center, h) = self.eye_center_h(traj, fov_blend, height, waterline_dy);
        let d_center = (center.x - eye.x) * h.x + (center.z - eye.z) * h.z;
        let pitch_max = if d_center > 1e-3 { (eye.y / d_center).atan() } else { PITCH_MAX };
        self.pitch = self.pitch.clamp(PITCH_MIN, pitch_max.max(PITCH_MIN));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wowsunpack::game_types::Vec2 as CoreVec2;
    use wowsunpack::game_types::Vec3 as CoreVec3;

    fn ring(cx: f32, cy: f32, cz: f32, sx: f32, sy: f32) -> CameraRing {
        CameraRing { pos_center: CoreVec3::new(cx, cy, cz), semi_axes: CoreVec2::new(sx, sy) }
    }

    fn traj_no_outer() -> CameraTrajectory {
        CameraTrajectory {
            pos_center: [CoreVec3::new(0.0, 2.0, 0.0), CoreVec3::new(0.0, 2.0, 0.0)],
            semi_axes: [CoreVec2::new(6.0, 9.0), CoreVec2::new(6.0, 9.0)],
            tags: String::new(),
            ignore_height_multiplier: true,
            outer: None,
        }
    }

    fn circular_traj() -> CameraTrajectory {
        CameraTrajectory {
            pos_center: [CoreVec3::new(0.0, 2.0, 0.0), CoreVec3::new(0.0, 2.0, 0.0)],
            semi_axes: [CoreVec2::new(8.0, 8.0), CoreVec2::new(8.0, 8.0)],
            tags: String::new(),
            ignore_height_multiplier: true,
            outer: None,
        }
    }

    #[test]
    fn game_horizontal_at_yaw_zero_is_minus_x() {
        let g = CameraPerspective { yaw: 0.0, pitch: 0.0, look_mode: LookMode::Game, ..Default::default() };
        let (_e, d) = g.eye_and_look_dir(&circular_traj(), 0.0, 0.0, 0.0);
        assert!((d - Vec3::new(-1.0, 0.0, 0.0)).norm() < 1e-4, "{d:?}");
    }

    #[test]
    fn game_and_through_center_differ_for_elliptical_ring() {
        let yaw = std::f32::consts::FRAC_PI_4;
        let g = CameraPerspective { yaw, look_mode: LookMode::Game, ..Default::default() };
        let c = CameraPerspective { yaw, look_mode: LookMode::ThroughCenter, ..Default::default() };
        let (_e, dg) = g.eye_and_look_dir(&traj_no_outer(), 0.0, 0.0, 0.0);
        let (_e2, dc) = c.eye_and_look_dir(&traj_no_outer(), 0.0, 0.0, 0.0);
        assert!((dg - dc).norm() > 1e-2, "should differ: {dg:?} vs {dc:?}");
    }

    #[test]
    fn game_and_through_center_match_for_circular_ring() {
        let yaw = std::f32::consts::FRAC_PI_4;
        let g = CameraPerspective { yaw, look_mode: LookMode::Game, ..Default::default() };
        let c = CameraPerspective { yaw, look_mode: LookMode::ThroughCenter, ..Default::default() };
        let (_e, dg) = g.eye_and_look_dir(&circular_traj(), 0.0, 0.0, 0.0);
        let (_e2, dc) = c.eye_and_look_dir(&circular_traj(), 0.0, 0.0, 0.0);
        assert!((dg - dc).norm() < 1e-4, "should match: {dg:?} vs {dc:?}");
    }

    #[test]
    fn clamp_pitch_to_far_side_caps_high_pitch() {
        let mut p =
            CameraPerspective { yaw: 0.0, pitch: 1.3, look_mode: LookMode::ThroughCenter, ..Default::default() };
        p.clamp_pitch_to_far_side(&circular_traj(), 0.0, 0.0, 0.0);
        let expected = (2.0_f32 / 8.0).atan();
        assert!((p.pitch - expected).abs() < 1e-4, "pitch={} expected={}", p.pitch, expected);
    }

    #[test]
    fn clamp_pitch_to_far_side_no_panic_when_range_collapses() {
        let traj = CameraTrajectory {
            pos_center: [CoreVec3::new(0.0, 0.01, 0.0), CoreVec3::new(0.0, 0.01, 0.0)],
            semi_axes: [CoreVec2::new(50.0, 50.0), CoreVec2::new(50.0, 50.0)],
            tags: String::new(),
            ignore_height_multiplier: true,
            outer: None,
        };
        let mut p =
            CameraPerspective { yaw: 0.0, pitch: 0.5, look_mode: LookMode::ThroughCenter, ..Default::default() };
        p.clamp_pitch_to_far_side(&traj, 0.0, 0.0, 0.0);
        assert!((p.pitch - 0.03).abs() < 1e-4, "pitch={}", p.pitch);
    }

    #[test]
    fn eye_on_ring_at_yaw_zero_is_plus_x() {
        let e = eye_on_ring(&ring(0.0, 2.0, 0.0, 6.0, 9.0), 0.0, 0.0);
        assert!((e - Vec3::new(6.0, 2.0, 0.0)).norm() < 1e-4, "{e:?}");
    }

    #[test]
    fn eye_on_ring_at_yaw_half_pi_is_plus_z_and_folds_waterline() {
        let e = eye_on_ring(&ring(0.0, 2.0, 0.0, 6.0, 9.0), 0.5, std::f32::consts::FRAC_PI_2);
        assert!((e - Vec3::new(0.0, 2.5, 9.0)).norm() < 1e-4, "{e:?}");
    }

    #[test]
    fn lerp_ring_endpoints_and_midpoint() {
        let a = ring(0.0, 2.0, 0.0, 6.0, 6.0);
        let b = ring(0.0, 4.0, 0.0, 10.0, 10.0);
        let lo = lerp_ring(&a, &b, 0.0);
        let hi = lerp_ring(&a, &b, 1.0);
        let mid = lerp_ring(&a, &b, 0.5);
        assert!((lo.pos_center.y - 2.0).abs() < 1e-4 && (lo.semi_axes.x - 6.0).abs() < 1e-4);
        assert!((hi.pos_center.y - 4.0).abs() < 1e-4 && (hi.semi_axes.x - 10.0).abs() < 1e-4);
        assert!((mid.pos_center.y - 3.0).abs() < 1e-4 && (mid.semi_axes.x - 8.0).abs() < 1e-4);
    }

    #[test]
    fn eye_and_look_dir_eye_on_ring_dir_down_and_inward() {
        let p = CameraPerspective { yaw: 0.0, pitch: 0.35, ..Default::default() };
        let (eye, dir) = p.eye_and_look_dir(&traj_no_outer(), 0.0, 0.0, 0.0);
        assert!((eye - Vec3::new(6.0, 2.0, 0.0)).norm() < 1e-4, "eye={eye:?}");
        assert!((dir.norm() - 1.0).abs() < 1e-4, "dir not unit: {}", dir.norm());
        assert!(dir.y < 0.0, "dir.y={}", dir.y);
        assert!(dir.x < 0.0, "dir.x={}", dir.x);
    }

    #[test]
    fn water_aim_point_hits_plane() {
        let eye = Vec3::new(0.0, 5.0, 0.0);
        let dir = Vec3::new(0.0, -1.0, 0.0).normalize();
        let p = water_aim_point(eye, dir, 5000.0);
        assert!(p.y.abs() < 1e-4, "p={p:?}");
    }

    #[test]
    fn water_aim_point_caps_near_horizontal() {
        let eye = Vec3::new(0.0, 5.0, 0.0);
        let dir = Vec3::new(1.0, -1e-6, 0.0).normalize();
        let p = water_aim_point(eye, dir, 100.0);
        assert!((p - (eye + dir * 100.0)).norm() < 1e-3, "p={p:?}");
    }

    #[test]
    fn water_aim_point_t_never_negative() {
        let eye = Vec3::new(0.0, -5.0, 0.0);
        let dir = Vec3::new(0.0, -1.0, 0.0);
        let p = water_aim_point(eye, dir, 5000.0);
        assert!((p - eye).norm() < 1e-4, "p={p:?}");
    }

    #[test]
    fn clamp_pitch_into_downward_range() {
        let mut p = CameraPerspective { pitch: 5.0, ..Default::default() };
        p.clamp();
        assert!((p.pitch - 1.4).abs() < 1e-6, "{}", p.pitch);
        let mut q = CameraPerspective { pitch: -1.0, ..Default::default() };
        q.clamp();
        assert!((q.pitch - 0.03).abs() < 1e-6, "{}", q.pitch);
    }
}
