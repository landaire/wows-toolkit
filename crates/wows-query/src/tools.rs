//! One definition of every query tool for LLM tool calling: name, description,
//! JSON input schema and a dispatcher taking JSON arguments.
//!
//! `replay_summary` is not registered because it takes file bytes, which each
//! front end obtains its own way.

use schemars::JsonSchema;
use schemars::Schema;
use schemars::generate::SchemaSettings;
use schemars::transform::RecursiveTransform;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Map;
use serde_json::Value;

use crate::GameData;
use crate::QueryError;
use crate::consumables::ShipConsumablesArgs;
use crate::gameparams::GameParamsQueryArgs;
use crate::penetration::PenetrationSearchArgs;
use crate::ships::FindShipsArgs;
use crate::ships::ShipArgs;
use crate::skills::CaptainSkillArgs;
use crate::skills::SkillTreeArgs;
use crate::stats::ShipStatsArgs;
use crate::upgrades::UpgradeArgs;

/// A query tool as presented to a model.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    /// JSON Schema of the arguments object, with every subschema inline.
    pub input_schema: Map<String, Value>,
}

/// Failure of [`GameData::call_tool`]. `Display` is written for the model
/// that made the call.
#[derive(Debug, thiserror::Error)]
pub enum ToolCallError {
    #[error("no tool named {name:?}")]
    UnknownTool { name: String },

    #[error("invalid arguments for {tool}: {source}")]
    InvalidArguments {
        tool: &'static str,
        #[source]
        source: serde_json::Error,
    },

