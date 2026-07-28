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
///
/// This is a ribbon-relative radius: it bounds how far a hit can sit from the
/// ribbon it caused.
const ATTRIBUTION_WINDOW: f32 = 0.5;

/// How far apart two of our own hits can be and still both be plausible causes
/// of one fire.
///
/// Twice [`ATTRIBUTION_WINDOW`], because this one is hit-relative rather than
/// ribbon-relative: two hits can each sit inside a single ribbon's window while
/// lying a full window apart on opposite sides of it. Using the ribbon radius
/// here would leave a band where a main-battery hit is close enough to the
/// ribbon to be credited while a secondary of ours nearer the ribbon still
/// fails the contest, which inflates the numerator on exactly the secondary
/// ships the contest exists to protect.
const CONTEST_WINDOW: f32 = 2.0 * ATTRIBUTION_WINDOW;

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
    /// The victim's fate could not be resolved, so neither "this hit landed
    /// after it died" nor "this burn transition is a death flare" can be ruled
    /// out. Distinct from `VictimDead` so the cost of an unresolved fate is
    /// visible in the tally instead of hiding inside a real exclusion.
    VictimFateUnknown,
    ShellCannotBurn,
    NotMainBattery,
    /// Carries the raw `Recognized` rather than a bare [`ShellHitType`]: an id
    /// this build's constants table does not name is also a hit type nothing
    /// establishes as rolling, and flattening it would lose which one it was.
    HitTypeDoesNotRoll(Recognized<ShellHitType>),
    NoSectionGeometry,
    /// One of our own secondary shells landed in the same section inside the
    /// attribution window, so a SetFire ribbon there cannot be assigned to this
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
    VictimFateUnknown,
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
            HitEligibility::VictimFateUnknown => Some(ExclusionReason::VictimFateUnknown),
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
    /// Fires we could not assign to a specific hit: a SetFire ribbon that
    /// matched no eligible hit inside the attribution window.
    ///
    /// A healthy replay carries plenty of these, so a nonzero count is not by
    /// itself a fault. The legitimate causes are fires set by our own
    /// secondaries with no main-battery candidate nearby, fires on a victim
    /// outside the recording client's AOI, fires whose causing hit was excluded
    /// by any of the eligibility rules, and fires whose main-battery candidate
    /// was dropped as ambiguous with one of our secondaries. What the number is
    /// for is proportion: an unattributed count far larger than `fires` on a
    /// ship with no secondaries means something upstream is wrong.
    pub unattributed_fires: u32,
    /// The attacker-side formula steps, for the hover breakdown.
    pub formula: Vec<FormulaStep>,
}

/// How `actual` was determined for one attributed fire.
///
/// This is what says whether the pair is usable as evidence. A rate computed
/// over pairs where several sections rose at once is biased toward agreement,
/// because `actual` was chosen with the prediction in hand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SectionEvidence {
    /// Exactly one section rose in the matched transition, so `actual` is that
    /// section and nothing about it depends on what was predicted.
    OneSectionRose,
    /// Several sections rose in the same transition and `actual` is whichever
    /// of them sits nearest the prediction. Predicted and actual are not
    /// independent here.
    NearestOfSeveral,
}

/// One attributed fire's predicted section against the one the server lit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SectionPrediction {
    pub predicted: BurnNodeIndex,
    pub actual: BurnNodeIndex,
    pub evidence: SectionEvidence,
}

impl EffectiveFireChance {
    /// `None` when there are no eligible hits: a rate over zero samples is not
    /// zero, it is unknown.
    pub fn rate(&self) -> Option<f32> {
        (self.eligible_hits > 0).then(|| self.fires as f32 / self.eligible_hits as f32)
    }

