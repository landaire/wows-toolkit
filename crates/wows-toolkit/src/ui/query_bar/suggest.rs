//! Suggestion sourcing and ranking for the query bar's dropdown.

// Consumed by later query-bar tasks (the dropdown widget, the DB-backed value
// editor); no call site in this crate yet.
#![allow(dead_code)]

use rust_i18n::t;

use crate::db::index::query_ast::CmpOp;
use crate::db::index::query_ast::DivisionScope;
use crate::db::index::query_ast::Expr;
use crate::db::index::query_ast::MatchExpr;
use crate::db::index::query_ast::MatchField;
use crate::db::index::query_ast::MatchTerm;
use crate::db::index::query_ast::Op;
use crate::db::index::query_ast::Quant;
use crate::db::index::query_ast::RosterField;
use crate::db::index::query_ast::RosterTerm;
use crate::db::index::query_ast::ShipClass;
use crate::db::index::query_ast::Value;
use crate::db::index::query_ast::ValueKind;
use crate::db::index::rows::VehicleRelation;
use crate::ui::query_bar::label::roster_field_label;

#[derive(Debug, Clone)]
pub struct Suggestion {
    /// Stable identifier, used for dedup and for tests.
    pub key: &'static str,
    pub label: String,
    /// Breadcrumb shown after the label. Not display text itself: a
    /// `SuggestionCategory` cannot carry a locale-dependent `t!()` result and
    /// stay `Copy`/`'static`, so the renderer translates it. Typed rather
    /// than a bare `&'static str` so the renderer's match is exhaustive: a
    /// future third category cannot silently render untranslated.
    pub context: SuggestionCategory,
    pub kind: SuggestionKind,
}

/// What kind of thing a suggestion's breadcrumb names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestionCategory {
    Preset,
    Roster,
}

#[derive(Debug, Clone)]
pub enum SuggestionKind {
    /// Commit a match-level field and open its value editor.
    MatchField(MatchField),
    /// Commit a roster field under a scope, and open its value editor.
    RosterField { field: RosterField, scope: Option<Scope> },
    /// Expand a named shape.
    Preset(&'static str),
    /// Commit the typed text as a free-text term.
    FreeText,
}

/// A scope prefix, matching the grammar's `self.` / `ally.` / `enemy.` / `div.`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    SelfPlayer,
    Ally,
    Enemy,
    Division,
    Anyone,
}

pub struct Preset {
    pub key: &'static str,
    /// English default text. Not what the dropdown shows: `static_suggestions`
    /// looks up a locale-specific label by `key` instead, since a `static`
    /// array cannot hold a runtime translation.
    pub label: &'static str,
    pub build: fn() -> MatchExpr,
}

fn preset_divmate_test_ship() -> MatchExpr {
    Expr::Leaf(MatchTerm::Roster {
        quant: Quant::Any,
        pred: Expr::All(vec![
            Expr::Leaf(RosterTerm {
                field: RosterField::Division,
                op: Op::Is,
                value: Value::Division(DivisionScope::Mine),
            }),
            Expr::Leaf(RosterTerm { field: RosterField::TestShip, op: Op::Is, value: Value::Bool(true) }),
        ]),
    })
}

fn preset_no_enemy_cv() -> MatchExpr {
    Expr::Leaf(MatchTerm::Roster {
        quant: Quant::None,
        pred: Expr::All(vec![
            Expr::Leaf(RosterTerm {
                field: RosterField::Relation,
                op: Op::Is,
                value: Value::Relation(VehicleRelation::Enemy),
            }),
            Expr::Leaf(RosterTerm {
                field: RosterField::Class,
                op: Op::Is,
                value: Value::Class(ShipClass::AirCarrier),
            }),
        ]),
    })
}

fn preset_all_enemies_died() -> MatchExpr {
    Expr::Leaf(MatchTerm::Roster {
        quant: Quant::None,
        pred: Expr::All(vec![
            Expr::Leaf(RosterTerm {
                field: RosterField::Relation,
                op: Op::Is,
                value: Value::Relation(VehicleRelation::Enemy),
            }),
            Expr::Leaf(RosterTerm { field: RosterField::Survived, op: Op::Is, value: Value::Bool(true) }),
        ]),
    })
}

fn preset_high_damage_enemies() -> MatchExpr {
    Expr::Leaf(MatchTerm::Roster {
        quant: Quant::Count(CmpOp::Ge, 3),
        pred: Expr::All(vec![
            Expr::Leaf(RosterTerm {
                field: RosterField::Relation,
                op: Op::Is,
                value: Value::Relation(VehicleRelation::Enemy),
            }),
            Expr::Leaf(RosterTerm { field: RosterField::Damage, op: Op::Gt, value: Value::Int(100_000) }),
        ]),
    })
}

fn preset_stream_sniper() -> MatchExpr {
    Expr::Leaf(MatchTerm::Roster {
        quant: Quant::Any,
        pred: Expr::Leaf(RosterTerm { field: RosterField::StreamSniper, op: Op::Is, value: Value::Bool(true) }),
    })
}

