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
