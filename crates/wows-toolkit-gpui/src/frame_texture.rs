//! Frames rasterised on the CPU, shown through one mutable GPU texture.
//!
//! A view that redraws its picture on every tick hands each frame to a
//! [`FrameTexture`], which writes it into a dynamic texture rather than
//! building a new image. The texture lives outside the sprite atlas, so a
//! stream of frames never reallocates atlas tiles and leaves nothing for the
//! window to release.

use std::cell::Cell;
use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::AnyWindowHandle;
use gpui_kit::App;
use gpui_kit::AppContext as _;
use gpui_kit::Bounds;
use gpui_kit::Canvas;
use gpui_kit::DevicePixels;
use gpui_kit::DynamicTexture;
use gpui_kit::ObjectFit;
use gpui_kit::Pixels;
use gpui_kit::Size;
use gpui_kit::Window;
use gpui_kit::canvas;
use gpui_kit::point;
use wows_toolkit_replay_data::frame::BgraFrame;

/// Shows a stream of [`BgraFrame`]s through one dynamic texture.
///
/// The texture is created for the first frame and for each change of size,
/// and written in place otherwise. Frames are uploaded through the window the
/// texture was last painted in, outside a frame, so the window redraws the
/// views that painted it; one handed over before that window is known goes up
/// when it is next painted.
pub struct FrameTexture(Rc<RefCell<Uploads>>);

impl FrameTexture {
    pub fn new() -> Self {
        Self(Rc::new(RefCell::new(Uploads {
            texture: None,
            pending: None,
            window: None,
            creation_failure_logged: false,
        })))
    }

    /// The size of the frame on screen, or before any is, of the one waiting
    /// to go up. A pointer is read against it. `None` before the first frame,
    /// and after [`Self::clear`].
    pub fn size(&self) -> Option<Size<DevicePixels>> {
        let uploads = self.0.borrow();
        uploads.texture.as_ref().map(|allocated| allocated.size).or(uploads.pending.as_ref().map(device_size))
    }

    /// Shows `frame` in place of whatever was shown before.
    ///
    /// Returns whether the window was scheduled to redraw with it. When it
    /// was not, the frame goes up when the texture is next painted, and the
    /// owning view must be notified to paint it.
    ///
    /// A frame the platform refuses, while it still holds earlier uploads,
    /// leaves the previous contents on screen and is tried again when the
    /// texture is next painted, unless a newer frame replaces it first.
    #[must_use = "notify the owning view when the frame did not reach a window"]
    pub fn present(&self, frame: BgraFrame, cx: &mut App) -> bool {
        let window = {
            let mut uploads = self.0.borrow_mut();
            uploads.pending = Some(frame);
            uploads.window
        };
        let Some(window) = window else { return false };

        // Fails only while that window is itself being updated, or once it
        // has closed. The frame then waits for the next paint. Only a write
        // into the texture already painted schedules its painters; a new
        // texture has none yet.
        cx.update_window(window, |_, window, _| self.0.borrow_mut().flush(window))
            .is_ok_and(|upload| upload == Upload::Written)
    }

    /// Shows nothing and releases the texture.
    pub fn clear(&self) {
        let mut uploads = self.0.borrow_mut();
        uploads.pending = None;
        if let Some(allocated) = uploads.texture.take() {
            allocated.texture.remove();
        }
    }

    /// Takes `frame` as the one on screen without uploading it, for a test
    /// that reads pointer positions against a frame and has no painter.
    #[cfg(test)]
    pub fn hold_for_test(&self, frame: BgraFrame) {
        self.0.borrow_mut().pending = Some(frame);
    }
}

impl Default for FrameTexture {
    fn default() -> Self {
        Self::new()
    }
}

/// What a [`FrameTexture`] holds, shared with the element painting it.
struct Uploads {
    texture: Option<Allocated>,
    /// The newest frame not yet in the texture.
    pending: Option<BgraFrame>,
    window: Option<AnyWindowHandle>,
    /// Reported once: a platform without an upload backend refuses every
    /// frame.
    creation_failure_logged: bool,
}

