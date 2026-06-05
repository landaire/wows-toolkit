#![cfg(feature = "vfs")]

#[path = "support/mod.rs"]
mod support;

fn run_corpus(filename: &str) {
    let (old, mut new_world) = support::both(filename);
    support::assert_full_parity(&old, &mut new_world, filename);
}

fn run_shot_hits(filename: &str) {
    let (old_hits, new_hits) = support::both_stepped(filename);
    assert_eq!(
        old_hits.len(),
        new_hits.len(),
        "accumulated shot_hits count mismatch in {filename}: old={} new={}",
        old_hits.len(),
        new_hits.len()
    );
    for (i, (o, n)) in old_hits.iter().zip(new_hits.iter()).enumerate() {
        assert_eq!(o.clock, n.clock, "shot_hits[{i}] clock mismatch in {filename}");
        assert_eq!(o.hit, n.hit, "shot_hits[{i}] hit mismatch in {filename}");
        assert_eq!(
            o.victim_entity_id, n.victim_entity_id,
            "shot_hits[{i}] victim_entity_id mismatch in {filename}"
        );
        assert_eq!(o.salvo, n.salvo, "shot_hits[{i}] salvo mismatch in {filename}");
        assert_eq!(o.fired_at, n.fired_at, "shot_hits[{i}] fired_at mismatch in {filename}");
        assert_eq!(
            o.victim_position, n.victim_position,
            "shot_hits[{i}] victim_position mismatch in {filename}"
        );
        assert_eq!(o.victim_yaw, n.victim_yaw, "shot_hits[{i}] victim_yaw mismatch in {filename}");
        assert_eq!(o.victim_pitch, n.victim_pitch, "shot_hits[{i}] victim_pitch mismatch in {filename}");
        assert_eq!(o.victim_roll, n.victim_roll, "shot_hits[{i}] victim_roll mismatch in {filename}");
    }
    assert_eq!(old_hits, new_hits, "accumulated shot_hits mismatch in {filename}");
}

// v0.8.2, pvp, Domination
#[test]
#[cfg_attr(not(all(has_game_data, has_build_1427460)), ignore)]
fn corpus_v0_8_2_montana_pvp() {
    run_corpus("20190420_125057_PASB017-Montana-1945_15_NE_north.wowsreplay");
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_1427460)), ignore)]
fn corpus_v0_8_2_montana_pvp_shot_hits() {
    run_shot_hits("20190420_125057_PASB017-Montana-1945_15_NE_north.wowsreplay");
}

// v0.8.5, pve, operation
#[test]
#[cfg_attr(not(all(has_game_data, has_build_1631917)), ignore)]
fn corpus_v0_8_5_bayern_pve_operation() {
    run_corpus("20190713_191438_PGSB106-Bayern_s01_NavalBase.wowsreplay");
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_1631917)), ignore)]
fn corpus_v0_8_5_new_orleans_ranked() {
    run_corpus("20190721_165022_PASC107-New-Orlean-1944_r01_military_navigation.wowsreplay");
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_1631917)), ignore)]
fn corpus_v0_8_5_new_orleans_ranked_shot_hits() {
    run_shot_hits("20190721_165022_PASC107-New-Orlean-1944_r01_military_navigation.wowsreplay");
}

// v0.9.0, pve, operation
#[test]
#[cfg_attr(not(all(has_game_data, has_build_2171354)), ignore)]
fn corpus_v0_9_0_atlanta_pve_advance() {
    run_corpus("20200130_131002_PASC006-Atlanta-1942_s07_Advance.wowsreplay");
}

// v0.9.0, pvp, Domination
#[test]
#[cfg_attr(not(all(has_game_data, has_build_2171354)), ignore)]
fn corpus_v0_9_0_shimakaze_pvp() {
    run_corpus("20200117_205708_PJSD012-Shimakaze-1943_45_Zigzag.wowsreplay");
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_2171354)), ignore)]
fn corpus_v0_9_0_shimakaze_pvp_shot_hits() {
    run_shot_hits("20200117_205708_PJSD012-Shimakaze-1943_45_Zigzag.wowsreplay");
}

