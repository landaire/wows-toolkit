use std::collections::HashMap;

extern crate nalgebra as na;
use na::Rotation3;
use na::Vector3;

use crate::viewport::camera::ArcballCamera;
use crate::viewport::camera::mat4_mul;
use crate::viewport::picking;
use crate::viewport::picking::PickableMesh;
use crate::viewport::types::HitResult;
use crate::viewport::types::LightingSettings;
use crate::viewport::types::MeshId;
use crate::viewport::types::Uniforms;
use crate::viewport::types::Vec2;
use crate::viewport::types::Vec3;
use crate::viewport::types::Vertex;
use crate::viewport::types::ViewRect;

const MAT4_IDENTITY: [[f32; 4]; 4] =
    [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

const SHADER_SOURCE: &str = r#"
struct Uniforms {
    mvp: mat4x4<f32>,
    model_view: mat4x4<f32>,
    light_dir: vec4<f32>,
    flat_color: vec4<f32>,
    key_color: vec4<f32>,
    params: vec4<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

@group(1) @binding(0) var diffuse_texture: texture_2d<f32>;
@group(1) @binding(1) var diffuse_sampler: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) uv: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) normal_vs: vec3<f32>,
    @location(2) position_vs: vec3<f32>,
    @location(3) uv: vec2<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = uniforms.mvp * vec4(in.position, 1.0);
    out.normal_vs = (uniforms.model_view * vec4(in.normal, 0.0)).xyz;
    out.position_vs = (uniforms.model_view * vec4(in.position, 1.0)).xyz;
    out.color = in.color;
    out.uv = in.uv;
    return out;
}

@fragment
fn fs_main(in: VertexOutput, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    // Sample texture and multiply with vertex color.
    // Non-textured meshes bind a 1x1 white fallback, so this is a passthrough.
    let tex_color = textureSample(diffuse_texture, diffuse_sampler, in.uv);
    let base_color = tex_color * in.color;

    var color = base_color.rgb;
    if (uniforms.light_dir.w > 0.5) {
        // Hull lighting: half-Lambert key over a flat ambient floor, plus rim and specular.
        // Half-Lambert keeps the far side lit (never fully black) so all angles stay visible.
        // The hull is double-sided; flip the normal on back faces so both sides shade
        // consistently and the see-through hull does not show a hard front/back seam.
        var N = normalize(in.normal_vs);
        if (!front_facing) {
            N = -N;
        }
        let V = normalize(-in.position_vs);
        let L = normalize(uniforms.light_dir.xyz);
        let half_lambert = dot(N, L) * 0.5 + 0.5;
        let H = normalize(L + V);
        let spec = pow(max(dot(N, H), 0.0), uniforms.params.w) * uniforms.params.z;
        let rim = pow(1.0 - max(dot(N, V), 0.0), uniforms.params.y) * uniforms.params.x;
        // Fade the directional terms (key, rim, specular) as the surface becomes
        // transparent. A see-through hull shows its near and far walls at once, and a
        // directional light shades those two walls differently, forming a hard seam.
        // Leaning on the seamless flat ambient term for transparent surfaces hides it;
        // opaque geometry occludes its far wall via depth and keeps full directional.
        let dir_factor = smoothstep(0.5, 1.0, base_color.a);
        let directional = (rim + spec) * dir_factor;
        let lighting = uniforms.flat_color.rgb + uniforms.key_color.rgb * half_lambert * dir_factor;
        color = base_color.rgb * lighting + vec3<f32>(directional, directional, directional);
    }

    return vec4(color, base_color.a);
}
"#;

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const MSAA_SAMPLE_COUNT: u32 = 4;

/// Shared GPU resources (created once, reusable across viewports).
pub struct GpuPipeline {
    /// Pipeline with depth writes enabled — used for opaque geometry (armor).
    pipeline: wgpu::RenderPipeline,
    /// Pipeline without depth writes — used for transparent hull geometry.
    pipeline_no_depth_write: wgpu::RenderPipeline,
    /// Pipeline that ignores depth — used for highlight overlays (always on top).
    pipeline_overlay: wgpu::RenderPipeline,
    uniform_bind_group_layout: wgpu::BindGroupLayout,
    texture_bind_group_layout: wgpu::BindGroupLayout,
    /// Shared sampler for all diffuse textures.
    default_sampler: wgpu::Sampler,
    /// 1x1 white texture bind group — bound for non-textured meshes.
    fallback_texture_bind_group: wgpu::BindGroup,
}

