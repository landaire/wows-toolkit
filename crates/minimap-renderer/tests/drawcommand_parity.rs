//! Captures DrawCommands from `MinimapRenderer::draw_frame` driven by
//! `BattleWorld + view()` at the replay midpoint, compares against an insta
//! snapshot to detect regressions in the renderer output.
//!
//! Run with INSTA_UPDATE=always to regenerate snapshots:
//!   cargo test -p wows_minimap_renderer --features vfs,rendering \
//!     --no-default-features -- drawcommand_parity
#![cfg(feature = "vfs")]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::OnceLock;

use wows_replays::ReplayFile;
use wows_replays::analyzer::Analyzer;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::Version;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::rpc::entitydefs::EntitySpec;
use wowsunpack::vfs::VfsPath;
use wowsunpack::vfs::impls::physical::PhysicalFS;
use wows_replays::game_constants::GameConstants;

use wows_minimap_renderer::MapInfo;
use wows_minimap_renderer::RenderOptions;
use wows_minimap_renderer::DrawCommand;
use wows_minimap_renderer::renderer::MinimapRenderer;
use wows_minimap_renderer::map_data::MinimapPos;

// ---------------------------------------------------------------------------
// Fixture loading
// ---------------------------------------------------------------------------

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests")
        .join("fixtures")
        .join("replays")
}

#[derive(Clone, Copy)]
struct BuildResources {
    provider: &'static GameMetadataProvider,
    constants: &'static GameConstants,
}

fn build_cache() -> &'static Mutex<HashMap<u32, BuildResources>> {
    static CACHE: OnceLock<Mutex<HashMap<u32, BuildResources>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn resources_for_build(version: &Version) -> BuildResources {
    if let Some(res) = build_cache().lock().unwrap().get(&version.build) {
        return *res;
    }

    let dir = wows_data_mgr::game_dir_for_build(version.build)
        .unwrap_or_else(|| panic!("game data for build {} not available", version.build));
    let vfs_root = dir.join("vfs");
    assert!(vfs_root.exists(), "vfs dir not found at {}", vfs_root.display());
    let vfs = VfsPath::new(PhysicalFS::new(&vfs_root));

    let rkyv_path = dir.join("game_params.rkyv");
    let provider = match wowsunpack::game_params::cache::load(&rkyv_path) {
        Some(params) => GameMetadataProvider::from_params_with_vfs(params, &vfs)
            .unwrap_or_else(|e| panic!("failed to build game metadata for build {}: {e:?}", version.build)),
        None => GameMetadataProvider::from_vfs(&vfs)
            .unwrap_or_else(|e| panic!("failed to load GameParams for build {}: {e:?}", version.build)),
    };
    let constants = GameConstants::from_vfs(&vfs);

    let res = BuildResources {
        provider: Box::leak(Box::new(provider)),
        constants: Box::leak(Box::new(constants)),
    };
    build_cache().lock().unwrap().insert(version.build, res);
    res
}

// ---------------------------------------------------------------------------
// DrawCommand normalization
// ---------------------------------------------------------------------------

fn round2(v: f32) -> f32 {
    (v * 100.0).round() / 100.0
}

fn fmt_pos(p: MinimapPos) -> String {
    format!("({:.2},{:.2})", p.x, p.y)
}

