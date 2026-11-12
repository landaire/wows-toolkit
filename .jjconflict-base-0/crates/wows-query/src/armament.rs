//! What one ship carries for the selected hull and modules: guns, torpedoes,
//! anti-submarine weapons, aircraft and consumables. Every category the ship
//! lacks is reported as `none`.

use std::cmp::Reverse;
use std::collections::BTreeMap;

use serde::Serialize;
use serde::Serializer;
use wows_core::units::Km;
use wows_core::units::Meters;
use wows_core::units::Millimeters;
use wows_core::units::Seconds;
use wows_ship_stats::mounts::BarrelCount;
use wows_ship_stats::mounts::MountCount;
use wows_ship_stats::mounts::selected_mounts;
use wowsunpack::game_params::keys::ComponentType;
use wowsunpack::game_params::translations::translate_unit;
use wowsunpack::game_params::ttx::ShipUpgradeSelection;
use wowsunpack::game_params::ttx::model::Hp;
use wowsunpack::game_params::ttx::modifiers::ModifierBundle;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::MountPoint;

use crate::aircraft::OrdnanceKind;
use crate::aircraft::Squadron;
use crate::aircraft::SquadronRole;
use crate::builds::Charges;
use crate::consumables::ConsumableStats;
use crate::consumables::consumable_stats;
use crate::consumables::slot_choice;
use crate::data::GameData;
use crate::error::QueryError;
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

/// The armament of one ship with one module selection.
#[derive(Debug, Clone, Serialize)]
pub struct Armament {
    /// The mounted hull, gun and torpedo modules.
    pub modules: Vec<ModuleChoice>,

    pub main_battery: Equipped<Vec<GunGroup>>,
    pub secondary_battery: Equipped<Vec<GunGroup>>,
    pub torpedoes: Equipped<TorpedoArmament>,

    /// Ship-mounted depth charges, the depth charge air strike and submarine
    /// detection consumables, each stated present or absent.
    pub anti_submarine: AntiSubmarine,

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

    /// "choose one of: A, B" when the slot offers several consumables.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub choice: Option<String>,

    pub options: Vec<String>,
}

/// What a ship has against submarines. Ship-mounted depth charges and the
/// depth charge air strike are separate weapons: lacking one says nothing of
/// the other.
#[derive(Debug, Clone, Serialize)]
pub struct AntiSubmarine {
    /// Both kinds of weapon and the detection consumables in one sentence,
    /// e.g. "no ship-mounted depth charge launchers; has a depth charge air
    /// strike: ...".
    pub summary: String,

    pub depth_charge_launchers: Equipped<DepthChargeLaunchers>,
    pub depth_charge_air_strike: Equipped<DepthChargeAirStrike>,
    pub submarine_detection: Equipped<Vec<SubmarineDetection>>,
}

/// Depth charges dropped or thrown from the ship itself, stock, as the port
/// shows them (FactoryDepthCharges.py:8-15).
#[derive(Debug, Clone, Serialize)]
pub struct DepthChargeLaunchers {
    pub launchers: MountCount,

    /// Attacks before a reload (`maxPacks`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub charges: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub reload_s: Option<Seconds>,

    /// Every launcher's depth charges per shot, times shots per attack.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depth_charges_per_attack: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub damage_per_depth_charge: Option<Hp>,

    /// Radius of one depth charge's blast (`depthSplashRadius`) in the
    /// game world's meters, the scale ship models are drawn at; full damage
    /// within the inner part of it, a third beyond (`pointsOfDamage`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub splash_radius_m: Option<Meters>,
}

/// An air strike whose aircraft drop depth charges, called from the ship.
#[derive(Debug, Clone, Serialize)]
pub struct DepthChargeAirStrike {
    pub aircraft: String,

    /// Planes one strike sends (`attackerSize`, which the client keeps per
    /// air strike plane, ShipParams.py:275).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub planes: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub depth_charges_per_plane: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub damage_per_depth_charge: Option<Hp>,

    /// As [`DepthChargeLaunchers::splash_radius_m`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub splash_radius_m: Option<Meters>,

    /// Strikes before a reload.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub charges: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub reload_s: Option<Seconds>,

    /// Farthest point the strike can be called on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range_km: Option<Km>,
}

impl DepthChargeAirStrike {
    /// Depth charges one strike drops: every plane's.
    pub fn depth_charges_per_strike(&self) -> Option<u32> {
        Some(self.planes? * self.depth_charges_per_plane?)
    }
}

/// The anti-submarine weapons of one hull.
pub(crate) struct AswWeapons {
    pub(crate) launchers: Option<DepthChargeLaunchers>,
    pub(crate) air_strike: Option<DepthChargeAirStrike>,
}

