//! Frames rasterised on the CPU, shown through a `surface` element.
//!
//! A [`FrameSink`] takes [`BgraFrame`]s from any thread and keeps only the newest. A [`FrameView`]
//! shows it. zgui renders the element only when the sink asks, so a view whose frames have stopped
//! costs no frames at all.
//!
//! The view is drawn by a callback renderer rather than fed presented textures, because zgui
//! stretches a presented texture over the element's box and a frame has to keep its shape. The
//! renderer keeps one texture the size of the frame, written once per frame, and draws it fitted
//! into the texture zgui sizes to the element. zgui renders again by itself after a resize, a lost
//! device, or a texture it released while the element was hidden; each of those redraws from the
//! frame texture, and a new device has the newest frame written to it again.
//!
//! zgui samples a surface's texture as stored, with no sRGB decode, and a frame's bytes are already
//! sRGB, so both textures are `Unorm`.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;

use wows_toolkit_replay_data::frame::BgraFrame;
use zgui::component;
use zgui::css;
use zgui::elements::surface;
use zgui::geom::Device;
use zgui::geom::Size;
use zgui::surface::SurfaceElementExt;
use zgui::surface::SurfaceInvalidator;
use zgui::surface::SurfaceRenderCx;
use zgui::surface::SurfaceRenderer;
use zgui::surface::wgpu;
use zgui::view::Classes;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;

const SHEET: &str = "wt-frame-view";

const CSS: &str = css!(
    ".wt-frame-view {
        display: block;
        width: 100%;
        height: 100%;
        min-width: 0;
        min-height: 0;
    }"
);

/// The format of the texture zgui allocates for the renderer.
const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Draws the frame texture over the whole viewport, which the renderer sets to the fitted
/// rectangle.
const SHADER: &str = "
struct Placed {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> Placed {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    var placed: Placed;
    placed.position = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    placed.uv = uv;
    return placed;
}

@group(0) @binding(0) var frame: texture_2d<f32>;
@group(0) @binding(1) var smoothing: sampler;

@fragment
fn fragment(placed: Placed) -> @location(0) vec4<f32> {
    return textureSample(frame, smoothing, placed.uv);
}
";

/// Hands frames to the [`FrameView`] showing them, from any thread.
///
/// Shown by one view at a time: the view binds the sink's invalidator, and zgui binds an
/// invalidator to one renderer.
#[derive(Clone)]
pub struct FrameSink(Arc<Mailbox>);

impl FrameSink {
    pub fn new() -> Self {
        Self(Arc::new(Mailbox {
            latest: Mutex::new(Latest { frame: None, serial: FrameSerial(0), owed: false }),
            invalidator: SurfaceInvalidator::new(),
        }))
    }

    /// Shows `frame` in place of the one before it. Frames handed over faster than the window
    /// draws replace each other, and only the newest is written.
    pub fn push(&self, frame: BgraFrame) {
        self.0.hand_over(Some(frame));
    }

    /// Shows nothing.
    pub fn clear(&self) {
        self.0.hand_over(None);
    }

    /// The size of the newest frame. `None` after [`Self::clear`] and before the first frame.
    #[cfg(test)]
    pub fn frame_size(&self) -> Option<FrameSize> {
        self.0.latest().frame.as_ref().map(FrameSize::of)
    }
}

impl Default for FrameSink {
    fn default() -> Self {
        Self::new()
    }
}

/// A frame's extent in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameSize {
    pub width: u32,
    pub height: u32,
}

impl FrameSize {
    fn of(frame: &BgraFrame) -> Self {
        Self { width: frame.width(), height: frame.height() }
    }
}

/// What a [`FrameSink`] and the renderer showing it share.
struct Mailbox {
    latest: Mutex<Latest>,
    invalidator: SurfaceInvalidator,
}

impl Mailbox {
    fn hand_over(&self, frame: Option<BgraFrame>) {
        let wake = {
            let mut latest = self.latest();
            latest.frame = frame;
            latest.serial = FrameSerial(latest.serial.0 + 1);
            !std::mem::replace(&mut latest.owed, true)
        };
        if wake {
            self.invalidator.invalidate();
        }
    }

    /// The newest frame, taken by a render, so the next push wakes the window again.
    fn take(&self) -> Taken {
        let mut latest = self.latest();
        latest.owed = false;

        Taken { frame: latest.frame.clone(), serial: latest.serial }
    }

