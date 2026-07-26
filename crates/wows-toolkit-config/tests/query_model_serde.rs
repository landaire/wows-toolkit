use jiff::Timestamp;
use wows_core::game_types::AccountId;
use wows_core::game_types::GameParamId;
use wows_toolkit_config::index::query_model::Chip;
use wows_toolkit_config::index::query_model::Connector;
use wows_toolkit_config::index::query_model::Field;
use wows_toolkit_config::index::query_model::Group;
use wows_toolkit_config::index::query_model::Op;
use wows_toolkit_config::index::query_model::Query;
use wows_toolkit_config::index::query_model::StatKind;
use wows_toolkit_config::index::query_model::Subject;
use wows_toolkit_config::index::query_model::Value;
use wows_toolkit_config::index::rows::MatchOutcome;
use wows_toolkit_config::index::rows::SourceId;

/// A `Query` spanning multiple groups and field/value kinds must survive a
/// JSON round-trip byte-for-byte equal, so the search tab can persist the
/// user's current query across app restarts.
#[test]
fn query_round_trips_through_json() {
    let query = Query {
        connector: Connector::Or,
        groups: vec![
            Group {
                chips: vec![
                    Chip {
                        field: Field::Stat { kind: StatKind::Damage, subject: Subject::Player(AccountId(1234)) },
                        op: Op::Ge,
                        value: Value::Int(50_000),
                    },
                    Chip {
                        field: Field::Stat { kind: StatKind::Survived, subject: Subject::SelfPlayer },
                        op: Op::Is,
                        value: Value::Bool(true),
                    },
                    Chip {
                        field: Field::SelfShip,
                        op: Op::Is,
                        value: Value::Ship(GameParamId::from(4_181_143_478u64)),
                    },
                ],
            },
            Group {
                chips: vec![
                    Chip { field: Field::Outcome, op: Op::Is, value: Value::Outcome(MatchOutcome::Win) },
                    Chip {
                        field: Field::Date,
                        op: Op::Ge,
                        value: Value::Timestamp(Timestamp::from_second(1_700_000_000).unwrap()),
                    },
                    Chip { field: Field::Map, op: Op::Contains, value: Value::Text("Ocean".to_string()) },
                    Chip { field: Field::Group, op: Op::Is, value: Value::Source(SourceId(7)) },
                ],
            },
        ],
    };

    let json = serde_json::to_string(&query).expect("query must serialize to JSON");
    let restored: Query = serde_json::from_str(&json).expect("query must deserialize from JSON");

    assert_eq!(query, restored);
}
