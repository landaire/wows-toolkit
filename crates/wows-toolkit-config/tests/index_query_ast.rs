//! Behaviour tests for the query AST against a real SQLite database. The
//! unit tests in `query_sql` assert SQL shape; these assert what actually
//! comes back.

use std::path::Path;
use std::path::PathBuf;

use jiff::Timestamp;
use sqlx::sqlite::SqlitePoolOptions;
use wows_core::game_types::AccountId;
use wows_core::game_types::ArenaId;
use wows_core::game_types::GameParamId;
use wows_toolkit_config::index::query;
use wows_toolkit_config::index::query_ast::CmpOp;
use wows_toolkit_config::index::query_ast::DivisionScope;
use wows_toolkit_config::index::query_ast::Expr;
use wows_toolkit_config::index::query_ast::MatchExpr;
use wows_toolkit_config::index::query_ast::MatchTerm;
use wows_toolkit_config::index::query_ast::Op;
use wows_toolkit_config::index::query_ast::Quant;
use wows_toolkit_config::index::query_ast::RosterExpr;
use wows_toolkit_config::index::query_ast::RosterField;
use wows_toolkit_config::index::query_ast::RosterTerm;
use wows_toolkit_config::index::query_ast::Value;
use wows_toolkit_config::index::query_sql::CompileCtx;
use wows_toolkit_config::index::rows::IndexedVehicleRow;
use wows_toolkit_config::index::rows::MatchOutcome;
use wows_toolkit_config::index::rows::ObjectiveMatch;
use wows_toolkit_config::index::rows::ReplayRecord;
use wows_toolkit_config::index::rows::SourceId;
use wows_toolkit_config::index::rows::VehicleRelation;

async fn mem_pool() -> sqlx::SqlitePool {
    let pool = SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    pool
}

fn a_match(arena: i64) -> ObjectiveMatch {
    ObjectiveMatch {
        arena_id: ArenaId::new(arena),
        timestamp: Timestamp::from_second(1_700_000_000 + arena).unwrap(),
        map: "spaces/13_OC_new_dawn".into(),
        game_mode: "Domination".into(),
        game_type: "pvp".into(),
        match_group: "pvp".into(),
        version_build: Some(1234),
    }
}

fn a_record(arena: i64, source: SourceId) -> ReplayRecord {
    ReplayRecord {
        arena_id: ArenaId::new(arena),
        source_id: source,
        replay_path: PathBuf::from(format!("{arena}.wowsreplay")),
        file_mtime: Some(42),
        outcome: MatchOutcome::Win,
        self_account_id: Some(AccountId(7)),
        self_ship_id: Some(GameParamId::from(999u64)),
        self_survived: Some(true),
        self_damage: Some(123_456),
        self_kills: Some(2),
        self_pr: Some(1500.0),
        results_available: true,
        indexed_at: Timestamp::from_second(1_700_000_100).unwrap(),
    }
}

/// A roster row with everything defaulted; tests override the fields they care
/// about. Nothing here is a sentinel: `None` means the column is genuinely
/// unknown for this row.
fn a_vehicle(arena: i64, account: i64, relation: VehicleRelation) -> IndexedVehicleRow {
    IndexedVehicleRow {
        arena_id: ArenaId::new(arena),
        account_id: AccountId(account),
        player_name: format!("Player{account}"),
        clan: "CLAN".into(),
        realm: Some("na".into()),
        ship_id: GameParamId::from(900u64 + account as u64),
        ship_index: "PJSD018".into(),
        ship_name: "Harugumo".into(),
        nation: "japan".into(),
        species: "Destroyer".into(),
        tier: 10,
        relation,
        division_id: None,
        survived: Some(true),
        damage: Some(50_000),
        kills: Some(1),
        spotting: Some(10_000),
        potential: Some(500_000),
        received: Some(5_000),
        pr: Some(1200.0),
        is_test_ship: false,
        disconnected: Some(false),
        is_stream_sniper: None,
        sniper_twitch_login: None,
    }
}

async fn seed(pool: &sqlx::SqlitePool, arena: i64, vehicles: Vec<IndexedVehicleRow>) -> SourceId {
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let src = query::ensure_default_source(pool, Path::new("C:/wows/replays"), now).await.unwrap();
    query::upsert_match(pool, &a_match(arena)).await.unwrap();
    query::upsert_vehicles(pool, &vehicles).await.unwrap();
    query::upsert_record(pool, &a_record(arena, src)).await.unwrap();
    src
}

