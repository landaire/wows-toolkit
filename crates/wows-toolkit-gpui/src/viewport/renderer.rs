use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::rc::Weak;

extern crate nalgebra as na;
use gpui_wgpu::wgpu;
use na::Rotation3;
use na::Vector3;

use crate::viewport::camera::ArcballCamera;
use crate::viewport::camera::mat4_mul;
use crate::viewport::picking;
use crate::viewport::picking::PickableMesh;
use crate::viewport::types::HitResult;
use crate::viewport::types::LightingSettings;
use crate::viewport::types::MeshId;
use crate::viewport::types::TargetSize;
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

// Set where the target has no sRGB view (GLES): the shader stores encoded color
// itself, and blending and the MSAA resolve then act on encoded values.
override ENCODE_SRGB: bool = false;

fn srgb_encode(linear: vec3<f32>) -> vec3<f32> {
    let clamped = clamp(linear, vec3<f32>(0.0), vec3<f32>(1.0));
    let low = clamped * 12.92;
    let high = 1.055 * pow(clamped, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(high, low, clamped <= vec3<f32>(0.0031308));
}

@group(1) @binding(0) var diffuse_texture: texture_2d<f32>;
@group(1) @binding(1) var diffuse_sampler: sampler;

struct MaterialParams {
    // x: alpha cutoff, y: 1 with a normal map, z: 1 with a metal-gloss map.
    flags: vec4<f32>,
    // xy: albedo UV scale, zw: albedo UV offset.
    albedo_uv: vec4<f32>,
};

@group(1) @binding(2) var<uniform> material: MaterialParams;
@group(1) @binding(3) var normal_texture: texture_2d<f32>;
@group(1) @binding(4) var metal_gloss_texture: texture_2d<f32>;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) uv: vec2<f32>,
    @location(4) tangent: vec4<f32>,
};

struct VertexOutput {
    // Invariant so a highlight drawn over the same vertices as a surface
    // passes a depth test against it, whatever pipeline drew the surface.
    @invariant @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) normal_vs: vec3<f32>,
    @location(2) position_vs: vec3<f32>,
    @location(3) uv: vec2<f32>,
    @location(4) tangent_vs: vec4<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = uniforms.mvp * vec4(in.position, 1.0);
    out.normal_vs = (uniforms.model_view * vec4(in.normal, 0.0)).xyz;
    out.position_vs = (uniforms.model_view * vec4(in.position, 1.0)).xyz;
    out.color = in.color;
    out.uv = in.uv;
    out.tangent_vs = vec4((uniforms.model_view * vec4(in.tangent.xyz, 0.0)).xyz, in.tangent.w);
    return out;
}

// The normal a tangent-space map sample gives. The map stores X and Y only;
// Z is rebuilt. A vertex without a tangent keeps the geometric normal.
fn mapped_normal(n: vec3<f32>, tangent: vec4<f32>, encoded: vec2<f32>) -> vec3<f32> {
    let along_u = tangent.xyz - n * dot(n, tangent.xyz);
    let length_u = length(along_u);
    if (length_u < 1e-5) {
        return n;
    }
    let t = along_u / length_u;
    let b = cross(n, t) * tangent.w;
    let xy = encoded * 2.0 - 1.0;
    let z = sqrt(max(1.0 - dot(xy, xy), 0.0));
    return normalize(t * xy.x + b * xy.y + n * z);
}

