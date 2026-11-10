//! Directory Stats from a replay listing, as a top-level tab after Stats.
//! Its own binary: the game data is reported ready for the whole process.

use std::path::PathBuf;
use std::time::Duration;

use nsh_core::Points;
use nsh_core::Size;
use wows_toolkit_viewmodel::settings::keys;

const REPLAY: &str = "20260223_115252_PZSC718-Narai_s06_Atoll.wowsreplay";
const GONE: Duration = Duration::from_secs(5);

/// `text` as a selector regex matching exactly it: paths would otherwise read as one.
fn exactly(text: &str) -> String {
    let escaped: String =
        text.chars().flat_map(|c| r"\.+*?()|[]{}^$".contains(c).then_some('\\').into_iter().chain([c])).collect();
    format!("/^{escaped}$/")
}

#[test]
fn directory_stats_open_from_the_listing_as_a_tab_after_stats() {
    let storage = std::env::temp_dir().join(format!("wows-toolkit-nsh-dirstats-settings-{}", std::process::id()));
    let directory = std::env::temp_dir().join(format!("wows-toolkit-nsh-dirstats-listing-{}", std::process::id()));
    for path in [&storage, &directory] {
        let _ = std::fs::remove_dir_all(path);
        std::fs::create_dir(path).expect("a test directory is created");
    }
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/replays").join(REPLAY);
    std::fs::copy(fixture, directory.join(REPLAY)).expect("the fixture replay is copied");
    wows_toolkit_config::storage_override::set(storage.clone());
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
    wows_toolkit_nsh::game_data::test_support::report_ready();

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

    app.press("Directory Stats").unwrap();
    let root = wows_toolkit_replay_data::directories::canonical_root(directory.clone());
    let short = wows_toolkit_replay_data::directories::shortened_path(&root);
    let tab = format!("role:tab[{}]", exactly(&format!("{short} Stats")));
    app.wait_for(&tab).unwrap();
    assert!(app.screen().selected(&tab).unwrap(), "the opened stats are shown");

    // Tabs: Replay Inspector, the directory, Launcher, Stats, then the directory's stats.
    app.press("role:tab[Stats]").unwrap();
    assert!(!app.screen().selected(&tab).unwrap());
    app.press("role:tab >> nth=4").unwrap();
    assert!(app.screen().selected(&tab).unwrap(), "the directory's stats follow Stats");

    app.press(&format!("label:{}", exactly(&format!("Close directory stats {short} Stats")))).unwrap();
    app.wait_for_gone(&tab, GONE).unwrap();
    assert!(app.screen().selected("role:tab[Stats]").unwrap(), "closing the shown stats shows Stats");

    let _ = std::fs::remove_dir_all(&directory);
    let _ = std::fs::remove_dir_all(&storage);
}
