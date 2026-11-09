#[cfg(feature = "vfs")]
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::RwLock;

use gettext::Catalog;
use itertools::Itertools;

use pickled::HashableValue;
use pickled::Value;
use pickled::value::Shared;
#[cfg(feature = "vfs")]
use tracing::debug;

use crate::Rc;
#[cfg(feature = "vfs")]
use crate::data::DataFileWithCallback;
use crate::data::ResourceLoader;
use crate::data::TranslationKey;
use crate::error::GameDataError;
use crate::game_params::convert::ValueDictExt;
use crate::game_params::convert::base_params_dict;
use crate::game_params::convert::game_params_to_pickle;
use crate::game_params::ttx::components::LauncherDirection;
use crate::game_params::ttx::components::LauncherDirectionGroup;
use crate::game_params::ttx::components::LoaderCount;
use crate::game_params::ttx::components::TorpedoLoaderGroup;
use crate::game_params::ttx::components::TorpedoLoading;
use crate::game_params::ttx::model::AmmoCount;
use crate::game_params::ttx::model::Hp;
use crate::game_params::ttx::model::Seconds;
use crate::game_params::ttx::special_abilities::Activation;
use crate::game_params::ttx::special_abilities::AlternativeAmmo;
use crate::game_params::ttx::special_abilities::AlternativeFireMode;
use crate::game_params::ttx::special_abilities::BurstShots;
use crate::game_params::ttx::special_abilities::CombatInstructions;
use crate::game_params::ttx::special_abilities::ProgressDecay;
use crate::game_params::ttx::special_abilities::ProgressEvent;
use crate::game_params::ttx::special_abilities::ProgressPercent;
use crate::game_params::ttx::special_abilities::ProgressTrigger;
use crate::game_types::ChargeCount;
use crate::game_types::GameParamId;
use crate::game_types::Vec2;
use crate::game_types::Vec3;
use crate::rpc::entitydefs::EntitySpec;
#[cfg(feature = "vfs")]
use crate::rpc::entitydefs::parse_scripts;

use super::keys;
use super::types::*;

pub struct GameMetadataProvider {
    params: GameParams,
    param_id_to_translation_id: HashMap<GameParamId, String>,
    translations: RwLock<Option<Catalog>>,
    /// Advanced by every [`Self::set_translations`]. Callers caching localized
    /// strings key on this so an in-place catalog swap invalidates them.
    translation_epoch: std::sync::atomic::AtomicU64,
    specs: Arc<Vec<EntitySpec>>,
}

impl GameParamProvider for GameMetadataProvider {
    fn game_param_by_id(&self, id: GameParamId) -> Option<Rc<Param>> {
        self.params.game_param_by_id(id)
    }

    fn game_param_by_index(&self, index: &str) -> Option<Rc<Param>> {
        self.params.game_param_by_index(index)
    }

    fn game_param_by_name(&self, name: &str) -> Option<Rc<Param>> {
        self.params.game_param_by_name(name)
    }

    fn params(&self) -> &[Rc<Param>] {
        self.params.params()
    }
}

impl ResourceLoader for GameMetadataProvider {
    fn localized_name_from_param(&self, param: &Param) -> Option<String> {
        self.param_localization_id(param.id()).and_then(|id| self.translate(id))
    }

    fn localized_name_from_id(&self, id: &TranslationKey) -> Option<String> {
        self.translate(id.as_str())
    }

    fn game_param_by_id(&self, id: GameParamId) -> Option<Rc<Param>> {
        self.params.game_param_by_id(id)
    }

    fn entity_specs(&self) -> &[EntitySpec] {
        self.specs.as_slice()
    }
}

macro_rules! game_param_to_type {
    ($params:ident, $key:expr, String) => {
        game_param_to_type!($params, $key, string_ref, String).inner().to_string()
    };
    ($params:ident, $key:expr, i8) => {
        game_param_to_type!($params, $key, i64) as i8
    };
    ($params:ident, $key:expr, i16) => {
        game_param_to_type!($params, $key, i64) as i16
    };
    ($params:ident, $key:expr, i32) => {
        game_param_to_type!($params, $key, i64) as i32
    };
    ($params:ident, $key:expr, u8) => {
        game_param_to_type!($params, $key, i64) as u8
    };
    ($params:ident, $key:expr, u16) => {
        game_param_to_type!($params, $key, i64) as u16
    };
    ($params:ident, $key:expr, u32) => {
        game_param_to_type!($params, $key, i64) as u32
    };
    ($params:ident, $key:expr, u64) => {
        game_param_to_type!($params, $key, i64) as u64
    };
    ($params:ident, $key:expr, usize) => {
        game_param_to_type!($params, $key, i64) as usize
    };
    ($params:ident, $key:expr, isize) => {
        game_param_to_type!($params, $key, i64) as isize
    };
    ($params:ident, $key:expr, f32) => {
        game_param_to_type!($params, $key, f64) as f32
    };

    // The above matches in this macro will either expand to
    // game_param_to_type!($params, $key, f64) as f32
    // game_param_to_type!($params, $key, i64) as <PRIMITIVE_TYPE>
    // game_param_to_type!($params, $key, bool)
    //
    // But the primitive types do not have an inner shared value,
    // so we need to handle those specially here.
    ($params:ident, $key:expr, i64) => {
        *$params
            .get(&HashableValue::String($key.to_string().into()))
            .unwrap_or_else(|| panic!("could not get {}", $key))
            .i64_ref()
            .unwrap_or_else(|| panic!("{} is not an i64", $key))
    };
    ($params:ident, $key:expr, f64) => {
        *$params
            .get(&HashableValue::String($key.to_string().into()))
            .unwrap_or_else(|| panic!("could not get {}", $key))
            .f64_ref()
            .unwrap_or_else(|| panic!("{} is not a f64", $key))
    };
    ($params:ident, $key:expr, bool) => {
        *$params
            .get(&HashableValue::String($key.to_string().into()))
            .unwrap_or_else(|| panic!("could not get {}", $key))
            .bool_ref()
            .unwrap_or_else(|| panic!("{} is not a bool", $key))
    };

    // Optional hashmap: None when the key is absent or its value is none.
    ($params:ident, $key:expr, Option<HashMap<(), ()>>) => {
        $params
            .get(&HashableValue::String($key.to_string().into()))
            .filter(|value| !value.is_none())
            .map(|_| game_param_to_type!($params, $key, HashMap<(), ()>))
    };
    ($params:ident, $key:expr, Option<$ty:tt>) => {
        $params
            .get(&HashableValue::String($key.to_string().into()))
            .and_then(|value| {
                if value.is_none() {
                    None
                } else {
                    Some(game_param_to_type!($params, $key, $ty))
                }
            })
    };
    ($params:ident, $key:expr, HashMap<(), ()>) => {
        $params
            .get(&HashableValue::String($key.to_string().into()))
            .unwrap_or_else(|| panic!("could not get {}", $key))
            .dict_or_object_dict()
            .unwrap_or_else(|| panic!("{} is not a {}", $key, stringify!(HashMap<(), ()>)))
    };
    ($params:ident, $key:expr, &[()]) => {
        game_param_to_type!($params, $key, list_ref, &[()])
    };
    ($args:ident, $key:expr, $conversion_func:ident, $ty:ty) => {
        $args
            .get(&HashableValue::String($key.to_string().into()))
            .unwrap_or_else(|| panic!("could not get {}", $key))
            .$conversion_func()
            .unwrap_or_else(|| panic!("{} is not a {}", $key, stringify!($ty)))
    };
}

/// Build modifier records from a GameParams `modifiers` dict.
///
/// The dict may contain a sibling `excludedConsumables` array (e.g. on the
/// Survival Expert skill: `{"reloadFactor": 0.925, "excludedConsumables": ["crashCrew", "regenCrew"]}`).
/// When present, that list scopes every other entry in the same dict.
fn build_skill_modifiers(modifiers: &pickled::Dict) -> Vec<CrewSkillModifier> {
    let excluded_consumables: Vec<String> = modifiers
        .get(&HashableValue::String("excludedConsumables".to_owned().into()))
        .and_then(|v| v.list_ref())
        .map(|list| list.inner().iter().filter_map(|item| item.string_ref().map(|s| s.inner().to_owned())).collect())
        .unwrap_or_default();

    modifiers
        .iter()
        .filter_map(|(modifier_name, modifier_data)| {
            let modifier_name = modifier_name.string_ref().expect("modifier name is not a string").to_owned();
            let modifier_name = modifier_name.inner();

            // Sibling control key, consumed above and not a modifier itself.
            if modifier_name == "excludedConsumables" {
                return None;
            }

            let mk_uniform = |v: f32| {
                CrewSkillModifier::builder()
                    .name(modifier_name.to_owned())
                    .aircraft_carrier(v)
                    .auxiliary(v)
                    .battleship(v)
                    .cruiser(v)
                    .destroyer(v)
                    .submarine(v)
                    .excluded_consumables(excluded_consumables.clone())
                    .build()
            };

            if let Some(common_value) = modifier_data.i64_ref().cloned() {
                Some(mk_uniform(common_value as f32))
            } else if let Some(common_value) = modifier_data.f64_ref().cloned() {
                Some(mk_uniform(common_value as f32))
            } else if let Some(modifier_data) = modifier_data.dict_or_object_dict() {
                let modifier_data = modifier_data.inner();
                // Skip dicts that aren't per-species modifier dicts.
                modifier_data.get(&HashableValue::String("AirCarrier".to_owned().into()))?;

                // Builds that predate a class (e.g. Submarine) omit its key, and no
                // ship of that class exists there to read the value.
                let read_species = |key: &str| -> f32 {
                    modifier_data
                        .get(&HashableValue::String(key.to_owned().into()))
                        .and_then(|v| v.f64_ref().map(|f| *f as f32).or_else(|| v.i64_ref().map(|i| *i as f32)))
                        .unwrap_or(1.0)
                };

                Some(
                    CrewSkillModifier::builder()
                        .name(modifier_name.to_owned())
                        .aircraft_carrier(read_species("AirCarrier"))
                        .auxiliary(read_species("Auxiliary"))
                        .battleship(read_species("Battleship"))
                        .cruiser(read_species("Cruiser"))
                        .destroyer(read_species("Destroyer"))
                        .submarine(read_species("Submarine"))
                        .excluded_consumables(excluded_consumables.clone())
                        .build(),
                )
            } else {
                // Non-numeric, non-dict modifiers (bools, other lists, etc.).
                None
            }
        })
        .collect()
}

/// Keys on a crew-skill dict that are structural rather than effect modifiers.
/// Pre-rework skills (=0.6.x and earlier) store modifier coefficients as flat
/// sibling keys, so any key outside this set is treated as a modifier.
const SKILL_STRUCTURAL_KEYS: &[&str] = &[
    "column",
    "skillType",
    "tier",
    "turnOffOnRetraining",
    "modifiers",
    "LogicTrigger",
    "canBeLearned",
    "isEpic",
    "uiTreatAsTrigger",
];

fn build_crew_skills(skills: &pickled::Dict) -> Vec<CrewSkill> {
    skills
        .iter()
        .filter_map(|(hashable_skill_name, skill_data)| {
            let skill_name = hashable_skill_name.string_ref().expect("hashable_skill_name is not a String").to_owned();

            let skill_name = skill_name.inner();

            if skill_data.is_none() {
                return None;
            }

            let skill_data = skill_data.dict_or_object_dict().expect("skill data is not dictionary");
            let skill_data = skill_data.inner();

            let logic_trigger_data = game_param_to_type!(skill_data, "LogicTrigger", Option<HashMap<(), ()>>);

            let logic_trigger = logic_trigger_data.map(|logic_trigger_data| {
                let logic_trigger_data = logic_trigger_data.inner();
                // Triggered effects live on the trigger, not the skill: the
                // skill-level "modifiers" is empty for triggered skills.
                let trigger_modifiers = game_param_to_type!(logic_trigger_data, "modifiers", Option<HashMap<(), ()>>)
                    .map(|m| build_skill_modifiers(&m.inner()));
                let damage_value = logic_trigger_data
                    .get(&pk("damageValue"))
                    .and_then(|v| v.f64_ref().map(|f| *f as f32).or_else(|| v.i64_ref().map(|i| *i as f32)));

                let count_to_modifier = logic_trigger_data
                    .get(&pk("countToModifier"))
                    .and_then(|v| v.dict_or_object_dict())
                    .map(|ctm_dict| {
                        let ctm_dict = ctm_dict.inner();
                        let mut pairs: Vec<(u32, Vec<CrewSkillModifier>)> = ctm_dict
                            .iter()
                            .filter_map(|(k, v)| {
                                let count: u32 = k.string_ref()?.inner().parse().ok()?;
                                let block_name = v.string_ref()?.inner();
                                let block =
                                    logic_trigger_data.get(&pk(block_name)).and_then(|bv| bv.dict_or_object_dict())?;
                                Some((count, build_skill_modifiers(&block.inner())))
                            })
                            .collect();
                        pairs.sort_by_key(|(count, _)| *count);
                        pairs
                    })
                    .unwrap_or_default();

                // The pairs arrive as a tuple or a list.
                let read_interpolator = |key: &str| {
                    let points: Vec<(f32, f32)> = match logic_trigger_data.get(&pk(key))? {
                        Value::List(list) => {
                            list.inner().iter().filter_map(read_pair_both).map(|[x, y]| (x, y)).collect()
                        }
                        Value::Tuple(tuple) => {
                            tuple.inner().iter().filter_map(read_pair_both).map(|[x, y]| (x, y)).collect()
                        }
                        _ => return None,
                    };
                    (!points.is_empty()).then(|| Interpolator::from_points(points))
                };
                let heat_interpolator = read_interpolator("heatInterpolator");
                let cooling_interpolator = read_interpolator("coolingInterpolator");

                CrewSkillLogicTrigger::builder()
                    .maybe_burn_count(game_param_to_type!(logic_trigger_data, "burnCount", Option<usize>))
                    .maybe_change_priority_target_penalty(game_param_to_type!(
                        logic_trigger_data,
                        "changePriorityTargetPenalty",
                        Option<f32>
                    ))
                    // consumableType/coolingDelay are absent from some builds'
                    // LogicTrigger (added across patches); thread the Option through
                    // so absence stays None instead of becoming a fake default.
                    .maybe_consumable_type(game_param_to_type!(logic_trigger_data, "consumableType", Option<String>))
                    .maybe_cooling_delay(game_param_to_type!(logic_trigger_data, "coolingDelay", Option<f32>))
                    .maybe_cooling_interpolator(cooling_interpolator)
                    .count_to_modifier(count_to_modifier)
                    .maybe_damage_value(damage_value)
                    .maybe_divider_type(game_param_to_type!(logic_trigger_data, "dividerType", Option<String>))
                    .maybe_divider_value(game_param_to_type!(logic_trigger_data, "dividerValue", Option<f32>))
                    .maybe_duration(game_param_to_type!(logic_trigger_data, "duration", Option<f32>))
                    .maybe_energy_coeff(game_param_to_type!(logic_trigger_data, "energyCoeff", Option<f32>))
                    .maybe_flood_count(game_param_to_type!(logic_trigger_data, "floodCount", Option<usize>))
                    .maybe_health_factor(game_param_to_type!(logic_trigger_data, "healthFactor", Option<f32>))
                    .maybe_heat_interpolator(heat_interpolator)
                    .maybe_modifiers(trigger_modifiers)
                    .maybe_trigger_desc_ids(game_param_to_type!(logic_trigger_data, "triggerDescIds", Option<String>))
                    .maybe_trigger_type(game_param_to_type!(logic_trigger_data, "triggerType", Option<String>))
                    .build()
            });

            // Modern skills nest effect coefficients under "modifiers"; pre-rework
            // skills store them as flat sibling keys alongside the structural keys.
            let modifiers = match skill_data.get(&pk("modifiers")).and_then(|v| v.dict_or_object_dict()) {
                Some(modifiers) => Some(build_skill_modifiers(&modifiers.inner())),
                None => {
                    let flat: pickled::Dict = skill_data
                        .iter()
                        .filter(|(key, _)| {
                            key.string_ref()
                                .map(|s| !SKILL_STRUCTURAL_KEYS.contains(&s.inner().as_str()))
                                .unwrap_or(false)
                        })
                        .map(|(key, value)| (key.clone(), value.clone()))
                        .collect();
                    (!flat.is_empty()).then(|| build_skill_modifiers(&flat))
                }
            };

            // Modern tier is a per-species dict; pre-rework builds use one scalar.
            let tier = match skill_data.get(&pk("tier")).and_then(|v| v.dict_or_object_dict()) {
                Some(tier_data) => {
                    let tier_data = tier_data.inner();
                    let cost = |species: &str| {
                        u8::try_from(game_param_to_type!(tier_data, species, i64)).ok().map(SkillPointCost::new)
                    };
                    (|| {
                        Some(
                            CrewSkillTiers::builder()
                                .aircraft_carrier(cost("AirCarrier")?)
                                .auxiliary(cost("Auxiliary")?)
                                .battleship(cost("Battleship")?)
                                .cruiser(cost("Cruiser")?)
                                .destroyer(cost("Destroyer")?)
                                .submarine(cost("Submarine")?)
                                .build(),
                        )
                    })()
                }
                None => skill_data
                    .get(&pk("tier"))
                    .and_then(|v| v.i64_ref())
                    .and_then(|&v| u8::try_from(v).ok())
                    .map(SkillPointCost::new)
                    .map(|cost| {
                        CrewSkillTiers::builder()
                            .aircraft_carrier(cost)
                            .auxiliary(cost)
                            .battleship(cost)
                            .cruiser(cost)
                            .destroyer(cost)
                            .submarine(cost)
                            .build()
                    }),
            };

            // skillType is the identity learned-skill lists refer to; a skill
            // without one cannot be matched, so it is not modeled.
            let skill_type = skill_data.get(&pk("skillType")).and_then(|v| v.i64_ref())?;
            let skill_type = CrewSkillType::new(u32::try_from(*skill_type).ok()?);

            Some(
                CrewSkill::builder()
                    .internal_name(CrewSkillName::from(skill_name.to_owned()))
                    .can_be_learned(game_param_to_type!(skill_data, "canBeLearned", Option<bool>).unwrap_or_default())
                    .is_epic(game_param_to_type!(skill_data, "isEpic", Option<bool>).unwrap_or_default())
                    .skill_type(skill_type)
                    .ui_treat_as_trigger(
                        game_param_to_type!(skill_data, "uiTreatAsTrigger", Option<bool>).unwrap_or_default(),
                    )
                    .maybe_tier(tier)
                    .maybe_modifiers(modifiers)
                    .maybe_logic_trigger(logic_trigger)
                    .build(),
            )
        })
        .collect()
}

/// Extract a list-of-strings field (a list or tuple), returning empty when the
/// field is absent or not a string sequence. Older builds omit some of these
/// list fields entirely.
fn string_list_field(dict: &pickled::Dict, key: &str) -> Vec<String> {
    dict.get(&HashableValue::String(key.to_owned().into())).map(read_string_sequence).unwrap_or_default()
}

fn build_crew_personality(personality: &pickled::Dict) -> CrewPersonality {
    let ships = game_param_to_type!(personality, "ships", HashMap<(), ()>);
    let ships = ships.inner();
    let ships = CrewPersonalityShips::builder()
        .groups(string_list_field(&ships, "groups"))
        .nation(string_list_field(&ships, "nation"))
        .peculiarity(string_list_field(&ships, "peculiarity"))
        .ships(string_list_field(&ships, "ships"))
        .build();

    CrewPersonality::builder()
        .can_reset_skills_for_free(
            game_param_to_type!(personality, "canResetSkillsForFree", Option<bool>).unwrap_or_default(),
        )
        .maybe_has_custom_background(personality.get(&pk("hasCustomBackground")).and_then(|v| v.bool_ref().copied()))
        .maybe_has_overlay(personality.get(&pk("hasOverlay")).and_then(|v| v.bool_ref().copied()))
        .maybe_has_rank(personality.get(&pk("hasRank")).and_then(|v| v.bool_ref().copied()))
        .maybe_has_sample_voiceover(personality.get(&pk("hasSampleVO")).and_then(|v| v.bool_ref().copied()))
        .maybe_is_animated(personality.get(&pk("isAnimated")).and_then(|v| v.bool_ref().copied()))
        .maybe_is_person(personality.get(&pk("isPerson")).and_then(|v| v.bool_ref().copied()))
        .maybe_is_retrainable(personality.get(&pk("isRetrainable")).and_then(|v| v.bool_ref().copied()))
        .maybe_is_unique(personality.get(&pk("isUnique")).and_then(|v| v.bool_ref().copied()))
        .maybe_peculiarity(
            personality.get(&pk("peculiarity")).and_then(|v| v.string_ref()).map(|s| s.inner().to_string()),
        )
        .maybe_permissions(personality.get(&pk("permissions")).and_then(|v| v.i64_ref()).map(|&v| v as u32))
        // An empty personName names no person.
        .maybe_person_name(
            game_param_to_type!(personality, "personName", Option<String>).filter(|name| !name.is_empty()),
        )
        .maybe_subnation(personality.get(&pk("subnation")).and_then(|v| v.string_ref()).map(|s| s.inner().to_string()))
        .tags(
            personality
                .get(&pk("tags"))
                .and_then(|v| v.list_ref())
                .map(|list| {
                    list.inner().iter().filter_map(|value| value.string_ref().map(|s| s.inner().to_owned())).collect()
                })
                .unwrap_or_default(),
        )
        .ships(ships)
        .build()
}

/// The string-keyed entries of a dict, sorted by key for a stable order.
fn sorted_entries(dict: &pickled::Dict) -> Vec<(String, &Value)> {
    dict.iter()
        .filter_map(|(key, value)| Some((key.string_ref()?.inner().to_owned(), value)))
        .sorted_by(|left, right| left.0.cmp(&right.0))
        .collect()
}

/// The `N` of a `level_N` key.
fn level_number(key: &str) -> Option<u32> {
    key.strip_prefix("level_")?.parse().ok()
}

/// `UniqueSkills` of a crew: each talent is an `EventTrigger` holding a
/// `GameLogicTrigger*` block with its `Activator*` conditions, and the talent
/// params blocks it applies (Components/UniqueSkills.py `getTalentParams`
/// takes every params block not named with the trigger prefix).
fn build_crew_talents(unique_skills: &pickled::Dict) -> Vec<CrewTalent> {
    sorted_entries(unique_skills)
        .into_iter()
        .filter_map(|(id, talent)| {
            let talent = talent.dict_or_object_dict()?;
            let talent = talent.inner();
            // Older builds define talents without isUnlimited, battleGroup or sortIndex.
            talent.contains_key(&pk("isUnlimited")).then(|| build_crew_talent(id, &talent))
        })
        .sorted_by(|left, right| left.sort_index().cmp(&right.sort_index()).then_with(|| left.id().cmp(right.id())))
        .collect()
}

fn build_crew_talent(id: String, talent: &pickled::Dict) -> CrewTalent {
    let mut activators = Vec::new();
    let mut effects = Vec::new();
    for (key, value) in sorted_entries(talent) {
        let Some(block) = value.dict_or_object_dict() else {
            continue;
        };
        let block = block.inner();

        if key.starts_with("GameLogic") {
            for (part_key, part) in sorted_entries(&block) {
                if let Some(activator) = part.dict_or_object_dict().filter(|_| part_key.starts_with("Activator")) {
                    activators.push(build_talent_activator(&activator.inner()));
                }
            }
            continue;
        }

        if let Some(effect) = build_talent_effect(key, &block) {
            effects.push(effect);
        }
    }

    let allowed_classes = talent
        .get(&pk("triggerAllowedShips"))
        .map(read_string_sequence)
        .unwrap_or_default()
        .iter()
        .filter_map(|name| Species::from_name(name).known().copied())
        .collect();
    // Missing or negative counts are malformed; the panic skips the param
    // like any other missing required field.
    let count = |key: &str| -> u32 {
        let value =
            talent.get(&pk(key)).and_then(|v| v.i64_ref()).copied().unwrap_or_else(|| panic!("talent has no {key}"));
        u32::try_from(value).unwrap_or_else(|_| panic!("talent {key} is {value}"))
    };
    let is_unlimited =
        talent.get(&pk("isUnlimited")).and_then(|v| v.bool_ref().copied()).expect("talent has no isUnlimited");

    CrewTalent::builder()
        .id(id)
        .battle_group(TalentBattleGroup::from_name(&game_param_to_type!(talent, "battleGroup", String)))
        .sort_index(count("sortIndex"))
        .maybe_max_activations((!is_unlimited).then(|| count("maxTriggerNum")))
        .allowed_classes(allowed_classes)
        .activators(activators)
        .effects(effects)
        .build()
}

fn build_talent_activator(activator: &pickled::Dict) -> TalentActivator {
    let mut values = Vec::new();
    let mut levels = Vec::new();
    let mut parts = Vec::new();
    for (key, value) in sorted_entries(activator) {
        if let Some(level) = level_number(&key) {
            if let Some(overrides) = value.dict_or_object_dict() {
                levels.push((level, talent_values(&overrides.inner())));
            }
            continue;
        }

        if key.starts_with("ActivatorSub") {
            if let Some(part) = value.dict_or_object_dict() {
                let part = part.inner();
                parts.push(TalentActivatorPart {
                    kind: game_param_to_type!(part, "type", String),
                    values: talent_values(&part),
                    damage_types: string_list_field(&part, "damageTypes"),
                });
            }
            continue;
        }

        // The talent's `maxTriggerNum` bounds activations.
        if key == "maxActivations" {
            continue;
        }

        if let Some(number) = value_f32(value) {
            values.push(TalentValue { name: key, value: number });
        }
    }
    levels.sort_by_key(|(level, _)| *level);

    let ribbons = activator
        .get(&pk("ribbons"))
        .map(|value| read_sequence(value).iter().filter_map(|value| value.i64_ref().copied()).collect())
        .unwrap_or_default();

    TalentActivator::builder()
        .kind(game_param_to_type!(activator, "type", String))
        .values(values)
        .levels(levels.into_iter().map(|(_, values)| values).collect())
        .damage_types(string_list_field(activator, "damageTypes"))
        .ribbons(ribbons)
        .parts(parts)
        .build()
}

