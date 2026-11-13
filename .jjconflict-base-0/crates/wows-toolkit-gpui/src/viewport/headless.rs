//! Test-only offscreen rendering with pixel readback.
//!
//! The application renders on GPUI's device and never reads a frame back. Tests
//! that assert on pixels stand up a device of their own here and copy the
//! resolved frame to the CPU.

use gpui_wgpu::wgpu;

use crate::viewport::renderer::GpuPipeline;
use crate::viewport::renderer::SceneAttachments;
use crate::viewport::renderer::SceneColorTarget;
use crate::viewport::renderer::Viewport3D;
use crate::viewport::types::TargetSize;

/// The target a window shows: sRGB, encoded by the hardware.
pub const SRGB_TARGET: SceneColorTarget = SceneColorTarget::Srgb(wgpu::TextureFormat::Rgba8UnormSrgb);

/// A standalone device for tests, with the viewport pipeline on it.
pub struct HeadlessDevice {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub pipeline: GpuPipeline,
}

impl HeadlessDevice {
    /// The first adapter wgpu offers, with no surface, and the pipeline
    /// rendering into `color`.
    pub fn new(color: SceneColorTarget) -> anyhow::Result<Self> {
        let instance = wgpu::Instance::default();
        let adapter = gpui_kit::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
            apply_limit_buckets: false,
        }))
        .map_err(|e| anyhow::anyhow!("no suitable wgpu adapter: {e}"))?;
        let (device, queue) = gpui_kit::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("armor_viewport_test_device"),
            ..Default::default()
        }))
        .map_err(|e| anyhow::anyhow!("failed to create wgpu device: {e}"))?;
        let pipeline = GpuPipeline::new(&device, &queue, color);

        Ok(Self { device, queue, pipeline })
    }

    /// Renders `viewport` and returns the frame as tightly packed RGBA8 in
    /// the pipeline's color format, row by row. Both target kinds store
    /// sRGB-encoded values.
    pub fn render_rgba(&self, viewport: &Viewport3D, size: TargetSize) -> Vec<u8> {
        let attachments = SceneAttachments::new(&self.device, &self.pipeline, size);
        let resolve = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("viewport_3d_test_resolve"),
            size: size.extent(),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.pipeline.color().format(),
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        viewport.render(
            &self.device,
            &self.queue,
            &self.pipeline,
            &attachments,
            &resolve.create_view(&Default::default()),
        );

        // Rows of a buffer copy are aligned to 256 bytes; the padding is
        // stripped after mapping.
        let row_bytes = size.width() * 4;
        let padded_row_bytes =
            row_bytes.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("viewport_3d_test_readback"),
            size: u64::from(padded_row_bytes * size.height()),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("viewport_3d_test_copy") });
        encoder.copy_texture_to_buffer(
            resolve.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row_bytes),
                    rows_per_image: Some(size.height()),
                },
            },
            size.extent(),
        );
        self.queue.submit(std::iter::once(encoder.finish()));

        let slice = readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        self.device.poll(wgpu::PollType::wait_indefinitely()).expect("the test device finishes its work");
        let mapped = slice.get_mapped_range().expect("the readback buffer is mapped");
        let mut rgba = Vec::with_capacity((row_bytes * size.height()) as usize);
        for row in mapped.chunks_exact(padded_row_bytes as usize) {
            rgba.extend_from_slice(&row[..row_bytes as usize]);
        }

        rgba
    }
}

mod tests {
    use super::*;
    use crate::viewport::camera::ArcballCamera;
    use crate::viewport::renderer::AlbedoUv;
    use crate::viewport::renderer::HighlightDepth;
    use crate::viewport::renderer::LAYER_DEFAULT;
    use crate::viewport::renderer::LAYER_HIGHLIGHT;
    use crate::viewport::renderer::LAYER_OVERLAY;
    use crate::viewport::renderer::MaterialBinding;
    use crate::viewport::renderer::MipChain;
    use crate::viewport::renderer::TextureEncoding;
    use crate::viewport::renderer::TextureImage;
    use crate::viewport::tangents::vertex_tangents;
    use crate::viewport::types::Vec3;
    use crate::viewport::types::Vertex;