fn preset_i_survived() -> MatchExpr {
    Expr::Leaf(MatchTerm::Roster {
        quant: Quant::Any,
        pred: Expr::All(vec![
            Expr::Leaf(RosterTerm {
                field: RosterField::Relation,
                op: Op::Is,
                value: Value::Relation(VehicleRelation::SelfPlayer),
            }),
            Expr::Leaf(RosterTerm { field: RosterField::Survived, op: Op::Is, value: Value::Bool(true) }),
        ]),
    })
}

fn preset_i_disconnected() -> MatchExpr {
    Expr::Leaf(MatchTerm::Roster {
        quant: Quant::Any,
        pred: Expr::All(vec![
            Expr::Leaf(RosterTerm {
                field: RosterField::Relation,
                op: Op::Is,
                value: Value::Relation(VehicleRelation::SelfPlayer),
            }),
            Expr::Leaf(RosterTerm { field: RosterField::Disconnected, op: Op::Is, value: Value::Bool(true) }),
        ]),
    })
}

pub static PRESETS: &[Preset] = &[
    Preset {
        key: "divmate_test_ship",
        label: "Someone in my division played a test ship",
        build: preset_divmate_test_ship,
    },
    Preset { key: "no_enemy_cv", label: "No enemy carrier", build: preset_no_enemy_cv },
    Preset { key: "all_enemies_died", label: "Every enemy died", build: preset_all_enemies_died },
    Preset { key: "high_damage_enemies", label: "3 or more enemies over 100k", build: preset_high_damage_enemies },
    Preset { key: "stream_sniper", label: "Contains a stream sniper", build: preset_stream_sniper },
    Preset { key: "i_survived", label: "I survived", build: preset_i_survived },
    Preset { key: "i_disconnected", label: "I disconnected", build: preset_i_disconnected },
];

fn preset_translation_key(key: &str) -> &'static str {
    match key {
        "divmate_test_ship" => "ui.search.suggest.preset_divmate_test_ship",
        "no_enemy_cv" => "ui.search.suggest.preset_no_enemy_cv",
        "all_enemies_died" => "ui.search.suggest.preset_all_enemies_died",
        "high_damage_enemies" => "ui.search.suggest.preset_high_damage_enemies",
        "stream_sniper" => "ui.search.suggest.preset_stream_sniper",
        "i_survived" => "ui.search.suggest.preset_i_survived",
        "i_disconnected" => "ui.search.suggest.preset_i_disconnected",
        other => unreachable!("preset {other} has no translation key"),
    }
}

/// Named shortcuts whose scope is not mechanically derivable: a custom label
/// and a scope the "stat fields cross every scope, everything else gets
/// `Anyone`" rule would not produce on its own.
const EXPLICIT_FIELD_SHORTCUTS: &[(&str, RosterField, Scope, &str)] = &[
    ("my_damage", RosterField::Damage, Scope::SelfPlayer, "ui.search.suggest.my_damage"),
    ("enemy_ship", RosterField::Ship, Scope::Enemy, "ui.search.suggest.enemy_ship"),
    ("allied_ship", RosterField::Ship, Scope::Ally, "ui.search.suggest.allied_ship"),
    ("player_in_match", RosterField::Account, Scope::Anyone, "ui.search.suggest.player_in_match"),
    ("someone_in_division", RosterField::Name, Scope::Division, "ui.search.suggest.someone_in_division"),
];

/// The full `(stat field) x (scope)` cross product, minus `(Damage,
/// SelfPlayer)`, which `EXPLICIT_FIELD_SHORTCUTS` already covers as
/// `my_damage`.
const STAT_SHORTCUTS: &[(RosterField, Scope, &str)] = &[
    (RosterField::Damage, Scope::Ally, "stat.damage.ally"),
    (RosterField::Damage, Scope::Enemy, "stat.damage.enemy"),
    (RosterField::Damage, Scope::Division, "stat.damage.division"),
    (RosterField::Damage, Scope::Anyone, "stat.damage.anyone"),
    (RosterField::Kills, Scope::SelfPlayer, "stat.kills.self"),
    (RosterField::Kills, Scope::Ally, "stat.kills.ally"),
    (RosterField::Kills, Scope::Enemy, "stat.kills.enemy"),
    (RosterField::Kills, Scope::Division, "stat.kills.division"),
    (RosterField::Kills, Scope::Anyone, "stat.kills.anyone"),
    (RosterField::Spotting, Scope::SelfPlayer, "stat.spotting.self"),
    (RosterField::Spotting, Scope::Ally, "stat.spotting.ally"),
    (RosterField::Spotting, Scope::Enemy, "stat.spotting.enemy"),
    (RosterField::Spotting, Scope::Division, "stat.spotting.division"),
    (RosterField::Spotting, Scope::Anyone, "stat.spotting.anyone"),
    (RosterField::Potential, Scope::SelfPlayer, "stat.potential.self"),
    (RosterField::Potential, Scope::Ally, "stat.potential.ally"),
    (RosterField::Potential, Scope::Enemy, "stat.potential.enemy"),
    (RosterField::Potential, Scope::Division, "stat.potential.division"),
    (RosterField::Potential, Scope::Anyone, "stat.potential.anyone"),
    (RosterField::Received, Scope::SelfPlayer, "stat.received.self"),
    (RosterField::Received, Scope::Ally, "stat.received.ally"),
    (RosterField::Received, Scope::Enemy, "stat.received.enemy"),
    (RosterField::Received, Scope::Division, "stat.received.division"),
    (RosterField::Received, Scope::Anyone, "stat.received.anyone"),
    (RosterField::Pr, Scope::SelfPlayer, "stat.pr.self"),
    (RosterField::Pr, Scope::Ally, "stat.pr.ally"),
    (RosterField::Pr, Scope::Enemy, "stat.pr.enemy"),
    (RosterField::Pr, Scope::Division, "stat.pr.division"),
    (RosterField::Pr, Scope::Anyone, "stat.pr.anyone"),
];