/// The numeric entries of a dict.
fn talent_values(dict: &pickled::Dict) -> Vec<TalentValue> {
    sorted_entries(dict)
        .into_iter()
        .filter_map(|(name, value)| Some(TalentValue { name, value: value_f32(value)? }))
        .collect()
}

/// A talent params block, `None` when it has no integer `uniqueType`.
fn build_talent_effect(name: String, block: &pickled::Dict) -> Option<TalentEffect> {
    let effect_type =
        block.get(&pk("uniqueType")).and_then(|v| v.i64_ref()).and_then(|kind| u32::try_from(*kind).ok())?;
    let levels = sorted_entries(block)
        .into_iter()
        .filter_map(|(key, value)| Some((level_number(&key)?, value.dict_or_object_dict()?)))
        .sorted_by_key(|(level, _)| *level)
        .map(|(_, overrides)| talent_effect_values(&overrides.inner()))
        .collect();
    let flag = |key: &str| block.get(&pk(key)).and_then(|v| v.bool_ref().copied()).unwrap_or(false);

    Some(
        TalentEffect::builder()
            .name(name)
            .effect_type(TalentEffectType::new(effect_type))
            .use_ship_tier_as_work_time(flag("useShipTierAsWorkTime"))
            .scale_regen_with_ship_tier(flag("scaleRegenWithShipTier"))
            .maybe_buff(game_param_to_type!(block, "buffParamsId", Option<String>).filter(|buff| !buff.is_empty()))
            .values(talent_effect_values(block))
            .levels(levels)
            .build(),
    )
}

/// Numeric fields and `modifiers` entries of a talent params block.
/// `uniqueType` is the block's kind, and the `...UI` copies hold the totals
/// the client displays, which follow from the levels.
fn talent_effect_values(block: &pickled::Dict) -> Vec<CrewSkillModifier> {
    let fields: pickled::Dict = block
        .iter()
        .filter(|(key, _)| {
            key.string_ref().is_some_and(|key| {
                let key = key.inner();
                key.as_str() != "uniqueType" && !key.ends_with("UI") && level_number(key).is_none()
            })
        })
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    let mut values = build_skill_modifiers(&fields);
    if let Some(modifiers) = block.get(&pk("modifiers")).and_then(|v| v.dict_or_object_dict()) {
        values.extend(build_skill_modifiers(&modifiers.inner()));
    }
    values.sort_by(|left, right| left.name().cmp(right.name()));

    values
}

fn build_buff_drop(param_data: &pickled::Dict) -> BuffDrop {
    let string = |key: &str| param_data.get(&pk(key)).and_then(|v| v.string_ref()).map(|s| s.inner().to_string());
    let buffs = param_data
        .get(&pk("payload"))
        .and_then(|v| v.dict_or_object_dict())
        .map(|payload| {
            sorted_entries(&payload.inner())
                .into_iter()
                .filter_map(|(_, item)| {
                    let item = item.dict_or_object_dict()?;
                    let name = item.inner().get(&pk("params"))?.string_ref()?.inner().to_string();
                    Some(name)
                })
                .collect()
        })
        .unwrap_or_default();

    BuffDrop::builder()
        .maybe_marker_name_active(string("markerNameActive"))
        .maybe_marker_name_inactive(string("markerNameInactive"))
        .maybe_sorting(param_data.get(&pk("sorting")).and_then(|v| v.i64_ref()).copied())
        .maybe_max_buff_level(
            param_data.get(&pk("maxBuffLevel")).and_then(|v| v.i64_ref()).and_then(|level| u32::try_from(*level).ok()),
        )
        .buffs(buffs)
        .build()
}

/// The `level` block of a `Modifier` param: every one holds the same full
/// table of default values, which its level blocks override.
fn modifier_defaults(params: &pickled::Dict) -> Option<pickled::Dict> {
    params.values().find_map(|param| {
        let param = param.dict_or_object_dict()?;
        let param = param.inner();
        let species = param.get(&pk(keys::TYPEINFO))?.dict_or_object_dict()?;
        if species.inner().get(&pk(keys::TYPEINFO_SPECIES))?.string_ref()?.inner().as_str() != "Modifier" {
            return None;
        }
        let defaults = param.get(&pk("level"))?.dict_or_object_dict()?;

        Some(defaults.inner().clone())
    })
}

/// A `Modifier` param's blocks, each without the entries equal to
/// `defaults`. Without defaults every entry is kept.
fn build_modifier_param(param_data: &pickled::Dict, defaults: Option<&pickled::Dict>) -> ModifierParam {
    let changed = |block: &pickled::Dict| -> Vec<CrewSkillModifier> {
        let changed: pickled::Dict = block
            .iter()
            .filter(|(key, value)| defaults.and_then(|defaults| defaults.get(key)) != Some(*value))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        let mut modifiers = build_skill_modifiers(&changed);
        modifiers.sort_by(|left, right| left.name().cmp(right.name()));

        modifiers
    };

    let mut levels: Vec<(u32, Vec<CrewSkillModifier>)> = sorted_entries(param_data)
        .into_iter()
        .filter_map(|(key, value)| Some((level_number(&key)?, changed(&value.dict_or_object_dict()?.inner()))))
        .collect();
    levels.sort_by_key(|(level, _)| *level);
    let mut levels: Vec<Vec<CrewSkillModifier>> = levels.into_iter().map(|(_, modifiers)| modifiers).collect();
    if levels.is_empty()
        && let Some(block) = param_data.get(&pk("modifier")).and_then(|v| v.dict_or_object_dict())
    {
        levels.push(changed(&block.inner()));
    }

    let classes = param_data
        .get(&pk("restrictions"))
        .and_then(|v| v.dict_or_object_dict())
        .map(|restrictions| string_list_field(&restrictions.inner(), "subtype"))
        .unwrap_or_default()
        .iter()
        .filter_map(|name| Species::from_name(name).known().copied())
        .collect();

    ModifierParam::builder().classes(classes).levels(levels).build()
}

/// `None` when the category lacks `consumableType` or `reloadTime`, carries a
/// `lifeCycleType` or `maxCapacity` the client does not define, or is
/// COUNT_BASED without a valid `numConsumables` and a `workTime`.
fn build_ability_category(category_data: &pickled::Dict) -> Option<AbilityCategory> {
    let reload_time = read_float(category_data, "reloadTime")?;

    // Detection radius fields. Newer clients nest these under a "logic" sub-object;
    // older ones (e.g. 0.9.x) put them directly on the category. Read from "logic"
    // when present, otherwise fall back to the category root -- without this, radar
    // and hydro range circles never resolve for pre-rework replays.
    let logic = category_data.get(&pk("logic")).and_then(|v| v.dict_or_object_dict());
    let read_f32 = |key: &str| -> Option<f32> {
        let hk = pk(key);
        let conv = |v: &Value| v.f64_ref().map(|f| *f as f32).or_else(|| v.i64_ref().map(|i| *i as f32));
        logic.as_ref().and_then(|l| l.inner().get(&hk).and_then(conv)).or_else(|| category_data.get(&hk).and_then(conv))
    };

    let dist_ship = read_f32("distShip").map(BigWorldDistance::from);
    let dist_torpedo = read_f32("distTorpedo").map(BigWorldDistance::from);
    let hydrophone_wave_radius = read_f32("hydrophoneWaveRadius").map(Meters::from);
    let patrol_radius = read_f32("radius").map(BigWorldDistance::from);

    // Generic numeric field map mirroring the client's consumable attribute
    // extraction: category root merged with `logic`, logic winning collisions.
    let num = |v: &Value| v.f64_ref().map(|f| *f as f32).or_else(|| v.i64_ref().map(|i| *i as f32));
    let mut effect_fields: BTreeMap<String, f32> = BTreeMap::new();
    for (key, value) in category_data.iter() {
        if let (Some(name), Some(n)) = (key.string_ref(), num(value)) {
            effect_fields.insert(name.inner().to_owned(), n);
        }
    }
    if let Some(logic) = logic.as_ref() {
        for (key, value) in logic.inner().iter() {
            if let (Some(name), Some(n)) = (key.string_ref(), num(value)) {
                effect_fields.insert(name.inner().to_owned(), n);
            }
        }
    }
    // tacticalParams is part of the client's merged attribute dict (workRange/aimRange
    // live here); merged after logic so it wins collisions, matching GPToParamsDict.
    let tactical = category_data.get(&pk("tacticalParams")).and_then(|v| v.dict_or_object_dict());
    if let Some(tactical) = tactical.as_ref() {
        for (key, value) in tactical.inner().iter() {
            if let (Some(name), Some(n)) = (key.string_ref(), num(value)) {
                effect_fields.insert(name.inner().to_owned(), n);
            }
        }
    }
    // The client merges logic's nested `modifiers` dict last (GPToParamsDict lines 70-73),
    // so its fields override on collision.
    let mut ability_modifiers: Vec<crate::game_params::types::CrewSkillModifier> = Vec::new();
    if let Some(logic) = logic.as_ref()
        && let Some(modifiers_dict) = logic.inner().get(&pk("modifiers")).and_then(|v| v.dict_or_object_dict())
    {
        for (key, value) in modifiers_dict.inner().iter() {
            if let (Some(name), Some(n)) = (key.string_ref(), num(value)) {
                effect_fields.insert(name.inner().to_owned(), n);
            }
        }
        ability_modifiers = build_skill_modifiers(&modifiers_dict.inner());
    }

    // Empty strings mean the field is unused.
    let logic_string = |key: &str| {
        logic
            .as_ref()
            .and_then(|logic| logic.inner().get(&pk(key)).and_then(|v| v.string_ref()).map(|s| s.inner().to_owned()))
            .filter(|value| !value.is_empty())
    };
    let target_team = tactical
        .as_ref()
        .and_then(|tactical| {
            tactical.inner().get(&pk("targetTeam")).and_then(|v| v.string_ref()).map(|s| s.inner().to_owned())
        })
        .filter(|team| !team.is_empty());

    let consumable_type = game_param_to_type!(category_data, "consumableType", Option<String>)?;
    // The client's ConsumableLifeCycleType default is COUNT_BASED when the
    // category omits lifeCycleType.
    let lifecycle = match read_f32("lifeCycleType") {
        None => count_based_lifecycle(category_data)?,
        Some(0.0) => count_based_lifecycle(category_data)?,
        Some(1.0) => time_based_lifecycle(category_data)?,
        Some(_) => return None,
    };
    let group = game_param_to_type!(category_data, "group", Option<String>)
        .and_then(|group| ConsumableGroup::from_game_str(&group));
    let icon_id = game_param_to_type!(category_data, "iconIDs", Option<String>).filter(|icon| !icon.is_empty());

    Some(
        AbilityCategory::builder()
            .maybe_buff(logic_string("buff"))
            .maybe_buff_on_self(logic_string("buffOnSelf"))
            .maybe_target_team(target_team)
            .effect_fields(effect_fields)
            .modifiers(ability_modifiers)
            .maybe_special_sound_id(game_param_to_type!(category_data, "SpecialSoundID", Option<String>))
            .consumable_type(consumable_type)
            .maybe_group(group)
            .maybe_icon_id(icon_id)
            .lifecycle(lifecycle)
            .maybe_preparation_time(read_float(category_data, "preparationTime"))
            .reload_time(reload_time)
            .maybe_dist_ship(dist_ship)
            .maybe_dist_torpedo(dist_torpedo)
            .maybe_hydrophone_wave_radius(hydrophone_wave_radius)
            .maybe_patrol_radius(patrol_radius)
            .maybe_regeneration_hp_speed(read_f32("regenerationHPSpeed"))
            .maybe_regeneration_hp_speed_units(read_f32("regenerationHPSpeedUnits"))
            .build(),
    )
}

/// `None` without a `numConsumables` the client defines or a `workTime`.
fn count_based_lifecycle(category_data: &pickled::Dict) -> Option<ConsumableLifecycle> {
    let charges = ChargeCount::from_game_params(game_param_to_type!(category_data, "numConsumables", Option<i64>)?)?;
    let work_time = read_float(category_data, "workTime")?;

    Some(ConsumableLifecycle::CountBased { charges, work_time })
}

/// Absent fields take the defaults of the client's `TimeBasedConsumable`
/// schema class: an unlimited pool, a 10 s shortest activation and no
/// refill. `None` for a `maxCapacity` the client does not define.
fn time_based_lifecycle(category_data: &pickled::Dict) -> Option<ConsumableLifecycle> {
    let capacity = match read_float(category_data, "maxCapacity") {
        Some(value) => ConsumableCapacity::from_game_params(value)?,
        None => ConsumableCapacity::Unlimited,
    };

    Some(ConsumableLifecycle::TimeBased {
        capacity,
        min_work_time: read_float(category_data, "minWorkTime").unwrap_or(10.0),
        capacity_regen_coeff: read_float(category_data, "capacityRegenCoeff").unwrap_or(0.0),
    })
}

fn build_ability(ability_data: &pickled::Dict) -> Ability {
    let test_key = HashableValue::String("consumableType".to_string().into());
    let categories: HashMap<String, AbilityCategory> =
        HashMap::from_iter(ability_data.iter().filter_map(|(key, value)| {
            // GameParams pickled categories arrive as either Value::Dict or
            // Value::Object(DictObject); `is_not_dict` misses the object case
            // and was dropping every Ability variant on the floor.
            let dict = value.dict_or_object_dict()?;
            let inner = dict.inner();
            if !inner.contains_key(&test_key) {
                return None;
            }
            let name = key.string_ref().unwrap().inner().to_owned();
            let Some(category) = build_ability_category(&inner) else {
                tracing::warn!(category = %name, "ability category lacks a valid reloadTime, lifeCycleType or charge pool");
                return None;
            };
            Some((name, category))
        }));

    Ability::builder()
        .can_buy(game_param_to_type!(ability_data, "canBuy", bool))
        .is_free(game_param_to_type!(ability_data, "freeOfCharge", bool))
        .categories(categories)
        .build()
}

/// Helper: create a pickled dict key. Checks for `String` first, then `Bytes`.
fn pk(key: &str) -> HashableValue {
    HashableValue::String(key.to_string().into())
}

/// Helper: read a float from a pickled dict, accepting both f64 and i64.
fn read_float(dict: &pickled::Dict, key: &str) -> Option<f32> {
    dict.get(&pk(key)).and_then(|v| v.f64_ref().map(|f| *f as f32).or_else(|| v.i64_ref().map(|i| *i as f32)))
}

/// Helper: read a nonzero price. GameParams writes `0` for "not sold for this
/// currency", which is absence rather than a free item.
fn read_price(dict: &pickled::Dict, key: &str) -> Option<u64> {
    let value = dict.get(&pk(key))?;
    let price = value.i64_ref().copied().or_else(|| value.f64_ref().map(|f| *f as i64))?;

    u64::try_from(price).ok().filter(|price| *price > 0)
}

/// Helper: read every string from a pickled list or tuple value.
fn read_string_sequence(val: &Value) -> Vec<String> {
    let strings = |items: &[Value]| -> Vec<String> {
        items.iter().filter_map(|v| v.string_ref().map(|s| s.inner().clone())).collect()
    };

    if let Some(list) = val.list_ref() {
        strings(&list.inner())
    } else if let Some(tuple) = val.tuple_ref() {
        strings(tuple.inner())
    } else {
        Vec::new()
    }
}

/// Helper: the items of a pickled list or tuple value.
fn read_sequence(val: &Value) -> Vec<Value> {
    if let Some(list) = val.list_ref() {
        list.inner().to_vec()
    } else if let Some(tuple) = val.tuple_ref() {
        tuple.inner().to_vec()
    } else {
        Vec::new()
    }
}

/// Launcher groups of a torpedo component (`groups`, `loaders`,
/// `groupsDirection`), `None` when it groups no launchers. The client picks
/// its loader-limited reload by `groups` alone (`TorpedoesReloadManagers`).
fn read_torpedo_loading(component: &pickled::Dict) -> Option<TorpedoLoading> {
    // `[[groupId, [hardpoint, ...]], ...]`, `[[count, [groupId, ...]], ...]`
    // and `[[direction, [groupId, ...]], ...]`.
    let pairs = |key: &str| -> Vec<(Value, Value)> {
        component
            .get(&pk(key))
            .map(read_sequence)
            .unwrap_or_default()
            .iter()
            .filter_map(|entry| {
                let pair = read_sequence(entry);
                Some((pair.first()?.clone(), pair.get(1)?.clone()))
            })
            .collect()
    };

    let groups: HashMap<i64, Vec<String>> = pairs("groups")
        .iter()
        .filter_map(|(id, hardpoints)| Some((*id.i64_ref()?, read_string_sequence(hardpoints))))
        .collect();
    if groups.is_empty() {
        return None;
    }

    let hardpoints = |ids: &Value| -> Vec<String> {
        read_sequence(ids).iter().filter_map(|id| groups.get(id.i64_ref()?)).flatten().cloned().collect()
    };
    let loader_groups = pairs("loaders")
        .iter()
        .filter_map(|(count, ids)| {
            let loaders = LoaderCount(u32::try_from(*count.i64_ref()?).ok()?);
            Some(TorpedoLoaderGroup { loaders, hardpoints: hardpoints(ids) })
        })
        .collect();
    let directions = pairs("groupsDirection")
        .iter()
        .filter_map(|(direction, ids)| {
            let direction = LauncherDirection::from_game_str(direction.string_ref()?.inner());
            Some(LauncherDirectionGroup { direction, hardpoints: hardpoints(ids) })
        })
        .collect();

    Some(TorpedoLoading { loader_groups, directions })
}

/// Combat Instructions of a `specials` component (its `RageMode` section),
/// `None` when it has none.
fn read_combat_instructions(specials: &pickled::Dict) -> Option<CombatInstructions> {
    let section = specials.get(&pk(keys::RAGE_MODE))?.dict_or_object_dict()?;
    let section = section.inner();

    // The `RageMode` class default for an absent isAutoUsage is false.
    let activation = match section.get(&pk("isAutoUsage")).and_then(|value| value.bool_ref().copied()) {
        Some(true) => Activation::Automatic,
        Some(false) | None => Activation::Manual,
    };
    // A zero boostPreparation readies the ability as the meter fills.
    let preparation = read_float(&section, "boostPreparation").filter(|time| *time > 0.0).map(Seconds::from);
    let decay = match (
        read_float(&section, "decrementDelay"),
        read_float(&section, "decrementCount"),
        read_float(&section, "decrementPeriod"),
    ) {
        (Some(delay), Some(loss), Some(period)) if loss > 0.0 => Some(ProgressDecay {
            delay: Seconds::from(delay),
            loss: ProgressPercent(loss),
            period: Seconds::from(period),
        }),
        _ => None,
    };
    let modifiers = section
        .get(&pk("modifiers"))
        .and_then(|value| value.dict_or_object_dict())
        .map(|modifiers| build_skill_modifiers(&modifiers.inner()))
        .unwrap_or_default();

    // The client reads every `GameLogicTrigger*` child, in name order
    // (Components/GameLogicTriggers.py).
    let mut trigger_names: Vec<String> = section
        .keys()
        .filter_map(|key| key.string_ref().map(|name| name.inner().clone()))
        .filter(|name| name.starts_with("GameLogicTrigger"))
        .collect();
    trigger_names.sort();
    let triggers = trigger_names
        .iter()
        .filter_map(|name| section.get(&pk(name))?.dict_or_object_dict())
        .filter_map(|trigger| read_progress_trigger(&trigger.inner()))
        .collect();

    Some(CombatInstructions {
        name: read_string(&section, "rageModeName").filter(|name| !name.is_empty()),
        activation,
        duration: read_float(&section, "boostDuration").map(Seconds::from),
        preparation,
        decay,
        modifiers,
        triggers,
    })
}

/// One trigger of a `RageMode` section, `None` unless its first action adds
/// meter progress, as the client lists them (`RageModeSystem.buildRageModeTrigger`).
fn read_progress_trigger(trigger: &pickled::Dict) -> Option<ProgressTrigger> {
    let child = |prefix: &str| -> Option<pickled::Dict> {
        let mut names: Vec<String> = trigger
            .keys()
            .filter_map(|key| key.string_ref().map(|name| name.inner().clone()))
            .filter(|name| name.to_ascii_lowercase().starts_with(prefix))
            .collect();
        names.sort();
        let first = trigger.get(&pk(names.first()?))?.dict_or_object_dict()?;

        Some(first.inner().clone())
    };
    let action = child("action")?;
    if read_string(&action, "type").as_deref() != Some("RageModeProgressAction") {
        return None;
    }
    let activator = child("activator")?;

    let ids = |key: &str| -> Vec<i64> {
        activator
            .get(&pk(key))
            .map(read_sequence)
            .unwrap_or_default()
            .iter()
            .filter_map(|value| value.i64_ref().copied())
            .collect()
    };
    let kind = read_string(&activator, "type")?;
    let event = match kind.as_str() {
        "RibbonActivator" => ProgressEvent::Ribbon { ribbons: ids("ribbons"), sub_ribbons: ids("subRibbons") },
        "PotentialDamageActivator" => {
            ProgressEvent::PotentialDamage { per: Hp::from(read_float(&activator, "potentialDamageShift")?) }
        }
        "TimerActivator" => ProgressEvent::Timer {
            period: Seconds::from(read_float(&activator, "duration")?),
            repeating: activator.get(&pk("isRepeating")).and_then(|value| value.bool_ref().copied()).unwrap_or(false),
        },
        "ConsumableUseActivator" => ProgressEvent::ConsumableUse {
            consumables: activator.get(&pk("trackedConsumables")).map(read_string_sequence).unwrap_or_default(),
        },
        "AdjustmentShotActivator" => {
            ProgressEvent::AdjustmentShot { radius: Meters::from(read_float(&activator, "radius")?) }
        }
        "VisibilityChangedActivator" => ProgressEvent::VisibilityChanged {
            visible: activator.get(&pk("isVisible")).and_then(|value| value.bool_ref().copied())?,
        },
        _ => ProgressEvent::Other { activator: kind },
    };

    Some(ProgressTrigger {
        // GameParams writes progressName "default" when the activator type names the trigger.
        progress_name: read_string(&action, "progressName").filter(|name| !name.is_empty() && name != "default"),
        gain: ProgressPercent(read_float(&action, "progress")?),
        event,
        // An activator without requiredCount advances on every event.
        required_count: read_float(&activator, "requiredCount").map_or(1, |count| count as u32),
        // GameParams writes a negative maxActivations, like an absent one, for no limit.
        max_activations: read_float(&activator, "maxActivations")
            .filter(|count| *count >= 0.0)
            .map(|count| count as u32),
        starts_enabled: trigger.get(&pk("startEnabled")).and_then(|value| value.bool_ref().copied()).unwrap_or(true),
    })
}

/// The alternative fire mode of an artillery component, `None` when it has
/// none.
fn read_alternative_fire(artillery: &pickled::Dict) -> Option<AlternativeFireMode> {
    let section = artillery.get(&pk(keys::SWITCHABLE_MODE_ARTILLERY))?.dict_or_object_dict()?;
    let section = section.inner();

    let pool: HashMap<String, i64> = section
        .get(&pk("secondaryAmmoPool"))
        .and_then(|value| value.dict_or_object_dict())
        .map(|pool| {
            pool.inner()
                .iter()
                .filter_map(|(name, count)| Some((name.string_ref()?.inner().clone(), *count.i64_ref()?)))
                .collect()
        })
        .unwrap_or_default();
    // An entry whose pool count the client does not define is dropped.
    let ammo = section
        .get(&pk("secondaryAmmoList"))
        .map(read_string_sequence)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|name| {
            // GameParams writes -1 for an unlimited pool.
            let limit = match pool.get(&name).copied() {
                None => None,
                Some(-1) => Some(AmmoCount::Infinite),
                Some(count) => Some(AmmoCount::Finite(u32::try_from(count).ok()?)),
            };
            Some(AlternativeAmmo { name, limit })
        })
        .collect();
    let modifiers = section
        .get(&pk("modifiers"))
        .and_then(|value| value.dict_or_object_dict())
        .map(|modifiers| build_skill_modifiers(&modifiers.inner()))
        .unwrap_or_default();

    Some(AlternativeFireMode {
        shots: BurstShots(u32::try_from(*section.get(&pk("shotsCount"))?.i64_ref()?).ok()?),
        shot_interval: Seconds::from(read_float(&section, "burstReloadTime")?),
        full_reload: Seconds::from(read_float(&section, "fullReloadTime")?),
        modifiers,
        ammo,
    })
}

/// Port price and research links from a ship's `ShipUpgradeInfo` dict.
fn read_ship_research(upgrade_info: &pickled::Dict) -> super::types::ShipResearch {
    let mut next_ships: Vec<String> = upgrade_info
        .values()
        .filter_map(|module| module.dict_or_object_dict())
        .flat_map(|module| module.inner().get(&pk(keys::NEXT_SHIPS)).map(read_string_sequence).unwrap_or_default())
        .collect();
    next_ships.sort();
    next_ships.dedup();

    let required_ship = upgrade_info
        .get(&pk("requiredShip"))
        .and_then(|v| v.string_ref())
        .map(|s| s.inner().clone())
        .filter(|s| !s.is_empty());

    super::types::ShipResearch::builder()
        .maybe_cost_credits(read_price(upgrade_info, "costCR").map(super::types::Credits::new))
        .maybe_cost_doubloons(read_price(upgrade_info, "costGold").map(super::types::Doubloons::new))
        .maybe_cost_xp(read_price(upgrade_info, "costXP").map(super::types::Experience::new))
        .next_ships(next_ships)
        .maybe_required_ship(required_ship)
        .build()
}

/// Helper: extract first string from a pickled list value.
fn read_first_string(val: &Value) -> Option<String> {
    let list = val.list_ref()?;
    list.inner().first().and_then(|v| v.string_ref().map(|s| s.inner().clone()))
}

/// Helper: extract all strings from a pickled list value.
fn read_all_strings(val: &Value) -> Vec<String> {
    let Some(list) = val.list_ref() else {
        return Vec::new();
    };
    list.inner().iter().filter_map(|v| v.string_ref().map(|s| s.inner().clone())).collect()
}

