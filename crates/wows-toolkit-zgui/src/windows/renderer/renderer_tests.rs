//! The renderer's transport and the watched playback, driven through their controls.

use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use landaire_ui::Scheme;
use landaire_ui::ThemeChoice;
use landaire_ui::ThemeRootProps;
use landaire_ui::ToastHostProps;
use landaire_ui::test_support::Element;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use wows_minimap_renderer::viewport::MAX_ZOOM;
use wows_minimap_renderer::viewport::MIN_ZOOM;
use wows_minimap_renderer::viewport::MapViewport;
use wows_replay_insights::timeline::TimelineEvent;
use wows_replay_insights::timeline::TimelineEventKind;
use wows_replays::types::ElapsedClock;
use wows_replays::types::GameClock;
use wows_toolkit_replay_data::replay_render::export::ExportProgress;
use wows_toolkit_replay_data::replay_render::export::ExportStage;
use wows_toolkit_replay_data::replay_render::playback::DEFAULT_SPEED;
use wows_toolkit_replay_data::replay_render::playback::Playback;
use wows_toolkit_replay_data::replay_render::playback::speed_label;
use wows_toolkit_replay_data::replay_render::timeline::BattleEvents;
use wows_toolkit_replay_data::replay_render::track::DEFAULT_MAP_ORIGIN;
use wows_toolkit_replay_data::replay_render::track::FrameIndex;
use wows_toolkit_replay_data::replay_render::track::Track;
use wows_toolkit_services::collab::CollabLink;
use wows_toolkit_services::collab::SharedWindow;
use wows_toolkit_services::collab::push_ping_aged;
use wowsunpack::data::Version;
use wt_collab_client::PlaybackFrame;
use zgui::component;
use zgui::geom::DevicePx;
use zgui::geom::Point;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::WithUntracked;
use zgui::view;
use zgui::view::IntoView;
use zgui::view::ShowProps;
use zgui::vocab::Modifiers;
use zgui::vocab::NamedKey;
use zgui::vocab::Role;

use crate::services::Services;
use crate::windows::renderer::session::RendererSession;
use crate::windows::renderer::view::ReplayRendererProps;
use crate::windows::renderer::watched::WatchedPlaybackProps;

/// Wide enough that the whole transport is on one row.
const VIEWPORT: WindowSize = WindowSize { width: 1400.0, height: 700.0 };

/// A track holding `clocks`' worth of empty frames, with the battle starting at the first.
fn track(clocks: Vec<f32>) -> Track {
    Track {
        frames: vec![Vec::new(); clocks.len()],
        clocks: clocks.into_iter().map(GameClock).collect(),
        battle_start: GameClock(0.0),
        battle_end: None,
        space_size: 1400.0,
        version: Version::base(99, 0, 0),
        space: String::new(),
        map_origin: DEFAULT_MAP_ORIGIN,
    }
}

/// Half-second frames, as a bake produces, over `seconds` of recording.
fn half_second_frames(seconds: usize) -> Vec<f32> {
    (0..seconds * 2).map(|frame| frame as f32 * 0.5).collect()
}

/// A viewport on `track` in the window's theme and message host, as a secondary window shows one.
#[component]
fn Viewport(track: Rc<RefCell<Option<Track>>>, opened: Rc<Cell<Option<RendererSession>>>) -> impl IntoView {
    Services::provide(None);
    let track = track.borrow_mut().take().expect("the viewport is built once");
    let session = RendererSession::ready_for_test(track);
    opened.set(Some(session));
    let choice = Signal::stored_local(ThemeChoice::Fixed(Scheme::Dark));

    view! {
        ThemeRoot(choice = choice) {
            ToastHost {
                ReplayRenderer(session = session)
            }
        }
    }
}

fn open_viewport(track: Track) -> (HeadlessWindow, RendererSession) {
    let track = Rc::new(RefCell::new(Some(track)));
    let opened = Rc::new(Cell::new(None));
    let mounted = Rc::clone(&opened);
    let window = HeadlessWindow::open(HeadlessOptions::new(VIEWPORT), move || {
        let (track, opened) = (Rc::clone(&track), Rc::clone(&mounted));
        view! { Viewport(track = track, opened = opened) }
    })
    .expect("the viewport opens headlessly");
    let session = opened.get().expect("the viewport was built");

    (window, session)
}