@fragment
fn fs_main(in: VertexOutput, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    // Sample texture and multiply with vertex color.
    // Non-textured meshes bind a 1x1 white fallback, so this is a passthrough.
    // Maps are sampled before the discard, where derivatives are defined, and
    // only when the material has them; the flags are uniform per draw.
    let tex_color = textureSample(diffuse_texture, diffuse_sampler, in.uv * material.albedo_uv.xy + material.albedo_uv.zw);
    var normal_sample = vec4<f32>(0.5, 0.5, 1.0, 1.0);
    if (material.flags.y > 0.5) {
        normal_sample = textureSample(normal_texture, diffuse_sampler, in.uv);
    }
    var metal_gloss = vec4<f32>(0.0);
    if (material.flags.z > 0.5) {
        metal_gloss = textureSample(metal_gloss_texture, diffuse_sampler, in.uv);
    }
    // A sample under the texture's alpha cutoff draws nothing, so the cut-out
    // parts of an alpha-tested texture neither show nor hide what lies behind
    // them.
    if (tex_color.a < material.flags.x) {
        discard;
    }
    let base_color = tex_color * in.color;

    var color = base_color.rgb;
    if (uniforms.light_dir.w > 0.5) {
        // Hull lighting: half-Lambert key over a flat ambient floor, plus rim and specular.
        // Half-Lambert keeps the far side lit (never fully black) so all angles stay visible.
        // The hull is double-sided; flip the normal on back faces so both sides shade
        // consistently and the see-through hull does not show a hard front/back seam.
        var N = normalize(in.normal_vs);
        if (material.flags.y > 0.5) {
            N = mapped_normal(N, in.tangent_vs, normal_sample.xy);
        }
        if (!front_facing) {
            N = -N;
        }
        let V = normalize(-in.position_vs);
        let L = normalize(uniforms.light_dir.xyz);
        let half_lambert = dot(N, L) * 0.5 + 0.5;
        let H = normalize(L + V);

        // Gloss scales the set shininess from a quarter (matte) to four times
        // (glossy) and strengthens the highlight; metal tints it by the albedo.
        var shininess = uniforms.params.w;
        var spec_strength = uniforms.params.z;
        var spec_color = vec3<f32>(1.0, 1.0, 1.0);
        if (material.flags.z > 0.5) {
            let gloss = metal_gloss.r;
            shininess = shininess * exp2((gloss - 0.5) * 4.0);
            spec_strength = spec_strength * (0.5 + gloss);
            spec_color = mix(spec_color, min(base_color.rgb, vec3<f32>(1.0)), metal_gloss.g);
        }
        let spec = pow(max(dot(N, H), 0.0), shininess) * spec_strength;
        let rim = pow(1.0 - max(dot(N, V), 0.0), uniforms.params.y) * uniforms.params.x;
        // Fade the directional terms (key, rim, specular) as the surface becomes
        // transparent. A see-through hull shows its near and far walls at once, and a
        // directional light shades those two walls differently, forming a hard seam.
        // Leaning on the seamless flat ambient term for transparent surfaces hides it;
        // opaque geometry occludes its far wall via depth and keeps full directional.
        let dir_factor = smoothstep(0.5, 1.0, base_color.a);
        let directional = (vec3<f32>(rim) + spec_color * spec) * dir_factor;
        let lighting = uniforms.flat_color.rgb + uniforms.key_color.rgb * half_lambert * dir_factor;
        color = base_color.rgb * lighting + directional;
    }

    if (ENCODE_SRGB) {
        color = srgb_encode(color);
    }
    return vec4(color, base_color.a);
}
"#;

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const MSAA_SAMPLE_COUNT: u32 = 4;
/// Bytes of the per-material uniform the fragment shader reads.
const MATERIAL_PARAMS_SIZE: u64 = std::mem::size_of::<MaterialUniform>() as u64;

/// The color format the scene is drawn in, and where its sRGB encoding happens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneColorTarget {
    /// An sRGB format: the hardware encodes the shader's linear output, and
    /// blending and the MSAA resolve act on linear values.
    Srgb(wgpu::TextureFormat),
    /// A linear format the device offers no sRGB view of. The shader encodes.
    ShaderEncoded(wgpu::TextureFormat),
}

impl SceneColorTarget {
    pub fn format(self) -> wgpu::TextureFormat {
        match self {
            Self::Srgb(format) | Self::ShaderEncoded(format) => format,
        }
    }

    /// `color` as the attachment stores it. A clear value bypasses the shader,
    /// so a shader-encoded target is cleared to the encoded value.
    fn clear_value(self, color: wgpu::Color) -> wgpu::Color {
        let encode = |linear: f64| {
            let linear = linear.clamp(0.0, 1.0);
            if linear <= 0.0031308 { linear * 12.92 } else { 1.055 * linear.powf(1.0 / 2.4) - 0.055 }
        };
        match self {
            Self::Srgb(_) => color,
            Self::ShaderEncoded(_) => {
                wgpu::Color { r: encode(color.r), g: encode(color.g), b: encode(color.b), a: color.a }
            }
        }
    }
}

