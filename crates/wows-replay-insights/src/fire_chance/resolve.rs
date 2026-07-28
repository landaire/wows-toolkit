//! Assemble [`FireChanceInput`] for one finished replay.
//!
//! [`analysis`](super::analysis) takes every victim-side and attacker-side fact
//! pre-resolved, because each one has its own failure modes and the statistic
//! must be able to refuse rather than guess. This module is where that
//! resolution happens: it reads a finished [`BattleReport`] plus the build's
//! GameParams and produces the owned facts `FireChanceInput` borrows.
//!
//! It is the headless entry point the feature exists to provide: a server
//! analysing a replay calls [`resolve_fire_chance_input`], then
//! [`ResolvedFireChanceInput::input`], then [`analyze`](super::analysis::analyze),
//! with no toolkit types involved.
//!
//! Every resolution failure is a refusal, never a substitute value: an
//! unresolvable ship-wide fact fails the whole statistic with a named
//! [`FireChanceResolveError`], and an unresolvable per-victim fact drops that
//! victim, whose hits then classify as `NoSectionGeometry`.

use std::collections::HashMap;

use wowsunpack::data::Version;
use wowsunpack::game_params::ttx::components::ArtilleryGunStats;
use wowsunpack::game_params::ttx::components::ShipTtxComponents;
use wowsunpack::game_params::types::CrewSkillType;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::KnownCrewSkill;
use wowsunpack::game_params::types::Param;
use wowsunpack::game_params::types::ShipConfigData;
use wowsunpack::models::fire_nodes::FireSectionGeometry;

use wows_battle_world::report::BattleReport;
use wows_replays::analyzer::battle_controller::Player;
use wows_replays::types::EntityId;

use crate::build::ResolvedBuild;
use crate::build::inventory_from_build;
use crate::fire_chance::analysis::AttackerContext;
use crate::fire_chance::analysis::FireChanceInput;
use crate::fire_chance::analysis::FirePrevention;
use crate::fire_chance::analysis::VictimContext;
use crate::fire_chance::analysis::VictimFate;

/// The `ShipUpgradeInfo` slot a battery's ammo list is reached through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatterySlot {
    /// `_Artillery`, which keys the main-battery component map.
    MainBattery,
    /// `_Hull`, which keys the ATBA (secondary) component map: the secondary
    /// component is referenced by the hull upgrade, not by a slot of its own.
    Hull,
}

impl std::fmt::Display for BatterySlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BatterySlot::MainBattery => write!(f, "_Artillery"),
            BatterySlot::Hull => write!(f, "_Hull"),
        }
    }
}

/// Why an attacker-side fact could not be resolved.
///
/// Every variant refuses the whole statistic. They are attacker-side because a
/// victim-side failure drops one victim and is reported through
/// [`ResolutionDiagnostics`] instead.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FireChanceResolveError {
    #[error("the recording player has no vehicle entity, so there is nothing to attribute hits to")]
    NoSelfVehicle,
    #[error("the recording player's build did not resolve from GameParams")]
    NoSelfBuild,
    #[error("{ship} is not a vehicle in this build's GameParams")]
    NotAVehicle { ship: String },
    #[error("{ship} carries no extracted TTX components, so no battery resolves")]
    NoTtxComponents { ship: String },
    #[error("{ship} has no {slot} upgrade at all, so it carries no main battery")]
    NoSuchSlot { ship: String, slot: BatterySlot },
    #[error("{ship} has no equipped {slot} module and more than one to choose from")]
    AmbiguousModule { ship: String, slot: BatterySlot },
    #[error("{ship}: {slot} upgrade {upgrade} has no TTX component entry")]
    UnknownUpgrade { ship: String, slot: BatterySlot, upgrade: String },
    #[error("{ship}: {slot} upgrade {upgrade} carries an empty ammoList")]
    EmptyAmmoList { ship: String, slot: BatterySlot, upgrade: String },
    #[error("{ship}: hull {hull} names no ATBA component, but {ship} carries secondary ammo elsewhere")]
    UnresolvedSecondaryBattery { ship: String, hull: String },
}

