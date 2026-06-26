//! Stat-attribution provenance: which inputs (modules, upgrades, skills,
//! consumables, innate effects) produced each TTX stat value, and the magnitude
//! each contributed. Built by the recording factory path; rendered by `render`.

use std::collections::HashMap;

use crate::game_params::ttx::labels::TtxStat;
use crate::game_params::ttx::module_options::ModuleSlot;
use crate::game_params::types::CrewSkillName;
use crate::game_types::Consumable;
use crate::recognized::Recognized;

/// The identity of one attribution input.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum InputId {
    /// A selected module: the source of base values and of module-level
    /// coefficients not carried in the bundle (fire-control `maxDistCoef`,
    /// engine `speedCoef`). `name` is the `ShipUpgradeInfo` key.
    Module { slot: ModuleSlot, name: String },
    /// A modernization (upgrade), one per equipped slot. `name` is the upgrade key.
    Upgrade { name: String },
    /// A commander skill, keyed by internal name.
    Skill { name: CrewSkillName },
    /// A ship consumable.
    Consumable(Recognized<Consumable>),
    /// A ship innate skill, keyed by `skill_type`.
    Innate { skill_type: String },
}

/// Coefficient (multiply) vs bonus (add).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Mul,
    Add,
}

/// One applied contribution to a stat.
#[derive(Clone, Debug, PartialEq)]
pub struct Contribution {
    pub input: InputId,
    /// The modifier name, or a module-coefficient label (e.g. "maxDistCoef").
    pub modifier_name: String,
    pub op: Op,
    pub operand: f32,
}

/// Identity of one stat row: the `TtxStat` and its collection qualifier (ammo
/// kind / mount label / launcher index). The `(stat, qualifier)` pair `rows()`,
/// the diff, and the coverage check key on.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct StatKey {
    pub stat: TtxStat,
    pub qualifier: Option<String>,
}

/// Full provenance for one stat value.
#[derive(Clone, Debug, PartialEq)]
pub struct StatAttribution {
    pub stat: TtxStat,
    pub qualifier: Option<String>,
    /// Base value and the module it came from.
    pub base_value: f32,
    pub base_source: InputId,
    /// Contributions in application order.
    pub steps: Vec<Contribution>,
    /// Upstream stats this value is derived from (rotation time from rotation
    /// speed; on-fire detection from base detection; range detection from
    /// detection and gun range). Empty for non-derived stats. Orthogonal to
    /// `value` (replay) and to coverage; the render layer follows it to surface a
    /// derived stat's real cause.
    pub derived_from: Vec<StatKey>,
    /// Final value (equals the card's `StatValue` magnitude).
    pub value: f32,
}

impl StatAttribution {
    /// The stat value with ONLY `input` applied to the base: the base folded
    /// through just this input's steps, in their recorded (game-formula) order.
    /// Returns `None` when `input` contributes no step to this stat.
    ///
    /// This is the exact "with only this modifier equipped" value for every stat,
    /// mixed chains included, because isolating one input simply runs the game's
    /// fixed formula with that input's contribution alone present. It is NOT the
    /// input's additive share of the full change when the chain interleaves Mul
    /// and Add (see [`order_sensitive`](Self::order_sensitive)).
    pub fn isolated(&self, input: &InputId) -> Option<f32> {
        let mut matched = false;
        let value = self.steps.iter().filter(|c| &c.input == input).fold(self.base_value, |acc, c| {
            matched = true;
            match c.op {
                Op::Mul => acc * c.operand,
                Op::Add => acc + c.operand,
            }
        });
        matched.then_some(value)
    }

    /// The signed change each step makes to the running value, in the stat's
    /// units, in game-formula order. Telescoping: `base_value + sum == value`.
    /// A multiplicative step's delta is taken at its position (`running * (k-1)`),
    /// so it reflects in-loadout compounding rather than an isolated effect.
    pub fn step_deltas(&self) -> Vec<f32> {
        let mut acc = self.base_value;
        self.steps
            .iter()
            .map(|c| {
                let next = match c.op {
                    Op::Mul => acc * c.operand,
                    Op::Add => acc + c.operand,
                };
                let delta = next - acc;
                acc = next;
                delta
            })
            .collect()
    }

