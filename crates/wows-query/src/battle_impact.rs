//! A judgment of how much each ship of one class can sway a battle, scored
//! from its ship_comparison standings with weights stated in the output.

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use wows_core::units::Km;

use crate::comparison::ComparedShip;
use crate::comparison::CruiserType;
use crate::comparison::FactorStanding;
use crate::comparison::Listing;
use crate::comparison::RankScope;
use crate::comparison::ShipComparisonArgs;
use crate::comparison::number;
use crate::data::GameData;
use crate::error::QueryError;
use crate::names::Interpretations;
use crate::ships::ShipClass;
use crate::ships::Tier;

/// Arguments for `battle_impact`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BattleImpactArgs {
    /// destroyer, cruiser or battleship. Optional with ships: then the class of the first ship named. Carriers and submarines are not scored.
    pub class: Option<ShipClass>,

    /// One tier, e.g. 10. Use min_tier and max_tier for a range.
    pub tier: Option<Tier>,
    pub min_tier: Option<Tier>,
    pub max_tier: Option<Tier>,

    /// Ships asked about, e.g. ["Moskva", "Des Moines"]; each is listed with its rank even outside the top. Without tiers, the group spans the tiers of the ships named.
    pub ships: Option<Vec<String>>,

    /// Highest scoring ships listed. Defaults to 10; 0 lists only the ships named.
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BattleImpactOutput {
    #[serde(skip_serializing_if = "Interpretations::is_empty")]
    pub interpretations: Interpretations,

    /// e.g. "tier 10 cruisers".
    pub group: String,

    /// Ships scored.
    pub ranked: usize,

    pub formula: &'static str,

    /// Per component, its weight and its factors with their weights.
    pub weights: Vec<String>,

    /// The highest scoring ships and the ships named, best first.
    pub ships: Vec<ImpactEntry>,

    /// The lowest scoring ships not in `ships`, lowest first: "56. Name 31.2".
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub lowest: Vec<String>,

    pub notes: &'static [&'static str],
}

/// One ship's score with the components behind it.
#[derive(Debug, Clone, Serialize)]
pub struct ImpactEntry {
    /// From 1, the highest score first; equal scores share a rank.
    pub rank: usize,

    /// "T10 Moskva (cruiser, Russia)".
    pub ship: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub cruiser_type: Option<CruiserType>,

    /// GameParams index. Not in the tool output.
    #[serde(skip)]
    pub index: String,

    pub score: ImpactScore,

    /// Per component, its score and the factors that raise it most.
    pub components: PerComponent<ComponentEntry>,

    /// Factors that beat less than 20% of the group.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub weak_points: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ComponentEntry {
    pub score: ImpactScore,

    /// The factors adding the most to the score, e.g. "radar_range 12.5 km
    /// (beats 90%)".
    pub top: Vec<String>,
}

/// A score from 0 to 100, one decimal: the weighted share of the group a
/// ship beats.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ImpactScore(f32);

impl ImpactScore {
    fn from_share(share: Share) -> Self {
        Self((share.0 * 1000.0).round() / 10.0)
    }

    pub fn value(self) -> f32 {
        self.0
    }
}

/// One value per component; absent where the component has no factor.
#[derive(Debug, Clone, Serialize)]
pub struct PerComponent<T> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub firepower: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub close_quarters: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accuracy: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handling: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reach: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub survivability: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mobility: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commander: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spotting: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub utility: Option<T>,
}

impl<T> PerComponent<T> {
    fn new() -> Self {
        Self {
            firepower: None,
            close_quarters: None,
            accuracy: None,
            handling: None,
            reach: None,
            survivability: None,
            mobility: None,
            commander: None,
            spotting: None,
            utility: None,
        }
    }

    fn slot_mut(&mut self, component: Component) -> &mut Option<T> {
        match component {
            Component::Firepower => &mut self.firepower,
            Component::CloseQuarters => &mut self.close_quarters,
            Component::Accuracy => &mut self.accuracy,
            Component::Handling => &mut self.handling,
            Component::Reach => &mut self.reach,
            Component::Survivability => &mut self.survivability,
            Component::Mobility => &mut self.mobility,
            Component::Commander => &mut self.commander,
            Component::Spotting => &mut self.spotting,
            Component::Utility => &mut self.utility,
        }
    }
}

impl<T> Default for PerComponent<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// A group of factors scored together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Component {
    Firepower,
    CloseQuarters,
    Accuracy,
    Handling,
    Reach,
    Survivability,
    Mobility,
    Commander,
    Spotting,
    Utility,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScoreModel {
    Arithmetic,
    Battleship,
}

impl Component {
    fn key(self) -> &'static str {
        match self {
            Self::Firepower => "firepower",
            Self::CloseQuarters => "close_quarters",
            Self::Accuracy => "accuracy",
            Self::Handling => "handling",
            Self::Reach => "reach",
            Self::Survivability => "survivability",
            Self::Mobility => "mobility",
            Self::Commander => "commander",
            Self::Spotting => "spotting",
            Self::Utility => "utility",
        }
    }
}

/// A weight relative to the other weights of its level.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Weight(f32);

