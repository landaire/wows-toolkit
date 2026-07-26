use jiff::Timestamp;
use wows_core::game_types::AccountId;
use wows_core::game_types::GameParamId;

use super::rows::MatchOutcome;
use super::rows::SourceId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Outcome,
    Map,
    Mode,
    SelfShip,
    Class,
    Tier,
    Date,
    SelfDamage,
    Kills,
    Pr,
    Survived,
    PlayerPresent,
    EnemyShip,
    AllyShip,
    Group,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq)]
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

#[derive(Debug, Clone, PartialEq)]
pub struct Chip {
    pub field: Field,
    pub op: Op,
    pub value: Value,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Group {
    pub chips: Vec<Chip>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Connector {
    And,
    Or,
}

#[derive(Debug, Clone, PartialEq)]
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

impl Field {
    pub fn value_kind(self) -> ValueKind {
        match self {
            Field::Outcome => ValueKind::Outcome,
            Field::Map | Field::Mode => ValueKind::Text,
            Field::SelfShip => ValueKind::Ship,
            Field::Class => ValueKind::Class,
            Field::Tier | Field::SelfDamage | Field::Kills => ValueKind::Int,
            Field::Pr => ValueKind::Int,
            Field::Date => ValueKind::Timestamp,
            Field::Survived => ValueKind::Bool,
            Field::PlayerPresent => ValueKind::Account,
            Field::EnemyShip | Field::AllyShip => ValueKind::Ship,
            Field::Group => ValueKind::Source,
        }
    }

    pub fn allowed_ops(self) -> &'static [Op] {
        match self {
            Field::Map | Field::Mode => TEXT_OPS,
            Field::Tier | Field::SelfDamage | Field::Kills | Field::Pr | Field::Date => NUM_OPS,
            Field::Outcome | Field::Class | Field::Survived | Field::Group => ENUM_OPS,
            Field::SelfShip => ENUM_OPS,
            Field::PlayerPresent | Field::EnemyShip | Field::AllyShip => PRESENCE_OPS,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowed_ops_match_field_kind() {
        // Text field offers text ops, numeric field offers comparisons, presence field offers Present/NotPresent.
        assert!(Field::Map.allowed_ops().contains(&Op::Contains));
        assert!(!Field::Map.allowed_ops().contains(&Op::Gt));
        assert!(Field::SelfDamage.allowed_ops().contains(&Op::Ge));
        assert!(!Field::SelfDamage.allowed_ops().contains(&Op::Contains));
        assert!(Field::PlayerPresent.allowed_ops().contains(&Op::Present));
        assert!(Field::Survived.allowed_ops().contains(&Op::Is));
        assert_eq!(Field::Map.value_kind(), ValueKind::Text);
        assert_eq!(Field::Tier.value_kind(), ValueKind::Int);
        assert_eq!(Field::PlayerPresent.value_kind(), ValueKind::Account);
    }

    #[test]
    fn query_default_is_empty() {
        let q = Query::default();
        assert!(q.groups.is_empty());
        assert!(matches!(q.connector, Connector::And));
    }
}
