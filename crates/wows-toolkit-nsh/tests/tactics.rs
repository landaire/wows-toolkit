use nsh_core::{Points, Size};

fn registries() -> Vec<nsh_core::element::Registry> {
    vec![nsh_base::registry(), wows_toolkit_nsh::registry()]
}

fn start() -> nsh_testing::Headless {
    let entry = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ui/App.tsx");
    nsh_testing::tsx(&entry, registries()).expect("the TSX app starts")
}

fn app() -> nsh_testing::Headless {
    let storage = std::env::temp_dir().join(format!("wows-toolkit-nsh-tactics-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&storage);
    std::fs::create_dir(&storage).expect("the test settings directory is created");
    wows_toolkit_config::storage_override::set(storage);

    let mut app = start();
    app.set_viewport(Size::new(Points(1400.0), Points(1000.0))).expect("the desktop viewport is applied");
    app.set_environment("desktop", |environment| {
        environment.os = nsh_core::platform::Os::Linux;
        environment.idiom = nsh_core::platform::Idiom::Desktop;
    })
    .expect("a desktop");
    app
}

/// The board opens from the application menu over the window, takes up and
/// puts down tools from the toolbar and its shortcuts, and closes again.
#[test]
fn the_board_opens_takes_tools_and_closes() {
    let mut app = app();

    app.press("Open application menu").unwrap();
    app.press("Tactics Board").unwrap();
    // Whether a build loads depends on the machine; the toolbar shows either way.
    app.wait_for("label:Ranges").unwrap();

    app.press("label:Line").unwrap();
    app.wait_for("label:Undo").unwrap();
    app.press_key("escape").unwrap();
    app.press_key("mod+2").unwrap();
    app.press("label:Thicker stroke").unwrap();
    app.wait_for("text:3").unwrap();
    app.press_key("[").unwrap();
    app.wait_for("text:2").unwrap();

    app.press("label:Keyboard Shortcuts").unwrap();
    app.wait_for("Multi-select").unwrap();
    app.close_menu().ok();

    app.press("Close").unwrap();
    app.wait_for_gone("label:Ranges", std::time::Duration::from_secs(5)).unwrap();
}