    /// Fraction of attributed fires whose predicted section is the bit the
    /// server lit. `None` when no fires were attributed.
    ///
    /// Taken over every pair, including the ones whose `actual` was chosen as
    /// the risen bit nearest the prediction. Those are not independent of the
    /// prediction, so a caller measuring how well the positional model works
    /// should read [`SectionPrediction::evidence`] and compute the rate over
    /// [`SectionEvidence::OneSectionRose`] alone.
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
    ///
    /// This must be the recording player's own vehicle. Attribution runs off
    /// the SetFire ribbon log, which the server sends only for the recording
    /// perspective, so any other attacker would take its denominator from one
    /// player's hits and its numerator from another player's fires.
    /// [`analyze`] refuses that combination rather than reporting it.
    pub entity: EntityId,
    pub build: &'a ResolvedBuild,
    /// Projectile GameParams names in the equipped main battery's `ammoList`.
    pub main_battery_ammo: &'a [String],
    /// Projectile GameParams names in the equipped secondary battery's
    /// `ammoList`. Secondary fire arrives on the same `receiveArtilleryShots`
    /// path as the main battery and is only separable by this list, which is
    /// what makes a secondary/main-battery collision detectable at all.
    ///
    /// `Some(&[])` is a ship that carries no secondaries; `None` is a secondary
    /// battery we could not resolve, which disables the contest guard entirely
    /// and would credit secondary-set fires to coincident main-battery hits.
    /// [`analyze`] returns `None` for that rather than reporting a number the
    /// guard never checked.
    pub secondary_ammo: Option<&'a [String]>,
}

/// Whether the victim learned Fire Prevention Expert, which forces
/// `burningFlags` bit 2 off unconditionally.
///
/// Three-valued because `false` would otherwise mean both "did not take it" and
/// "we could not resolve this victim's build", and the second reading admits
/// node-2 hits that provably could not have started a fire. Victim skill data
/// is not reliably resolvable on old replays, so the unresolved case is
/// ordinary rather than exotic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FirePrevention {
    Learned,
    NotLearned,
    Unknown,
}

/// What became of the victim.
///
/// Three-valued for the same reason as [`FirePrevention`]: `Option<GameClock>`
/// would conflate "survived" with "we do not know", and under the second
/// reading post-death hits enter the denominator while the server's
/// death-flare burn transitions stay in the log where a ribbon can match one.
/// That corrupts numerator and denominator together.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VictimFate {
    Survived,
    DiedAt(GameClock),
    Unknown,
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
    pub fire_prevention: FirePrevention,
    pub fate: VictimFate,
    /// This victim's own consumable slots, build modifiers already applied.
    pub consumables: Vec<ConsumableInventory>,
}

