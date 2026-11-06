//! What a ship's ability key does: Combat Instructions (GameParams
//! `RageMode`) and the main battery's alternative fire mode, Burst Fire or
//! Modified Shells (`SwitchableModeArtilleryModule`). Players call either
//! "the F key", its default binding (update notes 0.10.10).

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use wows_core::units::DamagePerMinute;
use wows_core::units::Seconds;
use wows_ship_stats::DerivedRow;
use wows_ship_stats::DerivedStat;
use wows_ship_stats::DerivedValue;
use wowsunpack::Rc;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::TranslationKey;
use wowsunpack::game_params::ttx::ShipUpgradeSelection;
use wowsunpack::game_params::ttx::components::ArtilleryGunStats;
use wowsunpack::game_params::ttx::components::ShipTtxComponents;
use wowsunpack::game_params::ttx::special_abilities::Activation;
use wowsunpack::game_params::ttx::special_abilities::AlternativeAmmo;
use wowsunpack::game_params::ttx::special_abilities::AlternativeFireMode;
use wowsunpack::game_params::ttx::special_abilities::BurstShots;
use wowsunpack::game_params::ttx::special_abilities::CombatInstructions;
use wowsunpack::game_params::ttx::special_abilities::ProgressEvent;
use wowsunpack::game_params::ttx::special_abilities::ProgressTrigger;
use wowsunpack::game_params::types::AmmoType;
use wowsunpack::game_params::types::GameParamProvider;

use crate::builds::round;
use crate::data::GameData;
use crate::error::QueryError;
use crate::loadout::AbilityModifiers;
use crate::names::Interpretations;
use crate::names::Lookup;
use crate::ships::Shares;
use crate::ships::Ship;
use crate::ships::ShipArgs;
use crate::ships::ShipClass;
use crate::ships::ShipCounts;
use crate::ships::ShipFilter;
use crate::ships::ShipStatus;
use crate::ships::ShipSummary;
use crate::ships::Tier;
use crate::ships::TierRange;
use crate::text::clean;

const COMBAT_INSTRUCTIONS: &str = "Combat Instructions";

const USES_PER_BATTLE: &str = "No limit per battle and no cooldown or charges: each use empties the meter, which must fill to 100% again before the next use.";

const FIRE_MODE_USE: &str = "Toggled with the fire mode key (F by default) at any time except while a burst is firing; no limit per battle and no cooldown. The mode stays on until switched back.";

const NO_ABILITY: &str = "No special ability: the ship has neither Combat Instructions nor an alternative main battery fire mode, so its ability key (F by default) does nothing.";

/// Arguments for `special_ability_search`. Every filter is optional.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SpecialAbilitySearchArgs {
    pub kind: Option<AbilityKind>,

    /// Words of the ability name or of one of its effects, e.g. "secondary battery firing range", "reload", "Modified Secondary Munitions".
    pub effect: Option<String>,

    pub min_tier: Option<Tier>,
    pub max_tier: Option<Tier>,
    pub class: Option<ShipClass>,

    /// Nation as GameParams spells it, e.g. "USA", "Spain", "Pan_America".
    pub nation: Option<String>,

    /// Leave out ships in closed testing. Defaults to false: they are listed under `test`.
    pub exclude_test_ships: Option<bool>,

    /// Include event, clan-battle and retired variants. Defaults to false.
    pub include_event_variants: Option<bool>,
}

/// A kind of ship ability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AbilityKind {
    CombatInstructions,

    /// Every alternative main battery fire mode: Burst Fire, Modified Shells, or both.
    AlternativeFireMode,

    /// Alternative fire modes that fire bursts.
    BurstFire,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShipSpecialAbilityOutput {
    /// Names read as something they do not spell exactly.
    #[serde(skip_serializing_if = "Interpretations::is_empty")]
    pub interpretations: Interpretations,

    pub ship: ShipSummary,

    /// What the ability key does on this ship. Empty when it has no ability.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub abilities: Vec<SpecialAbility>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub none: Option<&'static str>,
}

