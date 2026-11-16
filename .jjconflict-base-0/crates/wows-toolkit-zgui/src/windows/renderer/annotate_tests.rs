//! The drawing tools, the shortcut sheet and a session's marks, driven through the renderer's own
//! controls.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use rust_i18n::t;
use wows_minimap_renderer::HUD_HEIGHT;
use wows_minimap_renderer::MINIMAP_SIZE;
use wows_replays::types::GameClock;
use wows_toolkit_replay_data::replay_render::annotate::HANDLE_RADIUS;
use wows_toolkit_replay_data::replay_render::annotate::MAX_NIB;
use wows_toolkit_replay_data::replay_render::track::DEFAULT_MAP_ORIGIN;
use wows_toolkit_replay_data::replay_render::track::Track;
use wows_toolkit_replay_data::replay_render::view::FrameSize;
use wows_toolkit_services::collab::CollabLink;
use wows_toolkit_services::collab::push_ping_aged;
use wowsunpack::data::Version;
use wt_collab_client::AnnotationSyncState;
use wt_collab_client::UserCursor;
use wt_collab_client::drawing::Stroke;
use wt_collab_client::drawing::Tool;
use wt_collab_client::types::Annotation;
use zgui::component;
use zgui::geom::Device;
use zgui::geom::DevicePx;
use zgui::geom::Point;
use zgui::geom::Rect;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::WithUntracked;
use zgui::view;
use zgui::view::IntoView;
use zgui::vocab::Modifiers;
use zgui::vocab::NamedKey;

use crate::services::Services;
use crate::windows::renderer::session::RendererSession;
use crate::windows::renderer::view::ReplayRendererProps;

/// A renderer over an empty two-frame battle, with the services a window has.
#[component]
fn Mounted(opened: Rc<Cell<Option<RendererSession>>>) -> impl IntoView {
    Services::provide(None);
    let track = Track {
        frames: vec![Vec::new(); 2],
        clocks: vec![GameClock(0.0), GameClock(30.0)],
        battle_start: GameClock(0.0),
        battle_end: None,
        space_size: 1400.0,
        version: Version::base(99, 0, 0),
        space: String::new(),
        map_origin: DEFAULT_MAP_ORIGIN,
    };
    let session = RendererSession::ready_for_test(track);
    opened.set(Some(session));

    // A tab gives the renderer the room it fills.
    view! {
        column(style:width = "1400px", style:height = "900px") {
            ReplayRenderer(session = session)
        }
    }
}

/// Opens the renderer wide enough that the whole transport is on screen, with a frame placed on
/// the map as a draw would place one.
fn open() -> (HeadlessWindow, RendererSession) {
    let opened = Rc::new(Cell::new(None));
    let mounted = Rc::clone(&opened);
    let mut window =
        HeadlessWindow::open(HeadlessOptions::new(WindowSize { width: 1400.0, height: 900.0 }), move || {
            view! { Mounted(opened = Rc::clone(&mounted)) }
        })
        .expect("the renderer opens headlessly");
    let session = opened.get().expect("the renderer was built");
    session.frame_size.set(Some(FrameSize { width: MINIMAP_SIZE, height: MINIMAP_SIZE + HUD_HEIGHT }));
    settle(&mut window);

    (window, session)
}

fn settle(window: &mut HeadlessWindow) {
    window.advance(Duration::from_millis(300));
}

fn click(window: &mut HeadlessWindow, id: &str) {
    let element = window.by_test_id(id).unwrap_or_else(|| panic!("{id} is drawn"));
    window.click(element).unwrap_or_else(|err| panic!("{id} takes a click: {err:?}"));
    settle(window);
}

fn tool(session: RendererSession) -> Tool {
    session.annotator.with_untracked(|annotator| annotator.drawing().tool().clone())
}

/// Where a map point lands in the window, in device pixels, through the placement the map element
/// recorded.
fn window_point(session: RendererSession, at: (f32, f32)) -> Point<DevicePx, Device> {
    let origin = session.map_origin.get_untracked();
    let window = session.map.with_untracked(|map| {
        let element = map.element_point(at, origin).expect("the point is on the map");
        map.to_window(element).expect("the map was placed")
    });

    Point::new(DevicePx(window.x), DevicePx(window.y))
}

