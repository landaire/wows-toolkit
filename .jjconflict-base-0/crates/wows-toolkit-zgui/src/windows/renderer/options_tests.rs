//! The renderer's gear, timeline, ship menu, hover readings and export notice, driven through
//! their controls.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use wows_minimap_renderer::CANVAS_HEIGHT;
use wows_minimap_renderer::MINIMAP_SIZE;
use wows_minimap_renderer::ShipVisibility;
use wows_minimap_renderer::advantage::AdvantageBreakdown;
use wows_minimap_renderer::draw_command::ConsumableLines;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_minimap_renderer::map_data::MinimapPos;
use wows_replay_insights::timeline::TimelineEvent;
use wows_replay_insights::timeline::TimelineEventKind;
use wows_replay_insights::timeline::kind_index;
use wows_replays::types::ElapsedClock;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wows_toolkit_replay_data::replay_render::export::ExportTarget;
use wows_toolkit_replay_data::replay_render::hover::ConsumableHover;
use wows_toolkit_replay_data::replay_render::playback::Playback;
use wows_toolkit_replay_data::replay_render::timeline::BattleEvents;
use wows_toolkit_replay_data::replay_render::track::DEFAULT_MAP_ORIGIN;
use wows_toolkit_replay_data::replay_render::track::Track;
use wows_toolkit_replay_data::replay_render::view::ElementPoint;
use wows_toolkit_replay_data::replay_render::view::FrameSize;
use wows_toolkit_replay_data::replay_render::view::WindowPoint;
use wowsunpack::data::Version;
use wowsunpack::game_types::AdvantageLevel;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::WithUntracked;
use zgui::view;
use zgui::vocab::Modifiers;
use zgui::vocab::NamedKey;

use crate::services::Services;
use crate::windows::renderer::session::HoveredConsumable;
use crate::windows::renderer::session::RendererSession;
use crate::windows::renderer::session::Status;
use crate::windows::renderer::view::ReplayRendererProps;

/// A track of empty frames drawn at `clocks`, with the battle starting at `battle_start`.
fn track(clocks: &[f32], battle_start: f32) -> Track {
    Track {
        frames: vec![Vec::new(); clocks.len()],
        clocks: clocks.iter().copied().map(GameClock).collect(),
        battle_start: GameClock(battle_start),
        battle_end: None,
        space_size: 1400.0,
        version: Version::base(99, 0, 0),
        map_origin: DEFAULT_MAP_ORIGIN,
        space: String::new(),
    }
}

/// Half-second frames over `seconds` of recording.
fn half_seconds(seconds: usize) -> Vec<f32> {
    (0..seconds * 2).map(|frame| frame as f32 * 0.5).collect()
}

/// Events read off a battle, one advantage change at each elapsed clock given.
fn events(at: &[f32]) -> BattleEvents {
    let read = at
        .iter()
        .map(|seconds| TimelineEvent {
            clock: ElapsedClock(*seconds),
            kind: TimelineEventKind::AdvantageChanged { label: format!("at {seconds}"), is_friendly: true },
        })
        .collect();

    BattleEvents::from_read(read, None, HashMap::new())
}