/// One ability, with values for no captain skills or upgrades.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SpecialAbility {
    CombatInstructions(CombatInstructionsInfo),
    AlternativeFireMode(AlternativeFireInfo),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CombatInstructionsInfo {
    /// In-game title, e.g. "Modified Secondary Munitions".
    pub name: String,

    /// Hull modules that carry these instructions, when the hulls differ.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hulls: Vec<String>,

    pub activation: String,

    /// Each event that fills the meter and the progress it adds.
    pub charge: Vec<String>,

    /// How the meter empties while no event fills it. Absent when it keeps its progress.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress_loss: Option<String>,

    pub duration_s: Seconds,

    /// Changes while active.
    pub effects: Vec<String>,

    pub uses_per_battle: &'static str,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AlternativeFireInfo {
    /// In-game title: "Burst Fire", "Modified Shells" or "Burst Fire and Modified Shells".
    pub name: String,

    pub description: String,

    /// Main battery modules that have this mode, when they differ.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub modules: Vec<String>,

    /// Salvos each turret fires per burst. Absent without bursts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shots_per_burst: Option<BurstShots>,

    /// Reload between the salvos of one burst.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shot_interval_s: Option<Seconds>,

    /// Reload after the last salvo of a burst, or after each salvo without bursts.
    pub reload_s: Seconds,

    /// Main battery reload in the standard fire mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub standard_reload_s: Option<Seconds>,

    /// One burst and its reload: (shots - 1) x interval + reload.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cycle_s: Option<Seconds>,

    /// Changes while the mode is on, besides the reload.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<String>,

    /// Shells the mode loads instead of the standard ones, with their limit per battle.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub shells: Vec<String>,

    pub usage: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct SpecialAbilitySearchOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tiers: Option<TierRange>,

    /// Ships matching the filters other than kind and effect.
    pub considered: ShipCounts,

    /// Ships among those considered with a matching ability.
    pub qualifying: ShipCounts,

    #[serde(flatten)]
    pub shares: Shares,

    /// Qualifying ships per ability kind; a ship with both kinds counts in each.
    pub by_kind: BTreeMap<AbilityKind, usize>,

    /// Released qualifying ships, "T10 Libertad (battleship, Spain): Combat Instructions \"...\": effects", highest tier first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub released: Vec<String>,

    /// Qualifying ships in closed testing, listed the same way.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub test: Vec<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub omitted: Option<usize>,

    pub note: &'static str,
}

/// The ability key held on: the modifiers it applies and, for an
/// alternative fire mode, the ship with its main battery switched.
pub(crate) struct ActiveAbility {
    /// The ship in its alternative fire mode. `None` without one.
    pub(crate) switched: Option<Ship>,

    pub(crate) modifiers: Vec<AbilityModifiers>,

    /// Bursts of the alternative fire mode.
    pub(crate) burst: Option<Burst>,

    /// "Combat Instructions: Modified Secondary Munitions", one per ability.
    pub(crate) names: Vec<String>,
}

/// The bursts of an alternative fire mode, with no skills or upgrades.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Burst {
    pub(crate) shots: BurstShots,
    base_interval: Seconds,
    base_reload: Seconds,
}

impl Burst {
    /// `None` when the mode fires single salvos.
    fn from_mode(mode: &AlternativeFireMode) -> Option<Self> {
        mode.is_burst().then(|| Self {
            shots: mode.shots,
            base_interval: Seconds::new(mode.shot_interval.value()),
            base_reload: Seconds::new(mode.full_reload.value()),
        })
    }

    /// Reload between the salvos of a burst when the reload after it is
    /// `reload`. The client scales both by the same reload modifiers
    /// (`FactoryArtillery.createAltFireModeTTX`, `BurstFireReloader`).
    pub(crate) fn interval(self, reload: Seconds) -> Seconds {
        Seconds::new(self.base_interval.value() * reload.value() / self.base_reload.value())
    }

