#![cfg(feature = "vfs")]
mod support;

#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn ingests_without_panic() {
    let h = support::load("20260213_143518_PASB110-Vermont_22_tierra_del_fuego.wowsreplay");
    let mut world =
        wows_battle_world::BattleWorld::new(&h.replay.meta, h.game_params, Some(h.game_constants));
    let mut parser = wows_replays::packet2::Parser::with_version(h.specs, h.version);
    let mut remaining = &h.replay.packet_data[..];
    use wows_replays::analyzer::Analyzer;
    while !remaining.is_empty() {
        let packet = parser.parse_packet(&mut remaining).expect("parse");
        world.process(&packet);
    }
    world.finish();
}
