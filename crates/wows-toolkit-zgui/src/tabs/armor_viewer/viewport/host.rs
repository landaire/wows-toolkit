//! The device the viewports draw on, and the renderer zgui calls to draw one into its `surface`
//! element.
//!
//! zgui owns the texture: an `Rgba8Unorm` texture the size of the element, sampled as stored. The
//! scene is drawn through its `Rgba8UnormSrgb` view, so the hardware encodes the shader's linear
//! output and the texture holds sRGB bytes, which is what zgui composites.
//!
//! The pipeline is compiled once per device generation on a thread of its own, and shared by
//! every viewport through a thread-local registry. The renderers hold the device and the registry
//! only finds it: a renderer that sees a new generation lets go of the old device, which goes,
//! with every mesh buffer made on it, once the last renderer drawing on it has done the same. Each
//! viewport then finds its device gone, clears its scene and uploads it again on the next.

use std::cell::RefCell;
use std::panic::AssertUnwindSafe;
use std::rc::Rc;
use std::rc::Weak;
use std::sync::Arc;
use std::sync::Mutex;

use futures::channel::oneshot;
use image::ImageEncoder;
use wows_toolkit_armor::viewport::renderer::GpuPipeline;
use wows_toolkit_armor::viewport::renderer::SceneAttachments;
use wows_toolkit_armor::viewport::renderer::SceneColorTarget;
use wows_toolkit_armor::viewport::types::TargetSize;
use wows_toolkit_armor::viewport::types::ViewRect;
use zgui::surface::SurfaceInvalidator;
use zgui::surface::SurfaceRenderCx;
use zgui::surface::SurfaceRenderer;
use zgui::surface::wgpu;

use super::model::GpuState;
use super::model::SceneDevice;
use super::model::ViewportModel;
use super::state::ArmorViewportState;
use super::state::ViewCx;

/// The format of the texture zgui allocates for the viewport.
const TEXTURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// The view the scene is drawn through, so the hardware encodes it.
const VIEW_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// The target the pipeline renders for.
const COLOR_TARGET: SceneColorTarget = SceneColorTarget::Srgb(VIEW_FORMAT);

/// What the device registry knows of zgui's current device.
#[derive(Default)]
enum DeviceState {
    /// No viewport has asked yet.
    #[default]
    Unknown,
    /// The pipeline for `generation` is compiling on a thread of its own.
    Building { generation: u64, device: wgpu::Device, queue: wgpu::Queue, build: Arc<Build> },
    /// Weak: the renderers drawing on the device hold it, so a device no window draws on any more
    /// is not kept, nor taken for the device of a window opened after it.
    Ready(Weak<SceneDevice>),
    /// The pipeline for `generation` could not be compiled.
    Failed { generation: u64, reason: String },
}

/// A compile in flight: what it produced, and the surfaces to draw again once it has.
#[derive(Default)]
struct Build {
    outcome: Mutex<Option<Result<GpuPipeline, String>>>,
    waiters: Mutex<Vec<SurfaceInvalidator>>,
}

thread_local! {
    static DEVICE: RefCell<DeviceState> = RefCell::default();
}

/// What a viewport finds when it asks for the device.
pub(super) enum DeviceStatus {
    Ready(Rc<SceneDevice>),
    Waiting,
    Failed(String),
}