/// Helper: read a string value from a pickled dict.
fn read_string(dict: &pickled::Dict, key: &str) -> Option<String> {
    dict.get(&pk(key)).and_then(|v| v.string_ref()).map(|s| s.inner().to_string())
}

/// Read a list-or-tuple of strings into a `Vec<String>` (empty for other types).
fn read_string_seq(val: &Value) -> Vec<String> {
    let items = if let Some(l) = val.list_ref() {
        l.inner().to_vec()
    } else if let Some(t) = val.tuple_ref() {
        t.inner().to_vec()
    } else {
        return Vec::new();
    };
    items.iter().filter_map(|v| v.string_ref().map(|s| s.inner().clone())).collect()
}

/// Parse a `customMiscs` dict (preset name -> list of misc names).
fn parse_custom_miscs(dict: &pickled::Dict) -> HashMap<String, Vec<String>> {
    dict.iter()
        .filter_map(|(k, v)| {
            let key = k.string_ref()?.inner().clone();
            Some((key, read_string_seq(v)))
        })
        .collect()
}

fn value_f32(v: &Value) -> Option<f32> {
    v.f64_ref().map(|f| *f as f32).or_else(|| v.i64_ref().map(|i| *i as f32))
}

/// Read the first numeric element of a list-or-tuple value (e.g. `rotationSpeed[0]`).
fn read_first_float(v: &Value) -> Option<f32> {
    if let Some(l) = v.list_ref() {
        l.inner().first().and_then(value_f32)
    } else if let Some(t) = v.tuple_ref() {
        t.inner().first().and_then(value_f32)
    } else {
        None
    }
}

/// Read both numeric elements from a list-or-tuple `[inner, outer]` pair.
fn read_pair_both(v: &Value) -> Option<[f32; 2]> {
    if let Some(l) = v.list_ref() {
        let g = l.inner();
        Some([value_f32(g.first()?)?, value_f32(g.get(1)?)?])
    } else if let Some(t) = v.tuple_ref() {
        let s = t.inner();
        Some([value_f32(s.first()?)?, value_f32(s.get(1)?)?])
    } else {
        None
    }
}

/// Read a 3-element float triple from a list-or-tuple value.
fn read_vec3_value(elem: &Value) -> Option<[f32; 3]> {
    if let Some(l) = elem.list_ref() {
        let g = l.inner();
        if g.len() != 3 {
            return None;
        }
        Some([value_f32(&g[0])?, value_f32(&g[1])?, value_f32(&g[2])?])
    } else if let Some(t) = elem.tuple_ref() {
        let s = t.inner();
        if s.len() != 3 {
            return None;
        }
        Some([value_f32(&s[0])?, value_f32(&s[1])?, value_f32(&s[2])?])
    } else {
        None
    }
}

/// Read a 3-element float triple from the element at `idx` of a nested list-or-tuple.
fn read_vec3_at(v: &Value, idx: usize) -> Option<[f32; 3]> {
    if let Some(l) = v.list_ref() {
        read_vec3_value(l.inner().get(idx)?)
    } else if let Some(t) = v.tuple_ref() {
        read_vec3_value(t.inner().get(idx)?)
    } else {
        None
    }
}

/// Derive hull `floodProb` from `floodNodes` per `PreprocessedHull.py:11-12`:
/// `(DEFAULT_UW_DAMAGE_COEFF - floodNodes[0][0]) / DEFAULT_UW_DAMAGE_COEFF`, or 0.0
/// when `floodNodes[0][0] == DEFAULT_UW_DAMAGE_COEFF`. `floodNodes` is a list of
/// triples; reads `floodNodes[0][0]` defensively, `None` if absent/empty.
fn read_flood_prob(hull_data: &pickled::Dict) -> Option<f32> {
    use crate::game_params::ttx::constants::DEFAULT_UW_DAMAGE_COEFF;
    let node0 = read_vec3_at(hull_data.get(&pk(keys::FLOOD_NODES))?, 0)?[0];
    if node0 == DEFAULT_UW_DAMAGE_COEFF {
        Some(0.0)
    } else {
        Some((DEFAULT_UW_DAMAGE_COEFF - node0) / DEFAULT_UW_DAMAGE_COEFF)
    }
}

/// Read hull `burnNodes`: a list or tuple of `(probability, damage, duration)`
/// triples, one per fire section. Returns empty when the key is absent so callers
/// can tell "no sections known" from "four sections"; there is no correct default.
fn read_burn_nodes(hull_data: &pickled::Dict) -> Vec<crate::game_params::ttx::components::BurnNode> {
    read_node_triples(hull_data, keys::BURN_NODES)
        .into_iter()
        .map(|t| crate::game_params::ttx::components::BurnNode {
            probability: t[0],
            damage_per_sec: crate::game_params::ttx::model::Percent::from(t[1]),
            duration: crate::game_params::ttx::model::Seconds::from(t[2]),
        })
        .collect()
}

/// Read hull `floodNodes`: `(torpedo damage coefficient, damage, duration)`
/// triples, one per flooding section.
fn read_flood_nodes(hull_data: &pickled::Dict) -> Vec<crate::game_params::ttx::components::FloodNode> {
    read_node_triples(hull_data, keys::FLOOD_NODES)
        .into_iter()
        .map(|t| crate::game_params::ttx::components::FloodNode {
            torpedo_damage_coeff: t[0],
            damage_per_sec: crate::game_params::ttx::model::Percent::from(t[1]),
            duration: crate::game_params::ttx::model::Seconds::from(t[2]),
        })
        .collect()
}

/// The hull sections of a hull component: its entries whose `hlType` is a hull
/// section type, sorted by name.
fn read_hull_hit_locations(hull_data: &pickled::Dict) -> Vec<crate::game_params::ttx::components::HullHitLocation> {
    use crate::game_params::ttx::components::HullHitLocation;
    use crate::game_params::ttx::components::HullSectionType;

    let mut sections: Vec<HullHitLocation> = hull_data
        .iter()
        .filter_map(|(key, value)| {
            let name = key.string_ref()?.inner().to_string();
            let section = value.dict_or_object_dict()?;
            let section = section.inner();
            let section_type = HullSectionType::from_hl_type(&read_string(&section, keys::HL_TYPE)?)?;
            Some(HullHitLocation {
                name,
                section_type,
                max_hp: read_float(&section, keys::MAX_HP).map(crate::game_params::ttx::model::Hp::from),
                parent: read_string(&section, keys::PARENT_HL),
                volume_coeff: read_float(&section, keys::VOLUME_COEFF),
                regenerated_hp_part: read_float(&section, keys::REGENERATED_HP_PART),
                enhanced_regenerated_hp_part: read_float(&section, keys::ENHANCED_REGENERATED_HP_PART),
            })
        })
        .collect();
    sections.sort_by(|left, right| left.name.cmp(&right.name));

    sections
}

/// The float triples of a hull node list. Returns empty when the key is absent.
fn read_node_triples(hull_data: &pickled::Dict, key: &str) -> Vec<[f32; 3]> {
    let Some(value) = hull_data.get(&pk(key)) else {
        return Vec::new();
    };

    if let Some(l) = value.list_ref() {
        node_triples(l.inner().as_slice(), key)
    } else if let Some(t) = value.tuple_ref() {
        node_triples(t.inner().as_slice(), key)
    } else {
        tracing::warn!(key, "hull node list is neither a list nor a tuple");
        Vec::new()
    }
}

/// All or nothing: the length of the result is the hull's section count, so
/// dropping one unreadable triple would silently turn a four-section hull into a
/// three-section one for every consumer downstream.
fn node_triples(entries: &[Value], key: &str) -> Vec<[f32; 3]> {
    let mut nodes = Vec::with_capacity(entries.len());
    for entry in entries {
        let Some(t) = read_vec3_value(entry) else {
            tracing::warn!(
                key,
                entries = entries.len(),
                "hull node is not a float triple; treating the hull as having none"
            );
            return Vec::new();
        };
        nodes.push(t);
    }

    nodes
}

/// Read the submarine `SubmarineBattery` sub-object's `capacity` and `regenRate`
/// (PreprocessedHull.py:23-30). `None` for hulls without the sub-object (non-subs).
fn read_submarine_battery(hull_data: &pickled::Dict) -> (Option<f32>, Option<f32>) {
    let Some(battery) = hull_data.get(&pk(keys::SUBMARINE_BATTERY)).and_then(|v| v.dict_or_object_dict()) else {
        return (None, None);
    };
    let battery = battery.inner();
    (read_float(&battery, keys::BATTERY_CAPACITY), read_float(&battery, keys::BATTERY_REGEN_RATE))
}

/// Read `visibilityFactorsBySubmarine['PERISCOPE']` (PreprocessedHull.py:13).
/// `None` for hulls without the dict (non-subs) or a missing key.
fn read_visibility_by_periscope(hull_data: &pickled::Dict) -> Option<f32> {
    let dict = hull_data.get(&pk(keys::VISIBILITY_FACTORS_BY_SUBMARINE)).and_then(|v| v.dict_or_object_dict())?;
    let dict = dict.inner();
    read_float(&dict, keys::VISIBILITY_PERISCOPE)
}

/// Whether the hull can leave the surface. Every hull carries `buoyancyStates`;
/// the port reads them only when `maxBuoyancyLevel` is nonzero
/// (FactoryUnderwaterMobility.py:7, PreprocessedHull.py:36).
fn can_dive(hull_data: &pickled::Dict) -> bool {
    read_float(hull_data, keys::MAX_BUOYANCY_LEVEL).is_some_and(|level| level != 0.0)
}

/// Read `buoyancyStates`: per depth state `[[top, bottom], speed coefficient,
/// transition time]` (BuoyancyOwner.py:94). Ordered by depth state. Empty
/// for hulls that cannot dive.
fn read_depth_speed_coefs(hull_data: &pickled::Dict) -> Vec<crate::game_params::ttx::components::DepthSpeedCoef> {
    use wows_core::game_types::BuoyancyState;

    if !can_dive(hull_data) {
        return Vec::new();
    }
    let Some(states) = hull_data.get(&pk(keys::BUOYANCY_STATES)).and_then(|v| v.dict_or_object_dict()) else {
        return Vec::new();
    };

    let mut coefs: Vec<crate::game_params::ttx::components::DepthSpeedCoef> = states
        .inner()
        .iter()
        .filter_map(|(key, value)| {
            let name = key.string_ref()?.inner();
            let state = match name.as_str() {
                "SURFACE" => BuoyancyState::Surface,
                "PERISCOPE" => BuoyancyState::Periscope,
                "SEMI_DEEP_WATER" => BuoyancyState::SemiDeepWater,
                "DEEP_WATER" => BuoyancyState::DeepWater,
                "DEEP_WATER_INVUL" => BuoyancyState::DeepWaterInvul,
                other => {
                    tracing::warn!(state = other, "unknown buoyancy state; its speed coefficient is ignored");
                    return None;
                }
            };
            let speed_coef = if let Some(l) = value.list_ref() {
                value_f32(l.inner().get(1)?)
            } else {
                value_f32(value.tuple_ref()?.inner().get(1)?)
            }?;
            Some(crate::game_params::ttx::components::DepthSpeedCoef { state, speed_coef })
        })
        .collect();
    coefs.sort_by_key(|coef| coef.state as u8);

    coefs
}

/// Read `visibilityCoefGKInSmoke`, the in-smoke detection range.
/// `None` when absent or at or below `MINIMAL_VALID_VALUE`.
fn read_visibility_in_smoke(hull_data: &pickled::Dict) -> Option<f32> {
    // GameParams writes a near-zero placeholder for hulls without an in-smoke
    // range; the client shows it only above this gate (createVisibilityTTX@140).
    const MINIMAL_VALID_VALUE: f32 = 0.01;

    read_float(hull_data, keys::VISIBILITY_COEF_GK_IN_SMOKE).filter(|range| *range > MINIMAL_VALID_VALUE)
}

fn read_ignore_height(v: Option<&Value>) -> bool {
    v.map(|v| if let Some(b) = v.bool_ref() { *b } else { v.i64_ref().map(|i| *i != 0).unwrap_or(false) })
        .unwrap_or(false)
}

/// Parse one trajectory dict's orbit geometry: both posCenter FOV endpoints,
/// the per-endpoint ellipse radii, and the height gate. Returns `None` if any
/// are missing. `semi_axes[i]` carries `(semiAxisH, semiAxisV)` for FOV endpoint `i`.
fn read_trajectory_geometry(traj: &Value) -> Option<TrajectoryGeometry> {
    let dict = traj.dict_or_object_dict()?;
    let guard = dict.inner();
    let pc_val = guard.get(&pk("posCenter"));
    let pc0 = pc_val.and_then(|v| read_vec3_at(v, 0))?;
    let pc1 = pc_val.and_then(|v| read_vec3_at(v, 1))?;
    let h = guard.get(&pk("semiAxisH")).and_then(read_pair_both)?;
    let v = guard.get(&pk("semiAxisV")).and_then(read_pair_both)?;
    Some(TrajectoryGeometry {
        pos_center: [Vec3::new(pc0[0], pc0[1], pc0[2]), Vec3::new(pc1[0], pc1[1], pc1[2])],
        semi_axes: [Vec2::new(h[0], v[0]), Vec2::new(h[1], v[1])],
        ignore_height_multiplier: read_ignore_height(guard.get(&pk("ignoreHeightMultiplier"))),
    })
}

/// Read camera orbit trajectories from a ship's pickled `ship_data` dict.
/// Returns `(mode_name, trajectory)` per mode with a readable `InnerTrajectory`,
/// sorted by mode name; missing/malformed entries are skipped. A mode's
/// `OuterTrajectory` is read into `outer` when present.
pub fn read_camera_trajectories(ship_data: &pickled::Dict) -> Vec<(String, CameraTrajectory)> {
    let Some(cameras) = ship_data.get(&pk("Cameras")).and_then(|v| v.dict_or_object_dict()) else {
        return Vec::new();
    };
    let mut out: Vec<(String, CameraTrajectory)> = Vec::new();
    let cameras_guard = cameras.inner();
    for (key, val) in cameras_guard.iter() {
        let Some(name) = key.string_ref().map(|s| s.inner().to_string()) else {
            continue;
        };
        let Some(mode) = val.dict_or_object_dict() else {
            continue;
        };
        let mode_guard = mode.inner();
        let Some(inner) = mode_guard.get(&pk("InnerTrajectory")).and_then(read_trajectory_geometry) else {
            continue;
        };
        let tags =
            mode_guard.get(&pk("tags")).and_then(|v| v.string_ref()).map(|s| s.inner().to_string()).unwrap_or_default();
        let outer = mode_guard.get(&pk("OuterTrajectory")).and_then(read_trajectory_geometry);
        out.push((
            name,
            CameraTrajectory {
                pos_center: inner.pos_center,
                semi_axes: inner.semi_axes,
                tags,
                ignore_height_multiplier: inner.ignore_height_multiplier,
                outer,
            },
        ));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Extract `pitchDeadZones` from a mount dict.
/// Each entry is `[yaw_min, yaw_max, pitch_min, pitch_max]` in degrees.
fn parse_pitch_dead_zones(mount_dict: &pickled::Dict) -> Vec<[f32; 4]> {
    fn as_f32(v: &Value) -> Option<f32> {
        v.f64_ref().map(|f| *f as f32).or_else(|| v.i64_ref().map(|i| *i as f32))
    }
    fn extract_entries(items: &[Value]) -> Vec<[f32; 4]> {
        items
            .iter()
            .filter_map(|entry| {
                let inner: Vec<f32> = if let Some(l) = entry.list_ref() {
                    l.inner().iter().filter_map(as_f32).collect()
                } else {
                    let t = entry.tuple_ref()?;
                    t.inner().iter().filter_map(as_f32).collect()
                };
                if inner.len() == 4 { Some([inner[0], inner[1], inner[2], inner[3]]) } else { None }
            })
            .collect()
    }
    let Some(val) = mount_dict.get(&pk("pitchDeadZones")) else {
        return Vec::new();
    };
    if let Some(l) = val.list_ref() {
        extract_entries(&l.inner())
    } else if let Some(t) = val.tuple_ref() {
        extract_entries(t.inner())
    } else {
        Vec::new()
    }
}

/// Reads a two-number `(start, end)` angle pair in degrees.
fn read_yaw_sector(value: &Value) -> Option<YawSector> {
    let numbers: Vec<f32> = match value {
        Value::List(list) => list.inner().iter().filter_map(read_number).collect(),
        Value::Tuple(tuple) => tuple.inner().iter().filter_map(read_number).collect(),
        _ => return None,
    };
    let [start, end] = numbers.as_slice() else {
        return None;
    };

    Some(YawSector::new(Degrees::new(*start), Degrees::new(*end)))
}

fn read_number(value: &Value) -> Option<f32> {
    value.f64_ref().map(|float| *float as f32).or_else(|| value.i64_ref().map(|int| *int as f32))
}

/// Attaches `horizSector`, `deadZone`, `additionalAimSector`, `numBarrels`
/// and `ammoList` of a mount dict. `horizSector` with equal ends means
/// unlimited rotation (the client's `limitedRotation` test); a span of a
/// full turn is unlimited as well. An `additionalAimSector` of zeros is no
/// start maneuver (the client's `hasAdditionalAimSector`).
fn with_aiming(mount_point: MountPoint, mount: &pickled::Dict) -> MountPoint {
    let traverse = mount.get(&pk("horizSector")).and_then(read_yaw_sector).map(|sector| {
        let span = (sector.end.value() - sector.start.value()).abs();
        if sector.start == sector.end || span >= 360.0 { Traverse::Unlimited } else { Traverse::Limited(sector) }
    });

    let dead_zones = match mount.get(&pk("deadZone")) {
        Some(Value::List(list)) => list.inner().iter().filter_map(read_yaw_sector).collect(),
        Some(Value::Tuple(tuple)) => tuple.inner().iter().filter_map(read_yaw_sector).collect(),
        _ => Vec::new(),
    };

    let aim_offsets = mount
        .get(&pk("additionalAimSector"))
        .and_then(read_yaw_sector)
        .filter(|offsets| offsets.start.value() != 0.0 || offsets.end.value() != 0.0);

    // GameParams stores the count as a float; a fractional or negative count
    // is malformed and left absent.
    let num_barrels = read_float(mount, keys::NUM_BARRELS)
        .filter(|count| *count >= 0.0 && count.fract() == 0.0)
        .map(|count| count as u32);

    // ammoList can be pickled as a List or a Tuple.
    let ammo = match mount.get(&pk(keys::AMMO_LIST)) {
        Some(Value::Tuple(tuple)) => {
            tuple.inner().iter().filter_map(|value| value.string_ref().map(|name| name.inner().clone())).collect()
        }
        Some(value) => read_all_strings(value),
        None => Vec::new(),
    };

    mount_point.with_aiming(traverse, dead_zones, aim_offsets, num_barrels, ammo)
}

/// Parse a raw GameParams armor dict into an [`ArmorMap`].
///
/// Raw keys are `(model_index << 16) | material_id`.  We group by `material_id`
/// and collect per-layer thicknesses ordered by ascending `model_index`.
fn parse_armor_dict(dict: &pickled::Dict) -> ArmorMap {
    use std::collections::BTreeMap;

    // First pass: collect (model_index, material_id) → thickness.
    let mut by_material: HashMap<u32, BTreeMap<u32, f32>> = HashMap::new();
    for (k, v) in dict.iter() {
        let raw_key: u32 =
            match k.string_ref().and_then(|s| s.inner().parse().ok()).or_else(|| k.i64_ref().map(|&i| i as u32)) {
                Some(k) => k,
                None => continue,
            };
        let value = match v.f64_ref().map(|f| *f as f32).or_else(|| v.i64_ref().map(|i| *i as f32)) {
            Some(v) => v,
            None => continue,
        };
        let model_index = raw_key >> 16;
        let material_id = raw_key & 0xFFFF;
        by_material.entry(material_id).or_default().insert(model_index, value);
    }

    by_material
}

/// Extract mount points (HP_* entries with model paths) from a component dict.
fn extract_mounts(ship_data: &pickled::Dict, component_name: &str) -> Vec<MountPoint> {
    let Some(comp_data) = ship_data.get(&pk(component_name)).and_then(|v| v.dict_or_object_dict()) else {
        return Vec::new();
    };

    comp_data
        .inner()
        .iter()
        .filter_map(|(k, v)| {
            let key_str = k.string_ref()?.inner();
            if !key_str.starts_with(keys::HP_PREFIX) {
                return None;
            }
            let mount_dict = v.dict_or_object_dict()?;
            let mount_inner = mount_dict.inner();
            let model_path = read_string(&mount_inner, keys::MODEL)?;

            // Extract per-mount armor map (turret shell thickness).
            let mount_armor: Option<ArmorMap> = mount_inner
                .get(&pk(keys::ARMOR))
                .and_then(|v| v.dict_or_object_dict())
                .map(|d| parse_armor_dict(&d.inner()));

            // Extract mount species from typeinfo.species.
            let species = mount_inner
                .get(&pk("typeinfo"))
                .and_then(|v| v.dict_or_object_dict())
                .and_then(|d| read_string(&d.inner(), "species"))
                .and_then(|s| MountSpecies::from_gp_str(&s));

            // Extract pitchDeadZones: list of [yaw_min, yaw_max, pitch_min, pitch_max].
            let pitch_dead_zones: Vec<[f32; 4]> = parse_pitch_dead_zones(&mount_inner);

            // Misc-part selection: a whitelist of node names (`miscFilter`) plus a
            // preset-keyed extra set (`customMiscs`). Applied when rendering the
            // mount model's own misc nodes.
            let misc_filter = mount_inner.get(&pk("miscFilter")).map(read_string_seq).unwrap_or_default();
            let custom_miscs = mount_inner
                .get(&pk("customMiscs"))
                .and_then(|v| v.dict_or_object_dict())
                .map(|d| parse_custom_miscs(&d.inner()))
                .unwrap_or_default();

            let mount_point =
                MountPoint::with_armor(key_str.clone(), model_path, mount_armor, species, pitch_dead_zones)
                    .with_miscs(misc_filter, custom_miscs);

            Some(with_aiming(mount_point, &mount_inner))
        })
        .collect()
}

/// Parse the trailing decimal digits of a hardpoint name.
fn hardpoint_number(key: &str) -> Option<u32> {
    // ASCII digits are one byte each, so the count is a valid byte offset.
    let digit_count = key.chars().rev().take_while(|c| c.is_ascii_digit()).count();
    key[key.len() - digit_count..].parse().ok()
}

/// `ShipAbilities` or `PlaneAbilities`: per slot in slot order, the
/// `(ability, category)` choices.
fn build_ability_slots(abilities_data: &pickled::Dict) -> Vec<Vec<(String, String)>> {
    abilities_data
        .iter()
        .filter_map(|(slot_name, slot_data)| {
            let _slot_name = slot_name.string_ref().expect("ability slot name is not a string");
            if slot_data.is_none() {
                return None;
            }

            let slot_data = slot_data.dict_or_object_dict().expect("slot data is not a dictionary");
            let slot_data = slot_data.inner();

            let slot = game_param_to_type!(slot_data, "slot", usize);
            let abils = game_param_to_type!(slot_data, "abils", &[()]).inner();
            let abils: Vec<(String, String)> = abils
                .iter()
                .map(|abil| {
                    let map_abil = |abil: &Vec<Value>| {
                        (
                            abil[0].string_ref().expect("abil[0] is not a string").inner().clone(),
                            abil[1].string_ref().expect("abil[1] is not a string").inner().clone(),
                        )
                    };
                    match abil {
                        Value::Tuple(inner) => map_abil(inner.inner()),
                        Value::List(inner) => map_abil(&inner.inner()),
                        _ => panic!("abil is not a list/tuple"),
                    }
                })
                .collect();

            Some((slot, abils))
        })
        .sorted_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, abils)| abils)
        .collect()
}

