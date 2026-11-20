//! The application icon, for the desktop's window icon and for the title bar the app draws.

use std::sync::LazyLock;

use image::ImageFormat;
use zgui::prelude::WindowIcon;
use zgui_image::ImageBytes;

/// 256 pixels square, the largest size a Windows window icon is drawn at.
const PNG: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/wows_toolkit_256.png"));

/// Decoded once: every window is handed the same icon.
static WINDOW_ICON: LazyLock<WindowIcon> = LazyLock::new(|| {
    // Compiled in, so a failure here is a build defect rather than bad input.
    let pixels =
        image::load_from_memory_with_format(PNG, ImageFormat::Png).expect("the embedded icon is a PNG").into_rgba8();
    let (width, height) = pixels.dimensions();
    WindowIcon::from_rgba(pixels.into_raw(), width, height).expect("decoded pixels match their dimensions")
});

/// Registered for the life of the process, since any window's title bar may show it.
static IMAGE: LazyLock<ImageBytes> = LazyLock::new(|| ImageBytes::new(PNG));

/// The icon a window asks the desktop to show for it. Wayland compositors take theirs from the
/// desktop entry named by the application ID instead.
pub fn window_icon() -> WindowIcon {
    WINDOW_ICON.clone()
}

/// The `src` of an image element showing the icon.
pub fn image_src() -> String {
    IMAGE.url()
}
