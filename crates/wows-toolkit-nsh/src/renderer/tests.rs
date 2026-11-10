//! The viewer driven the way the frame surface drives it: input events in,
//! playback state and the map's geometry out.

use wows_minimap_renderer::CANVAS_HEIGHT;
use wows_minimap_renderer::MINIMAP_SIZE;
use wows_replays::types::GameClock;
use wows_toolkit_replay_data::playback::DEFAULT_MAP_ORIGIN;
use wows_toolkit_replay_data::playback::Track;
use wowsunpack::data::Version;
use wt_collab_client::link::CollabLink;

use crate::frames::Modifiers;
use crate::frames::PixelPoint;
use crate::frames::PixelSize;
use crate::frames::PointerButton;
use crate::frames::ScaleFactor;
use crate::frames::ScrollSteps;
use crate::frames::SurfaceInput;
use crate::renderer::geometry::CanvasSize;
use crate::renderer::viewer::Viewer;

/// A viewer holding a battle of `clocks.len()` empty frames, shown on a view
/// of exactly the canvas's size, so a device pixel is a canvas pixel.
fn ready(clocks: &[f32]) -> Viewer {
    let mut viewer = Viewer::new("battle.wowsreplay".into(), Vec::new(), CollabLink::default());
    let track = Track {
        frames: vec![Vec::new(); clocks.len()],
        clocks: clocks.iter().copied().map(GameClock).collect(),
        battle_start: GameClock(0.0),
        battle_end: None,
        space_size: 1400.0,
        version: Version::base(99, 0, 0),
        map_origin: DEFAULT_MAP_ORIGIN,
        space: String::new(),
    };
    viewer.adopt_track_for_test(track);
    let size = PixelSize { width: MINIMAP_SIZE, height: CANVAS_HEIGHT };
    viewer.apply(SurfaceInput::Resized { size, scale: ScaleFactor::new(1.0) });
    viewer.drew(CanvasSize { width: MINIMAP_SIZE, height: CANVAS_HEIGHT }, Vec::new());
    viewer
}

fn key(viewer: &mut Viewer, name: &str, modifiers: Modifiers) {
    viewer.apply(SurfaceInput::Key { key: name.to_owned(), pressed: true, modifiers });
    viewer.apply(SurfaceInput::Key { key: name.to_owned(), pressed: false, modifiers });
}

fn clocks(count: usize) -> Vec<f32> {
    (0..count).map(|index| index as f32 * 0.5).collect()
}

#[test]
fn the_keyboard_plays_seeks_and_changes_speed() {
    let mut viewer = ready(&clocks(200));

    key(&mut viewer, "space", Modifiers::default());
    assert!(viewer.is_playing());
    key(&mut viewer, "space", Modifiers::default());
    assert!(!viewer.is_playing());

    key(&mut viewer, "Right", Modifiers::default());
    assert_eq!(viewer.at().get(), 20, "ten seconds on is twenty half-second frames");
    key(&mut viewer, "Left", Modifiers::default());
    assert_eq!(viewer.at().get(), 0);

    let before = viewer.speed();
    key(&mut viewer, "Up", Modifiers::default());
    assert!(viewer.speed() > before);
    key(&mut viewer, "Down", Modifiers::default());
    assert_eq!(viewer.speed(), before);
}

#[test]
fn playing_from_the_end_starts_again_and_stops_at_the_end() {
    let mut viewer = ready(&clocks(3));
    viewer.go_to_end();
    viewer.toggle_playing();
    assert_eq!(viewer.at().get(), 0, "playing from the end starts again");

    viewer.tick();
    viewer.tick();
    assert_eq!(viewer.at().get(), 2);
    viewer.tick();
    assert!(!viewer.is_playing(), "and stops on the last frame");
}

#[test]
fn ctrl_held_puts_the_shortcut_sheet_up_until_released_or_unfocused() {
    let mut viewer = ready(&clocks(4));

    viewer.apply(SurfaceInput::Key { key: "Control_L".to_owned(), pressed: true, modifiers: Modifiers::default() });
    assert!(viewer.ctrl_held());
    viewer.apply(SurfaceInput::Focus { focused: false });
    assert!(!viewer.ctrl_held());
}

#[test]
fn the_wheel_zooms_about_the_pointer_and_a_double_click_puts_the_map_back() {
    let mut viewer = ready(&clocks(4));
    let over_map = PixelPoint { x: 200.0, y: 300.0 };
    let under = viewer.map_point(over_map).expect("the pointer is over the map");

    viewer.apply(SurfaceInput::Wheel {
        at: Some(over_map),
        dx: ScrollSteps::new(0.0),
        dy: ScrollSteps::new(-3.0),
        modifiers: Modifiers::default(),
    });
    assert!(viewer.map_view().zoom() > 1.0, "a step up zooms in");
    let after = viewer.map_point(over_map).expect("still over the map");
    assert!((after.x - under.x).abs() < 1.0 && (after.y - under.y).abs() < 1.0, "what was under the pointer stays");

    let click = |viewer: &mut Viewer, clicks: u32| {
        viewer.apply(SurfaceInput::PointerDown { at: over_map, button: PointerButton::Primary, clicks, modifiers: Modifiers::default() });
        viewer.apply(SurfaceInput::PointerUp { at: over_map, button: PointerButton::Primary, modifiers: Modifiers::default() });
    };
    click(&mut viewer, 1);
    assert!(!viewer.map_view().is_whole_map(), "a single click leaves the zoom");
    click(&mut viewer, 2);
    assert!(viewer.map_view().is_whole_map(), "a double click puts the whole map back");
}

#[test]
fn a_drag_pans_a_zoomed_map_and_the_hud_strip_is_not_the_map() {
    let mut viewer = ready(&clocks(4));
    assert!(viewer.map_point(PixelPoint { x: 10.0, y: 5.0 }).is_none(), "the HUD strip is above the map");

    viewer.set_zoom(4.0);
    let before = viewer.map_view().pan();
    let start = PixelPoint { x: 400.0, y: 400.0 };
    viewer.apply(SurfaceInput::PointerDown { at: start, button: PointerButton::Primary, clicks: 1, modifiers: Modifiers::default() });
    viewer.apply(SurfaceInput::PointerMoved { at: PixelPoint { x: 300.0, y: 350.0 }, modifiers: Modifiers::default() });
    viewer.apply(SurfaceInput::PointerUp { at: PixelPoint { x: 300.0, y: 350.0 }, button: PointerButton::Primary, modifiers: Modifiers::default() });

    let after = viewer.map_view().pan();
    assert!(after.0 > before.0 && after.1 > before.1, "the map follows the pointer: {before:?} -> {after:?}");
}

#[test]
fn drawing_tools_are_refused_without_a_session() {
    let mut viewer = ready(&clocks(4));
    key(&mut viewer, "2", Modifiers { control: true, ..Modifiers::default() });
    assert!(!viewer.has_tool(), "an annotation is something everyone in a session sees");
}

#[test]
fn a_toggled_layer_and_per_ship_ranges_change_what_is_drawn() {
    let mut viewer = ready(&clocks(4));
    assert!(!viewer.options().show_ship_config, "ranges start off");

    viewer.set_ranges_for("captain", |filter| filter.detection = true);
    assert!(viewer.options().show_ship_config, "one ship's range turns the layer on");
    viewer.set_ranges_for("captain", |filter| filter.detection = false);
    assert!(!viewer.options().show_ship_config, "and off again with the last one");
}
