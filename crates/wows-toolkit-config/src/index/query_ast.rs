//! The search query AST: a boolean tree over match-level fields and quantified
//! predicates over the match roster.
//!
//! Kept free of SQL (`query_sql`) and of parsing (`query_text`) so the shape of
//! a query can be reasoned about and tested on its own.

use jiff::Timestamp;
use wows_core::game_types::AccountId;
use wows_core::game_types::GameMode;
use wows_core::game_types::GameParamId;
use wows_core::game_types::PersonalRatingCategory;

use super::rows::MatchOutcome;
use super::rows::SourceId;
use super::rows::VehicleRelation;

/// A boolean tree. Generic over its leaf so the match level and the roster
/// level share one set of tree operations and one renderer.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr<L> {
    /// AND. Empty matches everything.
    All(Vec<Expr<L>>),
    /// OR. Empty matches nothing.
    Any(Vec<Expr<L>>),
    Not(Box<Expr<L>>),
    Leaf(L),
}

pub type MatchExpr = Expr<MatchTerm>;
pub type RosterExpr = Expr<RosterTerm>;

impl<L> Expr<L> {
    /// True for the query that constrains nothing, so callers can skip emitting
    /// a WHERE clause entirely.
    pub fn is_empty_all(&self) -> bool {
        matches!(self, Expr::All(cs) if cs.is_empty())
    }

    /// Every direct subexpression, so a tree walker sees the whole tree. A
    /// `Not` yields its operand: skipping it would make a walk silently miss
    /// everything under a negation.
    pub fn children(&self) -> &[Expr<L>] {
        match self {
            Expr::All(cs) | Expr::Any(cs) => cs,
            Expr::Not(inner) => std::slice::from_ref(&**inner),
            Expr::Leaf(_) => &[],
        }
    }
}

impl<L> Default for Expr<L> {
    fn default() -> Self {
        Expr::All(Vec::new())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum MatchTerm {
    /// A field comparison. The `Op` must be one of `field.allowed_ops()` and the
    /// `Value` must match `field.value_kind()`; nothing checks this at compile
    /// time and the printer does not round trip a term that breaks it.
    ///
    /// `Equals`, `Eq`, and `Is` all print as `=`, and the parser picks between
    /// them from the field's `ValueKind`, so a tree built with the wrong one of
    /// the three reparses into a different tree.
    Field(MatchField, Op, Value),
    Roster {
        quant: Quant,
        pred: RosterExpr,
    },
    /// A bare word: matched against map, player name, clan, and ship name.
    FreeText(String),
}

/// One roster field comparison.
///
/// As with `MatchTerm::Field`, `op` must come from `field.allowed_ops()` and
/// `value` must match `field.value_kind()`. On top of the printing hazard the
/// three `=` spellings carry, `try_print_sugar` matches `Op::Is` specifically:
/// a sugar-shaped tree whose relation term was built with `Op::Equals` prints
/// as the general `any(...)` form instead.
#[derive(Debug, Clone, PartialEq)]
pub struct RosterTerm {
    pub field: RosterField,
    pub op: Op,
    pub value: Value,
}

/// How many roster rows must satisfy the predicate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quant {
    /// At least one. Compiles to EXISTS so SQLite can stop at the first hit.
    Any,
    /// Zero. Compiles to NOT EXISTS.
    None,
    Count(CmpOp, u32),
}

impl Quant {
    pub fn inverse(self) -> Quant {
        match self {
            Quant::Any => Quant::None,
            Quant::None => Quant::Any,
            Quant::Count(op, n) => Quant::Count(op.inverse(), n),
        }
    }
}

/// The numeric comparisons. Separate from `Op` because a row count can only be
/// compared numerically; sharing `Op` would make `count(...) contains 3`
/// representable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
}

impl CmpOp {
    pub const ALL: [CmpOp; 6] = [CmpOp::Eq, CmpOp::Ne, CmpOp::Gt, CmpOp::Ge, CmpOp::Lt, CmpOp::Le];

    pub fn as_sql(self) -> &'static str {
        match self {
            CmpOp::Eq => "=",
            CmpOp::Ne => "<>",
            CmpOp::Gt => ">",
            CmpOp::Ge => ">=",
            CmpOp::Lt => "<",
            CmpOp::Le => "<=",
        }
    }

    pub fn inverse(self) -> CmpOp {
        match self {
            CmpOp::Eq => CmpOp::Ne,
            CmpOp::Ne => CmpOp::Eq,
            CmpOp::Gt => CmpOp::Le,
            CmpOp::Ge => CmpOp::Lt,
            CmpOp::Lt => CmpOp::Ge,
            CmpOp::Le => CmpOp::Gt,
        }
    }
}

