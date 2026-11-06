//! Ships whose citadel sits behind a turtleback, found in each ship's armor
//! model.

use std::collections::BTreeMap;
use std::collections::HashSet;

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

use crate::armor_assessment::ArmorFingerprint;
use crate::armor_assessment::parallel_map;
use crate::armor_assessment::side_geometry;
use crate::citadel::Turtleback;
use crate::data::GameData;
use crate::error::QueryError;
use crate::ships::Shares;
use crate::ships::Ship;
use crate::ships::ShipClass;
use crate::ships::ShipCounts;
use crate::ships::ShipFilter;
use crate::ships::Tier;
use crate::ships::TierRange;

/// Arguments for `turtleback_search`. Every filter is optional.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TurtlebackSearchArgs {
    pub min_tier: Option<Tier>,
    pub max_tier: Option<Tier>,
    pub class: Option<ShipClass>,

    /// Nation as GameParams spells it, e.g. "Germany", "USA", "Russia".
    pub nation: Option<String>,

    /// Leave out ships in closed testing. Defaults to false.
    pub exclude_test_ships: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TurtlebackSearchOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tiers: Option<TierRange>,

    /// Ships matching the filters whose stock hull has citadel plates.
    pub considered: ShipCounts,
    pub considered_by_nation: BTreeMap<String, usize>,

    /// Ships among those considered with a turtleback; listed in `ships`.
    pub qualifying: ShipCounts,
    pub qualifying_by_nation: BTreeMap<String, usize>,

    #[serde(flatten)]
    pub shares: Shares,

    /// "T9 F. der Grosse (battleship, Germany): 120 mm deck sloped 67.2 deg
    /// behind a 300 mm belt", highest tier first.
    pub ships: Vec<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub omitted: Option<usize>,

    /// Ships left out because they share a listed ship's armor model, such
    /// as collaboration copies; the tech tree ship stands for them.
    #[serde(skip_serializing_if = "is_zero")]
    pub copies: usize,

    /// Ships matching the filters whose stock hull has no citadel plates.
    #[serde(skip_serializing_if = "is_zero")]
    pub without_citadel: usize,

    /// Ships matching the filters this game data holds no armor model for.
    #[serde(skip_serializing_if = "is_zero")]
    pub without_armor_model: usize,

    pub rule: &'static str,
}

/// What the armor model of one ship's stock hull shows.
enum CitadelFinding {
    NoArmorModel,
    NoCitadel,
    Citadel { fingerprint: ArmorFingerprint, turtleback: Option<Turtleback> },
}

impl GameData {
    /// Ships whose citadel sits behind a turtleback, by the same reading of
    /// the armor model as `citadel_profile`.
    pub fn turtleback_search(&self, args: &TurtlebackSearchArgs) -> Result<TurtlebackSearchOutput, QueryError> {
        const SHIP_LIMIT: usize = 40;

        let tiers = TierRange::from_bounds(args.min_tier, args.max_tier);
        let filter = ShipFilter {
            tiers,
            class: args.class,
            nation: args.nation.clone(),
            include_irregular: false,
            exclude_test: args.exclude_test_ships.unwrap_or(false),
        };

        // Of ships sharing one armor model, the first by group (tech tree
        // first) and name stands for them.
        let mut ships: Vec<&Ship> = self.ships.iter().filter(|ship| filter.matches(ship)).collect();
        ships.sort_by(|left, right| {
            left.group_order().cmp(&right.group_order()).then_with(|| left.name.cmp(&right.name))
        });
        let findings = parallel_map(&ships, |ship| self.citadel_finding(ship));

        let mut seen: HashSet<ArmorFingerprint> = HashSet::new();
        let mut considered: Vec<&Ship> = Vec::new();
        let mut qualifying: Vec<(&Ship, Turtleback)> = Vec::new();
        let mut copies = 0;
        let mut without_citadel = 0;
        let mut without_armor_model = 0;
        for (ship, finding) in ships.into_iter().zip(findings) {
            match finding? {
                CitadelFinding::NoArmorModel => without_armor_model += 1,
                CitadelFinding::NoCitadel => without_citadel += 1,
                CitadelFinding::Citadel { fingerprint, .. } if !seen.insert(fingerprint) => copies += 1,
                CitadelFinding::Citadel { turtleback, .. } => {
                    considered.push(ship);
                    if let Some(turtleback) = turtleback {
                        qualifying.push((ship, turtleback));
                    }
                }
            }
        }
        qualifying.sort_by(|left, right| right.0.tier.cmp(&left.0.tier).then_with(|| left.0.name.cmp(&right.0.name)));

        let considered_counts = ShipCounts::from_ships(considered.iter().copied());
        let qualifying_counts = ShipCounts::from_ships(qualifying.iter().map(|(ship, _)| *ship));
        let listed: Vec<String> =
            qualifying.iter().take(SHIP_LIMIT).map(|(ship, turtleback)| describe(ship, turtleback)).collect();

        Ok(TurtlebackSearchOutput {
            tiers,
            considered_by_nation: by_nation(considered.iter().copied()),
            qualifying_by_nation: by_nation(qualifying.iter().map(|(ship, _)| *ship)),
            shares: Shares::new(&qualifying_counts.by_class, &considered_counts),
            considered: considered_counts,
            qualifying: qualifying_counts,
            omitted: (qualifying.len() > listed.len()).then(|| qualifying.len() - listed.len()),
            ships: listed,
            copies,
            without_citadel,
            without_armor_model,
            rule: "A turtleback is an armored deck sloped 25 to 75 deg from vertical behind a near-vertical belt at least 1.5 times as thick, crossed by most horizontal rays meeting the citadel within 2 m of the waterline, with the deck's foot no higher than 1 m above the waterline. Read from the stock hull's armor model.",
        })
    }

    fn citadel_finding(&self, ship: &Ship) -> Result<CitadelFinding, QueryError> {
        let Some(meshes) = self.stock_meshes(ship)? else {
            return Ok(CitadelFinding::NoArmorModel);
        };
        let Some(geometry) = side_geometry(&meshes)? else {
            return Ok(CitadelFinding::NoCitadel);
        };

        Ok(CitadelFinding::Citadel {
            fingerprint: ArmorFingerprint::from_meshes(&meshes),
            turtleback: geometry.shape.turtleback,
        })
    }
}

fn by_nation<'a>(ships: impl Iterator<Item = &'a Ship>) -> BTreeMap<String, usize> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for ship in ships {
        *counts.entry(ship.param.nation().to_owned()).or_default() += 1;
    }

    counts
}

fn describe(ship: &Ship, turtleback: &Turtleback) -> String {
    format!(
        "{}: {} mm deck sloped {} deg behind a {} mm belt",
        ship.label(),
        turtleback.median_deck_mm.value(),
        turtleback.slope_deg.value(),
        turtleback.median_belt_mm.value()
    )
}

fn is_zero(value: &usize) -> bool {
    *value == 0
}
