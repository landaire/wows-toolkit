use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use wows_core::Version;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::TranslationKey;
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
    /// Names read as something they do not spell exactly.
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
            skills.retain(|skill| self.skill_points(skill, class).is_some());
            if skills.is_empty() {
                return Err(QueryError::SkillNotForClass { skill: args.skill.clone(), class: class.label() });
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
        interpretations: &mut Interpretations,
    ) -> Result<(&CrewSkill, u8), QueryError> {
        let skills = self.match_skills(query, interpretations)?;

        skills
            .into_iter()
            .find_map(|skill| self.skill_points(skill, class).map(|points| (skill, points)))
            .ok_or_else(|| QueryError::SkillNotForClass { skill: query.to_owned(), class: class.label() })
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
