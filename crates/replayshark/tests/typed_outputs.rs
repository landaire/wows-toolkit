//! Snapshots of the typed analyses of a fixture replay.
//!
//! Skipped when the replay's build has no local game data (see
//! `wows_data_mgr::game_dir_for_build`).

use std::path::PathBuf;

use replayshark::game_data::CliGameDataContext;
use replayshark::query::PlayerFilter;
use wows_replays::ReplayFile;
use wows_replays::context::CachedContext;
use wows_replays::context::GameDataContext;

const REPLAY: &str = "20260213_143518_PASB110-Vermont_22_tierra_del_fuego.wowsreplay";

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/replays").join(name)
}

#[test]
fn typed_outputs_of_a_fixture_replay() {
    let replay = ReplayFile::from_file(&fixture(REPLAY)).expect("the fixture replay reads");
    let version = replayshark::query::game_version(&replay.meta).expect("the fixture has a version");
    let Some(build_dir) = version.build_number().and_then(wows_data_mgr::game_dir_for_build) else {
        eprintln!("Skipping: no game data for build {:?}", version.build_number());
        return;
    };

    let ctx = CachedContext::new(CliGameDataContext::new(None, Some(&build_dir), None));
    let ctx: &dyn GameDataContext = &ctx;

    let summary = replayshark::summary::summarize(&replay, ctx).expect("summary");
    insta::assert_yaml_snapshot!("summary", summary);

    let chat = replayshark::chat::chat_log(&replay, ctx).expect("chat");
    insta::assert_yaml_snapshot!("chat", chat);

    let players = replayshark::query::players(&replay, ctx, &PlayerFilter::default()).expect("players");
    insta::assert_yaml_snapshot!("players", players);

    let filter = PlayerFilter { ship: Some("vermont".to_owned()), ..PlayerFilter::default() };
    let vermont = replayshark::query::players(&replay, ctx, &filter).expect("filtered players");
    assert_eq!(vermont.iter().map(|row| row.player_name.as_str()).collect::<Vec<_>>(), ["P_yk"]);

    let roster = replayshark::query::roster(&replay.meta, ctx).expect("roster");
    insta::assert_yaml_snapshot!("roster", roster);
    assert!(roster.matches(&["battleship".to_owned()], false));
    assert!(!roster.matches(&["battleship".to_owned()], true));

    let arena_id = replayshark::query::arena_id(&replay, ctx).expect("arena id");
    insta::assert_yaml_snapshot!("arena_id", arena_id);

    let survey = replayshark::survey::survey_replay(&replay, ctx, false).expect("survey");
    insta::assert_yaml_snapshot!("survey", survey);
}
