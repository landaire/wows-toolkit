use nsh_core::{Points, Size};

fn registries() -> Vec<nsh_core::element::Registry> {
    vec![nsh_base::registry(), wows_toolkit_nsh::registry()]
}

#[cfg(feature = "buck")]
fn app() -> nsh_testing::Headless {
    let path = std::env::var("NSH_BUNDLE_PATH").expect("Buck provides the generated TSX bundle");
    let source = std::fs::read_to_string(path).expect("the generated TSX bundle is readable");
    let config = nsh_engine::EngineConfig {
        ui_sets: registries(),
        ..nsh_engine::EngineConfig::new(nsh_engine::Bundle::source(source))
    };
    let mut app = nsh_testing::Headless::start(config).expect("the TSX app starts");
    app.set_viewport(Size::new(Points(1400.0), Points(1000.0))).expect("the desktop viewport is applied");
    app
}

#[cfg(not(feature = "buck"))]
fn app() -> nsh_testing::Headless {
    let entry = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ui/App.tsx");
    let mut app = nsh_testing::tsx(&entry, registries()).expect("the TSX app starts");
    app.set_viewport(Size::new(Points(1400.0), Points(1000.0))).expect("the desktop viewport is applied");
    app
}

#[test]
fn replay_inspector_opens_and_displays_metadata() {
    const FILE: &str = "20260223_115252_PZSC718-Narai_s06_Atoll.wowsreplay";

    let mut app = app();
    let replay = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/replays").join(FILE);
    app.on_pick_paths(vec![replay]).unwrap();

    app.press("Open Replay File...").unwrap();
    assert_eq!(app.screen().file_requests().len(), 1, "the replay button must show one file dialog");
    app.wait_for(FILE).unwrap();
    app.wait_for("Players (15)").unwrap();
    app.wait_for("Battle").unwrap();
    app.wait_for("PVEBattle").unwrap();

    app.press("Settings").unwrap();
    app.press("Replay Inspector").unwrap();
    app.wait_for(FILE).unwrap();
    app.wait_for("Players (15)").unwrap();

    let invalid = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    app.on_pick_paths(vec![invalid]).unwrap();
    app.press("Open Replay File...").unwrap();
    app.wait_for("text:/^could not parse replay /").unwrap();
}

#[test]
fn top_level_navigation_and_application_menu_work() {
    let mut app = app();

    app.wait_for("Replay Inspector").unwrap();
    app.press("Settings").unwrap();
    app.wait_for(
        "Configure game paths, replay loading, updates, sharing, collaboration, language, theme, and diagnostics.",
    )
    .unwrap();

    app.press("label:Open application menu").unwrap();
    app.wait_for("Tactics Board").unwrap();
    app.press("About WoWs Toolkit").unwrap();
    app.wait_for("Tools for World of Warships replays, game data, statistics, rendering, and collaboration.").unwrap();
    app.press("Close").unwrap();
    app.wait_for_gone(
        "Tools for World of Warships replays, game data, statistics, rendering, and collaboration.",
        std::time::Duration::from_secs(5),
    )
    .unwrap();

    app.press("label:Open application menu").unwrap();
    app.press("Tactics Board").unwrap();
    app.wait_for("Select a map to start a tactics board.").unwrap();
    app.press("Close").unwrap();
    app.wait_for_gone("Select a map to start a tactics board.", std::time::Duration::from_secs(5)).unwrap();
}