/// Comparison operators available to a leaf term.
///
/// `Present` and `NotPresent` from the old model are deliberately absent: they
/// meant "the roster contains a row matching this", which is exactly
/// `Quant::Any` and `Quant::None`.
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
    /// IS NOT NULL. Distinguishes an unrecorded stat from a low one.
    IsSet,
    /// IS NULL.
    IsNotSet,
}

impl Op {
    pub const ALL: [Op; 13] = [
        Op::Contains,
        Op::Equals,
        Op::NotEquals,
        Op::Eq,
        Op::Ne,
        Op::Gt,
        Op::Ge,
        Op::Lt,
        Op::Le,
        Op::Is,
        Op::IsNot,
        Op::IsSet,
        Op::IsNotSet,
    ];

    /// The operator that negates this one, when one exists. Used so a pill can
    /// negate in place instead of gaining a `Not` wrapper.
    pub fn inverse(self) -> Option<Op> {
        match self {
            Op::Equals => Some(Op::NotEquals),
            Op::NotEquals => Some(Op::Equals),
            Op::Eq => Some(Op::Ne),
            Op::Ne => Some(Op::Eq),
            Op::Gt => Some(Op::Le),
            Op::Le => Some(Op::Gt),
            Op::Ge => Some(Op::Lt),
            Op::Lt => Some(Op::Ge),
            Op::Is => Some(Op::IsNot),
            Op::IsNot => Some(Op::Is),
            Op::IsSet => Some(Op::IsNotSet),
            Op::IsNotSet => Some(Op::IsSet),
            Op::Contains => None,
        }
    }

    /// True when the operator takes no right-hand operand.
    pub fn is_nullary(self) -> bool {
        matches!(self, Op::IsSet | Op::IsNotSet)
    }

    /// The token the grammar prints for this operator.
    pub fn as_token(self) -> &'static str {
        match self {
            Op::Contains => ":",
            Op::Equals | Op::Eq | Op::Is => "=",
            Op::NotEquals | Op::Ne | Op::IsNot => "!=",
            Op::Gt => ">",
            Op::Ge => ">=",
            Op::Lt => "<",
            Op::Le => "<=",
            Op::IsSet => "is-set",
            Op::IsNotSet => "is-not-set",
        }
    }

    /// The name this operator is stored under, one per variant.
    ///
    /// Distinct from `as_token`, which spells three variants `=` and three
    /// `!=`. A stored operator read back through the token would come back as
    /// whichever of those variants the reader guessed, and the wrong guess is
    /// an operator its field rejects.
    pub fn persist_name(self) -> &'static str {
        match self {
            Op::Contains => "contains",
            Op::Equals => "equals",
            Op::NotEquals => "not-equals",
            Op::Eq => "eq",
            Op::Ne => "ne",
            Op::Gt => "gt",
            Op::Ge => "ge",
            Op::Lt => "lt",
            Op::Le => "le",
            Op::Is => "is",
            Op::IsNot => "is-not",
            Op::IsSet => "is-set",
            Op::IsNotSet => "is-not-set",
        }
    }

    /// The operator `name` was written for, or `None` for a name this build
    /// does not know.
    pub fn from_persist_name(name: &str) -> Option<Op> {
        Op::ALL.into_iter().find(|op| op.persist_name() == name)
    }
}

/// The operator each field was last committed with, so a filter created for a
/// field starts on the comparison the user reached for last rather than on the
/// one its class defaults to.
///
/// Keyed by `MatchField::name`/`RosterField::name`, which
/// `every_field_name_is_unique_and_lowercase` pins as unique across both sets,
/// so one key space covers them.
///
/// Nothing reads an entry without `allowed_ops` in hand. `Op` spells equality
/// three ways and inequality three more, all printing as the same token, so a
/// term carrying an operator its field disallows prints and reparses into a
/// different tree with nothing at compile time to catch it. An entry written
/// before a field's operator set changed, or typed into the settings file by
/// hand, is exactly how one would arrive.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OperatorPreferences {
    by_field: std::collections::BTreeMap<String, Op>,
}

impl OperatorPreferences {
    /// The remembered operator for `field`, when it is one `allowed` still
    /// lists and one that takes a right-hand value.
    ///
    /// The `allowed` check is the invariant this whole type exists behind; see
    /// the type's own documentation. The nullary check is the other half: both
    /// callers hand the user a value to fill in straight after seeding, and
    /// `is-set` has no right-hand side to fill.
    pub fn preferred(&self, field: &str, allowed: &[Op]) -> Option<Op> {
        let op = *self.by_field.get(field)?;
        (allowed.contains(&op) && !op.is_nullary()).then_some(op)
    }

