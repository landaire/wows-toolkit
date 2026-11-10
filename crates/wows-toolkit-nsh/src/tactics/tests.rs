//! The board's producer driven as the frame view drives it: input through
//! `send_input`, frames read back with `latest`.

use std::io::Write as _;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use wows_replay_insights::cap_layout::CapLayoutDb;
use wows_replays::game_constants::GameConstants;
use wowsunpack::data::Version;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::vfs::MemoryFS;
use wowsunpack::vfs::VfsPath;
use wt_collab_client::types::Annotation;

use crate::frames::Modifiers;
use crate::frames::PixelPoint;
use crate::frames::PixelSize;
use crate::frames::PointerButton;
use crate::frames::SurfaceId;
use crate::frames::SurfaceInput;
use crate::tactics::board::Board;
use crate::tactics::board::MapStatus;
use crate::tactics::board::map_value;
use crate::tactics::game::BoardGame;
use crate::tactics::producer::Shared;
use crate::tactics::view;
use crate::tactics::view::ToolName;

/// The map's water, which a pixel of drawn map reads as.
const WATER: [u8; 3] = [40, 90, 160];
const SPACE: &str = "spaces/00_test_bay";

/// A build with one map: its art, and measurements making it 1200 world
/// units across.
struct TestGame {
    vfs: VfsPath,
    metadata: GameMetadataProvider,
    constants: GameConstants,
}

impl BoardGame for TestGame {
    fn vfs(&self) -> &VfsPath {
        &self.vfs
    }

    fn metadata(&self) -> &GameMetadataProvider {
        &self.metadata
    }

    fn constants(&self) -> &GameConstants {
        &self.constants
    }

    fn version(&self) -> Option<Version> {
        None
    }
}

fn test_game() -> Arc<dyn BoardGame> {
    let vfs = VfsPath::new(MemoryFS::new());
    // Any face parses as the game's font; the renderer needs one and the test
    // machine may have none it looks for.
    let fonts = vfs.join("gui/fonts").expect("a fonts path");
    fonts.create_dir_all().expect("the fonts directory");
    fonts
        .join("Warhelios.ttf")
        .and_then(|path| path.create_file())
        .expect("the font file")
        .write_all(include_bytes!("../../../wows-toolkit-gpui/assets/fonts/Phosphor.ttf"))
        .expect("the font is written");
    let space = vfs.join(SPACE).expect("a space path");
    space.create_dir_all().expect("the space directory");

    let art = image::RgbImage::from_pixel(768, 768, image::Rgb(WATER));
    let mut png = Vec::new();
    art.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).expect("the art encodes");
    space.join("minimap_water.png").and_then(|path| path.create_file()).expect("the art file").write_all(&png).expect("the art is written");

    let settings = r#"<space.settings><bounds minX="-8" maxX="7" minY="-8" maxY="7"/><chunkSize>100</chunkSize></space.settings>"#;
    space
        .join("space.settings")
        .and_then(|path| path.create_file())
        .expect("the settings file")
        .write_all(settings.as_bytes())
        .expect("the settings are written");

    Arc::new(TestGame {
        vfs,
        metadata: GameMetadataProvider::from_params_no_specs(Vec::new()).expect("an empty provider"),
        constants: GameConstants::defaults(),
    })
}

const SIZE: PixelSize = PixelSize { width: 400, height: 300 };

