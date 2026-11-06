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
use crate::names::Interpretations;
use crate::names::Lookup;
use crate::names::NameIndex;
use crate::names::NameKind;
use crate::names::ResolutionMethod;
use crate::ships::Ship;
use crate::ships::ShipArgs;
use crate::ships::ShipClass;
use crate::ships::ShipSummary;
use crate::ships::Tier;
use crate::text::clean;

/// Community names of upgrades, with the GameParams name of the upgrade.
/// Every target is checked by the `alias_tables_resolve_to_their_targets` test.
pub(crate) const UPGRADE_ALIASES: &[(&str, &str)] = &[
    ("CSM", "PCM027_ConcealmentMeasures_Mod_I"),
    ("Concealment Module", "PCM027_ConcealmentMeasures_Mod_I"),
    ("Radar Mod", "PCM042_RLSSearch_Mod_I"),
    ("Hydro Mod", "PCM041_SonarSearch_Mod_I"),
    ("Smoke Mod", "PCM037_SmokeGenerator_Mod_I"),
];

/// A mountable upgrade indexed by its English name.
pub(crate) struct Upgrade {
    pub(crate) param: Rc<Param>,
    pub(crate) name: String,

    /// 0-based slot.
    pub(crate) slot: u8,
}

impl Upgrade {
    pub(crate) fn from_param(provider: &GameMetadataProvider, param: &Rc<Param>) -> Option<Self> {
        let slot = param.modernization()?.slot()?;
        // Some names break across two lines in the port.
        let name = translate_module(param.name(), provider).0.map(|name| clean(&name.replace('\n', " ")))?;

        Some(Self { param: Rc::clone(param), name, slot })
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
    /// Names read as something they do not spell exactly.
    #[serde(skip_serializing_if = "Interpretations::is_empty")]
    pub interpretations: Interpretations,

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
    /// Names read as something they do not spell exactly.
    #[serde(skip_serializing_if = "Interpretations::is_empty")]
    pub interpretations: Interpretations,

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
        let mut interpretations = Interpretations::default();
        let ship = match args.ship.as_deref() {
            Some(name) => Some(self.resolve_ship(name, &mut interpretations)?),
            None => None,
        };
        let matches = self.match_upgrades(&args.upgrade, &mut interpretations)?;

        let upgrades = matches.into_iter().map(|upgrade| self.upgrade_details(upgrade, ship)).collect();

        Ok(UpgradeOutput { interpretations, upgrades })
    }

    /// Every upgrade that fits one ship, by slot.
    pub fn ship_upgrades(&self, args: &ShipArgs) -> Result<ShipUpgrades, QueryError> {
        let mut interpretations = Interpretations::default();
        let ship = self.resolve_ship(&args.ship, &mut interpretations)?;
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

        Ok(ShipUpgrades { interpretations, ship: ship.summary(), slots })
    }

    /// The upgrade named `query` that fits `ship`.
    pub(crate) fn resolve_upgrade_for_ship(
        &self,
        query: &str,
        ship: &Ship,
        interpretations: &mut Interpretations,
    ) -> Result<&Upgrade, QueryError> {
        let matches = self.match_upgrades(query, interpretations)?;
        let Some(first) = matches.first() else {
            return Err(QueryError::UpgradeNotFound { query: query.to_owned(), suggestions: Vec::new() });
        };

        matches
            .iter()
            .copied()
            .find(|upgrade| upgrade.fits(ship))
            .ok_or_else(|| QueryError::UpgradeNotApplicable { upgrade: first.name.clone(), ship: ship.name.clone() })
    }

    /// Upgrades a player-written name, abbreviation or GameParams name refers
    /// to; several when upgrades share a name.
    fn match_upgrades(&self, query: &str, interpretations: &mut Interpretations) -> Result<Vec<&Upgrade>, QueryError> {
        match self.upgrade_names.lookup(query) {
            Lookup::Found(resolved, targets) => {
                interpretations.note(query, &resolved, NameKind::Upgrade);
                Ok(targets.iter().map(|index| &self.upgrades[*index]).collect())
            }
            Lookup::Ambiguous(candidates) => Err(QueryError::AmbiguousUpgrade { query: query.to_owned(), candidates }),
            Lookup::NotFound(suggestions) => Err(QueryError::UpgradeNotFound { query: query.to_owned(), suggestions }),
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

/// Upgrade names, GameParams names and indexes, and [`UPGRADE_ALIASES`].
pub(crate) fn upgrade_names(upgrades: &[Upgrade]) -> NameIndex<usize> {
    let mut names = NameIndex::default();
    for (index, upgrade) in upgrades.iter().enumerate() {
        names.add_name(&upgrade.name, index, false);
        names.add_id(upgrade.param.name(), &upgrade.name, index);
        names.add_id(upgrade.param.index(), &upgrade.name, index);
    }
    for (alias, param) in UPGRADE_ALIASES {
        names.add_alias(alias, param, ResolutionMethod::ExactAlias);
    }
    names
}