    /// Remembers `op` as what `field` was last committed with.
    ///
    /// A nullary operator is not remembered: `preferred` would refuse it
    /// anyway, and storing one would drop the last comparison the user picked
    /// that seeding can actually express.
    ///
    /// No `allowed_ops` check here on purpose. Callers pass an operator the
    /// tree already carries, and keeping the single validation at the read is
    /// what makes it cover entries this build never wrote.
    pub fn record(&mut self, field: &str, op: Op) {
        if op.is_nullary() {
            return;
        }
        self.by_field.insert(field.to_owned(), op);
    }

    pub fn is_empty(&self) -> bool {
        self.by_field.is_empty()
    }
}

impl serde::Serialize for OperatorPreferences {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_map(self.by_field.iter().map(|(field, op)| (field.as_str(), op.persist_name())))
    }
}

impl<'de> serde::Deserialize<'de> for OperatorPreferences {
    /// Drops an entry naming an operator this build does not know and keeps the
    /// rest, so a settings file written by a newer build, or edited by hand,
    /// still loads.
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let stored = std::collections::BTreeMap::<String, String>::deserialize(deserializer)?;
        let mut by_field = std::collections::BTreeMap::new();
        for (field, name) in stored {
            match Op::from_persist_name(&name) {
                Some(op) => {
                    by_field.insert(field, op);
                }
                None => tracing::warn!("dropping operator preference for {field:?}: unknown operator {name:?}"),
            }
        }
        Ok(OperatorPreferences { by_field })
    }
}

const TEXT_OPS: &[Op] = &[Op::Contains, Op::Equals, Op::NotEquals];
const NUM_OPS: &[Op] = &[Op::Eq, Op::Ne, Op::Gt, Op::Ge, Op::Lt, Op::Le];
const NUM_OPS_NULLABLE: &[Op] = &[Op::Eq, Op::Ne, Op::Gt, Op::Ge, Op::Lt, Op::Le, Op::IsSet, Op::IsNotSet];
const ENUM_OPS: &[Op] = &[Op::Is, Op::IsNot];
const ENUM_OPS_NULLABLE: &[Op] = &[Op::Is, Op::IsNot, Op::IsSet, Op::IsNotSet];
/// The rating bands are ordered, so a comparison against one is meaningful and
/// reads as a PR threshold: `rating >= unicum` is every player at or past the
/// Unicum floor. Equality stays the band's own half-open range.
const ORDERED_ENUM_OPS: &[Op] = &[Op::Is, Op::IsNot, Op::Gt, Op::Ge, Op::Lt, Op::Le];

/// Which `Value` variant a field expects. Drives both the parser's value
/// production and the widget's value editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    Text,
    Int,
    Float,
    Bool,
    Outcome,
    Relation,
    Division,
    Class,
    Ship,
    Account,
    Source,
    Timestamp,
    GameMode,
    /// A named personal-rating band. Not a stored column: it compiles to a
    /// range over `indexed_vehicle.pr`.
    Rating,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Text(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Outcome(MatchOutcome),
    Relation(VehicleRelation),
    Division(DivisionScope),
    Class(ShipClass),
    Ship(GameParamId),
    Account(AccountId),
    Source(SourceId),
    Timestamp(Timestamp),
    /// Always a recognised mode: the parser rejects an unrecognised token as a
    /// `BadValue`, the dropdown only ever offers a mode from `GameMode::ALL`,
    /// and the indexer (`replay_index.rs`) writes `game_mode_id` through
    /// `Recognized::known()`, so an unrecognised id cannot reach the database
    /// either. `GameMode::from_id`, at the decode boundary, is where
    /// `Recognized` belongs; carrying it here would model a state that can
    /// never occur.
    GameMode(GameMode),
    /// A personal-rating band, always one the parser or the dropdown produced
    /// from `PersonalRatingCategory::ALL`. It names a PR range rather than a
    /// stored value, which is what lets the filter run against replays indexed
    /// before it existed.
    Rating(PersonalRatingCategory),
    /// The operand-less companion to `Op::IsSet` and `Op::IsNotSet`. Named
    /// `NoOperand` rather than `None` so it is never misread as `Option::None`.
    NoOperand,
}

/// Which division a roster row belongs to, relative to the perspective player.
/// A raw `division_id` is meaningless across matches, so it is never a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DivisionScope {
    /// The same division as the perspective player.
    Mine,
    /// Any division at all.
    Any,
    /// No division: the player was solo.
    None,
}

impl DivisionScope {
    pub const ALL: [DivisionScope; 3] = [DivisionScope::Mine, DivisionScope::Any, DivisionScope::None];

    pub fn as_token(self) -> &'static str {
        match self {
            DivisionScope::Mine => "mine",
            DivisionScope::Any => "any",
            DivisionScope::None => "none",
        }
    }

    pub fn from_token(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "mine" | "my" => Some(DivisionScope::Mine),
            "any" | "yes" | "true" => Some(DivisionScope::Any),
            "none" | "no" | "solo" | "false" => Some(DivisionScope::None),
            _ => None,
        }
    }
}

