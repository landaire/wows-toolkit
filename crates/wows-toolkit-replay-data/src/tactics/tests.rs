use wows_minimap_renderer::MINIMAP_SIZE;
use wows_minimap_renderer::MapInfo;
use wows_minimap_renderer::viewport::MapViewport;
use wows_replay_insights::cap_layout::CapPointLayout;
use wows_replays::types::TeamId;
use wows_toolkit_services::collab::CapPointId;
use wows_toolkit_viewmodel::text_filter::FilterQuery;
use wowsunpack::game_types::WorldPos2D;

use crate::tactics::BoardCapPoint;
use crate::tactics::BoardTool;
use crate::tactics::CapDrag;
use crate::tactics::ENEMY_COLOR;
use crate::tactics::ElementPos;
use crate::tactics::ElementSize;
use crate::tactics::FRIENDLY_COLOR;
use crate::tactics::Letterbox;
use crate::tactics::MapChoice;
use crate::tactics::MapEntry;
use crate::tactics::NEUTRAL_COLOR;
use crate::tactics::NEW_CAP_RADIUS;
use crate::tactics::RESIZE_BAND_PX;
use crate::tactics::WORLD_UNITS_PER_KM;
use crate::tactics::band_in_world;
use crate::tactics::board::cap_drag_at;
use crate::tactics::map_entries;
use crate::tactics::radius_km;
use crate::tactics::team_color;
use crate::tactics::unique_map_for_space;

fn cap(index: usize, x: f32, z: f32, radius: f32) -> BoardCapPoint {
    BoardCapPoint {
        id: CapPointId::new(index as u64 + 1),
        index,
        world_x: x,
        world_z: z,
        radius,
        team: None,
        frozen: false,
    }
}

/// A press inside a zone moves it; one near its edge widens it.
#[test]
fn a_press_near_the_edge_widens_rather_than_moves() {
    let zone = cap(0, 0.0, 0.0, 1000.0);
    let what = |away: f32| cap_drag_at(&zone, WorldPos2D { x: away, z: 0.0 }, 8.0);

    assert_eq!(what(0.0), Some(CapDrag::Move));
    assert_eq!(what(500.0), Some(CapDrag::Move));
    assert_eq!(what(995.0), Some(CapDrag::Resize), "just inside the edge widens it");
    assert_eq!(what(1005.0), Some(CapDrag::Resize), "and so does just outside");
    assert_eq!(what(1100.0), None, "a press well past the zone is not on it");
}

/// A frozen zone is picked out but never dragged.
#[test]
fn a_frozen_zone_is_picked_but_not_dragged() {
    let zone = BoardCapPoint { frozen: true, ..cap(0, 0.0, 0.0, 1000.0) };
    let what = |away: f32| cap_drag_at(&zone, WorldPos2D { x: away, z: 0.0 }, 8.0);

    assert_eq!(what(0.0), Some(CapDrag::None));
    assert_eq!(what(995.0), Some(CapDrag::None), "its edge does not widen it");
    assert_eq!(what(1005.0), None);
}

/// A cap circle is talked about in kilometres, and the model holds it in the game's own
/// thirty-metre units.
#[test]
fn a_zone_reads_in_kilometres() {
    assert!((radius_km(NEW_CAP_RADIUS) - 4.5).abs() < 0.01);
    assert!((radius_km(WORLD_UNITS_PER_KM) - 1.0).abs() < 0.001);
}

/// A zone reads by the team that holds it, and by nobody's colour when it is neutral.
#[test]
fn a_zone_is_coloured_by_who_holds_it() {
    assert_eq!(team_color(None), NEUTRAL_COLOR);
    assert_eq!(team_color(Some(TeamId::new(0))), FRIENDLY_COLOR);
    assert_eq!(team_color(Some(TeamId::new(1))), ENEMY_COLOR);
}

/// A layout's neutral cap is stated as a negative team, which is an absence rather than a team.
#[test]
fn a_layouts_negative_team_reads_as_nobody() {
    let point = CapPointLayout {
        index: 0,
        position: WorldPos2D { x: 1.0, z: 2.0 },
        radius: wowsunpack::game_params::types::BigWorldDistance::from(600.0),
        cp_type: wowsunpack::game_types::ControlPointType::Control,
        team_id: -1,
        initially_enabled: true,
    };
    assert_eq!(BoardCapPoint::from_layout(&point).team, None);

    let held = CapPointLayout { team_id: 1, ..point };
    assert_eq!(BoardCapPoint::from_layout(&held).team, Some(TeamId::new(1)));
}

