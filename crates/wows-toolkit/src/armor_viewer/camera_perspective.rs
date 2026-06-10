use crate::viewport_3d::types::Vec3;
use wowsunpack::game_params::types::CameraRing;
use wowsunpack::game_params::types::CameraTrajectory;

const PITCH_LIMIT: f32 = 1.4;

/// What the locked camera aims at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LookTarget {
    /// Look along the aim yaw (game-faithful framing).
    AimDirection,
    /// Look straight at the ship center.
    ShipCenter,
}

/// First-person preview state: where on the ring the eye sits and how it aims.
#[derive(Clone, Copy, Debug)]
pub struct CameraPerspective {
    /// Ring parameter / aim yaw, radians.
    pub yaw: f32,
    /// Look tilt, radians; positive looks up.
    pub pitch: f32,
    /// Inner(0)..outer(1) orbit blend.
    pub zoom: f32,
    /// Vertical FOV in degrees.
    pub fov_deg: f32,
    pub look_target: LookTarget,
}

impl Default for CameraPerspective {
    fn default() -> Self {
        Self { yaw: 0.0, pitch: -0.15, zoom: 0.0, fov_deg: 75.0, look_target: LookTarget::AimDirection }
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

impl CameraPerspective {
    /// Clamp the live state to its valid ranges.
    pub fn clamp(&mut self) {
        self.pitch = self.pitch.clamp(-PITCH_LIMIT, PITCH_LIMIT);
        self.zoom = self.zoom.clamp(0.0, 1.0);
        self.fov_deg = self.fov_deg.clamp(30.0, 120.0);
    }

    /// Model-space (eye, look target) for the current state against a trajectory.
    /// The ring is the inner orbit, or the inner->outer blend by `zoom` when the
    /// mode defines an outer orbit.
    pub(crate) fn eye_and_target(
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
        let target = match self.look_target {
            LookTarget::ShipCenter => Vec3::new(0.0, waterline_dy, 0.0),
            LookTarget::AimDirection => {
                let inward_raw = Vec3::new(center.x - eye.x, 0.0, center.z - eye.z);
                let inward =
                    if inward_raw.norm() > 1e-6 { inward_raw.normalize() } else { Vec3::new(0.0, 0.0, -1.0) };
                let up = Vec3::new(0.0, 1.0, 0.0);
                let dir = inward * self.pitch.cos() + up * self.pitch.sin();
                eye + dir
            }
        };
        (eye, target)
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
    fn ship_center_target_is_origin_at_waterline() {
        let p = CameraPerspective { look_target: LookTarget::ShipCenter, ..Default::default() };
        let (_eye, target) = p.eye_and_target(&traj_no_outer(), 0.0, 0.0, 0.5);
        assert!((target - Vec3::new(0.0, 0.5, 0.0)).norm() < 1e-4, "{target:?}");
    }

    #[test]
    fn aim_direction_pitch_zero_is_horizontal_and_inward() {
        let p = CameraPerspective { yaw: 0.0, pitch: 0.0, look_target: LookTarget::AimDirection, ..Default::default() };
        let (eye, target) = p.eye_and_target(&traj_no_outer(), 0.0, 0.0, 0.0);
        let dir = (target - eye).normalize();
        assert!(dir.y.abs() < 1e-4, "dir.y={}", dir.y);
        assert!(dir.x < -0.9, "dir.x={}", dir.x);
    }

    #[test]
    fn aim_direction_positive_pitch_looks_up() {
        let p = CameraPerspective { yaw: 0.0, pitch: 0.3, look_target: LookTarget::AimDirection, ..Default::default() };
        let (eye, target) = p.eye_and_target(&traj_no_outer(), 0.0, 0.0, 0.0);
        assert!((target - eye).normalize().y > 0.0);
    }
}