/// The playable ship classes.
///
/// `indexed_vehicle.species` stores `format!("{s:?}")` of the `wowsunpack`
/// `Species` enum (see `replay_index.rs:96`), so `as_db_str` reproduces those
/// exact strings. This crate deliberately does not depend on `wowsunpack`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShipClass {
    AirCarrier,
    Battleship,
    Cruiser,
    Destroyer,
    Submarine,
    Auxiliary,
}

impl ShipClass {
    pub const ALL: [ShipClass; 6] = [
        ShipClass::AirCarrier,
        ShipClass::Battleship,
        ShipClass::Cruiser,
        ShipClass::Destroyer,
        ShipClass::Submarine,
        ShipClass::Auxiliary,
    ];

    pub fn as_db_str(self) -> &'static str {
        match self {
            ShipClass::AirCarrier => "AirCarrier",
            ShipClass::Battleship => "Battleship",
            ShipClass::Cruiser => "Cruiser",
            ShipClass::Destroyer => "Destroyer",
            ShipClass::Submarine => "Submarine",
            ShipClass::Auxiliary => "Auxiliary",
        }
    }

    pub fn from_db_str(s: &str) -> Option<Self> {
        ShipClass::ALL.into_iter().find(|c| c.as_db_str() == s)
    }

    /// The token the grammar accepts, lowercase, plus the common abbreviations.
    pub fn from_token(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "cv" | "carrier" | "aircarrier" => Some(ShipClass::AirCarrier),
            "bb" | "battleship" => Some(ShipClass::Battleship),
            "ca" | "cl" | "cruiser" => Some(ShipClass::Cruiser),
            "dd" | "destroyer" => Some(ShipClass::Destroyer),
            "ss" | "sub" | "submarine" => Some(ShipClass::Submarine),
            "aux" | "auxiliary" => Some(ShipClass::Auxiliary),
            _ => None,
        }
    }
}

/// Match-level fields, read from `indexed_match` and `replay_record`.
///
/// The `replay_record.self_*` columns are deliberately absent: they duplicate
/// the roster row where `relation = 'self'`, so filtering on the perspective
/// player goes through `any(relation:self and ...)` and every stat is
/// uniformly available for every subject.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchField {
    Map,
    GameType,
    GameMode,
    MatchGroup,
    Date,
    Build,
    Outcome,
    Group,
    ResultsAvailable,
}

impl MatchField {
    pub const ALL: [MatchField; 9] = [
        MatchField::Map,
        MatchField::GameType,
        MatchField::GameMode,
        MatchField::MatchGroup,
        MatchField::Date,
        MatchField::Build,
        MatchField::Outcome,
        MatchField::Group,
        MatchField::ResultsAvailable,
    ];

    pub fn name(self) -> &'static str {
        match self {
            MatchField::Map => "map",
            MatchField::GameType => "game-type",
            MatchField::GameMode => "game-mode",
            MatchField::MatchGroup => "match-group",
            MatchField::Date => "date",
            MatchField::Build => "build",
            MatchField::Outcome => "outcome",
            MatchField::Group => "group",
            MatchField::ResultsAvailable => "results-available",
        }
    }

    pub fn aliases(self) -> &'static [&'static str] {
        match self {
            MatchField::GameType => &["type"],
            MatchField::GameMode => &["mode"],
            MatchField::Date => &["when"],
            MatchField::Outcome => &["result"],
            MatchField::Group => &["source"],
            _ => &[],
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        let s = s.to_ascii_lowercase();
        MatchField::ALL.into_iter().find(|f| f.name() == s || f.aliases().contains(&s.as_str()))
    }

    pub fn value_kind(self) -> ValueKind {
        match self {
            MatchField::Map | MatchField::GameType | MatchField::MatchGroup => ValueKind::Text,
            MatchField::GameMode => ValueKind::GameMode,
            MatchField::Date => ValueKind::Timestamp,
            MatchField::Build => ValueKind::Int,
            MatchField::Outcome => ValueKind::Outcome,
            MatchField::Group => ValueKind::Source,
            MatchField::ResultsAvailable => ValueKind::Bool,
        }
    }

    pub fn allowed_ops(self) -> &'static [Op] {
        match self {
            MatchField::Map | MatchField::GameType | MatchField::MatchGroup => TEXT_OPS,
            MatchField::Date => NUM_OPS,
            // version_build is the one nullable match column.
            MatchField::Build => NUM_OPS_NULLABLE,
            MatchField::Outcome | MatchField::Group | MatchField::ResultsAvailable | MatchField::GameMode => ENUM_OPS,
        }
    }
}