impl GpuPipeline {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("viewport_3d_shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_SOURCE.into()),
        });

        let uniform_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("viewport_3d_uniform_bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let texture_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("viewport_3d_texture_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("viewport_3d_pipeline_layout"),
            bind_group_layouts: &[Some(&uniform_bind_group_layout), Some(&texture_bind_group_layout)],
            immediate_size: 0,
        });

        let vertex_state = wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[Vertex::LAYOUT],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        };

        let fragment_state = wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: COLOR_FORMAT,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        };

        let primitive_state = wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None, // Double-sided
            ..Default::default()
        };

        // Pipeline with depth writes (for opaque armor meshes).
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("viewport_3d_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: vertex_state.clone(),
            fragment: Some(fragment_state.clone()),
            primitive: primitive_state,
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: MSAA_SAMPLE_COUNT,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview_mask: None,
            cache: None,
        });

        // Pipeline without depth writes (for transparent hull).
        let pipeline_no_depth_write = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("viewport_3d_pipeline_no_depth_write"),
            layout: Some(&pipeline_layout),
            vertex: vertex_state.clone(),
            fragment: Some(fragment_state.clone()),
            primitive: primitive_state,
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState {
                    constant: 2, // push transparent geometry slightly behind opaque
                    slope_scale: 1.0,
                    clamp: 0.0,
                },
            }),
            multisample: wgpu::MultisampleState {
                count: MSAA_SAMPLE_COUNT,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview_mask: None,
            cache: None,
        });

        // Pipeline for overlays — ignores depth so highlights are always visible.
        let pipeline_overlay = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("viewport_3d_pipeline_overlay"),
            layout: Some(&pipeline_layout),
            vertex: vertex_state,
            fragment: Some(fragment_state),
            primitive: primitive_state,
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: MSAA_SAMPLE_COUNT,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview_mask: None,
            cache: None,
        });

        // Create shared sampler (repeat wrapping, linear filtering).
        let default_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("viewport_3d_default_sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });

        // Create 1x1 white fallback texture.
        let fallback_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("viewport_3d_fallback_texture"),
            size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &fallback_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &[255u8, 255, 255, 255],
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4), rows_per_image: Some(1) },
            wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        );
        let fallback_view = fallback_texture.create_view(&Default::default());
        let fallback_texture_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("viewport_3d_fallback_texture_bg"),
            layout: &texture_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&fallback_view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&default_sampler) },
            ],
        });

        Self {
            pipeline,
            pipeline_no_depth_write,
            pipeline_overlay,
            uniform_bind_group_layout,
            texture_bind_group_layout,
            default_sampler,
            fallback_texture_bind_group,
        }
    }

    /// Create a texture bind group from RGBA8 pixel data.
    pub fn create_texture_bind_group(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        rgba_data: &[u8],
        width: u32,
        height: u32,
    ) -> wgpu::BindGroup {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("viewport_3d_hull_texture"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba_data,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * width), rows_per_image: Some(height) },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );

        let view = texture.create_view(&Default::default());
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("viewport_3d_texture_bg"),
            layout: &self.texture_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.default_sampler) },
            ],
        })
    }
}

/// Render layer constants. Lower values draw first (behind), higher values draw last (on top).
/// - Layers <= LAYER_OPAQUE_MAX: depth-writing pipeline (opaque armor).
/// - LAYER_HULL: no-depth-write pipeline with depth test (transparent hull, behind armor).
/// - LAYER_OVERLAY: no depth test at all (highlight overlays, always visible on top).
pub const LAYER_DEFAULT: i32 = 0;
pub const LAYER_HULL: i32 = 1;
pub const LAYER_OVERLAY: i32 = 2;

/// Layers at or below this value write to the depth buffer (opaque pass).
const LAYER_OPAQUE_MAX: i32 = 0;

/// Per-mesh GPU buffers.
struct GpuMesh {
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    index_count: u32,
    visible: bool,
    layer: i32,
    /// Optional per-mesh texture bind group. When None, the fallback white texture is used.
    texture_bind_group: Option<wgpu::BindGroup>,
    /// If true, this mesh is in world space and should NOT be affected by model_roll.
    world_space: bool,
    /// If true, this mesh participates in hull lighting (when lighting is enabled).
    lit: bool,
}

