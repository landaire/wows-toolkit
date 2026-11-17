//! The watched window, fed frames through a session that has no peer task behind it.
//!
//! The session's lock type comes from `CollabLink::for_test`, which spares naming the crate that
//! provides it.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use landaire_ui::ThemeRootProps;
use landaire_ui::test_support::Drawing;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use tokio::runtime::Runtime;
use wows_minimap_renderer::CANVAS_HEIGHT;
use wows_minimap_renderer::MINIMAP_SIZE;
use wows_minimap_renderer::SavedRenderOptions;
use wows_minimap_renderer::ShipVisibility;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_minimap_renderer::map_data::MinimapPos;
use wows_replays::types::ElapsedClock;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wows_toolkit_replay_data::replay_render::view::FrameSize;
use wows_toolkit_replay_data::replay_render::view::MapView;
use wows_toolkit_services::collab::CollabLink;
use wows_toolkit_services::collab::SharedWindow;
use wows_toolkit_services::collab::push_ping_aged;
use wows_toolkit_viewmodel::settings::ThemeChoice;
use wt_collab_client::AnnotationSyncState;
use wt_collab_client::OpenReplay;
use wt_collab_client::PlaybackFrame;
use wt_collab_client::UserCursor;
use wt_collab_client::peer::LocalEvent;
use wt_collab_client::protocol::CollabRenderOptions;
use wt_collab_client::protocol::DisplayOptionField;
use wt_collab_client::types::Annotation;
use zgui::component;
use zgui::geom::Device;
use zgui::geom::DevicePx;
use zgui::geom::Point;
use zgui::reactive::Signal;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::WithUntracked;
use zgui::view;
use zgui::view::IntoView;
use zgui::view::ShowProps;

use crate::interaction_tests::save_shot;
use crate::services::GameDataRequest;
use crate::services::GameDataState;
use crate::services::Services;
use crate::theme::SemanticTokensProps;
use crate::theme::palette_choice;
use crate::windows::WindowSurfaceProps;
use crate::windows::renderer::watched::WatchSession;
use crate::windows::renderer::watched::WatchedPlaybackProps;
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

/// The wheel zooms the watched map about the pointer, a drag moves it, and a double click puts the
/// whole map back, as in the renderer.
#[test]
fn the_watched_map_zooms_pans_and_resets() {
    let state = Arc::new(Default::default());
    let (link, _sent) = CollabLink::for_test(Arc::clone(&state));
    let (mut window, watch) = open(link);
    // What a drawn frame sets: a pointer is read against the frame's size, HUD strip and all.
    watch.frame_size.set(Some(FrameSize { width: MINIMAP_SIZE, height: CANVAS_HEIGHT }));
    take_in(&mut window);
    let viewport = window.by_test_id("watched-playback-viewport").expect("the map is drawn");

    window.wheel(viewport, -100.0).expect("the map takes the wheel");
    take_in(&mut window);
    let zoomed = watch.map.with_untracked(MapView::view);
    assert!(zoomed.zoom() > 1.0, "turning the wheel toward the reader zooms in: {zoomed:?}");

    let at = window.bounds(viewport).expect("the map has a box");
    let to = Point::<DevicePx, Device>::new(
        DevicePx(at.origin.x.0 + at.size.width.0 / 2.0 + 120.0),
        DevicePx(at.origin.y.0 + at.size.height.0 / 2.0 + 80.0),
    );
    window.drag(viewport, to, 6).expect("the map drags");
    take_in(&mut window);
    let dragged = watch.map.with_untracked(MapView::view);
    assert_ne!(dragged, zoomed, "a drag moves the zoomed map");
    assert_eq!(dragged.zoom(), zoomed.zoom(), "without zooming it");

    window.double_click(viewport).expect("the map takes a double click");
    take_in(&mut window);
    assert!(watch.map.with_untracked(|map| map.view().is_whole_map()), "a double click shows the whole map");
}

