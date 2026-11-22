use sqlx::SqlitePool;
use sqlx::sqlite::SqlitePoolOptions;
use wows_toolkit_config::queries;
use wows_toolkit_config::queries::SessionStatDetails;
use wows_toolkit_config::queries::SessionStatKey;
use wows_toolkit_config::queries::SessionStatRow;

async fn mem_pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    pool
}

fn row() -> SessionStatRow {
    SessionStatRow {
        id: 0,
        sort_key: "2026-11-01 20:00:00".to_owned(),
        ship_name: "Yamato".to_owned(),
        ship_id: 4_288_675_536,
        player_id: 7,
        game_time: "01.11.2026 20:00:00".to_owned(),
        match_group: "pvp".to_owned(),
        damage: 120_000,
        spotting_damage: 30_000,
        frags: 2,
        raw_xp: 2_400,
        base_xp: 1_600,
        is_win: true,
        is_loss: false,
        is_draw: false,
        is_div: false,
        achievements: "[]".to_owned(),
        match_contribution: Some(1_250.0),
        match_contribution_version: Some(3),
        potential_damage: Some(1_900_000),
        received_damage: Some(48_000),
        hits: Some(37),
        fires: Some(4),
        floods: Some(1),
        citadels: Some(3),
        crits: Some(6),
        distance_km: Some(23.5),
        capture_points: Some(1_800),
        planes_killed: Some(11),
        anti_air_damage: Some(9_400),
        time_lived_secs: Some(1_034),
        survived: Some(false),
        torpedoes_spotted: Some(5),
        heals: None,
    }
}

#[tokio::test]
async fn a_written_game_reads_back_with_every_detail() {
    let pool = mem_pool().await;
    queries::add_session_stat(&pool, &row()).await.expect("the game is written");

    let read = queries::get_all_session_stats(&pool).await.expect("the games are read");
    let [stored] = <[SessionStatRow; 1]>::try_from(read).expect("one game is read");

    // The row has no `PartialEq`; its debug form names every column. The id is the database's.
    assert_eq!(format!("{:?}", SessionStatRow { id: 0, ..stored }), format!("{:?}", row()));
}

/// A row an older build wrote names none of the detail columns, and reads with all of them absent.
#[tokio::test]
async fn a_row_written_before_the_details_reads_them_as_absent() {
    let pool = mem_pool().await;
    sqlx::query(
        "INSERT INTO session_stats (sort_key, ship_name, ship_id, player_id, game_time, match_group, damage, \
         spotting_damage, frags, raw_xp, base_xp, is_win, is_loss, is_draw, is_div, achievements) \
         VALUES ('2026-01-01 10:00:00', 'Yamato', 1, 7, '01.01.2026 10:00:00', 'pvp', 1, 0, 0, 0, 0, 1, 0, 0, 0, '[]')",
    )
    .execute(&pool)
    .await
    .expect("an old row is written");

    let read = queries::get_all_session_stats(&pool).await.expect("the games are read");
    let [old] = read.as_slice() else { panic!("one row is read: {read:?}") };
    let details = [
        old.potential_damage,
        old.received_damage,
        old.hits,
        old.fires,
        old.floods,
        old.citadels,
        old.crits,
        old.capture_points,
        old.planes_killed,
        old.anti_air_damage,
        old.time_lived_secs,
        old.torpedoes_spotted,
        old.heals,
    ];
    assert!(details.iter().all(Option::is_none), "{old:?}");
    assert_eq!(old.distance_km, None);
    assert_eq!(old.survived, None);
}

/// The details of [`row`]'s battle as a later parse reads them.
fn reread_details() -> SessionStatDetails {
    let row = row();

    SessionStatDetails {
        key: SessionStatKey { game_time: row.game_time, player_id: row.player_id },
        potential_damage: Some(2_100_000),
        received_damage: Some(51_000),
        hits: Some(40),
        fires: Some(5),
        floods: Some(2),
        citadels: Some(4),
        crits: Some(7),
        distance_km: Some(24.25),
        capture_points: Some(1_900),
        planes_killed: Some(12),
        anti_air_damage: Some(9_800),
        time_lived_secs: Some(1_100),
        survived: Some(true),
        torpedoes_spotted: Some(6),
        heals: Some(2),
    }
}

