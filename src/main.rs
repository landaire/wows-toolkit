#![warn(clippy::all, rust_2018_idioms)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

// When compiling natively:
#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result<()> {
    use std::backtrace::Backtrace;
    use std::env;
    use std::io::Write;

    use std::sync::Once;

    #[cfg(all(debug_assertions, feature = "logging"))]
    {
        // Janky hack to address https://github.com/tokio-rs/tracing/issues/1817
        struct NewType(Pretty);

        impl<'writer> FormatFields<'writer> for NewType {
            fn format_fields<R: RecordFields>(&self, writer: Writer<'writer>, fields: R) -> core::fmt::Result {
                self.0.format_fields(writer, fields)
            }
        }

        // use tracing_appender::rolling::Rotation;
        use tracing::level_filters::LevelFilter;
        use tracing_appender::rolling::Rotation;
        use tracing_subscriber::Layer;
        use tracing_subscriber::field::RecordFields;
        use tracing_subscriber::fmt;
        use tracing_subscriber::fmt::FormatFields;
        use tracing_subscriber::fmt::format::Pretty;
        use tracing_subscriber::fmt::format::Writer;
        use tracing_subscriber::fmt::time::LocalTime;
        use tracing_subscriber::layer::SubscriberExt;

        let file_appender = tracing_appender::rolling::Builder::new()
            .rotation(Rotation::HOURLY)
            .max_log_files(1)
            .filename_prefix("wows_toolkit.log")
            .build(".")
            .expect("failed to build file appender");
        let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
        let subscriber = tracing_subscriber::registry()
            .with(
                fmt::Layer::new()
                    .pretty()
                    // .with_writer(std::io::stdout)
                    // .with_timer(LocalTime::rfc_3339())
                    .fmt_fields(NewType(Pretty::default()))
                    .with_ansi(true)
                    .with_filter(LevelFilter::DEBUG),
            )
            .with(fmt::Layer::new().with_writer(non_blocking).with_timer(LocalTime::rfc_3339()).with_ansi(false).with_filter(LevelFilter::DEBUG));
        #[cfg(all(debug_assertions, feature = "logging"))]
        tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");
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
                    let _ = writeln!(file, "{}", info);
                    let _ = writeln!(file, "Backtrace:\n{}", Backtrace::force_capture());
                }
            }));
        });
    }

    let icon_data: &[u8] = &include_bytes!("../assets/wows_toolkit.png")[..];

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
