//! How a ship's hull and build resist fires and flooding.
//!
//! A hull's `burnNodes` are `(probability, damage, duration)` triples, one per
//! fire zone. The client builds each node with `probability * burnProb`,
//! `damage` and `duration * hlCritTimeCoeff * burnTime`
//! (`HitLocationEffects._createNodes`), and a hit starts a fire when a roll
//! falls under the attacker's fire chance times that probability.

use wows_core::units;
use wowsunpack::game_params::ttx::components::HullComponentStats;
use wowsunpack::game_params::ttx::model::Percent;
use wowsunpack::game_params::ttx::model::Seconds;
use wowsunpack::game_params::ttx::modifiers::ModifierBundle;
use wowsunpack::game_params::types::CrewSkill;
use wowsunpack::game_params::types::KnownCrewSkill;

/// A hull's fire resistance with a build's modifiers applied.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FireResistance {
    /// Hull `burnNodes` probability, set by tier. Every node of a hull carries
    /// the same value, so the first one stands for all.
    pub hull_coefficient: f32,

    /// The build's `burnProb`, e.g. 0.9 with Fire Prevention Expert.
    pub build_coefficient: f32,

    /// Fires that can burn at once.
    pub zones: u32,
    pub duration: Seconds,

    /// Maximum health lost per second to one fire.
    pub damage_per_sec: Percent,
}

impl FireResistance {
    /// `None` when the hull has no `burnNodes`. `merges_fire_zones` is true
    /// with Fire Prevention Expert, which holds the third node unlit and lets
    /// a hit there light the second (`HitLocationEffects.setBurningFlags`).
    pub fn new(hull: &HullComponentStats, modifiers: &ModifierBundle, merges_fire_zones: bool) -> Option<Self> {
        const MERGED_NODE: usize = 2;

        let node = hull.burn_nodes.first()?;
        let nodes = hull.burn_nodes.len();
        let zones = if merges_fire_zones && nodes > MERGED_NODE { nodes - 1 } else { nodes };

        Some(Self {
            hull_coefficient: node.probability,
            build_coefficient: modifiers.coef("burnProb"),
            zones: zones as u32,
            duration: Seconds::from(
                node.duration.value() * modifiers.coef("hlCritTimeCoeff") * modifiers.coef("burnTime"),
            ),
            damage_per_sec: Percent::from(node.damage_per_sec.value() * modifiers.coef("vulnerabilityBurn")),
        })
    }

    /// What an attacking shell's fire chance is multiplied by against this
    /// ship.
    pub fn chance_factor(&self) -> f32 {
        self.hull_coefficient * self.build_coefficient
    }

    /// Chance that one hit by a shell with fire chance `shell` starts a fire
    /// in a zone not already burning. The roll is `random() < chance`, so a
    /// product past 100 percent always starts one.
    pub fn chance_against(&self, shell: units::Percent) -> units::Percent {
        units::Percent::from_fraction((shell.to_fraction() * self.chance_factor()).clamp(0.0, 1.0))
    }
}

/// A hull's flooding with a build's modifiers applied.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Flooding {
    pub duration: Seconds,

    /// Maximum health lost per second to one flood.
    pub damage_per_sec: Percent,
}

impl Flooding {
    /// `None` when the hull has no `floodNodes`. Every node of a hull carries
    /// the same values.
    pub fn new(hull: &HullComponentStats, modifiers: &ModifierBundle) -> Option<Self> {
        let node = hull.flood_nodes.first()?;

        Some(Self {
            duration: Seconds::from(
                node.duration.value() * modifiers.coef("hlCritTimeCoeff") * modifiers.coef("floodTime"),
            ),
            damage_per_sec: Percent::from(node.damage_per_sec.value() * modifiers.coef("vulnerabilityFlood")),
        })
    }
}

/// Whether `skill` is Fire Prevention Expert, whose `fireResistanceEnabled`
/// flag merges two fire zones. The flag is not a numeric modifier, so it is
/// read off the skill.
pub fn merges_fire_zones(skill: &CrewSkill) -> bool {
    KnownCrewSkill::recognize(skill.internal_name(), skill.skill_type()).known()
        == Some(&KnownCrewSkill::FirePreventionExpert)
}

#[cfg(test)]
mod tests {
    use wowsunpack::data::Version;
    use wowsunpack::game_params::ttx::components::BurnNode;
    use wowsunpack::game_params::types::CrewSkillModifier;
    use wowsunpack::game_params::types::Species;

    use super::*;

    fn hull(nodes: usize) -> HullComponentStats {
        let node = BurnNode { probability: 0.5005, damage_per_sec: Percent::from(0.3), duration: Seconds::from(60.0) };
        HullComponentStats { burn_nodes: vec![node; nodes], ..Default::default() }
    }

    fn uniform(name: &str, value: f32) -> CrewSkillModifier {
        CrewSkillModifier::builder()
            .name(name.to_owned())
            .aircraft_carrier(value)
            .auxiliary(value)
            .battleship(value)
            .cruiser(value)
            .destroyer(value)
            .submarine(value)
            .excluded_consumables(Vec::new())
            .build()
    }

    #[test]
    fn fire_prevention_merges_a_zone_only_on_hulls_with_a_third_node() {
        let modifiers = ModifierBundle::empty(Species::Battleship);

        let four = FireResistance::new(&hull(4), &modifiers, true).expect("hull has nodes");
        let one = FireResistance::new(&hull(1), &modifiers, true).expect("hull has nodes");

        assert_eq!((four.zones, one.zones), (3, 1));
    }

    #[test]
    fn effective_chance_is_capped_at_certain() {
        let modifiers =
            ModifierBundle::from_modifiers(&[uniform("burnProb", 0.9)], Species::Battleship, Version::base(15, 10, 0))
                .expect("burnProb classifies");
        let mut bot = hull(4);
        bot.burn_nodes[0].probability = 9.0;

        let tier_ten = FireResistance::new(&hull(4), &modifiers, false).expect("hull has nodes");
        let bot = FireResistance::new(&bot, &modifiers, false).expect("hull has nodes");

        assert!((tier_ten.chance_against(units::Percent::new(20.0)).value() - 20.0 * 0.5005 * 0.9).abs() < 1e-4);
        assert_eq!(bot.chance_against(units::Percent::new(20.0)), units::Percent::new(100.0));
    }
}
