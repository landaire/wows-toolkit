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
use crate::aircraft::ShipAircraftArgs;
use crate::armor_assessment::ArmorAssessmentArgs;
use crate::armor_trace::ArmorTraceArgs;
use crate::citadel::CitadelProfileArgs;
use crate::consumables::ShipConsumablesArgs;
use crate::firing_arcs::FiringArcsArgs;
use crate::gameparams::GameParamsQueryArgs;
use crate::penetration::PenetrationSearchArgs;
use crate::ships::FindShipsArgs;
use crate::ships::ShipArgs;
use crate::ships::ShipOverviewArgs;
use crate::skills::CaptainSkillArgs;
use crate::skills::SkillTreeArgs;
use crate::stat_search::StatNamesArgs;
use crate::stat_search::StatSearchArgs;
use crate::stats::ShipStatsArgs;
use crate::tech_tree::TechTreeLinesArgs;
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

/// A tool failure as data: the message plus the names a host can offer
/// instead, so it never parses the message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ToolErrorDetails {
    pub message: String,

    /// Names an ambiguous query matched equally; ask which one was meant.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<String>,

    /// Names near one that matched nothing.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub suggestions: Vec<String>,
}

impl ToolCallError {
    pub fn details(&self) -> ToolErrorDetails {
        let (candidates, suggestions) = match self {
            Self::Query(error) => (error.candidates().to_vec(), error.suggestions().to_vec()),
            _ => (Vec::new(), Vec::new()),
        };
        ToolErrorDetails { message: self.to_string(), candidates, suggestions }
    }
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
        description: "List ships by name, tier, class or nation, including ships in closed testing (status \"test\", counted in test). Returns name, GameParams index, tier, class, nation, group and status. With a name, a ship it names as the single-ship tools read it (exact name, alias such as \"DM\", initials or former name) comes first with name_match \"named\"; ships whose names merely contain it follow with name_match \"partial\".",
        schema: input_schema::<FindShipsArgs>,
        call: |game, arguments| run(arguments, |args| Ok(game.find_ships(args))),
    },
    Tool {
        name: "ship_overview",
        description: "Port purchase price in credits (purchase_credits) and research XP of tech tree ships, a note on how ships are obtained when price and research fields do not tell it (e.g. Armory ships sold for coal or steel, prices not in game data, or tier 11 ships, which cost no XP), research tree predecessors and successors, group and upgrade slot count of one ship, and its armament: main and secondary battery mounts with barrels and caliber (e.g. \"2 x 2 152 mm\"), torpedo launchers and torpedoes, depth charge launchers, aircraft squadrons and air strike (an air strike with depth_charge ordnance is anti-submarine armament), and consumables by slot. Every armament category the ship lacks is listed as \"none\". Armament is for stock modules; pass names from other_modules in modules for another hull, guns or torpedoes. other_squadrons lists squadrons of researched plane modules.",
        schema: input_schema::<ShipOverviewArgs>,
        call: |game, arguments| run(arguments, |args| game.ship_overview(args)),
    },
    Tool {
        name: "tech_tree_lines",
        description: "Tech tree lines, each the research path from tier 1 to one tier 10 ship (tier 11 ships are not part of a line), with the research XP and purchase credits of its ships in a tier range (default 8 to 10) and their totals, cheapest first by sort_by: research_xp (ties broken by credits) or purchase_credits (ties broken by XP). branches_from names the highest-tier ship the line shares with another line; shared_with on a ship lists the tier 10 ships of the other lines through it. Costs leave out module research and upgrades. Filter by nation and by class of the tier 10 ship.",
        schema: input_schema::<TechTreeLinesArgs>,
        call: |game, arguments| run(arguments, |args| game.tech_tree_lines(args)),
    },
    Tool {
        name: "ship_armor",
        description: "Armor of one ship by hull zone: plate thickness above the waterline (thickness_mm when every plate is equal, else thinnest_mm and thickest_mm), and each plate by its in-game name led by its zone, e.g. \"bow: Fore End Deck\", with thickness in mm. Plates of 0 mm, which only close a zone for hit detection, are left out.",
        schema: input_schema::<ShipArgs>,
        call: |game, arguments| run(arguments, |args| game.ship_armor(args)),
    },
    Tool {
        name: "ship_aircraft",
        description: "Squadrons of a carrier, hybrid ship or ship with an air strike: role, aircraft, squadron size, attack flight size, payload per plane, ordnance kind (bomb, skip bomb, rocket, torpedo, depth charge), ammo type, damage per projectile and HE penetration, plus air strike charges, reload and range. With thickness_mm, says whether each ordnance defeats that plate.",
        schema: input_schema::<ShipAircraftArgs>,
        call: |game, arguments| run(arguments, |args| game.ship_aircraft(args)),
    },
    Tool {
        name: "ship_shells",
        description: "Main and secondary battery shells of one ship: ammo type, caliber, HE/SAP penetration, the thickest whole-mm plate AP overmatches, alpha damage, muzzle velocity and fire chance. Each HE/SAP shell's penetration carries builds: base, best (the best-fit build, which takes Inertia Fuse for HE Shells only when it lifts the shell over its tier's armor threshold) and alternatives such as \"with Inertia Fuse for HE Shells\" with a note. tier_fillers are skills taken only to fill cheaper skill tiers; they are not conditions.",
        schema: input_schema::<ShipArgs>,
        call: |game, arguments| run(arguments, |args| game.ship_shells(args)),
    },
    Tool {
        name: "ship_concealment",
        description: "Detectability of one ship by ships, by aircraft, after firing the main battery, and after firing the main battery from smoke, each with builds: base (stock), best (best-fit modules, skills such as Concealment Expert and upgrades, listed in assumed) and alternatives (e.g. without Concealment Expert). tier_fillers are skills taken only to fill cheaper skill tiers; they are not conditions. in_smoke_evidence explains, with values per skill and upgrade, why concealment skills and upgrades do not change the in-smoke value.",
        schema: input_schema::<ShipArgs>,
        call: |game, arguments| run(arguments, |args| game.ship_concealment(args)),
    },
    Tool {
        name: "penetration_search",
        description: "Find ships whose shells defeat a plate outright: HE or SAP penetration at least the thickness, or AP whose caliber overmatches it (caliber > 14.3 x thickness). Give thickness_mm, or target_ship with zone. Without ammo, returns separate HE, SAP and AP overmatch groups. Each group gives qualifying (ships that defeat the plate, listed in ships) and considered (ships under the same filters that fire that ammo), each as total and by_class, and share_pct (qualifying as a percent of considered) and share_pct_by_class, which answers \"what percent of <class>\". With a target ship and no tiers, searches the target's tier +-2.",
        schema: input_schema::<PenetrationSearchArgs>,
        call: |game, arguments| run(arguments, |args| game.penetration_search(args)),
    },
    Tool {
        name: "armor_trace",
        description: "Trace a shooter's shell or bomb through a target ship's 3D armor model, e.g. whether Sekiryu's AP bombs can citadel Yamato or Des Moines AP can citadel a broadside cruiser at 10 km. Casts a grid of parallel rays along the line of fire (gun range and target angle, or the bomber's dive) and runs AP through every plate with the armor viewer's simulation; HE and SAP are tested against the first plate. Without ammo, traces each shell or bomb type the weapon fires or carries. Returns one trace per type: the fraction of rays over the citadel that detonate inside it, a summary per struck zone, example rays with their plates, a verdict, and the assumptions behind it.",
        schema: input_schema::<ArmorTraceArgs>,
        call: |game, arguments| run(arguments, |args| game.armor_trace(args)),
    },
    Tool {
        name: "citadel_profile",
        description: "Shape and exposure of one ship's citadel from its 3D armor model: share of the citadel face above the waterline per side, roof height, slope of the citadel plates by height band (vertical, inclined belt, sloped deck, flat deck), whether it is a turtleback, and its height along the ship. For broadside flat fire, and for a shooter's main battery AP at range_km when given, the armor in front of the citadel (effective thickness spread and the least, median and most armored plate stacks with overmatch, ricochet and fuse events, over rays without a glancing strike, with the glancing rays counted apart) and how often the citadel plate is struck past 60 deg or past the ricochet angles.",
        schema: input_schema::<CitadelProfileArgs>,
        call: |game, arguments| run(arguments, |args| game.citadel_profile(args)),
    },
    Tool {
        name: "armor_assessment",
        description: "How well one ship's armor protects it, for questions such as \"is X's armor good\" or \"is X tanky\". Traces the ship's 3D armor model against shooter groups chosen from its matchmaking tiers (the most common destroyer, light cruiser, heavy cruiser and battleship AP calibers, cruiser HE and SAP) at broadside, 30 and 45 deg angled, in close (5-10 km), moderate (12-15 km) and long (15 km and beyond) range bands; shooters that cannot reach a band are listed as excluded. Per band, group and aspect: citadel, penetration, overpenetration, ricochet, shatter and fuse-armed rates with ray counts, ranked among released ships of its class within one tier. Properties: citadel roof and floor against the waterline, share of the citadel side above water, turtleback, the citadel face above water, and plates that arm the fuse of AP that would otherwise overpenetrate. Ends with a verdict built from those numbers.",
        schema: input_schema::<ArmorAssessmentArgs>,
        call: |game, arguments| run(arguments, |args| game.armor_assessment(args)),
    },
    Tool {
        name: "ship_stats",
        description: "Port stat card of one ship with stock modules. Without skills and upgrades, build_stats gives main battery reload and range, secondary range, torpedo reload, torpedoes_per_minute, and the damage per minute and per second of each main battery ammo, secondary gun group and torpedo, each with builds: base, best-fit build (assumed) and alternatives. torpedoes_per_minute counts the tubes, and the loader groups when the game reloads only some launchers at a time (submarine groups name bow, stern or rotating), and states the limit. Damage rates are theoretical maxima: every shell or torpedo hits for full damage, without fires, flooding, citadel or armor; gun rates also give the barrels bearing broadside and bow-on. tier_fillers are skills taken only to fill cheaper skill tiers; they are not conditions. With captain skills and upgrades, lists the stats they change, from and to, and which skill or upgrade caused each change.",
        schema: input_schema::<ShipStatsArgs>,
        call: |game, arguments| run(arguments, |args| game.ship_stats(args)),
    },
    Tool {
        name: "ship_firing_arcs",
        description: "Which guns and torpedo tubes of one ship bear on a target at each bearing: barrels bearing bow-on, stern-on (within 30 degrees) and on each broadside, where all barrels bear or never, damage of one salvo per ammo type, fires per minute and fire chance per salvo assuming every shell hits, and main battery dispersion at a range. Optional skills, upgrades and modules; guns or torpedoes that fit only another hull need that hull in modules too. Optional target_ship applies the target's fire resistance.",
        schema: input_schema::<FiringArcsArgs>,
        call: |game, arguments| run(arguments, |args| game.ship_firing_arcs(args)),
    },
    Tool {
        name: "stat_names",
        description: "Every stat stat_search can compare, as \"name (unit, per item): label\". Includes armor zone plate thickness and total main battery barrels.",
        schema: input_schema::<StatNamesArgs>,
        call: |game, arguments| run(arguments, |args| Ok(game.stat_names(args))),
    },
    Tool {
        name: "stat_search",
        description: "Find ships whose stats meet every condition, e.g. [{\"stat\": \"artillery.reload_time\", \"op\": \"<\", \"value\": 5}]. Get stat names from stat_names. Lists ships that qualify stock, and ships that need modules, upgrades or captain skills, with the fewest needed. Returns counts of ships considered and qualifying, by class, and share_pct (qualifying as a percent of considered) and share_pct_by_class, which answers \"what percent of <class>\".",
        schema: input_schema::<StatSearchArgs>,
        call: |game, arguments| run(arguments, |args| game.stat_search(args)),
    },
    Tool {
        name: "captain_skill",
        description: "One captain skill by name, abbreviation (IFHE, SI, CE, SE, PT, IFA, AR, DE, BoS, RPF) or former name (BFT, AFT): description, point cost and effects per ship class, or for one ship's class with ship.",
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
        description: "Consumables of one ship by slot with charges, reload, action time, detection ranges and heal rate. Without skills and upgrades, each consumable carries builds for action time, charges, reload and smoke lifetime: base, best (consumable upgrades and skills such as Superintendent and Consumable Enhancements, listed in assumed) and alternatives. tier_fillers are skills taken only to fill cheaper skill tiers; they are not conditions. consumable narrows to one, e.g. \"radar\", \"hydro\", \"DCP\". With skills and upgrades, applies them instead.",
        schema: input_schema::<ShipConsumablesArgs>,
        call: |game, arguments| run(arguments, |args| game.ship_consumables(args)),
    },
    Tool {
        name: "gameparams_query",
        description: "Run a jq filter (jaq dialect) over raw GameParams entries, for values the other tools do not report. select is one entry {\"param\": \"PASC020\"} (name, index or id), one ship {\"ship\": \"Des Moines\"}, or a set {\"set\": {\"type\": \"Ship\", \"nation\": \"USA\", \"species\": \"Cruiser\", \"tier\": 10}} of at most limit entries. The filter runs once per entry with the entry object as input; entries without output are omitted. Entries are large: start with `keys` (no leading dot; `.keys` reads a field named keys), then `.Field | keys`, then pick fields. Output is cut to max_bytes; when truncated, hint says how to narrow the filter. The filter has 5 seconds; env is not available. A ship's costGold is a nominal value, not what the ship sells for; never quote it as a price.",
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