/// Every roster field that is not a stat field and not already named by
/// `EXPLICIT_FIELD_SHORTCUTS`, offered under `Anyone`. `Account` is absent: it
/// is the `player_in_match` explicit entry, which is already `Anyone`-scoped.
const REMAINING_FIELDS: &[RosterField] = &[
    RosterField::Relation,
    RosterField::Division,
    RosterField::Name,
    RosterField::Clan,
    RosterField::Realm,
    RosterField::Ship,
    RosterField::ShipIndex,
    RosterField::Nation,
    RosterField::Class,
    RosterField::Tier,
    RosterField::TestShip,
    RosterField::Survived,
    RosterField::Disconnected,
    RosterField::StreamSniper,
    RosterField::SniperLogin,
];

fn scope_word(scope: Scope) -> std::borrow::Cow<'static, str> {
    match scope {
        Scope::SelfPlayer => t!("ui.search.suggest.scope_self"),
        Scope::Ally => t!("ui.search.suggest.scope_ally"),
        Scope::Enemy => t!("ui.search.suggest.scope_enemy"),
        Scope::Division => t!("ui.search.suggest.scope_division"),
        Scope::Anyone => t!("ui.search.suggest.scope_anyone"),
    }
}

/// Lowercases the first character so a scope word can lead the label
/// ("My damage") while the field name that follows reads as a continuation
/// of it rather than its own capitalized clause.
fn lowercase_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn composed_label(field: RosterField, scope: Scope) -> String {
    format!("{} {}", scope_word(scope), lowercase_first(&roster_field_label(field)))
}

/// Every suggestion the dropdown can show without a database round trip:
/// presets and roster-field shortcuts. Flattened with breadcrumbs rather than
/// cascading, so typing `dam` matches `My damage`, `Any player damage`, and
/// `Enemy damage` directly; drilling into a category is never required.
pub fn static_suggestions() -> Vec<Suggestion> {
    let mut out = Vec::with_capacity(
        PRESETS.len() + EXPLICIT_FIELD_SHORTCUTS.len() + STAT_SHORTCUTS.len() + REMAINING_FIELDS.len(),
    );

    for p in PRESETS {
        out.push(Suggestion {
            key: p.key,
            label: t!(preset_translation_key(p.key)).into_owned(),
            context: SuggestionCategory::Preset,
            kind: SuggestionKind::Preset(p.key),
        });
    }

    for &(key, field, scope, label_key) in EXPLICIT_FIELD_SHORTCUTS {
        out.push(Suggestion {
            key,
            label: t!(label_key).into_owned(),
            context: SuggestionCategory::Roster,
            kind: SuggestionKind::RosterField { field, scope: Some(scope) },
        });
    }

    for &(field, scope, key) in STAT_SHORTCUTS {
        out.push(Suggestion {
            key,
            label: composed_label(field, scope),
            context: SuggestionCategory::Roster,
            kind: SuggestionKind::RosterField { field, scope: Some(scope) },
        });
    }

    for &field in REMAINING_FIELDS {
        out.push(Suggestion {
            key: field.name(),
            label: composed_label(field, Scope::Anyone),
            context: SuggestionCategory::Roster,
            kind: SuggestionKind::RosterField { field, scope: Some(Scope::Anyone) },
        });
    }

    out
}

/// Indices into `all`, best first. Empty needle keeps declaration order.
///
/// Case-insensitive; a prefix match outranks a word-boundary match, which
/// outranks a plain substring match; a label with none of the three is
/// dropped. Ties keep declaration order, which `Vec::sort_by_key`'s
/// stability gives for free since candidates are scored in that order.
pub fn rank(needle: &str, all: &[Suggestion]) -> Vec<usize> {
    let needle = needle.trim();
    if needle.is_empty() {
        return (0..all.len()).collect();
    }
    let needle = needle.to_ascii_lowercase();
    let mut scored: Vec<(usize, u8)> =
        all.iter().enumerate().filter_map(|(i, s)| match_tier(&s.label, &needle).map(|tier| (i, tier))).collect();
    scored.sort_by_key(|&(_, tier)| tier);
    scored.into_iter().map(|(i, _)| i).collect()
}