/// A consumable that reveals submarines, stock.
#[derive(Debug, Clone, Serialize)]
pub struct SubmarineDetection {
    pub consumable: String,

    /// 1-based consumable slot.
    pub slot: usize,

    /// "choose one of: A, B" when the slot offers other consumables.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub choice: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub detection_m: Option<Meters>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub charges: Option<Charges>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub action_time_s: Option<Seconds>,

    pub reload_s: Seconds,
}

impl GameData {
    /// The armament of `ship` with the modules in `selection`.
    pub(crate) fn armament(&self, ship: &Ship, selection: &ShipUpgradeSelection) -> Result<Armament, QueryError> {
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
        let stock = self.effective_modifiers(ship, &self.stock_loadout())?;

        Ok(Armament {
            modules: mounted_modules.iter().map(choice).collect(),
            main_battery: Equipped::from_list(self.gun_groups(main)),
            secondary_battery: Equipped::from_list(self.gun_groups(secondaries)),
            torpedoes: self.torpedo_armament(torpedoes),
            anti_submarine: self.anti_submarine(ship, selection, stock.build.modifiers.bundle()),
            aircraft: self.aircraft_armament(ship, selection, &stock_squadrons),
            consumables: Equipped::from_list(self.consumable_choices(ship)),
            other_modules: other_modules.iter().map(choice).collect(),
            other_squadrons,
        })
    }

    /// Ship-mounted depth charges and the depth charge air strike of `hull`,
    /// base values without skills or upgrades.
    pub(crate) fn asw_weapons(&self, ship: &Ship, hull: Option<&str>) -> AswWeapons {
        let components = ship.param.vehicle().and_then(|vehicle| vehicle.ttx_components());
        let projectile = |name: &str| self.provider().game_param_by_name(name);
        let damage_of = |name: &str| {
            let damage = projectile(name)?.projectile()?.alpha_damage()?;
            Some(Hp::from(damage.round()))
        };
        let splash_of = |name: &str| {
            projectile(name)?.projectile()?.depth_splash_radius().map(|radius| Meters::new(round_to(radius, 100.0)))
        };

        let launchers =
            components.zip(hull).and_then(|(components, hull)| components.depth_charges.get(hull)).map(|charges| {
                DepthChargeLaunchers {
                    launchers: MountCount(
                        u32::try_from(charges.launchers.len()).expect("a hull has fewer than 2^32 launchers"),
                    ),
                    charges: charges.charges,
                    reload_s: charges.reload_time.map(|reload| Seconds::new(reload.value())),
                    depth_charges_per_attack: charges.depth_charges_per_attack(),
                    damage_per_depth_charge: charges.projectile().and_then(damage_of),
                    splash_radius_m: charges.projectile().and_then(splash_of),
                }
            });

        let strike = components.zip(hull).and_then(|(components, hull)| components.air_supports.get(hull));
        let air_strike = strike.and_then(|strike| {
            let squadron = strike.planes.iter().find_map(|plane| self.squadron(plane, SquadronRole::AirStrike))?;
            let ordnance = squadron.ordnance.filter(|ordnance| ordnance.kind == OrdnanceKind::DepthCharge)?;
            Some(DepthChargeAirStrike {
                aircraft: squadron.aircraft,
                planes: squadron.attack_flight_size,
                depth_charges_per_plane: squadron.payload_per_plane,
                damage_per_depth_charge: ordnance.damage,
                splash_radius_m: splash_of(&ordnance.param),
                charges: strike.charges,
                reload_s: strike.reload_time.map(|reload| Seconds::new(reload.value())),
                range_km: strike.max_dist.map(|meters| Km::new(meters.value() / 1000.0)),
            })
        });

        AswWeapons { launchers, air_strike }
    }