    /// The running stat value after each step applies, in game-formula order
    /// (a waterfall: `base -> after step 0 -> after step 1 -> ... -> value`). The
    /// last element equals `value`. Pair with [`step_deltas`](Self::step_deltas)
    /// to show "+970 -> 20370" per contributor.
    pub fn running_values(&self) -> Vec<f32> {
        let mut acc = self.base_value;
        self.steps
            .iter()
            .map(|c| {
                acc = match c.op {
                    Op::Mul => acc * c.operand,
                    Op::Add => acc + c.operand,
                };
                acc
            })
            .collect()
    }

    /// The total signed amount `input` contributed to the final value, in the
    /// stat's units (the sum of its steps' [`step_deltas`](Self::step_deltas)).
    /// `None` when `input` contributes no step. This is the "+1000 hp" /
    /// "-1.2 deg/s" figure; the per-input contributions sum to `value -
    /// base_value`. When an input stacks multiplicatively with others, its share
    /// reflects the game's fixed application order (see
    /// [`order_sensitive`](Self::order_sensitive)).
    pub fn contribution(&self, input: &InputId) -> Option<f32> {
        let deltas = self.step_deltas();
        let mut total = 0.0;
        let mut matched = false;
        for (c, d) in self.steps.iter().zip(deltas) {
            if &c.input == input {
                total += d;
                matched = true;
            }
        }
        matched.then_some(total)
    }

    /// True when the step chain interleaves `Mul` and `Add` steps, so an input's
    /// [`isolated`](Self::isolated) value is not its additive share of the full
    /// change (an additive step applied before a later multiply gets amplified).
    /// The fixed game formula is identical for every loadout; this only flags how
    /// a per-input delta should be interpreted, not any equip-order dependence.
    pub fn order_sensitive(&self) -> bool {
        let mut has_mul = false;
        let mut has_add = false;
        for c in &self.steps {
            match c.op {
                Op::Mul => has_mul = true,
                Op::Add => has_add = true,
            }
        }
        has_mul && has_add
    }
}

/// Provenance for a whole card: one `StatAttribution` per `StatRow`, in
/// `ShipStats::rows()` order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShipStatsProvenance {
    pub attributions: Vec<StatAttribution>,
}

impl ShipStatsProvenance {
    /// Replay a stat's `base_value` then each `step` in order, returning the
    /// reconstructed value. Used by the self-check test to prove the recorded
    /// steps reproduce the factory-computed value.
    pub fn replay(attr: &StatAttribution) -> f32 {
        attr.steps.iter().fold(attr.base_value, |acc, c| match c.op {
            Op::Mul => acc * c.operand,
            Op::Add => acc + c.operand,
        })
    }
}

/// Per-source raw modifier values, the provenance side-channel built next to the
/// `ModifierBundle` in `Effects::resolve`. `name -> [(source, raw_value)]`,
/// preserving contribution order.
#[derive(Clone, Debug, Default)]
pub struct ModifierSources {
    by_name: HashMap<String, Vec<(InputId, f32)>>,
}

impl ModifierSources {
    pub fn record(&mut self, name: &str, input: InputId, raw: f32) {
        self.by_name.entry(name.to_string()).or_default().push((input, raw));
    }
    pub fn get(&self, name: &str) -> &[(InputId, f32)] {
        self.by_name.get(name).map(Vec::as_slice).unwrap_or(&[])
    }
}

/// Appends contributions for one stat while recording. Held only on the `On` path.
pub struct StepBuilder<'a> {
    steps: &'a mut Vec<Contribution>,
    derived: &'a mut Vec<StatKey>,
}

impl StepBuilder<'_> {
    /// One `Mul` contribution per source behind a coefficient `name`.
    pub fn coef(&mut self, sources: &ModifierSources, name: &str) {
        for (input, raw) in sources.get(name) {
            self.steps.push(Contribution {
                input: input.clone(),
                modifier_name: name.to_string(),
                op: Op::Mul,
                operand: *raw,
            });
        }
    }
    /// One `Add` contribution per source behind a bonus `name`, each scaled by
    /// `scale` (e.g. `healthPerLevel` scaled by ship level). `scale` defaults to
    /// `1.0` via callers passing `1.0`.
    pub fn bonus(&mut self, sources: &ModifierSources, name: &str, scale: f32) {
        for (input, raw) in sources.get(name) {
            self.steps.push(Contribution {
                input: input.clone(),
                modifier_name: name.to_string(),
                op: Op::Add,
                operand: *raw * scale,
            });
        }
    }
    /// A module-level coefficient not carried in the bundle (fire-control
    /// `maxDistCoef`, engine clamp factor). No-op when `value` is the identity
    /// `1.0` (the module did not move the stat).
    pub fn module(&mut self, input: InputId, name: &str, value: f32) {
        if value == 1.0 {
            return;
        }
        self.steps.push(Contribution { input, modifier_name: name.to_string(), op: Op::Mul, operand: value });
    }
    /// A module-level additive contribution (e.g. on-fire detection penalty). No-op
    /// when `value` is the additive identity `0.0`.
    pub fn module_add(&mut self, input: InputId, name: &str, value: f32) {
        if value == 0.0 {
            return;
        }
        self.steps.push(Contribution { input, modifier_name: name.to_string(), op: Op::Add, operand: value });
    }
    /// Record that the enclosing stat is derived from an upstream stat.
    pub fn derived_from(&mut self, stat: TtxStat, qualifier: Option<&str>) {
        self.derived.push(StatKey { stat, qualifier: qualifier.map(str::to_string) });
    }
}