/// Shared GPU resources (created once, reusable across viewports).
pub struct GpuPipeline {
    color: SceneColorTarget,
    /// Pipeline with depth writes enabled — used for opaque geometry (armor).
    pipeline: wgpu::RenderPipeline,
    /// Pipeline without depth writes — used for transparent hull geometry.
    pipeline_no_depth_write: wgpu::RenderPipeline,
    /// Pipeline that ignores depth: overlays, row highlights, and hover
    /// highlights in [`HighlightDepth::XRay`].
    pipeline_overlay: wgpu::RenderPipeline,
    /// Pipeline for hover highlights in [`HighlightDepth::Tested`]: hidden
    /// behind what is in front, never written to depth.
    pipeline_highlight_tested: wgpu::RenderPipeline,
    uniform_bind_group_layout: wgpu::BindGroupLayout,
    texture_bind_group_layout: wgpu::BindGroupLayout,
    /// Shared sampler for all diffuse textures.
    default_sampler: wgpu::Sampler,
    /// 1x1 white texture bind group — bound for non-textured meshes.
    fallback_texture_bind_group: wgpu::BindGroup,
    /// 1x1 flat normal, bound where a material lacks a surface map. The
    /// material's flags keep the shader from reading it.
    absent_map_view: wgpu::TextureView,
}

impl GpuPipeline {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, color: SceneColorTarget) -> Self {
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

        let texture_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let texture_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("viewport_3d_texture_bgl"),
            entries: &[
                texture_entry(0),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(MATERIAL_PARAMS_SIZE),
                    },
                    count: None,
                },
                texture_entry(3),
                texture_entry(4),
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
            buffers: &[Some(Vertex::LAYOUT)],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        };

        let encode_srgb =
            [("ENCODE_SRGB", if matches!(color, SceneColorTarget::ShaderEncoded(_)) { 1.0 } else { 0.0 })];
        let fragment_state = wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: color.format(),
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions { constants: &encode_srgb, ..Default::default() },
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

        // Overlays and highlights read depth without writing it.
        let depth_read_pipeline = |label: &str, depth_compare: wgpu::CompareFunction| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: vertex_state.clone(),
                fragment: Some(fragment_state.clone()),
                primitive: primitive_state,
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(depth_compare),
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
            })
        };
        let pipeline_overlay = depth_read_pipeline("viewport_3d_pipeline_overlay", wgpu::CompareFunction::Always);
        // Equal passes: a highlight lies on the very surface it marks.
        let pipeline_highlight_tested =
            depth_read_pipeline("viewport_3d_pipeline_highlight_tested", wgpu::CompareFunction::LessEqual);

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

        // 1x1 white albedo for non-textured meshes, and a 1x1 flat normal for
        // absent surface maps.
        let no_mips = MipChain::default();
        let white = TextureImage { pixels: &[255, 255, 255, 255], width: 1, height: 1, mips: &no_mips };
        let fallback_view = upload_texture(device, queue, white, TextureEncoding::Srgb);
        let flat_normal = TextureImage { pixels: &[128, 128, 255, 255], width: 1, height: 1, mips: &no_mips };
        let absent_map_view = upload_texture(device, queue, flat_normal, TextureEncoding::Linear);
        // The fallback is opaque white, which no cutoff discards.
        let fallback_binding = MaterialBinding {
            albedo: &fallback_view,
            normal: None,
            metal_gloss: None,
            alpha_cutoff: 0.0,
            albedo_uv: AlbedoUv::IDENTITY,
        };
        let fallback_texture_bind_group = material_bind_group(
            device,
            &texture_bind_group_layout,
            &default_sampler,
            &absent_map_view,
            fallback_binding,
        );

        Self {
            color,
            pipeline,
            pipeline_no_depth_write,
            pipeline_overlay,
            pipeline_highlight_tested,
            uniform_bind_group_layout,
            texture_bind_group_layout,
            default_sampler,
            fallback_texture_bind_group,
            absent_map_view,
        }
    }

    pub fn color(&self) -> SceneColorTarget {
        self.color
    }

    /// The pipeline `kind` draws with while hover highlights are in `highlight`.
    fn layer_pipeline(&self, kind: LayerKind, highlight: HighlightDepth) -> &wgpu::RenderPipeline {
        match kind {
            LayerKind::Opaque => &self.pipeline,
            LayerKind::Translucent => &self.pipeline_no_depth_write,
            LayerKind::Overlay | LayerKind::Highlight => match kind.depth_state(highlight) {
                DepthState::Ignored => &self.pipeline_overlay,
                DepthState::Written | DepthState::Tested => &self.pipeline_highlight_tested,
            },
        }
    }

    /// Uploads RGBA8 pixels and the lower levels of their mip chain, so a
    /// texture seen from afar is filtered rather than aliased.
    pub fn upload_texture(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        image: TextureImage<'_>,
        encoding: TextureEncoding,
    ) -> wgpu::TextureView {
        upload_texture(device, queue, image, encoding)
    }

    /// The bind group a mesh draws `material` with.
    pub fn create_material_bind_group(&self, device: &wgpu::Device, material: MaterialBinding<'_>) -> wgpu::BindGroup {
        material_bind_group(
            device,
            &self.texture_bind_group_layout,
            &self.default_sampler,
            &self.absent_map_view,
            material,
        )
    }
}

