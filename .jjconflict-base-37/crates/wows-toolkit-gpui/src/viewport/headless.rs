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
    use crate::viewport::renderer::LAYER_DEFAULT;
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
            .map(|&position| Vertex { position, normal: [0.0, 0.0, 1.0], color, uv: [0.0, 0.0] })
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
}