/// A zone goes over the wire and comes back the same zone.
#[test]
fn a_zone_round_trips_through_the_wire() {
    let zone = BoardCapPoint { team: Some(TeamId::new(1)), ..cap(2, 10.0, -4.0, 150.0) };
    assert_eq!(BoardCapPoint::from_wire(&zone.to_wire()), zone);
}

fn map(space_size: i32) -> MapInfo {
    MapInfo { space_size }
}

/// A press has to land on the zone it looks like it lands on.
#[test]
fn a_map_pixel_round_trips_to_the_world_and_back() {
    let map = map(1200);
    for at in [(0.0, 0.0), (100.0, 40.0), (383.5, 383.5), (767.0, 767.0)] {
        let world = map.minimap_to_world_f32(at.0, at.1, MINIMAP_SIZE);
        let back = map.world_to_minimap(world, MINIMAP_SIZE);
        assert!((back.x - at.0).abs() < 0.01 && (back.y - at.1).abs() < 0.01, "{at:?} came back as {back:?}");
    }
}

/// A zone drawn at the centre of the map is hit by a press at the centre of the map.
#[test]
fn a_press_at_the_centre_lands_on_a_zone_drawn_there() {
    let map = map(1200);
    let middle = MINIMAP_SIZE as f32 / 2.0;
    let centre = map.minimap_to_world_f32(middle, middle, MINIMAP_SIZE);
    let zone = BoardCapPoint { world_x: centre.x, world_z: centre.z, ..cap(0, 0.0, 0.0, 150.0) };

    let drawn =
        map.world_to_minimap(wowsunpack::game_types::WorldPos::new(zone.world_x, 0.0, zone.world_z), MINIMAP_SIZE);
    assert!((drawn.x - middle).abs() < 0.01);
    assert!((drawn.y - middle).abs() < 0.01);

    let pressed = map.minimap_to_world_f32(drawn.x, drawn.y, MINIMAP_SIZE);
    let away = ((pressed.x - zone.world_x).powi(2) + (pressed.z - zone.world_z).powi(2)).sqrt();
    assert!(away < 1.0, "the press is {away} units from the zone it was aimed at");
}

/// A press is read against the part of the map on screen, so a zoomed board still lands where it
/// looks like it lands.
#[test]
fn a_press_is_read_through_the_window_the_map_is_drawn_in() {
    let whole = MapViewport::default();
    assert_eq!(whole.to_map(100.0, 200.0), (100.0, 200.0));

    let middle = MINIMAP_SIZE as f32 / 2.0;
    let zoomed = whole.zoomed_about(whole.zoom() * 2.0, (middle, middle));
    let (x, y) = zoomed.to_map(middle, middle);
    assert!((x - middle).abs() < 0.01 && (y - middle).abs() < 0.01, "the centre moved to ({x}, {y})");

    let (x, _y) = zoomed.to_map(middle + 100.0, middle);
    assert!(x > middle && x < middle + 100.0, "a point 100px right of centre reads as {x}");
}

/// The resize tolerance is a screen distance, so it is the same few pixels on any map.
#[test]
fn the_resize_band_is_the_same_on_screen_whatever_the_map() {
    for space in [800i32, 1200, 1600] {
        let map = map(space);
        let on_screen = map.world_distance_to_minimap(band_in_world(&map), MINIMAP_SIZE);
        assert!((on_screen - RESIZE_BAND_PX).abs() < 0.01, "a {space}-unit map gives a {on_screen}px band");
    }
}

fn element(width: f32, height: f32) -> ElementSize {
    ElementSize { width, height }
}

/// A frame narrower than its element is centred in it rather than stretched.
#[test]
fn a_frame_is_centred_in_the_element() {
    let fit = Letterbox::fitting(element(400., 200.), (100., 100.)).expect("a frame with extent");
    assert_eq!(fit.scale, 2.0, "held to the tighter direction");
    assert_eq!(fit.top, 0.0, "filling it top to bottom");
    assert_eq!(fit.left, 100.0, "and centred across it");
}