/// An RGBA8 texture and the lower levels of its mip chain, which must have
/// been built from `pixels`.
pub struct TextureImage<'a> {
    pub pixels: &'a [u8],
    pub width: u32,
    pub height: u32,
    pub mips: &'a MipChain,
}

/// How a texture's bytes are read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextureEncoding {
    /// sRGB-encoded color, linearized by the sampler.
    Srgb,
    /// Data read as stored, such as normal and metal-gloss maps.
    Linear,
}

impl TextureEncoding {
    fn format(self) -> wgpu::TextureFormat {
        match self {
            Self::Srgb => wgpu::TextureFormat::Rgba8UnormSrgb,
            Self::Linear => wgpu::TextureFormat::Rgba8Unorm,
        }
    }
}

/// A scale then offset applied to the UVs the albedo is sampled at. The
/// surface maps keep the mesh's own UVs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AlbedoUv {
    pub scale: [f32; 2],
    pub offset: [f32; 2],
}

impl AlbedoUv {
    pub const IDENTITY: Self = Self { scale: [1.0, 1.0], offset: [0.0, 0.0] };
}

/// What one material binds: its albedo, the surface maps it has, and how
/// the albedo is sampled. Samples with alpha below `alpha_cutoff` are not
/// drawn.
pub struct MaterialBinding<'a> {
    pub albedo: &'a wgpu::TextureView,
    pub normal: Option<&'a wgpu::TextureView>,
    pub metal_gloss: Option<&'a wgpu::TextureView>,
    pub alpha_cutoff: f32,
    pub albedo_uv: AlbedoUv,
}

impl MaterialBinding<'_> {
    fn uniform(&self) -> MaterialUniform {
        let present = |map: Option<&wgpu::TextureView>| if map.is_some() { 1.0 } else { 0.0 };
        let AlbedoUv { scale, offset } = self.albedo_uv;

        MaterialUniform {
            flags: [self.alpha_cutoff, present(self.normal), present(self.metal_gloss), 0.0],
            albedo_uv: [scale[0], scale[1], offset[0], offset[1]],
        }
    }
}

/// The per-material uniform the fragment shader reads as `MaterialParams`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct MaterialUniform {
    /// Alpha cutoff, normal map present, metal-gloss map present, unused.
    flags: [f32; 4],
    /// Albedo UV scale, then offset.
    albedo_uv: [f32; 4],
}

fn upload_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    image: TextureImage<'_>,
    encoding: TextureEncoding,
) -> wgpu::TextureView {
    let TextureImage { pixels, width, height, mips } = image;
    let top = (width, height, pixels);
    let lower = mips.levels.iter().map(|level| (level.width, level.height, level.pixels.as_slice()));
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("viewport_3d_texture"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1 + mips.levels.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: encoding.format(),
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    for (mip_level, (level_width, level_height, pixels)) in (0u32..).zip(std::iter::once(top).chain(lower)) {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * level_width),
                rows_per_image: Some(level_height),
            },
            wgpu::Extent3d { width: level_width, height: level_height, depth_or_array_layers: 1 },
        );
    }

    texture.create_view(&Default::default())
}

