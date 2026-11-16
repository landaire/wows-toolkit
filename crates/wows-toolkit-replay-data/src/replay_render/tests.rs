use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use wows_minimap_renderer::CANVAS_HEIGHT;
use wows_minimap_renderer::EncoderKind;
use wows_minimap_renderer::MINIMAP_SIZE;
use wows_minimap_renderer::RenderOptions;
use wows_minimap_renderer::SavedRenderOptions;
use wows_minimap_renderer::ShipVisibility;
use wows_minimap_renderer::VideoCodec;
use wows_minimap_renderer::advantage::AdvantageBreakdown;
use wows_minimap_renderer::config::should_draw_command;
use wows_minimap_renderer::draw_command::AnnotationShape;
use wows_minimap_renderer::draw_command::ChargeCount;
use wows_minimap_renderer::draw_command::ConsumableAvailability;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_minimap_renderer::draw_command::HitPoints;
use wows_minimap_renderer::draw_command::RosterConsumable;
use wows_minimap_renderer::draw_command::RosterRow;
use wows_minimap_renderer::draw_command::RosterSide;
use wows_minimap_renderer::draw_command::ShipConfigCircleKind;
use wows_minimap_renderer::drawing::DrawnRegion;
use wows_minimap_renderer::drawing::RegionKind;
use wows_minimap_renderer::drawing::SidePanelLayout;
use wows_minimap_renderer::encoder::CodecSupport;
use wows_minimap_renderer::encoder::EncoderStatus;
use wows_minimap_renderer::map_data::MinimapPos;
use wows_minimap_renderer::toggles::TOGGLES;
use wows_minimap_renderer::toggles::Toggle;
use wows_minimap_renderer::trail::BakedTrails;
use wows_minimap_renderer::viewport::MapViewport;
use wows_replay_insights::timeline::HealthSnapshot;
use wows_replay_insights::timeline::KIND_COUNT;
use wows_replay_insights::timeline::ShipShotTimeline;
use wows_replay_insights::timeline::TimelineEvent;
use wows_replay_insights::timeline::TimelineEventKind;
use wows_replay_insights::timeline::kind_index;
use wows_replays::types::ElapsedClock;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wows_replays::types::PlaneId;
use wows_toolkit_services::collab::CollabLink;
use wows_toolkit_services::collab::ReplayId;
use wows_toolkit_services::collab::push_ping_aged;
use wowsunpack::data::Version;
use wowsunpack::game_params::types::Species;
use wowsunpack::game_types::AdvantageLevel;
use wt_collab_client::AnnotationSyncState;
use wt_collab_client::SessionState;
use wt_collab_client::drawing::Stroke;
use wt_collab_client::drawing::Tool;
use wt_collab_client::peer::LocalAnnotationEvent;
use wt_collab_client::peer::LocalEvent;
use wt_collab_client::types::Annotation;

use crate::load::GameDataCache;
use crate::replay_render::annotate::Annotator;
use crate::replay_render::annotate::Click;
use crate::replay_render::annotate::MAX_NIB;
use crate::replay_render::annotate::MIN_NIB;
use crate::replay_render::annotate::PointerDown;
use crate::replay_render::annotate::PointerUp;
use crate::replay_render::annotate::TOOLS;
use crate::replay_render::annotate::tool_for_key;
use crate::replay_render::armor::Advance;
use crate::replay_render::armor::ArmorFollow;
use crate::replay_render::armor::ArmorFollowed;
use crate::replay_render::armor::RealtimeArmorFeed;
use crate::replay_render::armor::has_armor_to_show;
use crate::replay_render::collab::PING_SECONDS;
use crate::replay_render::collab::adopt_range_overrides;
use crate::replay_render::collab::adopt_trail_overrides;
use crate::replay_render::collab::frame_broadcast_with_trails;
use crate::replay_render::collab::range_overrides;
use crate::replay_render::export::BatchStep;
use crate::replay_render::export::EXPORT_FRAME_COUNT;
use crate::replay_render::export::ExportFrames;
use crate::replay_render::export::ExportLayers;
use crate::replay_render::export::ExportProgress;
use crate::replay_render::export::ExportSettings;
use crate::replay_render::export::ExportStage;
use crate::replay_render::export::batch_export;
use crate::replay_render::export::batch_layers;
use crate::replay_render::export::export_settings_of;
use crate::replay_render::export::must_use_cpu;
use crate::replay_render::hover::advantage_hover;
use crate::replay_render::hover::consumable_at;
use crate::replay_render::motion::FrameMotion;
use crate::replay_render::options::adopt_saved_defaults;
use crate::replay_render::options::adopt_saved_self_ranges;
use crate::replay_render::options::batch_options;
use crate::replay_render::options::defaults_to_save;
use crate::replay_render::options::playback_options;
use crate::replay_render::options::playback_options_hidden;
use crate::replay_render::playback::FRAME_INTERVAL;
use crate::replay_render::playback::Playback;
use crate::replay_render::playback::SPEEDS;
use crate::replay_render::playback::Tick;
use crate::replay_render::playback::mmss;
use crate::replay_render::raster::raster_commands;
use crate::replay_render::ships::ALL_RANGES;
use crate::replay_render::ships::NO_RANGES;
use crate::replay_render::ships::ShipChoices;
use crate::replay_render::ships::ShipMenu;
use crate::replay_render::ships::per_ship_allows;
use crate::replay_render::ships::self_player;
use crate::replay_render::ships::ship_at;
use crate::replay_render::ships::ship_names;
use crate::replay_render::timeline::BattleEvents;
use crate::replay_render::track::BAKE_INTERVAL;
use crate::replay_render::track::DEFAULT_MAP_ORIGIN;
use crate::replay_render::track::FrameIndex;
use crate::replay_render::track::TRACK_BUDGET;
use crate::replay_render::track::Track;
use crate::replay_render::view::ElementBounds;
use crate::replay_render::view::FrameSize;
use crate::replay_render::view::MapView;
use crate::replay_render::view::Placement;
use crate::replay_render::view::Wheel;
use crate::replay_render::view::WindowPoint;

/// A track holding `clocks`' worth of empty frames, with the battle starting
/// at the first of them.
fn track(clocks: Vec<f32>) -> Track {
    Track {
        frames: vec![Vec::new(); clocks.len()],
        clocks: clocks.into_iter().map(GameClock).collect(),
        battle_start: GameClock(0.0),
        battle_end: None,
        // A middling map, so a range in metres lands somewhere sensible on
        // the minimap.
        space_size: 1400.0,
        version: Version::base(99, 0, 0),
        space: String::new(),
        video_stem: String::new(),
        map_origin: DEFAULT_MAP_ORIGIN,
        trails: BakedTrails::default(),
    }
}

/// What frame `at` of `track` draws, as playback draws it when it is on that frame.
fn drawn_at(
    track: &Track,
    at: FrameIndex,
    options: &RenderOptions,
    show_dead_ships: bool,
    choices: &ShipChoices,
    drawn_on: Vec<DrawCommand>,
) -> Option<Vec<DrawCommand>> {
    let motion = FrameMotion::between(track, at)?;

    Some(raster_commands(track, &motion, GameClock(track.seconds_at(at)), options, show_dead_ships, choices, drawn_on))
}

/// What the first frame of `track` moves to `clock`, with every layer drawn.
fn moved_to(track: &Track, clock: f32) -> Vec<DrawCommand> {
    let motion = FrameMotion::between(track, FrameIndex::FIRST).expect("the track has a first frame");

    motion.commands_at(track, GameClock(clock), |_| true).collect()
}

/// Where the tracers of `commands` have their tails and heads.
fn tracer_ends(commands: &[DrawCommand]) -> Vec<(f32, f32, f32, f32)> {
    commands
        .iter()
        .filter_map(|command| match command {
            DrawCommand::ShotTracer { from, to, .. } => Some((from.x, from.y, to.x, to.y)),
            _ => None,
        })
        .collect()
}

fn tracer(from: (f32, f32), to: (f32, f32)) -> DrawCommand {
    DrawCommand::ShotTracer {
        from: MinimapPos { x: from.0, y: from.1 },
        to: MinimapPos { x: to.0, y: to.1 },
        color: [255, 0, 0],
    }
}

fn tip(at: (f32, f32)) -> DrawCommand {
    DrawCommand::ShotTracerTip { at: MinimapPos { x: at.0, y: at.1 }, color: [0, 0, 255] }
}

fn torpedo(at: (f32, f32)) -> DrawCommand {
    DrawCommand::Torpedo { pos: MinimapPos { x: at.0, y: at.1 }, color: [0, 255, 0] }
}

/// Trails recorded for ship 1, `me`, at each `(clock, x)` along the top edge, as a bake records
/// them.
fn recorded_trail(samples: &[(f32, f32)]) -> BakedTrails {
    let recorded = samples.iter().map(|&(clock, x)| (MinimapPos { x, y: 0.0 }, GameClock(clock), 0)).collect();
    let history = HashMap::from([(EntityId::from(1u32), recorded)]);

    BakedTrails::from_history(history, |_| Some("me".to_owned()), |_| None)
}

/// Says when the battle inside a track ran.
fn set_battle_window(track: &mut Track, start: f32, end: Option<f32>) {
    track.battle_start = GameClock(start);
    track.battle_end = end.map(GameClock);
}

/// Events at the elapsed clocks given.
fn seeded_events(at: &[f32]) -> Vec<TimelineEvent> {
    at.iter()
        .map(|seconds| TimelineEvent {
            clock: ElapsedClock(*seconds),
            kind: TimelineEventKind::AdvantageChanged { label: format!("at {seconds}"), is_friendly: true },
        })
        .collect()
}

/// A view whose frame is the size the renderer produces, painted at exactly
/// that size at the window's corner, so a window position is a frame
/// position.
fn painted() -> MapView {
    let mut view = MapView::new();
    view.set_placement(Some(Placement {
        bounds: ElementBounds {
            origin: WindowPoint { x: 0.0, y: 0.0 },
            width: MINIMAP_SIZE as f32,
            height: CANVAS_HEIGHT as f32,
        },
        frame: FrameSize { width: MINIMAP_SIZE, height: CANVAS_HEIGHT },
    }));
    view
}

/// Where the map point `at` is in the window.
fn window_point(view: &MapView, at: (f32, f32)) -> Option<WindowPoint> {
    view.to_window(view.element_point(at, DEFAULT_MAP_ORIGIN)?)
}

/// A press on the map as a front end handles it: the annotator first, and a
/// pan when nothing there wants it.
fn press(annotator: &mut Annotator, link: &CollabLink, view: &mut MapView, at: WindowPoint) -> PointerDown {
    let down = annotator.pointer_down(link, view, DEFAULT_MAP_ORIGIN, at);
    if down == PointerDown::Unclaimed {
        view.start_pan(at);
    }
    down
}

fn session() -> (Arc<parking_lot::Mutex<SessionState>>, CollabLink, std::sync::mpsc::Receiver<LocalEvent>) {
    let state = Arc::new(parking_lot::Mutex::new(SessionState::default()));
    let (link, sent) = CollabLink::for_test(Arc::clone(&state));
    (state, link, sent)
}

fn toggle(id: &str) -> &'static Toggle {
    TOGGLES.iter().find(|toggle| toggle.id == id).expect("the toggle is offered")
}

fn feed() -> RealtimeArmorFeed {
    let mut health_history = BTreeMap::new();
    health_history.insert(GameClock(0.0), HealthSnapshot { health: 15400.0, max_health: 15400.0 });
    health_history.insert(GameClock(30.0), HealthSnapshot { health: 12000.0, max_health: 15400.0 });
    RealtimeArmorFeed::new(ShipShotTimeline { hits: Vec::new(), health_history, hull: None })
}

/// The drawing tools answer to the keys the egui board uses, so a reader who
/// learned them there does not learn them again.
#[test]
fn the_tool_shortcuts_are_the_egui_boards() {
    assert_eq!(tool_for_key("1"), Some(Tool::Arrow));
    assert_eq!(tool_for_key("2"), Some(Tool::Freehand));
    assert_eq!(tool_for_key("3"), Some(Tool::Eraser));
    assert_eq!(tool_for_key("4"), Some(Tool::Line));
    assert_eq!(tool_for_key("5"), Some(Tool::Circle { filled: false }));
    assert_eq!(tool_for_key("6"), Some(Tool::Rectangle { filled: false }));
    assert_eq!(tool_for_key("7"), Some(Tool::Triangle { filled: false }));
    assert_eq!(tool_for_key("m"), Some(Tool::Measurement));
    assert_eq!(tool_for_key("8"), None, "a key that is not a tool takes nothing up");
}

/// The transport reads in the clock the game shows, not in seconds.
#[test]
fn game_time_reads_as_minutes_and_seconds() {
    assert_eq!(mmss(0.0), "00:00");
    assert_eq!(mmss(9.4), "00:09");
    assert_eq!(mmss(75.0), "01:15");
    assert_eq!(mmss(1205.0), "20:05");
    // The battle clock runs backwards from the loading screen, so a frame
    // before the battle started reads as its beginning rather than as a
    // minus sign.
    assert_eq!(mmss(-3.0), "00:00");
}

