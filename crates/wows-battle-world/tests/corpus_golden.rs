//! Golden snapshots of the new BattleWorld, driven standalone over each corpus
//! fixture. These replace the old differential parity tests: the committed
//! `.snap` files are the regression backstop for BattleWorld behavior.
//!
//! Two snapshots per fixture:
//!   - `report`: a normalized view of `into_report()` (consumes the world).
//!   - `digest`: a normalized final-state digest from a second world run.
//!
//! Everything snapshotted is normalized for determinism: ids are rendered as
//! stable scalars, collections are sorted by id, and floats are rounded to a
//! fixed precision so platform rounding noise never flips a snapshot.
#![cfg(feature = "vfs")]

#[path = "support/mod.rs"]
mod support;

use serde::Serialize;
use wows_battle_world::BattleWorld;
use wows_replays::ReplayFile;
use wows_replays::analyzer::Analyzer;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::Version;

fn r3(v: f32) -> f32 {
    (v * 1000.0).round() / 1000.0
}

fn r3o(v: Option<f32>) -> Option<f32> {
    v.map(r3)
}

#[derive(Serialize)]
struct PosEntry {
    id: u32,
    x: f32,
    y: f32,
    z: f32,
    yaw: f32,
    pitch: f32,
    roll: f32,
}

#[derive(Serialize)]
struct MinimapEntry {
    id: u32,
    x: f32,
    y: f32,
    heading: f32,
    visible: bool,
    visibility_flags: u32,
    is_invisible: bool,
}

#[derive(Serialize)]
struct VehicleEntry {
    id: u32,
    health: f32,
    max_health: f32,
    is_alive: bool,
    is_invisible: bool,
    visibility_flags: u32,
    team_id: i8,
    owner: u32,
    selected_weapon: String,
    ship_params_id: String,
}

#[derive(Serialize)]
struct EntityKindEntry {
    id: u32,
    kind: String,
}

#[derive(Serialize)]
struct KillEntry {
    clock: f32,
    killer: u32,
    victim: u32,
    cause: String,
}

#[derive(Serialize)]
struct ChatEntry {
    clock: f32,
    sender_name: String,
    channel: String,
    message: String,
    entity_id: u32,
    sender_relation: String,
    player: Option<String>,
}

#[derive(Serialize)]
struct CapturePointEntry {
    index: usize,
    position: Option<(f32, f32, f32)>,
    radius: f32,
    control_point_type: String,
    team_id: i64,
    invader_team: i64,
    progress: (f64, f64),
    has_invaders: bool,
    both_inside: bool,
    is_enabled: bool,
}

#[derive(Serialize)]
struct TeamScoreEntry {
    team_index: usize,
    score: i64,
}

#[derive(Serialize)]
struct ConsumableCount {
    id: u32,
    active: usize,
    inventory_slots: usize,
}

#[derive(Serialize)]
struct PlaneEntry {
    plane_id: String,
    owner_id: u32,
    team_id: u32,
    params_id: String,
}

#[derive(Serialize)]
struct WardEntry {
    plane_id: String,
    owner_id: u32,
    position: (f32, f32, f32),
    radius: String,
}

#[derive(Serialize)]
struct Digest {
    positions: Vec<PosEntry>,
    minimap: Vec<MinimapEntry>,
    vehicles: Vec<VehicleEntry>,
    entity_kinds: Vec<EntityKindEntry>,
    kills: Vec<KillEntry>,
    chat: Vec<ChatEntry>,
    capture_points: Vec<CapturePointEntry>,
    team_scores: Vec<TeamScoreEntry>,
    consumables: Vec<ConsumableCount>,
    planes: Vec<PlaneEntry>,
    wards: Vec<WardEntry>,
    active_shots: usize,
    active_torpedoes: usize,
    battle_stage: String,
    time_left: Option<i64>,
    winning_team: Option<i8>,
    finish_type: Option<String>,
    battle_start_clock: Option<f32>,
    battle_end_clock: Option<f32>,
}

