use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use wows_core::Version;
use wowsunpack::data::ResourceLoader;
use wowsunpack::game_params::modifier_settings_data::describe_modifiers;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::skill_grid_data::skill_grid;
use wowsunpack::game_params::types::CrewSkill;
use wowsunpack::game_params::types::CrewSkillModifier;
use wowsunpack::game_params::types::Species;

use crate::data::GameData;
use crate::error::QueryError;
use crate::names::Interpretations;
use crate::names::Lookup;
use crate::names::NameIndex;
use crate::names::NameKind;
use crate::names::ResolutionMethod;
use crate::ships::ShipClass;
use crate::ships::ShipSummary;
use crate::text::clean;

/// Community abbreviations and former names of captain skills, with the
/// internal name of the skill they mean today. Initialisms of current names
/// ("MBAAS", "DFE") resolve without an entry; these are the ones that differ
/// from the current name or clash with another skill's initialism ("CE" is
/// also Consumable Enhancements). The pre-0.10 Basic and Advanced Firing
/// Training skills became the main battery and AA skills with the same
/// modifier families (`GMShotDelay` and `GMMaxDist`, each with AA). Every
/// target is checked by the `alias_tables_resolve_to_their_targets` test.
pub(crate) const SKILL_ALIASES: &[(&str, &str, ResolutionMethod)] = &[
    ("IFHE", "HePenetration", ResolutionMethod::ExactAlias),
    ("SI", "ConsumablesAdditional", ResolutionMethod::ExactAlias),
    ("CE", "DetectionVisibilityRange", ResolutionMethod::ExactAlias),
    ("AR", "ArmamentReloadAaDamage", ResolutionMethod::ExactAlias),
    ("DE", "HeFireProbability", ResolutionMethod::ExactAlias),
    ("FP", "DefenceFireProbability", ResolutionMethod::ExactAlias),
    ("FPE", "DefenceFireProbability", ResolutionMethod::ExactAlias),
    ("PT", "DetectionAiming", ResolutionMethod::ExactAlias),
    ("BoS", "DefenceCritFireFlooding", ResolutionMethod::ExactAlias),
    ("GtG", "GmTurn", ResolutionMethod::ExactAlias),
    ("CQC", "TriggerGmAtbaReloadBb", ResolutionMethod::ExactAlias),
    ("LS", "Maneuverability", ResolutionMethod::ExactAlias),
    ("RL", "DetectionDirection", ResolutionMethod::ExactAlias),
    ("PM", "DefenseCritProbability", ResolutionMethod::ExactAlias),
    ("SE", "DefenseHp", ResolutionMethod::ExactAlias),
    ("SHAP", "ApDamageBb", ResolutionMethod::ExactAlias),
    ("MSBA", "AtbaAccuracy", ResolutionMethod::ExactAlias),
    ("IFA", "DetectionAlert", ResolutionMethod::ExactAlias),
    ("RPF", "DetectionDirection", ResolutionMethod::Historical),
    ("Radio Position Finding", "DetectionDirection", ResolutionMethod::Historical),
    ("BFT", "GmReloadAaDamageConstant", ResolutionMethod::Historical),
    ("Basic Firing Training", "GmReloadAaDamageConstant", ResolutionMethod::Historical),
    ("AFT", "GmRangeAaDamageBubbles", ResolutionMethod::Historical),
    ("Advanced Firing Training", "GmRangeAaDamageBubbles", ResolutionMethod::Historical),
    ("MFCSA", "AtbaAccuracy", ResolutionMethod::Historical),
    ("Manual Fire Control for Secondary Armament", "AtbaAccuracy", ResolutionMethod::Historical),
];