/// A share of the other ships of a group beaten, 0 to 1.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
struct Share(f32);

/// How a ship without a factor its peers have counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Absent {
    /// Below every ship with it: a consumable or armament the ship lacks.
    Worst,

    /// Zero capability: an optional system the component measures directly.
    Zero,

    /// Rank only among ships with the factor; a ship without it is scored on
    /// its other factors.
    Unmeasured,
}

/// A factor of a component: one ship_comparison factor, or a family of
/// per-ammo factors of which the ship's best counts.
struct FactorSpec {
    name: &'static str,
    keys: &'static [&'static str],
    weight: Weight,
    absent: Absent,
}

struct ComponentSpec {
    component: Component,
    weight: Weight,
    factors: &'static [FactorSpec],
}

/// A factor's share for one ship.
struct FactorShare {
    spec: &'static FactorSpec,

    share: Share,

    /// The standing the share comes from; `None` when the ship lacks the
    /// factor.
    standing: Option<FactorStanding>,
}

impl FactorShare {
    /// "dpm_he 285000 HP/min (beats 92%)" or "radar_range none (beats 23%)".
    fn describe(&self) -> String {
        let beats = (self.share.0 * 100.0).round();
        match &self.standing {
            Some(standing) => {
                let value = number(standing.value);
                let shown = match standing.unit {
                    "%" => format!("{value}%"),
                    "x" => format!("{value}x"),
                    unit if unit == standing.factor => value,
                    unit => format!("{value} {unit}"),
                };
                format!("{} {shown} (beats {beats}%)", standing.factor)
            }
            None => format!("{} none (beats {beats}%)", self.spec.name),
        }
    }
}

struct ComponentScore {
    component: Component,
    score: Share,
    factors: Vec<FactorShare>,
}

/// One ship's score and components; `None` without any component.
struct ShipImpact {
    score: Option<Share>,
    components: Vec<ComponentScore>,
}

