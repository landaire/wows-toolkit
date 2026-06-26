//! Render `ShipStatsProvenance` into translated, formatted attribution lines:
//! the localized stat label, the base module, and each contributing input with
//! its magnitude. Mirrors `render::render_stat_rows`.

use std::collections::HashMap;

use crate::data::ResourceLoader;
use crate::game_params::ttx::labels::TtxStat;
use crate::game_params::ttx::labels::stat_display_label;
use crate::game_params::ttx::model::StatRow;
use crate::game_params::ttx::provenance::Contribution;
use crate::game_params::ttx::provenance::InputId;
use crate::game_params::ttx::provenance::Op;
use crate::game_params::ttx::provenance::ShipStatsProvenance;
use crate::game_params::ttx::provenance::StatKey;
use crate::game_params::types::GameParamProvider;
use crate::recognized::Recognized;

/// One contributing input rendered for display.
#[derive(Clone, Debug, PartialEq)]
pub struct ContributorLine {
    /// Localized input name (e.g. "Main Battery Mod 3", "Adrenaline Rush").
    pub label: String,
    /// The applied magnitude, formatted: "x0.95" (Mul) or "+350" (Add).
    pub effect: String,
    /// The signed amount this step moved the stat, in its units, trimmed
    /// ("+1000", "-1.2"). Per-step deltas sum to `value - base_value`; for an
    /// `order_sensitive` stat a multiplicative step's delta reflects its position
    /// in the game formula.
    pub delta: String,
    /// The running stat value after this step applies, trimmed (the waterfall
    /// absolute; the last contributor's equals the final `value`).
    pub value_after: String,
}

/// One stat's full attribution, rendered.
#[derive(Clone, Debug, PartialEq)]
pub struct AttributionLine {
    pub stat: TtxStat,
    pub qualifier: Option<String>,
    pub label: String,
    pub value: String,
    pub base_label: String,
    pub base_value: String,
    pub contributors: Vec<ContributorLine>,
    /// Upstream stats this value is derived from (e.g. rotation time from
    /// rotation speed). A consumer can resolve each key against the rendered
    /// set to recurse into the upstream stat's contributors.
    pub derived_from: Vec<StatKey>,
    /// True when this stat's chain interleaves multiply and add. The per-step
    /// `delta`s still sum to the total change, but a multiplicative step's delta
    /// reflects its position in the game formula (an additive input applied before
    /// a later multiply is amplified), so consumers may want to note that.
    pub order_sensitive: bool,
}

/// The display label for an attribution input. Module/Upgrade names are resolved
/// through the loader/provider where possible, falling back to the raw key.
fn input_label(input: &InputId, loader: &dyn ResourceLoader, provider: &dyn GameParamProvider) -> String {
    match input {
        InputId::Module { name, .. } | InputId::Upgrade { name } => resolve_param_label(name, loader, provider),
        InputId::Skill { name } => name.as_str().to_string(),
        InputId::Consumable(c) => match c {
            Recognized::Known(k) => k.name().to_string(),
            Recognized::Unknown(raw) => raw.clone(),
        },
        InputId::Innate { skill_type } => skill_type.clone(),
    }
}

/// Resolve a param key to a localized name, falling back to the key itself.
fn resolve_param_label(key: &str, loader: &dyn ResourceLoader, provider: &dyn GameParamProvider) -> String {
    provider
        .game_param_by_name(key)
        .and_then(|p| loader.localized_name_from_param(&p))
        .unwrap_or_else(|| key.to_string())
}

/// Format a single contribution's magnitude. ASCII `x` / `+`, no unicode.
fn format_effect(c: &Contribution) -> String {
    match c.op {
        Op::Mul => format!("x{}", trim(c.operand)),
        Op::Add => format!("+{}", trim(c.operand)),
    }
}

/// Format a signed unit delta: "+1000", "-1.2" (`trim` already carries the minus).
fn format_delta(d: f32) -> String {
    let s = trim(d);
    if d >= 0.0 { format!("+{s}") } else { s }
}

