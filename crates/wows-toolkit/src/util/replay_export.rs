//! The replay export, as the egui app writes it.
//!
//! The document itself is `wows_toolkit_viewmodel::replay_export`, which the
//! GPUI port writes too; this adapts the egui `Replay` to it.

pub use wows_toolkit_viewmodel::replay_export::FlattenedVehicle;
pub use wows_toolkit_viewmodel::replay_export::Match as ExportedMatch;

use crate::ui::replay_parser::Replay;

/// The match `replay` holds, ready to serialize.
///
/// `is_debug_mode` keeps the fields an ordinary export strips: enemy builds,
/// and test-ship results for anyone but the recording player.
pub fn export_match(replay: &Replay, is_debug_mode: bool) -> ExportedMatch {
    let battle_report = replay.battle_report.as_ref().expect("no battle report for replay?");
    let ui_report = replay.ui_report.as_ref().expect("no UI report for replay?");

    ExportedMatch::new(ui_report.normalized(), battle_report.players(), battle_report.game_chat(), is_debug_mode)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::path::PathBuf;

    /// Exports a real replay through the path the Export menu takes.
    ///
    /// This document is read by ShipBuilds.com, so it is checked against a
    /// real match rather than a fabricated one: a field that quietly stops
    /// being filled would still serialize. Needs a local game install and a
    /// replay recorded on an installed build. Run with:
    ///
    /// ```text
    /// WOWS_EXPORT_TEST_DIR="E:\WoWs\World_of_Warships" \
    /// WOWS_EXPORT_TEST_REPLAY="E:\WoWs\World_of_Warships\replays\some.wowsreplay" \
    /// cargo test -p wows_toolkit --lib -- --ignored --nocapture a_real_replay_exports_a_populated_match
    /// ```
    #[test]
    #[ignore = "needs a local game install + a replay recorded on an installed build"]
    fn a_real_replay_exports_a_populated_match() {
        let wows_dir = std::env::var("WOWS_EXPORT_TEST_DIR").expect("set WOWS_EXPORT_TEST_DIR to a WoWs install");
        let replay_path = std::env::var("WOWS_EXPORT_TEST_REPLAY").expect("set WOWS_EXPORT_TEST_REPLAY to a replay");
        let dump_dir = std::env::var("WOWS_EXPORT_TEST_DUMPS").unwrap_or_default();

        let deps = crate::profiling::headless_deps(crate::data::wows_data::BuildDataCache::new(
            PathBuf::from(&wows_dir),
            "en".to_string(),
            dump_dir,
        ));
        let replay = crate::profiling::load_one(Path::new(&replay_path), &deps).expect("the replay loads");

        let exported = export_match(&replay, false);
        assert!(!exported.vehicles.is_empty(), "a real match exports its vehicles");

        // Rated through the report, so the export carries what the table
        // shows rather than a column of nulls.
        let rated = exported.vehicles.iter().filter(|vehicle| vehicle.personal_rating.is_some()).count();
        println!("{rated} of {} vehicles carry a personal rating", exported.vehicles.len());

        let rated = exported.vehicles.iter().filter(|vehicle| vehicle.server_results.is_some()).count();
        println!("exported {} vehicles, {rated} with server results", exported.vehicles.len());
        assert!(rated > 0, "a finished match carries server results");

        // The ordinary export strips enemy builds; the debug one keeps them.
        let enemy_builds = |match_data: &ExportedMatch| {
            match_data.vehicles.iter().filter(|v| v.is_enemy && v.translated_build.is_some()).count()
        };
        assert_eq!(enemy_builds(&exported), 0, "an ordinary export carries no enemy builds");
        let debug = export_match(&replay, true);
        println!("{} enemy builds survive a debug export", enemy_builds(&debug));

        let json = serde_json::to_string(&exported).expect("the export serializes");
        assert!(json.len() > 1000, "the document is not a stub");
    }
}