/// Convert a DrawCommand to a stable string for snapshot comparison.
///
/// Asset fields (images) are omitted; floats are rounded to 2 decimal places.
fn normalize(cmd: &DrawCommand) -> String {
    match cmd {
        DrawCommand::ShotTracer { from, to, color } => {
            format!("ShotTracer from={} to={} color={color:?}", fmt_pos(*from), fmt_pos(*to))
        }
        DrawCommand::Torpedo { pos, color } => {
            format!("Torpedo pos={} color={color:?}", fmt_pos(*pos))
        }
        DrawCommand::Smoke { pos, radius, color, alpha } => {
            format!("Smoke pos={} r={radius} color={color:?} alpha={:.2}", fmt_pos(*pos), alpha)
        }
        DrawCommand::Ship {
            entity_id, pos, yaw, species, color, visibility, opacity, is_self,
            player_name, ship_name, is_detected_teammate, is_disconnected, name_color,
        } => {
            format!(
                "Ship eid={entity_id:?} pos={} yaw={:.3} sp={species:?} color={color:?} vis={visibility:?} op={:.2} self={is_self} dn={is_detected_teammate} dc={is_disconnected} nc={name_color:?} pn={player_name:?} sn={ship_name:?}",
                fmt_pos(*pos), round2(*yaw), round2(*opacity)
            )
        }
        DrawCommand::HealthBar { entity_id, pos, fraction, fill_color, background_color, background_alpha } => {
            format!(
                "HealthBar eid={entity_id:?} pos={} frac={:.3} fc={fill_color:?} bg={background_color:?} bga={:.2}",
                fmt_pos(*pos), round2(*fraction), round2(*background_alpha)
            )
        }
        DrawCommand::DeadShip { entity_id, pos, yaw, species, color, is_self, player_name, ship_name } => {
            format!(
                "DeadShip eid={entity_id:?} pos={} yaw={:.3} sp={species:?} color={color:?} self={is_self} pn={player_name:?} sn={ship_name:?}",
                fmt_pos(*pos), round2(*yaw)
            )
        }
        DrawCommand::BuffZone { pos, radius, color, alpha, marker_name } => {
            format!(
                "BuffZone pos={} r={radius} color={color:?} alpha={:.2} mn={marker_name:?}",
                fmt_pos(*pos), round2(*alpha)
            )
        }
        DrawCommand::CapturePoint { pos, radius, color, alpha, label, progress, invader_color, .. } => {
            format!(
                "CapturePoint pos={} r={radius} color={color:?} alpha={:.2} label={label:?} prog={:.3} ic={invader_color:?}",
                fmt_pos(*pos), round2(*alpha), round2(*progress)
            )
        }
        DrawCommand::TurretDirection { entity_id, pos, yaw, color, length } => {
            format!(
                "TurretDirection eid={entity_id:?} pos={} yaw={:.3} color={color:?} len={length}",
                fmt_pos(*pos), round2(*yaw)
            )
        }
        DrawCommand::Building { pos, color, is_alive, icon_type, relation } => {
            format!(
                "Building pos={} color={color:?} alive={is_alive} icon={icon_type:?} rel={relation:?}",
                fmt_pos(*pos)
            )
        }
        DrawCommand::WeatherZone { pos, radius } => {
            format!("WeatherZone pos={} r={radius}", fmt_pos(*pos))
        }
        DrawCommand::Plane { plane_id, owner_entity_id, pos, icon_key, player_name, ship_name } => {
            format!(
                "Plane pid={plane_id:?} eid={owner_entity_id:?} pos={} icon={icon_key:?} pn={player_name:?} sn={ship_name:?}",
                fmt_pos(*pos)
            )
        }
        DrawCommand::ConsumableRadius { entity_id, pos, radius_px, color, alpha } => {
            format!(
                "ConsumableRadius eid={entity_id:?} pos={} r={radius_px} color={color:?} alpha={:.2}",
                fmt_pos(*pos), round2(*alpha)
            )
        }
        DrawCommand::PatrolRadius { plane_id, pos, radius_px, color, alpha } => {
            format!(
                "PatrolRadius pid={plane_id:?} pos={} r={radius_px} color={color:?} alpha={:.2}",
                fmt_pos(*pos), round2(*alpha)
            )
        }
        DrawCommand::ConsumableIcons { entity_id, pos, icon_keys, is_friendly, has_hp_bar } => {
            format!(
                "ConsumableIcons eid={entity_id:?} pos={} keys={icon_keys:?} friendly={is_friendly} hp={has_hp_bar}",
                fmt_pos(*pos)
            )
        }
        DrawCommand::ShipConfigCircle {
            entity_id, pos, radius_px, color, alpha, dashed, label, kind, player_name, is_self,
        } => {
            format!(
                "ShipConfigCircle eid={entity_id:?} pos={} r={:.2} color={color:?} alpha={:.2} dashed={dashed} label={label:?} kind={kind:?} pn={player_name:?} self={is_self}",
                fmt_pos(*pos), round2(*radius_px), round2(*alpha)
            )
        }
        DrawCommand::PositionTrail { entity_id, player_name, points } => {
            let pts: Vec<String> = points.iter().map(|(p, c)| format!("({:.2},{:.2},{c:?})", p.x, p.y)).collect();
            format!("PositionTrail eid={entity_id:?} pn={player_name:?} pts=[{}]", pts.join(","))
        }
        DrawCommand::TeamBuffs { friendly_buffs, enemy_buffs } => {
            format!("TeamBuffs friendly={friendly_buffs:?} enemy={enemy_buffs:?}")
        }
        DrawCommand::ScoreBar {
            team0, team1, team0_color, team1_color, max_score, team0_timer, team1_timer, advantage,
        } => {
            format!(
                "ScoreBar t0={team0} t1={team1} c0={team0_color:?} c1={team1_color:?} max={max_score} t0t={team0_timer:?} t1t={team1_timer:?} adv={advantage:?}"
            )
        }
        DrawCommand::TeamAdvantage { level, color, breakdown: _ } => {
            format!("TeamAdvantage level={level:?} color={color:?}")
        }
        DrawCommand::Timer { time_remaining, elapsed } => {
            format!("Timer tr={time_remaining:?} el={elapsed:?}")
        }
        DrawCommand::PreBattleCountdown { seconds } => {
            format!("PreBattleCountdown s={seconds}")
        }
        DrawCommand::KillFeed { entries } => {
            let names: Vec<_> = entries.iter().map(|e| (&e.killer_name, &e.victim_name, &e.cause)).collect();
            format!("KillFeed {names:?}")
        }
        DrawCommand::ChatOverlay { entries } => {
            let msgs: Vec<_> = entries.iter().map(|e| (&e.player_name, &e.message)).collect();
            format!("ChatOverlay {msgs:?}")
        }
        DrawCommand::BattleResultOverlay { result, finish_type, color, subtitle_above } => {
            format!(
                "BattleResultOverlay result={result:?} ft={finish_type:?} color={color:?} sa={subtitle_above}"
            )
        }
        DrawCommand::StatsPanel { x, width } => {
            format!("StatsPanel x={x} w={width}")
        }
        DrawCommand::StatsSilhouette {
            x, y, width, height, ship_param_id, hp_fraction, hp_current, hp_max,
            player_name, clan_tag, clan_color, ship_name, ..
        } => {
            format!(
                "StatsSilhouette x={x} y={y} w={width} h={height} sid={ship_param_id:?} hpf={:.3} hpc={:.0} hpm={:.0} pn={player_name:?} ct={clan_tag:?} cc={clan_color:?} sn={ship_name:?}",
                round2(*hp_fraction), hp_current, hp_max
            )
        }
        DrawCommand::StatsDamage {
            x, y, width, breakdowns, damage_spotting, spotting_breakdowns,
            damage_potential, potential_breakdowns,
        } => {
            let bd: Vec<_> = breakdowns.iter().map(|b| (&b.label, b.damage as i64)).collect();
            let sbd: Vec<_> = spotting_breakdowns.iter().map(|b| (&b.label, b.damage as i64)).collect();
            let pbd: Vec<_> = potential_breakdowns.iter().map(|b| (&b.label, b.damage as i64)).collect();
            format!(
                "StatsDamage x={x} y={y} w={width} dmg={:.0} spot={:.0} pot={:.0} bd={bd:?} sbd={sbd:?} pbd={pbd:?}",
                damage_spotting, damage_spotting, damage_potential
            )
        }
        DrawCommand::StatsRibbons { x, y, width, ribbons } => {
            let rs: Vec<_> = ribbons.iter().map(|r| (&r.ribbon, r.count)).collect();
            format!("StatsRibbons x={x} y={y} w={width} {rs:?}")
        }
        DrawCommand::StatsActivityFeed { x, y, width, height, entries } => {
            format!("StatsActivityFeed x={x} y={y} w={width} h={height} n={}", entries.len())
        }
        DrawCommand::TeamRoster { side, x, y, width, height, rows } => {
            let row_ids: Vec<_> = rows.iter().map(|r| r.entity_id).collect();
            format!(
                "TeamRoster side={side:?} x={x} y={y} w={width} h={height} eids={row_ids:?}"
            )
        }
    }
}

