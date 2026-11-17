//! The board driven as a front end drives it, in a session with no peer task behind it.

use std::sync::Arc;
use std::sync::mpsc::Receiver;

use parking_lot::Mutex;
use wows_replay_insights::cap_layout::CapLayoutDb;
use wows_toolkit_services::collab::CollabLink;
use wows_toolkit_services::collab::UserId;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::vfs::MemoryFS;
use wowsunpack::vfs::VfsPath;
use wt_collab_client::PeerRole;
use wt_collab_client::SessionState;
use wt_collab_client::TacticsBoardSessionState;
use wt_collab_client::TacticsMapInfo;
use wt_collab_client::UserCursor;
use wt_collab_client::drawing::Tool;
use wt_collab_client::peer::LocalAnnotationEvent;
use wt_collab_client::peer::LocalEvent;
use wt_collab_client::protocol::MAX_TACTICS_BOARDS;
use wt_collab_client::types::Annotation;

use crate::load::GameDataCache;
use crate::load::LoadedGameData;
use crate::tactics::ElementPos;
use crate::tactics::ElementSize;
use crate::tactics::INKS;
use crate::tactics::MapChoice;
use crate::tactics::board::Board;
use crate::tactics::board::BoardKey;
use crate::tactics::board::BoardNotice;
use crate::tactics::board::Modifiers;
use crate::tactics::board::PickedShape;
use crate::tactics::board::ShapeSize;

/// This end's user in the session.
const ME: u64 = 1;
/// The peer who opened [`PEERS_BOARD`].
const PEER: u64 = 9;
const PEERS_BOARD: u64 = 42;

/// A build that has loaded, which is what an announcement waits for. It has no map art.
fn loaded_build() -> GameDataCache {
    let provider = GameMetadataProvider::from_params_no_specs(Vec::new()).expect("an empty provider builds");
    let vfs: VfsPath = MemoryFS::new().into();

    GameDataCache::holding_for_test(Arc::new(LoadedGameData::for_test(provider, vfs, 1)))
}

fn session(role: PeerRole) -> (Arc<Mutex<SessionState>>, CollabLink, Receiver<LocalEvent>) {
    let state = Arc::new(Mutex::new(SessionState::default()));
    {
        let mut held = state.lock();
        held.my_user_id = ME;
        held.role = role;
    }
    let (link, sent) = CollabLink::for_test(Arc::clone(&state));

    (state, link, sent)
}

/// Puts a board the peer opened in the session.
fn share_peers_board(state: &Mutex<SessionState>) {
    let board = TacticsBoardSessionState {
        owner_user_id: PEER,
        tactics_map: TacticsMapInfo {
            map_name: "spaces/13_OC_new_dawn".into(),
            display_name: "New Dawn".into(),
            map_id: 13,
            map_image_png: Vec::new(),
            map_info: None,
        },
        ..Default::default()
    };
    state.lock().tactics_boards.insert(PEERS_BOARD, board);
}

/// The boards this end has announced since the last look, by id.
fn announced(sent: &Receiver<LocalEvent>) -> Vec<u64> {
    sent.try_iter()
        .filter_map(|event| match event {
            LocalEvent::TacticsMapOpened { board_id, .. } => Some(board_id),
            _ => None,
        })
        .collect()
}

/// Hands the board the art an announcement asked for, as the front end does off the UI thread.
fn finish_announcing(board: &mut Board) {
    if let Some(request) = board.take_announcement() {
        board.finish_announce(request.load());
    }
}

/// A board with no map of its own, opened on the peer's board as the session is followed.
fn board_on_peers(link: CollabLink) -> Board {
    let mut board = Board::new(Some(loaded_build()), CapLayoutDb::default());
    board.set_collab(link);
    board.tick();
    assert!(board.is_a_peers(), "the board opened on the peer's");

    board
}

fn owner_of(state: &Mutex<SessionState>, board_id: u64) -> u64 {
    state.lock().tactics_boards[&board_id].owner_user_id
}

/// A board handed a link again while on a peer's board joins that board rather than announcing
/// it, which would make this reader its owner.
#[test]
fn a_new_link_leaves_a_peers_board_with_its_owner() {
    let (state, link, sent) = session(PeerRole::CoHost);
    share_peers_board(&state);
    let mut board = board_on_peers(link.clone());
    announced(&sent);

    board.set_collab(link);
    finish_announcing(&mut board);

    assert_eq!(announced(&sent), Vec::<u64>::new(), "nothing is announced");
    assert_eq!(owner_of(&state, PEERS_BOARD), PEER);
    assert!(board.is_a_peers(), "the board is still the peer's");
}