fn find(window: &HeadlessWindow, id: &str) -> Element {
    window.by_test_id(id).unwrap_or_else(|| panic!("{id} is drawn"))
}

fn click(window: &mut HeadlessWindow, id: &str) {
    let element = find(window, id);
    window.click(element).unwrap_or_else(|err| panic!("{id} takes a click: {err}"));
    window.advance(Duration::from_millis(16));
}

fn is_disabled(window: &mut HeadlessWindow, id: &str) -> bool {
    let element = find(window, id);
    window.a11y(element).unwrap_or_else(|| panic!("{id} is in the accessibility tree")).disabled
}

fn at(session: RendererSession) -> usize {
    session.playback.with_untracked(Playback::at).get()
}

/// Puts the keyboard on the viewport, as a reader does by clicking the map.
fn focus_viewport(window: &mut HeadlessWindow) {
    click(window, "replay-renderer-viewport");
}

/// Events at the battle clocks given.
fn events_at(at: &[f32]) -> BattleEvents {
    let events = at
        .iter()
        .map(|seconds| TimelineEvent {
            clock: ElapsedClock(*seconds),
            kind: TimelineEventKind::AdvantageChanged { label: format!("at {seconds}"), is_friendly: true },
        })
        .collect();

    BattleEvents::from_read(events, None, HashMap::new())
}

#[test]
fn the_transport_controls_are_drawn_and_take_a_click() {
    let (mut window, session) = open_viewport(track(half_second_frames(60)));

    for control in [
        "replay-renderer-play",
        "replay-renderer-jump-to-start",
        "replay-renderer-jump-to-end",
        "replay-renderer-back-10s",
        "replay-renderer-forward-10s",
        "replay-renderer-previous-event",
        "replay-renderer-next-event",
        "replay-renderer-export",
        "replay-renderer-clipboard",
        "replay-renderer-pop-out",
        "replay-renderer-zoom-reset",
        "replay-renderer-speed",
        "replay-renderer-clock",
    ] {
        assert!(window.by_test_id(control).is_some(), "{control} is drawn");
    }

    click(&mut window, "replay-renderer-jump-to-end");
    assert_eq!(at(session), 119, "the end of the track");

    click(&mut window, "replay-renderer-back-10s");
    assert_eq!(at(session), 99, "ten seconds is twenty half-second frames");

    click(&mut window, "replay-renderer-forward-10s");
    assert_eq!(at(session), 119);

    click(&mut window, "replay-renderer-jump-to-start");
    assert_eq!(at(session), 0);

    click(&mut window, "replay-renderer-play");
    assert!(session.playback.with_untracked(Playback::is_playing), "the play control starts playback");
}

/// The transport is two rows, with the scrubber above the controls, as a video player lays them
/// out: a bar sharing a row with a dozen controls is hard to aim at and hard to read.
#[test]
fn the_scrubber_has_a_row_above_the_controls() {
    let (window, _session) = open_viewport(track(half_second_frames(60)));

    let scrubber = window.bounds(find(&window, "replay-renderer-scrubber")).expect("the scrubber has a box");
    let play = window.bounds(find(&window, "replay-renderer-play")).expect("the play control has a box");

    assert!(
        scrubber.origin.y.0 + scrubber.size.height.0 <= play.origin.y.0,
        "the scrubber row ends before the controls begin: {scrubber:?} then {play:?}"
    );
    assert!(scrubber.size.width.0 > play.size.width.0 * 4.0, "a row of its own: {scrubber:?}");
}

/// A press on the middle of the scrubber lands in the middle of the track. A slider built over a
/// fraction with a whole-number step could only reach the two ends.
#[test]
fn a_scrubber_press_lands_on_the_frame_under_it() {
    let (mut window, session) = open_viewport(track((0..40).map(|frame| frame as f32 * 5.0).collect()));

    let scrubber = window.bounds(find(&window, "replay-renderer-scrubber")).expect("the scrubber has a box");
    let middle = Point::new(
        DevicePx(scrubber.origin.x.0 + scrubber.size.width.0 / 2.0),
        DevicePx(scrubber.origin.y.0 + scrubber.size.height.0 / 2.0),
    );
    window.click_at(middle);
    window.advance(Duration::from_millis(16));

    let landed = at(session);
    assert!((15..=24).contains(&landed), "the middle of the track is reachable, landed on {landed}");
}

