extern crate nalgebra as na;
use na::Matrix4;
use na::Vector4;

use super::types::Vec3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
    Z,
}

/// Target (azimuth, elevation) for the orthographic view looking along a signed axis.
/// Top/bottom keep the current azimuth so the spin is purely vertical.
pub fn ortho_view(axis: Axis, positive: bool, current_azimuth: f32) -> (f32, f32) {
    use std::f32::consts::FRAC_PI_2;
    use std::f32::consts::PI;
    let lim = FRAC_PI_2 - 0.01;
    match axis {
        Axis::Z => (if positive { 0.0 } else { PI }, 0.0),
        Axis::X => (if positive { FRAC_PI_2 } else { -FRAC_PI_2 }, 0.0),
        Axis::Y => (current_azimuth, if positive { lim } else { -lim }),
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CameraAnimation {
    start_az: f32,
    start_el: f32,
    target_az: f32,
    target_el: f32,
    elapsed: f32,
    duration: f32,
}

/// Arcball orbital camera for 3D model inspection.
#[derive(Clone, Debug)]
pub struct ArcballCamera {
    /// Center of rotation (typically model center from bounding box).
    pub target: Vec3,
    /// Distance from the target.
    pub distance: f32,
    /// Horizontal angle in radians.
    pub azimuth: f32,
    /// Vertical angle in radians, clamped to (-PI/2, PI/2).
    pub elevation: f32,
    /// Field of view in radians.
    pub fov: f32,
    /// Near clip plane.
    pub near: f32,
    /// Far clip plane.
    pub far: f32,
    /// Active snap animation, if any.
    pub animation: Option<CameraAnimation>,
}

impl Default for ArcballCamera {
    fn default() -> Self {
        Self {
            target: Vec3::zeros(),
            distance: 5.0,
            azimuth: std::f32::consts::FRAC_PI_4,
            elevation: 0.3,
            fov: std::f32::consts::FRAC_PI_4,
            near: 0.1,
            far: 10000.0,
            animation: None,
        }
    }
}

impl ArcballCamera {
    /// Create a camera that frames the given bounding box.
    pub fn from_bounds(min: Vec3, max: Vec3) -> Self {
        let center = (min + max) * 0.5;
        let extent = max - min;
        let max_extent = extent.x.max(extent.y).max(extent.z);
        let fov = std::f32::consts::FRAC_PI_4;
        let distance = (max_extent * 0.5) / (fov * 0.5).tan();

        Self {
            target: center,
            distance,
            azimuth: std::f32::consts::FRAC_PI_4,
            elevation: 0.3,
            fov,
            near: distance * 0.01,
            far: distance * 100.0,
            animation: None,
        }
    }

    /// Reset camera to frame the given bounding box.
    pub fn reset(&mut self, min: Vec3, max: Vec3) {
        *self = Self::from_bounds(min, max);
    }

    /// Begin an eased move to (target_az, target_el), keeping target and distance.
    /// Azimuth target is unwrapped to the shortest path from the current azimuth.
    pub fn animate_to(&mut self, target_az: f32, target_el: f32, duration: f32) {
        use std::f32::consts::PI;
        let mut delta = (target_az - self.azimuth).rem_euclid(2.0 * PI);
        if delta > PI {
            delta -= 2.0 * PI;
        }
        self.animation = Some(CameraAnimation {
            start_az: self.azimuth,
            start_el: self.elevation,
            target_az: self.azimuth + delta,
            target_el,
            elapsed: 0.0,
            duration: duration.max(1e-3),
        });
    }

    /// Advance a running animation by dt seconds. Returns true while still animating.
    pub fn update_animation(&mut self, dt: f32) -> bool {
        let Some(anim) = self.animation.as_mut() else {
            return false;
        };
        anim.elapsed += dt;
        let t = (anim.elapsed / anim.duration).clamp(0.0, 1.0);
        let e = if t < 0.5 { 4.0 * t * t * t } else { 1.0 - (-2.0 * t + 2.0).powi(3) / 2.0 };
        self.azimuth = anim.start_az + (anim.target_az - anim.start_az) * e;
        self.elevation = anim.start_el + (anim.target_el - anim.start_el) * e;
        let limit = std::f32::consts::FRAC_PI_2 - 0.01;
        self.elevation = self.elevation.clamp(-limit, limit);
        if t >= 1.0 {
            self.animation = None;
            false
        } else {
            true
        }
    }

    /// Camera position in world space.
    pub fn eye_position(&self) -> Vec3 {
        let cos_el = self.elevation.cos();
        let sin_el = self.elevation.sin();
        let cos_az = self.azimuth.cos();
        let sin_az = self.azimuth.sin();

        let offset =
            Vec3::new(self.distance * cos_el * sin_az, self.distance * sin_el, self.distance * cos_el * cos_az);

        self.target + offset
    }

    /// Place the camera so its derived eye sits at `eye` while it looks toward
    /// `target`, back-solving distance/azimuth/elevation. No-op if eye and
    /// target coincide. The perspective lock uses this and bypasses the orbit
    /// elevation clamp.
    pub fn set_eye_and_target(&mut self, eye: Vec3, target: Vec3) {
        let offset = eye - target;
        let distance = offset.norm();
        if distance < 1e-6 {
            return;
        }
        self.target = target;
        self.distance = distance;
        self.elevation = (offset.y / distance).clamp(-1.0, 1.0).asin();
        self.azimuth = offset.x.atan2(offset.z);
    }

    /// Compute the view matrix (world -> camera).
    pub fn view_matrix(&self) -> [[f32; 4]; 4] {
        let eye = self.eye_position();
        look_at(eye, self.target, Vec3::new(0.0, 1.0, 0.0))
    }

    /// Compute the projection matrix for a given aspect ratio.
    pub fn projection_matrix(&self, aspect: f32) -> [[f32; 4]; 4] {
        perspective(self.fov, aspect, self.near, self.far)
    }

    /// Handle mouse drag for orbiting.
    pub fn orbit(&mut self, delta: egui::Vec2, viewport_size: egui::Vec2) {
        self.animation = None;
        let sensitivity = 2.0 * std::f32::consts::PI / viewport_size.x.max(1.0);
        self.azimuth -= delta.x * sensitivity;
        self.elevation += delta.y * sensitivity;

        let limit = std::f32::consts::FRAC_PI_2 - 0.01;
        self.elevation = self.elevation.clamp(-limit, limit);
    }

    /// Handle scroll for zooming.
    pub fn zoom(&mut self, scroll_delta: f32) {
        let factor = (-scroll_delta * 0.002).exp();
        self.distance *= factor;
        self.distance = self.distance.clamp(self.near * 0.1, self.far * 0.5);
    }

    /// Handle middle-mouse drag for panning.
    pub fn pan(&mut self, delta: egui::Vec2, viewport_size: egui::Vec2) {
        let view = self.view_matrix();
        let view_na = mat4_to_na(view);

        // Extract right and up vectors from view matrix columns
        let right = Vec3::new(view_na[(0, 0)], view_na[(1, 0)], view_na[(2, 0)]);
        let up = Vec3::new(view_na[(0, 1)], view_na[(1, 1)], view_na[(2, 1)]);

        let scale = self.distance * (self.fov * 0.5).tan() * 2.0 / viewport_size.y.max(1.0);

        self.target -= (right * delta.x + up * delta.y) * scale;
    }

    /// Move the camera target using WASD-style keys.
    /// W/S move forward/backward along the camera's horizontal look direction.
    /// A/D strafe left/right. Speed scales with distance but has a floor.
    pub fn wasd(&mut self, forward: f32, right: f32) {
        // Forward direction projected onto horizontal plane (Y-up)
        let fwd = Vec3::new(-(self.azimuth.sin()), 0.0, -(self.azimuth.cos())).normalize();
        // Right direction (perpendicular to forward, horizontal)
        let rt = Vec3::new(-fwd.z, 0.0, fwd.x).normalize();

        let min_speed = self.far * 0.00005;
        let speed = (self.distance * 0.0025).max(min_speed);
        self.target += (fwd * forward + rt * right) * speed;
    }

    /// Move the camera target up/down (world Y axis).
    pub fn move_vertical(&mut self, amount: f32) {
        let min_speed = self.far * 0.00005;
        let speed = (self.distance * 0.0025).max(min_speed);
        self.target.y += amount * speed;
    }

    /// Rotate the camera around the target (azimuth).
    pub fn rotate_horizontal(&mut self, amount: f32) {
        self.azimuth += amount * 0.03;
    }

    /// Project a world-space point to screen coordinates within the given viewport rect.
    /// Returns `None` if the point is behind the camera.
    pub fn project_to_screen(&self, world_pos: Vec3, viewport_rect: egui::Rect) -> Option<egui::Pos2> {
        let aspect = viewport_rect.width() / viewport_rect.height().max(1.0);
        let view = mat4_to_na(self.view_matrix());
        let proj = mat4_to_na(self.projection_matrix(aspect));
        let vp = proj * view;

        let pos = Vector4::new(world_pos.x, world_pos.y, world_pos.z, 1.0);
        let clip = vp * pos;

        if clip.w <= 0.0 {
            return None; // behind camera
        }

        let ndc_x = clip.x / clip.w;
        let ndc_y = clip.y / clip.w;

        // NDC [-1,1] -> screen coords
        let sx = viewport_rect.left() + (ndc_x + 1.0) * 0.5 * viewport_rect.width();
        let sy = viewport_rect.top() + (1.0 - ndc_y) * 0.5 * viewport_rect.height();

        Some(egui::Pos2::new(sx, sy))
    }

    /// Handle standard 3D navigation input on a UI response.
    /// Left-drag = orbit, scroll = zoom, middle-drag = pan, double-click = reset.
    /// WASD = move camera target forward/left/backward/right.
    pub fn handle_input(&mut self, response: &egui::Response, bounds: Option<(Vec3, Vec3)>) {
        let viewport_size = response.rect.size();

        // Left-drag: orbit
        if response.dragged_by(egui::PointerButton::Primary) {
            self.orbit(response.drag_delta(), viewport_size);
        }

        // Middle-drag: pan
        if response.dragged_by(egui::PointerButton::Middle) {
            self.pan(response.drag_delta(), viewport_size);
        }

        // Scroll: zoom
        if response.hovered() {
            let scroll = response.ctx.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                self.zoom(scroll);
            }
        }

        // WASD: move camera target (only when viewport is hovered, to avoid capturing typing)
        if response.hovered() {
            let (fwd, rt) = response.ctx.input(|i| {
                let mut f = 0.0_f32;
                let mut r = 0.0_f32;
                if i.key_down(egui::Key::W) {
                    f += 1.0;
                }
                if i.key_down(egui::Key::S) {
                    f -= 1.0;
                }
                if i.key_down(egui::Key::A) {
                    r -= 1.0;
                }
                if i.key_down(egui::Key::D) {
                    r += 1.0;
                }
                (f, r)
            });
            if fwd != 0.0 || rt != 0.0 {
                self.wasd(fwd, rt);
            }

            // Arrow keys: Up/Down = move vertically, Left/Right = rotate around target
            let (vert, rot) = response.ctx.input(|i| {
                let mut v = 0.0_f32;
                let mut r = 0.0_f32;
                if i.key_down(egui::Key::ArrowUp) {
                    v += 1.0;
                }
                if i.key_down(egui::Key::ArrowDown) {
                    v -= 1.0;
                }
                if i.key_down(egui::Key::ArrowLeft) {
                    r += 1.0;
                }
                if i.key_down(egui::Key::ArrowRight) {
                    r -= 1.0;
                }
                (v, r)
            });
            if vert != 0.0 {
                self.move_vertical(vert);
            }
            if rot != 0.0 {
                self.rotate_horizontal(rot);
            }
        }

        // Double-click: reset camera
        if response.double_clicked()
            && let Some((min, max)) = bounds
        {
            self.reset(min, max);
        }
    }
}

// --- Conversion helpers between [[f32; 4]; 4] (col-major) and nalgebra Matrix4 ---

/// Convert our column-major `[[f32; 4]; 4]` into a `Matrix4<f32>`.
/// Our layout: `m[col][row]`, nalgebra stores column-major internally
/// but its `new()` constructor takes arguments in **row-major** order,
/// and `from_columns` takes column slices.
pub(crate) fn mat4_to_na(m: [[f32; 4]; 4]) -> Matrix4<f32> {
    Matrix4::from_columns(&[
        Vector4::new(m[0][0], m[0][1], m[0][2], m[0][3]),
        Vector4::new(m[1][0], m[1][1], m[1][2], m[1][3]),
        Vector4::new(m[2][0], m[2][1], m[2][2], m[2][3]),
        Vector4::new(m[3][0], m[3][1], m[3][2], m[3][3]),
    ])
}

/// Convert a `Matrix4<f32>` back to our column-major `[[f32; 4]; 4]`.
pub(crate) fn na_to_mat4(m: Matrix4<f32>) -> [[f32; 4]; 4] {
    let c0 = m.column(0);
    let c1 = m.column(1);
    let c2 = m.column(2);
    let c3 = m.column(3);
    [
        [c0[0], c0[1], c0[2], c0[3]],
        [c1[0], c1[1], c1[2], c1[3]],
        [c2[0], c2[1], c2[2], c2[3]],
        [c3[0], c3[1], c3[2], c3[3]],
    ]
}

/// Look-at matrix (right-handed, Y-up).
fn look_at(eye: Vec3, target: Vec3, up: Vec3) -> [[f32; 4]; 4] {
    let f = (target - eye).normalize();
    let s = f.cross(&up).normalize();
    let u = s.cross(&f);

    // Build the same column-major matrix as the original code:
    //   col0 = [s.x,  u.x, -f.x, 0]
    //   col1 = [s.y,  u.y, -f.y, 0]
    //   col2 = [s.z,  u.z, -f.z, 0]
    //   col3 = [-s.dot(eye), -u.dot(eye), f.dot(eye), 1]
    let m = Matrix4::new(
        s.x,
        s.y,
        s.z,
        -s.dot(&eye),
        u.x,
        u.y,
        u.z,
        -u.dot(&eye),
        -f.x,
        -f.y,
        -f.z,
        f.dot(&eye),
        0.0,
        0.0,
        0.0,
        1.0,
    );

    na_to_mat4(m)
}

/// Perspective projection matrix (right-handed, depth [0, 1]).
fn perspective(fov_y: f32, aspect: f32, near: f32, far: f32) -> [[f32; 4]; 4] {
    let f = 1.0 / (fov_y * 0.5).tan();
    let range = near - far;

    // Build the same column-major matrix as the original code.
    // Matrix4::new takes row-major arguments.
    let m = Matrix4::new(
        f / aspect,
        0.0,
        0.0,
        0.0,
        0.0,
        f,
        0.0,
        0.0,
        0.0,
        0.0,
        far / range,
        (near * far) / range,
        0.0,
        0.0,
        -1.0,
        0.0,
    );

    na_to_mat4(m)
}

/// Multiply two 4x4 matrices (column-major layout: m[col][row]).
pub(crate) fn mat4_mul(a: [[f32; 4]; 4], b: [[f32; 4]; 4]) -> [[f32; 4]; 4] {
    na_to_mat4(mat4_to_na(a) * mat4_to_na(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_eye_and_target_round_trips_eye() {
        let mut c = ArcballCamera::default();
        let cases = [
            (Vec3::new(10.0, 5.0, 3.0), Vec3::new(0.0, 0.0, 0.0)),
            (Vec3::new(-4.0, 2.0, 7.0), Vec3::new(1.0, 1.0, -1.0)),
            (Vec3::new(0.0, 8.0, 0.0), Vec3::new(0.0, 0.0, 0.0)),
        ];
        for (eye, target) in cases {
            c.set_eye_and_target(eye, target);
            let got = c.eye_position();
            assert!((got - eye).norm() < 1e-4, "eye {got:?} != {eye:?}");
            assert!((c.target - target).norm() < 1e-6);
        }
    }

    #[test]
    fn set_eye_and_target_aims_at_target() {
        let mut c = ArcballCamera::default();
        let eye = Vec3::new(5.0, 3.0, -2.0);
        let target = Vec3::new(-1.0, 0.5, 4.0);
        c.set_eye_and_target(eye, target);
        let look = (c.target - c.eye_position()).normalize();
        let want = (target - eye).normalize();
        assert!((look - want).norm() < 1e-4, "look {look:?} want {want:?}");
    }

    #[test]
    fn set_eye_and_target_noop_when_coincident() {
        let mut c = ArcballCamera::default();
        let before = (c.target, c.distance, c.azimuth, c.elevation);
        c.set_eye_and_target(Vec3::new(1.0, 1.0, 1.0), Vec3::new(1.0, 1.0, 1.0));
        assert_eq!(before, (c.target, c.distance, c.azimuth, c.elevation));
    }
}

#[cfg(test)]
mod gizmo_anim_tests {
    use super::*;
    use std::f32::consts::FRAC_PI_2;
    use std::f32::consts::PI;

    fn cam() -> ArcballCamera {
        ArcballCamera::from_bounds(Vec3::new(-1.0, -1.0, -1.0), Vec3::new(1.0, 1.0, 1.0))
    }

    #[test]
    fn animate_to_reaches_target_at_end() {
        let mut c = cam();
        c.animate_to(PI, 0.0, 0.4);
        let still = c.update_animation(1.0);
        assert!(!still);
        assert!((c.azimuth - PI).abs() < 1e-3, "az={}", c.azimuth);
        assert!(c.elevation.abs() < 1e-3, "el={}", c.elevation);
        assert!(c.animation.is_none());
    }

    #[test]
    fn animate_midpoint_is_between() {
        let mut c = cam();
        c.azimuth = 0.0;
        c.elevation = 0.0;
        c.animate_to(FRAC_PI_2, 0.0, 0.4);
        let still = c.update_animation(0.2);
        assert!(still);
        assert!(c.azimuth > 0.05 && c.azimuth < FRAC_PI_2 - 0.05, "az={}", c.azimuth);
    }

    #[test]
    fn azimuth_takes_shortest_path() {
        let mut c = cam();
        c.azimuth = 0.1;
        c.animate_to(2.0 * PI - 0.1, 0.0, 0.4);
        c.update_animation(0.2);
        let a = c.azimuth.rem_euclid(2.0 * PI);
        assert!(!(0.2..=2.0 * PI - 0.2).contains(&a), "az={}", c.azimuth);
    }

    #[test]
    fn orbit_cancels_animation() {
        let mut c = cam();
        c.animate_to(PI, 0.0, 0.4);
        c.orbit(egui::Vec2::new(10.0, 0.0), egui::Vec2::new(400.0, 400.0));
        assert!(c.animation.is_none());
    }

    #[test]
    fn ortho_views_are_correct() {
        let lim = FRAC_PI_2 - 0.01;
        assert_eq!(ortho_view(Axis::Z, true, 1.23), (0.0, 0.0));
        assert_eq!(ortho_view(Axis::Z, false, 1.23), (PI, 0.0));
        assert_eq!(ortho_view(Axis::X, true, 1.23), (FRAC_PI_2, 0.0));
        assert_eq!(ortho_view(Axis::X, false, 1.23), (-FRAC_PI_2, 0.0));
        assert_eq!(ortho_view(Axis::Y, true, 1.23), (1.23, lim));
        assert_eq!(ortho_view(Axis::Y, false, 1.23), (1.23, -lim));
    }
}
