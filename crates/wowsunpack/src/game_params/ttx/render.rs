//! Render TTX stat rows into translated, formatted lines, and diff two cards.
//!
//! `render_stat_rows` turns a card's rows into labeled lines (translated label via
//! `stat_label`, value via `StatValue`'s `Display`); `diff_stat_rows` reports only the
//! rows that change between two cards. The `stat` stays on each output so a consumer can
//! fall back to `TtxStat::field_key` when no translated label exists.

use std::collections::HashMap;
use std::collections::HashSet;

use crate::data::ResourceLoader;
use crate::game_params::ttx::labels::TtxStat;
use crate::game_params::ttx::labels::stat_label;
use crate::game_params::ttx::model::StatRow;
use crate::game_params::ttx::model::StatValue;

/// A rendered stat row: the stat, its collection qualifier (ammo kind / launcher index),
/// the translated label (`None` -> caller falls back to `stat.field_key()`), and the
/// value formatted with its unit.
#[derive(Clone, Debug, PartialEq)]
pub struct StatLine {
    pub stat: TtxStat,
    pub qualifier: Option<String>,
    pub label: Option<String>,
    pub value: String,
}

/// Render `rows` to absolute labeled lines, preserving input order.
pub fn render_stat_rows(rows: &[StatRow], loader: &dyn ResourceLoader) -> Vec<StatLine> {
    rows.iter()
        .map(|row| StatLine {
            stat: row.stat,
            qualifier: row.qualifier.clone(),
            label: stat_label(row.stat, loader),
            value: row.value.to_string(),
        })
        .collect()
}

/// A rendered change for one `(stat, qualifier)` key. `from`/`to` are the formatted
/// values; `None` means the stat is absent on that side (removed / added).
#[derive(Clone, Debug, PartialEq)]
pub struct StatDelta {
    pub stat: TtxStat,
    pub qualifier: Option<String>,
    pub label: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
}

