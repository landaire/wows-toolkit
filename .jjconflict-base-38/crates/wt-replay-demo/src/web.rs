//! The browser entry point, and fetching the clip.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use wows_minimap_renderer::track_file::TrackFile;

use crate::app::DemoApp;
use crate::app::Inbox;
use crate::error::LoadError;
use crate::page::PageOptions;
use crate::page::TRACK_URL;

/// Called when the module loads.
#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    let window = web_sys::window().ok_or("no window")?;
    let canvas = window
        .document()
        .ok_or("no document")?
        .get_element_by_id("the_canvas_id")
        .ok_or("no canvas element")?
        .dyn_into::<web_sys::HtmlCanvasElement>()?;
    let page = PageOptions::parse(&window.location().search()?);

    let web_options = eframe::WebOptions {
        // The demo sits inside a scrolling page and has nothing to scroll
        // itself, so the wheel scrolls the page.
        should_prevent_default: Box::new(|event| !matches!(event, egui::Event::MouseWheel { .. })),
        should_stop_propagation: Box::new(|event| !matches!(event, egui::Event::MouseWheel { .. })),
        ..eframe::WebOptions::default()
    };

    wasm_bindgen_futures::spawn_local(async move {
        let started = eframe::WebRunner::new()
            .start(
                canvas,
                web_options,
                Box::new(move |cc| {
                    let inbox: Inbox = Rc::new(RefCell::new(None));
                    let ctx = cc.egui_ctx.clone();
                    let delivery = Rc::clone(&inbox);
                    // A failure here is drawn by the app itself, which is
                    // running by the time the fetch settles.
                    wasm_bindgen_futures::spawn_local(async move {
                        let result = fetch_track(TRACK_URL).await;
                        *delivery.borrow_mut() = Some(result);
                        ctx.request_repaint();
                    });

                    Ok(Box::new(DemoApp::new(cc, &page, inbox)))
                }),
            )
            .await;
        if let Err(error) = started {
            web_sys::console::error_1(&error);
            let reason = error.as_string().unwrap_or_else(|| format!("{error:?}"));
            show_page_error(&format!("The replay demo could not start: {reason}"));
        }
    });

    Ok(())
}

/// Puts `message` over the canvas, for a failure the app cannot draw because
/// it never started.
fn show_page_error(message: &str) {
    let Some(document) = web_sys::window().and_then(|window| window.document()) else {
        return;
    };
    let Some(body) = document.body() else {
        return;
    };
    let Ok(notice) = document.create_element("div") else {
        return;
    };

    notice.set_class_name("error");
    notice.set_text_content(Some(message));
    // Nothing is left to report to if even this fails.
    let _ = body.append_child(&notice);
}

async fn fetch_track(url: &str) -> Result<TrackFile, LoadError> {
    let fetch_error = |error: JsValue| LoadError::Fetch {
        url: url.to_owned(),
        reason: error.as_string().unwrap_or_else(|| format!("{error:?}")),
    };

    let window = web_sys::window().ok_or_else(|| fetch_error(JsValue::from_str("no window")))?;
    let response: web_sys::Response =
        JsFuture::from(window.fetch_with_str(url)).await.map_err(fetch_error)?.dyn_into().map_err(fetch_error)?;
    if !response.ok() {
        return Err(LoadError::Status { url: url.to_owned(), status: response.status() });
    }

    let buffer = JsFuture::from(response.array_buffer().map_err(fetch_error)?).await.map_err(fetch_error)?;
    let bytes = js_sys::Uint8Array::new(&buffer).to_vec();

    Ok(TrackFile::decode(&bytes)?)
}
