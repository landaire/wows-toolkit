//! The armor viewport's use of GPUI's own wgpu device.
//!
//! The viewport draws on the device GPUI draws the window with, into a dynamic
//! texture GPUI samples in its own pass, so a frame never leaves the GPU. GPUI
//! publishes its context once the first window exists and replaces it after
//! device loss. [`SharedSceneDevice`] follows those generations. Everything a
//! pane made on a device (pipeline, targets, mesh buffers) is owned through
//! its [`SceneDevice`], which is dropped as soon as GPUI retires the device;
//! the viewports upload again on the next one when they render.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::rc::Weak;
use std::sync::Once;

use gpui_kit::App;
use gpui_kit::AppContext as _;
use gpui_kit::Bounds;
use gpui_kit::DevicePixels;
use gpui_kit::EntityId;
use gpui_kit::Pixels;
use gpui_kit::Window;
use gpui_kit::size;
use gpui_wgpu::WgpuAppContext;
use gpui_wgpu::WgpuContext;
use gpui_wgpu::WgpuContextGeneration;
use gpui_wgpu::WgpuDynamicTexture;
use gpui_wgpu::wgpu;
use wows_toolkit_armor::viewport::device::ViewportDevice;
use wows_toolkit_armor::viewport::renderer::GpuPipeline;
use wows_toolkit_armor::viewport::renderer::SceneAttachments;
use wows_toolkit_armor::viewport::renderer::SceneColorTarget;
use wows_toolkit_armor::viewport::renderer::Viewport3D;
use wows_toolkit_armor::viewport::types::TargetSize;

#[derive(Debug, thiserror::Error)]
pub enum SceneDeviceError {
    /// The platform draws this window without wgpu, so there is no device to
    /// share.
    #[error("the window is not drawn through wgpu")]
    NoWgpuContext,
    #[error("GPUI could not allocate a {width}x{height} viewport texture: {reason:#}")]
    Texture { width: u32, height: u32, reason: anyhow::Error },
    #[error("the viewport frame could not be drawn: {reason:#}")]
    Render { reason: anyhow::Error },
    #[error("the viewport texture could not be painted: {reason:#}")]
    Paint { reason: anyhow::Error },
}

/// GPUI's device with the pipeline the viewport draws with on it. The pane's
/// [`SharedSceneDevice`] holds the only lasting reference; viewports hold
/// weak ones, so dropping it there releases everything below.
pub struct SceneDevice {
    context: WgpuContext,
    /// The pipeline, and the mesh buffers of the viewports drawing on it.
    scene: ViewportDevice<WgpuContextGeneration>,
    /// Each viewport's render target.
    targets: RefCell<HashMap<EntityId, SceneTarget>>,
}

/// A [`SceneDevice`] compiled on a background thread, before it takes on the
/// UI thread's bookkeeping.
struct Compiled {
    context: WgpuContext,
    generation: WgpuContextGeneration,
    pipeline: GpuPipeline,
}

impl SceneDevice {
    /// The color target the viewport renders for `context`. GPUI picks the
    /// texture format per context, and whether the device can render it
    /// through an sRGB view; a one-pixel texture reports both.
    fn color_target(context: &WgpuContext) -> Result<SceneColorTarget, SceneDeviceError> {
        let probe = context
            .create_dynamic_texture(size(DevicePixels(1), DevicePixels(1)))
            .map_err(|reason| SceneDeviceError::Texture { width: 1, height: 1, reason })?;
        Ok(match probe.srgb_view_format() {
            Some(format) => SceneColorTarget::Srgb(format),
            None => SceneColorTarget::ShaderEncoded(probe.format()),
        })
    }

    /// Compiles the pipeline on `context`'s device. Slow enough to keep off
    /// the UI thread.
    fn build(context: WgpuContext, generation: WgpuContextGeneration, color: SceneColorTarget) -> Compiled {
        let pipeline = GpuPipeline::new(&context.device, &context.queue, color);
        let info = context.adapter.get_info();
        tracing::info!(
            "armor viewport renders on GPUI's device: {} ({:?}, {:?}, driver {}), {generation:?}, {color:?}",
            info.name,
            info.device_type,
            info.backend,
            info.driver_info
        );

        Compiled { context, generation, pipeline }
    }

    fn new(compiled: Compiled) -> Self {
        let Compiled { context, generation, pipeline } = compiled;
        let scene = ViewportDevice::new((*context.device).clone(), (*context.queue).clone(), generation, pipeline);
        Self { context, scene, targets: RefCell::default() }
    }

    pub fn device(&self) -> &wgpu::Device {
        self.scene.device()
    }

    pub fn queue(&self) -> &wgpu::Queue {
        self.scene.queue()
    }

    pub fn pipeline(&self) -> &GpuPipeline {
        self.scene.pipeline()
    }