fn material_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    absent_map: &wgpu::TextureView,
    material: MaterialBinding<'_>,
) -> wgpu::BindGroup {
    use wgpu::util::DeviceExt;

    let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("viewport_3d_material_params"),
        contents: bytemuck::bytes_of(&material.uniform()),
        usage: wgpu::BufferUsages::UNIFORM,
    });

    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("viewport_3d_material_bg"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(material.albedo) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
            wgpu::BindGroupEntry { binding: 2, resource: params.as_entire_binding() },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(material.normal.unwrap_or(absent_map)),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::TextureView(material.metal_gloss.unwrap_or(absent_map)),
            },
        ],
    })
}

/// The levels of a texture's mip chain below its full size, down to one
/// texel. Built where the texture is decoded, off the UI thread, since the
/// upload that uses it runs on it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MipChain {
    levels: Vec<MipLevel>,
}

impl MipChain {
    /// Halves `pixels` (RGBA8, `width` by `height`) until one texel is left.
    pub fn build(width: u32, height: u32, pixels: &[u8]) -> Self {
        let mut levels = Vec::new();
        let (mut source_width, mut source_height) = (width, height);
        while source_width > 1 || source_height > 1 {
            let source = levels.last().map_or(pixels, |level: &MipLevel| level.pixels.as_slice());
            let level = halve(source, source_width, source_height);
            (source_width, source_height) = (level.width, level.height);
            levels.push(level);
        }

        Self { levels }
    }
}

/// One level of a texture's mip chain, RGBA8.
#[derive(Clone, Debug, PartialEq, Eq)]
struct MipLevel {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

/// The level below `pixels`: each texel the mean of the 2x2 block above it.
/// Where a dimension is odd its last row or column is dropped, except at
/// 1 texel, which is repeated. Averaged in the stored sRGB encoding.
fn halve(pixels: &[u8], width: u32, height: u32) -> MipLevel {
    let out_width = (width / 2).max(1) as usize;
    let out_height = (height / 2).max(1) as usize;
    let stride = width as usize * 4;
    let last_x = width as usize - 1;
    let last_y = height as usize - 1;

    let mut out = Vec::with_capacity(out_width * out_height * 4);
    for y in 0..out_height {
        let top = (2 * y).min(last_y) * stride;
        let bottom = (2 * y + 1).min(last_y) * stride;
        for x in 0..out_width {
            let left = (2 * x).min(last_x) * 4;
            let right = (2 * x + 1).min(last_x) * 4;
            for channel in 0..4 {
                let sum = u32::from(pixels[top + left + channel])
                    + u32::from(pixels[top + right + channel])
                    + u32::from(pixels[bottom + left + channel])
                    + u32::from(pixels[bottom + right + channel]);
                out.push(((sum + 2) / 4) as u8);
            }
        }
    }

    MipLevel { width: out_width as u32, height: out_height as u32, pixels: out }
}

/// The uniform sets one frame binds: model space with hull lighting, world
/// space, and model space unlit.
struct SceneUniforms {
    model_lit: Uniforms,
    world: Uniforms,
    model_unlit: Uniforms,
}

/// A uniform buffer and the bind group that exposes it to the shader.
struct UniformSlot {
    buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

impl UniformSlot {
    fn new(device: &wgpu::Device, pipeline: &GpuPipeline, label: &str) -> Self {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout: &pipeline.uniform_bind_group_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }],
        });

        Self { buffer, bind_group }
    }
}

/// What one viewport draws a frame with before resolving it into a target:
/// the multisampled color and depth attachments, and the uniform buffers
/// each frame rewrites.
pub struct SceneAttachments {
    size: TargetSize,
    msaa_color: wgpu::TextureView,
    depth: wgpu::TextureView,
    model_lit: UniformSlot,
    world: UniformSlot,
    model_unlit: UniformSlot,
}

impl SceneAttachments {
    pub fn new(device: &wgpu::Device, pipeline: &GpuPipeline, size: TargetSize) -> Self {
        let attachment = |label: &str, format: wgpu::TextureFormat| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: size.extent(),
                    mip_level_count: 1,
                    sample_count: MSAA_SAMPLE_COUNT,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };

        Self {
            size,
            msaa_color: attachment("viewport_3d_msaa_color", pipeline.color.format()),
            depth: attachment("viewport_3d_depth", DEPTH_FORMAT),
            model_lit: UniformSlot::new(device, pipeline, "viewport_3d_model_lit_uniforms"),
            world: UniformSlot::new(device, pipeline, "viewport_3d_world_uniforms"),
            model_unlit: UniformSlot::new(device, pipeline, "viewport_3d_model_unlit_uniforms"),
        }
    }

    pub fn size(&self) -> TargetSize {
        self.size
    }
}

/// Render layer constants. Lower values draw first (behind), higher values draw last (on top).
/// - Layers <= LAYER_OPAQUE_MAX: depth-writing pipeline (opaque armor).
/// - LAYER_HULL: no-depth-write pipeline with depth test (transparent hull, behind armor).
/// - LAYER_OVERLAY: no depth test at all (overlays and row highlights, always visible on top).
/// - LAYER_HIGHLIGHT: hover highlights, depth-tested or not per [`HighlightDepth`].
pub const LAYER_DEFAULT: i32 = 0;
pub const LAYER_HULL: i32 = 1;
pub const LAYER_OVERLAY: i32 = 2;
pub const LAYER_HIGHLIGHT: i32 = 3;

/// Layers at or below this value write to the depth buffer (opaque pass).
const LAYER_OPAQUE_MAX: i32 = 0;

/// Whether a hover highlight shows through the geometry in front of it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HighlightDepth {
    /// Drawn over everything.
    #[default]
    XRay,
    /// Hidden where other geometry is in front, as any surface is. A
    /// translucent hull writes no depth, so what shows through it stays lit.
    Tested,
}

/// How a layer's meshes meet the depth buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DepthState {
    /// Tested and written.
    Written,
    /// Tested, not written.
    Tested,
    /// Neither tested nor written: drawn over everything.
    Ignored,
}

/// How meshes on `layer` meet the depth buffer while hover highlights are in
/// `highlight`.
pub fn layer_depth_state(layer: i32, highlight: HighlightDepth) -> DepthState {
    LayerKind::of(layer).depth_state(highlight)
}

/// Which pipeline a layer draws with.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LayerKind {
    Opaque,
    Translucent,
    Overlay,
    Highlight,
}

impl LayerKind {
    fn of(layer: i32) -> Self {
        if layer <= LAYER_OPAQUE_MAX {
            Self::Opaque
        } else if layer < LAYER_OVERLAY {
            Self::Translucent
        } else if layer < LAYER_HIGHLIGHT {
            Self::Overlay
        } else {
            Self::Highlight
        }
    }

    fn depth_state(self, highlight: HighlightDepth) -> DepthState {
        match (self, highlight) {
            (Self::Opaque, _) => DepthState::Written,
            (Self::Translucent, _) | (Self::Highlight, HighlightDepth::Tested) => DepthState::Tested,
            (Self::Overlay, _) | (Self::Highlight, HighlightDepth::XRay) => DepthState::Ignored,
        }
    }
}

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

/// Drops a viewport's mesh buffers from outside it, for a device that is
/// being released.
pub struct MeshRelease(Weak<RefCell<HashMap<MeshId, GpuMesh>>>);

impl MeshRelease {
    /// Drops the buffers. The viewport keeps its pick data and uploads its
    /// meshes again on the next device.
    pub fn release(&self) {
        if let Some(meshes) = self.0.upgrade()
            && let Ok(mut meshes) = meshes.try_borrow_mut()
        {
            meshes.clear();
        }
    }
}

/// A complete 3D viewport instance. Each consumer (armor pane, replay viewer, etc.)
/// creates one of these. Holds its own camera and scene meshes; [`Self::render`]
/// draws them into a caller-owned target.
pub struct Viewport3D {
    pub camera: ArcballCamera,
    /// Shared so the device the buffers were made on can release them when
    /// it is lost (`MeshRelease`).
    meshes: Rc<RefCell<HashMap<MeshId, GpuMesh>>>,
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
    /// How [`LAYER_HIGHLIGHT`] meshes meet the geometry in front of them.
    highlight_depth: HighlightDepth,
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
            meshes: Rc::default(),
            pick_data: HashMap::new(),
            next_mesh_id: 0,
            clear_color: wgpu::Color { r: 0.12, g: 0.12, b: 0.18, a: 1.0 },
            needs_redraw: true,
            model_roll: 0.0,
            model_yaw: 0.0,
            cursor_ndc: None,
            lighting: LightingSettings::default(),
            highlight_depth: HighlightDepth::default(),
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