/// Everything [`analyze`] reads.
pub struct FireChanceInput<'a> {
    /// Replay version, used to fold the attacker's modifiers.
    pub version: Version,
    pub attacker: AttackerContext<'a>,
    /// The recording player's vehicle entity, i.e. whose ribbons `ribbons`
    /// holds. Carried separately from [`AttackerContext::entity`] so the
    /// self-player invariant is checkable rather than merely documented.
    pub self_entity: EntityId,
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
/// `None` when the eligibility model cannot be built at all: the attacker is
/// not the recording player, whose ribbons are the only fire attribution there
/// is; the secondary battery's `ammoList` is unresolved, which would leave the
/// secondary-contamination guard silently disabled; no victim's hull resolved
/// to fire-section geometry; or the attacker's ship carries no tier and so no
/// burn-chance formula. Every one means unavailable, never zero.
pub fn analyze(input: &FireChanceInput<'_>) -> Option<EffectiveFireChance> {
    if input.attacker.entity != input.self_entity {
        return None;
    }
    let secondary_ammo = input.attacker.secondary_ammo?;

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
            // An unknown fate cannot narrow the transition log, so `VictimTrack`
            // keeps every transition including any death flare. `classify`
            // refuses every hit on such a victim, so nothing reads that log.
            let died_at = match victim.fate {
                VictimFate::DiedAt(clock) => Some(clock),
                VictimFate::Survived | VictimFate::Unknown => None,
            };
            let track = VictimTrack::build(
                *entity,
                input.burn_state_changes,
                activations,
                &victim.consumables,
                input.presence,
                died_at,
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

        // Our own secondaries contest a main-battery hit in the same window and
        // section, so record where they landed even though they are not
        // candidates themselves.
        if matches!(eligibility, HitEligibility::NotMainBattery)
            && is_secondary_shell(input, secondary_ammo, salvo.params_id)
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

    // The contest is decided on the hits alone, before any ribbon is looked at.
    // Deciding it during attribution would drop only the ambiguous hits that
    // started a fire and keep the ambiguous ones that did not, which is
    // outcome-conditioned selection: it would depress the reported chance in
    // proportion to secondary throughput, on exactly the ships the guard exists
    // to protect. A ribbon left with no candidate falls to `unattributed_fires`,
    // which is what a fire we cannot assign to a weapon is.
    let (counted, contested): (Vec<&Candidate<'_>>, Vec<&Candidate<'_>>) =
        candidates.iter().partition(|c| !contested_by_a_secondary(c, &secondaries));
    if !contested.is_empty() {
        *exclusions.entry(ExclusionReason::SecondaryFireAmbiguous).or_insert(0) += contested.len() as u32;
    }

    let attribution = attribute(input, &counted);

    let per_ship = per_ship_breakdown(input, &counted, &attribution.fires_by_victim, formula_applies);
    let expected_fires = formula_applies.then(|| sum_expected(&counted)).flatten();

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

fn is_secondary_shell(input: &FireChanceInput<'_>, secondary_ammo: &[String], shell: GameParamId) -> bool {
    input.params.game_param_by_id(shell).is_some_and(|param| secondary_ammo.iter().any(|name| name == param.name()))
}

/// Whether one of our own secondary shells landed on the same victim and
/// section close enough that a fire there could have been either shell's.
fn contested_by_a_secondary(candidate: &Candidate<'_>, secondaries: &[SecondaryImpact]) -> bool {
    secondaries.iter().any(|s| {
        s.victim == candidate.victim
            && s.section == candidate.section
            && clock_distance(s.clock, candidate.hit.clock) <= CONTEST_WINDOW
    })
}

/// Total expected fires over `candidates`.
///
/// `None` when any one hit's expectation could not be computed: summing the
/// rest would silently treat the unknown as zero and understate the total.
fn sum_expected(candidates: &[&Candidate<'_>]) -> Option<f32> {
    candidates.iter().try_fold(0.0f32, |total, candidate| Some(total + candidate.expected?.get()))
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

    match victim.fate {
        VictimFate::DiedAt(died_at) if hit.clock >= died_at => return HitEligibility::VictimDead,
        // An unresolved fate leaves both "this hit landed after it died" and
        // "one of these transitions is a death flare" open, and a flare left in
        // the log can be matched to a ribbon, so it corrupts the numerator as
        // well as the denominator. Refusing the whole victim is blunt and
        // costs every sample against it, which is why it carries its own tally
        // key: a large count is the signal that the caller should resolve fate
        // rather than a defect in the model.
        VictimFate::Unknown => return HitEligibility::VictimFateUnknown,
        VictimFate::DiedAt(_) | VictimFate::Survived => {}
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

    // `Unknown` refuses alongside `Learned`: a node-2 hit on a victim whose
    // build we could not read might have been impossible, and admitting it is
    // the direction that corrupts.
    let suppressed = matches!(victim.fire_prevention, FirePrevention::Learned | FirePrevention::Unknown);
    if suppressed && section.get() == FIRE_PREVENTION_SUPPRESSED_NODE {
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
    unattributed: u32,
    fires_by_victim: HashMap<EntityId, u32>,
}

/// Match each self `SetFire` ribbon to one eligible hit of ours.
///
/// The ribbon says a fire was ours, not which weapon set it, so it separates us
/// from other attackers. It cannot separate us from our own secondaries; hits
/// a secondary could equally have caused were already removed from `candidates`
/// before this runs, so a ribbon they would have matched falls to
/// `unattributed` here.
fn attribute(input: &FireChanceInput<'_>, candidates: &[&Candidate<'_>]) -> Attribution {
    let mut consumed = vec![false; candidates.len()];
    let mut predictions = Vec::new();
    let mut fires_by_victim: HashMap<EntityId, u32> = HashMap::new();
    let mut unattributed = 0u32;

    for ribbon in input.ribbons.iter().filter(|r| r.ribbon == Ribbon::SetFire) {
        for _ in 0..ribbon.count {
            let pick = candidates
                .iter()
                .enumerate()
                .filter(|(i, _)| !consumed[*i])
                .filter(|(_, c)| within_window(c.hit.clock, ribbon.clock))
                .filter_map(|(i, c)| lit_change(input, c, ribbon.clock).map(|change| (i, *c, change)))
                .min_by(|(_, a, _), (_, b, _)| {
                    clock_distance(a.hit.clock, ribbon.clock).total_cmp(&clock_distance(b.hit.clock, ribbon.clock))
                });

            let Some((index, candidate, change)) = pick else {
                unattributed += 1;
                continue;
            };

            consumed[index] = true;
            *fires_by_victim.entry(candidate.victim).or_insert(0) += 1;
            predictions.push(SectionPrediction {
                predicted: candidate.section,
                actual: nearest_risen(change, candidate.section),
                evidence: if change.newly_lit().count() == 1 {
                    SectionEvidence::OneSectionRose
                } else {
                    SectionEvidence::NearestOfSeveral
                },
            });
        }
    }

    Attribution { predictions, unattributed, fires_by_victim }
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
/// complete, and [`SectionEvidence`] marks which case it was so a consumer can
/// tell the independent pairs from the rest.
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
    // Grouped by ship index rather than entity so two of the same ship on the
    // enemy team read as one row, which is what a per-target-ship breakdown
    // means. `BTreeMap` because the row order must not depend on hash seeds.
    let mut groups: BTreeMap<&str, Vec<&Candidate<'_>>> = BTreeMap::new();
    for candidate in counted {
        let Some(victim) = input.victims.get(&candidate.victim) else { continue };
        groups.entry(victim.ship_index.as_str()).or_default().push(candidate);
    }

    groups
        .into_values()
        .filter_map(|group| {
            let victim = input.victims.get(&group.first()?.victim)?;
            let mut entities: Vec<EntityId> = group.iter().map(|c| c.victim).collect();
            entities.sort_unstable();
            entities.dedup();
            Some(PerShipFireChance {
                victim_ship_index: victim.ship_index.clone(),
                victim_ship_name: victim.ship_name.clone(),
                eligible_hits: group.len() as u32,
                fires: entities.iter().filter_map(|e| fires_by_victim.get(e)).sum(),
                // Same rule as the aggregate: one hit with an uncomputable
                // expectation makes the row's total unknown, not smaller.
                expected_fires: formula_applies.then(|| sum_expected(&group)).flatten(),
            })
        })
        .collect()
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

    /// Four nodes bow to stern, an Iowa-like hull, in meters: the space
    /// `FireSectionGeometry` holds.
    const NODES: [f32; 4] = [93.0, 19.0, -35.0, -99.0];

    /// A hit landing squarely on one section's node, as a world position.
    ///
    /// `NODES` is in meters and a `WorldPos` is in world units, so the
    /// conversion is what puts the impact on the section it names. Placing the
    /// raw meters here instead would push every hit fifteen times too far out
    /// and clamp the whole fixture onto the bow and stern nodes.
    fn impact_on_section(section: u8) -> WorldPos {
        let offset = wowsunpack::game_params::types::Meters::from(NODES[section as usize]).to_world().value();
        WorldPos::new(offset, 0.0, 0.0)
    }

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
        victim_fire_prevention: FirePrevention,
        victim_fate: VictimFate,
        hit_section: u8,
        hit_type: ShellHitType,
        hit_from_atba: bool,
        secondary_hit_section: Option<u8>,
        secondary_hit_offset: f32,
        ribbon_offset: f32,
        uncomputable_node_probability: Option<u8>,
        second_main_hit_section: Option<u8>,
        server_lit_section: u8,
        second_server_lit_section: Option<u8>,
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
        secondary_battery_unresolved: bool,
        attacker_is_not_the_recording_player: bool,
    }

    fn fixture() -> Fixture {
        Fixture {
            main_burn_prob: 0.12,
            victim_node_probability: 0.6,
            victim_burn_prob: 1.0,
            victim_fire_prevention: FirePrevention::NotLearned,
            victim_fate: VictimFate::Survived,
            hit_section: 0,
            hit_type: ShellHitType::Normal,
            hit_from_atba: false,
            secondary_hit_section: None,
            secondary_hit_offset: 0.1,
            ribbon_offset: 0.0,
            uncomputable_node_probability: None,
            second_main_hit_section: None,
            server_lit_section: 0,
            second_server_lit_section: None,
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
            secondary_battery_unresolved: false,
            attacker_is_not_the_recording_player: false,
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
            self.victim_fire_prevention = FirePrevention::Learned;
            self
        }

        fn with_an_unresolved_victim_build(mut self) -> Fixture {
            self.victim_fire_prevention = FirePrevention::Unknown;
            self
        }

        fn with_an_unresolved_victim_fate(mut self) -> Fixture {
            self.victim_fate = VictimFate::Unknown;
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

        fn with_the_secondary_hit_offset_by(mut self, seconds: f32) -> Fixture {
            self.secondary_hit_offset = seconds;
            self
        }

        fn with_the_ribbon_offset_by(mut self, seconds: f32) -> Fixture {
            self.ribbon_offset = seconds;
            self
        }

        /// A node probability that cannot produce a number, so the hit is a
        /// valid trial whose expectation is unknown.
        fn with_an_uncomputable_node_probability(mut self, section: u8) -> Fixture {
            self.uncomputable_node_probability = Some(section);
            self
        }

        /// A second main-battery hit, placed clear of the ribbon window so it
        /// only adds to the denominator.
        fn also_hitting_section(mut self, section: u8) -> Fixture {
            self.second_main_hit_section = Some(section);
            self
        }

        fn server_lights_section(mut self, section: u8) -> Fixture {
            self.server_lit_section = section;
            self
        }

        /// A battleship salvo can land several shells inside one server tick,
        /// so a single transition lighting more than one section is ordinary.
        fn server_also_lights_section(mut self, section: u8) -> Fixture {
            self.second_server_lit_section = Some(section);
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
            self.victim_fate = VictimFate::DiedAt(clock);
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

        fn with_an_unresolved_secondary_battery(mut self) -> Fixture {
            self.secondary_battery_unresolved = true;
            self
        }

        fn attacking_from_another_perspective(mut self) -> Fixture {
            self.attacker_is_not_the_recording_player = true;
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
                    GameClock(HIT_CLOCK.0 + self.secondary_hit_offset),
                    section,
                    ShellHitType::Normal,
                ));
            }
            if let Some(section) = self.second_main_hit_section {
                hits.push(hit(
                    victim_id(),
                    main_shell_id(),
                    GameClock(HIT_CLOCK.0 + 20.0),
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
            let mut lit = 1u16 << self.server_lit_section;
            if let Some(second) = self.second_server_lit_section {
                lit |= 1u16 << second;
            }
            changes.push(BurnStateChange {
                victim: victim_id(),
                clock: HIT_CLOCK,
                previous: self.burn_mask_before,
                current: self.burn_mask_before | lit,
            });

            let mut ribbons = Vec::new();
            if self.ribbons {
                ribbons.push(RibbonEvent {
                    clock: GameClock(HIT_CLOCK.0 + self.ribbon_offset),
                    ribbon: Ribbon::SetFire,
                    count: 1,
                });
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
            presence.windows.insert(victim_id(), windows);
            presence.windows.insert(other_victim_id(), vec![PresenceWindow { entered: GameClock(0.0), left: None }]);
            // Both windows are open, and an open window reaches only as far as
            // the last update received, so the fixture states when that was:
            // well past every clock any case here queries.
            presence.note_seen(victim_id(), GameClock(1000.0));
            presence.note_seen(other_victim_id(), GameClock(1000.0));

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
                    node_probability: node_probability(
                        self.victim_node_probability,
                        self.uncomputable_node_probability,
                    ),
                    burn_prob: self.victim_burn_prob,
                    fire_prevention: self.victim_fire_prevention,
                    fate: self.victim_fate,
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
                        fire_prevention: FirePrevention::NotLearned,
                        fate: VictimFate::Survived,
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
                    entity: if self.attacker_is_not_the_recording_player { other_victim_id() } else { attacker_id() },
                    build,
                    main_battery_ammo: main_ammo,
                    secondary_ammo: if self.secondary_battery_unresolved { None } else { Some(atba_ammo) },
                },
                self_entity: attacker_id(),
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

    /// One probability per fire section, optionally with one section made
    /// uncomputable.
    fn node_probability(probability: f32, uncomputable: Option<u8>) -> Vec<f32> {
        let mut nodes = vec![probability; NODES.len()];
        if let Some(section) = uncomputable {
            nodes[section as usize] = f32::NAN;
        }
        nodes
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
                position: impact_on_section(section),
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
        // The fire happened and was ours; with its only candidate dropped there
        // is nothing left to assign it to.
        assert_eq!(out.unattributed_fires, 1);
    }

    /// The contest is decided on the hits, not on the outcome. Dropping a
    /// contested hit only when a ribbon matched it would remove the ambiguous
    /// successes and keep the ambiguous failures, depressing the reported
    /// chance in proportion to secondary throughput.
    #[test]
    fn a_coincident_secondary_drops_the_hit_even_when_no_fire_started() {
        let out = analyze(&fixture().with_coincident_secondary_hit().without_ribbons().build()).expect("geometry");
        assert_eq!(out.eligible_hits, 0);
        assert_eq!(out.fires, 0);
        assert_eq!(out.exclusions[&ExclusionReason::SecondaryFireAmbiguous], 1);
        assert_eq!(out.unattributed_fires, 0);
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
        assert_eq!(disagree.section_predictions[0].evidence, SectionEvidence::OneSectionRose);
    }

    /// When several sections rise at once, `actual` is chosen as the risen bit
    /// nearest the prediction, so the pair agrees more often than an
    /// independent one would. The pair is still recorded, and marked, because
    /// a consumer measuring the positional model has to be able to drop it:
    /// scoring these alongside the independent pairs inflates the rate.
    #[test]
    fn a_transition_lighting_several_sections_is_marked_as_dependent_evidence() {
        let out = analyze(&fixture().hitting_section(1).server_lights_section(1).server_also_lights_section(3).build())
            .expect("geometry");

        assert_eq!(out.section_predictions.len(), 1);
        assert_eq!(out.section_predictions[0].predicted.get(), 1);
        assert_eq!(out.section_predictions[0].actual.get(), 1, "the risen bit nearest the prediction");
        assert_eq!(out.section_predictions[0].evidence, SectionEvidence::NearestOfSeveral);
    }

    /// The nearest risen bit is not always the predicted one, so a multi-bit
    /// transition can still disagree. This is what bounds how far the bias goes.
    #[test]
    fn several_risen_sections_can_still_miss_the_prediction() {
        let out = analyze(&fixture().hitting_section(0).server_lights_section(2).server_also_lights_section(3).build())
            .expect("geometry");

        assert_eq!(out.section_predictions[0].predicted.get(), 0);
        assert_eq!(out.section_predictions[0].actual.get(), 2);
        assert_eq!(out.section_predictions[0].evidence, SectionEvidence::NearestOfSeveral);
        assert_eq!(out.section_agreement(), Some(0.0));
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

    /// Two of our own hits can each sit inside one ribbon's window while lying
    /// a full window apart on opposite sides of it, so the contest window is
    /// twice the attribution window. Main-battery hit at `c`, ribbon at
    /// `c + 0.5` so the hit is pickable, our own secondary at `c + 0.7` and so
    /// nearer the ribbon than the hit is: at the narrower window the
    /// main-battery hit would be credited a fire the secondary may have set.
    #[test]
    fn a_secondary_contests_across_the_full_hit_to_hit_window() {
        let out = analyze(
            &fixture()
                .with_the_ribbon_offset_by(0.5)
                .with_coincident_secondary_hit()
                .with_the_secondary_hit_offset_by(0.7)
                .build(),
        )
        .expect("geometry");
        assert_eq!(out.eligible_hits, 0);
        assert_eq!(out.fires, 0);
        assert_eq!(out.exclusions[&ExclusionReason::SecondaryFireAmbiguous], 1);
        assert_eq!(out.unattributed_fires, 1);
    }

    /// A secondary beyond the contest window does not reach the hit at all.
    #[test]
    fn a_secondary_beyond_the_contest_window_does_not_contest() {
        let out = analyze(&fixture().with_coincident_secondary_hit().with_the_secondary_hit_offset_by(1.5).build())
            .expect("geometry");
        assert_eq!(out.eligible_hits, 1);
        assert_eq!(out.fires, 1);
    }

    /// One hit whose expectation cannot be computed makes the total unknown,
    /// not smaller. Summing the rest would report a number that silently omits
    /// a trial it counted in the denominator, at both levels.
    #[test]
    fn one_uncomputable_expectation_makes_the_total_unknown() {
        let out = analyze(
            &fixture().hitting_section(0).with_an_uncomputable_node_probability(3).also_hitting_section(3).build(),
        )
        .expect("geometry");
        assert_eq!(out.eligible_hits, 2);
        assert_eq!(out.expected_fires, None);
        assert_eq!(out.per_ship.len(), 1);
        assert_eq!(out.per_ship[0].expected_fires, None);
    }

    /// A victim whose build we could not read might have Fire Prevention
    /// Expert, so a node-2 hit on it might have been impossible. Unknown
    /// refuses alongside Learned; only a build we actually read admits.
    #[test]
    fn an_unresolved_victim_build_refuses_a_node_two_hit() {
        let out = analyze(&fixture().with_an_unresolved_victim_build().hitting_section(2).build()).expect("geometry");
        assert_eq!(out.eligible_hits, 0);
        assert_eq!(out.exclusions[&ExclusionReason::SectionSuppressedByFirePrevention], 1);

        // The suppression is node 2 only; other sections are unaffected by an
        // unresolved build.
        let elsewhere =
            analyze(&fixture().with_an_unresolved_victim_build().hitting_section(0).build()).expect("geometry");
        assert_eq!(elsewhere.eligible_hits, 1);
    }

    /// An unresolved fate leaves both "this hit landed after it died" and "this
    /// transition is a death flare" open, so every hit on that victim is
    /// refused, under its own tally key so the cost is visible.
    #[test]
    fn an_unresolved_victim_fate_refuses_the_hit() {
        let out = analyze(&fixture().with_an_unresolved_victim_fate().build()).expect("geometry");
        assert_eq!(out.eligible_hits, 0);
        assert_eq!(out.fires, 0);
        assert_eq!(out.exclusions[&ExclusionReason::VictimFateUnknown], 1);
    }

    /// Without the secondary battery's ammoList the contest guard cannot run at
    /// all, and a number it never checked is worse than no number.
    #[test]
    fn an_unresolved_secondary_battery_yields_no_result() {
        assert!(analyze(&fixture().with_an_unresolved_secondary_battery().build()).is_none());
    }

    /// Ribbons exist only for the recording perspective, so any other attacker
    /// would take its denominator from one player's hits and its numerator from
    /// another player's fires.
    #[test]
    fn an_attacker_who_is_not_the_recording_player_yields_no_result() {
        assert!(analyze(&fixture().attacking_from_another_perspective().build()).is_none());
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
