//! A replay directory's tab offers no pin until the stored pins are read.
//! Its own binary: the stored pins are read once per process.

use std::path::PathBuf;

use nsh_core::Points;
use nsh_core::Size;
use wows_toolkit_viewmodel::settings::keys;

#[test]
fn pinning_waits_for_the_stored_pins() {
    let storage = std::env::temp_dir().join(format!("wows-toolkit-nsh-pins-settings-{}", std::process::id()));
    let directory = std::env::temp_dir().join(format!("wows-toolkit-nsh-pins-listing-{}", std::process::id()));
    for path in [&storage, &directory] {
        let _ = std::fs::remove_dir_all(path);
        std::fs::create_dir(path).expect("a test directory is created");
    }
    wows_toolkit_config::storage_override::set(storage.clone());
    tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a database runtime starts").block_on(
        async {
            let pool = wows_toolkit_config::open_db().await.expect("the test database opens");
            for key in [keys::BUILD_CONSENT_SHOWN, keys::REPLAY_CONSENT_SHOWN, keys::LANGUAGE_SELECTION_SHOWN] {
                wows_toolkit_config::queries::set_setting_raw(&pool, key, "true")
                    .await
                    .expect("a question is answered");
            }
            wows_toolkit_config::queries::set_setting_raw(&pool, keys::CHECK_FOR_UPDATES, "false")
                .await
                .expect("the startup update check is off");
            // A row the startup read cannot decode, so the stored pins are never read.
            wows_toolkit_config::queries::set_setting_raw(
                &pool,
                wows_toolkit_replay_data::directories::PINNED_DIRECTORIES_KEY,
                "not json",
            )
            .await
            .expect("the unreadable pins are stored");
        },
    );

    let entry = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ui/App.tsx");
    let mut app =
        nsh_testing::tsx(&entry, vec![nsh_base::registry(), wows_toolkit_nsh::registry()]).expect("the TSX app starts");
    app.set_viewport(Size::new(Points(1400.0), Points(1000.0))).expect("the desktop viewport is applied");
    app.set_environment("desktop", |environment| {
        environment.os = nsh_core::platform::Os::Linux;
        environment.idiom = nsh_core::platform::Idiom::Desktop;
    })
    .expect("the desktop environment is applied");

    app.on_pick_paths(vec![directory.clone()]).unwrap();
    app.press("label:Open").unwrap();
    app.press("Open Replay Directory...").unwrap();
    app.wait_for("label:/^Close directory /").unwrap();

    app.right_click("label:/^Close directory /").unwrap();
    app.wait_for("Pin directory").unwrap();
    assert!(!app.screen().enabled("Pin directory").unwrap(), "a pin waits for the stored pins");
    assert!(app.screen().enabled("Close directory").unwrap(), "an unpinned directory still closes");

    let _ = std::fs::remove_dir_all(&directory);
    let _ = std::fs::remove_dir_all(&storage);
}