async fn run(pool: &sqlx::SqlitePool, expr: &MatchExpr) -> Vec<ArenaId> {
    let ctx = CompileCtx::default();
    query::search_by_ast(pool, expr, &ctx, 500).await.unwrap().into_iter().map(|h| h.arena_id).collect()
}

fn roster(quant: Quant, pred: RosterExpr) -> MatchExpr {
    Expr::Leaf(MatchTerm::Roster { quant, pred })
}

fn rleaf(field: RosterField, op: Op, value: Value) -> RosterExpr {
    Expr::Leaf(RosterTerm { field, op, value })
}

#[tokio::test]
async fn empty_query_returns_everything() {
    let pool = mem_pool().await;
    seed(&pool, 1, vec![a_vehicle(1, 7, VehicleRelation::SelfPlayer)]).await;
    assert_eq!(run(&pool, &Expr::All(vec![])).await, vec![ArenaId::new(1)]);
}

#[tokio::test]
async fn division_mine_finds_a_divmate_on_a_test_ship() {
    let pool = mem_pool().await;
    let mut me = a_vehicle(1, 7, VehicleRelation::SelfPlayer);
    me.division_id = Some(55);
    let mut mate = a_vehicle(1, 8, VehicleRelation::Ally);
    mate.division_id = Some(55);
    mate.is_test_ship = true;
    // Same test ship, but not in my division.
    let mut stranger = a_vehicle(1, 9, VehicleRelation::Ally);
    stranger.division_id = Some(77);
    stranger.is_test_ship = true;
    seed(&pool, 1, vec![me, mate, stranger]).await;

    let q = roster(
        Quant::Any,
        Expr::All(vec![
            rleaf(RosterField::Division, Op::Is, Value::Division(DivisionScope::Mine)),
            rleaf(RosterField::TestShip, Op::Is, Value::Bool(true)),
        ]),
    );
    assert_eq!(run(&pool, &q).await, vec![ArenaId::new(1)]);
}

#[tokio::test]
async fn division_mine_is_false_when_the_self_player_was_solo() {
    let pool = mem_pool().await;
    // Self has no division; another player does. "mine" must not match theirs.
    let me = a_vehicle(1, 7, VehicleRelation::SelfPlayer);
    let mut other = a_vehicle(1, 8, VehicleRelation::Ally);
    other.division_id = Some(55);
    seed(&pool, 1, vec![me, other]).await;

    let q = roster(Quant::Any, rleaf(RosterField::Division, Op::Is, Value::Division(DivisionScope::Mine)));
    assert!(run(&pool, &q).await.is_empty());
}

#[tokio::test]
async fn division_mine_ignores_an_enemy_sharing_the_division_id() {
    let pool = mem_pool().await;
    let mut me = a_vehicle(1, 7, VehicleRelation::SelfPlayer);
    me.division_id = Some(55);
    // Same id on the other team. A division is same-team, so this must not count.
    let mut enemy = a_vehicle(1, 9, VehicleRelation::Enemy);
    enemy.division_id = Some(55);
    enemy.is_test_ship = true;
    seed(&pool, 1, vec![me, enemy]).await;

    let q = roster(
        Quant::Any,
        Expr::All(vec![
            rleaf(RosterField::Division, Op::Is, Value::Division(DivisionScope::Mine)),
            rleaf(RosterField::TestShip, Op::Is, Value::Bool(true)),
        ]),
    );
    assert!(run(&pool, &q).await.is_empty());
}

#[tokio::test]
async fn count_thresholds_over_the_enemy_team() {
    let pool = mem_pool().await;
    let me = a_vehicle(1, 7, VehicleRelation::SelfPlayer);
    let mut e1 = a_vehicle(1, 8, VehicleRelation::Enemy);
    e1.damage = Some(150_000);
    let mut e2 = a_vehicle(1, 9, VehicleRelation::Enemy);
    e2.damage = Some(120_000);
    let mut e3 = a_vehicle(1, 10, VehicleRelation::Enemy);
    e3.damage = Some(10_000);
    seed(&pool, 1, vec![me, e1, e2, e3]).await;

    let high_enemies = |n: u32, op: CmpOp| {
        roster(
            Quant::Count(op, n),
            Expr::All(vec![
                rleaf(RosterField::Relation, Op::Is, Value::Relation(VehicleRelation::Enemy)),
                rleaf(RosterField::Damage, Op::Gt, Value::Int(100_000)),
            ]),
        )
    };
    assert_eq!(run(&pool, &high_enemies(2, CmpOp::Ge)).await, vec![ArenaId::new(1)]);
    assert!(run(&pool, &high_enemies(3, CmpOp::Ge)).await.is_empty());
    assert_eq!(run(&pool, &high_enemies(2, CmpOp::Eq)).await, vec![ArenaId::new(1)]);
}

