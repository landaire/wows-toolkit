//! The replay settings drive the commands and the CPU frame of a paused viewport.

use std::collections::HashSet;
use std::io::Cursor;
use std::io::Write as _;
use std::path::PathBuf;
use std::time::Instant;

use image::ImageFormat;
use image::Rgb;
use image::RgbImage;
use wows_minimap_renderer::STATS_PANEL_WIDTH;
use wows_minimap_renderer::TEAM_ROSTER_WIDTH;
use wows_minimap_renderer::draw_command::ActivityFeedEntry;
use wows_minimap_renderer::draw_command::ActivityFeedKind;
use wows_minimap_renderer::draw_command::BatteryFraction;
use wows_minimap_renderer::draw_command::BuildingRelation;
use wows_minimap_renderer::draw_command::ChatEntry;
use wows_minimap_renderer::draw_command::KillFeedEntry;
use wows_minimap_renderer::draw_command::Meters;
use wows_minimap_renderer::draw_command::RosterSide;
use wows_minimap_renderer::draw_command::RowShade;
use wows_minimap_renderer::draw_command::ShipConfigCircleKind;
use wows_minimap_renderer::draw_command::ShipConfigFilter;
use wows_minimap_renderer::drawing::SidePanelLayout;
use wows_minimap_renderer::toggles::TOGGLES;
use wows_minimap_renderer::trail::BakedTrails;
use wows_replays::ReplayFile;
use wows_replays::types::PlaneId;
use wows_toolkit_replay_data::frame::BgraFrame;
use wows_toolkit_replay_data::load::GameDataCache;
use wows_toolkit_replay_data::load::LoadedGameData;
use wows_toolkit_replay_data::replay_render::motion::FrameMotion;
use wows_toolkit_replay_data::replay_render::raster::build_canvas;
use wows_toolkit_replay_data::replay_render::raster::raster_commands;
use wowsunpack::game_types::BattleResult;
use wowsunpack::vfs::MemoryFS;
use wowsunpack::vfs::VfsPath;
use zgui::reactive::prelude::Update;
use zgui::vocab::Toggled;

use super::*;

const GEAR: &str = "replay-renderer-settings-toggle";
const ADVANTAGE_HOVER: &str = "replay-renderer-advantage-hover";
const AGE_TRAILS: &str = "renderer-opt-trails";
const SPEED_TRAILS: &str = "renderer-opt-speed-trails";
const DEAD_TRAILS: &str = "renderer-opt-dead-trails";
const RANGES: &str = "renderer-opt-ship-ranges";
const ROSTERS: &str = "renderer-opt-team-rosters";
const STATS: &str = "renderer-opt-stats-panel";
const CHAT: &str = "renderer-opt-chat";
const KILL_FEED: &str = "renderer-opt-kill-feed";
const SHIP_POSITION: MinimapPos = MinimapPos { x: 250.0, y: 200.0 };

struct DisplaySwitch {
    id: &'static str,
    visible: fn(&DrawCommand) -> bool,
}

