//! Icons drawn in a view: a Rust view's `icon` and the TSX `Icon`.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use nsh_core::Color;
use nsh_core::MountBatch;
use nsh_core::MountItem;
use nsh_core::canvas::CanvasOp;
use nsh_core::canvas::Paint;
use nsh_core::element::Root;
use nsh_core::prelude::*;
use nsh_engine::EngineConfig;
use nsh_testing::Headless;
use wows_toolkit_nsh::icons::Icon;
use wows_toolkit_nsh::icons::icon;

/// The text color the views around the icons set.
const TEXT_COLOR: Color = Color(0x00ff_00ff);

/// The color the colored TSX icon is given.
const OWN_COLOR: Color = Color(0xff00_00ff);

/// The color the filled TSX icon is given.
const FILL_COLOR: Color = Color(0x0000_ffff);

/// One color a canvas drawing paints with.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Painted {
    Stroke(Color),
    Fill(Color),
}

/// The paints of each canvas drawing mounted.
type Paints = Arc<Mutex<Vec<Vec<Painted>>>>;

/// Records the paints of every canvas drawing `app` mounts from now on.
fn record_paints(app: &mut Headless) -> Paints {
    let paints = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&paints);
    app.observe(move |_step: &str, batch: &MountBatch| {
        let mut sink = sink.lock().expect("the paint record is not poisoned");
        for item in &batch.items {
            let MountItem::Canvas { drawing, .. } = item else { continue };
            let painted = drawing
                .ops
                .iter()
                .filter_map(|op| match op {
                    CanvasOp::StrokeStyle(Paint::Color(color)) => Some(Painted::Stroke(*color)),
                    CanvasOp::FillStyle(Paint::Color(color)) => Some(Painted::Fill(*color)),
                    _ => None,
                })
                .collect();
            sink.push(painted);
        }
    });

    paints
}

fn painted(paints: &Paints, paint: Painted) -> bool {
    paints.lock().expect("the paint record is not poisoned").iter().any(|drawing| drawing.contains(&paint))
}

#[component]
fn RustIcon() -> impl IntoElement {
    view().text_color(rgb(0x00ff00)).child(icon(Icon::Search, px(16.0)).label("Search"))
}

#[test]
fn a_rust_icon_strokes_in_the_text_color() {
    let mut app = Headless::start(EngineConfig::new(Root::of::<RustIcon>())).expect("the engine starts");
    let paints = record_paints(&mut app);

    app.first_render().expect("the view renders");
    app.wait_for("label:Search").expect("the icon is on screen");

    assert!(painted(&paints, Painted::Stroke(TEXT_COLOR)), "the icon strokes in the text color: {paints:?}");
}

#[test]
fn a_tsx_icon_paints_in_its_own_color_or_the_text_color() {
    let registries = vec![nsh_base::registry(), wows_toolkit_nsh::registry()];
    let entry = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/icons/icon_view.tsx");
    let mut app = nsh_testing::tsx(&entry, registries).expect("the TSX view starts");
    let paints = record_paints(&mut app);

    app.wait_for("label:Search").expect("the uncolored icon is on screen");
    app.wait_for("label:Close").expect("the colored icon is on screen");
    app.wait_for("label:Twitch").expect("the filled icon is on screen");

    assert!(
        painted(&paints, Painted::Stroke(TEXT_COLOR)),
        "an icon without a color strokes in the text color: {paints:?}"
    );
    assert!(painted(&paints, Painted::Stroke(OWN_COLOR)), "an icon with a color strokes in it: {paints:?}");
    assert!(painted(&paints, Painted::Fill(FILL_COLOR)), "a filled icon fills in its color: {paints:?}");
}
