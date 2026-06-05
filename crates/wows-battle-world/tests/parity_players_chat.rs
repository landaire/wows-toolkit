#![cfg(feature = "vfs")]

#[path = "support/mod.rs"]
mod support;

use wows_replays::analyzer::battle_controller::ConnectionChangeInfo;
use wows_replays::analyzer::battle_controller::listener::BattleControllerState;

fn run_parity(filename: &str) {
    let (old, new_world) = support::both(filename);

    // Player entity parity.
    let old_players = old.player_entities();
    let new_players = new_world.player_entities();

    assert_eq!(
        old_players.len(),
        new_players.len(),
        "player_entities key count mismatch in {filename}: old={} new={}",
        old_players.len(),
        new_players.len(),
    );

    let mut old_keys: Vec<_> = old_players.keys().copied().collect();
    let mut new_keys: Vec<_> = new_players.keys().copied().collect();
    old_keys.sort();
    new_keys.sort();
    assert_eq!(old_keys, new_keys, "player_entities key sets differ in {filename}");

    for (id, old_p) in old_players {
        let new_p = new_players
            .get(id)
            .unwrap_or_else(|| panic!("player_entities missing id={id:?} in {filename}"));

        let old_state = old_p.initial_state();
        let new_state = new_p.initial_state();

        assert_eq!(
            old_state.username(),
            new_state.username(),
            "player[{id:?}] name mismatch in {filename}",
        );
        assert_eq!(
            old_state.meta_ship_id(),
            new_state.meta_ship_id(),
            "player[{id:?}] account id mismatch in {filename}",
        );
        assert_eq!(
            old_p.relation(),
            new_p.relation(),
            "player[{id:?}] relation mismatch in {filename}",
        );
        assert_eq!(
            old_p.vehicle().id(),
            new_p.vehicle().id(),
            "player[{id:?}] vehicle param id mismatch in {filename}",
        );

        let old_cci: Vec<_> = old_p.connection_change_info().iter().map(|c: &ConnectionChangeInfo| {
            (c.at_game_duration(), c.event_kind(), c.had_death_event())
        }).collect();
        let new_cci: Vec<_> = new_p.connection_change_info().iter().map(|c: &ConnectionChangeInfo| {
            (c.at_game_duration(), c.event_kind(), c.had_death_event())
        }).collect();
        assert_eq!(
            old_cci, new_cci,
            "player[{id:?}] connection_change_info mismatch in {filename}: old={old_cci:?} new={new_cci:?}",
        );
    }

    // Chat parity.
    let old_chat = old.game_chat();
    let new_chat = new_world.chat();

    assert_eq!(
        old_chat.len(),
        new_chat.len(),
        "chat length mismatch in {filename}: old={} new={}",
        old_chat.len(),
        new_chat.len(),
    );

    for (i, (o, n)) in old_chat.iter().zip(new_chat.iter()).enumerate() {
        assert_eq!(o.clock, n.clock, "chat[{i}] clock mismatch in {filename}");
        assert_eq!(
            o.sender_name, n.sender_name,
            "chat[{i}] sender_name mismatch in {filename}"
        );
        assert_eq!(o.channel, n.channel, "chat[{i}] channel mismatch in {filename}");
        assert_eq!(o.message, n.message, "chat[{i}] message mismatch in {filename}");
        assert_eq!(o.entity_id, n.entity_id, "chat[{i}] entity_id mismatch in {filename}");
        assert_eq!(
            o.sender_relation, n.sender_relation,
            "chat[{i}] sender_relation mismatch in {filename}"
        );

        // Compare linked player identity without pointer equality.
        match (&o.player, &n.player) {
            (None, None) => {}
            (Some(op), Some(np)) => {
                assert_eq!(
                    op.initial_state().username(),
                    np.initial_state().username(),
                    "chat[{i}] player name mismatch in {filename}",
                );
                assert_eq!(
                    op.initial_state().meta_ship_id(),
                    np.initial_state().meta_ship_id(),
                    "chat[{i}] player account id mismatch in {filename}",
                );
            }
            _ => panic!(
                "chat[{i}] player presence mismatch in {filename}: old={} new={}",
                o.player.is_some(),
                n.player.is_some(),
            ),
        }
    }
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn parity_players_chat_vermont_pvp() {
    run_parity("20260213_143518_PASB110-Vermont_22_tierra_del_fuego.wowsreplay");
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn parity_players_chat_marceau_pvp() {
    run_parity("20260213_203056_PFSD210-Marceau_22_tierra_del_fuego.wowsreplay");
}

#[test]
#[cfg_attr(not(all(has_game_data, has_build_11965230)), ignore)]
fn parity_players_chat_narai_operation() {
    run_parity("20260223_115252_PZSC718-Narai_s06_Atoll.wowsreplay");
}
