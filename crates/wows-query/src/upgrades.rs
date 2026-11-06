use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use wows_core::recognized::Recognized;
use wowsunpack::Rc;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::translations::translate_module;
use wowsunpack::game_params::types::Credits;
use wowsunpack::game_params::types::Modernization;
use wowsunpack::game_params::types::ModernizationKind;
use wowsunpack::game_params::types::Param;
use wowsunpack::game_params::types::Species;

use crate::data::GameData;
use crate::error::QueryError;
use crate::fuzzy;
use crate::fuzzy::Candidate;
use crate::fuzzy::Fuzzy;
use crate::ships::Ship;
use crate::ships::ShipArgs;
use crate::ships::ShipClass;
use crate::ships::ShipSummary;
use crate::ships::Tier;
use crate::ships::search_key;
use crate::text::clean;

/// A mountable upgrade indexed by its English name.
pub(crate) struct Upgrade {
    pub(crate) param: Rc<Param>,
    pub(crate) name: String,
    pub(crate) search_key: String,

    /// 0-based slot.
    pub(crate) slot: u8,
}

impl Upgrade {
    pub(crate) fn from_param(provider: &GameMetadataProvider, param: &Rc<Param>) -> Option<Self> {
        let slot = param.modernization()?.slot()?;
        let name = translate_module(param.name(), provider).0.map(|name| clean(&name))?;

        Some(Self { param: Rc::clone(param), search_key: search_key(&name), name, slot })
    }

    pub(crate) fn modernization(&self) -> &Modernization {
        self.param.modernization().expect("indexed upgrades are modernizations")
    }

    pub(crate) fn fits(&self, ship: &Ship) -> bool {
        let Some(vehicle) = ship.param.vehicle() else {
            return false;
        };

        self.modernization().applies_to(
            ship.param.name(),
            vehicle.level(),
            ship.class.species().name(),
            ship.param.nation(),
            vehicle.group(),
        )
    }

