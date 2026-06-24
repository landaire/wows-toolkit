//! Stat-attribution provenance: which inputs (modules, upgrades, skills,
//! consumables, innate effects) produced each TTX stat value, and the magnitude
//! each contributed. Built by the recording factory path; rendered by `render`.

use std::collections::HashMap;

use crate::game_params::ttx::labels::TtxStat;
use crate::game_params::ttx::module_options::ModuleSlot;
use crate::game_params::types::CrewSkillName;
use crate::game_types::Consumable;

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
    Consumable(Consumable),
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
