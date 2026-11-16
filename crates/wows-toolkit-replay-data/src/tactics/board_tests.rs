//! The board driven as a front end drives it, in a session with no peer task behind it.

use std::sync::Arc;
use std::sync::mpsc::Receiver;

use parking_lot::Mutex;
use wows_replay_insights::cap_layout::CapLayoutDb;
use wows_toolkit_services::collab::CollabLink;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::vfs::MemoryFS;
use wowsunpack::vfs::VfsPath;
use wt_collab_client::PeerRole;
use wt_collab_client::SessionState;
use wt_collab_client::TacticsBoardSessionState;
use wt_collab_client::TacticsMapInfo;
use wt_collab_client::peer::LocalEvent;
use wt_collab_client::protocol::MAX_TACTICS_BOARDS;

use crate::load::GameDataCache;
use crate::load::LoadedGameData;
use crate::tactics::MapChoice;
use crate::tactics::board::Board;
use crate::tactics::board::BoardNotice;

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
