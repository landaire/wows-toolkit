#![warn(clippy::all, rust_2018_idioms)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

// When compiling natively:
#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result<()> {
    use std::backtrace::Backtrace;
    use std::env;
    use std::io::Write;

    use std::path::Path;
    use std::sync::Once;

    // Check to see if we need to delete the previous application
    let args: Vec<String> = env::args().collect();
    if args.len() == 2 {
        let current_path = Path::new(args[0].as_str());
        let old_path = Path::new(args[1].as_str());
        // Sanity check -- ensure that these files are in the same directory
        if current_path.parent() == old_path.parent()
            && let Some(name) = old_path.file_name().and_then(|name| name.to_str())
            && name.contains(".exe")
            && old_path.exists()
        {
            // Sleep for 1 second to give the parent process some time to exit.
            // This is racy but better than just failing.

            use std::time::Duration;
            std::thread::sleep(Duration::from_secs(1));

            let _ = std::fs::remove_file(old_path);
        }
    }

    // Enable the panic handler if the feature is explicitly enabled or
    // debug assertions are not enabled.
    if cfg!(any(feature = "panic_handler", not(debug_assertions))) {
        static SET_HOOK: Once = Once::new();

        let main_thread = std::thread::current().id();
        // Set a custom panic hook only once
        SET_HOOK.call_once(|| {
            let default_hook = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                let panicking_thread_id = std::thread::current().id();

                if panicking_thread_id != main_thread {
                    // Don't log panics if they aren't on the main thread
                    default_hook(info);
                    return;
                }

                // If we panic, we want to write the panic message to the log file
                // before we exit
                let panic_path = wows_toolkit::WowsToolkitApp::panic_log_path();
                // TOOD: possible race if multiple panics happen at once?
                if let Ok(mut file) = std::fs::File::create(&panic_path) {
                    let _ = writeln!(file, "{info}");
                    let _ = writeln!(file, "Backtrace:\n{}", Backtrace::force_capture());
                }
            }));
        });
    }

    // The i18n!() macro generates a lazy static whose initializer needs ~1.2 MB
    // of stack in debug builds (one HashMap::insert per translation key). Trigger
    // it on a thread with enough stack so the main thread's default 1 MB isn't exceeded.
    std::thread::Builder::new()
        .stack_size(4 * 1024 * 1024)
        .spawn(wows_toolkit::init_i18n)
        .expect("failed to spawn i18n init thread")
        .join()
        .expect("i18n init thread panicked");

    let icon_data: &[u8] = &include_bytes!("../../../assets/wows_toolkit.png")[..];

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([600.0, 400.0])
            .with_min_inner_size([400.0, 300.0])
            .with_icon(eframe::icon_data::from_png_bytes(icon_data).expect("failed to load application icon"))
            .with_title(format!("{} v{}", wows_toolkit::APP_NAME, env!("CARGO_PKG_VERSION")))
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        wows_toolkit::APP_NAME,
        native_options,
        Box::new(|cc| {
            let app = wows_toolkit::WowsToolkitApp::new(cc);
            Ok(Box::new(app))
        }),
    )
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