/// The bar follows the transport's own controls: a jump that left the thumb behind would read as
/// not having worked.
#[test]
fn a_jump_takes_the_scrubber_with_it() {
    let (mut window, _session) = open_viewport(track((0..40).map(|frame| frame as f32 * 5.0).collect()));
    // How far along its slider the scrubber's thumb sits, as a fraction. The scrubber is the first
    // slider and its thumb the first, since its row is above the controls.
    let thumb = |window: &mut HeadlessWindow| {
        let slider = *window.by_role(Role::Slider).first().expect("the scrubber is a slider");
        let thumb = window.by_class("zui-slider__thumb").expect("the scrubber has a thumb");
        let slider = window.bounds(slider).expect("the scrubber has a box");
        let thumb = window.bounds(thumb).expect("the thumb has a box");
        let centre = thumb.origin.x.0 + thumb.size.width.0 / 2.0;
        (centre - slider.origin.x.0) / slider.size.width.0
    };

    click(&mut window, "replay-renderer-jump-to-end");
    let at_end = thumb(&mut window);
    assert!(at_end > 0.9, "the thumb is at the end of the bar: {at_end}");

    click(&mut window, "replay-renderer-jump-to-start");
    let at_start = thumb(&mut window);
    assert!(at_start < 0.1, "and back at its start: {at_start}");
}

/// The event controls refuse a click until the battle has been read, rather than doing nothing
/// and looking broken.
#[test]
fn the_event_controls_refuse_until_the_battle_has_been_read() {
    let (mut window, session) = open_viewport(track(half_second_frames(60)));

    assert!(is_disabled(&mut window, "replay-renderer-next-event"), "refused while nothing is read");
    assert!(is_disabled(&mut window, "replay-renderer-previous-event"));
    click(&mut window, "replay-renderer-next-event");
    assert_eq!(at(session), 0, "and clicking it moves nothing");

    session.events.set(events_at(&[10.0, 30.0]));
    window.advance(Duration::from_millis(16));
    assert!(!is_disabled(&mut window, "replay-renderer-next-event"), "it takes a click once the battle is read");
    assert!(is_disabled(&mut window, "replay-renderer-previous-event"), "nothing happened before the start");

    click(&mut window, "replay-renderer-next-event");
    assert_eq!(at(session), 20, "which steps to the first thing that happened");
    let said = window.texts_by_class("lui-toasts").concat();
    assert!(said.contains("at 10"), "and says what happened there: {said:?}");
    click(&mut window, "replay-renderer-next-event");
    assert_eq!(at(session), 60);
    click(&mut window, "replay-renderer-previous-event");
    assert_eq!(at(session), 20);
}

/// Shift with an arrow walks the events as the buttons do, and says where it landed.
#[test]
fn shift_arrows_walk_the_events_and_say_where() {
    let (mut window, session) = open_viewport(track(half_second_frames(60)));
    session.events.set(events_at(&[10.0, 30.0]));
    focus_viewport(&mut window);

    window.press_key(NamedKey::ArrowRight, Modifiers::SHIFT);
    window.advance(Duration::from_millis(16));

    assert_eq!(at(session), 20, "the first event");
    let said = window.texts_by_class("lui-toasts").concat();
    assert!(said.contains("at 10"), "is said: {said:?}");
}

/// The pop-out control is offered while the viewport is in a tab and withdrawn once it has a
/// window of its own, so a popped-out viewport cannot be popped out again.
#[test]
fn the_pop_out_control_is_withdrawn_once_the_viewport_has_a_window() {
    let (mut window, session) = open_viewport(track(vec![0.0, 30.0]));
    assert!(window.by_test_id("replay-renderer-pop-out").is_some(), "a viewport in a tab offers it");

    session.mark_popped_out();
    window.advance(Duration::from_millis(16));

    assert!(window.by_test_id("replay-renderer-pop-out").is_none(), "one with a window of its own does not");
    assert!(window.by_test_id("replay-renderer-play").is_some(), "only that one control went");
}