/// What the per-victim resolution cost, for callers that need to know whether a
/// low sample count is the model refusing or the data missing.
///
/// Counts are over the report's players, so they sum to
/// `report.players().len()` minus the recording player.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResolutionDiagnostics {
    /// Victims with every fact resolved.
    pub resolved: u32,
    /// Dropped: no vehicle entity, or the build did not resolve from GameParams.
    pub no_build: u32,
    /// Dropped: the ship carries no TTX components, or its equipped hull upgrade
    /// is not in them.
    pub no_hull_component: u32,
    /// Dropped: the hull resolved but carries no `burnNodes`, so it has no fire
    /// sections at all.
    pub no_burn_nodes: u32,
    /// Dropped: the hull upgrade names no `.model` path, so its fire-section
    /// geometry cannot be looked up.
    pub no_hull_model: u32,
    /// Resolved, but with [`VictimFate::Unknown`]: every hit against them is
    /// refused. Counted separately because it is a whole-victim loss that the
    /// exclusion tally would otherwise attribute to the eligibility model.
    pub unknown_fate: u32,
    /// Resolved, but with [`FirePrevention::Unknown`]: node-2 hits against them
    /// are refused.
    pub unknown_fire_prevention: u32,
}

/// Owns the facts [`FireChanceInput`] borrows.
///
/// A separate owning type rather than a plain `FireChanceInput` return, because
/// the resolved build, victim contexts and ammo lists are all created here and
/// so cannot borrow from the report. Call [`Self::input`] to pair them back up
/// with the report's logs.
pub struct ResolvedFireChanceInput {
    version: Version,
    self_entity: EntityId,
    build: ResolvedBuild,
    main_battery_ammo: Vec<String>,
    secondary_ammo: Vec<String>,
    victims: HashMap<EntityId, VictimContext>,
    diagnostics: ResolutionDiagnostics,
}

impl ResolvedFireChanceInput {
    /// Every ship that could be hit, keyed by vehicle entity id.
    ///
    /// Exposed so a caller can resolve fire-section geometry for exactly the
    /// hulls that matter: [`VictimContext::hull_model_path`] is the lookup key
    /// and `node_probability.len()` is the section count the geometry must
    /// agree with.
    pub fn victims(&self) -> &HashMap<EntityId, VictimContext> {
        &self.victims
    }

    pub fn diagnostics(&self) -> ResolutionDiagnostics {
        self.diagnostics
    }

    /// Pair the resolved facts with the report's logs.
    ///
    /// `report` must be the same report this was resolved from; passing another
    /// one would key the logs by entity ids from a different match.
    pub fn input<'a>(
        &'a self,
        report: &'a BattleReport,
        params: &'a dyn GameParamProvider,
        geometry: &'a dyn Fn(&str) -> Option<FireSectionGeometry>,
    ) -> FireChanceInput<'a> {
        FireChanceInput {
            version: self.version,
            attacker: AttackerContext {
                entity: self.self_entity,
                build: &self.build,
                main_battery_ammo: &self.main_battery_ammo,
                secondary_ammo: Some(&self.secondary_ammo),
            },
            self_entity: self.self_entity,
            victims: &self.victims,
            hits: report.hit_history(),
            salvos: report.salvos(),
            ribbons: report.ribbon_events(),
            burn_state_changes: report.burn_state_changes(),
            presence: report.presence(),
            activations: report.active_consumables(),
            params,
            geometry,
        }
    }
}

