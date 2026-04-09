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
use crate::data::DataFileWithCallback;
use crate::data::ResourceLoader;
use crate::error::GameDataError;
use crate::game_params::convert::game_params_to_pickle;
use crate::game_types::GameParamId;
use crate::rpc::entitydefs::EntitySpec;
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

    // Hashmaps that may fail to resolve
    ($params:ident, $key:expr, Option<HashMap<(), ()>>) => {
        if
        $params
            .get(&HashableValue::String($key.to_string().into()))
            .unwrap_or_else(|| panic!("could not get {}", $key))
            .is_none() {
                None
            } else {
                Some(game_param_to_type!($params, $key, HashMap<(), ()>))
            }
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

/// TODO: Too many unpredictable schema differences >:(
/// Need to just create structs for everything.
fn build_skill_modifiers(modifiers: &BTreeMap<HashableValue, Value>) -> Vec<CrewSkillModifier> {
    modifiers
        .iter()
        .filter_map(|(modifier_name, modifier_data)| {
            let modifier_name = modifier_name.string_ref().expect("modifier name is not a string").to_owned();

            let modifier_name = modifier_name.inner();

            let modifier = if let Some(common_value) = modifier_data.i64_ref().cloned() {
                let common_value = common_value as f32;
                CrewSkillModifier::builder()
                    .aircraft_carrier(common_value)
                    .auxiliary(common_value)
                    .battleship(common_value)
                    .cruiser(common_value)
                    .destroyer(common_value)
                    .submarine(common_value)
                    .name(modifier_name.to_owned())
                    .build()
            } else if let Some(common_value) = modifier_data.f64_ref().cloned() {
                let common_value = common_value as f32;
                CrewSkillModifier::builder()
                    .aircraft_carrier(common_value)
                    .auxiliary(common_value)
                    .battleship(common_value)
                    .cruiser(common_value)
                    .destroyer(common_value)
                    .submarine(common_value)
                    .name(modifier_name.to_owned())
                    .build()
            } else if let Some(modifier_data) = modifier_data.dict_or_object_dict() {
                let modifier_data = modifier_data.inner();

                // Skip dicts that aren't per-species modifier dicts
                modifier_data.get(&HashableValue::String("AirCarrier".to_owned().into()))?;

                let read_species = |key: &str| -> f32 {
                    modifier_data
                        .get(&HashableValue::String(key.to_owned().into()))
                        .and_then(|v| v.f64_ref().map(|f| *f as f32).or_else(|| v.i64_ref().map(|i| *i as f32)))
                        .unwrap_or(1.0)
                };

                CrewSkillModifier::builder()
                    .aircraft_carrier(read_species("AirCarrier"))
                    .auxiliary(read_species("Auxiliary"))
                    .battleship(read_species("Battleship"))
                    .cruiser(read_species("Cruiser"))
                    .destroyer(read_species("Destroyer"))
                    .submarine(read_species("Submarine"))
                    .name(modifier_name.to_owned())
                    .build()
            } else {
                // Skip non-numeric, non-dict modifiers (bools, arrays, etc.)
                return None;
            };

            Some(modifier)
        })
        .collect()
}

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

            let logic_modifiers = game_param_to_type!(skill_data, "modifiers", Option<HashMap<(), ()>>);

            let logic_modifiers = logic_modifiers.map(|modifiers| build_skill_modifiers(&modifiers.inner()));

            let logic_trigger_data = game_param_to_type!(skill_data, "LogicTrigger", Option<HashMap<(), ()>>);

            let logic_trigger = logic_trigger_data.map(|logic_trigger_data| {
                let logic_trigger_data = logic_trigger_data.inner();
                CrewSkillLogicTrigger::builder()
                    .maybe_burn_count(game_param_to_type!(logic_trigger_data, "burnCount", Option<usize>))
                    .change_priority_target_penalty(game_param_to_type!(
                        logic_trigger_data,
                        "changePriorityTargetPenalty",
                        f32
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
                    .maybe_modifiers(logic_modifiers)
                    .trigger_desc_ids(game_param_to_type!(logic_trigger_data, "triggerDescIds", String))
                    .trigger_type(game_param_to_type!(logic_trigger_data, "triggerType", String))
                    .build()
            });

            let modifiers = game_param_to_type!(skill_data, "modifiers", Option<HashMap<(), ()>>);

            let modifiers = modifiers.map(|modifiers| build_skill_modifiers(&modifiers.inner()));

            let tier_data = game_param_to_type!(skill_data, "tier", HashMap<(), ()>);
            let tier_data = tier_data.inner();
            let tier = CrewSkillTiers::builder()
                .aircraft_carrier(game_param_to_type!(tier_data, "AirCarrier", usize))
                .auxiliary(game_param_to_type!(tier_data, "Auxiliary", usize))
                .battleship(game_param_to_type!(tier_data, "Battleship", usize))
                .cruiser(game_param_to_type!(tier_data, "Cruiser", usize))
                .destroyer(game_param_to_type!(tier_data, "Destroyer", usize))
                .submarine(game_param_to_type!(tier_data, "Submarine", usize))
                .build();

            Some(
                CrewSkill::builder()
                    .internal_name(skill_name.to_owned())
                    .can_be_learned(game_param_to_type!(skill_data, "canBeLearned", bool))
                    .is_epic(game_param_to_type!(skill_data, "isEpic", bool))
                    .skill_type(game_param_to_type!(skill_data, "skillType", usize))
                    .ui_treat_as_trigger(game_param_to_type!(skill_data, "uiTreatAsTrigger", bool))
                    .tier(tier)
                    .maybe_modifiers(modifiers)
                    .maybe_logic_trigger(logic_trigger)
                    .build(),
            )
        })
        .collect()
}