const MAP_SWITCHES: &[DisplaySwitch] = &[
    DisplaySwitch { id: "renderer-opt-hp-bars", visible: |cmd| matches!(cmd, DrawCommand::HealthBar { .. }) },
    DisplaySwitch { id: "renderer-opt-tracers", visible: |cmd| matches!(cmd, DrawCommand::ShotTracer { .. }) },
    DisplaySwitch { id: "renderer-opt-torpedoes", visible: |cmd| matches!(cmd, DrawCommand::Torpedo { .. }) },
    DisplaySwitch { id: "renderer-opt-planes", visible: |cmd| matches!(cmd, DrawCommand::Plane { .. }) },
    DisplaySwitch { id: "renderer-opt-smoke", visible: |cmd| matches!(cmd, DrawCommand::Smoke { .. }) },
    DisplaySwitch {
        id: "renderer-opt-consumables",
        visible: |cmd| matches!(cmd, DrawCommand::ConsumableRadius { .. }),
    },
    DisplaySwitch {
        id: "renderer-opt-submarine-surveillance",
        visible: |cmd| matches!(cmd, DrawCommand::SubmarineSurveillanceRadius { .. }),
    },
    DisplaySwitch {
        id: "renderer-opt-submarine-status",
        visible: |cmd| matches!(cmd, DrawCommand::SubmarineStatus { .. }),
    },
    DisplaySwitch { id: "renderer-opt-capture-points", visible: |cmd| matches!(cmd, DrawCommand::CapturePoint { .. }) },
    DisplaySwitch { id: "renderer-opt-buildings", visible: |cmd| matches!(cmd, DrawCommand::Building { .. }) },
    DisplaySwitch {
        id: "renderer-opt-player-names",
        visible: |cmd| matches!(cmd, DrawCommand::Ship { player_name: Some(_), .. }),
    },
    DisplaySwitch {
        id: "renderer-opt-ship-names",
        visible: |cmd| matches!(cmd, DrawCommand::Ship { ship_name: Some(_), .. }),
    },
    DisplaySwitch { id: "renderer-opt-dead-ships", visible: |cmd| matches!(cmd, DrawCommand::DeadShip { .. }) },
    DisplaySwitch {
        id: "renderer-opt-dead-ship-names",
        visible: |cmd| matches!(cmd, DrawCommand::DeadShip { player_name: Some(_), ship_name: Some(_), .. }),
    },
    DisplaySwitch {
        id: "renderer-opt-camera-direction",
        visible: |cmd| matches!(cmd, DrawCommand::CameraDirection { .. }),
    },
    DisplaySwitch {
        id: "renderer-opt-armament",
        visible: |cmd| matches!(cmd, DrawCommand::Ship { name_color: Some(_), .. }),
    },
    DisplaySwitch { id: "renderer-opt-score", visible: |cmd| matches!(cmd, DrawCommand::ScoreBar { .. }) },
    DisplaySwitch { id: "renderer-opt-timer", visible: |cmd| matches!(cmd, DrawCommand::Timer { .. }) },
    DisplaySwitch { id: KILL_FEED, visible: |cmd| matches!(cmd, DrawCommand::KillFeed { .. }) },
    DisplaySwitch { id: CHAT, visible: |cmd| matches!(cmd, DrawCommand::ChatOverlay { .. }) },
    DisplaySwitch {
        id: "renderer-opt-battle-result",
        visible: |cmd| matches!(cmd, DrawCommand::BattleResultOverlay { .. }),
    },
    DisplaySwitch { id: "renderer-opt-buffs", visible: |cmd| matches!(cmd, DrawCommand::TeamBuffs { .. }) },
    DisplaySwitch {
        id: "renderer-opt-advantage",
        visible: |cmd| matches!(cmd, DrawCommand::ScoreBar { advantage: Some(_), .. }),
    },
    DisplaySwitch {
        id: "renderer-opt-score-timers",
        visible: |cmd| matches!(cmd, DrawCommand::ScoreBar { team0_timer: Some(_), team1_timer: Some(_), .. }),
    },
];

#[test]
fn map_switches_redraw_the_paused_frame_in_both_directions() {
    let mut covered: Vec<_> = MAP_SWITCHES.iter().map(|case| case.id).collect();
    covered.extend([AGE_TRAILS, SPEED_TRAILS, DEAD_TRAILS, RANGES, ROSTERS, STATS]);
    covered.sort_unstable();
    let mut offered: Vec<_> = TOGGLES.iter().map(|toggle| toggle.id).collect();
    offered.sort_unstable();
    assert_eq!(covered, offered);

    let (mut window, session) = open_battle(map_commands());
    click(&mut window, GEAR);
    for DisplaySwitch { id, visible } in MAP_SWITCHES {
        check_switch(&mut window, session, id, |window, commands, on| {
            assert_eq!(commands.iter().any(visible), on, "{id}: draw command");
            assert!(commands.iter().any(|cmd| matches!(cmd, DrawCommand::Ship { .. })), "{id}: the ship remains");
            match *id {
                "renderer-opt-hp-bars" => {
                    for command in commands {
                        if let DrawCommand::ConsumableIcons { has_hp_bar, .. }
                        | DrawCommand::SubmarineStatus { has_hp_bar, .. } = command
                        {
                            assert_eq!(*has_hp_bar, on, "leave room only for a visible HP bar");
                        }
                    }
                }
                "renderer-opt-armament" => {
                    let tips = commands.iter().filter(|cmd| matches!(cmd, DrawCommand::ShotTracerTip { .. })).count();
                    assert_eq!(tips, usize::from(on), "shell tip tint follows the armament switch");
                    assert!(commands.iter().any(|cmd| matches!(cmd, DrawCommand::ShotTracer { .. })));
                }
                "renderer-opt-score" | "renderer-opt-advantage" => {
                    assert_eq!(shown(window, ADVANTAGE_HOVER), on);
                    assert_eq!(commands.iter().any(|cmd| matches!(cmd, DrawCommand::TeamAdvantage { .. })), on);
                }
                _ => {}
            }
        });
    }
}