#[derive(Serialize)]
struct PlayerEntry {
    account_id: i64,
    name: String,
    entity_id: u32,
    team_id: i64,
    meta_ship_id: i64,
    relation: String,
    is_bot: bool,
    vehicle_id: String,
    division: Option<char>,
}

#[derive(Serialize)]
struct FragEntry {
    killer_account_id: i64,
    killer_name: String,
    victims: Vec<String>,
}

#[derive(Serialize)]
struct DamageStatEntry {
    weapon: String,
    category: String,
    count: i64,
    total: f32,
}

#[derive(Serialize)]
struct ReportSnapshot {
    arena_id: String,
    version: String,
    map_name: String,
    game_mode: String,
    game_type: String,
    match_group: String,
    self_player: i64,
    battle_result: Option<String>,
    finish_type: Option<String>,
    max_duration: u32,
    played_duration: Option<f32>,
    extra_duration: Option<f32>,
    players: Vec<PlayerEntry>,
    frags: Vec<FragEntry>,
    chat: Vec<ChatEntry>,
    capture_points: Vec<CapturePointEntry>,
    team_scores: Vec<TeamScoreEntry>,
    self_damage_stats: Vec<DamageStatEntry>,
    active_consumable_avatars: usize,
    active_consumable_total: usize,
}

fn make_world(
    filename: &str,
) -> (ReplayFile, BattleWorld<'static, 'static, wowsunpack::game_params::provider::GameMetadataProvider>) {
    let h = support::load(filename);
    let replay = h.replay;
    let meta: &'static wows_replays::ReplayMeta = Box::leak(Box::new(replay.meta.clone()));
    let world = BattleWorld::new(meta, h.game_params, Some(h.game_constants));
    (replay, world)
}

fn drive<G: ResourceLoader>(
    replay: &ReplayFile,
    world: &mut BattleWorld<'_, '_, G>,
    specs: &[wowsunpack::rpc::entitydefs::EntitySpec],
    version: Version,
) {
    let mut parser = wows_replays::packet2::Parser::with_version(specs, version);
    let mut remaining = &replay.packet_data[..];
    while !remaining.is_empty() {
        let packet = parser.parse_packet(&mut remaining).expect("packet parse");
        world.process(&packet);
    }
    world.finish();
}

/// Pre-0.9 replays carry no roster RPC, so no player is tagged Self and
/// `into_report` panics by design. Snapshot `None` for those fixtures.
fn report_snapshot(filename: &str) -> Option<ReportSnapshot> {
    let h = support::load(filename);
    let specs = h.specs;
    let version = h.version;
    let (replay, mut world) = make_world(filename);
    drive(&replay, &mut world, specs, version);

    let has_self = world.player_entities().values().any(|p| p.relation().is_self());
    if !has_self {
        return None;
    }
    let report = world.into_report();

    let mut players: Vec<PlayerEntry> = report
        .players()
        .iter()
        .map(|p| {
            let st = p.initial_state();
            PlayerEntry {
                account_id: st.db_id().raw(),
                name: st.username().to_string(),
                entity_id: st.entity_id().raw(),
                team_id: st.team_id(),
                meta_ship_id: st.meta_ship_id().raw(),
                relation: format!("{:?}", p.relation()),
                is_bot: p.is_bot(),
                vehicle_id: format!("{:?}", p.vehicle().id()),
                division: report.divisions().get(&st.entity_id()).copied(),
            }
        })
        .collect();
    players.sort_by(|a, b| (a.account_id, a.entity_id).cmp(&(b.account_id, b.entity_id)));

    let mut frags: Vec<FragEntry> = report
        .frags()
        .iter()
        .map(|(killer, deaths)| {
            let st = killer.initial_state();
            let mut victims: Vec<String> =
                deaths.iter().map(|d| format!("{}@{:?}", d.killer().raw(), d.cause())).collect();
            victims.sort();
            FragEntry { killer_account_id: st.db_id().raw(), killer_name: st.username().to_string(), victims }
        })
        .collect();
    frags.sort_by(|a, b| {
        (a.killer_account_id, a.killer_name.clone()).cmp(&(b.killer_account_id, b.killer_name.clone()))
    });

    let mut self_damage_stats: Vec<DamageStatEntry> = report
        .self_damage_stats()
        .iter()
        .map(|e| DamageStatEntry {
            weapon: format!("{:?}", e.weapon),
            category: format!("{:?}", e.category),
            count: e.count,
            total: r3(e.total as f32),
        })
        .collect();
    self_damage_stats
        .sort_by(|a, b| (a.weapon.clone(), a.category.clone()).cmp(&(b.weapon.clone(), b.category.clone())));

    let active_consumable_total: usize = report.active_consumables().values().map(|v| v.len()).sum();

    Some(ReportSnapshot {
        arena_id: format!("{:?}", report.arena_id()),
        version: format!("{:?}", report.version()),
        map_name: report.map_name().to_string(),
        game_mode: report.game_mode().to_string(),
        game_type: format!("{:?}", report.game_type()),
        match_group: report.match_group().to_string(),
        self_player: report.self_player().initial_state().db_id().raw(),
        battle_result: report.battle_result().map(|r| serde_json::to_string(r).unwrap_or_default()),
        finish_type: report.finish_type().map(|f| format!("{f:?}")),
        max_duration: report.max_duration(),
        played_duration: r3o(report.played_duration()),
        extra_duration: r3o(report.extra_duration()),
        players,
        frags,
        chat: chat_entries(report.game_chat()),
        capture_points: capture_point_entries(report.capture_points()),
        team_scores: team_score_entries(report.team_scores()),
        self_damage_stats,
        active_consumable_avatars: report.active_consumables().len(),
        active_consumable_total,
    })
}

