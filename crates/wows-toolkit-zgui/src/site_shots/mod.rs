//! Screenshots for the project website, drawn from a real game install and real replays with
//! player names anonymized.
//!
//! The tests are ignored; run them by hand:
//!
//! ```text
//! WT_SITE_STORAGE=/path/to/copy/of/storage WT_WOWS_DIR=/path/to/World_of_Warships \
//! WT_SITE_REPLAY=/path/to/showcase.wowsreplay \
//!   cargo test -p wows-toolkit-zgui -- --ignored site_shots --test-threads=1
//! ```
//!
//! `WT_SITE_STORAGE` must be a copy of the app's storage directory: the shell writes settings
//! into it. Each shot is written to `target/zgui-shots/site/<name>-<dark|light>.png`;
//! `scripts/site/convert_shots.sh` converts them into the site's WebP images.

mod apps;
mod replays;

use std::path::PathBuf;

use landaire_ui::Appearance;
use landaire_ui::test_support::DisplayScale;
use landaire_ui::test_support::Drawing;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use wows_toolkit_viewmodel::settings::ThemeChoice;
use wows_toolkit_viewmodel::settings::keys;

use crate::interaction_tests::REVIEW_SIZE;

/// Both appearances, with the file suffix each is saved under.
pub(crate) const APPEARANCES: [(Appearance, &str); 2] = [(Appearance::Dark, "dark"), (Appearance::Light, "light")];

/// A required environment variable, read as a path.
pub(crate) fn env_path(name: &str) -> PathBuf {
    PathBuf::from(std::env::var_os(name).unwrap_or_else(|| panic!("{name} must be set; see site_shots/mod.rs")))
}

/// Points the process at the copied storage directory and turns on name anonymization in it.
///
/// Must run before the first shell opens: the storage directory is chosen once per process.
pub(crate) fn use_site_storage() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let dir = env_path("WT_SITE_STORAGE");
        assert!(!dir.as_os_str().is_empty(), "WT_SITE_STORAGE is empty; name a copy of the storage directory");
        let dir = std::fs::canonicalize(&dir)
            .unwrap_or_else(|err| panic!("WT_SITE_STORAGE {} does not resolve: {err}", dir.display()));

        // The shots write settings into the directory, so the reader's own must never be used.
        let normal = wows_toolkit_config::platform_storage_dir().and_then(|normal| std::fs::canonicalize(normal).ok());
        assert!(
            normal.as_ref() != Some(&dir),
            "WT_SITE_STORAGE {} is the app's own storage directory; point it at a copy",
            dir.display()
        );

        wows_toolkit_config::storage_override::set(dir.clone());
        assert_eq!(
            wows_toolkit_config::storage_dir().as_ref(),
            Some(&dir),
            "another test chose the storage directory first; run the site shots on their own"
        );

        let runtime = wows_toolkit_services::runtime::build().expect("a tokio runtime starts");
        runtime.block_on(async {
            let pool = wows_toolkit_config::open_db().await.expect("the settings database opens");
            let mut replay = wows_toolkit_config::queries::get_setting::<wows_toolkit_config::ReplaySettings>(
                &pool,
                keys::REPLAY_SETTINGS,
            )
            .await
            .unwrap_or_default();
            replay.anonymize_player_names = true;
            wows_toolkit_config::queries::set_setting(&pool, keys::REPLAY_SETTINGS, &replay)
                .await
                .expect("a setting is written");
            let wows_dir = env_path("WT_WOWS_DIR").to_string_lossy().into_owned();
            wows_toolkit_config::queries::set_setting(&pool, keys::WOWS_DIR, &wows_dir)
                .await
                .expect("a setting is written");
        });
    });
}

/// Shows `appearance` in the shell this test opened. The copied storage names a theme, which
/// would otherwise override the desktop's.
pub(crate) fn show_appearance(window: &mut HeadlessWindow, appearance: Appearance) {
    let choice = match appearance {
        Appearance::Dark => ThemeChoice::Dark,
        Appearance::Light => ThemeChoice::Light,
    };
    crate::interaction_tests::shell().set_theme(choice);
    window.advance(std::time::Duration::from_millis(300));
}

/// The shell in `appearance`, opened on the copied storage.
pub(crate) fn open_site_shell(appearance: Appearance) -> HeadlessWindow {
    use_site_storage();
    let mut window = crate::interaction_tests::open_shell(options(appearance));
    show_appearance(&mut window, appearance);

    window
}

/// The review size at a 1.25 display scale, drawn on the graphics device.
pub(crate) fn options(appearance: Appearance) -> HeadlessOptions {
    HeadlessOptions {
        drawing: Drawing::Device,
        desktop: Some(appearance),
        scale: DisplayScale(1.25),
        ..HeadlessOptions::new(REVIEW_SIZE)
    }
}

/// Writes what `window` shows to `target/zgui-shots/site/<name>-<suffix>.png`.
pub(crate) fn save(window: &mut HeadlessWindow, name: &str, suffix: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/zgui-shots/site")
        .join(format!("{name}-{suffix}.png"));
    window.save_png(&path).expect("the window is drawn to a PNG");
    println!("{}", path.display());
    path
}