#[test]
fn trail_and_range_switches_keep_per_ship_choices() {
    let live = EntityId::from(1u32);
    let dead = EntityId::from(2u32);
    let (mut window, session) = open_battle(vec![
        ship(),
        range_circle(live, "Captain", ShipConfigCircleKind::Detection, SHIP_POSITION),
        range_circle(live, "Captain", ShipConfigCircleKind::MainBattery, SHIP_POSITION),
        range_circle(EntityId::from(3u32), "Other", ShipConfigCircleKind::Detection, pos(450.0, 500.0)),
    ]);
    session.choices.update(|choices| {
        choices.replace_ranges(HashMap::from([(
            "Captain".to_owned(),
            ShipConfigFilter { detection: true, ..Default::default() },
        )]));
    });
    click(&mut window, GEAR);

    check_switch(&mut window, session, AGE_TRAILS, |_, commands, on| {
        assert_eq!(trails(commands).count(), if on { 2 } else { 0 });
    });
    check_switch(&mut window, session, SPEED_TRAILS, |_, commands, on| {
        assert_eq!(trails(commands).count(), if on { 2 } else { 0 });
    });

    press(&mut window, AGE_TRAILS);
    check_switch(&mut window, session, SPEED_TRAILS, |_, commands, on| {
        let (_, colors) = trails(commands).find(|(entity, _)| *entity == live).expect("the live ship has a trail");
        let hottest = colors.iter().position(|(_, color)| *color == [255, 0, 0]);
        assert_eq!(hottest, Some(if on { 1 } else { 2 }), "fastest sample vs. newest sample");
    });
    check_switch(&mut window, session, DEAD_TRAILS, |_, commands, on| {
        assert!(trails(commands).any(|(entity, _)| entity == live));
        assert_eq!(trails(commands).any(|(entity, _)| entity == dead), on);
    });

    session.choices.update(|choices| choices.replace_trail_hidden(HashSet::from(["Lost".to_owned()])));
    session.draw();
    check_switch(&mut window, session, AGE_TRAILS, |_, commands, on| {
        let ids: Vec<_> = trails(commands).map(|(entity, _)| entity).collect();
        if on {
            assert_eq!(ids, [live]);
        } else {
            assert!(ids.is_empty());
        }
    });

    check_switch(&mut window, session, RANGES, |_, commands, on| {
        let circles: Vec<_> = commands
            .iter()
            .filter_map(|command| match command {
                DrawCommand::ShipConfigCircle { entity_id, kind, .. } => Some((*entity_id, *kind)),
                _ => None,
            })
            .collect();
        if on {
            assert_eq!(circles, [(live, ShipConfigCircleKind::Detection)]);
        } else {
            assert!(circles.is_empty());
        }
    });
}