fn chat_entries(chat: &[wows_replays::analyzer::battle_controller::GameMessage]) -> Vec<ChatEntry> {
    chat.iter()
        .map(|m| ChatEntry {
            clock: r3(m.clock.0),
            sender_name: m.sender_name.clone(),
            channel: format!("{:?}", m.channel),
            message: m.message.clone(),
            entity_id: m.entity_id.raw(),
            sender_relation: format!("{:?}", m.sender_relation),
            player: m
                .player
                .as_ref()
                .map(|p| format!("{}:{}", p.initial_state().db_id().raw(), p.initial_state().username())),
        })
        .collect()
}

fn capture_point_entries(
    points: &[wows_replays::analyzer::battle_controller::state::CapturePointState],
) -> Vec<CapturePointEntry> {
    points
        .iter()
        .map(|c| CapturePointEntry {
            index: c.index,
            position: c.position.map(|p| (r3(p.x), r3(p.y), r3(p.z))),
            radius: r3(c.radius),
            control_point_type: format!("{:?}", c.control_point_type),
            team_id: c.team_id,
            invader_team: c.invader_team,
            progress: ((c.progress.0 * 1000.0).round() / 1000.0, (c.progress.1 * 1000.0).round() / 1000.0),
            has_invaders: c.has_invaders,
            both_inside: c.both_inside,
            is_enabled: c.is_enabled,
        })
        .collect()
}

fn team_score_entries(scores: &[wows_replays::analyzer::battle_controller::state::TeamScore]) -> Vec<TeamScoreEntry> {
    scores.iter().map(|s| TeamScoreEntry { team_index: s.team_index, score: s.score }).collect()
}