    /// A unit cube centered at the origin with one flat color on every vertex.
    /// Normals are placeholders (the cube renders unlit); winding is irrelevant
    /// since the pipeline is double-sided.
    fn unit_cube(color: [f32; 4]) -> (Vec<Vertex>, Vec<u32>) {
        let corners = [
            [-0.5, -0.5, -0.5],
            [0.5, -0.5, -0.5],
            [0.5, 0.5, -0.5],
            [-0.5, 0.5, -0.5],
            [-0.5, -0.5, 0.5],
            [0.5, -0.5, 0.5],
            [0.5, 0.5, 0.5],
            [-0.5, 0.5, 0.5],
        ];
        let vertices: Vec<Vertex> = corners
            .iter()
            .map(|&position| Vertex { position, normal: [0.0, 0.0, 1.0], color, uv: [0.0, 0.0], tangent: [0.0; 4] })
            .collect();
        #[rustfmt::skip]
        let indices: Vec<u32> = vec![
            0, 1, 2, 0, 2, 3, // back
            4, 6, 5, 4, 7, 6, // front
            0, 4, 5, 0, 5, 1, // bottom
            3, 2, 6, 3, 6, 7, // top
            0, 3, 7, 0, 7, 4, // left
            1, 5, 6, 1, 6, 2, // right
        ];

        (vertices, indices)
    }

    /// Renders a green cube into `color` and returns its center pixel and a
    /// corner pixel of the clear color.
    fn cube_pixels(color: SceneColorTarget) -> [[u8; 4]; 2] {
        let headless = HeadlessDevice::new(color).expect("a wgpu adapter is available");
        let mut viewport = Viewport3D::new();
        let (vertices, indices) = unit_cube([0.0, 0.8, 0.0, 1.0]);
        viewport.add_mesh(&headless.device, &vertices, &indices, LAYER_DEFAULT);
        viewport.camera = ArcballCamera::from_bounds(Vec3::new(-0.5, -0.5, -0.5), Vec3::new(0.5, 0.5, 0.5));
        let size = TargetSize::new(256, 256).expect("a nonzero size");

        let rgba = headless.render_rgba(&viewport, size);
        assert_eq!(rgba.len(), (size.width() * size.height() * 4) as usize);

        let center = ((size.height() / 2 * size.width() + size.width() / 2) * 4) as usize;
        let pixel = |at: usize| [rgba[at], rgba[at + 1], rgba[at + 2], rgba[at + 3]];
        [pixel(center), pixel(0)]
    }

    /// A green cube renders, and the center pixel is the mesh color (not the
    /// clear color) with an sRGB-encoded value.
    ///
    /// Needs a real GPU adapter. Run with:
    ///
    /// ```text
    /// cargo test -p wows-toolkit-gpui -- --ignored --nocapture headless_device_renders_cube_center_pixel
    /// ```
    #[test]
    #[ignore = "needs a real GPU adapter; see the doc comment for the run command"]
    fn headless_device_renders_cube_center_pixel() {
        let [[r, g, b, a], _] = cube_pixels(SRGB_TARGET);
        eprintln!("center pixel RGBA = ({r}, {g}, {b}, {a})");

        // The clear color is a dark blue-grey; the cube face is saturated green.
        assert!(g > r + 60 && g > b + 60, "the center ({r}, {g}, {b}) is not the green cube");
        assert_eq!(a, 255, "the cube is opaque");
        // Linear 0.8 encodes to about 231; a target storing linear values would
        // hold 204.
        assert!(g >= 210, "green {g} is below the sRGB-encoded value; sRGB encoding looks wrong");
    }

    /// A target the device cannot view as sRGB gets the encoding from the
    /// shader, and stores what the hardware-encoded target does.
    ///
    /// Needs a real GPU adapter. Run with:
    ///
    /// ```text
    /// cargo test -p wows-toolkit-gpui -- --ignored --nocapture shader_encoded_target_matches_srgb
    /// ```
    #[test]
    #[ignore = "needs a real GPU adapter; see the doc comment for the run command"]
    fn shader_encoded_target_matches_srgb() {
        let srgb = cube_pixels(SRGB_TARGET);
        let encoded = cube_pixels(SceneColorTarget::ShaderEncoded(wgpu::TextureFormat::Rgba8Unorm));

        for (which, (srgb, encoded)) in ["center", "corner"].iter().zip(srgb.iter().zip(&encoded)) {
            assert!(
                srgb.iter().zip(encoded).all(|(s, e)| s.abs_diff(*e) <= 1),
                "the {which} pixel is {encoded:?} shader-encoded and {srgb:?} sRGB"
            );
        }
    }