impl GameData {
    /// The ships of one class and tier range ranked by battle impact.
    pub fn battle_impact(&self, args: &BattleImpactArgs) -> Result<BattleImpactOutput, QueryError> {
        const UNSCORED: &str = "battle_impact scores destroyers, cruisers and battleships; carriers and submarines are not scored: their impact comes from aircraft and underwater play these factors do not measure";
        // Ships listed when no limit is given, and lowest ranked ships named
        // in `lowest`.
        const DEFAULT_LIMIT: usize = 10;
        const LOWEST_SHOWN: usize = 5;
        const ARITHMETIC_FORMULA: &str = "factor share = share of the other ships of the group (of the same tier for armor factors) whose value the ship's beats, ties counting half. A ship without a measured factor others have counts as worse than every ship with it and tied with the others without it. A factor marked unmeasured ranks only among peers that have it and is left out for ships without it. A family (gun_dpm, gun_alpha) takes the ship's best ammo, ranked among the ships with that ammo. component = weighted mean of its factor shares. score = weighted mean of the components with the class weights below, 0 to 100.";
        const BATTLESHIP_FORMULA: &str = "factor share = share of the other ships of the group (of the same tier for armor and offensive overmatch factors) whose value the ship's beats, ties counting half. A ship without a measured factor others have counts as worse than every ship with it and tied with the others without it; no funny button contributes zero funny-button utility. A factor marked unmeasured ranks only among peers that have it and is left out for ships without it. A family (gun_dpm, gun_alpha) takes the ship's best ammo, ranked among the ships with that ammo. component = weighted mean of its factor shares. Damage delivery = the power mean with exponent -8 of firepower, accuracy, gun handling, survivability and mobility after flooring each at 20%; it is multiplied by 65% for each of those components or reach that beats less than half of the group. Reach is a tactical weakness but not a linear damage bottleneck because dispersion is measured at 12 km. Score = damage delivery x71.5%, survivability x5%, funny-button utility x15%, unique-commander talent potential x5%, close quarters x1% and spotting x2.5%, 0 to 100.";
        const NOTES: &[&str] = &[
            "A judgment from game data, not from win rates or player results: the weights are a stated opinion of what sways a battle. Read the components to see why a ship scores as it does.",
            "Scores compare released ships of one class and tier range only; a named test or experimental ship is included on request. A cruiser's score says nothing against a battleship's.",
            "Values are those of ship_comparison: each on its best-fit build, consumables stock, and horizontal dispersion, vertical dispersion, flight time and raw AP penetration measured at 12 km. AP penetration is the head-on Krupp result at impact velocity, distinct from overmatch. The AP fuse threshold is the normalized armor thickness needed to arm the shell; a hit that crosses the target without arming overpenetrates, so lower thresholds score higher. Fuse time and residual velocity still decide how far an armed shell travels. Damage assumes every shell and torpedo hits. Secondary DPM incorporates shell damage, barrel count and reload; fires per minute incorporates projectile count, reload and each shell's fire chance across the main and secondary batteries. SAP coverage is the share of external armor plate thicknesses in the ship's matchmaking range that the battery can penetrate. A consumable counts when any slot offers it, though a slot holds one choice.",
            "The funny button means the F key: Combat Instructions or an alternative fire mode. Its effect index measures modifier magnitude and duration. Its access index uses the least demanding repeatable charge path. Equal burden means 20 ribbons, 10 adjustment salvos, 4 visibility changes, 3 consumable uses, 120 timer seconds, or potential damage equal to 10 times ship HP.",
            "Mobile smoke counts its active duration. Unique-commander talent potential measures the modifiers from a talent's first activation; talent activation difficulty, improved skills and non-modifier talent effects are not estimated.",
            "Not counted: anti-aircraft, exact shell hit probability, consumable uptime, ability charge events during a real match and player positioning.",
        ];

        let mut named = Vec::new();
        let mut resolved = Interpretations::default();
        for name in args.ships.iter().flatten() {
            let ship = self.resolve_ship(name, &mut resolved)?;
            let ship = match &ship.replica_of {
                Some(original) => self.ships.iter().find(|other| other.param.index() == original.index).unwrap_or(ship),
                None => ship,
            };
            named.push(ship);
        }

        let Some(class) = args.class.or_else(|| named.first().map(|ship| ship.class)) else {
            return Err(QueryError::InvalidArguments { reason: "give class, or name ships" });
        };
        let Some(specs) = class_specs(class) else {
            return Err(QueryError::InvalidArguments { reason: UNSCORED });
        };

        let comparison = self.compare_group(
            &ShipComparisonArgs {
                class: Some(class),
                tier: args.tier,
                min_tier: args.min_tier,
                max_tier: args.max_tier,
                ships: args.ships.clone(),
                reference_range_km: Some(Km::new(12.0)),
            },
            Listing::Every,
            true,
        )?;
        let entries = comparison.ships;
        let model = if class == ShipClass::Battleship { ScoreModel::Battleship } else { ScoreModel::Arithmetic };
        let impacts = impacts(model, specs, &entries);
        let ranks = ranks(&impacts);

        // Highest score first; ships without a score are left out.
        let mut order: Vec<(usize, Share)> =
            impacts.iter().enumerate().filter_map(|(index, impact)| Some((index, impact.score?))).collect();
        order.sort_by(|left, right| right.1.0.total_cmp(&left.1.0));
        let order: Vec<usize> = order.into_iter().map(|(index, _)| index).collect();
        let limit = args.limit.unwrap_or(DEFAULT_LIMIT);
        let is_named = |index: usize| named.iter().any(|ship| ship.param.index() == entries[index].index);
        let listed: Vec<usize> = order
            .iter()
            .enumerate()
            .filter(|(place, index)| *place < limit || is_named(**index))
            .map(|(_, index)| *index)
            .collect();
        let ships =
            listed.iter().filter_map(|index| entry(&entries[*index], &impacts[*index], ranks[*index]?)).collect();
        let lowest = order
            .iter()
            .rev()
            .filter(|index| !listed.contains(index))
            .take(LOWEST_SHOWN)
            .filter_map(|index| {
                let name = entries[*index].ship.as_str();
                let score = ImpactScore::from_share(impacts[*index].score?);
                Some(format!("{}. {name} {}", ranks[*index]?, score.value()))
            })
            .collect();

        Ok(BattleImpactOutput {
            interpretations: comparison.interpretations,
            group: comparison.group,
            ranked: order.len(),
            formula: if model == ScoreModel::Battleship { BATTLESHIP_FORMULA } else { ARITHMETIC_FORMULA },
            weights: weight_lines(model, specs),
            ships,
            lowest,
            notes: NOTES,
        })
    }
}