/// Where an element's top-left corner is, in device pixels.
fn corner(window: &HeadlessWindow, id: &str) -> (f32, f32) {
    let element = window.by_test_id(id).unwrap_or_else(|| panic!("{id} is drawn"));
    let bounds = window.bounds(element).unwrap_or_else(|| panic!("{id} has a box"));

    (bounds.origin.x.0, bounds.origin.y.0)
}

/// Without a session the tools are refused, since an annotation is something everyone in one
/// sees; given one, the popover opens on them.
#[test]
fn the_tools_are_refused_without_a_session() {
    let (mut window, session) = open();
    let toggle = window.by_test_id("replay-renderer-tools-toggle").expect("the tools control is drawn");
    assert_eq!(window.a11y(toggle).map(|state| state.disabled), Some(true), "refused with no session");

    let _ = window.click(toggle);
    settle(&mut window);
    assert!(window.by_test_id("replay-renderer-tools").is_none(), "a refused control opens nothing");

    let state = Arc::default();
    let (link, _sent) = CollabLink::for_test(Arc::clone(&state));
    session.set_collab(link);
    settle(&mut window);
    let toggle = window.by_test_id("replay-renderer-tools-toggle").expect("the tools control is drawn");
    assert_eq!(window.a11y(toggle).map(|state| state.disabled), Some(false), "offered in a session");

    click(&mut window, "replay-renderer-tools-toggle");
    assert!(window.by_test_id("replay-renderer-tools").is_some(), "the tools open");
}

/// A tool, a ship to place and an ink chosen on the popover are what is in hand.
#[test]
fn choosing_on_the_popover_puts_it_in_hand() {
    let (mut window, session) = open();
    let state = Arc::default();
    let (link, _sent) = CollabLink::for_test(Arc::clone(&state));
    session.set_collab(link);
    settle(&mut window);
    click(&mut window, "replay-renderer-tools-toggle");

    click(&mut window, "replay-renderer-tool-arrow");
    assert_eq!(tool(session), Tool::Arrow);

    click(&mut window, "replay-renderer-tool-rectangle");
    assert_eq!(tool(session), Tool::Rectangle { filled: false });

    click(&mut window, "replay-renderer-place-enemy-DD");
    assert!(
        matches!(tool(session), Tool::Ship { ref species, friendly: false, .. } if species == "Destroyer"),
        "an enemy destroyer is in hand: {:?}",
        tool(session)
    );

    click(&mut window, "replay-renderer-tool-select");
    assert_eq!(tool(session), Tool::None, "the selector puts the tool down");

    click(&mut window, "replay-renderer-ink-red");
    let ink = session.annotator.with_untracked(|annotator| annotator.drawing().color());
    assert_eq!(ink, [230, 50, 50, 255]);
    let red = window.by_test_id("replay-renderer-ink-red").expect("the swatch is drawn");
    assert!(window.by_attribute("data-chosen", "true") == Some(red), "the chosen ink is outlined");
}

/// The nib steps one at a time and stops at the thickest the board offers, where the step that
/// would go past it is refused.
#[test]
fn the_nib_steps_within_its_range() {
    let (mut window, session) = open();
    let state = Arc::default();
    let (link, _sent) = CollabLink::for_test(Arc::clone(&state));
    session.set_collab(link);
    settle(&mut window);
    click(&mut window, "replay-renderer-tools-toggle");

    let opened_at = session.annotator.with_untracked(|annotator| annotator.drawing().width());
    click(&mut window, "replay-renderer-nib-thicker");
    let width = session.annotator.with_untracked(|annotator| annotator.drawing().width());
    assert_eq!(width, opened_at + 1.0);
    let shown = window.by_test_id("replay-renderer-nib").expect("the nib is shown");
    assert_eq!(window.a11y(shown).and_then(|state| state.label), Some(format!("{width:.0}")));

    while session.annotator.with_untracked(|annotator| annotator.drawing().width()) < MAX_NIB {
        click(&mut window, "replay-renderer-nib-thicker");
    }
    let thicker = window.by_test_id("replay-renderer-nib-thicker").expect("the step is drawn");
    assert_eq!(window.a11y(thicker).map(|state| state.disabled), Some(true), "nothing thicker to step to");
}

