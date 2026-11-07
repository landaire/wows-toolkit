#[cfg(target_os = "linux")]
use nsh_platform_linux as platform;

#[cfg(target_os = "linux")]
fn main() -> impl std::process::Termination {
    const APP_ID: &str = "dev.landaire.wows-toolkit-nsh";

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

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

    platform::set_ui_sets(vec![nsh_base::registry()]);
    let components = platform::Components::default();
    wows_toolkit_nsh::frames::native::register(&components);

    platform::run_app(
        platform::AppSource::Rust(
            platform::Root::of::<wows_toolkit_nsh::App>()
                .tokens(nsh_base::Theme::default().into_tokens())
                .app_id(APP_ID),
        ),
        platform::AppOptions {
            title: "WoWs Toolkit (nsh spike)".to_owned(),
            app_id: APP_ID.to_owned(),
            size: (1400, 1000),
            fps: std::env::var_os("WTN_FPS").is_some(),
            // The app paints a dark background; dark theme tokens keep text legible.
            appearance: platform::Appearance::Dark,
            components,
            ..Default::default()
        },
    )
}

#[cfg(not(target_os = "linux"))]
fn main() -> std::process::ExitCode {
    eprintln!("wows-toolkit-nsh is a spike for nsh's Linux GTK backend; it does nothing on this platform");
    std::process::ExitCode::FAILURE
}