/// Components and factors with their weights for `class`; `None` for
/// carriers and submarines. Weights say what decides a ship's influence in
/// its role: a destroyer's by spotting, concealment and damage, torpedoes
/// below guns; a cruiser's by damage, spotting and staying alive; a
/// battleship's by damage and staying alive, with little spotting of its
/// own. Accuracy and range weigh least: players rank them below spotting,
/// damage, HP, armor and healing. Big-shell alpha weighs near gun_dpm for
/// cruisers and battleships: gun_dpm counts every shell as a full hit,
/// which small shells do not achieve against armor. Fire starts are a small
/// part of firepower: gun_dpm already counts HE damage.
fn class_specs(class: ShipClass) -> Option<&'static [ComponentSpec]> {
    const GUN_DPM: FactorSpec = FactorSpec {
        name: "gun_dpm",
        keys: &["dpm_ap", "dpm_he", "dpm_sap"],
        weight: Weight(1.0),
        absent: Absent::Worst,
    };
    const ACCURACY: &[FactorSpec] = &[
        FactorSpec { name: "dispersion", keys: &["dispersion"], weight: Weight(1.0), absent: Absent::Worst },
        FactorSpec {
            name: "vertical_dispersion",
            keys: &["vertical_dispersion"],
            weight: Weight(1.0),
            absent: Absent::Worst,
        },
        FactorSpec { name: "sigma", keys: &["sigma"], weight: Weight(1.0), absent: Absent::Worst },
    ];
    const REACH: &[FactorSpec] = &[FactorSpec {
        name: "main_battery_range",
        keys: &["main_battery_range"],
        weight: Weight(1.0),
        absent: Absent::Worst,
    }];
    const ALPHA: &[&str] = &["alpha_ap", "alpha_he", "alpha_sap"];

    const DESTROYER: &[ComponentSpec] = &[
        ComponentSpec {
            component: Component::Firepower,
            weight: Weight(0.3),
            factors: &[
                GUN_DPM,
                FactorSpec { name: "gun_alpha", keys: ALPHA, weight: Weight(0.4), absent: Absent::Worst },
                FactorSpec {
                    name: "ap_penetration",
                    keys: &["ap_penetration"],
                    weight: Weight(0.25),
                    absent: Absent::Unmeasured,
                },
                FactorSpec {
                    name: "ap_fuse_threshold",
                    keys: &["ap_fuse_threshold"],
                    weight: Weight(0.25),
                    absent: Absent::Unmeasured,
                },
                FactorSpec { name: "torpedo_dpm", keys: &["torpedo_dpm"], weight: Weight(0.8), absent: Absent::Worst },
                FactorSpec {
                    name: "fires_per_minute",
                    keys: &["fires_per_minute"],
                    weight: Weight(0.2),
                    absent: Absent::Worst,
                },
            ],
        },
        ComponentSpec { component: Component::Accuracy, weight: Weight(0.05), factors: ACCURACY },
        ComponentSpec { component: Component::Reach, weight: Weight(0.05), factors: REACH },
        ComponentSpec {
            component: Component::Survivability,
            weight: Weight(0.15),
            factors: &[
                FactorSpec { name: "health", keys: &["health"], weight: Weight(1.0), absent: Absent::Worst },
                FactorSpec {
                    name: "repair_party_total_heal",
                    keys: &["repair_party_total_heal"],
                    weight: Weight(0.75),
                    absent: Absent::Worst,
                },
                FactorSpec {
                    name: "damage_control_reload",
                    keys: &["damage_control_reload"],
                    weight: Weight(0.5),
                    absent: Absent::Worst,
                },
            ],
        },
        ComponentSpec {
            component: Component::Mobility,
            weight: Weight(0.1),
            factors: &[
                FactorSpec { name: "speed", keys: &["speed"], weight: Weight(1.0), absent: Absent::Worst },
                FactorSpec {
                    name: "rudder_shift",
                    keys: &["rudder_shift"],
                    weight: Weight(0.75),
                    absent: Absent::Worst,
                },
                FactorSpec {
                    name: "turning_circle",
                    keys: &["turning_circle"],
                    weight: Weight(0.75),
                    absent: Absent::Worst,
                },
            ],
        },
        ComponentSpec {
            component: Component::Spotting,
            weight: Weight(0.35),
            factors: &[
                FactorSpec { name: "concealment", keys: &["concealment"], weight: Weight(2.0), absent: Absent::Worst },
                FactorSpec { name: "radar_range", keys: &["radar_range"], weight: Weight(1.0), absent: Absent::Worst },
                FactorSpec {
                    name: "radar_duration",
                    keys: &["radar_duration"],
                    weight: Weight(0.5),
                    absent: Absent::Worst,
                },
                FactorSpec { name: "hydro_range", keys: &["hydro_range"], weight: Weight(0.75), absent: Absent::Worst },
                FactorSpec {
                    name: "torpedo_detection",
                    keys: &["torpedo_detection"],
                    weight: Weight(0.5),
                    absent: Absent::Worst,
                },
                FactorSpec {
                    name: "smoke_lifetime",
                    keys: &["smoke_lifetime"],
                    weight: Weight(0.75),
                    absent: Absent::Worst,
                },
            ],
        },
    ];
    const CRUISER: &[ComponentSpec] = &[
        ComponentSpec {
            component: Component::Firepower,
            weight: Weight(0.3),
            factors: &[
                GUN_DPM,
                FactorSpec { name: "gun_alpha", keys: ALPHA, weight: Weight(0.75), absent: Absent::Worst },
                FactorSpec {
                    name: "ap_penetration",
                    keys: &["ap_penetration"],
                    weight: Weight(0.5),
                    absent: Absent::Unmeasured,
                },
                FactorSpec {
                    name: "ap_fuse_threshold",
                    keys: &["ap_fuse_threshold"],
                    weight: Weight(0.5),
                    absent: Absent::Unmeasured,
                },
                FactorSpec { name: "torpedo_dpm", keys: &["torpedo_dpm"], weight: Weight(0.2), absent: Absent::Worst },
                FactorSpec {
                    name: "fires_per_minute",
                    keys: &["fires_per_minute"],
                    weight: Weight(0.15),
                    absent: Absent::Worst,
                },
            ],
        },
        ComponentSpec { component: Component::Accuracy, weight: Weight(0.05), factors: ACCURACY },
        ComponentSpec { component: Component::Reach, weight: Weight(0.05), factors: REACH },
        ComponentSpec {
            component: Component::Survivability,
            weight: Weight(0.25),
            factors: &[
                FactorSpec { name: "health", keys: &["health"], weight: Weight(1.0), absent: Absent::Worst },
                FactorSpec {
                    name: "repair_party_total_heal",
                    keys: &["repair_party_total_heal"],
                    weight: Weight(1.0),
                    absent: Absent::Worst,
                },
                FactorSpec {
                    name: "damage_control_reload",
                    keys: &["damage_control_reload"],
                    weight: Weight(0.25),
                    absent: Absent::Worst,
                },
                FactorSpec {
                    name: "bb_ap_citadel",
                    keys: &["bb_ap_citadel"],
                    weight: Weight(0.75),
                    absent: Absent::Unmeasured,
                },
                FactorSpec {
                    name: "bow_overmatch",
                    keys: &["bow_overmatch"],
                    weight: Weight(0.5),
                    absent: Absent::Unmeasured,
                },
                FactorSpec {
                    name: "cruiser_he_damaging",
                    keys: &["cruiser_he_damaging"],
                    weight: Weight(0.5),
                    absent: Absent::Unmeasured,
                },
            ],
        },
        ComponentSpec {
            component: Component::Mobility,
            weight: Weight(0.1),
            factors: &[
                FactorSpec { name: "speed", keys: &["speed"], weight: Weight(1.0), absent: Absent::Worst },
                FactorSpec {
                    name: "rudder_shift",
                    keys: &["rudder_shift"],
                    weight: Weight(0.75),
                    absent: Absent::Worst,
                },
                FactorSpec {
                    name: "turning_circle",
                    keys: &["turning_circle"],
                    weight: Weight(0.75),
                    absent: Absent::Worst,
                },
            ],
        },
        ComponentSpec {
            component: Component::Spotting,
            weight: Weight(0.25),
            factors: &[
                FactorSpec { name: "concealment", keys: &["concealment"], weight: Weight(1.5), absent: Absent::Worst },
                FactorSpec { name: "radar_range", keys: &["radar_range"], weight: Weight(1.0), absent: Absent::Worst },
                FactorSpec {
                    name: "radar_duration",
                    keys: &["radar_duration"],
                    weight: Weight(0.5),
                    absent: Absent::Worst,
                },
                FactorSpec { name: "hydro_range", keys: &["hydro_range"], weight: Weight(0.75), absent: Absent::Worst },
                FactorSpec {
                    name: "torpedo_detection",
                    keys: &["torpedo_detection"],
                    weight: Weight(0.5),
                    absent: Absent::Worst,
                },
                FactorSpec {
                    name: "smoke_lifetime",
                    keys: &["smoke_lifetime"],
                    weight: Weight(0.5),
                    absent: Absent::Worst,
                },
            ],
        },
    ];
    const BATTLESHIP: &[ComponentSpec] = &[
        ComponentSpec {
            component: Component::Firepower,
            weight: Weight(0.13),
            factors: &[
                FactorSpec {
                    name: "effective_gun_damage",
                    keys: &["effective_gun_damage"],
                    weight: Weight(1.0),
                    absent: Absent::Worst,
                },
                FactorSpec { name: "gun_dpm", keys: GUN_DPM.keys, weight: Weight(2.0), absent: Absent::Worst },
                FactorSpec { name: "gun_alpha", keys: ALPHA, weight: Weight(1.5), absent: Absent::Worst },
                FactorSpec {
                    name: "main_battery_reload",
                    keys: &["main_battery_reload"],
                    weight: Weight(0.75),
                    absent: Absent::Worst,
                },
                FactorSpec {
                    name: "main_battery_range",
                    keys: &["main_battery_range"],
                    weight: Weight(0.25),
                    absent: Absent::Worst,
                },
                FactorSpec {
                    name: "ap_penetration",
                    keys: &["ap_penetration"],
                    weight: Weight(1.0),
                    absent: Absent::Unmeasured,
                },
                FactorSpec {
                    name: "ap_fuse_threshold",
                    keys: &["ap_fuse_threshold"],
                    weight: Weight(0.5),
                    absent: Absent::Unmeasured,
                },
                FactorSpec {
                    name: "cruiser_bow_overmatch",
                    keys: &["cruiser_bow_overmatch"],
                    weight: Weight(1.0),
                    absent: Absent::Unmeasured,
                },
                FactorSpec {
                    name: "cruiser_full_bow_overmatch",
                    keys: &["cruiser_full_bow_overmatch"],
                    weight: Weight(0.75),
                    absent: Absent::Unmeasured,
                },
                FactorSpec {
                    name: "cruiser_external_overmatch",
                    keys: &["cruiser_external_overmatch"],
                    weight: Weight(0.5),
                    absent: Absent::Unmeasured,
                },
                FactorSpec {
                    name: "main_sap_coverage",
                    keys: &["main_sap_coverage"],
                    weight: Weight(1.5),
                    absent: Absent::Unmeasured,
                },
            ],
        },
        ComponentSpec { component: Component::Accuracy, weight: Weight(0.13), factors: ACCURACY },
        ComponentSpec {
            component: Component::Handling,
            weight: Weight(0.13),
            factors: &[
                FactorSpec {
                    name: "turret_traverse",
                    keys: &["turret_traverse"],
                    weight: Weight(0.5),
                    absent: Absent::Worst,
                },
                FactorSpec { name: "bow_barrels", keys: &["bow_barrels"], weight: Weight(0.5), absent: Absent::Worst },
                FactorSpec {
                    name: "all_bear_from_bow",
                    keys: &["all_bear_from_bow"],
                    weight: Weight(1.0),
                    absent: Absent::Worst,
                },
            ],
        },
        ComponentSpec { component: Component::Reach, weight: Weight(0.13), factors: REACH },
        ComponentSpec {
            component: Component::Survivability,
            weight: Weight(0.05),
            factors: &[
                FactorSpec { name: "health", keys: &["health"], weight: Weight(1.0), absent: Absent::Worst },
                FactorSpec {
                    name: "repair_party_total_heal",
                    keys: &["repair_party_total_heal"],
                    weight: Weight(1.0),
                    absent: Absent::Worst,
                },
                FactorSpec {
                    name: "damage_control_reload",
                    keys: &["damage_control_reload"],
                    weight: Weight(0.25),
                    absent: Absent::Worst,
                },
                FactorSpec {
                    name: "bb_ap_citadel",
                    keys: &["bb_ap_citadel"],
                    weight: Weight(0.75),
                    absent: Absent::Unmeasured,
                },
                FactorSpec {
                    name: "bow_overmatch",
                    keys: &["bow_overmatch"],
                    weight: Weight(0.5),
                    absent: Absent::Unmeasured,
                },
                FactorSpec {
                    name: "cruiser_he_damaging",
                    keys: &["cruiser_he_damaging"],
                    weight: Weight(0.25),
                    absent: Absent::Unmeasured,
                },
            ],
        },
        ComponentSpec {
            component: Component::Mobility,
            weight: Weight(0.13),
            factors: &[
                FactorSpec { name: "speed", keys: &["speed"], weight: Weight(1.0), absent: Absent::Worst },
                FactorSpec {
                    name: "rudder_shift",
                    keys: &["rudder_shift"],
                    weight: Weight(1.0),
                    absent: Absent::Worst,
                },
                FactorSpec {
                    name: "turning_circle",
                    keys: &["turning_circle"],
                    weight: Weight(1.0),
                    absent: Absent::Worst,
                },
            ],
        },
        ComponentSpec {
            component: Component::Spotting,
            weight: Weight(0.025),
            factors: &[
                FactorSpec { name: "concealment", keys: &["concealment"], weight: Weight(1.0), absent: Absent::Worst },
                FactorSpec { name: "radar_range", keys: &["radar_range"], weight: Weight(1.0), absent: Absent::Worst },
                FactorSpec { name: "hydro_range", keys: &["hydro_range"], weight: Weight(0.5), absent: Absent::Worst },
                FactorSpec {
                    name: "torpedo_detection",
                    keys: &["torpedo_detection"],
                    weight: Weight(0.25),
                    absent: Absent::Worst,
                },
                FactorSpec {
                    name: "smoke_lifetime",
                    keys: &["smoke_lifetime"],
                    weight: Weight(0.5),
                    absent: Absent::Worst,
                },
                FactorSpec {
                    name: "mobile_smoke_duration",
                    keys: &["mobile_smoke_duration"],
                    weight: Weight(1.0),
                    absent: Absent::Worst,
                },
            ],
        },
        ComponentSpec {
            component: Component::Utility,
            weight: Weight(0.15),
            factors: &[
                FactorSpec {
                    name: "funny_button_effect",
                    keys: &["funny_button_effect"],
                    weight: Weight(1.25),
                    absent: Absent::Zero,
                },
                FactorSpec {
                    name: "funny_button_access",
                    keys: &["funny_button_access"],
                    weight: Weight(1.0),
                    absent: Absent::Zero,
                },
            ],
        },
        ComponentSpec {
            component: Component::Commander,
            weight: Weight(0.05),
            factors: &[FactorSpec {
                name: "commander_talent_effect",
                keys: &["commander_talent_effect"],
                weight: Weight(1.0),
                absent: Absent::Zero,
            }],
        },
        ComponentSpec {
            component: Component::CloseQuarters,
            weight: Weight(0.01),
            factors: &[
                FactorSpec {
                    name: "fires_per_minute",
                    keys: &["fires_per_minute"],
                    weight: Weight(0.5),
                    absent: Absent::Worst,
                },
                FactorSpec {
                    name: "secondary_dpm",
                    keys: &["secondary_dpm"],
                    weight: Weight(1.0),
                    absent: Absent::Worst,
                },
                FactorSpec {
                    name: "secondary_range",
                    keys: &["secondary_range"],
                    weight: Weight(0.75),
                    absent: Absent::Worst,
                },
                FactorSpec {
                    name: "secondary_sap_coverage",
                    keys: &["secondary_sap_coverage"],
                    weight: Weight(0.75),
                    absent: Absent::Unmeasured,
                },
                FactorSpec { name: "torpedo_dpm", keys: &["torpedo_dpm"], weight: Weight(0.5), absent: Absent::Worst },
            ],
        },
    ];

    match class {
        ShipClass::Destroyer => Some(DESTROYER),
        ShipClass::Cruiser => Some(CRUISER),
        ShipClass::Battleship => Some(BATTLESHIP),
        ShipClass::AircraftCarrier | ShipClass::Submarine => None,
    }
}