fn build_ship(ship_data: &pickled::Dict) -> Vehicle {
    let ability_data = game_param_to_type!(ship_data, keys::SHIP_ABILITIES, Option<HashMap<(), ()>>);
    let abilities: Option<Vec<Vec<(String, String)>>> =
        ability_data.map(|abilities_data| build_ability_slots(&abilities_data.inner()));

    let upgrade_data = game_param_to_type!(ship_data, keys::SHIP_UPGRADE_INFO, HashMap<(), ()>);
    let research = read_ship_research(&upgrade_data.inner());
    let upgrades: Vec<String> = upgrade_data
        .inner()
        .keys()
        .map(|upgrade_name| upgrade_name.string_ref().expect("upgrade name is not a string").inner().to_owned())
        .collect();

    let level = game_param_to_type!(ship_data, "level", u32);
    let group = game_param_to_type!(ship_data, "group", String);
    let origin_ship = read_string(ship_data, keys::ORIGIN_SHIP_NAME).filter(|name| !name.is_empty());
    let peculiarity = read_string(ship_data, keys::PECULIARITY)
        .filter(|peculiarity| !peculiarity.is_empty() && peculiarity != keys::PECULIARITY_DEFAULT);

    // Extract hull config data using ShipUpgradeInfo for proper type identification.
    // Each _Hull upgrade in ShipUpgradeInfo maps to specific hull, artillery, and ATBA
    // components. We build a HullUpgradeConfig for each, keyed by upgrade name.
    let mut hull_upgrades = HashMap::new();

    // TTX base stats (hull + engine), keyed by upgrade selection. Populated from
    // the same ShipUpgradeInfo walk: hull stats inside the _Hull branch below,
    // engine stats from the separate _Engine upgrade entries.
    let mut ttx_components = crate::game_params::ttx::components::ShipTtxComponents::default();

    // Innate skills are hull-level and identical across hull upgrades in practice.
    // Collected once, from the first _Hull upgrade that carries an innateSkills
    // component; subsequent hulls are skipped.
    let mut innate_skills: Vec<InnateSkill> = Vec::new();
    let mut innate_skills_set = false;

    for (upgrade_name_val, upgrade_value) in upgrade_data.inner().iter() {
        let Some(upgrade_name) = upgrade_name_val.string_ref().map(|s| s.inner().clone()) else {
            continue;
        };
        let Some(upgrade_dict) = upgrade_value.dict_or_object_dict() else {
            continue;
        };
        let upgrade_dict = upgrade_dict.inner();

        let Some(uc_type) =
            upgrade_dict.get(&pk(keys::UC_TYPE)).and_then(|v| v.string_ref().map(|s| s.inner().clone()))
        else {
            continue;
        };

        // Stock selection: the chain root in each slot is the empty-`prev` upgrade.
        // An absent `prev` key (single-option slots) is treated as the root too.
        let is_stock = match upgrade_dict.get(&pk(keys::PREV)) {
            Some(v) => v.string_ref().map(|s| s.inner().is_empty()).unwrap_or(true),
            None => true,
        };
        if is_stock {
            let slot = match uc_type.as_str() {
                keys::UC_TYPE_HULL => Some(&mut ttx_components.stock_selection.hull),
                keys::UC_TYPE_ENGINE => Some(&mut ttx_components.stock_selection.engine),
                keys::UC_TYPE_ARTILLERY => Some(&mut ttx_components.stock_selection.artillery),
                keys::UC_TYPE_TORPEDOES => Some(&mut ttx_components.stock_selection.torpedoes),
                keys::UC_TYPE_SUO => Some(&mut ttx_components.stock_selection.fire_control),
                _ => None,
            };
            if let Some(slot) = slot {
                slot.get_or_insert_with(|| upgrade_name.clone());
            }
        }

        // Engine is a standalone _Engine upgrade (not nested in the hull upgrade's
        // components); read its speedCoef into the TTX engine map.
        if uc_type == keys::UC_TYPE_ENGINE
            && let Some(eng_comp) = upgrade_dict
                .get(&pk(keys::COMPONENTS))
                .and_then(|v| v.dict_or_object_dict())
                .and_then(|c| c.inner().get(&pk(keys::COMP_ENGINE)).and_then(read_first_string))
            && let Some(eng_data) = ship_data.get(&pk(&eng_comp)).and_then(|v| v.dict_or_object_dict())
        {
            ttx_components.engines.insert(
                upgrade_name.clone(),
                crate::game_params::ttx::components::EngineComponentStats {
                    speed_coef: read_float(&eng_data.inner(), keys::SPEED_COEF),
                },
            );
        }

        // A gun or torpedo upgrade lists one component per hull it fits, in
        // hull order; a hull mounts the listed one it names as its default or
        // as an alternative.
        let component_key = if uc_type == keys::UC_TYPE_TORPEDOES {
            Some(keys::COMP_TORPEDOES)
        } else if uc_type == keys::UC_TYPE_ARTILLERY {
            Some(keys::COMP_ARTILLERY)
        } else {
            None
        };
        if let Some(component_key) = component_key {
            let listed = upgrade_dict
                .get(&pk(keys::COMPONENTS))
                .and_then(|v| v.dict_or_object_dict())
                .and_then(|c| c.inner().get(&pk(component_key)).map(read_all_strings))
                .unwrap_or_default();
            if component_key == keys::COMP_ARTILLERY {
                for component in &listed {
                    if ttx_components.alternative_fire.contains_key(component) {
                        continue;
                    }
                    let Some(data) = ship_data.get(&pk(component)).and_then(|v| v.dict_or_object_dict()) else {
                        continue;
                    };

                    if let Some(mode) = read_alternative_fire(&data.inner()) {
                        ttx_components.alternative_fire.insert(component.clone(), mode);
                    }
                }
            }
            if !listed.is_empty() {
                ttx_components.upgrade_components.insert(upgrade_name.clone(), listed);
            }
        }

        // Combat Instructions hang off the hull's `specials` component.
        if uc_type == keys::UC_TYPE_HULL
            && let Some(specials) = upgrade_dict
                .get(&pk(keys::COMPONENTS))
                .and_then(|v| v.dict_or_object_dict())
                .and_then(|c| c.inner().get(&pk(keys::COMP_SPECIALS)).and_then(read_first_string))
            && let Some(specials_data) = ship_data.get(&pk(&specials)).and_then(|v| v.dict_or_object_dict())
            && let Some(instructions) = read_combat_instructions(&specials_data.inner())
        {
            ttx_components.combat_instructions.insert(upgrade_name.clone(), instructions);
        }

        // Torpedoes are a standalone _Torpedoes upgrade naming a torpedo component
        // whose HP_AGT_* gun sub-objects are the launchers; read each launcher's
        // base stats and ammo names into the TTX torpedo map.
        if uc_type == keys::UC_TYPE_TORPEDOES
            && let Some(torp_comp) = upgrade_dict
                .get(&pk(keys::COMPONENTS))
                .and_then(|v| v.dict_or_object_dict())
                .and_then(|c| c.inner().get(&pk(keys::COMP_TORPEDOES)).and_then(read_first_string))
            && let Some(torp_data) = ship_data.get(&pk(&torp_comp)).and_then(|v| v.dict_or_object_dict())
        {
            let mut launchers = Vec::new();
            for (key, val) in torp_data.inner().iter() {
                let is_launcher = key.string_ref().is_some_and(|s| keys::is_torpedo_hardpoint(s.inner()));
                if !is_launcher {
                    continue;
                }
                let Some(gun) = val.dict_or_object_dict() else {
                    continue;
                };
                let gun = gun.inner();
                // ammoList can be pickled as a List or a Tuple.
                let ammo = match gun.get(&pk(keys::AMMO_LIST)) {
                    Some(Value::Tuple(t)) => {
                        t.inner().iter().filter_map(|v| v.string_ref().map(|s| s.inner().clone())).collect()
                    }
                    Some(val) => read_all_strings(val),
                    None => Vec::new(),
                };
                launchers.push(crate::game_params::ttx::components::TorpedoLauncherStats {
                    shot_delay: read_float(&gun, keys::SHOT_DELAY).map(crate::game_params::ttx::model::Seconds::from),
                    rotation_speed: gun
                        .get(&pk(keys::ROTATION_SPEED))
                        .and_then(read_first_float)
                        .map(crate::game_params::ttx::model::DegreesPerSecond::from),
                    num_barrels: read_float(&gun, keys::NUM_BARRELS),
                    ammo_switch_coeff: read_float(&gun, keys::AMMO_SWITCH_COEFF),
                    ammo,
                });
            }
            if !launchers.is_empty() {
                ttx_components.torpedoes.insert(upgrade_name.clone(), launchers);
            }

            // Every component the upgrade lists, one per hull it fits.
            let components = upgrade_dict
                .get(&pk(keys::COMPONENTS))
                .and_then(|v| v.dict_or_object_dict())
                .and_then(|c| c.inner().get(&pk(keys::COMP_TORPEDOES)).map(read_all_strings))
                .unwrap_or_default();
            for component in components {
                if ttx_components.torpedo_loading.contains_key(&component) {
                    continue;
                }
                let Some(data) = ship_data.get(&pk(&component)).and_then(|v| v.dict_or_object_dict()) else {
                    continue;
                };

                if let Some(loading) = read_torpedo_loading(&data.inner()) {
                    ttx_components.torpedo_loading.insert(component, loading);
                }
            }
        }

        // Main battery is a standalone _Artillery upgrade naming an artillery
        // component with a component-level maxDist and HP_AGM_* gun sub-objects;
        // read the range plus each gun's base stats and ammo names.
        if uc_type == keys::UC_TYPE_ARTILLERY
            && let Some(arty_comp) = upgrade_dict
                .get(&pk(keys::COMPONENTS))
                .and_then(|v| v.dict_or_object_dict())
                .and_then(|c| c.inner().get(&pk(keys::COMP_ARTILLERY)).and_then(read_first_string))
            && let Some(arty_data) = ship_data.get(&pk(&arty_comp)).and_then(|v| v.dict_or_object_dict())
        {
            let arty_data = arty_data.inner();
            let mut guns = Vec::new();
            for (key, val) in arty_data.iter() {
                let is_gun = key.string_ref().is_some_and(|s| keys::is_main_gun_hardpoint(s.inner()));
                if !is_gun {
                    continue;
                }
                let Some(gun) = val.dict_or_object_dict() else {
                    continue;
                };
                let gun = gun.inner();
                // ammoList can be pickled as a List or a Tuple.
                let ammo = match gun.get(&pk(keys::AMMO_LIST)) {
                    Some(Value::Tuple(t)) => {
                        t.inner().iter().filter_map(|v| v.string_ref().map(|s| s.inner().clone())).collect()
                    }
                    Some(val) => read_all_strings(val),
                    None => Vec::new(),
                };
                guns.push(crate::game_params::ttx::components::ArtilleryGunStats {
                    shot_delay: read_float(&gun, keys::SHOT_DELAY).map(crate::game_params::ttx::model::Seconds::from),
                    rotation_speed: gun
                        .get(&pk(keys::ROTATION_SPEED))
                        .and_then(read_first_float)
                        .map(crate::game_params::ttx::model::DegreesPerSecond::from),
                    num_barrels: read_float(&gun, keys::NUM_BARRELS),
                    barrel_diameter: read_float(&gun, keys::BARREL_DIAMETER).map(Meters::from),
                    ammo_switch_coeff: read_float(&gun, keys::AMMO_SWITCH_COEFF),
                    min_radius: read_float(&gun, keys::MIN_RADIUS),
                    ideal_radius: read_float(&gun, keys::IDEAL_RADIUS),
                    ideal_distance: read_float(&gun, keys::IDEAL_DISTANCE),
                    radius_on_zero: read_float(&gun, keys::RADIUS_ON_ZERO),
                    radius_on_delim: read_float(&gun, keys::RADIUS_ON_DELIM),
                    radius_on_max: read_float(&gun, keys::RADIUS_ON_MAX),
                    delim: read_float(&gun, keys::DISPERSION_DELIM),
                    ammo,
                });
            }
            if !guns.is_empty() {
                ttx_components.artillery.insert(
                    upgrade_name.clone(),
                    crate::game_params::ttx::components::ArtilleryComponentStats {
                        max_dist: read_float(&arty_data, keys::MAX_DIST).map(Meters::from),
                        sigma_count: read_float(&arty_data, keys::SIGMA_COUNT),
                        guns,
                    },
                );
            }
        }

        // Secondaries (ATBA) are referenced by _Hull upgrades via the `atba` component
        // slot; the component carries a component-level maxDist and HP_<nation>GS_* gun
        // sub-objects (mixed calibers). Read the range plus each gun's base stats and
        // ammo names, keyed by hull upgrade name (PreprocessedATBA.py:12-30).
        if uc_type == keys::UC_TYPE_HULL
            && let Some(atba_comp) = upgrade_dict
                .get(&pk(keys::COMPONENTS))
                .and_then(|v| v.dict_or_object_dict())
                .and_then(|c| c.inner().get(&pk(keys::COMP_ATBA)).and_then(read_first_string))
            && let Some(atba_data) = ship_data.get(&pk(&atba_comp)).and_then(|v| v.dict_or_object_dict())
        {
            let atba_data = atba_data.inner();
            let mut guns = Vec::new();
            for (key, val) in atba_data.iter() {
                let Some(hardpoint) =
                    key.string_ref().map(|s| s.inner()).filter(|name| keys::is_secondary_gun_hardpoint(name))
                else {
                    continue;
                };
                let Some(gun) = val.dict_or_object_dict() else {
                    continue;
                };
                let gun = gun.inner();
                // ammoList can be pickled as a List or a Tuple.
                let ammo = match gun.get(&pk(keys::AMMO_LIST)) {
                    Some(Value::Tuple(t)) => {
                        t.inner().iter().filter_map(|v| v.string_ref().map(|s| s.inner().clone())).collect()
                    }
                    Some(val) => read_all_strings(val),
                    None => Vec::new(),
                };
                let stats = crate::game_params::ttx::components::ArtilleryGunStats {
                    shot_delay: read_float(&gun, keys::SHOT_DELAY).map(crate::game_params::ttx::model::Seconds::from),
                    rotation_speed: gun
                        .get(&pk(keys::ROTATION_SPEED))
                        .and_then(read_first_float)
                        .map(crate::game_params::ttx::model::DegreesPerSecond::from),
                    num_barrels: read_float(&gun, keys::NUM_BARRELS),
                    barrel_diameter: read_float(&gun, keys::BARREL_DIAMETER).map(Meters::from),
                    ammo_switch_coeff: read_float(&gun, keys::AMMO_SWITCH_COEFF),
                    min_radius: read_float(&gun, keys::MIN_RADIUS),
                    ideal_radius: read_float(&gun, keys::IDEAL_RADIUS),
                    ideal_distance: read_float(&gun, keys::IDEAL_DISTANCE),
                    radius_on_zero: read_float(&gun, keys::RADIUS_ON_ZERO),
                    radius_on_delim: read_float(&gun, keys::RADIUS_ON_DELIM),
                    radius_on_max: read_float(&gun, keys::RADIUS_ON_MAX),
                    delim: read_float(&gun, keys::DISPERSION_DELIM),
                    ammo,
                };
                guns.push(crate::game_params::ttx::components::SecondaryGun { hardpoint: hardpoint.clone(), stats });
            }
            if !guns.is_empty() {
                ttx_components.secondaries.insert(
                    upgrade_name.clone(),
                    crate::game_params::ttx::components::SecondaryComponentStats {
                        // maxDist is stored in KM downstream (PreprocessedATBA.py:30);
                        // keep the raw meters value here and divide at the factory.
                        max_dist: read_float(&atba_data, keys::MAX_DIST).map(Meters::from),
                        sigma_count: read_float(&atba_data, keys::SIGMA_COUNT),
                        guns,
                    },
                );
            }
        }

        // Carrier and hybrid squadrons: each plane-slot upgrade names a component
        // whose `planes` list holds the aircraft params.
        if let Some(slot) =
            crate::game_params::ttx::components::PlaneSlot::ALL.into_iter().find(|slot| slot.uc_type() == uc_type)
            && let Some(plane_comp) = upgrade_dict
                .get(&pk(keys::COMPONENTS))
                .and_then(|v| v.dict_or_object_dict())
                .and_then(|c| c.inner().get(&pk(slot.component_key())).and_then(read_first_string))
            && let Some(plane_data) = ship_data.get(&pk(&plane_comp)).and_then(|v| v.dict_or_object_dict())
        {
            let planes = plane_data.inner().get(&pk(keys::PLANES)).map(read_string_sequence).unwrap_or_default();
            if !planes.is_empty() {
                ttx_components.squadrons.insert(
                    upgrade_name.clone(),
                    crate::game_params::ttx::components::SquadronUpgrade { slot, planes, stock: is_stock },
                );
            }
        }

        // Air strikes hang off the hull: its `airSupport` component names the
        // aircraft in `ammoList`.
        if uc_type == keys::UC_TYPE_HULL
            && let Some(support_comp) = upgrade_dict
                .get(&pk(keys::COMPONENTS))
                .and_then(|v| v.dict_or_object_dict())
                .and_then(|c| c.inner().get(&pk(keys::COMP_AIR_SUPPORT)).and_then(read_first_string))
            && let Some(support_data) = ship_data.get(&pk(&support_comp)).and_then(|v| v.dict_or_object_dict())
        {
            let support_data = support_data.inner();
            let planes = support_data.get(&pk(keys::AMMO_LIST)).map(read_string_sequence).unwrap_or_default();
            if !planes.is_empty() {
                ttx_components.air_supports.insert(
                    upgrade_name.clone(),
                    crate::game_params::ttx::components::AirSupportStats {
                        planes,
                        // A negative or fractional chargesNum is not a charge count.
                        charges: read_float(&support_data, keys::CHARGES_NUM)
                            .filter(|charges| *charges >= 0.0 && charges.fract() == 0.0)
                            .map(|charges| charges as u32),
                        reload_time: read_float(&support_data, keys::RELOAD_TIME)
                            .map(crate::game_params::ttx::model::Seconds::from),
                        max_dist: read_float(&support_data, keys::MAX_DIST).map(Meters::from),
                    },
                );
            }
        }

        // Ship-mounted depth charges hang off the hull as well: its
        // `depthCharges` component holds the reload and charges, and one
        // `HP_*` launcher per mount.
        if uc_type == keys::UC_TYPE_HULL
            && let Some(charges_comp) = upgrade_dict
                .get(&pk(keys::COMPONENTS))
                .and_then(|v| v.dict_or_object_dict())
                .and_then(|c| c.inner().get(&pk(keys::COMP_DEPTH_CHARGES)).and_then(read_first_string))
            && let Some(charges_data) = ship_data.get(&pk(&charges_comp)).and_then(|v| v.dict_or_object_dict())
        {
            let charges_data = charges_data.inner();
            // A negative or fractional count is malformed and left absent.
            let count = |dict: &pickled::Dict, key: &str| {
                read_float(dict, key).filter(|count| *count >= 0.0 && count.fract() == 0.0).map(|count| count as u32)
            };
            let mut launchers: Vec<(String, crate::game_params::ttx::components::DepthChargeLauncher)> = charges_data
                .iter()
                .filter_map(|(key, value)| {
                    let name = key.string_ref()?.inner();
                    if !name.starts_with(keys::HP_PREFIX) {
                        return None;
                    }
                    let launcher = value.dict_or_object_dict()?;
                    let launcher = launcher.inner();
                    Some((
                        name.clone(),
                        crate::game_params::ttx::components::DepthChargeLauncher {
                            bombs: count(&launcher, keys::NUM_BOMBS),
                            ammo: launcher.get(&pk(keys::AMMO_LIST)).map(read_string_sequence).unwrap_or_default(),
                        },
                    ))
                })
                .collect();
            // By hardpoint, so the first launcher does not depend on dict order.
            launchers.sort_by(|(left, _), (right, _)| left.cmp(right));
            if !launchers.is_empty() {
                ttx_components.depth_charges.insert(
                    upgrade_name.clone(),
                    crate::game_params::ttx::components::DepthChargeStats {
                        charges: count(&charges_data, keys::MAX_PACKS),
                        reload_time: read_float(&charges_data, keys::RELOAD_TIME)
                            .map(crate::game_params::ttx::model::Seconds::from),
                        shots_per_attack: count(&charges_data, keys::NUM_SHOTS),
                        launchers: launchers.into_iter().map(|(_, launcher)| launcher).collect(),
                    },
                );
            }
        }

        // Fire control is a standalone _Suo upgrade naming an FC component with
        // maxDistCoef and sigmaCountCoef (PreprocessedFireControl.py:7); store both
        // keyed by upgrade name.
        if uc_type == keys::UC_TYPE_SUO
            && let Some(fc_comp) = upgrade_dict
                .get(&pk(keys::COMPONENTS))
                .and_then(|v| v.dict_or_object_dict())
                .and_then(|c| c.inner().get(&pk(keys::COMP_FIRE_CONTROL)).and_then(read_first_string))
            && let Some(fc_data) = ship_data.get(&pk(&fc_comp)).and_then(|v| v.dict_or_object_dict())
            && let Some(max_dist_coef) = read_float(&fc_data.inner(), keys::MAX_DIST_COEF)
        {
            // The client's `FireControl.sigmaCountCoef` default.
            let sigma_count_coef = read_float(&fc_data.inner(), keys::SIGMA_COUNT_COEF).unwrap_or(1.0);
            ttx_components.fire_controls.insert(
                upgrade_name.clone(),
                crate::game_params::ttx::components::FireControlStats { max_dist_coef, sigma_count_coef },
            );
        }

        // Only process _Hull upgrades -- they define the complete config for a hull loadout
        if uc_type != keys::UC_TYPE_HULL {
            continue;
        }

        let Some(components) = upgrade_dict.get(&pk(keys::COMPONENTS)).and_then(|v| v.dict_or_object_dict()) else {
            continue;
        };
        let components = components.inner();

        // Innate skills are parsed from the stock hull only; all hulls carry the same
        // innateSkills component name in practice, so we stop after the first find.
        if !innate_skills_set && let Some(innate_names) = components.get(&pk(keys::COMP_INNATE_SKILLS)) {
            for innate_name in read_all_strings(innate_names) {
                let Some(innate_comp) = ship_data.get(&pk(&innate_name)).and_then(|v| v.dict_or_object_dict()) else {
                    continue;
                };
                let innate_comp = innate_comp.inner();
                for (_, skill_val) in innate_comp.iter() {
                    let Some(skill_dict) = skill_val.dict_or_object_dict() else {
                        continue;
                    };
                    let skill_dict = skill_dict.inner();
                    let Some(skill_type) = read_string(&skill_dict, "skillType") else {
                        continue;
                    };
                    let breakpoints = skill_dict
                        .get(&pk("healthModifiers"))
                        .and_then(|v| v.list_ref())
                        .map(|list| {
                            list.inner()
                                .iter()
                                .filter_map(|pair| {
                                    let pair = pair
                                        .list_ref()
                                        .map(|l| l.inner().clone())
                                        .or_else(|| pair.tuple_ref().map(|t| t.inner().to_vec()))?;
                                    let health_fraction = pair.first().and_then(value_f32)?;
                                    let bp_name = pair.get(1)?.string_ref()?.inner().clone();
                                    let block = skill_dict.get(&pk(&bp_name)).and_then(|v| v.dict_or_object_dict())?;
                                    let modifiers = build_skill_modifiers(&block.inner());
                                    Some(InnateSkillBreakpoint::new(health_fraction, modifiers))
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    innate_skills.push(InnateSkill::new(skill_type, breakpoints));
                }
            }
            innate_skills_set = true;
        }

        let mut config = crate::game_params::types::HullUpgradeConfig::default();

        // Extract component name mappings for all component types.
        // Also track alternatives (multiple options per type).
        for ct in keys::ComponentType::ALL {
            if let Some(val) = components.get(&pk(ct.key())) {
                let all_names = read_all_strings(val);
                if let Some(first) = all_names.first() {
                    config.component_names.insert(*ct, first.clone());
                }
                if all_names.len() > 1 {
                    config.component_alternatives.insert(*ct, all_names);
                }
            }
        }

        // Read hull detection data and hull model path.
        if let Some(hull_comp) = config.component_names.get(&keys::ComponentType::Hull)
            && let Some(hull_data) = ship_data.get(&pk(hull_comp)).and_then(|v| v.dict_or_object_dict())
        {
            let hull_data = hull_data.inner();
            // A zero visibilityFactor states no detection range.
            let detection = |key: &str| read_float(&hull_data, key).filter(|km| *km > 0.0).map(Km::from);
            config.detection_km = detection(keys::VISIBILITY_FACTOR);
            config.air_detection_km = detection(keys::VISIBILITY_FACTOR_BY_PLANE);
            config.hull_model_path = read_string(&hull_data, keys::MODEL);
            config.draft = read_float(&hull_data, keys::DRAFT).map(Meters::from);
            config.dock_y_offset = read_float(&hull_data, keys::DOCK_Y_OFFSET);

            // TTX hull base stats from the same resolved hull component sub-object.
            let (battery_capacity, battery_regen_rate) = read_submarine_battery(&hull_data);
            ttx_components.hulls.insert(
                upgrade_name.clone(),
                crate::game_params::ttx::components::HullComponentStats {
                    health: read_float(&hull_data, keys::HEALTH).map(crate::game_params::ttx::model::Hp::from),
                    max_speed: read_float(&hull_data, keys::MAX_SPEED).map(crate::game_params::ttx::model::Knots::from),
                    speed_coef: read_float(&hull_data, keys::SPEED_COEF),
                    turning_radius: read_float(&hull_data, keys::TURNING_RADIUS).map(Meters::from),
                    rudder_time: read_float(&hull_data, keys::RUDDER_TIME)
                        .map(crate::game_params::ttx::model::Seconds::from),
                    visibility_factor: read_float(&hull_data, keys::VISIBILITY_FACTOR).map(Km::from),
                    visibility_factor_by_plane: read_float(&hull_data, keys::VISIBILITY_FACTOR_BY_PLANE).map(Km::from),
                    visibility_coef_fire: read_float(&hull_data, keys::VISIBILITY_COEF_FIRE).map(Km::from),
                    visibility_coef_fire_by_plane: read_float(&hull_data, keys::VISIBILITY_COEF_FIRE_BY_PLANE)
                        .map(Km::from),
                    visibility_coef_gk_in_smoke: read_visibility_in_smoke(&hull_data).map(Km::from),
                    visibility_factor_by_periscope: read_visibility_by_periscope(&hull_data).map(Km::from),
                    flood_prob: read_flood_prob(&hull_data),
                    battery_capacity,
                    battery_regen_rate,
                    depth_speed_coefs: read_depth_speed_coefs(&hull_data),
                    buoyancy_rudder_time: can_dive(&hull_data)
                        .then(|| read_float(&hull_data, keys::BUOYANCY_RUDDER_TIME))
                        .flatten()
                        .map(crate::game_params::ttx::model::Seconds::from),
                    max_buoyancy_speed: can_dive(&hull_data)
                        .then(|| read_float(&hull_data, keys::MAX_BUOYANCY_SPEED))
                        .flatten()
                        .map(MetersPerSecond::from),
                    burn_nodes: read_burn_nodes(&hull_data),
                    flood_nodes: read_flood_nodes(&hull_data),
                    hit_locations: read_hull_hit_locations(&hull_data),
                },
            );
        }

        // Extract mount points for all model component types.
        for ct in keys::ComponentType::ALL {
            if let Some(comp_name) = config.component_names.get(ct) {
                let mounts = extract_mounts(ship_data, comp_name);
                if !mounts.is_empty() {
                    config.mounts_by_type.insert(*ct, ComponentMounts::new(comp_name.clone(), mounts));
                }
            }
        }

        // Extract mount points for alternative components (non-default selections).
        for alternatives in config.component_alternatives.values() {
            for alt_name in alternatives.iter().skip(1) {
                // Skip the first (default) since it is already in mounts_by_type.
                let mounts = extract_mounts(ship_data, alt_name);
                if !mounts.is_empty() {
                    config.alternative_mounts.insert(alt_name.clone(), ComponentMounts::new(alt_name.clone(), mounts));
                }
            }
        }

        // Store if we got meaningful data (detection or mounts).
        if config.detection_km.is_some() || !config.mounts_by_type.is_empty() {
            hull_upgrades.insert(upgrade_name, config);
        }
    }

    // Collect weapon ranges from all upgrade types.
    // Artillery, torpedo, and secondary upgrades are independent of hull selection.
    let mut torpedo_ammo = HashSet::new();
    let mut main_battery_ammo = HashSet::new();
    let mut secondary_battery_ammo = HashSet::new();
    let mut max_main_battery_m: Option<Meters> = None;
    let mut max_secondary_battery_m: Option<Meters> = None;
    // (hardpoint_key, first ammo name); populated from the first atba-bearing hull only
    let mut secondary_guns_raw: Vec<(String, Option<String>)> = Vec::new();

    for (_upgrade_name_val, upgrade_value) in upgrade_data.inner().iter() {
        let Some(upgrade_dict) = upgrade_value.dict_or_object_dict() else {
            continue;
        };
        let upgrade_dict = upgrade_dict.inner();
        let Some(uc_type) =
            upgrade_dict.get(&pk(keys::UC_TYPE)).and_then(|v| v.string_ref().map(|s| s.inner().clone()))
        else {
            continue;
        };

        let components = upgrade_dict.get(&pk(keys::COMPONENTS)).and_then(|v| v.dict_or_object_dict());
        let Some(components) = components else {
            continue;
        };

        // Collect main battery maxDist from _Hull and _Artillery upgrades
        if (uc_type == keys::UC_TYPE_HULL || uc_type == keys::UC_TYPE_ARTILLERY)
            && let Some(art_comp) = components.inner().get(&pk(keys::COMP_ARTILLERY)).and_then(read_first_string)
            && let Some(art_data) = ship_data.get(&pk(&art_comp)).and_then(|v| v.dict_or_object_dict())
        {
            if let Some(m) = read_float(&art_data.inner(), keys::MAX_DIST).map(Meters::from) {
                max_main_battery_m = Some(match max_main_battery_m {
                    Some(prev) if prev.value() >= m.value() => prev,
                    _ => m,
                });
            }
            // Collect main battery ammo from all mounts in the artillery component
            for (_mount_key, mount_val) in art_data.inner().iter() {
                let Some(mount_dict) = mount_val.dict_or_object_dict() else {
                    continue;
                };
                let mount_inner = mount_dict.inner();
                let Some(ammo_val) = mount_inner.get(&pk(keys::AMMO_LIST)) else {
                    continue;
                };
                let mut insert_ammo = |item: &Value| {
                    if let Some(name) = item.string_ref() {
                        main_battery_ammo.insert(name.inner().clone());
                    }
                };
                match ammo_val {
                    Value::Tuple(t) => t.inner().iter().for_each(&mut insert_ammo),
                    Value::List(l) => l.inner().iter().for_each(&mut insert_ammo),
                    _ => {}
                }
            }
        }

        // Collect secondary battery maxDist and ammo from _Hull upgrades
        if uc_type == keys::UC_TYPE_HULL
            && let Some(atba_comp) = components.inner().get(&pk(keys::COMP_ATBA)).and_then(read_first_string)
            && let Some(atba_data) = ship_data.get(&pk(&atba_comp)).and_then(|v| v.dict_or_object_dict())
        {
            if let Some(m) = read_float(&atba_data.inner(), keys::MAX_DIST).map(Meters::from) {
                max_secondary_battery_m = Some(match max_secondary_battery_m {
                    Some(prev) if prev.value() >= m.value() => prev,
                    _ => m,
                });
            }
            let capture_per_gun = secondary_guns_raw.is_empty();
            for (mount_key, mount_val) in atba_data.inner().iter() {
                let Some(mount_dict) = mount_val.dict_or_object_dict() else {
                    continue;
                };
                let mount_inner = mount_dict.inner();
                let ammo_val = mount_inner.get(&pk(keys::AMMO_LIST));

                let mut insert_ammo = |item: &Value| {
                    if let Some(name) = item.string_ref() {
                        secondary_battery_ammo.insert(name.inner().clone());
                    }
                };
                match ammo_val {
                    Some(Value::Tuple(t)) => t.inner().iter().for_each(&mut insert_ammo),
                    Some(Value::List(l)) => l.inner().iter().for_each(&mut insert_ammo),
                    _ => {}
                }

                // Every HP_ mount takes a slot so wire gun indices stay aligned.
                if capture_per_gun
                    && let Some(key_str) = mount_key.string_ref().map(|s| s.inner().clone())
                    && key_str.starts_with(keys::HP_PREFIX)
                {
                    let first_ammo = match ammo_val {
                        Some(Value::Tuple(t)) => {
                            t.inner().iter().find_map(|i| i.string_ref().map(|s| s.inner().clone()))
                        }
                        Some(Value::List(l)) => {
                            l.inner().iter().find_map(|i| i.string_ref().map(|s| s.inner().clone()))
                        }
                        _ => None,
                    };
                    secondary_guns_raw.push((key_str, first_ammo));
                }
            }
        }

        // Collect torpedo ammo from _Torpedoes upgrades
        if uc_type != keys::UC_TYPE_TORPEDOES {
            continue;
        }
        let Some(components) = upgrade_dict.get(&pk(keys::COMPONENTS)).and_then(|v| v.dict_or_object_dict()) else {
            continue;
        };
        // Get the torpedo component name(s) from this upgrade
        let Some(torp_comp) = components.inner().get(&pk(keys::COMP_TORPEDOES)).and_then(read_first_string) else {
            continue;
        };
        // Look up that component in the ship data and extract ammo from launchers
        let Some(torp_data) = ship_data.get(&pk(&torp_comp)).and_then(|v| v.dict_or_object_dict()) else {
            continue;
        };
        for (_key, val) in torp_data.inner().iter() {
            let Some(launcher) = val.dict_or_object_dict() else {
                continue;
            };
            let launcher_inner = launcher.inner();
            let Some(ammo_val) = launcher_inner.get(&pk(keys::AMMO_LIST)) else {
                continue;
            };
            // ammoList can be either a Tuple or List in pickled data
            let mut insert_ammo = |item: &Value| {
                if let Some(name) = item.string_ref() {
                    torpedo_ammo.insert(name.inner().clone());
                }
            };
            match ammo_val {
                Value::Tuple(t) => t.inner().iter().for_each(&mut insert_ammo),
                Value::List(l) => {
                    let items = l.inner();
                    items.iter().for_each(&mut insert_ammo);
                }
                _ => continue,
            };
        }
    }

    // Resolve the first hull component name from hull upgrades
    // (e.g. "B_Hull" for Shimakaze). Falls back to "A_Hull" if no hull upgrades exist.
    let first_hull_comp_name: Option<String> = hull_upgrades
        .iter()
        .min_by_key(|(k, _)| (*k).clone())
        .and_then(|(_, config)| config.component_names.get(&keys::ComponentType::Hull).cloned());
    let hull_comp_key = first_hull_comp_name.as_deref().unwrap_or(keys::A_HULL);

    // Mounts without a trailing number sort last.
    secondary_guns_raw.sort_by_key(|(k, _)| {
        let number = hardpoint_number(k);
        (number.is_none(), number)
    });
    let secondary_guns: Vec<Option<String>> = secondary_guns_raw.into_iter().map(|(_, name)| name).collect();

    let config_data = if hull_upgrades.is_empty()
        && torpedo_ammo.is_empty()
        && main_battery_ammo.is_empty()
        && secondary_battery_ammo.is_empty()
        && max_main_battery_m.is_none()
        && max_secondary_battery_m.is_none()
    {
        None
    } else {
        Some(crate::game_params::types::ShipConfigData {
            hull_upgrades,
            main_battery_m: max_main_battery_m,
            secondary_battery_m: max_secondary_battery_m,
            torpedo_ammo,
            main_battery_ammo,
            secondary_battery_ammo,
            secondary_guns,
        })
    };

    // Extract model path, armor map, and hit location groups from the first hull component.
    let a_hull = ship_data.get(&pk(hull_comp_key)).and_then(|v| v.dict_or_object_dict());

    let model_path: Option<String> = a_hull.as_ref().and_then(|hull_dict| read_string(&hull_dict.inner(), keys::MODEL));

    let armor: Option<ArmorMap> = a_hull.as_ref().and_then(|hull_dict| {
        hull_dict
            .inner()
            .get(&pk(keys::ARMOR))
            .and_then(|v| v.dict_or_object_dict())
            .map(|d| parse_armor_dict(&d.inner()))
    });

    // Hit location zones are stored as top-level entries in the hull component (e.g. Bow, Cit, SS).
    // Each zone is a dict with an `hlType` field. We scan all entries and filter by that.
    let hit_locations: Option<HashMap<String, HitLocation>> = a_hull.as_ref().map(|hull_dict| {
        hull_dict
            .inner()
            .iter()
            .filter_map(|(k, v)| {
                let name = k.string_ref()?.inner().to_string();
                let group_shared = v.dict_or_object_dict()?;
                let group_dict = group_shared.inner();
                // Only consider entries with hlType — that marks them as hit location zones.
                let hl_type = read_string(&group_dict, keys::HL_TYPE)?;
                let max_hp = read_float(&group_dict, keys::MAX_HP);
                let regenerated_hp_part = read_float(&group_dict, keys::REGENERATED_HP_PART);
                let thickness = read_float(&group_dict, keys::THICKNESS);
                let splash_boxes: Vec<String> = group_dict
                    .get(&pk(keys::SPLASH_BOXES))
                    .map(|v| {
                        let extract = |items: &[Value]| -> Vec<String> {
                            items.iter().filter_map(|s| s.string_ref().map(|s| s.inner().to_string())).collect()
                        };
                        if let Some(list) = v.list_ref() {
                            extract(&list.inner())
                        } else if let Some(tuple) = v.tuple_ref() {
                            extract(tuple.inner())
                        } else {
                            Vec::new()
                        }
                    })
                    .unwrap_or_default();
                Some((
                    name,
                    HitLocation::builder()
                        .maybe_max_hp(max_hp)
                        .hl_type(hl_type)
                        .maybe_regenerated_hp_part(regenerated_hp_part)
                        .maybe_thickness(thickness)
                        .splash_boxes(splash_boxes)
                        .build(),
                ))
            })
            .collect()
    });

    let permoflages: Vec<String> = ship_data
        .get(&pk(keys::PERMOFLAGES))
        .map(|val| {
            // permoflages can be either a list or a tuple in the pickle data.
            let extract = |items: &[Value]| -> Vec<String> {
                items.iter().filter_map(|item| item.string_ref().map(|s| s.inner().to_string())).collect()
            };
            if let Some(list) = val.list_ref() {
                extract(&list.inner())
            } else if let Some(tuple) = val.tuple_ref() {
                extract(tuple.inner())
            } else {
                Vec::new()
            }
        })
        .unwrap_or_default();

    let camera_trajectories = read_camera_trajectories(ship_data);

    Vehicle::builder()
        .level(level)
        .group(group)
        .maybe_abilities(abilities)
        .upgrades(upgrades)
        .maybe_config_data(config_data)
        .maybe_model_path(model_path)
        .maybe_armor(armor)
        .maybe_hit_locations(hit_locations)
        .permoflages(permoflages)
        .camera_trajectories(camera_trajectories)
        .maybe_ttx_components((!ttx_components.is_empty()).then_some(ttx_components))
        .innate_skills(innate_skills)
        .research(research)
        .maybe_origin_ship(origin_ship)
        .maybe_peculiarity(peculiarity)
        .build()
}

impl GameMetadataProvider {
    /// Shared handle to the entity specs, for callers that need to hold them
    /// independently of the provider's lifetime.
    pub fn entity_specs_arc(&self) -> Arc<Vec<EntitySpec>> {
        Arc::clone(&self.specs)
    }

    /// Loads game metadata directly from game files. This operation is fairly expensive
    /// considering `GameParams.data` must be deserialized and converted to a strongly-typed
    /// representation.
    ///
    /// See [`GameMetadataProvider::from_params`] if you wish to use caching.
    #[cfg(feature = "vfs")]
    pub fn from_vfs(vfs: &vfs::VfsPath) -> Result<GameMetadataProvider, GameDataError> {
        debug!("deserializing gameparams");

        let mut game_params_data = Vec::new();
        vfs.join("content/GameParams.data")?.open_file()?.read_to_end(&mut game_params_data)?;

        let params = Self::params_from_data(game_params_data)?;

        Self::from_params_with_vfs(params, vfs)
    }

    /// Decode a raw `GameParams.data` blob (zlib-compressed pickle) into the
    /// parsed `Param` list, without needing a VFS. This is the shared core of
    /// `from_vfs`: it runs `game_params_to_pickle`, unwraps the params dict
    /// across the modern/old/list/tuple root shapes, and parses each entry via
    /// `parse_single_param`. The resulting `Vec<Param>` is exactly what callers
    /// serialize to the min CBOR consumed by downstream tools.
    pub fn params_from_data(game_params_data: Vec<u8>) -> Result<Vec<Param>, GameDataError> {
        let pickled_params: Value = game_params_to_pickle(game_params_data)?;

        let params_dict = base_params_dict(&pickled_params)?;
        let modifier_defaults = modifier_defaults(&params_dict.inner());

        let new_params = params_dict
            .inner()
            .values()
            .filter_map(|param| {
                // Wrap each param in catch_unwind so missing fields in old game
                // versions skip the individual param rather than aborting everything.
                let param = param.clone();
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    Self::parse_single_param(&param, &params_dict, modifier_defaults.as_ref())
                }))
                .unwrap_or_default()
            })
            .collect::<Vec<Param>>();

        Ok(new_params)
    }

    /// Parse a single param from pickled dict data. Panics on missing fields are
    /// caught by the caller's `catch_unwind`. `modifier_defaults` is the block
    /// of values every `Modifier` param starts from.
    fn parse_single_param(
        param: &Value,
        params_dict: &Shared<pickled::Dict>,
        modifier_defaults: Option<&pickled::Dict>,
    ) -> Option<Param> {
        if param.is_none() {
            return None;
        }

        let param_data = param.dict_or_object_dict().expect("Params root level dictionary values are not dictionaries");
        let param_data = param_data.inner();

        let (nation, species, typ) = param_data.get(&pk(keys::TYPEINFO)).and_then(|type_info| {
            type_info.dict_or_object_dict().and_then(|type_info_main| {
                let type_info = type_info_main.inner();
                let (nation, species, ty) = (
                    type_info.get(&pk(keys::TYPEINFO_NATION))?,
                    type_info.get(&pk(keys::TYPEINFO_SPECIES))?,
                    type_info.get(&pk(keys::TYPEINFO_TYPE))?,
                );

                let (Value::String(nation), Value::String(ty)) = (nation, ty) else {
                    return None;
                };

                Some((nation.clone(), species.clone(), ty.clone()))
            })
        })?;

        let param_type = ParamType::from_name(typ.inner().as_str())?;
        let nation = nation.inner().clone();
        let species = species.string_ref().map(|s| Species::from_name(s.inner().as_str()));

        let parsed_param_data = match param_type {
            ParamType::Ship => Some(ParamData::Vehicle(build_ship(&param_data))),
            ParamType::Crew => {
                let money_training_level = game_param_to_type!(param_data, "moneyTrainingLevel", usize);

                let personality = game_param_to_type!(param_data, "CrewPersonality", HashMap<(), ()>);
                let personality = personality.inner();
                let crew_personality = build_crew_personality(&personality);

                let skills = game_param_to_type!(param_data, "Skills", Option<HashMap<(), ()>>);
                // Owned per crew here; `share_crew_skills` collapses the
                // duplicates once every param is built.
                let skills = skills.map(|skills| crate::Rc::from(build_crew_skills(&skills.inner())));
                // Crews without the key have no talents.
                let talents = game_param_to_type!(param_data, "UniqueSkills", Option<HashMap<(), ()>>)
                    .map(|unique_skills| build_crew_talents(&unique_skills.inner()))
                    .unwrap_or_default();

                Some(ParamData::Crew(
                    Crew::builder()
                        .money_training_level(money_training_level)
                        .personality(crew_personality)
                        .maybe_skills(skills)
                        .talents(talents)
                        .build(),
                ))
            }
            ParamType::Achievement => {
                let is_group = game_param_to_type!(param_data, "group", Option<bool>).unwrap_or_default();
                let one_per_battle = game_param_to_type!(param_data, "onePerBattle", Option<bool>).unwrap_or_default();
                let ui_type = game_param_to_type!(param_data, "uiType", Option<String>);
                let ui_name = game_param_to_type!(param_data, "uiName", Option<String>);

                Some(ParamData::Achievement(
                    Achievement::builder()
                        .is_group(is_group)
                        .one_per_battle(one_per_battle)
                        .maybe_ui_type(ui_type)
                        .maybe_ui_name(ui_name)
                        .build(),
                ))
            }
            ParamType::Ability => Some(ParamData::Ability(build_ability(&param_data))),
            ParamType::Exterior => {
                let camouflage = param_data
                    .get(&pk(keys::CAMOUFLAGE))
                    .and_then(|v| v.string_ref())
                    .map(|s| s.inner().to_string())
                    .filter(|s| !s.is_empty());
                let title = param_data
                    .get(&pk(keys::TITLE))
                    .and_then(|v| v.string_ref())
                    .map(|s| s.inner().to_string())
                    .filter(|s| !s.is_empty());
                let modifiers = param_data
                    .get(&pk("modifiers"))
                    .and_then(|v| v.dict_or_object_dict())
                    .map(|d| build_skill_modifiers(&d.inner()))
                    .unwrap_or_default();
                // Absent `isTileflage` means a non-tile exterior; `false` is the correct default.
                let is_tileflage =
                    game_param_to_type!(param_data, keys::IS_TILEFLAGE, Option<bool>).unwrap_or_default();
                let unpeculiar_camouflage = param_data
                    .get(&pk(keys::UNPECULIAR_CAMOUFLAGE))
                    .and_then(|v| v.string_ref())
                    .map(|s| s.inner().to_string())
                    .filter(|s| !s.is_empty());
                let hidden = game_param_to_type!(param_data, keys::HIDDEN, Option<bool>).unwrap_or_default();
                Some(ParamData::Exterior(
                    Exterior::builder()
                        .maybe_camouflage(camouflage)
                        .maybe_title(title)
                        .modifiers(modifiers)
                        .is_tileflage(is_tileflage)
                        .maybe_unpeculiar_camouflage(unpeculiar_camouflage)
                        .hidden(hidden)
                        .build(),
                ))
            }
            ParamType::Modernization => {
                let modifiers = param_data
                    .get(&pk("modifiers"))
                    .and_then(|v| v.dict_or_object_dict())
                    .map(|d| build_skill_modifiers(&d.inner()))
                    .unwrap_or_default();
                let slot = param_data
                    .get(&pk("slot"))
                    .and_then(|v| v.i64_ref())
                    .and_then(|&v| if v >= 0 { Some(v as u8) } else { None });
                let ship_levels = param_data
                    .get(&pk("shiplevel"))
                    .and_then(|v| v.list_ref())
                    .map(|a| a.inner().iter().filter_map(|x| x.i64_ref().map(|&n| n as u32)).collect())
                    .unwrap_or_default();
                let ship_types = string_list_field(&param_data, "shiptype");
                let nations = string_list_field(&param_data, "nation");
                let groups = string_list_field(&param_data, "group");
                let ships = string_list_field(&param_data, "ships");
                let excludes = string_list_field(&param_data, "excludes");
                let kind = param_data
                    .get(&pk("type"))
                    .and_then(|v| v.i64_ref())
                    .map(|&raw| super::types::ModernizationKind::from_raw(raw));
                let cost_credits = read_price(&param_data, "costCR").map(super::types::Credits::new);
                Some(ParamData::Modernization(super::types::Modernization::new(
                    modifiers,
                    slot,
                    ship_levels,
                    ship_types,
                    nations,
                    groups,
                    ships,
                    excludes,
                    kind,
                    cost_credits,
                )))
            }
            ParamType::Unit => {
                let uc_type = param_data
                    .get(&pk(keys::UC_TYPE))
                    .and_then(|v| v.string_ref())
                    .map(|s| s.inner().to_string())
                    .filter(|s| !s.is_empty());
                Some(ParamData::Unit(super::types::Unit::new(uc_type)))
            }
            ParamType::Drop => Some(ParamData::Drop(build_buff_drop(&param_data))),
            ParamType::Aircraft => {
                let subtypes: Vec<String> = param_data
                    .get(&pk("planeSubtype"))
                    .and_then(|v| v.list_ref())
                    .map(|list| {
                        list.inner()
                            .iter()
                            .filter_map(|item| item.string_ref().map(|s| s.inner().to_string()))
                            .collect()
                    })
                    .unwrap_or_default();
                let category = if subtypes.iter().any(|s| s == "airsupport") {
                    PlaneCategory::Airsupport
                } else if subtypes.iter().any(|s| s == "consumable") {
                    PlaneCategory::Consumable
                } else {
                    PlaneCategory::Controllable
                };
                let ordnance = param_data
                    .get(&pk("bombName"))
                    .and_then(|v| v.string_ref())
                    .map(|s| s.inner().to_string())
                    .filter(|name| !name.is_empty());
                // Resolve ammo type: bombName -> projectile dict -> ammoType
                let ammo_type = ordnance
                    .as_deref()
                    .and_then(|bomb_name| {
                        params_dict
                            .inner()
                            .get(&HashableValue::String(bomb_name.to_owned().into()))
                            .and_then(|proj| proj.dict_or_object_dict())
                            .and_then(|proj_dict| {
                                proj_dict
                                    .inner()
                                    .get(&pk("ammoType"))
                                    .and_then(|v| v.string_ref())
                                    .map(|s| s.inner().to_string())
                            })
                    })
                    .filter(|ammo_type| !ammo_type.is_empty());
                let read_opt_f32 = |key: &str| -> Option<f32> {
                    param_data
                        .get(&pk(key))
                        .and_then(|v| v.f64_ref().map(|f| *f as f32).or_else(|| v.i64_ref().map(|i| *i as f32)))
                };
                let read_count = |key: &str| -> Option<u32> {
                    param_data.get(&pk(key)).and_then(|v| v.i64_ref()).and_then(|count| u32::try_from(*count).ok())
                };
                let hangar_restore =
                    param_data.get(&pk("hangarSettings")).and_then(|v| v.dict_or_object_dict()).and_then(|hangar| {
                        let hangar = hangar.inner();
                        let planes = hangar.get(&pk("restoreAmount")).and_then(|v| v.i64_ref())?;
                        let time = hangar
                            .get(&pk("timeToRestore"))
                            .and_then(|v| v.f64_ref().map(|f| *f as f32).or_else(|| v.i64_ref().map(|i| *i as f32)))?;
                        Some(crate::game_params::types::HangarRestore {
                            planes: u32::try_from(*planes).ok()?,
                            time: crate::game_params::types::Seconds::new(time),
                        })
                    });
                // Planes without the key carry no consumables.
                let abilities = param_data
                    .get(&pk("PlaneAbilities"))
                    .and_then(|v| v.dict_or_object_dict())
                    .map(|slots| build_ability_slots(&slots.inner()))
                    .unwrap_or_default();
                Some(ParamData::Aircraft(
                    Aircraft::builder()
                        .category(category)
                        .maybe_ammo_type(ammo_type)
                        .subtypes(subtypes)
                        .maybe_squadron_size(read_count("numPlanesInSquadron"))
                        .maybe_attack_flight_size(read_count("attackerSize"))
                        .maybe_attack_count(read_count("attackCount"))
                        .maybe_projectiles_per_attack(read_count("projectilesPerAttack"))
                        .maybe_ordnance(ordnance)
                        // GameParams writes angleOfDive 0 for planes that do not dive.
                        .maybe_angle_of_dive(
                            read_opt_f32("angleOfDive").filter(|angle| *angle != 0.0).map(Degrees::from),
                        )
                        .maybe_speed_with_ordnance(
                            read_opt_f32("speedMoveWithBomb").map(crate::game_params::ttx::model::Knots::from),
                        )
                        .maybe_attack_cooldown(
                            read_opt_f32("attackCooldown").map(crate::game_params::types::Seconds::new),
                        )
                        .maybe_hangar_restore(hangar_restore)
                        .abilities(abilities)
                        .build(),
                ))
            }
            ParamType::Projectile => {
                let ammo_type = param_data
                    .get(&pk("ammoType"))
                    .and_then(|v| v.string_ref())
                    .map(|s| s.inner().to_string())
                    .filter(|ammo_type| !ammo_type.is_empty());
                let max_dist = param_data
                    .get(&pk(keys::MAX_DIST))
                    .and_then(|v| v.f64_ref().map(|f| *f as f32).or_else(|| v.i64_ref().map(|i| *i as f32)))
                    .map(BigWorldDistance::from);
                let read_opt_f32 = |key: &str| -> Option<f32> {
                    param_data
                        .get(&pk(key))
                        .and_then(|v| v.f64_ref().map(|f| *f as f32).or_else(|| v.i64_ref().map(|i| *i as f32)))
                };
                let read_opt_bool =
                    |key: &str| -> Option<bool> { param_data.get(&pk(key)).and_then(|v| v.bool_ref().copied()) };
                let bullet_diametr = read_opt_f32("bulletDiametr");
                let bullet_mass = read_opt_f32("bulletMass");
                let bullet_speed = read_opt_f32("bulletSpeed");
                let bullet_krupp = read_opt_f32("bulletKrupp");
                let bullet_cap = read_opt_bool("bulletCap");
                let bullet_cap_normalize_max_angle = read_opt_f32("bulletCapNormalizeMaxAngle");
                let bullet_detonator = read_opt_f32("bulletDetonator");
                let bullet_detonator_threshold = read_opt_f32("bulletDetonatorThreshold");
                let bullet_ricochet_at = read_opt_f32("bulletRicochetAt");
                let bullet_always_ricochet_at = read_opt_f32("bulletAlwaysRicochetAt");
                let alpha_piercing_he = read_opt_f32("alphaPiercingHE");
                let alpha_piercing_cs = read_opt_f32("alphaPiercingCS");
                let alpha_damage = read_opt_f32("alphaDamage");
                let burn_prob = read_opt_f32("burnProb");
                let uw_critical = read_opt_f32("uwCritical");
                let time_factor = read_opt_f32("timeFactor");
                let bullet_air_drag = read_opt_f32("bulletAirDrag");
                let speed = read_opt_f32("speed");
                let damage = read_opt_f32("damage");
                let visibility_factor = read_opt_f32("visibilityFactor");
                let torpedo_type = param_data.get(&pk("torpedoType")).and_then(|v| v.i64_ref().copied());
                // Pairs are pickled as lists or tuples.
                let read_pairs = |key: &str| -> Option<Vec<(f32, f32)>> {
                    param_data.get(&pk(key)).map(|value| {
                        read_sequence(value)
                            .iter()
                            .filter_map(|item| {
                                let pair = read_sequence(item);
                                let dist = value_f32(pair.first()?)?;
                                let coeff = value_f32(pair.get(1)?)?;
                                Some((dist, coeff))
                            })
                            .collect()
                    })
                };
                let distance_of_damage = read_pairs("distanceOfDamage");
                let ignore_classes = param_data.get(&pk("ignoreClasses")).and_then(|v| v.list_ref()).map(|list| {
                    list.inner()
                        .iter()
                        .filter_map(|item| item.string_ref().map(|s| s.inner().to_string()))
                        .collect::<Vec<_>>()
                });
                Some(ParamData::Projectile(
                    Projectile::builder()
                        .maybe_ammo_type(ammo_type)
                        .maybe_max_dist(max_dist)
                        .maybe_bullet_diametr(bullet_diametr)
                        .maybe_bullet_mass(bullet_mass)
                        .maybe_bullet_speed(bullet_speed)
                        .maybe_bullet_krupp(bullet_krupp)
                        .maybe_bullet_cap(bullet_cap)
                        .maybe_bullet_cap_normalize_max_angle(bullet_cap_normalize_max_angle)
                        .maybe_bullet_detonator(bullet_detonator)
                        .maybe_bullet_detonator_threshold(bullet_detonator_threshold)
                        .maybe_depth_splash_radius(read_opt_f32("depthSplashRadius"))
                        .maybe_bullet_ricochet_at(bullet_ricochet_at)
                        .maybe_bullet_always_ricochet_at(bullet_always_ricochet_at)
                        .maybe_alpha_piercing_he(alpha_piercing_he)
                        .maybe_alpha_piercing_cs(alpha_piercing_cs)
                        .maybe_alpha_damage(alpha_damage)
                        .maybe_burn_prob(burn_prob)
                        .maybe_uw_critical(uw_critical)
                        .maybe_time_factor(time_factor)
                        .maybe_bullet_air_drag(bullet_air_drag)
                        .maybe_speed(speed)
                        .maybe_damage(damage)
                        .maybe_visibility_factor(visibility_factor)
                        .maybe_distance_of_damage(distance_of_damage)
                        .maybe_torpedo_type(torpedo_type)
                        .maybe_maneuver_dist(read_opt_f32("maneuverDist").map(BigWorldDistance::from))
                        .maybe_arming_time(read_opt_f32("armingTime"))
                        .maybe_ignore_classes(ignore_classes)
                        .maybe_plane_ammo_type(
                            param_data
                                .get(&pk("planeAmmoType"))
                                .and_then(|v| v.string_ref())
                                .map(|s| s.inner().to_string())
                                .filter(|kind| !kind.is_empty()),
                        )
                        .maybe_falling_time_coef(read_opt_f32("fallingTimeCoef"))
                        .build(),
                ))
            }
            ParamType::Building => {
                let level = game_param_to_type!(param_data, "level", u32);
                let hull = param_data.get(&pk("hull")).and_then(|v| v.dict_or_object_dict());
                let health = hull.as_ref().and_then(|h| {
                    let inner = h.inner();
                    inner
                        .get(&pk("health"))
                        .and_then(|v| v.f64_ref().map(|f| *f as f32).or_else(|| v.i64_ref().map(|i| *i as f32)))
                });
                Some(ParamData::Building(super::types::Building::builder().level(level).maybe_health(health).build()))
            }
            _ => {
                // Some params (e.g. Drops) have typeinfo.type = "Other"
                // but contain Drop-specific fields
                if param_data.contains_key(&pk("markerNameActive")) {
                    Some(ParamData::Drop(build_buff_drop(&param_data)))
                } else if species.as_ref().and_then(|species| species.known()) == Some(&Species::Modifier) {
                    Some(ParamData::Modifier(build_modifier_param(&param_data, modifier_defaults)))
                } else {
                    None
                }
            }
        }?;

        let id_value = param_data.get(&pk(keys::PARAM_ID)).expect("param has no id field");
        let id = GameParamId::from(
            *id_value.i64_ref().unwrap_or_else(|| panic!("param id is not an i64, got: {id_value:?}")),
        );

        let index = param_data
            .get(&pk(keys::PARAM_INDEX))
            .expect("param has no index field")
            .string_ref()
            .expect("param index is not a string")
            .inner()
            .clone();

        let name = param_data
            .get(&pk(keys::PARAM_NAME))
            .expect("param has no name field")
            .string_ref()
            .expect("param name is not a string")
            .inner()
            .clone();

        Some(
            Param::builder()
                .id(id)
                .index(index)
                .name(name)
                .maybe_species(species)
                .nation(nation)
                .data(parsed_param_data)
                .build(),
        )
    }

    /// Constructs a GameMetadataProvider from a pre-built list of GameParams and a VFS.
    #[cfg(feature = "vfs")]
    pub fn from_params_with_vfs(params: Vec<Param>, vfs: &vfs::VfsPath) -> Result<GameMetadataProvider, GameDataError> {
        let param_id_to_translation_id =
            HashMap::from_iter(params.iter().map(|param| (param.id(), format!("IDS_{}", param.index()))));

        let data_file_loader = DataFileWithCallback::new(|path| {
            debug!("requesting file: {path}");

            let mut file_data = Vec::new();
            vfs.join(path)
                .expect("failed to join path")
                .open_file()
                .expect("failed to open file")
                .read_to_end(&mut file_data)
                .expect("failed to read file");

            Ok(Cow::Owned(file_data))
        });

        let specs = Arc::new(parse_scripts(&data_file_loader)?);

        Ok(GameMetadataProvider {
            params: params.into(),
            param_id_to_translation_id,
            translations: RwLock::new(None),
            translation_epoch: std::sync::atomic::AtomicU64::new(0),
            specs,
        })
    }

    /// Similar to [`Self::from_params`], but does not allow looking up specs. Useful for scenarios where you
    /// want to use utility functions for only game params.
    pub fn from_params_no_specs(params: Vec<Param>) -> Result<GameMetadataProvider, GameDataError> {
        let param_id_to_translation_id =
            HashMap::from_iter(params.iter().map(|param| (param.id(), format!("IDS_{}", param.index()))));

        let specs = Arc::new(Vec::new());

        Ok(GameMetadataProvider {
            params: params.into(),
            param_id_to_translation_id,
            translations: RwLock::new(None),
            translation_epoch: std::sync::atomic::AtomicU64::new(0),
            specs,
        })
    }

    pub fn set_translations(&self, catalog: Catalog) {
        *self.translations.write().expect("translations lock poisoned") = Some(catalog);
        self.translation_epoch.fetch_add(1, std::sync::atomic::Ordering::Release);
    }

    /// The number of catalog swaps so far. Two equal values around a cached
    /// read mean the localized strings behind it did not change.
    pub fn translation_epoch(&self) -> u64 {
        self.translation_epoch.load(std::sync::atomic::Ordering::Acquire)
    }

    /// Look up a translation key in the catalog, returning `None` when the
    /// catalog is absent or gettext returns the key unchanged (= not found).
    fn translate(&self, key: &str) -> Option<String> {
        let guard = self.translations.read().ok()?;
        let catalog = guard.as_ref()?;
        let result = catalog.gettext(key);
        if result == key { None } else { Some(result.to_string()) }
    }

    /// The plural form of a translation key for `count`. Plural entries in
    /// the client catalog use the key as both the singular and plural id.
    /// `None` when the catalog is absent or lacks the key.
    pub fn localized_plural_from_id(&self, key: &TranslationKey, count: u64) -> Option<String> {
        let guard = self.translations.read().ok()?;
        let catalog = guard.as_ref()?;
        let result = catalog.ngettext(key.as_str(), key.as_str(), count);
        if result == key.as_str() { None } else { Some(result.to_string()) }
    }

    pub fn param_localization_id(&self, ship_id: GameParamId) -> Option<&str> {
        self.param_id_to_translation_id.get(&ship_id).map(|s| s.as_str())
    }

    // pub fn get(&self, path: &str) -> Option<&pickled::Value> {
    //     let path_parts = path.split("/");
    //     let mut current = Some(&self.0);
    //     while let Some(pickled::Value::Dict(dict)) = current {

    //     }
    //     None
    // }

    /// Build a `ShellInfo` from a projectile's `GameParamId`.
    ///
    /// Resolves the param, extracts the projectile data, and converts it to
    /// the flattened `ShellInfo` representation. Returns `None` if the param
    /// doesn't exist, isn't a projectile, or lacks the fields `ShellInfo`
    /// requires.
    pub fn resolve_shell_from_param_id(&self, params_id: GameParamId) -> Option<ShellInfo> {
        let param = GameParamProvider::game_param_by_id(self, params_id)?;
        let projectile = param.projectile()?;
        let name = param.name().to_string();
        projectile.to_shell_info(name)
    }
}

#[cfg(test)]
mod camera_tests {
    use super::*;
    use pickled::value::Shared;

    fn fv(f: f64) -> Value {
        Value::F64(f)
    }

    fn list(items: Vec<Value>) -> Value {
        Value::List(Shared::new(items))
    }

    fn tuple(items: Vec<Value>) -> Value {
        Value::Tuple(pickled::value::SharedFrozen::new(items))
    }

    fn dict(entries: Vec<(HashableValue, Value)>) -> Value {
        Value::Dict(Shared::new(entries.into_iter().collect()))
    }

    fn iv(i: i64) -> Value {
        Value::I64(i)
    }

    fn typeinfo(nation: &str, species: &str, ty: &str) -> Value {
        dict(vec![(pk("nation"), sv(nation)), (pk("species"), sv(species)), (pk("type"), sv(ty))])
    }

    #[test]
    fn parse_single_param_retains_torpedo_fields() {
        // Real PAPT027_Mk_16_mod_1 (Gearing Mk16) values, plus a synthetic
        // distanceOfDamage/ignoreClasses to exercise the array readers.
        let proj = dict(vec![
            (pk("id"), iv(4242)),
            (pk("index"), sv("PAPT027")),
            (pk("name"), sv("PAPT027_Mk_16_mod_1")),
            (pk("typeinfo"), typeinfo("USA", "Torpedo", "Projectile")),
            (pk("ammoType"), sv("torpedo")),
            (pk("maxDist"), fv(350.0)),
            (pk("alphaDamage"), fv(53500.0)),
            (pk("damage"), fv(1200.0)),
            (pk("speed"), fv(66.0)),
            (pk("visibilityFactor"), fv(1.4)),
            (pk("torpedoType"), iv(1)),
            (pk("distanceOfDamage"), list(vec![list(vec![fv(83.33), fv(0.1)]), list(vec![fv(86.66), fv(1.0)])])),
            (pk("ignoreClasses"), list(vec![sv("Cruiser"), sv("Destroyer")])),
            (pk("maneuverDist"), fv(20.0)),
            (pk("armingTime"), iv(1)),
        ]);
        let params_dict: Shared<pickled::Dict> = Shared::new(pickled::Dict::new());
        let param = GameMetadataProvider::parse_single_param(&proj, &params_dict, None).expect("param parsed");
        let projectile = param.projectile().expect("projectile data");

        assert_eq!(projectile.alpha_damage(), Some(53500.0));
        assert_eq!(projectile.max_dist().map(|d| d.value()), Some(350.0));
        assert_eq!(projectile.speed(), Some(66.0));
        assert_eq!(projectile.damage(), Some(1200.0));
        assert_eq!(projectile.visibility_factor(), Some(1.4));
        assert_eq!(projectile.torpedo_type(), Some(1));
        assert_eq!(projectile.distance_of_damage(), Some(&[(83.33, 0.1), (86.66, 1.0)][..]));
        assert_eq!(projectile.ignore_classes(), Some(&["Cruiser".to_string(), "Destroyer".to_string()][..]));
        assert_eq!(projectile.maneuver_dist().map(|d| d.value()), Some(20.0));
        assert_eq!(projectile.arming_time(), Some(1.0));
    }

    #[test]
    fn parse_single_param_torpedo_fields_absent_are_none() {
        let proj = dict(vec![
            (pk("id"), iv(7)),
            (pk("index"), sv("PAPT099")),
            (pk("name"), sv("PAPT099_NoTorp")),
            (pk("typeinfo"), typeinfo("USA", "Torpedo", "Projectile")),
            (pk("ammoType"), sv("torpedo")),
        ]);
        let params_dict: Shared<pickled::Dict> = Shared::new(pickled::Dict::new());
        let param = GameMetadataProvider::parse_single_param(&proj, &params_dict, None).expect("param parsed");
        let projectile = param.projectile().expect("projectile data");
        assert_eq!(projectile.speed(), None);
        assert_eq!(projectile.damage(), None);
        assert_eq!(projectile.visibility_factor(), None);
        assert_eq!(projectile.torpedo_type(), None);
        assert_eq!(projectile.distance_of_damage(), None);
        assert_eq!(projectile.ignore_classes(), None);
    }

    #[test]
    fn read_camera_trajectories_empty_dict_returns_empty() {
        let ship_data: pickled::Dict = pickled::Dict::new();
        let result = read_camera_trajectories(&ship_data);
        assert!(result.is_empty());
    }

    fn sv(s: &str) -> Value {
        Value::String(s.to_string().into())
    }

    fn build_inner_traj(extra: Vec<(HashableValue, Value)>) -> Value {
        let pos_center = list(vec![list(vec![fv(0.0), fv(1.958), fv(0.0)]), list(vec![fv(0.0), fv(2.008), fv(0.0)])]);
        let mut entries = vec![
            (pk("posCenter"), pos_center),
            (pk("semiAxisH"), list(vec![fv(6.552), fv(5.981)])),
            (pk("semiAxisV"), list(vec![fv(4.123), fv(3.999)])),
        ];
        entries.extend(extra);
        dict(entries)
    }

    #[test]
    fn read_camera_trajectories_parses_mode() {
        let inner_traj = build_inner_traj(vec![]);
        let mode = dict(vec![(pk("InnerTrajectory"), inner_traj)]);
        let cameras = dict(vec![(pk("TestMode"), mode)]);
        let ship_data: pickled::Dict = [(pk("Cameras"), cameras)].into_iter().collect();
        let result = read_camera_trajectories(&ship_data);
        assert_eq!(result.len(), 1);
        let (name, traj) = &result[0];
        assert_eq!(name, "TestMode");
        assert!((traj.pos_center[0].y - 1.958_f32).abs() < 1e-4);
        assert!((traj.pos_center[1].y - 2.008_f32).abs() < 1e-4);
        assert!((traj.semi_axes[0].x - 6.552_f32).abs() < 1e-4);
        assert!((traj.semi_axes[1].x - 5.981_f32).abs() < 1e-4);
        assert!((traj.semi_axes[0].y - 4.123_f32).abs() < 1e-4);
        assert!((traj.semi_axes[1].y - 3.999_f32).abs() < 1e-4);
        // defaults when tags/ignoreHeightMultiplier are absent
        assert_eq!(traj.tags, "");
        assert!(!traj.ignore_height_multiplier);
    }

    #[test]
    fn read_camera_trajectories_parses_tags_and_ignore_height_bool() {
        let inner_traj = build_inner_traj(vec![(pk("ignoreHeightMultiplier"), Value::Bool(true))]);
        let mode = dict(vec![(pk("InnerTrajectory"), inner_traj), (pk("tags"), sv("AT"))]);
        let cameras = dict(vec![(pk("TagMode"), mode)]);
        let ship_data: pickled::Dict = [(pk("Cameras"), cameras)].into_iter().collect();
        let result = read_camera_trajectories(&ship_data);
        assert_eq!(result.len(), 1);
        let (name, traj) = &result[0];
        assert_eq!(name, "TagMode");
        assert_eq!(traj.tags, "AT");
        assert!(traj.ignore_height_multiplier);
    }

    #[test]
    fn read_camera_trajectories_parses_ignore_height_as_int() {
        let inner_traj = build_inner_traj(vec![(pk("ignoreHeightMultiplier"), Value::I64(1))]);
        let mode = dict(vec![(pk("InnerTrajectory"), inner_traj)]);
        let cameras = dict(vec![(pk("IntMode"), mode)]);
        let ship_data: pickled::Dict = [(pk("Cameras"), cameras)].into_iter().collect();
        let result = read_camera_trajectories(&ship_data);
        assert_eq!(result.len(), 1);
        assert!(result[0].1.ignore_height_multiplier);
    }

    fn build_outer_traj() -> Value {
        let pos_center = list(vec![list(vec![fv(0.0), fv(4.199), fv(0.0)]), list(vec![fv(0.0), fv(6.023), fv(0.0)])]);
        dict(vec![
            (pk("posCenter"), pos_center),
            (pk("semiAxisH"), list(vec![fv(18.785), fv(17.315)])),
            (pk("semiAxisV"), list(vec![fv(12.5), fv(11.0)])),
        ])
    }

    #[test]
    fn read_camera_trajectories_parses_outer_trajectory() {
        let mode =
            dict(vec![(pk("InnerTrajectory"), build_inner_traj(vec![])), (pk("OuterTrajectory"), build_outer_traj())]);
        let cameras = dict(vec![(pk("PairMode"), mode)]);
        let ship_data: pickled::Dict = [(pk("Cameras"), cameras)].into_iter().collect();
        let result = read_camera_trajectories(&ship_data);
        assert_eq!(result.len(), 1);
        let outer = result[0].1.outer.as_ref().expect("outer parsed");
        assert!((outer.pos_center[0].y - 4.199_f32).abs() < 1e-4);
        assert!((outer.pos_center[1].y - 6.023_f32).abs() < 1e-4);
        assert!((outer.semi_axes[0].x - 18.785_f32).abs() < 1e-4);
        assert!((outer.semi_axes[0].y - 12.5_f32).abs() < 1e-4);
        assert!((outer.semi_axes[1].y - 11.0_f32).abs() < 1e-4);
    }

    #[test]
    fn read_camera_trajectories_outer_absent_is_none() {
        let mode = dict(vec![(pk("InnerTrajectory"), build_inner_traj(vec![]))]);
        let cameras = dict(vec![(pk("NoOuter"), mode)]);
        let ship_data: pickled::Dict = [(pk("Cameras"), cameras)].into_iter().collect();
        let result = read_camera_trajectories(&ship_data);
        assert!(result[0].1.outer.is_none());
    }

    #[test]
    fn read_camera_trajectories_skips_scalar_sibling_keys() {
        let pos_center = list(vec![list(vec![fv(0.0), fv(1.0), fv(0.0)]), list(vec![fv(0.0), fv(1.0), fv(0.0)])]);
        let inner_traj = dict(vec![
            (pk("posCenter"), pos_center),
            (pk("semiAxisH"), list(vec![fv(1.0), fv(1.0)])),
            (pk("semiAxisV"), list(vec![fv(1.0), fv(1.0)])),
        ]);
        let mode = dict(vec![(pk("InnerTrajectory"), inner_traj)]);
        let cameras = dict(vec![(pk("RealMode"), mode), (pk("inertialRollCoef"), Value::F64(0.5))]);
        let ship_data: pickled::Dict = [(pk("Cameras"), cameras)].into_iter().collect();
        let result = read_camera_trajectories(&ship_data);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "RealMode");
    }

    #[test]
    fn build_ability_category_collects_effect_fields_logic_wins() {
        // logic.modifiers carries displayable numeric fields and is merged last
        // (after tacticalParams), so it wins collisions (GPToParamsDict lines 70-73).
        let modifiers = dict(vec![(pk("shootShift"), fv(3.0)), (pk("workRange"), fv(5000.0))]);
        let logic = dict(vec![
            (pk("regenerationHPSpeed"), fv(0.005)),
            // collides with the top-level reloadTime; logic must win.
            (pk("reloadTime"), fv(99.0)),
            // collides with tacticalParams; tacticalParams must win.
            (pk("workRange"), fv(7.0)),
            (pk("modifiers"), modifiers),
        ]);
        let tactical = dict(vec![(pk("workRange"), fv(1000.0)), (pk("aimRange"), fv(11.0))]);
        let category_data: pickled::Dict = vec![
            (pk("reloadTime"), fv(40.0)),
            (pk("workTime"), Value::I64(28)),
            (pk("numConsumables"), Value::I64(3)),
            (pk("consumableType"), sv("regenCrew")),
            (pk("logic"), logic),
            (pk("tacticalParams"), tactical),
        ]
        .into_iter()
        .collect();

        let cat = build_ability_category(&category_data).expect("category has consumableType and numConsumables");
        let fields = cat.effect_fields();

        assert_eq!(fields.get("workTime"), Some(&28.0_f32));
        assert_eq!(fields.get("numConsumables"), Some(&3.0_f32));
        assert_eq!(fields.get("regenerationHPSpeed"), Some(&0.005_f32));
        // logic value overrides the top-level on collision.
        assert_eq!(fields.get("reloadTime"), Some(&99.0_f32));
        // tacticalParams numeric fields are merged in.
        assert_eq!(fields.get("aimRange"), Some(&11.0_f32));
        // logic.modifiers is merged last and overrides tacticalParams on collision.
        assert_eq!(fields.get("workRange"), Some(&5000.0_f32));
        // logic.modifiers numeric fields are folded in.
        assert_eq!(fields.get("shootShift"), Some(&3.0_f32));
        // non-numeric string fields are skipped.
        assert!(!fields.contains_key("consumableType"));
    }

    #[test]
    fn build_ability_category_converts_game_params_markers() {
        let category = |extra: Vec<(HashableValue, Value)>| -> pickled::Dict {
            let mut data = vec![(pk("reloadTime"), fv(60.0)), (pk("consumableType"), sv("smokeGenerator"))];
            data.extend(extra);
            data.into_iter().collect()
        };

        let unlimited = build_ability_category(&category(vec![
            (pk("numConsumables"), Value::I64(-1)),
            (pk("workTime"), fv(10.0)),
            (pk("group"), sv("squadron")),
            (pk("iconIDs"), sv("")),
        ]))
        .expect("valid category");
        assert_eq!(
            unlimited.lifecycle(),
            ConsumableLifecycle::CountBased { charges: ChargeCount::Unlimited, work_time: 10.0 }
        );
        assert_eq!(unlimited.group(), Some(ConsumableGroup::Squadron));
        assert_eq!(unlimited.icon_id(), None);
        assert_eq!(unlimited.preparation_time(), None);

        let finite = build_ability_category(&category(vec![
            (pk("numConsumables"), Value::I64(2)),
            (pk("workTime"), fv(10.0)),
            (pk("lifeCycleType"), Value::I64(0)),
        ]))
        .expect("valid category");
        assert_eq!(
            finite.lifecycle(),
            ConsumableLifecycle::CountBased { charges: ChargeCount::Finite(2), work_time: 10.0 }
        );
        assert_eq!(finite.group(), None);

        assert!(
            build_ability_category(&category(vec![(pk("numConsumables"), Value::I64(-2)), (pk("workTime"), fv(10.0))]))
                .is_none()
        );
        assert!(build_ability_category(&category(vec![(pk("numConsumables"), Value::I64(2))])).is_none());
        assert!(build_ability_category(&category(Vec::new())).is_none());
        assert!(build_ability_category(&category(vec![(pk("lifeCycleType"), Value::I64(2))])).is_none());
    }

    /// TIME_BASED categories carry neither `numConsumables` nor `workTime`, as
    /// `PCY067_ArtilleryBoosterPremium_TimeBased.PAM_DD_High` shows.
    #[test]
    fn build_ability_category_reads_time_based_pool() {
        let category = |capacity: Value| -> pickled::Dict {
            vec![
                (pk("reloadTime"), fv(5.0)),
                (pk("consumableType"), sv("artilleryBoosters")),
                (pk("lifeCycleType"), Value::I64(1)),
                (pk("maxCapacity"), capacity),
                (pk("minWorkTime"), fv(1.0)),
                (pk("capacityRegenCoeff"), fv(0.0)),
                (pk("logic"), dict(vec![(pk("boostCoeff"), fv(0.5))])),
            ]
            .into_iter()
            .collect()
        };

        let booster = build_ability_category(&category(fv(80.0))).expect("valid category");
        assert_eq!(
            booster.lifecycle(),
            ConsumableLifecycle::TimeBased {
                capacity: ConsumableCapacity::Seconds(80.0),
                min_work_time: 1.0,
                capacity_regen_coeff: 0.0,
            }
        );
        assert_eq!(booster.reload_time(), 5.0);
        assert_eq!(booster.effect_fields().get("boostCoeff"), Some(&0.5));

        let unlimited = build_ability_category(&category(Value::I64(-1))).expect("valid category");
        assert!(matches!(
            unlimited.lifecycle(),
            ConsumableLifecycle::TimeBased { capacity: ConsumableCapacity::Unlimited, .. }
        ));

        assert!(build_ability_category(&category(fv(-2.0))).is_none());
    }

    /// Build a ship dict mirroring Gearing's (`PASD013_Gearing_1945`) real shape:
    /// a `_Hull` upgrade naming an `A_Hull` component and a separate `_Engine`
    /// upgrade naming an `A_Engine` component, with the real GameParams values.
    /// `build_ship` should extract typed TTX hull/engine base stats from these.
    #[test]
    fn build_ship_extracts_ttx_hull_and_engine_base_stats() {
        let a_hull = dict(vec![
            (pk("health"), fv(19400.0)),
            (pk("maxSpeed"), fv(36.0)),
            (pk("speedCoef"), fv(1.0)),
            (pk("turningRadius"), fv(640.0)),
            (pk("rudderTime"), fv(4.25)),
            (pk("visibilityFactor"), fv(7.33)),
            // Real Gearing PASD013_Gearing_1945.A_Hull visibility coefficients (jaq-verified).
            (pk("visibilityFactorByPlane"), fv(3.41)),
            (pk("visibilityCoefFire"), fv(2.0)),
            (pk("visibilityCoefFireByPlane"), fv(2.0)),
            (pk("visibilityCoefGK"), fv(1e-6)),
            (pk("visibilityCoefGKInSmoke"), fv(2.83)),
            (
                pk("visibilityFactorsBySubmarine"),
                dict(vec![(pk("PERISCOPE"), fv(3.41)), (pk("SURFACE"), fv(0.0)), (pk("DEEP_WATER"), fv(2.0))]),
            ),
            (pk("model"), sv("A_Hull.model")),
            // Yamato-style floodNodes (list of triples) -> flood_prob (0.333-0.15)/0.333.
            (pk("floodNodes"), list(vec![list(vec![fv(0.15), fv(0.5), fv(40.0)])])),
            // SubmarineBattery sub-object (Balao-style) -> battery fields.
            (pk("SubmarineBattery"), dict(vec![(pk("capacity"), fv(240.0)), (pk("regenRate"), fv(1.2))])),
        ]);
        let a_engine = dict(vec![(pk("speedCoef"), fv(0.0))]);

        let hull_components =
            dict(vec![(pk("hull"), list(vec![sv("A_Hull")])), (pk("artillery"), list(vec![sv("A_Artillery")]))]);
        let hull_upgrade = dict(vec![(pk("ucType"), sv("_Hull")), (pk("components"), hull_components)]);

        let engine_components = dict(vec![(pk("engine"), list(vec![sv("A_Engine")]))]);
        let engine_upgrade = dict(vec![(pk("ucType"), sv("_Engine")), (pk("components"), engine_components)]);

        let upgrade_info =
            dict(vec![(pk("PAUH911_Gearing_1945"), hull_upgrade), (pk("PAUE903_D10_ENG_STOCK"), engine_upgrade)]);

        let ship_data: pickled::Dict = vec![
            (pk("level"), Value::I64(10)),
            (pk("group"), sv("special")),
            (pk("ShipUpgradeInfo"), upgrade_info),
            (pk("A_Hull"), a_hull),
            (pk("A_Engine"), a_engine),
        ]
        .into_iter()
        .collect();

        let vehicle = build_ship(&ship_data);
        let ttx = vehicle.ttx_components().expect("ttx components extracted");

        let hull = ttx.hull("PAUH911_Gearing_1945").expect("hull stats present");
        assert_eq!(hull.health, Some(crate::game_params::ttx::model::Hp::from(19400.0)));
        assert_eq!(hull.max_speed, Some(crate::game_params::ttx::model::Knots::from(36.0)));
        assert_eq!(hull.speed_coef, Some(1.0));
        assert_eq!(hull.turning_radius, Some(Meters::from(640.0)));
        assert_eq!(hull.rudder_time, Some(crate::game_params::ttx::model::Seconds::from(4.25)));
        assert_eq!(hull.visibility_factor, Some(Km::from(7.33)));
        assert_eq!(hull.visibility_factor_by_plane, Some(Km::from(3.41)));
        assert_eq!(hull.visibility_coef_fire, Some(Km::from(2.0)));
        assert_eq!(hull.visibility_coef_fire_by_plane, Some(Km::from(2.0)));
        assert_eq!(hull.visibility_coef_gk_in_smoke, Some(Km::from(2.83)));
        // visibilityFactorsBySubmarine['PERISCOPE'] (PreprocessedHull.py:13).
        assert_eq!(hull.visibility_factor_by_periscope, Some(Km::from(3.41)));
        // flood_prob derived from floodNodes[0][0]=0.15 (PreprocessedHull.py:12).
        let flood = hull.flood_prob.expect("flood_prob derived");
        assert!((flood - (0.333 - 0.15) / 0.333).abs() < 1e-6, "got {flood}");
        assert_eq!(hull.battery_capacity, Some(240.0));
        assert_eq!(hull.battery_regen_rate, Some(1.2));
        assert_eq!(
            hull.flood_nodes,
            vec![crate::game_params::ttx::components::FloodNode {
                torpedo_damage_coeff: 0.15,
                damage_per_sec: crate::game_params::ttx::model::Percent::from(0.5),
                duration: crate::game_params::ttx::model::Seconds::from(40.0),
            }]
        );

        let engine = ttx.engine("PAUE903_D10_ENG_STOCK").expect("engine stats present");
        assert_eq!(engine.speed_coef, Some(0.0));
    }

    #[test]
    fn in_smoke_visibility_at_or_below_gate_is_absent() {
        let hull =
            |range: f64| -> pickled::Dict { vec![(pk("visibilityCoefGKInSmoke"), fv(range))].into_iter().collect() };
        assert_eq!(read_visibility_in_smoke(&hull(2.83)), Some(2.83));
        assert_eq!(read_visibility_in_smoke(&hull(0.01)), None);
        assert_eq!(read_visibility_in_smoke(&hull(0.0)), None);
        assert_eq!(read_visibility_in_smoke(&pickled::Dict::new()), None);
    }

    /// Build a minimal ship dict with just an `A_Hull` component carrying
    /// `burnNodes` and `size`, mirroring Iowa's (`PASB018_Iowa_1944`) real
    /// GameParams values (jaq-verified).
    fn hull_with_burn_nodes(entries: Vec<(HashableValue, Value)>) -> pickled::Dict {
        let a_hull = dict(entries);
        let hull_components = dict(vec![(pk("hull"), list(vec![sv("A_Hull")]))]);
        let hull_upgrade = dict(vec![(pk("ucType"), sv("_Hull")), (pk("components"), hull_components)]);
        let upgrade_info = dict(vec![(pk("PAUH911_Iowa_1944"), hull_upgrade)]);

        vec![
            (pk("level"), Value::I64(10)),
            (pk("group"), sv("special")),
            (pk("ShipUpgradeInfo"), upgrade_info),
            (pk("A_Hull"), a_hull),
        ]
        .into_iter()
        .collect()
    }

    /// burnNodes is the per-hull fire-resistance stat plus the section count. Both
    /// are per-hull: surface combatants carry four nodes, most submarines one.
    #[test]
    fn hull_burn_nodes_are_extracted() {
        let ship_data = hull_with_burn_nodes(vec![(
            pk(keys::BURN_NODES),
            list(vec![
                list(vec![fv(0.6004), fv(0.3), fv(60.0)]),
                list(vec![fv(0.6004), fv(0.3), fv(60.0)]),
                list(vec![fv(0.6004), fv(0.3), fv(60.0)]),
                list(vec![fv(0.6004), fv(0.3), fv(60.0)]),
            ]),
        )]);

        let vehicle = build_ship(&ship_data);
        let ttx = vehicle.ttx_components().expect("ttx components extracted");
        let hull = ttx.hull("PAUH911_Iowa_1944").expect("hull stats present");

        assert_eq!(hull.burn_nodes.len(), 4);
        assert!((hull.burn_nodes[0].probability - 0.6004).abs() < 1e-6);
        assert!((hull.burn_nodes[0].damage_per_sec.value() - 0.3).abs() < 1e-6);
        assert!((hull.burn_nodes[0].duration.value() - 60.0).abs() < 1e-6);
    }

    /// One unreadable triple must not shrink the section count. The length of
    /// `burnNodes` is the authority on how many fire sections a hull has, and a
    /// silently shortened list would hand a four-section hull to the geometry
    /// resolver as a three-section one.
    #[test]
    fn hull_burn_nodes_are_all_or_nothing() {
        let ship_data = hull_with_burn_nodes(vec![(
            pk(keys::BURN_NODES),
            list(vec![
                list(vec![fv(0.6004), fv(0.3), fv(60.0)]),
                list(vec![fv(0.6004), fv(0.3)]),
                list(vec![fv(0.6004), fv(0.3), fv(60.0)]),
                list(vec![fv(0.6004), fv(0.3), fv(60.0)]),
            ]),
        )]);

        let vehicle = build_ship(&ship_data);
        let ttx = vehicle.ttx_components().expect("ttx components extracted");
        let hull = ttx.hull("PAUH911_Iowa_1944").expect("hull stats present");

        assert!(hull.burn_nodes.is_empty(), "got {:?}", hull.burn_nodes);
    }

    /// `burnNodes` is read from a list or a tuple, like every other nested
    /// vector-of-triples field. Older builds pickle these containers either way.
    #[test]
    fn hull_burn_nodes_read_from_a_tuple_container() {
        let triple = || tuple(vec![fv(0.6004), fv(0.3), fv(60.0)]);
        let ship_data =
            hull_with_burn_nodes(vec![(pk(keys::BURN_NODES), tuple(vec![triple(), triple(), triple(), triple()]))]);

        let vehicle = build_ship(&ship_data);
        let ttx = vehicle.ttx_components().expect("ttx components extracted");
        let hull = ttx.hull("PAUH911_Iowa_1944").expect("hull stats present");

        assert_eq!(hull.burn_nodes.len(), 4);
        assert!((hull.burn_nodes[3].duration.value() - 60.0).abs() < 1e-6);
    }

    /// A hull with no burnNodes key yields an empty list, never a fabricated
    /// four-node default. A fabricated count would later make fire sections
    /// "eligible" that do not exist on the ship.
    #[test]
    fn hull_without_burn_nodes_yields_empty() {
        let ship_data = hull_with_burn_nodes(vec![]);

        let vehicle = build_ship(&ship_data);
        let ttx = vehicle.ttx_components().expect("ttx components extracted");
        let hull = ttx.hull("PAUH911_Iowa_1944").expect("hull stats present");

        assert!(hull.burn_nodes.is_empty());
    }

    /// Build a ship dict mirroring Gearing's (`PASD013_Gearing_1945`) real
    /// `A_Torpedoes` shape: a `_Torpedoes` upgrade naming an `A_Torpedoes`
    /// component with two `HP_AGT_*` launchers (real GameParams values).
    #[test]
    fn build_ship_extracts_torpedo_launcher_base_stats() {
        let hp_agt = |barrels: f64| {
            dict(vec![
                (pk("shotDelay"), fv(103.0)),
                (pk("rotationSpeed"), list(vec![fv(25.0), fv(25.0)])),
                (pk("numBarrels"), fv(barrels)),
                (pk("ammoSwitchCoeff"), fv(0.2)),
                (pk("ammoList"), list(vec![sv("PAPT027_Mk_16_mod_1")])),
            ])
        };
        let a_torpedoes = dict(vec![(pk("HP_AGT_1"), hp_agt(5.0)), (pk("HP_AGT_2"), hp_agt(5.0))]);

        let torp_components = dict(vec![(pk("torpedoes"), list(vec![sv("A_Torpedoes")]))]);
        let torp_upgrade = dict(vec![(pk("ucType"), sv("_Torpedoes")), (pk("components"), torp_components)]);

        // Minimal hull upgrade so build_ship has a hull component.
        let hull_components = dict(vec![(pk("hull"), list(vec![sv("A_Hull")]))]);
        let hull_upgrade = dict(vec![(pk("ucType"), sv("_Hull")), (pk("components"), hull_components)]);
        let a_hull = dict(vec![(pk("health"), fv(19400.0)), (pk("model"), sv("A_Hull.model"))]);

        let upgrade_info =
            dict(vec![(pk("PAUH911_Gearing_1945"), hull_upgrade), (pk("PAUT901_D10_TORP_STOCK"), torp_upgrade)]);

        let ship_data: pickled::Dict = vec![
            (pk("level"), Value::I64(10)),
            (pk("group"), sv("special")),
            (pk("ShipUpgradeInfo"), upgrade_info),
            (pk("A_Hull"), a_hull),
            (pk("A_Torpedoes"), a_torpedoes),
        ]
        .into_iter()
        .collect();

        let vehicle = build_ship(&ship_data);
        let ttx = vehicle.ttx_components().expect("ttx components extracted");

        let launchers = ttx.torpedoes("PAUT901_D10_TORP_STOCK").expect("torpedo stats present");
        assert_eq!(launchers.len(), 2);
        let l = &launchers[0];
        assert_eq!(l.shot_delay, Some(crate::game_params::ttx::model::Seconds::from(103.0)));
        assert_eq!(l.rotation_speed, Some(crate::game_params::ttx::model::DegreesPerSecond::from(25.0)));
        assert_eq!(l.num_barrels, Some(5.0));
        assert_eq!(l.ammo_switch_coeff, Some(0.2));
        assert_eq!(l.ammo, vec!["PAPT027_Mk_16_mod_1".to_string()]);

        // A ship without torpedoes yields no torpedo entry.
        assert!(ttx.torpedoes("PAUT901_D10_TORP_STOCK").is_some());
        assert!(ttx.torpedoes("NoSuchUpgrade").is_none());
    }

    /// Orzel's (`PWSS508_Orzel`) `A_Torpedoes` grouping: bow and stern groups
    /// with two loaders each and a rotating launcher with its own loader
    /// (real values, fewer launchers).
    #[test]
    fn build_ship_extracts_torpedo_loader_groups() {
        let pair = |head: Value, ids: &[i64]| list(vec![head, list(ids.iter().map(|id| Value::I64(*id)).collect())]);
        let group = |id: i64, hardpoints: &[&str]| {
            list(vec![Value::I64(id), list(hardpoints.iter().map(|name| sv(name)).collect())])
        };
        let launcher = || dict(vec![(pk("numBarrels"), fv(1.0)), (pk("shotDelay"), fv(62.0))]);
        let a_torpedoes = dict(vec![
            (pk("HP_WGT_1"), launcher()),
            (pk("HP_WGT_2"), launcher()),
            (pk("HP_WGT_5"), launcher()),
            (pk("HP_WGT_9"), launcher()),
            (
                pk("groups"),
                list(vec![group(0, &["HP_WGT_1", "HP_WGT_2"]), group(1, &["HP_WGT_9"]), group(3, &["HP_WGT_5"])]),
            ),
            (
                pk("groupsDirection"),
                list(vec![pair(sv("forward"), &[0]), pair(sv("rear"), &[3]), pair(sv("rotating"), &[1])]),
            ),
            (
                pk("loaders"),
                list(vec![pair(Value::I64(2), &[0]), pair(Value::I64(1), &[1]), pair(Value::I64(2), &[3])]),
            ),
        ]);

        let torp_components = dict(vec![(pk("torpedoes"), list(vec![sv("A_Torpedoes")]))]);
        let torp_upgrade = dict(vec![(pk("ucType"), sv("_Torpedoes")), (pk("components"), torp_components)]);
        let hull_components = dict(vec![(pk("hull"), list(vec![sv("A_Hull")]))]);
        let hull_upgrade = dict(vec![(pk("ucType"), sv("_Hull")), (pk("components"), hull_components)]);
        let a_hull = dict(vec![(pk("health"), fv(10000.0)), (pk("model"), sv("A_Hull.model"))]);
        let upgrade_info = dict(vec![(pk("PWUH508_Orzel"), hull_upgrade), (pk("PWUT508_Orzel"), torp_upgrade)]);
        let ship_data: pickled::Dict = vec![
            (pk("level"), Value::I64(8)),
            (pk("group"), sv("upgradeable")),
            (pk("ShipUpgradeInfo"), upgrade_info),
            (pk("A_Hull"), a_hull),
            (pk("A_Torpedoes"), a_torpedoes),
        ]
        .into_iter()
        .collect();

        let vehicle = build_ship(&ship_data);
        let ttx = vehicle.ttx_components().expect("ttx components extracted");
        let hardpoints = |names: &[&str]| names.iter().map(|name| (*name).to_owned()).collect::<Vec<_>>();

        assert_eq!(
            ttx.torpedo_loading("A_Torpedoes"),
            Some(&TorpedoLoading {
                loader_groups: vec![
                    TorpedoLoaderGroup { loaders: LoaderCount(2), hardpoints: hardpoints(&["HP_WGT_1", "HP_WGT_2"]) },
                    TorpedoLoaderGroup { loaders: LoaderCount(1), hardpoints: hardpoints(&["HP_WGT_9"]) },
                    TorpedoLoaderGroup { loaders: LoaderCount(2), hardpoints: hardpoints(&["HP_WGT_5"]) },
                ],
                directions: vec![
                    LauncherDirectionGroup {
                        direction: LauncherDirection::Forward,
                        hardpoints: hardpoints(&["HP_WGT_1", "HP_WGT_2"]),
                    },
                    LauncherDirectionGroup {
                        direction: LauncherDirection::Rear,
                        hardpoints: hardpoints(&["HP_WGT_5"])
                    },
                    LauncherDirectionGroup {
                        direction: LauncherDirection::Rotating,
                        hardpoints: hardpoints(&["HP_WGT_9"]),
                    },
                ],
            })
        );
    }

    /// A ship with no `_Torpedoes` upgrade extracts no torpedo entries.
    #[test]
    fn build_ship_no_torpedoes_yields_no_torpedo_entry() {
        let hull_components = dict(vec![(pk("hull"), list(vec![sv("A_Hull")]))]);
        let hull_upgrade = dict(vec![(pk("ucType"), sv("_Hull")), (pk("components"), hull_components)]);
        let a_hull = dict(vec![(pk("health"), fv(10000.0)), (pk("model"), sv("A_Hull.model"))]);
        let upgrade_info = dict(vec![(pk("PAUH001_Hull"), hull_upgrade)]);

        let ship_data: pickled::Dict = vec![
            (pk("level"), Value::I64(5)),
            (pk("group"), sv("special")),
            (pk("ShipUpgradeInfo"), upgrade_info),
            (pk("A_Hull"), a_hull),
        ]
        .into_iter()
        .collect();

        let vehicle = build_ship(&ship_data);
        let ttx = vehicle.ttx_components().expect("ttx components extracted");
        assert!(ttx.torpedoes.is_empty());
        assert!(ttx.torpedoes("PAUH001_Hull").is_none());
    }

    /// Ise (`PJSB526_Ise`) shape: a `_TorpedoBomber` upgrade naming a component
    /// with `planes`, and a hull whose `airSupport` component lists the air
    /// strike aircraft in `ammoList` (real values).
    #[test]
    fn build_ship_extracts_squadrons_and_air_strikes() {
        let bomber_components = dict(vec![(pk("torpedoBomber"), list(vec![sv("A_TorpedoBomber")]))]);
        let bomber_upgrade = dict(vec![
            (pk("ucType"), sv("_TorpedoBomber")),
            (pk("components"), bomber_components),
            (pk("prev"), sv("")),
        ]);
        let a_torpedo_bomber = dict(vec![(pk("planes"), list(vec![sv("PJAB606_ISE_STOCK")]))]);

        let hull_components =
            dict(vec![(pk("hull"), list(vec![sv("A_Hull")])), (pk("airSupport"), list(vec![sv("A_AirSupport")]))]);
        let hull_upgrade = dict(vec![(pk("ucType"), sv("_Hull")), (pk("components"), hull_components)]);
        let a_hull = dict(vec![(pk("health"), fv(68300.0)), (pk("model"), sv("A_Hull.model"))]);
        let a_air_support = dict(vec![
            (pk("ammoList"), list(vec![sv("PJAD904_ASW_T5")])),
            (pk("chargesNum"), Value::I64(1)),
            (pk("reloadTime"), fv(20.0)),
            (pk("maxDist"), Value::I64(8000)),
        ]);

        let upgrade_info = dict(vec![(pk("PJUH550_ISE"), hull_upgrade), (pk("PJUB550_TB_ISE"), bomber_upgrade)]);
        let ship_data: pickled::Dict = vec![
            (pk("level"), Value::I64(6)),
            (pk("group"), sv("special")),
            (pk("ShipUpgradeInfo"), upgrade_info),
            (pk("A_Hull"), a_hull),
            (pk("A_TorpedoBomber"), a_torpedo_bomber),
            (pk("A_AirSupport"), a_air_support),
        ]
        .into_iter()
        .collect();

        let vehicle = build_ship(&ship_data);
        let ttx = vehicle.ttx_components().expect("ttx components extracted");

        let squadron = ttx.squadrons.get("PJUB550_TB_ISE").expect("torpedo bomber upgrade parsed");
        assert_eq!(squadron.slot, crate::game_params::ttx::components::PlaneSlot::TorpedoBomber);
        assert_eq!(squadron.planes, vec!["PJAB606_ISE_STOCK".to_string()]);
        assert!(squadron.stock);

        let strike = ttx.air_supports.get("PJUH550_ISE").expect("air strike parsed");
        assert_eq!(strike.planes, vec!["PJAD904_ASW_T5".to_string()]);
        assert_eq!(strike.charges, Some(1));
        assert_eq!(strike.reload_time, Some(crate::game_params::ttx::model::Seconds::from(20.0)));
        assert_eq!(strike.max_dist, Some(Meters::from(8000.0)));
    }

    /// Gearing (`PASD013_Gearing_1945`) shape: a hull whose `depthCharges`
    /// component has two roller tracks of 4 and six throwers of 1 (real
    /// values, two of the throwers here).
    #[test]
    fn build_ship_extracts_depth_charges() {
        let hull_components = dict(vec![
            (pk("hull"), list(vec![sv("A_Hull")])),
            (pk("depthCharges"), list(vec![sv("A_DepthChargeGuns")])),
        ]);
        let hull_upgrade = dict(vec![(pk("ucType"), sv("_Hull")), (pk("components"), hull_components)]);
        let a_hull = dict(vec![(pk("health"), fv(19100.0)), (pk("model"), sv("A_Hull.model"))]);
        let roller =
            || dict(vec![(pk("numBombs"), Value::I64(4)), (pk("ammoList"), list(vec![sv("PAPD001_mk6_mod1")]))]);
        let thrower =
            || dict(vec![(pk("numBombs"), Value::I64(1)), (pk("ammoList"), list(vec![sv("PAPD003_mk6_mod1_shoot")]))]);
        let depth_charges = dict(vec![
            (pk("HP_AGB_7"), roller()),
            (pk("HP_AGB_8"), roller()),
            (pk("HP_AGB_1"), thrower()),
            (pk("HP_AGB_2"), thrower()),
            (pk("maxPacks"), Value::I64(2)),
            (pk("numShots"), Value::I64(1)),
            (pk("reloadTime"), fv(40.0)),
        ]);

        let upgrade_info = dict(vec![(pk("PAUH911_Gearing_1945"), hull_upgrade)]);
        let ship_data: pickled::Dict = vec![
            (pk("level"), Value::I64(10)),
            (pk("group"), sv("upgradeable")),
            (pk("ShipUpgradeInfo"), upgrade_info),
            (pk("A_Hull"), a_hull),
            (pk("A_DepthChargeGuns"), depth_charges),
        ]
        .into_iter()
        .collect();

        let vehicle = build_ship(&ship_data);
        let ttx = vehicle.ttx_components().expect("ttx components extracted");
        let charges = ttx.depth_charges.get("PAUH911_Gearing_1945").expect("depth charges parsed");
        assert_eq!(charges.charges, Some(2));
        assert_eq!(charges.reload_time, Some(crate::game_params::ttx::model::Seconds::from(40.0)));
        assert_eq!(charges.launchers.len(), 4);
        assert_eq!(charges.depth_charges_per_attack(), Some(10));
        assert_eq!(charges.projectile(), Some("PAPD003_mk6_mod1_shoot"));
    }

    /// Kleber (`PFSD110_Kleber`) shape: hull sections with `hlType`, where the
    /// midsection is a saturating `simple_hitlocation` draining `Hull`, beside
    /// a gun hit location that is not a hull section (real values).
    #[test]
    fn build_ship_extracts_hull_hit_locations() {
        use crate::game_params::ttx::components::HullSectionType;

        let hull_components = dict(vec![(pk("hull"), list(vec![sv("A_Hull")]))]);
        let hull_upgrade = dict(vec![(pk("ucType"), sv("_Hull")), (pk("components"), hull_components)]);
        let section = |hl_type: &str, max_hp: f64, volume_coeff: f64, parent: Option<&str>| {
            let mut entries = vec![
                (pk("hlType"), sv(hl_type)),
                (pk("maxHP"), fv(max_hp)),
                (pk("volumeCoeff"), fv(volume_coeff)),
                (pk("regeneratedHPPart"), fv(0.5)),
                (pk("enhancedRegeneratedHPPart"), fv(0.0)),
            ];
            if let Some(parent) = parent {
                entries.push((pk("parentHL"), sv(parent)));
            }
            dict(entries)
        };
        let a_hull = dict(vec![
            (pk("health"), fv(21900.0)),
            (pk("Cas"), section("simple_hitlocation", 2800.0, 0.165, Some("Hull"))),
            (pk("Bow"), section("simple_hitlocation", 1700.0, 0.165, Some("Hull"))),
            (pk("Hull"), section("hull_hitlocation", 16400.0, 0.165, None)),
            (pk("HP_FGM_1"), section("artillery_hitlocation", 500.0, 0.1, None)),
        ]);

        let upgrade_info = dict(vec![(pk("PFUH110_Kleber"), hull_upgrade)]);
        let ship_data: pickled::Dict = vec![
            (pk("level"), Value::I64(10)),
            (pk("group"), sv("upgradeable")),
            (pk("ShipUpgradeInfo"), upgrade_info),
            (pk("A_Hull"), a_hull),
        ]
        .into_iter()
        .collect();

        let vehicle = build_ship(&ship_data);
        let ttx = vehicle.ttx_components().expect("ttx components extracted");
        let sections = &ttx.hull("PFUH110_Kleber").expect("hull parsed").hit_locations;
        let names: Vec<&str> = sections.iter().map(|section| section.name.as_str()).collect();
        assert_eq!(names, ["Bow", "Cas", "Hull"], "sorted by name, gun hit location left out");

        let midsection = &sections[1];
        assert_eq!(midsection.section_type, HullSectionType::Simple);
        assert_eq!(midsection.max_hp, Some(crate::game_params::ttx::model::Hp::from(2800.0)));
        assert_eq!(midsection.parent.as_deref(), Some("Hull"));
        assert_eq!(midsection.volume_coeff, Some(0.165));
        assert_eq!(sections[2].section_type, HullSectionType::Hull);
        assert_eq!(sections[2].parent, None);
    }

    /// Build a ship dict mirroring Worcester's (`PASC016_Worcester_1948`) real
    /// `ArtilleryDefault` shape: an `_Artillery` upgrade naming an `ArtilleryDefault`
    /// component with a component-level `maxDist` and `HP_AGM_*` guns (real values).
    #[test]
    fn build_ship_extracts_artillery_gun_base_stats() {
        let hp_agm = || {
            dict(vec![
                (pk("shotDelay"), fv(4.6)),
                (pk("rotationSpeed"), list(vec![fv(25.0), fv(50.0)])),
                (pk("numBarrels"), fv(2.0)),
                (pk("barrelDiameter"), fv(0.152)),
                (pk("ammoSwitchCoeff"), fv(1.0)),
                (pk("minRadius"), fv(1.1)),
                (pk("idealRadius"), fv(8.0)),
                (pk("idealDistance"), fv(1000.0)),
                (
                    pk("ammoList"),
                    list(vec![sv("PAPA051_152mm_HE_HC_Mark_39_Mod_0"), sv("PAPA050_152mm_AP_130lbs_Mk35")]),
                ),
            ])
        };
        let arty_default =
            dict(vec![(pk("maxDist"), fv(15320.0)), (pk("HP_AGM_1"), hp_agm()), (pk("HP_AGM_2"), hp_agm())]);

        let arty_components = dict(vec![(pk("artillery"), list(vec![sv("ArtilleryDefault")]))]);
        let arty_upgrade = dict(vec![(pk("ucType"), sv("_Artillery")), (pk("components"), arty_components)]);

        // Minimal hull upgrade so build_ship has a hull component.
        let hull_components = dict(vec![(pk("hull"), list(vec![sv("A_Hull")]))]);
        let hull_upgrade = dict(vec![(pk("ucType"), sv("_Hull")), (pk("components"), hull_components)]);
        let a_hull = dict(vec![(pk("health"), fv(40300.0)), (pk("model"), sv("A_Hull.model"))]);

        let upgrade_info =
            dict(vec![(pk("PAUH911_Worcester_1948"), hull_upgrade), (pk("PAUA901_Worcester_ART_STOCK"), arty_upgrade)]);

        let ship_data: pickled::Dict = vec![
            (pk("level"), Value::I64(10)),
            (pk("group"), sv("special")),
            (pk("ShipUpgradeInfo"), upgrade_info),
            (pk("A_Hull"), a_hull),
            (pk("ArtilleryDefault"), arty_default),
        ]
        .into_iter()
        .collect();

        let vehicle = build_ship(&ship_data);
        let ttx = vehicle.ttx_components().expect("ttx components extracted");

        let arty = ttx.artillery("PAUA901_Worcester_ART_STOCK").expect("artillery stats present");
        assert_eq!(arty.max_dist, Some(Meters::from(15320.0)));
        assert_eq!(arty.guns.len(), 2);
        let g = &arty.guns[0];
        assert_eq!(g.shot_delay, Some(crate::game_params::ttx::model::Seconds::from(4.6)));
        assert_eq!(g.rotation_speed, Some(crate::game_params::ttx::model::DegreesPerSecond::from(25.0)));
        assert_eq!(g.num_barrels, Some(2.0));
        assert_eq!(g.barrel_diameter, Some(Meters::from(0.152)));
        assert_eq!(g.ammo_switch_coeff, Some(1.0));
        assert_eq!(g.min_radius, Some(1.1));
        assert_eq!(g.ideal_radius, Some(8.0));
        assert_eq!(g.ideal_distance, Some(1000.0));
        assert_eq!(
            g.ammo,
            vec!["PAPA051_152mm_HE_HC_Mark_39_Mod_0".to_string(), "PAPA050_152mm_AP_130lbs_Mk35".to_string()]
        );

        assert!(ttx.artillery("NoSuchUpgrade").is_none());
    }

    /// A ship with no `_Artillery` upgrade extracts no artillery entries.
    #[test]
    fn build_ship_no_artillery_yields_no_artillery_entry() {
        let hull_components = dict(vec![(pk("hull"), list(vec![sv("A_Hull")]))]);
        let hull_upgrade = dict(vec![(pk("ucType"), sv("_Hull")), (pk("components"), hull_components)]);
        let a_hull = dict(vec![(pk("health"), fv(10000.0)), (pk("model"), sv("A_Hull.model"))]);
        let upgrade_info = dict(vec![(pk("PAUH001_Hull"), hull_upgrade)]);

        let ship_data: pickled::Dict = vec![
            (pk("level"), Value::I64(5)),
            (pk("group"), sv("special")),
            (pk("ShipUpgradeInfo"), upgrade_info),
            (pk("A_Hull"), a_hull),
        ]
        .into_iter()
        .collect();

        let vehicle = build_ship(&ship_data);
        let ttx = vehicle.ttx_components().expect("ttx components extracted");
        assert!(ttx.artillery.is_empty());
        assert!(ttx.artillery("PAUH001_Hull").is_none());
    }

    /// Build a ship mirroring Iowa's (`PASB018_Iowa_1944`) real fire-control shape:
    /// two `_Suo` upgrades, each naming an FC component carrying `maxDistCoef`. The
    /// stock FC (`AB1_FireControl`) has coef 1.0; the range-extender FC
    /// (`AB2_FireControl`) has coef 1.1.
    #[test]
    fn build_ship_extracts_fire_control_max_dist_coef() {
        let ab1_fc = dict(vec![(pk("maxDistCoef"), fv(1.0)), (pk("sigmaCountCoef"), fv(1.0))]);
        let ab2_fc = dict(vec![(pk("maxDistCoef"), fv(1.1)), (pk("sigmaCountCoef"), fv(1.0))]);

        let suo1_components = dict(vec![(pk("fireControl"), list(vec![sv("AB1_FireControl")]))]);
        let suo1_upgrade = dict(vec![(pk("ucType"), sv("_Suo")), (pk("components"), suo1_components)]);
        let suo2_components = dict(vec![(pk("fireControl"), list(vec![sv("AB2_FireControl")]))]);
        let suo2_upgrade = dict(vec![(pk("ucType"), sv("_Suo")), (pk("components"), suo2_components)]);

        // Minimal hull upgrade so build_ship has a hull component.
        let hull_components = dict(vec![(pk("hull"), list(vec![sv("A_Hull")]))]);
        let hull_upgrade = dict(vec![(pk("ucType"), sv("_Hull")), (pk("components"), hull_components)]);
        let a_hull = dict(vec![(pk("health"), fv(76600.0)), (pk("model"), sv("A_Hull.model"))]);

        let upgrade_info = dict(vec![
            (pk("PAUH018_Iowa_1944"), hull_upgrade),
            (pk("PAUS821_Suo"), suo1_upgrade),
            (pk("PAUS822_Suo"), suo2_upgrade),
        ]);

        let ship_data: pickled::Dict = vec![
            (pk("level"), Value::I64(9)),
            (pk("group"), sv("special")),
            (pk("ShipUpgradeInfo"), upgrade_info),
            (pk("A_Hull"), a_hull),
            (pk("AB1_FireControl"), ab1_fc),
            (pk("AB2_FireControl"), ab2_fc),
        ]
        .into_iter()
        .collect();

        let vehicle = build_ship(&ship_data);
        let ttx = vehicle.ttx_components().expect("ttx components extracted");

        assert_eq!(ttx.fire_control_max_dist_coef("PAUS821_Suo"), Some(1.0));
        assert_eq!(ttx.fire_control_max_dist_coef("PAUS822_Suo"), Some(1.1));
        assert_eq!(ttx.fire_control_max_dist_coef("NoSuchUpgrade"), None);
        assert_eq!(ttx.fire_control_sigma_count_coef("PAUS822_Suo"), Some(1.0));
    }

    /// A ship with no `_Suo` upgrade extracts no fire-control entries.
    #[test]
    fn build_ship_no_fire_control_yields_no_fc_entry() {
        let hull_components = dict(vec![(pk("hull"), list(vec![sv("A_Hull")]))]);
        let hull_upgrade = dict(vec![(pk("ucType"), sv("_Hull")), (pk("components"), hull_components)]);
        let a_hull = dict(vec![(pk("health"), fv(10000.0)), (pk("model"), sv("A_Hull.model"))]);
        let upgrade_info = dict(vec![(pk("PAUH001_Hull"), hull_upgrade)]);

        let ship_data: pickled::Dict = vec![
            (pk("level"), Value::I64(5)),
            (pk("group"), sv("special")),
            (pk("ShipUpgradeInfo"), upgrade_info),
            (pk("A_Hull"), a_hull),
        ]
        .into_iter()
        .collect();

        let vehicle = build_ship(&ship_data);
        let ttx = vehicle.ttx_components().expect("ttx components extracted");
        assert!(ttx.fire_controls.is_empty());
        assert!(ttx.fire_control_max_dist_coef("PAUH001_Hull").is_none());
    }

    /// Mirrors Baltimore's `ShipUpgradeInfo`: a researchable hull unlocking the
    /// next ship, a zero doubloon price, and an empty `requiredShip`.
    #[test]
    fn build_ship_extracts_research_costs_and_links() {
        let hull_components = dict(vec![(pk("hull"), list(vec![sv("A_Hull")]))]);
        let hull_upgrade = dict(vec![
            (pk("ucType"), sv("_Hull")),
            (pk("components"), hull_components),
            (pk("nextShips"), list(vec![sv("PASC020_Des_Moines_1948")])),
            (pk("prev"), sv("PAUH801_Baltimore_1944")),
        ]);
        let stock_hull = dict(vec![(pk("ucType"), sv("_Hull")), (pk("nextShips"), list(vec![])), (pk("prev"), sv(""))]);
        let a_hull = dict(vec![(pk("health"), fv(10000.0)), (pk("model"), sv("A_Hull.model"))]);
        let upgrade_info = dict(vec![
            (pk("PAUH801_Baltimore_1944"), stock_hull),
            (pk("PAUH802_Baltimore_1948"), hull_upgrade),
            (pk("costCR"), iv(13_600_000)),
            (pk("costGold"), iv(0)),
            (pk("costXP"), iv(165_000)),
            (pk("requiredShip"), sv("")),
        ]);

        let ship_data: pickled::Dict = vec![
            (pk("level"), Value::I64(9)),
            (pk("group"), sv("upgradeable")),
            (pk("ShipUpgradeInfo"), upgrade_info),
            (pk("A_Hull"), a_hull),
        ]
        .into_iter()
        .collect();

        let research = build_ship(&ship_data).research().clone();
        assert_eq!(research.cost_credits(), Some(super::super::types::Credits::new(13_600_000)));
        assert_eq!(research.cost_doubloons(), None);
        assert_eq!(research.cost_xp(), Some(super::super::types::Experience::new(165_000)));
        assert_eq!(research.next_ships(), ["PASC020_Des_Moines_1948".to_owned()]);
        assert_eq!(research.required_ship(), None);
    }

    #[test]
    fn mounts_carry_traverse_dead_zones_barrels_and_ammo() {
        // Tianjin PZSC719 HP_ZGS_1: unlimited rotation with a rear dead zone;
        // Des Moines HP_AGM_3: limited to +-150.
        let atba = dict(vec![(
            pk("HP_ZGS_1"),
            dict(vec![
                (pk("model"), sv("content/gun.model")),
                (pk("typeinfo"), typeinfo("China", "Secondary", "Gun")),
                (pk("horizSector"), tuple(vec![fv(0.0), fv(0.0)])),
                (pk("deadZone"), tuple(vec![tuple(vec![fv(147.0), fv(-147.0)])])),
                (pk("numBarrels"), fv(2.0)),
                (pk("ammoList"), tuple(vec![sv("PZPA001_HE")])),
            ]),
        )]);
        let artillery = dict(vec![(
            pk("HP_AGM_3"),
            dict(vec![
                (pk("model"), sv("content/turret.model")),
                (pk("horizSector"), tuple(vec![fv(-150.0), iv(150)])),
                (pk("deadZone"), tuple(vec![])),
                (pk("numBarrels"), fv(3.0)),
            ]),
        )]);
        let ship_data: pickled::Dict = vec![(pk("A_ATBA"), atba), (pk("A_Artillery"), artillery)].into_iter().collect();

        let secondary = &extract_mounts(&ship_data, "A_ATBA")[0];
        assert_eq!(secondary.traverse(), Some(Traverse::Unlimited));
        assert_eq!(secondary.num_barrels(), Some(2));
        assert_eq!(secondary.ammo(), ["PZPA001_HE".to_owned()]);
        assert_eq!(secondary.can_fire_at(Degrees::new(146.0)), Some(true));
        assert_eq!(secondary.can_fire_at(Degrees::new(180.0)), Some(false));
        assert_eq!(secondary.can_fire_at(Degrees::new(-147.0)), Some(false));
        assert_eq!(secondary.can_fire_at(Degrees::new(-146.0)), Some(true));

        let turret = &extract_mounts(&ship_data, "A_Artillery")[0];
        assert_eq!(turret.num_barrels(), Some(3));
        assert_eq!(turret.can_fire_at(Degrees::new(150.0)), Some(true));
        assert_eq!(turret.can_fire_at(Degrees::new(151.0)), Some(false));
        assert_eq!(turret.can_fire_at(Degrees::new(-150.0)), Some(true));
        assert_eq!(turret.can_fire_at(Degrees::new(0.0)), Some(true));
        assert_eq!(turret.can_fire_at(Degrees::new(180.0)), Some(false));
    }

    /// Fixed tubes whose torpedoes turn after launch fire across the turn,
    /// and a dead zone keeps the launcher yaws it covers out of it.
    #[test]
    fn torpedo_start_maneuver_widens_where_launchers_fire() {
        // Defence PBSC810 HP_BGT_2: fixed tubes, torpedoes turn up to 65 deg
        // to starboard. Myoko PJSC705 HP_JGT_3: a dead zone near the bow and
        // a turn of up to 15 deg aft.
        let torpedoes = dict(vec![
            (
                pk("HP_BGT_2"),
                dict(vec![
                    (pk("model"), sv("content/tubes.model")),
                    (pk("horizSector"), tuple(vec![fv(-0.1), fv(0.1)])),
                    (pk("deadZone"), tuple(vec![])),
                    (pk("additionalAimSector"), tuple(vec![fv(0.0), fv(65.0)])),
                    (pk("numBarrels"), fv(4.0)),
                ]),
            ),
            (
                pk("HP_JGT_3"),
                dict(vec![
                    (pk("model"), sv("content/tubes.model")),
                    (pk("horizSector"), tuple(vec![fv(0.0), fv(88.0)])),
                    (pk("deadZone"), tuple(vec![tuple(vec![fv(0.0), fv(36.0)])])),
                    (pk("additionalAimSector"), tuple(vec![fv(-0.0), fv(15.0)])),
                    (pk("numBarrels"), fv(3.0)),
                ]),
            ),
        ]);
        let ship_data: pickled::Dict = vec![(pk("A_Torpedoes"), torpedoes)].into_iter().collect();
        let mounts = extract_mounts(&ship_data, "A_Torpedoes");
        let mount = |name: &str| mounts.iter().find(|mount| mount.hp_name() == name).expect("mount parsed");

        let fixed = mount("HP_BGT_2");
        assert_eq!(fixed.aim_offsets(), Some(YawSector::new(Degrees::new(0.0), Degrees::new(65.0))));
        assert_eq!(fixed.can_fire_at(Degrees::new(0.0)), Some(true));
        assert_eq!(fixed.can_fire_at(Degrees::new(30.0)), Some(true));
        assert_eq!(fixed.can_fire_at(Degrees::new(65.0)), Some(true));
        assert_eq!(fixed.can_fire_at(Degrees::new(66.0)), Some(false));
        assert_eq!(fixed.can_fire_at(Degrees::new(-5.0)), Some(false));

        let turning = mount("HP_JGT_3");
        assert_eq!(turning.can_fire_at(Degrees::new(20.0)), Some(false));
        assert_eq!(turning.can_fire_at(Degrees::new(37.0)), Some(true));
        assert_eq!(turning.can_fire_at(Degrees::new(100.0)), Some(true));
        assert_eq!(turning.can_fire_at(Degrees::new(104.0)), Some(false));
    }
}