/// A complete 3D viewport instance. Each consumer (armor pane, replay viewer, etc.)
/// creates one of these. Holds its own camera and scene meshes; rendering goes
/// through `render_offscreen_rgba` into a CPU-readback buffer.
pub struct Viewport3D {
    pub camera: ArcballCamera,
    pub gizmo: crate::viewport::gizmo::NavGizmo,
    meshes: HashMap<MeshId, GpuMesh>,
    pick_data: HashMap<MeshId, PickableMesh>,
    next_mesh_id: u64,
    pub clear_color: wgpu::Color,
    /// Whether the scene has changed and needs re-rendering.
    needs_redraw: bool,
    /// Model roll angle in radians (rotation around the longitudinal/Z axis).
    pub model_roll: f32,
    /// Model yaw angle in radians (rotation around the vertical/Y axis).
    pub model_yaw: f32,
    /// Cursor position in NDC ([-1,1] range), updated each frame for flashlight lighting.
    pub cursor_ndc: Option<[f32; 2]>,
    /// Hull lighting parameters used when building per-frame uniforms.
    pub lighting: LightingSettings,
}

impl Default for Viewport3D {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BindKind {
    World,
    ModelLit,
    ModelUnlit,
}

impl Viewport3D {
    pub fn new() -> Self {
        Self {
            camera: ArcballCamera::default(),
            gizmo: crate::viewport::gizmo::NavGizmo::default(),
            meshes: HashMap::new(),
            pick_data: HashMap::new(),
            next_mesh_id: 0,
            clear_color: wgpu::Color { r: 0.12, g: 0.12, b: 0.18, a: 1.0 },
            needs_redraw: true,
            model_roll: 0.0,
            model_yaw: 0.0,
            cursor_ndc: None,
            lighting: LightingSettings::default(),
        }
    }

    /// Upload a mesh to the GPU. Returns a MeshId for later reference.
    pub fn add_mesh(&mut self, device: &wgpu::Device, vertices: &[Vertex], indices: &[u32], layer: i32) -> MeshId {
        use wgpu::util::DeviceExt;

        let id = MeshId(self.next_mesh_id);
        self.next_mesh_id += 1;

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("viewport_3d_vb_{}", id.0)),
            contents: bytemuck::cast_slice(vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("viewport_3d_ib_{}", id.0)),
            contents: bytemuck::cast_slice(indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        self.meshes.insert(
            id,
            GpuMesh {
                world_space: false,
                lit: false,
                vertex_buffer,
                index_buffer,
                index_count: indices.len() as u32,
                visible: true,
                layer,
                texture_bind_group: None,
            },
        );

        // Keep CPU-side data for picking
        let positions: Vec<[f32; 3]> = vertices.iter().map(|v| v.position).collect();
        self.pick_data.insert(id, PickableMesh { positions, indices: indices.to_vec() });

        self.needs_redraw = true;
        id
    }

    /// Upload a textured mesh to the GPU. The texture bind group is bound per-mesh during rendering.
    pub fn add_textured_mesh(
        &mut self,
        device: &wgpu::Device,
        vertices: &[Vertex],
        indices: &[u32],
        layer: i32,
        texture_bind_group: wgpu::BindGroup,
    ) -> MeshId {
        use wgpu::util::DeviceExt;

        let id = MeshId(self.next_mesh_id);
        self.next_mesh_id += 1;

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("viewport_3d_tex_vb_{}", id.0)),
            contents: bytemuck::cast_slice(vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("viewport_3d_tex_ib_{}", id.0)),
            contents: bytemuck::cast_slice(indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        self.meshes.insert(
            id,
            GpuMesh {
                world_space: false,
                lit: false,
                vertex_buffer,
                index_buffer,
                index_count: indices.len() as u32,
                visible: true,
                layer,
                texture_bind_group: Some(texture_bind_group),
            },
        );

        // Keep CPU-side data for picking
        let positions: Vec<[f32; 3]> = vertices.iter().map(|v| v.position).collect();
        self.pick_data.insert(id, PickableMesh { positions, indices: indices.to_vec() });

        self.needs_redraw = true;
        id
    }

