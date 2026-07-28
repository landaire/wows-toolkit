//! Effective fire chance: fires started over hits that could have started one.
//!
//! Most HE hits never had a chance: the section was already burning, Damage
//! Control Party was running, or Fire Prevention Expert had permanently killed
//! that section. Dividing by every hit buries the signal, so a hit enters the
//! denominator only when every one of those can be ruled out from the replay.
//! Every ambiguity resolves toward refusal: admitting a hit that could not have
//! started a fire silently corrupts the result, while refusing one that could
//! have merely costs a sample.

use std::collections::BTreeMap;
use std::collections::HashMap;

use wowsunpack::data::Version;
use wowsunpack::game_params::ttx::factories::SMALL_PROJECTILE_MAX_DIAMETER_M;
use wowsunpack::game_params::ttx::modifiers::ModifierBundle;
use wowsunpack::game_params::ttx::provenance::Op;
use wowsunpack::game_params::ttx::weapon_tables::calculate_burn_chance;
use wowsunpack::game_params::ttx::weapon_tables::is_small_projectile;
use wowsunpack::game_params::types::CrewSkillType;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_types::GameParamId;
use wowsunpack::game_types::Ribbon;
use wowsunpack::game_types::ShellHitType;
use wowsunpack::models::fire_nodes::BurnNodeIndex;
use wowsunpack::models::fire_nodes::FireSectionGeometry;
use wowsunpack::recognized::Recognized;

use wows_battle_world::resources::BurnStateChange;
use wows_battle_world::resources::PresenceLog;
use wows_battle_world::resources::RibbonEvent;
use wows_replays::analyzer::battle_controller::state::ActiveConsumable;
use wows_replays::analyzer::battle_controller::state::ConsumableInventory;
use wows_replays::analyzer::battle_controller::state::ResolvedShotHit;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;

use crate::build::ResolvedBuild;
use crate::fire_chance::geometry::section_for_hit;
use crate::fire_chance::victim::DamageControlState;
use crate::fire_chance::victim::VictimTrack;

/// One server tick (`TICKS_PER_SECOND` = 7, `ma779114d`) plus packet jitter. The
/// ribbon and the `burningFlags` update are separate packets from the same tick.
const ATTRIBUTION_WINDOW: f32 = 0.5;

/// The `burningFlags` bit Fire Prevention Expert forces off unconditionally
/// (`setBurningFlags`, `m09838fe6/m0700235d.py:344`), which is what "reduces the
/// maximum number of fires from 4 to 3" means mechanically.
const FIRE_PREVENTION_SUPPRESSED_NODE: u8 = 2;

/// A probability in 0..=1.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct BurnChance(f32);

impl BurnChance {
    /// `None` for NaN or a value outside 0..=1.
    pub fn new(chance: f32) -> Option<BurnChance> {
        (chance.is_finite() && (0.0..=1.0).contains(&chance)).then_some(BurnChance(chance))
    }

    pub fn get(self) -> f32 {
        self.0
    }
}

/// Why a hit did or did not enter the denominator.
#[derive(Clone, Debug, PartialEq)]
pub enum HitEligibility {
    Eligible {
        section: BurnNodeIndex,
        /// The hit's expected fire probability. `None` when the attacker's
        /// modifier bundle could not be folded for this game version, which
        /// makes the whole formula unavailable; the hit is still a valid trial.
        expected: Option<BurnChance>,
    },
    SectionAlreadyBurning(BurnNodeIndex),
    SectionSuppressedByFirePrevention,
    DamageControlActive,
    DamageControlUnknown,
    ObservationGap,
    ConsumableModelUnreliable,
    VictimDead,
    ShellCannotBurn,
    NotMainBattery,
    /// Carries the raw `Recognized` rather than a bare [`ShellHitType`]: an id
    /// this build's constants table does not name is also a hit type nothing
    /// establishes as rolling, and flattening it would lose which one it was.
    HitTypeDoesNotRoll(Recognized<ShellHitType>),
    NoSectionGeometry,
    /// One of our own secondary shells landed in the same section inside the
    /// attribution window, so the SetFire ribbon cannot be assigned to this
    /// main-battery hit rather than to the secondary.
    SecondaryFireAmbiguous,
}

/// Payload-free counterpart of [`HitEligibility`], used as the exclusion-tally
/// key. Separate from `HitEligibility` because the tally counts reasons, not
/// instances, and `BurnNodeIndex`/`ShellHitType` payloads would fragment it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExclusionReason {
    SectionAlreadyBurning,
    SectionSuppressedByFirePrevention,
    DamageControlActive,
    DamageControlUnknown,
    ObservationGap,
    ConsumableModelUnreliable,
    VictimDead,
    ShellCannotBurn,
    NotMainBattery,
    HitTypeDoesNotRoll,
    NoSectionGeometry,
    SecondaryFireAmbiguous,
}

impl HitEligibility {
    /// `None` for `Eligible`; the tally key otherwise.
    pub fn exclusion_reason(&self) -> Option<ExclusionReason> {
        match self {
            HitEligibility::Eligible { .. } => None,
            HitEligibility::SectionAlreadyBurning(_) => Some(ExclusionReason::SectionAlreadyBurning),
            HitEligibility::SectionSuppressedByFirePrevention => {
                Some(ExclusionReason::SectionSuppressedByFirePrevention)
            }
            HitEligibility::DamageControlActive => Some(ExclusionReason::DamageControlActive),
            HitEligibility::DamageControlUnknown => Some(ExclusionReason::DamageControlUnknown),
            HitEligibility::ObservationGap => Some(ExclusionReason::ObservationGap),
            HitEligibility::ConsumableModelUnreliable => Some(ExclusionReason::ConsumableModelUnreliable),
            HitEligibility::VictimDead => Some(ExclusionReason::VictimDead),
            HitEligibility::ShellCannotBurn => Some(ExclusionReason::ShellCannotBurn),
            HitEligibility::NotMainBattery => Some(ExclusionReason::NotMainBattery),
            HitEligibility::HitTypeDoesNotRoll(_) => Some(ExclusionReason::HitTypeDoesNotRoll),
            HitEligibility::NoSectionGeometry => Some(ExclusionReason::NoSectionGeometry),
            HitEligibility::SecondaryFireAmbiguous => Some(ExclusionReason::SecondaryFireAmbiguous),
        }
    }
}

