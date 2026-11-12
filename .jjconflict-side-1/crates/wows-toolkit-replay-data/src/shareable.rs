//! What the data-sharing setting needs from one parsed battle.

/// Whether the recording player's ship is positively not a test ship.
///
/// Any doubt is `false`: an unknown ship keeps the whole file off the service,
/// which is the rule `decide_upload_action` documents.
pub fn self_is_not_a_test_ship(report: &wows_battle_world::report::BattleReport) -> bool {
    report
        .players()
        .iter()
        .find(|player| player.relation().is_self())
        .and_then(|player| player.vehicle().vehicle())
        .map(|vehicle| !vehicle.is_test_ship())
        .unwrap_or(false)
}