/// The transport opens at the speed the egui renderer opens at, over the
/// ladder it offers. Real time is too slow to watch a battle through, so a
/// viewport that opens at 1x reads as broken.
#[test]
fn the_transport_opens_at_the_speed_the_egui_renderer_does() {
    let playback = Playback::new();

    assert_eq!(playback.speed(), 20.0);
    assert!(SPEEDS.contains(&playback.speed()), "and it is one of the offered speeds");
    assert_eq!(SPEEDS, [1.0, 5.0, 10.0, 20.0, 40.0, 60.0], "the egui ladder");
}

/// The clock reads the battle's own time, not the recording's.
///
/// A replay starts recording on the loading screen, so the raw clock of the
/// first frame is already a good half-minute in. The egui renderer subtracts
/// the battle's start; a port that did not would open a battle at 00:40.
#[test]
fn the_clock_counts_from_the_battle_rather_than_the_recording() {
    let mut track = track((0..40).map(|frame| frame as f32 * 5.0).collect());
    let mut playback = Playback::new();

    set_battle_window(&mut track, 40.0, Some(160.0));
    assert_eq!(playback.clock_label(&track), "00:00", "the first frame is the loading screen, not minus 40s");

    // Frame 10 is 50s of recording, which is 10s of battle.
    playback.go_to(&track, FrameIndex::new(10));
    assert_eq!(playback.clock_label(&track), "00:10");
}

/// The skip controls move by game time, not by frames.
///
/// A track is sampled at a fixed interval, so ten seconds is however many
/// frames that interval divides into; a control that stepped frames would
/// move a different distance on every replay.
#[test]
fn a_ten_second_skip_moves_ten_seconds() {
    // Half-second frames, as a bake produces.
    let track = track((0..200).map(|frame| frame as f32 * 0.5).collect());
    let mut playback = Playback::new();

    playback.go_to(&track, FrameIndex::FIRST);
    playback.seek_by(&track, 10.0);
    assert_eq!(playback.at(), FrameIndex::new(20), "twenty half-second frames");

    playback.seek_by(&track, -10.0);
    assert_eq!(playback.at(), FrameIndex::FIRST);

    // Neither end runs off the track.
    playback.seek_by(&track, -10.0);
    assert_eq!(playback.at(), FrameIndex::FIRST);
    playback.go_to(&track, FrameIndex::new(199));
    playback.seek_by(&track, 10.0);
    assert_eq!(playback.at(), FrameIndex::new(199));
}

/// The speed keys walk the offered ladder and stop at its ends.
#[test]
fn the_speed_keys_walk_the_ladder() {
    let mut playback = Playback::new();

    assert_eq!(playback.speed(), 20.0);
    playback.step_speed(1);
    assert_eq!(playback.speed(), 40.0);
    playback.step_speed(1);
    assert_eq!(playback.speed(), 60.0);
    playback.step_speed(1);
    assert_eq!(playback.speed(), 60.0, "there is no rung past the top of the ladder");

    for _ in 0..5 {
        playback.step_speed(-1);
    }
    assert_eq!(playback.speed(), 1.0, "nor below its bottom");
}

/// The bar marks where the battle began and ended.
///
/// A replay records from the loading screen on and often past the last shot,
/// so those marks are what tell a reader which stretch of the bar is the
/// battle.
#[test]
fn the_seek_bar_marks_the_battle_inside_the_recording() {
    // A hundred half-second frames: fifty seconds of recording.
    let mut track = track((0..101).map(|frame| frame as f32 * 0.5).collect());

    set_battle_window(&mut track, 10.0, Some(40.0));
    assert_eq!(track.battle_marks().len(), 2);
    assert_eq!(track.position_of(GameClock(10.0)), Some(0.2));
    assert_eq!(track.position_of(GameClock(40.0)), Some(0.8));

    // A replay cut short before the battle ended has one mark.
    set_battle_window(&mut track, 10.0, None);
    assert_eq!(track.battle_marks().len(), 1);
}

/// A move of the scrubber is a frame to go to.
#[test]
fn a_scrubber_move_lands_on_the_frame_it_names() {
    let track = track((0..40).map(|frame| frame as f32 * 5.0).collect());
    let mut playback = Playback::new();

    playback.seek_to_slider(&track, 17.0);
    assert_eq!(playback.at(), FrameIndex::new(17), "the middle of the track is reachable");

    playback.seek_to_slider(&track, 3.0);
    assert_eq!(playback.at(), FrameIndex::new(3));

    // Past the end clamps rather than panicking on the index.
    playback.seek_to_slider(&track, 999.0);
    assert_eq!(playback.at(), FrameIndex::new(39));
}

/// A viewport opens showing what was last saved as the defaults, and the
/// gear's Save Defaults writes what it is showing back.
#[test]
fn a_viewport_opens_showing_what_was_saved_as_the_defaults() {
    let stored = SavedRenderOptions {
        show_torpedoes: false,
        show_player_names: true,
        include_pre_battle: true,
        prefer_cpu_encoder: true,
        show_stats_panel: true,
        // Kept for the egui renderer rather than read here, so a save from
        // this app must not clear it.
        show_self_radar_range: true,
        ..SavedRenderOptions::default()
    };
    let mut options = playback_options_hidden();
    let mut show_dead_ships = true;

    adopt_saved_defaults(&stored, &mut options, &mut show_dead_ships);
    let export = export_settings_of(&stored);

    assert!(!options.show_torpedoes, "a layer turned off stays off");
    assert!(options.show_player_names, "and one turned on comes back on");
    assert!(options.show_stats_panel, "the saved stats panel is drawn");
    assert!(export.include_pre_battle);
    assert!(export.prefer_cpu);

    options.show_smoke = false;
    options.show_stats_panel = false;
    let saved = defaults_to_save(&stored, &options, show_dead_ships, &export, None);

    assert!(!saved.show_smoke, "what the viewport shows is what is saved");
    assert!(!saved.show_torpedoes);
    assert!(saved.show_self_radar_range, "the ranges this app does not read are left as they were");
    assert!(saved.include_pre_battle, "as are the export settings it did not change");
    assert!(!saved.show_stats_panel, "and the stats panel is saved as the viewport shows it");
}

/// The self ranges the egui renderer saves go on the recording player's ship when a viewport
/// opens, and what that ship shows is saved back as them.
#[test]
fn the_saved_self_ranges_follow_the_recording_players_ship() {
    let stored = SavedRenderOptions {
        show_self_detection_range: true,
        show_self_main_battery_range: true,
        ..SavedRenderOptions::default()
    };
    let mut track = track(vec![0.0, 0.5]);
    track.frames[0] = vec![drawn_ship(1, "someone", 10.0, false), drawn_ship(2, "me", 20.0, true)];
    let own = self_player(&track.frames);
    assert_eq!(own.as_deref(), Some("me"));
    let mut options = playback_options_hidden();
    let mut choices = ShipChoices::new();

    adopt_saved_self_ranges(&stored, own.as_deref(), &mut choices, &mut options);

    let mine = choices.ranges_for("me");
    assert!(mine.detection && mine.main_battery, "the saved ranges are on the recording player's ship");
    assert!(!mine.secondary_battery && !mine.torpedo, "and only those");
    assert_eq!(choices.ranges_for("someone"), NO_RANGES, "nobody else's");
    assert!(options.show_ship_config, "so the range layer is drawn");

    choices.set_ranges_for("me", |filter| filter.torpedo = true, &mut options);
    let saved =
        defaults_to_save(&stored, &options, true, &ExportSettings::default(), own.map(|me| choices.ranges_for(&me)));
    assert!(saved.show_self_torpedo_range, "what the ship shows is saved");
    assert!(saved.show_self_detection_range);
    assert!(!saved.show_self_radar_range);
}

/// A ship drawn at `x` along the top of the map.
fn drawn_ship(entity: u32, player: &str, x: f32, is_self: bool) -> DrawCommand {
    DrawCommand::Ship {
        entity_id: EntityId::from(entity),
        pos: MinimapPos { x, y: 0.0 },
        yaw: 0.0,
        species: None,
        color: None,
        visibility: ShipVisibility::Visible,
        opacity: 1.0,
        is_self,
        player_name: Some(player.to_owned()),
        ship_name: None,
        is_detected_teammate: false,
        is_disconnected: false,
        name_color: None,
    }
}

/// The ranges one end chooses reach another by entity and land on the same players there, and
/// the range layer goes on to draw them.
#[test]
fn range_choices_travel_by_entity_and_land_by_player() {
    let frames = vec![vec![drawn_ship(7, "seven", 10.0, false), drawn_ship(9, "nine", 20.0, true)]];
    let names = ship_names(&frames);
    let mut sent = ShipChoices::new();
    let mut options = RenderOptions { show_ship_config: false, ..RenderOptions::default() };
    sent.set_every_range_for("nine", true, &mut options);
    sent.set_every_range_for("seven", false, &mut options);

    let overrides = range_overrides(&sent, &names);
    assert_eq!(overrides, vec![(EntityId::from(9u32), ALL_RANGES)], "a ship showing none is left out");

    let mut taken = ShipChoices::new();
    taken.set_every_range_for("seven", true, &mut options);
    let mut their_options = RenderOptions { show_ship_config: false, ..RenderOptions::default() };
    adopt_range_overrides(&overrides, &names, &mut taken, &mut their_options);

    assert_eq!(taken.ranges_for("nine"), ALL_RANGES);
    assert_eq!(taken.ranges_for("seven"), NO_RANGES, "every ship's own choice is replaced");
    assert!(their_options.show_ship_config, "and the range layer is drawn");
}

/// Hidden trails land as they were sent, and the trail layer goes on to draw the rest.
#[test]
fn hidden_trails_land_and_the_rest_are_drawn() {
    let mut choices = ShipChoices::new();
    let mut options = RenderOptions { show_trails: false, ..RenderOptions::default() };

    adopt_trail_overrides(&["seven".to_owned()], &mut choices, &mut options);

    assert!(!choices.trail_shown("seven"));
    assert!(choices.trail_shown("nine"));
    assert!(options.show_trails);
}

/// A watcher is sent the trails up to the frame's clock, behind the frame, and none while the
/// trail layer is off.
#[test]
fn a_watched_frame_carries_the_trails_the_layers_ask_for() {
    let mut track = track(vec![0.0, 10.0, 20.0]);
    track.frames[1] = vec![drawn_ship(1, "me", 100.0, true)];
    track.trails = recorded_trail(&[(0.0, 0.0), (10.0, 100.0), (20.0, 200.0)]);
    let commands = track.commands(FrameIndex::new(1)).expect("a frame").to_vec();
    let on = RenderOptions { show_trails: true, ..playback_options_hidden() };

    let sent = frame_broadcast_with_trails(ReplayId::from_raw(1), &track, FrameIndex::new(1), &commands, &on);

    let DrawCommand::PositionTrail { points, .. } = &sent.commands[0] else {
        panic!("the trail goes first: {:?}", sent.commands)
    };
    assert_eq!(points.iter().map(|(pos, _)| pos.x).collect::<Vec<_>>(), [0.0, 100.0]);
    assert_eq!(sent.commands.len(), commands.len() + 1);
    assert!(matches!(sent.commands[1], DrawCommand::Ship { .. }), "the frame follows");
    let off = RenderOptions { show_trails: false, ..on };
    let sent = frame_broadcast_with_trails(ReplayId::from_raw(1), &track, FrameIndex::new(1), &commands, &off);
    assert_eq!(sent.commands.len(), commands.len());
    assert!(!sent.commands.iter().any(|command| matches!(command, DrawCommand::PositionTrail { .. })));
}

/// The bracket keys widen and narrow the nib, within the range the egui
/// board holds it to.
#[test]
fn the_bracket_keys_move_the_nib_within_its_range() {
    let mut annotator = Annotator::new();
    let opened_at = annotator.drawing().width();

    annotator.take_up(Tool::Freehand);
    annotator.step_nib(1.0);
    assert!(annotator.drawing().width() > opened_at, "] draws thicker");

    for _ in 0..20 {
        annotator.step_nib(1.0);
    }
    assert_eq!(annotator.drawing().width(), MAX_NIB, "and stops at the thickest the board offers");

    for _ in 0..20 {
        annotator.step_nib(-1.0);
    }
    assert_eq!(annotator.drawing().width(), MIN_NIB, "as it does at the thinnest");
}