fn build_crew_personality(personality: &BTreeMap<HashableValue, Value>) -> CrewPersonality {
    let ships = game_param_to_type!(personality, "ships", HashMap<(), ()>);
    let ships = ships.inner();
    let ships = CrewPersonalityShips::builder()
        .groups(
            game_param_to_type!(ships, "groups", &[()])
                .inner()
                .iter()
                .map(|value| value.string_ref().expect("group entry is not a string").inner().to_owned())
                .collect(),
        )
        .nation(
            game_param_to_type!(ships, "nation", &[()])
                .inner()
                .iter()
                .map(|value| value.string_ref().expect("nation entry is not a string").inner().to_owned())
                .collect(),
        )
        .peculiarity(
            game_param_to_type!(ships, "peculiarity", &[()])
                .inner()
                .iter()
                .map(|value| value.string_ref().expect("peculiarity entry is not a string").inner().to_owned())
                .collect(),
        )
        .ships(
            game_param_to_type!(ships, "ships", &[()])
                .inner()
                .iter()
                .map(|value| value.string_ref().expect("ships entry is not a string").inner().to_owned())
                .collect(),
        )
        .build();

    CrewPersonality::builder()
        .can_reset_skills_for_free(game_param_to_type!(personality, "canResetSkillsForFree", bool))
        .cost_credits(game_param_to_type!(personality, "costCR", usize))
        .cost_elite_xp(game_param_to_type!(personality, "costELXP", usize))
        .cost_gold(game_param_to_type!(personality, "costGold", usize))
        .cost_xp(game_param_to_type!(personality, "costXP", usize))
        .has_custom_background(
            personality
                .get(&HashableValue::String("hasCustomBackground".to_string().into()))
                .and_then(|v| v.bool_ref().copied())
                .unwrap_or(false),
        )
        .has_overlay(
            personality
                .get(&HashableValue::String("hasOverlay".to_string().into()))
                .and_then(|v| v.bool_ref().copied())
                .unwrap_or(false),
        )
        .has_rank(
            personality
                .get(&HashableValue::String("hasRank".to_string().into()))
                .and_then(|v| v.bool_ref().copied())
                .unwrap_or(false),
        )
        .has_sample_voiceover(
            personality
                .get(&HashableValue::String("hasSampleVO".to_string().into()))
                .and_then(|v| v.bool_ref().copied())
                .unwrap_or(false),
        )
        .is_animated(game_param_to_type!(personality, "isAnimated", bool))
        .is_person(game_param_to_type!(personality, "isPerson", bool))
        .is_retrainable(game_param_to_type!(personality, "isRetrainable", bool))
        .is_unique(game_param_to_type!(personality, "isUnique", bool))
        .peculiarity(game_param_to_type!(personality, "peculiarity", String))
        .permissions(game_param_to_type!(personality, "permissions", u32))
        .person_name(game_param_to_type!(personality, "personName", String))
        .subnation(game_param_to_type!(personality, "subnation", String))
        .tags(
            game_param_to_type!(personality, "tags", &[()])
                .inner()
                .iter()
                .map(|value| value.string_ref().expect("peculiarity entry is not a string").inner().to_owned())
                .collect(),
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

    // Extract detection radius fields from "logic" sub-object
    let logic =
        category_data.get(&HashableValue::String("logic".to_owned().into())).and_then(|v| v.dict_or_object_dict());

    let dist_ship = logic.as_ref().and_then(|l| {
        l.inner()
            .get(&HashableValue::String("distShip".to_owned().into()))
            .and_then(|v| v.f64_ref().map(|f| *f as f32).or_else(|| v.i64_ref().map(|i| *i as f32)))
            .map(BigWorldDistance::from)
    });

    let dist_torpedo = logic.as_ref().and_then(|l| {
        l.inner()
            .get(&HashableValue::String("distTorpedo".to_owned().into()))
            .and_then(|v| v.f64_ref().map(|f| *f as f32).or_else(|| v.i64_ref().map(|i| *i as f32)))
            .map(BigWorldDistance::from)
    });

    let hydrophone_wave_radius = logic.as_ref().and_then(|l| {
        l.inner()
            .get(&HashableValue::String("hydrophoneWaveRadius".to_owned().into()))
            .and_then(|v| v.f64_ref().map(|f| *f as f32).or_else(|| v.i64_ref().map(|i| *i as f32)))
            .map(Meters::from)
    });

    let patrol_radius = logic.as_ref().and_then(|l| {
        l.inner()
            .get(&HashableValue::String("radius".to_owned().into()))
            .and_then(|v| v.f64_ref().map(|f| *f as f32).or_else(|| v.i64_ref().map(|i| *i as f32)))
            .map(BigWorldDistance::from)
    });

    AbilityCategory::builder()
        .maybe_special_sound_id(game_param_to_type!(category_data, "SpecialSoundID", Option<String>))
        .consumable_type(game_param_to_type!(category_data, "consumableType", String))
        .description_id(game_param_to_type!(category_data, "descIDs", String))
        .group(game_param_to_type!(category_data, "group", String))
        .icon_id(game_param_to_type!(category_data, "iconIDs", String))
        .num_consumables(game_param_to_type!(category_data, "numConsumables", isize))
        .preparation_time(game_param_to_type!(category_data, "preparationTime", f32))
        .reload_time(reload_time)
        .title_id(game_param_to_type!(category_data, "titleIDs", String))
        .work_time(work_time)
        .maybe_dist_ship(dist_ship)
        .maybe_dist_torpedo(dist_torpedo)
        .maybe_hydrophone_wave_radius(hydrophone_wave_radius)
        .maybe_patrol_radius(patrol_radius)
        .build()
}

fn build_ability(ability_data: &BTreeMap<HashableValue, Value>) -> Ability {
    let test_key = HashableValue::String("numConsumables".to_string().into());
    let categories: HashMap<String, AbilityCategory> =
        HashMap::from_iter(ability_data.iter().filter_map(|(key, value)| {
            if value.is_not_dict() {
                return None;
            }

            let value = value.dict_or_object_dict().unwrap();
            let value = value.inner();
            if value.contains_key(&test_key) {
                Some((key.string_ref().unwrap().inner().to_owned(), build_ability_category(&value)))
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

/// Helper: read a pickled dict string key.
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
    let mut max_main_battery_m: Option<Meters> = None;
    let mut max_secondary_battery_m: Option<Meters> = None;

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

        // Collect secondary battery maxDist from _Hull upgrades
        if uc_type == keys::UC_TYPE_HULL
            && let Some(atba_comp) = components.inner().get(&pk(keys::COMP_ATBA)).and_then(read_first_string)
            && let Some(atba_data) = ship_data.get(&pk(&atba_comp)).and_then(|v| v.dict_or_object_dict())
            && let Some(m) = read_float(&atba_data.inner(), keys::MAX_DIST).map(Meters::from)
        {
            max_secondary_battery_m = Some(match max_secondary_battery_m {
                Some(prev) if prev.value() >= m.value() => prev,
                _ => m,
            });
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

    let config_data = if hull_upgrades.is_empty()
        && torpedo_ammo.is_empty()
        && main_battery_ammo.is_empty()
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
        .build()
}

impl GameMetadataProvider {
    /// Loads game metadata directly from game files. This operation is fairly expensive
    /// considering `GameParams.data` must be deserialized and converted to a strongly-typed
    /// representation.
    ///
    /// See [`GameMetadataProvider::from_params`] if you wish to use caching.
    pub fn from_vfs(vfs: &vfs::VfsPath) -> Result<GameMetadataProvider, GameDataError> {
        debug!("deserializing gameparams");

        let mut game_params_data = Vec::new();
        vfs.join("content/GameParams.data")?.open_file()?.read_to_end(&mut game_params_data)?;

        let pickled_params: Value = game_params_to_pickle(game_params_data)?;

        let params_dict = if let Some(params_dict) = pickled_params.dict_or_object_dict() {
            let params_dict = params_dict.inner();
            params_dict
                .get(&pk(""))
                .expect("failed to get default game_params")
                .dict_or_object_dict()
                .expect("game params is not a dict")
                .clone()
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
                if param.is_none() {
                    return None;
                }

                let param_data =
                    param.dict_or_object_dict().expect("Params root level dictionary values are not dictionaries");
                let param_data = param_data.inner();

                param_data
                    .get(&pk(keys::TYPEINFO))
                    .and_then(|type_info| {
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
                    })
                    .and_then(|(nation, species, typ)| {
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
                                let is_group = game_param_to_type!(param_data, "group", bool);
                                let one_per_battle = game_param_to_type!(param_data, "onePerBattle", bool);
                                let ui_type = game_param_to_type!(param_data, "uiType", String);
                                let ui_name = game_param_to_type!(param_data, "uiName", String);

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
                                Some(ParamData::Exterior(
                                    Exterior::builder().maybe_camouflage(camouflage).maybe_title(title).build(),
                                ))
                            }
                            ParamType::Modernization => {
                                let modifiers = param_data
                                    .get(&pk("modifiers"))
                                    .and_then(|v| v.dict_or_object_dict())
                                    .map(|d| build_skill_modifiers(&d.inner()))
                                    .unwrap_or_default();
                                Some(ParamData::Modernization(super::types::Modernization::new(modifiers)))
                            }
                            ParamType::Unit => Some(ParamData::Unit),
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
                                let sorting =
                                    param_data.get(&pk("sorting")).and_then(|v| v.i64_ref()).copied().unwrap_or(0);
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
                                Some(ParamData::Aircraft(
                                    Aircraft::builder().category(category).ammo_type(ammo_type).build(),
                                ))
                            }
                            ParamType::Projectile => {
                                let ammo_type = param_data
                                    .get(&pk("ammoType"))
                                    .and_then(|v| v.string_ref())
                                    .map(|s| s.inner().to_string())
                                    .unwrap_or_default();
                                let max_dist = param_data
                                    .get(&pk(keys::MAX_DIST))
                                    .and_then(|v| {
                                        v.f64_ref().map(|f| *f as f32).or_else(|| v.i64_ref().map(|i| *i as f32))
                                    })
                                    .map(BigWorldDistance::from);
                                let read_opt_f32 = |key: &str| -> Option<f32> {
                                    param_data.get(&pk(key)).and_then(|v| {
                                        v.f64_ref().map(|f| *f as f32).or_else(|| v.i64_ref().map(|i| *i as f32))
                                    })
                                };
                                let read_opt_bool = |key: &str| -> Option<bool> {
                                    param_data.get(&pk(key)).and_then(|v| v.bool_ref().copied())
                                };
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
                                        inner.get(&pk("health")).and_then(|v| {
                                            v.f64_ref().map(|f| *f as f32).or_else(|| v.i64_ref().map(|i| *i as f32))
                                        })
                                    })
                                    .unwrap_or(0.0);
                                Some(ParamData::Building(
                                    super::types::Building::builder().level(level).health(health).build(),
                                ))
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
                                    let sorting =
                                        param_data.get(&pk("sorting")).and_then(|v| v.i64_ref()).copied().unwrap_or(0);
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
                    })
            })
            .collect::<Vec<Param>>();

        let params = new_params;

        Self::from_params_with_vfs(params, vfs)
    }

    /// Constructs a GameMetadataProvider from a pre-built list of GameParams and a VFS.
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

        let specs = Arc::new(parse_scripts(&data_file_loader).unwrap());

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