    /// Add a mesh that is rendered on a given layer but excluded from picking.
    pub fn add_non_pickable_mesh(
        &mut self,
        device: &wgpu::Device,
        vertices: &[Vertex],
        indices: &[u32],
        layer: i32,
    ) -> MeshId {
        use wgpu::util::DeviceExt;

        let id = MeshId(self.next_mesh_id);
        self.next_mesh_id += 1;

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("viewport_3d_np_vb_{}", id.0)),
            contents: bytemuck::cast_slice(vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("viewport_3d_np_ib_{}", id.0)),
            contents: bytemuck::cast_slice(indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        self.meshes.insert(
            id,
            GpuMesh {
                world_space: false,
                lit: false,
                vertex_buffer,
                index_buffer,
                index_count: indices.len() as u32,
                visible: true,
                layer,
                texture_bind_group: None,
            },
        );

        self.needs_redraw = true;
        id
    }

    /// Add a non-pickable mesh that stays in world space (unaffected by model_roll).
    pub fn add_world_space_mesh(
        &mut self,
        device: &wgpu::Device,
        vertices: &[Vertex],
        indices: &[u32],
        layer: i32,
    ) -> MeshId {
        use wgpu::util::DeviceExt;

        let id = MeshId(self.next_mesh_id);
        self.next_mesh_id += 1;

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("viewport_3d_ws_vb_{}", id.0)),
            contents: bytemuck::cast_slice(vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("viewport_3d_ws_ib_{}", id.0)),
            contents: bytemuck::cast_slice(indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        self.meshes.insert(
            id,
            GpuMesh {
                world_space: true,
                lit: false,
                vertex_buffer,
                index_buffer,
                index_count: indices.len() as u32,
                visible: true,
                layer,
                texture_bind_group: None,
            },
        );

        self.needs_redraw = true;
        id
    }

    /// Add a non-pickable textured mesh.
    pub fn add_textured_non_pickable_mesh(
        &mut self,
        device: &wgpu::Device,
        vertices: &[Vertex],
        indices: &[u32],
        layer: i32,
        texture_bind_group: wgpu::BindGroup,
    ) -> MeshId {
        use wgpu::util::DeviceExt;

        let id = MeshId(self.next_mesh_id);
        self.next_mesh_id += 1;

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("viewport_3d_tex_np_vb_{}", id.0)),
            contents: bytemuck::cast_slice(vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("viewport_3d_tex_np_ib_{}", id.0)),
            contents: bytemuck::cast_slice(indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        self.meshes.insert(
            id,
            GpuMesh {
                world_space: false,
                lit: false,
                vertex_buffer,
                index_buffer,
                index_count: indices.len() as u32,
                visible: true,
                layer,
                texture_bind_group: Some(texture_bind_group),
            },
        );

        self.needs_redraw = true;
        id
    }

    /// Add a mesh that is rendered but excluded from picking (e.g. highlight overlays).
    pub fn add_overlay_mesh(&mut self, device: &wgpu::Device, vertices: &[Vertex], indices: &[u32]) -> MeshId {
        use wgpu::util::DeviceExt;

        let id = MeshId(self.next_mesh_id);
        self.next_mesh_id += 1;

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("viewport_3d_overlay_vb_{}", id.0)),
            contents: bytemuck::cast_slice(vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("viewport_3d_overlay_ib_{}", id.0)),
            contents: bytemuck::cast_slice(indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        self.meshes.insert(
            id,
            GpuMesh {
                world_space: false,
                lit: false,
                vertex_buffer,
                index_buffer,
                index_count: indices.len() as u32,
                visible: true,
                layer: LAYER_OVERLAY,
                texture_bind_group: None,
            },
        );

        // No pick_data entry — this mesh is invisible to picking
        self.needs_redraw = true;
        id
    }

    /// Remove a mesh and free GPU resources.
    pub fn remove_mesh(&mut self, id: MeshId) {
        self.meshes.remove(&id);
        self.pick_data.remove(&id);
        self.needs_redraw = true;
    }

    /// Set mesh visibility.
    pub fn set_visible(&mut self, id: MeshId, visible: bool) {
        if let Some(mesh) = self.meshes.get_mut(&id)
            && mesh.visible != visible
        {
            mesh.visible = visible;
            self.needs_redraw = true;
        }
    }