    fn generation(&self) -> WgpuContextGeneration {
        self.scene.generation()
    }

    fn is_lost(&self) -> bool {
        self.context.device_lost()
    }

    /// Whether this is GPUI's current device and it is not lost.
    pub fn is_current(&self) -> bool {
        !self.is_lost() && gpui_wgpu::app_context().is_some_and(|app| self.scene.is_generation(app.generation))
    }

    /// Releases `viewport`'s mesh buffers along with this device.
    pub fn track(&self, viewport: &Viewport3D) {
        self.scene.track(viewport);
    }

    /// Paints `view`'s scene at `bounds`, `size` device pixels, drawing
    /// `viewport` into it first when `redraw` is set or the target is new.
    /// Returns whether it drew. Call during paint, so the frame being drawn
    /// samples the new contents.
    pub fn paint_view(
        &self,
        view: EntityId,
        viewport: &Viewport3D,
        redraw: bool,
        size: TargetSize,
        window: &mut Window,
        bounds: Bounds<Pixels>,
    ) -> Result<bool, SceneDeviceError> {
        let mut targets = self.targets.borrow_mut();
        let mut redraw = redraw;
        if targets.get(&view).is_none_or(|target| target.size() != size) {
            // Released before the replacement is allocated.
            targets.remove(&view);
            targets.insert(view, self.create_target(size)?);
            redraw = true;
        }
        let target = &targets[&view];

        let drew = redraw
            && match target.render(self, viewport, window) {
                Ok(()) => true,
                Err(error) => {
                    tracing::warn!("armor viewport: {error}");
                    false
                }
            };
        target.paint(window, bounds)?;
        Ok(drew)
    }

    /// Releases `view`'s target.
    pub fn forget_view(&self, view: EntityId) {
        self.targets.borrow_mut().remove(&view);
    }

    fn create_target(&self, size: TargetSize) -> Result<SceneTarget, SceneDeviceError> {
        let texture_size = gpui_kit::size(DevicePixels(size.width() as i32), DevicePixels(size.height() as i32));
        let texture = self
            .context
            .create_dynamic_texture(texture_size)
            .map_err(|reason| SceneDeviceError::Texture { width: size.width(), height: size.height(), reason })?;
        let attachments = self.scene.attachments(size);

        Ok(SceneTarget { texture, attachments })
    }
}

/// One viewport's render target: the texture GPUI samples and the attachments
/// that resolve into it.
struct SceneTarget {
    texture: WgpuDynamicTexture,
    attachments: SceneAttachments,
}

impl SceneTarget {
    fn size(&self) -> TargetSize {
        self.attachments.size()
    }

    /// Draws `viewport` into the texture. The work is submitted before GPUI's
    /// own, so the frame GPUI draws next samples it.
    fn render(&self, scene: &SceneDevice, viewport: &Viewport3D, window: &mut Window) -> Result<(), SceneDeviceError> {
        let draw = |resolve_target: &wgpu::TextureView| {
            scene.scene.render(viewport, &self.attachments, resolve_target);
            Ok(())
        };
        let drawn = match scene.pipeline().color() {
            SceneColorTarget::Srgb(_) => self.texture.render_srgb(window, draw),
            SceneColorTarget::ShaderEncoded(_) => self.texture.render(window, draw),
        };

        drawn.map_err(|reason| SceneDeviceError::Render { reason })
    }

    /// Paints the texture at `bounds`. Call during paint.
    fn paint(&self, window: &mut Window, bounds: Bounds<Pixels>) -> Result<(), SceneDeviceError> {
        self.texture.paint(window, bounds).map_err(|reason| SceneDeviceError::Paint { reason })
    }
}

/// What a pane's viewports last learned of GPUI's device.
#[derive(Default)]
enum SharedState {
    /// No viewport has asked yet.
    #[default]
    Unknown,
    /// The pipeline for this generation is compiling on a background thread.
    Building {
        generation: WgpuContextGeneration,
        /// The views that asked meanwhile, notified once it is done.
        waiters: Vec<EntityId>,
    },
    Ready(Rc<SceneDevice>),
    /// This generation's device is lost or released; GPUI has not published
    /// a replacement yet.
    Lost(WgpuContextGeneration),
    Failed {
        generation: Option<WgpuContextGeneration>,
        error: Rc<SceneDeviceError>,
    },
}

thread_local! {
    /// Every pane's device state on the UI thread, which is where GPUI
    /// reports a retired device.
    static PANES: RefCell<Vec<Weak<RefCell<SharedState>>>> = const { RefCell::new(Vec::new()) };
}