/// 0 = the label starts with the needle; 1 = some word in the label does; 2 =
/// the needle appears somewhere else in the label; `None` = no match at all.
fn match_tier(label: &str, needle_lower: &str) -> Option<u8> {
    let label_lower = label.to_ascii_lowercase();
    if label_lower.starts_with(needle_lower) {
        return Some(0);
    }
    if word_boundary_match(&label_lower, needle_lower) {
        return Some(1);
    }
    if label_lower.contains(needle_lower) {
        return Some(2);
    }
    None
}

fn word_boundary_match(label_lower: &str, needle_lower: &str) -> bool {
    label_lower.split(|c: char| !c.is_alphanumeric()).any(|word| word.starts_with(needle_lower))
}

/// What the bar needs the Search tab to fetch for the value editor it is
/// showing. The tab services these on the tokio runtime, debounced.
#[derive(Debug, Clone, PartialEq)]
pub enum ValueRequest {
    Players { needle: String },
    Ships { needle: String },
    Sources,
    Maps,
}

/// Characters that end a field name and begin its value in the grammar.
const OPERATOR_CHARS: [char; 5] = [':', '=', '!', '<', '>'];

/// The value lookup the caret's text calls for, read off its last
/// whitespace-separated fragment: `enemy.ship:yam` asks for ships matching
/// `yam`. `None` while the fragment names no field, or names one whose values
/// do not come from the database, in which case the dropdown keeps showing
/// static suggestions.
pub fn value_request_for(pending: &str) -> Option<ValueRequest> {
    let (key, needle) = split_on_operator(active_fragment(pending))?;
    let bare = key.rsplit('.').next()?;
    if bare.is_empty() {
        return None;
    }
    if let Some(field) = RosterField::from_name(bare) {
        return request_for_kind(field.value_kind(), needle);
    }
    let field = MatchField::from_name(bare)?;
    // `map` reads as text but its values are display names the caller resolves
    // against loaded game data, so it has its own request rather than none.
    if field == MatchField::Map {
        return Some(ValueRequest::Maps);
    }
    request_for_kind(field.value_kind(), needle)
}

/// Splits `<key><op><value>` at the first operator character. `None` when the
/// fragment carries no operator, which means the user is still typing a field
/// name and no value lookup applies yet.
fn split_on_operator(fragment: &str) -> Option<(&str, &str)> {
    let at = fragment.char_indices().find(|(_, c)| OPERATOR_CHARS.contains(c))?.0;
    let (key, rest) = fragment.split_at(at);
    Some((key, rest.trim_start_matches(OPERATOR_CHARS)))
}

/// Where the caret's active fragment begins: just past the last whitespace, or
/// the start of the text when there is none.
fn active_fragment_start(pending: &str) -> usize {
    pending.char_indices().rev().find(|(_, c)| c.is_whitespace()).map_or(0, |(i, c)| i + c.len_utf8())
}

/// The part of the caret's text the user is currently typing: everything after
/// the last whitespace, which is empty when the caret sits just past a space
/// and a new term is about to begin.
///
/// The one place that decides what "currently typing" means. Everything that
/// asks the question goes through it -- which value to look up, which
/// suggestions to rank, and what a committed row replaces -- so the four cannot
/// disagree about where the term under the caret starts.
pub fn active_fragment(pending: &str) -> &str {
    let (_, fragment) = pending.split_at(active_fragment_start(pending));
    fragment
}

/// The caret's text with the *value* of its active fragment replaced.
/// `outcome:win enemy.ship:yam` plus `1234` gives
/// `outcome:win enemy.ship:1234`: the earlier fragments and the field's own
/// key and operator survive, and the half-typed value does not. A fragment
/// with no operator yet is replaced whole, which is what a bare word wants.
pub fn replace_active_value(pending: &str, value: &str) -> String {
    let start = active_fragment_start(pending);
    let (head, fragment) = pending.split_at(start);
    // `split_on_operator` yields the value as a suffix of the fragment, so the
    // difference in length is exactly the key plus its operator run.
    let keep = match split_on_operator(fragment) {
        Some((_, existing)) => fragment.len() - existing.len(),
        None => 0,
    };
    let (prefix, _) = fragment.split_at(keep);
    format!("{head}{prefix}{value}")
}

/// The caret's text with its active fragment replaced outright. Committing a
/// field suggestion part way through a query must not discard what the user
/// already typed before it.
pub fn replace_active_fragment(pending: &str, replacement: &str) -> String {
    let (head, _) = pending.split_at(active_fragment_start(pending));
    format!("{head}{replacement}")
}

/// The grammar text a match-level field suggestion puts in the caret, ready
/// for a value to be typed after it.
pub fn match_field_prefix(field: MatchField) -> Option<String> {
    Some(format!("{}{}", field.name(), leading_op(field.allowed_ops())?.as_token()))
}

/// The grammar text a roster-field suggestion puts in the caret. A roster
/// field name is only reachable through a scope prefix or a quantifier, so the
/// scope is not optional here.
pub fn roster_field_prefix(field: RosterField, scope: Scope) -> Option<String> {
    Some(format!("{}{}{}", scope_token(scope), field.name(), leading_op(field.allowed_ops())?.as_token()))
}

