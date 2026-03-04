extern crate nalgebra as na;
use na::Matrix4;
use na::Vector4;

use super::types::Vec3;

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
        }
    }

    /// Reset camera to frame the given bounding box.
    pub fn reset(&mut self, min: Vec3, max: Vec3) {
        *self = Self::from_bounds(min, max);
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
            let scroll = response.ctx.input(|i| i.raw_scroll_delta.y);
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