/// One step of the attacker-side burn chance, for the hover breakdown. Built
/// from the ordered `AppliedModifier` list `calculate_burn_chance` returns,
/// keeping only the steps that actually moved the value (every modifier is
/// applied unconditionally, most at their identity).
#[derive(Clone, Debug, PartialEq)]
pub struct FormulaStep {
    /// GameParams modifier name, e.g. `"artilleryBurnChanceBonus"`.
    pub modifier: String,
    /// Where it came from, as the GameParams identifier of the upgrade, skill
    /// or signal that carries it (e.g. `"PCM020_DamageControl_Mod_I"`); this
    /// crate has no translations, so the UI localizes it. `None` when no
    /// equipped upgrade, skill or signal in the build carries this name, which
    /// means the value came from the stock table and the step is an identity.
    pub source: Option<String>,
    pub op: FormulaOp,
    pub value: f32,
    /// Running chance after this step.
    pub result: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormulaOp {
    Multiply,
    Add,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PerShipFireChance {
    pub victim_ship_index: String,
    pub victim_ship_name: String,
    pub eligible_hits: u32,
    pub fires: u32,
    pub expected_fires: Option<f32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EffectiveFireChance {
    pub eligible_hits: u32,
    pub fires: u32,
    /// `None` on builds predating the modern modifier names, where the
    /// attacker formula does not apply. The observed rate is still valid.
    pub expected_fires: Option<f32>,
    pub per_ship: Vec<PerShipFireChance>,
    pub exclusions: BTreeMap<ExclusionReason, u32>,
    /// Predicted-vs-actual section for every attributed fire. This is the
    /// evidence for the nearest-node assumption; [`Self::section_agreement`] is
    /// its ratio. Stored as pairs so a corpus test can build a confusion matrix.
    pub section_predictions: Vec<SectionPrediction>,
    /// SetFire ribbons that matched no eligible hit. Nonzero means something
    /// upstream is wrong; surfaced rather than swallowed.
    pub unattributed_fires: u32,
    /// The attacker-side formula steps, for the hover breakdown.
    pub formula: Vec<FormulaStep>,
}

/// One attributed fire's predicted section against the one the server lit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SectionPrediction {
    pub predicted: BurnNodeIndex,
    pub actual: BurnNodeIndex,
}

impl EffectiveFireChance {
    /// `None` when there are no eligible hits: a rate over zero samples is not
    /// zero, it is unknown.
    pub fn rate(&self) -> Option<f32> {
        (self.eligible_hits > 0).then(|| self.fires as f32 / self.eligible_hits as f32)
    }

    /// Fraction of attributed fires whose predicted section is the bit the
    /// server lit. `None` when no fires were attributed.
    pub fn section_agreement(&self) -> Option<f32> {
        if self.section_predictions.is_empty() {
            return None;
        }
        let agreed = self.section_predictions.iter().filter(|p| p.predicted == p.actual).count();
        Some(agreed as f32 / self.section_predictions.len() as f32)
    }
}

/// The firing ship: who, their build, and which shells each battery loads.
pub struct AttackerContext<'a> {
    /// Vehicle entity id, matched against a salvo's `owner_id`.
    pub entity: EntityId,
    pub build: &'a ResolvedBuild,
    /// Projectile GameParams names in the equipped main battery's `ammoList`.
    pub main_battery_ammo: &'a [String],
    /// Projectile GameParams names in the equipped secondary battery's
    /// `ammoList`. Secondary fire arrives on the same `receiveArtilleryShots`
    /// path as the main battery and is only separable by this list, which is
    /// what makes a secondary/main-battery collision detectable at all.
    pub secondary_ammo: &'a [String],
}

/// One ship that could be hit, with every GameParams fact the eligibility model
/// needs already resolved. Resolution lives with the caller because its failure
/// modes (no hull upgrade, no `burnNodes`, an unresolvable build) belong to the
/// data layer, not to the statistic.
#[derive(Clone, Debug)]
pub struct VictimContext {
    pub ship_index: String,
    pub ship_name: String,
    /// Hull `.model` path, the key the geometry lookup takes.
    pub hull_model_path: String,
    /// `hull.burnNodes[i].probability`, indexed by fire section.
    pub node_probability: Vec<f32>,
    /// The defender's folded `burnProb` coefficient (Damage Control System
    /// Modification 1 `0.95`, Fire Prevention Expert `0.9`).
    pub burn_prob: f32,
    /// Fire Prevention Expert is learned, so `burningFlags` bit 2 is forced off
    /// for this ship and a hit there can never start a fire.
    pub fire_resistance_enabled: bool,
    /// Clock the victim died at, if it did. Burn transitions at or after it are
    /// discarded: the server lights extra nodes on death for visual effect.
    pub died_at: Option<GameClock>,
    /// This victim's own consumable slots, build modifiers already applied.
    pub consumables: Vec<ConsumableInventory>,
}

/// Everything [`analyze`] reads.
pub struct FireChanceInput<'a> {
    /// Replay version, used to fold the attacker's modifiers.
    pub version: Version,
    pub attacker: AttackerContext<'a>,
    /// Every ship that could be hit, keyed by vehicle entity id.
    pub victims: &'a HashMap<EntityId, VictimContext>,
    /// Whole-match hit history (`BattleReport::hit_history`).
    pub hits: &'a [ResolvedShotHit],
    /// Self-player ribbon increments (`BattleReport::ribbon_events`).
    pub ribbons: &'a [RibbonEvent],
    pub burn_state_changes: &'a [BurnStateChange],
    pub presence: &'a PresenceLog,
    /// Consumable activations keyed by the vehicle that used them.
    pub activations: &'a HashMap<EntityId, Vec<ActiveConsumable>>,
    pub params: &'a dyn GameParamProvider,
    /// Fire-section geometry by hull `.model` path.
    pub geometry: &'a dyn Fn(&str) -> Option<FireSectionGeometry>,
}

/// One eligible hit, kept until attribution can decide whether a secondary
/// contests it.
struct Candidate<'a> {
    hit: &'a ResolvedShotHit,
    victim: EntityId,
    section: BurnNodeIndex,
    expected: Option<BurnChance>,
    shell: GameParamId,
}

/// Where one of our own secondary shells landed, for contest detection.
struct SecondaryImpact {
    victim: EntityId,
    section: BurnNodeIndex,
    clock: GameClock,
}

/// Effective fire chance for one attacker.
///
/// `None` when the eligibility model cannot be built at all: no victim's hull
/// resolved to fire-section geometry, or the attacker's ship carries no tier
/// and so no burn-chance formula. Both mean unavailable, never zero.
pub fn analyze(input: &FireChanceInput<'_>) -> Option<EffectiveFireChance> {
    let geometry: HashMap<EntityId, FireSectionGeometry> = input
        .victims
        .iter()
        .filter_map(|(entity, victim)| (input.geometry)(&victim.hull_model_path).map(|g| (*entity, g)))
        .collect();
    if geometry.is_empty() {
        return None;
    }

    let tier = input.attacker.build.ship.vehicle()?.level();
    let species = input.attacker.build.species;

    // `Err` names a modifier absent from this version's MODIFIER_SETTINGS.
    // Pre-0.7 builds name the burn-chance modifiers entirely differently, so
    // the attacker formula does not apply and `expected_fires` stays `None`.
    // The observed rate needs no formula, so the analysis still runs on the
    // stock bundle: the shell gate only asks whether the clamped chance is
    // positive, and AP and SAP carry `burnProb` -0.5, a sentinel sized to
    // absorb every additive bonus in the game, so that answer is the same under
    // any bundle.
    let folded = input.attacker.build.modifier_bundle(input.version);
    let formula_applies = folded.is_ok();
    let bundle = folded.unwrap_or_else(|_| ModifierBundle::empty(species));

    let tracks: HashMap<EntityId, VictimTrack> = input
        .victims
        .iter()
        .map(|(entity, victim)| {
            // Per-victim slices, not the whole match: `ActiveConsumable` and
            // `ConsumableInventory` carry no entity identity, so another ship's
            // Damage Control Party would otherwise prove this one Down. An
            // absent entry means no activation was ever observed for this ship,
            // which is a fact `VictimTrack` reasons from rather than a default.
            let activations = input.activations.get(entity).map(Vec::as_slice).unwrap_or(&[]);
            let track = VictimTrack::build(
                *entity,
                input.burn_state_changes,
                activations,
                &victim.consumables,
                input.presence,
                victim.died_at,
            );
            (*entity, track)
        })
        .collect();

    let mut candidates: Vec<Candidate<'_>> = Vec::new();
    let mut secondaries: Vec<SecondaryImpact> = Vec::new();
    let mut exclusions: BTreeMap<ExclusionReason, u32> = BTreeMap::new();

    for hit in input.hits {
        // `salvo` is `Some` only for a hit matched to its originating salvo.
        // Without it the shell is unidentifiable and `victim_entity_id` falls
        // back to the self ship, so an unmatched hit is not a candidate at all.
        let Some(salvo) = hit.salvo.as_ref() else { continue };
        if salvo.owner_id != input.attacker.entity {
            continue;
        }

        let eligibility = classify(input, &geometry, &tracks, &bundle, tier, formula_applies, hit, salvo.params_id);

        // Our own secondaries contest any fire credited to a main-battery hit
        // in the same window and section, so record where they landed even
        // though they are not candidates themselves.
        if matches!(eligibility, HitEligibility::NotMainBattery)
            && is_secondary_shell(input, salvo.params_id)
            && let Some(section) = section_of(input, &geometry, hit)
        {
            secondaries.push(SecondaryImpact { victim: hit.victim_entity_id, section, clock: hit.clock });
        }

        match eligibility {
            HitEligibility::Eligible { section, expected } => candidates.push(Candidate {
                hit,
                victim: hit.victim_entity_id,
                section,
                expected,
                shell: salvo.params_id,
            }),
            other => {
                if let Some(reason) = other.exclusion_reason() {
                    *exclusions.entry(reason).or_insert(0) += 1;
                }
            }
        }
    }

    let attribution = attribute(input, &candidates, &secondaries);
    if !attribution.contested.is_empty() {
        *exclusions.entry(ExclusionReason::SecondaryFireAmbiguous).or_insert(0) += attribution.contested.len() as u32;
    }

    let counted: Vec<&Candidate<'_>> =
        candidates.iter().enumerate().filter(|(i, _)| !attribution.contested.contains(i)).map(|(_, c)| c).collect();

    let per_ship = per_ship_breakdown(input, &counted, &attribution.fires_by_victim, formula_applies);
    let expected_fires =
        formula_applies.then(|| counted.iter().filter_map(|c| c.expected).map(BurnChance::get).sum::<f32>());

    Some(EffectiveFireChance {
        eligible_hits: counted.len() as u32,
        fires: attribution.predictions.len() as u32,
        expected_fires,
        per_ship,
        exclusions,
        section_predictions: attribution.predictions,
        unattributed_fires: attribution.unattributed,
        formula: formula_steps(input, &bundle, tier, &counted),
    })
}