/// The operator a field suggestion commits with. Taken from `allowed_ops` and
/// never hand-picked: three `Op` variants print as `=` and the wrong one
/// yields text that reparses into a different term. A nullary operator is
/// skipped because the caret is about to be handed a value to type.
fn leading_op(allowed: &[Op]) -> Option<Op> {
    allowed.iter().copied().find(|op| !op.is_nullary()).or_else(|| allowed.first().copied())
}

/// The grammar's prefix for a scope. Lives beside `Scope` so the two cannot
/// drift apart.
pub fn scope_token(scope: Scope) -> &'static str {
    match scope {
        Scope::SelfPlayer => "self.",
        Scope::Ally => "ally.",
        Scope::Enemy => "enemy.",
        Scope::Division => "div.",
        Scope::Anyone => "anyone.",
    }
}

fn request_for_kind(kind: ValueKind, needle: &str) -> Option<ValueRequest> {
    match kind {
        ValueKind::Ship => Some(ValueRequest::Ships { needle: needle.to_owned() }),
        ValueKind::Account => Some(ValueRequest::Players { needle: needle.to_owned() }),
        ValueKind::Source => Some(ValueRequest::Sources),
        ValueKind::Text
        | ValueKind::Int
        | ValueKind::Float
        | ValueKind::Bool
        | ValueKind::Outcome
        | ValueKind::Relation
        | ValueKind::Division
        | ValueKind::Class
        | ValueKind::Timestamp => None,
    }
}

/// One row of the value editor: a database-resolved choice for the field the
/// caret is typing a value for. The Search tab supplies both halves because the
/// grammar takes an id for a ship, an account, or a source while the user needs
/// to read a name.
#[derive(Debug, Clone, PartialEq)]
pub struct ValueOption {
    /// What the row shows.
    pub label: String,
    /// The literal appended to the caret's text when the row is picked, already
    /// quoted if the grammar needs it quoted.
    pub token: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_needle_returns_everything_in_declaration_order() {
        let all = static_suggestions();
        let order = rank("", &all);
        assert_eq!(order.len(), all.len());
        assert_eq!(order, (0..all.len()).collect::<Vec<_>>());
    }

    #[test]
    fn a_prefix_match_outranks_a_mid_word_match() {
        let all = static_suggestions();
        let order = rank("dam", &all);
        assert!(!order.is_empty());
        let first = &all[order[0]];
        assert!(
            first.label.to_lowercase().starts_with("dam") || first.label.to_lowercase().contains("damage"),
            "got {}",
            first.label
        );
    }

    /// Every damage label in the real static set is scope-prefixed ("My
    /// damage", "Ally damage", ...), so `a_prefix_match_outranks_a_mid_word_match`
    /// can never see a true tier-0 hit: no label starts with "dam", so its
    /// disjunct on "starts_with" never fires, and the test degrades to "a
    /// damage suggestion is first" -- true even if word-boundary and
    /// substring were scored the wrong way round. This test builds synthetic
    /// suggestions that each hit exactly one tier for the same needle and
    /// asserts the full order, so it fails if any two tiers are out of
    /// order or collapsed together.
    #[test]
    fn tiers_rank_prefix_above_word_boundary_above_substring() {
        let suggestions = vec![
            synthetic("substring", "Dislocation report"),
            synthetic("word_boundary", "Ready the catapult"),
            synthetic("prefix", "Catapult launch"),
        ];
        let order = rank("cat", &suggestions);
        assert_eq!(order, vec![2, 1, 0], "expected prefix, then word-boundary, then substring");
    }

    fn synthetic(key: &'static str, label: &str) -> Suggestion {
        Suggestion {
            key,
            label: label.to_string(),
            context: SuggestionCategory::Roster,
            kind: SuggestionKind::FreeText,
        }
    }

    #[test]
    fn ranking_is_case_insensitive() {
        let all = static_suggestions();
        assert_eq!(rank("DAM", &all), rank("dam", &all));
    }

    #[test]
    fn a_needle_matching_nothing_returns_nothing() {
        let all = static_suggestions();
        assert!(rank("zzzznotathing", &all).is_empty());
    }

    #[test]
    fn every_static_suggestion_has_a_non_empty_label_and_a_unique_key() {
        let all = static_suggestions();
        let mut seen = std::collections::HashSet::new();
        for s in &all {
            assert!(!s.label.trim().is_empty());
            assert!(seen.insert(s.key), "duplicate suggestion key: {}", s.key);
        }
    }

    #[test]
    fn every_preset_expands_to_a_tree_that_round_trips() {
        use crate::db::index::query_text::parse_query;
        use crate::db::index::query_text::print_query;
        for p in PRESETS {
            let expr = (p.build)();
            let printed = print_query(&expr);
            let reparsed =
                parse_query(&printed).unwrap_or_else(|e| panic!("preset {} printed {printed:?}: {e}", p.key));
            assert_eq!(reparsed, expr, "preset {} did not round trip: {printed}", p.key);
        }
    }

    #[test]
    fn every_preset_uses_an_operator_its_field_allows() {
        // Picking an op outside allowed_ops produces a tree that prints and
        // reparses into a different one.
        for p in PRESETS {
            assert_ops_allowed(&(p.build)(), p.key);
        }
    }