impl Uploads {
    fn flush(&mut self, window: &mut Window) -> Upload {
        let Some(frame) = self.pending.take() else { return Upload::Idle };

        if let Some(allocated) = self.texture.as_ref().filter(|allocated| allocated.size == device_size(&frame)) {
            let bounds = Bounds { origin: point(DevicePixels(0), DevicePixels(0)), size: device_size(&frame) };
            return match window.update_dynamic_texture(allocated.texture, bounds, frame.bytes(), frame.bytes_per_row())
            {
                Ok(()) => Upload::Written,
                Err(err) => {
                    tracing::trace!("frame texture: upload refused: {err:#}");
                    self.pending = Some(frame);
                    Upload::Refused
                }
            };
        }

        match window.create_dynamic_texture(device_size(&frame), frame.bytes(), frame.bytes_per_row()) {
            Ok(texture) => {
                let allocated = Allocated { texture, size: device_size(&frame) };
                // Released only once its replacement exists, so a failed
                // creation leaves the previous frame on screen.
                if let Some(previous) = self.texture.replace(allocated) {
                    previous.texture.remove();
                }
                Upload::Created
            }
            Err(err) => {
                if std::mem::replace(&mut self.creation_failure_logged, true) {
                    tracing::debug!("frame texture: could not be created: {err:#}");
                } else {
                    tracing::warn!("frame texture: could not be created: {err:#}");
                }
                self.pending = Some(frame);
                Upload::Failed
            }
        }
    }
}

impl Drop for Uploads {
    fn drop(&mut self) {
        if let Some(allocated) = self.texture.take() {
            allocated.texture.remove();
        }
    }
}