    /// One burst and the reload after it: (shots - 1) x interval + reload.
    pub(crate) fn cycle(self, reload: Seconds) -> Seconds {
        Seconds::new(self.shots.0.saturating_sub(1) as f32 * self.interval(reload).value() + reload.value())
    }

    /// Main battery damage per minute averaged over bursts, in place of the
    /// stat engine's one salvo per reload.
    pub(crate) fn average_damage_rates(self, derived: &mut [DerivedRow], reload: Seconds) {
        let factor = self.shots.0 as f32 * reload.value() / self.cycle(reload).value();
        for row in derived.iter_mut().filter(|row| row.stat == DerivedStat::MainBatteryDamagePerMinute) {
            if let DerivedValue::DamagePerMinute(rate) = row.value {
                row.value = DerivedValue::DamagePerMinute(DamagePerMinute::new(rate.value() * factor));
            }
        }
    }
}

impl GameData {
    /// What one ship's ability key does.
    pub fn ship_special_ability(&self, args: &ShipArgs) -> Result<ShipSpecialAbilityOutput, QueryError> {
        let mut interpretations = Interpretations::default();
        let ship = self.resolve_ship(&args.ship, &mut interpretations)?;
        let abilities = self.special_abilities(ship);

        Ok(ShipSpecialAbilityOutput {
            interpretations,
            ship: ship.summary(),
            none: abilities.is_empty().then_some(NO_ABILITY),
            abilities,
        })
    }

    /// Ships with an ability key, by kind and effect.
    pub fn special_ability_search(
        &self,
        args: &SpecialAbilitySearchArgs,
    ) -> Result<SpecialAbilitySearchOutput, QueryError> {
        const SHIP_LIMIT: usize = 150;

        let tiers = TierRange::from_bounds(args.min_tier, args.max_tier);
        let filter = ShipFilter {
            tiers,
            class: args.class,
            nation: args.nation.clone(),
            include_irregular: args.include_event_variants.unwrap_or(false),
            exclude_test: args.exclude_test_ships.unwrap_or(false),
            include_replicas: false,
        };
        let effect = args.effect.as_deref().map(str::to_lowercase);

        let considered: Vec<&Ship> = self.ships.iter().filter(|ship| filter.matches(ship)).collect();
        let mut qualifying: Vec<(&Ship, Vec<SpecialAbility>)> = Vec::new();
        for ship in &considered {
            let abilities: Vec<SpecialAbility> = self
                .special_abilities(ship)
                .into_iter()
                .filter(|ability| args.kind.is_none_or(|kind| ability.is(kind)))
                .filter(|ability| effect.as_deref().is_none_or(|words| ability.mentions(words)))
                .collect();
            if !abilities.is_empty() {
                qualifying.push((ship, abilities));
            }
        }
        qualifying.sort_by(|left, right| right.0.tier.cmp(&left.0.tier).then_with(|| left.0.name.cmp(&right.0.name)));

        let mut by_kind: BTreeMap<AbilityKind, usize> = BTreeMap::new();
        for (_, abilities) in &qualifying {
            for kind in [AbilityKind::CombatInstructions, AbilityKind::AlternativeFireMode, AbilityKind::BurstFire] {
                if abilities.iter().any(|ability| ability.is(kind)) {
                    *by_kind.entry(kind).or_default() += 1;
                }
            }
        }

        let considered_counts = ShipCounts::from_ships(considered.iter().copied());
        let qualifying_counts = ShipCounts::from_ships(qualifying.iter().map(|(ship, _)| *ship));
        let mut released = Vec::new();
        let mut test = Vec::new();
        for (ship, abilities) in qualifying.iter().take(SHIP_LIMIT) {
            let line = format!(
                "{}: {}",
                ship.listing_label(),
                abilities.iter().map(SpecialAbility::summary).collect::<Vec<_>>().join("; ")
            );
            match ship.status() {
                ShipStatus::Released => released.push(line),
                ShipStatus::Test => test.push(line),
            }
        }

        Ok(SpecialAbilitySearchOutput {
            tiers,
            shares: Shares::new(&qualifying_counts.by_class, &considered_counts),
            considered: considered_counts,
            qualifying: qualifying_counts,
            by_kind,
            omitted: (qualifying.len() > SHIP_LIMIT).then(|| qualifying.len() - SHIP_LIMIT),
            released,
            test,
            note: "Read from GameParams of the current client for each ship's modules: Combat Instructions from the hull, alternative fire modes from the main battery. Test ships may change before release. Replicas such as BA ships are named in their original's [replicas].",
        })
    }