    #[test]
    fn the_division_test_ship_preset_expands_to_the_documented_shape() {
        let p = PRESETS.iter().find(|p| p.key == "divmate_test_ship").expect("the preset");
        let printed = crate::db::index::query_text::print_query(&(p.build)());
        assert_eq!(printed, "div.test-ship=true");
    }

    #[test]
    fn no_preset_carries_a_placeholder_value() {
        // A preset is a complete tree. A shortcut that needs the user to pick a
        // value is a RosterField suggestion, so no zero-id stands in for one.
        // Vacuous today: no shipped preset builds a Value::Ship or
        // Value::Account. It is a forward guard for the day one does.
        for p in PRESETS {
            assert_no_placeholder_ids(&(p.build)(), p.key);
        }
    }

    #[test]
    fn a_scoped_ship_fragment_asks_for_ships_matching_what_follows_the_operator() {
        assert_eq!(value_request_for("enemy.ship:yam"), Some(ValueRequest::Ships { needle: "yam".into() }));
        assert_eq!(value_request_for("ship="), Some(ValueRequest::Ships { needle: String::new() }));
    }

    #[test]
    fn only_the_last_fragment_decides_the_request() {
        assert_eq!(value_request_for("outcome:win enemy.ship:yam"), Some(ValueRequest::Ships { needle: "yam".into() }));
    }

    #[test]
    fn account_source_and_map_fields_each_have_their_own_request() {
        assert_eq!(value_request_for("account:12"), Some(ValueRequest::Players { needle: "12".into() }));
        assert_eq!(value_request_for("group:"), Some(ValueRequest::Sources));
        assert_eq!(value_request_for("map:oce"), Some(ValueRequest::Maps));
        // `source` is an alias of `group`, so it must resolve the same way.
        assert_eq!(value_request_for("source:"), Some(ValueRequest::Sources));
    }

    #[test]
    fn a_finished_fragment_stops_asking_once_a_space_follows_it() {
        // The user has moved on to a new term, so ship rows for the previous
        // one are stale. This is what makes `value_request_for` agree with the
        // replacements and with what the dropdown ranks: all four read the
        // active fragment, which a trailing space makes empty.
        assert_eq!(active_fragment("enemy.ship:1234 "), "");
        assert_eq!(value_request_for("enemy.ship:1234 "), None);
        assert_eq!(value_request_for("enemy.ship:1234"), Some(ValueRequest::Ships { needle: "1234".into() }));
    }

    #[test]
    fn the_active_fragment_is_what_every_caller_reads() {
        for (pending, expected) in
            [("", ""), ("ene", "ene"), ("outcome:win ene", "ene"), ("outcome:win ", ""), ("\u{4e2d}\u{e9} ou", "ou")]
        {
            let fragment = active_fragment(pending);
            assert_eq!(fragment, expected, "{pending:?}");
            // The replacements are defined in terms of the same split, so
            // swapping the fragment for itself is the identity.
            assert_eq!(replace_active_fragment(pending, fragment), pending, "{pending:?}");
        }
    }

    #[test]
    fn a_field_whose_values_are_not_in_the_database_asks_for_nothing() {
        assert_eq!(value_request_for("tier>=8"), None);
        assert_eq!(value_request_for("relation:enemy"), None);
        assert_eq!(value_request_for("name:blah"), None);
    }

    #[test]
    fn text_with_no_operator_or_no_field_asks_for_nothing() {
        assert_eq!(value_request_for(""), None);
        assert_eq!(value_request_for("win"), None);
        assert_eq!(value_request_for("enemy.shi"), None);
        assert_eq!(value_request_for(":"), None);
        assert_eq!(value_request_for("nonsense:x"), None);
    }

    #[test]
    fn a_multi_byte_needle_splits_on_a_character_boundary() {
        // The bar re-reads this on every keystroke, so a byte-index split would
        // be a reachable panic rather than a theoretical one.
        assert_eq!(
            value_request_for("enemy.ship:\u{e9}\u{4e2d}"),
            Some(ValueRequest::Ships { needle: "\u{e9}\u{4e2d}".into() })
        );
        assert_eq!(value_request_for("\u{e9}\u{4e2d}:x"), None);
    }

    #[test]
    fn a_multi_byte_field_name_does_not_panic_on_the_operator_split() {
        for input in ["\u{e9}", "\u{e9}:", ":\u{e9}", "a\u{4e2d}b>=1", "enemy.\u{e9}:x"] {
            let _ = value_request_for(input);
        }
    }

    use crate::db::index::query_text::parse_query;

    const SCOPES: [Scope; 5] = [Scope::SelfPlayer, Scope::Ally, Scope::Enemy, Scope::Division, Scope::Anyone];