// ---------------------------------------------------------------------------
// Test driver
// ---------------------------------------------------------------------------

/// Drive BattleWorld over the replay, calling draw_frame at the middle packet,
/// then again at the last packet. Returns the normalized+sorted DrawCommands
/// from each of those two frames joined with a separator.
fn capture_frames(filename: &str) -> String {
    let path = fixtures_dir().join(filename);

    // Count total packets (first pass)
    let replay_count = ReplayFile::from_file(&path)
        .unwrap_or_else(|e| panic!("failed to parse {filename}: {e:?}"));
    let version = Version::from_client_exe(&replay_count.meta.clientVersionFromExe);
    let res = resources_for_build(&version);
    let specs: &'static [EntitySpec] = res.provider.entity_specs();

    let total_packets = {
        let mut parser = wows_replays::packet2::Parser::with_version(specs, version);
        let mut remaining = &replay_count.packet_data[..];
        let mut n = 0usize;
        while !remaining.is_empty() {
            match parser.parse_packet(&mut remaining) {
                Ok(_) => n += 1,
                Err(_) => break,
            }
        }
        n
    };
    let mid = total_packets / 2;

    // Seed inventory facts (second pass through file)
    let replay_facts = ReplayFile::from_file(&path)
        .unwrap_or_else(|e| panic!("failed to parse {filename}: {e:?}"));
    let facts = wows_replays::analyzer::battle_controller::merged::gather_replay_facts(
        res.constants,
        version,
        specs,
        &[&replay_facts],
    );

    // Main pass: drive BattleWorld and collect frames
    let replay_main = ReplayFile::from_file(&path)
        .unwrap_or_else(|e| panic!("failed to parse {filename}: {e:?}"));
    let meta: &'static wows_replays::ReplayMeta = Box::leak(Box::new(replay_main.meta));

    let mut world = wows_battle_world::BattleWorld::new(meta, res.provider, Some(res.constants));
    for (entity_id, fact) in &facts {
        let inv = wows_replay_insights::build::build_inventory_from_facts(fact, res.provider, version);
        if !inv.is_empty() {
            world.set_consumable_inventory(*entity_id, inv);
        }
    }

    let map_info = Some(MapInfo { space_size: 30000 });
    let options = RenderOptions::default();
    let mut renderer = MinimapRenderer::new(map_info, res.provider, version, options);

    let mut parser = wows_replays::packet2::Parser::with_version(specs, version);
    let mut remaining = &replay_main.packet_data[..];
    let mut count = 0usize;
    let mut mid_frame: Option<Vec<DrawCommand>> = None;

    while !remaining.is_empty() {
        let packet = match parser.parse_packet(&mut remaining) {
            Ok(p) => p,
            Err(_) => break,
        };
        world.process(&packet);
        count += 1;
        if count == mid {
            renderer.populate_players(&world.view());
            renderer.update_squadron_info(&world.view());
            renderer.update_ship_abilities(&world.view());
            mid_frame = Some(renderer.draw_frame(&world.view()));
        }
    }

    // Final frame
    renderer.populate_players(&world.view());
    renderer.update_squadron_info(&world.view());
    renderer.update_ship_abilities(&world.view());
    let final_frame = renderer.draw_frame(&world.view());

    let mid_cmds = mid_frame.unwrap_or_default();

    let fmt = |cmds: &[DrawCommand]| -> String {
        let mut lines: Vec<String> = cmds.iter().map(normalize).collect();
        lines.sort();
        lines.join("\n")
    };

    format!("=== mid ({count} pkts, mid={mid}) ===\n{}\n\n=== final ===\n{}", fmt(&mid_cmds), fmt(&final_frame))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

// v15.1, pvp, Domination (Vermont BB)
#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn drawcommand_parity_vermont_pvp() {
    insta::assert_snapshot!(capture_frames("20260213_143518_PASB110-Vermont_22_tierra_del_fuego.wowsreplay"));
}

// v13.3, pvp (V-170 DD)
#[test]
#[cfg_attr(not(all(has_game_data, has_build_8260685)), ignore)]
fn drawcommand_parity_v170_pvp() {
    insta::assert_snapshot!(capture_frames("20240422_161541_PGSD104-V-170_08_NE_passage.wowsreplay"));
}

// v0.11.9, pvp, ArmsRace (Cossack) -- buff_zones, captured_buffs
#[test]
#[cfg_attr(not(all(has_game_data, has_build_6359964)), ignore)]
fn drawcommand_parity_cossack_armsrace() {
    insta::assert_snapshot!(capture_frames("20221101_004346_PBSD517-Cossack_37_Ridge.wowsreplay"));
}