/// Setting a peer's board on another map makes it this reader's own board under a new id, and
/// leaves the peer's board with the peer.
#[test]
fn a_map_chosen_on_a_peers_board_is_announced_as_a_new_board() {
    let (state, link, sent) = session(PeerRole::CoHost);
    share_peers_board(&state);
    let mut board = board_on_peers(link);
    announced(&sent);

    board.set_map(MapChoice { map_id: Some(1), space: "spaces/01_solomon_islands".into(), label: "Solomon".into() });
    finish_announcing(&mut board);

    let [opened] = announced(&sent)[..] else { panic!("one board is announced") };
    assert_ne!(opened, PEERS_BOARD, "under an id of its own");
    assert_eq!(owner_of(&state, opened), ME);
    assert_eq!(owner_of(&state, PEERS_BOARD), PEER, "the peer's board stays the peer's");
    assert!(!board.is_a_peers());
}

/// A peer's board stays on its map when the session already holds every board it takes: the new
/// board it would become is one every peer drops, and the reader is told why.
#[test]
fn a_full_session_keeps_a_peers_board_on_its_map() {
    let (state, link, sent) = session(PeerRole::CoHost);
    share_peers_board(&state);
    for other in 1..MAX_TACTICS_BOARDS as u64 {
        let board = state.lock().tactics_boards[&PEERS_BOARD].clone();
        state.lock().tactics_boards.insert(PEERS_BOARD + other, board);
    }
    let mut board = board_on_peers(link);
    let was = board.map().cloned();
    announced(&sent);

    board.set_map(MapChoice { map_id: Some(1), space: "spaces/01_solomon_islands".into(), label: "Solomon".into() });
    finish_announcing(&mut board);

    assert_eq!(announced(&sent), Vec::<u64>::new(), "nothing is announced");
    assert_eq!(board.map().cloned(), was, "the board stays on the peer's map");
    assert!(board.is_a_peers());
    assert_eq!(board.take_notices(), [BoardNotice::SessionBoardsFull { limit: MAX_TACTICS_BOARDS }]);
}

/// A board with no session on a map, shown at the minimap's own size so an element point is the
/// same map point.
fn board_at_minimap_size() -> Board {
    let mut board = Board::new(None, CapLayoutDb::default());
    board.set_map(MapChoice { map_id: Some(1), space: "spaces/01_solomon_islands".into(), label: "Solomon".into() });
    board.set_element(ElementSize { width: 768.0, height: 768.0 });
    board.drawn(Some((768.0, 768.0)));

    board
}

/// Drags `tool` from `from` to `to`, then puts it down.
fn draw(board: &mut Board, tool: Tool, from: ElementPos, to: ElementPos) {
    board.set_tool(tool.clone());
    board.pointer_down(from, Modifiers::default());
    board.pointer_move(to, true);
    board.pointer_up(to);
    board.set_tool(tool);
}

/// Presses and lets go at `at` with nothing in hand, which picks out the shape there.
fn click(board: &mut Board, at: ElementPos) {
    board.pointer_down(at, Modifiers::default());
    board.pointer_up(at);
}

/// The first Escape puts the tool down and keeps the pick; the next puts the pick and placing a
/// capture point down too.
#[test]
fn escape_puts_the_tool_down_then_the_selection() {
    let mut board = board_at_minimap_size();
    let corner = ElementPos { x: 200.0, y: 200.0 };
    draw(&mut board, Tool::Rectangle { filled: false }, corner, ElementPos { x: 400.0, y: 300.0 });
    click(&mut board, corner);
    assert!(board.rotation_handle().is_some(), "the rectangle is picked out");

    board.set_tool(Tool::Line);
    board.key(BoardKey::Escape, Modifiers::default());
    assert_eq!(*board.tool(), Tool::None);
    assert!(board.rotation_handle().is_some(), "the pick outlasts the tool");

    board.set_adding(true);
    board.key(BoardKey::Escape, Modifiers::default());
    assert!(board.rotation_handle().is_none(), "the pick is put down");
    assert!(!board.adding(), "and so is placing a capture point");
}

/// The positions the session has been told of the reader's pointer since the last look.
fn told_cursor(sent: &Receiver<LocalEvent>) -> Vec<Option<[f32; 2]>> {
    sent.try_iter()
        .filter_map(|event| match event {
            LocalEvent::CursorPosition(at) => Some(at),
            _ => None,
        })
        .collect()
}

/// The reader's pointer is told to the session once a tick, at the last place it moved to, and
/// at once when it leaves the map.
#[test]
fn the_pointer_is_told_to_the_session_once_a_tick() {
    let (_state, link, sent) = session(PeerRole::Peer);
    let mut board = board_at_minimap_size();
    board.set_collab(link);

    for x in [100.0, 200.0, 300.0] {
        board.pointer_move(ElementPos { x, y: 400.0 }, false);
    }
    assert_eq!(told_cursor(&sent), Vec::<Option<[f32; 2]>>::new(), "nothing is said between ticks");

    board.tick();
    board.tick();
    assert_eq!(told_cursor(&sent), [Some([300.0, 400.0])], "one tick says the last place, once");

    board.pointer_left();
    assert_eq!(told_cursor(&sent), [None], "leaving is said at once");
}