/// The peers' pointers and pings are drawn over the watched battle and a pointer that has stood
/// still is taken down, as in the renderer; the watcher's own pointer and pings reach the session.
#[test]
fn peers_are_drawn_over_the_watched_battle_and_the_watcher_is_heard() {
    let state = Arc::new(Default::default());
    let (link, sent) = CollabLink::for_test(Arc::clone(&state));
    state.lock().cursors.push(UserCursor {
        user_id: 9,
        name: "Peer".to_owned(),
        color: [0, 200, 255],
        pos: Some([400.0, 400.0]),
        last_update: std::time::Instant::now(),
    });
    let (mut window, watch) = open(link);
    watch.frame_size.set(Some(FrameSize { width: MINIMAP_SIZE, height: CANVAS_HEIGHT }));
    // After the window opens: a ripple lasts a second of real time.
    push_ping_aged(&state, [300.0, 300.0], Duration::ZERO);
    take_in(&mut window);

    assert!(window.by_test_id("replay-renderer-peer-cursor-9").is_some(), "the peer's pointer is drawn");
    assert!(window.by_test_id("replay-renderer-peer-ping-7").is_some(), "and its ping");
    state.lock().cursors[0].last_update = std::time::Instant::now() - Duration::from_secs(6);
    take_in(&mut window);
    assert!(window.by_test_id("replay-renderer-peer-cursor-9").is_none(), "a pointer still for six seconds is gone");

    let viewport = window.by_test_id("watched-playback-viewport").expect("the map is drawn");
    window.hover(viewport).expect("the map has a box");
    window.middle_click(viewport).expect("the map has a box");
    take_in(&mut window);
    let said: Vec<LocalEvent> = sent.try_iter().collect();
    assert!(
        said.iter().any(|event| matches!(event, LocalEvent::CursorPosition(Some(_)))),
        "the watcher's pointer is told to the session"
    );
    assert!(said.iter().any(|event| matches!(event, LocalEvent::Ping(_))), "and so is its ping");
}

/// What the session has drawn on the map is drawn over the watched battle.
#[test]
fn the_sessions_shapes_are_drawn_over_the_watched_battle() {
    let state = Arc::new(Default::default());
    let (link, _sent) = CollabLink::for_test(Arc::clone(&state));
    let (mut window, watch) = open(link);
    state.lock().push_frame(REPLAY, frame(vec![torpedo()]));
    take_in(&mut window);
    let shapes = |watch: WatchSession| {
        watch
            .shown()
            .expect("a frame has arrived")
            .into_iter()
            .filter(|command| matches!(command, DrawCommand::Annotation { .. }))
            .count()
    };
    assert_eq!(shapes(watch), 0, "nothing is drawn on the map yet");

    {
        let mut held = state.lock();
        held.current_annotation_sync = Some(AnnotationSyncState {
            annotations: vec![Annotation::Circle {
                center: [300.0, 300.0],
                radius: 40.0,
                color: [255, 0, 0, 255],
                width: 3.0,
                filled: false,
            }],
            owners: vec![1],
            ids: vec![5],
        });
        held.annotation_sync_version = 1;
        // What the peer task does as the host's shape lands.
        held.repaint_viewport(REPLAY);
    }
    take_in(&mut window);

    assert_eq!(shapes(watch), 1, "the host's circle is drawn over the battle");
}

/// The watched window takes up what the reader saved as the renderer's defaults, less the gutter
/// layers it has no room for, and the session's display settings, where it holds some, stand over
/// them.
#[test]
fn the_watched_window_takes_up_the_saved_defaults_under_the_sessions() {
    let state = Arc::new(Default::default());
    let (link, _sent) = CollabLink::for_test(Arc::clone(&state));
    let (mut window, watch) = open(link);
    state.lock().push_frame(REPLAY, frame(vec![timer(), torpedo()]));
    take_in(&mut window);
    let saved = SavedRenderOptions { show_timer: false, show_team_rosters: true, ..SavedRenderOptions::default() };

    watch.adopt_defaults(&saved);

    assert!(!shows_timer(watch), "the saved defaults leave the timer off");
    assert!(
        !watch.layers.with_untracked(|layers| layers.options.show_team_rosters),
        "and the rosters, which a watched frame has no room for, stay off"
    );

    {
        let mut held = state.lock();
        held.current_render_options = Some(CollabRenderOptions { show_timer: true, ..CollabRenderOptions::default() });
        held.render_options_version = 1;
        held.repaint_viewport(REPLAY);
    }
    take_in(&mut window);
    watch.adopt_defaults(&saved);

    assert!(shows_timer(watch), "the session's timer stands over the saved defaults");
}

/// A watched window in the dark theme with this machine's game data, for review.
#[component]
fn Reviewed(runtime: Arc<Runtime>, shared: SharedWindow, link: CollabLink) -> impl IntoView {
    let services = Services::provide(Some(runtime));
    services.game_data.adopt(GameDataRequest {
        wows_dir: std::env::var_os("WT_WOWS_DIR").map(PathBuf::from),
        cache_dir: String::new(),
        locale: None,
        auto_dump: false,
    });
    let choice = Signal::stored_local(palette_choice(ThemeChoice::Dark));
    let shown = Signal::stored_local(ThemeChoice::Dark);

    view! {
        ThemeRoot(choice = choice) {
            SemanticTokens(choice = shown)
            WindowSurface {
                // The watch draws with the builds loaded when it opens, so it opens once one has.
                Show(when = move || matches!(services.game_data.state(), GameDataState::Ready(_))) {
                    WatchedPlayback(shared = shared.clone(), link = link.clone())
                }
            }
        }
    }
}