    /// Clear all meshes.
    pub fn clear(&mut self) {
        self.meshes.clear();
        self.pick_data.clear();
        self.needs_redraw = true;
    }

    /// Mark the viewport as needing a redraw (e.g. after camera change).
    pub fn mark_dirty(&mut self) {
        self.needs_redraw = true;
    }

    /// Mark a mesh as world-space (unaffected by model_roll).
    pub fn set_world_space(&mut self, id: MeshId, world_space: bool) {
        if let Some(mesh) = self.meshes.get_mut(&id) {
            mesh.world_space = world_space;
        }
    }

    /// Mark a mesh as lit (participates in hull lighting). Default is unlit.
    pub fn set_lit(&mut self, id: MeshId, lit: bool) {
        if let Some(mesh) = self.meshes.get_mut(&id) {
            mesh.lit = lit;
            self.needs_redraw = true;
        }
    }

    /// Whether the viewport needs a redraw.
    pub fn is_dirty(&self) -> bool {
        self.needs_redraw
    }

    /// Returns true if the viewport has any meshes to render.
    pub fn has_meshes(&self) -> bool {
        !self.meshes.is_empty()
    }

    /// Build the three per-frame uniform sets from the current camera, model
    /// rotation, and lighting: `(model_lit, world, model_unlit)`. Consumed by the
    /// offscreen readback path (`render_offscreen_rgba`).
    fn scene_uniforms(&self, size: (u32, u32)) -> (Uniforms, Uniforms, Uniforms) {
        let aspect = size.0 as f32 / size.1 as f32;
        let model_mat = {
            let has_roll = self.model_roll.abs() > 1e-6;
            let has_yaw = self.model_yaw.abs() > 1e-6;
            if !has_roll && !has_yaw {
                MAT4_IDENTITY
            } else {
                // model_mat = Ry(+yaw) * Rz(-roll): roll first (in model space), then yaw.
                let rz = Rotation3::from_axis_angle(&Vector3::z_axis(), -self.model_roll);
                let ry = Rotation3::from_axis_angle(&Vector3::y_axis(), self.model_yaw);
                let rot = ry * rz;
                let m = rot.to_homogeneous();
                // nalgebra Matrix4 is column-major; extract as [[f32;4];4] where arr[col][row].
                let s = m.as_slice();
                [
                    [s[0], s[1], s[2], s[3]],
                    [s[4], s[5], s[6], s[7]],
                    [s[8], s[9], s[10], s[11]],
                    [s[12], s[13], s[14], s[15]],
                ]
            }
        };
        let view_mat = self.camera.view_matrix();
        let proj_mat = self.camera.projection_matrix(aspect);
        let model_view = mat4_mul(view_mat, model_mat);
        let mvp = mat4_mul(proj_mat, model_view);

        // World-fixed key light: transform the world-space direction into view space
        // (rotation only, w=0) so highlights stay on a fixed world side as the camera
        // orbits. The model rotation must NOT affect the light, so this uses the pure
        // view matrix, not model_view.
        let lw = self.lighting.light_dir_world();
        let lv = [
            view_mat[0][0] * lw[0] + view_mat[1][0] * lw[1] + view_mat[2][0] * lw[2],
            view_mat[0][1] * lw[0] + view_mat[1][1] * lw[1] + view_mat[2][1] * lw[2],
            view_mat[0][2] * lw[0] + view_mat[1][2] * lw[1] + view_mat[2][2] * lw[2],
        ];
        let len = (lv[0] * lv[0] + lv[1] * lv[1] + lv[2] * lv[2]).sqrt().max(1e-6);
        let lit_flag = if self.lighting.enabled { 1.0 } else { 0.0 };
        let light_dir = [lv[0] / len, lv[1] / len, lv[2] / len, lit_flag];
        let flat = self.lighting.flat_rgb();
        let key = self.lighting.key_rgb();
        let flat_color = [flat[0], flat[1], flat[2], 0.0];
        let key_color = [key[0], key[1], key[2], 0.0];
        let params = [
            self.lighting.rim_strength,
            self.lighting.rim_power,
            self.lighting.specular_strength,
            self.lighting.shininess,
        ];

        let uniforms = Uniforms { mvp, model_view, light_dir, flat_color, key_color, params };

        // World-space uniforms (no model rotation) - used for waterline etc. Always unlit.
        let world_mvp = mat4_mul(proj_mat, view_mat);
        let world_uniforms = Uniforms {
            mvp: world_mvp,
            model_view: view_mat,
            light_dir: [light_dir[0], light_dir[1], light_dir[2], 0.0],
            flat_color,
            key_color,
            params,
        };

        let model_unlit_uniforms = Uniforms { light_dir: [light_dir[0], light_dir[1], light_dir[2], 0.0], ..uniforms };

        (uniforms, world_uniforms, model_unlit_uniforms)
    }