    /// The center pixel of an opaque green cube under a white highlight
    /// cube `scale` times its size on `layer`, drawn in `depth`.
    fn highlighted_cube_center(headless: &HeadlessDevice, scale: f32, layer: i32, depth: HighlightDepth) -> [u8; 4] {
        let mut viewport = Viewport3D::new();
        let (vertices, indices) = unit_cube([0.0, 0.8, 0.0, 1.0]);
        viewport.add_mesh(&headless.device, &vertices, &indices, LAYER_DEFAULT);
        let (mut highlight, indices) = unit_cube([1.0, 1.0, 1.0, 0.35]);
        for vertex in &mut highlight {
            vertex.position = vertex.position.map(|axis| axis * scale);
        }
        viewport.add_non_pickable_mesh(&headless.device, &highlight, &indices, layer);
        viewport.set_highlight_depth(depth);
        viewport.camera = ArcballCamera::from_bounds(Vec3::new(-0.5, -0.5, -0.5), Vec3::new(0.5, 0.5, 0.5));
        let size = TargetSize::new(64, 64).expect("a nonzero size");

        let rgba = headless.render_rgba(&viewport, size);
        let center = ((size.height() / 2 * size.width() + size.width() / 2) * 4) as usize;

        [rgba[center], rgba[center + 1], rgba[center + 2], rgba[center + 3]]
    }

    /// A hover highlight inside the cube shows through it only in x-ray, one
    /// on the cube's own surface shows depth-tested too, and a row highlight
    /// on the overlay layer shows through whatever the hover depth.
    ///
    /// Needs a real GPU adapter. Run with:
    ///
    /// ```text
    /// cargo test -p wows-toolkit-gpui -- --ignored --nocapture a_tested_highlight_shows_where_its_surface_is_visible
    /// ```
    #[test]
    #[ignore = "needs a real GPU adapter; see the doc comment for the run command"]
    fn a_tested_highlight_shows_where_its_surface_is_visible() {
        let headless = HeadlessDevice::new(SRGB_TARGET).expect("a wgpu adapter is available");
        let inside_xray = highlighted_cube_center(&headless, 0.5, LAYER_HIGHLIGHT, HighlightDepth::XRay);
        let inside_tested = highlighted_cube_center(&headless, 0.5, LAYER_HIGHLIGHT, HighlightDepth::Tested);
        let surface_tested = highlighted_cube_center(&headless, 1.0, LAYER_HIGHLIGHT, HighlightDepth::Tested);
        let row_tested = highlighted_cube_center(&headless, 0.5, LAYER_OVERLAY, HighlightDepth::Tested);
        eprintln!(
            "inside x-ray {inside_xray:?} inside tested {inside_tested:?} surface tested {surface_tested:?} row {row_tested:?}"
        );

        // The white highlight lifts red off the green cube's zero.
        let highlighted = |[r, ..]: [u8; 4]| r > 100;
        assert!(highlighted(inside_xray), "x-ray shows the highlight through the cube: {inside_xray:?}");
        assert!(!highlighted(inside_tested), "the cube hides a tested highlight: {inside_tested:?}");
        assert!(highlighted(surface_tested), "a tested highlight shows on its own surface: {surface_tested:?}");
        assert!(highlighted(row_tested), "a row highlight ignores the hover depth: {row_tested:?}");
    }

