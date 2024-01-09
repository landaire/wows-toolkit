#![warn(clippy::all, rust_2018_idioms)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

// When compiling natively:
#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result<()> {
    use std::{env, path::Path};

    env_logger::init(); // Log to stderr (if you run with `RUST_LOG=debug`).

    let icon_data: &[u8] = &include_bytes!("../assets/wows_toolkit.png")[..];

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([600.0, 400.0])
            .with_min_inner_size([400.0, 300.0])
            .with_icon(eframe::icon_data::from_png_bytes(icon_data).expect("failed to load application icon"))
            .with_title(format!("{} v{}", wows_toolkit::APP_NAME, env!("CARGO_PKG_VERSION"))),
        ..Default::default()
    };

    // Check to see if we need to delete the previous application
    let args: Vec<String> = env::args().collect();
    if args.len() == 2 {
        let current_path = Path::new(args[0].as_str());
        let old_path = Path::new(args[1].as_str());
        // Sanity check -- ensure that these files are in the same directory
        if current_path.parent() == old_path.parent() {
            if let Some(name) = old_path.file_name().and_then(|name| name.to_str()) {
                if name.contains(".exe") && old_path.exists() {
                    let _ = std::fs::remove_file(old_path);
                }
            }
        }
    }

    eframe::run_native(wows_toolkit::APP_NAME, native_options, Box::new(|cc| Box::new(wows_toolkit::WowsToolkitApp::new(cc))))
}

// When compiling to web using trunk:
#[cfg(target_arch = "wasm32")]
fn main() {
    // Redirect `log` message to `console.log` and friends:
    eframe::WebLogger::init(log::LevelFilter::Debug).ok();

    let web_options = eframe::WebOptions::default();

    wasm_bindgen_futures::spawn_local(async {
        eframe::WebRunner::new()
            .start(
                "the_canvas_id", // hardcode it
                web_options,
                Box::new(|cc| Box::new(wows_toolkit::WowsToolkitApp::new(cc))),
            )
            .await
            .expect("failed to start eframe");
    });
}