/// Every entry's score under `specs`. A component without any factor for a
/// ship is left out and the other weights share its part.
fn impacts(model: ScoreModel, specs: &'static [ComponentSpec], entries: &[ComparedShip]) -> Vec<ShipImpact> {
    (0..entries.len())
        .map(|index| {
            let components: Vec<(Weight, ComponentScore)> = specs
                .iter()
                .filter_map(|spec| {
                    let factors: Vec<FactorShare> =
                        spec.factors.iter().filter_map(|factor| factor_share(factor, index, entries)).collect();
                    let score = weighted_mean(factors.iter().map(|share| (share.spec.weight, share.share)))?;
                    Some((spec.weight, ComponentScore { component: spec.component, score, factors }))
                })
                .collect();
            let score = impact_score(model, &components);

            ShipImpact { score, components: components.into_iter().map(|(_, component)| component).collect() }
        })
        .collect()
}

fn impact_score(model: ScoreModel, components: &[(Weight, ComponentScore)]) -> Option<Share> {
    if model == ScoreModel::Arithmetic {
        return weighted_mean(components.iter().map(|(weight, component)| (*weight, component.score)));
    }

    let is_delivery = |component: Component| {
        matches!(
            component,
            Component::Firepower | Component::Accuracy | Component::Handling | Component::Reach | Component::Mobility
        )
    };
    let is_delivery_core = |component: Component| {
        matches!(
            component,
            Component::Firepower
                | Component::Accuracy
                | Component::Handling
                | Component::Survivability
                | Component::Mobility
        )
    };
    let delivery_components: Vec<Share> = components
        .iter()
        .filter(|(_, component)| is_delivery_core(component.component))
        .map(|(_, component)| component.score)
        .collect();
    let delivery = weighted_strict_mean(delivery_components.iter().map(|score| (Weight(1.0), *score)))?;
    let weaknesses = components
        .iter()
        .filter(|(_, component)| is_delivery(component.component) || component.component == Component::Survivability)
        .filter(|(_, component)| component.score.0 < 0.5)
        .count() as i32;
    let delivery = Share(delivery.0 * 0.65_f32.powi(weaknesses));
    weighted_mean(
        std::iter::once((Weight(0.715), delivery)).chain(
            components
                .iter()
                .filter(|(_, component)| !is_delivery(component.component))
                .map(|(weight, component)| (*weight, component.score)),
        ),
    )
}