/// A ship of `species` at `(x, y)` on the minimap, friendly or not.
fn reviewed_ship(entity: u32, species: &str, friendly: bool, x: f32, y: f32) -> DrawCommand {
    DrawCommand::Ship {
        entity_id: EntityId::from(entity),
        pos: MinimapPos { x, y },
        yaw: 0.8,
        species: Some(species.to_owned()),
        color: Some(if friendly { [76, 232, 170] } else { [254, 77, 42] }),
        visibility: ShipVisibility::Visible,
        opacity: 1.0,
        is_self: false,
        player_name: None,
        ship_name: None,
        is_detected_teammate: false,
        is_disconnected: false,
        name_color: None,
    }
}

/// A watched battle with its HUD, a peer's pointer, a ping and a drawing over it, drawn on the
/// graphics device for review. Reads the game data dump, or `WT_WOWS_DIR`'s install.
#[test]
#[ignore = "draws on the graphics device with real game data; run by hand to review the watched view"]
fn the_watched_view_draws_the_hud_peers_and_drawings() {
    let runtime = Arc::new(wows_toolkit_services::runtime::build().expect("a tokio runtime starts"));
    let state = Arc::new(Default::default());
    let (link, _sent) = CollabLink::for_test(Arc::clone(&state));
    let shared = SharedWindow {
        replay_id: REPLAY,
        replay_name: "watched".to_owned(),
        map: "Trap".to_owned(),
        map_name: "spaces/28_naval_mission".to_owned(),
        art_png: None,
    };
    state.lock().open_replays.push(OpenReplay {
        replay_id: REPLAY,
        replay_name: shared.replay_name.clone(),
        map_image_png: Vec::new(),
        game_version: String::new(),
        map_name: shared.map_name.clone(),
        display_name: shared.map.clone(),
    });
    let options = HeadlessOptions { drawing: Drawing::Device, ..HeadlessOptions::new(VIEWPORT) };
    let mut window = HeadlessWindow::open(options, move || {
        let (runtime, shared, link) = (Arc::clone(&runtime), shared.clone(), link.clone());
        view! { Reviewed(runtime = runtime, shared = shared, link = link) }
    })
    .expect("the watched window opens on the graphics device");
    let in_real_time = |window: &mut HeadlessWindow| {
        window.advance(Duration::from_millis(100));
        std::thread::sleep(Duration::from_millis(100));
    };
    let started = std::time::Instant::now();
    while window.by_test_id("watched-playback").is_none() {
        assert!(started.elapsed() < Duration::from_secs(600), "the game data loads");
        in_real_time(&mut window);
    }

    let score = DrawCommand::ScoreBar {
        team0: 620,
        team1: 410,
        team0_color: [76, 232, 170],
        team1_color: [254, 77, 42],
        max_score: 1000,
        team0_timer: Some("4:12".to_owned()),
        team1_timer: None,
        advantage: None,
    };
    let ships = vec![
        reviewed_ship(1, "Destroyer", true, 220.0, 520.0),
        reviewed_ship(2, "Cruiser", true, 260.0, 580.0),
        reviewed_ship(3, "Battleship", true, 180.0, 610.0),
        reviewed_ship(4, "Cruiser", false, 520.0, 220.0),
        reviewed_ship(5, "Destroyer", false, 470.0, 300.0),
    ];
    {
        let mut held = state.lock();
        held.push_frame(REPLAY, frame([vec![score, timer()], ships].concat()));
        held.cursors.push(UserCursor {
            user_id: 9,
            name: "Peer".to_owned(),
            color: [0, 200, 255],
            pos: Some([420.0, 360.0]),
            last_update: std::time::Instant::now(),
        });
        held.current_annotation_sync = Some(AnnotationSyncState {
            annotations: vec![Annotation::Circle {
                center: [470.0, 300.0],
                radius: 60.0,
                color: [255, 220, 0, 255],
                width: 3.0,
                filled: false,
            }],
            owners: vec![9],
            ids: vec![1],
        });
        held.annotation_sync_version = 1;
        held.repaint_viewport(REPLAY);
    }
    let started = std::time::Instant::now();
    while window.by_test_id("watched-playback-waiting").is_some() {
        let taken = window.by_test_id("watched-playback-clock").is_some();
        assert!(started.elapsed() < Duration::from_secs(300), "the frame is drawn (taken from the session: {taken})");
        in_real_time(&mut window);
    }
    for _ in 0..3 {
        in_real_time(&mut window);
    }
    // A ripple lasts a second of real time, so it is dropped just before the shot.
    push_ping_aged(&state, [300.0, 520.0], Duration::from_millis(300));
    window.advance(Duration::from_millis(50));

    println!("{}", save_shot(&mut window, "watched-dark").display());
}