#[cfg(test)]
mod crew_skill_logic_trigger_tests {
    use super::*;
    use pickled::value::Shared;

    fn fv(f: f64) -> Value {
        Value::F64(f)
    }

    fn iv(i: i64) -> Value {
        Value::I64(i)
    }

    fn sv(s: &str) -> Value {
        Value::String(s.to_string().into())
    }

    fn list(items: Vec<Value>) -> Value {
        Value::List(Shared::new(items))
    }

    fn dict(entries: Vec<(HashableValue, Value)>) -> Value {
        Value::Dict(Shared::new(entries.into_iter().collect()))
    }

    fn base_trigger_entries(trigger_type: &str) -> Vec<(HashableValue, Value)> {
        vec![
            (pk("triggerType"), sv(trigger_type)),
            (pk("triggerDescIds"), sv("")),
            (pk("consumableType"), sv("")),
            (pk("coolingDelay"), fv(0.0)),
            (pk("duration"), fv(0.0)),
            (pk("energyCoeff"), fv(0.0)),
        ]
    }

    fn build_skill_with_trigger(trigger_entries: Vec<(HashableValue, Value)>) -> Vec<CrewSkill> {
        let logic_trigger = dict(trigger_entries);
        let skill_entries = vec![
            (pk("LogicTrigger"), logic_trigger),
            (pk("column"), iv(0)),
            (pk("skillType"), iv(1)),
            (pk("canBeLearned"), Value::Bool(true)),
            (pk("isEpic"), Value::Bool(false)),
            (pk("uiTreatAsTrigger"), Value::Bool(true)),
            (
                pk("tier"),
                dict(vec![
                    (pk("AirCarrier"), iv(1)),
                    (pk("Auxiliary"), iv(1)),
                    (pk("Battleship"), iv(1)),
                    (pk("Cruiser"), iv(1)),
                    (pk("Destroyer"), iv(1)),
                    (pk("Submarine"), iv(1)),
                ]),
            ),
        ];
        let skill_dict: pickled::Dict =
            vec![(HashableValue::String("TestSkill".to_string().into()), dict(skill_entries))].into_iter().collect();
        build_crew_skills(&skill_dict)
    }