    /// A poisoned lock still holds a whole frame: nothing panics between its writes.
    fn latest(&self) -> MutexGuard<'_, Latest> {
        self.latest.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

struct Latest {
    frame: Option<BgraFrame>,
    serial: FrameSerial,
    /// Whether a push or clear waits for a render, so a burst of pushes wakes the window once.
    owed: bool,
}

/// Counts pushes and clears, so a renderer tells a new frame from the one it already wrote.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FrameSerial(u64);

struct Taken {
    frame: Option<BgraFrame>,
    serial: FrameSerial,
}

/// Shows the newest frame handed to `sink`, fitted inside the element without stretching.
///
/// The element fills its container unless `class` sizes it.
#[component]
pub fn FrameView(sink: FrameSink, #[prop(into, optional)] class: Classes) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let renderer = FrameRenderer { slot: FrameSlot::new(Arc::clone(&sink.0)), device: None };

    surface().class(SHEET).class(class).renderer_with_invalidator(renderer, &sink.0.invalidator).into_view()
}

/// What zgui calls to fill the element.
struct FrameRenderer {
    slot: FrameSlot,
    /// Rebuilt for each device zgui renders on.
    device: Option<DeviceFrames>,
}

impl SurfaceRenderer for FrameRenderer {
    fn render(&mut self, cx: &mut SurfaceRenderCx<'_>) {
        let fresh = self.device.as_ref().is_none_or(|held| held.device != *cx.device);
        if fresh {
            self.device = Some(DeviceFrames::new(cx.device));
        }
        let Some(device) = self.device.as_mut() else { return };

        let mut target = DeviceTarget { device, cx, fresh };
        self.slot.render_to(&mut target);
    }

    fn format(&self) -> wgpu::TextureFormat {
        TARGET_FORMAT
    }
}

/// The device-independent half of a render: which frame to write, and when.
struct FrameSlot {
    mailbox: Arc<Mailbox>,
    /// The serial of the frame the target holds.
    written: Option<FrameSerial>,
}

impl FrameSlot {
    fn new(mailbox: Arc<Mailbox>) -> Self {
        Self { mailbox, written: None }
    }

    /// Writes the newest frame when the target does not hold it, then draws.
    fn render_to(&mut self, target: &mut impl FrameTarget) {
        let taken = self.mailbox.take();
        if target.is_fresh() || self.written != Some(taken.serial) {
            target.write(taken.frame.as_ref());
            self.written = Some(taken.serial);
        }

        target.draw();
    }
}

/// Where a render writes and draws: the graphics device, or a recorder under test.
trait FrameTarget {
    /// Whether the target holds no frame written before, as on a new device.
    fn is_fresh(&self) -> bool;
    /// Takes `frame` as the one drawn; `None` draws nothing.
    fn write(&mut self, frame: Option<&BgraFrame>);
    fn draw(&mut self);
}

/// The frame texture and what draws it, on one device.
struct DeviceFrames {
    device: wgpu::Device,
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    frame: Option<FrameTexture>,
}

impl DeviceFrames {
    fn new(device: &wgpu::Device) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("wt.frame_view"),
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
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("wt.frame_view"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("wt.frame_view"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("wt.frame_view"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: TARGET_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("wt.frame_view"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..wgpu::SamplerDescriptor::default()
        });

        Self { device: device.clone(), layout, pipeline, sampler, frame: None }
    }
}

/// A texture holding one frame, and the size it was made at.
struct FrameTexture {
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    size: FrameSize,
}

/// One render's view of the device.
struct DeviceTarget<'a, 'cx> {
    device: &'a mut DeviceFrames,
    cx: &'a mut SurfaceRenderCx<'cx>,
    fresh: bool,
}

impl FrameTarget for DeviceTarget<'_, '_> {
    fn is_fresh(&self) -> bool {
        self.fresh
    }

    fn write(&mut self, frame: Option<&BgraFrame>) {
        // A frame with no area has nothing to draw, and a texture cannot be made at its size.
        let Some(frame) = frame.filter(|frame| frame.width() > 0 && frame.height() > 0) else {
            self.device.frame = None;
            return;
        };

        let size = FrameSize::of(frame);
        let extent = wgpu::Extent3d { width: size.width, height: size.height, depth_or_array_layers: 1 };
        if self.device.frame.as_ref().is_none_or(|held| held.size != size) {
            let texture = self.cx.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("wt.frame_view.frame"),
                size: extent,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Bgra8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let bind_group = self.cx.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("wt.frame_view.frame"),
                layout: &self.device.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.device.sampler) },
                ],
            });
            self.device.frame = Some(FrameTexture { texture, bind_group, size });
        }
        let Some(held) = self.device.frame.as_ref() else { return };

        self.cx.queue.write_texture(
            held.texture.as_image_copy(),
            frame.bytes(),
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(frame.bytes_per_row()), rows_per_image: None },
            extent,
        );
    }

    fn draw(&mut self) {
        let mut encoder =
            self.cx.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("wt.frame_view") });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("wt.frame_view"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: self.cx.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let placed =
                self.device.frame.as_ref().and_then(|held| fitted(held.size, self.cx.size).map(|rect| (rect, held)));
            if let Some((rect, held)) = placed {
                pass.set_pipeline(&self.device.pipeline);
                pass.set_bind_group(0, &held.bind_group, &[]);
                pass.set_viewport(rect.x, rect.y, rect.width, rect.height, 0.0, 1.0);
                pass.draw(0..3, 0..1);
            }
        }

        self.cx.queue.submit([encoder.finish()]);
    }
}