    /// Renders a lit, textured quad facing the camera with the given normal
    /// and metal-gloss texels, and returns its center pixel.
    fn lit_quad_center(headless: &HeadlessDevice, normal: Option<[u8; 4]>, metal_gloss: Option<[u8; 4]>) -> [u8; 4] {
        let positions = [[-0.5, -0.5, 0.0], [0.5, -0.5, 0.0], [0.5, 0.5, 0.0], [-0.5, 0.5, 0.0]];
        let normals = [[0.0, 0.0, 1.0]; 4];
        let uvs = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
        let indices = vec![0, 1, 2, 0, 2, 3];
        let tangents = vertex_tangents(&positions, &normals, &uvs, &indices);
        let vertices: Vec<Vertex> = (0..4)
            .map(|i| Vertex {
                position: positions[i],
                normal: normals[i],
                color: [1.0; 4],
                uv: uvs[i],
                tangent: tangents[i],
            })
            .collect();

        let no_mips = MipChain::default();
        let texel = |pixels: &[u8; 4], encoding| {
            let image = TextureImage { pixels, width: 1, height: 1, mips: &no_mips };
            headless.pipeline.upload_texture(&headless.device, &headless.queue, image, encoding)
        };
        let albedo = texel(&[128, 128, 128, 255], TextureEncoding::Srgb);
        let normal = normal.map(|pixels| texel(&pixels, TextureEncoding::Linear));
        let metal_gloss = metal_gloss.map(|pixels| texel(&pixels, TextureEncoding::Linear));
        let binding = MaterialBinding {
            albedo: &albedo,
            normal: normal.as_ref(),
            metal_gloss: metal_gloss.as_ref(),
            alpha_cutoff: 0.0,
            albedo_uv: AlbedoUv::IDENTITY,
        };
        let bind_group = headless.pipeline.create_material_bind_group(&headless.device, binding);

        let mut viewport = Viewport3D::new();
        let mesh =
            viewport.add_textured_non_pickable_mesh(&headless.device, &vertices, &indices, LAYER_DEFAULT, bind_group);
        viewport.set_lit(mesh, true);
        viewport.camera = ArcballCamera::from_bounds(Vec3::new(-0.5, -0.5, -0.1), Vec3::new(0.5, 0.5, 0.1));
        let size = TargetSize::new(64, 64).expect("a nonzero size");

        let rgba = headless.render_rgba(&viewport, size);
        let center = ((size.height() / 2 * size.width() + size.width() / 2) * 4) as usize;
        [rgba[center], rgba[center + 1], rgba[center + 2], rgba[center + 3]]
    }

    /// A material without surface maps shades as one with a flat normal map
    /// and no metal-gloss map; a tilted normal changes the shading, and under
    /// a highlight so do gloss and metalness.
    ///
    /// Needs a real GPU adapter. Run with:
    ///
    /// ```text
    /// cargo test -p wows-toolkit-gpui -- --ignored --nocapture surface_maps_change_only_what_they_name
    /// ```
    #[test]
    #[ignore = "needs a real GPU adapter; see the doc comment for the run command"]
    fn surface_maps_change_only_what_they_name() {
        let headless = HeadlessDevice::new(SRGB_TARGET).expect("a wgpu adapter is available");

        let plain = lit_quad_center(&headless, None, None);
        let flat = lit_quad_center(&headless, Some([128, 128, 0, 255]), None);
        let tilted = lit_quad_center(&headless, Some([230, 128, 0, 255]), None);
        // Tilted toward the default key light's highlight, so gloss and metal show.
        let toward_highlight = Some([255, 204, 0, 255]);
        let matte = lit_quad_center(&headless, toward_highlight, Some([0, 0, 0, 255]));
        let glossy = lit_quad_center(&headless, toward_highlight, Some([255, 0, 0, 255]));
        let glossy_metal = lit_quad_center(&headless, toward_highlight, Some([255, 255, 0, 255]));
        eprintln!("plain {plain:?} flat {flat:?} tilted {tilted:?}");
        eprintln!("matte {matte:?} glossy {glossy:?} glossy metal {glossy_metal:?}");

        assert!(plain.iter().zip(&flat).all(|(p, f)| p.abs_diff(*f) <= 1), "{plain:?} != {flat:?}");
        let differs = |a: [u8; 4], b: [u8; 4]| a.iter().zip(&b).take(3).any(|(a, b)| a.abs_diff(*b) > 4);
        assert!(differs(plain, tilted), "{plain:?} ~ {tilted:?}");
        assert!(differs(matte, glossy), "gloss narrows the highlight: {matte:?} ~ {glossy:?}");
        assert!(differs(glossy, glossy_metal), "metal tints it by the gray albedo: {glossy:?} ~ {glossy_metal:?}");
    }
}