fn digest_snapshot(filename: &str) -> Digest {
    let h = support::load(filename);
    let specs = h.specs;
    let version = h.version;
    let (replay, mut world) = make_world(filename);
    drive(&replay, &mut world, specs, version);

    let mut positions: Vec<PosEntry> = world
        .positions()
        .into_iter()
        .map(|(id, t)| PosEntry {
            id: id.raw(),
            x: r3(t.pos.x),
            y: r3(t.pos.y),
            z: r3(t.pos.z),
            yaw: r3(t.yaw.0),
            pitch: r3(t.pitch.0),
            roll: r3(t.roll.0),
        })
        .collect();
    positions.sort_by_key(|e| e.id);

    let mut minimap: Vec<MinimapEntry> = world
        .minimap()
        .into_iter()
        .map(|(id, m)| MinimapEntry {
            id: id.raw(),
            x: r3(m.pos.x),
            y: r3(m.pos.y),
            heading: r3(m.heading.0),
            visible: m.visible,
            visibility_flags: m.visibility_flags.0,
            is_invisible: m.is_invisible,
        })
        .collect();
    minimap.sort_by_key(|e| e.id);

    let mut vehicles: Vec<VehicleEntry> = world
        .vehicle_props_all()
        .into_iter()
        .map(|(id, vp)| VehicleEntry {
            id: id.raw(),
            health: r3(vp.health()),
            max_health: r3(vp.max_health()),
            is_alive: vp.is_alive(),
            is_invisible: vp.is_invisible(),
            visibility_flags: vp.visibility_flags(),
            team_id: vp.team_id(),
            owner: vp.owner(),
            selected_weapon: format!("{:?}", vp.selected_weapon()),
            ship_params_id: format!("{:?}", vp.ship_config().ship_params_id()),
        })
        .collect();
    vehicles.sort_by_key(|e| e.id);

    let mut entity_kinds: Vec<EntityKindEntry> = world
        .entity_kinds()
        .into_iter()
        .map(|(id, k)| EntityKindEntry { id: id.raw(), kind: format!("{k:?}") })
        .collect();
    entity_kinds.sort_by_key(|e| e.id);

    let kills: Vec<KillEntry> = world
        .kills()
        .iter()
        .map(|k| KillEntry {
            clock: r3(k.clock.0),
            killer: k.killer.raw(),
            victim: k.victim.raw(),
            cause: format!("{:?}", k.cause),
        })
        .collect();

    let chat = chat_entries(world.chat());
    let capture_points = capture_point_entries(&world.capture_points());
    let team_scores = team_score_entries(&world.team_scores());

    let active = world.active_consumables();
    let inventories = world.consumable_inventories();
    let mut consumable_ids: Vec<u32> = active.keys().chain(inventories.keys()).map(|id| id.raw()).collect();
    consumable_ids.sort_unstable();
    consumable_ids.dedup();
    let consumables: Vec<ConsumableCount> = consumable_ids
        .into_iter()
        .map(|raw| {
            let id = wowsunpack::game_types::EntityId::from(raw);
            ConsumableCount {
                id: raw,
                active: active.get(&id).map(|v| v.len()).unwrap_or(0),
                inventory_slots: inventories.get(&id).map(|v| v.len()).unwrap_or(0),
            }
        })
        .collect();

    let mut planes: Vec<PlaneEntry> = world
        .active_planes()
        .into_values()
        .map(|p| PlaneEntry {
            plane_id: format!("{:?}", p.plane_id),
            owner_id: p.owner_id.raw(),
            team_id: p.team_id,
            params_id: format!("{:?}", p.params_id),
        })
        .collect();
    planes.sort_by(|a, b| a.plane_id.cmp(&b.plane_id));

    let mut wards: Vec<WardEntry> = world
        .active_wards()
        .into_values()
        .map(|w| WardEntry {
            plane_id: format!("{:?}", w.plane_id),
            owner_id: w.owner_id.raw(),
            position: (r3(w.position.x), r3(w.position.y), r3(w.position.z)),
            radius: format!("{:?}", w.radius),
        })
        .collect();
    wards.sort_by(|a, b| a.plane_id.cmp(&b.plane_id));

    let active_shots = world.active_shots().len();
    let active_torpedoes = world.active_torpedoes().len();

    Digest {
        positions,
        minimap,
        vehicles,
        entity_kinds,
        kills,
        chat,
        capture_points,
        team_scores,
        consumables,
        planes,
        wards,
        active_shots,
        active_torpedoes,
        battle_stage: format!("{:?}", world.battle_stage()),
        time_left: world.time_left(),
        winning_team: world.winning_team(),
        finish_type: world.finish_type().map(|f| format!("{f:?}")),
        battle_start_clock: r3o(world.battle_start_clock().map(|c| c.0)),
        battle_end_clock: r3o(world.battle_end_clock().map(|c| c.0)),
    }
}

