//! A renderer's layers following a collab session's display settings, and telling it of its own.
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
use wows_minimap_renderer::ShipVisibility;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_minimap_renderer::map_data::MinimapPos;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wows_toolkit_replay_data::replay_render::ships::ALL_RANGES;
use wows_toolkit_replay_data::replay_render::track::DEFAULT_MAP_ORIGIN;
use wows_toolkit_replay_data::replay_render::track::Track;
use wows_toolkit_services::collab::CollabLink;
use wowsunpack::data::Version;
use wt_collab_client::PeerRole;
use wt_collab_client::peer::LocalEvent;
use wt_collab_client::protocol::CollabRenderOptions;
use wt_collab_client::protocol::DisplayOptionField;
use zgui::reactive::prelude::WithUntracked;
use zgui::view;

use crate::services::Services;
use crate::windows::renderer::session::RendererSession;
use crate::windows::renderer::view::ReplayRendererProps;

const SEVEN: u32 = 7;

/// Two frames with one ship in them, entity 7, played by "seven".
fn track() -> Track {
    let ship = DrawCommand::Ship {
        entity_id: EntityId::from(SEVEN),
        pos: MinimapPos { x: 100.0, y: 100.0 },
        yaw: 0.0,
        species: None,
        color: None,
        visibility: ShipVisibility::Visible,
        opacity: 1.0,
        is_self: false,
        player_name: Some("seven".into()),
        ship_name: None,
        is_detected_teammate: false,
        is_disconnected: false,
        name_color: None,
    };
    Track {
        frames: vec![vec![ship.clone()], vec![ship]],
        clocks: vec![GameClock(0.0), GameClock(0.5)],
        battle_start: GameClock(0.0),
        battle_end: None,
        space_size: 1400.0,
        version: Version::base(99, 0, 0),
        map_origin: DEFAULT_MAP_ORIGIN,
        space: String::new(),
        video_stem: String::new(),
        trails: Default::default(),
    }
}

/// A renderer over `track()` in a headless window, and its session.
fn open() -> (HeadlessWindow, RendererSession) {
    let handle = Rc::new(Cell::new(None));
    let built = Rc::clone(&handle);
    let window = HeadlessWindow::open(HeadlessOptions::new(WindowSize { width: 1200.0, height: 800.0 }), move || {
        Services::provide(None);
        let session = RendererSession::ready_for_test(track());
        built.set(Some(session));
        view! {
            column(style:width = "1200px", style:height = "800px") {
                ReplayRenderer(session = session)
            }
        }
    })
    .expect("the renderer opens headlessly");
    let session = handle.get().expect("the renderer was built");

    (window, session)
}

/// Long enough for a running session to have looked at the display settings again.
fn tick(window: &mut HeadlessWindow) {
    window.advance(Duration::from_millis(100));
}

fn click(window: &mut HeadlessWindow, test_id: &str) {
    let element = window.by_test_id(test_id).unwrap_or_else(|| panic!("{test_id} is drawn"));
    window.click(element).expect("the element has a box");
    window.advance(Duration::from_millis(50));
}

fn shows_timer(session: RendererSession) -> bool {
    session.layers.with_untracked(|layers| layers.options.show_timer)
}

fn shows_hp_bars(session: RendererSession) -> bool {
    session.layers.with_untracked(|layers| layers.options.show_hp_bars)
}

/// The host turning a layer off reaches a peer's renderer, over what the peer had on.
#[test]
fn a_peer_takes_up_the_hosts_layers() {
    let (mut window, session) = open();
    let state = Arc::new(Default::default());
    let (link, _sent) = CollabLink::for_test(Arc::clone(&state));
    state.lock().role = PeerRole::Peer;
    session.set_collab(link);
    assert!(shows_timer(session), "the timer is drawn at first");

    let hosts = session.layers.with_untracked(|layers| {
        let mut options = layers.options.clone();
        options.show_timer = false;
        CollabRenderOptions::from_render_options(&options, layers.show_dead_ships)
    });
    {
        let mut held = state.lock();
        held.current_render_options = Some(hosts);
        held.render_options_version = 1;
    }
    tick(&mut window);

    assert!(!shows_timer(session), "the host's layers are drawn");
}

/// A toggle on the renderer's gear tells the session the one layer that changed, and the host
/// holds what it now shows.
#[test]
fn a_toggle_tells_the_session_what_changed() {
    let (mut window, session) = open();
    let state = Arc::new(Default::default());
    let (link, sent) = CollabLink::for_test(Arc::clone(&state));
    state.lock().role = PeerRole::Host;
    session.set_collab(link);
    // A session withdraws the export controls, which moves the rest of the transport.
    tick(&mut window);

    click(&mut window, "replay-renderer-settings-toggle");
    click(&mut window, "renderer-opt-timer");

    let said: Vec<_> = sent
        .try_iter()
        .filter_map(|event| match event {
            LocalEvent::DisplayToggle(field, value) => Some((field, value)),
            _ => None,
        })
        .collect();
    assert_eq!(said, vec![(DisplayOptionField::ShowTimer, false)]);
    let held = state.lock().current_render_options.clone().expect("the host holds its layers");
    assert!(!held.show_timer);
}

/// A peer's toggle reaching the host changes that one layer of the host's renderer, and leaves
/// the rest as the host had them.
#[test]
fn a_peers_toggle_changes_one_layer_on_the_host() {
    let (mut window, session) = open();
    let state = Arc::new(Default::default());
    let (link, _sent) = CollabLink::for_test(Arc::clone(&state));
    state.lock().role = PeerRole::Host;
    session.set_collab(link);
    let had_hp_bars = shows_hp_bars(session);

    {
        let mut held = state.lock();
        held.current_render_options
            .as_mut()
            .expect("the host holds its layers")
            .set_field(DisplayOptionField::ShowTimer, false);
        held.render_options_version = 1;
    }
    tick(&mut window);

    assert!(!shows_timer(session), "the peer's toggle is drawn");
    assert_eq!(shows_hp_bars(session), had_hp_bars, "and nothing else changed");
}

/// Ranges a session sets by entity land on that entity's player, and per-ship choices made here
/// are sent the same way.
#[test]
fn ship_ranges_travel_by_entity() {
    let (mut window, session) = open();
    let state = Arc::new(Default::default());
    let (link, sent) = CollabLink::for_test(Arc::clone(&state));
    session.set_collab(link);

    {
        let mut held = state.lock();
        held.current_range_overrides = Some(vec![(EntityId::from(SEVEN), ALL_RANGES)]);
        held.range_override_version = 1;
    }
    tick(&mut window);
    assert_eq!(session.choices.with_untracked(|choices| choices.ranges_for("seven")), ALL_RANGES);
    assert!(session.layers.with_untracked(|layers| layers.options.show_ship_config), "the range layer is on");

    session.choose(|choices, _, options| choices.set_every_range_for("seven", false, options));
    let said: Vec<_> = sent
        .try_iter()
        .filter_map(|event| match event {
            LocalEvent::RangeOverrides(overrides) => Some(overrides),
            _ => None,
        })
        .collect();
    assert_eq!(said, vec![Vec::new()], "a ship showing no ranges is left out");
}
