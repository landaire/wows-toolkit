//! Unique commanders: their improved captain skills and their talents.
//!
//! A crew carries its own captain skill table, and the client builds a
//! crew's skill modifiers from it (`CrewCache.py:44`). A unique commander's
//! table marks the entries that differ from the standard skill `isEpic`
//! (`hasEpicSkills`, `mdc440371.py:603`): learning the skill with that
//! commander applies the improved entry instead of the standard one.
//!
//! Talents are the crew's `UniqueSkills`. Each activation applies the talent
//! block of that level, the base block updated with `level_N`
//! (`ParamsByLevel.py`), and mixes it into the ship's talent modifiers, so
//! the effects of successive activations stack (`Components/UniqueSkills.py`
//! `UniqueSkill.activate` and `__updateModifier`). A regeneration talent
//! starts a heal over time instead; an activation while one runs adds the
//! rest of the running heal as a bonus (`UniqueSkills.onRegenStart`). A
//! talent works only in the battle types of its `battleGroup`
//! (`UniqueSkills._isBattleGroupAllowed`).

use std::borrow::Cow;
use std::num::NonZeroUsize;

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use serde::Serializer;
use wows_core::units::Seconds;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::TranslationKey;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::ttx::modifiers::RELOAD_FACTOR;
use wowsunpack::game_params::ttx::modifiers::fold_modifier_values;
use wowsunpack::game_params::ttx::render_attributions;
use wowsunpack::game_params::types::Crew;
use wowsunpack::game_params::types::CrewSkill;
use wowsunpack::game_params::types::CrewSkillModifier;
use wowsunpack::game_params::types::CrewTalent;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::KnownTalentEffect;
use wowsunpack::game_params::types::Species;
use wowsunpack::game_params::types::TalentBattleGroup;
use wowsunpack::game_params::types::TalentEffect;

use crate::data::GameData;
use crate::error::QueryError;
use crate::loadout::ResolvedLoadout;
use crate::names::Interpretations;
use crate::names::Lookup;
use crate::names::NameIndex;
use crate::names::NameKind;
use crate::names::search_key;
use crate::ships::Ship;
use crate::ships::ShipClass;
use crate::ships::ShipSummary;
use crate::special_abilities::Burst;
use crate::stats::Card;
use crate::stats::StatChange;
use crate::stats::changes;
use crate::stats::derived_changes;
use crate::text::clean;

/// Arguments for `commander`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CommanderArgs {
    /// Unique commander name, e.g. "Yamamoto Isoroku", "Halsey", "Jean-Jacques Honore" or "John Doe".
    pub commander: String,

    /// Only describe the commander for this ship's class, e.g. "Yamato".
    pub ship: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CommanderOutput {
    /// Names read as something they do not spell exactly.
    #[serde(skip_serializing_if = "Interpretations::is_empty")]
    pub interpretations: Interpretations,

    /// The ship named in the arguments.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ship: Option<ShipSummary>,

    pub commander: CommanderDetails,

    /// Battle types in which no talent works.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub talent_restrictions: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CommanderDetails {
    pub name: String,

    /// Nations whose ships the commander serves on, as GameParams spells them.
    pub nations: Vec<String>,

    /// Ships the commander serves on besides the ships of `nations`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ships: Vec<String>,

    /// Captain skills this commander has an improved version of. Learning
    /// the skill with this commander applies the improved effects instead of
    /// the standard ones.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub improved_skills: Vec<ImprovedSkill>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub talents: Vec<Talent>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImprovedSkill {
    pub name: String,

    /// Point cost and effects, standard and improved, grouped by classes
    /// that share them.
    pub classes: Vec<ImprovedSkillClass>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ImprovedSkillClass {
    pub classes: Vec<ShipClass>,
    pub points: u8,
    pub standard: Vec<String>,
    pub improved: Vec<String>,
}

/// A talent: activated in battle by a condition, then changing the ship.
#[derive(Debug, Clone, Serialize)]
pub struct Talent {
    pub name: String,

    /// Ship classes the talent works on, when not every class.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub classes: Vec<ShipClass>,

    /// One per set of battle types the talent is tuned for.
    pub variants: Vec<TalentVariant>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TalentVariant {
    pub battle_types: &'static str,

    /// What activates the talent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,

    /// How the effect grows over activations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progression: Option<String>,

    pub activations_per_battle: ActivationLimit,

    /// Effects per activation. One entry without `activation` when every
    /// activation does the same.
    pub activations: Vec<TalentActivation>,

    /// Modifier effects with every activation of a battle taken, for
    /// talents that activate more than once.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub after_all_activations: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TalentActivation {
    /// 1 for the first activation in a battle.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activation: Option<u32>,

    /// What this activation needs, when it differs per activation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,

    pub effects: Vec<String>,
}

/// How often a talent can activate in one battle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationLimit {
    Unlimited,
    PerBattle(u32),
}

impl Serialize for ActivationLimit {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Unlimited => serializer.serialize_str("unlimited"),
            Self::PerBattle(count) => serializer.serialize_u32(*count),
        }
    }
}