    #[test]
    fn count_to_modifier_parsed_and_sorted() {
        let mut entries = base_trigger_entries("activationOnBurnFlood");
        entries.push((pk("countToModifier"), dict(vec![(pk("2"), sv("BurnFlood_2")), (pk("1"), sv("BurnFlood_1"))])));
        entries.push((pk("BurnFlood_1"), dict(vec![(pk("GMShotDelay"), fv(0.9))])));
        entries.push((pk("BurnFlood_2"), dict(vec![(pk("GMShotDelay"), fv(0.95))])));

        let skills = build_skill_with_trigger(entries);
        let trigger = skills[0].logic_trigger().expect("trigger present");
        let ctm = trigger.count_to_modifier();
        assert_eq!(ctm.len(), 2, "expected 2 stacks");
        assert_eq!(ctm[0].0, 1);
        assert_eq!(ctm[0].1.len(), 1);
        assert_eq!(ctm[0].1[0].name(), "GMShotDelay");
        assert!((ctm[0].1[0].get_for_species(&Species::Battleship) - 0.9).abs() < 1e-6);
        assert_eq!(ctm[1].0, 2);
        assert!((ctm[1].1[0].get_for_species(&Species::Battleship) - 0.95).abs() < 1e-6);
    }

    #[test]
    fn damage_value_integer_parsed() {
        let mut entries = base_trigger_entries("potentialDamageRatio");
        entries.push((pk("damageValue"), iv(2000000)));
        entries.push((pk("healthFactor"), fv(1.0)));

        let skills = build_skill_with_trigger(entries);
        let trigger = skills[0].logic_trigger().expect("trigger present");
        assert_eq!(trigger.damage_value(), Some(2_000_000.0_f32));
    }