/// Where a map pixel lands on the 400 by 300 frame: the 768-pixel map is
/// fitted to the frame's height and centred across it.
fn on_frame(map_x: f32, map_y: f32) -> PixelPoint {
    let scale = 300.0 / 768.0;
    PixelPoint { x: 50.0 + map_x * scale, y: map_y * scale }
}

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let started = Instant::now();
    while !done() {
        assert!(started.elapsed() < Duration::from_secs(20), "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A board on its own surface, sized and set on the test map, with the map
/// drawn.
fn board_on_map(name: &str) -> Arc<Shared> {
    crate::isolate_test_storage();
    let shared = Shared::start(&SurfaceId::new(name), Board::new(Some(test_game()), CapLayoutDb::default(), None));
    shared.surface().send_input(SurfaceInput::Resized { size: SIZE, scale: 1.0 });
    let value = {
        let board = shared.lock();
        let map = board.maps().iter().find(|map| map.space == SPACE).expect("the test map is offered");
        map_value(map)
    };
    shared.update(|board| board.set_map_value(&value)).expect("the map is set");
    wait_until("the map to be drawn", || shared.lock().status() == MapStatus::Shown);
    shared
}

fn frame_pixel(shared: &Shared, at: PixelPoint) -> [u8; 4] {
    let frame = shared.surface().latest().expect("a frame is shown");
    let offset = (at.y as usize * frame.size.width as usize + at.x as usize) * 4;
    frame.pixels[offset..offset + 4].try_into().expect("four bytes a pixel")
}

fn send(shared: &Shared, input: SurfaceInput) {
    shared.surface().send_input(input);
}

fn press(shared: &Shared, at: PixelPoint) {
    send(shared, SurfaceInput::PointerDown { at, button: PointerButton::Primary, clicks: 1, modifiers: Modifiers::default() });
}

fn drag_to(shared: &Shared, at: PixelPoint) {
    send(shared, SurfaceInput::PointerMoved { at, modifiers: Modifiers::default() });
}

fn release(shared: &Shared, at: PixelPoint) {
    send(shared, SurfaceInput::PointerUp { at, button: PointerButton::Primary, modifiers: Modifiers::default() });
}

/// The map is drawn at the frame's size, fitted without stretching: water in
/// the middle, the margin beside it.
#[test]
fn the_map_is_drawn_letterboxed_at_the_reported_size() {
    let shared = board_on_map("tactics-test-render");

    let frame = shared.surface().latest().expect("a frame is shown");
    assert_eq!(frame.size, SIZE);
    // Off the grid lines the renderer draws over the map.
    let water = frame_pixel(&shared, on_frame(30.0, 50.0));
    let close = |pixel: [u8; 4]| pixel[..3].iter().zip(WATER).all(|(got, want)| got.abs_diff(want) <= 16);
    assert!(close(water), "the map shows its water: {water:?}");
    let margin = frame_pixel(&shared, PixelPoint { x: 10.0, y: 150.0 });
    assert!(!close(margin), "the left margin does not: {margin:?}");

    send(&shared, SurfaceInput::Resized { size: PixelSize { width: 300, height: 500 }, scale: 2.0 });
    wait_until("a frame at the new size", || {
        shared.surface().latest().is_some_and(|frame| frame.size == PixelSize { width: 300, height: 500 })
    });
}

/// With "Add cap" on, a click places a capture point where it lands, and a
/// drag on it moves it.
#[test]
fn a_click_places_a_cap_and_a_drag_moves_it() {
    let shared = board_on_map("tactics-test-caps");
    shared.update(|board| board.set_adding(true));

    press(&shared, on_frame(384.0, 384.0));
    wait_until("a placed cap", || shared.lock().caps().len() == 1);
    {
        let board = shared.lock();
        let cap = &board.caps()[0];
        assert!(cap.world_x.abs() < 5.0 && cap.world_z.abs() < 5.0, "placed at the middle: {cap:?}");
        assert_eq!(board.selected_cap().map(|cap| cap.letter()), Some("A".to_owned()));
        assert!(!board.adding(), "placing one cap puts the tool down");
    }
    release(&shared, on_frame(384.0, 384.0));

    press(&shared, on_frame(384.0, 384.0));
    drag_to(&shared, on_frame(576.0, 384.0));
    release(&shared, on_frame(576.0, 384.0));
    wait_until("the cap to move east", || shared.lock().caps()[0].world_x > 250.0);

    shared.update(Board::undo);
    assert!(shared.lock().caps()[0].world_x.abs() < 5.0, "undo puts it back");
    shared.update(Board::redo);
    assert!(shared.lock().caps()[0].world_x > 250.0, "and redo moves it again");
}

/// A line tool drag draws a line in map pixels; the eraser rubs it out.
#[test]
fn a_tool_drag_draws_and_the_eraser_rubs_out() {
    let shared = board_on_map("tactics-test-draw");
    shared.update(|board| board.set_tool(view::tool_of(Some(ToolName::Line))));

    press(&shared, on_frame(100.0, 100.0));
    drag_to(&shared, on_frame(300.0, 100.0));
    release(&shared, on_frame(300.0, 100.0));
    wait_until("a drawn line", || shared.lock().shape_count() == 1);
    let Annotation::Line { start, end, .. } = shared.lock().shapes()[0].clone() else {
        panic!("the line tool draws a line")
    };
    assert!((start[0] - 100.0).abs() < 3.0 && (end[0] - 300.0).abs() < 3.0, "{start:?} to {end:?}");

    shared.update(|board| board.set_tool(view::tool_of(Some(ToolName::Eraser))));
    press(&shared, on_frame(200.0, 100.0));
    release(&shared, on_frame(200.0, 100.0));
    wait_until("the line to be rubbed out", || shared.lock().shape_count() == 0);
}

/// Without a tool, a click picks a shape out and a drag moves it; the
/// handle of a picked rectangle turns it.
#[test]
fn a_picked_shape_moves_and_turns() {
    let shared = board_on_map("tactics-test-pick");
    shared.update(|board| board.set_tool(view::tool_of(Some(ToolName::Rectangle))));
    press(&shared, on_frame(300.0, 300.0));
    drag_to(&shared, on_frame(400.0, 360.0));
    release(&shared, on_frame(400.0, 360.0));
    wait_until("a drawn rectangle", || shared.lock().shape_count() == 1);
    shared.update(|board| board.set_tool(view::tool_of(None)));

    // The edge of the rectangle, where the pick reaches.
    press(&shared, on_frame(400.0, 330.0));
    release(&shared, on_frame(400.0, 330.0));
    wait_until("the rectangle to be picked", || shared.lock().picked() == [0]);

    let before = shared.lock().shapes()[0].clone();
    press(&shared, on_frame(400.0, 330.0));
    drag_to(&shared, on_frame(450.0, 330.0));
    release(&shared, on_frame(450.0, 330.0));
    wait_until("the rectangle to move", || shared.lock().shapes()[0] != before);
    let Annotation::Rectangle { center, rotation, .. } = shared.lock().shapes()[0].clone() else {
        panic!("still a rectangle")
    };
    assert!(center[0] > 340.0, "moved east: {center:?}");

    // The handle sits a fixed distance above the shape's top edge.
    let handle = {
        let board = shared.lock();
        let Some(super::board::Frame::Map(job)) = board.frame() else { panic!("a map frame") };
        job.handle.expect("a picked rectangle has a handle").0
    };
    press(&shared, PixelPoint { x: handle.0, y: handle.1 });
    drag_to(&shared, PixelPoint { x: handle.0 + 80.0, y: handle.1 + 60.0 });
    release(&shared, PixelPoint { x: handle.0 + 80.0, y: handle.1 + 60.0 });
    wait_until("the rectangle to turn", || {
        matches!(shared.lock().shapes()[0], Annotation::Rectangle { rotation: turned, .. } if turned != rotation)
    });
}

/// The wheel zooms about the pointer, a drag pans the zoomed map, and a
/// double click puts the whole map back.
#[test]
fn the_wheel_zooms_a_drag_pans_and_a_double_click_resets() {
    let shared = board_on_map("tactics-test-view");

    send(&shared, SurfaceInput::Wheel { at: Some(on_frame(384.0, 384.0)), dx: 0.0, dy: -5.0, modifiers: Modifiers::default() });
    wait_until("a zoom", || shared.lock().view().zoom() > 1.0);
    let zoomed = shared.lock().view();

    press(&shared, on_frame(100.0, 100.0));
    drag_to(&shared, on_frame(150.0, 120.0));
    release(&shared, on_frame(150.0, 120.0));
    wait_until("a pan", || shared.lock().view().pan() != zoomed.pan());

    send(&shared, SurfaceInput::PointerDown {
        at: on_frame(100.0, 100.0),
        button: PointerButton::Primary,
        clicks: 2,
        modifiers: Modifiers::default(),
    });
    release(&shared, on_frame(100.0, 100.0));
    wait_until("the whole map", || shared.lock().view().is_whole_map());
}

/// The context menu puts the tool down and picks the capture point under the
/// pointer, which its entries then act on.
#[test]
fn the_context_menu_picks_the_cap_under_it() {
    let shared = board_on_map("tactics-test-menu");
    shared.update(|board| board.set_adding(true));
    press(&shared, on_frame(384.0, 384.0));
    release(&shared, on_frame(384.0, 384.0));
    wait_until("a placed cap", || shared.lock().caps().len() == 1);
    shared.update(|board| {
        board.set_tool(view::tool_of(Some(ToolName::Freehand)));
        board.context_menu(on_frame(384.0, 384.0));
    });

    let board = shared.lock();
    assert_eq!(board.tool(), &wt_collab_client::drawing::Tool::None);
    assert_eq!(board.selected_cap().map(|cap| cap.letter()), Some("A".to_owned()));
    let shown = view::of(&board, 0);
    assert!(shown.has_map_info && shown.can_edit_caps);
    assert_eq!(shown.selected_cap.map(|cap| cap.radius_km), Some("4.5".to_owned()));
}

/// Keys are the view's keymap's: the producer ignores the frame's own.
#[test]
fn frame_keys_change_nothing() {
    let shared = board_on_map("tactics-test-keys");
    shared.update(|board| board.set_tool(view::tool_of(Some(ToolName::Line))));
    send(&shared, SurfaceInput::Key { key: "Escape".to_owned(), pressed: true, modifiers: Modifiers::default() });
    send(&shared, SurfaceInput::Resized { size: SIZE, scale: 1.0 });
    std::thread::sleep(Duration::from_millis(50));

    assert_eq!(shared.lock().tool(), &wt_collab_client::drawing::Tool::Line);
}

/// A session hears the board's map, then its capture points, and follows a
/// peer's change to them.
#[test]
fn a_session_hears_the_board_and_its_caps() {
    use wt_collab_client::peer::LocalEvent;

    let shared = board_on_map("tactics-test-collab");
    shared.update(|board| board.set_adding(true));
    press(&shared, on_frame(384.0, 384.0));
    release(&shared, on_frame(384.0, 384.0));
    wait_until("a placed cap", || shared.lock().caps().len() == 1);

    let state = Arc::new(parking_lot::Mutex::new(wt_collab_client::SessionState::default()));
    let (link, events) = wt_collab_client::link::CollabLink::with_outbox(Arc::clone(&state));
    shared.set_collab(link);

    let mut heard = Vec::new();
    wait_until("the announcement and the cap", || {
        heard.extend(events.try_iter());
        heard.iter().any(|event| matches!(event, LocalEvent::CapPoint { .. }))
    });
    let announced = heard.iter().position(|event| matches!(event, LocalEvent::TacticsMapOpened { .. }));
    let capped = heard.iter().position(|event| matches!(event, LocalEvent::CapPoint { .. }));
    assert!(announced.is_some() && announced < capped, "the map is announced before its caps");
}

/// A link the session watch is given reaches the board, which announces its
/// map to that session.
#[test]
fn the_board_follows_a_new_session_link() {
    use wt_collab_client::peer::LocalEvent;

    let shared = board_on_map("tactics-test-follow");
    let (links, watched) = tokio::sync::watch::channel(wt_collab_client::link::CollabLink::default());
    let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().expect("a runtime");
    runtime.spawn(Arc::clone(&shared).follow_links(watched));

    let state = Arc::new(parking_lot::Mutex::new(wt_collab_client::SessionState::default()));
    let (link, events) = wt_collab_client::link::CollabLink::with_outbox(state);
    links.send(link).expect("the follower listens");

    let mut heard = Vec::new();
    wait_until("the announcement", || {
        heard.extend(events.try_iter());
        heard.iter().any(|event| matches!(event, LocalEvent::TacticsMapOpened { .. }))
    });
}

#[test]
fn an_open_the_board_closed_under_leaves_the_board_alone() {
    let gate = crate::tactics::OpenGate::new();
    let ticket = gate.begin();
    gate.close(|| {});

    let mut replaced = false;
    assert!(!gate.finish(ticket, || replaced = true));
    assert!(!replaced, "a closed board is not replaced by the open it closed under");
}

#[test]
fn a_later_open_overtakes_an_earlier_one() {
    let gate = crate::tactics::OpenGate::new();
    let earlier = gate.begin();
    let later = gate.begin();

    assert!(!gate.finish(earlier, || {}), "the overtaken open leaves the board alone");
    assert!(gate.finish(later, || {}), "the latest open replaces the board");
}

#[test]
fn an_open_nothing_interrupted_replaces_the_board() {
    let gate = crate::tactics::OpenGate::new();
    let ticket = gate.begin();

    let mut replaced = false;
    assert!(gate.finish(ticket, || replaced = true));
    assert!(replaced);
}
