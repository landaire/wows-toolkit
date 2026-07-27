use jiff::Timestamp;
use wows_core::game_types::AccountId;
use wows_core::game_types::GameParamId;

use super::rows::MatchOutcome;
use super::rows::SourceId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Field {
    Outcome,
    Map,
    Mode,
    SelfShip,
    Class,
    Tier,
    Date,
    PlayerPresent,
    /// Match-level: does any roster row's `player_name` or `clan` contain the
    /// given substring (case-insensitive)? Unlike `PlayerPresent`, this has no
    /// resolved identity; it is a free-text substring search over both columns.
    PlayerNameOrClan,
    EnemyShip,
    AllyShip,
    Group,
    /// Match-level flag: does this arena's roster contain a vehicle whose
    /// `is_stream_sniper` was fuzzy-matched to a Twitch chatter? Not a `StatKind`
    /// (it has no per-subject meaning) and has no `Subject`.
    ContainsStreamSniper,
    /// A roster stat, scoped to a subject (me, any player, or a specific player).
    /// Always reads `indexed_vehicle` (the roster), never `replay_record.self_*`,
    /// so all seven stats are uniformly available for every subject.
    Stat {
        kind: StatKind,
        subject: Subject,
    },
}

/// Which roster stat a `Field::Stat` reads. Maps 1:1 to an `indexed_vehicle` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum StatKind {
    Damage,
    Kills,
    Spotting,
    Potential,
    Received,
    Pr,
    Survived,
    Disconnected,
}

impl StatKind {
    pub const ALL: [StatKind; 8] = [
        StatKind::Damage,
        StatKind::Kills,
        StatKind::Spotting,
        StatKind::Potential,
        StatKind::Received,
        StatKind::Pr,
        StatKind::Survived,
        StatKind::Disconnected,
    ];

    /// The `indexed_vehicle` column this stat reads.
    pub fn column(self) -> &'static str {
        match self {
            StatKind::Damage => "damage",
            StatKind::Kills => "kills",
            StatKind::Spotting => "spotting",
            StatKind::Potential => "potential",
            StatKind::Received => "received",
            StatKind::Pr => "pr",
            StatKind::Survived => "survived",
            StatKind::Disconnected => "disconnected",
        }
    }

    /// True for `Survived` and `Disconnected`, the boolean-valued stats.
    pub fn is_bool(self) -> bool {
        matches!(self, StatKind::Survived | StatKind::Disconnected)
    }
}