    /// Record the scene draw into `encoder` against the given MSAA color, resolve
    /// color, and depth views. Sorts meshes by layer and binds the matching
    /// pipeline/uniform group per mesh. Used by `render_offscreen_rgba`.
    #[allow(clippy::too_many_arguments)]
    fn encode_scene_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        pipeline: &GpuPipeline,
        msaa_color_view: &wgpu::TextureView,
        resolve_color_view: &wgpu::TextureView,
        depth_view: &wgpu::TextureView,
        model_lit_bg: &wgpu::BindGroup,
        model_unlit_bg: &wgpu::BindGroup,
        world_bg: &wgpu::BindGroup,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("viewport_3d_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: msaa_color_view,
                resolve_target: Some(resolve_color_view),
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(self.clear_color), store: wgpu::StoreOp::Store },
                depth_slice: None,
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth_view,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                stencil_ops: None,
            }),
            ..Default::default()
        });

        pass.set_bind_group(0, model_unlit_bg, &[]);
        // Start with fallback texture; per-mesh textures override below.
        pass.set_bind_group(1, &pipeline.fallback_texture_bind_group, &[]);

        // Sort meshes by layer: armor (opaque, depth-write) first, then hull + overlays (transparent, no depth-write).
        let mut sorted: Vec<(MeshId, &GpuMesh)> =
            self.meshes.iter().filter(|(_, m)| m.visible && m.index_count > 0).map(|(id, m)| (*id, m)).collect();
        sorted.sort_by_key(|(_, m)| m.layer);

        let mut current_layer_kind: i32 = -1; // force first set_pipeline
        let mut has_custom_texture = false; // track whether we need to rebind fallback
        let mut current_bind = BindKind::ModelUnlit;
        for (_id, mesh) in sorted {
            let layer_kind = if mesh.layer <= LAYER_OPAQUE_MAX {
                0 // opaque
            } else if mesh.layer < LAYER_OVERLAY {
                1 // transparent (hull)
            } else {
                2 // overlay (always on top)
            };
            if layer_kind != current_layer_kind {
                match layer_kind {
                    0 => pass.set_pipeline(&pipeline.pipeline),
                    1 => pass.set_pipeline(&pipeline.pipeline_no_depth_write),
                    _ => pass.set_pipeline(&pipeline.pipeline_overlay),
                }
                current_layer_kind = layer_kind;
            }

            // Pick the uniform bind group: world-space (always unlit), model-space lit
            // (hull), or model-space unlit (armor/overlays).
            let desired_bg = if mesh.world_space {
                BindKind::World
            } else if mesh.lit {
                BindKind::ModelLit
            } else {
                BindKind::ModelUnlit
            };
            if desired_bg != current_bind {
                let bg = match desired_bg {
                    BindKind::World => world_bg,
                    BindKind::ModelLit => model_lit_bg,
                    BindKind::ModelUnlit => model_unlit_bg,
                };
                pass.set_bind_group(0, bg, &[]);
                current_bind = desired_bg;
            }

            // Bind per-mesh texture or fallback
            if let Some(ref tex_bg) = mesh.texture_bind_group {
                pass.set_bind_group(1, tex_bg, &[]);
                has_custom_texture = true;
            } else if has_custom_texture {
                // Rebind fallback after a textured mesh
                pass.set_bind_group(1, &pipeline.fallback_texture_bind_group, &[]);
                has_custom_texture = false;
            }

            pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
            pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..mesh.index_count, 0, 0..1);
        }
    }

    /// Render the current scene to an offscreen target and read the pixels back as
    /// tightly-packed sRGB RGBA8 (`width * height * 4`). Runs entirely on the owned
    /// wgpu device with no window surface, so it is usable headless. Returns
    /// `(width, height, rgba)`.
    pub fn render_offscreen_rgba(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pipeline: &GpuPipeline,
        size: (u32, u32),
    ) -> Option<(u32, u32, Vec<u8>)> {
        if size.0 == 0 || size.1 == 0 {
            return None;
        }

        // Transient render targets allocated per call (MSAA color + resolve + depth).
        let msaa_color = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("viewport_3d_offscreen_msaa_color"),
            size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: MSAA_SAMPLE_COUNT,
            dimension: wgpu::TextureDimension::D2,
            format: COLOR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let msaa_color_view = msaa_color.create_view(&Default::default());

        // Resolve target is COPY_SRC (not TEXTURE_BINDING) since we read it back.
        let resolve_color = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("viewport_3d_offscreen_color"),
            size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: COLOR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let resolve_color_view = resolve_color.create_view(&Default::default());

        let depth = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("viewport_3d_offscreen_depth"),
            size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: MSAA_SAMPLE_COUNT,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let depth_view = depth.create_view(&Default::default());

        // Per-call uniform buffers + bind groups.
        let (uniforms, world_uniforms, model_unlit_uniforms) = self.scene_uniforms(size);
        use wgpu::util::DeviceExt;
        let make = |contents: &[u8], label: &str| {
            let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &pipeline.uniform_bind_group_layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }],
            })
        };
        let model_lit_bg = make(bytemuck::bytes_of(&uniforms), "viewport_3d_offscreen_uniforms");
        let world_bg = make(bytemuck::bytes_of(&world_uniforms), "viewport_3d_offscreen_world_uniforms");
        let model_unlit_bg = make(bytemuck::bytes_of(&model_unlit_uniforms), "viewport_3d_offscreen_model_unlit");

        let mut encoder = device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("viewport_3d_offscreen_encoder") });

        self.encode_scene_pass(
            &mut encoder,
            pipeline,
            &msaa_color_view,
            &resolve_color_view,
            &depth_view,
            &model_lit_bg,
            &model_unlit_bg,
            &world_bg,
        );

        // Copy the resolved color into a readback buffer. bytes_per_row must be a
        // multiple of 256, so pad each row and strip the padding after mapping.
        let unpadded_bpr = size.0 * 4;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT; // 256
        let padded_bpr = unpadded_bpr.div_ceil(align) * align;
        let buffer_size = (padded_bpr * size.1) as u64;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("viewport_3d_offscreen_readback"),
            size: buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &resolve_color,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bpr),
                    rows_per_image: Some(size.1),
                },
            },
            wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
        );

        queue.submit(std::iter::once(encoder.finish()));

        // Map and block until the GPU work completes.
        let slice = readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        let _ = device.poll(wgpu::PollType::wait_indefinitely());

        let mapped = slice.get_mapped_range();
        let mut rgba = vec![0u8; (unpadded_bpr * size.1) as usize];
        for row in 0..size.1 as usize {
            let src = row * padded_bpr as usize;
            let dst = row * unpadded_bpr as usize;
            rgba[dst..dst + unpadded_bpr as usize].copy_from_slice(&mapped[src..src + unpadded_bpr as usize]);
        }
        drop(mapped);
        readback.unmap();

        Some((size.0, size.1, rgba))
    }

    /// Transform a world-space ray into model space (inverse of model matrix).
    /// The model matrix is Ry(yaw) * Rz(-roll), so the inverse is Rz(+roll) * Ry(-yaw).
    fn ray_to_model_space(&self, origin: Vec3, dir: Vec3) -> (Vec3, Vec3) {
        let has_roll = self.model_roll.abs() > 1e-6;
        let has_yaw = self.model_yaw.abs() > 1e-6;
        if !has_roll && !has_yaw {
            return (origin, dir);
        }
        // Inverse: Rz(+roll) * Ry(-yaw) — composed as a single rotation.
        let inv_rot = {
            let rz = Rotation3::from_axis_angle(&Vector3::z_axis(), self.model_roll);
            let ry = Rotation3::from_axis_angle(&Vector3::y_axis(), -self.model_yaw);
            rz * ry
        };
        (inv_rot * origin, inv_rot * dir)
    }

    /// Transform a model-space position back to world space (apply model matrix).
    /// The model matrix is Ry(+yaw) * Rz(-roll): apply Rz(-roll) first, then Ry(+yaw).
    pub fn pos_to_world_space(&self, p: Vec3) -> Vec3 {
        let has_roll = self.model_roll.abs() > 1e-6;
        let has_yaw = self.model_yaw.abs() > 1e-6;
        if !has_roll && !has_yaw {
            return p;
        }
        // Model matrix: Ry(+yaw) * Rz(-roll)
        let model_rot = {
            let ry = Rotation3::from_axis_angle(&Vector3::y_axis(), self.model_yaw);
            let rz = Rotation3::from_axis_angle(&Vector3::z_axis(), -self.model_roll);
            ry * rz
        };
        model_rot * p
    }

    /// Transform a model-space normal back to world space (apply model rotation).
    fn normal_to_world_space(&self, n: Vec3) -> Vec3 {
        self.pos_to_world_space(n)
    }

    /// Collect pickable mesh references with visibility info.
    fn pick_mesh_refs(&self) -> Vec<(MeshId, &PickableMesh, bool)> {
        self.pick_data
            .iter()
            .map(|(id, mesh)| {
                let visible = self.meshes.get(id).is_some_and(|m| m.visible);
                (*id, mesh, visible)
            })
            .collect()
    }

    /// Perform CPU picking at a screen position within the given viewport rect.
    pub fn pick(&self, screen_pos: Vec2, viewport_rect: ViewRect) -> Option<HitResult> {
        let (origin, dir) = picking::screen_to_ray(screen_pos, viewport_rect, &self.camera)?;
        let (origin, dir) = self.ray_to_model_space(origin, dir);
        let mesh_refs = self.pick_mesh_refs();
        let mut hit = picking::pick_all_ray(origin, dir, &mesh_refs).into_iter().next()?.0;
        hit.world_position = self.pos_to_world_space(hit.world_position);
        Some(hit)
    }

    /// Unproject a screen position to a world-space ray (origin, direction).
    pub fn screen_to_ray(&self, screen_pos: Vec2, viewport_rect: ViewRect) -> Option<(Vec3, Vec3)> {
        picking::screen_to_ray(screen_pos, viewport_rect, &self.camera)
    }

    /// Perform CPU picking that returns ALL hits along the ray, sorted by distance.
    /// Each hit includes the triangle normal for impact angle calculations.
    pub fn pick_all(&self, screen_pos: Vec2, viewport_rect: ViewRect) -> Vec<(HitResult, Vec3)> {
        let Some((origin, dir)) = picking::screen_to_ray(screen_pos, viewport_rect, &self.camera) else {
            return Vec::new();
        };
        let (origin, dir) = self.ray_to_model_space(origin, dir);
        let mesh_refs = self.pick_mesh_refs();
        picking::pick_all_ray(origin, dir, &mesh_refs)
            .into_iter()
            .map(|(mut hit, normal)| {
                hit.world_position = self.pos_to_world_space(hit.world_position);
                (hit, self.normal_to_world_space(normal))
            })
            .collect()
    }

    /// Pick ALL triangles hit by an arbitrary world-space ray, sorted by distance.
    /// Each hit includes the triangle normal for angle calculations.
    pub fn pick_all_ray(&self, origin: Vec3, direction: Vec3) -> Vec<(HitResult, Vec3)> {
        let (origin, dir) = self.ray_to_model_space(origin, direction);
        let mesh_refs = self.pick_mesh_refs();
        picking::pick_all_ray(origin, dir, &mesh_refs)
            .into_iter()
            .map(|(mut hit, normal)| {
                hit.world_position = self.pos_to_world_space(hit.world_position);
                (hit, self.normal_to_world_space(normal))
            })
            .collect()
    }

    /// Like `pick_all_ray`, but the ray is already in raw model/mesh space.
    /// Skips model rotation transforms on both input and output.
    pub fn pick_all_ray_model_space(&self, origin: Vec3, direction: Vec3) -> Vec<(HitResult, Vec3)> {
        let mesh_refs = self.pick_mesh_refs();
        picking::pick_all_ray(origin, direction, &mesh_refs)
    }

    /// Top-right corner rect for the navigation gizmo within the viewport.
    pub fn gizmo_rect(&self, viewport: ViewRect) -> ViewRect {
        crate::viewport::gizmo::gizmo_rect(viewport)
    }
}
