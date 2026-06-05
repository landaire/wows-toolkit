#![cfg(feature = "vfs")]

#[path = "support/mod.rs"]
mod support;

use wows_replays::ReplayFile;
use wows_replays::analyzer::Analyzer;
use wows_replays::analyzer::battle_controller::BattleController;
use wows_replays::analyzer::battle_controller::listener::BattleControllerState;
use wows_replays::analyzer::battle_controller::state::ResolvedShotHit;
use wowsunpack::data::ResourceLoader as _;
use wowsunpack::data::Version;
use wowsunpack::rpc::entitydefs::EntitySpec;

/// End-of-replay parity for in-flight artillery salvos.
fn run_active_shots(filename: &str) {
    let (old, mut new_world) = support::both(filename);
    let old_shots = old.active_shots();
    let new_shots = new_world.active_shots();
    assert_eq!(
        old_shots.len(),
        new_shots.len(),
        "active_shots count mismatch in {filename}: old={} new={}",
        old_shots.len(),
        new_shots.len()
    );
    assert_eq!(old_shots, new_shots.as_slice(), "active_shots mismatch in {filename}");
}

/// End-of-replay parity for in-flight torpedoes.
fn run_active_torpedoes(filename: &str) {
    let (old, mut new_world) = support::both(filename);
    let old_torps = old.active_torpedoes();
    let new_torps = new_world.active_torpedoes();
    assert_eq!(
        old_torps.len(),
        new_torps.len(),
        "active_torpedoes count mismatch in {filename}: old={} new={}",
        old_torps.len(),
        new_torps.len()
    );
    assert_eq!(old_torps, new_torps.as_slice(), "active_torpedoes mismatch in {filename}");
}

/// Drive both controllers packet-by-packet, accumulating each frame's shot_hits.
///
/// Because Tracked clears the hit log every packet, the end-of-replay state only
/// holds the final frame. Accumulating per-packet validates every resolved hit
/// across the whole replay.
fn both_stepped(filename: &str) -> (Vec<ResolvedShotHit>, Vec<ResolvedShotHit>) {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests")
        .join("fixtures")
        .join("replays")
        .join(filename);
    let replay = ReplayFile::from_file(&path).unwrap_or_else(|e| panic!("failed to parse {filename}: {e:?}"));
    let version = Version::from_client_exe(&replay.meta.clientVersionFromExe);

    let handle = support::load(filename);
    let provider = handle.game_params;
    let constants = handle.game_constants;
    let specs: &'static [EntitySpec] = provider.entity_specs();

    let meta: &'static wows_replays::ReplayMeta = Box::leak(Box::new(replay.meta));

    let mut old = BattleController::new(meta, provider, Some(constants));
    let mut new_world = wows_battle_world::BattleWorld::new(meta, provider, Some(constants));

    let mut old_acc: Vec<ResolvedShotHit> = Vec::new();
    let mut new_acc: Vec<ResolvedShotHit> = Vec::new();

    let mut parser = wows_replays::packet2::Parser::with_version(specs, version);
    let mut remaining = &replay.packet_data[..];
    while !remaining.is_empty() {
        let packet = parser.parse_packet(&mut remaining).expect("packet parse");
        old.process(&packet);
        new_world.process(&packet);
        old_acc.extend_from_slice(old.shot_hits());
        new_acc.extend(new_world.shot_hits());
    }
    old.finish();
    new_world.finish();

    (old_acc, new_acc)
}

fn run_shot_hits(filename: &str) {
    let (old_hits, new_hits) = both_stepped(filename);
    assert!(!old_hits.is_empty(), "no shot_hits accumulated in {filename}; fixture exercises nothing");
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

#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn parity_active_shots_vermont() {
    run_active_shots("20260213_143518_PASB110-Vermont_22_tierra_del_fuego.wowsreplay");
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn parity_active_torpedoes_marceau() {
    run_active_torpedoes("20260213_203056_PFSD210-Marceau_22_tierra_del_fuego.wowsreplay");
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn parity_shot_hits_vermont() {
    run_shot_hits("20260213_143518_PASB110-Vermont_22_tierra_del_fuego.wowsreplay");
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn parity_shot_hits_marceau() {
    run_shot_hits("20260213_203056_PFSD210-Marceau_22_tierra_del_fuego.wowsreplay");
}
