/// Arcball orbital camera for 3D model inspection.
#[derive(Clone, Debug)]
pub struct ArcballCamera {
    /// Center of rotation (typically model center from bounding box).
    pub target: [f32; 3],
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
            target: [0.0, 0.0, 0.0],
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
    pub fn from_bounds(min: [f32; 3], max: [f32; 3]) -> Self {
        let center = [(min[0] + max[0]) * 0.5, (min[1] + max[1]) * 0.5, (min[2] + max[2]) * 0.5];
        let extent = [max[0] - min[0], max[1] - min[1], max[2] - min[2]];
        let max_extent = extent[0].max(extent[1]).max(extent[2]);
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
    pub fn reset(&mut self, min: [f32; 3], max: [f32; 3]) {
        *self = Self::from_bounds(min, max);
    }

    /// Camera position in world space.
    pub fn eye_position(&self) -> [f32; 3] {
        let cos_el = self.elevation.cos();
        let sin_el = self.elevation.sin();
        let cos_az = self.azimuth.cos();
        let sin_az = self.azimuth.sin();

        [
            self.target[0] + self.distance * cos_el * sin_az,
            self.target[1] + self.distance * sin_el,
            self.target[2] + self.distance * cos_el * cos_az,
        ]
    }

    /// Compute the view matrix (world -> camera).
    pub fn view_matrix(&self) -> [[f32; 4]; 4] {
        let eye = self.eye_position();
        look_at(eye, self.target, [0.0, 1.0, 0.0])
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

        // Extract right and up vectors from view matrix
        let right = [view[0][0], view[1][0], view[2][0]];
        let up = [view[0][1], view[1][1], view[2][1]];

        let scale = self.distance * (self.fov * 0.5).tan() * 2.0 / viewport_size.y.max(1.0);

        self.target[0] -= (right[0] * delta.x + up[0] * delta.y) * scale;
        self.target[1] -= (right[1] * delta.x + up[1] * delta.y) * scale;
        self.target[2] -= (right[2] * delta.x + up[2] * delta.y) * scale;
    }

    /// Move the camera target using WASD-style keys.
    /// W/S move forward/backward along the camera's horizontal look direction.
    /// A/D strafe left/right. Speed scales with distance but has a floor.
    pub fn wasd(&mut self, forward: f32, right: f32) {
        // Forward direction projected onto horizontal plane (Y-up)
        let fwd = normalize([-(self.azimuth.sin()), 0.0, -(self.azimuth.cos())]);
        // Right direction (perpendicular to forward, horizontal)
        let rt = normalize([-fwd[2], 0.0, fwd[0]]);

        let min_speed = self.far * 0.0004;
        let speed = (self.distance * 0.02).max(min_speed);
        self.target[0] += (fwd[0] * forward + rt[0] * right) * speed;
        self.target[1] += (fwd[1] * forward + rt[1] * right) * speed;
        self.target[2] += (fwd[2] * forward + rt[2] * right) * speed;
    }

    /// Move the camera target up/down (world Y axis).
    pub fn move_vertical(&mut self, amount: f32) {
        let min_speed = self.far * 0.0004;
        let speed = (self.distance * 0.02).max(min_speed);
        self.target[1] += amount * speed;
    }

    /// Rotate the camera around the target (azimuth).
    pub fn rotate_horizontal(&mut self, amount: f32) {
        self.azimuth += amount * 0.03;
    }

    /// Handle standard 3D navigation input on a UI response.
    /// Left-drag = orbit, scroll = zoom, middle-drag = pan, double-click = reset.
    /// WASD = move camera target forward/left/backward/right.
    pub fn handle_input(&mut self, response: &egui::Response, bounds: Option<([f32; 3], [f32; 3])>) {
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
        if response.double_clicked() {
            if let Some((min, max)) = bounds {
                self.reset(min, max);
            }
        }
    }
}

/// Look-at matrix (right-handed, Y-up).
fn look_at(eye: [f32; 3], target: [f32; 3], up: [f32; 3]) -> [[f32; 4]; 4] {
    let f = normalize(sub(target, eye));
    let s = normalize(cross(f, up));
    let u = cross(s, f);

    [
        [s[0], u[0], -f[0], 0.0],
        [s[1], u[1], -f[1], 0.0],
        [s[2], u[2], -f[2], 0.0],
        [-dot(s, eye), -dot(u, eye), dot(f, eye), 1.0],
    ]
}

/// Perspective projection matrix (right-handed, depth [0, 1]).
fn perspective(fov_y: f32, aspect: f32, near: f32, far: f32) -> [[f32; 4]; 4] {
    let f = 1.0 / (fov_y * 0.5).tan();
    let range = near - far;

    [
        [f / aspect, 0.0, 0.0, 0.0],
        [0.0, f, 0.0, 0.0],
        [0.0, 0.0, far / range, -1.0],
        [0.0, 0.0, (near * far) / range, 0.0],
    ]
}

// --- Vec3 math helpers ---

pub(crate) fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub(crate) fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub(crate) fn scale(a: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

pub(crate) fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub(crate) fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

pub(crate) fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = dot(v, v).sqrt();
    if len < 1e-10 {
        return [0.0, 0.0, 0.0];
    }
    [v[0] / len, v[1] / len, v[2] / len]
}

/// Multiply two 4x4 matrices (column-major layout: m[col][row]).
pub(crate) fn mat4_mul(a: [[f32; 4]; 4], b: [[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut out = [[0.0f32; 4]; 4];
    for col in 0..4 {
        for row in 0..4 {
            out[col][row] =
                a[0][row] * b[col][0] + a[1][row] * b[col][1] + a[2][row] * b[col][2] + a[3][row] * b[col][3];
        }
    }
    out
}

/// Invert a 4x4 matrix. Returns None if singular.
pub(crate) fn mat4_inverse(m: [[f32; 4]; 4]) -> Option<[[f32; 4]; 4]> {
    // Flatten to compute cofactors
    let m = [
        m[0][0], m[0][1], m[0][2], m[0][3], m[1][0], m[1][1], m[1][2], m[1][3], m[2][0], m[2][1], m[2][2], m[2][3],
        m[3][0], m[3][1], m[3][2], m[3][3],
    ];

    let mut inv = [0.0f32; 16];

    inv[0] =
        m[5] * m[10] * m[15] - m[5] * m[11] * m[14] - m[9] * m[6] * m[15] + m[9] * m[7] * m[14] + m[13] * m[6] * m[11]
            - m[13] * m[7] * m[10];
    inv[4] =
        -m[4] * m[10] * m[15] + m[4] * m[11] * m[14] + m[8] * m[6] * m[15] - m[8] * m[7] * m[14] - m[12] * m[6] * m[11]
            + m[12] * m[7] * m[10];
    inv[8] =
        m[4] * m[9] * m[15] - m[4] * m[11] * m[13] - m[8] * m[5] * m[15] + m[8] * m[7] * m[13] + m[12] * m[5] * m[11]
            - m[12] * m[7] * m[9];
    inv[12] =
        -m[4] * m[9] * m[14] + m[4] * m[10] * m[13] + m[8] * m[5] * m[14] - m[8] * m[6] * m[13] - m[12] * m[5] * m[10]
            + m[12] * m[6] * m[9];
    inv[1] =
        -m[1] * m[10] * m[15] + m[1] * m[11] * m[14] + m[9] * m[2] * m[15] - m[9] * m[3] * m[14] - m[13] * m[2] * m[11]
            + m[13] * m[3] * m[10];
    inv[5] =
        m[0] * m[10] * m[15] - m[0] * m[11] * m[14] - m[8] * m[2] * m[15] + m[8] * m[3] * m[14] + m[12] * m[2] * m[11]
            - m[12] * m[3] * m[10];
    inv[9] =
        -m[0] * m[9] * m[15] + m[0] * m[11] * m[13] + m[8] * m[1] * m[15] - m[8] * m[3] * m[13] - m[12] * m[1] * m[11]
            + m[12] * m[3] * m[9];
    inv[13] =
        m[0] * m[9] * m[14] - m[0] * m[10] * m[13] - m[8] * m[1] * m[14] + m[8] * m[2] * m[13] + m[12] * m[1] * m[10]
            - m[12] * m[2] * m[9];
    inv[2] =
        m[1] * m[6] * m[15] - m[1] * m[7] * m[14] - m[5] * m[2] * m[15] + m[5] * m[3] * m[14] + m[13] * m[2] * m[7]
            - m[13] * m[3] * m[6];
    inv[6] =
        -m[0] * m[6] * m[15] + m[0] * m[7] * m[14] + m[4] * m[2] * m[15] - m[4] * m[3] * m[14] - m[12] * m[2] * m[7]
            + m[12] * m[3] * m[6];
    inv[10] =
        m[0] * m[5] * m[15] - m[0] * m[7] * m[13] - m[4] * m[1] * m[15] + m[4] * m[3] * m[13] + m[12] * m[1] * m[7]
            - m[12] * m[3] * m[5];
    inv[14] =
        -m[0] * m[5] * m[14] + m[0] * m[6] * m[13] + m[4] * m[1] * m[14] - m[4] * m[2] * m[13] - m[12] * m[1] * m[6]
            + m[12] * m[2] * m[5];
    inv[3] =
        -m[1] * m[6] * m[11] + m[1] * m[7] * m[10] + m[5] * m[2] * m[11] - m[5] * m[3] * m[10] - m[9] * m[2] * m[7]
            + m[9] * m[3] * m[6];
    inv[7] = m[0] * m[6] * m[11] - m[0] * m[7] * m[10] - m[4] * m[2] * m[11] + m[4] * m[3] * m[10] + m[8] * m[2] * m[7]
        - m[8] * m[3] * m[6];
    inv[11] = -m[0] * m[5] * m[11] + m[0] * m[7] * m[9] + m[4] * m[1] * m[11] - m[4] * m[3] * m[9] - m[8] * m[1] * m[7]
        + m[8] * m[3] * m[5];
    inv[15] = m[0] * m[5] * m[10] - m[0] * m[6] * m[9] - m[4] * m[1] * m[10] + m[4] * m[2] * m[9] + m[8] * m[1] * m[6]
        - m[8] * m[2] * m[5];

    let det = m[0] * inv[0] + m[1] * inv[4] + m[2] * inv[8] + m[3] * inv[12];
    if det.abs() < 1e-10 {
        return None;
    }

    let inv_det = 1.0 / det;
    let mut result = [[0.0f32; 4]; 4];
    for col in 0..4 {
        for row in 0..4 {
            result[col][row] = inv[col * 4 + row] * inv_det;
        }
    }
    Some(result)
}