    #[test]
    fn interpolators_parsed_from_lists() {
        let mut entries = base_trigger_entries("atbaHeat");
        entries.push((
            pk("heatInterpolator"),
            list(vec![list(vec![fv(0.0), fv(0.0)]), list(vec![fv(10.0), fv(0.5)]), list(vec![fv(45.0), fv(1.0)])]),
        ));
        entries
            .push((pk("coolingInterpolator"), list(vec![list(vec![fv(0.0), fv(1.0)]), list(vec![fv(10.0), fv(0.0)])])));

        let skills = build_skill_with_trigger(entries);
        let trigger = skills[0].logic_trigger().expect("trigger present");
        let heat = trigger.heat_interpolator().expect("heat interpolator present");
        assert_eq!(heat.points(), &[(0.0, 0.0), (10.0, 0.5), (45.0, 1.0)]);
        let cool = trigger.cooling_interpolator().expect("cooling interpolator present");
        assert_eq!(cool.points(), &[(0.0, 1.0), (10.0, 0.0)]);
    }

    #[test]
    fn interpolators_parsed_from_tuples() {
        let mut entries = base_trigger_entries("atbaHeat");
        entries.push((
            pk("heatInterpolator"),
            Value::Tuple(pickled::value::SharedFrozen::new(vec![
                list(vec![fv(0.0), fv(0.0)]),
                list(vec![fv(10.0), fv(0.5)]),
                list(vec![fv(45.0), fv(1.0)]),
            ])),
        ));

        let skills = build_skill_with_trigger(entries);
        let trigger = skills[0].logic_trigger().expect("trigger present");
        let heat = trigger.heat_interpolator().expect("heat interpolator present");
        assert_eq!(heat.points(), &[(0.0, 0.0), (10.0, 0.5), (45.0, 1.0)]);
    }