    /// Ship-mounted depth charges and the depth charge air strike of the
    /// selected hull, and the stock submarine detection consumables.
    fn anti_submarine(&self, ship: &Ship, selection: &ShipUpgradeSelection, bundle: &ModifierBundle) -> AntiSubmarine {
        /// Consumable types that reveal submarines: Hydroacoustic Search and
        /// Submarine Surveillance.
        const DETECTION_TYPES: [&str; 2] = ["sonar", "submarineLocator"];

        let AswWeapons { launchers, air_strike } = self.asw_weapons(ship, selection.hull.as_deref());

        let slots = ship.param.vehicle().and_then(|vehicle| vehicle.abilities()).unwrap_or_default();
        let mut detection = Vec::new();
        for (index, slot) in slots.iter().enumerate() {
            let options: Vec<(String, Option<ConsumableStats>)> = slot
                .iter()
                .filter_map(|(ability, variant)| {
                    let param = self.provider().game_param_by_name(ability)?;
                    let category = param.ability()?.get_category(variant)?;
                    let name = self.consumable_name(ability, category);
                    let stats = DETECTION_TYPES
                        .contains(&category.consumable_type_raw())
                        .then(|| consumable_stats(name.clone(), category, bundle));
                    Some((name, stats))
                })
                .collect();
            let names: Vec<String> = options.iter().map(|(name, _)| name.clone()).collect();
            for stats in options.into_iter().filter_map(|(_, stats)| stats) {
                detection.push(SubmarineDetection {
                    consumable: stats.name,
                    slot: index + 1,
                    choice: slot_choice(&names),
                    detection_m: stats.ship_detection_m,
                    charges: stats.charges,
                    action_time_s: stats.action_time_s,
                    reload_s: stats.reload_s,
                });
            }
        }

        AntiSubmarine {
            summary: anti_submarine_summary(launchers.as_ref(), air_strike.as_ref(), &detection),
            depth_charge_launchers: launchers.map_or(Equipped::None, Equipped::Some),
            depth_charge_air_strike: air_strike.map_or(Equipped::None, Equipped::Some),
            submarine_detection: Equipped::from_list(detection),
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
                (!options.is_empty()).then(|| ConsumableChoice {
                    slot: index + 1,
                    choice: slot_choice(&options),
                    options,
                })
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
                None => projectile.ammo_type()?.to_ascii_lowercase().replace('_', " "),
            };
            Some(kind)
        });
        let kind = kind.unwrap_or_else(|| "projectile".to_owned());

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

/// Each kind of anti-submarine weapon and the detection consumables, absent
/// ones named as absent, so a missing kind is not read as the ship having
/// no depth charges at all.
fn anti_submarine_summary(
    launchers: Option<&DepthChargeLaunchers>,
    air_strike: Option<&DepthChargeAirStrike>,
    detection: &[SubmarineDetection],
) -> String {
    let details = |parts: Vec<Option<String>>| parts.into_iter().flatten().collect::<Vec<_>>().join(", ");
    let counted = |count: u32, one: &str, many: &str| format!("{count} {}", if count == 1 { one } else { many });
    let damage = |damage: Option<Hp>| damage.map(|damage| format!("{} damage each", damage.value()));
    let reload = |reload: Option<Seconds>| reload.map(|reload| format!("{} s reload", reload.value()));
    let charges = |charges: Option<u32>| charges.map(|charges| counted(charges, "charge", "charges"));

    let launcher_text = launchers.map(|launchers| {
        format!(
            "{} ({})",
            counted(launchers.launchers.0, "ship-mounted depth charge launcher", "ship-mounted depth charge launchers"),
            details(vec![
                charges(launchers.charges),
                launchers.depth_charges_per_attack.map(|count| counted(
                    count,
                    "depth charge per attack",
                    "depth charges per attack"
                )),
                damage(launchers.damage_per_depth_charge),
                reload(launchers.reload_s),
            ])
        )
    });
    let strike_text = air_strike.map(|strike| {
        let flight = match strike.planes {
            Some(planes) => format!("{planes} x {}", strike.aircraft),
            None => strike.aircraft.clone(),
        };
        format!(
            "has a depth charge air strike ({})",
            details(vec![
                Some(flight),
                strike.depth_charges_per_plane.map(|count| counted(
                    count,
                    "depth charge per plane",
                    "depth charges per plane"
                )),
                damage(strike.damage_per_depth_charge),
                charges(strike.charges),
                reload(strike.reload_s),
                strike.range_km.map(|range| format!("up to {} km away", range.value())),
            ])
        )
    });
    let weapons = match (launcher_text, strike_text) {
        (None, None) => {
            "no anti-submarine weapons: no ship-mounted depth charge launchers and no depth charge air strike"
                .to_owned()
        }
        (Some(launchers), None) => format!("{launchers}; no depth charge air strike"),
        (None, Some(strike)) => format!("no ship-mounted depth charge launchers; {strike}"),
        (Some(launchers), Some(strike)) => format!("{launchers}; {strike}"),
    };

    let detection_text = if detection.is_empty() {
        "no submarine detection consumable".to_owned()
    } else {
        let consumables: Vec<String> = detection
            .iter()
            .map(|consumable| match consumable.detection_m {
                Some(range) => format!("{} ({} km)", consumable.consumable, range.value() / 1000.0),
                None => consumable.consumable.clone(),
            })
            .collect();
        format!("submarine detection: {}", consumables.join(", "))
    };

    format!("{weapons}; {detection_text}")
}