/// Undo is refused until something was drawn, and takes back what was.
#[test]
fn undo_takes_back_what_was_drawn() {
    let (mut window, session) = open();
    let state = Arc::default();
    let (link, _sent) = CollabLink::for_test(Arc::clone(&state));
    session.set_collab(link);
    settle(&mut window);
    click(&mut window, "replay-renderer-tools-toggle");

    let undo = window.by_test_id("replay-renderer-annotations-undo").expect("undo is drawn");
    assert_eq!(window.a11y(undo).map(|state| state.disabled), Some(true), "nothing to undo yet");

    session.annotate(|annotator, link| {
        annotator.take_up(Tool::Line);
        annotator.stroke(link, Stroke::Began { at: [100.0, 100.0] });
        annotator.stroke(link, Stroke::Ended { at: [200.0, 100.0] });
    });
    settle(&mut window);
    let undo = window.by_test_id("replay-renderer-annotations-undo").expect("undo is drawn");
    assert_eq!(window.a11y(undo).map(|state| state.disabled), Some(false), "the line can be taken back");

    click(&mut window, "replay-renderer-annotations-undo");
    assert!(!session.annotator.with_untracked(|annotator| annotator.can_undo()), "and it was");
    assert!(state.lock().current_annotation_sync.as_ref().is_none_or(|sync| sync.annotations.is_empty()));
}

/// The primary modifier and Z takes back what was drawn, as the shortcut sheet says.
#[test]
fn the_undo_chord_takes_back_what_was_drawn() {
    let (mut window, session) = open();
    let state = Arc::default();
    let (link, _sent) = CollabLink::for_test(Arc::clone(&state));
    session.set_collab(link);
    settle(&mut window);
    session.annotate(|annotator, link| {
        annotator.take_up(Tool::Line);
        annotator.stroke(link, Stroke::Began { at: [100.0, 100.0] });
        annotator.stroke(link, Stroke::Ended { at: [200.0, 100.0] });
        annotator.take_up(Tool::None);
    });
    click(&mut window, "replay-renderer-viewport");

    window.press_character('z', Modifiers::PRIMARY);
    settle(&mut window);
    assert!(!session.annotator.with_untracked(|annotator| annotator.can_undo()), "the line was taken back");
    assert!(state.lock().current_annotation_sync.as_ref().is_none_or(|sync| sync.annotations.is_empty()));
}

/// Keys typed into a search on the renderer's popovers stay with the search: the undo chord does
/// not take back a drawing, and a space does not start playback.
#[test]
fn typing_in_a_search_keeps_the_keys() {
    let (mut window, session) = open();
    let state = Arc::default();
    let (link, _sent) = CollabLink::for_test(Arc::clone(&state));
    session.set_collab(link);
    settle(&mut window);
    session.annotate(|annotator, link| {
        annotator.take_up(Tool::Line);
        annotator.stroke(link, Stroke::Began { at: [100.0, 100.0] });
        annotator.stroke(link, Stroke::Ended { at: [200.0, 100.0] });
        annotator.take_up(Tool::None);
    });
    click(&mut window, "replay-renderer-timeline-toggle");
    click(&mut window, "replay-renderer-timeline-search");

    window.type_text("a b");
    settle(&mut window);
    let typed = session.events.with_untracked(|events| events.filter().search.clone());
    assert_eq!(typed, "a b", "the search has the keyboard");
    assert!(!session.playback.with_untracked(|playback| playback.is_playing()), "playback did not start");

    window.press_character('z', Modifiers::PRIMARY);
    settle(&mut window);
    assert!(session.annotator.with_untracked(|annotator| annotator.can_undo()), "the drawing stands");
}