/// Roster fields, read from `indexed_vehicle`. Every one already exists as a
/// column; see `migrations/005_replay_index.sql`, `006`, and `007`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RosterField {
    Relation,
    Division,
    Account,
    Name,
    Clan,
    Realm,
    Ship,
    ShipIndex,
    Nation,
    Class,
    Tier,
    TestShip,
    Damage,
    Kills,
    Spotting,
    Potential,
    Received,
    Pr,
    Rating,
    Survived,
    Disconnected,
    StreamSniper,
    SniperLogin,
}

impl RosterField {
    pub const ALL: [RosterField; 23] = [
        RosterField::Relation,
        RosterField::Division,
        RosterField::Account,
        RosterField::Name,
        RosterField::Clan,
        RosterField::Realm,
        RosterField::Ship,
        RosterField::ShipIndex,
        RosterField::Nation,
        RosterField::Class,
        RosterField::Tier,
        RosterField::TestShip,
        RosterField::Damage,
        RosterField::Kills,
        RosterField::Spotting,
        RosterField::Potential,
        RosterField::Received,
        RosterField::Pr,
        RosterField::Rating,
        RosterField::Survived,
        RosterField::Disconnected,
        RosterField::StreamSniper,
        RosterField::SniperLogin,
    ];

    pub fn name(self) -> &'static str {
        match self {
            RosterField::Relation => "relation",
            RosterField::Division => "division",
            RosterField::Account => "account",
            RosterField::Name => "name",
            RosterField::Clan => "clan",
            RosterField::Realm => "realm",
            RosterField::Ship => "ship",
            RosterField::ShipIndex => "ship-index",
            RosterField::Nation => "nation",
            RosterField::Class => "class",
            RosterField::Tier => "tier",
            RosterField::TestShip => "test-ship",
            RosterField::Damage => "damage",
            RosterField::Kills => "kills",
            RosterField::Spotting => "spotting",
            RosterField::Potential => "potential",
            RosterField::Received => "received",
            RosterField::Pr => "pr",
            RosterField::Rating => "rating",
            RosterField::Survived => "survived",
            RosterField::Disconnected => "disconnected",
            RosterField::StreamSniper => "stream-sniper",
            RosterField::SniperLogin => "sniper-login",
        }
    }

    pub fn aliases(self) -> &'static [&'static str] {
        match self {
            RosterField::Division => &["div"],
            RosterField::Name => &["player"],
            RosterField::Class => &["species"],
            RosterField::TestShip => &["test"],
            RosterField::Damage => &["dmg"],
            RosterField::Spotting => &["spot"],
            RosterField::Potential => &["pot"],
            RosterField::Received => &["tanked"],
            RosterField::StreamSniper => &["sniper"],
            _ => &[],
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        let s = s.to_ascii_lowercase();
        RosterField::ALL.into_iter().find(|f| f.name() == s || f.aliases().contains(&s.as_str()))
    }

    /// The `indexed_vehicle` column this field reads. Always chosen by a closed
    /// match, never interpolated from user text.
    pub fn column(self) -> &'static str {
        match self {
            RosterField::Relation => "relation",
            RosterField::Division => "division_id",
            RosterField::Account => "account_id",
            RosterField::Name => "player_name",
            RosterField::Clan => "clan",
            RosterField::Realm => "realm",
            RosterField::Ship => "ship_id",
            RosterField::ShipIndex => "ship_index",
            RosterField::Nation => "nation",
            RosterField::Class => "species",
            RosterField::Tier => "tier",
            RosterField::TestShip => "is_test_ship",
            RosterField::Damage => "damage",
            RosterField::Kills => "kills",
            RosterField::Spotting => "spotting",
            RosterField::Potential => "potential",
            RosterField::Received => "received",
            RosterField::Pr => "pr",
            // The band is a range over the same PR column, which is what lets
            // the filter reach history indexed before it existed.
            RosterField::Rating => "pr",
            RosterField::Survived => "survived",
            RosterField::Disconnected => "disconnected",
            RosterField::StreamSniper => "is_stream_sniper",
            RosterField::SniperLogin => "sniper_twitch_login",
        }
    }

    pub fn value_kind(self) -> ValueKind {
        match self {
            RosterField::Relation => ValueKind::Relation,
            RosterField::Division => ValueKind::Division,
            RosterField::Account => ValueKind::Account,
            RosterField::Name | RosterField::Clan | RosterField::Realm => ValueKind::Text,
            RosterField::Ship => ValueKind::Ship,
            RosterField::ShipIndex | RosterField::Nation | RosterField::SniperLogin => ValueKind::Text,
            RosterField::Class => ValueKind::Class,
            RosterField::Tier => ValueKind::Int,
            RosterField::TestShip | RosterField::Survived | RosterField::Disconnected | RosterField::StreamSniper => {
                ValueKind::Bool
            }
            RosterField::Pr => ValueKind::Float,
            RosterField::Rating => ValueKind::Rating,
            RosterField::Damage
            | RosterField::Kills
            | RosterField::Spotting
            | RosterField::Potential
            | RosterField::Received => ValueKind::Int,
        }
    }

    pub fn allowed_ops(self) -> &'static [Op] {
        match self {
            RosterField::Relation | RosterField::Division | RosterField::Class => ENUM_OPS,
            // NOT NULL in the schema.
            RosterField::TestShip => ENUM_OPS,
            RosterField::Tier => NUM_OPS,
            RosterField::Account | RosterField::Ship => ENUM_OPS,
            RosterField::Name | RosterField::Clan | RosterField::ShipIndex | RosterField::Nation => TEXT_OPS,
            // Nullable text columns.
            RosterField::Realm | RosterField::SniperLogin => {
                &[Op::Contains, Op::Equals, Op::NotEquals, Op::IsSet, Op::IsNotSet]
            }
            // Nullable stats: results may never have been written for the match.
            RosterField::Damage
            | RosterField::Kills
            | RosterField::Spotting
            | RosterField::Potential
            | RosterField::Received
            | RosterField::Pr => NUM_OPS_NULLABLE,
            RosterField::Rating => ORDERED_ENUM_OPS,
            RosterField::Survived | RosterField::Disconnected | RosterField::StreamSniper => ENUM_OPS_NULLABLE,
        }
    }
}