/// A talent's effect on the stat card in `ship_stats`, after `activations`.
#[derive(Debug, Clone, Serialize)]
pub struct TalentStatChanges {
    pub talent: String,

    /// What activates the talent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,

    /// Activations counted, for talents that activate more than once.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activations: Option<u32>,

    /// What the last activation counted needs, when activations differ.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activation_condition: Option<String>,

    pub changes: Vec<StatChange>,
}

/// Arguments for `commander_search`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CommanderSearchArgs {
    /// Words an improved skill or talent must mention in its name, condition or effects, e.g. "restores HP", "heal", "reload", "speed", "fires". Omit to list every unique commander.
    pub effect: Option<String>,

    /// Nation as GameParams spells it, e.g. "Japan", "USA", "United_Kingdom", "Pan_Asia".
    pub nation: Option<String>,

    /// Only commanders who serve on this ship, with effects for its class.
    pub ship: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CommanderSearchOutput {
    #[serde(skip_serializing_if = "Interpretations::is_empty")]
    pub interpretations: Interpretations,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub ship: Option<ShipSummary>,

    /// Commanders with a match.
    pub matched: usize,

    /// Each commander with only the improved skills and talents that match.
    pub commanders: Vec<CommanderDetails>,

    /// Battle types in which no talent works.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub talent_restrictions: Option<String>,
}

/// A unique commander with improved skills or talents.
#[derive(Debug)]
pub(crate) struct Commander {
    pub(crate) name: String,

    /// GameParams index, e.g. "PJW018", the stem of talent translation keys.
    pub(crate) index: String,

    pub(crate) crew: Crew,

    /// Indexes into the standard skill table, each with the improved entry
    /// that replaces it.
    pub(crate) improved: Vec<(usize, CrewSkill)>,
}

impl Commander {
    /// Unique commanders with improved skills or talents, by name, and the
    /// names that resolve to them.
    pub(crate) fn index(provider: &GameMetadataProvider, standard: &[CrewSkill]) -> (Vec<Self>, NameIndex<usize>) {
        let mut commanders = Vec::new();
        for param in provider.params() {
            let Some(crew) = param.crew() else {
                continue;
            };
            if !crew.personality().is_unique() {
                continue;
            }

            let improved: Vec<(usize, CrewSkill)> = crew
                .skills()
                .unwrap_or_default()
                .iter()
                .filter(|skill| skill.is_epic())
                .filter_map(|skill| {
                    standard
                        .iter()
                        .position(|other| other.internal_name() == skill.internal_name())
                        .map(|index| (index, skill.clone()))
                })
                .collect();
            if improved.is_empty() && crew.talents().is_empty() {
                continue;
            }

            // Crews without a person name or localized name are templates, not commanders.
            let Some(person_name) = crew.personality().name() else {
                continue;
            };
            let key = TranslationKey::new(format!("IDS_{}", person_name.to_uppercase()));
            let Some(name) = provider.localized_name_from_id(&key) else {
                continue;
            };

            commanders.push(Self { name: clean(&name), index: param.index().to_owned(), crew: crew.clone(), improved });
        }
        commanders.sort_by(|left, right| left.name.cmp(&right.name).then_with(|| left.index.cmp(&right.index)));

        let mut names = NameIndex::default();
        for (position, commander) in commanders.iter().enumerate() {
            names.add_name(&commander.name, position, false);
            names.add_id(&commander.index, &commander.name, position);
        }

        (commanders, names)
    }

    /// Whether the commander can command `ship`.
    pub(crate) fn serves(&self, ship: &Ship) -> bool {
        let ships = self.crew.personality().ships();

        ships.nations().iter().any(|nation| nation == ship.param.nation())
            || ships.ships().iter().any(|name| name == ship.param.name())
    }

    /// The commander's improved version of `standard`.
    pub(crate) fn improved_skill(&self, standard: &CrewSkill) -> Option<&CrewSkill> {
        self.improved
            .iter()
            .find(|(_, skill)| skill.internal_name() == standard.internal_name())
            .map(|(_, skill)| skill)
    }