/// Holding the primary modifier puts the shortcut sheet up in the corner, listing every tool's
/// chord, and letting go takes it down.
#[test]
fn the_primary_modifier_shows_the_shortcut_sheet() {
    let (mut window, _session) = open();
    assert!(window.by_test_id("replay-renderer-shortcuts").is_none(), "down until the key is held");

    click(&mut window, "replay-renderer-viewport");
    window.press_key(NamedKey::Control, Modifiers::PRIMARY);
    settle(&mut window);
    let sheet = window.by_test_id("replay-renderer-shortcuts").expect("the sheet is up while the key is held");

    let viewport = window.by_test_id("replay-renderer-viewport").expect("the map is drawn");
    let (sheet, viewport) = (window.bounds(sheet).expect("a box"), window.bounds(viewport).expect("a box"));
    let right = |rect: &Rect<DevicePx, Device>| rect.origin.x.0 + rect.size.width.0;
    let bottom = |rect: &Rect<DevicePx, Device>| rect.origin.y.0 + rect.size.height.0;
    assert!(right(&viewport) - right(&sheet) < 16.0, "in the right-hand corner: {sheet:?} in {viewport:?}");
    assert!(bottom(&viewport) - bottom(&sheet) < 16.0, "at the bottom: {sheet:?} in {viewport:?}");
    assert!(window.by_label(&t!("ui.renderer.annotations.tool_measurement")).is_some(), "the tools are listed");

    window.press_key(NamedKey::Escape, Modifiers::NONE);
    settle(&mut window);
    assert!(window.by_test_id("replay-renderer-shortcuts").is_none(), "down once the key is let go");
}

/// A peer's cursor is drawn where they point on the map, and follows them as the session
/// redraws; a fresh ping is drawn and one whose ripple has run out is not.
#[test]
fn a_peer_cursor_appears_where_it_points() {
    let (mut window, session) = open();
    let state = Arc::default();
    push_ping_aged(&state, [200.0, 200.0], Duration::ZERO);
    let (link, _sent) = CollabLink::for_test(Arc::clone(&state));
    session.set_collab(link);
    settle(&mut window);
    assert!(window.by_test_id("replay-renderer-peer-ping-7").is_some(), "a fresh ping is drawn");

    let cursor = |at: [f32; 2]| UserCursor {
        user_id: 9,
        name: "Peer".to_owned(),
        color: [0, 200, 255],
        pos: Some(at),
        last_update: std::time::Instant::now(),
    };
    state.lock().cursors.push(cursor([300.0, 400.0]));
    settle(&mut window);
    let expected = window_point(session, (300.0, 400.0));
    let (left, top) = corner(&window, "replay-renderer-peer-cursor-9");
    assert!((left - expected.x.0).abs() < 1.5 && (top - expected.y.0).abs() < 1.5, "at {expected:?}, got {left},{top}");

    state.lock().cursors[0] = cursor([500.0, 100.0]);
    settle(&mut window);
    let expected = window_point(session, (500.0, 100.0));
    let (left, top) = corner(&window, "replay-renderer-peer-cursor-9");
    assert!((left - expected.x.0).abs() < 1.5 && (top - expected.y.0).abs() < 1.5, "follows to {expected:?}");

    state.lock().pings.clear();
    push_ping_aged(&state, [200.0, 200.0], Duration::from_secs(5));
    settle(&mut window);
    assert!(window.by_test_id("replay-renderer-peer-ping-7").is_none(), "a finished ripple is not drawn");
}