/// The transport's keys reach playback: space plays, the arrows seek ten seconds and walk the
/// speed ladder, and the speed dropdown follows rather than showing the speed before last.
#[test]
fn the_keys_drive_the_transport_and_the_speed_dropdown_follows() {
    let (mut window, session) = open_viewport(track(half_second_frames(60)));
    // The speed the dropdown has selected, read off its one selected option with the list open.
    // The keyboard goes back to the viewport afterwards.
    let shown_speed = |window: &mut HeadlessWindow| {
        click(window, "replay-renderer-speed");
        let selected: Vec<String> = window
            .by_role(Role::ListBoxOption)
            .into_iter()
            .filter_map(|option| window.a11y(option))
            .filter(|option| option.selected == Some(true))
            .filter_map(|option| option.label)
            .collect();
        window.press_key(NamedKey::Escape, Modifiers::NONE);
        // Long enough for the list's closing animation.
        window.advance(Duration::from_millis(500));
        let combo = *window.by_role(Role::ComboBox).first().expect("the speed control is a dropdown");
        assert_eq!(window.a11y(combo).and_then(|state| state.expanded), Some(false), "the list closes again");
        focus_viewport(window);
        assert_eq!(selected.len(), 1, "one speed is selected: {selected:?}");
        selected.into_iter().next()
    };
    assert_eq!(session.playback.with_untracked(Playback::speed), DEFAULT_SPEED);
    assert_eq!(shown_speed(&mut window), Some(speed_label(DEFAULT_SPEED)), "it opens on the speed the viewport does");

    focus_viewport(&mut window);
    window.press_key(NamedKey::ArrowRight, Modifiers::NONE);
    window.advance(Duration::from_millis(16));
    assert_eq!(at(session), 20, "the right arrow skips ten seconds");
    window.press_key(NamedKey::ArrowLeft, Modifiers::NONE);
    window.advance(Duration::from_millis(16));
    assert_eq!(at(session), 0, "and the left arrow back");

    window.press_key(NamedKey::ArrowUp, Modifiers::NONE);
    window.advance(Duration::from_millis(16));
    assert_eq!(session.playback.with_untracked(Playback::speed), 40.0);
    assert_eq!(shown_speed(&mut window), Some(speed_label(40.0)), "the dropdown follows the keys");
    for _ in 0..8 {
        window.press_key(NamedKey::ArrowDown, Modifiers::NONE);
    }
    window.advance(Duration::from_millis(16));
    assert_eq!(session.playback.with_untracked(Playback::speed), 1.0, "the ladder stops at its bottom");
    assert_eq!(shown_speed(&mut window), Some(speed_label(1.0)));

    window.press_key(NamedKey::Space, Modifiers::NONE);
    window.advance(Duration::from_millis(16));
    assert!(session.playback.with_untracked(Playback::is_playing), "space plays");
    window.press_key(NamedKey::Space, Modifiers::NONE);
    window.advance(Duration::from_millis(16));
    assert!(!session.playback.with_untracked(Playback::is_playing), "and pauses");
}

/// Playing walks the track on the window's clock and stops at its end, and the control then
/// offers to play again.
#[test]
fn playing_walks_the_track_and_stops_at_its_end() {
    let (mut window, session) = open_viewport(track(vec![0.0, 30.0, 61.0]));

    click(&mut window, "replay-renderer-play");
    assert_eq!(
        window.a11y(find(&window, "replay-renderer-play")).and_then(|state| state.label),
        Some(rust_i18n::t!("ui.replay.renderer.pause").into_owned()),
        "a playing transport offers to pause"
    );
    window.advance(Duration::from_secs(5));

    assert_eq!(at(session), 2, "the last frame");
    assert!(!session.playback.with_untracked(Playback::is_playing), "and it stops there");
    assert_eq!(
        window.a11y(find(&window, "replay-renderer-play")).and_then(|state| state.label),
        Some(rust_i18n::t!("ui.replay.renderer.play").into_owned())
    );
}