    /// 1-based slot as the port shows it.
    pub(crate) fn port_slot(&self) -> u8 {
        self.slot + 1
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpgradeKind {
    Regular,
    /// Changes one consumable.
    ConsumableModification,
    /// Unique (legendary) upgrade for specific ships.
    Unique,
    /// A GameParams `type` this crate does not recognize.
    Unknown(i64),
}

impl UpgradeKind {
    fn from_modernization(modernization: &Modernization) -> Option<Self> {
        modernization.kind().map(|kind| match kind {
            Recognized::Known(ModernizationKind::Regular) => Self::Regular,
            Recognized::Known(ModernizationKind::ConsumableModification) => Self::ConsumableModification,
            Recognized::Known(ModernizationKind::Unique) => Self::Unique,
            Recognized::Unknown(raw) => Self::Unknown(*raw),
        })
    }
}

/// Arguments for `upgrade`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpgradeArgs {
    /// Upgrade name, e.g. "Main Battery Modification 2" or "Concealment System Modification 1".
    pub upgrade: String,

    /// Ship to check the upgrade against; also selects the class for effects.
    pub ship: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpgradeOutput {
    /// Usually one. Several when upgrades share a name.
    pub upgrades: Vec<UpgradeDetails>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpgradeDetails {
    pub name: String,
    pub param: String,

    /// 1-based slot as the port shows it.
    pub slot: u8,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<UpgradeKind>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_credits: Option<Credits>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Effects, grouped by classes that share them.
    pub effects: Vec<UpgradeClassEffects>,

    /// Tiers it fits, together with the class and nation criteria.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tiers: Vec<Tier>,

    /// Nations it fits. Absent when it fits every nation, or lists none.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub nations: Vec<String>,

    /// Ships it fits regardless of the other criteria.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ships: Vec<String>,

    /// Whether it fits the ship named in the arguments.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fits_ship: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UpgradeClassEffects {
    pub classes: Vec<ShipClass>,
    pub effects: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShipUpgrades {
    pub ship: ShipSummary,
    pub slots: Vec<UpgradeSlot>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpgradeSlot {
    /// 1-based slot as the port shows it.
    pub slot: u8,
    pub options: Vec<UpgradeOption>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpgradeOption {
    pub name: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<UpgradeKind>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_credits: Option<Credits>,

    pub effects: Vec<String>,
}

impl GameData {
    /// One upgrade: slot, price, effects and which ships it fits.
    pub fn upgrade(&self, args: &UpgradeArgs) -> Result<UpgradeOutput, QueryError> {
        let ship = match args.ship.as_deref() {
            Some(name) => Some(self.resolve_ship(name)?),
            None => None,
        };
        let matches = self.match_upgrades(&args.upgrade)?;

        let upgrades = matches.into_iter().map(|upgrade| self.upgrade_details(upgrade, ship)).collect();

        Ok(UpgradeOutput { upgrades })
    }

    /// Every upgrade that fits one ship, by slot.
    pub fn ship_upgrades(&self, args: &ShipArgs) -> Result<ShipUpgrades, QueryError> {
        let ship = self.resolve_ship(&args.ship)?;
        let species = ship.class.species();

        let mut slots: BTreeMap<u8, Vec<UpgradeOption>> = BTreeMap::new();
        for upgrade in self.upgrades.iter().filter(|upgrade| upgrade.fits(ship)) {
            let modernization = upgrade.modernization();
            slots.entry(upgrade.port_slot()).or_default().push(UpgradeOption {
                name: upgrade.name.clone(),
                kind: UpgradeKind::from_modernization(modernization),
                cost_credits: modernization.cost_credits(),
                effects: self.describe(modernization.modifiers(), species),
            });
        }
        let slots = slots.into_iter().map(|(slot, options)| UpgradeSlot { slot, options }).collect();

        Ok(ShipUpgrades { ship: ship.summary(), slots })
    }

    /// The upgrade named `query` that fits `ship`.
    pub(crate) fn resolve_upgrade_for_ship(&self, query: &str, ship: &Ship) -> Result<&Upgrade, QueryError> {
        let matches = self.match_upgrades(query)?;
        let Some(first) = matches.first() else {
            return Err(QueryError::UpgradeNotFound { query: query.to_owned(), suggestions: Vec::new() });
        };

        matches
            .iter()
            .copied()
            .find(|upgrade| upgrade.fits(ship))
            .ok_or_else(|| QueryError::UpgradeNotApplicable { upgrade: first.name.clone(), ship: ship.name.clone() })
    }

    fn match_upgrades(&self, query: &str) -> Result<Vec<&Upgrade>, QueryError> {
        let key = search_key(query);

        let exact: Vec<&Upgrade> = self
            .upgrades
            .iter()
            .filter(|upgrade| upgrade.search_key == key || upgrade.param.name().eq_ignore_ascii_case(query.trim()))
            .collect();
        if !exact.is_empty() {
            return Ok(exact);
        }

        let matching = |accept: &dyn Fn(&str) -> bool| -> Vec<&Upgrade> {
            self.upgrades.iter().filter(|upgrade| !key.is_empty() && accept(&upgrade.search_key)).collect()
        };
        let prefix = matching(&|name| name.starts_with(key.as_str()));
        let partial = if prefix.is_empty() { matching(&|name| name.contains(key.as_str())) } else { prefix };
        let mut distinct: Vec<&str> = partial.iter().map(|upgrade| upgrade.name.as_str()).collect();
        distinct.sort();
        distinct.dedup();
        match distinct.len() {
            0 => {}
            1 => return Ok(partial),
            _ => {
                return Err(QueryError::AmbiguousUpgrade {
                    query: query.to_owned(),
                    candidates: distinct.into_iter().take(15).map(str::to_owned).collect(),
                });
            }
        }

        match fuzzy::closest(
            &key,
            self.upgrades.iter().map(|upgrade| Candidate::primary(&upgrade.search_key, &upgrade.name)),
        ) {
            Fuzzy::Unique(name) => Ok(self.upgrades.iter().filter(|upgrade| upgrade.name == name).collect()),
            Fuzzy::Suggestions(suggestions) => Err(QueryError::UpgradeNotFound {
                query: query.to_owned(),
                suggestions: suggestions.into_iter().map(str::to_owned).collect(),
            }),
        }
    }

    fn upgrade_details(&self, upgrade: &Upgrade, ship: Option<&Ship>) -> UpgradeDetails {
        let modernization = upgrade.modernization();

        let classes: Vec<ShipClass> = match ship {
            Some(ship) => vec![ship.class],
            None => {
                let listed: Vec<ShipClass> = modernization
                    .ship_types()
                    .iter()
                    .filter_map(|name| Species::from_name(name).known().copied())
                    .filter_map(ShipClass::from_species)
                    .collect();
                if listed.is_empty() { ShipClass::ALL.to_vec() } else { listed }
            }
        };
        let mut effects: Vec<UpgradeClassEffects> = Vec::new();
        for class in classes {
            let lines = self.describe(modernization.modifiers(), class.species());
            match effects.iter_mut().find(|group| group.effects == lines) {
                Some(group) => group.classes.push(class),
                None => effects.push(UpgradeClassEffects { classes: vec![class], effects: lines }),
            }
        }

        let mut tiers: Vec<Tier> = modernization.ship_levels().iter().map(|level| Tier(*level)).collect();
        tiers.sort();
        tiers.dedup();

        UpgradeDetails {
            name: upgrade.name.clone(),
            param: upgrade.param.name().to_owned(),
            slot: upgrade.port_slot(),
            kind: UpgradeKind::from_modernization(modernization),
            cost_credits: modernization.cost_credits(),
            description: translate_module(upgrade.param.name(), self.provider()).1.map(|text| clean(&text)),
            effects,
            tiers,
            nations: self.listed_nations(modernization.nations()),
            ships: modernization.ships().iter().map(|name| self.ship_display_name(name)).collect(),
            fits_ship: ship.map(|ship| upgrade.fits(ship)),
        }
    }

    /// `nations` unless it covers every nation with regular ships.
    fn listed_nations(&self, nations: &[String]) -> Vec<String> {
        let covers_all = self
            .ships
            .iter()
            .filter(|ship| ship.is_regular())
            .all(|ship| nations.iter().any(|nation| nation == ship.param.nation()));

        if covers_all { Vec::new() } else { nations.to_vec() }
    }
}