/// The scene device on `cx`'s device. The first ask of a generation starts compiling the pipeline
/// and answers `Waiting`; `waiter` is asked to draw again when the compile is done.
fn current_device(cx: &SurfaceRenderCx<'_>, waiter: &SurfaceInvalidator) -> DeviceStatus {
    DEVICE.with(|state| {
        let mut state = state.borrow_mut();
        let finished = match &*state {
            DeviceState::Ready(scene) => {
                if let Some(scene) =
                    scene.upgrade().filter(|scene| scene.is_generation(cx.generation) && scene.device() == cx.device)
                {
                    return DeviceStatus::Ready(scene);
                }
                None
            }
            DeviceState::Failed { generation, reason } if *generation == cx.generation => {
                return DeviceStatus::Failed(reason.clone());
            }
            DeviceState::Building { generation, device, queue, build } if *generation == cx.generation => {
                let outcome = build.outcome.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take();
                match outcome {
                    None => {
                        build.waiters.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(waiter.clone());
                        return DeviceStatus::Waiting;
                    }
                    Some(outcome) => Some((*generation, device.clone(), queue.clone(), outcome)),
                }
            }
            _ => None,
        };

        if let Some((generation, device, queue, outcome)) = finished {
            return match outcome {
                Ok(pipeline) => {
                    let scene = Rc::new(SceneDevice::new(device, queue, generation, pipeline));
                    *state = DeviceState::Ready(Rc::downgrade(&scene));
                    DeviceStatus::Ready(scene)
                }
                Err(reason) => {
                    tracing::error!("armor viewport cannot render: {reason}");
                    *state = DeviceState::Failed { generation, reason: reason.clone() };
                    DeviceStatus::Failed(reason)
                }
            };
        }

        // A new generation, or a device no renderer holds any more: compile for this one.
        *state = DeviceState::Unknown;
        let build = Arc::new(Build::default());
        build.waiters.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(waiter.clone());
        let (device, queue) = (cx.device.clone(), cx.queue.clone());
        let info = cx.adapter_info.clone();
        let generation = cx.generation;
        let thread_build = Arc::clone(&build);
        let (thread_device, thread_queue) = (device.clone(), queue.clone());
        let spawned = std::thread::Builder::new().name("armor-viewport-pipeline".to_owned()).spawn(move || {
            let compiled = std::panic::catch_unwind(AssertUnwindSafe(|| {
                GpuPipeline::new(&thread_device, &thread_queue, COLOR_TARGET)
            }))
            .map_err(|panic| {
                panic
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| panic.downcast_ref::<&str>().map(|reason| (*reason).to_owned()))
                    .unwrap_or_else(|| "the pipeline compile panicked".to_owned())
            });
            tracing::info!(
                "armor viewport renders on zgui's device: {} ({:?}, {:?}, driver {}), generation {generation}",
                info.name,
                info.device_type,
                info.backend,
                info.driver_info
            );
            *thread_build.outcome.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(compiled);
            let waiters =
                std::mem::take(&mut *thread_build.waiters.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
            for waiter in waiters {
                waiter.invalidate();
            }
        });
        if let Err(error) = spawned {
            let reason = error.to_string();
            *state = DeviceState::Failed { generation, reason: reason.clone() };
            return DeviceStatus::Failed(reason);
        }
        *state = DeviceState::Building { generation, device, queue, build };
        DeviceStatus::Waiting
    })
}

/// Why a screenshot could not be taken.
#[derive(Debug, thiserror::Error)]
pub enum ScreenshotError {
    #[error("the viewport has no scene drawn to take")]
    NothingDrawn,
    #[error("the viewport was closed before it drew again")]
    Closed,
    #[error("the frame could not be read back: {0}")]
    Readback(String),
    #[error("the frame could not be encoded as PNG: {0}")]
    Encode(#[from] image::ImageError),
}

/// A screenshot asked for, answered by the next frame drawn.
pub(super) type ScreenshotReply = oneshot::Sender<Result<Vec<u8>, ScreenshotError>>;

/// The attachments one viewport renders with, and the device and size they were made for.
struct Attachments {
    device: Weak<SceneDevice>,
    attachments: SceneAttachments,
}

/// What zgui calls to fill one viewport's `surface` element.
pub(super) struct ViewportRenderer {
    pub(super) state: ArmorViewportState,
    /// The device this viewport draws on, held for it: the viewport's model holds it weakly.
    device: Option<Rc<SceneDevice>>,
    attachments: Option<Attachments>,
}

impl ViewportRenderer {
    pub(super) fn new(state: ArmorViewportState) -> Self {
        Self { state, device: None, attachments: None }
    }
}

impl SurfaceRenderer for ViewportRenderer {
    fn render(&mut self, cx: &mut SurfaceRenderCx<'_>) {
        let Some(invalidator) = self.state.invalidator() else { return };
        let status = current_device(cx, &invalidator);
        self.device = match &status {
            DeviceStatus::Ready(scene) => Some(Rc::clone(scene)),
            DeviceStatus::Waiting | DeviceStatus::Failed(_) => None,
        };
        let scale = if cx.scale.is_finite() && cx.scale > 0.0 { cx.scale } else { 1.0 };
        let bounds = ViewRect::new(0.0, 0.0, cx.size.width as f32 / scale, cx.size.height as f32 / scale);
        let timestamp = cx.timestamp;
        let state = self.state;
        let frame = state.rendering(|| {
            state.update(|model, view_cx| {
                model.place(bounds, view_cx);
                model.sync_gpu(status, view_cx);
                let animate = model.advance_frame(timestamp, view_cx);
                model.rescale_hardpoints();
                model.rewrite_length_marks();
                (animate, model.scene_is_shown())
            })
        });
        let Some((animate, shown)) = frame else { return };

        let drawn = state.rendering(|| self.draw(cx, shown));
        let screenshots = self.state.take_screenshots();
        if !screenshots.is_empty() {
            match drawn {
                Some(()) => read_back(cx, screenshots),
                None => {
                    for reply in screenshots {
                        let _ = reply.send(Err(ScreenshotError::NothingDrawn));
                    }
                }
            }
        }
        if animate {
            // The overlay draws the rings and the gizmo from the camera of this frame.
            self.state.repaint_overlay();
            cx.request_animation_frame();
        }
    }