/// The zoom control moves the view, and Reset puts the whole map back and is refused while the
/// whole map is already showing.
#[test]
fn the_zoom_control_and_reset_agree_with_the_view() {
    let (mut window, session) = open_viewport(track(vec![0.0, 0.5]));
    assert!(is_disabled(&mut window, "replay-renderer-zoom-reset"), "nothing to reset at the whole map");

    let sliders = window.by_role(Role::Slider);
    let zoom = *sliders.get(1).expect("the zoom control is the second slider");
    let zoom = window.bounds(zoom).expect("the zoom control has a box");
    window.click_at(Point::new(
        DevicePx(zoom.origin.x.0 + zoom.size.width.0 / 2.0),
        DevicePx(zoom.origin.y.0 + zoom.size.height.0 / 2.0),
    ));
    window.advance(Duration::from_millis(16));
    let zoomed = session.map.with_untracked(|map| map.view().zoom());
    let middle = (MIN_ZOOM + MAX_ZOOM) / 2.0;
    assert!((zoomed - middle).abs() < 1.0, "the middle of the control is the middle of its range: {zoomed}");
    assert!(!is_disabled(&mut window, "replay-renderer-zoom-reset"));

    click(&mut window, "replay-renderer-zoom-reset");
    assert_eq!(session.map.with_untracked(|map| map.view()), MapViewport::default());
    assert!(is_disabled(&mut window, "replay-renderer-zoom-reset"));
}

/// An export says how far it has got over the video, and the transport refuses while it runs:
/// the export holds the renderer the transport draws through.
#[test]
fn an_export_reports_its_progress_over_the_video() {
    let (mut window, session) = open_viewport(track(vec![0.0, 30.0]));
    assert!(window.by_test_id("replay-renderer-export-overlay").is_none(), "nothing is being exported");
    assert!(!is_disabled(&mut window, "replay-renderer-play"));

    session.export.update(|export| {
        export.progress = Some(ExportProgress { done: 0, total: 0, stage: ExportStage::Encoding });
    });
    window.advance(Duration::from_millis(16));
    assert!(window.by_test_id("replay-renderer-export-overlay").is_some(), "the wait for the encoder is shown");
    assert!(
        window.by_label(&rust_i18n::t!("ui.renderer.preparing_export")).is_some(),
        "and named while there is nothing to measure"
    );

    session.export.update(|export| {
        export.progress = Some(ExportProgress { done: 40, total: 100, stage: ExportStage::Encoding });
    });
    window.advance(Duration::from_millis(16));
    assert!(window.by_test_id("replay-renderer-export-overlay").is_some(), "the bar is over the video");
    for control in
        ["replay-renderer-play", "replay-renderer-export", "replay-renderer-clipboard", "replay-renderer-jump-to-end"]
    {
        assert!(is_disabled(&mut window, control), "{control} refuses while the export runs");
    }

    session.export.update(|export| {
        export.progress = None;
        export.failure = Some("the encoder stopped".to_owned());
    });
    window.advance(Duration::from_millis(16));
    assert!(window.by_test_id("replay-renderer-export-overlay").is_none());
    assert!(window.by_test_id("replay-renderer-export-failure").is_some(), "a failed export says why");
    assert!(!is_disabled(&mut window, "replay-renderer-play"));
}

/// A session with peers redraws on its own: a peer's pointer and pings move while nothing here is
/// touched, and the tick shedding a finished ping is what shows it. Handing back an inert link
/// stops it.
#[test]
fn a_running_session_redraws_without_being_touched() {
    let (mut window, session) = open_viewport(track(vec![0.0, 30.0]));
    // The lock's type comes from `push_ping_aged`, which spares naming the crate that provides it.
    let state = Arc::new(Default::default());
    push_ping_aged(&state, [10.0, 10.0], Duration::from_secs(5));
    push_ping_aged(&state, [20.0, 20.0], Duration::ZERO);
    let (link, _sent) = CollabLink::for_test(Arc::clone(&state));

    session.set_collab(link);
    assert_eq!(state.lock().pings.len(), 2, "both are there before the session has ticked");
    let before = session.collab_frame.get_untracked();
    window.advance(Duration::from_millis(200));

    assert!(session.collab_frame.get_untracked() > before, "the viewport redrew");
    {
        let held = state.lock();
        assert_eq!(held.pings.len(), 1, "the finished ripple is shed");
        assert_eq!(held.pings[0].pos, [20.0, 20.0], "and the one still running is kept");
    }

    session.set_collab(CollabLink::default());
    push_ping_aged(&state, [30.0, 30.0], Duration::from_secs(5));
    window.advance(Duration::from_millis(200));
    assert_eq!(state.lock().pings.len(), 2, "nothing is shed once the session has gone");
}

