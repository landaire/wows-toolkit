//! What one ship carries for the selected hull and modules: guns, torpedoes,
//! depth charges, aircraft and consumables. Every category the ship lacks is
//! reported as `none`.

use std::cmp::Reverse;
use std::collections::BTreeMap;

use serde::Serialize;
use serde::Serializer;
use wows_core::units::Meters;
use wows_core::units::Millimeters;
use wows_ship_stats::mounts::selected_mounts;
use wowsunpack::game_params::keys::ComponentType;
use wowsunpack::game_params::translations::translate_unit;
use wowsunpack::game_params::ttx::ShipUpgradeSelection;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::MountPoint;

use crate::aircraft::OrdnanceKind;
use crate::aircraft::Squadron;
use crate::aircraft::SquadronRole;
use crate::data::GameData;
use crate::firing_arcs::round_to;
use crate::modules::Module;
use crate::modules::ModuleKind;
use crate::shells::shell_ammo;
use crate::ships::Ship;
use crate::text::clean;

/// A category of armament the ship either carries or lacks. Serializes as the
/// carried value, or as the string "none".
#[derive(Debug, Clone, PartialEq)]
pub enum Equipped<T> {
    None,
    Some(T),
}

impl<T> Equipped<Vec<T>> {
    fn from_list(items: Vec<T>) -> Self {
        if items.is_empty() { Self::None } else { Self::Some(items) }
    }
}

impl<T> Serialize for Equipped<T>
where
    T: Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::None => serializer.serialize_str("none"),
            Self::Some(value) => value.serialize(serializer),
        }
    }
}

/// Number of gun mounts or torpedo launchers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct MountCount(pub u32);

/// Number of barrels or tubes, on one mount or on several.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct BarrelCount(pub u32);

/// The armament of one ship with one module selection.
#[derive(Debug, Clone, Serialize)]
pub struct Armament {
    /// The mounted hull, gun and torpedo modules.
    pub modules: Vec<ModuleChoice>,

    pub main_battery: Equipped<Vec<GunGroup>>,
    pub secondary_battery: Equipped<Vec<GunGroup>>,
    pub torpedoes: Equipped<TorpedoArmament>,

    /// Launchers on the ship. An anti-submarine air strike is listed under
    /// `aircraft`.
    pub depth_charge_launchers: Equipped<MountCount>,

    pub aircraft: Equipped<AircraftArmament>,
    pub consumables: Equipped<Vec<ConsumableChoice>>,

    /// Hull, gun and torpedo modules not mounted here, to pass in `modules`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub other_modules: Vec<ModuleChoice>,

    /// Squadrons of researched plane modules.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub other_squadrons: Vec<SquadronLine>,
}

/// A module by kind and the name `modules` arguments accept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModuleChoice {
    pub kind: ModuleKind,
    pub name: String,
}

/// Mounts of one gun type.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GunGroup {
    /// Mounts x barrels per mount and caliber, e.g. "2 x 2 152 mm".
    pub layout: String,

    pub mounts: MountCount,
    pub barrels_per_mount: BarrelCount,

    /// Absent when the mount lists no shell.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caliber_mm: Option<Millimeters>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TorpedoArmament {
    pub launchers: Vec<LauncherGroup>,
    pub torpedoes: Vec<Torpedo>,
}