    /// The talents that work in random battles: the regular variants and
    /// those for every battle type.
    pub(crate) fn random_battle_talents(&self) -> impl Iterator<Item = &CrewTalent> {
        self.crew
            .talents()
            .iter()
            .filter(|talent| matches!(talent.battle_group(), TalentBattleGroup::Every | TalentBattleGroup::Regular))
    }
}

impl GameData {
    /// One unique commander: improved skills against the standard ones, and
    /// talents with what activates them and what they do.
    pub fn commander(&self, args: &CommanderArgs) -> Result<CommanderOutput, QueryError> {
        let mut interpretations = Interpretations::default();
        let commander = self.resolve_commander(&args.commander, &mut interpretations)?;
        let ship = match args.ship.as_deref() {
            Some(name) => Some(self.resolve_ship(name, &mut interpretations)?),
            None => None,
        };
        if let Some(ship) = ship {
            self.check_serves(commander, ship)?;
        }

        let details = self.commander_details(commander, ship.map(|ship| ship.class), &|_| true);
        let talent_restrictions = (!details.talents.is_empty()).then(|| self.talent_restrictions()).flatten();

        Ok(CommanderOutput { interpretations, ship: ship.map(Ship::summary), commander: details, talent_restrictions })
    }

    /// Unique commanders whose improved skills or talents mention `effect`.
    pub fn commander_search(&self, args: &CommanderSearchArgs) -> Result<CommanderSearchOutput, QueryError> {
        let mut interpretations = Interpretations::default();
        let ship = match args.ship.as_deref() {
            Some(name) => Some(self.resolve_ship(name, &mut interpretations)?),
            None => None,
        };
        let terms = args.effect.as_deref().map(search_terms).unwrap_or_default();

        let mut commanders = Vec::new();
        for commander in &self.commanders {
            if let Some(nation) = args.nation.as_deref()
                && !commander
                    .crew
                    .personality()
                    .ships()
                    .nations()
                    .iter()
                    .any(|served| served.eq_ignore_ascii_case(nation))
            {
                continue;
            }
            if let Some(ship) = ship
                && !commander.serves(ship)
            {
                continue;
            }

            let details = self.commander_details(commander, ship.map(|ship| ship.class), &|text| {
                let key = search_key(text);
                terms.iter().all(|alternatives| alternatives.iter().any(|term| key.contains(term.as_str())))
            });
            if details.improved_skills.is_empty() && details.talents.is_empty() {
                continue;
            }
            commanders.push(details);
        }

        Ok(CommanderSearchOutput {
            interpretations,
            ship: ship.map(Ship::summary),
            matched: commanders.len(),
            commanders,
            talent_restrictions: self.talent_restrictions(),
        })
    }

    pub(crate) fn resolve_commander(
        &self,
        query: &str,
        interpretations: &mut Interpretations,
    ) -> Result<&Commander, QueryError> {
        match self.commander_names.lookup(query) {
            Lookup::Found(resolved, targets) => {
                interpretations.note(query, &resolved, NameKind::Commander);
                let [index] = targets[..] else {
                    let candidates = targets.iter().map(|index| self.commanders[*index].name.clone()).collect();
                    return Err(QueryError::AmbiguousCommander { query: query.to_owned(), candidates });
                };

                Ok(&self.commanders[index])
            }
            Lookup::Ambiguous(candidates) => {
                Err(QueryError::AmbiguousCommander { query: query.to_owned(), candidates })
            }
            Lookup::NotFound(suggestions) => {
                Err(QueryError::CommanderNotFound { query: query.to_owned(), suggestions })
            }
        }
    }

    pub(crate) fn check_serves(&self, commander: &Commander, ship: &Ship) -> Result<(), QueryError> {
        if commander.serves(ship) {
            return Ok(());
        }

        Err(QueryError::CommanderNotForShip {
            commander: commander.name.clone(),
            ship: ship.name.clone(),
            nations: commander.crew.personality().ships().nations().to_vec(),
            ships: self.extra_ships(commander),
        })
    }

    /// The standard skill table with `commander`'s improved entries in place.
    pub(crate) fn skill_table(&self, commander: Option<&Commander>) -> Cow<'_, [CrewSkill]> {
        let Some(commander) = commander else {
            return Cow::Borrowed(&self.skills);
        };

        let mut skills = self.skills.clone();
        for (index, skill) in &commander.improved {
            skills[*index] = skill.clone();
        }

