//! The renderer on a real replay, drawn on the graphics device for review.
//!
//! `WT_RENDERER_REPLAY` names the replay; `WT_WOWS_DIR` the install, or the game data dump is read
//! without one.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use landaire_ui::ThemeRootProps;
use landaire_ui::ToastHostProps;
use landaire_ui::test_support::Drawing;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use tokio::runtime::Runtime;
use wows_toolkit_services::collab::CollabLink;
use wows_toolkit_viewmodel::settings::ThemeChoice;
use zgui::component;
use zgui::reactive::Signal;
use zgui::reactive::prelude::WithUntracked;
use zgui::view;
use zgui::view::IntoView;

use crate::interaction_tests::save_shot;
use crate::services::GameDataRequest;
use crate::services::Services;
use crate::theme::SemanticTokensProps;
use crate::theme::palette_choice;
use crate::windows::WindowSurfaceProps;
use crate::windows::renderer::session::RenderRequest;
use crate::windows::renderer::session::RendererSession;
use crate::windows::renderer::session::Status;
use crate::windows::renderer::view::ReplayRendererProps;

/// The main window's size, as a renderer tab is reviewed in.
const TAB_SIZE: WindowSize = WindowSize { width: 1280.0, height: 800.0 };

/// A popped-out renderer's window when nothing was remembered.
const POPPED_OUT_SIZE: WindowSize = WindowSize { width: 1000.0, height: 860.0 };

/// A large window, where a frame drawn at the canvas's own size would be enlarged.
const LARGE_SIZE: WindowSize = WindowSize { width: 2400.0, height: 1600.0 };

/// Eight minutes into the battle, when the ships have spread over the map.
const MID_BATTLE: f32 = 480.0;

/// A renderer on `replay` in a secondary window's theme and message host.
#[component]
fn Shown(
    runtime: Arc<Runtime>,
    replay: PathBuf,
    theme: ThemeChoice,
    popped_out: bool,
    opened: Rc<Cell<Option<RendererSession>>>,
) -> impl IntoView {
    let services = Services::provide(Some(runtime));
    services.game_data.adopt(GameDataRequest {
        wows_dir: std::env::var_os("WT_WOWS_DIR").map(PathBuf::from),
        cache_dir: String::new(),
        locale: None,
        auto_dump: false,
    });
    let session = RendererSession::open(RenderRequest { path: replay, alts: Vec::new() }, CollabLink::default());
    if popped_out {
        session.mark_popped_out();
    }
    opened.set(Some(session));
    let choice = Signal::stored_local(palette_choice(theme));
    let shown = Signal::stored_local(theme);

    view! {
        ThemeRoot(choice = choice) {
            SemanticTokens(choice = shown)
            ToastHost {
                WindowSurface {
                    ReplayRenderer(session = session)
                }
            }
        }
    }
}

/// Opens the renderer on the graphics device, bakes the replay and moves to the middle of the
/// battle. `None` when no replay is named.
fn open(theme: ThemeChoice, size: WindowSize, popped_out: bool) -> Option<(HeadlessWindow, RendererSession)> {
    let Some(replay) = std::env::var_os("WT_RENDERER_REPLAY").map(PathBuf::from) else {
        eprintln!("skipped: WT_RENDERER_REPLAY names no replay");
        return None;
    };
    let runtime = Arc::new(wows_toolkit_services::runtime::build().expect("a tokio runtime starts"));
    let opened = Rc::new(Cell::new(None));
    let mounted = Rc::clone(&opened);
    let options = HeadlessOptions { drawing: Drawing::Device, ..HeadlessOptions::new(size) };
    let mut window = HeadlessWindow::open(options, move || {
        let (runtime, replay, opened) = (Arc::clone(&runtime), replay.clone(), Rc::clone(&mounted));
        view! { Shown(runtime = runtime, replay = replay, theme = theme, popped_out = popped_out, opened = opened) }
    })
    .expect("the renderer opens on the graphics device");
    let session = opened.get().expect("the renderer was built");

    let started = Instant::now();
    while session.status.with_untracked(|status| matches!(status, Status::Baking)) {
        assert!(started.elapsed() < Duration::from_secs(1200), "the bake finishes");
        run_in_real_time(&mut window);
    }
    if let Status::Failed(reason) = session.status.with_untracked(Clone::clone) {
        panic!("the bake failed: {reason}");
    }

    session.go_to_elapsed(MID_BATTLE);
    for _ in 0..10 {
        run_in_real_time(&mut window);
    }

    Some((window, session))
}