/// The changed rows from `baseline` to `candidate`, keyed by `(stat, qualifier)`:
/// changed (both sides, differing value), added (`from = None`), or removed (`to =
/// None`). Unchanged rows are omitted. Output is candidate-row order, then removed
/// baseline rows -- deterministic.
pub fn diff_stat_rows(baseline: &[StatRow], candidate: &[StatRow], loader: &dyn ResourceLoader) -> Vec<StatDelta> {
    let base: HashMap<(TtxStat, Option<String>), StatValue> =
        baseline.iter().map(|r| ((r.stat, r.qualifier.clone()), r.value)).collect();
    let candidate_keys: HashSet<(TtxStat, Option<String>)> =
        candidate.iter().map(|r| (r.stat, r.qualifier.clone())).collect();

    let mut out = Vec::new();
    for row in candidate {
        let key = (row.stat, row.qualifier.clone());
        let from = base.get(&key);
        if from == Some(&row.value) {
            continue;
        }
        out.push(StatDelta {
            stat: row.stat,
            qualifier: row.qualifier.clone(),
            label: stat_label(row.stat, loader),
            from: from.map(|v| v.to_string()),
            to: Some(row.value.to_string()),
        });
    }
    for row in baseline {
        let key = (row.stat, row.qualifier.clone());
        if candidate_keys.contains(&key) {
            continue;
        }
        out.push(StatDelta {
            stat: row.stat,
            qualifier: row.qualifier.clone(),
            label: stat_label(row.stat, loader),
            from: Some(row.value.to_string()),
            to: None,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Rc;
    use crate::game_params::types::Km;
    use crate::game_params::types::Param;

    struct EchoLoader;
    impl ResourceLoader for EchoLoader {
        fn localized_name_from_param(&self, _p: &Param) -> Option<String> {
            None
        }
        fn localized_name_from_id(&self, id: &crate::data::TranslationKey) -> Option<String> {
            Some(id.as_str().to_string())
        }
        fn game_param_by_id(&self, _id: crate::game_types::GameParamId) -> Option<Rc<Param>> {
            None
        }
        fn entity_specs(&self) -> &[crate::rpc::entitydefs::EntitySpec] {
            &[]
        }
    }

    fn row(stat: TtxStat, qualifier: Option<&str>, value: StatValue) -> StatRow {
        StatRow { stat, qualifier: qualifier.map(|s| s.to_string()), value }
    }

    #[test]
    fn render_uses_label_and_display_value() {
        let loader = EchoLoader;
        // SeaDetection has a label_key; ArtilleryDispersionVertical does not.
        let rows = vec![
            row(TtxStat::SeaDetection, None, StatValue::Km(Km::from(7.3))),
            row(TtxStat::ArtilleryDispersionVertical, None, StatValue::Km(Km::from(0.27))),
        ];
        let lines = render_stat_rows(&rows, &loader);
        assert_eq!(lines.len(), 2);
        // Echo loader returns the IDS key as the "translation".
        assert_eq!(lines[0].label.as_deref(), TtxStat::SeaDetection.label_key());
        assert!(lines[0].label.is_some());
        assert_eq!(lines[0].value, StatValue::Km(Km::from(7.3)).to_string());
        // No label_key -> None; caller would use field_key.
        assert_eq!(lines[1].label, None);
        assert_eq!(lines[1].stat.field_key(), "artillery.dispersion_vertical");
    }

    #[test]
    fn diff_reports_changed_added_removed_and_skips_equal() {
        let loader = EchoLoader;
        let baseline = vec![
            row(TtxStat::Health, None, StatValue::Hp(crate::game_params::ttx::model::Hp::from(19400.0))),
            row(TtxStat::Speed, None, StatValue::Knots(crate::game_params::ttx::model::Knots::from(36.0))),
            row(TtxStat::SeaDetection, None, StatValue::Km(Km::from(7.3))),
        ];
        let candidate = vec![
            // Health changed.
            row(TtxStat::Health, None, StatValue::Hp(crate::game_params::ttx::model::Hp::from(21000.0))),
            // Speed unchanged (skipped).
            row(TtxStat::Speed, None, StatValue::Knots(crate::game_params::ttx::model::Knots::from(36.0))),
            // SeaDetection removed (absent here); TurningRadius added.
            row(TtxStat::TurningRadius, None, StatValue::Meters(crate::game_params::types::Meters::from(640.0))),
        ];
        let deltas = diff_stat_rows(&baseline, &candidate, &loader);
        // Health changed, TurningRadius added, SeaDetection removed; Speed skipped.
        assert_eq!(deltas.len(), 3);
        let health = deltas.iter().find(|d| d.stat == TtxStat::Health).expect("health");
        assert!(health.from.is_some() && health.to.is_some() && health.from != health.to);
        let added = deltas.iter().find(|d| d.stat == TtxStat::TurningRadius).expect("added");
        assert_eq!(added.from, None);
        assert!(added.to.is_some());
        let removed = deltas.iter().find(|d| d.stat == TtxStat::SeaDetection).expect("removed");
        assert!(removed.from.is_some());
        assert_eq!(removed.to, None);
        assert!(deltas.iter().all(|d| d.stat != TtxStat::Speed), "equal row must be skipped");
    }

    #[test]
    fn identical_cards_yield_empty_diff() {
        let loader = EchoLoader;
        let rows = vec![row(TtxStat::SeaDetection, None, StatValue::Km(Km::from(7.3)))];
        assert!(diff_stat_rows(&rows, &rows, &loader).is_empty());
    }

    #[test]
    fn diff_keys_on_qualifier_distinctly() {
        use crate::game_params::ttx::model::Hp;
        let loader = EchoLoader;
        let baseline = vec![
            row(TtxStat::ShellDamage, Some("HE"), StatValue::Hp(Hp::from(1800.0))),
            row(TtxStat::ShellDamage, Some("AP"), StatValue::Hp(Hp::from(5000.0))),
        ];
        let candidate = vec![
            row(TtxStat::ShellDamage, Some("HE"), StatValue::Hp(Hp::from(2000.0))),
            row(TtxStat::ShellDamage, Some("AP"), StatValue::Hp(Hp::from(5000.0))),
        ];
        let deltas = diff_stat_rows(&baseline, &candidate, &loader);
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].stat, TtxStat::ShellDamage);
        assert_eq!(deltas[0].qualifier.as_deref(), Some("HE"));
        assert!(deltas[0].from.is_some() && deltas[0].to.is_some() && deltas[0].from != deltas[0].to);
    }
}
