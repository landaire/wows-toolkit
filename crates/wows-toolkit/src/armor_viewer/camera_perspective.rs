use crate::viewport_3d::types::Vec3;
use wowsunpack::game_params::types::CameraRing;
use wowsunpack::game_params::types::CameraTrajectory;

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
}

impl Default for CameraPerspective {
    fn default() -> Self {
        Self { yaw: 0.0, pitch: 0.35, zoom: 0.0, fov_deg: 75.0 }
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

    /// Model-space eye and unit look direction. The look ray is the inward
    /// (toward orbit center) horizontal direction tilted down by `pitch`
    /// (`pitch > 0` aims toward the water). The ring is the inner orbit, or the
    /// inner->outer blend by `zoom` when the mode defines an outer orbit.
    pub(crate) fn eye_and_look_dir(
        &self,
        traj: &CameraTrajectory,
        fov_blend: f32,
        height: f32,
        waterline_dy: f32,
    ) -> (Vec3, Vec3) {
        let inner = traj.resolve(fov_blend, height);
        let ring = match traj.resolve_outer(fov_blend, height) {
            Some(outer) => lerp_ring(&inner, &outer, self.zoom),
            None => inner,
        };
        let eye = eye_on_ring(&ring, waterline_dy, self.yaw);
        let center = Vec3::new(ring.pos_center.x, ring.pos_center.y + waterline_dy, ring.pos_center.z);
        let inward_raw = Vec3::new(center.x - eye.x, 0.0, center.z - eye.z);
        let inward = if inward_raw.norm() > 1e-6 { inward_raw.normalize() } else { Vec3::new(0.0, 0.0, -1.0) };
        let up = Vec3::new(0.0, 1.0, 0.0);
        let dir = inward * self.pitch.cos() - up * self.pitch.sin();
        (eye, dir)
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