    /// Every ability of one ship, Combat Instructions first.
    pub(crate) fn special_abilities(&self, ship: &Ship) -> Vec<SpecialAbility> {
        let Some(components) = ship.param.vehicle().and_then(|vehicle| vehicle.ttx_components()) else {
            return Vec::new();
        };
        let species = ship.class.species();

        let mut hulls: Vec<&String> = components.hulls.keys().collect();
        hulls.sort();
        let instructions =
            group_by_module(hulls.iter().map(|hull| (hull.as_str(), components.combat_instructions(hull))), |hull| {
                self.unit_name(hull)
            });

        let mut artillery: Vec<&String> = components.artillery.keys().collect();
        artillery.sort();
        let modes = group_by_module(
            artillery.iter().map(|upgrade| {
                let mode = components
                    .upgrade_components(upgrade)
                    .first()
                    .and_then(|component| components.alternative_fire(component));
                (upgrade.as_str(), mode)
            }),
            |upgrade| self.unit_name(upgrade),
        );

        let mut abilities: Vec<SpecialAbility> = instructions
            .into_iter()
            .map(|(instructions, hulls)| {
                SpecialAbility::CombatInstructions(self.describe_instructions(instructions, hulls, species))
            })
            .collect();
        for (mode, modules) in modes {
            let standard = standard_gun(ship, components, mode);
            abilities
                .push(SpecialAbility::AlternativeFireMode(self.describe_fire_mode(mode, modules, standard, species)));
        }

        abilities
    }

    /// The abilities of `ship`'s stock modules switched on.
    pub(crate) fn active_ability(&self, ship: &Ship) -> Result<ActiveAbility, QueryError> {
        let no_ability = || QueryError::NoSpecialAbility { ship: ship.name.clone() };
        let components = ship.param.vehicle().and_then(|vehicle| vehicle.ttx_components()).ok_or_else(no_ability)?;
        let stock = ShipUpgradeSelection::stock(&ship.param);

        let mut modifiers = Vec::new();
        let mut names = Vec::new();
        if let Some(instructions) = stock.hull.as_deref().and_then(|hull| components.combat_instructions(hull)) {
            let name = format!("{COMBAT_INSTRUCTIONS}: {}", self.instructions_title(instructions));
            modifiers.push(AbilityModifiers {
                source: format!("RageMode:{}", instructions.name),
                label: name.clone(),
                modifiers: instructions.modifiers.clone(),
            });
            names.push(name);
        }

        let mode = stock
            .artillery
            .as_deref()
            .and_then(|artillery| components.upgrade_components(artillery).first())
            .and_then(|component| components.alternative_fire(component));
        let mut switched = None;
        let mut burst = None;
        if let Some(mode) = mode {
            let name = self.fire_mode_title(mode);
            modifiers.push(AbilityModifiers {
                source: "SwitchableModeArtilleryModule".to_owned(),
                label: name.clone(),
                modifiers: mode.modifiers.clone(),
            });
            names.push(name);
            let param = ship.param.in_alternative_fire_mode().ok_or_else(no_ability)?;
            switched = Some(ship.with_param(Rc::new(param)));
            burst = Burst::from_mode(mode);
        }

        if names.is_empty() {
            return Err(no_ability());
        }

        Ok(ActiveAbility { switched, modifiers, burst, names })
    }