/// Resolve every caller-supplied fact [`analyze`](super::analysis::analyze)
/// needs for the recording player of `report`.
///
/// `report.hit_history()` is empty unless the ingest ran with
/// `BattleWorld::set_record_hit_history(true)`, which produces a zero-sample
/// result rather than an error: this function cannot tell an unrecorded parse
/// from a match in which nothing was hit. `report.salvos()` is the same for
/// `BattleWorld::set_record_salvo_history(true)` and the shells-fired count.
pub fn resolve_fire_chance_input(
    report: &BattleReport,
    params: &dyn GameParamProvider,
) -> Result<ResolvedFireChanceInput, FireChanceResolveError> {
    let version = report.version();
    let self_player = report.self_player();
    if self_player.vehicle_entity().is_none() {
        return Err(FireChanceResolveError::NoSelfVehicle);
    }
    let self_entity = self_player.initial_state().entity_id();
    let build = ResolvedBuild::from_player(self_player.as_ref(), &ParamProviderRef(params), version)
        .ok_or(FireChanceResolveError::NoSelfBuild)?;

    let ship_name = build.ship.name().to_owned();
    let vehicle = build.ship.vehicle().ok_or(FireChanceResolveError::NotAVehicle { ship: ship_name.clone() })?;
    let ttx = vehicle.ttx_components().ok_or(FireChanceResolveError::NoTtxComponents { ship: ship_name.clone() })?;

    let artillery = equipped_upgrade(&build, "_Artillery", ttx.artillery.keys())
        .map_err(|gap| gap.into_error(&ship_name, BatterySlot::MainBattery))?;
    let main_battery_ammo = ammo_list(
        ttx.artillery.get(&artillery).map(|component| component.guns.as_slice()),
        &ship_name,
        BatterySlot::MainBattery,
        &artillery,
    )?;

    let hull = equipped_upgrade(&build, "_Hull", ttx.hulls.keys())
        .map_err(|gap| gap.into_error(&ship_name, BatterySlot::Hull))?;
    if !ttx.hulls.contains_key(&hull) {
        return Err(FireChanceResolveError::UnknownUpgrade {
            ship: ship_name.clone(),
            slot: BatterySlot::Hull,
            upgrade: hull,
        });
    }
    let secondary_ammo = secondary_ammo_list(ttx, vehicle.config_data(), &ship_name, &hull)?;

    // A match observed to its finish saw every `ShipDestroyed` in it, so a
    // victim missing from the kill log survived. Without that, absence proves
    // nothing and the fate stays unknown; see `deaths_by_victim`.
    let deaths_are_complete = report.battle_result().is_some();
    let deaths = report.deaths_by_victim();

    let mut victims = HashMap::new();
    let mut diagnostics = ResolutionDiagnostics::default();
    for player in report.players() {
        let entity = player.initial_state().entity_id();
        if entity == self_entity {
            continue;
        }
        match resolve_victim(player, params, version, deaths, deaths_are_complete) {
            Ok(victim) => {
                diagnostics.resolved += 1;
                if victim.fate == VictimFate::Unknown {
                    diagnostics.unknown_fate += 1;
                }
                if victim.fire_prevention == FirePrevention::Unknown {
                    diagnostics.unknown_fire_prevention += 1;
                }
                victims.insert(entity, victim);
            }
            Err(reason) => match reason {
                VictimGap::Build => diagnostics.no_build += 1,
                VictimGap::HullComponent => diagnostics.no_hull_component += 1,
                VictimGap::BurnNodes => diagnostics.no_burn_nodes += 1,
                VictimGap::HullModel => diagnostics.no_hull_model += 1,
            },
        }
    }

    Ok(ResolvedFireChanceInput { version, self_entity, build, main_battery_ammo, secondary_ammo, victims, diagnostics })
}

/// The fact a victim was missing. Private: callers read the aggregate through
/// [`ResolutionDiagnostics`], since a single unresolvable victim is ordinary
/// rather than exceptional.
enum VictimGap {
    Build,
    HullComponent,
    BurnNodes,
    HullModel,
}

fn resolve_victim(
    player: &Player,
    params: &dyn GameParamProvider,
    version: Version,
    deaths: &HashMap<EntityId, wows_replays::types::GameClock>,
    deaths_are_complete: bool,
) -> Result<VictimContext, VictimGap> {
    let entity = player.initial_state().entity_id();
    let build = ResolvedBuild::from_player(player, &ParamProviderRef(params), version).ok_or(VictimGap::Build)?;
    let vehicle = build.ship.vehicle().ok_or(VictimGap::Build)?;
    let ttx = vehicle.ttx_components().ok_or(VictimGap::HullComponent)?;

    let hull = equipped_upgrade(&build, "_Hull", ttx.hulls.keys()).map_err(|_| VictimGap::HullComponent)?;
    let hull_stats = ttx.hulls.get(&hull).ok_or(VictimGap::HullComponent)?;
    if hull_stats.burn_nodes.is_empty() {
        return Err(VictimGap::BurnNodes);
    }
    let hull_model_path = vehicle.model_path_for_hull(&hull).ok_or(VictimGap::HullModel)?.to_owned();

    let fate = match deaths.get(&entity) {
        Some(clock) => VictimFate::DiedAt(*clock),
        None if deaths_are_complete => VictimFate::Survived,
        None => VictimFate::Unknown,
    };

    Ok(VictimContext {
        ship_index: build.ship.index().to_owned(),
        // The GameParams name, not a localized one: this crate carries no
        // translations, so the consumer localizes it.
        ship_name: build.ship.name().to_owned(),
        hull_model_path,
        node_probability: hull_stats.burn_nodes.iter().map(|node| node.probability).collect(),
        // `ModifierSet` folds `burnProb` multiplicatively with an identity of
        // 1.0 and needs no per-version modifier table, so this resolves for
        // every build rather than failing where `ModifierBundle` would.
        burn_prob: build.modifiers.coefficient("burnProb"),
        fire_prevention: fire_prevention(&build),
        fate,
        consumables: inventory_from_build(&build),
    })
}

