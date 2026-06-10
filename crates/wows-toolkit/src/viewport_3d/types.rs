extern crate nalgebra as na;
/// 3D vector type used throughout the crate. Alias for `nalgebra::Vector3<f32>`.
pub type Vec3 = na::Vector3<f32>;

/// A single vertex sent to the GPU.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 4],
    pub uv: [f32; 2],
}

impl Vertex {
    pub const LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[
            // position
            wgpu::VertexAttribute { offset: 0, shader_location: 0, format: wgpu::VertexFormat::Float32x3 },
            // normal
            wgpu::VertexAttribute { offset: 12, shader_location: 1, format: wgpu::VertexFormat::Float32x3 },
            // color
            wgpu::VertexAttribute { offset: 24, shader_location: 2, format: wgpu::VertexFormat::Float32x4 },
            // uv
            wgpu::VertexAttribute { offset: 40, shader_location: 3, format: wgpu::VertexFormat::Float32x2 },
        ],
    };
}

/// Opaque handle to a mesh uploaded to the GPU.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MeshId(pub(crate) u64);

/// Result of a picking (hover) query.
#[derive(Clone, Debug)]
pub struct HitResult {
    pub mesh_id: MeshId,
    pub triangle_index: usize,
    pub distance: f32,
    pub world_position: Vec3,
}

/// Hull lighting parameters (a rendering input - lives in the generic viewport module).
/// Applied to meshes flagged `lit` (hull only); armor plates and world-space overlays
/// always render flat. `Default` is the In-Game preset, so persisted settings missing
/// this field load with the good-looking values.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct LightingSettings {
    /// Master switch. When false the hull renders flat (today's uniform look).
    pub enabled: bool,
    /// "Flat" (ambient) term intensity - the floor that keeps all angles visible.
    pub flat_intensity: f32,
    /// Ambient color.
    pub flat_color: [f32; 3],
    /// "Directional" (key) term intensity.
    pub key_intensity: f32,
    /// Key light color.
    pub key_color: [f32; 3],
    /// Key light azimuth in degrees (world space).
    pub azimuth_deg: f32,
    /// Key light elevation in degrees (world space).
    pub elevation_deg: f32,
    /// Rim/fresnel strength.
    pub rim_strength: f32,
    /// Rim/fresnel falloff exponent.
    pub rim_power: f32,
    /// Specular highlight strength.
    pub specular_strength: f32,
    /// Specular exponent (shininess).
    pub shininess: f32,
}

impl Default for LightingSettings {
    fn default() -> Self {
        Self::in_game()
    }
}

impl LightingSettings {
    /// Tuned to read like the in-game model while keeping every angle visible.
    pub fn in_game() -> Self {
        Self {
            enabled: true,
            flat_intensity: 0.5,
            flat_color: [1.0, 1.0, 1.0],
            key_intensity: 0.6,
            key_color: [1.0, 0.97, 0.92],
            azimuth_deg: 135.0,
            elevation_deg: 45.0,
            rim_strength: 0.15,
            rim_power: 3.0,
            specular_strength: 0.2,
            shininess: 24.0,
        }
    }

    /// Reproduces the legacy uniform look: pure ambient, no directional/rim/specular.
    pub fn flat() -> Self {
        Self {
            enabled: true,
            flat_intensity: 1.0,
            flat_color: [1.0, 1.0, 1.0],
            key_intensity: 0.0,
            key_color: [1.0, 1.0, 1.0],
            azimuth_deg: 135.0,
            elevation_deg: 45.0,
            rim_strength: 0.0,
            rim_power: 3.0,
            specular_strength: 0.0,
            shininess: 24.0,
        }
    }

    /// Lower ambient, stronger directional + specular for dramatic screenshots.
    pub fn studio() -> Self {
        Self {
            enabled: true,
            flat_intensity: 0.3,
            flat_color: [0.9, 0.93, 1.0],
            key_intensity: 0.9,
            key_color: [1.0, 0.98, 0.95],
            azimuth_deg: 120.0,
            elevation_deg: 35.0,
            rim_strength: 0.3,
            rim_power: 4.0,
            specular_strength: 0.5,
            shininess: 48.0,
        }
    }

    /// World-space unit vector pointing toward the light, from azimuth/elevation.
    pub fn light_dir_world(&self) -> [f32; 3] {
        let az = self.azimuth_deg.to_radians();
        let el = self.elevation_deg.to_radians();
        let ce = el.cos();
        [ce * az.sin(), el.sin(), ce * az.cos()]
    }

    /// Flat color premultiplied by flat intensity (ready for the shader).
    pub fn flat_rgb(&self) -> [f32; 3] {
        [self.flat_color[0] * self.flat_intensity, self.flat_color[1] * self.flat_intensity, self.flat_color[2] * self.flat_intensity]
    }

    /// Key color premultiplied by key intensity (ready for the shader).
    pub fn key_rgb(&self) -> [f32; 3] {
        [self.key_color[0] * self.key_intensity, self.key_color[1] * self.key_intensity, self.key_color[2] * self.key_intensity]
    }
}

/// Uniform data sent to the vertex/fragment shader.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Uniforms {
    pub mvp: [[f32; 4]; 4],
    pub model_view: [[f32; 4]; 4],
    pub light_dir: [f32; 4],
}

#[cfg(test)]
mod lighting_tests {
    use super::*;

    #[test]
    fn default_is_in_game_preset() {
        assert_eq!(LightingSettings::default(), LightingSettings::in_game());
    }

    #[test]
    fn flat_preset_has_no_directional_or_rim() {
        let f = LightingSettings::flat();
        assert!(f.enabled);
        assert_eq!(f.key_intensity, 0.0);
        assert_eq!(f.rim_strength, 0.0);
        assert_eq!(f.specular_strength, 0.0);
        assert_eq!(f.flat_intensity, 1.0);
    }

    #[test]
    fn light_dir_world_is_unit_and_points_up_for_positive_elevation() {
        let mut s = LightingSettings::in_game();
        s.azimuth_deg = 0.0;
        s.elevation_deg = 90.0;
        let d = s.light_dir_world();
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        assert!((len - 1.0).abs() < 1e-5, "not unit: {d:?}");
        assert!(d[1] > 0.99, "elevation 90 should point +Y: {d:?}");
    }

    #[test]
    fn serde_round_trips_and_missing_fields_default_to_in_game() {
        let json = serde_json::to_string(&LightingSettings::studio()).unwrap();
        let back: LightingSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(back, LightingSettings::studio());
        // An empty object should fill every field from Default (= in_game).
        let empty: LightingSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(empty, LightingSettings::in_game());
    }
}