/// A peer's pointer is placed on the board where it is on the map once the session is followed,
/// and goes when the session ends.
#[test]
fn a_peers_pointer_is_placed_on_the_map() {
    let (state, link, _sent) = session(PeerRole::Peer);
    let mut board = board_at_minimap_size();
    board.set_collab(link);
    state.lock().cursors.push(UserCursor {
        user_id: PEER,
        name: "Peer".to_owned(),
        color: [0, 200, 255],
        pos: Some([300.0, 400.0]),
        last_update: std::time::Instant::now(),
    });
    assert!(board.peer_cursors().is_empty(), "nothing is shown before the session is followed");

    board.tick();
    let [cursor] = &board.peer_cursors()[..] else { panic!("one pointer is shown") };
    assert_eq!(cursor.user_id, UserId::new(PEER));
    assert_eq!(cursor.name, "Peer");
    assert_eq!(cursor.at, ElementPos { x: 300.0, y: 400.0 });
    let revision = board.revision();
    board.tick();
    assert_eq!(board.revision(), revision, "a pointer that has not moved changes nothing");
    state.lock().cursors[0].pos = Some([310.0, 400.0]);
    board.tick();
    assert_ne!(board.revision(), revision, "one that has moved does");

    board.set_collab(CollabLink::default());
    assert!(board.peer_cursors().is_empty(), "a session that ended shows no one");
}

/// The shapes the session has been told of since the last look.
fn told_shapes(sent: &Receiver<LocalEvent>) -> Vec<Annotation> {
    sent.try_iter()
        .filter_map(|event| match event {
            LocalEvent::Annotation(LocalAnnotationEvent::Set { annotation, .. }) => Some(annotation),
            _ => None,
        })
        .collect()
}

/// A board in a session, announced, so what is drawn on it is the session's.
fn board_in_session(link: CollabLink) -> Board {
    let mut board = board_at_minimap_size();
    board.set_game_data(Some(loaded_build()));
    board.set_collab(link);
    finish_announcing(&mut board);
    assert!(board.in_session());

    board
}

/// The picked shape's ink, size and fill change on the board and reach the session, each as a
/// step that can be taken back.
#[test]
fn a_picked_shape_is_edited_through_the_session() {
    let (_state, link, sent) = session(PeerRole::Host);
    let mut board = board_in_session(link);
    let corner = ElementPos { x: 200.0, y: 200.0 };
    draw(&mut board, Tool::Rectangle { filled: false }, corner, ElementPos { x: 400.0, y: 300.0 });
    click(&mut board, corner);
    told_shapes(&sent);
    let (ink, _) = INKS[3];

    board.set_picked_color(ink);
    board.set_picked_size(ShapeSize::MapUnits(150.0));
    board.set_picked_filled(true);

    let Some(PickedShape::Outline { color, size: ShapeSize::MapUnits(size), filled }) = board.picked_shape() else {
        panic!("the rectangle is still picked: {:?}", board.picked_shape());
    };
    assert_eq!((color, filled), (ink, true));
    assert!((size - 150.0).abs() < 1e-3, "resized to {size}");
    let told = told_shapes(&sent);
    assert_eq!(told.len(), 3, "each change is told");
    let Some(Annotation::Rectangle { half_size, .. }) = told.last() else { panic!("the rectangle is told") };
    assert!((half_size[0] / half_size[1] - 2.0).abs() < 1e-3, "it keeps its shape: {half_size:?}");

    board.undo();
    assert!(matches!(board.shapes()[..], [Annotation::Rectangle { filled: false, .. }]), "the fill is taken back");
}

/// A placed ship picked out alone changes side, and the session hears of it.
#[test]
fn a_picked_ship_changes_side() {
    let (_state, link, sent) = session(PeerRole::Host);
    let mut board = board_in_session(link);
    let at = ElementPos { x: 300.0, y: 300.0 };
    let ship = Tool::Ship { species: "Destroyer".to_owned(), friendly: true, yaw: 0.0 };
    board.set_tool(ship.clone());
    click(&mut board, at);
    board.set_tool(ship);
    click(&mut board, at);
    assert_eq!(board.picked_shape(), Some(PickedShape::Ship { friendly: true }));
    told_shapes(&sent);

    board.set_picked_friendly(false);

    assert_eq!(board.picked_shape(), Some(PickedShape::Ship { friendly: false }));
    assert!(matches!(told_shapes(&sent)[..], [Annotation::Ship { friendly: false, .. }]), "the session is told");
}
