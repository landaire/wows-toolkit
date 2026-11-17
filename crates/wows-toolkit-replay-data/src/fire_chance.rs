//! The recording player's effective fire chance, with the names the replay's build gives the
//! GameParams ids the analysis reports.

use std::collections::HashMap;

use wows_replay_insights::fire_chance::analysis::EffectiveFireChance;
use wows_replay_insights::fire_chance::analysis::PerShipFireChance;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::TranslationKey;
use wowsunpack::data::Version;
use wowsunpack::game_params::describe::DescribeContext;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::CrewSkillName;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::skill_translation_keys_for;

/// An effective fire chance and the names of the modifiers and ships it reports.
#[derive(Clone, Debug)]
pub struct FireChance {
    pub analysis: EffectiveFireChance,
    pub names: FireChanceNames,
}

impl FireChance {
    /// `analysis` with its ids named through the build the replay was recorded on.
    pub fn named(analysis: EffectiveFireChance, provider: &GameMetadataProvider, version: &Version) -> Self {
        let names = FireChanceNames::resolve(&analysis, provider, version);

        Self { analysis, names }
    }
}

/// Display names for a fire chance's formula sources and victim ships, read when the replay is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FireChanceNames {
    /// By the source's GameParams id.
    sources: HashMap<String, String>,
    /// By the victim ship's GameParams index.
    ships: HashMap<String, String>,
}

impl FireChanceNames {
    fn resolve(fire_chance: &EffectiveFireChance, provider: &GameMetadataProvider, version: &Version) -> Self {
        Self::from_lookups(
            fire_chance,
            |source| source_name(provider, version, source),
            |index| {
                let param = GameParamProvider::game_param_by_index(provider, index)?;
                provider.localized_name_from_param(&param)
            },
        )
    }

    /// The names `source_name` and `ship_name` give the ids in `fire_chance`, by source id and by
    /// ship index.
    pub(crate) fn from_lookups(
        fire_chance: &EffectiveFireChance,
        source_name: impl Fn(&str) -> Option<String>,
        ship_name: impl Fn(&str) -> Option<String>,
    ) -> Self {
        let sources = fire_chance
            .formula
            .iter()
            .filter_map(|step| step.source.as_deref())
            .filter_map(|source| Some((source.to_owned(), source_name(source)?)))
            .collect();
        let ships = fire_chance
            .per_ship
            .iter()
            .filter_map(|ship| Some((ship.victim_ship_index.clone(), ship_name(&ship.victim_ship_index)?)))
            .collect();

        Self { sources, ships }
    }

    /// The name of the upgrade, signal or skill `source` identifies. The id itself where the build
    /// names none, as it is still the most exact thing to show.
    pub fn source(&self, source: &str) -> String {
        self.sources.get(source).cloned().unwrap_or_else(|| source.to_owned())
    }

    /// The name of `ship`'s ship. Its internal name where the build names none, as it is still the
    /// most exact thing to show.
    pub fn ship(&self, ship: &PerShipFireChance) -> String {
        self.ships.get(&ship.victim_ship_index).cloned().unwrap_or_else(|| ship.victim_ship_name.clone())
    }
}

/// The name of an equipped upgrade or signal, else of a crew skill, which has no GameParams entry
/// of its own and is named through its translation keys.
fn source_name(provider: &GameMetadataProvider, version: &Version, source: &str) -> Option<String> {
    let upgrade_or_signal = GameParamProvider::game_param_by_name(provider, source).and_then(|param| {
        let cx = DescribeContext { resource_loader: provider, version, species: None, param_name: None };
        param.display_name(&cx)
    });
    if upgrade_or_signal.is_some() {
        return upgrade_or_signal;
    }

    let skill = CrewSkillName::from(source);
    let (primary, fallback) = skill_translation_keys_for(&skill, "IDS_SKILL", version);

    provider
        .localized_name_from_id(&TranslationKey::new(primary))
        .or_else(|| provider.localized_name_from_id(&TranslationKey::new(fallback)))
}

#[cfg(test)]
mod tests {
    use wows_replay_insights::fire_chance::analysis::FormulaOp;
    use wows_replay_insights::fire_chance::analysis::FormulaStep;

    use super::*;
    use crate::test_support::fire_chance_result;
    use crate::test_support::per_ship_fire_chance;

    #[test]
    fn named_ids_read_by_name_and_the_rest_by_id() {
        let mut fire_chance = fire_chance_result();
        let step = |source: &str| FormulaStep {
            modifier: "burnChanceFactorHighLevel".to_owned(),
            source: Some(source.to_owned()),
            op: FormulaOp::Multiply,
            value: 1.1,
            result: 0.13,
        };
        fire_chance.formula = vec![step("PCEF003_Signal_VictorLima"), step("PCM_Retired")];
        fire_chance.per_ship =
            vec![per_ship_fire_chance("4181604048", "PJSB018_Yamato_1944"), per_ship_fire_chance("1", "PASB_Retired")];
        let names = FireChanceNames::from_lookups(
            &fire_chance,
            |source| (source == "PCEF003_Signal_VictorLima").then(|| "Victor Lima".to_owned()),
            |index| (index == "4181604048").then(|| "Yamato".to_owned()),
        );

        assert_eq!(names.source("PCEF003_Signal_VictorLima"), "Victor Lima");
        assert_eq!(names.source("PCM_Retired"), "PCM_Retired");
        assert_eq!(names.ship(&fire_chance.per_ship[0]), "Yamato");
        assert_eq!(names.ship(&fire_chance.per_ship[1]), "PASB_Retired");
    }
}