// v0.10.0, pvp, Domination_3point
#[test]
#[cfg_attr(not(all(has_game_data, has_build_3343484)), ignore)]
fn corpus_v0_10_0_jean_bart_pvp() {
    run_corpus("20210202_105419_PFSB518-Jean-Bart_44_Path_warrior.wowsreplay");
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_3343484)), ignore)]
fn corpus_v0_10_0_jean_bart_pvp_shot_hits() {
    run_shot_hits("20210202_105419_PFSB518-Jean-Bart_44_Path_warrior.wowsreplay");
}

// v0.10.5, clan, CvC_Domination
#[test]
#[cfg_attr(not(all(has_game_data, has_build_4046169)), ignore)]
fn corpus_v0_10_5_shimakaze_clan() {
    run_corpus("20210621_014820_PJSD912-Shimakaze-1943_18_NE_ice_islands.wowsreplay");
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_4046169)), ignore)]
fn corpus_v0_10_5_shimakaze_clan_shot_hits() {
    run_shot_hits("20210621_014820_PJSD912-Shimakaze-1943_18_NE_ice_islands.wowsreplay");
}

// v0.11.0, brawl, Domination_Special
#[test]
#[cfg_attr(not(all(has_game_data, has_build_5045210)), ignore)]
fn corpus_v0_11_0_conte_di_cavour_brawl() {
    run_corpus("20220124_194638_PISB105-Conte-di-Cavour_22_tierra_del_fuego.wowsreplay");
}

// v0.11.0, ranked, Ranked_Domination
#[test]
#[cfg_attr(not(all(has_game_data, has_build_5045210)), ignore)]
fn corpus_v0_11_0_grossdeutschland_ranked() {
    run_corpus("20220210_003215_PGSB110-Grossdeutschland_15_NE_north.wowsreplay");
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_5045210)), ignore)]
fn corpus_v0_11_0_grossdeutschland_ranked_shot_hits() {
    run_shot_hits("20220210_003215_PGSB110-Grossdeutschland_15_NE_north.wowsreplay");
}

// v0.11.9, pvp, ArmsRace
#[test]
#[cfg_attr(not(all(has_game_data, has_build_6359964)), ignore)]
fn corpus_v0_11_9_cossack_armsrace_pvp() {
    run_corpus("20221101_004346_PBSD517-Cossack_37_Ridge.wowsreplay");
}

// v12.3, pvp, Domination (S-189 submarine)
#[test]
#[cfg_attr(not(all(has_game_data, has_build_6965290)), ignore)]
fn corpus_v12_3_s189_submarine_pvp() {
    run_corpus("20230419_203306_PRSS508-S-189_42_Neighbors.wowsreplay");
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_6965290)), ignore)]
fn corpus_v12_3_s189_submarine_pvp_shot_hits() {
    run_shot_hits("20230419_203306_PRSS508-S-189_42_Neighbors.wowsreplay");
}

// v12.6, pve, operation
#[test]
#[cfg_attr(not(all(has_game_data, has_build_7266701)), ignore)]
fn corpus_v12_6_yellow_dragon_pve_operation() {
    run_corpus("20230813_200638_PJSC717-Yellow-Dragon_s06_Atoll.wowsreplay");
}

// v13.2, pvp, Domination (Annapolis)
#[test]
#[cfg_attr(not(all(has_game_data, has_build_8151735)), ignore)]
fn corpus_v13_2_annapolis_pvp() {
    run_corpus("20240402_192304_PASC111-Annapolis_22_tierra_del_fuego.wowsreplay");
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_8151735)), ignore)]
fn corpus_v13_2_annapolis_pvp_shot_hits() {
    run_shot_hits("20240402_192304_PASC111-Annapolis_22_tierra_del_fuego.wowsreplay");
}

