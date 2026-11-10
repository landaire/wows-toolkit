//! The application tab strip over the real TSX bundle: its labels follow a
//! language change at once. Kept apart from the other suites because the
//! language chosen here is saved.

use std::path::PathBuf;
use std::time::Duration;

use nsh_core::Points;
use nsh_core::Size;
use wows_toolkit_viewmodel::settings::keys;

const GONE: Duration = Duration::from_secs(5);

fn configure_test_storage() {
    let path = std::env::temp_dir().join(format!("wows-toolkit-nsh-tabs-settings-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir(&path).expect("the test settings directory is created");
    wows_toolkit_config::storage_override::set(path);
    // The first-run questions would stand in front of every tab.
    tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a database runtime starts").block_on(
        async {
            let pool = wows_toolkit_config::open_db().await.expect("the test database opens");
            for key in [keys::BUILD_CONSENT_SHOWN, keys::REPLAY_CONSENT_SHOWN, keys::LANGUAGE_SELECTION_SHOWN] {
                wows_toolkit_config::queries::set_setting_raw(&pool, key, "true").await.expect("a question is answered");
            }
            wows_toolkit_config::queries::set_setting_raw(&pool, keys::CHECK_FOR_UPDATES, "false")
                .await
                .expect("the startup update check is off");
            wows_toolkit_config::queries::set_setting_raw(&pool, keys::LOCALE, "\"en\"")
                .await
                .expect("the language starts as English");
        },
    );
}

fn start() -> nsh_testing::Headless {
    configure_test_storage();
    let entry = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ui/App.tsx");
    let mut app =
        nsh_testing::tsx(&entry, vec![nsh_base::registry(), wows_toolkit_nsh::registry()]).expect("the TSX app starts");
    app.set_viewport(Size::new(Points(1400.0), Points(1000.0))).expect("the desktop viewport is applied");
    app
}

#[test]
fn a_language_change_relabels_the_tabs_and_the_palette_at_once() {
    let mut app = start();
    app.wait_for("role:tab[Replay Inspector]").unwrap();

    app.press("role:tab[Settings]").unwrap();
    app.press("label:Language").unwrap();
    app.press("Deutsch").unwrap();

    app.wait_for("role:tab[Replay-Inspektor]").unwrap();
    app.wait_for("role:tab[Einstellungen]").unwrap();
    app.wait_for_gone("role:tab[Replay Inspector]", GONE).unwrap();
    assert!(app.screen().selected("role:tab[Einstellungen]").unwrap(), "the Settings tab stays selected");

    app.press_key("mod+k").unwrap();
    app.wait_for("Gehe zu: Einstellungen").unwrap();
    app.wait_for("Replay-Datei manuell öffnen...").unwrap();
}
