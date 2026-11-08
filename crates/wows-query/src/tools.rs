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
use crate::commanders::CommanderArgs;
use crate::commanders::CommanderSearchArgs;
use crate::comparison::ShipComparisonArgs;
use crate::consumables::ShipConsumablesArgs;
use crate::firing_arcs::FiringArcsArgs;
use crate::game_modes::GameModeBuffsArgs;
use crate::gameparams::GameParamsQueryArgs;
use crate::healing::HealingSourcesArgs;
use crate::penetration::PenetrationSearchArgs;
use crate::ships::FindShipsArgs;
use crate::ships::ShipArgs;
use crate::ships::ShipOverviewArgs;
use crate::skills::CaptainSkillArgs;
use crate::skills::SkillTreeArgs;
use crate::special_abilities::SpecialAbilitySearchArgs;
use crate::stat_search::StatNamesArgs;
use crate::stat_search::StatSearchArgs;
use crate::stats::ShipStatsArgs;
use crate::tech_tree::TechTreeLinesArgs;
use crate::turtleback::TurtlebackSearchArgs;
use crate::upgrades::UniqueUpgradesArgs;
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
        description: "Tech tree lines, each the research path from tier 1 to one tier 10 ship (tier 11 ships are not part of a line), with the research XP and purchase credits of its ships in a tier range (default 8 to 10) and their totals, and the value of a Research Bureau reset of the line. A reset rolls the line back to tier 1; each tier 5 to 10 ship of the line pays Research Points once researched again and it wins a battle or earns 300 base XP in one battle: tier 5 200, tiers 6 and 7 500, tiers 8 and 9 1,500, tier 10 6,000. A line's points depend on the tiers of the ships in its path: a line with one ship at every tier from 5 to 10 pays 10,200; most carrier lines have only tiers 6, 8 and 10 and pay 8,000, the Essex line 8,500 and submarine lines 8,700. Quote each line's research_points. Carrier lines cost less in total because they have fewer ships, so compare lines by cost per point, never by raw cost. reset gives research_points with points_by_ship, regrind (research XP and purchase credits of every ship from tier 2 to 10), paying_ships (the same for the ships that pay points), credits_per_point and xp_per_point (regrind over research_points), sale_credits (half the purchase price of the tier 2 to 10 ships, paid by the reset when the player owns them all) and net_credits_if_owned. Points are base values before season multipliers. Lines are cheapest first by sort_by: credits_per_point (default; ties broken by XP per point), xp_per_point (ties broken by credits per point), research_xp or purchase_credits (tier range totals, each broken by the other). Remaining ties go by tier 10 ship name. branches_from names the highest-tier ship the line shares with another line; shared_with on a ship lists the tier 10 ships of the other lines through it. Costs leave out module research and purchases and upgrades. Filter by nation, by class of the tier 10 ship, or by the tier 10 ship itself, e.g. tier_10_ship \"Harugumo\" for the Harugumo line.",
        schema: input_schema::<TechTreeLinesArgs>,
        call: |game, arguments| run(arguments, |args| game.tech_tree_lines(args)),
    },
    Tool {
        name: "ship_armor",
        description: "Armor of one ship by hull zone: plate thickness above the waterline (thickness_mm when every plate is equal, else thinnest_mm and thickest_mm), and each plate by its in-game name led by its zone, e.g. \"bow: Fore End Deck\", with thickness in mm. Plates of 0 mm, which only close a zone for hit detection, are left out. Thickness is nominal, not protection: what reaches the citadel depends on every plate in front of it, which armor_assessment traces.",
        schema: input_schema::<ShipArgs>,
        call: |game, arguments| run(arguments, |args| game.ship_armor(args)),
    },
    Tool {
        name: "ship_aircraft",
        description: "Squadrons of a carrier, hybrid ship or ship with an air strike: role, aircraft, squadron size, attack flight size, payload per plane, ordnance kind (bomb, skip bomb, rocket, torpedo, depth charge), ammo type, damage per projectile, HE, SAP and AP penetration and fire chance per projectile, plus air strike charges, reload and range. AP bomb and rocket penetration (ordnance.ap.penetration_mm) is computed at bulletSpeed with the game's AP penetration formula; for some ordnance it differs from the port. ordnance.fire_chance_pct is the port value without skills and is 0 when the ordnance cannot start fires; fire_chance gives base and best-fit build with the skills that raise it. With thickness_mm, says whether each ordnance defeats that plate.",
        schema: input_schema::<ShipAircraftArgs>,
        call: |game, arguments| run(arguments, |args| game.ship_aircraft(args)),
    },
    Tool {
        name: "ship_shells",
        description: "Main and secondary battery shells of one ship: ammo type, caliber, HE/SAP penetration, the thickest whole-mm plate AP or SAP overmatches (an overmatched plate cannot ricochet it), alpha damage, muzzle velocity, fire chance, and for AP and SAP the ricochet angles from the plate normal (ricochets start, every hit ricochets) as game data states them per shell; HE does not ricochet. ricochet_note marks a shell whose angles differ from those most shells of its ammo type share, e.g. AP whose ricochets start at 60 deg instead of 45. Each HE/SAP shell's penetration carries builds: base, best (the best-fit build, which takes Inertia Fuse for HE Shells only when it lifts the shell over its tier's armor threshold) and alternatives such as \"with Inertia Fuse for HE Shells\" with a note.",
        schema: input_schema::<ShipArgs>,
        call: |game, arguments| run(arguments, |args| game.ship_shells(args)),
    },
    Tool {
        name: "ship_concealment",
        description: "Detectability of one ship by ships, by aircraft, after firing the main battery, and after firing the main battery from smoke, each with builds: base (stock), best (best-fit modules, skills such as Concealment Expert and upgrades, listed in assumed) and alternatives (e.g. without Concealment Expert). in_smoke_evidence explains, with values per skill and upgrade, why concealment skills and upgrades do not change the in-smoke value.",
        schema: input_schema::<ShipArgs>,
        call: |game, arguments| run(arguments, |args| game.ship_concealment(args)),
    },
    Tool {
        name: "penetration_search",
        description: "Find ships whose shells, bombs or rockets defeat a plate: HE or SAP penetration at least the thickness, or AP whose caliber overmatches it (caliber > 14.3 x thickness). SAP that also overmatches the plate cannot ricochet off it and is marked \"overmatches\"; HE never overmatches. AP bombs and rockets overmatch by the same rule. Give thickness_mm, or target_ship with zone. armament selects main_battery, secondary_battery, aircraft (carrier and ship-launched squadrons) and airstrike. Without it every armament is searched, each in its own groups; groups without armament are the main battery. For carriers, planes, bombs or rockets, pass armament [\"aircraft\"] or read the aircraft groups. Without ammo, returns separate HE, SAP and AP overmatch groups per armament. Each group gives qualifying (ships that defeat the plate, listed in ships with the weapon) and considered (ships under the same filters whose armament fires that ammo), each as total and by_class, and share_pct (qualifying as a percent of considered) and share_pct_by_class, which answers \"what percent of <class>\". With a target ship and no tiers, searches the target's tier +-2.",
        schema: input_schema::<PenetrationSearchArgs>,
        call: |game, arguments| run(arguments, |args| game.penetration_search(args)),
    },
    Tool {
        name: "armor_trace",
        description: "Trace a shooter's shell or bomb through a target ship's 3D armor model, e.g. whether Sekiryu's AP bombs can citadel Yamato or Des Moines AP can citadel a broadside cruiser at 10 km. Casts a grid of parallel rays along the line of fire (gun range and target angle, or the bomber's dive) and runs AP through every plate with the armor viewer's simulation; HE and SAP are tested against the first plate, where SAP ricochets at steep angles unless it overmatches the plate. Without ammo, traces each shell or bomb type the weapon fires or carries. Returns one trace per type: the fraction of rays over the citadel that detonate inside it, a summary per struck zone, example rays with their plates, a verdict, and the assumptions behind it.",
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
        description: "How well one ship's armor protects it, for questions such as \"is X's armor good\" or \"is X tanky\". Traces the ship's 3D armor model against shooter groups chosen from its matchmaking tiers (the most common destroyer, light cruiser, heavy cruiser and battleship AP calibers, cruiser HE and SAP) at broadside, 30 and 45 deg angled, in close (5-10 km), moderate (12-15 km) and long (15 km and beyond) range bands; shooters that cannot reach a band are listed as excluded. Per band, group and aspect: citadel, penetration, overpenetration, ricochet, shatter and fuse-armed rates with ray counts, ranked among released ships of its class within one tier. Properties: citadel roof and floor against the waterline, share of the citadel side above water, turtleback, the citadel face above water, and plates that arm the fuse of AP that would otherwise overpenetrate. Ends with a verdict built from those numbers. Destroyers and submarines have no citadel: citadel_metrics says citadel rates do not apply to them, and a missing citadel is no weakness.",
        schema: input_schema::<ArmorAssessmentArgs>,
        call: |game, arguments| run(arguments, |args| game.armor_assessment(args)),
    },
    Tool {
        name: "ship_comparison",
        description: "Compare the ships of one class and tier (or tier range), optionally of one nation, on the factors that decide their strength, for questions such as \"worst tier 10 battleship\", \"why is X considered bad\" or \"is Incomparable better than C. Colombo\". Name the ships asked about in ships: each named ship gets a full entry, its category (tech_tree, or premium for premium, special and Research Bureau ships, which belong to no tech tree line), per factor the value on its best-fit build (unique upgrades included), its rank in the group and the share of the group it beats, and outliers: values better or worse than 90% of the group, plus signals, consumables, special abilities and the unique upgrade. Without ships, only the group summary is returned: ship_names (every ranked ship) and leaders. leaders gives per factor the best and worst ships of the group, three each without ships named and one each with them, each with its nation and value and with \"premium\" or \"test\" for a ship not of the tech tree, e.g. \"Colbert (France, premium)\": a premium ship heads no line to research. torpedo_leaders the same per torpedo option. Factors: health, Repair Party heal; main battery reload, range, damage per minute, salvo damage and shell flight time per ammo, turret traverse, dispersion and sigma, fire starts, barrels bearing ahead and astern and the angles at which all bear; secondary damage and range; torpedo damage per minute, torpedoes per minute with loader limits, reload, launch angle off the bow and tubes per broadside; anti-aircraft damage and range; concealment by ships and aircraft; speed, rudder shift, turning circle. Per torpedo option: speed, range, damage, damage at the end of its run and damage by distance, flooding coefficient, detectability, reaction time and damage per minute, ranked among every torpedo option of the group. Cruisers and battleships also get battleship AP citadel and overpenetration rates, the share of matchmaking battleships that overmatch the bow and stern, and, for ships named, matchups against every shooter group of armor_assessment (not nominal plate thickness); destroyers get no armor judgment. signals give the play style the numbers point to, such as kiting or bow-in play. Released and test ships are ranked; replicas are named in their original's entry. No single score is given: weigh the factors for the question.",
        schema: input_schema::<ShipComparisonArgs>,
        call: |game, arguments| run(arguments, |args| game.ship_comparison_summary(args)),
    },
    Tool {
        name: "turtleback_search",
        description: "Which ships have a turtleback: an armored deck sloping down behind a thicker belt to meet it near or below the waterline, read from each ship's 3D armor model. Filter by tier, class and nation. Returns considered and qualifying counts per class and per nation with shares, and the qualifying ships with deck thickness, slope and belt thickness. Ships sharing one armor model, such as collaboration copies, count once.",
        schema: input_schema::<TurtlebackSearchArgs>,
        call: |game, arguments| run(arguments, |args| game.turtleback_search(args)),
    },
    Tool {
        name: "ship_stats",
        description: "Port stat card of one ship with stock modules, followed by stats computed from it: main battery barrels, salvo damage and damage per minute per ammo type, dispersion at the stock firing range, torpedo tubes, time to use every charge of a consumable, fire starts per minute per armament and in total, and fire and flooding. Fire chance reduction is how much less likely any shell is to set the ship on fire than its listed fire chance; it comes from the hull and grows with tier (49.95% on a tier 10 hull and on every aircraft carrier, 13.32% on a stock tier 5 hull and 16.65% on its upgraded hull) and Fire Prevention Expert multiplies what is left by 0.9. Also fires that can burn at once (usually 4, 1 on submarines; Fire Prevention Expert merges two into one), fire and flooding duration, damage per second in percent of maximum health and damage of one whole fire or flood. Without skills and upgrades, build_stats gives main battery reload and range, secondary range, torpedo reload, torpedoes_per_minute, the damage per minute and per second of each main battery ammo, secondary gun group and torpedo, and fires_per_minute, each with builds: base, best-fit build (assumed) and alternatives. torpedoes_per_minute counts the tubes, and the loader groups when the game reloads only some launchers at a time (submarine groups name bow, stern or rotating), and states the limit. Torpedo rates are against one target at the bearing where the most tubes bear, counting only the launchers that turn to it, so a ship with launchers on both sides brings one side to bear; all_launchers_ceiling is every launcher firing at once (submarine rates always count every launcher). A ship with the Torpedo Reload Booster consumable also gets torpedo_reload_booster: the reload it sets, charges, action time, cooldown (with the ship's special ability active as an alternative when it changes them), and torpedoes_per_minute and torpedo_damage averaged over one booster cycle with the booster used whenever ready. A ship with a consumable whose buff shortens the launcher reload while active (Auxiliary and Torpedo Weaponry Coordination) gets torpedo_reload_buffs: the reload factor while active, charges, action time, cooldown, and torpedoes_per_minute and torpedo_damage averaged over one cycle with it used whenever ready. torpedo reload is the launchers' own reload, a different thing from these consumables. A ship with a Main Battery Reload Booster gets the alternative \"with Main Battery Reload Booster active\" in each main battery ammo's damage rate builds: the rate while the booster runs, the reload time times its main_battery_reload_factor (ship_consumables gives its charges or time-based capacity); with special_ability_active it is the rate with both the special ability (e.g. Combat Instructions) and the booster active. Damage rates are theoretical maxima: every shell or torpedo hits for full damage, without fires, flooding, citadel or armor. Gun damage rates and fire starts are against one target at the bearing where they peak (bearing, degrees clockwise from the bow: 90 is the starboard beam, 270 the port beam), counting only the mounts whose firing sectors cover it, with by_bearing at bow-on, 30, 45, 60, 90, 120, 135, 150 and stern-on for both sides; all_barrels_ceiling, all_mounts_ceiling and all_launchers_ceiling are every mount or launcher firing at once, which no single target sees, so never quote them as what a ship does to one target. fires_per_minute counts every armament that can start fires: main battery ammo with a fire chance, each secondary gun group (and secondaries, every group together), carrier and hybrid squadrons and air strikes, with projectiles per minute, the fire chance of one projectile and the chance one salvo starts a fire; its total is what answers which ship starts the most fires, and Inertia Fuse for HE Shells appears as an alternative because it halves shell fire chance. With captain skills and upgrades, lists the stats they change, from and to, and which skill or upgrade caused each change. special_ability_active switches on the ship's F key ability (ship_special_ability says what it is): Combat Instructions apply their effects, e.g. a longer secondary range; Burst Fire or Modified Shells switch the main battery, and build_stats then gives burst_fire (shots_per_burst, shot_interval, reload_after_burst, cycle) and each main battery ammo's damage_per_minute over a full burst cycle with during_burst, the rate while a burst fires; changes lists what the ability changes against the standard stock card. With commander (a unique commander such as \"Jean-Jacques Honore\"), the commander's improved skills replace the standard ones in skills (marked improved) and build_stats, and talents lists each talent that changes a stat: its activation condition, the activations counted and the changes. Talents are never applied to changes or stats.",
        schema: input_schema::<ShipStatsArgs>,
        call: |game, arguments| run(arguments, |args| game.ship_stats(args)),
    },
    Tool {
        name: "ship_special_ability",
        description: "What one ship's special ability does, the ability players call \"the F key\": Combat Instructions (in-game name; a meter filled by events such as main battery hits or potential damage received, then activated for a set duration with effects such as faster secondary reload, or started automatically on some ships) and the alternative main battery fire mode (\"Burst Fire\", \"Modified Shells\" or \"Burst Fire and Modified Shells\": shots per burst, interval between shots, reload after the burst, cycle time, shells and effects). Gives charge conditions with progress per event, progress loss, duration, effects, and uses per battle (no charges or cooldown: each use needs the meter refilled). Ships without either say so. Values are for no captain skills or upgrades; ship_stats with special_ability_active gives stats with the ability on.",
        schema: input_schema::<ShipArgs>,
        call: |game, arguments| run(arguments, |args| game.ship_special_ability(args)),
    },
    Tool {
        name: "special_ability_search",
        description: "Which ships have a special ability (the F key): Combat Instructions, an alternative main battery fire mode, or Burst Fire, optionally filtered by words of the ability name or an effect (e.g. \"secondary battery firing range\", \"reload\") and by tier, class and nation. Lists every qualifying ship, released and test apart, with the ability, its duration or burst timing and effects, and counts per class and kind with shares of the ships considered.",
        schema: input_schema::<SpecialAbilitySearchArgs>,
        call: |game, arguments| run(arguments, |args| game.special_ability_search(args)),
    },
    Tool {
        name: "ship_firing_arcs",
        description: "Which guns and torpedo tubes of one ship bear on a target at each bearing: barrels bearing bow-on, stern-on (within 30 degrees) and on each broadside, where all barrels bear or never, damage of one salvo per ammo type, fires per minute and the fire chance of one salvo assuming every shell hits, and main battery dispersion at a range. salvo_fire_chance_pct is the chance a salvo starts at least one fire, 1 - (1 - p)^n over its n bearing shells with p the chance per hit; use it to answer \"fire chance of a salvo\" rather than adding shell chances. Optional skills, upgrades and modules; guns or torpedoes that fit only another hull need that hull in modules too. Optional target_ship applies the target's fire resistance: each shell's fire chance on that target is its fire chance times the target's fire_chance_factor, from its hull's tier and from target_skills and target_upgrades such as Fire Prevention Expert.",
        schema: input_schema::<FiringArcsArgs>,
        call: |game, arguments| run(arguments, |args| game.ship_firing_arcs(args)),
    },
    Tool {
        name: "stat_names",
        description: "Every stat stat_search can compare, as \"name (unit, per item): label\". Includes armor zone plate thickness, which is nominal and not protection (use armor_assessment or ship_comparison for that), stats computed from the card, such as total main battery barrels, salvo damage, damage per minute, dispersion at the stock firing range and sigma, and main battery AP and SAP ricochet angles, which game data states per shell.",
        schema: input_schema::<StatNamesArgs>,
        call: |game, arguments| run(arguments, |args| Ok(game.stat_names(args))),
    },
    Tool {
        name: "stat_search",
        description: "Find ships whose stats meet every condition, e.g. [{\"stat\": \"artillery.reload_time\", \"op\": \"<\", \"value\": 5}], or main battery AP whose ricochets start later than most, [{\"stat\": \"artillery.ap.ricochet_start\", \"op\": \">\", \"value\": 45}], or rank ships by one stat. Get stat names from stat_names. With conditions: lists ships that qualify stock, and ships that need modules, upgrades or captain skills, with the fewest needed, and counts of ships considered and qualifying by class, with share_pct and share_pct_by_class, which answers \"what percent of <class>\". For \"best\", \"lowest\", \"highest\", \"worst\" or \"most\" questions, give rank instead of guessing thresholds, e.g. the best concealment tier 10 destroyer is {\"rank\": {\"stat\": \"visibility.sea_detection\", \"order\": \"asc\"}, \"class\": \"destroyer\", \"min_tier\": 10, \"max_tier\": 10} and the worst concealment is the same with \"order\": \"desc\": one call lists the top ships in order, each with its value on the ranked build, its base value (stock hull and modules, no captain skills, no upgrades), the build, ties, and for stats with several rows the row the value comes from, e.g. \"main battery 457 mm HE\". Ship labels end \", premium\" for premium, special and Research Bureau ships, which belong to no tech tree line, and \", test\" for ships in testing. Without rank.build every ship is ranked on the build players run for the stat, the one that makes it better (lower detection, reload and dispersion; higher damage, range and health), in either order. Give \"build\": \"base\" for base values such as base detection, and \"lowest\" or \"highest\" only for how far skills and upgrades can push a stat either way; never rank one build and quote another column. Low tiers lead many stats across all tiers, so give min_tier and max_tier when the question names or implies a tier, and say which tiers you ranked when it does not. Conditions and rank combine: only qualifying ships are ranked. Consumable stats (consumable.charges, consumable.work_time, consumable.reload_time, ...) have one row per consumable, named with its English name and type; the type is the qualifier, e.g. \"artilleryBoosters\" for the Main Battery Reload Booster. Time-based consumables, switched on and off and drawing on a pool of seconds of use instead of charges (e.g. 20 de Julio's Main Battery Reload Booster, Sibir's Repair Party), are the only ones with consumable.capacity: [{\"stat\": \"consumable.capacity\", \"op\": \">=\", \"value\": 0}] lists every ship with one, each with the consumable and its capacity in seconds. For which ships start the most fires, rank fires_per_minute.total: fire starts against one target at the bearing where the ship starts the most, counting only the gun mounts that bear there, plus squadrons and air strikes; each ranked ship carries fires_by_armament (bearing, total, main_battery, secondary, aircraft), so no further call is needed for the split. fires_per_minute.secondary ranks every secondary group together against one target and fires_per_minute.secondary_group one group; fires_per_minute.total_all_mounts and fires_per_minute.secondary_all_mounts count every mount at once, a ceiling no single target sees. artillery.shells.burn_chance is the chance of one main battery shell only.",
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
        description: "One upgrade (modernization) by name: slot, kind (regular, consumable_modification, or unique for a unique upgrade made for specific ships), credit price, effects per class and which tiers, nations and ships it fits.",
        schema: input_schema::<UpgradeArgs>,
        call: |game, arguments| run(arguments, |args| game.upgrade(args)),
    },
    Tool {
        name: "ship_upgrades",
        description: "Every upgrade that fits one ship, by slot, with kind, price and effects. A ship with a unique upgrade lists it in its slot with kind \"unique\" and, under unique_upgrades, which ships it fits and the regular upgrades of the slot it replaces.",
        schema: input_schema::<ShipArgs>,
        call: |game, arguments| run(arguments, |args| game.ship_upgrades(args)),
    },
    Tool {
        name: "unique_upgrades",
        description: "Unique (legendary) upgrades, each made for specific ships, for questions such as \"which ships have a unique upgrade\" or \"what does Colombo's unique upgrade do\": name, slot, the ships it fits, effects, and the regular upgrades of the slot it replaces. Filter by tier, class and nation of the ships. Best-fit builds in the other tools take a ship's unique upgrade where it improves a stat and give the build without it as an alternative.",
        schema: input_schema::<UniqueUpgradesArgs>,
        call: |game, arguments| run(arguments, |args| Ok(game.unique_upgrades(args))),
    },
    Tool {
        name: "ship_consumables",
        description: "Consumables of one ship by slot with charges, reload, action time, detection ranges and heal rate. Each slot carries one consumable in battle: a slot with choice (\"choose one of: Surveillance Radar, Fighters, Spotting Aircraft\") carries only the option the player picks, never several at once. A time-based consumable (e.g. 20 de Julio's Main Battery Reload Booster) has no charges or action time: capacity_s is its seconds of use per battle, spent in activations of at least min_activation_s with reload_s between them. A Main Battery Reload Booster gives main_battery_reload_factor, the main battery reload time multiplier while active (0.5 halves it); ship_stats gives the damage rates with it active. A Repair Party gives heal: HP and percent of max_hp restored by one charge (per_charge = heal_percent_per_s x action_time_s x max_hp) and by every charge (per_battle); quote these for how much a Repair Party heals, never the rate times the charges. They are ceilings: only repairable damage is restored. A consumable that is used on an allied ship (e.g. Engineering Crew Assistance) or applies a buff carries buff: who it can be used on, range, and its effects on the ally and on your own ship. Without skills and upgrades, each consumable carries builds for action time, charges, capacity, reload, smoke lifetime and Repair Party heal_per_charge and heal_per_battle: base, best (consumable upgrades and skills such as Superintendent, Consumable Enhancements and Survivability Expert, listed in assumed) and alternatives. consumable narrows to one, e.g. \"radar\", \"hydro\", \"DCP\"; choice still names the slot's other options. With skills and upgrades, applies them instead.",
        schema: input_schema::<ShipConsumablesArgs>,
        call: |game, arguments| run(arguments, |args| game.ship_consumables(args)),
    },
    Tool {
        name: "healing_sources",
        description: "Every way one ship can restore HP, for questions such as \"how can Akizuki heal without a Repair Party\": its own consumables that heal it (Repair Party, and heals its squadrons drop on it), consumables that allied ships and carrier squadrons use on it (e.g. Engineering Crew Assistance, Repair Assistance) with the ships that carry them and their values, talents of unique commanders who can command it with what activates them, and game mode buffs such as Arms Race HP regeneration. Without ship, lists the consumables that heal allied ships and the ships that carry them.",
        schema: input_schema::<HealingSourcesArgs>,
        call: |game, arguments| run(arguments, |args| game.healing_sources(args)),
    },
    Tool {
        name: "commander",
        description: "One unique (legendary) commander by name, e.g. \"Yamamoto Isoroku\", \"Halsey\", \"Jean-Jacques Honore\", \"John Doe\": the nations and ships they serve on, their improved captain skills with the standard and improved effects per class, and their talents: battle types, what activates them, effects per activation, activations per battle and the effects after every activation. With ship, only that ship's class. Pass the commander to ship_stats to apply the improved skills to a ship.",
        schema: input_schema::<CommanderArgs>,
        call: |game, arguments| run(arguments, |args| game.commander(args)),
    },
    Tool {
        name: "commander_search",
        description: "Find unique commanders by what their improved skills or talents do, e.g. effect \"restores HP\", \"reload\", \"fires\", by nation, or by a ship they can command. Each commander lists only the improved skills and talents that match. Omit effect to list every unique commander with improved skills or talents.",
        schema: input_schema::<CommanderSearchArgs>,
        call: |game, arguments| run(arguments, |args| game.commander_search(args)),
    },
    Tool {
        name: "game_mode_buffs",
        description: "Buffs a game mode hands out in battle, e.g. Arms Race: each buff with its effects per number of stacks, per ship class or for one ship's class. Picking up a buff applies it to every ship of the team; picking up the same buff again adds a stack, up to max_stacks.",
        schema: input_schema::<GameModeBuffsArgs>,
        call: |game, arguments| run(arguments, |args| game.game_mode_buffs(args)),
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