/// A recorded game without details takes the ones read later, and keeps every other column,
/// its rating included.
#[tokio::test]
async fn updating_a_games_details_fills_them_and_keeps_the_rest() {
    let pool = mem_pool().await;
    let recorded = SessionStatRow {
        potential_damage: None,
        received_damage: None,
        hits: None,
        fires: None,
        floods: None,
        citadels: None,
        crits: None,
        distance_km: None,
        capture_points: None,
        planes_killed: None,
        anti_air_damage: None,
        time_lived_secs: None,
        survived: None,
        torpedoes_spotted: None,
        heals: None,
        ..row()
    };
    queries::add_session_stat(&pool, &recorded).await.expect("the game is written");
    let details = reread_details();

    let updated = queries::update_session_stat_details(&pool, &details).await.expect("the details are written");
    let again = queries::update_session_stat_details(&pool, &details).await.expect("the details are written");

    assert!(updated, "the game held no details");
    assert!(!again, "the game holds these details");
    let read = queries::get_all_session_stats(&pool).await.expect("the games are read");
    let [stored] = <[SessionStatRow; 1]>::try_from(read).expect("one game is read");
    let expected = SessionStatRow {
        potential_damage: details.potential_damage,
        received_damage: details.received_damage,
        hits: details.hits,
        fires: details.fires,
        floods: details.floods,
        citadels: details.citadels,
        crits: details.crits,
        distance_km: details.distance_km,
        capture_points: details.capture_points,
        planes_killed: details.planes_killed,
        anti_air_damage: details.anti_air_damage,
        time_lived_secs: details.time_lived_secs,
        survived: details.survived,
        torpedoes_spotted: details.torpedoes_spotted,
        heals: details.heals,
        ..row()
    };
    assert_eq!(format!("{:?}", SessionStatRow { id: 0, ..stored }), format!("{expected:?}"));
}

/// Details of a battle the session does not hold add nothing to it.
#[tokio::test]
async fn updating_the_details_of_a_game_not_recorded_adds_nothing() {
    let pool = mem_pool().await;
    queries::add_session_stat(&pool, &row()).await.expect("the game is written");
    let key = |game_time: &str, player_id| SessionStatKey { game_time: game_time.to_owned(), player_id };
    let other_account = SessionStatDetails { key: key("01.11.2026 20:00:00", 8), ..reread_details() };
    let other_time = SessionStatDetails { key: key("02.11.2026 20:00:00", 7), ..reread_details() };

    for details in [other_account, other_time] {
        let updated = queries::update_session_stat_details(&pool, &details).await.expect("the update runs");
        assert!(!updated, "{details:?}");
    }

    let read = queries::get_all_session_stats(&pool).await.expect("the games are read");
    let [stored] = <[SessionStatRow; 1]>::try_from(read).expect("one game is read");
    assert_eq!(format!("{:?}", SessionStatRow { id: 0, ..stored }), format!("{:?}", row()));
}

/// The keys read are those of the recorded games.
#[tokio::test]
async fn the_session_keys_name_each_recorded_game() {
    let pool = mem_pool().await;
    queries::add_session_stat(&pool, &row()).await.expect("the game is written");
    let later = SessionStatRow { game_time: "02.11.2026 20:00:00".to_owned(), ..row() };
    queries::add_session_stat(&pool, &later).await.expect("the game is written");

    let mut keys = queries::session_stat_keys(&pool).await.expect("the keys are read");
    keys.sort_by(|a, b| a.game_time.cmp(&b.game_time));

    let expected = ["01.11.2026 20:00:00", "02.11.2026 20:00:00"]
        .map(|game_time| SessionStatKey { game_time: game_time.to_owned(), player_id: 7 });
    assert_eq!(keys, expected);
}
