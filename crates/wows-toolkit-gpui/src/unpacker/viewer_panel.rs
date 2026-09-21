//! A file viewer tab: the contents of one VFS file, as text or as an image.
//!
//! The egui app opens each viewer as its own OS window; here it is a tab in
//! the Unpacker's dock, which is the in-canvas equivalent and keeps the file
//! beside the listing it came from.

use std::sync::Arc;

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::dock::BasePanel;
use gpui_kit::component::dock::Panel;
use gpui_kit::component::dock::PanelEvent;
use gpui_kit::component::h_flex;
use gpui_kit::component::v_flex;
use gpui_kit::*;
use rust_i18n::t;

use wows_toolkit_viewmodel::unpacker::viewer::ViewerContent;

/// What the panel draws.
enum Shown {
    Text(SharedString),
    /// Decoded once on open; the atlas caches it by the image's own id.
    Image(Arc<RenderImage>),
    /// The bytes loaded but could not be decoded as an image.
    Undecodable(String),
}

pub struct FileViewerPanel {
    title: SharedString,
    shown: Shown,
    scroll: ScrollHandle,
    focus_handle: FocusHandle,
}

impl EventEmitter<PanelEvent> for FileViewerPanel {}

impl FileViewerPanel {
    pub fn new(title: SharedString, content: ViewerContent, cx: &mut Context<Self>) -> Self {
        let shown = match content {
            ViewerContent::Plaintext { text, .. } => Shown::Text(text.into()),
            ViewerContent::Image { bytes } => match decode_image(&bytes) {
                Ok(image) => Shown::Image(Arc::new(image)),
                Err(reason) => Shown::Undecodable(reason),
            },
        };

        Self { title, shown, scroll: ScrollHandle::new(), focus_handle: cx.focus_handle() }
    }
}

/// Decodes into the BGRA layout `RenderImage` expects.
///
/// GPUI's image buffer is BGRA while the `image` crate decodes to RGBA, so the
/// red and blue channels are swapped; skipping this shows every image with its
/// colours inverted.
fn decode_image(bytes: &[u8]) -> Result<RenderImage, String> {
    let decoded = image::load_from_memory(bytes).map_err(|err| err.to_string())?;
    let mut rgba = decoded.into_rgba8();
    for pixel in rgba.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Ok(RenderImage::new(vec![image::Frame::new(rgba)]))
}

impl Focusable for FileViewerPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl BasePanel for FileViewerPanel {
    fn panel_name(&self) -> &'static str {
        "UnpackerFileViewerPanel"
    }
}

impl Panel for FileViewerPanel {
    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.title.clone()
    }
}

impl Render for FileViewerPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().border;

        let header = h_flex()
            .flex_none()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(border)
            .child(div().text_xs().text_color(crate::theme::text_dim()).child(self.title.clone()));

        let body: AnyElement = match &self.shown {
            Shown::Text(text) => div()
                .id("file-viewer-text")
                .size_full()
                .overflow_scroll()
                .track_scroll(&self.scroll)
                .p_2()
                .font_family("monospace")
                .text_xs()
                .child(text.clone())
                .into_any_element(),
            Shown::Image(image) => div()
                .id("file-viewer-image")
                .size_full()
                .overflow_scroll()
                .track_scroll(&self.scroll)
                .p_2()
                .child(img(image.clone()))
                .into_any_element(),
            Shown::Undecodable(reason) => v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_sm()
                        .text_color(crate::theme::text_dim())
                        .child(t!("ui.unpacker.image_decode_failed", reason = reason).to_string()),
                )
                .into_any_element(),
        };

        v_flex().size_full().child(header).child(div().flex_1().min_h(px(0.)).child(body))
    }
}
