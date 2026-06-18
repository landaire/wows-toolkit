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
use pickled::object::DictObject;
use pickled::value::Shared;
#[cfg(feature = "vfs")]
use tracing::debug;

/// Extension trait that provides dict extraction for both `Value::Dict` and `Value::Object`.
trait ValueDictExt {
    fn dict_or_object_dict(&self) -> Option<Shared<BTreeMap<HashableValue, Value>>>;
}

impl ValueDictExt for Value {
    fn dict_or_object_dict(&self) -> Option<Shared<BTreeMap<HashableValue, Value>>> {
        match self {
            Value::Dict(d) => Some(d.clone()),
            Value::Object(o) => {
                let inner = o.inner();
                let dict_obj = inner.as_any().downcast_ref::<DictObject>()?;
                Some(Shared::new(dict_obj.state().clone()))
            }
            _ => None,
        }
    }
}

use crate::Rc;
#[cfg(feature = "vfs")]
use crate::data::DataFileWithCallback;
use crate::data::ResourceLoader;
use crate::error::GameDataError;
use crate::game_params::convert::game_params_to_pickle;
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

    fn localized_name_from_id(&self, id: &str) -> Option<String> {
        self.translate(id)
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
fn build_skill_modifiers(modifiers: &BTreeMap<HashableValue, Value>) -> Vec<CrewSkillModifier> {
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

fn build_crew_skills(skills: &BTreeMap<HashableValue, Value>) -> Vec<CrewSkill> {
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
                CrewSkillLogicTrigger::builder()
                    .maybe_burn_count(game_param_to_type!(logic_trigger_data, "burnCount", Option<usize>))
                    .maybe_change_priority_target_penalty(game_param_to_type!(
                        logic_trigger_data,
                        "changePriorityTargetPenalty",
                        Option<f32>
                    ))
                    .consumable_type(game_param_to_type!(logic_trigger_data, "consumableType", String))
                    .cooling_delay(game_param_to_type!(logic_trigger_data, "coolingDelay", f32))
                    .cooling_interpolator(Vec::default())
                    .maybe_divider_type(game_param_to_type!(logic_trigger_data, "dividerType", Option<String>))
                    .maybe_divider_value(game_param_to_type!(logic_trigger_data, "dividerValue", Option<f32>))
                    .duration(game_param_to_type!(logic_trigger_data, "duration", f32))
                    .energy_coeff(game_param_to_type!(logic_trigger_data, "energyCoeff", f32))
                    .maybe_flood_count(game_param_to_type!(logic_trigger_data, "floodCount", Option<usize>))
                    .maybe_health_factor(game_param_to_type!(logic_trigger_data, "healthFactor", Option<f32>))
                    .heat_interpolator(Vec::default())
                    .maybe_modifiers(trigger_modifiers)
                    .trigger_desc_ids(game_param_to_type!(logic_trigger_data, "triggerDescIds", String))
                    .trigger_type(game_param_to_type!(logic_trigger_data, "triggerType", String))
                    .build()
            });

            // Modern skills nest effect coefficients under "modifiers"; pre-rework
            // skills store them as flat sibling keys alongside the structural keys.
            let modifiers = match skill_data.get(&pk("modifiers")).and_then(|v| v.dict_or_object_dict()) {
                Some(modifiers) => Some(build_skill_modifiers(&modifiers.inner())),
                None => {
                    let flat: BTreeMap<HashableValue, Value> = skill_data
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
                    CrewSkillTiers::builder()
                        .aircraft_carrier(
                            SkillPointCost::new(game_param_to_type!(tier_data, "AirCarrier", usize) as u8),
                        )
                        .auxiliary(SkillPointCost::new(game_param_to_type!(tier_data, "Auxiliary", usize) as u8))
                        .battleship(SkillPointCost::new(game_param_to_type!(tier_data, "Battleship", usize) as u8))
                        .cruiser(SkillPointCost::new(game_param_to_type!(tier_data, "Cruiser", usize) as u8))
                        .destroyer(SkillPointCost::new(game_param_to_type!(tier_data, "Destroyer", usize) as u8))
                        .submarine(SkillPointCost::new(game_param_to_type!(tier_data, "Submarine", usize) as u8))
                        .build()
                }
                None => {
                    let t =
                        skill_data.get(&pk("tier")).and_then(|v| v.i64_ref()).map(|&v| v as usize).unwrap_or_default();
                    let cost = SkillPointCost::new(t as u8);
                    CrewSkillTiers::builder()
                        .aircraft_carrier(cost)
                        .auxiliary(cost)
                        .battleship(cost)
                        .cruiser(cost)
                        .destroyer(cost)
                        .submarine(cost)
                        .build()
                }
            };

            Some(
                CrewSkill::builder()
                    .internal_name(CrewSkillName::from(skill_name.to_owned()))
                    .can_be_learned(game_param_to_type!(skill_data, "canBeLearned", Option<bool>).unwrap_or_default())
                    .is_epic(game_param_to_type!(skill_data, "isEpic", Option<bool>).unwrap_or_default())
                    .skill_type(CrewSkillType::new(
                        game_param_to_type!(skill_data, "skillType", Option<usize>).unwrap_or_default() as u32,
                    ))
                    .ui_treat_as_trigger(
                        game_param_to_type!(skill_data, "uiTreatAsTrigger", Option<bool>).unwrap_or_default(),
                    )
                    .tier(tier)
                    .maybe_modifiers(modifiers)
                    .maybe_logic_trigger(logic_trigger)
                    .build(),
            )
        })
        .collect()
}

