#![warn(clippy::all, rust_2018_idioms)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

#[cfg(all(feature = "dhat-heap", not(target_arch = "wasm32")))]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

#[cfg(feature = "dhat-heap")]
static DHAT_PROFILER: std::sync::Mutex<Option<dhat::Profiler>> = std::sync::Mutex::new(None);

// When compiling natively:
#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result<()> {
    // Heap profiling. Controlled by env vars so shutdown strategy can be tuned
    // without rebuilding:
    //   DHAT_RUN_SECS   seconds before snapshotting (default 25)
    //   DHAT_TRIM       backtrace depth to retain (default 16)
    //   DHAT_EXIT       "1" => process::exit after drop; else idle so an external
    //                   harness can confirm the file then kill the process
    // Markers are appended to dhat-markers.log (the profiling build has no console
    // on Windows) so we can see exactly how far shutdown gets.
    #[cfg(feature = "dhat-heap")]
    {
        fn dhat_marker(msg: &str) {
            use std::io::Write as _;
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open("dhat-markers.log") {
                let _ = writeln!(f, "{msg}");
            }
        }
        let trim = std::env::var("DHAT_TRIM").ok().and_then(|v| v.parse::<usize>().ok()).unwrap_or(16);
        let profiler = dhat::Profiler::builder().trim_backtraces(Some(trim)).build();
        *DHAT_PROFILER.lock().unwrap() = Some(profiler);
        dhat_marker("profiler_started");
        let secs = std::env::var("DHAT_RUN_SECS").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(25);
        // Profiler::drop does not converge for this app (it allocates without
        // bound while symbolizing a ~1 GiB live heap with other threads still
        // running), so do not drop it. HeapStats::get gives the live/peak heap
        // totals with no symbolization. Log periodically; the harness kills us.
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(secs));
            for round in 0.. {
                let s = dhat::HeapStats::get();
                dhat_marker(&format!(
                    "heapstats round={round} curr_bytes={} curr_blocks={} max_bytes={} max_blocks={} total_bytes={} total_blocks={}",
                    s.curr_bytes, s.curr_blocks, s.max_bytes, s.max_blocks, s.total_bytes, s.total_blocks
                ));
                std::thread::sleep(std::time::Duration::from_secs(3));
            }
        });
    }
    use std::backtrace::Backtrace;
    use std::env;
    use std::io::Write;
    use std::sync::Once;

    match wows_toolkit::cli::resolve(env::args_os()) {
        Ok(wows_toolkit::cli::Invocation::FinalizeUpdate { replaced }) => {
            finalize_update(&replaced);
        }
        Ok(wows_toolkit::cli::Invocation::Run(_)) => {}
        Err(error) => {
            // use_stderr() is false exactly for --help/--version, which arrive
            // through this same arm and are not failures; pick the icon and
            // caption accordingly.
            let is_error = error.use_stderr();
            let title = if is_error { "wows_toolkit: argument error" } else { "wows_toolkit" };
            wows_toolkit::cli::report_startup_message(title, &error.render().to_string(), is_error);
            std::process::exit(error.exit_code());
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

    let mut viewport = egui::ViewportBuilder::default()
        .with_min_inner_size([400.0, 300.0])
        .with_icon(eframe::icon_data::from_png_bytes(icon_data).expect("failed to load application icon"))
        .with_title(format!("{} v{}", wows_toolkit::APP_NAME, env!("CARGO_PKG_VERSION")))
        .with_drag_and_drop(true);

    // Restore window position/size from the database before creating the window.
    // Position can only be set on the ViewportBuilder, not via viewport commands.
    if let Some(settings) = wows_toolkit::load_main_window_settings() {
        use wows_toolkit::WindowSettingsEguiExt;
        viewport = settings.apply_to_builder(viewport, [600.0, 400.0]);
    } else {
        viewport = viewport.with_inner_size([600.0, 400.0]);
    }

    let mut wgpu_options = eframe::egui_wgpu::WgpuConfiguration::default();
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(ref mut setup) = wgpu_options.wgpu_setup {
        // Keep the default wide backend set (PRIMARY | GL, overridable with WGPU_BACKEND) so we
        // run on Vulkan/Metal/DX12/GL across platforms. Bias adapter selection toward DX12 on
        // Windows, otherwise prefer a discrete GPU on a real graphics API over a GL fallback.
        setup.native_adapter_selector = Some(std::sync::Arc::new(|adapters, surface| {
            adapters
                .iter()
                .filter(|adapter| surface.is_none_or(|surface| adapter.is_surface_supported(surface)))
                .min_by_key(|adapter| {
                    let info = adapter.get_info();
                    // Pick the most capable GPU first.
                    let device_rank = match info.device_type {
                        wgpu::DeviceType::DiscreteGpu => 0u8,
                        wgpu::DeviceType::IntegratedGpu => 1,
                        wgpu::DeviceType::VirtualGpu => 2,
                        wgpu::DeviceType::Cpu => 3,
                        wgpu::DeviceType::Other => 4,
                    };
                    // Then prefer Vulkan/Metal over DX12: the DXGI flip-model swapchain
                    // stutters during native window moves and non-client hover animations on
                    // Windows. GL avoids it too but is the least battle-tested backend, so it
                    // stays the last resort. DX12 remains the fallback when Vulkan is absent.
                    let backend_rank = match info.backend {
                        wgpu::Backend::Vulkan | wgpu::Backend::Metal => 0u8,
                        wgpu::Backend::Dx12 => 1,
                        wgpu::Backend::Gl => 2,
                        _ => 3,
                    };
                    (device_rank, backend_rank)
                })
                .cloned()
                .ok_or_else(|| "no compatible wgpu adapter available".to_string())
        }));
    }

    let native_options = eframe::NativeOptions { viewport, wgpu_options, ..Default::default() };
    eframe::run_native(
        wows_toolkit::APP_NAME,
        native_options,
        Box::new(|cc| {
            let app = wows_toolkit::WowsToolkitApp::new(cc);
            Ok(Box::new(app))
        }),
    )
}

/// Delete the binary this process replaced. Failures are not worth surfacing:
/// the update has already succeeded by this point, and the only consequence is
/// a stale file next to the executable.
#[cfg(not(target_arch = "wasm32"))]
fn finalize_update(replaced: &Path) {
    let Ok(current_exe) = std::env::current_exe() else {
        return;
    };

    if wows_toolkit::cli::validate_finalize_target(&current_exe, replaced).is_err() {
        return;
    }

    // Give the parent process time to exit before unlinking its image. Racy,
    // but a failed delete only leaves a stale file.
    std::thread::sleep(std::time::Duration::from_secs(1));
    let _ = std::fs::remove_file(replaced);
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