fn run_golden(name: &str, filename: &str) {
    insta::with_settings!({ snapshot_suffix => format!("{name}_report") }, {
        insta::assert_yaml_snapshot!(report_snapshot(filename));
    });
    insta::with_settings!({ snapshot_suffix => format!("{name}_digest") }, {
        insta::assert_yaml_snapshot!(digest_snapshot(filename));
    });
}

// v0.8.2, pvp, Domination
#[test]
#[cfg_attr(not(all(has_game_data, has_build_1427460)), ignore)]
fn golden_v0_8_2_montana_pvp() {
    run_golden("v0_8_2_montana_pvp", "20190420_125057_PASB017-Montana-1945_15_NE_north.wowsreplay");
}

// v0.8.5, pve, operation
#[test]
#[cfg_attr(not(all(has_game_data, has_build_1631917)), ignore)]
fn golden_v0_8_5_bayern_pve_operation() {
    run_golden("v0_8_5_bayern_pve_operation", "20190713_191438_PGSB106-Bayern_s01_NavalBase.wowsreplay");
}

// v0.8.5, ranked
#[test]
#[cfg_attr(not(all(has_game_data, has_build_1631917)), ignore)]
fn golden_v0_8_5_new_orleans_ranked() {
    run_golden(
        "v0_8_5_new_orleans_ranked",
        "20190721_165022_PASC107-New-Orlean-1944_r01_military_navigation.wowsreplay",
    );
}

// v0.9.0, pve, operation
#[test]
#[cfg_attr(not(all(has_game_data, has_build_2171354)), ignore)]
fn golden_v0_9_0_atlanta_pve_advance() {
    run_golden("v0_9_0_atlanta_pve_advance", "20200130_131002_PASC006-Atlanta-1942_s07_Advance.wowsreplay");
}

// v0.9.0, pvp, Domination
#[test]
#[cfg_attr(not(all(has_game_data, has_build_2171354)), ignore)]
fn golden_v0_9_0_shimakaze_pvp() {
    run_golden("v0_9_0_shimakaze_pvp", "20200117_205708_PJSD012-Shimakaze-1943_45_Zigzag.wowsreplay");
}

// v0.10.0, pvp, Domination_3point
#[test]
#[cfg_attr(not(all(has_game_data, has_build_3343484)), ignore)]
fn golden_v0_10_0_jean_bart_pvp() {
    run_golden("v0_10_0_jean_bart_pvp", "20210202_105419_PFSB518-Jean-Bart_44_Path_warrior.wowsreplay");
}

// v0.10.5, clan, CvC_Domination
#[test]
#[cfg_attr(not(all(has_game_data, has_build_4046169)), ignore)]
fn golden_v0_10_5_shimakaze_clan() {
    run_golden("v0_10_5_shimakaze_clan", "20210621_014820_PJSD912-Shimakaze-1943_18_NE_ice_islands.wowsreplay");
}

// v0.11.0, brawl, Domination_Special
#[test]
#[cfg_attr(not(all(has_game_data, has_build_5045210)), ignore)]
fn golden_v0_11_0_conte_di_cavour_brawl() {
    run_golden(
        "v0_11_0_conte_di_cavour_brawl",
        "20220124_194638_PISB105-Conte-di-Cavour_22_tierra_del_fuego.wowsreplay",
    );
}

// v0.11.0, ranked, Ranked_Domination
#[test]
#[cfg_attr(not(all(has_game_data, has_build_5045210)), ignore)]
fn golden_v0_11_0_grossdeutschland_ranked() {
    run_golden("v0_11_0_grossdeutschland_ranked", "20220210_003215_PGSB110-Grossdeutschland_15_NE_north.wowsreplay");
}

// v0.11.9, pvp, ArmsRace
#[test]
#[cfg_attr(not(all(has_game_data, has_build_6359964)), ignore)]
fn golden_v0_11_9_cossack_armsrace_pvp() {
    run_golden("v0_11_9_cossack_armsrace_pvp", "20221101_004346_PBSD517-Cossack_37_Ridge.wowsreplay");
}

