//! Stat-attribution provenance: which inputs (modules, upgrades, skills,
//! consumables, innate effects) produced each TTX stat value, and the magnitude
//! each contributed. Built by the recording factory path; rendered by `render`.

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
            value: 23870.0,
        };
        // 19400 * 1.05 + 3500 = 23870.
        assert!((ShipStatsProvenance::replay(&attr) - 23870.0).abs() < 1e-3);
    }
}