    fn format(&self) -> wgpu::TextureFormat {
        TEXTURE_FORMAT
    }

    fn view_formats(&self) -> &[wgpu::TextureFormat] {
        &[VIEW_FORMAT]
    }

    fn usage(&self) -> wgpu::TextureUsages {
        wgpu::TextureUsages::COPY_SRC
    }
}

impl ViewportRenderer {
    /// Draws the scene, or clears the texture when there is none to show, so the element's
    /// background shows through. `None` when nothing of a scene was drawn.
    fn draw(&mut self, cx: &mut SurfaceRenderCx<'_>, shown: bool) -> Option<()> {
        let view = cx.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(VIEW_FORMAT),
            ..wgpu::TextureViewDescriptor::default()
        });
        let scene = self.state.read_untracked(ViewportModel::scene).flatten().filter(|_| shown);
        let size = TargetSize::new(cx.size.width, cx.size.height);
        let (Some(scene), Some(size)) = (scene, size) else {
            clear(cx, &view);
            return None;
        };

        let stale = self.attachments.as_ref().is_none_or(|held| {
            !std::ptr::eq(held.device.as_ptr(), Rc::as_ptr(&scene)) || held.attachments.size() != size
        });
        if stale {
            // Released before the replacement is allocated.
            self.attachments = None;
            self.attachments =
                Some(Attachments { device: Rc::downgrade(&scene), attachments: scene.attachments(size) });
        }
        let attachments = &self.attachments.as_ref()?.attachments;
        self.state.update(|model, _| {
            scene.render(&model.viewport, attachments, &view);
            model.viewport.clear_dirty();
        })
    }
}

/// Clears the texture to nothing.
fn clear(cx: &SurfaceRenderCx<'_>, view: &wgpu::TextureView) {
    let mut encoder =
        cx.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("armor_viewport_clear") });
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("armor_viewport_clear"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    cx.queue.submit(std::iter::once(encoder.finish()));
}

/// Copies the frame just drawn to a buffer and answers `replies` with it as PNG, mapping and
/// encoding off the UI thread.
fn read_back(cx: &SurfaceRenderCx<'_>, replies: Vec<ScreenshotReply>) {
    let (width, height) = (cx.size.width, cx.size.height);
    let row_bytes = width * 4;
    let padded_row_bytes = row_bytes.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = cx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("armor_viewport_screenshot"),
        size: u64::from(padded_row_bytes) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder =
        cx.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("armor_viewport_screenshot") });
    encoder.copy_texture_to_buffer(
        cx.texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_row_bytes),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
    );
    cx.queue.submit(std::iter::once(encoder.finish()));

    let device = cx.device.clone();
    let encoded = move || -> Result<Vec<u8>, ScreenshotError> {
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|error| ScreenshotError::Readback(error.to_string()))?;
        let mapped = slice.get_mapped_range().map_err(|error| ScreenshotError::Readback(error.to_string()))?;
        let mut rgba = Vec::with_capacity((row_bytes * height) as usize);
        for row in mapped.chunks_exact(padded_row_bytes as usize) {
            rgba.extend_from_slice(&row[..row_bytes as usize]);
        }
        drop(mapped);
        encode_png(width, height, rgba)
    };
    let spawned = std::thread::Builder::new().name("armor-viewport-screenshot".to_owned()).spawn(move || {
        let png = encoded();
        let mut replies = replies.into_iter();
        let last = replies.next_back();
        for reply in replies {
            let copy = match &png {
                Ok(bytes) => Ok(bytes.clone()),
                Err(error) => Err(ScreenshotError::Readback(error.to_string())),
            };
            let _ = reply.send(copy);
        }
        if let Some(reply) = last {
            let _ = reply.send(png);
        }
    });
    if let Err(error) = spawned {
        tracing::warn!("armor viewport: no thread to read a screenshot back on: {error}");
    }
}

/// `rgba`, tightly packed rows of sRGB pixels, as a PNG.
pub(super) fn encode_png(width: u32, height: u32, rgba: Vec<u8>) -> Result<Vec<u8>, ScreenshotError> {
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png).write_image(&rgba, width, height, image::ExtendedColorType::Rgba8)?;
    Ok(png)
}