/// The share of its peers entry `index` beats on `spec`, ties counting
/// half. A ship without the factor counts by `spec.absent`. `None` when no
/// peer has the factor, the ship has no peer, or its factor is unmeasured.
fn factor_share(spec: &'static FactorSpec, index: usize, entries: &[ComparedShip]) -> Option<FactorShare> {
    let in_family = |standing: &&FactorStanding| spec.keys.contains(&standing.factor);
    let has = |entry: &ComparedShip| entry.standings.iter().any(|standing| in_family(&standing));
    let scope = entries.iter().flat_map(|entry| &entry.standings).find(in_family)?.scope;
    let own = &entries[index];
    let peers: Vec<&ComparedShip> =
        entries.iter().filter(|entry| scope == RankScope::Group || entry.tier == own.tier).collect();
    let others = peers.len().checked_sub(1).filter(|others| *others > 0)? as f32;
    let having = peers.iter().filter(|entry| has(entry)).count();
    if having == 0 {
        return None;
    }

    let lacking = (peers.len() - having) as f32;
    // Ammo no other ship fires ranks in the middle of the ships with the
    // family.
    let best = own
        .standings
        .iter()
        .filter(in_family)
        .map(|standing| (standing.rank.beaten_ties_half().map_or(0.5, |beaten| beaten.value() / 100.0), standing))
        .max_by(|left, right| left.0.total_cmp(&right.0));
    let (share, standing) = match (best, spec.absent) {
        (Some((beaten, standing)), absent) => {
            (present_factor_share(beaten, having, lacking, others, absent), Some(standing.clone()))
        }
        (None, Absent::Worst) => ((lacking - 1.0) / 2.0 / others, None),
        (None, Absent::Zero) => (0.0, None),
        (None, Absent::Unmeasured) => return None,
    };

    Some(FactorShare { spec, share: Share(share), standing })
}