    #[test]
    fn missing_new_fields_yield_defaults() {
        let entries = base_trigger_entries("activationOnDetectTrigger");

        let skills = build_skill_with_trigger(entries);
        let trigger = skills[0].logic_trigger().expect("trigger present");
        assert!(trigger.count_to_modifier().is_empty());
        assert_eq!(trigger.damage_value(), None);
        assert_eq!(trigger.heat_interpolator(), None);
        assert_eq!(trigger.cooling_interpolator(), None);
    }
}

#[cfg(test)]
mod innate_skill_tests {
    use super::*;
    use pickled::value::Shared;

    fn fv(f: f64) -> Value {
        Value::F64(f)
    }

    fn sv(s: &str) -> Value {
        Value::String(s.to_string().into())
    }

    fn list(items: Vec<Value>) -> Value {
        Value::List(Shared::new(items))
    }

    fn dict(entries: Vec<(HashableValue, Value)>) -> Value {
        Value::Dict(Shared::new(entries.into_iter().collect()))
    }

    fn build_minimal_ship(extra_entries: Vec<(HashableValue, Value)>) -> Vehicle {
        let hull_components = dict(vec![(pk("hull"), list(vec![sv("A_Hull")]))]);
        let hull_upgrade =
            dict(vec![(pk("ucType"), sv("_Hull")), (pk("components"), hull_components), (pk("prev"), sv(""))]);
        let upgrade_info = dict(vec![(pk("PAUH001_Stock"), hull_upgrade)]);
        let a_hull = dict(vec![(pk("health"), fv(10000.0))]);
        let mut entries = vec![
            (pk("level"), Value::I64(8)),
            (pk("group"), sv("special")),
            (pk("ShipUpgradeInfo"), upgrade_info),
            (pk("A_Hull"), a_hull),
        ];
        entries.extend(extra_entries);
        let ship_data: pickled::Dict = entries.into_iter().collect();
        build_ship(&ship_data)
    }

    #[test]
    fn innate_skills_parsed_from_hull_component() {
        let full_health_block = dict(vec![(pk("GMShotDelay"), fv(1.0))]);
        let half_health_block = dict(vec![(pk("GMShotDelay"), fv(0.9))]);
        let adrenaline_rush = dict(vec![
            (pk("skillType"), sv("adrenalineRush")),
            (
                pk("healthModifiers"),
                list(vec![list(vec![fv(1.0), sv("fullHealth")]), list(vec![fv(0.5), sv("halfHealth")])]),
            ),
            (pk("fullHealth"), full_health_block),
            (pk("halfHealth"), half_health_block),
        ]);
        let a_innate = dict(vec![(pk("AdrenalineRush"), adrenaline_rush)]);

        let hull_components =
            dict(vec![(pk("hull"), list(vec![sv("A_Hull")])), (pk("innateSkills"), list(vec![sv("A_Innate")]))]);
        let hull_upgrade =
            dict(vec![(pk("ucType"), sv("_Hull")), (pk("components"), hull_components), (pk("prev"), sv(""))]);
        let upgrade_info = dict(vec![(pk("PAUH001_Stock"), hull_upgrade)]);
        let a_hull = dict(vec![(pk("health"), fv(10000.0))]);
        let ship_data: pickled::Dict = vec![
            (pk("level"), Value::I64(8)),
            (pk("group"), sv("special")),
            (pk("ShipUpgradeInfo"), upgrade_info),
            (pk("A_Hull"), a_hull),
            (pk("A_Innate"), a_innate),
        ]
        .into_iter()
        .collect();

        let vehicle = build_ship(&ship_data);
        let skills = vehicle.innate_skills();
        assert_eq!(skills.len(), 1, "expected one innate skill");
        let skill = &skills[0];
        assert_eq!(skill.skill_type(), "adrenalineRush");
        let bps = skill.breakpoints();
        assert_eq!(bps.len(), 2);
        assert!((bps[0].health_fraction() - 1.0).abs() < 1e-6);
        assert_eq!(bps[0].modifiers().len(), 1);
        assert_eq!(bps[0].modifiers()[0].name(), "GMShotDelay");
        assert!((bps[0].modifiers()[0].get_for_species(&Species::Battleship) - 1.0).abs() < 1e-6);
        assert!((bps[1].health_fraction() - 0.5).abs() < 1e-6);
        assert!((bps[1].modifiers()[0].get_for_species(&Species::Battleship) - 0.9).abs() < 1e-6);
    }

    #[test]
    fn ship_without_innate_skills_has_empty_vec() {
        let vehicle = build_minimal_ship(vec![]);
        assert!(vehicle.innate_skills().is_empty());
    }
}
