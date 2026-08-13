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

    use wows_toolkit::gpu::select::RenderMode;

    let cli = match wows_toolkit::cli::resolve(env::args_os()) {
        Ok(wows_toolkit::cli::Invocation::FinalizeUpdate { replaced }) => {
            finalize_update(&replaced);
            // The updater relaunch carries no render flags in either form, and
            // the app still has to start afterwards.
            wows_toolkit::cli::Cli::default()
        }
        Ok(wows_toolkit::cli::Invocation::ListGpus) => {
            let (message, failed) = match wows_toolkit::gpu::probe::probe() {
                Ok(adapters) => (wows_toolkit::cli::describe_adapters(&adapters), false),
                Err(error) => (format!("Failed to read the display adapter registry: {error}\n"), true),
            };
            wows_toolkit::cli::report_startup_message("wows_toolkit: display adapters", &message, failed);
            std::process::exit(i32::from(failed));
        }
        Ok(wows_toolkit::cli::Invocation::Run(cli)) => cli,
        Err(error) => {
            // use_stderr() is false exactly for --help/--version, which arrive
            // through this same arm and are not failures; pick the icon and
            // caption accordingly.
            let is_error = error.use_stderr();
            let title = if is_error { "wows_toolkit: argument error" } else { "wows_toolkit" };
            wows_toolkit::cli::report_startup_message(title, &error.render().to_string(), is_error);
            std::process::exit(error.exit_code());
        }
    };

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
                    // Which render configuration was running, which policies
                    // were in force, and what got in anyway. With the policies
                    // applied the module list should be empty, which makes the
                    // report evidence that injection was not the cause; without
                    // them it names the culprit. The render mode has to be here
                    // rather than only in the log, because a crash is exactly
                    // when the log may not have been flushed.
                    if let Some(mode) = wows_toolkit::boot::active_mode() {
                        let _ = writeln!(file, "\nRender mode: {}", mode.as_token());
                    }
                    if let Some(report) = wows_toolkit::hardening::report() {
                        let _ = write!(file, "{report}");
                    }
                    // The snapshot rather than a fresh enumeration: walking the
                    // module list takes the loader lock, and a fault inside a
                    // hooked driver can leave another thread holding it, which
                    // would hang the process instead of letting it exit.
                    if let Some(inventory) = wows_toolkit::hardening::modules::snapshot() {
                        let _ = write!(file, "{}", wows_toolkit::hardening::modules::describe(inventory));
                    }
                }
            }));
        });
    }

    // Load the translation catalogs before anything asks for a string. This
    // once needed a 4 MB thread: `i18n!()` was pointed at the catalogs and its
    // generated initializer built one temporary per key in a single frame. The
    // catalogs are parsed from embedded source now (see `TranslationsBackend`),
    // which needs well under 64 KB, so the main thread is fine.
    wows_toolkit::init_i18n();

    let icon_data: &[u8] = &include_bytes!(concat!(env!("OUT_DIR"), "/wows_toolkit.png"))[..];

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

    // An unreadable adapter registry is not fatal: without a probe there is
    // nothing to pin, which is exactly the full-enumeration behaviour that
    // existed before pinning.
    let adapters = wows_toolkit::gpu::probe::probe().unwrap_or_else(|error| {
        tracing::warn!("Failed to probe display adapters, falling back to full enumeration: {error}");
        Vec::new()
    });
    let overrides = cli.render_overrides();
    let fingerprint = wows_toolkit::gpu::probe::fingerprint(&adapters);
    // A run driven by explicit render flags is a diagnostic, not evidence about
    // any mode: recording it would make the next bare launch trust a
    // configuration that was never attempted, and killing it would demote a
    // fallback it never exercised.
    let remember_mode_for_next_launch = !overrides.are_set();
    let mode =
        if remember_mode_for_next_launch { wows_toolkit::boot::planned_mode(&fingerprint) } else { RenderMode::FIRST };

    let render_config = match wows_toolkit::gpu::select::resolve(&adapters, &overrides, None, mode) {
        Ok(config) => config,
        Err(error) => {
            wows_toolkit::cli::report_startup_message("wows_toolkit: adapter selection", &error.to_string(), true);
            std::process::exit(2);
        }
    };
    // Record the mode that will actually run, not the one that was asked for.
    // `resolve` skips modes the present adapters cannot satisfy, and recording
    // an unsatisfiable one would spend a launch per skipped mode re-attempting
    // an identical configuration.
    if remember_mode_for_next_launch {
        wows_toolkit::boot::remember_mode(&fingerprint, render_config.mode);
    }
    wows_toolkit::boot::set_active_mode(render_config.mode);
    tracing::info!(
        "Render configuration: mode {}, backends {:?}, adapter {:?}",
        render_config.mode.as_token(),
        render_config.backends,
        render_config.adapter
    );

    // Before any window exists and before wgpu creates its instance: the
    // extension points attach at window creation, and overlays hook at
    // swapchain creation. The report is stashed for the app to log once the
    // tracing subscriber exists, and for the panic hook to include.
    wows_toolkit::hardening::apply_startup_mitigations(wows_toolkit::hardening::HardeningRequest {
        hardening: cli.hardening(render_config.mode),
        code_integrity: wows_toolkit::load_code_integrity_preference(),
    });

    let mut wgpu_options = eframe::egui_wgpu::WgpuConfiguration::default();
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(ref mut setup) = wgpu_options.wgpu_setup {
        apply_render_config(&render_config, setup);
    }

    let native_options = eframe::NativeOptions { viewport, wgpu_options, ..Default::default() };
    let result = eframe::run_native(
        wows_toolkit::APP_NAME,
        native_options,
        Box::new(|cc| {
            let app = wows_toolkit::WowsToolkitApp::new(cc);
            Ok(Box::new(app))
        }),
    );

    // A release build is a windows subsystem binary with no console, so a
    // returned error would otherwise be a silent exit with no window: the app
    // would look like it simply did nothing. Pinning can cause exactly that, by
    // selecting an adapter the driver then declines to enumerate.
    if let Err(ref error) = result {
        let message = format!(
            "The app could not start with render mode {}.\n\n{error}\n\n\
             The next launch will try a safer mode. To choose one now, run with \
             --cpu-renderer or --gpu-safe-mode. If those also fail, add \
             --no-hardening, which is the only way to reach the software \
             renderer with the process mitigations turned off.",
            render_config.mode.as_token()
        );
        wows_toolkit::cli::report_startup_message("wows_toolkit: startup failed", &message, true);
    }
    result
}