/// Which fire section a hit landed in, when the victim's hull has geometry.
fn section_of(
    input: &FireChanceInput<'_>,
    geometry: &HashMap<EntityId, FireSectionGeometry>,
    hit: &ResolvedShotHit,
) -> Option<BurnNodeIndex> {
    let geom = geometry.get(&hit.victim_entity_id)?;
    let section =
        section_for_hit(geom, hit.hit.position, hit.victim_position, hit.victim_yaw, hit.victim_pitch, hit.victim_roll);
    // The hull's `burnNodes` list is what makes a section's probability
    // readable; a geometry longer than it would index past the hull's own data.
    let victim = input.victims.get(&hit.victim_entity_id)?;
    (usize::from(section.get()) < victim.node_probability.len()).then_some(section)
}

fn is_secondary_shell(input: &FireChanceInput<'_>, shell: GameParamId) -> bool {
    input
        .params
        .game_param_by_id(shell)
        .is_some_and(|param| input.attacker.secondary_ammo.iter().any(|name| name == param.name()))
}

/// Whether one of our shells could have started a fire where it landed.
///
/// The checks run in a fixed order and the first match wins, so the exclusion
/// tally names the most specific reason a hit failed rather than the first
/// cheap one that also happened to hold.
#[allow(clippy::too_many_arguments)]
fn classify(
    input: &FireChanceInput<'_>,
    geometry: &HashMap<EntityId, FireSectionGeometry>,
    tracks: &HashMap<EntityId, VictimTrack>,
    bundle: &ModifierBundle,
    tier: u32,
    formula_applies: bool,
    hit: &ResolvedShotHit,
    shell: GameParamId,
) -> HitEligibility {
    let Some(param) = input.params.game_param_by_id(shell) else {
        return HitEligibility::ShellCannotBurn;
    };
    if !input.attacker.main_battery_ammo.iter().any(|name| name == param.name()) {
        return HitEligibility::NotMainBattery;
    }
    let Some(projectile) = param.projectile() else {
        return HitEligibility::ShellCannotBurn;
    };
    // A projectile with no `burnProb` at all carries no fire chance to gate on.
    let Some(burn_prob) = projectile.burn_prob() else {
        return HitEligibility::ShellCannotBurn;
    };
    let is_small = is_small_projectile(projectile.bullet_diametr(), SMALL_PROJECTILE_MAX_DIAMETER_M);
    let (pre_clamp, _) = calculate_burn_chance(tier, burn_prob, bundle, is_small);
    // `calculate_burn_chance` returns the pre-clamp value; ttx's contract is
    // that the caller applies `max(0)`. The upper bound is the probability's own
    // definition: additive bonuses can in principle sum past 1, and a chance
    // above 1 still only means "always".
    let chance = pre_clamp.clamp(0.0, 1.0);
    // A non-finite chance survives the clamp, and `NaN <= 0.0` is false, so it
    // is rejected explicitly: admitting a hit whose chance is unknowable is the
    // direction that corrupts the result.
    if !chance.is_finite() || chance <= 0.0 {
        return HitEligibility::ShellCannotBurn;
    }

    if !rolls_for_fire(&hit.hit.hit_type.shell_hit) {
        return HitEligibility::HitTypeDoesNotRoll(hit.hit.hit_type.shell_hit.clone());
    }

    // A victim with no context has no hull, so it has no geometry either.
    let (Some(victim), Some(section)) = (input.victims.get(&hit.victim_entity_id), section_of(input, geometry, hit))
    else {
        return HitEligibility::NoSectionGeometry;
    };
    let Some(track) = tracks.get(&hit.victim_entity_id) else {
        return HitEligibility::NoSectionGeometry;
    };

    if victim.died_at.is_some_and(|died_at| hit.clock >= died_at) {
        return HitEligibility::VictimDead;
    }

    // A point query, not a range: a presence window logs a baseline burn mask
    // when it opens and every transition inside it, so a victim in AOI at the
    // hit clock has an exact mask there. What an earlier gap loses is when a
    // section was lit, which this analysis does not ask.
    if !input.presence.continuously_observed(hit.victim_entity_id, hit.clock, hit.clock) {
        return HitEligibility::ObservationGap;
    }

    if track.cooldown_unreliable() {
        return HitEligibility::ConsumableModelUnreliable;
    }
    match track.damage_control_at(hit.clock) {
        DamageControlState::Running => return HitEligibility::DamageControlActive,
        DamageControlState::Unknown => return HitEligibility::DamageControlUnknown,
        DamageControlState::Down => {}
    }

    if victim.fire_resistance_enabled && section.get() == FIRE_PREVENTION_SUPPRESSED_NODE {
        return HitEligibility::SectionSuppressedByFirePrevention;
    }

    if track.burn_mask_before(hit.clock) & section.bit_mask() != 0 {
        return HitEligibility::SectionAlreadyBurning(section);
    }

    // Node probability is indexed by section; `section_of` has already checked
    // the section is inside the hull's own `burnNodes` list. The product is
    // clamped because a few non-combat and bot hulls carry a node probability
    // of 9.0, which the client's `random() < prob` roll simply reads as always.
    // A non-finite product survives the clamp and `BurnChance::new` rejects it,
    // which is the honest answer: it would take a GameParams float that is not
    // a number, and that makes the expectation unknown rather than zero.
    let expected = formula_applies
        .then(|| chance * victim.node_probability[usize::from(section.get())] * victim.burn_prob)
        .and_then(|product| BurnChance::new(product.clamp(0.0, 1.0)));
    HitEligibility::Eligible { section, expected }
}

/// Hit types that roll for fire. An HE shell detonates on the plate whether or
/// not it penetrates, so a shatter rolls. Ricochets, overpenetrations,
/// underwater hits and ids this build cannot name are excluded because nothing
/// establishes that they roll.
fn rolls_for_fire(shell_hit: &Recognized<ShellHitType>) -> bool {
    matches!(shell_hit.known(), Some(ShellHitType::Normal | ShellHitType::MajorHit | ShellHitType::NoPenetration))
}

