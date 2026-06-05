#![cfg(feature = "vfs")]
#[path = "support/mod.rs"]
mod support;

fn run_ingest(filename: &str) {
    let h = support::load(filename);
    let mut world = wows_battle_world::BattleWorld::new(
        &h.replay.meta,
        h.game_params,
        Some(h.game_constants),
    );
    let mut parser = wows_replays::packet2::Parser::with_version(h.specs, h.version);
    let mut remaining = &h.replay.packet_data[..];
    use wows_replays::analyzer::Analyzer;
    while !remaining.is_empty() {
        let packet = parser.parse_packet(&mut remaining).expect("parse");
        world.process(&packet);
    }
    world.finish();
}

// v0.8.2, pvp, Domination
#[test]
#[cfg_attr(not(all(has_game_data, has_build_1427460)), ignore)]
fn v0_8_2_pvp_montana() {
    run_ingest("20190420_125057_PASB017-Montana-1945_15_NE_north.wowsreplay");
}

// v0.8.5, ranked, Ranked_Epicenter
#[test]
#[cfg_attr(not(all(has_game_data, has_build_1631917)), ignore)]
fn v0_8_5_ranked_new_orleans() {
    run_ingest(
        "20190721_165022_PASC107-New-Orlean-1944_r01_military_navigation.wowsreplay",
    );
}

// v0.8.5, pve, Attack_On_Base_Normal (operation)
#[test]
#[cfg_attr(not(all(has_game_data, has_build_1631917)), ignore)]
fn v0_8_5_pve_navalbase_operation() {
    run_ingest("20190713_191438_PGSB106-Bayern_s01_NavalBase.wowsreplay");
}

// v0.9.0, pve, OP_02_03_s07_Advance (operation)
#[test]
#[cfg_attr(not(all(has_game_data, has_build_2171354)), ignore)]
fn v0_9_0_pve_operation_advance() {
    run_ingest("20200130_131002_PASC006-Atlanta-1942_s07_Advance.wowsreplay");
}

// v0.9.0, pvp, Domination
#[test]
#[cfg_attr(not(all(has_game_data, has_build_2171354)), ignore)]
fn v0_9_0_pvp_shimakaze() {
    run_ingest("20200117_205708_PJSD012-Shimakaze-1943_45_Zigzag.wowsreplay");
}

// v0.10.0, pvp, Domination_3point
#[test]
#[cfg_attr(not(all(has_game_data, has_build_3343484)), ignore)]
fn v0_10_0_pvp_jean_bart() {
    run_ingest("20210202_105419_PFSB518-Jean-Bart_44_Path_warrior.wowsreplay");
}

// v0.10.5, clan, CvC_Domination
#[test]
#[cfg_attr(not(all(has_game_data, has_build_4046169)), ignore)]
fn v0_10_5_clan_shimakaze() {
    run_ingest("20210621_014820_PJSD912-Shimakaze-1943_18_NE_ice_islands.wowsreplay");
}

// v0.11.0, ranked, Ranked_Domination
#[test]
#[cfg_attr(not(all(has_game_data, has_build_5045210)), ignore)]
fn v0_11_0_ranked_grossdeutschland() {
    run_ingest("20220210_003215_PGSB110-Grossdeutschland_15_NE_north.wowsreplay");
}

// v0.11.0, brawl, Domination_Special
#[test]
#[cfg_attr(not(all(has_game_data, has_build_5045210)), ignore)]
fn v0_11_0_brawl_conte_di_cavour() {
    run_ingest("20220124_194638_PISB105-Conte-di-Cavour_22_tierra_del_fuego.wowsreplay");
}

// v0.11.9, pvp, ArmsRace
#[test]
#[cfg_attr(not(all(has_game_data, has_build_6359964)), ignore)]
fn v0_11_9_pvp_armsrace_cossack() {
    run_ingest("20221101_004346_PBSD517-Cossack_37_Ridge.wowsreplay");
}

// v12.6.0, pve, PCVO009_OP_02_02 (operation: Raid on the Shipyard)
#[test]
#[cfg_attr(not(all(has_game_data, has_build_7266701)), ignore)]
fn v12_6_0_pve_operation_atoll() {
    run_ingest("20230813_200638_PJSC717-Yellow-Dragon_s06_Atoll.wowsreplay");
}

// v13.3.0, pvp, domination_3point
#[test]
#[cfg_attr(not(all(has_game_data, has_build_8260685)), ignore)]
fn v13_3_0_pvp_v170() {
    run_ingest("20240422_161541_PGSD104-V-170_08_NE_passage.wowsreplay");
}