/// Extract a list-of-strings field, returning empty when the field is absent or
/// not a string list. Older builds omit some of these list fields entirely.
fn string_list_field(dict: &BTreeMap<HashableValue, Value>, key: &str) -> Vec<String> {
    dict.get(&HashableValue::String(key.to_owned().into()))
        .and_then(|value| value.list_ref())
        .map(|list| list.inner().iter().filter_map(|v| v.string_ref().map(|s| s.inner().to_owned())).collect())
        .unwrap_or_default()
}

fn build_crew_personality(personality: &BTreeMap<HashableValue, Value>) -> CrewPersonality {
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
        .cost_credits(game_param_to_type!(personality, "costCR", Option<usize>).unwrap_or_default())
        .cost_elite_xp(game_param_to_type!(personality, "costELXP", Option<usize>).unwrap_or_default())
        .cost_gold(game_param_to_type!(personality, "costGold", Option<usize>).unwrap_or_default())
        .cost_xp(game_param_to_type!(personality, "costXP", Option<usize>).unwrap_or_default())
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
        .person_name(game_param_to_type!(personality, "personName", Option<String>).unwrap_or_default())
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

fn build_ability_category(category_data: &BTreeMap<HashableValue, Value>) -> AbilityCategory {
    let reload_time =
        if let Some(reload_time) = category_data.get(&HashableValue::String("reloadTime".to_owned().into())) {
            if let Some(reload_time) = reload_time.i64_ref() {
                *reload_time as f32
            } else {
                *reload_time.f64_ref().expect("workTime is not a f64") as f32
            }
        } else {
            panic!("could not get reloadTime");
        };

    let work_time = if let Some(work_time) = category_data.get(&HashableValue::String("workTime".to_owned().into())) {
        if let Some(work_time) = work_time.i64_ref() {
            *work_time as f32
        } else {
            *work_time.f64_ref().expect("workTime is not a f64") as f32
        }
    } else {
        panic!("could not get reloadTime");
    };

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

    AbilityCategory::builder()
        .effect_fields(effect_fields)
        .maybe_special_sound_id(game_param_to_type!(category_data, "SpecialSoundID", Option<String>))
        .consumable_type(game_param_to_type!(category_data, "consumableType", Option<String>).unwrap_or_default())
        .group(game_param_to_type!(category_data, "group", Option<String>).unwrap_or_default())
        .icon_id(game_param_to_type!(category_data, "iconIDs", Option<String>).unwrap_or_default())
        .num_consumables(game_param_to_type!(category_data, "numConsumables", Option<isize>).unwrap_or_default())
        .preparation_time(game_param_to_type!(category_data, "preparationTime", Option<f32>).unwrap_or_default())
        .reload_time(reload_time)
        .work_time(work_time)
        .maybe_dist_ship(dist_ship)
        .maybe_dist_torpedo(dist_torpedo)
        .maybe_hydrophone_wave_radius(hydrophone_wave_radius)
        .maybe_patrol_radius(patrol_radius)
        .maybe_regeneration_hp_speed(read_f32("regenerationHPSpeed"))
        .maybe_regeneration_hp_speed_units(read_f32("regenerationHPSpeedUnits"))
        .build()
}

fn build_ability(ability_data: &BTreeMap<HashableValue, Value>) -> Ability {
    let test_key = HashableValue::String("numConsumables".to_string().into());
    let categories: HashMap<String, AbilityCategory> =
        HashMap::from_iter(ability_data.iter().filter_map(|(key, value)| {
            // GameParams pickled categories arrive as either Value::Dict or
            // Value::Object(DictObject); `is_not_dict` misses the object case
            // and was dropping every Ability variant on the floor.
            let dict = value.dict_or_object_dict()?;
            let inner = dict.inner();
            if inner.contains_key(&test_key) {
                Some((key.string_ref().unwrap().inner().to_owned(), build_ability_category(&inner)))
            } else {
                None
            }
        }));

    Ability::builder()
        .can_buy(game_param_to_type!(ability_data, "canBuy", bool))
        .cost_credits(game_param_to_type!(ability_data, "costCR", isize))
        .cost_gold(game_param_to_type!(ability_data, "costGold", isize))
        .is_free(game_param_to_type!(ability_data, "freeOfCharge", bool))
        .categories(categories)
        .build()
}

/// Helper: create a pickled dict key. Checks for `String` first, then `Bytes`.
fn pk(key: &str) -> HashableValue {
    HashableValue::String(key.to_string().into())
}

/// Helper: read a float from a pickled dict, accepting both f64 and i64.
fn read_float(dict: &BTreeMap<HashableValue, Value>, key: &str) -> Option<f32> {
    dict.get(&pk(key)).and_then(|v| v.f64_ref().map(|f| *f as f32).or_else(|| v.i64_ref().map(|i| *i as f32)))
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
fn read_string(dict: &BTreeMap<HashableValue, Value>, key: &str) -> Option<String> {
    dict.get(&pk(key)).and_then(|v| v.string_ref()).map(|s| s.inner().to_string())
}

fn value_f32(v: &Value) -> Option<f32> {
    v.f64_ref().map(|f| *f as f32).or_else(|| v.i64_ref().map(|i| *i as f32))
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

/// Read a 3-element float triple from the element at `idx` of a nested list-or-tuple.
fn read_vec3_at(v: &Value, idx: usize) -> Option<[f32; 3]> {
    fn triple(elem: &Value) -> Option<[f32; 3]> {
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
    if let Some(l) = v.list_ref() {
        triple(l.inner().get(idx)?)
    } else if let Some(t) = v.tuple_ref() {
        triple(t.inner().get(idx)?)
    } else {
        None
    }
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
pub fn read_camera_trajectories(ship_data: &BTreeMap<HashableValue, Value>) -> Vec<(String, CameraTrajectory)> {
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
fn parse_pitch_dead_zones(mount_dict: &BTreeMap<HashableValue, Value>) -> Vec<[f32; 4]> {
    fn as_f32(v: &Value) -> Option<f32> {
        v.f64_ref().map(|f| *f as f32).or_else(|| v.i64_ref().map(|i| *i as f32))
    }
    fn extract_entries(items: &[Value]) -> Vec<[f32; 4]> {
        items
            .iter()
            .filter_map(|entry| {
                let inner: Vec<f32> = if let Some(l) = entry.list_ref() {
                    l.inner().iter().filter_map(as_f32).collect()
                } else if let Some(t) = entry.tuple_ref() {
                    t.inner().iter().filter_map(as_f32).collect()
                } else {
                    return None;
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

/// Parse a raw GameParams armor dict into an [`ArmorMap`].
///
/// Raw keys are `(model_index << 16) | material_id`.  We group by `material_id`
/// and collect per-layer thicknesses ordered by ascending `model_index`.
fn parse_armor_dict(dict: &BTreeMap<HashableValue, Value>) -> ArmorMap {
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
fn extract_mounts(ship_data: &BTreeMap<HashableValue, Value>, component_name: &str) -> Vec<MountPoint> {
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

            Some(MountPoint::with_armor(key_str.clone(), model_path, mount_armor, species, pitch_dead_zones))
        })
        .collect()
}

/// Parse the trailing decimal digits of a hardpoint name as a sort key.
/// Mounts without a trailing number sort last (u32::MAX).
fn hardpoint_number(key: &str) -> u32 {
    let digits: String = key.chars().rev().take_while(|c| c.is_ascii_digit()).collect::<String>();
    if digits.is_empty() {
        return u32::MAX;
    }
    let reversed: String = digits.chars().rev().collect();
    reversed.parse().unwrap_or(u32::MAX)
}

fn build_ship(ship_data: &BTreeMap<HashableValue, Value>) -> Vehicle {
    let ability_data = game_param_to_type!(ship_data, keys::SHIP_ABILITIES, Option<HashMap<(), ()>>);
    let abilities: Option<Vec<Vec<(String, String)>>> = ability_data.map(|abilities_data| {
        abilities_data
            .inner()
            .iter()
            .filter_map(|(slot_name, slot_data)| {
                let _slot_name = slot_name.string_ref().expect("ship ability slot name is not a string");
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
            // drop the slot
            .map(|abil| abil.1)
            .collect()
    });

    let upgrade_data = game_param_to_type!(ship_data, keys::SHIP_UPGRADE_INFO, HashMap<(), ()>);
    let upgrades: Vec<String> = upgrade_data
        .inner()
        .keys()
        .map(|upgrade_name| upgrade_name.string_ref().expect("upgrade name is not a string").inner().to_owned())
        .collect();

    let level = game_param_to_type!(ship_data, "level", u32);
    let group = game_param_to_type!(ship_data, "group", String);

    // Extract hull config data using ShipUpgradeInfo for proper type identification.
    // Each _Hull upgrade in ShipUpgradeInfo maps to specific hull, artillery, and ATBA
    // components. We build a HullUpgradeConfig for each, keyed by upgrade name.
    let mut hull_upgrades = HashMap::new();

    for (upgrade_name_val, upgrade_value) in upgrade_data.inner().iter() {
        let Some(upgrade_name) = upgrade_name_val.string_ref().map(|s| s.inner().clone()) else {
            continue;
        };
        let Some(upgrade_dict) = upgrade_value.dict_or_object_dict() else {
            continue;
        };
        let upgrade_dict = upgrade_dict.inner();

        // Only process _Hull upgrades -- they define the complete config for a hull loadout
        let Some(uc_type) =
            upgrade_dict.get(&pk(keys::UC_TYPE)).and_then(|v| v.string_ref().map(|s| s.inner().clone()))
        else {
            continue;
        };
        if uc_type != keys::UC_TYPE_HULL {
            continue;
        }

        let Some(components) = upgrade_dict.get(&pk(keys::COMPONENTS)).and_then(|v| v.dict_or_object_dict()) else {
            continue;
        };
        let components = components.inner();

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
            config.detection_km = Km::from(read_float(&hull_data, keys::VISIBILITY_FACTOR).unwrap_or(0.0));
            config.air_detection_km = Km::from(read_float(&hull_data, keys::VISIBILITY_FACTOR_BY_PLANE).unwrap_or(0.0));
            config.hull_model_path = read_string(&hull_data, keys::MODEL);
            config.draft = read_float(&hull_data, keys::DRAFT).map(Meters::from);
            config.dock_y_offset = read_float(&hull_data, keys::DOCK_Y_OFFSET);
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
        if config.detection_km.value() > 0.0 || !config.mounts_by_type.is_empty() {
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
    // (hardpoint_key, ammo_name); populated from the first atba-bearing hull only
    let mut secondary_guns_raw: Vec<(String, String)> = Vec::new();

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
                let Some(ammo_val) = mount_inner.get(&pk(keys::AMMO_LIST)) else {
                    continue;
                };
                let mut insert_ammo = |item: &Value| {
                    if let Some(name) = item.string_ref() {
                        secondary_battery_ammo.insert(name.inner().clone());
                    }
                };
                match ammo_val {
                    Value::Tuple(t) => t.inner().iter().for_each(&mut insert_ammo),
                    Value::List(l) => l.inner().iter().for_each(&mut insert_ammo),
                    _ => {}
                }
                if capture_per_gun {
                    let first_ammo = match ammo_val {
                        Value::Tuple(t) => t.inner().iter().find_map(|i| i.string_ref().map(|s| s.inner().clone())),
                        Value::List(l) => l.inner().iter().find_map(|i| i.string_ref().map(|s| s.inner().clone())),
                        _ => None,
                    };
                    if let (Some(name), Some(key_str)) = (first_ammo, mount_key.string_ref().map(|s| s.inner().clone()))
                    {
                        secondary_guns_raw.push((key_str, name));
                    }
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

    secondary_guns_raw.sort_by_key(|(k, _)| hardpoint_number(k));
    let secondary_guns: Vec<String> = secondary_guns_raw.into_iter().map(|(_, name)| name).collect();

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
                let max_hp = read_float(&group_dict, keys::MAX_HP).unwrap_or(0.0);
                let regenerated_hp_part = read_float(&group_dict, keys::REGENERATED_HP_PART).unwrap_or(0.0);
                let thickness = read_float(&group_dict, keys::THICKNESS).unwrap_or(0.0);
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
                        .max_hp(max_hp)
                        .hl_type(hl_type)
                        .regenerated_hp_part(regenerated_hp_part)
                        .thickness(thickness)
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
        .build()
}

impl GameMetadataProvider {
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

        let params_dict = if let Some(params_dict) = pickled_params.dict_or_object_dict() {
            let inner = params_dict.inner();
            if let Some(nested) = inner.get(&pk("")) {
                // Modern format: {"": {param_name: param_data, ...}}
                nested.dict_or_object_dict().expect("game params is not a dict").clone()
            } else {
                // Old format: {param_name: param_data, ...} (flat, no wrapper key)
                params_dict.clone()
            }
        } else if let Some(params_list) = pickled_params.list_ref() {
            let params = &params_list.inner()[0];
            params.dict_or_object_dict().expect("First element of GameParams list is not a dictionary").clone()
        } else if let Some(params_tuple) = pickled_params.tuple_ref() {
            let inner = params_tuple.inner();
            let params = &inner[0];
            params.dict_or_object_dict().expect("First element of GameParams tuple is not a dictionary").clone()
        } else {
            panic!("Root game params is not a dict, list, or tuple");
        };

        let new_params = params_dict
            .inner()
            .values()
            .filter_map(|param| {
                // Wrap each param in catch_unwind so missing fields in old game
                // versions skip the individual param rather than aborting everything.
                let param = param.clone();
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    Self::parse_single_param(&param, &params_dict)
                }))
                .unwrap_or_default()
            })
            .collect::<Vec<Param>>();

        Ok(new_params)
    }

    /// Parse a single param from pickled dict data. Panics on missing fields are
    /// caught by the caller's `catch_unwind`.
    fn parse_single_param(param: &Value, params_dict: &Shared<BTreeMap<HashableValue, Value>>) -> Option<Param> {
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
                let skills = skills.map(|skills| build_crew_skills(&skills.inner()));

                Some(ParamData::Crew(
                    Crew::builder()
                        .money_training_level(money_training_level)
                        .personality(crew_personality)
                        .maybe_skills(skills)
                        .build(),
                ))
            }
            ParamType::Achievement => {
                let is_group = game_param_to_type!(param_data, "group", Option<bool>).unwrap_or_default();
                let one_per_battle = game_param_to_type!(param_data, "onePerBattle", Option<bool>).unwrap_or_default();
                let ui_type = game_param_to_type!(param_data, "uiType", Option<String>).unwrap_or_default();
                let ui_name = game_param_to_type!(param_data, "uiName", Option<String>).unwrap_or_default();

                Some(ParamData::Achievement(
                    Achievement::builder()
                        .is_group(is_group)
                        .one_per_battle(one_per_battle)
                        .ui_type(ui_type)
                        .ui_name(ui_name)
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
                Some(ParamData::Exterior(
                    Exterior::builder().maybe_camouflage(camouflage).maybe_title(title).modifiers(modifiers).build(),
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
                Some(ParamData::Modernization(super::types::Modernization::new(
                    modifiers,
                    slot,
                    ship_levels,
                    ship_types,
                    nations,
                    groups,
                    ships,
                    excludes,
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
            ParamType::Drop => {
                let marker_name_active = param_data
                    .get(&pk("markerNameActive"))
                    .and_then(|v| v.string_ref())
                    .map(|s| s.inner().to_string())
                    .unwrap_or_default();
                let marker_name_inactive = param_data
                    .get(&pk("markerNameInactive"))
                    .and_then(|v| v.string_ref())
                    .map(|s| s.inner().to_string())
                    .unwrap_or_default();
                let sorting = param_data.get(&pk("sorting")).and_then(|v| v.i64_ref()).copied().unwrap_or(0);
                Some(ParamData::Drop(
                    super::types::BuffDrop::builder()
                        .marker_name_active(marker_name_active)
                        .marker_name_inactive(marker_name_inactive)
                        .sorting(sorting)
                        .build(),
                ))
            }
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
                // Resolve ammo type: bombName -> projectile dict -> ammoType
                let ammo_type = param_data
                    .get(&pk("bombName"))
                    .and_then(|v| v.string_ref())
                    .filter(|s| !s.inner().is_empty())
                    .and_then(|bomb_name| {
                        params_dict
                            .inner()
                            .get(&HashableValue::String(bomb_name.clone()))
                            .and_then(|proj| proj.dict_or_object_dict())
                            .and_then(|proj_dict| {
                                proj_dict
                                    .inner()
                                    .get(&pk("ammoType"))
                                    .and_then(|v| v.string_ref())
                                    .map(|s| s.inner().to_string())
                            })
                    })
                    .unwrap_or_default();
                Some(ParamData::Aircraft(Aircraft::builder().category(category).ammo_type(ammo_type).build()))
            }
            ParamType::Projectile => {
                let ammo_type = param_data
                    .get(&pk("ammoType"))
                    .and_then(|v| v.string_ref())
                    .map(|s| s.inner().to_string())
                    .unwrap_or_default();
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
                let bullet_air_drag = read_opt_f32("bulletAirDrag");
                Some(ParamData::Projectile(
                    Projectile::builder()
                        .ammo_type(ammo_type)
                        .maybe_max_dist(max_dist)
                        .maybe_bullet_diametr(bullet_diametr)
                        .maybe_bullet_mass(bullet_mass)
                        .maybe_bullet_speed(bullet_speed)
                        .maybe_bullet_krupp(bullet_krupp)
                        .maybe_bullet_cap(bullet_cap)
                        .maybe_bullet_cap_normalize_max_angle(bullet_cap_normalize_max_angle)
                        .maybe_bullet_detonator(bullet_detonator)
                        .maybe_bullet_detonator_threshold(bullet_detonator_threshold)
                        .maybe_bullet_ricochet_at(bullet_ricochet_at)
                        .maybe_bullet_always_ricochet_at(bullet_always_ricochet_at)
                        .maybe_alpha_piercing_he(alpha_piercing_he)
                        .maybe_alpha_piercing_cs(alpha_piercing_cs)
                        .maybe_alpha_damage(alpha_damage)
                        .maybe_burn_prob(burn_prob)
                        .maybe_bullet_air_drag(bullet_air_drag)
                        .build(),
                ))
            }
            ParamType::Building => {
                let level = game_param_to_type!(param_data, "level", u32);
                let hull = param_data.get(&pk("hull")).and_then(|v| v.dict_or_object_dict());
                let health = hull
                    .as_ref()
                    .and_then(|h| {
                        let inner = h.inner();
                        inner
                            .get(&pk("health"))
                            .and_then(|v| v.f64_ref().map(|f| *f as f32).or_else(|| v.i64_ref().map(|i| *i as f32)))
                    })
                    .unwrap_or(0.0);
                Some(ParamData::Building(super::types::Building::builder().level(level).health(health).build()))
            }
            _ => {
                // Some params (e.g. Drops) have typeinfo.type = "Other"
                // but contain Drop-specific fields
                if param_data.contains_key(&pk("markerNameActive")) {
                    let marker_name_active = param_data
                        .get(&pk("markerNameActive"))
                        .and_then(|v| v.string_ref())
                        .map(|s| s.inner().to_string())
                        .unwrap_or_default();
                    let marker_name_inactive = param_data
                        .get(&pk("markerNameInactive"))
                        .and_then(|v| v.string_ref())
                        .map(|s| s.inner().to_string())
                        .unwrap_or_default();
                    let sorting = param_data.get(&pk("sorting")).and_then(|v| v.i64_ref()).copied().unwrap_or(0);
                    Some(ParamData::Drop(
                        super::types::BuffDrop::builder()
                            .marker_name_active(marker_name_active)
                            .marker_name_inactive(marker_name_inactive)
                            .sorting(sorting)
                            .build(),
                    ))
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
            specs,
        })
    }

    pub fn set_translations(&self, catalog: Catalog) {
        *self.translations.write().expect("translations lock poisoned") = Some(catalog);
    }

    /// Look up a translation key in the catalog, returning `None` when the
    /// catalog is absent or gettext returns the key unchanged (= not found).
    fn translate(&self, key: &str) -> Option<String> {
        let guard = self.translations.read().ok()?;
        let catalog = guard.as_ref()?;
        let result = catalog.gettext(key);
        if result == key { None } else { Some(result.to_string()) }
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
    /// doesn't exist or isn't a projectile.
    pub fn resolve_shell_from_param_id(&self, params_id: GameParamId) -> Option<ShellInfo> {
        let param = GameParamProvider::game_param_by_id(self, params_id)?;
        let projectile = param.projectile()?;
        let name = param.name().to_string();
        Some(projectile.to_shell_info(name))
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

    fn dict(entries: Vec<(HashableValue, Value)>) -> Value {
        Value::Dict(Shared::new(entries.into_iter().collect()))
    }

    #[test]
    fn read_camera_trajectories_empty_dict_returns_empty() {
        let ship_data: BTreeMap<HashableValue, Value> = BTreeMap::new();
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
        let ship_data: BTreeMap<HashableValue, Value> = [(pk("Cameras"), cameras)].into_iter().collect();
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
        let ship_data: BTreeMap<HashableValue, Value> = [(pk("Cameras"), cameras)].into_iter().collect();
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
        let ship_data: BTreeMap<HashableValue, Value> = [(pk("Cameras"), cameras)].into_iter().collect();
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
        let ship_data: BTreeMap<HashableValue, Value> = [(pk("Cameras"), cameras)].into_iter().collect();
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
        let ship_data: BTreeMap<HashableValue, Value> = [(pk("Cameras"), cameras)].into_iter().collect();
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
        let ship_data: BTreeMap<HashableValue, Value> = [(pk("Cameras"), cameras)].into_iter().collect();
        let result = read_camera_trajectories(&ship_data);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "RealMode");
    }

    #[test]
    fn build_ability_category_collects_effect_fields_logic_wins() {
        let logic = dict(vec![
            (pk("regenerationHPSpeed"), fv(0.005)),
            // collides with the top-level reloadTime; logic must win.
            (pk("reloadTime"), fv(99.0)),
            // collides with tacticalParams; tacticalParams must win.
            (pk("workRange"), fv(7.0)),
        ]);
        let tactical = dict(vec![(pk("workRange"), fv(1000.0)), (pk("aimRange"), fv(11.0))]);
        let category_data: BTreeMap<HashableValue, Value> = vec![
            (pk("reloadTime"), fv(40.0)),
            (pk("workTime"), Value::I64(28)),
            (pk("numConsumables"), Value::I64(3)),
            (pk("consumableType"), sv("regenCrew")),
            (pk("logic"), logic),
            (pk("tacticalParams"), tactical),
        ]
        .into_iter()
        .collect();

        let cat = build_ability_category(&category_data);
        let fields = cat.effect_fields();

        assert_eq!(fields.get("workTime"), Some(&28.0_f32));
        assert_eq!(fields.get("numConsumables"), Some(&3.0_f32));
        assert_eq!(fields.get("regenerationHPSpeed"), Some(&0.005_f32));
        // logic value overrides the top-level on collision.
        assert_eq!(fields.get("reloadTime"), Some(&99.0_f32));
        // tacticalParams numeric fields are merged in.
        assert_eq!(fields.get("aimRange"), Some(&11.0_f32));
        // tacticalParams overrides logic on collision.
        assert_eq!(fields.get("workRange"), Some(&1000.0_f32));
        // non-numeric string fields are skipped.
        assert!(!fields.contains_key("consumableType"));
    }
}
