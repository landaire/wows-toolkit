#[cfg(target_os = "linux")]
use nsh_platform_linux as platform;

#[cfg(target_os = "linux")]
fn main() -> platform::gtk4::glib::ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("wows-toolkit-nsh: {error}");
            platform::gtk4::glib::ExitCode::FAILURE
        }
    }
}

#[cfg(target_os = "linux")]
fn run() -> Result<platform::gtk4::glib::ExitCode, Box<dyn std::error::Error>> {
    const APP_ID: &str = "dev.landaire.wows-toolkit-nsh";

    // Ring is installed because both rustls providers are in the graph, and
    // the first TLS client panics unless one is chosen.
    let _ = rustls::crypto::ring::default_provider().install_default();
    // The log file is what a bug report is copied from, so it starts before
    // anything that could fail. An unreadable database reads as logging on,
    // the default every front end uses.
    let to_file = wows_toolkit_config::load_startup_setting::<bool>(
        wows_toolkit_viewmodel::settings::keys::ENABLE_LOGGING,
    )
    .unwrap_or(None)
    .unwrap_or(true);
    let _log_guard = wows_toolkit_nsh::shell::logging::init(to_file);
    wows_toolkit_viewmodel::logging::install_panic_hook();

    // Scripted runs: WTN_EXIT_AFTER_SECS bounds the run for measurements. The
    // quit runs on the GTK main loop; process::exit from another thread races
    // the GL driver's teardown and crashes in gsk_gl_frame_is_busy.
    if let Some(seconds) = std::env::var("WTN_EXIT_AFTER_SECS").ok().and_then(|value| value.parse::<u64>().ok()) {
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(seconds));
            log::info!("exiting after {seconds} s");
            platform::gtk4::glib::MainContext::default().invoke(|| {
                use platform::gtk4::gio::prelude::ApplicationExt;

                if let Some(app) = platform::gtk4::gio::Application::default() {
                    app.quit();
                }
            });
        });
    }

    platform::set_ui_sets(vec![nsh_base::registry(), wows_toolkit_nsh::registry()]);
    let components = platform::Components::default();
    wows_toolkit_nsh::frames::native::register(&components);

    let capabilities = bundle_capabilities()?;
    let bundle = platform::Bundle::source(bundle_source()?)
        .with_app_id(Some(APP_ID))
        .with_capabilities_json(capabilities.as_deref())
        .map_err(std::io::Error::other)?;
    Ok(platform::run_app(
        platform::AppSource::Bundle(bundle),
        platform::AppOptions {
            title: "WoWs Toolkit".to_owned(),
            app_id: APP_ID.to_owned(),
            size: (1400, 1000),
            fps: std::env::var_os("WTN_FPS").is_some(),
            components,
            ..Default::default()
        },
    ))
}

#[cfg(all(target_os = "linux", feature = "buck"))]
fn bundle_source() -> Result<String, Box<dyn std::error::Error>> {
    Ok(include_str!(env!("NSH_BUNDLE_PATH")).to_owned())
}

#[cfg(all(target_os = "linux", not(feature = "buck")))]
fn bundle_source() -> Result<String, Box<dyn std::error::Error>> {
    let entry = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ui/App.tsx");
    let bundled = nsh_bundler::bundle(&nsh_bundler::BundleOptions {
        entry,
        minify: false,
        split: false,
        ui_sets: vec![nsh_base::registry().into(), wows_toolkit_nsh::registry().into()],
    })?;
    Ok(bundled.code)
}

#[cfg(target_os = "linux")]
fn bundle_capabilities() -> Result<Option<String>, serde_json::Error> {
    let manifest: serde_json::Value = serde_json::from_str(include_str!("../ui/nsh.json"))?;
    Ok(manifest.get("capabilities").map(serde_json::Value::to_string))
}

#[cfg(not(target_os = "linux"))]
fn main() -> std::process::ExitCode {
    eprintln!("wows-toolkit-nsh requires nsh's Linux GTK backend");
    std::process::ExitCode::FAILURE
}