        Cow::Owned(skills)
    }

    /// The commander's improved skills and talents for `class`, or for every
    /// class, keeping those `keep` accepts some text of.
    fn commander_details(
        &self,
        commander: &Commander,
        class: Option<ShipClass>,
        keep: &dyn Fn(&str) -> bool,
    ) -> CommanderDetails {
        let classes: Vec<ShipClass> = match class {
            Some(class) => vec![class],
            None => ShipClass::ALL.to_vec(),
        };

        let improved_skills = commander
            .improved
            .iter()
            .filter_map(|(index, improved)| self.improved_skill(&self.skills[*index], improved, &classes))
            .filter(|skill| {
                keep(&skill.name) || skill.classes.iter().any(|group| group.improved.iter().any(|line| keep(line)))
            })
            .collect();

        let talents = self
            .talents_where(commander, class, |_| true)
            .into_iter()
            .filter(|talent| {
                keep(&talent.name)
                    || talent.variants.iter().any(|variant| {
                        variant.condition.as_deref().is_some_and(keep)
                            || variant
                                .activations
                                .iter()
                                .any(|activation| activation.effects.iter().any(|line| keep(line)))
                    })
            })
            .collect::<Vec<_>>();

        CommanderDetails {
            name: commander.name.clone(),
            nations: commander.crew.personality().ships().nations().to_vec(),
            ships: self.extra_ships(commander),
            improved_skills,
            talents,
        }
    }

    /// English names of the ships the commander serves on besides the ships
    /// of their nations.
    fn extra_ships(&self, commander: &Commander) -> Vec<String> {
        commander
            .crew
            .personality()
            .ships()
            .ships()
            .iter()
            .map(|name| {
                self.provider()
                    .game_param_by_name(name)
                    .and_then(|param| self.provider().localized_name_from_param(&param))
                    .map_or_else(|| name.clone(), |localized| clean(&localized))
            })
            .collect()
    }

    fn improved_skill(
        &self,
        standard: &CrewSkill,
        improved: &CrewSkill,
        classes: &[ShipClass],
    ) -> Option<ImprovedSkill> {
        let mut grouped: Vec<ImprovedSkillClass> = Vec::new();
        for &class in classes {
            let Some(points) = self.skill_points(standard, class) else {
                continue;
            };
            let standard_lines = self.skill_lines(standard, class.species());
            let improved_lines = self.skill_lines(improved, class.species());
            if standard_lines == improved_lines {
                continue;
            }

            match grouped.iter_mut().find(|group| {
                group.points == points && group.standard == standard_lines && group.improved == improved_lines
            }) {
                Some(group) => group.classes.push(class),
                None => grouped.push(ImprovedSkillClass {
                    classes: vec![class],
                    points,
                    standard: standard_lines,
                    improved: improved_lines,
                }),
            }
        }

        (!grouped.is_empty()).then(|| ImprovedSkill { name: self.skill_name(standard), classes: grouped })
    }

    /// A skill's effects for one class: always-on effects, then triggered ones.
    fn skill_lines(&self, skill: &CrewSkill, species: Species) -> Vec<String> {
        let mut lines = self.describe(skill.modifiers().map(Vec::as_slice).unwrap_or_default(), species);
        lines.extend(self.describe(
            skill.logic_trigger().and_then(|trigger| trigger.modifiers()).map(Vec::as_slice).unwrap_or_default(),
            species,
        ));

        lines
    }

    /// The commander's talents for `class`, or for every class, that `keep`
    /// accepts, with the variants of one talent for different battle types
    /// together.
    pub(crate) fn talents_where(
        &self,
        commander: &Commander,
        class: Option<ShipClass>,
        keep: impl Fn(&CrewTalent) -> bool,
    ) -> Vec<Talent> {
        let mut talents: Vec<(u32, Talent)> = Vec::new();
        for talent in commander.crew.talents().iter().filter(|talent| keep(talent)) {
            let allowed: Vec<ShipClass> =
                talent.allowed_classes().iter().filter_map(|species| ShipClass::from_species(*species)).collect();
            if class.is_some_and(|class| !allowed.contains(&class)) {
                continue;
            }

            let key = talent.translation_key(&commander.index);
            let variant = self.talent_variant(&key, talent, class.map_or_else(|| allowed.clone(), |class| vec![class]));
            match talents.iter_mut().find(|(sort_index, _)| *sort_index == talent.sort_index()) {
                Some((_, existing)) => existing.variants.push(variant),
                None => {
                    let classes = if class.is_some() || ShipClass::ALL.iter().all(|class| allowed.contains(class)) {
                        Vec::new()
                    } else {
                        allowed
                    };
                    talents.push((
                        talent.sort_index(),
                        Talent { name: self.talent_name(&key), classes, variants: vec![variant] },
                    ));
                }
            }
        }

        talents.into_iter().map(|(_, talent)| talent).collect()
    }

    /// The talent's English name, or its effect key when the game has none.
    pub(crate) fn talent_name(&self, key: &str) -> String {
        self.translate(&format!("IDS_TALENT_{key}")).unwrap_or_else(|| format!("talent {key}"))
    }

    fn talent_variant(&self, key: &str, talent: &CrewTalent, classes: Vec<ShipClass>) -> TalentVariant {
        let limit = match talent.max_activations() {
            Some(count) => ActivationLimit::PerBattle(count),
            None => ActivationLimit::Unlimited,
        };
        let levels = activation_levels(talent);

        let activations = match levels {
            None => vec![TalentActivation {
                activation: None,
                condition: None,
                effects: self.talent_lines(talent, 1, &classes),
            }],
            Some(levels) => {
                let mut activations: Vec<TalentActivation> = Vec::new();
                for activation in 1..=levels.get() as u32 {
                    let effects = self.talent_lines(talent, activation, &classes);
                    // An activation that changes nothing further is left out.
                    if activations.last().is_some_and(|previous| previous.effects == effects) {
                        continue;
                    }
                    activations.push(TalentActivation {
                        activation: Some(activation),
                        condition: self.activation_condition(key, talent, activation),
                        effects,
                    });
                }
                activations
            }
        };

        // The total after every activation a battle allows, when the battle
        // allows more activations than the talent has levels.
        let after_all_activations = match limit {
            ActivationLimit::PerBattle(count)
                if count > 1 && levels.is_none_or(|levels| count as usize > levels.get()) =>
            {
                self.talent_total_lines(talent, count, &classes)
            }
            _ => Vec::new(),
        };

        TalentVariant {
            battle_types: battle_types(talent.battle_group()),
            condition: self.talent_condition(key, talent),
            // A talent without level blocks has zero levels.
            progression: self
                .translate(&format!("IDS_TALENT_{key}_PROGRESSION_DESC"))
                .map(|text| text.replace("%(_levelsCount)i", &levels.map_or(0, NonZeroUsize::get).to_string())),
            activations_per_battle: limit,
            activations,
            after_all_activations,
        }
    }

    /// What activates the talent in its battle types.
    pub(crate) fn talent_condition(&self, key: &str, talent: &CrewTalent) -> Option<String> {
        let group = match talent.battle_group() {
            TalentBattleGroup::Operations => "OPERATIONS",
            _ => "REGULAR",
        };

        self.translate(&format!("IDS_TALENT_{key}_DESCRIPTION_BATTLE_GROUP_{group}"))
    }

    /// What the Nth activation needs: the talent's per-activation text with
    /// the number its activator's level sets.
    pub(crate) fn activation_condition(&self, key: &str, talent: &CrewTalent, activation: u32) -> Option<String> {
        let amount = talent
            .activators()
            .iter()
            .filter_map(|activator| activator.levels().get(activation as usize - 1))
            .flat_map(|values| values.iter())
            .map(|value| value.value)
            .find(|value| *value != 0.0)?;
        let text = self.provider().localized_plural_from_id(
            &TranslationKey::new(format!("IDS_TALENT_{key}_LEVEL_ACTIVATION")),
            amount.round() as u64,
        )?;

        Some(clean(&text.replace("%(_activationNum)s", &format_amount(amount))))
    }

    /// Effects after the first `activation` activations for `classes`: the
    /// modifier changes summed over them, then what the Nth activation does in
    /// battle. Prefixed by class when they differ between classes.
    fn talent_lines(&self, talent: &CrewTalent, activation: u32, classes: &[ShipClass]) -> Vec<String> {
        self.per_class_lines(classes, |species| {
            let mut lines = self.talent_modifier_lines(&talent_modifiers(talent, activation, species, self), species);
            lines.extend(
                talent
                    .effects()
                    .iter()
                    .filter(|effect| !is_modifier_effect(effect))
                    .flat_map(|effect| self.talent_effect_lines(effect, &effect.activation(activation), species)),
            );

            lines
        })
    }

    /// Modifier effects of the first `count` activations together.
    fn talent_total_lines(&self, talent: &CrewTalent, count: u32, classes: &[ShipClass]) -> Vec<String> {
        self.per_class_lines(classes, |species| {
            self.talent_modifier_lines(&talent_modifiers(talent, count, species, self), species)
        })
    }

    /// Modifier lines, with a `reloadFactor` as one line naming the
    /// consumables it spares.
    fn talent_modifier_lines(&self, modifiers: &[CrewSkillModifier], species: Species) -> Vec<String> {
        let (reload, rest): (Vec<CrewSkillModifier>, Vec<CrewSkillModifier>) =
            modifiers.iter().cloned().partition(|modifier| modifier.name() == RELOAD_FACTOR);
        let mut lines: Vec<String> = reload
            .iter()
            .map(|modifier| {
                let percent = (modifier.get_for_species(&species) - 1.0) * 100.0;
                let sign = if percent > 0.0 { "+" } else { "" };
                let mut line = format!("{sign}{}% Consumable cooldown time", format_amount(percent));
                if !modifier.excluded_consumables().is_empty() {
                    let names: Vec<&str> = modifier
                        .excluded_consumables()
                        .iter()
                        .map(|kind| match self.consumable_names.lookup(kind) {
                            Lookup::Found(resolved, _) => resolved.name,
                            Lookup::Ambiguous(_) | Lookup::NotFound(_) => kind.as_str(),
                        })
                        .collect();
                    line.push_str(&format!(", except {}", names.join(" and ")));
                }
                line
            })
            .collect();
        lines.extend(self.describe(&rest, species));

        lines
    }

    pub(crate) fn per_class_lines(&self, classes: &[ShipClass], lines: impl Fn(Species) -> Vec<String>) -> Vec<String> {
        let per_class: Vec<(ShipClass, Vec<String>)> =
            classes.iter().map(|class| (*class, lines(class.species()))).collect();
        let Some((_, first)) = per_class.first() else {
            return Vec::new();
        };
        if per_class.iter().all(|(_, other)| other == first) {
            return first.clone();
        }

        per_class
            .iter()
            .flat_map(|(class, lines)| lines.iter().map(move |line| format!("{}: {line}", class.label())))
            .collect()
    }

    /// One talent effect block as text.
    fn talent_effect_lines(
        &self,
        effect: &TalentEffect,
        values: &[CrewSkillModifier],
        species: Species,
    ) -> Vec<String> {
        let value =
            |name: &str| values.iter().find(|value| value.name() == name).map(|value| value.get_for_species(&species));
        let work_time = if effect.use_ship_tier_as_work_time() {
            "the ship's tier in seconds".to_owned()
        } else {
            value("workTime")
                .map_or_else(|| "an unknown time".to_owned(), |seconds| format!("{} s", format_amount(seconds)))
        };

        match effect.effect_type().known() {
            Some(KnownTalentEffect::Regen) => {
                RegenRate::from_params(value("regenerationHPSpeed"), value("regenerationHPSpeedUnits"))
                    .map(|rate| {
                        regen_line(
                            rate,
                            effect.scale_regen_with_ship_tier(),
                            &work_time,
                            value("workTime").filter(|_| !effect.use_ship_tier_as_work_time()),
                        )
                    })
                    .into_iter()
                    .collect()
            }
            Some(KnownTalentEffect::CrashCrew) => {
                let label = self
                    .translate("IDS_PARAMS_MODIFIER_CRASHCREW_TALENT")
                    .unwrap_or_else(|| "Extinguishes fires and repairs damaged modules.".to_owned());
                vec![format!("{label} Action time: {work_time}.")]
            }
            Some(KnownTalentEffect::AddBuff) => effect
                .buff()
                .and_then(|buff| self.translate(&format!("IDS_SHIP_MODIFIER_PARAM_HEADER_{}", buff.to_uppercase())))
                .map(|buff| vec![format!("Applies the buff {buff}")])
                .unwrap_or_default(),
            Some(KnownTalentEffect::ShootShift) => {
                let shifted: Vec<CrewSkillModifier> =
                    values.iter().filter(|value| value.name() != "workTime").cloned().collect();
                let lines = self.describe(&shifted, species);
                match value("workTime") {
                    Some(_) => lines.into_iter().map(|line| format!("{line} for {work_time}")).collect(),
                    None => lines,
                }
            }
            _ => self.describe(values, species),
        }
    }

    pub(crate) fn translate(&self, key: &str) -> Option<String> {
        self.provider()
            .localized_name_from_id(&TranslationKey::new(key))
            .map(|text| clean(&text))
            .filter(|text| !text.is_empty())
    }

    fn talent_restrictions(&self) -> Option<String> {
        self.translate("IDS_HINT_TALENT_RESTRICTIONS")
    }

    /// How each of the commander's random battle talents for `ship` would
    /// change `card`, the card of `loadout` with `burst` averaged in, after
    /// each number of activations that differs. Talents whose modifiers
    /// change no stat on the card are left out.
    pub(crate) fn talent_stat_changes(
        &self,
        ship: &Ship,
        commander: &Commander,
        loadout: &ResolvedLoadout,
        card: &Card,
        burst: Option<Burst>,
    ) -> Result<Vec<TalentStatChanges>, QueryError> {
        let species = ship.class.species();
        let mut alternatives = Vec::new();
        for talent in commander.random_battle_talents().filter(|talent| talent.allowed_classes().contains(&species)) {
            let key = talent.translation_key(&commander.index);
            let name = self.talent_name(&key);
            // A talent without level blocks acts the same on every
            // activation, so one activation shows it.
            let levels = activation_levels(talent).map_or(1, |levels| levels.get() as u32);
            let mut counts: Vec<u32> = (1..=levels).collect();
            if let Some(limit) = talent.max_activations()
                && limit > levels
            {
                counts.push(limit);
            }

            for count in counts {
                let modifiers = talent_modifiers(talent, count, species, self);
                if modifiers.is_empty() {
                    continue;
                }

                let mut talented = loadout.clone();
                talented.modernizations.push((name.clone(), modifiers));
                talented.labels.insert(name.clone(), name.clone());
                let mut candidate = self.card(ship, &talented)?;
                if let Some(burst) = burst
                    && let Some(reload) = candidate.stats.artillery.as_ref().and_then(|artillery| artillery.reload_time)
                {
                    burst.average_damage_rates(&mut candidate.derived, Seconds::new(reload.value()));
                }
                let rows = candidate.stats.rows();
                let attributions = render_attributions(&candidate.provenance, &rows, self.provider(), self.provider());
                let mut changes = changes(&card.stats, &candidate.stats, &attributions, &talented, self, |_| true);
                changes.extend(derived_changes(
                    &card.derived,
                    &candidate.derived,
                    &attributions,
                    &talented,
                    self,
                    ship,
                ));
                // Only what the talent moves: a change none of its modifiers
                // caused is left out.
                changes.retain_mut(|change| {
                    change.because.retain(|cause| cause.starts_with(&name));
                    !change.because.is_empty()
                });
                if changes.is_empty() {
                    continue;
                }

                let multiple = levels > 1 || talent.max_activations().is_some_and(|limit| limit > 1);
                alternatives.push(TalentStatChanges {
                    talent: name.clone(),
                    condition: self.talent_condition(&key, talent),
                    activations: multiple.then_some(count),
                    activation_condition: (count <= levels)
                        .then(|| self.activation_condition(&key, talent, count))
                        .flatten(),
                    changes,
                });
            }
        }

        Ok(alternatives)
    }
}

