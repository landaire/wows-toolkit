//! A file viewer panel: one VFS file as highlighted read-only text or as an
//! image.

use base64::Engine as _;
use nsh_base::editor;
use nsh_core::prelude::*;
use wows_toolkit_viewmodel::unpacker::viewer::ViewerContent;
use wows_toolkit_viewmodel::unpacker::viewer::highlight_language;

use super::Tab;
use super::listing::extension;
use super::ui::caption;
use super::ui::class;
use super::ui::status;

/// What a viewer panel shows, read once when it opens.
pub struct ViewerDocument {
    pub title: String,
    pub content: ViewerContent,
}

/// The MIME type an image's `data:` URI names, by extension.
fn image_mime(title: &str) -> &'static str {
    match extension(title).as_deref() {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("svg") => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

pub fn viewer_view(document: &ViewerDocument, tab: Tab) -> AnyElement {
    let header = view()
        .app_class(class("flex-row items-center px-2 py-1 border-b border-border"))
        .child(caption(document.title.clone()));

    let body = match &document.content {
        ViewerContent::Plaintext { extension, text } => editor()
            .app_class(class("flex-1 min-h-0"))
            .accessibility_label(document.title.clone())
            .value(SharedString::from(text.clone()))
            // A language the editor does not highlight shows as plain text.
            .language(SharedString::from(highlight_language(extension).unwrap_or("text")))
            .read_only(true)
            .line_numbers(true)
            .font_size(px(12.0))
            .into_element(),
        ViewerContent::Image { bytes } => image_view(&document.title, bytes, tab),
    };

    view().app_class(class("flex-1 min-h-0 min-w-0")).child(header).child(body).into_element()
}

/// The image at its own size in a scroll view, or why it could not be
/// decoded: the platform decodes it, and reports a failure rather than
/// showing nothing.
fn image_view(title: &str, bytes: &[u8], tab: Tab) -> AnyElement {
    let source =
        format!("data:{};base64,{}", image_mime(title), base64::engine::general_purpose::STANDARD.encode(bytes));
    let failure = signal(None::<String>);
    let size = signal(None::<Size<DevicePixels>>);

    view()
        .app_class(class("flex-1 min-h-0"))
        .child(when(
            move || failure.with(Option::is_some),
            // Built only once a failure is set, so the reason is present.
            move || {
                status(
                    tab.text_with("ui.unpacker.image_decode_failed", &[("reason", &failure.get().unwrap_or_default())]),
                )
            },
        ))
        .child(when(
            move || failure.with(Option::is_none),
            move || {
                let source = source.clone();
                scroll().app_class(class("flex-1 min-h-0")).child(
                    view().app_class(class("p-2")).child(
                        image(source)
                            .label(tab.text("ui.unpacker.view_contents"))
                            .style_with(move |style| match size.get() {
                                Some(size) => style.w(px(size.width.0 as f32)).h(px(size.height.0 as f32)),
                                None => style,
                            })
                            .on_load(move |loaded: &Size<DevicePixels>| size.set(Some(*loaded)))
                            .on_error(move |reason: &str| failure.set(Some(reason.to_owned()))),
                    ),
                )
            },
        ))
        .into_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn images_are_sent_with_the_type_their_extension_names() {
        assert_eq!(image_mime("a/b.PNG"), "image/png");
        assert_eq!(image_mime("b.jpeg"), "image/jpeg");
        assert_eq!(image_mime("icon.svg"), "image/svg+xml");
    }
}