/// The two directions agree to the pixel, which is what lets a handle be placed by one and
/// pressed by the other.
#[test]
fn the_two_directions_are_each_others_inverse() {
    let fit = Letterbox::fitting(element(400., 200.), (100., 100.)).expect("a frame with extent");
    for at in [(0.0, 0.0), (50.0, 50.0), (99.0, 1.0)] {
        let on_screen = fit.to_element(at).expect("a point the frame shows");
        let back = fit.to_frame(on_screen).expect("and it is over the map");
        assert!((back.0 - at.0).abs() < 1e-3 && (back.1 - at.1).abs() < 1e-3, "{at:?} -> {back:?}");
    }
}

/// A press in the margin beside the map is not on it, and a point the frame does not show has
/// nowhere on screen to be.
#[test]
fn a_point_outside_is_refused_both_ways() {
    let fit = Letterbox::fitting(element(400., 200.), (100., 100.)).expect("a frame with extent");
    assert!(fit.to_frame(ElementPos { x: 10.0, y: 80.0 }).is_none(), "in the left margin");
    assert!(fit.to_element((-1.0, 50.0)).is_none(), "off the frame");
    assert!(fit.to_element((100.0, 50.0)).is_none(), "and past its far edge");
}

/// A frame with no extent cannot be placed against at all.
#[test]
fn a_frame_with_no_extent_is_refused() {
    assert!(Letterbox::fitting(element(400., 200.), (0., 100.)).is_none());
}

/// Two maps that read alike are told apart in the picker by their id or space.
#[test]
fn maps_that_read_alike_are_named_apart() {
    let maps = vec![
        MapChoice { map_id: Some(16), space: "spaces/16_OC_bees_to_honey".into(), label: "Hotspot".into() },
        MapChoice { map_id: None, space: "spaces/99_hotspot".into(), label: "Hotspot".into() },
        MapChoice { map_id: Some(4), space: "spaces/04_Archipelago".into(), label: "Archipelago".into() },
    ];
    let entries = map_entries(&maps);
    let titles: Vec<&str> = entries.iter().map(|entry| entry.title.as_str()).collect();
    assert_eq!(titles, ["Hotspot (16)", "Hotspot (spaces/99_hotspot)", "Archipelago"]);

    let found = |typed: &str| -> Vec<String> {
        MapEntry::rank(&entries, &FilterQuery::new(typed)).into_iter().map(|entry| entry.title.clone()).collect()
    };
    assert_eq!(found("bees"), ["Hotspot (16)"], "found by its space");
    assert_eq!(found("16"), ["Hotspot (16)"], "and by its id");
    assert_eq!(found("").len(), 3, "an empty search lists every map in order");
    assert_eq!(found("a")[0], "Archipelago", "a title start outranks a letter inside a word");
}

/// A space listed once names its map; one listed twice names none.
#[test]
fn a_space_names_a_map_only_when_it_is_listed_once() {
    let one = MapChoice { map_id: Some(1), space: "spaces/a".into(), label: "A".into() };
    let two = MapChoice { map_id: Some(2), space: "spaces/a".into(), label: "A".into() };
    assert_eq!(unique_map_for_space(std::slice::from_ref(&one), "spaces/a"), Some(one.clone()));
    assert_eq!(unique_map_for_space(&[one, two], "spaces/a"), None);
}

/// Each tool's chord takes it up, whatever case the key is typed in.
#[test]
fn each_tool_has_its_own_chord() {
    for tool in BoardTool::ALL {
        assert_eq!(BoardTool::for_shortcut(&tool.shortcut().to_lowercase()), Some(tool));
    }
    assert_eq!(BoardTool::for_shortcut("9"), None);
}

/// A board set on a map with no build loaded, shown 768 pixels square in a 768 pixel element.
fn drawn_board() -> crate::tactics::board::Board {
    let mut board = crate::tactics::board::Board::new(None, wows_replay_insights::cap_layout::CapLayoutDb::default());
    board.set_map(MapChoice { map_id: Some(1), space: "spaces/01_solomon_islands".into(), label: "Solomon".into() });
    board.set_element(ElementSize { width: 768.0, height: 768.0 });
    board.drawn(Some((768.0, 768.0)));
    board
}

