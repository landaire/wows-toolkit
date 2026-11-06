use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::TranslationKey;
use wowsunpack::game_params::modifier_settings_data::describe_modifiers;
use wowsunpack::game_params::skill_grid_data::skill_grid;
use wowsunpack::game_params::types::CrewSkill;
use wowsunpack::game_params::types::CrewSkillModifier;
use wowsunpack::game_params::types::Species;

use crate::data::GameData;
use crate::error::QueryError;
use crate::fuzzy;
use crate::fuzzy::Candidate;
use crate::fuzzy::Fuzzy;
use crate::ships::ShipClass;
use crate::ships::search_key;
use crate::text::clean;

/// Community acronyms and the internal names they stand for.
const ACRONYMS: &[(&str, &[&str])] = &[
    ("ifhe", &["HePenetration"]),
    ("si", &["ConsumablesAdditional"]),
    ("ce", &["DetectionVisibilityRange"]),
    ("ar", &["ArmamentReloadAaDamage", "ArmamentReloadSubmarine"]),
    ("de", &["HeFireProbability"]),
    ("fp", &["DefenceFireProbability"]),
    ("fpe", &["DefenceFireProbability"]),
    ("pt", &["DetectionAiming"]),
    ("bos", &["DefenceCritFireFlooding"]),
    ("gtg", &["GmTurn"]),
    ("cqc", &["TriggerGmAtbaReloadBb"]),
    ("ls", &["Maneuverability"]),
    ("rl", &["DetectionDirection"]),
    ("pm", &["DefenseCritProbability"]),
    ("se", &["DefenseHp", "PlanesHp"]),
    ("shap", &["ApDamageBb"]),
    ("msba", &["AtbaAccuracy"]),
    ("ifa", &["DetectionAlert"]),
];

/// Arguments for `captain_skill`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CaptainSkillArgs {
    /// Skill name, e.g. "Inertia Fuse for HE Shells", or a common acronym such as IFHE, SI, CE, AR, DE, BoS.
    pub skill: String,

    /// Only describe the skill for this class.
    pub class: Option<ShipClass>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaptainSkillOutput {
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
        let mut skills = self.match_skills(&args.skill)?;
        if let Some(class) = args.class {
            skills.retain(|skill| self.skill_points(skill, class).is_some());
            if skills.is_empty() {
                return Err(QueryError::SkillNotForClass { skill: args.skill.clone(), class: class.label() });
            }
        }

        let classes: Vec<ShipClass> = match args.class {
            Some(class) => vec![class],
            None => ShipClass::ALL.to_vec(),
        };
        let skills = skills.into_iter().map(|skill| self.skill_details(skill, &classes)).collect();

        Ok(CaptainSkillOutput { skills })
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
        skill
            .translated_name(self.provider(), &self.version())
            .map(|name| clean(&name))
            .unwrap_or_else(|| skill.internal_name().as_str().to_owned())
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
    ) -> Result<(&CrewSkill, u8), QueryError> {
        let skills = self.match_skills(query)?;

        skills
            .into_iter()
            .find_map(|skill| self.skill_points(skill, class).map(|points| (skill, points)))
            .ok_or_else(|| QueryError::SkillNotForClass { skill: query.to_owned(), class: class.label() })
    }

    /// Skills a player-written name, acronym or internal name refers to.
    fn match_skills(&self, query: &str) -> Result<Vec<&CrewSkill>, QueryError> {
        let key = search_key(query);
        let by_internal = |names: &[&str]| -> Vec<&CrewSkill> {
            self.skills.iter().filter(|skill| names.contains(&skill.internal_name().as_str())).collect()
        };

        if let Some((_, names)) = ACRONYMS.iter().find(|(acronym, _)| *acronym == key) {
            let skills = by_internal(names);
            if skills.is_empty() {
                return Err(QueryError::SkillNotFound { query: query.to_owned(), suggestions: Vec::new() });
            }
            return Ok(skills);
        }

        let named: Vec<(&CrewSkill, String)> =
            self.skills.iter().map(|skill| (skill, search_key(&self.skill_name(skill)))).collect();
        let exact: Vec<&CrewSkill> = named
            .iter()
            .filter(|(skill, name)| *name == key || skill.internal_name().as_str().eq_ignore_ascii_case(query.trim()))
            .map(|(skill, _)| *skill)
            .collect();
        if !exact.is_empty() {
            return Ok(exact);
        }

        let matching = |accept: &dyn Fn(&str) -> bool| -> Vec<&CrewSkill> {
            named.iter().filter(|(_, name)| !key.is_empty() && accept(name)).map(|(skill, _)| *skill).collect()
        };
        let prefix = matching(&|name| name.starts_with(key.as_str()));
        let partial = if prefix.is_empty() { matching(&|name| name.contains(key.as_str())) } else { prefix };
        let mut distinct: Vec<String> = partial.iter().map(|skill| self.skill_name(skill)).collect();
        distinct.sort();
        distinct.dedup();
        match distinct.len() {
            0 => {}
            1 => return Ok(partial),
            _ => return Err(QueryError::AmbiguousSkill { query: query.to_owned(), candidates: distinct }),
        }

        let names: Vec<String> = named.iter().map(|(skill, _)| self.skill_name(skill)).collect();
        let candidates = named.iter().zip(&names).map(|((_, key), name)| Candidate::primary(key, name));
        match fuzzy::closest(&key, candidates) {
            Fuzzy::Unique(name) => Ok(names
                .iter()
                .zip(&named)
                .filter(|(skill_name, _)| *skill_name == name)
                .map(|(_, (skill, _))| *skill)
                .collect()),
            Fuzzy::Suggestions(suggestions) => Err(QueryError::SkillNotFound {
                query: query.to_owned(),
                suggestions: suggestions.into_iter().map(str::to_owned).collect(),
            }),
        }
    }

    fn skill_details(&self, skill: &CrewSkill, classes: &[ShipClass]) -> CaptainSkillDetails {
        let version = self.version();
        let trigger = skill.logic_trigger();
        let trigger_sentence = trigger
            .and_then(|trigger| trigger.trigger_type())
            .filter(|kind| !kind.is_empty())
            .and_then(|kind| {
                self.provider()
                    .localized_name_from_id(&TranslationKey::new(format!("IDS_SKILL_TRIGGER_{}", kind.to_uppercase())))
            })
            .map(|sentence| clean(&sentence))
            .filter(|sentence| !sentence.is_empty());

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