/// Arguments for `captain_skill`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CaptainSkillArgs {
    /// Skill name, e.g. "Inertia Fuse for HE Shells", a common abbreviation such as IFHE, SI, CE, SE, PT, IFA, AR, DE, BoS, or a former name such as BFT.
    pub skill: String,

    /// Only describe the skill for this class.
    pub class: Option<ShipClass>,

    /// Only describe the skill for this ship's class, e.g. "GK". Overrides class.
    pub ship: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaptainSkillOutput {
    /// Names read as something they do not spell exactly, or as one of several ships sharing a name.
    #[serde(skip_serializing_if = "Interpretations::is_empty")]
    pub interpretations: Interpretations,

    /// The ship named in the arguments.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ship: Option<ShipSummary>,

    /// Usually one skill. Several when a name is used by one skill per class,
    /// such as Adrenaline Rush.
    pub skills: Vec<CaptainSkillDetails>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaptainSkillDetails {
    pub name: String,
    pub internal_name: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Condition under which `triggered_effects` apply.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger: Option<String>,

    /// Point cost and effects, grouped by classes that share them.
    pub classes: Vec<SkillClassEffects>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SkillClassEffects {
    pub classes: Vec<ShipClass>,
    pub points: u8,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<String>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub triggered_effects: Vec<String>,
}

/// Arguments for `skill_tree`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SkillTreeArgs {
    pub class: ShipClass,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillTree {
    pub class: ShipClass,
    pub rows: Vec<SkillTreeRow>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillTreeRow {
    pub points: u8,
    pub skills: Vec<String>,
}

impl GameData {
    /// One captain skill: point cost and effects per class.
    pub fn captain_skill(&self, args: &CaptainSkillArgs) -> Result<CaptainSkillOutput, QueryError> {
        let mut interpretations = Interpretations::default();
        let ship = match args.ship.as_deref() {
            Some(name) => Some(self.resolve_ship(name, &mut interpretations)?),
            None => None,
        };
        let class = ship.map(|ship| ship.class).or(args.class);

        let mut skills = self.match_skills(&args.skill, &mut interpretations)?;
        if let Some(class) = class {
            let all = skills.clone();
            skills.retain(|skill| self.skill_points(skill, class).is_some());
            if skills.is_empty() {
                return Err(self.skill_not_for_class(&args.skill, &all, class, ship.map(|ship| ship.name.as_str())));
            }
        }

        let classes: Vec<ShipClass> = match class {
            Some(class) => vec![class],
            None => ShipClass::ALL.to_vec(),
        };
        let skills = skills.into_iter().map(|skill| self.skill_details(skill, &classes)).collect();

        Ok(CaptainSkillOutput { interpretations, ship: ship.map(|ship| ship.summary()), skills })
    }

    /// The captain skill tree of one class, by point cost.
    pub fn skill_tree(&self, args: &SkillTreeArgs) -> SkillTree {
        let mut rows: BTreeMap<u8, Vec<String>> = BTreeMap::new();
        for slot in skill_grid(&self.version(), args.class.species()).unwrap_or_default() {
            let name = self
                .skills
                .iter()
                .find(|skill| skill.internal_name().as_str() == slot.skill)
                .map_or_else(|| slot.skill.to_owned(), |skill| self.skill_name(skill));
            rows.entry(slot.tier + 1).or_default().push(name);
        }

        SkillTree {
            class: args.class,
            rows: rows.into_iter().map(|(points, skills)| SkillTreeRow { points, skills }).collect(),
        }
    }

    /// English name of a skill, or its internal name when it has none.
    pub(crate) fn skill_name(&self, skill: &CrewSkill) -> String {
        english_skill_name(skill, self.provider(), &self.version())
    }

    /// Point cost of `skill` in the class's tree, `None` when it is not in it.
    pub(crate) fn skill_points(&self, skill: &CrewSkill, class: ShipClass) -> Option<u8> {
        skill_grid(&self.version(), class.species())?
            .iter()
            .find(|slot| slot.skill == skill.internal_name().as_str())
            .map(|slot| slot.tier + 1)
    }

    /// The skill named `query` in the class's tree.
    pub(crate) fn resolve_skill_for_class(
        &self,
        query: &str,
        class: ShipClass,
        ship: &str,
        interpretations: &mut Interpretations,
    ) -> Result<(&CrewSkill, u8), QueryError> {
        let skills = self.match_skills(query, interpretations)?;

        skills
            .iter()
            .find_map(|skill| self.skill_points(skill, class).map(|points| (*skill, points)))
            .ok_or_else(|| self.skill_not_for_class(query, &skills, class, Some(ship)))
    }

    /// `skills` are every skill `query` matched; none is in `class`'s tree.
    fn skill_not_for_class(
        &self,
        query: &str,
        skills: &[&CrewSkill],
        class: ShipClass,
        ship: Option<&str>,
    ) -> QueryError {
        let classes = ShipClass::ALL
            .into_iter()
            .filter(|other| skills.iter().any(|skill| self.skill_points(skill, *other).is_some()))
            .collect();
        let skill = skills.first().map_or_else(|| query.to_owned(), |skill| self.skill_name(skill));

        QueryError::SkillNotForClass { query: query.to_owned(), skill, class, ship: ship.map(str::to_owned), classes }
    }

    /// Skills a player-written name, abbreviation or internal name refers
    /// to: one per class tree that names a skill this way.
    fn match_skills(&self, query: &str, interpretations: &mut Interpretations) -> Result<Vec<&CrewSkill>, QueryError> {
        match self.skill_names.lookup(query) {
            Lookup::Found(resolved, targets) => {
                interpretations.note(query, &resolved, NameKind::Skill);
                Ok(targets.iter().map(|index| &self.skills[*index]).collect())
            }
            Lookup::Ambiguous(candidates) => Err(QueryError::AmbiguousSkill { query: query.to_owned(), candidates }),
            Lookup::NotFound(suggestions) => Err(QueryError::SkillNotFound { query: query.to_owned(), suggestions }),
        }
    }

    /// The condition under which a skill's triggered effects apply.
    pub(crate) fn skill_trigger_sentence(&self, skill: &CrewSkill) -> Option<String> {
        skill
            .logic_trigger()
            .and_then(|trigger| trigger.trigger_desc_key())
            .and_then(|key| self.provider().localized_name_from_id(&key))
            .map(|sentence| clean(&sentence))
            .filter(|sentence| !sentence.is_empty())
    }

    fn skill_details(&self, skill: &CrewSkill, classes: &[ShipClass]) -> CaptainSkillDetails {
        let version = self.version();
        let trigger = skill.logic_trigger();
        let trigger_sentence = self.skill_trigger_sentence(skill);

        let mut grouped: Vec<SkillClassEffects> = Vec::new();
        for &class in classes {
            let Some(points) = self.skill_points(skill, class) else {
                continue;
            };
            let species = class.species();
            let effects = self.describe(skill.modifiers().map(Vec::as_slice).unwrap_or_default(), species);
            let triggered_effects = self.describe(
                trigger.and_then(|trigger| trigger.modifiers()).map(Vec::as_slice).unwrap_or_default(),
                species,
            );

            match grouped.iter_mut().find(|group| {
                group.points == points && group.effects == effects && group.triggered_effects == triggered_effects
            }) {
                Some(group) => group.classes.push(class),
                None => grouped.push(SkillClassEffects { classes: vec![class], points, effects, triggered_effects }),
            }
        }

        CaptainSkillDetails {
            name: self.skill_name(skill),
            internal_name: skill.internal_name().as_str().to_owned(),
            description: skill.translated_description(self.provider(), &version).map(|text| clean(&text)),
            trigger: trigger_sentence,
            classes: grouped,
        }
    }

    /// Formatted modifier lines for one class.
    pub(crate) fn describe(&self, modifiers: &[CrewSkillModifier], species: Species) -> Vec<String> {
        describe_modifiers(
            self.version(),
            modifiers.iter().map(|modifier| (modifier.name(), modifier.get_for_species(&species))),
            species,
            self.provider(),
        )
        .iter()
        .map(|line| clean(line))
        .collect()
    }
}

fn english_skill_name(skill: &CrewSkill, provider: &GameMetadataProvider, version: &Version) -> String {
    skill
        .translated_name(provider, version)
        .map(|name| clean(&name))
        .unwrap_or_else(|| skill.internal_name().as_str().to_owned())
}

/// Skill names, internal names and [`SKILL_ALIASES`].
pub(crate) fn skill_names(
    provider: &GameMetadataProvider,
    version: &Version,
    skills: &[CrewSkill],
) -> NameIndex<usize> {
    let mut names = NameIndex::default();
    for (index, skill) in skills.iter().enumerate() {
        let name = english_skill_name(skill, provider, version);
        names.add_name(&name, index, false);
        names.add_id(skill.internal_name().as_str(), &name, index);
    }
    for (alias, internal_name, method) in SKILL_ALIASES {
        names.add_alias(alias, internal_name, *method);
    }
    names
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use wows_data_mgr::builds::BuildsIndex;
    use wowsunpack::game_params::modifier_settings_data::Measure;
    use wowsunpack::game_params::modifier_settings_data::Transform;
    use wowsunpack::game_params::modifier_settings_data::format_modifier;
    use wowsunpack::game_params::modifier_settings_data::modifier_setting;

    use super::*;

    /// Percent effects checked, and the ones whose displayed number is
    /// implausible: over 100%, or over 100% in total at 1% HP.
    #[derive(Debug, Default, Serialize)]
    struct EffectAudit {
        checked: usize,
        implausible: Vec<String>,
    }

    impl EffectAudit {
        /// Recomputes the client's percent for one modifier and checks the
        /// formatted line shows it.
        fn check(&mut self, game: &GameData, source: &str, class: ShipClass, modifier: &CrewSkillModifier) {
            let version = game.version();
            let name = modifier.name();
            let value = modifier.get_for_species(&class.species());
            let Some(setting) = modifier_setting(version, name) else {
                return;
            };
            let shows_number = setting.measure == Measure::Percent
                && setting.transform == Transform::None
                && !setting.hidden
                && !setting.measure_value_hidden
                && value != setting.base_value
                && !setting.translations.iter().any(|(key, _)| *key == value);
            if !shows_number {
                return;
            }

            // ModifierSettings.py:264-282.
            let expected = (value * setting.multiplier - setting.base_value) * 100.0;
            let line = format_modifier(version, name, value, class.species(), game.provider())
                .map(|line| clean(&line))
                .unwrap_or_else(|| panic!("{source}: {name} = {value} renders"));
            let shown = leading_percent(&line).unwrap_or_else(|| panic!("{source}: no percent in {line:?}"));
            let tolerance =
                if expected.abs() < 2.0 { 0.005 } else { 0.5 / 10f32.powi(i32::from(setting.round_digits)) };
            assert!(
                (shown - expected).abs() <= tolerance + 1e-3,
                "{source}: {name} = {value} shows {shown}, the client {expected}"
            );

            let at_one_percent_hp = line
                .strip_suffix(" at 1% HP)")
                .and_then(|rest| rest.rsplit_once('('))
                .and_then(|(_, total)| leading_percent(total));
            self.checked += 1;
            if shown.abs() > 100.0 || at_one_percent_hp.is_some_and(|total| total.abs() > 100.0) {
                self.implausible.push(format!("{source} ({}): {line}", class.label()));
            }
        }
    }

    /// The number before the first `%` of a formatted effect.
    fn leading_percent(text: &str) -> Option<f32> {
        text.split_once('%')?.0.parse().ok()
    }

    #[test]
    #[cfg_attr(not(has_build_13533378), ignore)]
    fn skill_and_upgrade_effects_show_plausible_percents() {
        let root = PathBuf::from(std::env::var_os("WOWS_GAME_DATA").expect("WOWS_GAME_DATA names the dump root"));
        let index = BuildsIndex::load(&root.join("builds.toml")).expect("dump root has a readable builds.toml");
        let entry = index.find_by_build(13533378).expect("dump root lists build 13533378");
        let cache = std::env::temp_dir().join("wows-query-test-cache");
        let game = GameData::load_dump(&root.join(&entry.dir), Some(&cache)).expect("dump loads");

        let mut audit = EffectAudit::default();
        for skill in &game.skills {
            let source = format!("skill {}", game.skill_name(skill));
            let trigger = skill.logic_trigger();
            let modifiers = skill
                .modifiers()
                .into_iter()
                .flatten()
                .chain(trigger.and_then(|trigger| trigger.modifiers()).into_iter().flatten())
                .chain(
                    trigger.into_iter().flat_map(|trigger| trigger.count_to_modifier()).flat_map(|(_, block)| block),
                );
            let classes: Vec<ShipClass> =
                ShipClass::ALL.iter().copied().filter(|class| game.skill_points(skill, *class).is_some()).collect();
            for modifier in modifiers {
                for &class in &classes {
                    audit.check(&game, &source, class, modifier);
                }
            }
        }
        for upgrade in &game.upgrades {
            let source = format!("upgrade {}", upgrade.name);
            for modifier in upgrade.modernization().modifiers() {
                for class in upgrade.classes() {
                    audit.check(&game, &source, class, modifier);
                }
            }
        }
        audit.implausible.sort();
        audit.implausible.dedup();

        insta::assert_yaml_snapshot!(audit);
    }
}
