//! The watched window, fed frames through a session that has no peer task behind it.
//!
//! The session's lock type comes from `CollabLink::for_test`, which spares naming the crate that
//! provides it.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_minimap_renderer::map_data::MinimapPos;
use wows_replays::types::ElapsedClock;
use wows_replays::types::GameClock;
use wows_toolkit_services::collab::CollabLink;
use wows_toolkit_services::collab::SharedWindow;
use wt_collab_client::PlaybackFrame;
use wt_collab_client::peer::LocalEvent;
use wt_collab_client::protocol::CollabRenderOptions;
use wt_collab_client::protocol::DisplayOptionField;
use zgui::reactive::prelude::GetUntracked;
use zgui::view;

use crate::services::Services;
use crate::windows::renderer::watched::WatchSession;
use crate::windows::renderer::watched::WatchedViewProps;

const VIEWPORT: WindowSize = WindowSize { width: 1000.0, height: 800.0 };
const REPLAY: u64 = 78;

/// A watched window on `link`'s session, and its watch.
fn open(link: CollabLink) -> (HeadlessWindow, WatchSession) {
    let handle = Rc::new(Cell::new(None));
    let built = Rc::clone(&handle);
    let window = HeadlessWindow::open(HeadlessOptions::new(VIEWPORT), move || {
        Services::provide(None);
        let shared = SharedWindow {
            replay_id: REPLAY,
            replay_name: "watched".to_owned(),
            map: "Ocean".to_owned(),
            map_name: "00_CO_ocean".to_owned(),
            art_png: None,
        };
        let watch = WatchSession::open(shared, link.clone(), None);
        built.set(Some(watch));
        view! { WatchedView(watch = watch) }
    })
    .expect("the watched window opens headlessly");
    let watch = handle.get().expect("the watch was opened");

    (window, watch)
}

/// A frame of the host's playback carrying `commands`.
fn frame(commands: Vec<DrawCommand>) -> PlaybackFrame {
    PlaybackFrame {
        replay_id: REPLAY,
        commands,
        clock: GameClock(30.0),
        frame_index: 10,
        total_frames: 100,
        game_duration: 1200.0,
    }
}

fn timer() -> DrawCommand {
    DrawCommand::Timer { time_remaining: Some(600), elapsed: ElapsedClock(42.0) }
}

fn torpedo() -> DrawCommand {
    DrawCommand::Torpedo { pos: MinimapPos { x: 50.0, y: 60.0 }, color: [0, 255, 0] }
}

/// Long enough for the window to follow what the session nudged it about.
fn take_in(window: &mut HeadlessWindow) {
    window.advance(Duration::from_millis(120));
}

fn click(window: &mut HeadlessWindow, test_id: &str) {
    let element = window.by_test_id(test_id).unwrap_or_else(|| panic!("{test_id} is drawn"));
    window.click(element).expect("the element has a box");
    window.advance(Duration::from_millis(50));
}

fn shows_timer(watch: WatchSession) -> bool {
    watch.shown().expect("a frame has arrived").iter().any(|command| matches!(command, DrawCommand::Timer { .. }))
}

fn shows_torpedo(watch: WatchSession) -> bool {
    watch.shown().expect("a frame has arrived").iter().any(|command| matches!(command, DrawCommand::Torpedo { .. }))
}

/// A layer turned off in the watched window's own toggles leaves the host's frames drawn without
/// it, and leaves the other layers drawn.
#[test]
fn a_layer_turned_off_in_the_watched_window_is_not_drawn() {
    let state = Arc::new(Default::default());
    let (link, _sent) = CollabLink::for_test(Arc::clone(&state));
    let (mut window, watch) = open(link);
    state.lock().push_frame(REPLAY, frame(vec![timer(), torpedo()]));
    take_in(&mut window);
    assert!(shows_timer(watch) && shows_torpedo(watch), "both layers are drawn at first");

    click(&mut window, "watched-playback-layers-toggle");
    assert!(window.by_test_id("renderer-opt-team-rosters").is_none(), "a watched frame has no room for the rosters");
    click(&mut window, "renderer-opt-timer");

    assert!(!shows_timer(watch), "the timer is no longer drawn");
    assert!(shows_torpedo(watch), "the torpedoes still are");

    state.lock().push_frame(REPLAY, frame(vec![timer(), torpedo()]));
    take_in(&mut window);
    assert!(!shows_timer(watch), "nor is it on the host's next frame");
}

/// A layer changed here is told to the session, and the host's layers, when they change, are
/// drawn over what was chosen here, as an egui peer's are.
#[test]
fn the_hosts_layers_are_drawn_over_the_watched_windows_own() {
    let state = Arc::new(Default::default());
    let (link, sent) = CollabLink::for_test(Arc::clone(&state));
    let (mut window, watch) = open(link);
    state.lock().push_frame(REPLAY, frame(vec![timer(), torpedo()]));
    take_in(&mut window);

    click(&mut window, "watched-playback-layers-toggle");
    click(&mut window, "renderer-opt-torpedoes");
    assert!(!shows_torpedo(watch), "the torpedoes are turned off here");
    let said: Vec<_> = sent
        .try_iter()
        .filter_map(|event| match event {
            LocalEvent::DisplayToggle(field, value) => Some((field, value)),
            _ => None,
        })
        .collect();
    assert_eq!(said, vec![(DisplayOptionField::ShowTorpedoes, false)], "and the session is told");

    let hosts = CollabRenderOptions { show_timer: false, show_torpedoes: true, ..CollabRenderOptions::default() };
    {
        let mut held = state.lock();
        held.current_render_options = Some(hosts);
        held.render_options_version = 1;
        // What the peer task does as the host's settings land.
        held.repaint_viewport(REPLAY);
    }
    take_in(&mut window);

    assert!(!shows_timer(watch), "the host's timer is off");
    assert!(shows_torpedo(watch), "and its torpedoes are on, over the choice made here");
}

/// A peer the host has locked out of the settings keeps drawing what the host shows, and tells
/// the session nothing, since the host would drop it.
#[test]
fn a_locked_peers_toggle_is_refused() {
    let state = Arc::new(Default::default());
    let (link, sent) = CollabLink::for_test(Arc::clone(&state));
    {
        let mut held = state.lock();
        held.role = wt_collab_client::PeerRole::Peer;
        held.permissions.settings_locked = true;
    }
    let (mut window, watch) = open(link);
    state.lock().push_frame(REPLAY, frame(vec![timer(), torpedo()]));
    take_in(&mut window);

    click(&mut window, "watched-playback-layers-toggle");
    click(&mut window, "renderer-opt-torpedoes");

    assert!(shows_torpedo(watch), "the torpedoes are still drawn");
    assert!(!sent.try_iter().any(|event| matches!(event, LocalEvent::DisplayToggle(..))), "and nothing is sent");
}

/// Frames that arrive faster than the window looks leave it drawing the newest of them, rather
/// than one the host has already moved past.
#[test]
fn the_newest_of_a_burst_of_frames_is_drawn() {
    let state = Arc::new(Default::default());
    let (link, _sent) = CollabLink::for_test(Arc::clone(&state));
    let (mut window, watch) = open(link);

    for frame_index in 1..=6 {
        state.lock().push_frame(REPLAY, PlaybackFrame { frame_index, ..frame(vec![timer()]) });
    }
    take_in(&mut window);

    assert_eq!(watch.clock.get_untracked().map(|clock| clock.frame), Some(6));
}