/// Launchers with the same number of tubes.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LauncherGroup {
    /// Launchers x tubes per launcher, e.g. "3 x 5".
    pub layout: String,

    pub launchers: MountCount,
    pub tubes_per_launcher: BarrelCount,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Torpedo {
    pub name: String,

    /// GameParams name of the torpedo.
    pub param: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub caliber_mm: Option<Millimeters>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AircraftArmament {
    /// Carrier and hybrid squadrons of the stock plane modules.
    pub squadrons: Equipped<Vec<SquadronLine>>,

    pub air_strike: Equipped<AirStrikeLine>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SquadronLine {
    pub role: SquadronRole,
    pub aircraft: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub squadron_size: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub ordnance: Option<OrdnanceKind>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AirStrikeLine {
    pub aircraft: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub ordnance: Option<OrdnanceKind>,

    /// Times the air strike can be called.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uses: Option<u32>,
}

/// One consumable slot. A slot with several options mounts one of them.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ConsumableChoice {
    /// 1-based.
    pub slot: usize,
    pub options: Vec<String>,
}

impl GameData {
    /// The armament of `ship` with the modules in `selection`.
    pub(crate) fn armament(&self, ship: &Ship, selection: &ShipUpgradeSelection) -> Armament {
        let modules = self.ship_modules(ship);
        let armed = [ModuleKind::Hull, ModuleKind::Guns, ModuleKind::Torpedoes];
        let mounted = |param: &str| {
            [selection.hull.as_deref(), selection.artillery.as_deref(), selection.torpedoes.as_deref()]
                .contains(&Some(param))
        };
        let (mounted_modules, other_modules): (Vec<&Module>, Vec<&Module>) =
            modules.iter().filter(|module| armed.contains(&module.kind)).partition(|module| mounted(&module.param));
        let choice = |module: &&Module| ModuleChoice { kind: module.kind, name: module.name.clone() };

        let (stock_squadrons, upgraded_squadrons): (Vec<Squadron>, Vec<Squadron>) =
            self.carrier_squadrons(ship).into_iter().partition(|squadron| !squadron.module_upgrade);
        let mut other_squadrons: Vec<SquadronLine> = Vec::new();
        for line in upgraded_squadrons.iter().map(squadron_line) {
            if !other_squadrons.contains(&line) {
                other_squadrons.push(line);
            }
        }

        let main = selected_mounts(&ship.param, selection, ComponentType::Artillery);
        let secondaries = selected_mounts(&ship.param, selection, ComponentType::Atba);
        let torpedoes = selected_mounts(&ship.param, selection, ComponentType::Torpedoes);
        let depth_charges = selected_mounts(&ship.param, selection, ComponentType::DepthCharges).len();
        let depth_charge_launchers = match u32::try_from(depth_charges).expect("a hull has fewer than 2^32 mounts") {
            0 => Equipped::None,
            count => Equipped::Some(MountCount(count)),
        };

        Armament {
            modules: mounted_modules.iter().map(choice).collect(),
            main_battery: Equipped::from_list(self.gun_groups(main)),
            secondary_battery: Equipped::from_list(self.gun_groups(secondaries)),
            torpedoes: self.torpedo_armament(torpedoes),
            depth_charge_launchers,
            aircraft: self.aircraft_armament(ship, selection, &stock_squadrons),
            consumables: Equipped::from_list(self.consumable_choices(ship)),
            other_modules: other_modules.iter().map(choice).collect(),
            other_squadrons,
        }
    }

    /// Mounts grouped by caliber and barrel count, largest caliber first and
    /// mounts without a known caliber last.
    fn gun_groups(&self, mounts: &[MountPoint]) -> Vec<GunGroup> {
        // Calibers are keyed in tenths of a millimeter, the precision
        // `projectile_caliber` keeps, so they order as integers.
        let mut groups: BTreeMap<(Reverse<Option<u32>>, Reverse<u32>), u32> = BTreeMap::new();
        for mount in mounts {
            let Some(barrels) = mount.num_barrels() else {
                continue;
            };
            let tenths = self.mount_caliber(mount).map(|caliber| (caliber.value() * 10.0).round() as u32);
            *groups.entry((Reverse(tenths), Reverse(barrels))).or_default() += 1;
        }

        groups
            .into_iter()
            .map(|((Reverse(tenths), Reverse(barrels)), count)| {
                let caliber = tenths.map(|tenths| Millimeters::new(tenths as f32 / 10.0));
                let layout = match caliber {
                    Some(caliber) => format!("{count} x {barrels} {} mm", caliber.value()),
                    None => format!("{count} x {barrels}"),
                };
                GunGroup {
                    layout,
                    mounts: MountCount(count),
                    barrels_per_mount: BarrelCount(barrels),
                    caliber_mm: caliber,
                }
            })
            .collect()
    }

    fn torpedo_armament(&self, mounts: &[MountPoint]) -> Equipped<TorpedoArmament> {
        let mut groups: BTreeMap<Reverse<u32>, u32> = BTreeMap::new();
        let mut names: Vec<&String> = Vec::new();
        for mount in mounts {
            let Some(tubes) = mount.num_barrels() else {
                continue;
            };
            *groups.entry(Reverse(tubes)).or_default() += 1;
            for name in mount.ammo() {
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
        if groups.is_empty() {
            return Equipped::None;
        }
        names.sort();

        let launchers = groups
            .into_iter()
            .map(|(Reverse(tubes), count)| LauncherGroup {
                layout: format!("{count} x {tubes}"),
                launchers: MountCount(count),
                tubes_per_launcher: BarrelCount(tubes),
            })
            .collect();
        let torpedoes = names
            .into_iter()
            .map(|param| Torpedo {
                name: self.projectile_name(param),
                param: param.clone(),
                caliber_mm: self.projectile_caliber(param),
            })
            .collect();

        Equipped::Some(TorpedoArmament { launchers, torpedoes })
    }

    /// The stock squadrons and the selected hull's air strike.
    fn aircraft_armament(
        &self,
        ship: &Ship,
        selection: &ShipUpgradeSelection,
        squadrons: &[Squadron],
    ) -> Equipped<AircraftArmament> {
        let squadrons: Vec<SquadronLine> = squadrons.iter().map(squadron_line).collect();

        let strike = ship
            .param
            .vehicle()
            .and_then(|vehicle| vehicle.ttx_components())
            .zip(selection.hull.as_deref())
            .and_then(|(components, hull)| components.air_supports.get(hull));
        let air_strike = strike.and_then(|strike| {
            let squadron = strike.planes.iter().find_map(|plane| self.squadron(plane, SquadronRole::AirStrike))?;
            Some(AirStrikeLine {
                aircraft: squadron.aircraft.clone(),
                ordnance: ordnance_kind(&squadron),
                uses: strike.charges,
            })
        });

        if squadrons.is_empty() && air_strike.is_none() {
            return Equipped::None;
        }

        Equipped::Some(AircraftArmament {
            squadrons: Equipped::from_list(squadrons),
            air_strike: air_strike.map_or(Equipped::None, Equipped::Some),
        })
    }

    fn consumable_choices(&self, ship: &Ship) -> Vec<ConsumableChoice> {
        let slots = ship.param.vehicle().and_then(|vehicle| vehicle.abilities()).unwrap_or_default();

        slots
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| {
                let options: Vec<String> = slot
                    .iter()
                    .filter_map(|(ability, variant)| {
                        let param = self.provider().game_param_by_name(ability)?;
                        let category = param.ability()?.get_category(variant)?;
                        Some(self.consumable_name(ability, category))
                    })
                    .collect();
                (!options.is_empty()).then_some(ConsumableChoice { slot: index + 1, options })
            })
            .collect()
    }

    /// Caliber of the first shell a gun mount fires.
    fn mount_caliber(&self, mount: &MountPoint) -> Option<Millimeters> {
        mount.ammo().iter().find_map(|name| self.projectile_caliber(name))
    }

    fn projectile_caliber(&self, name: &str) -> Option<Millimeters> {
        let param = self.provider().game_param_by_name(name)?;
        let meters = param.projectile()?.bullet_diametr()?;

        // Calibers such as 76.2 mm keep one decimal.
        Some(Millimeters::new(round_to(Meters::new(meters).to_mm().value(), 10.0)))
    }

    /// English name of a unit such as a hull or module, or its GameParams
    /// name when it has none.
    pub(crate) fn unit_name(&self, param: &str) -> String {
        translate_unit(param, self.provider()).map_or_else(|| param.to_owned(), |name| clean(&name))
    }

    /// English name of a projectile from the translation catalog. When the
    /// catalog has none, a name built from its data, e.g. "457 mm HE", so a
    /// GameParams name never reaches a reader.
    pub(crate) fn projectile_name(&self, param: &str) -> String {
        if let Some(name) = translate_unit(param, self.provider()) {
            return clean(&name);
        }

        let kind = self.provider().game_param_by_name(param).and_then(|entry| {
            let projectile = entry.projectile()?;
            let kind = match shell_ammo(projectile) {
                Some(ammo) => ammo.label().to_owned(),
                None => projectile.ammo_type().to_ascii_lowercase().replace('_', " "),
            };
            Some(kind)
        });
        let kind = kind.filter(|kind| !kind.is_empty()).unwrap_or_else(|| "projectile".to_owned());

        match self.projectile_caliber(param) {
            Some(caliber) => format!("{caliber} {kind}"),
            None => kind,
        }
    }

    /// A stat row qualifier as players read it: a projectile's English name in
    /// place of its GameParams name, an aircraft's ordnance and English name
    /// in place of the aircraft's, any other qualifier unchanged.
    pub(crate) fn qualifier_name(&self, qualifier: &str) -> String {
        let Some(entry) = self.provider().game_param_by_name(qualifier) else {
            return qualifier.to_owned();
        };
        if entry.projectile().is_some() {
            return self.projectile_name(qualifier);
        }
        if entry.aircraft().is_none() {
            return qualifier.to_owned();
        }

        // Every role carries the same ordnance; the role only labels it.
        match self.squadron(qualifier, SquadronRole::DiveBombers).and_then(|squadron| squadron.ordnance) {
            Some(ordnance) => format!("{} {} on {}", ordnance.name, ordnance.kind.label(), self.aircraft_name(&entry)),
            None => self.aircraft_name(&entry),
        }
    }
}

fn ordnance_kind(squadron: &Squadron) -> Option<OrdnanceKind> {
    squadron.ordnance.as_ref().map(|ordnance| ordnance.kind.clone())
}

fn squadron_line(squadron: &Squadron) -> SquadronLine {
    SquadronLine {
        role: squadron.role,
        aircraft: squadron.aircraft.clone(),
        squadron_size: squadron.squadron_size,
        ordnance: ordnance_kind(squadron),
    }
}