/// Whether the build learned Fire Prevention Expert, which forces `burningFlags`
/// bit 2 off for the whole match.
///
/// Resolves to [`FirePrevention::Unknown`] the moment anything in the chain is
/// missing, including a single learned skill this build's captain cannot name:
/// an unidentifiable skill could be the one that suppresses the node, and
/// reading it as `NotLearned` would admit node-2 hits that could not have
/// started a fire.
fn fire_prevention(build: &ResolvedBuild) -> FirePrevention {
    let Some(crew) = build.captain.as_deref().and_then(|captain| captain.crew()) else {
        return FirePrevention::Unknown;
    };
    // The learned-skill list comes off a replay property that is empty both for
    // a captain with no skills and for any replay shape it did not parse from,
    // which is ordinary on old builds. The two readings are not separable here,
    // so an empty list is a gap like every other one in this chain.
    if build.skills.is_empty() {
        return FirePrevention::Unknown;
    }
    for &skill_type in &build.skills {
        let Some(skill) = crew.skill_by_type(CrewSkillType::from(skill_type)) else {
            return FirePrevention::Unknown;
        };
        let recognized = KnownCrewSkill::recognize(skill.internal_name(), skill.skill_type());
        if recognized.known() == Some(&KnownCrewSkill::FirePreventionExpert) {
            return FirePrevention::Learned;
        }
    }
    FirePrevention::NotLearned
}

/// Why a slot's equipped upgrade could not be named.
enum SlotGap {
    NoOptions,
    Ambiguous,
}

impl SlotGap {
    fn into_error(self, ship: &str, slot: BatterySlot) -> FireChanceResolveError {
        match self {
            SlotGap::NoOptions => FireChanceResolveError::NoSuchSlot { ship: ship.to_owned(), slot },
            SlotGap::Ambiguous => FireChanceResolveError::AmbiguousModule { ship: ship.to_owned(), slot },
        }
    }
}

/// The equipped upgrade name for one `ShipUpgradeInfo` slot.
///
/// Read from the replay's own equipped module list, which is the only place the
/// player's choice is recorded. Refuses when the replay names no module for the
/// slot and the ship has more than one to pick from: substituting the stock
/// upgrade there would be a guess about which hull or battery was mounted, and
/// the wrong hull means the wrong fire-section geometry. A slot with exactly one
/// option is not a guess, so it resolves without the replay's help.
fn equipped_upgrade<'a>(
    build: &ResolvedBuild,
    uc_type: &str,
    candidates: impl Iterator<Item = &'a String>,
) -> Result<String, SlotGap> {
    let equipped = build
        .modules
        .iter()
        .find(|module| module.unit().and_then(|unit| unit.uc_type()).is_some_and(|t| t.eq_ignore_ascii_case(uc_type)));
    if let Some(module) = equipped {
        return Ok(module.name().to_owned());
    }
    let mut candidates = candidates;
    let only = candidates.next().ok_or(SlotGap::NoOptions)?;
    if candidates.next().is_some() {
        return Err(SlotGap::Ambiguous);
    }
    Ok(only.clone())
}