/// A live texture and the size it was created at, which a frame of any other
/// size cannot be written into. GPUI keeps its contents across a device loss.
struct Allocated {
    texture: DynamicTexture,
    size: Size<DevicePixels>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Upload {
    Idle,
    /// Written into the texture already allocated.
    Written,
    /// Written into a newly allocated texture.
    Created,
    Refused,
    Failed,
}

/// Paints what `texture` holds, fitted inside the element without stretching.
///
/// `measured`, when given, is set to the element's bounds at layout, which is
/// what a view reads pointer positions against.
pub fn frame_view(texture: &FrameTexture, measured: Option<Rc<Cell<Option<Bounds<Pixels>>>>>) -> Canvas<()> {
    let uploads = Rc::clone(&texture.0);
    canvas(
        move |bounds, _window, _cx| {
            if let Some(measured) = measured {
                measured.set(Some(bounds));
            }
        },
        move |bounds, (), window, _cx| {
            let mut uploads = uploads.borrow_mut();
            uploads.window = Some(window.window_handle());

            // On wgpu an upload made while painting is queued ahead of this
            // frame's submission, so it shows now. A new texture changes the
            // size the view laid out against, so the view renders once more;
            // a refused upload is retried on the next frame. Nothing else
            // asks for either.
            if matches!(uploads.flush(window), Upload::Created | Upload::Refused) {
                window.request_animation_frame();
            }

            let Some(allocated) = uploads.texture.as_ref() else { return };
            let fitted = ObjectFit::Contain.get_bounds(bounds, allocated.size);
            if let Err(err) = window.paint_dynamic_texture(fitted, allocated.texture) {
                tracing::warn!("frame texture: could not be painted: {err:#}");
            }
        },
    )
}

/// What `frame` measures, in the units a dynamic texture takes.
fn device_size(frame: &BgraFrame) -> Size<DevicePixels> {
    Size { width: DevicePixels::from(frame.width()), height: DevicePixels::from(frame.height()) }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::collections::HashSet;
    use std::sync::Arc;
    use std::sync::LazyLock;
    use std::sync::Mutex;

    use gpui_kit::DynamicTextureId;
    use gpui_kit::DynamicTexturePlatform;
    use gpui_kit::TestAppContext;
    use image::RgbImage;

    use super::*;

    /// An upload platform that keeps each texture's bytes, and refuses updates
    /// to the textures a test names. Installed once for the test binary, so
    /// every test reads only the textures it created.
    #[derive(Default)]
    struct RecordingPlatform {
        textures: Mutex<HashMap<DynamicTextureId, Recorded>>,
        refusing: Mutex<HashSet<DynamicTextureId>>,
        /// Sizes whose creation fails.
        failing: Mutex<HashSet<Size<DevicePixels>>>,
    }

    struct Recorded {
        bytes: Vec<u8>,
        updates: usize,
        removed: bool,
    }

    impl DynamicTexturePlatform for RecordingPlatform {
        fn create_texture(&self, id: DynamicTextureId, size: Size<DevicePixels>, bytes: &[u8]) -> anyhow::Result<()> {
            anyhow::ensure!(!self.failing.lock().unwrap().contains(&size), "no room for the texture");
            self.textures.lock().unwrap().insert(id, Recorded { bytes: bytes.to_vec(), updates: 0, removed: false });
            Ok(())
        }

        fn update_texture(
            &self,
            id: DynamicTextureId,
            _bounds: Bounds<DevicePixels>,
            bytes: &[u8],
        ) -> anyhow::Result<()> {
            anyhow::ensure!(!self.refusing.lock().unwrap().contains(&id), "earlier uploads are still staged");
            let mut textures = self.textures.lock().unwrap();
            let recorded = textures.get_mut(&id).expect("updated after being created");
            recorded.bytes = bytes.to_vec();
            recorded.updates += 1;
            Ok(())
        }

        fn remove_texture(&self, id: DynamicTextureId) {
            if let Some(recorded) = self.textures.lock().unwrap().get_mut(&id) {
                recorded.removed = true;
            }
        }
    }

    fn platform() -> Arc<RecordingPlatform> {
        static PLATFORM: LazyLock<Arc<RecordingPlatform>> = LazyLock::new(|| {
            let platform = Arc::new(RecordingPlatform::default());
            gpui_kit::set_dynamic_texture_platform(Arc::clone(&platform) as Arc<dyn DynamicTexturePlatform>);
            platform
        });
        Arc::clone(&PLATFORM)
    }

    fn solid(width: u32, height: u32, shade: u8) -> BgraFrame {
        BgraFrame::from_rgb(&RgbImage::from_pixel(width, height, image::Rgb([shade, shade, shade])))
    }

    fn texture_id(texture: &FrameTexture) -> DynamicTextureId {
        texture.0.borrow().texture.as_ref().expect("a texture was created").texture.id()
    }

    struct Blank;

    impl gpui_kit::Render for Blank {
        fn render(&mut self, _window: &mut Window, _cx: &mut gpui_kit::Context<Self>) -> impl gpui_kit::IntoElement {
            gpui_kit::div()
        }
    }

    /// A frame of the texture's size is written in place, a frame of another
    /// size gets a new texture, and the old one is released.
    #[gpui_kit::test]
    fn a_new_size_recreates_the_texture_and_the_same_size_writes_in_place(cx: &mut TestAppContext) {
        let platform = platform();
        let window = cx.add_window(|_, _| Blank);
        let texture = FrameTexture::new();
        texture.0.borrow_mut().window = Some(window.into());

        assert!(!cx.update(|cx| texture.present(solid(4, 4, 1), cx)), "a new texture has no painter to redraw");
        let first = texture_id(&texture);
        assert!(cx.update(|cx| texture.present(solid(4, 4, 2), cx)), "a write redraws the texture's painters");
        assert_eq!(texture_id(&texture), first, "the same size is written into the same texture");
        assert_eq!(platform.textures.lock().unwrap()[&first].updates, 1);
        assert_eq!(platform.textures.lock().unwrap()[&first].bytes, solid(4, 4, 2).bytes());

        assert!(!cx.update(|cx| texture.present(solid(8, 2, 3), cx)));
        let second = texture_id(&texture);
        assert_ne!(second, first, "another size needs another texture");
        assert!(platform.textures.lock().unwrap()[&first].removed, "and the old one is released");
        assert_eq!(texture.size(), Some(Size { width: DevicePixels(8), height: DevicePixels(2) }));

        drop(texture);
        assert!(platform.textures.lock().unwrap()[&second].removed, "dropping the holder releases its texture");
    }

    /// A refused upload keeps what was on screen, and only the newest frame
    /// handed over since is written once the platform takes uploads again.
    #[gpui_kit::test]
    fn a_refused_upload_keeps_the_previous_frame_and_retries_the_newest(cx: &mut TestAppContext) {
        let platform = platform();
        let window = cx.add_window(|_, _| Blank);
        let texture = FrameTexture::new();
        texture.0.borrow_mut().window = Some(window.into());

        cx.update(|cx| texture.present(solid(4, 4, 1), cx));
        let id = texture_id(&texture);
        platform.refusing.lock().unwrap().insert(id);
        assert!(!cx.update(|cx| texture.present(solid(4, 4, 2), cx)), "a refused frame needs a paint to retry");
        assert!(!cx.update(|cx| texture.present(solid(4, 4, 3), cx)));
        assert_eq!(platform.textures.lock().unwrap()[&id].bytes, solid(4, 4, 1).bytes(), "the first frame stays up");

        platform.refusing.lock().unwrap().remove(&id);
        let flushed =
            window.update(cx, |_, window, _| texture.0.borrow_mut().flush(window)).expect("the window is open");
        assert_eq!(flushed, Upload::Written);
        let textures = platform.textures.lock().unwrap();
        assert_eq!(textures[&id].bytes, solid(4, 4, 3).bytes(), "the newest frame goes up");
        assert_eq!(textures[&id].updates, 1, "and the one it replaced never does");
    }

    /// A frame handed over before the texture has been painted waits for the
    /// window rather than being dropped.
    #[gpui_kit::test]
    fn a_frame_before_the_first_paint_waits_for_it(cx: &mut TestAppContext) {
        let platform = platform();
        let window = cx.add_window(|_, _| Blank);
        let texture = FrameTexture::new();

        assert!(!cx.update(|cx| texture.present(solid(2, 2, 7), cx)), "no window to redraw yet");
        assert!(texture.0.borrow().texture.is_none());
        assert_eq!(texture.size(), Some(Size { width: DevicePixels(2), height: DevicePixels(2) }));

        let flushed =
            window.update(cx, |_, window, _| texture.0.borrow_mut().flush(window)).expect("the window is open");
        assert_eq!(flushed, Upload::Created);
        assert_eq!(platform.textures.lock().unwrap()[&texture_id(&texture)].bytes, solid(2, 2, 7).bytes());
    }

    /// A frame whose texture cannot be created leaves the previous frame and
    /// its size on screen, and is tried again rather than dropped.
    #[gpui_kit::test]
    fn a_failed_creation_keeps_the_previous_frame_and_retries(cx: &mut TestAppContext) {
        let platform = platform();
        let window = cx.add_window(|_, _| Blank);
        let texture = FrameTexture::new();
        texture.0.borrow_mut().window = Some(window.into());
        let failing = Size { width: DevicePixels(3), height: DevicePixels(5) };
        platform.failing.lock().unwrap().insert(failing);

        assert!(!cx.update(|cx| texture.present(solid(6, 5, 1), cx)));
        let shown = texture_id(&texture);
        assert!(!cx.update(|cx| texture.present(solid(3, 5, 2), cx)));
        assert_eq!(texture_id(&texture), shown, "the previous texture stays up");
        assert_eq!(texture.size(), Some(Size { width: DevicePixels(6), height: DevicePixels(5) }));

        platform.failing.lock().unwrap().remove(&failing);
        let flushed =
            window.update(cx, |_, window, _| texture.0.borrow_mut().flush(window)).expect("the window is open");
        assert_eq!(flushed, Upload::Created);
        assert_eq!(texture.size(), Some(failing));
    }
}