impl ViewportModel {
    /// Takes the viewport's size from the frame being drawn.
    pub(super) fn place(&mut self, bounds: ViewRect, cx: &mut ViewCx) {
        if self.last_bounds == Some(bounds) {
            return;
        }
        let first = self.last_bounds.is_none();
        self.last_bounds = Some(bounds);
        // The markers' least on-screen size is held in scene units, so a resize that moves it far
        // enough builds them again; the first layout gives the floor its first value.
        if first {
            self.upload_hardpoints();
        }
        self.rescale_hardpoints();
        cx.notify();
    }

    /// Builds the hardpoint markers again when the camera or the view moved their least on-screen
    /// size far from the one they were built for.
    pub(super) fn rescale_hardpoints(&mut self) {
        let Some(bounds) = self.last_bounds else { return };
        if self.hardpoint_mesh.is_none() {
            return;
        }
        let world_per_pixel = self.viewport.camera.world_per_pixel(bounds.h);
        let ratio = world_per_pixel / self.hardpoint_world_per_pixel.max(f32::MIN_POSITIVE);
        if !(0.8..=1.25).contains(&ratio) {
            self.build_hardpoints(world_per_pixel);
        }
    }

    /// Whether the scene itself is what the viewport shows, rather than a load, a problem, an
    /// empty pane or the device being prepared.
    pub(super) fn scene_is_shown(&self) -> bool {
        self.ship_loading.is_none()
            && self.scene_problem.is_none()
            && self.current_armor.is_some()
            && matches!(self.gpu, GpuState::Ready(_))
    }

    /// Follows zgui's device. On a new device, uploads whatever was handed over while there was
    /// none, or uploads the current scene again: meshes made on an earlier device went with it.
    pub(super) fn sync_gpu(&mut self, status: DeviceStatus, cx: &mut ViewCx) {
        if let (DeviceStatus::Ready(scene), Some(held)) = (&status, self.scene())
            && Rc::ptr_eq(scene, &held)
        {
            return;
        }

        if matches!(self.gpu, GpuState::Ready(_)) {
            self.viewport.clear();
        }
        let was_waiting = matches!(self.gpu, GpuState::Waiting);
        self.gpu = match status {
            DeviceStatus::Ready(scene) => {
                scene.track(&self.viewport);
                GpuState::Ready(Rc::downgrade(&scene))
            }
            DeviceStatus::Waiting => GpuState::Waiting,
            DeviceStatus::Failed(reason) => GpuState::Failed(reason),
        };
        let ready = matches!(self.gpu, GpuState::Ready(_));
        if !(was_waiting && matches!(self.gpu, GpuState::Waiting)) {
            cx.notify();
        }
        if !ready {
            return;
        }

        // The textures were made on the device being replaced.
        self.hull_textures.clear();
        match self.pending_armor.take() {
            Some((armor, layer)) => self.upload_armor_now(armor, layer, cx),
            None => self.reupload_current_armor(cx),
        }
        // A comparison asked for while there was no device found no hull, and did nothing.
        if let Some(pending) = self.pending_comparison.take() {
            self.compare_incoming(&pending.taken, &pending.shells, cx);
        }
    }
}

impl ArmorViewportState {
    /// The scene as it is drawn now, as a PNG the size of the viewport in device pixels. Answered
    /// by the next frame, which this asks for.
    pub fn screenshot(self) -> impl Future<Output = Result<Vec<u8>, ScreenshotError>> {
        let (send, receive) = oneshot::channel();
        let asked = self.ask_screenshot(send);
        async move {
            if !asked {
                return Err(ScreenshotError::Closed);
            }
            receive.await.unwrap_or(Err(ScreenshotError::Closed))
        }
    }

    /// Puts the camera back where the scene opened.
    pub fn reset_camera(self) {
        self.update(|model, cx| {
            let Some(home) = model.home_camera.clone() else { return };
            model.viewport.camera = home;
            model.viewport.mark_dirty();
            cx.emit(super::types::ViewportChange::Camera);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::encode_png;

    #[test]
    fn a_frame_encodes_as_a_png_of_its_size() {
        let rgba = vec![255_u8; 4 * 3 * 2];
        let png = encode_png(3, 2, rgba).expect("a small frame encodes");
        let decoded = image::load_from_memory(&png).expect("the PNG decodes");
        assert_eq!((decoded.width(), decoded.height()), (3, 2));
    }
}