/// Which roster row(s) a `Field::Stat` predicate is evaluated against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Subject {
    /// The perspective player's own roster row (`indexed_vehicle.relation = 'self'`).
    SelfPlayer,
    /// Any roster row in the match, regardless of relation.
    AnyPlayer,
    /// A specific account's roster row, if present in the match.
    Player(AccountId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Op {
    Contains,
    Equals,
    NotEquals,
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
    Is,
    IsNot,
    Present,
    NotPresent,
}

/// Which `Value` variant a field expects; drives the UI value editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ValueKind {
    Text,
    Int,
    Outcome,
    Class,
    Bool,
    Ship,
    Account,
    Timestamp,
    Source,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Value {
    Text(String),
    Int(i64),
    Outcome(MatchOutcome),
    Class(String),
    Bool(bool),
    Ship(GameParamId),
    Account(AccountId),
    Timestamp(Timestamp),
    Source(SourceId),
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Chip {
    pub field: Field,
    pub op: Op,
    pub value: Value,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Group {
    pub chips: Vec<Chip>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Connector {
    And,
    Or,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Query {
    pub groups: Vec<Group>,
    pub connector: Connector,
}

impl Default for Query {
    fn default() -> Self {
        Self { groups: Vec::new(), connector: Connector::And }
    }
}

const TEXT_OPS: &[Op] = &[Op::Contains, Op::Equals, Op::NotEquals];
const NUM_OPS: &[Op] = &[Op::Eq, Op::Ne, Op::Gt, Op::Ge, Op::Lt, Op::Le];
const ENUM_OPS: &[Op] = &[Op::Is, Op::IsNot];
const PRESENCE_OPS: &[Op] = &[Op::Present, Op::NotPresent];
const CONTAINS_ONLY_OPS: &[Op] = &[Op::Contains];

impl Field {
    pub fn value_kind(self) -> ValueKind {
        match self {
            Field::Outcome => ValueKind::Outcome,
            Field::Map | Field::Mode => ValueKind::Text,
            Field::SelfShip => ValueKind::Ship,
            Field::Class => ValueKind::Class,
            Field::Tier => ValueKind::Int,
            Field::Date => ValueKind::Timestamp,
            Field::PlayerPresent => ValueKind::Account,
            Field::PlayerNameOrClan => ValueKind::Text,
            Field::EnemyShip | Field::AllyShip => ValueKind::Ship,
            Field::Group => ValueKind::Source,
            Field::ContainsStreamSniper => ValueKind::Bool,
            Field::Stat { kind, .. } => {
                if kind.is_bool() {
                    ValueKind::Bool
                } else {
                    ValueKind::Int
                }
            }
        }
    }

    pub fn allowed_ops(self) -> &'static [Op] {
        match self {
            Field::Map | Field::Mode => TEXT_OPS,
            Field::Tier | Field::Date => NUM_OPS,
            Field::Outcome | Field::Class | Field::Group => ENUM_OPS,
            Field::SelfShip => ENUM_OPS,
            Field::PlayerPresent | Field::EnemyShip | Field::AllyShip => PRESENCE_OPS,
            Field::PlayerNameOrClan => CONTAINS_ONLY_OPS,
            Field::ContainsStreamSniper => ENUM_OPS,
            Field::Stat { kind, .. } => {
                if kind.is_bool() {
                    ENUM_OPS
                } else {
                    NUM_OPS
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowed_ops_match_field_kind() {
        // Text field offers text ops, numeric field offers comparisons, presence field offers Present/NotPresent.
        let damage_self = Field::Stat { kind: StatKind::Damage, subject: Subject::SelfPlayer };
        let survived_self = Field::Stat { kind: StatKind::Survived, subject: Subject::SelfPlayer };
        assert!(Field::Map.allowed_ops().contains(&Op::Contains));
        assert!(!Field::Map.allowed_ops().contains(&Op::Gt));
        assert!(damage_self.allowed_ops().contains(&Op::Ge));
        assert!(!damage_self.allowed_ops().contains(&Op::Contains));
        assert!(Field::PlayerPresent.allowed_ops().contains(&Op::Present));
        assert!(survived_self.allowed_ops().contains(&Op::Is));
        assert_eq!(Field::Map.value_kind(), ValueKind::Text);
        assert_eq!(Field::Tier.value_kind(), ValueKind::Int);
        assert_eq!(Field::PlayerPresent.value_kind(), ValueKind::Account);
        assert_eq!(damage_self.value_kind(), ValueKind::Int);
        assert_eq!(survived_self.value_kind(), ValueKind::Bool);
    }

    #[test]
    fn stat_kind_all_covers_every_variant_with_correct_columns_and_bool_flag() {
        assert_eq!(StatKind::ALL.len(), 8);
        let columns: Vec<&str> = StatKind::ALL.iter().map(|k| k.column()).collect();
        assert_eq!(
            columns,
            vec!["damage", "kills", "spotting", "potential", "received", "pr", "survived", "disconnected"]
        );
        for kind in StatKind::ALL {
            assert_eq!(kind.is_bool(), matches!(kind, StatKind::Survived | StatKind::Disconnected));
        }
    }

    #[test]
    fn contains_stream_sniper_is_a_standalone_bool_field() {
        assert_eq!(Field::ContainsStreamSniper.value_kind(), ValueKind::Bool);
        assert!(Field::ContainsStreamSniper.allowed_ops().contains(&Op::Is));
        assert!(Field::ContainsStreamSniper.allowed_ops().contains(&Op::IsNot));
        assert!(!Field::ContainsStreamSniper.allowed_ops().contains(&Op::Gt));
    }

    #[test]
    fn query_default_is_empty() {
        let q = Query::default();
        assert!(q.groups.is_empty());
        assert!(matches!(q.connector, Connector::And));
    }
}