/// The equipped hull's secondary-battery ammo, or the empty list that says this
/// ship carries no secondaries.
///
/// An empty list is a claim, not a default. It disables the secondary-contest
/// guard by leaving it nothing to contest with, so every secondary-set fire
/// would be credited to whatever main-battery hit landed nearest. It is
/// produced only where the ship's own GameParams establish the claim.
///
/// A missing `ttx.secondaries` entry does not establish it on its own: that is
/// also what a hull whose ATBA component failed to extract looks like, and the
/// gun-hardpoint naming that extraction filters on is per-version. The
/// cross-check is the ship config's `secondary_battery_ammo`, collected from
/// every hull upgrade's ATBA component with no hardpoint filter at all. A ship
/// naming no secondary ammo anywhere has no ATBA component to extract; one
/// naming some has an ATBA that this hull's entry should have carried, so the
/// gap is refused.
fn secondary_ammo_list(
    ttx: &ShipTtxComponents,
    config: Option<&ShipConfigData>,
    ship: &str,
    hull: &str,
) -> Result<Vec<String>, FireChanceResolveError> {
    if let Some(component) = ttx.secondaries.get(hull) {
        return ammo_list(Some(component.guns.as_slice()), ship, BatterySlot::Hull, hull);
    }
    if config.is_some_and(|config| config.secondary_battery_ammo.is_empty()) {
        return Ok(Vec::new());
    }
    Err(FireChanceResolveError::UnresolvedSecondaryBattery { ship: ship.to_owned(), hull: hull.to_owned() })
}

/// The union of every gun's `ammoList` on a battery component.
///
/// A mount can carry mixed calibers, each with its own ammo list, and a shell
/// from any of them is that battery's shell.
fn ammo_list(
    guns: Option<&[ArtilleryGunStats]>,
    ship: &str,
    slot: BatterySlot,
    upgrade: &str,
) -> Result<Vec<String>, FireChanceResolveError> {
    let guns = guns.ok_or_else(|| FireChanceResolveError::UnknownUpgrade {
        ship: ship.to_owned(),
        slot,
        upgrade: upgrade.to_owned(),
    })?;
    let mut ammo: Vec<String> = Vec::new();
    for gun in guns {
        for name in &gun.ammo {
            if !ammo.iter().any(|existing| existing == name) {
                ammo.push(name.clone());
            }
        }
    }
    if ammo.is_empty() {
        return Err(FireChanceResolveError::EmptyAmmoList { ship: ship.to_owned(), slot, upgrade: upgrade.to_owned() });
    }
    Ok(ammo)
}

/// Adapts `&dyn GameParamProvider` to the generic `P: GameParamProvider` the
/// build resolver takes. Needed because `ResolvedBuild::from_player` is generic
/// and a trait object does not implement its own trait here.
struct ParamProviderRef<'a>(&'a dyn GameParamProvider);