/// Zero-cost when off, accumulating when on.
pub trait Recorder {
    const ON: bool;
    fn record(
        &mut self,
        stat: TtxStat,
        qualifier: Option<&str>,
        base_value: f32,
        base_source: InputId,
        final_value: f32,
        build: impl FnOnce(&mut StepBuilder<'_>),
    );
    fn into_provenance(self) -> ShipStatsProvenance;
}

/// The no-op recorder. Guarded by `if R::ON`, every recording block (including
/// `InputId` construction) is eliminated.
pub struct Off;

impl Recorder for Off {
    const ON: bool = false;
    fn record(
        &mut self,
        _stat: TtxStat,
        _qualifier: Option<&str>,
        _base_value: f32,
        _base_source: InputId,
        _final_value: f32,
        _build: impl FnOnce(&mut StepBuilder<'_>),
    ) {
    }
    fn into_provenance(self) -> ShipStatsProvenance {
        ShipStatsProvenance::default()
    }
}

/// The accumulating recorder.
#[derive(Default)]
pub struct On {
    attributions: Vec<StatAttribution>,
}

impl Recorder for On {
    const ON: bool = true;
    fn record(
        &mut self,
        stat: TtxStat,
        qualifier: Option<&str>,
        base_value: f32,
        base_source: InputId,
        final_value: f32,
        build: impl FnOnce(&mut StepBuilder<'_>),
    ) {
        let mut steps = Vec::new();
        let mut derived = Vec::new();
        build(&mut StepBuilder { steps: &mut steps, derived: &mut derived });
        self.attributions.push(StatAttribution {
            stat,
            qualifier: qualifier.map(str::to_string),
            base_value,
            base_source,
            steps,
            derived_from: derived,
            value: final_value,
        });
    }
    fn into_provenance(self) -> ShipStatsProvenance {
        ShipStatsProvenance { attributions: self.attributions }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_reconstructs_value() {
        let attr = StatAttribution {
            stat: TtxStat::Health,
            qualifier: None,
            base_value: 19400.0,
            base_source: InputId::Module { slot: ModuleSlot::Hull, name: "H".to_string() },
            steps: vec![
                Contribution {
                    input: InputId::Upgrade { name: "U".to_string() },
                    modifier_name: "healthHullCoeff".to_string(),
                    op: Op::Mul,
                    operand: 1.05,
                },
                Contribution {
                    input: InputId::Skill { name: CrewSkillName::from("S") },
                    modifier_name: "healthPerLevel".to_string(),
                    op: Op::Add,
                    operand: 3500.0,
                },
            ],
            derived_from: Vec::new(),
            value: 23870.0,
        };
        // 19400 * 1.05 + 3500 = 23870.
        assert!((ShipStatsProvenance::replay(&attr) - 23870.0).abs() < 1e-3);
    }

    fn health_mixed(upgrade: &InputId, skill: &InputId) -> StatAttribution {
        StatAttribution {
            stat: TtxStat::Health,
            qualifier: None,
            base_value: 19400.0,
            base_source: InputId::Module { slot: ModuleSlot::Hull, name: "H".to_string() },
            steps: vec![
                Contribution {
                    input: upgrade.clone(),
                    modifier_name: "healthHullCoeff".to_string(),
                    op: Op::Mul,
                    operand: 1.05,
                },
                Contribution {
                    input: skill.clone(),
                    modifier_name: "healthPerLevel".to_string(),
                    op: Op::Add,
                    operand: 3500.0,
                },
            ],
            derived_from: Vec::new(),
            value: 23870.0,
        }
    }

    #[test]
    fn isolated_is_base_with_only_that_input() {
        let upgrade = InputId::Upgrade { name: "U".to_string() };
        let skill = InputId::Skill { name: CrewSkillName::from("S") };
        let attr = health_mixed(&upgrade, &skill);
        // "With only the hull-coeff upgrade": base * 1.05, not the mixed final.
        assert!((attr.isolated(&upgrade).unwrap() - 19400.0 * 1.05).abs() < 1e-3);
        // "With only the per-level skill": base + 3500.
        assert!((attr.isolated(&skill).unwrap() - (19400.0 + 3500.0)).abs() < 1e-3);
        // An input that contributes no step returns None.
        assert_eq!(attr.isolated(&InputId::Innate { skill_type: "none".into() }), None);
    }

    #[test]
    fn contribution_is_in_context_signed_delta() {
        let upgrade = InputId::Upgrade { name: "U".to_string() };
        let skill = InputId::Skill { name: CrewSkillName::from("S") };
        let attr = health_mixed(&upgrade, &skill);
        // base 19400; x1.05 (upgrade) then +3500 (skill).
        // upgrade's delta is taken at its position: 19400 * 0.05 = 970.
        assert!((attr.contribution(&upgrade).unwrap() - 970.0).abs() < 1e-3);
        // the additive skill contributes its raw +3500.
        assert!((attr.contribution(&skill).unwrap() - 3500.0).abs() < 1e-3);
        // Per-input contributions telescope to value - base.
        let sum: f32 = attr.step_deltas().iter().sum();
        assert!((sum - (attr.value - attr.base_value)).abs() < 1e-3);
        // Running waterfall: 19400 -> 20370 -> 23870; last equals value.
        let running = attr.running_values();
        assert!((running[0] - 20370.0).abs() < 1e-3);
        assert!((running[1] - attr.value).abs() < 1e-3);
        // An input that contributes no step.
        assert_eq!(attr.contribution(&InputId::Innate { skill_type: "none".into() }), None);
    }

    #[test]
    fn order_sensitive_only_when_chain_mixes_mul_and_add() {
        let upgrade = InputId::Upgrade { name: "U".to_string() };
        let skill = InputId::Skill { name: CrewSkillName::from("S") };
        // Mixed Mul+Add chain.
        assert!(health_mixed(&upgrade, &skill).order_sensitive());

        // Pure-Mul chain (e.g. range): not order sensitive; isolated is base * coef.
        let pure = StatAttribution {
            stat: TtxStat::ArtilleryRange,
            qualifier: None,
            base_value: 16.0,
            base_source: InputId::Module { slot: ModuleSlot::Hull, name: "A".to_string() },
            steps: vec![Contribution {
                input: upgrade.clone(),
                modifier_name: "GMMaxDist".to_string(),
                op: Op::Mul,
                operand: 1.16,
            }],
            derived_from: Vec::new(),
            value: 16.0 * 1.16,
        };
        assert!(!pure.order_sensitive());
        assert!((pure.isolated(&upgrade).unwrap() - 16.0 * 1.16).abs() < 1e-3);
    }

    #[test]
    fn isolated_folds_all_steps_of_one_input() {
        // A single input providing both a coef and a bonus to one stat: isolated
        // applies both, in recorded order (base * 1.2 + 5).
        let skill = InputId::Skill { name: CrewSkillName::from("S") };
        let attr = StatAttribution {
            stat: TtxStat::GunRotationSpeed,
            qualifier: None,
            base_value: 30.0,
            base_source: InputId::Module { slot: ModuleSlot::Hull, name: "A".to_string() },
            steps: vec![
                Contribution {
                    input: skill.clone(),
                    modifier_name: "GMRotationSpeed".to_string(),
                    op: Op::Mul,
                    operand: 1.2,
                },
                Contribution {
                    input: skill.clone(),
                    modifier_name: "GMRotationSpeedBonus".to_string(),
                    op: Op::Add,
                    operand: 5.0,
                },
            ],
            derived_from: Vec::new(),
            value: 41.0,
        };
        assert!((attr.isolated(&skill).unwrap() - (30.0 * 1.2 + 5.0)).abs() < 1e-3);
        // Contribution sums the input's step deltas: (30*0.2) + 5 = 11.
        assert!((attr.contribution(&skill).unwrap() - 11.0).abs() < 1e-3);
    }
}

#[cfg(test)]
mod recorder_tests {
    use super::*;

    fn sources_with(name: &str, entries: &[(InputId, f32)]) -> ModifierSources {
        let mut s = ModifierSources::default();
        for (input, raw) in entries {
            s.record(name, input.clone(), *raw);
        }
        s
    }

    #[test]
    fn off_records_nothing() {
        let mut rec = Off;
        rec.record(
            TtxStat::Speed,
            None,
            36.0,
            InputId::Module { slot: ModuleSlot::Hull, name: "H".into() },
            37.8,
            |b| {
                b.coef(&ModifierSources::default(), "speedCoef");
            },
        );
        assert!(rec.into_provenance().attributions.is_empty());
    }

    #[test]
    fn on_records_base_and_per_source_steps() {
        let up = InputId::Upgrade { name: "U1".into() };
        let sk = InputId::Skill { name: CrewSkillName::from("S1") };
        let sources = sources_with("speedCoef", &[(up.clone(), 1.05), (sk.clone(), 1.10)]);

        let mut rec = On::default();
        rec.record(
            TtxStat::Speed,
            None,
            36.0,
            InputId::Module { slot: ModuleSlot::Hull, name: "H".into() },
            41.58,
            |b| {
                b.coef(&sources, "speedCoef");
            },
        );

        let prov = rec.into_provenance();
        assert_eq!(prov.attributions.len(), 1);
        let a = &prov.attributions[0];
        assert_eq!(a.base_value, 36.0);
        assert_eq!(a.steps.len(), 2);
        assert_eq!(a.steps[0].input, up);
        assert_eq!(a.steps[0].op, Op::Mul);
        assert!((a.steps[0].operand - 1.05).abs() < 1e-6);
        assert_eq!(a.steps[1].input, sk);
        // Replay: 36 * 1.05 * 1.10 = 41.58.
        assert!((ShipStatsProvenance::replay(a) - 41.58).abs() < 1e-3);
    }

    #[test]
    fn module_identity_factor_is_skipped() {
        let mut rec = On::default();
        rec.record(
            TtxStat::ArtilleryRange,
            None,
            11.13,
            InputId::Module { slot: ModuleSlot::Artillery, name: "A".into() },
            11.13,
            |b| {
                b.module(InputId::Module { slot: ModuleSlot::FireControl, name: "FC".into() }, "maxDistCoef", 1.0);
            },
        );
        assert!(rec.into_provenance().attributions[0].steps.is_empty());
    }

    #[test]
    fn module_add_skips_zero_and_records_add() {
        let hull_src = InputId::Module { slot: ModuleSlot::Hull, name: "H".into() };

        // Zero value: no step recorded.
        let mut rec = On::default();
        rec.record(TtxStat::SeaDetectionOnFire, None, 7.33, hull_src.clone(), 7.33, |b| {
            b.module_add(hull_src.clone(), "visibilityCoefFire", 0.0);
        });
        assert!(rec.into_provenance().attributions[0].steps.is_empty(), "zero fire should record no step");

        // Non-zero value: one Op::Add step recorded.
        let mut rec = On::default();
        rec.record(TtxStat::SeaDetectionOnFire, None, 7.33, hull_src.clone(), 9.33, |b| {
            b.module_add(hull_src.clone(), "visibilityCoefFire", 2.0);
        });
        let prov = rec.into_provenance();
        let a = &prov.attributions[0];
        assert_eq!(a.steps.len(), 1);
        assert_eq!(a.steps[0].op, Op::Add);
        assert!((a.steps[0].operand - 2.0).abs() < 1e-6);
        // Replay: 7.33 + 2.0 = 9.33.
        assert!((ShipStatsProvenance::replay(a) - 9.33).abs() < 1e-4);
    }

    #[test]
    fn on_records_derived_from_links() {
        let mut rec = On::default();
        rec.record(
            TtxStat::GunRotationTime,
            None,
            9.0,
            InputId::Module { slot: ModuleSlot::Artillery, name: "A".into() },
            9.0,
            |b| {
                b.derived_from(TtxStat::GunRotationSpeed, None);
            },
        );
        let prov = rec.into_provenance();
        let a = &prov.attributions[0];
        assert!(a.steps.is_empty());
        assert_eq!(a.derived_from, vec![StatKey { stat: TtxStat::GunRotationSpeed, qualifier: None }]);
        // derived_from does not affect replay.
        assert!((ShipStatsProvenance::replay(a) - 9.0).abs() < 1e-6);
    }
}