/// Every chord the cheat sheet lists is one the viewport answers to, so the
/// sheet cannot drift from the keys.
#[test]
fn the_cheat_sheet_lists_the_chords_that_work() {
    for button in TOOLS {
        let Some(chord) = button.chord else { continue };
        let key = chord.strip_prefix("Ctrl+").expect("every tool chord is a ctrl one").to_ascii_lowercase();
        assert_eq!(
            tool_for_key(&key).map(|tool| std::mem::discriminant(&tool)),
            Some(std::mem::discriminant(&(button.tool)())),
            "{chord} is listed as {}",
            button.label_key
        );
    }
}

/// A batch says which replay it has reached before it reads it, in order,
/// and a replay it cannot read is reported as failed rather than stopping
/// the rest.
#[test]
fn a_batch_says_which_replay_it_has_reached() {
    let dir = tempfile::tempdir().expect("a temp directory");
    // Not replays, so every one of them fails to bake: what is under test is
    // the reporting and the walk, not what a battle looks like.
    let paths: Vec<PathBuf> = ["first", "second"]
        .into_iter()
        .map(|stem| {
            let path = dir.path().join(format!("{stem}.wowsreplay"));
            std::fs::write(&path, b"not a replay").expect("the file is written");
            path
        })
        .collect();

    let mut reached = Vec::new();
    let outcome = batch_export(
        paths,
        &GameDataCache::new(dir.path().to_path_buf()),
        dir.path(),
        &SavedRenderOptions::default(),
        playback_options(),
        |step| reached.push(step),
    );

    assert!(outcome.written.is_empty(), "nothing renders out of files that are not replays");
    assert_eq!(outcome.failed.len(), 2, "and both are reported rather than the walk stopping at the first");
    assert_eq!(
        reached,
        vec![
            BatchStep { done: 0, total: 2, replay: "first".to_owned(), frames: None },
            BatchStep { done: 1, total: 2, replay: "second".to_owned(), frames: None },
        ],
        "each replay is named as the batch reaches it"
    );
}

/// A batch draws the reader's saved layers, the stats panel among them.
#[test]
fn a_batch_draws_the_saved_layers() {
    let saved = SavedRenderOptions {
        show_torpedoes: false,
        show_stats_panel: true,
        show_ship_config: true,
        show_dead_ships: false,
        ..SavedRenderOptions::default()
    };
    let (options, show_dead_ships) = batch_options(&saved);

    assert!(!options.show_torpedoes, "a layer the reader turned off is not filmed");
    assert!(!show_dead_ships);
    assert!(options.stats_panel_visible(), "the saved stats panel is filmed");
    assert_eq!(crate::minimap_preview::layout_for(&options), SidePanelLayout::StatsPanel, "on its own canvas");
    assert!(options.show_ship_config, "the saved range circles are filmed");
    assert!(
        !should_draw_command(&DrawCommand::KillFeed { entries: Vec::new() }, &options, show_dead_ships),
        "and the kill feed it replaces is not"
    );
}

/// A batch with range circles saved on films every ship's, as the egui batch does, and one with
/// them off films none.
#[test]
fn a_batch_films_every_ships_ranges_when_they_are_saved_on() {
    let mut track = track(vec![0.0, 0.5]);
    track.frames[0] = vec![drawn_ship(1, "someone", 10.0, false), range_circle(1, "someone")];
    track.frames[1] = vec![drawn_ship(2, "me", 20.0, true), range_circle(2, "me")];
    let (on, show_dead_ships) = batch_options(&SavedRenderOptions { show_ship_config: true, ..Default::default() });

    let layers = batch_layers(&on, show_dead_ships, &track);
    assert!(layers.choices.ranges_for("someone").detection && layers.choices.ranges_for("me").detection);
    let frames: Vec<_> = ExportFrames::new(Arc::new(track), true, layers).expect("there are frames").collect();
    let circles = |frame: &[DrawCommand]| {
        frame.iter().filter(|command| matches!(command, DrawCommand::ShipConfigCircle { .. })).count()
    };
    assert_eq!(circles(&frames[0]), 1, "the first ship's ranges are filmed");
    assert_eq!(circles(frames.last().expect("a last frame")), 1, "and the second's");

    let mut off_track = self::track(vec![0.0]);
    off_track.frames[0] = vec![drawn_ship(1, "someone", 10.0, false), range_circle(1, "someone")];
    let (off, show_dead_ships) = batch_options(&SavedRenderOptions::default());
    let layers = batch_layers(&off, show_dead_ships, &off_track);
    let first = ExportFrames::new(Arc::new(off_track), true, layers).and_then(|mut frames| frames.next());
    assert_eq!(first.map(|frame| circles(&frame)), Some(0), "none while they are saved off");
}

fn range_circle(entity: u32, player: &str) -> DrawCommand {
    DrawCommand::ShipConfigCircle {
        entity_id: EntityId::from(entity),
        pos: MinimapPos { x: 10.0, y: 0.0 },
        radius_px: 40.0,
        color: [255, 255, 255],
        alpha: 0.5,
        dashed: true,
        label: None,
        kind: ShipConfigCircleKind::Detection,
        player_name: player.to_owned(),
        is_self: false,
    }
}

/// A toggle changes what is drawn without re-baking: the commands are
/// filtered as a frame is rasterised, so the track is untouched and the
/// options are what moved.
#[test]
fn a_display_toggle_changes_what_is_drawn_without_rebaking() {
    let mut track = track(vec![0.0, 30.0]);
    track.frames[0] = vec![DrawCommand::Torpedo { pos: MinimapPos { x: 10.0, y: 10.0 }, color: [0, 0, 0] }];
    let mut options = playback_options_hidden();
    let mut show_dead_ships = true;
    let choices = ShipChoices::new();
    let baked = track.len();

    assert!(options.show_torpedoes, "the track was baked with them on");
    let drawn = drawn_at(&track, FrameIndex::FIRST, &options, show_dead_ships, &choices, Vec::new());
    assert_eq!(drawn.map(|drawn| drawn.len()), Some(1));

    (toggle("renderer-opt-torpedoes").write)(&mut options, &mut show_dead_ships, false);

    assert!(!options.show_torpedoes);
    let drawn = drawn_at(&track, FrameIndex::FIRST, &options, show_dead_ships, &choices, Vec::new());
    assert_eq!(drawn.map(|drawn| drawn.len()), Some(0), "the torpedo is no longer drawn");
    assert_eq!(track.len(), baked, "the track is untouched; only what is drawn of it moved");
}

/// Dead ships are gated beside the options rather than in them, as the egui
/// viewer gates them, so the toggle has to reach its own flag.
#[test]
fn the_dead_ship_toggle_reaches_its_own_flag() {
    let dead_ships = toggle("renderer-opt-dead-ships");
    let mut options = playback_options_hidden();
    let mut show_dead_ships = true;

    assert!((dead_ships.read)(&options, show_dead_ships));
    (dead_ships.write)(&mut options, &mut show_dead_ships, false);
    assert!(!show_dead_ships);
    assert!(!(dead_ships.read)(&options, show_dead_ships));
}

/// A wheel over the map zooms about what is under the pointer, which is what
/// makes a zoom feel like it is following the reader rather than the corner
/// of the map.
#[test]
fn the_wheel_zooms_about_what_is_under_the_pointer() {
    let mut view = painted();
    // A point on the map, and the same point in window coordinates: the
    // frame reserves a strip above the map.
    let on_map = (200.0_f32, 300.0_f32);
    let at = WindowPoint { x: on_map.0, y: on_map.1 + DEFAULT_MAP_ORIGIN.1 };
    let before = view.view().to_map(on_map.0, on_map.1);

    assert_eq!(view.zoom_by_wheel(at, 120.0, DEFAULT_MAP_ORIGIN), Wheel::Zoomed);

    assert!(view.view().zoom() > 1.0, "the wheel zoomed in");
    let after = view.view().to_map(on_map.0, on_map.1);
    assert!(
        (before.0 - after.0).abs() < 1.0 && (before.1 - after.1).abs() < 1.0,
        "the map did not slide out from under the pointer: {before:?} then {after:?}"
    );
}

/// A wheel outside the map does nothing.
///
/// The strip above the map carries the score bar and the timer, which do not
/// zoom; a wheel there has no map point to zoom about.
#[test]
fn a_wheel_over_the_hud_does_not_zoom() {
    let mut view = painted();

    assert_eq!(view.zoom_by_wheel(WindowPoint { x: 200.0, y: 4.0 }, 120.0, DEFAULT_MAP_ORIGIN), Wheel::Ignored);
    assert!(view.view().is_whole_map());
}

/// The zoom control moves the view, and Reset puts the whole map back.
#[test]
fn the_zoom_control_and_reset_agree_with_the_view() {
    let mut view = painted();

    view.zoom_to(4.0);
    assert_eq!(view.view().zoom(), 4.0);
    assert!(!view.view().is_whole_map());

    view.reset();
    assert!(view.view().is_whole_map());
}

/// Dragging the whole map does nothing: there is nowhere to drag to.
#[test]
fn the_whole_map_does_not_drag() {
    let mut view = painted();

    assert!(!view.start_pan(WindowPoint { x: 200.0, y: 300.0 }));
    assert!(!view.is_panning(), "the whole map has nowhere to drag to");
}

/// The event controls step to the next thing that happened, and stop at each
/// end of the battle.
///
/// Stepping back looks half a second behind the clock, as the egui renderer
/// does: without that, landing on an event and pressing back again would find
/// the same one and never move.
#[test]
fn the_event_controls_step_between_what_happened() {
    // Half-second frames over two minutes of recording.
    let mut track = track((0..240).map(|frame| frame as f32 * 0.5).collect());
    // The battle starts forty seconds into the recording, so an event at 10s
    // of battle is at 50s of track.
    set_battle_window(&mut track, 40.0, Some(110.0));
    let events = BattleEvents::from_read(seeded_events(&[10.0, 30.0, 60.0]), None, HashMap::new());
    let mut playback = Playback::new();

    assert!(events.previous(playback.elapsed(&track)).is_none(), "nothing happened before the battle began");
    let next = events.next(playback.elapsed(&track)).expect("an event follows").clock.seconds();
    playback.go_to_elapsed(&track, next);
    assert_eq!(playback.clock_label(&track), "00:10");

    let next = events.next(playback.elapsed(&track)).expect("another follows").clock.seconds();
    playback.go_to_elapsed(&track, next);
    assert_eq!(playback.clock_label(&track), "00:30");

    // Back from an event finds the one before it, not itself.
    let previous = events.previous(playback.elapsed(&track)).expect("one precedes it").clock.seconds();
    playback.go_to_elapsed(&track, previous);
    assert_eq!(playback.clock_label(&track), "00:10");

    playback.go_to(&track, FrameIndex::new(239));
    assert!(events.next(playback.elapsed(&track)).is_none(), "nothing happened after the last one");
}

/// On a long battle's coarser track, an event between two kept frames is landed on exactly, so
/// the next event control moves on from it rather than finding it again.
#[test]
fn the_next_event_moves_on_from_one_between_kept_frames() {
    // Two-second frames, as a twenty-minute battle keeps them, with the battle starting at 40s.
    let mut track = track((0..100).map(|frame| frame as f32 * 2.0).collect());
    set_battle_window(&mut track, 40.0, None);
    let events = BattleEvents::from_read(seeded_events(&[10.7, 30.0]), None, HashMap::new());
    let mut playback = Playback::new();

    let next = events.next(playback.elapsed(&track)).expect("an event follows").clock.seconds();
    playback.go_to_elapsed(&track, next);
    assert_eq!(playback.at(), FrameIndex::new(25), "the kept frame before it is what is drawn from");
    assert!((playback.elapsed(&track) - 10.7).abs() < 1e-4, "and playback is at the event itself");

    let next = events.next(playback.elapsed(&track)).expect("another follows").clock.seconds();
    assert_eq!(next, 30.0, "the event after it, not the same one");
}

/// With no events read yet, the controls have nowhere to go.
///
/// The second walk of the replay lands after the viewport opens, and a
/// control that jumped to the start of the battle in the meantime would read
/// as broken.
#[test]
fn the_event_controls_do_nothing_until_the_battle_has_been_read() {
    let track = track((0..40).map(|frame| frame as f32 * 0.5).collect());
    let events = BattleEvents::new();
    let mut playback = Playback::new();

    playback.go_to(&track, FrameIndex::new(10));
    assert!(events.previous(playback.elapsed(&track)).is_none());
    assert!(events.next(playback.elapsed(&track)).is_none());
    assert_eq!(playback.at(), FrameIndex::new(10), "playback stayed where it was");
}