fn present_factor_share(beaten: f32, having: usize, lacking: f32, others: f32, absent: Absent) -> f32 {
    if absent == Absent::Unmeasured { beaten } else { (beaten * (having - 1) as f32 + lacking) / others }
}

/// `None` for no values or no weight.
fn weighted_mean(values: impl Iterator<Item = (Weight, Share)>) -> Option<Share> {
    let (total, weights) =
        values.fold((0.0, 0.0), |(total, weights), (weight, value)| (total + weight.0 * value.0, weights + weight.0));

    (weights > 0.0).then(|| Share(total / weights))
}

/// `None` for no values or no weight. The negative exponent makes every
/// damage-delivery input matter; the floor limits one outlier's effect.
fn weighted_strict_mean(values: impl Iterator<Item = (Weight, Share)>) -> Option<Share> {
    const EXPONENT: f32 = -8.0;
    const FLOOR: f32 = 0.2;

    let (power_total, weights) = values.fold((0.0, 0.0), |(total, weights), (weight, value)| {
        (total + weight.0 * value.0.max(FLOOR).powf(EXPONENT), weights + weight.0)
    });

    (weights > 0.0).then(|| Share((power_total / weights).powf(EXPONENT.recip())))
}

/// Rank by score among the ships with one, from 1, equal displayed scores
/// sharing a rank.
fn ranks(impacts: &[ShipImpact]) -> Vec<Option<usize>> {
    let shown: Vec<Option<ImpactScore>> =
        impacts.iter().map(|impact| impact.score.map(ImpactScore::from_share)).collect();

    shown
        .iter()
        .map(|score| {
            let score = (*score)?;
            Some(1 + shown.iter().flatten().filter(|other| **other > score).count())
        })
        .collect()
}