/// A tenth of a second of frames, letting work off the UI thread land as it goes.
fn run_in_real_time(window: &mut HeadlessWindow) {
    window.advance(Duration::from_millis(100));
    std::thread::sleep(Duration::from_millis(100));
}

#[test]
#[ignore = "draws a real replay on the graphics device; run by hand to review the renderer"]
fn the_renderer_draws_a_real_replay() {
    for (theme, name) in [(ThemeChoice::Dark, "renderer-dark"), (ThemeChoice::Light, "renderer-light")] {
        let Some((mut window, _session)) = open(theme, TAB_SIZE, false) else { return };
        println!("{}", save_shot(&mut window, name).display());

        let gear = window.by_test_id("replay-renderer-settings-toggle").expect("the gear is drawn");
        window.click(gear).expect("the gear has a box");
        run_in_real_time(&mut window);
        println!("{}", save_shot(&mut window, &format!("{name}-gear")).display());
    }

    let Some((mut window, _session)) = open(ThemeChoice::Dark, POPPED_OUT_SIZE, true) else { return };
    println!("{}", save_shot(&mut window, "renderer-popped-out-dark").display());
}

#[test]
#[ignore = "draws a real replay on the graphics device; run by hand to review the drawing tools"]
fn the_drawing_tools_on_a_real_replay() {
    use wt_collab_client::drawing::Tool;
    use wt_collab_client::types::Annotation;

    let Some((mut window, session)) = open(ThemeChoice::Dark, TAB_SIZE, false) else { return };
    let toggle = window.by_test_id("replay-renderer-tools-toggle").expect("the tools control is drawn");
    window.click(toggle).expect("the tools control has a box");
    run_in_real_time(&mut window);
    let circle = window.by_test_id("replay-renderer-tool-circle").expect("the circle is offered");
    window.click(circle).expect("the circle has a box");
    run_in_real_time(&mut window);
    println!("{}", save_shot(&mut window, "renderer-tools").display());
    window.click(toggle).expect("the tools control has a box");

    session.annotate(|annotator, link| {
        annotator.take_up(Tool::None);
        link.add_annotation(Annotation::Circle {
            center: [300.0, 300.0],
            radius: 40.0,
            color: [230, 50, 50, 255],
            width: 2.0,
            filled: false,
        });
        link.add_annotation(Annotation::Measurement {
            start: [150.0, 550.0],
            end: [600.0, 450.0],
            color: [240, 230, 50, 255],
            width: 2.0,
        });
        annotator.pick_at(link, [340.0, 300.0], false);
    });
    for _ in 0..5 {
        run_in_real_time(&mut window);
    }
    println!("{}", save_shot(&mut window, "renderer-shape-edit").display());
}

#[test]
#[ignore = "draws a real replay on the graphics device; run by hand to review the renderer"]
fn the_renderer_is_sharp_in_a_large_window() {
    let Some((mut window, session)) = open(ThemeChoice::Dark, LARGE_SIZE, false) else { return };
    // A frame drawn at the window's own pixels takes longer than one at the canvas's size.
    let settle = |window: &mut HeadlessWindow| (0..50).for_each(|_| run_in_real_time(window));
    settle(&mut window);
    println!("{}", save_shot(&mut window, "renderer-large").display());

    session.zoom_to(3.0);
    settle(&mut window);
    println!("{}", save_shot(&mut window, "renderer-large-zoomed").display());
}