/// Display name to raw space name, used to resolve a `map` term typed as a
/// display name against the raw `indexed_match.map` column.
///
/// Built from loaded game data by the caller. Empty when no game data is
/// loaded, in which case a `map` term falls back to matching the raw name only.
#[derive(Debug, Clone, Default)]
pub struct MapCatalog {
    /// (raw space name, display name).
    entries: Vec<(String, String)>,
}

impl MapCatalog {
    /// A catalogue with no entries, usable in a `const` context so
    /// `CompileCtx::default` needs no allocation or `OnceLock`.
    pub const fn const_empty() -> Self {
        MapCatalog { entries: Vec::new() }
    }

    pub fn from_pairs(entries: Vec<(String, String)>) -> Self {
        Self { entries }
    }

    /// Raw space names whose display name contains `needle`, case-insensitively.
    /// The catalogue half of a `map:` term, which is a contains.
    pub fn raw_names_matching(&self, needle: &str) -> Vec<&str> {
        let needle = needle.to_ascii_lowercase();
        self.entries
            .iter()
            .filter(|(_, display)| display.to_ascii_lowercase().contains(&needle))
            .map(|(raw, _)| raw.as_str())
            .collect()
    }

    /// Raw space names whose display name is exactly `name`, case-insensitively.
    /// The catalogue half of a `map=` or `map!=` term: an equality resolved by
    /// substring would make every mapped name behave like a contains.
    pub fn raw_names_named(&self, name: &str) -> Vec<&str> {
        self.entries
            .iter()
            .filter(|(_, display)| display.eq_ignore_ascii_case(name))
            .map(|(raw, _)| raw.as_str())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A single `seen` set spans both loops below, so a name or alias reused by
    // any other MatchField or RosterField (including one equal to some other
    // field's canonical name) collides on insert. This is what would have
    // caught GameMode shipping with no aliases while GameType kept "mode".
    #[test]
    fn every_field_name_is_unique_and_lowercase() {
        let mut seen = std::collections::HashSet::new();
        for f in MatchField::ALL {
            for name in std::iter::once(f.name()).chain(f.aliases().iter().copied()) {
                assert_eq!(name, name.to_ascii_lowercase(), "field name must be lowercase: {name}");
                assert!(seen.insert(name), "duplicate field name: {name}");
            }
        }
        for f in RosterField::ALL {
            for name in std::iter::once(f.name()).chain(f.aliases().iter().copied()) {
                assert_eq!(name, name.to_ascii_lowercase(), "field name must be lowercase: {name}");
                assert!(seen.insert(name), "duplicate field name: {name}");
            }
        }
    }

    /// The reason preferences are stored by `persist_name` and not by
    /// `as_token`: three variants print as `=` and three as `!=`, so a name
    /// shared by two variants would read back as whichever one the reader
    /// happened to try first.
    #[test]
    fn every_operator_persist_name_is_distinct_and_reads_back() {
        let mut seen = std::collections::HashSet::new();
        for op in Op::ALL {
            assert!(seen.insert(op.persist_name()), "{op:?} shares a persist name");
            assert_eq!(Op::from_persist_name(op.persist_name()), Some(op));
        }
        assert_eq!(Op::from_persist_name("wat"), None);
    }

    /// The invariant the whole type exists behind. A remembered operator its
    /// field does not allow prints as a token the field's own `ValueKind` reads
    /// back as a different variant, so it has to be refused rather than seeded.
    #[test]
    fn a_remembered_operator_the_field_disallows_is_refused() {
        let mut prefs = OperatorPreferences::default();
        prefs.record(RosterField::Damage.name(), Op::Contains);
        assert!(
            !RosterField::Damage.allowed_ops().contains(&Op::Contains),
            "the fixture must store an operator the field really refuses"
        );
        assert_eq!(prefs.preferred(RosterField::Damage.name(), RosterField::Damage.allowed_ops()), None);

        // The positive control: the same map, the same field, an operator the
        // field does allow. Without it the assertion above passes for a map
        // that simply never stored anything.
        prefs.record(RosterField::Damage.name(), Op::Le);
        assert_eq!(prefs.preferred(RosterField::Damage.name(), RosterField::Damage.allowed_ops()), Some(Op::Le));
    }

    /// Both seeding paths hand the user a value to fill in straight after, and
    /// `is-set` has no right-hand side to fill. Storing one would also throw
    /// away the last comparison that seeding can express.
    #[test]
    fn a_nullary_operator_is_neither_stored_nor_offered() {
        let mut prefs = OperatorPreferences::default();
        prefs.record(RosterField::Damage.name(), Op::Le);
        prefs.record(RosterField::Damage.name(), Op::IsSet);
        assert!(
            RosterField::Damage.allowed_ops().contains(&Op::IsSet),
            "the field must really allow the nullary operator, or this proves nothing"
        );
        assert_eq!(
            prefs.preferred(RosterField::Damage.name(), RosterField::Damage.allowed_ops()),
            Some(Op::Le),
            "recording a nullary operator must leave the last usable one in place"
        );
    }

    #[test]
    fn preferences_survive_a_round_trip() {
        let mut prefs = OperatorPreferences::default();
        prefs.record(RosterField::Damage.name(), Op::Le);
        prefs.record(MatchField::Map.name(), Op::Equals);
        let json = serde_json::to_string(&prefs).expect("preferences serialize");
        assert_eq!(json, r#"{"damage":"le","map":"equals"}"#);
        assert_eq!(serde_json::from_str::<OperatorPreferences>(&json).expect("and read back"), prefs);
    }

    /// A settings file written by a build that knew an operator this one does
    /// not must still load. The unknown entry goes; everything beside it stays.
    #[test]
    fn an_unknown_operator_name_drops_only_its_own_entry() {
        let stored = r#"{"damage":"le","kills":"between","map":"equals"}"#;
        let prefs: OperatorPreferences = serde_json::from_str(stored).expect("the file still loads");
        assert_eq!(prefs.preferred(RosterField::Damage.name(), RosterField::Damage.allowed_ops()), Some(Op::Le));
        assert_eq!(prefs.preferred(MatchField::Map.name(), MatchField::Map.allowed_ops()), Some(Op::Equals));
        assert_eq!(prefs.preferred(RosterField::Kills.name(), RosterField::Kills.allowed_ops()), None);
    }

    // Regression: "mode" used to be aliased to GameType (BattleType: Random,
    // Ranked, Co-op, Clan, Brawl) while GameMode (Standard, Domination,
    // Epicenter, Arms Race) had no alias at all, so typing "mode" silently
    // landed on the wrong axis and fell back to free text.
    #[test]
    fn mode_alias_resolves_to_game_mode_not_game_type() {
        assert_eq!(MatchField::from_name("mode"), Some(MatchField::GameMode));
        assert_eq!(MatchField::from_name("type"), Some(MatchField::GameType));
    }

    #[test]
    fn field_lookup_round_trips_through_name_and_aliases() {
        for f in MatchField::ALL {
            assert_eq!(MatchField::from_name(f.name()), Some(f));
            for alias in f.aliases() {
                assert_eq!(MatchField::from_name(alias), Some(f));
            }
        }
        for f in RosterField::ALL {
            assert_eq!(RosterField::from_name(f.name()), Some(f));
            for alias in f.aliases() {
                assert_eq!(RosterField::from_name(alias), Some(f));
            }
        }
        assert_eq!(MatchField::from_name("nonsense"), None);
        assert_eq!(RosterField::from_name("nonsense"), None);
    }

    #[test]
    fn allowed_ops_are_never_empty_and_exclude_removed_variants() {
        for f in MatchField::ALL {
            assert!(!f.allowed_ops().is_empty(), "{:?} has no ops", f);
        }
        for f in RosterField::ALL {
            assert!(!f.allowed_ops().is_empty(), "{:?} has no ops", f);
        }
        // Presence is expressed by Quant::Any / Quant::None, not by an Op.
        let all_ops: Vec<Op> = MatchField::ALL
            .iter()
            .flat_map(|f| f.allowed_ops().iter().copied())
            .chain(RosterField::ALL.iter().flat_map(|f| f.allowed_ops().iter().copied()))
            .collect();
        assert!(all_ops.contains(&Op::IsSet));
        assert!(all_ops.contains(&Op::IsNotSet));
    }

    #[test]
    fn nullable_roster_fields_offer_is_set_and_non_nullable_do_not() {
        // damage is a nullable stat: results may never have been written.
        assert!(RosterField::Damage.allowed_ops().contains(&Op::IsSet));
        // tier is NOT NULL in the schema, so asking whether it is set is meaningless.
        assert!(!RosterField::Tier.allowed_ops().contains(&Op::IsSet));
    }

    #[test]
    fn op_inverse_is_an_involution_where_it_exists() {
        for op in Op::ALL {
            if let Some(inv) = op.inverse() {
                assert_eq!(inv.inverse(), Some(op), "{:?} inverse is not an involution", op);
                assert_ne!(inv, op, "{:?} is its own inverse", op);
            }
        }
        assert_eq!(Op::Ge.inverse(), Some(Op::Lt));
        assert_eq!(Op::Is.inverse(), Some(Op::IsNot));
        assert_eq!(Op::IsSet.inverse(), Some(Op::IsNotSet));
        assert_eq!(Op::Contains.inverse(), None);
    }

    #[test]
    fn quant_inverse_flips_any_and_none_and_inverts_count() {
        assert_eq!(Quant::Any.inverse(), Quant::None);
        assert_eq!(Quant::None.inverse(), Quant::Any);
        assert_eq!(Quant::Count(CmpOp::Ge, 3).inverse(), Quant::Count(CmpOp::Lt, 3));
    }

    #[test]
    fn ship_class_db_strings_match_what_the_indexer_writes() {
        // replay_index.rs:96 writes format!("{s:?}") of wowsunpack Species, so these
        // exact strings are the on-disk wire format and must not be renamed.
        assert_eq!(ShipClass::AirCarrier.as_db_str(), "AirCarrier");
        assert_eq!(ShipClass::Battleship.as_db_str(), "Battleship");
        assert_eq!(ShipClass::Cruiser.as_db_str(), "Cruiser");
        assert_eq!(ShipClass::Destroyer.as_db_str(), "Destroyer");
        assert_eq!(ShipClass::Submarine.as_db_str(), "Submarine");
        assert_eq!(ShipClass::Auxiliary.as_db_str(), "Auxiliary");
        for c in ShipClass::ALL {
            assert_eq!(ShipClass::from_db_str(c.as_db_str()), Some(c));
        }
        assert_eq!(ShipClass::from_db_str("Bomb"), None);
    }

    #[test]
    fn empty_all_matches_everything_and_empty_any_matches_nothing() {
        let all: MatchExpr = Expr::All(vec![]);
        let any: MatchExpr = Expr::Any(vec![]);
        assert!(all.is_empty_all());
        assert!(!any.is_empty_all());
    }

    #[test]
    fn children_descends_into_a_negation() {
        let leaf: MatchExpr = Expr::Leaf(MatchTerm::FreeText("yamato".into()));
        let negated = Expr::Not(Box::new(leaf.clone()));
        assert_eq!(negated.children(), std::slice::from_ref(&leaf));
        assert!(leaf.children().is_empty());
        assert_eq!(Expr::All(vec![negated.clone()]).children(), std::slice::from_ref(&negated));
    }

    #[test]
    fn map_catalogue_matches_display_names_case_insensitively() {
        let cat = MapCatalog::from_pairs(vec![
            ("spaces/28_naval_mission".into(), "Naval Mission".into()),
            ("spaces/13_OC_new_dawn".into(), "Ocean".into()),
        ]);
        assert_eq!(cat.raw_names_matching("ocean"), vec!["spaces/13_OC_new_dawn"]);
        assert_eq!(cat.raw_names_matching("NAVAL"), vec!["spaces/28_naval_mission"]);
        assert!(cat.raw_names_matching("nothing").is_empty());
        assert!(MapCatalog::default().raw_names_matching("ocean").is_empty());
    }

    #[test]
    fn map_catalogue_lookup_by_exact_name_does_not_match_a_longer_display_name() {
        let cat = MapCatalog::from_pairs(vec![
            ("spaces/13_OC_new_dawn".into(), "Ocean".into()),
            ("spaces/40_okinawa".into(), "Ocean Rift".into()),
        ]);
        assert_eq!(cat.raw_names_named("ocean"), vec!["spaces/13_OC_new_dawn"]);
        assert_eq!(cat.raw_names_named("OCEAN RIFT"), vec!["spaces/40_okinawa"]);
        assert!(cat.raw_names_named("oce").is_empty());
        assert_eq!(cat.raw_names_matching("ocean").len(), 2, "a contains still spans both");
    }
}