// v12.3, pvp, Domination (S-189 submarine)
#[test]
#[cfg_attr(not(all(has_game_data, has_build_6965290)), ignore)]
fn golden_v12_3_s189_submarine_pvp() {
    run_golden("v12_3_s189_submarine_pvp", "20230419_203306_PRSS508-S-189_42_Neighbors.wowsreplay");
}

// v12.6, pve, operation
#[test]
#[cfg_attr(not(all(has_game_data, has_build_7266701)), ignore)]
fn golden_v12_6_yellow_dragon_pve_operation() {
    run_golden("v12_6_yellow_dragon_pve_operation", "20230813_200638_PJSC717-Yellow-Dragon_s06_Atoll.wowsreplay");
}

// v13.2, pvp, Domination (Annapolis)
#[test]
#[cfg_attr(not(all(has_game_data, has_build_8151735)), ignore)]
fn golden_v13_2_annapolis_pvp() {
    run_golden("v13_2_annapolis_pvp", "20240402_192304_PASC111-Annapolis_22_tierra_del_fuego.wowsreplay");
}

// v13.3, pvp, Domination
#[test]
#[cfg_attr(not(all(has_game_data, has_build_8260685)), ignore)]
fn golden_v13_3_v170_pvp() {
    run_golden("v13_3_v170_pvp", "20240422_161541_PGSD104-V-170_08_NE_passage.wowsreplay");
}

// v13.10, pvp, Domination (Colbert)
#[test]
#[cfg_attr(not(all(has_game_data, has_build_9129736)), ignore)]
fn golden_v13_10_colbert_pvp() {
    run_golden("v13_10_colbert_pvp", "20241112_172819_PFSC510-Colbert_44_Path_warrior.wowsreplay");
}

// v14.1, pvp (Hull DD)
#[test]
#[cfg_attr(not(all(has_game_data, has_build_9531281)), ignore)]
fn golden_v14_1_hull_pvp() {
    run_golden("v14_1_hull_pvp", "20250206_020938_PASD720-Hull_47_Sleeping_Giant.wowsreplay");
}

// v14.2, pvp (Oland)
#[test]
#[cfg_attr(not(all(has_game_data, has_build_9643943)), ignore)]
fn golden_v14_2_oland_pvp() {
    run_golden("v14_2_oland_pvp", "20250117_004534_PWSD108-Oland_15_NE_north.wowsreplay");
}

// v14.9, pvp, naval mission (Ocean CV event)
#[test]
#[cfg_attr(not(all(has_game_data, has_build_10695045)), ignore)]
fn golden_v14_9_ocean_cv_naval_mission() {
    run_golden("v14_9_ocean_cv_naval_mission", "20251001_145225_PBSA710-Ocean_28_naval_mission.wowsreplay");
}

// v15.0, pvp (Forrest Sherman)
#[test]
#[cfg_attr(not(all(has_game_data, has_build_11791718)), ignore)]
fn golden_v15_0_forrest_sherman_pvp() {
    run_golden("v15_0_forrest_sherman_pvp", "20260127_185500_PASD610-Forrest-Sherman_56_AngelWings.wowsreplay");
}

// v15.1, pvp, Domination (Vermont)
#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn golden_v15_1_vermont_pvp() {
    run_golden("v15_1_vermont_pvp", "20260213_143518_PASB110-Vermont_22_tierra_del_fuego.wowsreplay");
}

// v15.1, pvp (Marceau)
#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn golden_v15_1_marceau_pvp() {
    run_golden("v15_1_marceau_pvp", "20260213_203056_PFSD210-Marceau_22_tierra_del_fuego.wowsreplay");
}

// v15.1, pve, operation (Narai)
#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn golden_v15_1_narai_pve_operation() {
    run_golden("v15_1_narai_pve_operation", "20260223_115252_PZSC718-Narai_s06_Atoll.wowsreplay");
}