#[tokio::test]
async fn none_expresses_all_enemies_died() {
    let pool = mem_pool().await;
    let me = a_vehicle(1, 7, VehicleRelation::SelfPlayer);
    let mut e1 = a_vehicle(1, 8, VehicleRelation::Enemy);
    e1.survived = Some(false);
    let mut e2 = a_vehicle(1, 9, VehicleRelation::Enemy);
    e2.survived = Some(false);
    seed(&pool, 1, vec![me, e1, e2]).await;

    let all_enemies_died = roster(
        Quant::None,
        Expr::All(vec![
            rleaf(RosterField::Relation, Op::Is, Value::Relation(VehicleRelation::Enemy)),
            rleaf(RosterField::Survived, Op::Is, Value::Bool(true)),
        ]),
    );
    assert_eq!(run(&pool, &all_enemies_died).await, vec![ArenaId::new(1)]);

    // One survivor flips it.
    let pool2 = mem_pool().await;
    let me2 = a_vehicle(2, 7, VehicleRelation::SelfPlayer);
    let mut s1 = a_vehicle(2, 8, VehicleRelation::Enemy);
    s1.survived = Some(false);
    let s2 = a_vehicle(2, 9, VehicleRelation::Enemy); // survived: Some(true)
    seed(&pool2, 2, vec![me2, s1, s2]).await;
    assert!(run(&pool2, &all_enemies_died).await.is_empty());
}

#[tokio::test]
async fn a_null_stat_fails_comparisons_but_is_reachable_via_is_not_set() {
    let pool = mem_pool().await;
    let mut me = a_vehicle(1, 7, VehicleRelation::SelfPlayer);
    me.damage = None;
    seed(&pool, 1, vec![me]).await;

    let gt = roster(Quant::Any, rleaf(RosterField::Damage, Op::Gt, Value::Int(0)));
    assert!(run(&pool, &gt).await.is_empty(), "a NULL stat must not satisfy a comparison");

    let le = roster(Quant::Any, rleaf(RosterField::Damage, Op::Le, Value::Int(0)));
    assert!(run(&pool, &le).await.is_empty(), "a NULL stat must not satisfy the inverse either");

    let unset = roster(Quant::Any, rleaf(RosterField::Damage, Op::IsNotSet, Value::NoOperand));
    assert_eq!(run(&pool, &unset).await, vec![ArenaId::new(1)]);
}

#[tokio::test]
async fn not_negates_a_whole_subtree() {
    let pool = mem_pool().await;
    seed(&pool, 1, vec![a_vehicle(1, 7, VehicleRelation::SelfPlayer)]).await;

    let tier_10 = roster(Quant::Any, rleaf(RosterField::Tier, Op::Eq, Value::Int(10)));
    assert_eq!(run(&pool, &tier_10).await, vec![ArenaId::new(1)]);
    assert!(run(&pool, &Expr::Not(Box::new(tier_10))).await.is_empty());
}

#[tokio::test]
async fn free_text_matches_a_player_name() {
    let pool = mem_pool().await;
    let mut me = a_vehicle(1, 7, VehicleRelation::SelfPlayer);
    me.player_name = "Bismarck_Fan".into();
    seed(&pool, 1, vec![me]).await;

    assert_eq!(run(&pool, &Expr::Leaf(MatchTerm::FreeText("bismarck".into()))).await, vec![ArenaId::new(1)]);
    assert!(run(&pool, &Expr::Leaf(MatchTerm::FreeText("nobody".into()))).await.is_empty());
}

#[tokio::test]
async fn limit_plus_one_lets_the_caller_detect_truncation() {
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let src = query::ensure_default_source(&pool, Path::new("C:/wows/replays"), now).await.unwrap();
    for arena in 1..=5 {
        query::upsert_match(&pool, &a_match(arena)).await.unwrap();
        query::upsert_vehicles(&pool, &[a_vehicle(arena, 7, VehicleRelation::SelfPlayer)]).await.unwrap();
        query::upsert_record(&pool, &a_record(arena, src)).await.unwrap();
    }
    let ctx = CompileCtx::default();
    let hits = query::search_by_ast(&pool, &Expr::All(vec![]), &ctx, 3).await.unwrap();
    assert_eq!(hits.len(), 4, "must fetch limit + 1 so the caller can detect more");
}
