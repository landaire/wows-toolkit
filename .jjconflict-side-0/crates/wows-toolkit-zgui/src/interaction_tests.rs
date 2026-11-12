//! The shell opened headlessly.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use landaire_ui::Appearance;
use landaire_ui::test_support::Drawing;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use tokio::runtime::Runtime;
use wows_toolkit_viewmodel::settings::keys;
use zgui::view;

use crate::shell::AppRootProps;

/// The size the GPUI shell is reviewed at.
pub(crate) const REVIEW_SIZE: WindowSize = WindowSize { width: 1280.0, height: 800.0 };

/// Points `storage_dir` at a temporary directory for the whole process, with every first-run
/// question answered and the update check off, so the shell opens as a returning reader sees it.
/// The directory is leaked: it has to outlive every test in the process.
pub(crate) fn returning_reader(runtime: &Runtime) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let dir = tempfile::tempdir().expect("a temporary directory can be made");
        wows_toolkit_config::storage_override::set(dir.path().to_path_buf());
        std::mem::forget(dir);

        runtime.block_on(async {
            let pool = wows_toolkit_config::open_db().await.expect("the settings database opens");
            for key in [keys::BUILD_CONSENT_SHOWN, keys::REPLAY_CONSENT_SHOWN, keys::LANGUAGE_SELECTION_SHOWN] {
                wows_toolkit_config::queries::set_setting(&pool, key, &true).await.expect("a setting is written");
            }
            wows_toolkit_config::queries::set_setting(&pool, keys::CHECK_FOR_UPDATES, &false)
                .await
                .expect("a setting is written");
        });
    });
}

/// Opens the shell headlessly as a returning reader sees it, and lets the settings load.
pub(crate) fn open_shell(options: HeadlessOptions) -> HeadlessWindow {
    let runtime = Arc::new(wows_toolkit_services::runtime::build().expect("a tokio runtime starts"));
    returning_reader(&runtime);

    let mut window = HeadlessWindow::open(options, move || {
        let runtime = Some(Arc::clone(&runtime));
        view! { AppRoot(runtime = runtime) }
    })
    .expect("the shell opens headlessly");

    // The settings are read on the tokio runtime, in real time rather than the window's clock.
    settle_in_real_time(&mut window);

    window
}

/// Runs two seconds of frames, letting work on the tokio runtime land as it goes.
pub(crate) fn settle_in_real_time(window: &mut HeadlessWindow) {
    for _ in 0..20 {
        window.advance(Duration::from_millis(100));
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// The shell's review size, drawn on the graphics device with the desktop in `appearance`.
pub(crate) fn review_options(appearance: Appearance) -> HeadlessOptions {
    HeadlessOptions { drawing: Drawing::Device, desktop: Some(appearance), ..HeadlessOptions::new(REVIEW_SIZE) }
}

/// Writes what `window` shows to `target/zgui-shots/<name>.png`.
pub(crate) fn save_shot(window: &mut HeadlessWindow, name: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/zgui-shots").join(format!("{name}.png"));
    window.save_png(&path).expect("the window is drawn to a PNG");

    path
}

/// Opens the shell on the graphics device with the desktop in `appearance`, and writes it to
/// `target/zgui-shots/<name>.png`.
fn shoot(appearance: Appearance, name: &str) -> PathBuf {
    let mut window = open_shell(review_options(appearance));

    save_shot(&mut window, name)
}

#[test]
#[ignore = "draws on the graphics device; run by hand to review the shell"]
fn the_shell_renders_in_dark_and_light() {
    for (appearance, name) in [(Appearance::Dark, "shell-dark"), (Appearance::Light, "shell-light")] {
        let path = shoot(appearance, name);
        println!("{}", path.display());
    }
}
