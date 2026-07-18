//! Owned wgpu device for the armor viewport plus the CPU-readback bridge into a
//! gpui `RenderImage`.
//!
//! gpui renders the app through its own backend (DirectX on Windows); this
//! module stands up a separate, standalone wgpu-29 device the viewport renders
//! into offscreen. The resolved pixels are read back on the CPU and handed to
//! gpui as a `RenderImage` for display via `img()`.

use std::sync::Arc;

use gpui::RenderImage;
use image::Frame;

use crate::viewport::camera::ArcballCamera;
use crate::viewport::renderer::GpuPipeline;
use crate::viewport::renderer::LAYER_DEFAULT;
use crate::viewport::renderer::Viewport3D;
use crate::viewport::types::Vec3;
use crate::viewport::types::Vertex;

/// A standalone wgpu instance/adapter/device/queue owned by the armor viewport.
/// Created once; the `Arc<Device>`/`Arc<Queue>` are shared with every render.
pub struct GpuContext {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,
}

impl GpuContext {
    /// Stand up the owned device. Blocks on adapter/device requests via pollster.
    pub fn new() -> anyhow::Result<Self> {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .map_err(|e| anyhow::anyhow!("no suitable wgpu adapter: {e}"))?;

        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("armor_viewport_device"),
                ..Default::default()
            }))
            .map_err(|e| anyhow::anyhow!("failed to create wgpu device: {e}"))?;

        Ok(Self { instance, adapter, device: Arc::new(device), queue: Arc::new(queue) })
    }

    /// Build the shared GPU pipeline for this device.
    pub fn pipeline(&self) -> GpuPipeline {
        GpuPipeline::new(&self.device, &self.queue)
    }
}

/// Convert a tightly-packed sRGB RGBA8 readback (`width * height * 4`) from
/// `Viewport3D::render_offscreen_rgba` into a gpui `RenderImage`.
///
/// gpui stores images as BGRA, so the R and B bytes are swapped in place before
/// the buffer is wrapped in an `image::Frame`.
pub fn readback_to_render_image(width: u32, height: u32, mut rgba: Vec<u8>) -> Arc<RenderImage> {
    for px in rgba.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    let buffer =
        image::RgbaImage::from_raw(width, height, rgba).expect("readback buffer length equals width * height * 4");
    Arc::new(RenderImage::new(vec![Frame::new(buffer)]))
}

/// Build a unit cube centered at the origin with one flat color on every vertex.
/// Normals are placeholders (the cube renders unlit); winding is irrelevant since
/// the pipeline is double-sided.
pub fn unit_cube(color: [f32; 4]) -> (Vec<Vertex>, Vec<u32>) {
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
    let vertices: Vec<Vertex> =
        corners.iter().map(|&position| Vertex { position, normal: [0.0, 0.0, 1.0], color, uv: [0.0, 0.0] }).collect();
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

/// Render a single colored test cube offscreen and return the raw sRGB RGBA8
/// readback. The Task-1 risk-gate primitive: owned device -> offscreen render ->
/// CPU readback.
pub fn render_test_cube_rgba(
    ctx: &GpuContext,
    pipeline: &GpuPipeline,
    size: (u32, u32),
    color: [f32; 4],
) -> Option<(u32, u32, Vec<u8>)> {
    let mut viewport = Viewport3D::new();
    let (vertices, indices) = unit_cube(color);
    viewport.add_mesh(&ctx.device, &vertices, &indices, LAYER_DEFAULT);
    viewport.camera = ArcballCamera::from_bounds(Vec3::new(-0.5, -0.5, -0.5), Vec3::new(0.5, 0.5, 0.5));
    viewport.render_offscreen_rgba(&ctx.device, &ctx.queue, pipeline, size)
}

/// Render the test cube and wrap the readback as a gpui image for display.
pub fn render_test_cube_image(
    ctx: &GpuContext,
    pipeline: &GpuPipeline,
    size: (u32, u32),
    color: [f32; 4],
) -> Option<Arc<RenderImage>> {
    let (w, h, rgba) = render_test_cube_rgba(ctx, pipeline, size, color)?;
    Some(readback_to_render_image(w, h, rgba))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `readback_to_render_image` swaps R and B in place so the RGBA readback
    /// becomes BGRA (gpui's `RenderImage` pixel layout); alpha is untouched.
    #[test]
    fn readback_to_render_image_swaps_r_and_b_to_bgra() {
        let rgba = vec![10u8, 20, 30, 40];
        let image = readback_to_render_image(1, 1, rgba.clone());
        let swapped = image.as_bytes(0).expect("single-frame image has frame 0");
        assert_eq!(swapped[0], rgba[2]);
        assert_eq!(swapped[1], rgba[1]);
        assert_eq!(swapped[2], rgba[0]);
        assert_eq!(swapped[3], rgba[3]);
    }

    /// The risk gate: an owned wgpu-29 device renders a green cube offscreen, the
    /// pixels read back, and the center pixel is the mesh color (not the clear
    /// color), with an sRGB-plausible value.
    #[test]
    fn owned_device_renders_cube_and_reads_back_center_pixel() {
        let Ok(ctx) = GpuContext::new() else {
            panic!("owned wgpu device creation failed - no adapter available");
        };
        let pipeline = ctx.pipeline();
        let size = (256u32, 256u32);
        let color = [0.0, 0.8, 0.0, 1.0]; // green cube

        let (w, h, rgba) =
            render_test_cube_rgba(&ctx, &pipeline, size, color).expect("offscreen render produced no pixels");
        assert_eq!((w, h), size);
        assert_eq!(rgba.len(), (w * h * 4) as usize);

        let cx = w / 2;
        let cy = h / 2;
        let idx = ((cy * w + cx) * 4) as usize;
        let (r, g, b, a) = (rgba[idx], rgba[idx + 1], rgba[idx + 2], rgba[idx + 3]);
        eprintln!("center pixel RGBA = ({r}, {g}, {b}, {a})");

        // Clear color is a dark blue-grey (~97, 97, 109 after sRGB encode); the
        // cube face is saturated green. Assert green dominates by a wide margin.
        assert!(g > 150, "center green channel {g} too low - looks like the clear color, not the cube");
        assert!(g > r + 60, "green {g} does not dominate red {r} - center is not the green cube");
        assert!(g > b + 60, "green {g} does not dominate blue {b} - center is not the green cube");
        assert_eq!(a, 255, "cube is opaque");

        // sRGB plausibility: linear 0.8 sRGB-encodes to ~231. A non-sRGB target
        // would store linear*255 = 204. The MSAA-resolved center of a solid face
        // sits near 231, well above 204, confirming the sRGB write path.
        assert!(g >= 210, "green {g} below the sRGB-encoded value (~231); sRGB encoding looks wrong");
    }
}
