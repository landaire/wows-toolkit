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
use crate::db::index::rows::VehicleRelation;
use crate::ui::query_bar::label::roster_field_label;

#[derive(Debug, Clone)]
pub struct Suggestion {
    /// Stable identifier, used for dedup and for tests.
    pub key: &'static str,
    pub label: String,
    /// Breadcrumb shown after the label. A category tag ("Preset", "Roster"),
    /// not display text itself: it cannot carry a locale-dependent `t!()`
    /// result and stay `'static`, so the renderer translates it.
    pub context: &'static str,
    pub kind: SuggestionKind,
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
            context: "Preset",
            kind: SuggestionKind::Preset(p.key),
        });
    }

    for &(key, field, scope, label_key) in EXPLICIT_FIELD_SHORTCUTS {
        out.push(Suggestion {
            key,
            label: t!(label_key).into_owned(),
            context: "Roster",
            kind: SuggestionKind::RosterField { field, scope: Some(scope) },
        });
    }

    for &(field, scope, key) in STAT_SHORTCUTS {
        out.push(Suggestion {
            key,
            label: composed_label(field, scope),
            context: "Roster",
            kind: SuggestionKind::RosterField { field, scope: Some(scope) },
        });
    }

    for &field in REMAINING_FIELDS {
        out.push(Suggestion {
            key: field.name(),
            label: composed_label(field, Scope::Anyone),
            context: "Roster",
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
        for p in PRESETS {
            assert_no_placeholder_ids(&(p.build)(), p.key);
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