/// Drops each pane's device of `generation`, and with it every target and
/// mesh buffer made on it, so GPUI can free the device before it makes the
/// next one.
fn retire(generation: WgpuContextGeneration) {
    let panes: Vec<_> = PANES.with(|panes| panes.borrow().iter().filter_map(Weak::upgrade).collect());
    for pane in panes {
        // Busy only if GPUI reports the loss from inside a viewport's own
        // call, which then sees the device lost on its next render.
        let Ok(mut state) = pane.try_borrow_mut() else { continue };
        if matches!(&*state, SharedState::Ready(scene) if scene.generation() == generation) {
            *state = SharedState::Lost(generation);
        }
    }
}

/// GPUI's current device as every viewport of one pane shares it, so the
/// viewports share one pipeline too.
#[derive(Clone)]
pub struct SharedSceneDevice(Rc<RefCell<SharedState>>);

impl Default for SharedSceneDevice {
    fn default() -> Self {
        static HOOK: Once = Once::new();
        HOOK.call_once(|| {
            gpui_wgpu::on_device_lost(|retired| {
                // Off the UI thread there are no panes to visit.
                let _ = PANES.try_with(|_| retire(retired.generation));
            });
        });

        let state = Rc::new(RefCell::new(SharedState::Unknown));
        PANES.with(|panes| {
            let mut panes = panes.borrow_mut();
            panes.retain(|pane| pane.strong_count() > 0);
            panes.push(Rc::downgrade(&state));
        });
        Self(state)
    }
}

impl SharedSceneDevice {
    /// The scene device on GPUI's current context. The first ask of each
    /// generation starts building it in the background and returns
    /// `Ok(None)`; `waiter` is notified when it is ready. `Ok(None)` also
    /// while that context's device is lost and GPUI has not replaced it yet.
    pub fn current(&self, waiter: EntityId, cx: &mut App) -> Result<Option<Rc<SceneDevice>>, Rc<SceneDeviceError>> {
        let app = gpui_wgpu::app_context();
        let generation = app.as_ref().map(|app| app.generation);
        let mut state = self.0.borrow_mut();
        match &mut *state {
            SharedState::Ready(scene) if Some(scene.generation()) == generation && !scene.is_lost() => {
                return Ok(Some(Rc::clone(scene)));
            }
            SharedState::Building { generation: building, waiters } if Some(*building) == generation => {
                if !waiters.contains(&waiter) {
                    waiters.push(waiter);
                }
                return Ok(None);
            }
            SharedState::Lost(lost) if generation.is_none_or(|generation| generation == *lost) => return Ok(None),
            SharedState::Failed { generation: failed, error } if *failed == generation => return Err(Rc::clone(error)),
            _ => {}
        }

        // Released before anything is built on the replacement device. Views
        // waiting on an earlier generation's build wait on this one instead.
        let mut waiters = match std::mem::take(&mut *state) {
            SharedState::Building { waiters, .. } => waiters,
            _ => Vec::new(),
        };
        if !waiters.contains(&waiter) {
            waiters.push(waiter);
        }
        let failed = |state: &mut SharedState, generation, error: SceneDeviceError, cx: &mut App| {
            let error = Rc::new(error);
            *state = SharedState::Failed { generation, error: Rc::clone(&error) };
            // Views waiting on an earlier build show the failure too.
            for other in waiters.iter().filter(|other| **other != waiter) {
                cx.notify(*other);
            }
            error
        };
        let Some(WgpuAppContext { context, generation }) = app else {
            return Err(failed(&mut state, None, SceneDeviceError::NoWgpuContext, cx));
        };
        if context.device_lost() {
            *state = SharedState::Lost(generation);
            return Ok(None);
        }
        let color = match SceneDevice::color_target(&context) {
            Ok(color) => color,
            Err(error) => {
                tracing::error!("armor viewport cannot render: {error}");
                return Err(failed(&mut state, Some(generation), error, cx));
            }
        };

        *state = SharedState::Building { generation, waiters };
        drop(state);
        let built = cx.background_spawn(async move { SceneDevice::build(context, generation, color) });
        let shared = self.clone();
        cx.spawn(async move |cx| {
            let built = built.await;
            cx.update(|cx| shared.finish_build(built, cx));
        })
        .detach();
        Ok(None)
    }

    /// Takes a device built in the background, unless a later generation's
    /// build has replaced the one it was for, and tells the waiting views.
    fn finish_build(&self, built: Compiled, cx: &mut App) {
        let waiters = {
            let mut state = self.0.borrow_mut();
            let SharedState::Building { generation, waiters } = &mut *state else { return };
            if *generation != built.generation {
                return;
            }
            let waiters = std::mem::take(waiters);
            *state = SharedState::Ready(Rc::new(SceneDevice::new(built)));
            waiters
        };
        for waiter in waiters {
            cx.notify(waiter);
        }
    }

    /// The scene device a viewport already built, without building one.
    pub fn existing(&self) -> Option<Rc<SceneDevice>> {
        match &*self.0.borrow() {
            SharedState::Ready(scene) if scene.is_current() => Some(Rc::clone(scene)),
            _ => None,
        }
    }
}
