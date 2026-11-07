//! Headless runs of the app root on the real nsh engine (no window, no QuickJS).

use std::time::Duration;

use nsh_core::Size;
use nsh_core::platform::Idiom;
use nsh_core::platform::Os;
use nsh_core::prelude::*;
use nsh_engine::EngineConfig;
use nsh_engine::Root;
use nsh_testing::Headless;

use wows_toolkit_nsh::App;

fn start() -> Headless {
    let mut config =
        EngineConfig { viewport: Size::new(px(1400.0), px(1000.0)), ..EngineConfig::new(Root::of::<App>()) };
    // A desktop with a mouse, as the GTK build runs.
    config.environment.os = Os::Linux;
    config.environment.idiom = Idiom::Desktop;

    let mut app = Headless::start(config).expect("the engine starts");
    app.first_render().expect("the first frame renders");

    app
}

#[test]
fn replay_table_counts_rows_and_sorts() {
    let mut app = start();

    app.wait_for("Rows: 5000 (29 replay files)").unwrap();
    app.wait_for("First row: PASB017-Montana-1945").unwrap();

    // The DataTable's sort button cycles ascending, descending; Montana is
    // also the smallest ship key, so the descending press changes the first row.
    app.press("id:sort:ship").unwrap();
    app.wait_for("First row: PASB017-Montana-1945").unwrap();
    app.press("id:sort:ship").unwrap();
    app.wait_for("First row: PZSC718-Narai").unwrap();

    // The first visible cell follows the sort.
    assert!(app.shows("PZSC718-Narai"), "texts: {:?}", app.screen().texts());

    // Every row now shows Narai, so the cell text needs an index; the press
    // reaches the table's onRowPress and opens the detail panel.
    app.press("PZSC718-Narai >> nth=2").unwrap();
    app.wait_for("Row id 79").unwrap();
}

#[test]
fn chart_and_frames_tabs_mount() {
    let mut app = start();

    app.press("Chart").unwrap();
    app.wait_for("Hover index: none").unwrap();
    app.hover("label:Series chart").unwrap();
    // The plot's middle; the exact row depends on the axis label room.
    app.wait_until("a hovered row", Duration::from_secs(5), |screen| {
        screen.texts().iter().any(|text| text.starts_with("Hover index: 24"))
    })
    .unwrap();

    app.press("Frames").unwrap();
    app.wait_for("native").unwrap();
}