    #[error(transparent)]
    Query(#[from] QueryError),

    #[error("failed to encode the {tool} answer")]
    Encode {
        tool: &'static str,
        #[source]
        source: serde_json::Error,
    },
}

struct Tool {
    name: &'static str,
    description: &'static str,
    schema: fn() -> Map<String, Value>,
    call: fn(&GameData, Value) -> Result<Value, CallFailure>,
}

enum CallFailure {
    Arguments(serde_json::Error),
    Query(QueryError),
    Encode(serde_json::Error),
}

const TOOLS: &[Tool] = &[
    Tool {
        name: "find_ships",
        description: "List ships by name fragment, tier, class or nation. Returns name, GameParams index, tier, class, nation and group.",
        schema: input_schema::<FindShipsArgs>,
        call: |game, arguments| run(arguments, |args| Ok(game.find_ships(args))),
    },
    Tool {
        name: "ship_overview",
        description: "Credit price and research XP of tech tree ships, the GameParams doubloon value (with a note when it is not how the ship is obtained, e.g. Armory ships sold for coal or steel), research tree predecessors and successors, group and upgrade slot count of one ship.",
        schema: input_schema::<ShipArgs>,
        call: |game, arguments| run(arguments, |args| game.ship_overview(args)),
    },
    Tool {
        name: "ship_armor",
        description: "Armor of one ship: per hull zone the thinnest and thickest plate above the waterline, and every plate by collision material (Bow_*, St_* stern, Cit_* citadel, Cas_* casemate, SS_* superstructure) with thickness in mm.",
        schema: input_schema::<ShipArgs>,
        call: |game, arguments| run(arguments, |args| game.ship_armor(args)),
    },
    Tool {
        name: "ship_shells",
        description: "Main and secondary battery shells of one ship: ammo type, caliber, HE/SAP penetration with and without Inertia Fuse for HE Shells, the thickest whole-mm plate AP overmatches, alpha damage, muzzle velocity and fire chance.",
        schema: input_schema::<ShipArgs>,
        call: |game, arguments| run(arguments, |args| game.ship_shells(args)),
    },
    Tool {
        name: "penetration_search",
        description: "Find ships whose shells defeat a plate outright: HE or SAP penetration at least the thickness, or AP whose caliber overmatches it (caliber > 14.3 x thickness). Give thickness_mm, or target_ship with zone. Without ammo, returns separate HE, SAP and AP overmatch groups. With a target ship and no tiers, searches the target's tier +-2.",
        schema: input_schema::<PenetrationSearchArgs>,
        call: |game, arguments| run(arguments, |args| game.penetration_search(args)),
    },
    Tool {
        name: "ship_stats",
        description: "Port stat card of one ship with stock modules. With captain skills and upgrades, lists the stats they change, from and to, and which skill or upgrade caused each change.",
        schema: input_schema::<ShipStatsArgs>,
        call: |game, arguments| run(arguments, |args| game.ship_stats(args)),
    },
    Tool {
        name: "captain_skill",
        description: "One captain skill by name or acronym (IFHE, SI, CE, AR, DE, BoS): description, point cost and effects per ship class.",
        schema: input_schema::<CaptainSkillArgs>,
        call: |game, arguments| run(arguments, |args| game.captain_skill(args)),
    },
    Tool {
        name: "skill_tree",
        description: "The captain skill tree of one ship class, skill names by point cost.",
        schema: input_schema::<SkillTreeArgs>,
        call: |game, arguments| run(arguments, |args| Ok(game.skill_tree(args))),
    },
    Tool {
        name: "upgrade",
        description: "One upgrade (modernization) by name: slot, kind, credit price, effects per class and which tiers, nations and ships it fits.",
        schema: input_schema::<UpgradeArgs>,
        call: |game, arguments| run(arguments, |args| game.upgrade(args)),
    },
    Tool {
        name: "ship_upgrades",
        description: "Every upgrade that fits one ship, by slot, with price and effects.",
        schema: input_schema::<ShipArgs>,
        call: |game, arguments| run(arguments, |args| game.ship_upgrades(args)),
    },
    Tool {
        name: "ship_consumables",
        description: "Consumables of one ship by slot with charges, reload, action time, detection ranges and heal rate, optionally with captain skills and upgrades applied.",
        schema: input_schema::<ShipConsumablesArgs>,
        call: |game, arguments| run(arguments, |args| game.ship_consumables(args)),
    },
    Tool {
        name: "gameparams_query",
        description: "Run a jq filter (jaq dialect) over raw GameParams entries, for values the other tools do not report. select is one entry {\"param\": \"PASC020\"} (name, index or id), one ship {\"ship\": \"Des Moines\"}, or a set {\"set\": {\"type\": \"Ship\", \"nation\": \"USA\", \"species\": \"Cruiser\", \"tier\": 10}} of at most limit entries. The filter runs once per entry with the entry object as input; entries without output are omitted. Entries are large: start with `keys`, then `.Field | keys`, then pick fields. Output is cut to max_bytes; when truncated, hint says how to narrow the filter. The filter has 5 seconds; env is not available.",
        schema: input_schema::<GameParamsQueryArgs>,
        call: |game, arguments| run(arguments, |args| game.gameparams_query(args)),
    },
];

impl GameData {
    /// Runs the tool `name` with JSON `arguments` and returns its JSON answer.
    pub fn call_tool(&self, name: &str, arguments: Value) -> Result<Value, ToolCallError> {
        let Some(tool) = TOOLS.iter().find(|tool| tool.name == name) else {
            return Err(ToolCallError::UnknownTool { name: name.to_owned() });
        };

        (tool.call)(self, arguments).map_err(|failure| match failure {
            CallFailure::Arguments(source) => ToolCallError::InvalidArguments { tool: tool.name, source },
            CallFailure::Query(error) => ToolCallError::Query(error),
            CallFailure::Encode(source) => ToolCallError::Encode { tool: tool.name, source },
        })
    }
}

/// Every registered tool, in a stable order.
pub fn specs() -> Vec<ToolSpec> {
    TOOLS
        .iter()
        .map(|tool| ToolSpec { name: tool.name, description: tool.description, input_schema: (tool.schema)() })
        .collect()
}

/// The input schema of an arguments type: inline subschemas, no `$schema`,
/// title, root description or format annotations.
pub fn input_schema<A>() -> Map<String, Value>
where
    A: JsonSchema,
{
    let generator = SchemaSettings::draft2020_12()
        .with(|settings| {
            settings.inline_subschemas = true;
            settings.meta_schema = None;
        })
        .with_transform(RecursiveTransform(simplify))
        .into_generator();
    let mut root = generator.into_root_schema_for::<A>();
    root.remove("title");
    root.remove("description");

    match root.to_value() {
        Value::Object(object) => object,
        // `true` and `false` are the only non-object schemas; argument types
        // are structs, which always produce an object.
        other => unreachable!("argument type has a boolean schema: {other}"),
    }
}

/// Closes objects to unknown properties and drops the `null` alternative of
/// optional fields: an absent field already means "not given", and small
/// models follow single-type schemas more reliably.
fn simplify(schema: &mut Schema) {
    let is_null = |variant: &Value| variant.get("type").is_some_and(|kind| kind == "null");
    if let Some(Value::Array(variants)) = schema.get("anyOf")
        && let [first, second] = variants.as_slice()
        && let Some(Value::Object(variant)) = match (is_null(first), is_null(second)) {
            (false, true) => Some(first.clone()),
            (true, false) => Some(second.clone()),
            _ => None,
        }
        && let Some(object) = schema.as_object_mut()
    {
        object.remove("anyOf");
        // The field's own description takes precedence over its type's.
        for (key, value) in variant {
            object.entry(key).or_insert(value);
        }
    }

    schema.remove("format");

    if schema.get("properties").is_some() {
        schema.insert("additionalProperties".to_owned(), Value::Bool(false));
    }

    if let Some(Value::Array(types)) = schema.get_mut("type") {
        types.retain(|kind| kind != "null");
        if let [single] = types.as_slice() {
            let single = single.clone();
            schema.insert("type".to_owned(), single);
        }
    }

    if let Some(Value::Array(values)) = schema.get_mut("enum") {
        values.retain(|value| !value.is_null());
    }
}

fn run<A, T>(arguments: Value, query: impl FnOnce(&A) -> Result<T, QueryError>) -> Result<Value, CallFailure>
where
    A: DeserializeOwned,
    T: Serialize,
{
    let args: A = serde_json::from_value(arguments).map_err(CallFailure::Arguments)?;
    let output = query(&args).map_err(CallFailure::Query)?;

    encode(&output).map_err(CallFailure::Encode)
}

/// Encodes through JSON text because `serde_json::to_value` widens `f32` to
/// `f64`, which prints 350.8 as 350.79998779296875. The text writer prints
/// each float at the shortest length that reads back as the same value of
/// its own type.
fn encode<T: Serialize>(output: &T) -> Result<Value, serde_json::Error> {
    serde_json::from_slice(&serde_json::to_vec(output)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_encode_at_their_own_precision() {
        #[derive(Serialize)]
        struct Output {
            single: f32,
            double: f64,
        }

        let value = encode(&Output { single: 350.8, double: 0.1 }).unwrap();
        assert_eq!(value.to_string(), r#"{"single":350.8,"double":0.1}"#);
    }

    #[test]
    fn tool_specs() {
        insta::assert_yaml_snapshot!(specs());
    }

    #[test]
    fn tool_names_are_unique() {
        let mut names: Vec<_> = TOOLS.iter().map(|tool| tool.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), TOOLS.len());
    }
}