/// An export starts at the battle unless the reader asked for what came
/// before it.
///
/// A replay records the loading screen and the countdown; exporting those by
/// default would put most of a minute of a still map at the front of every
/// video.
#[test]
fn an_export_leaves_out_the_pre_battle_phase_unless_it_is_asked_for() {
    // Half-second frames: the battle starts forty seconds in, at frame 80.
    let mut track = track((0..200).map(|frame| frame as f32 * 0.5).collect());
    set_battle_window(&mut track, 40.0, Some(95.0));
    let track = Arc::new(track);
    let layers = || ExportLayers {
        options: playback_options_hidden(),
        show_dead_ships: true,
        choices: ShipChoices::new(),
        drawn_on: Vec::new(),
    };

    assert_eq!(track.export_start(false), FrameIndex::new(80), "the eighty frames before the battle are cut");
    let battle = ExportFrames::new(Arc::clone(&track), false, layers()).expect("there are frames");
    assert_eq!(battle.duration_seconds(), 59.5, "the video covers the battle");

    assert_eq!(track.export_start(true), FrameIndex::FIRST, "and kept when asked for");
    let whole = ExportFrames::new(Arc::clone(&track), true, layers()).expect("there are frames");
    assert_eq!(whole.duration_seconds(), 99.5, "the video covers the recording");
}

/// Every export is the same length at the same rate, however few frames the bake kept: a long
/// battle's track is sampled more coarsely, and the time it covers is read off its own clocks.
#[test]
fn an_export_is_a_fixed_length_whatever_the_track_kept() {
    let layers = || ExportLayers {
        options: playback_options_hidden(),
        show_dead_ships: true,
        choices: ShipChoices::new(),
        drawn_on: Vec::new(),
    };
    // A twenty-minute battle decimated to a frame every two seconds.
    let coarse = Arc::new(track((0..600).map(|frame| frame as f32 * 2.0).collect()));
    let short = Arc::new(track((0..120).map(|frame| frame as f32 * 0.5).collect()));

    let coarse = ExportFrames::new(coarse, true, layers()).expect("there are frames");
    assert_eq!(coarse.duration_seconds(), 1198.0, "the time the track's clocks span");
    assert_eq!(coarse.len(), EXPORT_FRAME_COUNT);
    assert_eq!(coarse.count(), EXPORT_FRAME_COUNT, "every frame is written");

    let short = ExportFrames::new(short, true, layers()).expect("there are frames");
    assert_eq!(short.duration_seconds(), 59.5);
    assert_eq!(short.count(), EXPORT_FRAME_COUNT, "a short battle is stretched to the same length");
}

/// A written frame between two baked ones has each ship part of the way along, with what is
/// drawn at the ship moved with it.
#[test]
fn an_export_moves_ships_between_the_frames_the_bake_kept() {
    let mut track = track(vec![0.0, 10.0]);
    track.frames[0] = vec![drawn_ship(1, "me", 0.0, true), range_circle(1, "me")];
    track.frames[1] = vec![drawn_ship(1, "me", 100.0, true), range_circle(1, "me")];
    let mut choices = ShipChoices::new();
    let mut options = playback_options_hidden();
    choices.set_ranges_for("me", |filter| filter.detection = true, &mut options);
    let layers = ExportLayers { options, show_dead_ships: true, choices, drawn_on: Vec::new() };

    let frames: Vec<_> = ExportFrames::new(Arc::new(track), true, layers).expect("there are frames").collect();
    let middle = &frames[EXPORT_FRAME_COUNT / 2];
    let ship_x = middle.iter().find_map(|command| match command {
        DrawCommand::Ship { pos, .. } => Some(pos.x),
        _ => None,
    });
    let circle_x = middle.iter().find_map(|command| match command {
        DrawCommand::ShipConfigCircle { pos, .. } => Some(pos.x),
        _ => None,
    });
    let ship_x = ship_x.expect("the ship is drawn");
    assert!((ship_x - 50.0).abs() < 1.0, "halfway along: {ship_x}");
    assert!(circle_x.is_some_and(|x| (x - (ship_x + 10.0)).abs() < 1e-3), "its range moved with it: {circle_x:?}");
}

/// A trail in a written frame between two baked ones runs up to the ship, not to where the ship
/// was in the baked frame before.
#[test]
fn an_exported_trail_ends_at_its_ship_between_baked_frames() {
    let mut track = track(vec![0.0, 10.0, 20.0]);
    for (frame, x) in [0.0, 100.0, 200.0].into_iter().enumerate() {
        track.frames[frame] = vec![drawn_ship(1, "me", x, true)];
    }
    track.trails = recorded_trail(&[(0.0, 0.0), (10.0, 100.0), (20.0, 200.0)]);
    let layers = ExportLayers {
        options: RenderOptions { show_trails: true, ..playback_options_hidden() },
        show_dead_ships: true,
        choices: ShipChoices::new(),
        drawn_on: Vec::new(),
    };

    let frames: Vec<_> = ExportFrames::new(Arc::new(track), true, layers).expect("there are frames").collect();
    let frame = &frames[EXPORT_FRAME_COUNT * 3 / 4];
    let ship_x = frame.iter().find_map(|command| match command {
        DrawCommand::Ship { pos, .. } => Some(pos.x),
        _ => None,
    });
    let trail_end = frame.iter().find_map(|command| match command {
        DrawCommand::PositionTrail { points, .. } => points.last().map(|(pos, _)| pos.x),
        _ => None,
    });
    let ship_x = ship_x.expect("the ship is drawn");
    assert!(ship_x > 100.0 && ship_x < 200.0, "between the baked frames: {ship_x}");
    assert_eq!(trail_end, Some(ship_x));
}

/// The viewport's trail turns where the ship turned between two kept frames, because it is read
/// from the positions the bake recorded at every step rather than from the kept frames.
#[test]
fn a_trail_follows_the_ship_between_the_frames_the_bake_kept() {
    let mut track = track(vec![0.0, 10.0]);
    track.frames[0] = vec![drawn_ship(1, "me", 0.0, true)];
    track.frames[1] = vec![drawn_ship(1, "me", 100.0, true)];
    track.trails = recorded_trail(&[(0.0, 0.0), (2.0, 20.0), (4.0, 60.0), (6.0, 30.0), (8.0, 70.0), (10.0, 100.0)]);
    let options = RenderOptions { show_trails: true, ..playback_options_hidden() };
    let trail_xs = |at: FrameIndex| -> Option<Vec<f32>> {
        drawn_at(&track, at, &options, true, &ShipChoices::new(), Vec::new())?.into_iter().find_map(|command| {
            match command {
                DrawCommand::PositionTrail { points, .. } => Some(points.iter().map(|(pos, _)| pos.x).collect()),
                _ => None,
            }
        })
    };

    assert_eq!(trail_xs(FrameIndex::FIRST), None, "one position is not a trail");
    assert_eq!(
        trail_xs(FrameIndex::new(1)),
        Some(vec![0.0, 20.0, 60.0, 30.0, 70.0, 100.0]),
        "every recorded turn is drawn, not a straight line between the kept frames"
    );
}

/// Playing at 1x draws a frame every frame interval, each with the ship further along than the
/// last, between two baked frames half a second apart.
#[test]
fn real_time_playback_moves_a_ship_on_every_frame_between_two_baked_ones() {
    let mut track = track(vec![0.0, 0.5, 1.0]);
    for (frame, x) in [0.0, 10.0, 20.0].into_iter().enumerate() {
        track.frames[frame] = vec![drawn_ship(1, "me", x, true)];
    }
    let mut playback = Playback::new();
    playback.set_speed(1.0);
    playback.toggle_playing(&track);

    let mut xs = Vec::new();
    while playback.advance(&track, FRAME_INTERVAL) == Tick::Moved {
        let motion = FrameMotion::between(&track, playback.at()).expect("playback stays on the track");
        let drawn = raster_commands(
            &track,
            &motion,
            playback.clock(&track),
            &playback_options_hidden(),
            true,
            &ShipChoices::new(),
            Vec::new(),
        );
        xs.extend(drawn.iter().find_map(|command| match command {
            DrawCommand::Ship { pos, .. } => Some(pos.x),
            _ => None,
        }));
    }

    assert_eq!(playback.at(), FrameIndex::new(1), "the next baked frame ends the stretch");
    assert!(xs.len() >= 14, "half a second at thirty frames a second: {xs:?}");
    assert!(xs.windows(2).all(|pair| pair[1] > pair[0]), "every frame moves the ship on: {xs:?}");
    assert!(xs.iter().all(|x| *x > 0.0 && *x < 10.0), "between the two baked positions: {xs:?}");
}

/// A ship turning through due south turns the short way, not the long way round.
#[test]
fn a_ship_turns_the_short_way_between_frames() {
    let mut track = track(vec![0.0, 1.0]);
    let heading = |yaw: f32| {
        let mut ship = drawn_ship(1, "me", 0.0, true);
        if let DrawCommand::Ship { yaw: at, .. } = &mut ship {
            *at = yaw;
        }
        ship
    };
    track.frames[0] = vec![heading(3.0)];
    track.frames[1] = vec![heading(-3.0)];

    let yaw = moved_to(&track, 0.5).iter().find_map(|command| match command {
        DrawCommand::Ship { yaw, .. } => Some(*yaw),
        _ => None,
    });

    let yaw = yaw.expect("the ship is drawn");
    assert!((yaw - std::f32::consts::PI).abs() < 1e-3, "halfway is due south: {yaw}");
}

/// A shell flies along its line between frames, with its tip at its head. One that landed before
/// the next frame is gone past halfway, and one fired before it is out from halfway.
#[test]
fn shells_fly_between_frames_and_land_and_fire_halfway() {
    let mut track = track(vec![0.0, 1.0]);
    track.frames[0] = vec![tracer((0.0, 0.0), (10.0, 0.0)), tip((10.0, 0.0)), tracer((0.0, 50.0), (10.0, 52.0))];
    track.frames[1] = vec![tracer((20.0, 0.0), (30.0, 0.0)), tip((30.0, 0.0)), tracer((90.0, 90.0), (95.0, 90.0))];

    let early = moved_to(&track, 0.25);
    assert_eq!(
        tracer_ends(&early),
        [(5.0, 0.0, 15.0, 0.0), (0.0, 50.0, 10.0, 52.0)],
        "a quarter of the way along, and the one that lands still in the air"
    );
    assert!(
        early.iter().any(|command| matches!(command, DrawCommand::ShotTracerTip { at, .. } if at.x == 15.0)),
        "the tip rides the head"
    );

    let late = moved_to(&track, 0.75);
    assert_eq!(
        tracer_ends(&late),
        [(15.0, 0.0, 25.0, 0.0), (90.0, 90.0, 95.0, 90.0)],
        "three quarters of the way along, the landed one gone and the new one out"
    );
}

/// Of two salvos flying one line, each shell pairs with the one just ahead of it in the next frame
/// rather than the one past it.
#[test]
fn two_salvos_on_one_line_keep_to_their_own_shells() {
    let mut track = track(vec![0.0, 1.0]);
    track.frames[0] = vec![tracer((100.0, 0.0), (110.0, 0.0)), tracer((10.0, 0.0), (20.0, 0.0))];
    track.frames[1] = vec![tracer((120.0, 0.0), (130.0, 0.0)), tracer((30.0, 0.0), (40.0, 0.0))];

    assert_eq!(tracer_ends(&moved_to(&track, 0.5)), [(110.0, 0.0, 120.0, 0.0), (20.0, 0.0, 30.0, 0.0)]);
}

/// Torpedoes pair with the nearest of their colour, and one that hit is gone past halfway.
#[test]
fn torpedoes_run_between_frames() {
    let mut track = track(vec![0.0, 1.0]);
    track.frames[0] = vec![torpedo((10.0, 10.0)), torpedo((10.0, 14.0)), torpedo((200.0, 200.0))];
    track.frames[1] = vec![torpedo((12.0, 14.0)), torpedo((12.0, 10.0))];
    let positions = |commands: Vec<DrawCommand>| -> Vec<(f32, f32)> {
        commands
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Torpedo { pos, .. } => Some((pos.x, pos.y)),
                _ => None,
            })
            .collect()
    };

    assert_eq!(positions(moved_to(&track, 0.25)), [(10.5, 10.0), (10.5, 14.0), (200.0, 200.0)]);
    assert_eq!(positions(moved_to(&track, 0.5)), [(11.0, 10.0), (11.0, 14.0)], "the one that hit is gone");
}

/// A squadron and the circle its fighters patrol fly between frames by the squadron they belong to.
#[test]
fn planes_fly_between_frames() {
    let squadron = PlaneId::from(7u64);
    let plane = |x: f32| DrawCommand::Plane {
        plane_id: squadron,
        owner_entity_id: EntityId::from(1u32),
        pos: MinimapPos { x, y: 0.0 },
        icon_key: String::new(),
        player_name: None,
        ship_name: None,
    };
    let patrol = |x: f32| DrawCommand::PatrolRadius {
        plane_id: squadron,
        pos: MinimapPos { x, y: 0.0 },
        radius_px: 5,
        color: [0, 0, 0],
        alpha: 1.0,
    };
    let mut track = track(vec![0.0, 2.0]);
    track.frames[0] = vec![patrol(0.0), plane(0.0)];
    track.frames[1] = vec![patrol(40.0), plane(80.0)];

    let xs: Vec<f32> = moved_to(&track, 0.5)
        .iter()
        .filter_map(|command| match command {
            DrawCommand::Plane { pos, .. } | DrawCommand::PatrolRadius { pos, .. } => Some(pos.x),
            _ => None,
        })
        .collect();
    assert_eq!(xs, [10.0, 20.0]);
}