        self.meshes.borrow_mut().insert(
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
        self.pick_data.insert(id, PickableMesh::new(positions, indices.to_vec()));

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

        self.meshes.borrow_mut().insert(
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
        self.pick_data.insert(id, PickableMesh::new(positions, indices.to_vec()));

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

        self.meshes.borrow_mut().insert(
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

        self.meshes.borrow_mut().insert(
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

        self.meshes.borrow_mut().insert(
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

        self.meshes.borrow_mut().insert(
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
        self.meshes.borrow_mut().remove(&id);
        self.pick_data.remove(&id);
        self.needs_redraw = true;
    }

    /// Set mesh visibility.
    pub fn set_visible(&mut self, id: MeshId, visible: bool) {
        if let Some(mesh) = self.meshes.borrow_mut().get_mut(&id)
            && mesh.visible != visible
        {
            mesh.visible = visible;
            self.needs_redraw = true;
        }
    }

    /// Clear all meshes.
    pub fn clear(&mut self) {
        self.meshes.borrow_mut().clear();
        self.pick_data.clear();
        self.needs_redraw = true;
    }

    /// Mark the viewport as needing a redraw (e.g. after camera change).
    pub fn mark_dirty(&mut self) {
        self.needs_redraw = true;
    }

    /// Mark a mesh as world-space (unaffected by model_roll).
    pub fn set_world_space(&mut self, id: MeshId, world_space: bool) {
        if let Some(mesh) = self.meshes.borrow_mut().get_mut(&id) {
            mesh.world_space = world_space;
        }
    }

    /// Mark a mesh as lit (participates in hull lighting). Default is unlit.
    pub fn set_lit(&mut self, id: MeshId, lit: bool) {
        if let Some(mesh) = self.meshes.borrow_mut().get_mut(&id) {
            mesh.lit = lit;
            self.needs_redraw = true;
        }
    }

    /// Whether the viewport needs a redraw.
    pub fn is_dirty(&self) -> bool {
        self.needs_redraw
    }

    /// Clear the redraw flag once a frame of the current scene has been drawn.
    pub fn clear_dirty(&mut self) {
        self.needs_redraw = false;
    }

    pub fn highlight_depth(&self) -> HighlightDepth {
        self.highlight_depth
    }

    /// Sets how highlights meet the geometry in front of them, redrawing
    /// when that changes.
    pub fn set_highlight_depth(&mut self, depth: HighlightDepth) {
        if self.highlight_depth != depth {
            self.highlight_depth = depth;
            self.needs_redraw = true;
        }
    }

    /// Returns true if the viewport has any meshes to render.
    pub fn has_meshes(&self) -> bool {
        !self.meshes.borrow().is_empty()
    }

    /// A handle the device the meshes were made on uses to release them.
    pub fn mesh_release(&self) -> MeshRelease {
        MeshRelease(Rc::downgrade(&self.meshes))
    }

    /// Build the per-frame uniform sets from the current camera, model
    /// rotation, and lighting.
    fn scene_uniforms(&self, size: TargetSize) -> SceneUniforms {
        let aspect = size.width() as f32 / size.height() as f32;
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

        SceneUniforms { model_lit: uniforms, world: world_uniforms, model_unlit: model_unlit_uniforms }
    }

    /// Draws the scene into `resolve_target`, a single-sample view in the
    /// pipeline's color format and the attachments' size, and submits the work
    /// on `queue`.
    pub fn render(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pipeline: &GpuPipeline,
        attachments: &SceneAttachments,
        resolve_target: &wgpu::TextureView,
    ) {
        let uniforms = self.scene_uniforms(attachments.size);
        queue.write_buffer(&attachments.model_lit.buffer, 0, bytemuck::bytes_of(&uniforms.model_lit));
        queue.write_buffer(&attachments.world.buffer, 0, bytemuck::bytes_of(&uniforms.world));
        queue.write_buffer(&attachments.model_unlit.buffer, 0, bytemuck::bytes_of(&uniforms.model_unlit));

        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("viewport_3d_encoder") });
        self.encode_scene_pass(&mut encoder, pipeline, attachments, resolve_target);
        queue.submit(std::iter::once(encoder.finish()));
    }

    /// Record the scene draw into `encoder`, resolving into `resolve_target`.
    /// Sorts meshes by layer and binds the matching pipeline/uniform group per
    /// mesh.
    fn encode_scene_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        pipeline: &GpuPipeline,
        attachments: &SceneAttachments,
        resolve_target: &wgpu::TextureView,
    ) {
        let model_lit_bg = &attachments.model_lit.bind_group;
        let world_bg = &attachments.world.bind_group;
        let model_unlit_bg = &attachments.model_unlit.bind_group;
        let clear = pipeline.color.clear_value(self.clear_color);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("viewport_3d_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &attachments.msaa_color,
                resolve_target: Some(resolve_target),
                // The resolve carries the frame; the multisampled color is not
                // read again.
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(clear), store: wgpu::StoreOp::Discard },
                depth_slice: None,
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &attachments.depth,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                stencil_ops: None,
            }),
            ..Default::default()
        });

        pass.set_bind_group(0, model_unlit_bg, &[]);
        // Start with fallback texture; per-mesh textures override below.
        pass.set_bind_group(1, &pipeline.fallback_texture_bind_group, &[]);

        // Sort meshes by layer: armor (opaque, depth-write) first, then hull, overlays and highlights (no depth-write).
        let meshes = self.meshes.borrow();
        let mut sorted: Vec<(MeshId, &GpuMesh)> =
            meshes.iter().filter(|(_, m)| m.visible && m.index_count > 0).map(|(id, m)| (*id, m)).collect();
        sorted.sort_by_key(|(_, m)| m.layer);

        let mut current_layer_kind: Option<LayerKind> = None;
        let mut has_custom_texture = false; // track whether we need to rebind fallback
        let mut current_bind = BindKind::ModelUnlit;
        for (_id, mesh) in sorted {
            let layer_kind = LayerKind::of(mesh.layer);
            if current_layer_kind != Some(layer_kind) {
                pass.set_pipeline(pipeline.layer_pipeline(layer_kind, self.highlight_depth));
                current_layer_kind = Some(layer_kind);
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
        let meshes = self.meshes.borrow();
        self.pick_data
            .iter()
            .map(|(id, mesh)| {
                let visible = meshes.get(id).is_some_and(|m| m.visible);
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

    /// Unproject a screen position to a ray in model space, the space mesh
    /// vertices are uploaded in.
    pub fn screen_to_model_ray(&self, screen_pos: Vec2, viewport_rect: ViewRect) -> Option<(Vec3, Vec3)> {
        let (origin, dir) = picking::screen_to_ray(screen_pos, viewport_rect, &self.camera)?;

        Some(self.ray_to_model_space(origin, dir))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mip_chain_runs_down_to_one_texel() {
        let sizes = |width: u32, height: u32| -> Vec<(u32, u32)> {
            let pixels = vec![0u8; (width * height * 4) as usize];
            MipChain::build(width, height, &pixels).levels.iter().map(|level| (level.width, level.height)).collect()
        };

        assert!(sizes(1, 1).is_empty());
        assert_eq!(sizes(8, 2), vec![(4, 1), (2, 1), (1, 1)]);
        assert_eq!(sizes(300, 7).len(), 8);
    }

    #[test]
    fn a_halved_level_averages_each_block_and_repeats_an_odd_edge() {
        // 3x2: the right column has no partner and is averaged with itself.
        let pixels: Vec<u8> = [[0u8, 0, 0, 255], [100, 0, 0, 255], [40, 0, 0, 255]]
            .into_iter()
            .chain([[200, 0, 0, 255], [100, 0, 0, 255], [80, 0, 0, 255]])
            .flatten()
            .collect();

        let halved = halve(&pixels, 3, 2);

        assert_eq!((halved.width, halved.height), (1, 1));
        assert_eq!(halved.pixels, vec![100, 0, 0, 255]);
    }
}
