use std::collections::HashMap;

use crate::viewport_3d::camera::ArcballCamera;
use crate::viewport_3d::camera::mat4_mul;
use crate::viewport_3d::picking::PickableMesh;
use crate::viewport_3d::picking::{
    self,
};

const MAT4_IDENTITY: [[f32; 4]; 4] =
    [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
use crate::viewport_3d::types::HitResult;
use crate::viewport_3d::types::MeshId;
use crate::viewport_3d::types::Uniforms;
use crate::viewport_3d::types::Vertex;

const SHADER_SOURCE: &str = r#"
struct Uniforms {
    mvp: mat4x4<f32>,
    model_view: mat4x4<f32>,
    light_dir: vec4<f32>,
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
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Sample texture and multiply with vertex color.
    // Non-textured meshes bind a 1x1 white fallback, so this is a passthrough.
    let tex_color = textureSample(diffuse_texture, diffuse_sampler, in.uv);
    let base_color = tex_color * in.color;

    // Flat lighting — uniform brightness, no directional shading.
    let color = base_color.rgb;

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
            bind_group_layouts: &[&uniform_bind_group_layout, &texture_bind_group_layout],
            push_constant_ranges: &[],
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
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: MSAA_SAMPLE_COUNT,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
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
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::LessEqual,
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
            multiview: None,
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
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: MSAA_SAMPLE_COUNT,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
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
            mipmap_filter: wgpu::FilterMode::Linear,
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
}

/// Offscreen render target (MSAA color + resolve color + depth).
#[allow(dead_code)]
struct OffscreenTarget {
    /// MSAA color texture — render target for the multisampled pass.
    msaa_color_texture: wgpu::Texture,
    msaa_color_view: wgpu::TextureView,
    /// Resolve color texture (1x) — the resolved output registered with egui.
    color_texture: wgpu::Texture,
    color_view: wgpu::TextureView,
    depth_texture: wgpu::Texture,
    depth_view: wgpu::TextureView,
    egui_texture_id: Option<egui::TextureId>,
    size: (u32, u32),
}

/// A complete 3D viewport instance. Each consumer (armor pane, replay viewer, etc.)
/// creates one of these. Holds its own camera, offscreen target, and scene meshes.
pub struct Viewport3D {
    pub camera: ArcballCamera,
    meshes: HashMap<MeshId, GpuMesh>,
    pick_data: HashMap<MeshId, PickableMesh>,
    offscreen: Option<OffscreenTarget>,
    uniform_buffer: Option<wgpu::Buffer>,
    uniform_bind_group: Option<wgpu::BindGroup>,
    /// Separate uniform buffer for world-space meshes (no model rotation).
    world_uniform_buffer: Option<wgpu::Buffer>,
    world_uniform_bind_group: Option<wgpu::BindGroup>,
    next_mesh_id: u64,
    pub clear_color: wgpu::Color,
    /// Whether the scene has changed and needs re-rendering.
    needs_redraw: bool,
    /// Model roll angle in radians (rotation around the longitudinal/Z axis).
    pub model_roll: f32,
    /// Cursor position in NDC ([-1,1] range), updated each frame for flashlight lighting.
    pub cursor_ndc: Option<[f32; 2]>,
}

impl Default for Viewport3D {
    fn default() -> Self {
        Self::new()
    }
}

impl Viewport3D {
    pub fn new() -> Self {
        Self {
            camera: ArcballCamera::default(),
            meshes: HashMap::new(),
            pick_data: HashMap::new(),
            offscreen: None,
            uniform_buffer: None,
            uniform_bind_group: None,
            world_uniform_buffer: None,
            world_uniform_bind_group: None,
            next_mesh_id: 0,
            clear_color: wgpu::Color { r: 0.12, g: 0.12, b: 0.18, a: 1.0 },
            needs_redraw: true,
            model_roll: 0.0,
            cursor_ndc: None,
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

    /// Whether the viewport needs a redraw.
    pub fn is_dirty(&self) -> bool {
        self.needs_redraw
    }

    /// Returns true if the viewport has any meshes to render.
    pub fn has_meshes(&self) -> bool {
        !self.meshes.is_empty()
    }

    /// Render all visible meshes to the offscreen target.
    /// Returns the egui TextureId to display, or None if nothing to render.
    pub fn render(
        &mut self,
        render_state: &eframe::egui_wgpu::RenderState,
        pipeline: &GpuPipeline,
        size: (u32, u32),
    ) -> Option<egui::TextureId> {
        if size.0 == 0 || size.1 == 0 {
            return None;
        }

        let device = &render_state.device;
        let queue = &render_state.queue;

        // Create or resize offscreen target
        let needs_resize = self.offscreen.as_ref().is_none_or(|t| t.size != size);
        if needs_resize {
            // MSAA color texture — multisampled render target
            let msaa_color_texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("viewport_3d_msaa_color"),
                size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: MSAA_SAMPLE_COUNT,
                dimension: wgpu::TextureDimension::D2,
                format: COLOR_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let msaa_color_view = msaa_color_texture.create_view(&Default::default());

            // Resolve color texture (1x) — registered with egui for display
            let color_texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("viewport_3d_color"),
                size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: COLOR_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let color_view = color_texture.create_view(&Default::default());

            // Depth texture — multisampled to match MSAA color
            let depth_texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("viewport_3d_depth"),
                size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: MSAA_SAMPLE_COUNT,
                dimension: wgpu::TextureDimension::D2,
                format: DEPTH_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let depth_view = depth_texture.create_view(&Default::default());

            // Register or update egui texture (uses resolved 1x color)
            let egui_texture_id = if let Some(old) = self.offscreen.take() {
                if let Some(id) = old.egui_texture_id {
                    let mut renderer = render_state.renderer.write();
                    renderer.update_egui_texture_from_wgpu_texture(device, &color_view, wgpu::FilterMode::Linear, id);
                    Some(id)
                } else {
                    let mut renderer = render_state.renderer.write();
                    Some(renderer.register_native_texture(device, &color_view, wgpu::FilterMode::Linear))
                }
            } else {
                let mut renderer = render_state.renderer.write();
                Some(renderer.register_native_texture(device, &color_view, wgpu::FilterMode::Linear))
            };

            self.offscreen = Some(OffscreenTarget {
                msaa_color_texture,
                msaa_color_view,
                color_texture,
                color_view,
                depth_texture,
                depth_view,
                egui_texture_id,
                size,
            });
            self.needs_redraw = true;
        }

        let offscreen = self.offscreen.as_ref().unwrap();

        if !self.needs_redraw {
            return offscreen.egui_texture_id;
        }
        self.needs_redraw = false;

        // Create/update uniform buffer
        let aspect = size.0 as f32 / size.1 as f32;
        let model_mat = if self.model_roll.abs() > 1e-6 {
            // Rotation around Z axis (ship's longitudinal axis).
            // Negated because mesh Z is reversed (RH coords: +Z = stern).
            let (s, c) = (-self.model_roll).sin_cos();
            [[c, s, 0.0, 0.0], [-s, c, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]
        } else {
            MAT4_IDENTITY
        };
        let view_mat = self.camera.view_matrix();
        let proj_mat = self.camera.projection_matrix(aspect);
        let model_view = mat4_mul(view_mat, model_mat);
        let mvp = mat4_mul(proj_mat, model_view);

        // light_dir is kept in the uniform struct for layout compatibility but
        // the shader now uses a uniform camera-forward headlamp instead.
        let light_dir = [0.0_f32, 0.0, -1.0, 0.0];
        let uniforms = Uniforms { mvp, model_view, light_dir };

        // World-space uniforms (no model rotation) — used for waterline etc.
        let world_mvp = mat4_mul(proj_mat, view_mat);
        let world_uniforms = Uniforms { mvp: world_mvp, model_view: view_mat, light_dir };

        if let (Some(ub), Some(wub)) = (self.uniform_buffer.as_ref(), self.world_uniform_buffer.as_ref()) {
            queue.write_buffer(ub, 0, bytemuck::bytes_of(&uniforms));
            queue.write_buffer(wub, 0, bytemuck::bytes_of(&world_uniforms));
        } else {
            use wgpu::util::DeviceExt;
            let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("viewport_3d_uniforms"),
                contents: bytemuck::bytes_of(&uniforms),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("viewport_3d_uniform_bg"),
                layout: &pipeline.uniform_bind_group_layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }],
            });
            self.uniform_buffer = Some(buffer);
            self.uniform_bind_group = Some(bind_group);

            let world_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("viewport_3d_world_uniforms"),
                contents: bytemuck::bytes_of(&world_uniforms),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });
            let world_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("viewport_3d_world_uniform_bg"),
                layout: &pipeline.uniform_bind_group_layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: world_buffer.as_entire_binding() }],
            });
            self.world_uniform_buffer = Some(world_buffer);
            self.world_uniform_bind_group = Some(world_bind_group);
        }

        // Render
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("viewport_3d_encoder") });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("viewport_3d_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &offscreen.msaa_color_view,
                    resolve_target: Some(&offscreen.color_view),
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(self.clear_color), store: wgpu::StoreOp::Store },
                    depth_slice: None,
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &offscreen.depth_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });

            let model_bg = self.uniform_bind_group.as_ref().unwrap();
            let world_bg = self.world_uniform_bind_group.as_ref().unwrap();
            pass.set_bind_group(0, model_bg, &[]);
            // Start with fallback texture; per-mesh textures override below.
            pass.set_bind_group(1, &pipeline.fallback_texture_bind_group, &[]);

            // Sort meshes by layer: armor (opaque, depth-write) first, then hull + overlays (transparent, no depth-write).
            let mut sorted: Vec<(MeshId, &GpuMesh)> =
                self.meshes.iter().filter(|(_, m)| m.visible && m.index_count > 0).map(|(id, m)| (*id, m)).collect();
            sorted.sort_by_key(|(_, m)| m.layer);

            let mut current_layer_kind: i32 = -1; // force first set_pipeline
            let mut has_custom_texture = false; // track whether we need to rebind fallback
            let mut current_world_space = false;
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

                // Switch uniform bind group for world-space vs model-space meshes
                if mesh.world_space != current_world_space {
                    pass.set_bind_group(0, if mesh.world_space { world_bg } else { model_bg }, &[]);
                    current_world_space = mesh.world_space;
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

        queue.submit(std::iter::once(encoder.finish()));

        offscreen.egui_texture_id
    }

    /// Rotate a 3D point around the Z axis by `angle` radians.
    fn rotate_z(p: [f32; 3], angle: f32) -> [f32; 3] {
        let (s, c) = angle.sin_cos();
        [p[0] * c - p[1] * s, p[0] * s + p[1] * c, p[2]]
    }

    /// Transform a world-space ray into model space (inverse of model_roll).
    /// Returns the ray unchanged when model_roll is zero.
    fn ray_to_model_space(&self, origin: [f32; 3], dir: [f32; 3]) -> ([f32; 3], [f32; 3]) {
        if self.model_roll.abs() < 1e-6 {
            return (origin, dir);
        }
        // The model matrix rotates model-space -> world-space by -model_roll around Z.
        // The inverse (world -> model) is rotation by +model_roll.
        (Self::rotate_z(origin, self.model_roll), Self::rotate_z(dir, self.model_roll))
    }

    /// Transform a model-space position back to world space (apply model_roll).
    fn pos_to_world_space(&self, p: [f32; 3]) -> [f32; 3] {
        if self.model_roll.abs() < 1e-6 {
            return p;
        }
        Self::rotate_z(p, -self.model_roll)
    }

    /// Transform a model-space normal back to world space (apply model_roll).
    fn normal_to_world_space(&self, n: [f32; 3]) -> [f32; 3] {
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
    pub fn pick(&self, screen_pos: egui::Pos2, viewport_rect: egui::Rect) -> Option<HitResult> {
        let (origin, dir) = picking::screen_to_ray(screen_pos, viewport_rect, &self.camera)?;
        let (origin, dir) = self.ray_to_model_space(origin, dir);
        let mesh_refs = self.pick_mesh_refs();
        let mut hit = picking::pick_all_ray(origin, dir, &mesh_refs).into_iter().next()?.0;
        hit.world_position = self.pos_to_world_space(hit.world_position);
        Some(hit)
    }

    /// Unproject a screen position to a world-space ray (origin, direction).
    pub fn screen_to_ray(&self, screen_pos: egui::Pos2, viewport_rect: egui::Rect) -> Option<([f32; 3], [f32; 3])> {
        picking::screen_to_ray(screen_pos, viewport_rect, &self.camera)
    }

    /// Perform CPU picking that returns ALL hits along the ray, sorted by distance.
    /// Each hit includes the triangle normal for impact angle calculations.
    pub fn pick_all(&self, screen_pos: egui::Pos2, viewport_rect: egui::Rect) -> Vec<(HitResult, [f32; 3])> {
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
    pub fn pick_all_ray(&self, origin: [f32; 3], direction: [f32; 3]) -> Vec<(HitResult, [f32; 3])> {
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

    /// Handle standard 3D navigation input on a UI response.
    /// Left-drag = orbit, scroll = zoom, middle-drag = pan, double-click = reset.
    /// Returns true if the camera changed.
    pub fn handle_input(&mut self, response: &egui::Response, bounds: Option<([f32; 3], [f32; 3])>) -> bool {
        let old_az = self.camera.azimuth;
        let old_el = self.camera.elevation;
        let old_dist = self.camera.distance;
        let old_target = self.camera.target;

        self.camera.handle_input(response, bounds);

        let changed = (self.camera.azimuth - old_az).abs() > 1e-6
            || (self.camera.elevation - old_el).abs() > 1e-6
            || (self.camera.distance - old_dist).abs() > 1e-6
            || self.camera.target != old_target;

        if changed {
            self.needs_redraw = true;
        }
        changed
    }

    /// Free the offscreen textures and unregister from egui.
    pub fn destroy(&mut self, render_state: &eframe::egui_wgpu::RenderState) {
        if let Some(offscreen) = self.offscreen.take()
            && let Some(id) = offscreen.egui_texture_id
        {
            let mut renderer = render_state.renderer.write();
            renderer.free_texture(&id);
        }
        self.meshes.clear();
        self.pick_data.clear();
        self.uniform_buffer = None;
        self.uniform_bind_group = None;
    }
}