/// Picking a placed ship out on the map puts its rotation handle above it and the chooser that
/// names it on the popover.
#[test]
fn a_picked_ship_gets_a_handle_and_a_chooser() {
    let (mut window, session) = open();
    let state = Arc::default();
    let (link, _sent) = CollabLink::for_test(Arc::clone(&state));
    state.lock().current_annotation_sync = Some(AnnotationSyncState {
        annotations: vec![Annotation::Ship {
            pos: [300.0, 400.0],
            yaw: 0.5,
            species: "Cruiser".to_owned(),
            friendly: false,
            config: None,
        }],
        ids: vec![11],
        owners: vec![3],
    });
    session.set_collab(link);
    settle(&mut window);
    click(&mut window, "replay-renderer-tools-toggle");
    assert!(window.by_test_id("replay-renderer-ship-chooser").is_none(), "nothing to name until a ship is picked");
    assert!(window.by_test_id("replay-renderer-rotation-handle").is_none());

    click(&mut window, "replay-renderer-tools-toggle");
    assert!(window.by_test_id("replay-renderer-tools").is_none(), "the tools close again");

    // Picked as a click on the map picks; the map's own click handling is the view's to test.
    session.annotate(|annotator, link| annotator.pick_at(link, [300.0, 400.0], false));
    settle(&mut window);
    assert!(window.by_test_id("replay-renderer-rotation-handle").is_some(), "the picked ship can be turned");
    let ship = window_point(session, (300.0, 400.0));
    let (left, top) = corner(&window, "replay-renderer-rotation-handle");
    assert!(top < ship.y.0, "the handle sits above the ship: {top} against {}", ship.y.0);
    assert!((left + HANDLE_RADIUS - ship.x.0).abs() < 2.0, "over its middle: {left} against {}", ship.x.0);

    click(&mut window, "replay-renderer-tools-toggle");
    assert!(window.by_test_id("replay-renderer-ship-chooser").is_some(), "the chooser is up");
    assert!(window.by_label(&t!("ui.renderer.annotations.no_ship")).is_some(), "saying no ship is chosen yet");
    assert!(window.by_label(&t!("ui.renderer.annotations.ship_hint")).is_some(), "with a search to find one by");

    let search = window.by_test_id("replay-renderer-ship-search").expect("the search is drawn");
    let typed = |window: &mut HeadlessWindow| window.a11y(search).and_then(|state| state.value).unwrap_or_default();
    click(&mut window, "replay-renderer-ship-search");
    window.type_text("yam");
    settle(&mut window);
    assert_eq!(typed(&mut window), "yam");
    window.press_key(NamedKey::Escape, Modifiers::NONE);
    settle(&mut window);
    assert_eq!(typed(&mut window), "", "Escape empties the search");
    assert!(window.by_test_id("replay-renderer-tools").is_some(), "and leaves the tools open");
    window.press_key(NamedKey::Escape, Modifiers::NONE);
    settle(&mut window);
    assert!(window.by_test_id("replay-renderer-tools").is_none(), "Escape in the empty search closes them");
}

/// A click with the primary modifier held adds to what is picked, as the shortcut sheet says; a
/// click with Shift held picks afresh.
#[test]
fn the_primary_modifier_adds_to_the_picked() {
    let (mut window, session) = open();
    let state = Arc::default();
    let (link, _sent) = CollabLink::for_test(Arc::clone(&state));
    let ship =
        |pos: [f32; 2]| Annotation::Ship { pos, yaw: 0.0, species: "Cruiser".to_owned(), friendly: true, config: None };
    state.lock().current_annotation_sync = Some(AnnotationSyncState {
        annotations: vec![ship([300.0, 400.0]), ship([600.0, 400.0])],
        ids: vec![11, 12],
        owners: vec![3, 3],
    });
    session.set_collab(link);
    settle(&mut window);
    let picked = || session.annotator.with_untracked(|annotator| annotator.picked().picked().to_vec());

    window.click_at(window_point(session, (300.0, 400.0)));
    settle(&mut window);
    assert_eq!(picked(), [0], "a plain click picks one");

    window.click_at_with(window_point(session, (600.0, 400.0)), Modifiers::SHIFT);
    settle(&mut window);
    assert_eq!(picked(), [1], "Shift picks afresh rather than adding");

    window.click_at_with(window_point(session, (300.0, 400.0)), Modifiers::PRIMARY);
    settle(&mut window);
    assert_eq!(picked(), [1, 0], "the primary modifier adds");
}