/// Draws a line from `from` to `to` with the tool in hand.
fn stroke(board: &mut crate::tactics::board::Board, from: (f32, f32), to: (f32, f32)) {
    use crate::tactics::board::Modifiers;

    board.pointer_down(ElementPos { x: from.0, y: from.1 }, Modifiers::default());
    board.pointer_move(ElementPos { x: to.0, y: to.1 }, true);
    board.pointer_up(ElementPos { x: to.0, y: to.1 });
}

/// A line drawn on the board is one change, which is taken back and put back again.
#[test]
fn a_drawn_line_is_taken_back_and_put_back() {
    let mut board = drawn_board();
    board.set_tool(BoardTool::Line.tool());
    stroke(&mut board, (100.0, 100.0), (300.0, 300.0));
    assert_eq!(board.annotation_count(), 1);
    assert!(board.can_undo());

    board.undo();
    assert_eq!(board.annotation_count(), 0);
    assert!(board.can_redo());

    board.redo();
    assert_eq!(board.annotation_count(), 1);
}

/// Taking up the tool already in hand puts it down.
#[test]
fn taking_up_the_tool_in_hand_puts_it_down() {
    let mut board = drawn_board();
    board.set_tool(BoardTool::Arrow.tool());
    assert!(board.has_tool());
    board.set_tool(BoardTool::Arrow.tool());
    assert!(!board.has_tool());
}

/// The chords take up their tools, step the nib and take a change back.
#[test]
fn the_chords_reach_the_board() {
    use crate::tactics::board::BoardKey;
    use crate::tactics::board::Modifiers;

    let mut board = drawn_board();
    let ctrl = Modifiers { secondary: true, shift: false };
    assert!(board.key(BoardKey::Text("4"), ctrl));
    assert_eq!(*board.tool(), BoardTool::Line.tool());

    let nib = board.nib();
    assert!(board.key(BoardKey::Text("]"), Modifiers::default()));
    assert_eq!(board.nib(), nib + 1.0);

    stroke(&mut board, (100.0, 100.0), (300.0, 300.0));
    assert!(board.key(BoardKey::Text("z"), ctrl));
    assert_eq!(board.annotation_count(), 0);
    assert!(board.key(BoardKey::Text("z"), Modifiers { secondary: true, shift: true }));
    assert_eq!(board.annotation_count(), 1);

    assert!(board.key(BoardKey::Escape, Modifiers::default()));
    assert!(!board.has_tool());
    assert!(!board.key(BoardKey::Text("q"), ctrl), "a key the board has no use for is left alone");
}

/// A picked shape is erased by Delete.
#[test]
fn a_picked_shape_is_erased_by_delete() {
    use crate::tactics::board::BoardKey;
    use crate::tactics::board::Modifiers;

    let mut board = drawn_board();
    board.set_tool(BoardTool::Line.tool());
    stroke(&mut board, (100.0, 100.0), (300.0, 300.0));
    board.set_tool(BoardTool::Line.tool());
    assert!(!board.has_tool());

    board.pointer_down(ElementPos { x: 200.0, y: 200.0 }, Modifiers::default());
    board.pointer_up(ElementPos { x: 200.0, y: 200.0 });
    assert!(board.key(BoardKey::Delete, Modifiers::default()));
    assert_eq!(board.annotation_count(), 0);
}

/// The nib stays within what the board draws with.
#[test]
fn the_nib_stays_in_its_range() {
    let mut board = drawn_board();
    for _ in 0..20 {
        board.step_nib(1.0);
    }
    assert_eq!(board.nib(), crate::tactics::MAX_NIB);
    for _ in 0..20 {
        board.step_nib(-1.0);
    }
    assert_eq!(board.nib(), crate::tactics::MIN_NIB);
}

/// A board with no map asks for nothing to be shown, and one with a map but no build asks for no
/// drawing it cannot make.
#[test]
fn a_redraw_needs_a_map_and_a_build() {
    use crate::tactics::board::Redraw;

    let mut board = crate::tactics::board::Board::new(None, wows_replay_insights::cap_layout::CapLayoutDb::default());
    board.request_redraw();
    assert!(matches!(board.take_redraw(), Some(Redraw::Clear)));
    assert!(board.take_redraw().is_none(), "asked once, answered once");

    let mut board = drawn_board();
    board.request_redraw();
    assert!(board.take_redraw().is_none(), "no build to draw with");
}