    /// A literal the grammar reads for each value kind, so an emitted field
    /// prefix can be completed into a term and parsed.
    fn sample_literal(kind: ValueKind) -> &'static str {
        match kind {
            ValueKind::Text => "x",
            ValueKind::Int => "1",
            ValueKind::Float => "1.5",
            ValueKind::Bool => "true",
            ValueKind::Outcome => "win",
            ValueKind::Relation => "enemy",
            ValueKind::Division => "mine",
            ValueKind::Class => "dd",
            ValueKind::Ship | ValueKind::Account | ValueKind::Source => "1",
            ValueKind::Timestamp => "2024-01-01",
        }
    }

    #[test]
    fn replacing_a_value_keeps_the_field_and_drops_the_half_typed_value() {
        assert_eq!(replace_active_value("enemy.ship:yam", "1234"), "enemy.ship:1234");
        assert_eq!(replace_active_value("enemy.ship:", "1234"), "enemy.ship:1234");
        assert_eq!(replace_active_value("tier>=8", "10"), "tier>=10");
        assert_eq!(replace_active_value("", "1234"), "1234");
    }

    #[test]
    fn replacing_a_value_keeps_every_earlier_fragment() {
        // The defect this pins: appending to the whole caret string produced
        // `enemy.ship:yamYamato`, which does not parse.
        assert_eq!(replace_active_value("outcome:win enemy.ship:yam", "1234"), "outcome:win enemy.ship:1234");
    }

    #[test]
    fn a_replaced_value_leaves_text_the_grammar_reads_back() {
        for (pending, value) in [
            ("enemy.ship:yam", "1234"),
            ("outcome:win enemy.ship:yam", "1234"),
            ("group:", "3"),
            // A roster field carries its scope prefix at the match level;
            // that is the form `roster_field_prefix` emits.
            ("outcome:win anyone.account:9", "12345"),
        ] {
            let text = replace_active_value(pending, value);
            parse_query(&text).unwrap_or_else(|err| panic!("{pending:?} + {value:?} gave {text:?}: {err}"));
        }
    }

    #[test]
    fn a_replaced_value_yields_the_tree_the_caller_meant() {
        let text = replace_active_value("outcome:win enemy.ship:yam", "1234");
        let parsed = parse_query(&text).expect("the completed text parses");
        let expected = parse_query("outcome:win enemy.ship:1234").expect("the reference text parses");
        assert_eq!(parsed, expected, "got {text:?}");
    }

    #[test]
    fn replacing_a_fragment_keeps_every_earlier_fragment() {
        // The defect this pins: assigning the prefix over the whole caret
        // string threw away `outcome:win`.
        assert_eq!(replace_active_fragment("outcome:win ene", "enemy.ship:"), "outcome:win enemy.ship:");
        assert_eq!(replace_active_fragment("", "outcome="), "outcome=");
        assert_eq!(replace_active_fragment("dam", "self.damage>="), "self.damage>=");
    }

    #[test]
    fn a_multi_byte_fragment_is_replaced_on_a_character_boundary() {
        // The bar rewrites this on a keystroke, so a byte-index split would be
        // a reachable panic rather than a theoretical one.
        assert_eq!(replace_active_value("enemy.ship:\u{e9}\u{4e2d}", "1234"), "enemy.ship:1234");
        assert_eq!(replace_active_value("map:\u{e9} enemy.ship:\u{4e2d}", "1"), "map:\u{e9} enemy.ship:1");
        assert_eq!(replace_active_fragment("\u{4e2d}\u{e9} ou", "outcome="), "\u{4e2d}\u{e9} outcome=");
    }

    #[test]
    fn every_match_field_prefix_completes_into_the_term_it_names() {
        for field in MatchField::ALL {
            let prefix = match_field_prefix(field).expect("every field has an operator");
            let expected_op = leading_op(field.allowed_ops()).expect("every field has an operator");
            let text = format!("{prefix}{}", sample_literal(field.value_kind()));
            let parsed = parse_query(&text).unwrap_or_else(|err| panic!("{field:?} emitted {text:?}: {err}"));
            match &parsed {
                Expr::Leaf(MatchTerm::Field(got_field, got_op, _)) => {
                    assert_eq!(*got_field, field, "{text:?}");
                    assert_eq!(*got_op, expected_op, "{text:?} reparsed with a different operator");
                }
                other => panic!("{field:?} emitted {text:?}, which parsed as {other:?}"),
            }
        }
    }

    #[test]
    fn every_roster_field_prefix_completes_into_the_term_it_names_under_every_scope() {
        for field in RosterField::ALL {
            for scope in SCOPES {
                let prefix = roster_field_prefix(field, scope).expect("every field has an operator");
                let expected_op = leading_op(field.allowed_ops()).expect("every field has an operator");
                let text = format!("{prefix}{}", sample_literal(field.value_kind()));
                let parsed =
                    parse_query(&text).unwrap_or_else(|err| panic!("{field:?} under {scope:?} gave {text:?}: {err}"));
                let Expr::Leaf(MatchTerm::Roster { quant, pred }) = &parsed else {
                    panic!("{field:?} under {scope:?} emitted {text:?}, which parsed as {parsed:?}");
                };
                assert_eq!(*quant, Quant::Any, "a scope prefix is an existential: {text:?}");
                let term = sole_roster_term(pred)
                    .unwrap_or_else(|| panic!("{field:?} under {scope:?} gave {text:?}, pred {pred:?}"));
                assert_eq!(term.field, field, "{text:?}");
                assert_eq!(term.op, expected_op, "{text:?} reparsed with a different operator");
            }
        }
    }

    /// The field term inside a scoped predicate, whether the scope contributed
    /// a conjunct (`self.` and friends) or not (`anyone.`).
    fn sole_roster_term(pred: &RosterExpr) -> Option<&RosterTerm> {
        match pred {
            Expr::Leaf(term) => Some(term),
            Expr::All(cs) if cs.len() == 2 => match &cs[1] {
                Expr::Leaf(term) => Some(term),
                _ => None,
            },
            _ => None,
        }
    }

    #[test]
    fn every_emitted_prefix_round_trips_once_completed() {
        use crate::db::index::query_text::print_query;
        for field in MatchField::ALL {
            let text =
                format!("{}{}", match_field_prefix(field).expect("an operator"), sample_literal(field.value_kind()));
            let parsed = parse_query(&text).expect("the completed text parses");
            assert_eq!(parse_query(&print_query(&parsed)).ok().as_ref(), Some(&parsed), "{text:?}");
        }
        for field in RosterField::ALL {
            for scope in SCOPES {
                let text = format!(
                    "{}{}",
                    roster_field_prefix(field, scope).expect("an operator"),
                    sample_literal(field.value_kind())
                );
                let parsed = parse_query(&text).expect("the completed text parses");
                assert_eq!(parse_query(&print_query(&parsed)).ok().as_ref(), Some(&parsed), "{text:?}");
            }
        }
    }

    #[test]
    fn every_scope_token_is_one_the_grammar_recognises() {
        for scope in SCOPES {
            let text = format!("{}tier=10", scope_token(scope));
            parse_query(&text).unwrap_or_else(|err| panic!("{scope:?} emits {text:?}, which does not parse: {err}"));
        }
    }

    use crate::db::index::query_ast::RosterExpr;

    /// Walks a `MatchExpr`, asserting every `Field`/`RosterTerm` op it carries is
    /// in that field's `allowed_ops()`. `Op` has three near-synonymous equals
    /// variants and three not-equals, all rendering to the same token, so a term
    /// built with the wrong one prints and reparses into a different tree with
    /// no compile-time signal.
    fn assert_ops_allowed(expr: &MatchExpr, preset_key: &str) {
        match expr {
            Expr::Leaf(MatchTerm::Field(field, op, _)) => {
                assert!(field.allowed_ops().contains(op), "preset {preset_key}: {field:?} does not allow {op:?}");
            }
            Expr::Leaf(MatchTerm::Roster { pred, .. }) => assert_roster_ops_allowed(pred, preset_key),
            Expr::Leaf(MatchTerm::FreeText(_)) => {}
            Expr::Not(inner) => assert_ops_allowed(inner, preset_key),
            Expr::All(cs) | Expr::Any(cs) => cs.iter().for_each(|c| assert_ops_allowed(c, preset_key)),
        }
    }

    fn assert_roster_ops_allowed(expr: &RosterExpr, preset_key: &str) {
        match expr {
            Expr::Leaf(RosterTerm { field, op, .. }) => {
                assert!(
                    field.allowed_ops().contains(op),
                    "preset {preset_key}: roster field {field:?} does not allow {op:?}"
                );
            }
            Expr::Not(inner) => assert_roster_ops_allowed(inner, preset_key),
            Expr::All(cs) | Expr::Any(cs) => cs.iter().for_each(|c| assert_roster_ops_allowed(c, preset_key)),
        }
    }

    /// Walks a `MatchExpr`, asserting no `Value::Ship`/`Value::Account` carries a
    /// zero id. A preset is a complete tree, never a tree with a hole in it, so a
    /// zero id standing in for "not chosen yet" is exactly the sentinel the
    /// constraints forbid.
    fn assert_no_placeholder_ids(expr: &MatchExpr, preset_key: &str) {
        match expr {
            Expr::Leaf(MatchTerm::Field(_, _, value)) => assert_value_not_placeholder(value, preset_key),
            Expr::Leaf(MatchTerm::Roster { pred, .. }) => assert_roster_no_placeholder_ids(pred, preset_key),
            Expr::Leaf(MatchTerm::FreeText(_)) => {}
            Expr::Not(inner) => assert_no_placeholder_ids(inner, preset_key),
            Expr::All(cs) | Expr::Any(cs) => cs.iter().for_each(|c| assert_no_placeholder_ids(c, preset_key)),
        }
    }

    fn assert_roster_no_placeholder_ids(expr: &RosterExpr, preset_key: &str) {
        match expr {
            Expr::Leaf(RosterTerm { value, .. }) => assert_value_not_placeholder(value, preset_key),
            Expr::Not(inner) => assert_roster_no_placeholder_ids(inner, preset_key),
            Expr::All(cs) | Expr::Any(cs) => cs.iter().for_each(|c| assert_roster_no_placeholder_ids(c, preset_key)),
        }
    }

    fn assert_value_not_placeholder(value: &Value, preset_key: &str) {
        match value {
            Value::Ship(id) => assert_ne!(id.raw(), 0, "preset {preset_key} carries a placeholder ship id"),
            Value::Account(a) => assert_ne!(a.raw(), 0, "preset {preset_key} carries a placeholder account id"),
            _ => {}
        }
    }
}
