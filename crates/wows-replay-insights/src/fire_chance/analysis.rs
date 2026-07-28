//! Effective fire chance: fires started over hits that could have started one.
//!
//! Most HE hits never had a chance: the section was already burning, or Damage
//! Control Party was running. Dividing by every hit buries the signal, so a hit
//! enters the denominator only when every one of those can be ruled out from
//! the replay.
//! Every ambiguity resolves toward refusal: admitting a hit that could not have
//! started a fire silently corrupts the result, while refusing one that could
//! have merely costs a sample.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;

use wowsunpack::data::Version;
use wowsunpack::game_params::ttx::factories::SMALL_PROJECTILE_MAX_DIAMETER_M;
use wowsunpack::game_params::ttx::modifiers::ModifierBundle;
use wowsunpack::game_params::ttx::provenance::Op;
use wowsunpack::game_params::ttx::weapon_tables::calculate_burn_chance;
use wowsunpack::game_params::ttx::weapon_tables::is_small_projectile;
use wowsunpack::game_params::types::CrewSkillType;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_types::CollisionType;
use wowsunpack::game_types::GameParamId;
use wowsunpack::game_types::Ribbon;
use wowsunpack::game_types::ShellHitType;
use wowsunpack::game_types::ShotId;
use wowsunpack::models::fire_nodes::BurnNodeIndex;
use wowsunpack::models::fire_nodes::FireSectionGeometry;
use wowsunpack::recognized::Recognized;

use wows_battle_world::resources::BurnStateChange;
use wows_battle_world::resources::PresenceLog;
use wows_battle_world::resources::RibbonEvent;
use wows_battle_world::resources::SalvoEvent;
use wows_replays::analyzer::battle_controller::state::ActiveConsumable;
use wows_replays::analyzer::battle_controller::state::ConsumableInventory;
use wows_replays::analyzer::battle_controller::state::ResolvedShotHit;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;

use crate::build::ResolvedBuild;
use crate::fire_chance::geometry::section_for_hit;
use crate::fire_chance::victim::DamageControlState;
use crate::fire_chance::victim::VictimTrack;

pub use wows_core::game_types::ExclusionReason;

/// One server tick (`TICKS_PER_SECOND` = 7, `ma779114d`) plus packet jitter. The
/// ribbon and the `burningFlags` update are separate packets from the same tick.
///
/// This is a ribbon-relative radius: it bounds how far *before* its ribbon a
/// hit can sit, and how far either way a `burningFlags` transition can. How far
/// after its ribbon a hit can sit is [`RIBBON_REORDER_TOLERANCE`] instead.
const ATTRIBUTION_WINDOW: f32 = 0.5;

/// How far a hit may sit *after* the ribbon it caused and still be admitted.
///
/// The cause precedes the effect, so a hit later than its ribbon is packet
/// ordering, not physics. Only the jitter that reordering can produce is
/// allowed, well under [`ATTRIBUTION_WINDOW`], so a hit slightly before the
/// ribbon always beats one slightly after it.
const RIBBON_REORDER_TOLERANCE: f32 = 0.15;

/// How far apart two of our own hits can be and still both be plausible causes
/// of one fire.
///
/// This one is hit-relative rather than ribbon-relative: two hits can each sit
/// inside a single ribbon's admission band while lying most of that band apart
/// on opposite sides of the ribbon. It is set at least as wide as the band, so
/// any two hits that could both match one ribbon do contest each other. A
/// narrower window would leave a gap where a main-battery hit is close enough to
/// the ribbon to be credited while a secondary of ours nearer the ribbon still
/// fails the contest, which inflates the numerator on exactly the secondary
/// ships the contest exists to protect.
const CONTEST_WINDOW: f32 = 2.0 * ATTRIBUTION_WINDOW;

/// The fire section Fire Prevention Expert merges away, and the one it merges
/// into.
///
/// The skill does not make a region of the hull unburnable: it reduces four
/// fire zones to three by combining two of them, which is what "reduces the
/// maximum number of fires from 4 to 3" means mechanically. `_createNodes`
/// (`m09838fe6/m0700235d.py:318`) gives node 1 the `fireResistance` effect
/// group in place of its own `fire2` when `fireResistanceEnabled` is set, so
/// node 1 is what renders the combined zone, and `setBurningFlags` (same file,
/// line 346) holds node 2 permanently unlit, which is why node 2 never appears
/// in `burningFlags` on such a victim. A shell landing in node 2's region still
/// rolls for fire; the fire lights node 1.
const FIRE_PREVENTION_MERGED_NODE: u8 = 2;
const FIRE_PREVENTION_MERGE_TARGET: u8 = 1;

/// Both nodes are inside `MAX_NODES`, so placing a merged hit cannot fail on
/// the index. Pinned here rather than handled at the call site, where a
/// fallback would have to invent a section.
const _: () = assert!(
    FIRE_PREVENTION_MERGED_NODE < BurnNodeIndex::MAX_NODES && FIRE_PREVENTION_MERGE_TARGET < BurnNodeIndex::MAX_NODES
);

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
    /// The hit landed in [`FIRE_PREVENTION_MERGED_NODE`] on a victim whose
    /// build could not be resolved, so which section's burn bit the shell rolled
    /// against is undecidable: node 2's if the victim never learned Fire
    /// Prevention Expert, node 1's if it did. The already-burning test has no
    /// section to read, so the hit is refused. Scoped to that one node; a hit
    /// anywhere else on the same victim is unaffected by the gap.
    MergedSectionVictimBuildUnknown,
    DamageControlActive,
    DamageControlUnknown,
    ObservationGap,
    ConsumableModelUnreliable,
    /// The shell landed at or after the moment the victim died. Not a refusal:
    /// there was no live ship to set alight, so the hit was never a trial the
    /// eligibility model turned down.
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
    /// The shell's collision type says it struck terrain, water, a wave or
    /// nothing at all. Such a shell set no ship alight whatever victim the
    /// nearest-ship heuristic keyed it to, and its `shell_hit` is an ordinary
    /// `Normal`, so nothing else in the model would refuse it.
    ImpactNotOnAShip,
    /// The impact lies too far from the victim's burn nodes to have been a hit
    /// on that hull. `victim_entity_id` is a nearest-ship heuristic, so this is
    /// what a hit keyed to a neighbour in a tight formation looks like.
    ImpactOffTheHull,
    /// The victim's position and orientation were not known at the moment of
    /// the hit, so where on the hull the shell landed cannot be established.
    VictimPoseUnknown,
    /// Another shell of ours that cannot be ruled out as the cause landed in the
    /// same section inside the contest window, so a SetFire ribbon there cannot
    /// be assigned to this hit rather than to that one. See
    /// [`contesting_section`] for what qualifies.
    AmbiguousWithAnotherHit,
}

/// Which of the three buckets a classified hit falls in.
///
/// Three rather than two because "refused" and "never in the population" are
/// different claims. A refusal is a hit that might have started a fire and
/// could not be proven either way, and its count is the cost of the model's
/// caution. A hit on a ship that was already dead is neither: there was nothing
/// left to set alight, so nothing about it was ever in question.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitDisposition {
    /// A trial: it enters the denominator.
    Trial,
    Excluded(ExclusionReason),
    NotApplicable,
}