struct Attribution {
    predictions: Vec<SectionPrediction>,
    /// Candidate indices dropped because one of our secondaries could equally
    /// have set the fire.
    contested: Vec<usize>,
    unattributed: u32,
    fires_by_victim: HashMap<EntityId, u32>,
}

/// Match each self `SetFire` ribbon to one eligible hit of ours.
///
/// The ribbon says a fire was ours, not which weapon set it, so it separates us
/// from other attackers but not from our own secondaries. A ribbon whose window
/// holds both an eligible main-battery hit and one of our secondary hits on the
/// same victim and section is dropped from numerator and denominator alike,
/// exactly as a multi-attacker collision would be.
fn attribute(
    input: &FireChanceInput<'_>,
    candidates: &[Candidate<'_>],
    secondaries: &[SecondaryImpact],
) -> Attribution {
    let mut consumed = vec![false; candidates.len()];
    let mut contested: Vec<usize> = Vec::new();
    let mut predictions = Vec::new();
    let mut fires_by_victim: HashMap<EntityId, u32> = HashMap::new();
    let mut unattributed = 0u32;

    for ribbon in input.ribbons.iter().filter(|r| r.ribbon == Ribbon::SetFire) {
        for _ in 0..ribbon.count {
            let pick = candidates
                .iter()
                .enumerate()
                .filter(|(i, _)| !consumed[*i] && !contested.contains(i))
                .filter(|(_, c)| within_window(c.hit.clock, ribbon.clock))
                .filter_map(|(i, c)| lit_change(input, c, ribbon.clock).map(|change| (i, c, change)))
                .min_by(|(_, a, _), (_, b, _)| {
                    clock_distance(a.hit.clock, ribbon.clock).total_cmp(&clock_distance(b.hit.clock, ribbon.clock))
                });

            let Some((index, candidate, change)) = pick else {
                unattributed += 1;
                continue;
            };

            let ambiguous = secondaries.iter().any(|s| {
                s.victim == candidate.victim && s.section == candidate.section && within_window(s.clock, ribbon.clock)
            });
            if ambiguous {
                // Neither a fire nor unattributed: the fire is real and ours,
                // it just cannot be assigned to a weapon.
                contested.push(index);
                continue;
            }

            consumed[index] = true;
            *fires_by_victim.entry(candidate.victim).or_insert(0) += 1;
            predictions.push(SectionPrediction {
                predicted: candidate.section,
                actual: nearest_risen(change, candidate.section),
            });
        }
    }

    Attribution { predictions, contested, unattributed, fires_by_victim }
}

fn clock_distance(a: GameClock, b: GameClock) -> f32 {
    (a.seconds() - b.seconds()).abs()
}

fn within_window(clock: GameClock, ribbon: GameClock) -> bool {
    clock_distance(clock, ribbon) <= ATTRIBUTION_WINDOW
}

/// The burn-state change nearest the ribbon that lit at least one section on
/// this candidate's victim. Any newly lit bit qualifies, not only the predicted
/// one: requiring the prediction to match would make `section_agreement`
/// measure nothing.
fn lit_change<'a>(
    input: &'a FireChanceInput<'_>,
    candidate: &Candidate<'_>,
    ribbon: GameClock,
) -> Option<&'a BurnStateChange> {
    input
        .burn_state_changes
        .iter()
        .filter(|c| c.victim == candidate.victim && within_window(c.clock, ribbon))
        .filter(|c| c.newly_lit().next().is_some())
        .min_by(|a, b| clock_distance(a.clock, ribbon).total_cmp(&clock_distance(b.clock, ribbon)))
}

/// The bit that rose nearest the prediction. A single change can light several
/// sections; the pair is recorded either way so a corpus confusion matrix stays
/// complete.
fn nearest_risen(change: &BurnStateChange, predicted: BurnNodeIndex) -> BurnNodeIndex {
    // `lit_change` only returns changes with at least one risen bit, so the
    // fallback is unreachable; it reads the prediction back rather than
    // inventing a section, which would put a fabricated pair in the matrix.
    change.newly_lit().min_by_key(|risen| risen.get().abs_diff(predicted.get())).unwrap_or(predicted)
}

