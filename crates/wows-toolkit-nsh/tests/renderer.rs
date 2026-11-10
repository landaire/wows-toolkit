//! Headless tests of the replay renderer's controls over the real TSX views.

#![cfg(not(feature = "buck"))]

use nsh_core::{Points, Size};

const FIXTURE: &str = "20260223_115252_PZSC718-Narai_s06_Atoll.wowsreplay";

/// An app showing only the renderer on `replay`, through an entry written for
/// the test: the view takes the replay's path as a prop.
fn renderer_app(replay: &std::path::Path) -> (nsh_testing::Headless, tempfile::TempDir) {
    static STORAGE: std::sync::LazyLock<tempfile::TempDir> =
        std::sync::LazyLock::new(|| tempfile::tempdir().expect("a settings directory is created"));
    wows_toolkit_config::storage_override::set(STORAGE.path().to_owned());

    let ui = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ui/renderer");
    let dir = tempfile::tempdir().expect("a temporary directory");
    let entry = dir.path().join("App.tsx");
    std::fs::write(
        &entry,
        format!(
            "import {{ mount, View }} from \"nsh\";\nimport {{ ReplayRendererView }} from {ui:?};\nmount(() => <View style={{{{ flex: 1 }}}}><ReplayRendererView path={replay:?}/></View>);\n",
            ui = ui.display().to_string(),
            replay = replay.display().to_string(),
        ),
    )
    .expect("the entry is written");
    let mut app = nsh_testing::tsx(&entry, vec![nsh_base::registry(), wows_toolkit_nsh::registry()]).expect("the TSX app starts");
    app.set_viewport(Size::new(Points(1200.0), Points(900.0))).expect("the viewport is applied");
    (app, dir)
}

#[test]
fn a_replay_that_cannot_be_read_says_so_and_refuses_the_transport() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let broken = dir.path().join("broken.wowsreplay");
    std::fs::write(&broken, b"not a replay").expect("the file is written");

    let (mut app, _entry) = renderer_app(&broken);

    app.wait_for("This replay could not be played back").unwrap();
    app.wait_for("text:/the replay could not be read/").unwrap();
    app.wait_for("Play").unwrap();
}

#[test]
fn the_transport_and_popovers_are_offered_for_a_replay() {
    let replay = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/replays").join(FIXTURE);

    let (mut app, _entry) = renderer_app(&replay);

    app.wait_for("Replay renderer").unwrap();
    for control in ["Seek", "Zoom", "Clock", "Jump to start", "Play", "Events"] {
        app.wait_for(control).unwrap();
    }
    app.press("Events").unwrap();
    app.wait_for("Event Timeline").unwrap();
    app.press("Open in its own window").unwrap();
    app.wait_for("Back to the tab").unwrap();
    app.press("Back to the tab").unwrap();
    app.wait_for("Open in its own window").unwrap();
}