/// Ask the hybrid-graphics shim for the discrete GPU.
///
/// These are read by the vendor shims at process start, before any of our code
/// runs, which is why they are link-time exports rather than a runtime decision.
/// They only bias which GPU a muxed laptop hands the process; `VK_DRIVER_FILES`
/// is what actually decides whose driver code gets loaded, and it wins.
#[cfg(all(windows, target_env = "msvc"))]
#[allow(non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static NvOptimusEnablement: u32 = 1;

#[cfg(all(windows, target_env = "msvc"))]
#[allow(non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static AmdPowerXpressRequestHighPerformance: u32 = 1;

/// Point wgpu at exactly the device and driver that were resolved.
///
/// The ICD pin and the backend narrowing both have to happen before the wgpu
/// instance is created: adapter enumeration loads every installed ICD, so a
/// selector callback runs far too late to keep a vendor's code out of the
/// process.
#[cfg(not(target_arch = "wasm32"))]
fn apply_render_config(
    config: &wows_toolkit::gpu::select::RenderConfig,
    setup: &mut eframe::egui_wgpu::WgpuSetupCreateNew,
) {
    use wows_toolkit::gpu::select::AdapterChoice;
    use wows_toolkit::gpu::select::BackendSelection;
    use wows_toolkit::gpu::select::RenderBackend;

    if let BackendSelection::Only(backend) = config.backends {
        setup.instance_descriptor.backends = match backend {
            RenderBackend::Vulkan => wgpu::Backends::VULKAN,
            RenderBackend::Dx12 => wgpu::Backends::DX12,
        };
    }

    if let AdapterChoice::Pinned { icd, .. } = &config.adapter {
        // On Windows the loader discovers ICDs from the PnP adapter registry,
        // not only from manifest files, and that path ignores VK_DRIVER_FILES
        // entirely: with it set to the NVIDIA manifest the loader still logged
        // "Located json file ...amd-vulkan64.json from PnP registry" and loaded
        // amdvlk64.dll. VK_LOADER_DRIVERS_SELECT filters what that enumeration
        // yields, and is what actually keeps the other vendor out. The manifest
        // variables stay set for loaders older than 1.3.234, which do not
        // implement the select filter.
        let manifest = icd.as_path().as_os_str();
        // The select filter matches manifest file names. Handing it a full path
        // would match nothing and select zero drivers, which fails the launch
        // outright, so a path with no file name drops the filter rather than
        // applying a broken one.
        let name = icd.as_path().file_name();
        // SAFETY: Rust's set_var on Windows is SetEnvironmentVariableW and
        // reads go through GetEnvironmentVariableW, both internally
        // synchronised by the OS. Threads do exist by this point (the settings
        // read above builds a tokio runtime and an sqlx pool), and no code in
        // this process reads these variables; the Vulkan loader reads them
        // later, on this thread, during instance creation.
        unsafe {
            match name {
                Some(name) => std::env::set_var("VK_LOADER_DRIVERS_SELECT", name),
                None => std::env::remove_var("VK_LOADER_DRIVERS_SELECT"),
            }
            std::env::set_var("VK_DRIVER_FILES", manifest);
            std::env::set_var("VK_ICD_FILENAMES", manifest);
            // Implicit layers load into every Vulkan process regardless of
            // which driver was selected. AMD's switchable-graphics layer pulls
            // in amdvlk64.dll on its own, which defeats the pin, and the same
            // mechanism is how OBS and the Steam overlay inject. Disabling them
            // removed every third-party module from the process.
            std::env::set_var("VK_LOADER_LAYERS_DISABLE", "~implicit~");
        }
    } else {
        // An unpinned mode must actually be unpinned. These can be inherited:
        // the updater relaunches this binary, and a user can export them by
        // hand. Leaving an inherited pin in place would keep this stuck on a
        // driver that a safer mode was chosen specifically to escape.
        // SAFETY: as above.
        unsafe {
            for name in wows_toolkit::gpu::PIN_VARS {
                std::env::remove_var(name);
            }
        }
    }

    let choice = config.adapter.clone();
    setup.native_adapter_selector =
        Some(std::sync::Arc::new(move |adapters: &[wgpu::Adapter], surface: Option<&wgpu::Surface<'_>>| {
            let usable = || adapters.iter().filter(|a| surface.is_none_or(|s| a.is_surface_supported(s)));

            if let AdapterChoice::Cpu = choice {
                // WARP is the whole point of this mode; falling back to a hardware
                // adapter here would defeat it.
                return usable()
                    .find(|adapter| adapter.get_info().device_type == wgpu::DeviceType::Cpu)
                    .cloned()
                    .ok_or_else(|| "no CPU (WARP) adapter available".to_string());
            }

            // Select the pinned device by name where possible. The ICD filter
            // matches a manifest file name, and one manifest can serve several
            // devices: two NVIDIA cards share nv-vk64.json, as do an Intel iGPU
            // and an Arc dGPU. Ranking alone would then re-pick the device the
            // alternate mode exists to avoid. DX12 has no filter at all, so
            // there this is the only thing honouring the choice.
            if let AdapterChoice::Pinned { adapter: pinned, .. } = &choice
                && let Some(found) = usable().find(|adapter| adapter.get_info().name == pinned.as_str())
            {
                return Ok(found.clone());
            }

            // A pinned ICD usually leaves exactly one adapter, but a vendor can
            // expose several. Rank within whatever the pin admitted.
            usable()
                .min_by_key(|adapter| {
                    let info = adapter.get_info();
                    let device_rank = match info.device_type {
                        wgpu::DeviceType::DiscreteGpu => 0u8,
                        wgpu::DeviceType::IntegratedGpu => 1,
                        wgpu::DeviceType::VirtualGpu => 2,
                        wgpu::DeviceType::Cpu => 3,
                        wgpu::DeviceType::Other => 4,
                    };
                    // Prefer Vulkan/Metal over DX12: the DXGI flip-model swapchain
                    // stutters during native window moves and non-client hover
                    // animations on Windows. GL avoids it too but is the least
                    // battle-tested backend, so it stays the last resort.
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

/// Delete the binary this process replaced. Failures are not worth surfacing:
/// the update has already succeeded by this point, and the only consequence is
/// a stale file next to the executable.
#[cfg(not(target_arch = "wasm32"))]
fn finalize_update(replaced: &Path) {
    let Ok(current_exe) = std::env::current_exe() else {
        return;
    };

    let Ok(replaced) = wows_toolkit::cli::validate_finalize_target(&current_exe, replaced) else {
        return;
    };

    // Give the parent process time to exit before unlinking its image. Racy,
    // but a failed delete only leaves a stale file.
    std::thread::sleep(std::time::Duration::from_secs(1));
    // Delete the normalized path returned above, not the raw argument: that is
    // the path validate_finalize_target actually checked.
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