/// Two shells flying opposite ways along one line are not taken for one shell turning round.
#[test]
fn shells_flying_opposite_ways_on_one_line_are_not_paired() {
    let mut track = track(vec![0.0, 1.0]);
    track.frames[0] = vec![tracer((0.0, 0.0), (10.0, 0.0))];
    track.frames[1] = vec![tracer((20.0, 0.0), (10.0, 0.0))];

    assert_eq!(tracer_ends(&moved_to(&track, 0.25)), [(0.0, 0.0, 10.0, 0.0)], "the first stays as it was");
    assert_eq!(tracer_ends(&moved_to(&track, 0.75)), [(20.0, 0.0, 10.0, 0.0)], "and the second is out by then");
}

/// Smoke spreads where it lies, and a capture fills while the same team keeps taking it.
#[test]
fn smoke_spreads_and_a_capture_fills_between_frames() {
    let puff = |radius: i32, alpha: f32| DrawCommand::Smoke {
        pos: MinimapPos { x: 50.0, y: 50.0 },
        radius,
        color: [200, 200, 200],
        alpha,
    };
    let point = |progress: f32| DrawCommand::CapturePoint {
        pos: MinimapPos { x: 300.0, y: 300.0 },
        radius: 30,
        color: [255, 255, 255],
        alpha: 0.3,
        label: "A".to_owned(),
        progress,
        invader_color: Some([0, 255, 0]),
        flag_icon: None,
    };
    let mut track = track(vec![0.0, 1.0]);
    track.frames[0] = vec![point(0.2), puff(10, 0.4)];
    track.frames[1] = vec![point(0.4), puff(20, 0.8)];

    let between = moved_to(&track, 0.5);
    let progress = between.iter().find_map(|command| match command {
        DrawCommand::CapturePoint { progress, .. } => Some(*progress),
        _ => None,
    });
    let smoke = between.iter().find_map(|command| match command {
        DrawCommand::Smoke { radius, alpha, .. } => Some((*radius, *alpha)),
        _ => None,
    });
    assert!(progress.is_some_and(|progress| (progress - 0.3).abs() < 1e-6), "half filled: {progress:?}");
    assert!(smoke.is_some_and(|(radius, alpha)| radius == 15 && (alpha - 0.6).abs() < 1e-6), "half spread: {smoke:?}");
}

/// A written video frame between two baked ones has its shells part of the way along, as playback
/// draws them.
#[test]
fn an_export_flies_shells_between_the_frames_the_bake_kept() {
    let mut track = track(vec![0.0, 10.0]);
    track.frames[0] = vec![tracer((0.0, 0.0), (10.0, 0.0))];
    track.frames[1] = vec![tracer((100.0, 0.0), (110.0, 0.0))];
    let layers = ExportLayers {
        options: playback_options_hidden(),
        show_dead_ships: true,
        choices: ShipChoices::new(),
        drawn_on: Vec::new(),
    };

    let frames: Vec<_> = ExportFrames::new(Arc::new(track), true, layers).expect("there are frames").collect();
    let heads: Vec<f32> = frames.iter().flat_map(|frame| tracer_ends(frame)).map(|(_, _, head, _)| head).collect();

    assert_eq!(heads.len(), EXPORT_FRAME_COUNT, "the shell is in every written frame");
    assert!(heads.windows(2).all(|pair| pair[1] > pair[0]), "and flies on in each");
}

/// The game timer counts on between frames, and what happened by the next frame shows only once
/// that frame is reached.
#[test]
fn the_timer_counts_on_and_events_wait_for_their_frame() {
    let mut track = track(vec![0.0, 2.0]);
    track.frames[0] = vec![DrawCommand::Timer { time_remaining: Some(600), elapsed: ElapsedClock(10.5) }];
    track.frames[1] = vec![
        DrawCommand::Timer { time_remaining: Some(598), elapsed: ElapsedClock(12.5) },
        DrawCommand::KillFeed { entries: Vec::new() },
    ];

    let between = moved_to(&track, 1.75);
    let [DrawCommand::Timer { time_remaining, elapsed }] = &between[..] else {
        panic!("the timer alone, with no kill yet: {between:?}");
    };
    assert_eq!(*time_remaining, Some(598), "two seconds off the countdown");
    assert_eq!(*elapsed, ElapsedClock(12.25));
}

/// What is on screen is what is written: the trails asked for, and what was drawn over the
/// battle.
#[test]
fn an_export_writes_the_trails_and_what_was_drawn_over_the_battle() {
    let mut track = track(vec![0.0, 1.0, 2.0]);
    for (frame, x) in [0.0, 10.0, 20.0].into_iter().enumerate() {
        track.frames[frame] = vec![drawn_ship(1, "me", x, true)];
    }
    track.trails = recorded_trail(&[(0.0, 0.0), (1.0, 10.0), (2.0, 20.0)]);
    let track = Arc::new(track);
    let annotation = DrawCommand::Annotation {
        shape: AnnotationShape::Circle { center: MinimapPos { x: 5.0, y: 5.0 }, radius: 3.0, filled: false },
        color: [255, 0, 0, 255],
        width: 2.0,
    };
    let layers = |show_trails: bool| ExportLayers {
        options: RenderOptions { show_trails, ..playback_options_hidden() },
        show_dead_ships: true,
        choices: ShipChoices::new(),
        drawn_on: vec![annotation.clone()],
    };
    let last = |show_trails: bool| {
        ExportFrames::new(Arc::clone(&track), true, layers(show_trails)).and_then(Iterator::last).expect("a last frame")
    };
    let has_trail =
        |frame: &[DrawCommand]| frame.iter().any(|command| matches!(command, DrawCommand::PositionTrail { .. }));

    let with_trails = last(true);
    assert!(has_trail(&with_trails), "the trail is written");
    assert!(with_trails.iter().any(|command| matches!(command, DrawCommand::Annotation { .. })), "and the drawing");
    assert!(!has_trail(&last(false)), "no trail while trails are off");
}

/// A codec the GPU cannot encode falls back to software rather than failing
/// the export.
#[test]
fn a_codec_the_gpu_cannot_encode_falls_back_to_software() {
    let mut status = EncoderStatus {
        gpu_adapter_name: Some("test".into()),
        gpu_error: None,
        gpu_codecs: Default::default(),
        cpu_codecs: Default::default(),
    };
    status.gpu_codecs.insert(VideoCodec::H264, CodecSupport::Supported);
    status.cpu_codecs.insert(VideoCodec::Av1, CodecSupport::Supported);
    assert!(status.supports(EncoderKind::Gpu, VideoCodec::H264));

    let settings = ExportSettings::default();
    assert!(!must_use_cpu(&settings, &status), "the GPU handles the codec it would pick");

    let asked_for_av1 = ExportSettings { codec: Some(VideoCodec::Av1), ..settings };
    assert!(must_use_cpu(&asked_for_av1, &status), "AV1 has no GPU encoder here, so it falls back");

    let asked_for_cpu = ExportSettings { prefer_cpu: true, ..settings };
    assert!(must_use_cpu(&asked_for_cpu, &status));

    // Availability is read off the codec table, not the adapter name.
    let no_gpu = EncoderStatus { gpu_codecs: Default::default(), ..status };
    assert!(!no_gpu.gpu_available());
    assert!(must_use_cpu(&settings, &no_gpu), "with no GPU there is nothing to fall back from");
}

/// The timeline shows the events the filter admits, and says which of
/// "nothing happened" and "nothing matches" it means.
#[test]
fn the_timeline_filter_decides_what_the_list_shows() {
    assert!(!BattleEvents::new().is_read(), "the second walk has not landed yet");

    let mut events = BattleEvents::from_read(seeded_events(&[10.0, 30.0, 60.0]), None, HashMap::new());
    assert!(events.is_read());
    assert_eq!(events.visible().len(), 3);

    // Every seeded event is an advantage change, so switching that kind off
    // empties the list without emptying the battle.
    let advantage = kind_index(&events.events()[0].kind);
    events.set_filter(|filter| filter.kinds[advantage] = false);
    assert!(events.visible().is_empty());
    assert!(!events.events().is_empty(), "which is 'nothing matches', not 'nothing happened'");

    events.set_filter(|filter| {
        filter.kinds = [true; KIND_COUNT];
        filter.search = "at 30".into();
    });
    assert_eq!(events.visible().len(), 1, "the search looks through what a row says");
    assert_eq!(events.rows().len(), 1, "and the list has a row for it");

    // Letters that each start a word, which a plain substring search does not find, marked
    // where they matched.
    events.set_filter(|filter| filter.search = "a30".into());
    let rows = events.rows();
    let kept: Vec<f32> = rows.iter().map(|row| row.at).collect();
    assert_eq!(kept, [30.0]);
    assert_eq!(rows[0].marks, [0..1, 3..5]);

    events.set_filter(|filter| filter.search = "t".into());
    let kept: Vec<f32> = events.visible().iter().map(|event| event.clock.seconds()).collect();
    assert_eq!(kept, [10.0, 30.0, 60.0], "a timeline keeps the order things happened in");
}

/// Clicking a row moves playback to when it happened.
#[test]
fn clicking_a_timeline_row_seeks_to_it() {
    let mut track = track((0..200).map(|frame| frame as f32 * 0.5).collect());
    set_battle_window(&mut track, 40.0, Some(95.0));
    let mut playback = Playback::new();

    playback.go_to_elapsed(&track, 30.0);
    assert_eq!(playback.clock_label(&track), "00:30");
}

/// A playback bake carries every ship's range circles, so the viewport can
/// offer them without walking the battle again.
///
/// The bake reads a real replay against a real game install. Run with:
///
/// ```text
/// WOWS_RENDERER_TEST_REPLAY="E:\WoWs\World_of_Warships\replays\some.wowsreplay" \
/// WOWS_RENDERER_TEST_GAME_DIR="E:\WoWs\World_of_Warships" \
/// cargo test -p wows-toolkit-replay-data -- --ignored a_playback_bake_carries_every_ships_ranges
/// ```
#[test]
#[ignore = "needs a local game install and a replay; see the doc comment for the run command"]
fn a_playback_bake_carries_every_ships_ranges() {
    let replay = std::env::var("WOWS_RENDERER_TEST_REPLAY").expect("set WOWS_RENDERER_TEST_REPLAY to a replay");
    let game_dir =
        std::env::var("WOWS_RENDERER_TEST_GAME_DIR").expect("set WOWS_RENDERER_TEST_GAME_DIR to a WoWs install");

    let cache = GameDataCache::new(PathBuf::from(game_dir));
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let baked = crate::minimap_preview::bake_track(
        std::path::Path::new(&replay),
        &[],
        &cache,
        &cancel,
        TRACK_BUDGET,
        BAKE_INTERVAL,
        playback_options(),
    )
    .expect("the replay bakes");

    let mut ships: HashSet<String> = HashSet::new();
    let mut kinds: HashSet<String> = HashSet::new();
    for commands in &baked.frames {
        for command in commands {
            if let DrawCommand::ShipConfigCircle { player_name, kind, .. } = command {
                ships.insert(player_name.clone());
                kinds.insert(format!("{kind:?}"));
            }
        }
    }

    assert!(ships.len() > 1, "every ship's ranges are baked, not just the replay owner's: got {ships:?}");
    assert!(kinds.len() > 1, "and more than one kind of range: got {kinds:?}");
}

/// Turning one ship's ranges on draws ranges; turning the last one off stops
/// drawing them.
///
/// The global switch is what decides whether the layer is drawn at all, so
/// leaving it on with nothing selected would be a layer that shows nothing,
/// and leaving it off would ignore the ship that was asked for.
#[test]
fn asking_for_one_ships_ranges_turns_the_layer_on_and_off_with_it() {
    let mut choices = ShipChoices::new();
    let mut options = playback_options_hidden();

    assert!(!options.show_ship_config, "nothing is asked for yet");
    assert_eq!(choices.ranges_for("gapedd"), NO_RANGES);

    choices.set_ranges_for("gapedd", |filter| filter.detection = true, &mut options);
    assert!(options.show_ship_config);
    assert!(choices.ranges_for("gapedd").detection);
    assert!(!choices.ranges_for("someone_else").detection, "only the ship that was asked for");

    choices.set_ranges_for("gapedd", |filter| filter.detection = false, &mut options);
    assert!(!options.show_ship_config, "the last one off puts the layer away");
}