    fn instructions_title(&self, instructions: &CombatInstructions) -> String {
        self.translation(&format!("IDS_DOCK_RAGE_MODE_TITLE_{}", instructions.name.to_uppercase()))
            .unwrap_or_else(|| instructions.name.clone())
    }

    fn fire_mode_title(&self, mode: &AlternativeFireMode) -> String {
        let key = fire_mode_key(mode);
        self.translation(&format!("IDS_DOCK_FIRE_MODE_TITLE_{key}")).unwrap_or_else(|| key.to_owned())
    }

    fn translation(&self, key: &str) -> Option<String> {
        self.provider().localized_name_from_id(&TranslationKey::new(key)).map(|text| clean(&text))
    }

    fn describe_instructions(
        &self,
        instructions: &CombatInstructions,
        hulls: Vec<String>,
        species: wowsunpack::game_params::types::Species,
    ) -> CombatInstructionsInfo {
        let activation = match instructions.activation {
            Activation::Manual => "Press the Combat Instructions key (F by default) once the meter is full.".to_owned(),
            Activation::Automatic => "Starts by itself once the meter is full.".to_owned(),
        };
        let activation = match instructions.preparation {
            Some(preparation) => format!("{activation} Ready {} s after the meter fills.", preparation.value()),
            None => activation,
        };
        let progress_loss = instructions.decay.map(|decay| {
            format!(
                "After {} s without progress, the meter loses {}% every {} s.",
                decay.delay.value(),
                decay.loss.0,
                decay.period.value()
            )
        });

        CombatInstructionsInfo {
            name: self.instructions_title(instructions),
            hulls,
            activation,
            charge: instructions.triggers.iter().map(|trigger| self.trigger_line(trigger)).collect(),
            progress_loss,
            duration_s: Seconds::new(instructions.duration.value()),
            effects: self.describe(&instructions.modifiers, species),
            uses_per_battle: USES_PER_BATTLE,
        }
    }

    /// "Hit an enemy ship with a main battery shell: +6.5%", as the client
    /// words it (`RageModeSystem.buildRageModeTrigger`).
    fn trigger_line(&self, trigger: &ProgressTrigger) -> String {
        let activator = activator_name(&trigger.event);
        let name_ids = if trigger.progress_name == "default" { activator.as_str() } else { &trigger.progress_name };

        // The client fills the value of activator-named triggers and of the
        // time-based ones only.
        let value = match (&trigger.event, name_ids) {
            (ProgressEvent::Timer { period, .. }, _) => Some(format!("{}", period.value() as i64)),
            (ProgressEvent::PotentialDamage { per }, _) => Some(format!("{}", per.value() as i64)),
            (ProgressEvent::Ribbon { ribbons, sub_ribbons }, "RibbonActivator") => Some(format!(
                "sub-ribbon {}",
                ribbons.iter().chain(sub_ribbons).map(i64::to_string).collect::<Vec<_>>().join(", ")
            )),
            _ => None,
        };
        let label = self
            .translation(&format!("IDS_RAGE_MODE_TRIGGER_{}", name_ids.to_uppercase()))
            .unwrap_or_else(|| name_ids.replace('_', " "));
        let label = match value {
            Some(value) => label.replace("%(triggerValue)s", &value),
            None => label,
        };

        let mut line = format!("{label}: +{}%", trigger.gain.0);
        if trigger.required_count > 1 {
            line.push_str(&format!(" per {}", trigger.required_count));
        }
        if let Some(limit) = trigger.max_activations {
            line.push_str(&format!(", at most {limit} times"));
        }
        if let ProgressEvent::ConsumableUse { consumables } = &trigger.event
            && !consumables.is_empty()
        {
            let names: Vec<&str> = consumables
                .iter()
                .map(|kind| match self.consumable_names.lookup(kind) {
                    Lookup::Found(resolved, _) => resolved.name,
                    Lookup::Ambiguous(_) | Lookup::NotFound(_) => kind.as_str(),
                })
                .collect();
            line.push_str(&format!(" ({})", names.join(", ")));
        }

        line
    }