/// Trim a float to at most 3 decimals without trailing zeros.
fn trim(v: f32) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    s.to_string()
}

/// Render provenance attributions into display lines. Each line carries the
/// localized stat label, the displayed value sourced from the model `rows`
/// (so infinite ammo shows "inf", bools show "yes"/"no", units are preserved),
/// the base module name and its value, and per-contributor effects.
///
/// `rows` must be the `ShipStats::rows()` matching the card the provenance was
/// recorded for; keys are `(stat, qualifier)` pairs. If a key is absent (should
/// not occur when Task 10 coverage holds), the numeric `a.value` is trimmed as
/// a fallback.
pub fn render_attributions(
    prov: &ShipStatsProvenance,
    rows: &[StatRow],
    loader: &dyn ResourceLoader,
    provider: &dyn GameParamProvider,
) -> Vec<AttributionLine> {
    let display: HashMap<(TtxStat, Option<String>), String> =
        rows.iter().map(|r| ((r.stat, r.qualifier.clone()), r.value.to_string())).collect();

    prov.attributions
        .iter()
        .map(|a| {
            let key = (a.stat, a.qualifier.clone());
            debug_assert!(
                display.contains_key(&key),
                "render_attributions: stat {:?} qualifier {:?} absent from rows map; provenance key-set diverged from rows()",
                a.stat,
                a.qualifier
            );
            let value = display.get(&key).cloned().unwrap_or_else(|| trim(a.value));
            let base_value = if a.steps.is_empty() {
                // No contributors: base IS the final value (ammo counts, bools,
                // derived-only stats). Reuse the StatValue display string so the
                // base column shows "inf"/"yes"/"no" rather than the sentinel float.
                value.clone()
            } else {
                trim(a.base_value)
            };
            AttributionLine {
                stat: a.stat,
                qualifier: a.qualifier.clone(),
                label: stat_display_label(a.stat, loader),
                value,
                base_label: input_label(&a.base_source, loader, provider),
                base_value,
                contributors: a
                    .steps
                    .iter()
                    .zip(a.step_deltas())
                    .zip(a.running_values())
                    .map(|((c, delta), running)| ContributorLine {
                        label: input_label(&c.input, loader, provider),
                        effect: format_effect(c),
                        delta: format_delta(delta),
                        value_after: trim(running),
                    })
                    .collect(),
                derived_from: a.derived_from.clone(),
                order_sensitive: a.order_sensitive(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Rc;

    const BASE_HEALTH: f32 = 19400.0;
    const HEALTH_COEFF: f32 = 1.05;
    const HEALTH_BONUS: f32 = 3500.0;
    const HEALTH_FINAL: f32 = 23870.0;

    use crate::game_params::ttx::model::AmmoCount;
    use crate::game_params::ttx::model::Hp;
    use crate::game_params::ttx::model::StatValue;
    use crate::game_params::ttx::module_options::ModuleSlot;
    use crate::game_params::ttx::provenance::StatAttribution;
    use crate::game_params::types::CrewSkillName;
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

    struct EmptyProvider;
    impl GameParamProvider for EmptyProvider {
        fn game_param_by_id(&self, _id: crate::game_types::GameParamId) -> Option<Rc<Param>> {
            None
        }
        fn game_param_by_index(&self, _i: &str) -> Option<Rc<Param>> {
            None
        }
        fn game_param_by_name(&self, _n: &str) -> Option<Rc<Param>> {
            None
        }
        fn params(&self) -> &[Rc<Param>] {
            &[]
        }
    }

    #[test]
    fn renders_base_and_contributors() {
        let prov = ShipStatsProvenance {
            attributions: vec![StatAttribution {
                stat: TtxStat::Health,
                qualifier: None,
                base_value: BASE_HEALTH,
                base_source: InputId::Module { slot: ModuleSlot::Hull, name: "PAUH911".into() },
                steps: vec![
                    Contribution {
                        input: InputId::Skill { name: CrewSkillName::from("AdrenalineRush") },
                        modifier_name: "healthHullCoeff".into(),
                        op: Op::Mul,
                        operand: HEALTH_COEFF,
                    },
                    Contribution {
                        input: InputId::Upgrade { name: "PCM030".into() },
                        modifier_name: "healthPerLevel".into(),
                        op: Op::Add,
                        operand: HEALTH_BONUS,
                    },
                ],
                derived_from: Vec::new(),
                value: HEALTH_FINAL,
            }],
        };
        let rows =
            vec![StatRow { stat: TtxStat::Health, qualifier: None, value: StatValue::Hp(Hp::from(HEALTH_FINAL)) }];
        let lines = render_attributions(&prov, &rows, &EchoLoader, &EmptyProvider);
        assert_eq!(lines.len(), 1);
        let l = &lines[0];
        assert_eq!(l.base_label, "PAUH911");
        assert_eq!(l.base_value, "19400");
        assert_eq!(l.value, "23870");
        assert_eq!(l.contributors.len(), 2);
        assert_eq!(l.contributors[0].label, "AdrenalineRush");
        assert_eq!(l.contributors[0].effect, "x1.05");
        assert_eq!(l.contributors[1].effect, "+3500");
        // Mixed Mul+Add chain: order-sensitive, and each contributor carries its
        // signed unit delta. x1.05 on 19400 adds 970; +3500 adds 3500; they sum to
        // the 4470 total change (23870 - 19400).
        assert!(l.order_sensitive);
        assert_eq!(l.contributors[0].delta, "+970");
        assert_eq!(l.contributors[1].delta, "+3500");
        // Running waterfall absolutes: 19400 -> 20370 -> 23870 (= final value).
        assert_eq!(l.contributors[0].value_after, "20370");
        assert_eq!(l.contributors[1].value_after, "23870");
    }

    #[test]
    fn ammo_stat_shows_inf_not_sentinel() {
        // ShellMaxAmmo with Infinite: provenance value is -1.0 (the raw sentinel
        // stored in the attribution), but the display must come from the StatValue.
        let prov = ShipStatsProvenance {
            attributions: vec![StatAttribution {
                stat: TtxStat::ShellMaxAmmo,
                qualifier: Some("HE".into()),
                base_value: -1.0,
                base_source: InputId::Module { slot: ModuleSlot::Hull, name: "PAUH911".into() },
                steps: vec![],
                derived_from: Vec::new(),
                value: -1.0,
            }],
        };
        let rows = vec![StatRow {
            stat: TtxStat::ShellMaxAmmo,
            qualifier: Some("HE".into()),
            value: StatValue::Ammo(AmmoCount::Infinite),
        }];
        let lines = render_attributions(&prov, &rows, &EchoLoader, &EmptyProvider);
        assert_eq!(lines.len(), 1);
        let l = &lines[0];
        assert_eq!(l.value, "inf", "value should be 'inf', not '-1'");
        assert_eq!(l.base_value, "inf", "base_value should also be 'inf' when steps is empty");
    }

    #[test]
    fn bool_stat_shows_yes_not_one() {
        // TorpedoIsDamageIncreasing with true: provenance value is 1.0, but
        // display must come from the StatValue which renders as "yes".
        let prov = ShipStatsProvenance {
            attributions: vec![StatAttribution {
                stat: TtxStat::TorpedoIsDamageIncreasing,
                qualifier: Some("0".into()),
                base_value: 1.0,
                base_source: InputId::Module { slot: ModuleSlot::Hull, name: "PAUH911".into() },
                steps: vec![],
                derived_from: Vec::new(),
                value: 1.0,
            }],
        };
        let rows = vec![StatRow {
            stat: TtxStat::TorpedoIsDamageIncreasing,
            qualifier: Some("0".into()),
            value: StatValue::Bool(true),
        }];
        let lines = render_attributions(&prov, &rows, &EchoLoader, &EmptyProvider);
        assert_eq!(lines.len(), 1);
        let l = &lines[0];
        assert_eq!(l.value, "yes", "value should be 'yes', not '1'");
        assert_eq!(l.base_value, "yes", "base_value should also be 'yes' when steps is empty");
    }
}
