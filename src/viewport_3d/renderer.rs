use std::collections::HashMap;

use crate::viewport_3d::camera::ArcballCamera;
use crate::viewport_3d::camera::mat4_mul;
use crate::viewport_3d::picking::{self, PickableMesh};
use crate::viewport_3d::types::{HitResult, MeshId, Uniforms, Vertex};

const SHADER_SOURCE: &str = r#"
struct Uniforms {
    mvp: mat4x4<f32>,
    model_view: mat4x4<f32>,
    light_dir: vec4<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) normal_vs: vec3<f32>,
    @location(2) position_vs: vec3<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = uniforms.mvp * vec4(in.position, 1.0);
    out.normal_vs = (uniforms.model_view * vec4(in.normal, 0.0)).xyz;
    out.position_vs = (uniforms.model_view * vec4(in.position, 1.0)).xyz;
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // View direction (camera is at origin in view space)
    let V = normalize(-in.position_vs);

    // Double-sided normal: flip toward camera
    var n = normalize(in.normal_vs);
    if (dot(n, V) < 0.0) {
        n = -n;
    }

    // Key light
    let l = normalize(uniforms.light_dir.xyz);
    let NdotL = max(dot(n, l), 0.0);
    let diffuse1 = NdotL * 0.7;

    // Fill light (opposite side, dimmer)
    let l2 = normalize(vec3(-0.4, -0.3, -0.6));
    let diffuse2 = max(dot(n, l2), 0.0) * 0.2;

    // Blinn-Phong specular (key light only)
    let H = normalize(l + V);
    let spec = pow(max(dot(n, H), 0.0), 32.0) * 0.3;

    // Hemisphere ambient (sky=0.35, ground=0.15)
    let ambient = mix(0.15, 0.35, n.y * 0.5 + 0.5);

    // Rim/fresnel glow
    let rim = pow(1.0 - max(dot(n, V), 0.0), 3.0) * 0.15;

    let brightness = ambient + diffuse1 + diffuse2 + rim;
    let color = in.color.rgb * brightness + vec3(spec);

    return vec4(color, in.color.a);
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
}

impl GpuPipeline {
    pub fn new(device: &wgpu::Device) -> Self {
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

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("viewport_3d_pipeline_layout"),
            bind_group_layouts: &[&uniform_bind_group_layout],
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

        Self { pipeline, pipeline_no_depth_write, pipeline_overlay, uniform_bind_group_layout }
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
    next_mesh_id: u64,
    pub clear_color: wgpu::Color,
    /// Whether the scene has changed and needs re-rendering.
    needs_redraw: bool,
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
            next_mesh_id: 0,
            clear_color: wgpu::Color { r: 0.12, g: 0.12, b: 0.18, a: 1.0 },
            needs_redraw: true,
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
            GpuMesh { vertex_buffer, index_buffer, index_count: indices.len() as u32, visible: true, layer },
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
            GpuMesh { vertex_buffer, index_buffer, index_count: indices.len() as u32, visible: true, layer },
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
                vertex_buffer,
                index_buffer,
                index_count: indices.len() as u32,
                visible: true,
                layer: LAYER_OVERLAY,
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
        if let Some(mesh) = self.meshes.get_mut(&id) {
            if mesh.visible != visible {
                mesh.visible = visible;
                self.needs_redraw = true;
            }
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
        let view_mat = self.camera.view_matrix();
        let proj_mat = self.camera.projection_matrix(aspect);
        let mvp = mat4_mul(proj_mat, view_mat);

        let uniforms = Uniforms { mvp, model_view: view_mat, light_dir: [0.3, 0.8, 0.5, 0.0] };

        if self.uniform_buffer.is_none() {
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
        } else {
            queue.write_buffer(self.uniform_buffer.as_ref().unwrap(), 0, bytemuck::bytes_of(&uniforms));
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

            pass.set_bind_group(0, self.uniform_bind_group.as_ref().unwrap(), &[]);

            // Sort meshes by layer: armor (opaque, depth-write) first, then hull + overlays (transparent, no depth-write).
            let mut sorted: Vec<&GpuMesh> = self.meshes.values().filter(|m| m.visible && m.index_count > 0).collect();
            sorted.sort_by_key(|m| m.layer);

            let mut current_layer_kind: i32 = -1; // force first set_pipeline
            for mesh in sorted {
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
                pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..mesh.index_count, 0, 0..1);
            }
        }

        queue.submit(std::iter::once(encoder.finish()));

        offscreen.egui_texture_id
    }

    /// Perform CPU picking at a screen position within the given viewport rect.
    pub fn pick(&self, screen_pos: egui::Pos2, viewport_rect: egui::Rect) -> Option<HitResult> {
        let mesh_refs: Vec<(MeshId, &PickableMesh, bool)> = self
            .pick_data
            .iter()
            .map(|(id, mesh)| {
                let visible = self.meshes.get(id).is_some_and(|m| m.visible);
                (*id, mesh, visible)
            })
            .collect();

        picking::pick_meshes(screen_pos, viewport_rect, &self.camera, &mesh_refs)
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
        if let Some(offscreen) = self.offscreen.take() {
            if let Some(id) = offscreen.egui_texture_id {
                let mut renderer = render_state.renderer.write();
                renderer.free_texture(&id);
            }
        }
        self.meshes.clear();
        self.pick_data.clear();
        self.uniform_buffer = None;
        self.uniform_bind_group = None;
    }
}