    fn describe_fire_mode(
        &self,
        mode: &AlternativeFireMode,
        modules: Vec<String>,
        standard: Option<&ArtilleryGunStats>,
        species: wowsunpack::game_params::types::Species,
    ) -> AlternativeFireInfo {
        let burst = Burst::from_mode(mode);
        let reload = Seconds::new(mode.full_reload.value());

        AlternativeFireInfo {
            name: self.fire_mode_title(mode),
            description: self
                .translation(&format!("IDS_DOCK_FIRE_MODE_DESCRIPTION_{}", fire_mode_key(mode)))
                .unwrap_or_default(),
            modules,
            shots_per_burst: burst.map(|burst| burst.shots),
            shot_interval_s: burst.map(|burst| burst.interval(reload)),
            reload_s: reload,
            standard_reload_s: standard.and_then(|gun| gun.shot_delay).map(|delay| Seconds::new(delay.value())),
            cycle_s: burst.map(|burst| Seconds::new(round(burst.cycle(reload).value()))),
            effects: self.describe(&mode.modifiers, species),
            shells: mode.ammo.iter().map(|ammo| self.shell_label(ammo, standard)).collect(),
            usage: FIRE_MODE_USE,
        }
    }

    /// "AP 406 mm, 60 per battle", or "HE 406 mm (standard shell)" for one
    /// the standard fire mode also loads.
    fn shell_label(&self, ammo: &AlternativeAmmo, standard: Option<&ArtilleryGunStats>) -> String {
        let shell = self
            .provider()
            .game_param_by_name(&ammo.name)
            .and_then(|param| {
                let projectile = param.projectile()?;
                let kind = AmmoType::from_game_str(projectile.ammo_type());
                let caliber = projectile.bullet_diametr()?;
                Some(format!("{} {} mm", kind.display_name(), (caliber * 1000.0).round()))
            })
            .unwrap_or_else(|| ammo.name.clone());
        let shell = if standard.is_some_and(|gun| gun.ammo.contains(&ammo.name)) {
            format!("{shell} (standard shell)")
        } else {
            shell
        };

        match ammo.limit {
            Some(limit) => format!("{shell}, {limit} per battle"),
            None => shell,
        }
    }
}

impl SpecialAbility {
    fn is(&self, kind: AbilityKind) -> bool {
        match (self, kind) {
            (Self::CombatInstructions(_), AbilityKind::CombatInstructions) => true,
            (Self::AlternativeFireMode(_), AbilityKind::AlternativeFireMode) => true,
            (Self::AlternativeFireMode(mode), AbilityKind::BurstFire) => mode.shots_per_burst.is_some(),
            _ => false,
        }
    }

    fn mentions(&self, words: &str) -> bool {
        let (name, effects) = match self {
            Self::CombatInstructions(info) => (&info.name, &info.effects),
            Self::AlternativeFireMode(info) => (&info.name, &info.effects),
        };

        name.to_lowercase().contains(words) || effects.iter().any(|effect| effect.to_lowercase().contains(words))
    }