fn per_ship_breakdown(
    input: &FireChanceInput<'_>,
    counted: &[&Candidate<'_>],
    fires_by_victim: &HashMap<EntityId, u32>,
    formula_applies: bool,
) -> Vec<PerShipFireChance> {
    // Keyed by ship index rather than entity so two of the same ship on the
    // enemy team read as one row, which is what a per-target-ship breakdown
    // means. `BTreeMap` because the row order must not depend on hash seeds.
    let mut rows: BTreeMap<&str, PerShipFireChance> = BTreeMap::new();
    let mut victims_seen: BTreeMap<&str, Vec<EntityId>> = BTreeMap::new();

    for candidate in counted {
        let Some(victim) = input.victims.get(&candidate.victim) else { continue };
        let row = rows.entry(victim.ship_index.as_str()).or_insert_with(|| PerShipFireChance {
            victim_ship_index: victim.ship_index.clone(),
            victim_ship_name: victim.ship_name.clone(),
            eligible_hits: 0,
            fires: 0,
            expected_fires: formula_applies.then_some(0.0),
        });
        row.eligible_hits += 1;
        if let (Some(total), Some(expected)) = (row.expected_fires.as_mut(), candidate.expected) {
            *total += expected.get();
        }
        let seen = victims_seen.entry(victim.ship_index.as_str()).or_default();
        if !seen.contains(&candidate.victim) {
            seen.push(candidate.victim);
        }
    }

    for (index, entities) in victims_seen {
        if let Some(row) = rows.get_mut(index) {
            row.fires = entities.iter().filter_map(|e| fires_by_victim.get(e)).sum();
        }
    }

    rows.into_values().collect()
}

/// The attacker's burn-chance formula for the shell that produced the most
/// eligible hits, keeping only the steps that moved the value.
fn formula_steps(
    input: &FireChanceInput<'_>,
    bundle: &ModifierBundle,
    tier: u32,
    counted: &[&Candidate<'_>],
) -> Vec<FormulaStep> {
    let mut hits_per_shell: HashMap<GameParamId, u32> = HashMap::new();
    for candidate in counted {
        *hits_per_shell.entry(candidate.shell).or_insert(0) += 1;
    }
    // Ties break on the lower id so the rendered breakdown is stable between runs.
    let Some((shell, _)) = hits_per_shell.into_iter().max_by_key(|(id, count)| (*count, std::cmp::Reverse(id.raw())))
    else {
        return Vec::new();
    };

    let Some(projectile) = input.params.game_param_by_id(shell).and_then(|p| p.projectile().cloned()) else {
        return Vec::new();
    };
    let Some(burn_prob) = projectile.burn_prob() else {
        return Vec::new();
    };
    let is_small = is_small_projectile(projectile.bullet_diametr(), SMALL_PROJECTILE_MAX_DIAMETER_M);
    let (_, applied) = calculate_burn_chance(tier, burn_prob, bundle, is_small);

    let mut running = burn_prob;
    let mut steps = Vec::new();
    for modifier in applied {
        let (op, value) = match modifier.op {
            Op::Mul => (FormulaOp::Multiply, bundle.coef(modifier.name)),
            Op::Add => (FormulaOp::Add, bundle.bonus(modifier.name)),
        };
        let result = match op {
            FormulaOp::Multiply => running * value,
            FormulaOp::Add => running + value,
        };
        if result != running {
            steps.push(FormulaStep {
                modifier: modifier.name.to_owned(),
                source: modifier_source(input.attacker.build, modifier.name),
                op,
                value,
                result,
            });
        }
        running = result;
    }
    steps
}

/// The equipped upgrade, learned skill or signal that carries `name`, in the
/// order the build applies them. `None` means the value came from the stock
/// table rather than anything the player chose.
fn modifier_source(build: &ResolvedBuild, name: &str) -> Option<String> {
    for upgrade in &build.upgrades {
        if upgrade.modernization().is_some_and(|m| m.modifiers().iter().any(|x| x.name() == name)) {
            return Some(upgrade.name().to_owned());
        }
    }
    if let Some(crew) = build.captain.as_deref().and_then(|c| c.crew()) {
        for &skill_type in &build.skills {
            let Some(skill) = crew.skill_by_type(CrewSkillType::from(skill_type)) else { continue };
            if skill.modifiers().is_some_and(|mods| mods.iter().any(|x| x.name() == name)) {
                return Some(skill.internal_name().to_string());
            }
        }
    }
    for signal in &build.signals {
        if signal.exterior().is_some_and(|e| e.modifiers().iter().any(|x| x.name() == name)) {
            return Some(signal.name().to_owned());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;

    use wowsunpack::data::Version;
    use wowsunpack::game_params::types::CrewSkillModifier;
    use wowsunpack::game_params::types::GameParams;
    use wowsunpack::game_params::types::Modernization;
    use wowsunpack::game_params::types::Param;
    use wowsunpack::game_params::types::ParamData;
    use wowsunpack::game_params::types::Projectile;
    use wowsunpack::game_params::types::Species;
    use wowsunpack::game_params::types::Vehicle;
    use wowsunpack::game_types::GameParamId;
    use wowsunpack::game_types::Ribbon;
    use wowsunpack::game_types::ShellHitType;
    use wowsunpack::game_types::WorldPos;
    use wowsunpack::models::fire_nodes::FireSectionGeometry;
    use wowsunpack::recognized::Recognized;

    use wows_battle_world::resources::BurnStateChange;
    use wows_battle_world::resources::PresenceLog;
    use wows_battle_world::resources::PresenceWindow;
    use wows_battle_world::resources::RibbonEvent;
    use wows_replays::analyzer::battle_controller::state::ActiveConsumable;
    use wows_replays::analyzer::battle_controller::state::ConsumableInventory;
    use wows_replays::analyzer::battle_controller::state::ResolvedShotHit;
    use wows_replays::analyzer::decoder::ArtillerySalvo;
    use wows_replays::analyzer::decoder::HitType;
    use wows_replays::analyzer::decoder::ShotHit;
    use wows_replays::types::EntityId;
    use wows_replays::types::GameClock;
    use wows_replays::types::ShotId;

    use crate::build::ResolvedBuild;

    const VERSION: Version = Version::base(15, 0, 0);

    fn attacker_id() -> EntityId {
        EntityId::from(1u32)
    }

    fn victim_id() -> EntityId {
        EntityId::from(2u32)
    }

    fn ship_id() -> GameParamId {
        GameParamId::from(10u32)
    }

    fn main_shell_id() -> GameParamId {
        GameParamId::from(20u32)
    }

    fn atba_shell_id() -> GameParamId {
        GameParamId::from(21u32)
    }

    fn upgrade_id() -> GameParamId {
        GameParamId::from(30u32)
    }

    /// A second ship, used only where a scenario needs two victims.
    fn other_victim_id() -> EntityId {
        EntityId::from(3u32)
    }

    const MAIN_SHELL: &str = "PAPT001_HE";
    const ATBA_SHELL: &str = "PAPT002_ATBA_HE";

    const HULL_MODEL: &str = "content/gameplay/usa/ship/cruiser/ACR001/ACR001.model";

    /// Four nodes bow to stern, an Iowa-like hull.
    const NODES: [f32; 4] = [93.0, 19.0, -35.0, -99.0];

    const HIT_CLOCK: GameClock = GameClock(100.0);

    /// What the fixture makes Damage Control Party resolve to for the victim.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Dcp {
        Down,
        Running,
        Unknown,
        /// Two activations closer together than the modelled reload, the
        /// signature of a ship that refunds charges.
        Refund,
    }

    /// A scenario in literal values. Every test perturbs exactly one thing.
    struct Fixture {
        main_burn_prob: f32,
        victim_node_probability: f32,
        victim_burn_prob: f32,
        victim_fire_resistance: bool,
        victim_died_at: Option<GameClock>,
        hit_section: u8,
        hit_type: ShellHitType,
        hit_from_atba: bool,
        secondary_hit_section: Option<u8>,
        server_lit_section: u8,
        ribbons: bool,
        stray_ribbon: Option<GameClock>,
        burn_mask_before: u16,
        geometry_available: bool,
        dcp: Dcp,
        observation_gap: bool,
        salvo_matched: bool,
        salvo_from_another_ship: bool,
        hit_on_a_hull_we_cannot_place: bool,
        unclassifiable_modifier: bool,
    }

    fn fixture() -> Fixture {
        Fixture {
            main_burn_prob: 0.12,
            victim_node_probability: 0.6,
            victim_burn_prob: 1.0,
            victim_fire_resistance: false,
            victim_died_at: None,
            hit_section: 0,
            hit_type: ShellHitType::Normal,
            hit_from_atba: false,
            secondary_hit_section: None,
            server_lit_section: 0,
            ribbons: true,
            stray_ribbon: None,
            burn_mask_before: 0,
            geometry_available: true,
            dcp: Dcp::Down,
            observation_gap: false,
            salvo_matched: true,
            salvo_from_another_ship: false,
            hit_on_a_hull_we_cannot_place: false,
            unclassifiable_modifier: false,
        }
    }

    impl Fixture {
        fn with_shell_burn_prob(mut self, burn_prob: f32) -> Fixture {
            self.main_burn_prob = burn_prob;
            self
        }

        fn with_victim_node_probability(mut self, probability: f32) -> Fixture {
            self.victim_node_probability = probability;
            self
        }

        fn with_victim_burn_prob_modifier(mut self, burn_prob: f32) -> Fixture {
            self.victim_burn_prob = burn_prob;
            self
        }

        fn with_victim_fire_prevention(mut self) -> Fixture {
            self.victim_fire_resistance = true;
            self
        }

        fn hitting_section(mut self, section: u8) -> Fixture {
            self.hit_section = section;
            self
        }

        fn with_hit_type(mut self, hit_type: ShellHitType) -> Fixture {
            self.hit_type = hit_type;
            self
        }

        fn with_shell_from_atba(mut self) -> Fixture {
            self.hit_from_atba = true;
            self
        }

        fn with_coincident_secondary_hit(mut self) -> Fixture {
            self.secondary_hit_section = Some(self.hit_section);
            self
        }

        fn with_secondary_hit_on_section(mut self, section: u8) -> Fixture {
            self.secondary_hit_section = Some(section);
            self
        }

        fn server_lights_section(mut self, section: u8) -> Fixture {
            self.server_lit_section = section;
            self
        }

        fn without_ribbons(mut self) -> Fixture {
            self.ribbons = false;
            self
        }

        fn with_stray_ribbon_at(mut self, clock: GameClock) -> Fixture {
            self.stray_ribbon = Some(clock);
            self
        }

        fn with_burn_mask_before(mut self, mask: u16) -> Fixture {
            self.burn_mask_before = mask;
            self
        }

        fn without_geometry(mut self) -> Fixture {
            self.geometry_available = false;
            self
        }

        fn with_dcp_running(mut self) -> Fixture {
            self.dcp = Dcp::Running;
            self
        }

        fn with_dcp_unknown(mut self) -> Fixture {
            self.dcp = Dcp::Unknown;
            self
        }

        fn with_a_refunding_consumable(mut self) -> Fixture {
            self.dcp = Dcp::Refund;
            self
        }

        fn with_the_victim_dead_at(mut self, clock: GameClock) -> Fixture {
            self.victim_died_at = Some(clock);
            self
        }

        fn out_of_aoi_at_the_hit(mut self) -> Fixture {
            self.observation_gap = true;
            self
        }

        fn without_a_matched_salvo(mut self) -> Fixture {
            self.salvo_matched = false;
            self
        }

        fn fired_by_another_ship(mut self) -> Fixture {
            self.salvo_from_another_ship = true;
            self
        }

        fn also_hitting_a_hull_we_cannot_place(mut self) -> Fixture {
            self.hit_on_a_hull_we_cannot_place = true;
            self
        }

        fn with_a_modifier_this_version_cannot_classify(mut self) -> Fixture {
            self.unclassifiable_modifier = true;
            self
        }

        /// Assemble the borrowing input. The scenario's owned data is leaked so
        /// `build()` stays a single expression at every call site; a test
        /// process is short-lived and this is a few hundred bytes per test.
        fn build(self) -> FireChanceInput<'static> {
            let params: &'static GameParams = Box::leak(Box::new(GameParams::from(vec![
                ship_param(),
                shell_param(main_shell_id(), MAIN_SHELL, self.main_burn_prob, 0.203),
                shell_param(atba_shell_id(), ATBA_SHELL, 0.05, 0.127),
                unclassifiable_upgrade_param(),
            ])));

            let upgrades: &[GameParamId] = if self.unclassifiable_modifier { &[upgrade_id()] } else { &[] };
            let build: &'static ResolvedBuild = Box::leak(Box::new(
                ResolvedBuild::from_ids(
                    ship_id(),
                    &[],
                    upgrades,
                    None,
                    &[],
                    &[],
                    &[],
                    Species::Cruiser,
                    VERSION,
                    params,
                )
                .expect("fixture build resolves"),
            ));

            let main_ammo: &'static [String] = Box::leak(Box::new(vec![MAIN_SHELL.to_owned()]));
            let atba_ammo: &'static [String] = Box::leak(Box::new(vec![ATBA_SHELL.to_owned()]));

            let mut hits = vec![hit(
                victim_id(),
                if self.hit_from_atba { atba_shell_id() } else { main_shell_id() },
                HIT_CLOCK,
                self.hit_section,
                self.hit_type,
            )];
            if !self.salvo_matched {
                hits[0].salvo = None;
            }
            if self.salvo_from_another_ship
                && let Some(salvo) = hits[0].salvo.as_mut()
            {
                salvo.owner_id = EntityId::from(99u32);
            }
            if let Some(section) = self.secondary_hit_section {
                hits.push(hit(
                    victim_id(),
                    atba_shell_id(),
                    GameClock(HIT_CLOCK.0 + 0.1),
                    section,
                    ShellHitType::Normal,
                ));
            }
            if self.hit_on_a_hull_we_cannot_place {
                hits.push(hit(other_victim_id(), main_shell_id(), HIT_CLOCK, 0, ShellHitType::Normal));
            }

            let mut changes = Vec::new();
            if self.burn_mask_before != 0 {
                changes.push(BurnStateChange {
                    victim: victim_id(),
                    clock: GameClock(HIT_CLOCK.0 - 50.0),
                    previous: 0,
                    current: self.burn_mask_before,
                });
            }
            let lit = 1u16 << self.server_lit_section;
            changes.push(BurnStateChange {
                victim: victim_id(),
                clock: HIT_CLOCK,
                previous: self.burn_mask_before,
                current: self.burn_mask_before | lit,
            });

            let mut ribbons = Vec::new();
            if self.ribbons {
                ribbons.push(RibbonEvent { clock: HIT_CLOCK, ribbon: Ribbon::SetFire, count: 1 });
            }
            if let Some(clock) = self.stray_ribbon {
                ribbons.push(RibbonEvent { clock, ribbon: Ribbon::SetFire, count: 1 });
            }

            let windows = if self.observation_gap {
                // Left AOI before the hit and never came back, so nothing about
                // the victim is known at the hit clock.
                vec![PresenceWindow { entered: GameClock(0.0), left: Some(GameClock(50.0)) }]
            } else if self.dcp == Dcp::Unknown {
                // Back in AOI at the hit clock, but an activation could have
                // happened unseen during the earlier gap.
                vec![
                    PresenceWindow { entered: GameClock(0.0), left: Some(GameClock(50.0)) },
                    PresenceWindow { entered: GameClock(90.0), left: None },
                ]
            } else {
                vec![PresenceWindow { entered: GameClock(0.0), left: None }]
            };
            let mut presence = PresenceLog::default();
            presence.0.insert(victim_id(), windows);
            presence.0.insert(other_victim_id(), vec![PresenceWindow { entered: GameClock(0.0), left: None }]);

            let mut activations: HashMap<EntityId, Vec<ActiveConsumable>> = HashMap::new();
            let mut consumables = Vec::new();
            let activated_at: &[f32] = match self.dcp {
                Dcp::Running => &[HIT_CLOCK.0 - 5.0],
                Dcp::Refund => &[20.0, 50.0],
                Dcp::Down | Dcp::Unknown => &[],
            };
            if !activated_at.is_empty() {
                activations.insert(
                    victim_id(),
                    activated_at
                        .iter()
                        .map(|clock| ActiveConsumable {
                            consumable: Recognized::Known(wowsunpack::game_types::Consumable::DamageControl),
                            activated_at: GameClock(*clock),
                            duration: 15.0,
                            usage_params: None,
                        })
                        .collect(),
                );
                consumables.push(dcp_inventory());
            }

            let mut victims = HashMap::new();
            victims.insert(
                victim_id(),
                VictimContext {
                    ship_index: "PZAO".to_owned(),
                    ship_name: "Zao".to_owned(),
                    hull_model_path: HULL_MODEL.to_owned(),
                    node_probability: vec![self.victim_node_probability; NODES.len()],
                    burn_prob: self.victim_burn_prob,
                    fire_resistance_enabled: self.victim_fire_resistance,
                    died_at: self.victim_died_at,
                    consumables,
                },
            );
            if self.hit_on_a_hull_we_cannot_place {
                victims.insert(
                    other_victim_id(),
                    VictimContext {
                        ship_index: "PIOW".to_owned(),
                        ship_name: "Iowa".to_owned(),
                        hull_model_path: "content/gameplay/usa/ship/battleship/ASB028/ASB028.model".to_owned(),
                        node_probability: vec![0.6; NODES.len()],
                        burn_prob: 1.0,
                        fire_resistance_enabled: false,
                        died_at: None,
                        consumables: Vec::new(),
                    },
                );
            }

            let geometry_available = self.geometry_available;
            let geometry: &'static dyn Fn(&str) -> Option<FireSectionGeometry> =
                Box::leak(Box::new(move |path: &str| {
                    (geometry_available && path == HULL_MODEL)
                        .then(|| {
                            FireSectionGeometry::from_longitudinal(
                                NODES.iter().copied().map(wowsunpack::game_params::types::Meters::from).collect(),
                            )
                        })
                        .flatten()
                }));

            FireChanceInput {
                version: VERSION,
                attacker: AttackerContext {
                    entity: attacker_id(),
                    build,
                    main_battery_ammo: main_ammo,
                    secondary_ammo: atba_ammo,
                },
                victims: Box::leak(Box::new(victims)),
                hits: Box::leak(Box::new(hits)),
                ribbons: Box::leak(Box::new(ribbons)),
                burn_state_changes: Box::leak(Box::new(changes)),
                presence: Box::leak(Box::new(presence)),
                activations: Box::leak(Box::new(activations)),
                params,
                geometry,
            }
        }
    }

    fn ship_param() -> Param {
        Param::builder()
            .id(ship_id())
            .index("PZAO".to_owned())
            .name("PZAO_Zao".to_owned())
            .nation("Japan".to_owned())
            .data(ParamData::Vehicle(
                Vehicle::builder()
                    .level(10)
                    .group("special".to_owned())
                    .maybe_abilities(None)
                    .upgrades(Vec::new())
                    .maybe_config_data(None)
                    .maybe_model_path(None)
                    .maybe_armor(None)
                    .maybe_hit_locations(None)
                    .permoflages(Vec::new())
                    .camera_trajectories(Vec::new())
                    .maybe_ttx_components(None)
                    .innate_skills(Vec::new())
                    .build(),
            ))
            .build()
    }

    fn shell_param(id: GameParamId, name: &str, burn_prob: f32, caliber_m: f32) -> Param {
        Param::builder()
            .id(id)
            .index(name.to_owned())
            .name(name.to_owned())
            .nation("Japan".to_owned())
            .data(ParamData::Projectile(
                Projectile::builder().ammo_type("HE".to_owned()).burn_prob(burn_prob).bullet_diametr(caliber_m).build(),
            ))
            .build()
    }

    /// An upgrade carrying a modifier name no MODIFIER_SETTINGS table has, so
    /// folding the build's bundle fails the way a pre-0.7 replay's does.
    fn unclassifiable_upgrade_param() -> Param {
        Param::builder()
            .id(upgrade_id())
            .index("PCM999_NotAReal_Mod".to_owned())
            .name("PCM999_NotAReal_Mod".to_owned())
            .nation("Japan".to_owned())
            .data(ParamData::Modernization(Modernization::new(
                vec![
                    CrewSkillModifier::builder()
                        .name("definitelyNotAModifier_xyz".to_owned())
                        .aircraft_carrier(1.0)
                        .auxiliary(1.0)
                        .battleship(1.0)
                        .cruiser(1.0)
                        .destroyer(1.0)
                        .submarine(1.0)
                        .excluded_consumables(Vec::new())
                        .build(),
                ],
                None,
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            )))
            .build()
    }

    fn dcp_inventory() -> ConsumableInventory {
        ConsumableInventory {
            slot_index: 0,
            consumable_type_raw: "PCY001_CrashCrew".to_owned(),
            consumable: Recognized::Known(wowsunpack::game_types::Consumable::DamageControl),
            icon_key: "PCY001_CrashCrew".to_owned(),
            total_charges: wowsunpack::game_types::ChargeCount::Unlimited,
            charges_used: 0,
            work_time: 15.0,
            reload_time: 80.0,
            regen_hp_speed: None,
            regen_hp_speed_units: None,
            active_until: None,
        }
    }

    /// One hit on `victim`, placed along the hull so it resolves to `section`.
    fn hit(
        victim: EntityId,
        shell: GameParamId,
        clock: GameClock,
        section: u8,
        shell_hit: ShellHitType,
    ) -> ResolvedShotHit {
        ResolvedShotHit {
            clock,
            hit: ShotHit {
                owner_id: attacker_id(),
                hit_type: HitType {
                    collision: Recognized::Known(wowsunpack::game_types::CollisionType::HitEntity),
                    shell_hit: Recognized::Known(shell_hit),
                    raw: 0,
                },
                shot_id: ShotId::from(1u32),
                position: WorldPos::new(NODES[section as usize], 0.0, 0.0),
                terminal_ballistics: None,
            },
            victim_entity_id: victim,
            salvo: Some(ArtillerySalvo { owner_id: attacker_id(), params_id: shell, salvo_id: 1, shots: Vec::new() }),
            fired_at: Some(GameClock(clock.0 - 5.0)),
            victim_position: WorldPos::new(0.0, 0.0, 0.0),
            victim_yaw: 0.0,
            victim_pitch: 0.0,
            victim_roll: 0.0,
        }
    }

    /// Clean case: one HE hit on a cold target with DCP down, and a SetFire
    /// ribbon at the same clock.
    #[test]
    fn a_clean_hit_that_started_a_fire_counts_once() {
        let out = analyze(&fixture().build()).expect("geometry present");
        assert_eq!(out.eligible_hits, 1);
        assert_eq!(out.fires, 1);
        assert_eq!(out.rate(), Some(1.0));
        assert_eq!(out.unattributed_fires, 0);
    }

    /// The same hit with no ribbon is an eligible miss, not an exclusion.
    #[test]
    fn an_eligible_hit_with_no_ribbon_is_a_miss() {
        let out = analyze(&fixture().without_ribbons().build()).expect("geometry");
        assert_eq!(out.eligible_hits, 1);
        assert_eq!(out.fires, 0);
        assert_eq!(out.rate(), Some(0.0));
    }

    /// A hit on a section that is already burning had no chance.
    #[test]
    fn a_hit_on_a_burning_section_is_excluded() {
        let out = analyze(&fixture().with_burn_mask_before(0b0001).build()).expect("geometry");
        assert_eq!(out.eligible_hits, 0);
        assert_eq!(out.exclusions[&ExclusionReason::SectionAlreadyBurning], 1);
    }

    /// A different section burning does not exclude: the sections are
    /// independent, which is the whole point of resolving them.
    #[test]
    fn a_hit_on_a_free_section_counts_while_another_burns() {
        let out = analyze(&fixture().with_burn_mask_before(0b1000).build()).expect("geometry");
        assert_eq!(out.eligible_hits, 1);
    }

    /// With Fire Prevention Expert the victim's node 2 can never burn, so a
    /// hit there is excluded rather than counted as a miss.
    #[test]
    fn a_hit_on_the_fire_prevention_suppressed_node_is_excluded() {
        let out = analyze(&fixture().with_victim_fire_prevention().hitting_section(2).build()).expect("geometry");
        assert_eq!(out.eligible_hits, 0);
        assert_eq!(out.exclusions[&ExclusionReason::SectionSuppressedByFirePrevention], 1);
    }

    #[test]
    fn a_hit_while_damage_control_runs_is_excluded() {
        let out = analyze(&fixture().with_dcp_running().build()).expect("geometry");
        assert_eq!(out.eligible_hits, 0);
        assert_eq!(out.exclusions[&ExclusionReason::DamageControlActive], 1);
    }

    #[test]
    fn a_hit_with_unknown_damage_control_is_excluded() {
        let out = analyze(&fixture().with_dcp_unknown().build()).expect("geometry");
        assert_eq!(out.eligible_hits, 0);
        assert_eq!(out.exclusions[&ExclusionReason::DamageControlUnknown], 1);
    }

    /// AP carries burnProb -0.5, a sentinel that absorbs every additive bonus.
    /// The gate is calculate_burn_chance > 0, never an ammo-type string test.
    #[test]
    fn an_ap_hit_is_excluded_by_the_chance_gate() {
        let out = analyze(&fixture().with_shell_burn_prob(-0.5).build()).expect("geometry");
        assert_eq!(out.eligible_hits, 0);
        assert_eq!(out.exclusions[&ExclusionReason::ShellCannotBurn], 1);
    }

    #[test]
    fn a_secondary_hit_is_excluded() {
        let out = analyze(&fixture().with_shell_from_atba().build()).expect("geometry");
        assert_eq!(out.eligible_hits, 0);
        assert_eq!(out.exclusions[&ExclusionReason::NotMainBattery], 1);
    }

    /// A main-battery hit is dropped when one of our own secondary shells hit
    /// the same section inside the attribution window: the SetFire ribbon does
    /// not name the weapon, so crediting the main-battery shell would inflate
    /// the numerator on exactly the ships whose secondaries set most fires.
    #[test]
    fn a_fire_contested_by_our_own_secondary_is_dropped() {
        let out = analyze(&fixture().with_coincident_secondary_hit().build()).expect("geometry");
        assert_eq!(out.eligible_hits, 0);
        assert_eq!(out.fires, 0);
        assert_eq!(out.exclusions[&ExclusionReason::SecondaryFireAmbiguous], 1);
    }

    /// A secondary hit on a different section does not contest the fire.
    #[test]
    fn a_secondary_hit_elsewhere_does_not_contest() {
        let out = analyze(&fixture().hitting_section(0).with_secondary_hit_on_section(3).build()).expect("geometry");
        assert_eq!(out.eligible_hits, 1);
        assert_eq!(out.fires, 1);
    }

    /// Shatters roll (HE detonates on the plate); ricochets and overpens are
    /// excluded because nothing establishes that they do.
    #[test]
    fn hit_types_are_gated() {
        let shatter = analyze(&fixture().with_hit_type(ShellHitType::NoPenetration).build()).expect("geometry");
        assert_eq!(shatter.eligible_hits, 1);

        let ricochet = analyze(&fixture().with_hit_type(ShellHitType::Ricochet).build()).expect("geometry");
        assert_eq!(ricochet.eligible_hits, 0);
        assert_eq!(ricochet.exclusions[&ExclusionReason::HitTypeDoesNotRoll], 1);
    }

    /// Expected fires is attacker chance times the victim's node probability
    /// times the victim's burnProb: 0.12 * 0.6004 * 0.9.
    #[test]
    fn expected_fires_multiplies_both_halves() {
        let out = analyze(
            &fixture()
                .with_shell_burn_prob(0.12)
                .with_victim_node_probability(0.6004)
                .with_victim_burn_prob_modifier(0.9)
                .build(),
        )
        .expect("geometry");
        let want = 0.12 * 0.6004 * 0.9;
        assert!((out.expected_fires.expect("expected") - want).abs() < 1e-5);
    }

    /// A ribbon matching no eligible hit is reported, not dropped. Zero is the
    /// healthy value; nonzero means the pipeline is wrong somewhere.
    #[test]
    fn an_unmatched_ribbon_is_reported() {
        let out = analyze(&fixture().with_stray_ribbon_at(GameClock(999.0)).build()).expect("geometry");
        assert_eq!(out.unattributed_fires, 1);
    }

    /// Section agreement compares the section we predicted against the bit the
    /// server lit. This is the measurement the nearest-node assumption rests on.
    #[test]
    fn section_agreement_is_measured() {
        let agree = analyze(&fixture().server_lights_section(0).hitting_section(0).build()).expect("geometry");
        assert_eq!(agree.section_agreement(), Some(1.0));
        assert_eq!(agree.section_predictions.len(), 1);

        let disagree = analyze(&fixture().server_lights_section(3).hitting_section(0).build()).expect("geometry");
        assert_eq!(disagree.section_agreement(), Some(0.0));
        assert_eq!(disagree.section_predictions[0].actual.get(), 3);
    }

    /// With no attributed fires there is nothing to agree about.
    #[test]
    fn section_agreement_is_absent_without_fires() {
        let out = analyze(&fixture().without_ribbons().build()).expect("geometry");
        assert_eq!(out.section_agreement(), None);
    }

    /// A rate over zero samples is unknown, not zero.
    #[test]
    fn no_eligible_hits_yields_no_rate() {
        let out = analyze(&fixture().with_dcp_unknown().build()).expect("geometry");
        assert_eq!(out.rate(), None);
    }

    /// Without geometry there is no eligibility model, so there is no result.
    #[test]
    fn missing_geometry_yields_no_result() {
        assert!(analyze(&fixture().without_geometry().build()).is_none());
    }

    /// The per-ship breakdown names the victim and carries the same counts as
    /// the aggregate when only one ship was hit.
    #[test]
    fn the_per_ship_breakdown_names_the_victim() {
        let out = analyze(&fixture().build()).expect("geometry");
        assert_eq!(out.per_ship.len(), 1);
        assert_eq!(out.per_ship[0].victim_ship_index, "PZAO");
        assert_eq!(out.per_ship[0].victim_ship_name, "Zao");
        assert_eq!(out.per_ship[0].eligible_hits, 1);
        assert_eq!(out.per_ship[0].fires, 1);
    }

    /// A stock build moves no step of the burn-chance formula, so the hover
    /// breakdown is empty rather than a list of identities.
    #[test]
    fn a_stock_build_has_no_formula_steps() {
        let out = analyze(&fixture().build()).expect("geometry");
        assert!(out.formula.is_empty(), "got {:?}", out.formula);
    }

    /// A hit the parser never matched to a salvo names no shell, and its
    /// `victim_entity_id` falls back to the self ship, so it is not a candidate
    /// at all. Long-flight shells at maximum range land here systematically.
    #[test]
    fn an_unmatched_hit_is_not_a_candidate() {
        let out = analyze(&fixture().without_a_matched_salvo().build()).expect("geometry");
        assert_eq!(out.eligible_hits, 0);
        assert!(out.exclusions.is_empty(), "got {:?}", out.exclusions);
    }

    /// Someone else's shell is not ours to count, and is not an exclusion of
    /// ours either.
    #[test]
    fn another_ships_hit_is_ignored_entirely() {
        let out = analyze(&fixture().fired_by_another_ship().build()).expect("geometry");
        assert_eq!(out.eligible_hits, 0);
        assert!(out.exclusions.is_empty(), "got {:?}", out.exclusions);
    }

    /// The server lights extra burn nodes on death for visual effect, so a hit
    /// at or after the victim died proves nothing.
    #[test]
    fn a_hit_at_or_after_the_victims_death_is_excluded() {
        let out = analyze(&fixture().with_the_victim_dead_at(GameClock(90.0)).build()).expect("geometry");
        assert_eq!(out.eligible_hits, 0);
        assert_eq!(out.exclusions[&ExclusionReason::VictimDead], 1);
    }

    /// Outside the recording client's AOI nothing about the victim is known,
    /// so the burn mask the eligibility model reads is not reconstructible.
    #[test]
    fn a_hit_outside_the_observation_window_is_excluded() {
        let out = analyze(&fixture().out_of_aoi_at_the_hit().build()).expect("geometry");
        assert_eq!(out.eligible_hits, 0);
        assert_eq!(out.exclusions[&ExclusionReason::ObservationGap], 1);
    }

    /// Two activations closer than the modelled reload mean the ship refunds
    /// charges, so no cooldown inference about it is sound.
    #[test]
    fn a_hit_on_a_refunding_ship_is_excluded() {
        let out = analyze(&fixture().with_a_refunding_consumable().build()).expect("geometry");
        assert_eq!(out.eligible_hits, 0);
        assert_eq!(out.exclusions[&ExclusionReason::ConsumableModelUnreliable], 1);
    }

    /// One victim without geometry does not sink the analysis; that hit is an
    /// exclusion while the victim that does resolve still counts.
    #[test]
    fn a_hit_on_an_unplaceable_hull_is_excluded_without_losing_the_rest() {
        let out = analyze(&fixture().also_hitting_a_hull_we_cannot_place().build()).expect("geometry");
        assert_eq!(out.eligible_hits, 1);
        assert_eq!(out.exclusions[&ExclusionReason::NoSectionGeometry], 1);
    }

    /// A modifier name this version's table cannot classify makes the attacker
    /// formula unavailable. The observed rate does not need it and is still
    /// reported; only the expected value goes absent.
    #[test]
    fn an_unfoldable_build_keeps_the_observed_rate_and_drops_the_expected_value() {
        let out = analyze(&fixture().with_a_modifier_this_version_cannot_classify().build()).expect("geometry");
        assert_eq!(out.eligible_hits, 1);
        assert_eq!(out.fires, 1);
        assert_eq!(out.rate(), Some(1.0));
        assert_eq!(out.expected_fires, None);
        assert_eq!(out.per_ship[0].expected_fires, None);
    }
}
