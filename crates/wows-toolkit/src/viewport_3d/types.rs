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

/// Uniform data sent to the vertex/fragment shader.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Uniforms {
    pub mvp: [[f32; 4]; 4],
    pub model_view: [[f32; 4]; 4],
    pub light_dir: [f32; 4],
}