    /// "Combat Instructions \"Modified Secondary Munitions\" (manual, 30 s): effects".
    fn summary(&self) -> String {
        match self {
            Self::CombatInstructions(info) => {
                let activation = if info.activation.starts_with("Press") { "manual" } else { "automatic" };
                format!(
                    "{COMBAT_INSTRUCTIONS} \"{}\" ({activation}, {} s): {}",
                    info.name,
                    info.duration_s.value(),
                    info.effects.join(", ")
                )
            }
            Self::AlternativeFireMode(info) => {
                let cadence = match (info.shots_per_burst, info.shot_interval_s) {
                    (Some(shots), Some(interval)) => format!(
                        "{} salvos {} s apart, then {} s reload",
                        shots.0,
                        interval.value(),
                        info.reload_s.value()
                    ),
                    _ => format!("{} s reload", info.reload_s.value()),
                };
                let mut summary = format!("{} ({cadence})", info.name);
                if !info.effects.is_empty() {
                    summary.push_str(&format!(": {}", info.effects.join(", ")));
                }
                if !info.shells.is_empty() {
                    summary.push_str(&format!("; shells {}", info.shells.join(", ")));
                }
                summary
            }
        }
    }
}

/// `burst`, `burst_sec` or `sec`, upper case, as the client picks the title
/// (`FireModeSlotSystem.createUiData`).
fn fire_mode_key(mode: &AlternativeFireMode) -> &'static str {
    let modified = !mode.ammo.is_empty() || !mode.modifiers.is_empty();

    match (mode.is_burst(), modified) {
        (true, true) => "BURST_SEC",
        (true, false) => "BURST",
        (false, _) => "SEC",
    }
}

/// The GameParams activator type, which names a trigger whose progress
/// name is `default`.
fn activator_name(event: &ProgressEvent) -> String {
    match event {
        ProgressEvent::Ribbon { .. } => "RibbonActivator".to_owned(),
        ProgressEvent::PotentialDamage { .. } => "PotentialDamageActivator".to_owned(),
        ProgressEvent::Timer { .. } => "TimerActivator".to_owned(),
        ProgressEvent::ConsumableUse { .. } => "ConsumableUseActivator".to_owned(),
        ProgressEvent::AdjustmentShot { .. } => "AdjustmentShotActivator".to_owned(),
        ProgressEvent::VisibilityChanged { .. } => "VisibilityChangedActivator".to_owned(),
        ProgressEvent::Other { activator } => activator.clone(),
    }
}

/// The distinct abilities of a ship's modules of one slot, each with the
/// modules that carry it. Module names are listed only when the modules of
/// the slot differ.
fn group_by_module<'a, A>(
    modules: impl Iterator<Item = (&'a str, Option<&'a A>)>,
    name: impl Fn(&str) -> String,
) -> Vec<(&'a A, Vec<String>)>
where
    A: PartialEq,
{
    let modules: Vec<(&str, Option<&A>)> = modules.collect();
    let mut groups: Vec<(&A, Vec<String>)> = Vec::new();
    for (module, ability) in &modules {
        let Some(ability) = ability else {
            continue;
        };

        match groups.iter_mut().find(|(other, _)| other == ability) {
            Some((_, names)) => names.push(name(module)),
            None => groups.push((ability, vec![name(module)])),
        }
    }

    let uniform = groups.len() == 1 && modules.iter().all(|(_, ability)| ability.is_some());
    if uniform {
        for (_, names) in &mut groups {
            names.clear();
        }
    }

    groups
}

/// The first gun of the main battery module that has `mode`, stock first:
/// its standard reload and shells.
fn standard_gun<'a>(
    ship: &Ship,
    components: &'a ShipTtxComponents,
    mode: &AlternativeFireMode,
) -> Option<&'a ArtilleryGunStats> {
    let stock = ShipUpgradeSelection::stock(&ship.param);
    let mut upgrades: Vec<&String> = components.artillery.keys().collect();
    upgrades.sort_by_key(|upgrade| Some(*upgrade) != stock.artillery.as_ref());

    let upgrade = upgrades.into_iter().find(|upgrade| {
        components
            .upgrade_components(upgrade)
            .first()
            .and_then(|component| components.alternative_fire(component))
            .is_some_and(|other| other == mode)
    })?;

    components.artillery(upgrade)?.guns.first()
}