impl GameParamProvider for ParamProviderRef<'_> {
    fn game_param_by_id(&self, id: wowsunpack::game_types::GameParamId) -> Option<wowsunpack::Rc<Param>> {
        self.0.game_param_by_id(id)
    }

    fn game_param_by_index(&self, index: &str) -> Option<wowsunpack::Rc<Param>> {
        self.0.game_param_by_index(index)
    }

    fn game_param_by_name(&self, name: &str) -> Option<wowsunpack::Rc<Param>> {
        self.0.game_param_by_name(name)
    }

    fn params(&self) -> &[wowsunpack::Rc<Param>] {
        self.0.params()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use wowsunpack::game_params::keys::ComponentType;
    use wowsunpack::game_params::types::Achievement;
    use wowsunpack::game_params::types::Crew;
    use wowsunpack::game_params::types::CrewPersonality;
    use wowsunpack::game_params::types::CrewPersonalityShips;
    use wowsunpack::game_params::types::CrewSkill;
    use wowsunpack::game_params::types::CrewSkillName;
    use wowsunpack::game_params::types::CrewSkillTiers;
    use wowsunpack::game_params::types::GameParams;
    use wowsunpack::game_params::types::ParamData;
    use wowsunpack::game_params::types::SkillPointCost;
    use wowsunpack::game_params::types::Species;
    use wowsunpack::game_params::types::Unit;
    use wowsunpack::game_types::GameParamId;

    const VERSION: Version = Version::base(15, 0, 0);
    const FIRE_PREVENTION_SKILL: u8 = 14;
    const OTHER_SKILL: u8 = 15;

    /// The `_Hull`/`_Artillery` strings this module matches against
    /// `Unit::uc_type`, pinned to the constants that define them so a typo here
    /// fails rather than silently resolving nothing.
    #[test]
    fn the_slot_names_match_the_component_uc_types() {
        assert_eq!(ComponentType::Hull.uc_type(), Some("_Hull"));
        assert_eq!(ComponentType::Artillery.uc_type(), Some("_Artillery"));
        assert_eq!(BatterySlot::Hull.to_string(), "_Hull");
        assert_eq!(BatterySlot::MainBattery.to_string(), "_Artillery");
    }

    fn gun_with_ammo(ammo: &[&str]) -> ArtilleryGunStats {
        ArtilleryGunStats { ammo: ammo.iter().map(|name| (*name).to_owned()).collect(), ..Default::default() }
    }

    /// Mixed-caliber ATBA mounts carry a separate ammoList per gun group, and a
    /// shell from any of them is that battery's shell.
    #[test]
    fn the_ammo_list_is_the_union_over_every_gun() {
        let guns = [gun_with_ammo(&["HE_A", "AP_A"]), gun_with_ammo(&["HE_B", "AP_A"])];
        let ammo = ammo_list(Some(&guns), "SHIP", BatterySlot::Hull, "A_Hull").expect("resolves");
        assert_eq!(ammo, vec!["HE_A".to_owned(), "AP_A".to_owned(), "HE_B".to_owned()]);
    }

    /// A component we could not reach and a component whose guns name no shells
    /// are both unresolved, never an empty battery: an empty list would silently
    /// classify every shell as not-this-battery.
    #[test]
    fn an_unresolvable_ammo_list_is_an_error_not_an_empty_list() {
        assert_eq!(
            ammo_list(None, "SHIP", BatterySlot::MainBattery, "A_Artillery"),
            Err(FireChanceResolveError::UnknownUpgrade {
                ship: "SHIP".to_owned(),
                slot: BatterySlot::MainBattery,
                upgrade: "A_Artillery".to_owned(),
            })
        );
        assert_eq!(
            ammo_list(Some(&[gun_with_ammo(&[])]), "SHIP", BatterySlot::MainBattery, "A_Artillery"),
            Err(FireChanceResolveError::EmptyAmmoList {
                ship: "SHIP".to_owned(),
                slot: BatterySlot::MainBattery,
                upgrade: "A_Artillery".to_owned(),
            })
        );
    }

    fn ttx_with_secondaries(hull: &str, ammo: &[&str]) -> ShipTtxComponents {
        let mut ttx = ShipTtxComponents::default();
        ttx.secondaries.insert(
            hull.to_owned(),
            wowsunpack::game_params::ttx::components::SecondaryComponentStats {
                max_dist: None,
                guns: vec![gun_with_ammo(ammo)],
            },
        );
        ttx
    }

    fn config_naming_secondary_ammo(ammo: &[&str]) -> ShipConfigData {
        ShipConfigData {
            secondary_battery_ammo: ammo.iter().map(|name| (*name).to_owned()).collect(),
            ..Default::default()
        }
    }

    /// The equipped hull's own ATBA component is the answer whenever it exists.
    #[test]
    fn the_equipped_hulls_atba_component_names_the_secondary_ammo() {
        let ttx = ttx_with_secondaries("A_Hull", &["ATBA_HE"]);
        let config = config_naming_secondary_ammo(&["ATBA_HE"]);
        assert_eq!(secondary_ammo_list(&ttx, Some(&config), "SHIP", "A_Hull"), Ok(vec!["ATBA_HE".to_owned()]));
    }

    /// A ship that names no secondary ammo on any hull upgrade has no ATBA
    /// component to extract, which is the one case where an empty list is a
    /// fact rather than a guess.
    #[test]
    fn a_ship_naming_no_secondary_ammo_anywhere_carries_no_secondaries() {
        let ttx = ShipTtxComponents::default();
        let config = config_naming_secondary_ammo(&[]);
        assert_eq!(secondary_ammo_list(&ttx, Some(&config), "SHIP", "A_Hull"), Ok(Vec::new()));
    }

    /// A hull with no ATBA entry on a ship that names secondary ammo elsewhere
    /// is an extraction gap, not a ship without secondaries. Reading it as the
    /// second would silently disable the secondary-contest guard on exactly the
    /// brawlers it exists for.
    #[test]
    fn a_missing_atba_entry_on_a_ship_with_secondary_ammo_is_refused() {
        let ttx = ttx_with_secondaries("B_Hull", &["ATBA_HE"]);
        let config = config_naming_secondary_ammo(&["ATBA_HE"]);
        assert_eq!(
            secondary_ammo_list(&ttx, Some(&config), "SHIP", "A_Hull"),
            Err(FireChanceResolveError::UnresolvedSecondaryBattery {
                ship: "SHIP".to_owned(),
                hull: "A_Hull".to_owned(),
            })
        );
    }

    /// With no ship config there is nothing to establish either reading, so the
    /// gap is refused rather than resolved to "no secondaries".
    #[test]
    fn a_missing_atba_entry_without_ship_config_is_refused() {
        let ttx = ShipTtxComponents::default();
        assert!(secondary_ammo_list(&ttx, None, "SHIP", "A_Hull").is_err());
    }

    fn ship_param(id: GameParamId) -> Param {
        Param::builder()
            .id(id)
            .index("SHIP01".to_owned())
            .name("SHIP01".to_owned())
            .nation("USA".to_owned())
            .data(ParamData::Achievement(
                Achievement::builder()
                    .is_group(false)
                    .one_per_battle(false)
                    .ui_type(String::new())
                    .ui_name(String::new())
                    .build(),
            ))
            .build()
    }

    fn module_param(id: GameParamId, name: &str, uc_type: &str) -> Param {
        Param::builder()
            .id(id)
            .index(name.to_owned())
            .name(name.to_owned())
            .nation("USA".to_owned())
            .data(ParamData::Unit(Unit::new(Some(uc_type.to_owned()))))
            .build()
    }

    fn skill_tiers() -> CrewSkillTiers {
        CrewSkillTiers::builder()
            .aircraft_carrier(SkillPointCost::new(1))
            .auxiliary(SkillPointCost::new(1))
            .battleship(SkillPointCost::new(1))
            .cruiser(SkillPointCost::new(1))
            .destroyer(SkillPointCost::new(1))
            .submarine(SkillPointCost::new(1))
            .build()
    }

    fn personality() -> CrewPersonality {
        CrewPersonality::builder()
            .can_reset_skills_for_free(false)
            .cost_credits(0)
            .cost_elite_xp(0)
            .cost_gold(0)
            .cost_xp(0)
            .person_name(String::new())
            .ships(
                CrewPersonalityShips::builder()
                    .groups(Vec::new())
                    .nation(Vec::new())
                    .peculiarity(Vec::new())
                    .ships(Vec::new())
                    .build(),
            )
            .tags(Vec::new())
            .build()
    }

    fn skill(internal_name: &str, skill_type: u8) -> CrewSkill {
        CrewSkill::builder()
            .internal_name(CrewSkillName::from(internal_name))
            .can_be_learned(true)
            .is_epic(false)
            .skill_type(CrewSkillType::from(skill_type))
            .tier(skill_tiers())
            .ui_treat_as_trigger(false)
            .modifiers(Vec::new())
            .build()
    }

    /// `DefenceFireProbability` is Fire Prevention Expert's internal name; the
    /// other is a decoy so a test cannot pass by recognizing any skill at all.
    fn captain_param(id: GameParamId, skills: Option<Vec<CrewSkill>>) -> Param {
        let crew = Crew::builder().money_training_level(0).personality(personality()).maybe_skills(skills).build();
        Param::builder()
            .id(id)
            .index("PAW001".to_owned())
            .name("PAW001_Captain".to_owned())
            .nation("USA".to_owned())
            .data(ParamData::Crew(crew))
            .build()
    }

    struct Fixture {
        modules: Vec<(&'static str, &'static str)>,
        captain: Option<Vec<CrewSkill>>,
        with_captain: bool,
        learned: Vec<u8>,
    }

    impl Fixture {
        fn new() -> Fixture {
            Fixture {
                modules: Vec::new(),
                captain: Some(vec![
                    skill("DefenceFireProbability", FIRE_PREVENTION_SKILL),
                    skill("DetectionAlert", OTHER_SKILL),
                ]),
                with_captain: true,
                learned: Vec::new(),
            }
        }

        fn with_module(mut self, name: &'static str, uc_type: &'static str) -> Fixture {
            self.modules.push((name, uc_type));
            self
        }

        fn learning(mut self, skills: &[u8]) -> Fixture {
            self.learned = skills.to_vec();
            self
        }

        fn without_a_captain(mut self) -> Fixture {
            self.with_captain = false;
            self
        }

        fn whose_captain_names_no_skills(mut self) -> Fixture {
            self.captain = None;
            self
        }

        fn build(self) -> ResolvedBuild {
            let ship_id = GameParamId::from(1u32);
            let captain_id = GameParamId::from(999u32);
            let mut params = vec![ship_param(ship_id), captain_param(captain_id, self.captain)];
            let mut module_ids = Vec::new();
            for (index, (name, uc_type)) in self.modules.iter().enumerate() {
                let id = GameParamId::from(100u32 + index as u32);
                params.push(module_param(id, name, uc_type));
                module_ids.push(id);
            }
            let gp = GameParams::from(params);
            ResolvedBuild::from_ids(
                ship_id,
                &module_ids,
                &[],
                self.with_captain.then_some(captain_id),
                &self.learned,
                &[],
                &[],
                Species::Cruiser,
                VERSION,
                &gp,
            )
            .expect("fixture resolves")
        }
    }

    /// The replay's own module list is the only record of what was mounted, so
    /// it wins over any number of candidates.
    #[test]
    fn the_equipped_module_names_the_slot() {
        let build = Fixture::new().with_module("B_Hull", "_Hull").with_module("A_Artillery", "_Artillery").build();
        let candidates = ["A_Hull".to_owned(), "B_Hull".to_owned()];
        assert_eq!(equipped_upgrade(&build, "_Hull", candidates.iter()).ok(), Some("B_Hull".to_owned()));
    }

    /// GameParams is not consistent in casing across versions (`_ATBA` vs
    /// `_Atba`), so the match is case-insensitive.
    #[test]
    fn the_uc_type_match_ignores_case() {
        let build = Fixture::new().with_module("B_Hull", "_hull").build();
        assert_eq!(equipped_upgrade(&build, "_Hull", [].iter()).ok(), Some("B_Hull".to_owned()));
    }

    /// With no equipped module, a slot resolves only when there is nothing to
    /// choose between. Two candidates is a guess about which hull was mounted,
    /// and the wrong hull is the wrong fire-section geometry.
    #[test]
    fn an_unrecorded_slot_resolves_only_when_it_has_one_option() {
        let build = Fixture::new().build();
        let one = ["A_Hull".to_owned()];
        assert_eq!(equipped_upgrade(&build, "_Hull", one.iter()).ok(), Some("A_Hull".to_owned()));

        let two = ["A_Hull".to_owned(), "B_Hull".to_owned()];
        assert!(matches!(equipped_upgrade(&build, "_Hull", two.iter()), Err(SlotGap::Ambiguous)));
        assert!(matches!(equipped_upgrade(&build, "_Hull", [].iter()), Err(SlotGap::NoOptions)));
    }

    /// Fire Prevention Expert is recognized by its internal name, not by its
    /// numeric skill type, which is not stable across versions.
    #[test]
    fn fire_prevention_is_learned_when_the_skill_is() {
        let build = Fixture::new().learning(&[FIRE_PREVENTION_SKILL]).build();
        assert_eq!(fire_prevention(&build), FirePrevention::Learned);
    }

    /// A captain with skills, none of them Fire Prevention Expert, is a real
    /// `NotLearned`: nothing in the chain is missing.
    #[test]
    fn fire_prevention_is_not_learned_when_every_skill_resolves_to_something_else() {
        let build = Fixture::new().learning(&[OTHER_SKILL]).build();
        assert_eq!(fire_prevention(&build), FirePrevention::NotLearned);
    }

    /// An empty learned-skill list is a skill-less captain and a list that did
    /// not parse, and nothing here separates them. Reading it as `NotLearned`
    /// would admit node-2 hits on every victim whose skills the replay did not
    /// carry, which on old builds is most of them.
    #[test]
    fn an_empty_learned_skill_list_is_unknown_not_not_learned() {
        assert_eq!(fire_prevention(&Fixture::new().build()), FirePrevention::Unknown);
    }

    /// Every gap in the chain refuses. A learned skill the captain cannot name
    /// could be the one that kills node 2, and reading it as `NotLearned` would
    /// admit node-2 hits that could not have started a fire.
    #[test]
    fn an_unreadable_skill_chain_is_unknown_not_not_learned() {
        let no_captain = Fixture::new().without_a_captain().learning(&[FIRE_PREVENTION_SKILL]).build();
        assert_eq!(fire_prevention(&no_captain), FirePrevention::Unknown);

        let no_skill_table = Fixture::new().whose_captain_names_no_skills().learning(&[FIRE_PREVENTION_SKILL]).build();
        assert_eq!(fire_prevention(&no_skill_table), FirePrevention::Unknown);

        let unnameable = Fixture::new().learning(&[200]).build();
        assert_eq!(fire_prevention(&unnameable), FirePrevention::Unknown);
    }
}
