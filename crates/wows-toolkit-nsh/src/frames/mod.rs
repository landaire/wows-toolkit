//! CPU-rendered frames shown by the `wows.frame` native component.
//!
//! A producer (replay renderer, tactics board, minimap preview, armor
//! viewport) owns a [`Surface`] named by a [`SurfaceId`]. It presents RGBA
//! frames sized to the surface and reads the input the component forwards.
//! The TSX side places `<Frame surface="..."/>`; the component binds to the
//! surface of that name, so the producer and the view never share a handle.

#[cfg(target_os = "linux")]
pub mod native;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::Mutex;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use tokio::sync::mpsc;

/// Name of a frame surface, shared by its producer and its `<Frame>` view.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SurfaceId(Arc<str>);

impl SurfaceId {
    pub fn new(name: impl Into<Arc<str>>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A size in device pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelSize {
    pub width: u32,
    pub height: u32,
}

impl PixelSize {
    pub fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }
}

/// A position in device pixels, relative to the surface's top-left corner.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PixelPoint {
    pub x: f32,
    pub y: f32,
}

/// How the alpha channel of a frame's pixels is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlphaMode {
    /// tiny-skia pixmaps.
    Premultiplied,
    /// GPU readbacks and decoded images.
    Straight,
}

/// One frame: tightly packed RGBA8 rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub size: PixelSize,
    pub alpha: AlphaMode,
    pub pixels: Vec<u8>,
}

/// Errors from building a [`Frame`].
#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("frame of {width}x{height} needs {expected} bytes, got {actual}")]
    PixelCount { width: u32, height: u32, expected: usize, actual: usize },
}

impl Frame {
    pub fn new(size: PixelSize, alpha: AlphaMode, pixels: Vec<u8>) -> Result<Self, FrameError> {
        let expected = size.width as usize * size.height as usize * 4;
        if pixels.len() != expected {
            return Err(FrameError::PixelCount {
                width: size.width,
                height: size.height,
                expected,
                actual: pixels.len(),
            });
        }
        Ok(Self { size, alpha, pixels })
    }
}

/// Modifier keys held during an input event.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub meta: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerButton {
    Primary,
    Middle,
    Secondary,
    Other(u32),
}

/// Input the component forwards to the surface's producer.
#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceInput {
    /// The view's size changed; `scale` is device pixels per point.
    Resized { size: PixelSize, scale: f32 },
    PointerDown { at: PixelPoint, button: PointerButton, clicks: u32, modifiers: Modifiers },
    PointerUp { at: PixelPoint, button: PointerButton, modifiers: Modifiers },
    PointerMoved { at: PixelPoint, modifiers: Modifiers },
    PointerLeft,
    /// Wheel or touchpad scroll, in scroll steps (positive `dy`: down).
    Wheel { at: Option<PixelPoint>, dx: f32, dy: f32, modifiers: Modifiers },
    /// Touchpad pinch; `scale` is relative to the gesture's start.
    Pinch { scale: f32 },
    /// A key went down or up while the view had focus. `key` is the GDK key
    /// name (`"w"`, `"Left"`, `"space"`).
    Key { key: String, pressed: bool, modifiers: Modifiers },
    Focus { focused: bool },
}

/// A producer's frames and the input of the view showing them.
pub struct Surface {
    id: SurfaceId,
    latest: Mutex<Option<Arc<Frame>>>,
    generation: AtomicU64,
    view_size: Mutex<Option<(PixelSize, f32)>>,
    input: mpsc::UnboundedSender<SurfaceInput>,
    receiver: Mutex<Option<mpsc::UnboundedReceiver<SurfaceInput>>>,
}

impl Surface {
    fn new(id: SurfaceId) -> Self {
        let (input, receiver) = mpsc::unbounded_channel();
        Self {
            id,
            latest: Mutex::new(None),
            generation: AtomicU64::new(0),
            view_size: Mutex::new(None),
            input,
            receiver: Mutex::new(Some(receiver)),
        }
    }

    pub fn id(&self) -> &SurfaceId {
        &self.id
    }

    /// Replaces the shown frame. Frames not yet shown are dropped.
    pub fn present(&self, frame: Frame) {
        *self.latest.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(frame));
        self.generation.fetch_add(1, Ordering::Release);
    }

    /// Clears the view.
    pub fn clear(&self) {
        *self.latest.lock().unwrap_or_else(|e| e.into_inner()) = None;
        self.generation.fetch_add(1, Ordering::Release);
    }

    /// The input stream. Only one producer reads it; later calls get `None`.
    pub fn take_input(&self) -> Option<mpsc::UnboundedReceiver<SurfaceInput>> {
        self.receiver.lock().unwrap_or_else(|e| e.into_inner()).take()
    }

    /// The size of the view last reported, if one is shown.
    pub fn view_size(&self) -> Option<(PixelSize, f32)> {
        *self.view_size.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Sends input as the view would; also used by headless tests, where
    /// native components do not exist.
    pub fn send_input(&self, input: SurfaceInput) {
        if let SurfaceInput::Resized { size, scale } = &input {
            *self.view_size.lock().unwrap_or_else(|e| e.into_inner()) = Some((*size, *scale));
        }
        // The producer may have stopped; input for it is then irrelevant.
        let _ = self.input.send(input);
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// The latest frame, for the view and for tests.
    pub fn latest(&self) -> Option<Arc<Frame>> {
        self.latest.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

static SURFACES: LazyLock<Mutex<HashMap<SurfaceId, Arc<Surface>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// The surface named `id`, made on first use.
pub fn surface(id: &SurfaceId) -> Arc<Surface> {
    let mut surfaces = SURFACES.lock().unwrap_or_else(|e| e.into_inner());
    Arc::clone(surfaces.entry(id.clone()).or_insert_with(|| Arc::new(Surface::new(id.clone()))))
}

/// Forgets the surface named `id`. A view still bound to it keeps its last
/// frame; a later [`surface`] call makes a new one with a fresh input stream.
pub fn release(id: &SurfaceId) {
    SURFACES.lock().unwrap_or_else(|e| e.into_inner()).remove(id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_rejects_wrong_pixel_count() {
        let size = PixelSize { width: 2, height: 2 };
        assert!(Frame::new(size, AlphaMode::Straight, vec![0; 15]).is_err());
        assert!(Frame::new(size, AlphaMode::Straight, vec![0; 16]).is_ok());
    }

    #[test]
    fn input_reaches_the_single_reader_and_records_size() {
        let id = SurfaceId::new("test-input");
        let surface = surface(&id);
        let mut input = surface.take_input().expect("first reader gets the stream");
        assert!(surface.take_input().is_none());
        let size = PixelSize { width: 30, height: 20 };
        surface.send_input(SurfaceInput::Resized { size, scale: 2.0 });
        assert_eq!(surface.view_size(), Some((size, 2.0)));
        assert_eq!(input.try_recv().ok(), Some(SurfaceInput::Resized { size, scale: 2.0 }));
        release(&id);
        assert!(!Arc::ptr_eq(&surface, &super::surface(&id)));
        release(&id);
    }
}