/// A renderer over `track` in a headless window, and its session.
fn open(track: Track) -> (HeadlessWindow, RendererSession) {
    let handle = Rc::new(Cell::new(None));
    let built = Rc::clone(&handle);
    let mut track = Some(track);
    let window = HeadlessWindow::open(HeadlessOptions::new(WindowSize { width: 1200.0, height: 800.0 }), move || {
        Services::provide(None);
        let session = RendererSession::ready_for_test(track.take().expect("the window is built once"));
        built.set(Some(session));
        // A host the size of the window, as a tab or a window of its own gives the renderer.
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

/// Says a frame of the canvas's size landed, which is what places the map in the window.
fn land_a_frame(window: &mut HeadlessWindow, session: RendererSession) {
    session.frame_size.set(Some(FrameSize { width: MINIMAP_SIZE, height: CANVAS_HEIGHT }));
    settle(window);
}

fn settle(window: &mut HeadlessWindow) {
    window.advance(Duration::from_millis(50));
}

fn click(window: &mut HeadlessWindow, test_id: &str) {
    let element = window.by_test_id(test_id).unwrap_or_else(|| panic!("{test_id} is drawn"));
    window.click(element).expect("the element has a box");
    settle(window);
}

fn shown(window: &HeadlessWindow, test_id: &str) -> bool {
    window.by_test_id(test_id).is_some()
}

fn is_refused(window: &mut HeadlessWindow, test_id: &str) -> bool {
    let element = window.by_test_id(test_id).unwrap_or_else(|| panic!("{test_id} is drawn"));
    window.a11y(element).expect("the control is in the accessibility tree").disabled
}

/// Moves the keyboard focus forward until it is on `test_id`, which brings it into view.
fn tab_to(window: &mut HeadlessWindow, test_id: &str) {
    const MOST_STOPS: usize = 64;

    for _ in 0..MOST_STOPS {
        let element = window.by_test_id(test_id).unwrap_or_else(|| panic!("{test_id} is drawn"));
        if window.a11y(element).expect("the control is in the accessibility tree").focused {
            settle(window);
            return;
        }
        window.press_key(NamedKey::Tab, Modifiers::NONE);
    }

    panic!("{test_id} is never reached with the keyboard");
}

fn elapsed(session: RendererSession) -> f32 {
    let track = session.track_untracked().expect("the track is here");
    session.playback.with_untracked(|playback| playback.elapsed(&track))
}

/// The gear opens, and the switches inside it reach what the viewport draws.
#[test]
fn the_gear_opens_and_its_switches_reach_the_frame() {
    let (mut window, session) = open(track(&[0.0, 30.0], 0.0));
    assert!(!shown(&window, "renderer-opt-team-rosters"), "nothing is open yet");

    click(&mut window, "replay-renderer-settings-toggle");
    for switch in ["renderer-opt-hp-bars", "renderer-opt-trails", "renderer-opt-team-rosters"] {
        assert!(shown(&window, switch), "{switch} is in the open gear");
    }

    let was = session.layers.with_untracked(|layers| layers.options.show_hp_bars);
    click(&mut window, "renderer-opt-hp-bars");
    assert_eq!(
        session.layers.with_untracked(|layers| layers.options.show_hp_bars),
        !was,
        "the switch reaches what is drawn"
    );
}

/// The export settings under the switches reach what the next export is encoded with.
#[test]
fn the_export_settings_reach_the_next_export() {
    let (mut window, session) = open(track(&[0.0, 30.0], 0.0));
    click(&mut window, "replay-renderer-settings-toggle");

    assert!(!session.export.with_untracked(|export| export.settings.prefer_cpu));
    // The export settings sit under the layer switches, past the bottom of the gear's scrolled
    // panel; the reader moves to them first, and moving the focus there scrolls them into view.
    tab_to(&mut window, "replay-renderer-prefer-cpu");
    click(&mut window, "replay-renderer-prefer-cpu");
    assert!(session.export.with_untracked(|export| export.settings.prefer_cpu));

    tab_to(&mut window, "replay-renderer-include-pre-battle");
    click(&mut window, "replay-renderer-include-pre-battle");
    assert!(session.export.with_untracked(|export| export.settings.include_pre_battle));

    assert!(shown(&window, "replay-renderer-codec-auto"), "the encoder's own pick is always offered");
    assert!(shown(&window, "replay-renderer-save-defaults"));
}

/// The gear is refused until the bake lands, so the options it is baking under are never saved
/// over the reader's.
#[test]
fn the_gear_is_refused_until_the_track_is_here() {
    let (mut window, session) = open(track(&[0.0, 30.0], 0.0));
    session.status.set(Status::Baking);
    settle(&mut window);

    assert!(is_refused(&mut window, "replay-renderer-settings-toggle"));
}

/// The timeline opens and lists what happened.
#[test]
fn the_timeline_opens_and_lists_the_battle() {
    let (mut window, session) = open(track(&half_seconds(60), 0.0));
    session.events.set(events(&[10.0, 30.0, 50.0]));

    click(&mut window, "replay-renderer-timeline-toggle");
    assert!(shown(&window, "replay-renderer-timeline-list"), "the list is open");
    assert!(shown(&window, "replay-renderer-timeline-row-0"), "with a row per event");
    assert!(shown(&window, "replay-renderer-timeline-row-2"));
    assert!(!shown(&window, "replay-renderer-timeline-row-3"));
}

/// Until the second walk of the replay lands, the timeline says it is reading rather than
/// listing nothing.
#[test]
fn the_timeline_waits_for_the_battle_to_be_read() {
    let (mut window, session) = open(track(&half_seconds(60), 0.0));

    click(&mut window, "replay-renderer-timeline-toggle");
    assert!(!shown(&window, "replay-renderer-timeline-list"));
    assert!(is_refused(&mut window, "replay-renderer-timeline-copy"), "there is nothing to copy");

    session.events.set(events(&[10.0]));
    settle(&mut window);
    assert!(shown(&window, "replay-renderer-timeline-row-0"), "the list arrives with the events");
    assert!(!is_refused(&mut window, "replay-renderer-timeline-copy"));
}

/// The kind switches and the search decide what the list shows.
#[test]
fn the_timeline_filter_decides_what_the_list_shows() {
    let (mut window, session) = open(track(&half_seconds(100), 0.0));
    session.events.set(events(&[10.0, 30.0, 60.0]));
    click(&mut window, "replay-renderer-timeline-toggle");

    // Every seeded event is an advantage change, so switching that kind off empties the list
    // without emptying the battle.
    let advantage = kind_index(&TimelineEventKind::AdvantageChanged { label: String::new(), is_friendly: true });
    click(&mut window, &format!("replay-renderer-timeline-kind-{advantage}"));
    assert!(!shown(&window, "replay-renderer-timeline-list"));
    assert!(!session.events.with_untracked(|events| events.filter().kinds[advantage]));

    click(&mut window, "replay-renderer-timeline-all");
    assert!(shown(&window, "replay-renderer-timeline-row-2"), "every kind is back");

    click(&mut window, "replay-renderer-timeline-search");
    window.type_text("at 30");
    settle(&mut window);
    assert_eq!(session.events.with_untracked(|events| events.filter().search.clone()), "at 30");
    assert!(shown(&window, "replay-renderer-timeline-row-0"));
    assert!(!shown(&window, "replay-renderer-timeline-row-1"), "the search keeps one row");
    let marked = window.texts_by_class(landaire_ui::MARK_CLASS);
    assert!(!marked.is_empty() && marked.iter().all(|piece| piece == "at 30"), "{marked:?}");
}

/// Clicking a row moves playback to when it happened, on the battle's own clock.
#[test]
fn clicking_a_timeline_row_seeks_to_it() {
    // The battle starts forty seconds into a hundred seconds of recording.
    let (mut window, session) = open(track(&half_seconds(100), 40.0));
    session.events.set(events(&[10.0, 30.0]));
    click(&mut window, "replay-renderer-timeline-toggle");

    click(&mut window, "replay-renderer-timeline-row-1");
    assert_eq!(elapsed(session), 30.0);
}

/// The event controls are refused until the battle has been read, rather than doing nothing and
/// looking broken.
#[test]
fn the_event_controls_refuse_until_the_battle_has_been_read() {
    let (mut window, session) = open(track(&half_seconds(60), 0.0));
    assert!(is_refused(&mut window, "replay-renderer-next-event"), "refused while nothing is read");
    assert!(is_refused(&mut window, "replay-renderer-previous-event"));

    click(&mut window, "replay-renderer-next-event");
    assert_eq!(session.playback.with_untracked(Playback::at).get(), 0, "and clicking it moves nothing");

    session.events.set(events(&[10.0, 30.0]));
    settle(&mut window);
    assert!(!is_refused(&mut window, "replay-renderer-next-event"), "and takes a click once the battle is read");

    click(&mut window, "replay-renderer-next-event");
    assert_eq!(elapsed(session), 10.0, "which steps to the first thing that happened");
}

/// A ship drawn where the viewport's middle lands, named `gapedd`.
fn ship_in_the_middle(window: &mut HeadlessWindow, session: RendererSession) {
    land_a_frame(window, session);
    let viewport = window.by_test_id("replay-renderer-viewport").expect("the map is drawn");
    let bounds = window.bounds(viewport).expect("the map has a box");
    let middle = WindowPoint {
        x: bounds.origin.x.0 + bounds.size.width.0 / 2.0,
        y: bounds.origin.y.0 + bounds.size.height.0 / 2.0,
    };
    let origin = session.map_origin.get_untracked();
    let (x, y) = session.map.with_untracked(|map| map.map_point(middle, origin)).expect("the middle is on the map");

    let mut shown = track(&[0.0, 0.5], 0.0);
    shown.frames[0] = vec![DrawCommand::Ship {
        entity_id: EntityId::from(7u32),
        pos: MinimapPos { x, y },
        yaw: 0.0,
        species: None,
        color: None,
        visibility: ShipVisibility::Visible,
        opacity: 1.0,
        is_self: false,
        player_name: Some("gapedd".into()),
        ship_name: None,
        is_detected_teammate: false,
        is_disconnected: false,
        name_color: None,
    }];
    session.status.set(Status::Ready(Arc::new(shown)));
    settle(window);
}

fn open_ship_menu(window: &mut HeadlessWindow) {
    let viewport = window.by_test_id("replay-renderer-viewport").expect("the map is drawn");
    window.right_click(viewport).expect("the map has a box");
    settle(window);
}

/// A right-click on a ship opens its menu, whose switches reach whose layers are drawn.
#[test]
fn the_ship_menu_opens_on_the_ship_and_its_switches_reach_the_frame() {
    let (mut window, session) = open(track(&[0.0, 0.5], 0.0));
    ship_in_the_middle(&mut window, session);

    open_ship_menu(&mut window);
    let picked = session.menu.with_untracked(|menu| menu.as_ref().map(|menu| menu.player_name.clone()));
    assert_eq!(picked.as_deref(), Some("gapedd"));
    assert!(shown(&window, "replay-renderer-ship-menu"));
    assert!(!shown(&window, "replay-renderer-ship-armor"), "no hits were read, so there is no armor to show");

    click(&mut window, "replay-renderer-range-detection");
    assert!(session.choices.with_untracked(|choices| choices.ranges_for("gapedd").detection));
    assert!(
        session.layers.with_untracked(|layers| layers.options.show_ship_config),
        "asking for one turns the layer on"
    );

    click(&mut window, "replay-renderer-range-detection");
    assert!(!session.layers.with_untracked(|layers| layers.options.show_ship_config), "the last one off puts it away");

    // The only drawn ship hides its trail, which is the last of them.
    click(&mut window, "replay-renderer-ship-trail");
    assert!(!session.choices.with_untracked(|choices| choices.trail_shown("gapedd")));
    assert!(!session.layers.with_untracked(|layers| layers.options.show_trails));

    click(&mut window, "replay-renderer-ship-menu-close");
    assert!(session.menu.with_untracked(Option::is_none));
    assert!(!shown(&window, "replay-renderer-ship-menu"));
}

/// Escape closes the ship menu.
#[test]
fn escape_closes_the_ship_menu() {
    let (mut window, session) = open(track(&[0.0, 0.5], 0.0));
    ship_in_the_middle(&mut window, session);
    open_ship_menu(&mut window);
    assert!(shown(&window, "replay-renderer-ship-menu"));

    window.press_key(NamedKey::Escape, Modifiers::NONE);
    settle(&mut window);
    assert!(session.menu.with_untracked(Option::is_none));
    assert!(!shown(&window, "replay-renderer-ship-menu"));
}

/// The HUD strip holds the advantage's breakdown, and only while the frame carries one.
#[test]
fn the_advantage_hover_reads_the_frames_own_breakdown() {
    let (mut window, session) = open(track(&[0.0, 0.5], 0.0));
    land_a_frame(&mut window, session);
    assert!(!shown(&window, "replay-renderer-advantage-hover"), "a frame with no advantage has nothing to say");

    let mut advantaged = track(&[0.0, 0.5], 0.0);
    advantaged.frames[0] = vec![DrawCommand::TeamAdvantage {
        level: Some(AdvantageLevel::Weak),
        color: [255, 255, 255],
        breakdown: AdvantageBreakdown {
            score_projection: (6.0, 2.0),
            total: (6.0, 2.0),
            hp_data_reliable: false,
            ..Default::default()
        },
    }];
    session.status.set(Status::Ready(Arc::new(advantaged)));
    settle(&mut window);

    let strip = window.by_test_id("replay-renderer-advantage-hover").expect("the strip is over the HUD");
    window.hover(strip).expect("the strip has a box");
    window.advance(Duration::from_secs(1));
    assert!(shown(&window, "replay-renderer-advantage-lines"), "resting on the strip reads the breakdown");
}

/// The consumable under the pointer is read beside it, and the reading goes with the pointer.
#[test]
fn a_hovered_consumable_is_read_beside_the_pointer() {
    let (mut window, session) = open(track(&[0.0, 0.5], 0.0));
    land_a_frame(&mut window, session);
    assert!(!shown(&window, "replay-renderer-consumable-reading"));

    session.hovered.set(Some(HoveredConsumable {
        hover: ConsumableHover {
            player: "gapedd".into(),
            lines: ConsumableLines {
                name: "Damage Control".into(),
                charges: "3 charges".into(),
                timing: None,
                active: None,
                description: String::new(),
            },
        },
        at: ElementPoint { x: 40.0, y: 40.0 },
    }));
    settle(&mut window);
    let reading = window.by_test_id("replay-renderer-consumable-reading").expect("the reading is up");
    let viewport = window.by_test_id("replay-renderer-viewport").expect("the map is drawn");
    let (reading, viewport) =
        (window.bounds(reading).expect("the reading has a box"), window.bounds(viewport).expect("the map has a box"));
    assert!(reading.origin.x.0 > viewport.origin.x.0 + 40.0, "beside the pointer, not over it");

    session.hovered.set(None);
    settle(&mut window);
    assert!(!shown(&window, "replay-renderer-consumable-reading"));
}

/// An export waiting on the software-encode notice is dropped when the reader cancels.
#[test]
fn cancelling_the_software_encode_notice_drops_the_export() {
    let (mut window, session) = open(track(&[0.0, 0.5], 0.0));
    session.export_notice.set(Some(ExportTarget::File));
    settle(&mut window);
    assert!(shown(&window, "notice-software-encode"));

    click(&mut window, "notice-cancel");
    assert!(session.export_notice.with_untracked(Option::is_none));
    assert!(!shown(&window, "notice-software-encode"));
}