/// `None` for a ship without a score.
fn entry(entry: &ComparedShip, impact: &ShipImpact, rank: usize) -> Option<ImpactEntry> {
    // Factors named in `top` per component; weak points, the factors below
    // the share, named per ship.
    const TOP_SHOWN: usize = 2;
    const WEAK_SHARE: f32 = 0.2;
    const WEAK_SHOWN: usize = 3;

    let mut components = PerComponent::new();
    let mut weak: Vec<&FactorShare> = Vec::new();
    for component in &impact.components {
        let mut factors: Vec<&FactorShare> = component.factors.iter().collect();
        factors.sort_by(|left, right| {
            (right.spec.weight.0 * right.share.0).total_cmp(&(left.spec.weight.0 * left.share.0))
        });
        let top = factors
            .iter()
            .filter(|share| share.standing.is_some())
            .take(TOP_SHOWN)
            .map(|share| share.describe())
            .collect();
        *components.slot_mut(component.component) =
            Some(ComponentEntry { score: ImpactScore::from_share(component.score), top });
        weak.extend(component.factors.iter().filter(|share| share.share.0 < WEAK_SHARE));
    }

    // Most damaging first: what the factor's weight leaves unearned.
    weak.sort_by(|left, right| {
        ((1.0 - right.share.0) * right.spec.weight.0).total_cmp(&((1.0 - left.share.0) * left.spec.weight.0))
    });

    Some(ImpactEntry {
        rank,
        ship: entry.ship.clone(),
        cruiser_type: entry.cruiser_type,
        index: entry.index.clone(),
        score: ImpactScore::from_share(impact.score?),
        components,
        weak_points: weak.iter().take(WEAK_SHOWN).map(|share| share.describe()).collect(),
    })
}

/// "firepower 30%: gun_dpm (best of dpm_ap, dpm_he, dpm_sap) x1, ...".
fn weight_lines(model: ScoreModel, specs: &[ComponentSpec]) -> Vec<String> {
    let total: f32 = specs.iter().map(|spec| spec.weight.0).sum();

    specs
        .iter()
        .map(|spec| {
            let factors: Vec<String> = spec
                .factors
                .iter()
                .map(|factor| match factor.keys {
                    [single] if *single == factor.name => format!("{} x{}", factor.name, number(factor.weight.0)),
                    keys => format!("{} (best of {}) x{}", factor.name, keys.join(", "), number(factor.weight.0)),
                })
                .collect();
            let weight = if model == ScoreModel::Battleship {
                match spec.component {
                    Component::Firepower | Component::Accuracy | Component::Handling | Component::Mobility => {
                        "14.3%".to_owned()
                    }
                    Component::Survivability => "19.3%".to_owned(),
                    Component::Reach => "penalty only".to_owned(),
                    Component::Utility => "15%".to_owned(),
                    Component::Commander => "5%".to_owned(),
                    Component::CloseQuarters => "1%".to_owned(),
                    Component::Spotting => "2.5%".to_owned(),
                }
            } else {
                format!("{}%", (spec.weight.0 / total * 100.0).round())
            };
            format!("{} {weight}: {}", spec.component.key(), factors.join(", "))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comparison::factor_keys;

    /// A misspelled key would drop its factor from the score unnoticed.
    #[test]
    fn every_spec_key_is_a_comparison_factor() {
        let keys = factor_keys();
        for class in [ShipClass::Destroyer, ShipClass::Cruiser, ShipClass::Battleship] {
            let specs = class_specs(class).expect("class is scored");
            for key in specs.iter().flat_map(|spec| spec.factors).flat_map(|factor| factor.keys) {
                assert!(keys.contains(key), "{key} is no ship_comparison factor");
            }
        }
    }

    #[test]
    fn unmeasured_factor_does_not_beat_ships_without_it() {
        assert_eq!(present_factor_share(0.0, 5, 27.0, 31.0, Absent::Unmeasured), 0.0);
        assert_eq!(present_factor_share(0.5, 5, 27.0, 31.0, Absent::Unmeasured), 0.5);
        assert_eq!(present_factor_share(0.0, 5, 27.0, 31.0, Absent::Worst), 27.0 / 31.0);
    }
}