/// Hiding a trail is per ship, and the layer follows the last one.
#[test]
fn hiding_the_last_trail_puts_the_layer_away() {
    let mut choices = ShipChoices::new();
    let mut options = playback_options_hidden();
    // The frame carries no ship commands, so every drawn ship is hidden the
    // moment this one is.
    let drawn: Vec<String> = Vec::new();

    assert!(choices.trail_shown("gapedd"), "a ship shows its trail until it is hidden");

    choices.set_trail_shown("gapedd", true, &drawn, &mut options);
    assert!(options.show_trails, "asking for one turns trails on");

    choices.set_trail_shown("gapedd", false, &drawn, &mut options);
    assert!(!choices.trail_shown("gapedd"));
    assert!(!options.show_trails);
}

/// The per-ship filters decide whose trail and whose ranges are drawn.
#[test]
fn the_per_ship_filters_decide_whose_layers_are_drawn() {
    let mut hidden = HashSet::new();
    hidden.insert("hidden_player".to_string());
    let mut ranges = HashMap::new();
    ranges.insert("ranged_player".to_string(), ALL_RANGES);

    let trail = |name: &str| DrawCommand::PositionTrail {
        entity_id: EntityId::from(1u32),
        player_name: Some(name.to_string()),
        points: Vec::new(),
    };
    let circle = |name: &str| DrawCommand::ShipConfigCircle {
        entity_id: EntityId::from(1u32),
        pos: MinimapPos { x: 0.0, y: 0.0 },
        radius_px: 10.0,
        color: [0, 0, 0],
        alpha: 1.0,
        dashed: false,
        label: None,
        kind: ShipConfigCircleKind::Detection,
        player_name: name.to_string(),
        is_self: false,
    };

    assert!(per_ship_allows(&trail("shown_player"), &hidden, &ranges));
    assert!(!per_ship_allows(&trail("hidden_player"), &hidden, &ranges));
    assert!(per_ship_allows(&circle("ranged_player"), &hidden, &ranges));
    assert!(!per_ship_allows(&circle("unranged_player"), &hidden, &ranges), "a ship nobody asked for");
}

/// The advantage hover reads the breakdown out of the frame on screen.
///
/// The label is drawn into the frame by the renderer, so there is nothing to
/// hover that knows what it says; this reads the command behind it.
#[test]
fn the_advantage_hover_reads_the_frames_own_breakdown() {
    assert!(advantage_hover(&[]).is_none(), "a frame with no advantage has nothing to say");

    let breakdown = AdvantageBreakdown {
        score_projection: (6.0, 2.0),
        total: (6.0, 2.0),
        hp_data_reliable: false,
        ..Default::default()
    };
    let commands =
        vec![DrawCommand::TeamAdvantage { level: Some(AdvantageLevel::Weak), color: [255, 255, 255], breakdown }];

    let lines = advantage_hover(&commands).expect("the frame carries an advantage");
    assert!(lines.iter().any(|line| line.contains("+4.0")), "the difference is shown: {lines:?}");
    assert!(lines.iter().any(|line| line.contains("incomplete")), "and that the HP data was not reliable: {lines:?}");
}

/// A map point projects to where the pointer that named it was, so a peer's
/// cursor lands where they are pointing.
///
/// This is the inverse of the picking projection; the two have to agree or a
/// shared pointer drifts from what it is over.
#[test]
fn a_map_point_and_a_pointer_position_agree_both_ways() {
    let mut view = painted();

    for zoom in [1.0_f32, 3.5] {
        view.set_view(MapViewport::new(zoom, (40.0, 90.0)));

        let at = WindowPoint { x: 300.0, y: 200.0 + DEFAULT_MAP_ORIGIN.1 };
        let on_map = view.map_point(at, DEFAULT_MAP_ORIGIN).expect("the pointer is over the map");
        let back = view.element_point(on_map, DEFAULT_MAP_ORIGIN).expect("and the map point is on screen");

        assert!((back.x - at.x).abs() < 0.5, "x at zoom {zoom}");
        assert!((back.y - at.y).abs() < 0.5, "y at zoom {zoom}");
    }
}

/// A map point the viewport is not showing has nowhere to be drawn.
#[test]
fn a_point_outside_the_view_is_not_placed() {
    let mut view = painted();
    assert!(view.element_point((10.0, 10.0), DEFAULT_MAP_ORIGIN).is_some(), "the whole map shows its corner");

    // Zoomed into the far corner, the near one is off screen.
    view.set_view(MapViewport::new(4.0, (2304.0, 2304.0)));
    assert!(view.element_point((10.0, 10.0), DEFAULT_MAP_ORIGIN).is_none());
}

