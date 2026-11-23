//! WoWs Toolkit web client — WASM entry point.
//!
//! Connects to a desktop host's collaborative session via iroh relay,
//! receives map data and annotations, and renders them in a browser canvas.

// Most of this crate is WASM-only; suppress dead-code warnings when
// compiled for the host target (e.g. during `cargo check --workspace`).
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code, unused_imports))]

mod app;
mod assets;
mod connection;
mod state;
mod types;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsCast;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

/// WASM entry point — called automatically when the module loads.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    // Set up tracing for WASM — only enable our first-party crates to avoid
    // noise from iroh, quinn, rustls, etc.
    {
        use tracing_subscriber::Layer;
        use tracing_subscriber::layer::SubscriberExt;
        use tracing_subscriber::util::SubscriberInitExt;

        let wasm_layer = tracing_wasm::WASMLayer::new(
            tracing_wasm::WASMLayerConfigBuilder::new().set_max_level(tracing::Level::DEBUG).build(),
        );

        let filter = tracing_subscriber::filter::Targets::new()
            .with_target("wt_web", tracing::Level::DEBUG)
            .with_target("wt_collab_protocol", tracing::Level::INFO)
            .with_target("wt_collab_egui", tracing::Level::INFO)
            .with_target("wows_toolkit", tracing::Level::INFO);

        tracing_subscriber::registry().with(wasm_layer.with_filter(filter)).init();
    }

    let canvas = web_sys::window()
        .expect("no window")
        .document()
        .expect("no document")
        .get_element_by_id("the_canvas_id")
        .expect("no canvas element")
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .expect("element is not a canvas");

    let web_options = eframe::WebOptions::default();
    wasm_bindgen_futures::spawn_local(async {
        eframe::WebRunner::new()
            .start(canvas, web_options, Box::new(|cc| Ok(Box::new(app::WebApp::new(cc)))))
            .await
            .expect("failed to start eframe");
    });

    Ok(())
}