/// The modifiers of the first `count` activations together, folded per
/// name as the client mixes them. Heals, Damage Control, firing-position
/// shifts and buffs act in battle rather than through the ship's modifiers,
/// and names the stat engine cannot fold are left out.
pub(crate) fn talent_modifiers(
    talent: &CrewTalent,
    count: u32,
    species: Species,
    game: &GameData,
) -> Vec<CrewSkillModifier> {
    let mut names: Vec<&str> = Vec::new();
    let effects: Vec<&TalentEffect> = talent.effects().iter().filter(|effect| is_modifier_effect(effect)).collect();
    let all_values =
        || effects.iter().flat_map(|effect| effect.values().iter().chain(effect.levels().iter().flatten()));
    for value in all_values() {
        // Action time belongs to in-battle effects, not to a ship stat.
        if value.name() != "workTime" && !names.contains(&value.name()) {
            names.push(value.name());
        }
    }

    let mut modifiers = Vec::new();
    for name in names {
        let values = (1..=count).flat_map(|activation| {
            effects.iter().filter_map(move |effect| {
                effect
                    .activation(activation)
                    .iter()
                    .find(|value| value.name() == name)
                    .map(|value| value.get_for_species(&species))
            })
        });
        let Ok(total) = fold_modifier_values(game.version(), name, values) else {
            continue;
        };
        // A name these activations leave at its identity changes nothing.
        if fold_modifier_values(game.version(), name, []).is_ok_and(|identity| identity == total) {
            continue;
        }
        // A `reloadFactor` spares the consumables it lists; the stat engine
        // spreads it over the rest.
        let excluded = all_values()
            .find(|value| value.name() == name)
            .map(|value| value.excluded_consumables().to_vec())
            .unwrap_or_default();

        modifiers.push(uniform_modifier(name, total, excluded));
    }

    modifiers
}