impl HitEligibility {
    pub fn disposition(&self) -> HitDisposition {
        match self {
            HitEligibility::Eligible { .. } => HitDisposition::Trial,
            HitEligibility::VictimDead => HitDisposition::NotApplicable,
            HitEligibility::SectionAlreadyBurning(_) => {
                HitDisposition::Excluded(ExclusionReason::SectionAlreadyBurning)
            }
            HitEligibility::MergedSectionVictimBuildUnknown => {
                HitDisposition::Excluded(ExclusionReason::MergedSectionVictimBuildUnknown)
            }
            HitEligibility::DamageControlActive => HitDisposition::Excluded(ExclusionReason::DamageControlActive),
            HitEligibility::DamageControlUnknown => HitDisposition::Excluded(ExclusionReason::DamageControlUnknown),
            HitEligibility::ObservationGap => HitDisposition::Excluded(ExclusionReason::ObservationGap),
            HitEligibility::ConsumableModelUnreliable => {
                HitDisposition::Excluded(ExclusionReason::ConsumableModelUnreliable)
            }
            HitEligibility::VictimFateUnknown => HitDisposition::Excluded(ExclusionReason::VictimFateUnknown),
            HitEligibility::ShellCannotBurn => HitDisposition::Excluded(ExclusionReason::ShellCannotBurn),
            HitEligibility::NotMainBattery => HitDisposition::Excluded(ExclusionReason::NotMainBattery),
            HitEligibility::HitTypeDoesNotRoll(_) => HitDisposition::Excluded(ExclusionReason::HitTypeDoesNotRoll),
            HitEligibility::NoSectionGeometry => HitDisposition::Excluded(ExclusionReason::NoSectionGeometry),
            HitEligibility::ImpactNotOnAShip => HitDisposition::Excluded(ExclusionReason::ImpactNotOnAShip),
            HitEligibility::ImpactOffTheHull => HitDisposition::Excluded(ExclusionReason::ImpactOffTheHull),
            HitEligibility::VictimPoseUnknown => HitDisposition::Excluded(ExclusionReason::VictimPoseUnknown),
            HitEligibility::AmbiguousWithAnotherHit => {
                HitDisposition::Excluded(ExclusionReason::AmbiguousWithAnotherHit)
            }
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
    /// Every shell of ours the model classified against this ship: the trials,
    /// the refusals and the hits that were never in the population. Equal to
    /// `eligible_hits + exclusions.values().sum() + not_applicable`.
    ///
    /// There is no per-ship counterpart to
    /// [`EffectiveFireChance::he_shells_fired`]. A salvo is fired at the water,
    /// not at a victim, and its shells can land on several ships, so splitting
    /// a fired count between them would either double-count the salvo or invent
    /// an assignment the replay does not carry.
    pub shells_on_target: u32,
    pub eligible_hits: u32,
    /// Why the refused shells against this ship were refused, the per-ship form
    /// of [`EffectiveFireChance::exclusions`].
    pub exclusions: BTreeMap<ExclusionReason, u32>,
    /// Shells that landed on this ship at or after it died.
    pub not_applicable: u32,
    pub fires: u32,
    /// Expected number of fires over this ship's eligible hits, not a rate.
    /// Counted the way [`Self::fires`] is, saturating at one fire per section
    /// per server tick, so the two are the same quantity; see
    /// `expected_observable_fires`. [`Self::expected_rate`] is the rate the
    /// observed one is comparable against.
    pub expected_fires: Option<f32>,
}

impl PerShipFireChance {
    /// Fires started per eligible hit against this ship. `None` when there are
    /// no eligible hits against it: a rate over zero samples is not zero, it is
    /// unknown.
    ///
    /// This is the only level a rate is reported at, because fire resistance is
    /// a property of the victim: each ship carries its own `burnProb`
    /// coefficient and its own per-node probabilities, so a figure pooled over
    /// several victims describes the enemy composition as much as the shooter.
    ///
    /// It reads low against the true per-shell chance, and how low depends on
    /// the shooter. The game rolls for fire per shell, independently, so every
    /// shell that lands on an unburning section is a genuine trial and the
    /// denominator counts them all. The numerator cannot keep up: a section can
    /// only burn once, so when several shells reach one section in the same
    /// server tick and more than one of them rolls a fire, exactly one fire is
    /// ever observed. The shortfall grows with how many shells land on a single
    /// section at a time, so a ship landing dense salvos on one section is
    /// undercounted further than one whose hits spread across the hull. Read
    /// this as a lower bound on the per-shell chance rather than an unbiased
    /// estimate of it.
    pub fn rate(&self) -> Option<f32> {
        (self.eligible_hits > 0).then(|| self.fires as f32 / self.eligible_hits as f32)
    }

    /// Expected fires per eligible hit against this ship, the model's
    /// counterpart to [`Self::rate`]. Same zero-sample rule, and `None` as well
    /// whenever [`Self::expected_fires`] is unknown.
    ///
    /// Its numerator saturates exactly as the observed one does, so the two
    /// rates describe the same event and their ratio is the model's error
    /// rather than an artifact of how densely the shells landed.
    pub fn expected_rate(&self) -> Option<f32> {
        (self.eligible_hits > 0).then(|| Some(self.expected_fires? / self.eligible_hits as f32)).flatten()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EffectiveFireChance {
    /// How many fire-capable main-battery shells we fired.
    ///
    /// Taken from [`FireChanceInput::salvos`], which the ingest records when a
    /// salvo is fired, so a salvo that landed nothing counts exactly as one
    /// that landed everything. It is not derived from the hits: a count taken
    /// from `ResolvedShotHit`'s originating salvo can only ever see salvos that
    /// produced an impact the client was told about, which flatters the shooter
    /// by dropping every miss.
    ///
    /// Counted per salvo at its full width, and gated on the same "could this
    /// shell start a fire" test `classify` applies, so AP, SAP and secondary
    /// shells are out. It is a whole-match attacker-side figure with no
    /// per-ship counterpart; see [`PerShipFireChance::shells_on_target`].
    ///
    /// Zero when the parse ran without `BattleWorld::set_record_salvo_history`,
    /// which is indistinguishable here from a match in which nothing was fired.
    pub he_shells_fired: u32,
    /// Every shell of ours the eligibility model classified: the trials, the
    /// refusals and the hits that were never in the population. Equal to
    /// `eligible_hits + exclusions.values().sum() + not_applicable`, and kept
    /// as a field so a caller cannot reconstruct the total from a subset of the
    /// buckets.
    ///
    /// Every shell, not only the HE ones, which is what makes the partition
    /// hold: a shell that could not have burned is refused as `ShellCannotBurn`
    /// or `NotMainBattery` rather than kept out of the total, so the breakdown
    /// says how many of our hits were not HE instead of quietly dropping them.
    /// It is therefore not a subset of [`Self::he_shells_fired`], which is
    /// HE-only.
    ///
    /// The sum over [`Self::per_ship`] can be smaller: a hit keyed to a ship
    /// whose build never resolved has no row to sit in, and lands only here.
    pub shells_on_target: u32,
    /// Shells that landed on a ship at or after it died. Reported apart from
    /// [`Self::exclusions`] because it is not a refusal: there was no live ship
    /// to set alight, so the eligibility model was never asked anything.
    pub not_applicable: u32,
    /// Trials summed over every target ship. A count, and deliberately not
    /// half of a rate: dividing it into [`Self::fires`] would pool victims of
    /// different fire resistance into one figure, which
    /// [`PerShipFireChance::rate`] is the level that avoids.
    pub eligible_hits: u32,
    /// Attributed fires summed over every target ship, the counterpart count to
    /// [`Self::eligible_hits`].
    pub fires: u32,
    /// Expected number of fires over the eligible hits, not a rate. Comparable
    /// against [`Self::fires`] because it is the same quantity: both saturate
    /// at one fire per section per server tick, which is all a replay can show.
    /// See `expected_observable_fires`.
    ///
    /// `None` on builds predating the modern modifier names, where the
    /// attacker formula does not apply. The observed counts are still valid.
    pub expected_fires: Option<f32>,
    pub per_ship: Vec<PerShipFireChance>,
    pub exclusions: BTreeMap<ExclusionReason, u32>,
    /// Predicted-vs-actual section for every attributed fire. This is the
    /// evidence for the nearest-node assumption;
    /// [`Self::independent_section_agreement`] is its ratio over the pairs that
    /// can carry it. Stored as pairs so a corpus test can build a confusion
    /// matrix.
    pub section_predictions: Vec<SectionPrediction>,
    /// Fires we could not assign to a specific hit: a SetFire ribbon that
    /// matched no eligible hit inside the attribution window.
    ///
    /// A healthy replay carries plenty of these, so a nonzero count is not by
    /// itself a fault. The legitimate causes are fires set by our own
    /// secondaries with no main-battery candidate nearby, fires on a victim
    /// outside the recording client's AOI, fires whose causing hit was excluded
    /// by any of the eligibility rules, and fires whose only candidates were
    /// dropped as ambiguous with another hit of ours. What the number is for is
    /// proportion: an
    /// unattributed count far larger than `fires` on a ship with no secondaries
    /// means something upstream is wrong.
    pub unattributed_fires: u32,
    /// The shell's raw `burnProb` before any modifier step in [`Self::formula`]
    /// is applied, for the hover breakdown's base line. `None` when no shell
    /// resolved to compute it from (no eligible hit, or the projectile carries
    /// no `burnProb`), and when the attacker's modifier bundle could not be
    /// folded for this game version, which is the same condition that makes
    /// [`Self::expected_fires`] `None`: the steps would then come from the stock
    /// table rather than from this build. A resolved shell whose modifiers are
    /// all identities still yields `Some`, so `formula` itself may be empty
    /// while this is not.
    pub formula_base: Option<f32>,
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
    /// How many fire sections the victim's hull has. A one-section hull can
    /// only ever predict section 0 and can only ever have lit section 0, so
    /// such a pair agrees by construction and says nothing about the positional
    /// model. [`Self::is_independent_evidence`] is what drops those.
    pub hull_sections: usize,
}

impl SectionPrediction {
    /// Whether this pair can be scored as evidence for the nearest-node
    /// assumption: the matched transition lit exactly one section, so `actual`
    /// was not chosen with the prediction in hand, and the hull has more than
    /// one section, so agreeing was not automatic.
    pub fn is_independent_evidence(&self) -> bool {
        self.evidence == SectionEvidence::OneSectionRose && self.hull_sections > 1
    }
}

impl EffectiveFireChance {
    /// How many target ships carry at least one eligible hit, i.e. how many
    /// [`Self::per_ship`] rows can state a rate. Reported alongside the totals
    /// because those totals are a sum over this many separate populations.
    pub fn ships_with_trials(&self) -> usize {
        self.per_ship.iter().filter(|ship| ship.eligible_hits > 0).count()
    }

    /// The attacker-side formula's total as `(raw, clamped)`: the value the
    /// steps in [`Self::formula`] arrive at, and the value the eligibility
    /// model actually rolls with.
    ///
    /// A raw total can run past 100%, since additive bonuses have no ceiling on
    /// paper, but `classify` clamps into 0..=1 before gating a hit or computing
    /// its expectation. Both are returned so a breakdown can show the clamp
    /// where it bites instead of restating the rule itself. `None` when no
    /// shell resolved to compute a formula from, which is the same condition as
    /// [`Self::formula_base`] being `None`.
    pub fn formula_total(&self) -> Option<(f32, f32)> {
        let base = self.formula_base?;
        let raw = self.formula.last().map_or(base, |step| step.result);
        Some((raw, raw.clamp(0.0, 1.0)))
    }

    /// Fraction of attributed fires whose predicted section is the bit the
    /// server lit, over the pairs that are evidence for the positional model.
    /// `None` when there are none.
    ///
    /// Only the independent pairs are scored, because the rest agree for
    /// reasons that have nothing to do with the prediction: a transition
    /// lighting several sections at once has its `actual` chosen as the risen
    /// bit nearest the prediction, and a one-section hull agrees whatever was
    /// predicted. A rate taken over every pair is biased upward by both, which
    /// is why this is the only rate exposed. [`Self::section_predictions`]
    /// carries the raw pairs for a caller that wants to break them down
    /// differently.
    pub fn independent_section_agreement(&self) -> Option<f32> {
        let scored: Vec<&SectionPrediction> =
            self.section_predictions.iter().filter(|p| p.is_independent_evidence()).collect();
        if scored.is_empty() {
            return None;
        }
        let agreed = scored.iter().filter(|p| p.predicted == p.actual).count();
        Some(agreed as f32 / scored.len() as f32)
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

impl VictimContext {
    /// How many fire sections this hull has. The hull's `burnNodes` list is
    /// what defines that count, and `node_probability` is that list, so a
    /// caller resolving geometry asks here rather than restating which field
    /// carries the count.
    pub fn hull_section_count(&self) -> usize {
        self.node_probability.len()
    }
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
    /// Every salvo fired in the match (`BattleReport::salvos`), the fired side
    /// of the shell accounting. Recorded at fire time, so it holds the salvos
    /// that missed as well as the ones that landed, which is what makes
    /// [`EffectiveFireChance::he_shells_fired`] a real count rather than a
    /// lower bound taken off the hits.
    pub salvos: &'a [SalvoEvent],
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
    /// Index of this hit's [`ClassifiedHit`], so losing the contest can move it
    /// from trial to exclusion in the one place the counts are taken from.
    record: usize,
}

/// One of our shells that landed, and where it ended up in the accounting.
///
/// The tallies are all taken from this list rather than accumulated as the
/// classification runs, because the contest guard revises a hit's disposition
/// after every hit has been classified.
struct ClassifiedHit {
    victim: EntityId,
    disposition: HitDisposition,
}

/// Where one of our own shells landed that is not a trial itself but cannot be
/// ruled out as the cause of a fire in its section.
struct ContestingImpact {
    victim: EntityId,
    section: BurnNodeIndex,
    clock: GameClock,
}

/// Why a hit could not be placed in one of the victim's fire sections.
enum SectionGap {
    /// The victim has no resolvable fire-section geometry, or a section the
    /// geometry names that the hull's own `burnNodes` list does not.
    NoGeometry,
    /// The impact is too far from the hull's nodes to have been a hit on it.
    OffTheHull,
    /// The victim's position and orientation were not known at the moment of
    /// the hit, so the impact cannot be placed against the hull at all.
    NoVictimPose,
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
            let track =
                VictimTrack::build(*entity, input.burn_state_changes, activations, &victim.consumables, died_at);
            (*entity, track)
        })
        .collect();

    let mut candidates: Vec<Candidate<'_>> = Vec::new();
    let mut contenders: Vec<ContestingImpact> = Vec::new();
    let mut classified: Vec<ClassifiedHit> = Vec::new();
    let he_shells_fired = he_shells_fired(input, &bundle, tier);

    for hit in input.hits {
        // `salvo` is `Some` only for a hit matched to its originating salvo.
        // Without it the shell is unidentifiable and `victim_entity_id` falls
        // back to the self ship, so an unmatched hit is not a candidate at all.
        let Some(salvo) = hit.salvo.as_ref() else { continue };
        if salvo.owner_id != input.attacker.entity {
            continue;
        }

        let eligibility = classify(input, &geometry, &tracks, &bundle, tier, formula_applies, hit, salvo.params_id);

        for section in contesting_sections(input, &geometry, secondary_ammo, hit, salvo.params_id, &eligibility) {
            contenders.push(ContestingImpact { victim: hit.victim_entity_id, section, clock: hit.clock });
        }

        let record = classified.len();
        classified.push(ClassifiedHit { victim: hit.victim_entity_id, disposition: eligibility.disposition() });
        if let HitEligibility::Eligible { section, expected } = eligibility {
            candidates.push(Candidate {
                hit,
                victim: hit.victim_entity_id,
                section,
                expected,
                shell: salvo.params_id,
                record,
            });
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
        candidates.iter().partition(|c| !contested_by_another_hit(c, &contenders));
    for candidate in &contested {
        classified[candidate.record].disposition = HitDisposition::Excluded(ExclusionReason::AmbiguousWithAnotherHit);
    }

    let mut exclusions: BTreeMap<ExclusionReason, u32> = BTreeMap::new();
    let mut not_applicable = 0u32;
    for record in &classified {
        match record.disposition {
            HitDisposition::Trial => {}
            HitDisposition::Excluded(reason) => *exclusions.entry(reason).or_insert(0) += 1,
            HitDisposition::NotApplicable => not_applicable += 1,
        }
    }

    let attribution = attribute(input, &counted);

    let per_ship = per_ship_breakdown(input, &classified, &counted, &attribution.fires_by_victim, formula_applies);
    let expected_fires = formula_applies.then(|| expected_observable_fires(&counted)).flatten();
    // Gated on the same fact as `expected_fires`. Without it the breakdown would
    // be rendered from `ModifierBundle::empty`, showing a player running IFHE,
    // Demolition Expert and Victor Lima the stock table's fire chance labelled
    // as their build's.
    let (formula_base, formula) =
        if formula_applies { formula_steps(input, &bundle, tier, &counted) } else { (None, Vec::new()) };

    Some(EffectiveFireChance {
        he_shells_fired,
        shells_on_target: classified.len() as u32,
        not_applicable,
        eligible_hits: counted.len() as u32,
        fires: attribution.predictions.len() as u32,
        expected_fires,
        per_ship,
        exclusions,
        section_predictions: attribution.predictions,
        unattributed_fires: attribution.unattributed,
        formula_base,
        formula,
    })
}

/// Fire-capable main-battery shells the attacker fired over the whole match.
///
/// Reads the fired-side log rather than the hits, so a salvo that landed
/// nothing is counted; the gate is the same `burnable_main_battery_chance` the
/// hit path applies, so AP, SAP and secondary shells are out.
///
/// Keyed on the salvo id together with the shell and the salvo's first shot id,
/// because a salvo id names none of them on its own. One trigger pull reaches
/// the client as several `SHOTS_PACK` entries sharing a salvo id (over the
/// corpus, 17440 salvo records carry only 11360 distinct salvo ids), and the
/// game creates a shell for every shot in every one of them, so a key of just
/// the salvo id would throw away all but one record of each pull. Shot ids come
/// from a per-owner counter, so the first one separates those records; the
/// shell is in the key as well since nothing establishes that a salvo id is
/// unique across the batteries that raise them.
///
/// What the key still collapses is the same record arriving twice, which is
/// what a merged multi-perspective session produces: both clients are told the
/// same server-assigned shot ids, so the duplicate lands on the same key
/// instead of multiplying the count by the number of replays merged.
/// `BTreeMap` so the sum does not depend on hash seeds.
fn he_shells_fired(input: &FireChanceInput<'_>, bundle: &ModifierBundle, tier: u32) -> u32 {
    let mut shells_per_salvo: BTreeMap<(u32, u64, Option<u32>), u32> = BTreeMap::new();
    for salvo in input.salvos {
        if salvo.owner_id != input.attacker.entity {
            continue;
        }
        if burnable_main_battery_chance(input, bundle, tier, salvo.params_id).is_err() {
            continue;
        }
        let key = (salvo.salvo_id, salvo.params_id.raw(), salvo.first_shot.map(ShotId::raw));
        shells_per_salvo.insert(key, salvo.shots);
    }
    shells_per_salvo.values().sum()
}

/// Which fire section a hit landed in, when the victim's hull has geometry and
/// the impact is close enough to it to have been a hit on that hull.
fn section_of(
    input: &FireChanceInput<'_>,
    geometry: &HashMap<EntityId, FireSectionGeometry>,
    hit: &ResolvedShotHit,
) -> Result<BurnNodeIndex, SectionGap> {
    let geom = geometry.get(&hit.victim_entity_id).ok_or(SectionGap::NoGeometry)?;
    let pose = hit.victim_pose.as_ref().ok_or(SectionGap::NoVictimPose)?;
    let section = section_for_hit(geom, hit.hit.position, pose.position, pose.yaw, pose.pitch, pose.roll)
        .ok_or(SectionGap::OffTheHull)?;
    // The hull's `burnNodes` list is what makes a section's probability
    // readable; a geometry longer than it would index past the hull's own data.
    let victim = input.victims.get(&hit.victim_entity_id).ok_or(SectionGap::NoGeometry)?;
    if usize::from(section.get()) >= victim.node_probability.len() {
        return Err(SectionGap::NoGeometry);
    }
    Ok(section)
}

fn is_secondary_shell(input: &FireChanceInput<'_>, secondary_ammo: &[String], shell: GameParamId) -> bool {
    input.params.game_param_by_id(shell).is_some_and(|param| secondary_ammo.iter().any(|name| name == param.name()))
}

/// Where a hit that is not a trial still contests the ribbons near it, if it
/// does.
///
/// A hit contests when nothing establishes that it could not have started the
/// fire. Our own secondaries are the obvious case: they set fires and the ribbon
/// does not name the weapon. The rest are hits the eligibility model refused for
/// want of proof rather than by proof. [`rolls_for_fire`] excludes ricochets,
/// overpenetrations, underwater hits and ids this build cannot name on the
/// grounds that nothing establishes they roll, which is an unknown; the same
/// goes for a node-2 hit refused because the victim's Fire Prevention Expert
/// could not be read. Leaving those out of the contest lets a fire one of them
/// set be credited to a coincident eligible hit, which adds to the numerator
/// with no matching trial in the denominator.
///
/// `ShellCannotBurn` and `SectionAlreadyBurning` are proofs, so they do not
/// contest.
///
/// A hit refused as [`HitEligibility::MergedSectionVictimBuildUnknown`] returns
/// two sections rather than one. Which bit it could have lit is precisely what
/// the unresolved build leaves open: the node the shell landed in if the victim
/// never learned Fire Prevention Expert, the node that one merges into if it
/// did. Returning only the first would leave a candidate in the merge target
/// free to be credited a fire this shell may have set.
fn contesting_sections(
    input: &FireChanceInput<'_>,
    geometry: &HashMap<EntityId, FireSectionGeometry>,
    secondary_ammo: &[String],
    hit: &ResolvedShotHit,
    shell: GameParamId,
    eligibility: &HitEligibility,
) -> Vec<BurnNodeIndex> {
    // A shell that struck terrain or water is a proof like the others, and it
    // reaches here through the secondary arm: `classify` names a secondary
    // `NotMainBattery` before it ever reads the collision type. Without this a
    // secondary of ours that hit an island beside the victim would contest a
    // main-battery trial it could not possibly have contested.
    if struck_no_ship(&hit.hit.hit_type.collision) {
        return Vec::new();
    }
    let contests = match eligibility {
        HitEligibility::NotMainBattery => is_secondary_shell(input, secondary_ammo, shell),
        HitEligibility::HitTypeDoesNotRoll(_) => true,
        HitEligibility::MergedSectionVictimBuildUnknown => true,
        _ => false,
    };
    if !contests {
        return Vec::new();
    }
    let Ok(section) = section_of(input, geometry, hit) else { return Vec::new() };
    let mut sections = vec![section];
    if matches!(eligibility, HitEligibility::MergedSectionVictimBuildUnknown) {
        sections.push(merged_section());
    }
    sections
}

/// The fire section a hit in [`FIRE_PREVENTION_MERGED_NODE`] rolls against on a
/// victim that learned Fire Prevention Expert. Infallible by the const
/// assertion beside the constants.
fn merged_section() -> BurnNodeIndex {
    BurnNodeIndex::new(FIRE_PREVENTION_MERGE_TARGET).expect("the merge target is inside MAX_NODES")
}

/// Whether one of our own non-trial hits landed on the same victim and section
/// close enough that a fire there could have been either shell's.
fn contested_by_another_hit(candidate: &Candidate<'_>, contenders: &[ContestingImpact]) -> bool {
    contenders.iter().any(|other| {
        other.victim == candidate.victim
            && other.section == candidate.section
            && clock_distance(other.clock, candidate.hit.clock) <= CONTEST_WINDOW
    })
}

/// The key a set of shells has to share before they contest one section's one
/// fire: one victim, one fire section, one server tick.
///
/// The tick is keyed on the clock's exact bits because every hit in one
/// `ShotKills` packet carries that packet's clock unchanged, so two hits from
/// the same tick are bit-identical and two from different ticks are not.
type FireGroup = (EntityId, u8, u32);

fn fire_group(candidate: &Candidate<'_>) -> FireGroup {
    (candidate.victim, candidate.section.get(), candidate.hit.clock.seconds().to_bits())
}

/// Expected number of *observable* fires over `candidates`, saturating at one
/// per section per tick.
///
/// That saturation is what makes this comparable against the observed count.
/// The game rolls for fire per shell and independently, but a section can only
/// be burning once, so several shells reaching one section in one server tick
/// produce at most one fire between them however many of them rolled one. The
/// observed side counts fires, which means it counts that one; a prediction of
/// how many rolls succeeded would be a quantity the replay can never show.
///
/// Candidates are therefore grouped by victim, section and tick, and a group
/// expects one minus the product of `1 - p` over its members, which is the
/// chance at least one of its shells rolled a fire. The per-hit chances are
/// multiplied out rather than assumed equal, since a group can mix shells. A
/// group of one reduces to that hit's own chance, so where hits share no
/// section and tick this is exactly the sum of the per-hit chances.
///
/// `None` when any one hit's expectation could not be computed: that leaves its
/// group unknown and the total with it, where folding the rest in would
/// silently treat the unknown as zero and understate both.
fn expected_observable_fires(candidates: &[&Candidate<'_>]) -> Option<f32> {
    // `BTreeMap` so the summation order does not depend on hash seeds. The
    // running value is the chance the group has produced a fire so far, grown
    // one shell at a time; a group of one leaves it at that shell's chance
    // untouched by any arithmetic.
    let mut groups: BTreeMap<FireGroup, Option<f32>> = BTreeMap::new();
    for candidate in candidates {
        let group = groups.entry(fire_group(candidate)).or_insert(Some(0.0));
        *group = group.zip(candidate.expected).map(|(lit, chance)| {
            let chance = chance.get();
            lit + chance - lit * chance
        });
    }
    groups.into_values().try_fold(0.0f32, |total, group| Some(total + group?))
}

/// The clamped chance one of our main-battery shells rolls a fire with, or the
/// reason it could not have started one wherever it landed.
///
/// Split out of [`classify`] because the fired-shell count asks the same
/// question of a salvo with no hit in hand: what makes a shell an HE shell here
/// is that its folded burn chance is positive, never an ammo-type string, and
/// the two counts have to agree on that or the breakdown's denominator and its
/// headline would describe different populations.
fn burnable_main_battery_chance(
    input: &FireChanceInput<'_>,
    bundle: &ModifierBundle,
    tier: u32,
    shell: GameParamId,
) -> Result<f32, HitEligibility> {
    let Some(param) = input.params.game_param_by_id(shell) else {
        return Err(HitEligibility::ShellCannotBurn);
    };
    if !input.attacker.main_battery_ammo.iter().any(|name| name == param.name()) {
        return Err(HitEligibility::NotMainBattery);
    }
    let Some(projectile) = param.projectile() else {
        return Err(HitEligibility::ShellCannotBurn);
    };
    // A projectile with no `burnProb` at all carries no fire chance to gate on.
    let Some(burn_prob) = projectile.burn_prob() else {
        return Err(HitEligibility::ShellCannotBurn);
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
        return Err(HitEligibility::ShellCannotBurn);
    }
    Ok(chance)
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
    let chance = match burnable_main_battery_chance(input, bundle, tier, shell) {
        Ok(chance) => chance,
        Err(refusal) => return refusal,
    };

    // Ahead of the hit-type check because it is the more specific fact: a shell
    // that struck an island carries `SHELL_HIT_TYPE_NORMAL`, so `rolls_for_fire`
    // admits it and only the geometry gate stands between it and the
    // denominator. That gate exists to catch a mis-keyed victim, not to decide
    // what the shell hit, and it lets some of these through onto a hull.
    if struck_no_ship(&hit.hit.hit_type.collision) {
        return HitEligibility::ImpactNotOnAShip;
    }

    if !rolls_for_fire(&hit.hit.hit_type.shell_hit) {
        return HitEligibility::HitTypeDoesNotRoll(hit.hit.hit_type.shell_hit.clone());
    }

    // A victim with no context has no hull, so it has no geometry either.
    let section = match section_of(input, geometry, hit) {
        Ok(section) => section,
        Err(SectionGap::NoGeometry) => return HitEligibility::NoSectionGeometry,
        Err(SectionGap::OffTheHull) => return HitEligibility::ImpactOffTheHull,
        Err(SectionGap::NoVictimPose) => return HitEligibility::VictimPoseUnknown,
    };
    let Some(victim) = input.victims.get(&hit.victim_entity_id) else {
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
    match track.damage_control_at(input.presence, hit.clock) {
        DamageControlState::Running => return HitEligibility::DamageControlActive,
        DamageControlState::Unknown => return HitEligibility::DamageControlUnknown,
        DamageControlState::Down => {}
    }

    // Fire Prevention Expert merges the hull's third fire zone into the second,
    // so a shell landing in the merged node is an ordinary trial that rolls
    // against the merge target's bit. Applied before the burn-mask read below
    // and before the expectation is computed, which are the two places the
    // section index is what the answer depends on.
    //
    // With the victim's build unread there is no telling which of the two bits
    // the shell rolled against, so the burn-mask read has no section and the hit
    // is refused. Only this node is affected; every other section reads the same
    // bit either way.
    let section = if section.get() == FIRE_PREVENTION_MERGED_NODE {
        match victim.fire_prevention {
            FirePrevention::Learned => merged_section(),
            FirePrevention::Unknown => return HitEligibility::MergedSectionVictimBuildUnknown,
            FirePrevention::NotLearned => section,
        }
    } else {
        section
    };

    if track.burn_mask_before(hit.clock) & section.bit_mask() != 0 {
        return HitEligibility::SectionAlreadyBurning(section);
    }

    // Node probability is indexed by section; `section_of` has already checked
    // the section is inside the hull's own `burnNodes` list, and the merge
    // target is below the merged node so it is inside it too. Every hull in the
    // roster carries one probability across all of its burn nodes, so the merge
    // does not move this product; it is read at the merged index anyway, since
    // nothing guarantees that stays true. The product is
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

/// Whether the collision type positively says the shell struck something that is
/// not a ship.
///
/// Only a recognized non-ship collision refuses. A collision id this build's
/// constants table does not name leaves the question open, and reading every
/// unnamed id as "struck no ship" would refuse every hit in a replay whose build
/// carries no constants table at all, which is a whole population rather than a
/// sample here and there. The hits an unknown id lets through are the ones the
/// geometry gate was already the only guard against, so the direction is no
/// worse than before for them and strictly better everywhere the id is named.
fn struck_no_ship(collision: &Recognized<CollisionType>) -> bool {
    matches!(
        collision.known(),
        Some(CollisionType::NoHit | CollisionType::HitWater | CollisionType::HitGround | CollisionType::HitWave)
    )
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
    // Indices into `input.burn_state_changes` already claimed by an earlier
    // ribbon. One transition is one fire starting, so two ribbons reading the
    // same change would give one of them an `actual` section that belongs to
    // the other fire while still labelling the pair independent evidence.
    let mut claimed_changes: Vec<bool> = vec![false; input.burn_state_changes.len()];
    let mut predictions = Vec::new();
    let mut fires_by_victim: HashMap<EntityId, u32> = HashMap::new();
    let mut unattributed = 0u32;

    for ribbon in input.ribbons.iter().filter(|r| r.ribbon == Ribbon::SetFire) {
        for _ in 0..ribbon.count {
            let pick = candidates
                .iter()
                .enumerate()
                .filter(|(i, _)| !consumed[*i])
                .filter(|(_, c)| hit_could_have_caused(c.hit.clock, ribbon.clock))
                .filter_map(|(i, c)| lit_change(input, c, ribbon.clock, &claimed_changes).map(|change| (i, *c, change)))
                .min_by(|(_, a, _), (_, b, _)| {
                    clock_distance(a.hit.clock, ribbon.clock).total_cmp(&clock_distance(b.hit.clock, ribbon.clock))
                });

            let Some((index, candidate, change_index)) = pick else {
                unattributed += 1;
                continue;
            };

            consumed[index] = true;
            claimed_changes[change_index] = true;
            let change = &input.burn_state_changes[change_index];
            *fires_by_victim.entry(candidate.victim).or_insert(0) += 1;
            predictions.push(SectionPrediction {
                predicted: candidate.section,
                actual: nearest_risen(change, candidate.section),
                evidence: if change.newly_lit().count() == 1 {
                    SectionEvidence::OneSectionRose
                } else {
                    SectionEvidence::NearestOfSeveral
                },
                hull_sections: input.victims.get(&candidate.victim).map(|v| v.node_probability.len()).unwrap_or(0),
            });
        }
    }

    Attribution { predictions, unattributed, fires_by_victim }
}

fn clock_distance(a: GameClock, b: GameClock) -> f32 {
    (a.seconds() - b.seconds()).abs()
}

/// Whether a hit at `clock` is close enough to a ribbon to have caused it.
///
/// Asymmetric: the causing hit precedes the ribbon, so the band reaches
/// [`ATTRIBUTION_WINDOW`] back and only [`RIBBON_REORDER_TOLERANCE`] forward.
/// A symmetric band would let a hit just after the ribbon outrank one further
/// before it, which reverses cause and effect.
fn hit_could_have_caused(clock: GameClock, ribbon: GameClock) -> bool {
    let lead = ribbon.seconds() - clock.seconds();
    (-RIBBON_REORDER_TOLERANCE..=ATTRIBUTION_WINDOW).contains(&lead)
}

/// Whether a burn transition and a ribbon are close enough to be the same
/// event. Symmetric, unlike [`hit_could_have_caused`]: both are effects of the
/// same server tick arriving in separate packets, with no causal order between
/// them.
fn within_window(clock: GameClock, ribbon: GameClock) -> bool {
    clock_distance(clock, ribbon) <= ATTRIBUTION_WINDOW
}

/// Index of the unclaimed burn-state change nearest the ribbon that lit at least
/// one section on this candidate's victim. Any newly lit bit qualifies, not only
/// the predicted one: requiring the prediction to match would make
/// [`EffectiveFireChance::independent_section_agreement`] measure nothing.
///
/// Transitions at or after the victim's death are skipped for the same reason
/// [`VictimTrack`] drops them: `onEntityDeath` lights nodes for effect, and a
/// ribbon matching one of those would credit a fire nothing set.
fn lit_change(
    input: &FireChanceInput<'_>,
    candidate: &Candidate<'_>,
    ribbon: GameClock,
    claimed: &[bool],
) -> Option<usize> {
    let died_at = match input.victims.get(&candidate.victim).map(|victim| victim.fate) {
        Some(VictimFate::DiedAt(clock)) => Some(clock),
        _ => None,
    };
    input
        .burn_state_changes
        .iter()
        .enumerate()
        .filter(|(index, _)| !claimed[*index])
        .filter(|(_, c)| c.victim == candidate.victim && within_window(c.clock, ribbon))
        .filter(|(_, c)| died_at.is_none_or(|died_at| c.clock < died_at))
        .filter(|(_, c)| c.newly_lit().next().is_some())
        .min_by(|(_, a), (_, b)| clock_distance(a.clock, ribbon).total_cmp(&clock_distance(b.clock, ribbon)))
        .map(|(index, _)| index)
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

/// One target ship's tally, accumulated over the hits keyed to it.
struct ShipRow {
    ship_name: String,
    shells_on_target: u32,
    eligible_hits: u32,
    exclusions: BTreeMap<ExclusionReason, u32>,
    not_applicable: u32,
    /// Every vehicle the row folds together, since two of the same ship on the
    /// enemy team share one row but carry their fires separately.
    entities: BTreeSet<EntityId>,
}

/// A row per target ship, over every hit the model classified rather than only
/// the trials.
///
/// A ship hit only by shells the model refused still gets a row, stating no
/// rate: what the reader needs there is why none of those hits counted, which
/// is exactly what the row's exclusion tally says. Hits keyed to a ship that
/// never resolved to a [`VictimContext`] have no row to sit in and appear only
/// in the aggregate.
fn per_ship_breakdown(
    input: &FireChanceInput<'_>,
    classified: &[ClassifiedHit],
    counted: &[&Candidate<'_>],
    fires_by_victim: &HashMap<EntityId, u32>,
    formula_applies: bool,
) -> Vec<PerShipFireChance> {
    // Grouped by ship index rather than entity so two of the same ship on the
    // enemy team read as one row, which is what a per-target-ship breakdown
    // means. `BTreeMap` because the row order must not depend on hash seeds.
    let mut rows: BTreeMap<&str, ShipRow> = BTreeMap::new();
    for record in classified {
        let Some(victim) = input.victims.get(&record.victim) else { continue };
        let row = rows.entry(victim.ship_index.as_str()).or_insert_with(|| ShipRow {
            ship_name: victim.ship_name.clone(),
            shells_on_target: 0,
            eligible_hits: 0,
            exclusions: BTreeMap::new(),
            not_applicable: 0,
            entities: BTreeSet::new(),
        });
        row.shells_on_target += 1;
        row.entities.insert(record.victim);
        match record.disposition {
            HitDisposition::Trial => row.eligible_hits += 1,
            HitDisposition::Excluded(reason) => *row.exclusions.entry(reason).or_insert(0) += 1,
            HitDisposition::NotApplicable => row.not_applicable += 1,
        }
    }

    // The expectation needs the candidates themselves rather than their count,
    // because it saturates over the ones sharing a section and a tick.
    let mut trials: BTreeMap<&str, Vec<&Candidate<'_>>> = BTreeMap::new();
    for candidate in counted {
        let Some(victim) = input.victims.get(&candidate.victim) else { continue };
        trials.entry(victim.ship_index.as_str()).or_default().push(candidate);
    }

    rows.into_iter()
        .map(|(ship_index, row)| PerShipFireChance {
            victim_ship_index: ship_index.to_owned(),
            victim_ship_name: row.ship_name,
            shells_on_target: row.shells_on_target,
            eligible_hits: row.eligible_hits,
            exclusions: row.exclusions,
            not_applicable: row.not_applicable,
            fires: row.entities.iter().filter_map(|entity| fires_by_victim.get(entity)).sum(),
            // Same rule as the aggregate: one hit with an uncomputable
            // expectation makes the row's total unknown, not smaller.
            expected_fires: formula_applies
                .then(|| expected_observable_fires(trials.get(ship_index).map_or(&[], Vec::as_slice)))
                .flatten(),
        })
        .collect()
}

/// The attacker's burn-chance formula for the shell that produced the most
/// eligible hits: the shell's raw `burnProb` this formula started from, and
/// the modifier steps that moved it (identities dropped). The base is `Some`
/// whenever a shell resolved, even if every modifier turned out to be an
/// identity and the step list is empty: the hover's base line does not depend
/// on there being any steps to show under it.
fn formula_steps(
    input: &FireChanceInput<'_>,
    bundle: &ModifierBundle,
    tier: u32,
    counted: &[&Candidate<'_>],
) -> (Option<f32>, Vec<FormulaStep>) {
    let mut hits_per_shell: HashMap<GameParamId, u32> = HashMap::new();
    for candidate in counted {
        *hits_per_shell.entry(candidate.shell).or_insert(0) += 1;
    }
    // Ties break on the lower id so the rendered breakdown is stable between runs.
    let Some((shell, _)) = hits_per_shell.into_iter().max_by_key(|(id, count)| (*count, std::cmp::Reverse(id.raw())))
    else {
        return (None, Vec::new());
    };

    let Some(projectile) = input.params.game_param_by_id(shell).and_then(|p| p.projectile().cloned()) else {
        return (None, Vec::new());
    };
    let Some(burn_prob) = projectile.burn_prob() else {
        return (None, Vec::new());
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
    (Some(burn_prob), steps)
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
    use wows_replays::analyzer::battle_controller::state::VictimPose;
    use wows_replays::analyzer::decoder::ArtillerySalvo;
    use wows_replays::analyzer::decoder::ArtilleryShotData;
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

    /// One more of our own main-battery shells landing on the victim.
    struct ExtraHit {
        section: u8,
        /// Seconds from [`HIT_CLOCK`]; zero puts it in the same server tick as
        /// the first hit, which is what a salvo straddling one section looks
        /// like on the wire.
        offset: f32,
        hit_type: ShellHitType,
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
        /// How many shells the main-battery salvo fired. Every main-battery hit
        /// in a scenario belongs to that one salvo, so this is the whole
        /// fired-shell count and is independent of how many of them landed.
        main_salvo_shells: usize,
        /// How many further main-battery salvos were fired that landed nothing
        /// at all. They reach the analysis only through the salvo log, which is
        /// the whole reason that log exists.
        main_salvos_that_missed: usize,
        /// Every salvo logged twice, which is what a merged multi-perspective
        /// session produces when two clients were both told about the shot.
        every_salvo_delivered_twice: bool,
        secondary_hit_section: Option<u8>,
        secondary_hit_offset: f32,
        ribbon_offset: f32,
        uncomputable_node_probability: Option<u8>,
        extra_main_hits: Vec<ExtraHit>,
        hit_off_the_hull: bool,
        hit_collision: Recognized<wowsunpack::game_types::CollisionType>,
        burn_change_offset: f32,
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
            main_salvo_shells: 1,
            main_salvos_that_missed: 0,
            every_salvo_delivered_twice: false,
            secondary_hit_section: None,
            secondary_hit_offset: 0.1,
            ribbon_offset: 0.0,
            uncomputable_node_probability: None,
            extra_main_hits: Vec::new(),
            hit_off_the_hull: false,
            hit_collision: Recognized::Known(wowsunpack::game_types::CollisionType::HitEntity),
            burn_change_offset: 0.0,
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

        fn with_the_main_salvo_firing(mut self, shells: usize) -> Fixture {
            self.main_salvo_shells = shells;
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
        fn also_hitting_section(self, section: u8) -> Fixture {
            self.also_hitting_section_at(section, 20.0)
        }

        fn also_hitting_section_at(self, section: u8, offset: f32) -> Fixture {
            self.also_hitting_section_with(section, offset, ShellHitType::Normal)
        }

        fn also_hitting_section_with(mut self, section: u8, offset: f32, hit_type: ShellHitType) -> Fixture {
            self.extra_main_hits.push(ExtraHit { section, offset, hit_type });
            self
        }

        /// Move the first hit's impact far past the hull's ends, which is what
        /// a hit mis-keyed to a neighbouring ship looks like.
        fn with_the_impact_off_the_hull(mut self) -> Fixture {
            self.hit_off_the_hull = true;
            self
        }

        /// The shell struck an island. Its `shell_hit` stays `Normal`, which is
        /// what the game sends, so only the collision type separates it from a
        /// hit on the victim.
        fn with_the_shell_striking_terrain(mut self) -> Fixture {
            self.hit_collision = Recognized::Known(wowsunpack::game_types::CollisionType::HitGround);
            self
        }

        /// A collision id this build's constants table does not name.
        fn with_an_unnamed_collision_type(mut self) -> Fixture {
            self.hit_collision = Recognized::Unknown("7".to_owned());
            self
        }

        fn with_the_burn_change_offset_by(mut self, seconds: f32) -> Fixture {
            self.burn_change_offset = seconds;
            self
        }

        /// Fire `salvos` more main-battery salvos, none of which land a shell.
        fn with_main_salvos_that_missed(mut self, salvos: usize) -> Fixture {
            self.main_salvos_that_missed = salvos;
            self
        }

        fn with_every_salvo_delivered_twice(mut self) -> Fixture {
            self.every_salvo_delivered_twice = true;
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
            hits[0].hit.hit_type.collision = self.hit_collision.clone();
            if !self.salvo_matched {
                hits[0].salvo = None;
            }
            if self.hit_off_the_hull {
                let far = wowsunpack::game_params::types::Meters::from(600.0).to_world().value();
                hits[0].hit.position = WorldPos::new(far, 0.0, 0.0);
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
            for extra in &self.extra_main_hits {
                hits.push(hit(
                    victim_id(),
                    main_shell_id(),
                    GameClock(HIT_CLOCK.0 + extra.offset),
                    extra.section,
                    extra.hit_type,
                ));
            }
            if self.hit_on_a_hull_we_cannot_place {
                hits.push(hit(other_victim_id(), main_shell_id(), HIT_CLOCK, 0, ShellHitType::Normal));
            }
            // Every main-battery hit in a scenario shares salvo 1, so widening
            // the salvo is a property of the scenario rather than of any one hit.
            for hit in &mut hits {
                if let Some(salvo) = hit.salvo.as_mut()
                    && salvo.params_id == main_shell_id()
                {
                    salvo.shots = shot_list(self.main_salvo_shells);
                }
            }

            // The salvo log the ingest would have recorded: every distinct
            // salvo among the hits at its full width, plus the salvos this
            // scenario says landed nothing, which no hit can testify to.
            let mut salvos: Vec<SalvoEvent> = Vec::new();
            for hit in &hits {
                let Some(salvo) = hit.salvo.as_ref() else { continue };
                let already = salvos.iter().any(|logged| {
                    logged.owner_id == salvo.owner_id
                        && logged.salvo_id == salvo.salvo_id
                        && logged.params_id == salvo.params_id
                });
                if already {
                    continue;
                }
                salvos.push(SalvoEvent {
                    clock: hit.fired_at.unwrap_or(hit.clock),
                    owner_id: salvo.owner_id,
                    params_id: salvo.params_id,
                    salvo_id: salvo.salvo_id,
                    first_shot: salvo.shots.first().map(|shot| shot.shot_id),
                    shots: salvo.shots.len() as u32,
                });
            }
            for index in 0..self.main_salvos_that_missed {
                salvos.push(SalvoEvent {
                    clock: HIT_CLOCK,
                    owner_id: attacker_id(),
                    params_id: main_shell_id(),
                    // One salvo id shared by every pack, which is what a single
                    // trigger pull arriving as several `SHOTS_PACK` entries
                    // looks like on the wire.
                    salvo_id: 100,
                    first_shot: Some(ShotId::from(1000 + index as u32)),
                    shots: self.main_salvo_shells as u32,
                });
            }

            if self.every_salvo_delivered_twice {
                let once = salvos.clone();
                salvos.extend(once);
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
                clock: GameClock(HIT_CLOCK.0 + self.burn_change_offset),
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
            // Every scenario's victim carries a Damage Control Party slot: its
            // work time is what bounds how long an activation fired just before
            // the ship entered AOI could still be covering the hit clock, and
            // without one no continuity argument runs at all.
            let consumables = vec![dcp_inventory()];
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
                salvos: Box::leak(Box::new(salvos)),
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

    /// A salvo's shot list, `shells` shells wide. Only its length is read here,
    /// which is what makes the fired-shell count a per-salvo figure rather than
    /// a per-hit one.
    fn shot_list(shells: usize) -> Vec<ArtilleryShotData> {
        (0..shells)
            .map(|index| ArtilleryShotData {
                origin: WorldPos::new(0.0, 0.0, 0.0),
                pitch: 0.0,
                speed: 800.0,
                target: WorldPos::new(0.0, 0.0, 0.0),
                shot_id: ShotId::from(index as u32),
                gun_barrel_id: index as u16,
                server_time_left: 0.0,
                shooter_height: 0.0,
                hit_distance: 0.0,
            })
            .collect()
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
            salvo: Some(ArtillerySalvo { owner_id: attacker_id(), params_id: shell, salvo_id: 1, shots: shot_list(1) }),
            fired_at: Some(GameClock(clock.0 - 5.0)),
            victim_pose: Some(VictimPose { position: WorldPos::new(0.0, 0.0, 0.0), yaw: 0.0, pitch: 0.0, roll: 0.0 }),
        }
    }

    /// Clean case: one HE hit on a cold target with DCP down, and a SetFire
    /// ribbon at the same clock.
    #[test]
    fn a_clean_hit_that_started_a_fire_counts_once() {
        let out = analyze(&fixture().build()).expect("geometry present");
        assert_eq!(out.eligible_hits, 1);
        assert_eq!(out.fires, 1);
        assert_eq!(out.per_ship[0].rate(), Some(1.0));
        assert_eq!(out.unattributed_fires, 0);
    }

    /// The same hit with no ribbon is an eligible miss, not an exclusion.
    #[test]
    fn an_eligible_hit_with_no_ribbon_is_a_miss() {
        let out = analyze(&fixture().without_ribbons().build()).expect("geometry");
        assert_eq!(out.eligible_hits, 1);
        assert_eq!(out.fires, 0);
        assert_eq!(out.per_ship[0].rate(), Some(0.0));
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

    /// Fire Prevention Expert merges the victim's third fire zone into the
    /// second rather than making it unburnable, so a hit there is an ordinary
    /// trial whose fire lights the merge target.
    #[test]
    fn a_hit_on_the_merged_node_is_a_trial_against_the_zone_it_merges_into() {
        let out = analyze(&fixture().with_victim_fire_prevention().hitting_section(2).server_lights_section(1).build())
            .expect("geometry");
        assert_eq!(out.eligible_hits, 1);
        assert!(out.exclusions.is_empty(), "got {:?}", out.exclusions);
        assert_eq!(out.fires, 1);
        assert_eq!(out.section_predictions.len(), 1);
        assert_eq!(out.section_predictions[0].predicted.get(), FIRE_PREVENTION_MERGE_TARGET);
        assert_eq!(out.section_predictions[0].actual.get(), FIRE_PREVENTION_MERGE_TARGET);
    }

    /// The merged hit reads the burn bit of the zone it merged into, so the
    /// merge target already burning excludes it while the merged node's own bit
    /// leaves it alone. That bit is never set on such a victim anyway, so
    /// reading it would admit every merged hit whatever the ship was doing.
    #[test]
    fn a_merged_hit_is_gated_by_the_zone_it_merges_into() {
        let target_burning =
            analyze(&fixture().with_victim_fire_prevention().hitting_section(2).with_burn_mask_before(0b0010).build())
                .expect("geometry");
        assert_eq!(target_burning.eligible_hits, 0);
        assert_eq!(target_burning.exclusions[&ExclusionReason::SectionAlreadyBurning], 1);

        let merged_node_burning =
            analyze(&fixture().with_victim_fire_prevention().hitting_section(2).with_burn_mask_before(0b0100).build())
                .expect("geometry");
        assert_eq!(merged_node_burning.eligible_hits, 1);
    }

    /// The merge is scoped to one node. A hit anywhere else on a Fire
    /// Prevention victim is unaffected by it.
    #[test]
    fn the_merge_leaves_every_other_section_alone() {
        let out = analyze(&fixture().with_victim_fire_prevention().hitting_section(3).build()).expect("geometry");
        assert_eq!(out.eligible_hits, 1);
        assert_eq!(out.section_predictions[0].predicted.get(), 3);
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
        assert_eq!(out.exclusions[&ExclusionReason::AmbiguousWithAnotherHit], 1);
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
        assert_eq!(out.exclusions[&ExclusionReason::AmbiguousWithAnotherHit], 1);
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

    /// One candidate, section and clock to itself, with a stated expectation.
    fn candidate_with(hit: &ResolvedShotHit, section: u8, expected: Option<f32>) -> Candidate<'_> {
        Candidate {
            hit,
            victim: hit.victim_entity_id,
            section: BurnNodeIndex::new(section).expect("section in range"),
            expected: expected.map(|chance| BurnChance::new(chance).expect("a probability")),
            shell: main_shell_id(),
            // These candidates never reach the contest, which is the only thing
            // that reads the record index back.
            record: 0,
        }
    }

    /// A group of one saturates at nothing, so the group's expectation is that
    /// hit's own chance, to the bit.
    #[test]
    fn a_group_of_one_expects_exactly_that_hits_chance() {
        let hits = [hit(victim_id(), main_shell_id(), HIT_CLOCK, 0, ShellHitType::Normal)];
        let candidates = [candidate_with(&hits[0], 0, Some(0.37))];
        let borrowed: Vec<&Candidate<'_>> = candidates.iter().collect();
        assert_eq!(expected_observable_fires(&borrowed), Some(0.37));
    }

    /// Several shells on one section in one tick can only ever produce one
    /// visible fire between them, so the group expects the chance that at least
    /// one of them rolled, one minus the product of `1 - p` over the group,
    /// which is strictly under their sum. The
    /// chances are deliberately unequal, because the group is a product rather
    /// than anything that assumes they match.
    #[test]
    fn a_group_expects_one_saturating_fire_over_differing_chances() {
        let hits = [
            hit(victim_id(), main_shell_id(), HIT_CLOCK, 0, ShellHitType::Normal),
            hit(victim_id(), main_shell_id(), HIT_CLOCK, 0, ShellHitType::Normal),
        ];
        let candidates = [candidate_with(&hits[0], 0, Some(0.1)), candidate_with(&hits[1], 0, Some(0.2))];
        let borrowed: Vec<&Candidate<'_>> = candidates.iter().collect();
        let total = expected_observable_fires(&borrowed).expect("both chances known");
        assert!((total - (1.0 - 0.9 * 0.8)).abs() < 1e-6, "got {total}");
        assert!(total < 0.1 + 0.2, "got {total}, which is the naive sum or more");
    }

    /// Only shells sharing a victim, a section and a tick saturate against one
    /// another. A different section, a different tick and a different victim
    /// are each their own group, and separate groups add in full.
    #[test]
    fn separate_groups_add_their_expectations() {
        let later = GameClock(HIT_CLOCK.0 + 20.0);
        let hits = [
            hit(victim_id(), main_shell_id(), HIT_CLOCK, 0, ShellHitType::Normal),
            hit(victim_id(), main_shell_id(), HIT_CLOCK, 0, ShellHitType::Normal),
            hit(victim_id(), main_shell_id(), HIT_CLOCK, 1, ShellHitType::Normal),
            hit(victim_id(), main_shell_id(), later, 0, ShellHitType::Normal),
            hit(other_victim_id(), main_shell_id(), HIT_CLOCK, 0, ShellHitType::Normal),
        ];
        let candidates = [
            candidate_with(&hits[0], 0, Some(0.1)),
            candidate_with(&hits[1], 0, Some(0.2)),
            candidate_with(&hits[2], 1, Some(0.3)),
            candidate_with(&hits[3], 0, Some(0.4)),
            candidate_with(&hits[4], 0, Some(0.5)),
        ];
        let borrowed: Vec<&Candidate<'_>> = candidates.iter().collect();
        let total = expected_observable_fires(&borrowed).expect("every chance known");
        let shared_section = 1.0 - 0.9 * 0.8;
        assert!((total - (shared_section + 0.3 + 0.4 + 0.5)).abs() < 1e-6, "got {total}");
    }

    /// An unknown chance leaves its whole group unknown, and one unknown group
    /// leaves the total unknown. Folding the known members in would report a
    /// number that silently treats the unknown as zero.
    #[test]
    fn an_unknown_chance_makes_its_group_and_the_total_unknown() {
        let hits = [
            hit(victim_id(), main_shell_id(), HIT_CLOCK, 0, ShellHitType::Normal),
            hit(victim_id(), main_shell_id(), HIT_CLOCK, 0, ShellHitType::Normal),
            hit(victim_id(), main_shell_id(), HIT_CLOCK, 1, ShellHitType::Normal),
        ];
        let candidates = [
            candidate_with(&hits[0], 0, Some(0.1)),
            candidate_with(&hits[1], 0, None),
            candidate_with(&hits[2], 1, Some(0.2)),
        ];
        let borrowed: Vec<&Candidate<'_>> = candidates.iter().collect();
        assert_eq!(expected_observable_fires(&borrowed), None);
    }

    /// Two shells of ours reaching one section in one tick expect one fire
    /// between them, not two rolls, because one fire is all the replay can ever
    /// show. The observed side counts the same quantity, which is what makes
    /// the two comparable.
    #[test]
    fn same_tick_hits_on_one_section_expect_a_single_saturating_fire() {
        let out = analyze(
            &fixture()
                .with_shell_burn_prob(0.12)
                .with_victim_node_probability(0.6004)
                .with_victim_burn_prob_modifier(0.9)
                .hitting_section(0)
                .also_hitting_section_at(0, 0.0)
                .build(),
        )
        .expect("geometry");
        let per_hit = 0.12 * 0.6004 * 0.9;
        let saturating = 1.0 - (1.0 - per_hit) * (1.0 - per_hit);
        assert_eq!(out.eligible_hits, 2);
        let total = out.expected_fires.expect("expected");
        assert!((total - saturating).abs() < 1e-6, "got {total}");
        assert!(total < 2.0 * per_hit, "got {total}, which is the naive sum or more");
        let ship = out.per_ship[0].expected_fires.expect("expected");
        assert!((ship - saturating).abs() < 1e-6, "got {ship}");
        assert!((out.per_ship[0].expected_rate().expect("rate") - saturating / 2.0).abs() < 1e-6);
    }

    /// One hit's expectation is attacker chance times the victim's node
    /// probability times the victim's burnProb: 0.12 * 0.6004 * 0.9. Two hits
    /// landing on different sections are two groups, so they expect twice that
    /// many fires at the same per-hit rate, which is what tells a count apart
    /// from a rate.
    #[test]
    fn expected_fires_multiplies_both_halves_and_sums_over_hits() {
        let out = analyze(
            &fixture()
                .with_shell_burn_prob(0.12)
                .with_victim_node_probability(0.6004)
                .with_victim_burn_prob_modifier(0.9)
                .hitting_section(0)
                .also_hitting_section(3)
                .build(),
        )
        .expect("geometry");
        let per_hit = 0.12 * 0.6004 * 0.9;
        assert_eq!(out.eligible_hits, 2);
        assert!((out.expected_fires.expect("expected") - 2.0 * per_hit).abs() < 1e-5);
        assert!((out.per_ship[0].expected_fires.expect("expected") - 2.0 * per_hit).abs() < 1e-5);
        assert!((out.per_ship[0].expected_rate().expect("rate") - per_hit).abs() < 1e-5);
    }

    /// `expected_fires` is a sum, so an empty candidate list legitimately
    /// totals zero fires. No rate is stated anywhere over it: the rate that
    /// total would imply does not exist, and a caller rendering one would print
    /// a fabricated 0.0%.
    #[test]
    fn zero_eligible_hits_state_no_rate_anywhere() {
        let out = analyze(&fixture().with_hit_type(ShellHitType::Ricochet).build()).expect("geometry");
        assert_eq!(out.eligible_hits, 0);
        assert_eq!(out.expected_fires, Some(0.0));
        assert_eq!(out.ships_with_trials(), 0);
        assert_eq!(out.per_ship[0].rate(), None);
        assert_eq!(out.per_ship[0].expected_rate(), None);
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
        assert_eq!(agree.independent_section_agreement(), Some(1.0));
        assert_eq!(agree.section_predictions.len(), 1);
        assert_eq!(agree.section_predictions[0].hull_sections, NODES.len());

        let disagree = analyze(&fixture().server_lights_section(3).hitting_section(0).build()).expect("geometry");
        assert_eq!(disagree.independent_section_agreement(), Some(0.0));
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
        // Recorded, but not scored: `actual` was picked as the risen bit
        // nearest the prediction, so the pair is not evidence either way.
        assert!(!out.section_predictions[0].is_independent_evidence());
        assert_eq!(out.independent_section_agreement(), None);
    }

    /// With no attributed fires there is nothing to agree about.
    #[test]
    fn section_agreement_is_absent_without_fires() {
        let out = analyze(&fixture().without_ribbons().build()).expect("geometry");
        assert_eq!(out.independent_section_agreement(), None);
    }

    /// A ship hit only by shells the model refused still gets a row, because
    /// why none of them counted is what the reader needs there. The row states
    /// no rate, so nothing reports one over zero samples.
    #[test]
    fn a_ship_with_only_refused_hits_gets_a_row_that_states_no_rate() {
        let out = analyze(&fixture().with_dcp_unknown().build()).expect("geometry");
        assert_eq!(out.eligible_hits, 0);
        assert_eq!(out.per_ship.len(), 1);
        assert_eq!(out.per_ship[0].shells_on_target, 1);
        assert_eq!(out.per_ship[0].exclusions[&ExclusionReason::DamageControlUnknown], 1);
        assert_eq!(out.per_ship[0].rate(), None);
        assert_eq!(out.ships_with_trials(), 0);
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
    /// breakdown is empty rather than a list of identities. The base is still
    /// `Some`: a shell resolved, it just carries no modifier worth showing.
    #[test]
    fn a_stock_build_has_no_formula_steps_but_still_has_a_base() {
        let out = analyze(&fixture().build()).expect("geometry");
        assert!(out.formula.is_empty(), "got {:?}", out.formula);
        assert_eq!(out.formula_base, Some(0.12));
        assert_eq!(out.formula_total(), Some((0.12, 0.12)));
    }

    /// The formula total is the last step's running result, and the clamp is
    /// the one `classify` gates on, so a raw total past 100% reports both.
    #[test]
    fn the_formula_total_reads_the_last_step_and_carries_the_clamp() {
        let mut out = analyze(&fixture().build()).expect("geometry");
        out.formula = vec![FormulaStep {
            modifier: "artilleryBurnChanceBonus".to_owned(),
            source: None,
            op: FormulaOp::Add,
            value: 1.1,
            result: 1.22,
        }];
        let (raw, clamped) = out.formula_total().expect("a total");
        assert!((raw - 1.22).abs() < 1e-6);
        assert_eq!(clamped, 1.0);

        out.formula_base = None;
        assert_eq!(out.formula_total(), None);
    }

    /// Every hit the model looked at, trials and refusals alike.
    #[test]
    fn shells_on_target_total_the_trials_and_every_refusal() {
        let out =
            analyze(&fixture().hitting_section(0).also_hitting_section_with(3, 20.0, ShellHitType::Ricochet).build())
                .expect("geometry");
        assert_eq!(out.eligible_hits, 1);
        assert_eq!(out.exclusions[&ExclusionReason::HitTypeDoesNotRoll], 1);
        assert_eq!(out.shells_on_target, 2);
    }

    /// The three buckets partition the shells that landed, at both levels. A
    /// hit that fell out of the accounting entirely would show up here as a
    /// shortfall rather than as a quietly better rate.
    #[test]
    fn the_buckets_partition_the_shells_on_target() {
        let out = analyze(
            &fixture()
                .with_the_victim_dead_at(GameClock(90.0))
                .also_hitting_section(3)
                .also_hitting_a_hull_we_cannot_place()
                .build(),
        )
        .expect("geometry");

        assert_eq!(out.shells_on_target, 3);
        assert_eq!(out.not_applicable, 2, "both hits on the dead victim");
        assert_eq!(out.exclusions[&ExclusionReason::NoSectionGeometry], 1);
        assert_eq!(out.eligible_hits + out.exclusions.values().sum::<u32>() + out.not_applicable, out.shells_on_target);

        for ship in &out.per_ship {
            assert_eq!(
                ship.eligible_hits + ship.exclusions.values().sum::<u32>() + ship.not_applicable,
                ship.shells_on_target,
                "{} does not account for its own hits",
                ship.victim_ship_name
            );
        }
        assert_eq!(out.per_ship.iter().map(|ship| ship.shells_on_target).sum::<u32>(), out.shells_on_target);
    }

    /// A shell landing on a ship that was already dead is not a refusal: there
    /// was nothing left to set alight, so the eligibility model was never asked
    /// anything and the hit sits outside the exclusion tally entirely.
    #[test]
    fn a_hit_on_a_dead_victim_is_not_applicable_rather_than_excluded() {
        let out = analyze(&fixture().with_the_victim_dead_at(GameClock(90.0)).build()).expect("geometry");
        assert_eq!(out.not_applicable, 1);
        assert!(out.exclusions.is_empty(), "got {:?}", out.exclusions);
        assert_eq!(out.per_ship[0].not_applicable, 1);
    }

    /// A salvo is counted once at its full width however many of its shells were
    /// seen landing, so the fired count is not the hit count by another name.
    /// Both hits here belong to one six-shell salvo.
    #[test]
    fn shells_fired_counts_the_salvo_not_the_hits() {
        let out = analyze(&fixture().with_the_main_salvo_firing(6).hitting_section(0).also_hitting_section(3).build())
            .expect("geometry");
        assert_eq!(out.he_shells_fired, 6);
        assert_eq!(out.shells_on_target, 2);
    }

    /// The point of the fired-side log: a salvo that landed nothing is still
    /// fired, and counting only the salvos seen landing is what made this
    /// number read as a 75% hit rate across the corpus.
    #[test]
    fn shells_fired_counts_the_salvos_that_landed_nothing() {
        let out = analyze(&fixture().with_the_main_salvo_firing(6).with_main_salvos_that_missed(3).build())
            .expect("geometry");
        assert_eq!(out.he_shells_fired, 24, "four six-shell salvos, one of which landed a shell");
        assert_eq!(out.shells_on_target, 1);
    }

    /// A merged session hears the same salvo from every perspective that saw
    /// it. Both records carry the server's own shot ids, so they collapse onto
    /// one key instead of doubling the count.
    #[test]
    fn shells_fired_counts_a_duplicated_salvo_once() {
        let fixture = fixture()
            .with_the_main_salvo_firing(6)
            .with_main_salvos_that_missed(3)
            .with_every_salvo_delivered_twice()
            .build();
        let out = analyze(&fixture).expect("geometry");
        assert_eq!(out.he_shells_fired, 24);
    }

    /// Someone else's salvo is in the log too, since the log records every
    /// owner, and it is not ours to count.
    #[test]
    fn shells_fired_ignores_another_ship_salvo() {
        let out = analyze(&fixture().with_the_main_salvo_firing(6).fired_by_another_ship().build()).expect("geometry");
        assert_eq!(out.he_shells_fired, 0);
    }

    /// The fired count is gated on the same "could this shell start a fire"
    /// test the trials are, so an AP salvo contributes nothing to it.
    #[test]
    fn shells_fired_ignores_a_shell_that_cannot_burn() {
        let out =
            analyze(&fixture().with_shell_burn_prob(-0.5).with_the_main_salvo_firing(6).build()).expect("geometry");
        assert_eq!(out.he_shells_fired, 0);
        assert_eq!(out.shells_on_target, 1);
        assert_eq!(out.exclusions[&ExclusionReason::ShellCannotBurn], 1);
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
    fn a_hit_at_or_after_the_victims_death_is_not_a_trial() {
        let out = analyze(&fixture().with_the_victim_dead_at(GameClock(90.0)).build()).expect("geometry");
        assert_eq!(out.eligible_hits, 0);
        assert_eq!(out.not_applicable, 1);
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
        assert_eq!(out.exclusions[&ExclusionReason::AmbiguousWithAnotherHit], 1);
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
    /// Expert, so a hit in the merged node might have rolled against either
    /// bit and the already-burning test has no section to read.
    #[test]
    fn an_unresolved_victim_build_refuses_a_merged_node_hit() {
        let out = analyze(&fixture().with_an_unresolved_victim_build().hitting_section(2).build()).expect("geometry");
        assert_eq!(out.eligible_hits, 0);
        assert_eq!(out.exclusions[&ExclusionReason::MergedSectionVictimBuildUnknown], 1);

        // The merge is node 2 only; other sections are unaffected by an
        // unresolved build.
        let elsewhere =
            analyze(&fixture().with_an_unresolved_victim_build().hitting_section(0).build()).expect("geometry");
        assert_eq!(elsewhere.eligible_hits, 1);
    }

    /// A merged-node hit on a victim whose build we could not read may have lit
    /// either bit, so it contests a coincident trial in the merge target as well
    /// as one in the node it landed in. Crediting that trial with the ribbon
    /// would add a fire to the numerator that this shell may have set.
    #[test]
    fn an_unresolved_merged_node_hit_contests_both_bits() {
        let out = analyze(
            &fixture()
                .with_an_unresolved_victim_build()
                .hitting_section(1)
                .also_hitting_section_at(2, 0.0)
                .server_lights_section(1)
                .build(),
        )
        .expect("geometry");
        assert_eq!(out.eligible_hits, 0);
        assert_eq!(out.exclusions[&ExclusionReason::MergedSectionVictimBuildUnknown], 1);
        assert_eq!(out.exclusions[&ExclusionReason::AmbiguousWithAnotherHit], 1);
        assert_eq!(out.unattributed_fires, 1);
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
        assert_eq!(out.per_ship[0].rate(), Some(1.0));
        assert_eq!(out.expected_fires, None);
        assert_eq!(out.per_ship[0].expected_fires, None);
    }

    /// The formula breakdown goes with the expected value. Folded from the
    /// stock bundle it is a real formula for a build nobody is running, and the
    /// hover presents it as the player's own.
    #[test]
    fn an_unfoldable_build_shows_no_formula_at_all() {
        let out = analyze(&fixture().with_a_modifier_this_version_cannot_classify().build()).expect("geometry");
        assert_eq!(out.formula_base, None);
        assert!(out.formula.is_empty(), "got {:?}", out.formula);
    }

    /// Every hit in one `ShotKills` packet carries that packet's clock, so a
    /// salvo putting several shells into one section arrives as several hits at
    /// one clock, all reading the same pre-tick burn mask. The game rolls per
    /// shell, so each of them rolled and each is a trial. Only one fire can
    /// ever be seen, which is what makes the rate a lower bound.
    #[test]
    fn several_hits_on_one_section_in_one_tick_are_all_trials() {
        let out = analyze(&fixture().hitting_section(0).also_hitting_section_at(0, 0.0).build()).expect("geometry");
        assert_eq!(out.eligible_hits, 2);
        assert_eq!(out.fires, 1);
        assert_eq!(out.unattributed_fires, 0);
    }

    /// The same group with no fire counts identically. Whether a hit is a trial
    /// is decided on the hits alone, so it cannot depend on the outcome.
    #[test]
    fn a_same_tick_section_group_counts_in_full_when_no_fire_started() {
        let out = analyze(&fixture().hitting_section(0).also_hitting_section_at(0, 0.0).without_ribbons().build())
            .expect("geometry");
        assert_eq!(out.eligible_hits, 2);
        assert_eq!(out.fires, 0);
        assert_eq!(out.unattributed_fires, 0);
    }

    /// The sections are independent, so two hits sharing a tick but not a
    /// section are two separate trials.
    #[test]
    fn two_hits_in_one_tick_on_different_sections_both_count() {
        let out = analyze(&fixture().hitting_section(0).also_hitting_section_at(3, 0.0).build()).expect("geometry");
        assert_eq!(out.eligible_hits, 2);
    }

    /// Two hits on one section in different ticks are two separate trials: the
    /// second reads a burn mask the first's fire is already in.
    #[test]
    fn two_hits_on_one_section_in_different_ticks_both_count() {
        let out = analyze(&fixture().hitting_section(3).also_hitting_section_at(3, 20.0).build()).expect("geometry");
        assert_eq!(out.eligible_hits, 2);
    }

    /// A ricochet is not proof that a shell could not have rolled for fire, only
    /// an absence of proof that it could. So it contests a coincident eligible
    /// hit in its section: crediting the ribbon to that hit would add a fire to
    /// the numerator with no trial behind it in the denominator.
    #[test]
    fn a_hit_type_we_cannot_rule_out_contests_a_coincident_eligible_hit() {
        let out =
            analyze(&fixture().hitting_section(0).also_hitting_section_with(0, 0.0, ShellHitType::Ricochet).build())
                .expect("geometry");
        assert_eq!(out.eligible_hits, 0);
        assert_eq!(out.fires, 0);
        assert_eq!(out.exclusions[&ExclusionReason::HitTypeDoesNotRoll], 1);
        assert_eq!(out.exclusions[&ExclusionReason::AmbiguousWithAnotherHit], 1);
        assert_eq!(out.unattributed_fires, 1);
    }

    /// The contest is section-scoped, so an unprovable hit elsewhere on the
    /// hull leaves the trial alone.
    #[test]
    fn a_non_rolling_hit_on_another_section_does_not_contest() {
        let out =
            analyze(&fixture().hitting_section(0).also_hitting_section_with(3, 0.0, ShellHitType::Ricochet).build())
                .expect("geometry");
        assert_eq!(out.eligible_hits, 1);
        assert_eq!(out.fires, 1);
    }

    /// A shell that struck an island is keyed to a victim like any other, and
    /// the game reports its `shell_hit` as an ordinary `Normal`. Only the
    /// collision type says it hit no ship, and a shell that hit no ship set no
    /// ship alight.
    #[test]
    fn a_shell_that_struck_terrain_is_not_a_fire_trial() {
        let out = analyze(&fixture().with_the_shell_striking_terrain().build()).expect("geometry");
        assert_eq!(out.eligible_hits, 0);
        assert_eq!(out.exclusions[&ExclusionReason::ImpactNotOnAShip], 1);
    }

    /// A collision id the constants table does not name is an unknown, not a
    /// proof that the shell missed. Refusing it would empty the denominator of
    /// every replay whose build carries no constants table.
    #[test]
    fn an_unnamed_collision_type_still_reaches_the_geometry() {
        let out = analyze(&fixture().with_an_unnamed_collision_type().build()).expect("geometry");
        assert_eq!(out.eligible_hits, 1);
        assert_eq!(out.exclusions.get(&ExclusionReason::ImpactNotOnAShip), None);
    }

    /// `victim_entity_id` is the ship whose last known position was nearest the
    /// impact, which in a tight formation can be a neighbour. An impact that far
    /// from the hull's nodes would otherwise be clamped onto its bow or stern
    /// and enter the denominator as a trial that can only miss.
    #[test]
    fn an_impact_nowhere_near_the_victims_hull_is_excluded() {
        let out = analyze(&fixture().with_the_impact_off_the_hull().build()).expect("geometry");
        assert_eq!(out.eligible_hits, 0);
        assert_eq!(out.exclusions[&ExclusionReason::ImpactOffTheHull], 1);
    }

    /// The causing hit precedes its ribbon. A hit after it is packet ordering
    /// within a tick, so only that much jitter is allowed forward.
    #[test]
    fn a_hit_after_its_ribbon_is_attributed_only_within_the_reorder_tolerance() {
        let jittered = analyze(&fixture().with_the_ribbon_offset_by(-0.1).build()).expect("geometry");
        assert_eq!(jittered.fires, 1);

        let too_late = analyze(&fixture().with_the_ribbon_offset_by(-0.3).build()).expect("geometry");
        assert_eq!(too_late.eligible_hits, 1);
        assert_eq!(too_late.fires, 0);
        assert_eq!(too_late.unattributed_fires, 1);
    }

    /// `onEntityDeath` lights burn nodes for effect, so a transition at or after
    /// the victim died is not a fire anyone set. `VictimTrack` already drops
    /// those from the burn mask; attribution has to drop them too, or a shell
    /// that landed just before the kill is credited with a death flare.
    #[test]
    fn a_death_flare_transition_cannot_be_attributed() {
        let out = analyze(
            &fixture()
                .with_the_victim_dead_at(GameClock(HIT_CLOCK.0 + 0.2))
                .with_the_burn_change_offset_by(0.3)
                .with_the_ribbon_offset_by(0.3)
                .build(),
        )
        .expect("geometry");
        assert_eq!(out.eligible_hits, 1);
        assert_eq!(out.fires, 0);
        assert_eq!(out.unattributed_fires, 1);
    }

    /// One transition is one fire starting, so two ribbons cannot both read it.
    /// Letting them would give the second ribbon an `actual` section belonging
    /// to the first fire while still labelling the pair independent evidence.
    #[test]
    fn two_ribbons_cannot_share_one_burn_transition() {
        let out = analyze(
            &fixture()
                .hitting_section(0)
                .also_hitting_section_at(3, 0.0)
                .server_lights_section(0)
                .with_stray_ribbon_at(HIT_CLOCK)
                .build(),
        )
        .expect("geometry");
        assert_eq!(out.eligible_hits, 2);
        assert_eq!(out.fires, 1);
        assert_eq!(out.unattributed_fires, 1);
        assert_eq!(out.section_predictions.len(), 1);
    }
}