/// Where a frame is drawn inside the target, in device pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Fitted {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

/// The largest rectangle of `frame`'s shape that fits inside `target`, centred in it. `None` when
/// either has no area.
fn fitted(frame: FrameSize, target: Size<u32, Device>) -> Option<Fitted> {
    if frame.width == 0 || frame.height == 0 || target.width == 0 || target.height == 0 {
        return None;
    }

    let (frame_width, frame_height) = (frame.width as f32, frame.height as f32);
    let (target_width, target_height) = (target.width as f32, target.height as f32);
    let scale = (target_width / frame_width).min(target_height / frame_height);
    // Whole pixels, so a frame drawn at its own size is sampled texel for texel.
    let width = (frame_width * scale).round().max(1.0);
    let height = (frame_height * scale).round().max(1.0);
    let x = ((target_width - width) / 2.0).floor();
    let y = ((target_height - height) / 2.0).floor();

    Some(Fitted { x, y, width, height })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use image::Rgb;
    use image::RgbImage;
    use landaire_ui::HeadlessError;
    use landaire_ui::test_support::Drawing;
    use landaire_ui::test_support::HeadlessOptions;
    use landaire_ui::test_support::HeadlessWindow;
    use landaire_ui::test_support::WindowSize;
    use zgui::view;

    use super::*;

    /// A target that keeps what was written to it, in place of a device.
    #[derive(Default)]
    struct RecordingTarget {
        /// Each write, as the bytes written, or `None` for a clear.
        writes: Vec<Option<Vec<u8>>>,
        draws: usize,
        fresh: bool,
    }

    impl FrameTarget for RecordingTarget {
        fn is_fresh(&self) -> bool {
            self.fresh
        }

        fn write(&mut self, frame: Option<&BgraFrame>) {
            self.writes.push(frame.map(|frame| frame.bytes().to_vec()));
        }

        fn draw(&mut self) {
            self.draws += 1;
        }
    }

    /// A window's frame loop as zgui runs it for one renderer: the first frame renders, later
    /// ones only when the sink has asked.
    struct RecordingLoop {
        slot: FrameSlot,
        target: RecordingTarget,
        rendered_once: bool,
    }

    impl RecordingLoop {
        fn new(sink: &FrameSink) -> Self {
            Self { slot: FrameSlot::new(Arc::clone(&sink.0)), target: RecordingTarget::default(), rendered_once: false }
        }

        fn frame(&mut self) {
            let owed = self.slot.mailbox.latest().owed;
            if self.rendered_once && !owed {
                return;
            }
            self.rendered_once = true;
            self.slot.render_to(&mut self.target);
            self.target.fresh = false;
        }
    }

    fn solid(width: u32, height: u32, rgb: [u8; 3]) -> BgraFrame {
        BgraFrame::from_rgb(&RgbImage::from_pixel(width, height, Rgb(rgb)))
    }

    #[test]
    fn frames_pushed_from_another_thread_are_written_newest_only() {
        let sink = FrameSink::new();
        let mut frames = RecordingLoop::new(&sink);
        frames.frame();
        assert_eq!(frames.target.writes, [None], "the first render draws the empty sink");

        let producer = sink.clone();
        std::thread::spawn(move || {
            for shade in 1..=3 {
                producer.push(solid(4, 2, [shade, shade, shade]));
            }
        })
        .join()
        .expect("the producer thread finishes");
        frames.frame();

        assert_eq!(frames.target.writes.len(), 2, "three pushes between two frames write once");
        assert_eq!(frames.target.writes[1].as_deref(), Some(solid(4, 2, [3, 3, 3]).bytes()), "the newest is written");
        assert_eq!(sink.frame_size(), Some(FrameSize { width: 4, height: 2 }));
    }

    #[test]
    fn an_idle_sink_renders_nothing() {
        let sink = FrameSink::new();
        let mut frames = RecordingLoop::new(&sink);
        sink.push(solid(2, 2, [9, 9, 9]));
        frames.frame();
        let (writes, draws) = (frames.target.writes.len(), frames.target.draws);

        for _ in 0..10 {
            frames.frame();
        }

        assert_eq!(frames.target.writes.len(), writes, "no frame was pushed, so none is written");
        assert_eq!(frames.target.draws, draws, "and nothing asked the window to render");
    }

    /// zgui renders again without being asked after a resize; that redraws what is held.
    #[test]
    fn a_render_with_no_new_frame_draws_without_writing() {
        let sink = FrameSink::new();
        let mut slot = FrameSlot::new(Arc::clone(&sink.0));
        let mut target = RecordingTarget::default();
        sink.push(solid(2, 2, [1, 2, 3]));
        slot.render_to(&mut target);

        slot.render_to(&mut target);

        assert_eq!(target.writes.len(), 1);
        assert_eq!(target.draws, 2);
    }

    #[test]
    fn a_new_device_has_the_newest_frame_written_again() {
        let sink = FrameSink::new();
        let mut slot = FrameSlot::new(Arc::clone(&sink.0));
        let mut target = RecordingTarget::default();
        sink.push(solid(2, 2, [5, 5, 5]));
        slot.render_to(&mut target);

        target.fresh = true;
        slot.render_to(&mut target);

        assert_eq!(target.writes.len(), 2);
        assert_eq!(target.writes[1].as_deref(), Some(solid(2, 2, [5, 5, 5]).bytes()));
    }

    #[test]
    fn a_clear_is_written_as_no_frame() {
        let sink = FrameSink::new();
        let mut frames = RecordingLoop::new(&sink);
        sink.push(solid(2, 2, [5, 5, 5]));
        frames.frame();

        sink.clear();
        frames.frame();

        assert_eq!(frames.target.writes.last(), Some(&None));
        assert_eq!(sink.frame_size(), None);
    }

    #[test]
    fn a_frame_is_fitted_and_centred_without_stretching() {
        let target = |width, height| Size::<u32, Device>::new(width, height);

        assert_eq!(
            fitted(FrameSize { width: 10, height: 10 }, target(200, 100)),
            Some(Fitted { x: 50.0, y: 0.0, width: 100.0, height: 100.0 }),
            "a square in a wide box is pillarboxed"
        );
        assert_eq!(
            fitted(FrameSize { width: 40, height: 10 }, target(100, 100)),
            Some(Fitted { x: 0.0, y: 37.0, width: 100.0, height: 25.0 }),
            "a wide frame in a square box is letterboxed"
        );
        assert_eq!(fitted(FrameSize { width: 4, height: 4 }, target(0, 10)), None);
    }

    /// A frame's bytes are sRGB already; sampling them through an sRGB texture would decode them
    /// and the composed pixels would come out darker.
    #[test]
    fn a_frame_is_composed_at_its_own_colours() {
        const SHADE: [u8; 3] = [200, 120, 40];

        let sink = FrameSink::new();
        let shown = sink.clone();
        let options = HeadlessOptions {
            drawing: Drawing::Device,
            ..HeadlessOptions::new(WindowSize { width: 240.0, height: 120.0 })
        };
        let opened = HeadlessWindow::open(options, move || {
            let sink = shown.clone();
            view! {
                box(style:padding = "20px") {
                    box(style:width = "160px", style:height = "80px", attr:data-testid = "frame") {
                        FrameView(sink = sink)
                    }
                }
            }
        });
        let mut window = match opened {
            Ok(window) => window,
            Err(
                HeadlessError::NoWindow | HeadlessError::App { source: zgui::runtime::AppError::GpuUnavailable(_) },
            ) => {
                eprintln!("skipped: no graphics adapter to compose on");
                return;
            }
            Err(err) => panic!("the window opens: {err}"),
        };

        // A square frame in a two-by-one box: drawn in the middle half, nothing at the sides.
        std::thread::spawn(move || sink.push(solid(8, 8, SHADE))).join().expect("the producer thread finishes");
        window.advance(Duration::from_millis(50));

        let element = window.by_test_id("frame").expect("the frame view is in the document");
        let bounds = window.bounds(element).expect("the frame view has a box");
        let pixels = window.pixels().expect("the window is drawn on the device");
        let centre_x = (bounds.origin.x.0 + bounds.size.width.0 / 2.0) as i32;
        let centre_y = (bounds.origin.y.0 + bounds.size.height.0 / 2.0) as i32;
        let side_x = (bounds.origin.x.0 + 4.0) as i32;

        let centre = pixels.rgba(centre_x, centre_y);
        for (channel, (got, want)) in centre.iter().zip(SHADE).enumerate() {
            assert!(got.abs_diff(want) <= 2, "channel {channel} is composed as {got}, pushed as {want}: {centre:?}");
        }
        assert_ne!(pixels.rgba(side_x, centre_y)[..3], SHADE, "the frame keeps its shape inside the box");
    }
}