/// Activations whose effects differ: one per level block, at most the
/// activations a battle allows; `None` when every activation does the same.
pub(crate) fn activation_levels(talent: &CrewTalent) -> Option<NonZeroUsize> {
    let levels = talent
        .effects()
        .iter()
        .map(|effect| effect.levels().len())
        .chain(talent.activators().iter().map(|activator| activator.levels().len()))
        .max()?;

    let levels = match talent.max_activations() {
        Some(count) => levels.min(count as usize),
        None => levels,
    };

    NonZeroUsize::new(levels)
}

fn is_modifier_effect(effect: &TalentEffect) -> bool {
    !matches!(
        effect.effect_type().known(),
        None | Some(
            KnownTalentEffect::Regen
                | KnownTalentEffect::CrashCrew
                | KnownTalentEffect::ShootShift
                | KnownTalentEffect::AddBuff
                | KnownTalentEffect::AdditionalConsumables
                | KnownTalentEffect::AdditionalCoolConsumable
        )
    )
}

fn uniform_modifier(name: &str, value: f32, excluded_consumables: Vec<String>) -> CrewSkillModifier {
    CrewSkillModifier::builder()
        .name(name.to_owned())
        .aircraft_carrier(value)
        .auxiliary(value)
        .battleship(value)
        .cruiser(value)
        .destroyer(value)
        .submarine(value)
        .excluded_consumables(excluded_consumables)
        .build()
}