#[test]
fn panel_switches_replace_each_other_and_filter_the_activity_feed() {
    let mut commands = map_commands();
    let x = MINIMAP_SIZE as i32;
    let width = STATS_PANEL_WIDTH as i32;
    let clock = GameClock(2.0);
    commands.extend([
        DrawCommand::StatsPanel { x, width },
        DrawCommand::StatsActivityFeed {
            x,
            y: 120,
            width,
            height: 240,
            entries: vec![
                ActivityFeedEntry { clock, kind: ActivityFeedKind::Kill(kill()), reveal: 1.0, shade: RowShade::Plain },
                ActivityFeedEntry { clock, kind: ActivityFeedKind::Chat(chat()), reveal: 1.0, shade: RowShade::Banded },
            ],
        },
        DrawCommand::TeamRoster {
            side: RosterSide::Friendly,
            x: 0,
            y: 0,
            width: TEAM_ROSTER_WIDTH as i32,
            height: CANVAS_HEIGHT as i32,
            rows: Vec::new(),
        },
    ]);
    let (mut window, session) = open_battle(commands);
    click(&mut window, GEAR);
    check_panel(&mut window, session, SidePanelLayout::None);

    press(&mut window, STATS);
    check_panel(&mut window, session, SidePanelLayout::StatsPanel);
    for id in [KILL_FEED, CHAT] {
        check_switch(&mut window, session, id, |_, commands, on| {
            let entries = commands
                .iter()
                .find_map(|command| match command {
                    DrawCommand::StatsActivityFeed { entries, .. } => Some(entries),
                    _ => None,
                })
                .expect("stats activity feed");
            let has_kill = entries.iter().any(|entry| matches!(&entry.kind, ActivityFeedKind::Kill(_)));
            let has_chat = entries.iter().any(|entry| matches!(&entry.kind, ActivityFeedKind::Chat(_)));
            assert_eq!((has_kill, has_chat), (id != KILL_FEED || on, id != CHAT || on));
        });
    }

    press(&mut window, ROSTERS);
    check_panel(&mut window, session, SidePanelLayout::TeamRosters);
    press(&mut window, STATS);
    check_panel(&mut window, session, SidePanelLayout::StatsPanel);
    press(&mut window, STATS);
    check_panel(&mut window, session, SidePanelLayout::None);
    press(&mut window, ROSTERS);
    check_panel(&mut window, session, SidePanelLayout::TeamRosters);
    press(&mut window, ROSTERS);
    check_panel(&mut window, session, SidePanelLayout::None);
}

fn check_panel(window: &mut HeadlessWindow, session: RendererSession, layout: SidePanelLayout) {
    let frame = settled_frame(window, session);
    let stats = layout == SidePanelLayout::StatsPanel;
    let rosters = layout == SidePanelLayout::TeamRosters;
    let gutters = match layout {
        SidePanelLayout::None => 0,
        SidePanelLayout::StatsPanel => STATS_PANEL_WIDTH,
        SidePanelLayout::TeamRosters => 2 * TEAM_ROSTER_WIDTH,
    };
    assert_eq!(checked(window, STATS), stats);
    assert_eq!(checked(window, ROSTERS), rosters);
    assert_eq!(
        session.frame_size.get_untracked(),
        Some(FrameSize { width: MINIMAP_SIZE + gutters, height: CANVAS_HEIGHT })
    );
    assert!(frame.width() > 0 && frame.height() > 0);
    assert_eq!(session.map_origin.get_untracked().0, if rosters { TEAM_ROSTER_WIDTH as f32 } else { 0.0 });
    let commands = visible_commands(session);
    assert_eq!(commands.iter().any(|cmd| matches!(cmd, DrawCommand::StatsPanel { .. })), stats);
    assert_eq!(commands.iter().any(|cmd| matches!(cmd, DrawCommand::TeamRoster { .. })), rosters);
    assert_eq!(commands.iter().any(|cmd| matches!(cmd, DrawCommand::KillFeed { .. })), !stats);
    assert_eq!(commands.iter().any(|cmd| matches!(cmd, DrawCommand::ChatOverlay { .. })), !stats);
}