/// The start of the track is where a viewport opens.
#[test]
fn a_viewport_opens_on_the_first_frame() {
    let (_window, session) = open_viewport(track(half_second_frames(10)));

    assert_eq!(session.playback.with_untracked(Playback::at), FrameIndex::FIRST);
    assert!(!session.playback.with_untracked(Playback::is_playing));
}

/// A watched window, mounted while `shown` holds, over a session the test feeds frames into.
#[component]
fn Watching(shared: SharedWindow, link: CollabLink, shown: Rc<Cell<Option<RwSignal<bool>>>>) -> impl IntoView {
    Services::provide(None);
    let mounted = RwSignal::new(true);
    shown.set(Some(mounted));

    view! {
        Show(when = move || mounted.get()) {
            WatchedPlayback(shared = shared.clone(), link = link.clone())
        }
    }
}

fn shared_window(replay_id: u64) -> SharedWindow {
    SharedWindow {
        replay_id,
        replay_name: "watched".to_owned(),
        map: "Ocean".to_owned(),
        map_name: "00_CO_ocean".to_owned(),
        art_png: None,
    }
}

fn host_frame(replay_id: u64, seconds: f32, frame_index: usize) -> PlaybackFrame {
    PlaybackFrame {
        replay_id,
        commands: Vec::new(),
        clock: GameClock(seconds),
        frame_index,
        total_frames: 100,
        game_duration: 1200.0,
    }
}

/// A watched window waits for the host, then reads the host's clock off the newest frame, and
/// tells the session to stop sending once it closes.
#[test]
fn a_watched_window_follows_the_hosts_clock() {
    const REPLAY: u64 = 77;
    // The lock's type comes from `for_test`, which spares naming the crate that provides it.
    let state = Arc::new(Default::default());
    let (link, _sent) = CollabLink::for_test(Arc::clone(&state));
    let shown = Rc::new(Cell::new(None));
    let mounted = Rc::clone(&shown);
    let mut window = HeadlessWindow::open(HeadlessOptions::new(VIEWPORT), move || {
        let (shared, link, shown) = (shared_window(REPLAY), link.clone(), Rc::clone(&mounted));
        view! { Watching(shared = shared, link = link, shown = shown) }
    })
    .expect("the watched window opens headlessly");
    let shown = shown.get().expect("the window was built");

    assert!(state.lock().viewport_sinks.contains_key(&REPLAY), "the session is asked for the frames");
    assert!(window.by_test_id("watched-playback-waiting").is_some(), "it waits for the host");
    assert!(window.by_test_id("watched-playback-clock").is_none(), "with no clock to read yet");

    {
        let mut held = state.lock();
        held.push_frame(REPLAY, host_frame(REPLAY, 30.0, 10));
        held.push_frame(REPLAY, host_frame(REPLAY, 75.0, 50));
    }
    window.advance(Duration::from_millis(120));

    let time = find(&window, "watched-playback-clock-time");
    assert_eq!(window.a11y(time).and_then(|state| state.label).as_deref(), Some("1:15"), "the newest frame wins");
    assert!(window.by_test_id("watched-playback-progress").is_some(), "the host said how long the battle is");
    assert!(window.by_test_id("watched-playback-waiting").is_some(), "with no build to draw with, it still waits");

    shown.set(false);
    window.advance(Duration::from_millis(16));
    assert!(!state.lock().viewport_sinks.contains_key(&REPLAY), "a closed window stops the frames");
}