/// The rate of a heal over time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum RegenRate {
    /// Fraction of max HP per second.
    FractionPerSecond(f32),
    HpPerSecond(f32),
}

impl RegenRate {
    /// The rate of a regeneration talent from its `regenerationHPSpeed` and
    /// `regenerationHPSpeedUnits` values. Talent params write 0 for the one
    /// they do not use; the fraction wins when both are set.
    fn from_params(fraction: Option<f32>, units: Option<f32>) -> Option<Self> {
        if let Some(fraction) = fraction.filter(|fraction| *fraction > 0.0) {
            return Some(Self::FractionPerSecond(fraction));
        }

        units.filter(|units| *units > 0.0).map(Self::HpPerSecond)
    }
}

/// A heal over time at `rate` for `work_time`, with the total over `seconds`
/// when the heal lasts a known time.
pub(crate) fn regen_line(rate: RegenRate, per_tier: bool, work_time: &str, seconds: Option<f32>) -> String {
    let tier = if per_tier { " times the ship's tier" } else { "" };
    match rate {
        RegenRate::FractionPerSecond(fraction) => {
            let percent = fraction * 100.0;
            let total = seconds.map(|seconds| format!(", {}% of max HP in total", format_amount(percent * seconds)));

            format!(
                "HP regeneration: restores {}% of max HP per second{tier} for {work_time}{}",
                format_amount(percent),
                total.unwrap_or_default()
            )
        }
        RegenRate::HpPerSecond(units) => {
            let total = seconds.map(|seconds| format!(", {} HP{tier} in total", format_amount(units * seconds)));

            format!(
                "HP regeneration: restores {} HP per second{tier} for {work_time}{}",
                format_amount(units),
                total.unwrap_or_default()
            )
        }
    }
}