/// Right-clicking a ship picks it at any zoom.
///
/// A pointer position and a ship's position have to be compared in the same
/// space. Picking used to compare a drawn-layer point against a map-space
/// one, so it only agreed at zoom 1 and picked the wrong ship, or none, as
/// soon as the reader zoomed in.
#[test]
fn a_ship_is_picked_at_any_zoom() {
    let mut view = painted();
    let on_map = MinimapPos { x: 300.0, y: 400.0 };
    let commands = vec![DrawCommand::Ship {
        entity_id: EntityId::from(7u32),
        pos: on_map,
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

    for zoom in [1.0_f32, 2.0, 5.0] {
        view.set_view(MapViewport::new(zoom, (200.0, 300.0)));
        let Some(at) = window_point(&view, (on_map.x, on_map.y)) else { continue };
        let picked = view
            .map_point(at, DEFAULT_MAP_ORIGIN)
            .and_then(|map| ship_at(&commands, map))
            .map(|ship| ShipMenu::open(ship, at));

        assert_eq!(picked.map(|menu| menu.player_name).as_deref(), Some("gapedd"), "at zoom {zoom}");
    }
}

/// Playing advances through the track and stops at the end rather than
/// running past it, and the clock reads the game time of where it got to.
#[test]
fn playing_walks_the_track_and_stops_at_its_end() {
    let track = track(vec![0.0, 30.0, 61.0]);
    let mut playback = Playback::new();

    assert_eq!(playback.clock_label(&track), "00:00", "it opens at the start of the battle");
    playback.toggle_playing(&track);
    assert!(playback.is_playing());

    // Three seconds of game time a tick at 30x: the second frame after ten ticks, and the end of
    // the track eleven after that, with the transport stopped there rather than run past it.
    playback.set_speed(30.0);
    let ticks = |playback: &mut Playback, count: usize| -> Vec<Tick> {
        (0..count).map(|_| playback.advance(&track, Duration::from_millis(100))).collect()
    };
    assert!(ticks(&mut playback, 9).iter().all(|tick| *tick == Tick::Moved), "between the first two frames");
    assert_eq!(ticks(&mut playback, 1), [Tick::Advanced], "the second frame is reached at 30s");
    let rest = ticks(&mut playback, 11);
    assert_eq!(rest.last(), Some(&Tick::Finished), "the last frame is reached at 61s: {rest:?}");
    assert!(rest[..10].iter().all(|tick| *tick == Tick::Moved), "and nothing before it: {rest:?}");

    assert_eq!(playback.at(), FrameIndex::new(2), "playback reached the last frame");
    assert!(!playback.is_playing(), "and stopped there rather than running past it");
    assert_eq!(playback.clock_label(&track), "01:01");
    assert_eq!(playback.advance(&track, Duration::from_millis(100)), Tick::Idle, "a stopped transport does not move");
}

/// A viewport with nothing baked yet has no frames to walk, and the transport
/// must not divide by their count.
#[test]
fn an_empty_track_has_nowhere_to_play_to() {
    let track = track(Vec::new());
    let mut playback = Playback::new();

    assert_eq!(playback.clock_label(&track), "00:00");
    playback.toggle_playing(&track);
    playback.go_to(&track, FrameIndex::new(5));
    assert_eq!(playback.at(), FrameIndex::FIRST, "there is nowhere to seek to");
}

/// A roster icon reads what the frame drew, looked up through the region the
/// target recorded as it drew it.
#[test]
fn a_roster_icon_reads_what_the_frame_drew() {
    let view = painted();
    let entity_id = EntityId::from(7u32);
    let consumable = |name: &str| RosterConsumable {
        icon_key: format!("PCY_{name}"),
        display_name: name.to_string(),
        description: String::new(),
        total_charges: ChargeCount::Finite(3),
        charges_used: 0,
        work_time_secs: 0.0,
        reload_time_secs: 0.0,
        active_remaining_secs: None,
        availability: ConsumableAvailability::Ready,
    };
    let commands = vec![DrawCommand::TeamRoster {
        side: RosterSide::Friendly,
        x: 0,
        y: 0,
        width: 200,
        height: 400,
        rows: vec![RosterRow {
            entity_id,
            team_id: 0,
            player_name: "gapedd".to_string(),
            clan_tag: None,
            clan_color: None,
            ship_name: "Smaland".to_string(),
            ship_param_id: None,
            class_icon_key: None,
            species: None,
            hp: Some(HitPoints { current: 100.0, max: 100.0, healable: 0.0, healable_per_charge: 0.0 }),
            heal_availability: ConsumableAvailability::Unavailable,
            is_dead: false,
            is_self: false,
            is_spotted: false,
            is_disconnected: false,
            kills: 0,
            damage_dealt: 0.0,
            seconds_since_damage: None,
            consumables: vec![consumable("Damage Control"), consumable("Repair Party")],
        }],
    }];
    // The frame is laid out at its own size, so a canvas pixel is a window
    // pixel.
    let regions = vec![
        DrawnRegion { rect: [10.0, 10.0, 20.0, 20.0], kind: RegionKind::RosterConsumable { entity_id, index: 0 } },
        DrawnRegion { rect: [40.0, 10.0, 20.0, 20.0], kind: RegionKind::RosterConsumable { entity_id, index: 1 } },
    ];
    let reading = |x: f32, y: f32| {
        let at = view.canvas_point(WindowPoint { x, y }).expect("the frame was painted");
        consumable_at(&regions, &commands, at)
    };

    let first = reading(15.0, 15.0).expect("the pointer is on the first icon");
    assert_eq!(first.lines.name, "Damage Control");
    assert_eq!(first.player, "gapedd", "and it says whose it is");

    let second = reading(45.0, 15.0).expect("and on the second");
    assert_eq!(second.lines.name, "Repair Party", "the index picks the icon, not the row");

    assert!(reading(32.0, 15.0).is_none(), "between two icons is neither");
    assert!(reading(400.0, 400.0).is_none(), "and the map is not a roster");
}

/// The rosters and the stats panel compete for the same gutters, so turning
/// one on clears the other, as the egui checkboxes do.
#[test]
fn the_rosters_and_the_stats_panel_do_not_share_the_gutters() {
    let rosters = toggle("renderer-opt-team-rosters");
    let stats = toggle("renderer-opt-stats-panel");
    let mut options = RenderOptions { show_stats_panel: true, ..playback_options() };
    let mut dead = true;

    (rosters.write)(&mut options, &mut dead, true);
    assert!(options.show_team_rosters);
    assert!(!options.show_stats_panel, "the stats panel gives the gutter up");

    (rosters.write)(&mut options, &mut dead, false);
    assert!(!options.show_team_rosters);
    assert!(!options.show_stats_panel, "and turning the rosters off does not bring it back");

    (rosters.write)(&mut options, &mut dead, true);
    (stats.write)(&mut options, &mut dead, true);
    assert!(options.show_stats_panel);
    assert!(!options.show_team_rosters, "the rosters give the gutter up to the stats panel");
}

/// Turning the rosters or the stats panel on is a wider canvas rather than
/// another layer, so the layout has to follow the switch.
#[test]
fn the_canvas_follows_the_gutter_layer_that_is_on() {
    let plain = playback_options_hidden();
    assert_eq!(crate::minimap_preview::layout_for(&plain), SidePanelLayout::None);

    let rosters = RenderOptions { show_team_rosters: true, ..plain.clone() };
    assert_eq!(crate::minimap_preview::layout_for(&rosters), SidePanelLayout::TeamRosters);

    let stats = RenderOptions { show_stats_panel: true, ..plain };
    assert_eq!(crate::minimap_preview::layout_for(&stats), SidePanelLayout::StatsPanel);
}

/// While a wider canvas is built, a frame is drawn on the canvas there is, with the gutter layers
/// it has no room for left off.
#[test]
fn the_options_drawn_fit_the_canvas_there_is() {
    use crate::replay_render::raster::fit_to_canvas;
    use std::borrow::Cow;

    let plain = playback_options_hidden();
    let rosters = RenderOptions { show_team_rosters: true, ..plain.clone() };

    assert!(matches!(fit_to_canvas(&rosters, SidePanelLayout::TeamRosters), Cow::Borrowed(_)), "it fits already");
    let on_bare_map = fit_to_canvas(&rosters, SidePanelLayout::None);
    assert_eq!(crate::minimap_preview::layout_for(&on_bare_map), SidePanelLayout::None);
    let stats = RenderOptions { show_stats_panel: true, ..rosters.clone() };
    assert_eq!(
        crate::minimap_preview::layout_for(&fit_to_canvas(&stats, SidePanelLayout::StatsPanel)),
        SidePanelLayout::StatsPanel
    );
    assert_eq!(
        crate::minimap_preview::layout_for(&fit_to_canvas(&plain, SidePanelLayout::TeamRosters)),
        SidePanelLayout::TeamRosters
    );
}

/// A playback bake carries the rosters and the stats panel, or their switches
/// would have nothing to show.
#[test]
fn a_playback_bake_carries_the_rosters_and_the_stats_panel() {
    assert!(playback_options().show_team_rosters);
    assert!(playback_options().show_stats_panel);
    assert!(!playback_options_hidden().show_team_rosters, "but a viewport opens without them");
    assert!(!playback_options_hidden().show_stats_panel);
}

/// Both panels are baked into every frame, and the switches decide which one
/// reaches it: the stats panel's commands, and the kill feed and chat it
/// replaces, follow whether the panel is drawn rather than whether it is baked.
#[test]
fn the_stats_panel_switch_decides_what_reaches_the_frame() {
    let mut track = track(vec![0.0]);
    track.frames[0] = vec![
        DrawCommand::StatsPanel { x: 768, width: 250 },
        DrawCommand::KillFeed { entries: Vec::new() },
        DrawCommand::TeamRoster { side: RosterSide::Friendly, x: 0, y: 0, width: 200, height: 400, rows: Vec::new() },
    ];
    let kinds = |options: &RenderOptions| -> Vec<&'static str> {
        drawn_at(&track, FrameIndex::FIRST, options, true, &ShipChoices::new(), Vec::new())
            .expect("the frame is in the track")
            .iter()
            .map(|command| match command {
                DrawCommand::StatsPanel { .. } => "stats",
                DrawCommand::KillFeed { .. } => "kills",
                DrawCommand::TeamRoster { .. } => "rosters",
                _ => "other",
            })
            .collect()
    };
    let mut options = playback_options_hidden();
    let mut dead = true;

    assert_eq!(kinds(&options), ["kills"], "neither panel is drawn when a viewport opens");

    (toggle("renderer-opt-stats-panel").write)(&mut options, &mut dead, true);
    assert_eq!(kinds(&options), ["stats"], "the panel replaces the kill feed");

    (toggle("renderer-opt-team-rosters").write)(&mut options, &mut dead, true);
    assert_eq!(kinds(&options), ["kills", "rosters"], "the rosters take the gutter and the kill feed returns");
}

/// What a session has drawn reaches the frame, over the battle rather than
/// filtered with it.
#[test]
fn what_a_session_has_drawn_is_put_on_the_frame() {
    let (state, link, _sent) = session();
    state.lock().current_annotation_sync = Some(AnnotationSyncState {
        annotations: vec![Annotation::Circle {
            center: [400.0, 300.0],
            radius: 40.0,
            color: [255, 0, 0, 255],
            width: 2.0,
            filled: false,
        }],
        ..Default::default()
    });
    let track = track(vec![0.0, 30.0]);
    let annotator = Annotator::new();

    let drawn = annotator.overlay_commands(&link, &track, &GameDataCache::without_install());
    assert_eq!(drawn.len(), 1, "the circle became one command");

    // No display option hides what a reader drew deliberately.
    let hidden = playback_options_hidden();
    assert!(should_draw_command(&drawn[0], &hidden, false), "and nothing filters it out");
    let shown = drawn_at(&track, FrameIndex::FIRST, &hidden, false, &ShipChoices::new(), drawn);
    assert_eq!(shown.map(|shown| shown.len()), Some(1), "so it reaches the frame");
}

/// Drawing with a tool sends what it drew to the session, and the pointer
/// goes to the tool rather than to panning the map.
#[test]
fn a_tool_in_hand_draws_rather_than_pans() {
    let (_state, link, sent) = session();
    let mut view = painted();
    let mut annotator = Annotator::new();

    let zoomed = MapViewport::new(2.0, (100.0, 100.0));
    view.set_view(zoomed);
    annotator.take_up(Tool::Line);

    let from = window_point(&view, (200.0, 200.0)).expect("a point the viewport is showing");
    let to = window_point(&view, (300.0, 260.0)).expect("and another");
    assert_eq!(press(&mut annotator, &link, &mut view, from), PointerDown::Stroked);
    assert!(!view.is_panning(), "the drag went to the tool, not to a pan");
    assert_eq!(view.view(), zoomed, "so the map did not move");

    view.end_pan();
    assert_eq!(annotator.pointer_up(&link, &view, DEFAULT_MAP_ORIGIN, to), PointerUp::Stroked);

    let drawn: Vec<Annotation> = sent
        .try_iter()
        .filter_map(|event| match event {
            LocalEvent::Annotation(LocalAnnotationEvent::Set { annotation, .. }) => Some(annotation),
            _ => None,
        })
        .collect();
    let [Annotation::Line { start, end, .. }] = &drawn[..] else {
        panic!("the line tool sent one line, got {drawn:?}");
    };
    assert!((start[0] - 200.0).abs() < 1.0 && (start[1] - 200.0).abs() < 1.0, "drawn in map space: {start:?}");
    assert!((end[0] - 300.0).abs() < 1.0 && (end[1] - 260.0).abs() < 1.0, "{end:?}");
}

/// Without a tool in hand a drag still pans, so drawing does not cost the
/// reader the map.
#[test]
fn without_a_tool_a_drag_still_pans() {
    let link = CollabLink::default();
    let mut view = painted();
    let mut annotator = Annotator::new();

    view.set_view(MapViewport::new(2.0, (100.0, 100.0)));
    let at = window_point(&view, (200.0, 200.0)).expect("a point the viewport is showing");

    assert_eq!(press(&mut annotator, &link, &mut view, at), PointerDown::Unclaimed);
    assert!(view.is_panning());
}

/// A shape picked out and dragged moves, and the session hears about it once,
/// at the end, under the id it already knows.
#[test]
fn a_picked_shape_moves_with_the_drag() {
    let (state, link, sent) = session();
    state.lock().current_annotation_sync = Some(AnnotationSyncState {
        annotations: vec![Annotation::Circle {
            center: [200.0, 200.0],
            radius: 20.0,
            color: [255, 255, 255, 255],
            width: 2.0,
            filled: false,
        }],
        ids: vec![4242],
        owners: vec![7],
    });
    let mut view = painted();
    let mut annotator = Annotator::new();

    // On the circle's own edge, which is where a hit test finds it.
    let on = window_point(&view, (220.0, 200.0)).expect("a point the viewport is showing");
    assert_eq!(annotator.click(&link, &view, DEFAULT_MAP_ORIGIN, on, false, 1), Click::Picked);
    assert_eq!(annotator.picked().picked(), [0], "the click picked the circle out");

    assert_eq!(press(&mut annotator, &link, &mut view, on), PointerDown::Moving);
    assert!(!view.is_panning(), "the drag moves the shape rather than the map");

    let to = window_point(&view, (260.0, 230.0)).expect("and another");
    assert_eq!(annotator.pointer_up(&link, &view, DEFAULT_MAP_ORIGIN, to), PointerUp::Moved);

    let moved: Vec<(u64, Annotation)> = sent
        .try_iter()
        .filter_map(|event| match event {
            LocalEvent::Annotation(LocalAnnotationEvent::Set { id, annotation, .. }) => Some((id, annotation)),
            _ => None,
        })
        .collect();
    let [(id, Annotation::Circle { center, .. })] = &moved[..] else {
        panic!("one update for the one shape, got {moved:?}");
    };
    assert_eq!(*id, 4242, "under the id the session already knows");
    assert!((center[0] - 240.0).abs() < 1.0 && (center[1] - 230.0).abs() < 1.0, "moved by the drag: {center:?}");
}

/// A part-drawn shape reaches the frame, so a reader dragging sees what they
/// are about to get rather than nothing until they let go.
#[test]
fn a_part_drawn_shape_reaches_the_frame() {
    let (_state, link, _sent) = session();
    let mut view = painted();
    let mut annotator = Annotator::new();

    annotator.take_up(Tool::Circle { filled: false });
    let from = window_point(&view, (200.0, 200.0)).expect("a point the viewport is showing");
    let to = window_point(&view, (240.0, 200.0)).expect("and another");

    press(&mut annotator, &link, &mut view, from);
    annotator.pointer_move(&link, &view, DEFAULT_MAP_ORIGIN, to, true, false);

    assert_eq!(annotator.pointer_at().map(|at| at[0].round()), Some(240.0), "the pointer is followed");
    let part_drawn = annotator.drawing().in_progress(annotator.pointer_at().expect("the pointer is on the map"));
    let Some(Annotation::Circle { center, radius, .. }) = part_drawn else {
        panic!("a circle is part-drawn, got {part_drawn:?}");
    };
    assert!((center[0] - 200.0).abs() < 1.0 && (center[1] - 200.0).abs() < 1.0, "{center:?}");
    assert!((radius - 40.0).abs() < 1.0, "as wide as the drag has gone: {radius}");
}

/// Drawing with nobody connected still puts the shape on the map.
#[test]
fn drawing_without_a_session_still_puts_a_shape_on_the_map() {
    let link = CollabLink::default();
    let mut annotator = Annotator::new();

    annotator.take_up(Tool::Line);
    annotator.stroke(&link, Stroke::Began { at: [100.0, 100.0] });
    annotator.stroke(&link, Stroke::Ended { at: [200.0, 100.0] });
    assert_eq!(link.annotations().len(), 1, "the line a reader drew with nobody connected");
}

/// With nobody connected a drawing and an erasure are each taken back by an undo, as the egui
/// board's are.
#[test]
fn drawing_alone_is_undone() {
    let link = CollabLink::default();
    let mut annotator = Annotator::new();

    annotator.take_up(Tool::Line);
    annotator.stroke(&link, Stroke::Began { at: [100.0, 100.0] });
    annotator.stroke(&link, Stroke::Ended { at: [200.0, 100.0] });
    annotator.take_up(Tool::None);
    assert_eq!(annotator.undo_depth(), 1, "the drawing can be taken back");

    annotator.pick_at(&link, [150.0, 100.0], false);
    assert!(annotator.erase_picked(&link));
    assert_eq!(link.annotation_count(), 0);

    assert!(annotator.undo(&link));
    assert_eq!(link.annotation_count(), 1, "the erasure is taken back");
    assert!(annotator.undo(&link));
    assert_eq!(link.annotation_count(), 0, "and then the drawing");
    assert!(!annotator.can_undo());
}

/// The button of the shape already in hand fills it, and the next press empties it again, as the
/// egui toolbar's second click does; what is drawn then carries the fill.
#[test]
fn a_second_press_of_a_shape_button_toggles_its_fill() {
    let link = CollabLink::default();
    let mut annotator = Annotator::new();
    let in_hand = |annotator: &Annotator| annotator.drawing().tool().clone();

    annotator.choose(Tool::Circle { filled: false });
    assert_eq!(in_hand(&annotator), Tool::Circle { filled: false }, "the first press takes the circle up");
    annotator.choose(Tool::Circle { filled: false });
    assert_eq!(in_hand(&annotator), Tool::Circle { filled: true }, "the second fills it");

    annotator.stroke(&link, Stroke::Began { at: [100.0, 100.0] });
    annotator.stroke(&link, Stroke::Ended { at: [140.0, 100.0] });
    assert!(
        matches!(link.annotations()[..], [Annotation::Circle { filled: true, .. }]),
        "the circle is drawn filled: {:?}",
        link.annotations()
    );

    annotator.choose(Tool::Circle { filled: false });
    assert_eq!(in_hand(&annotator), Tool::Circle { filled: false }, "the third empties it");
    annotator.choose(Tool::Circle { filled: false });
    annotator.choose(Tool::Rectangle { filled: false });
    assert_eq!(in_hand(&annotator), Tool::Rectangle { filled: false }, "another shape starts empty");
    annotator.choose(Tool::Line);
    annotator.choose(Tool::Line);
    assert_eq!(in_hand(&annotator), Tool::Line, "a tool with nothing to fill stays as it is");
}

/// Clear All takes every shape off, in a session under the ids it knows them by, and one undo
/// puts them all back.
#[test]
fn clearing_everything_is_one_undo() {
    let line = |y: f32| Annotation::Line { start: [100.0, y], end: [200.0, y], color: [255; 4], width: 2.0 };

    let link = CollabLink::default();
    let mut annotator = Annotator::new();
    assert!(!annotator.clear_all(&link), "nothing to clear");
    link.add_annotation(line(100.0));
    link.add_annotation(line(200.0));
    assert!(annotator.clear_all(&link));
    assert_eq!(link.annotation_count(), 0, "alone, everything goes");
    assert!(annotator.undo(&link));
    assert_eq!(link.annotations(), [line(100.0), line(200.0)], "and comes back in order");

    let (state, link, sent) = session();
    state.lock().current_annotation_sync =
        Some(AnnotationSyncState { annotations: vec![line(100.0), line(200.0)], ids: vec![5, 6], owners: vec![1, 2] });
    let mut annotator = Annotator::new();
    assert!(annotator.clear_all(&link));
    let removed: Vec<u64> = sent
        .try_iter()
        .filter_map(|event| match event {
            LocalEvent::Annotation(LocalAnnotationEvent::Remove { id, .. }) => Some(id),
            _ => None,
        })
        .collect();
    assert_eq!(removed.len(), 2, "the session is told each shape went: {removed:?}");
    assert!(removed.contains(&5) && removed.contains(&6));

    assert!(annotator.undo(&link));
    assert_eq!(link.annotations(), [line(100.0), line(200.0)], "one undo puts them back");
}

/// Undoing takes the shape off again under the id it was added with.
#[test]
fn undoing_takes_back_what_was_just_drawn() {
    let (state, link, sent) = session();
    let mut annotator = Annotator::new();

    assert_eq!(annotator.undo_depth(), 0, "nothing to undo before anything is drawn");
    annotator.take_up(Tool::Line);
    annotator.stroke(&link, Stroke::Began { at: [100.0, 100.0] });
    annotator.stroke(&link, Stroke::Ended { at: [200.0, 100.0] });
    assert_eq!(annotator.undo_depth(), 1, "the list as it was before the line");

    // The session now holds what was drawn, under the id it was sent with,
    // which is what an undo has to name.
    let drawn: Vec<(u64, Annotation)> = sent
        .try_iter()
        .filter_map(|event| match event {
            LocalEvent::Annotation(LocalAnnotationEvent::Set { id, annotation, .. }) => Some((id, annotation)),
            _ => None,
        })
        .collect();
    let [(id, annotation)] = &drawn[..] else { panic!("one line was drawn, got {drawn:?}") };
    state.lock().current_annotation_sync =
        Some(AnnotationSyncState { annotations: vec![annotation.clone()], ids: vec![*id], owners: vec![0] });

    assert!(annotator.undo(&link));
    assert_eq!(annotator.undo_depth(), 0, "and there is nothing left to undo");

    let removed: Vec<u64> = sent
        .try_iter()
        .filter_map(|event| match event {
            LocalEvent::Annotation(LocalAnnotationEvent::Remove { id, .. }) => Some(id),
            _ => None,
        })
        .collect();
    assert_eq!(removed, vec![*id], "the line goes, under the id it arrived with");
}

/// Placing a ship sends one, tinted by the side it was placed for and facing
/// the way the tool was holding it.
#[test]
fn placing_a_ship_sends_one_for_the_side_it_was_placed_for() {
    let (_state, link, sent) = session();
    let mut annotator = Annotator::new();

    annotator.take_up(Tool::Ship { species: "Destroyer".to_string(), friendly: false, yaw: 0.75 });
    annotator.stroke(&link, Stroke::Clicked { at: [150.0, 250.0] });

    let placed: Vec<Annotation> = sent
        .try_iter()
        .filter_map(|event| match event {
            LocalEvent::Annotation(LocalAnnotationEvent::Set { annotation, .. }) => Some(annotation),
            _ => None,
        })
        .collect();
    let [Annotation::Ship { pos, yaw, species, friendly, .. }] = &placed[..] else {
        panic!("one ship was placed, got {placed:?}");
    };
    assert_eq!(*pos, [150.0, 250.0]);
    assert_eq!(*yaw, 0.75);
    assert_eq!(species, "Destroyer");
    assert!(!friendly, "for the side the tool was holding");

    // And it draws as a ship rather than as nothing.
    let drawn = wt_collab_client::geometry::annotation_commands(&placed[0]);
    assert!(matches!(drawn.as_slice(), [DrawCommand::Ship { .. }]), "{drawn:?}");
}

/// Naming a placed ship gives it an identity, which is what its ranges are
/// read from, and leaves everything else about it alone.
#[test]
fn naming_a_placed_ship_gives_it_an_identity() {
    let (state, link, sent) = session();
    state.lock().current_annotation_sync = Some(AnnotationSyncState {
        annotations: vec![Annotation::Ship {
            pos: [300.0, 400.0],
            yaw: 0.5,
            species: "Cruiser".to_string(),
            friendly: false,
            config: None,
        }],
        ids: vec![11],
        owners: vec![3],
    });
    let mut annotator = Annotator::new();

    // Nothing to choose a ship for until one is picked out.
    assert!(annotator.picked_ship(&link).is_none());
    annotator.pick_at(&link, [300.0, 400.0], false);
    assert_eq!(annotator.picked_ship(&link), Some(None), "picked, and unnamed");

    // Nothing was sent by picking alone.
    assert!(sent.try_iter().next().is_none(), "picking a shape changes nothing");

    // Without game data loaded there is no param to resolve, so nothing is
    // sent rather than a ship with an id of nothing.
    let named = annotator.name_picked_ship(
        &link,
        &GameDataCache::without_install(),
        Species::Cruiser,
        "PRSC610",
        "Moskva".to_string(),
    );
    assert!(!named);

    let updates: Vec<Annotation> = sent
        .try_iter()
        .filter_map(|event| match event {
            LocalEvent::Annotation(LocalAnnotationEvent::Set { annotation, .. }) => Some(annotation),
            _ => None,
        })
        .collect();
    assert!(updates.is_empty(), "a ship with no game data behind it is not named: {updates:?}");
}

/// The armor item is offered only for a ship the battle recorded hits on,
/// since where those landed is the whole point of the viewer.
#[test]
fn the_armor_item_is_offered_only_for_a_ship_that_was_hit() {
    let game_data = GameDataCache::without_install();
    let hit_ship = EntityId::from(7u32);
    let untouched = EntityId::from(8u32);
    let mut shots = HashMap::new();

    assert!(!has_armor_to_show(&shots, &[], hit_ship, &game_data), "before the battle has been read");

    // A ship with a timeline but nothing in it was never hit.
    shots.insert(untouched, ShipShotTimeline { hits: Vec::new(), health_history: BTreeMap::new(), hull: None });
    assert!(!has_armor_to_show(&shots, &[], untouched, &game_data), "a timeline with no hits is not an offer");

    // Asking for one anyway is refused rather than opening an empty viewer:
    // without game data there is no ship to load.
    assert!(ArmorFollow::start(untouched, &[], &shots, GameClock(0.0), &game_data).is_none());
}

/// A viewer that is open follows playback: every move tells it the clock and the health, so the
/// health strip and readout move between hits, but the hits only when they change, since showing
/// them again rebuilds the hull.
#[test]
fn an_open_armor_viewer_follows_the_clock_and_only_gets_hits_that_changed() {
    let ship = EntityId::from(7u32);
    // Following starts without a viewer, which is what the menu's own guard
    // is for; seeded here directly.
    let mut seeded = feed();
    seeded.advance_to(GameClock(0.0));
    let mut follow = ArmorFollow::new(ship, seeded);
    let mut moved_to = |step: usize| follow.follow(GameClock(step as f32 * 5.0));

    // This battle recorded no hits, so walking it hands over no hits, while the clock and the
    // health still move.
    let told: Vec<ArmorFollowed> = (1..8).filter_map(&mut moved_to).collect();
    assert_eq!(told.len(), 7, "every move tells the clock");
    assert!(told.iter().all(|followed| followed.hits.is_none()), "nothing was hit, so no hits are handed over");
    assert_eq!(told.last().map(|followed| (followed.at, followed.health)), Some((GameClock(35.0), Some(12000.0))));
    assert_eq!(told.first().and_then(|followed| followed.health), Some(15400.0), "before the damage at 30 s");

    // Standing still says nothing.
    assert!(moved_to(7).is_none(), "playback did not move");

    // Scrubbing back is the other case: what was drawn can no longer be
    // right, and a hull cannot be un-hit one shell at a time, so the viewer
    // is handed the hits again.
    let rewound = moved_to(1).expect("a step backwards is told");
    assert_eq!(rewound.hits.map(|hits| hits.len()), Some(0), "a step backwards hands the hull over again");
}

/// The clock says where playback is and how long the battle runs, so the end
/// is visible without reading the bar.
#[test]
fn the_clock_says_where_playback_is_and_how_long_there_is() {
    // Half-second frames over a minute of recording.
    let mut track = track((0..120).map(|frame| frame as f32 * 0.5).collect());
    let mut playback = Playback::new();

    // The battle starts forty seconds into the recording, so the clock counts
    // from there and so does the length.
    set_battle_window(&mut track, 40.0, None);
    assert_eq!(playback.clock_label(&track), "00:00", "the loading screen is not minus forty");
    assert_eq!(track.length_label(), "00:19", "and the battle runs to the end of the track");
    assert_eq!(playback.clock_and_length(&track), "00:00 / 00:19");

    playback.go_to(&track, FrameIndex::new(100));
    assert_eq!(playback.clock_and_length(&track), "00:10 / 00:19", "the length does not move with playback");
}

/// An export says how far it has got, and says which part of the work is
/// running: muxing comes after the last frame and takes a noticeable while
/// with nothing else to show for it.
#[test]
fn an_export_reports_its_progress_over_the_video() {
    let starting = ExportProgress { done: 0, total: 0, stage: ExportStage::Encoding };
    assert_eq!(starting.fraction(), None, "nothing to measure against while the encoder starts");
    let partway = ExportProgress { done: 40, total: 100, stage: ExportStage::Encoding };
    assert_eq!(partway.fraction(), Some(0.4));

    assert_eq!(ExportStage::Encoding.label_key(), "ui.renderer.encoding");
    assert_eq!(ExportStage::Muxing.label_key(), "ui.renderer.muxing", "and the wait after the last frame is named");
}

/// A wide viewport fills its sides as the map is zoomed into, and stays square
/// while there is no map to put there.
#[test]
fn a_wide_viewport_fills_its_sides_as_the_map_is_zoomed() {
    let mut view = MapView::new();
    // A viewport twice as wide as the canvas is tall.
    let (w, h) = (MINIMAP_SIZE as f32, CANVAS_HEIGHT as f32);
    view.set_placement(Some(Placement {
        bounds: ElementBounds { origin: WindowPoint { x: 0.0, y: 0.0 }, width: w * 2.0, height: h },
        frame: FrameSize { width: MINIMAP_SIZE, height: CANVAS_HEIGHT },
    }));

    assert_eq!(view.shaped_view().widen(), 1.0, "at rest there is no map to put beside it");

    view.set_view(MapViewport::new(1.2, (0.0, 0.0)));
    let creeping = view.shaped_view().widen();
    assert!(creeping > 1.0 && creeping <= 1.2, "it starts to fill, held to the zoom: {creeping}");

    view.set_view(MapViewport::new(6.0, (0.0, 0.0)));
    let filled = view.shaped_view().widen();
    assert!(filled > creeping, "and fills further in: {filled} against {creeping}");
    assert!(filled > 1.9 && filled < 2.1, "about the shape of the viewport: {filled}");
}

/// A ping is shed only once its ripple has run out.
#[test]
fn a_ping_outlives_its_own_frame() {
    let (state, link, _sent) = session();
    push_ping_aged(&state, [10.0, 10.0], Duration::from_millis(200));

    link.drop_stale_pings(Duration::from_secs_f32(PING_SECONDS));
    assert_eq!(state.lock().pings.len(), 1, "a fifth of a second into a one-second ripple");

    link.drop_stale_pings(Duration::from_millis(100));
    assert!(state.lock().pings.is_empty(), "and it goes once it has run out");
}

/// The first move draws the hull from nothing, because a viewport can open
/// anywhere in the battle and there may already be hits behind it.
#[test]
fn the_first_move_starts_the_hull_again() {
    let mut feed = feed();
    assert!(feed.at().is_none());
    assert_eq!(feed.advance_to(GameClock(60.0)), Advance::Rewound);
    assert_eq!(feed.at(), Some(GameClock(60.0)));
}

/// Stepping forward reports what it gained rather than everything.
#[test]
fn a_step_forward_reports_only_what_it_gained() {
    let mut feed = feed();
    feed.advance_to(GameClock(10.0));
    assert_eq!(feed.advance_to(GameClock(40.0)), Advance::Gained(0..0), "no hits in this timeline");
}

/// Scrubbing backwards cannot be undrawn hit by hit.
#[test]
fn a_step_backwards_asks_for_the_hull_again() {
    let mut feed = feed();
    feed.advance_to(GameClock(60.0));
    assert_eq!(feed.advance_to(GameClock(20.0)), Advance::Rewound);
}

/// Health follows the clock, holding its last reading.
#[test]
fn health_follows_where_playback_reached() {
    let mut feed = feed();
    assert!(feed.health().is_none(), "before playback has been placed");

    feed.advance_to(GameClock(10.0));
    assert_eq!(feed.health(), Some(15400.0));

    feed.advance_to(GameClock(45.0));
    assert_eq!(feed.health(), Some(12000.0));

    feed.advance_to(GameClock(5.0));
    assert_eq!(feed.health(), Some(15400.0), "and it follows a scrub backwards too");
}