// v13.3, pvp, Domination
#[test]
#[cfg_attr(not(all(has_game_data, has_build_8260685)), ignore)]
fn corpus_v13_3_v170_pvp() {
    run_corpus("20240422_161541_PGSD104-V-170_08_NE_passage.wowsreplay");
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_8260685)), ignore)]
fn corpus_v13_3_v170_pvp_shot_hits() {
    run_shot_hits("20240422_161541_PGSD104-V-170_08_NE_passage.wowsreplay");
}

// v13.10, pvp, Domination (Colbert)
#[test]
#[cfg_attr(not(all(has_game_data, has_build_9129736)), ignore)]
fn corpus_v13_10_colbert_pvp() {
    run_corpus("20241112_172819_PFSC510-Colbert_44_Path_warrior.wowsreplay");
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_9129736)), ignore)]
fn corpus_v13_10_colbert_pvp_shot_hits() {
    run_shot_hits("20241112_172819_PFSC510-Colbert_44_Path_warrior.wowsreplay");
}

// v14.1, pvp, (Hull DD)
#[test]
#[cfg_attr(not(all(has_game_data, has_build_9531281)), ignore)]
fn corpus_v14_1_hull_pvp() {
    run_corpus("20250206_020938_PASD720-Hull_47_Sleeping_Giant.wowsreplay");
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_9531281)), ignore)]
fn corpus_v14_1_hull_pvp_shot_hits() {
    run_shot_hits("20250206_020938_PASD720-Hull_47_Sleeping_Giant.wowsreplay");
}

// v14.2, pvp, (Oland)
#[test]
#[cfg_attr(not(all(has_game_data, has_build_9643943)), ignore)]
fn corpus_v14_2_oland_pvp() {
    run_corpus("20250117_004534_PWSD108-Oland_15_NE_north.wowsreplay");
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_9643943)), ignore)]
fn corpus_v14_2_oland_pvp_shot_hits() {
    run_shot_hits("20250117_004534_PWSD108-Oland_15_NE_north.wowsreplay");
}

// v14.9, pvp, naval mission (Ocean CV event)
#[test]
#[cfg_attr(not(all(has_game_data, has_build_10695045)), ignore)]
fn corpus_v14_9_ocean_cv_naval_mission() {
    run_corpus("20251001_145225_PBSA710-Ocean_28_naval_mission.wowsreplay");
}

// v15.0, pvp, (Forrest Sherman)
#[test]
#[cfg_attr(not(all(has_game_data, has_build_11791718)), ignore)]
fn corpus_v15_0_forrest_sherman_pvp() {
    run_corpus("20260127_185500_PASD610-Forrest-Sherman_56_AngelWings.wowsreplay");
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_11791718)), ignore)]
fn corpus_v15_0_forrest_sherman_pvp_shot_hits() {
    run_shot_hits("20260127_185500_PASD610-Forrest-Sherman_56_AngelWings.wowsreplay");
}

// v15.1, pvp, Domination (Vermont)
#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn corpus_v15_1_vermont_pvp() {
    run_corpus("20260213_143518_PASB110-Vermont_22_tierra_del_fuego.wowsreplay");
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn corpus_v15_1_vermont_pvp_shot_hits() {
    run_shot_hits("20260213_143518_PASB110-Vermont_22_tierra_del_fuego.wowsreplay");
}

// v15.1, pvp, (Marceau)
#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn corpus_v15_1_marceau_pvp() {
    run_corpus("20260213_203056_PFSD210-Marceau_22_tierra_del_fuego.wowsreplay");
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn corpus_v15_1_marceau_pvp_shot_hits() {
    run_shot_hits("20260213_203056_PFSD210-Marceau_22_tierra_del_fuego.wowsreplay");
}

// v15.1, pve, operation (Narai)
#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn corpus_v15_1_narai_pve_operation() {
    run_corpus("20260223_115252_PZSC718-Narai_s06_Atoll.wowsreplay");
}