fn check_switch(
    window: &mut HeadlessWindow,
    session: RendererSession,
    id: &str,
    check: impl Fn(&HeadlessWindow, &[DrawCommand], bool),
) {
    let at = session.playback.with_untracked(Playback::at);
    let track = session.track_untracked().expect("the fixture is loaded");
    let initial = checked(window, id);
    let before = settled_frame(window, session);
    check(window, &visible_commands(session), initial);

    let asked = session.frames_asked();
    let previously_shown = session.shown_draw_for_test();
    press(window, id);
    assert!(session.frames_asked() > asked, "{id}: a paused redraw was requested");
    assert_eq!(checked(window, id), !initial, "{id}: the checkbox changed");
    let changed = wait_for_frame(window, session, id, || session.shown_draw_for_test() > previously_shown);
    check(window, &visible_commands(session), !initial);
    assert!(changed.bytes() != before.bytes(), "{id}: the displayed pixels changed");

    let asked = session.frames_asked();
    let previously_shown = session.shown_draw_for_test();
    press(window, id);
    assert!(session.frames_asked() > asked, "{id}: restoring also redraws the paused frame");
    assert_eq!(checked(window, id), initial, "{id}: the checkbox was restored");
    let restored = wait_for_frame(window, session, id, || session.shown_draw_for_test() > previously_shown);
    check(window, &visible_commands(session), initial);
    assert!(restored.bytes() == before.bytes(), "{id}: the displayed pixels were restored");
    assert_eq!(session.playback.with_untracked(Playback::at), at, "{id}: playback did not advance");
    assert!(!session.playback.with_untracked(Playback::is_playing));
    assert!(Arc::ptr_eq(&track, &session.track_untracked().expect("the track is still loaded")), "{id}: no rebake");
}

fn press(window: &mut HeadlessWindow, id: &str) {
    tab_to(window, id);
    click(window, id);
}

fn checked(window: &mut HeadlessWindow, id: &str) -> bool {
    let element = window.by_test_id(id).unwrap_or_else(|| panic!("{id} is shown"));
    match window.a11y(element).and_then(|state| state.checked) {
        Some(Toggled::True) => true,
        Some(Toggled::False) => false,
        other => panic!("{id}: expected a checkbox, got {other:?}"),
    }
}

fn settled_frame(window: &mut HeadlessWindow, session: RendererSession) -> BgraFrame {
    wait_for_frame(window, session, "the canvas", || session.draws_settled_for_test())
}