/// Up to three decimals without trailing zeros.
pub(crate) fn format_amount(value: f32) -> String {
    let text = format!("{value:.3}");
    let text = text.trim_end_matches('0').trim_end_matches('.');

    if text == "-0" { "0".to_owned() } else { text.to_owned() }
}

fn battle_types(group: &TalentBattleGroup) -> &'static str {
    // Battle types per group: shared_constants BATTLE_TYPES.TALENT_GROUP_PVP
    // and TALENT_GROUP_OPERATIONS.
    match group {
        TalentBattleGroup::Every => "every battle type that allows talents",
        TalentBattleGroup::Regular => "Random, Co-op, Ranked and Brawl battles",
        TalentBattleGroup::Operations => "Operations",
        TalentBattleGroup::Disabled => "none",
        TalentBattleGroup::Unknown(_) => "unknown battle types",
    }
}

/// Search terms of a query, each with the words that count as it.
fn search_terms(query: &str) -> Vec<Vec<String>> {
    const SYNONYMS: &[(&str, &[&str])] = &[
        ("heal", &["restores", "regeneration", "repair"]),
        ("heals", &["restores", "regeneration", "repair"]),
        ("healing", &["restores", "regeneration", "repair"]),
        ("regen", &["regeneration"]),
    ];

    search_key(query)
        .split(' ')
        .filter(|word| !word.is_empty())
        .map(|word| match SYNONYMS.iter().find(|(name, _)| *name == word) {
            Some((_, alternatives)) => alternatives.iter().map(|alternative| (*alternative).to_owned()).collect(),
            None => vec![word.to_owned()],
        })
        .collect()
}