#[track_caller]
fn wait_for_frame(
    window: &mut HeadlessWindow,
    session: RendererSession,
    expected: &str,
    ready: impl Fn() -> bool,
) -> BgraFrame {
    let started = Instant::now();
    loop {
        settle(window);
        if ready()
            && let Some(frame) = session.sink().latest_frame()
        {
            return frame;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "{expected}: the requested CPU frame did not finish ({})",
            session.draw_status_for_test()
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn visible_commands(session: RendererSession) -> Vec<DrawCommand> {
    let track = session.track_untracked().expect("the fixture is loaded");
    let (at, clock) = session.playback.with_untracked(|playback| (playback.at(), playback.clock(&track)));
    let motion = FrameMotion::between(&track, at).expect("the paused frame exists");
    session.layers.with_untracked(|layers| {
        session.choices.with_untracked(|choices| {
            raster_commands(&track, &motion, clock, &layers.options, layers.show_dead_ships, choices, Vec::new())
        })
    })
}

fn open_battle(commands: Vec<DrawCommand>) -> (HeadlessWindow, RendererSession) {
    // Only the replay header identifies the canvas assets. Commands come from the fixture track.
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/replays/20260213_143518_PASB110-Vermont_22_tierra_del_fuego.wowsreplay");
    let replay = ReplayFile::from_file(&path).expect("the replay fixture reads");
    let version = Version::try_from_client_exe(&replay.meta.clientVersionFromExe).expect("the replay has a version");
    let provider = wowsunpack::game_params::provider::GameMetadataProvider::from_params_no_specs(Vec::new())
        .expect("the fixture provider builds");
    let vfs: VfsPath = MemoryFS::new().into();
    let map = replay.meta.mapName.strip_prefix("spaces/").unwrap_or(&replay.meta.mapName);
    let png = |size, color| {
        let mut bytes = Cursor::new(Vec::new());
        RgbImage::from_pixel(size, size, Rgb(color)).write_to(&mut bytes, ImageFormat::Png).expect("the PNG encodes");
        bytes.into_inner()
    };
    write_asset(&vfs, &format!("spaces/{map}/minimap.png"), &png(MINIMAP_SIZE, [5, 13, 19]));
    let icon = png(24, [243, 197, 123]);
    for path in [
        "gui/battle_hud/markers_minimap/plane/controllable/fighter_he_ally.png",
        "gui/consumables/consumable_PCY001.png",
        "gui/powerups/drops/icon_marker_damage.png",
    ] {
        write_asset(&vfs, path, &icon);
    }
    for path in
        ["gui/fla/minimap/ship_icons/minimap_submarine.svg", "gui/fla/minimap/ship_icons/minimap_auxiliary_dead.svg"]
    {
        write_asset(&vfs, path, br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24"><path fill="white" d="M2 2H22L12 22Z"/></svg>"#);
    }
    let data = GameDataCache::holding_for_test(Arc::new(LoadedGameData::for_test_with_version(provider, vfs, version)));
    let canvas = build_canvas(&path, &data, SidePanelLayout::None).expect("the replay canvas builds");
    let mut fixture = track(&[6.0, 6.5], 6.0);
    fixture.frames[0] = commands;
    let live = EntityId::from(1u32);
    let dead = EntityId::from(2u32);
    fixture.trails = BakedTrails::from_history(
        HashMap::from([
            (
                live,
                vec![
                    (pos(160.0, 210.0), GameClock(1.0), 0),
                    (pos(220.0, 220.0), GameClock(2.0), 300),
                    (SHIP_POSITION, GameClock(3.0), 50),
                ],
            ),
            (dead, vec![(pos(400.0, 520.0), GameClock(1.0), 0), (pos(450.0, 500.0), GameClock(2.0), 200)]),
        ]),
        |id| Some(if id == live { "Captain" } else { "Lost" }.to_owned()),
        |id| (id == dead).then_some(GameClock(4.0)),
    );
    let mut setup = Some((path, data, canvas));
    let (mut window, session) = open_with_setup(fixture, move |session| {
        let (path, data, canvas) = setup.take().expect("one fixture session");
        session.hold_canvas_for_test(path, data, canvas);
    });
    session.draw();
    settled_frame(&mut window, session);
    (window, session)
}

fn write_asset(vfs: &VfsPath, path: &str, bytes: &[u8]) {
    let asset = vfs.join(path).expect("valid asset path");
    asset.parent().create_dir_all().expect("asset directory");
    asset.create_file().expect("fixture asset").write_all(bytes).expect("written asset");
}

fn pos(x: f32, y: f32) -> MinimapPos {
    MinimapPos { x, y }
}

fn ship() -> DrawCommand {
    DrawCommand::Ship {
        entity_id: EntityId::from(1u32),
        pos: SHIP_POSITION,
        yaw: 0.0,
        species: Some("Submarine".into()),
        color: Some([140, 160, 180]),
        visibility: ShipVisibility::Visible,
        opacity: 1.0,
        is_self: false,
        player_name: Some("Captain".into()),
        ship_name: Some("Submarine".into()),
        is_detected_teammate: false,
        is_disconnected: false,
        name_color: Some([243, 151, 44]),
    }
}

fn range_circle(entity_id: EntityId, player_name: &str, kind: ShipConfigCircleKind, pos: MinimapPos) -> DrawCommand {
    DrawCommand::ShipConfigCircle {
        entity_id,
        pos,
        radius_px: 45.0,
        color: [250, 160, 50],
        alpha: 0.8,
        dashed: kind == ShipConfigCircleKind::Detection,
        label: Some("6.0 km".into()),
        kind,
        player_name: player_name.to_owned(),
        is_self: false,
    }
}

fn trails(commands: &[DrawCommand]) -> impl Iterator<Item = (EntityId, &[(MinimapPos, [u8; 3])])> {
    commands.iter().filter_map(|command| match command {
        DrawCommand::PositionTrail { entity_id, points, .. } => Some((*entity_id, points.as_slice())),
        _ => None,
    })
}

fn map_commands() -> Vec<DrawCommand> {
    let entity_id = EntityId::from(1u32);
    let color = [180, 140, 80];
    vec![
        DrawCommand::ScoreBar {
            team0: 600,
            team1: 400,
            team0_color: [0, 255, 0],
            team1_color: [255, 0, 0],
            max_score: 1000,
            team0_timer: Some("3:15".into()),
            team1_timer: Some("5:32".into()),
            advantage: Some((AdvantageLevel::Weak, 0)),
        },
        DrawCommand::TeamAdvantage {
            level: Some(AdvantageLevel::Weak),
            color,
            breakdown: AdvantageBreakdown::default(),
        },
        DrawCommand::TeamBuffs { friendly_buffs: vec![("damage".into(), 2)], enemy_buffs: Vec::new() },
        DrawCommand::CapturePoint {
            pos: pos(570.0, 540.0),
            radius: 28,
            color,
            alpha: 0.25,
            label: "A".into(),
            progress: 0.0,
            invader_color: None,
            flag_icon: None,
        },
        DrawCommand::ShotTracer { from: pos(300.0, 470.0), to: pos(350.0, 480.0), color },
        DrawCommand::ShotTracerTip { at: pos(350.0, 480.0), color: [255, 100, 60] },
        DrawCommand::Torpedo { pos: pos(450.0, 430.0), color },
        DrawCommand::Smoke { pos: pos(580.0, 255.0), radius: 28, color, alpha: 0.5 },
        DrawCommand::CameraDirection {
            entity_id,
            pos: SHIP_POSITION,
            yaw: std::f32::consts::PI,
            color: [255, 0, 0],
            length: wows_minimap_renderer::ICON_SIZE as i32,
        },
        DrawCommand::DeadShip {
            entity_id: EntityId::from(2u32),
            pos: pos(450.0, 550.0),
            yaw: 0.0,
            species: None,
            color: None,
            is_self: false,
            player_name: Some("Lost".into()),
            ship_name: Some("Cruiser".into()),
        },
        ship(),
        DrawCommand::HealthBar {
            entity_id,
            pos: SHIP_POSITION,
            fraction: 0.5,
            fill_color: color,
            background_color: [0; 3],
            background_alpha: 0.7,
        },
        DrawCommand::SubmarineStatus {
            entity_id,
            pos: SHIP_POSITION,
            battery: BatteryFraction::from_charge(60.0, 100.0),
            depth: Meters::new(30.0),
            has_hp_bar: true,
        },
        DrawCommand::SubmarineSurveillanceRadius {
            entity_id,
            pos: SHIP_POSITION,
            radius_px: 59,
            color: [100, 85, 190],
            alpha: 0.3,
        },
        DrawCommand::ConsumableRadius {
            entity_id,
            pos: SHIP_POSITION,
            radius_px: 83,
            color: [50, 190, 100],
            alpha: 0.3,
        },
        DrawCommand::ConsumableIcons {
            entity_id,
            pos: SHIP_POSITION,
            icon_keys: vec!["PCY001".into()],
            is_friendly: true,
            has_hp_bar: true,
        },
        DrawCommand::Building {
            pos: pos(690.0, 270.0),
            color,
            is_alive: true,
            icon_type: None,
            relation: BuildingRelation::Neutral,
        },
        DrawCommand::Plane {
            plane_id: PlaneId::from(1u64),
            owner_entity_id: entity_id,
            pos: pos(100.0, 500.0),
            icon_key: "controllable/fighter_he_ally".into(),
            player_name: None,
            ship_name: None,
        },
        DrawCommand::KillFeed { entries: vec![kill()] },
        DrawCommand::ChatOverlay { entries: vec![chat()] },
        DrawCommand::Timer { time_remaining: Some(590), elapsed: ElapsedClock(10.0), remaining: None },
        DrawCommand::BattleResultOverlay {
            result: BattleResult::Victory,
            finish_type: None,
            color,
            subtitle_above: false,
        },
    ]
}

fn kill() -> KillFeedEntry {
    KillFeedEntry {
        killer_name: Some("Captain".into()),
        killer_species: None,
        killer_ship_name: None,
        killer_color: [20, 210, 110],
        killer_is_friendly: true,
        victim_name: "Lost".into(),
        victim_species: None,
        victim_ship_name: None,
        victim_color: [250, 100, 50],
        victim_is_friendly: false,
        cause: None,
    }
}

fn chat() -> ChatEntry {
    ChatEntry {
        clan_tag: String::new(),
        clan_color: None,
        player_name: "Captain".into(),
        team_color: [20, 210, 110],
        ship_species: None,
        ship_name: None,
        message: "Capture point A".into(),
        message_color: [255; 3],
        opacity: 1.0,
        font_hint: Default::default(),
    }
}
