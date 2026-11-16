//! Reading an effective-fire-chance result out loud.
//!
//! The analysis itself is `wows_replay_insights::fire_chance`; this is the
//! wording both front ends put around it, so the same result reads the same
//! way in either app.
//!
//! The translation layer carries no plural machinery, so every count that can
//! be one takes a separate key, the way the session-stats labels do.

use std::borrow::Cow;
use std::collections::BTreeMap;

use rust_i18n::t;
use wows_replay_insights::fire_chance::analysis::EffectiveFireChance;
use wows_replay_insights::fire_chance::analysis::ExclusionReason;
use wows_replay_insights::fire_chance::analysis::FormulaOp;
use wows_replay_insights::fire_chance::analysis::PerShipFireChance;
use wows_replay_insights::fire_chance::analysis::UnattributedFireReason;
use wt_translations::keys;

/// "N fires", singular at one.
pub fn fires_text(fires: u32) -> Cow<'static, str> {
    if fires == 1 {
        return t!("ui.replay.sections.fire_chance_fires_one");
    }
    t!("ui.replay.sections.fire_chance_fires", fires = fires)
}

/// "N eligible hits", singular at one.
pub fn eligible_hits_text(hits: u32) -> Cow<'static, str> {
    if hits == 1 {
        return t!("ui.replay.sections.fire_chance_eligible_hits_one");
    }
    t!("ui.replay.sections.fire_chance_eligible_hits", hits = hits)
}

/// "N fires / M eligible hits", the counts a rate stands on.
///
/// Both figures carry their unit, because a bare pair of numbers says nothing
/// about which is which. The denominator says "eligible hits" rather than
/// "hits" so it reads as the same figure the breakdown's `eligible` row
/// states: a bare "hits" invites comparison against the hits on the ship,
/// which is a different and larger number.
pub fn counts_text(fires: u32, hits: u32) -> String {
    format!("{} / {}", fires_text(fires), eligible_hits_text(hits))
}

/// "across N target ships", singular at one.
pub fn ships_text(fire_chance: &EffectiveFireChance) -> Cow<'static, str> {
    let ships = fire_chance.ships_with_trials();
    if ships == 1 {
        return t!("ui.replay.sections.fire_chance_ships_one");
    }
    t!("ui.replay.sections.fire_chance_ships", ships = ships)
}

/// The expected-fires note beside the observed count, when there is one.
pub fn expected_fires_text(fire_chance: &EffectiveFireChance) -> Option<String> {
    let expected = fire_chance.expected_fires?;
    Some(t!("ui.replay.sections.fire_chance_expected_fires", fires = format!("{expected:.1}")).into_owned())
}

/// Sample-count headline: fires and eligible hits summed over every target
/// ship, and how many ships those totals cover.
///
/// Counts, not a rate. Fire resistance is a property of the victim, so a rate
/// only means something inside one target ship's row, where that victim's
/// `burnProb` coefficient and node probabilities are fixed. Reducing several
/// ships to one percentage would need a weighting between a ship hit twice
/// and one hit eighty times, and no weighting is the right one. Zero eligible
/// hits says so in words rather than showing a total nothing stands behind.
pub fn headline_text(fire_chance: &EffectiveFireChance) -> String {
    if fire_chance.eligible_hits == 0 {
        return t!("ui.replay.sections.fire_chance_no_eligible_hits").into_owned();
    }
    format!("{}   {}", counts_text(fire_chance.fires, fire_chance.eligible_hits), ships_text(fire_chance))
}

/// The headline plus the optional expected line beneath it, as plain text,
/// for a hover or the clipboard.
pub fn headline_lines(fire_chance: &EffectiveFireChance) -> Vec<String> {
    let mut lines = vec![headline_text(fire_chance)];
    if fire_chance.eligible_hits > 0
        && let Some(text) = expected_fires_text(fire_chance)
    {
        lines.push(format!("  {text}"));
    }
    lines
}

/// The per-ship rows, most-sampled first.
pub fn sorted_per_ship(fire_chance: &EffectiveFireChance) -> Vec<&PerShipFireChance> {
    let mut ships: Vec<&PerShipFireChance> = fire_chance.per_ship.iter().collect();
    ships.sort_by_key(|ship| std::cmp::Reverse(ship.eligible_hits));
    ships
}

/// The unit a count in the breakdown's left column carries, singular at one.
///
/// The count is printed apart from the label so a column of them lines up,
/// which is why these keys are bare nouns.
pub fn count_label(count: u32, plural: &'static str, singular: &'static str) -> Cow<'static, str> {
    t!(if count == 1 { singular } else { plural })
}

/// "N HE hits not attributable to a target ship", or `None` when every HE hit
/// on a ship landed on one the breakdown has a row for.
///
/// Derived here rather than read off the analysis, because the analysis
/// reports the remainder over every hit of ours and this listing is HE-only:
/// a hit keyed to our own ship, or to a player whose hull never resolved, is
/// counted in the aggregate HE line and carried by no row, and this is
/// exactly that difference.
pub fn no_target_ship_line(fire_chance: &EffectiveFireChance) -> Option<String> {
    let in_rows: u32 = fire_chance.per_ship.iter().map(|ship| ship.he_hits_on_a_ship).sum();
    let hits = fire_chance.he_hits_on_a_ship.saturating_sub(in_rows);
    if hits == 0 {
        return None;
    }
    let label = count_label(
        hits,
        "ui.replay.sections.fire_chance_no_target_ship",
        "ui.replay.sections.fire_chance_no_target_ship_one",
    );
    Some(format!("{hits} {label}"))
}

/// The whole effective-fire-chance breakdown as text: the attacker-side
/// formula, then what became of the HE shells and of the fire ribbons, then
/// the same accounting per target ship.
///
/// Built independently of which expanders are open, because this is what
/// clicking the headline copies and a reader pasting it elsewhere expects the
/// whole document.
pub fn breakdown_text(
    fire_chance: &EffectiveFireChance,
    localize_source: &dyn Fn(&str) -> String,
    localize_ship: &dyn Fn(&PerShipFireChance) -> String,
) -> String {
    let mut lines = fire_chance_formula_lines(fire_chance, localize_source);
    if !lines.is_empty() {
        lines.push(String::new());
    }
    lines.extend(fire_chance_breakdown_lines(fire_chance));
    let per_ship = fire_chance_per_ship_lines(fire_chance, localize_ship);
    if !per_ship.is_empty() {
        lines.push(String::new());
        lines.extend(per_ship);
    }
    lines.join("\n")
}

/// The full plain-text form for click-to-copy: the headline, then everything
/// either expander can show, whether or not it is open.
pub fn copy_text(
    fire_chance: &EffectiveFireChance,
    localize_source: &dyn Fn(&str) -> String,
    localize_ship: &dyn Fn(&PerShipFireChance) -> String,
) -> String {
    let mut lines = vec![t!("ui.replay.sections.fire_chance").into_owned()];
    lines.extend(headline_lines(fire_chance));
    lines.push(String::new());
    lines.push(breakdown_text(fire_chance, localize_source, localize_ship));
    lines.join("\n")
}

/// One target-ship row in plain text, the header of that ship's breakdown. The
/// on-screen expander lays the same figures out as a grid. This is the only
/// place a percentage is stated, because the victim's fire resistance is fixed
/// within the row. `PerShipFireChance::rate` is `None` over zero eligible hits,
/// which is unknown rather than a zero rate, and the expected column is the
/// matching per-hit chance so the two are comparable.
pub fn fire_chance_per_ship_line(
    ship: &PerShipFireChance,
    localize_ship: &dyn Fn(&PerShipFireChance) -> String,
) -> String {
    let rate_text = match ship.rate() {
        Some(rate) => format!("{:.1}%  {}", rate * 100.0, counts_text(ship.fires, ship.eligible_hits)),
        None => t!("ui.replay.sections.fire_chance_no_eligible_hits").into_owned(),
    };
    match ship.expected_rate() {
        Some(expected) => format!(
            "{}   {rate_text}   {} {:.1}%",
            localize_ship(ship),
            t!("ui.replay.sections.fire_chance_expected"),
            expected * 100.0
        ),
        None => format!("{}   {rate_text}", localize_ship(ship)),
    }
}

/// The attacker-side formula breakdown, in order: a base line for the shell's
/// raw `burnProb`, then each modifier step that moved the value, then the
/// resulting total. `localize_source` resolves a step's raw source identifier
/// to a display name (an equipped upgrade, signal or crew skill); passed in
/// rather than called directly so this function stays free of the metadata
/// provider and is testable with a stub. Empty when no shell resolved to
/// compute a formula from at all.
pub fn fire_chance_formula_lines(
    fire_chance: &EffectiveFireChance,
    localize_source: &dyn Fn(&str) -> String,
) -> Vec<String> {
    let (Some(base), Some((raw, clamped))) = (fire_chance.formula_base, fire_chance.formula_total()) else {
        return Vec::new();
    };
    let formula = &fire_chance.formula;

    let names: Vec<String> = formula
        .iter()
        .map(|step| match &step.source {
            Some(source) => format!("{} ({})", step.modifier, localize_source(source)),
            None => step.modifier.clone(),
        })
        .collect();
    let base_label = t!("ui.replay.sections.fire_chance_formula_base").into_owned();
    // 2-char slot for the step lines' "x "/"+ " prefix, so the base line's
    // label starts in the same column as a step's name even though it carries
    // no operator symbol of its own.
    let prefix_width = 2;
    let name_width = names
        .iter()
        .map(|name| name.chars().count())
        .chain(std::iter::once(base_label.chars().count()))
        .max()
        .unwrap_or(0);

    let mut lines = vec![t!("ui.replay.sections.fire_chance_formula").into_owned()];
    lines.push(format!("  {:prefix_width$}{base_label:<name_width$} {:.1}%", "", base * 100.0));
    for (step, name) in formula.iter().zip(&names) {
        let (symbol, value_text) = match step.op {
            FormulaOp::Multiply => ("x", format!("{:.2}", step.value)),
            FormulaOp::Add => ("+", format!("+{:.1}pp", step.value * 100.0)),
        };
        lines.push(format!("  {symbol} {name:<name_width$} {value_text}"));
    }

    // `formula_total` reports the raw product and the value the eligibility
    // model rolls with. Showing only the raw one would silently disagree with
    // the rate above wherever the clamp bites.
    if (raw - clamped).abs() > f32::EPSILON {
        lines.push(format!(
            "  = {:.1}%   ({} {:.1}%)",
            raw * 100.0,
            t!("ui.replay.sections.fire_chance_formula_clamped"),
            clamped * 100.0
        ));
    } else {
        lines.push(format!("  = {:.1}%", raw * 100.0));
    }
    lines
}

/// One population's counts as the breakdown reads them, taken from either the
/// whole battle or one target ship's row. Both carry the same figures, so the
/// two levels render through one function.
///
/// HE-only and ship-only throughout. Our AP and SAP hits, our secondaries and
/// the shells that struck terrain are none of them members of the population
/// this chain describes, so they appear nowhere in it; the analysis still counts
/// them, and its corpus checks still reconcile against them, but a listing whose
/// first line is a count of HE shells cannot state how many shells could not
/// burn without contradicting itself.
pub struct FireChanceTally<'a> {
    pub he_hits_on_a_ship: u32,
    pub eligible_hits: u32,
    pub exclusions: &'a BTreeMap<ExclusionReason, u32>,
    pub not_applicable: u32,
}

impl<'a> From<&'a EffectiveFireChance> for FireChanceTally<'a> {
    fn from(fire_chance: &'a EffectiveFireChance) -> FireChanceTally<'a> {
        FireChanceTally {
            he_hits_on_a_ship: fire_chance.he_hits_on_a_ship,
            eligible_hits: fire_chance.eligible_hits,
            exclusions: &fire_chance.exclusions,
            not_applicable: fire_chance.not_applicable,
        }
    }
}

impl<'a> From<&'a PerShipFireChance> for FireChanceTally<'a> {
    fn from(ship: &'a PerShipFireChance) -> FireChanceTally<'a> {
        FireChanceTally {
            he_hits_on_a_ship: ship.he_hits_on_a_ship,
            eligible_hits: ship.eligible_hits,
            exclusions: &ship.exclusions,
            not_applicable: ship.not_applicable,
        }
    }
}

/// One row of a count-and-label listing, at the depth it sits under the row
/// above it. The on-screen breakdown lays these out as a grid and the
/// clipboard form renders them as text, so both read the same rows and cannot
/// drift apart.
#[derive(Clone, Debug, PartialEq)]
pub struct TallyRow {
    pub depth: usize,
    pub count: u32,
    pub label: Cow<'static, str>,
}

impl TallyRow {
    pub fn new(depth: usize, count: u32, label: Cow<'static, str>) -> TallyRow {
        TallyRow { depth, count, label }
    }
}

/// Rows as plain text, counts right-aligned within their own depth so a nested
/// listing lines up independently of the wider figures above it.
///
/// `indent` is the leading whitespace the whole block sits under, so the
/// aggregate and the per-ship blocks share this function and differ only in
/// depth.
pub fn fire_chance_rows_to_lines(rows: &[TallyRow], indent: &str) -> Vec<String> {
    let mut widths: BTreeMap<usize, usize> = BTreeMap::new();
    for row in rows {
        let digits = row.count.to_string().len();
        let width = widths.entry(row.depth).or_insert(digits);
        *width = (*width).max(digits);
    }
    rows.iter()
        .map(|row| {
            let width = widths.get(&row.depth).copied().unwrap_or(1);
            let nesting = "  ".repeat(row.depth);
            format!("{indent}{nesting}{:>width$} {}", row.count, row.label)
        })
        .collect()
}

/// One population of our shells: how many HE hits landed on a ship, then the
/// eligibility model's own answer over them.
///
/// Eligible is pinned first, then the refusals by count descending with
/// zero-count reasons omitted, then the hits that were never the model's
/// question because the ship was already dead, apart from the refusals because
/// they are not one. The rows under the HE line sum to it exactly.
///
/// `heads` are the rows that sit above it at the same depth, which is where the
/// whole-battle block states its fired count. A per-ship block passes none: a
/// salvo is fired at the water rather than at a victim.
pub fn fire_chance_tally_rows(tally: &FireChanceTally<'_>, heads: &[TallyRow]) -> Vec<TallyRow> {
    let mut rows: Vec<TallyRow> = heads.to_vec();
    rows.push(TallyRow::new(
        0,
        tally.he_hits_on_a_ship,
        count_label(
            tally.he_hits_on_a_ship,
            "ui.replay.sections.fire_chance_he_hits",
            "ui.replay.sections.fire_chance_he_hits_one",
        ),
    ));
    rows.push(TallyRow::new(1, tally.eligible_hits, t!("ui.replay.sections.fire_chance_eligible")));

    let mut excluded: Vec<(&ExclusionReason, &u32)> =
        tally.exclusions.iter().filter(|(_, count)| **count > 0).collect();
    excluded.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    rows.extend(
        excluded.into_iter().map(|(reason, count)| TallyRow::new(1, *count, t!(keys::exclusion_reason_key(*reason)))),
    );
    if tally.not_applicable > 0 {
        rows.push(TallyRow::new(1, tally.not_applicable, t!("ui.replay.sections.fire_chance_not_applicable")));
    }
    rows
}

/// The whole battle's tally: HE shells fired, then the HE hits on a ship they
/// produced, then how those split.
pub fn fire_chance_battle_tally_rows(fire_chance: &EffectiveFireChance) -> Vec<TallyRow> {
    let fired = TallyRow::new(
        0,
        fire_chance.he_shells_fired,
        count_label(
            fire_chance.he_shells_fired,
            "ui.replay.sections.fire_chance_shells_fired",
            "ui.replay.sections.fire_chance_shells_fired_one",
        ),
    );
    fire_chance_tally_rows(&fire_chance.into(), std::slice::from_ref(&fired))
}

/// The `SetFire` ribbon accounting: every fire the game credited us with, split
/// into the ones a shell of ours could be named for and the ones that could not,
/// with a reason under each of the latter.
///
/// This is the block that answers "the game gave me six fires and this says
/// four". The two figures under the ribbon count sum to it by construction, so
/// the arithmetic is visible rather than asserted.
///
/// Whole-battle only. A `SetFire` ribbon names no victim, so an uncredited one
/// belongs to no target ship without inventing the assignment, and the
/// per-target-ship rows would have to state a total they cannot account for.
pub fn fire_chance_ribbon_rows(fire_chance: &EffectiveFireChance) -> Vec<TallyRow> {
    if fire_chance.set_fire_ribbons == 0 {
        return Vec::new();
    }
    let mut rows = vec![
        TallyRow::new(
            0,
            fire_chance.set_fire_ribbons,
            count_label(
                fire_chance.set_fire_ribbons,
                "ui.replay.sections.fire_chance_ribbons",
                "ui.replay.sections.fire_chance_ribbons_one",
            ),
        ),
        TallyRow::new(1, fire_chance.fires, t!("ui.replay.sections.fire_chance_ribbons_credited")),
    ];
    if fire_chance.unattributed_fires == 0 {
        return rows;
    }
    rows.push(TallyRow::new(
        1,
        fire_chance.unattributed_fires,
        t!("ui.replay.sections.fire_chance_ribbons_uncredited"),
    ));
    let mut reasons: Vec<(&UnattributedFireReason, &u32)> =
        fire_chance.unattributed_reasons.iter().filter(|(_, count)| **count > 0).collect();
    reasons.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    rows.extend(
        reasons
            .into_iter()
            .map(|(reason, count)| TallyRow::new(2, *count, t!(keys::unattributed_fire_reason_key(*reason)))),
    );
    rows
}

/// Shells fired, the hits they produced, how those split, and then what became
/// of the fire ribbons themselves. Top down, so the reader sees the whole
/// population before its parts.
pub fn fire_chance_breakdown_lines(fire_chance: &EffectiveFireChance) -> Vec<String> {
    let mut lines = fire_chance_rows_to_lines(&fire_chance_battle_tally_rows(fire_chance), "");
    let ribbons = fire_chance_ribbon_rows(fire_chance);
    if !ribbons.is_empty() {
        lines.push(String::new());
        lines.extend(fire_chance_rows_to_lines(&ribbons, ""));
    }
    lines
}

/// The same accounting per target ship, each row's rate over its own breakdown,
/// closed by the hits no target ship's row could carry.
///
/// There is no per-ship shells-fired line: a salvo is fired at the water rather
/// than at a victim, so the analysis states that count only once. Nor is there
/// a per-ship ribbon block: a `SetFire` ribbon names no victim.
pub fn fire_chance_per_ship_lines(
    fire_chance: &EffectiveFireChance,
    localize_ship: &dyn Fn(&PerShipFireChance) -> String,
) -> Vec<String> {
    if fire_chance.per_ship.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![t!("ui.replay.sections.fire_chance_per_ship").into_owned()];
    for ship in sorted_per_ship(fire_chance) {
        lines.push(format!("  {}", fire_chance_per_ship_line(ship, localize_ship)));
        lines.extend(fire_chance_rows_to_lines(&fire_chance_tally_rows(&ship.into(), &[]), "    "));
    }
    // Without this the rows silently fail to add up to the aggregate: a hit keyed
    // to the recording player's own ship, or to a player whose hull never
    // resolved, has no row to sit in.
    if let Some(line) = no_target_ship_line(fire_chance) {
        lines.push(format!("  {line}"));
    }
    lines
}

#[cfg(test)]
mod breakdown_tests {
    use super::*;
    use std::collections::BTreeMap;
    use wows_replay_insights::fire_chance::analysis::FormulaStep;
    // Only the tests name this: the analysis still counts our AP hits and our
    // splashes, and these check that none of it reaches the breakdown.
    use wows_replay_insights::fire_chance::analysis::NarrowingReason;

    fn fixture(eligible_hits: u32, fires: u32, expected_fires: Option<f32>) -> EffectiveFireChance {
        EffectiveFireChance {
            he_shells_fired: 0,
            hits: eligible_hits,
            narrowed: BTreeMap::new(),
            he_hits_on_a_ship: eligible_hits,
            hits_without_a_target_ship: 0,
            not_applicable: 0,
            eligible_hits,
            fires,
            expected_fires,
            per_ship: Vec::new(),
            exclusions: BTreeMap::new(),
            section_predictions: Vec::new(),
            set_fire_ribbons: fires,
            unattributed_fires: 0,
            unattributed_reasons: BTreeMap::new(),
            formula_base: None,
            formula: Vec::new(),
        }
    }

    /// A result carrying only a formula, for the breakdown lines. One eligible
    /// hit so the struct is a shape `analyze` could actually produce.
    fn formula_fixture(base: Option<f32>, formula: Vec<FormulaStep>) -> EffectiveFireChance {
        EffectiveFireChance { formula_base: base, formula, ..fixture(1, 0, None) }
    }

    fn formula_step(modifier: &str, source: Option<&str>, op: FormulaOp, value: f32, result: f32) -> FormulaStep {
        FormulaStep { modifier: modifier.to_owned(), source: source.map(str::to_owned), op, value, result }
    }

    /// Identity localizer: returns the source string unchanged, for tests that
    /// don't care about localization.
    fn no_localization(source: &str) -> String {
        source.to_owned()
    }

    fn ship(name: &str, eligible_hits: u32, fires: u32, expected_fires: Option<f32>) -> PerShipFireChance {
        PerShipFireChance {
            victim_ship_index: format!("{name}_INDEX"),
            victim_ship_name: name.to_owned(),
            hits: eligible_hits,
            he_hits_on_a_ship: eligible_hits,
            narrowed: BTreeMap::new(),
            eligible_hits,
            exclusions: BTreeMap::new(),
            not_applicable: 0,
            fires,
            expected_fires,
        }
    }

    /// The headline is counts and the ship count they cover. A percentage
    /// there would need a weighting across victims of different fire
    /// resistance, which is why the rate lives on the per-ship rows instead.
    #[test]
    fn headline_shows_counts_and_the_ships_they_cover() {
        let mut fc = fixture(63, 9, Some(6.3));
        fc.per_ship = vec![ship("Zao", 40, 6, None), ship("Iowa", 23, 3, None)];
        let headline = headline_text(&fc);
        assert!(!headline.contains('%'), "expected no percentage in {headline:?}");
        assert_eq!(headline, "9 fires / 63 hits that could have started one   across 2 target ships");
    }

    /// `expected_fires` is a count of fires, and the observed figure beside it
    /// is now a count too, so the line renders the count as it stands.
    #[test]
    fn headline_expected_line_renders_the_fire_count() {
        let mut fc = fixture(63, 9, Some(6.3));
        fc.per_ship = vec![ship("Zao", 63, 9, None)];
        assert_eq!(
            headline_lines(&fc),
            vec![
                "9 fires / 63 hits that could have started one   across 1 target ship".to_owned(),
                "  expected 6.3 fires".to_owned()
            ]
        );
    }

    /// With no eligible hits `expected_fires` is legitimately `Some(0.0)`: a
    /// sum over nothing. Nothing stands behind it, so no expected line is
    /// shown, exactly as no observed total is.
    #[test]
    fn headline_over_zero_eligible_hits_shows_no_expected_line() {
        let fc = fixture(0, 0, Some(0.0));
        assert_eq!(headline_lines(&fc), vec!["no hits that could have started a fire".to_owned()]);
    }

    /// Zero eligible hits is unknown, not zero: this must never render as a
    /// total or a rate.
    #[test]
    fn headline_over_zero_eligible_hits_shows_no_totals() {
        let fc = fixture(0, 0, None);
        let headline = headline_text(&fc);
        assert!(!headline.contains('%'), "expected no percentage in {headline:?}");
        assert_eq!(headline, "no hits that could have started a fire");
    }

    /// The whole shape in one place: HE shells fired, the HE hits on a ship
    /// they produced, and then the eligibility split. The hits that could have
    /// started a fire are pinned first, the refusals follow by count descending
    /// with zero-count reasons dropped, and the hits that were never in the
    /// population come last.
    #[test]
    fn the_breakdown_reads_fired_then_he_hits_then_the_split() {
        let mut fc = fixture(43, 4, None);
        fc.he_shells_fired = 171;
        fc.hits = 130;
        fc.he_hits_on_a_ship = 69;
        fc.narrowed.insert(NarrowingReason::ShellCannotBurn, 39);
        fc.narrowed.insert(NarrowingReason::ImpactNotOnAShip, 22);
        fc.not_applicable = 1;
        fc.exclusions.insert(ExclusionReason::SectionAlreadyBurning, 15);
        fc.exclusions.insert(ExclusionReason::DamageControlActive, 8);
        fc.exclusions.insert(ExclusionReason::ImpactUnplaceableOnVictim, 2);

        assert_eq!(
            fire_chance_breakdown_lines(&fc)[..7],
            [
                "171 HE shells fired".to_owned(),
                " 69 HE hits on a ship".to_owned(),
                "  43 could have started a fire".to_owned(),
                "  15 section already burning".to_owned(),
                "   8 Damage Control Party active".to_owned(),
                "   2 could not place the impact on the ship we matched it to".to_owned(),
                "   1 victim already dead, not applicable".to_owned(),
            ]
        );
    }

    /// The chain is HE-only and ship-only from top to bottom. An AP hit was
    /// never an HE shell and a splash in the water hit no ship, so neither has
    /// any business in a listing whose first line counts HE shells fired and
    /// whose last lines read as "these could have burned and did not".
    #[test]
    fn the_shell_and_terrain_filters_never_appear_in_the_breakdown() {
        let mut fc = fixture(43, 4, None);
        fc.hits = 130;
        fc.he_hits_on_a_ship = 69;
        fc.narrowed.insert(NarrowingReason::ShellCannotBurn, 39);
        fc.narrowed.insert(NarrowingReason::ImpactNotOnAShip, 22);
        fc.exclusions.insert(ExclusionReason::SectionAlreadyBurning, 26);

        let lines = fire_chance_breakdown_lines(&fc);
        assert!(!lines.iter().any(|line| line.contains("could not burn")), "got {lines:?}");
        assert!(!lines.iter().any(|line| line.contains("terrain")), "got {lines:?}");
        assert!(!lines.iter().any(|line| line.contains("130")), "got {lines:?}");
    }

    /// A victim that was never hit after it died contributes no row, rather
    /// than a zero one that reads as a category with nothing in it.
    #[test]
    fn the_breakdown_omits_the_not_applicable_row_when_it_is_empty() {
        let mut fc = fixture(2, 0, None);
        fc.he_shells_fired = 6;
        fc.hits = 2;
        let lines = fire_chance_breakdown_lines(&fc);
        assert!(!lines.iter().any(|line| line.contains("not applicable")), "got {lines:?}");
    }

    /// The head of the chain is the fired count and the HE hits on a ship, in
    /// that order and with nothing between them.
    #[test]
    fn the_breakdown_head_is_fired_then_he_hits_on_a_ship() {
        let mut fc = fixture(2, 0, None);
        fc.he_shells_fired = 6;
        fc.hits = 2;
        let lines = fire_chance_breakdown_lines(&fc);
        assert_eq!(lines[..2], ["6 HE shells fired".to_owned(), "2 HE hits on a ship".to_owned()]);
    }

    /// Each ship's own accounting sits under its row in the same shape, so the
    /// hover answers "why did so few hits on this ship count" without a second
    /// lookup. The row's denominator and the `eligible` line under it are the
    /// same figure, said the same way.
    #[test]
    fn the_per_ship_block_carries_each_ships_own_breakdown() {
        let mut fc = fixture(12, 2, None);
        let mut zao = ship("Zao", 12, 2, None);
        zao.hits = 20;
        zao.he_hits_on_a_ship = 17;
        zao.narrowed.insert(NarrowingReason::ShellCannotBurn, 3);
        zao.not_applicable = 3;
        zao.exclusions.insert(ExclusionReason::ObservationGap, 2);
        fc.per_ship = vec![zao];

        assert_eq!(
            fire_chance_per_ship_lines(&fc, &|s: &PerShipFireChance| s.victim_ship_name.clone()),
            vec![
                "Per Target Ship".to_owned(),
                "  Zao   16.7%  2 fires / 12 hits that could have started one".to_owned(),
                "    17 HE hits on a ship".to_owned(),
                "      12 could have started a fire".to_owned(),
                "       2 observation gap".to_owned(),
                "       3 victim already dead, not applicable".to_owned(),
            ]
        );
    }

    /// A shell that hit the water hit no ship, so it belongs to the whole
    /// battle's narrowing step and to no target ship's rows.
    #[test]
    fn no_per_ship_row_carries_a_terrain_count() {
        let mut fc = fixture(12, 2, None);
        fc.narrowed.insert(NarrowingReason::ImpactNotOnAShip, 22);
        fc.per_ship = vec![ship("Zao", 12, 2, None)];
        let lines = fire_chance_per_ship_lines(&fc, &|s: &PerShipFireChance| s.victim_ship_name.clone());
        assert!(!lines.iter().any(|line| line.contains("terrain")), "got {lines:?}");
    }

    /// The per-ship rows have to add back up to the aggregate, so the hits no
    /// row could carry are stated rather than dropped.
    #[test]
    fn the_per_ship_block_states_the_hits_no_row_carries() {
        let mut fc = fixture(12, 2, None);
        fc.he_hits_on_a_ship = 45;
        fc.per_ship = vec![ship("Zao", 12, 2, None)];
        let lines = fire_chance_per_ship_lines(&fc, &|s: &PerShipFireChance| s.victim_ship_name.clone());
        assert_eq!(lines.last().map(String::as_str), Some("  33 HE hits not attributable to a target ship"));
    }

    /// Same count-versus-rate rule as the headline: 12 hits expecting 1.656
    /// fires is a 13.8% per-hit rate. Both counts carry their unit, because a
    /// bare pair of numbers does not say which of them is which.
    #[test]
    fn per_ship_line_includes_expected_when_present() {
        let s = ship("Zao", 12, 2, Some(1.656));
        assert_eq!(
            fire_chance_per_ship_line(&s, &|s: &PerShipFireChance| s.victim_ship_name.clone()),
            "Zao   16.7%  2 fires / 12 hits that could have started one   expected 13.8%"
        );
    }

    #[test]
    fn per_ship_line_omits_expected_when_absent() {
        let s = ship("Iowa", 11, 1, None);
        assert_eq!(
            fire_chance_per_ship_line(&s, &|s: &PerShipFireChance| s.victim_ship_name.clone()),
            "Iowa   9.1%  1 fire / 11 hits that could have started one"
        );
    }

    /// One fire is one fire, not "1 fires", and one hit is one hit.
    #[test]
    fn counts_of_one_are_singular() {
        assert_eq!(counts_text(1, 1), "1 fire / 1 hit that could have started one");
        assert_eq!(counts_text(0, 2), "0 fires / 2 hits that could have started one");
    }

    /// The same shape over a real match's counts, taken from
    /// `20260309_140531_PGSC720-Bremen_s02_Naval_Defense.wowsreplay` as the
    /// corpus harness reports them. A fixture can be made to render anything;
    /// this is what the block actually says about a replay with 1276 hits in it.
    #[test]
    fn the_breakdown_renders_a_real_replays_counts() {
        let mut fc = fixture(202, 11, None);
        fc.he_shells_fired = 2312;
        fc.hits = 1276;
        fc.he_hits_on_a_ship = 1132;
        fc.narrowed.insert(NarrowingReason::NotMainBattery, 97);
        fc.narrowed.insert(NarrowingReason::ImpactNotOnAShip, 47);
        fc.not_applicable = 100;
        fc.exclusions.insert(ExclusionReason::SectionAlreadyBurning, 320);
        fc.exclusions.insert(ExclusionReason::MergedSectionVictimBuildUnknown, 257);
        fc.exclusions.insert(ExclusionReason::AmbiguousWithAnotherHit, 171);
        fc.exclusions.insert(ExclusionReason::DamageControlActive, 59);
        fc.exclusions.insert(ExclusionReason::ImpactUnplaceableOnVictim, 23);

        assert_eq!(
            fire_chance_breakdown_lines(&fc)[..9],
            [
                "2312 HE shells fired".to_owned(),
                "1132 HE hits on a ship".to_owned(),
                "  202 could have started a fire".to_owned(),
                "  320 section already burning".to_owned(),
                "  257 victim build unknown, fire zones may be merged".to_owned(),
                "  171 ambiguous with another hit of ours".to_owned(),
                "   59 Damage Control Party active".to_owned(),
                "   23 could not place the impact on the ship we matched it to".to_owned(),
                "  100 victim already dead, not applicable".to_owned(),
            ]
        );
        assert_eq!(fc.eligible_hits + fc.exclusions.values().sum::<u32>() + fc.not_applicable, fc.he_hits_on_a_ship);
    }

    /// The block that answers "the game gave me six fires and this says four".
    /// The ribbon count is the game's own figure, the two lines under it are
    /// what became of them, and the reasons under those say why each uncredited
    /// one could not be tied to a shell.
    #[test]
    fn the_ribbon_block_accounts_for_every_fire_the_game_gave_us() {
        let mut fc = fixture(43, 4, None);
        fc.set_fire_ribbons = 6;
        fc.unattributed_fires = 2;
        fc.unattributed_reasons.insert(UnattributedFireReason::NoHitInWindow, 1);
        fc.unattributed_reasons.insert(UnattributedFireReason::EveryNearbyHitExcluded, 1);

        let lines = fire_chance_breakdown_lines(&fc);
        let ribbons = &lines[lines.len() - 5..];
        assert_eq!(
            ribbons,
            [
                "6 SetFire ribbons".to_owned(),
                "  4 credited to a shell".to_owned(),
                "  2 not credited".to_owned(),
                "    1 every nearby hit of ours was already excluded".to_owned(),
                "    1 no hit of ours landed in the window".to_owned(),
            ]
        );
    }

    /// The arithmetic the block exists to make visible: credited plus
    /// uncredited is the ribbon count, and the reasons account for the
    /// uncredited ones exactly.
    #[test]
    fn the_ribbon_block_adds_up() {
        let mut fc = fixture(43, 4, None);
        fc.set_fire_ribbons = 6;
        fc.unattributed_fires = 2;
        fc.unattributed_reasons.insert(UnattributedFireReason::BurnStateNotObserved, 2);
        let rows = fire_chance_ribbon_rows(&fc);
        assert_eq!(rows[0].count, fc.fires + fc.unattributed_fires);
        assert_eq!(rows.iter().filter(|row| row.depth == 1).map(|row| row.count).sum::<u32>(), rows[0].count);
        assert_eq!(rows.iter().filter(|row| row.depth == 2).map(|row| row.count).sum::<u32>(), fc.unattributed_fires);
    }

    /// Every ribbon credited leaves nothing to explain, so the block stops at
    /// the two lines that state as much rather than showing an empty heading.
    #[test]
    fn the_ribbon_block_omits_the_reasons_when_every_fire_was_credited() {
        let mut fc = fixture(43, 4, None);
        fc.set_fire_ribbons = 4;
        let lines = fire_chance_breakdown_lines(&fc);
        assert_eq!(&lines[lines.len() - 2..], ["4 SetFire ribbons".to_owned(), "  4 credited to a shell".to_owned()]);
    }

    /// A player who started no fires at all has no ribbon accounting to show,
    /// and a block of zeroes would read as a category with nothing in it.
    #[test]
    fn the_ribbon_block_is_absent_without_ribbons() {
        let fc = fixture(43, 0, None);
        assert!(fire_chance_ribbon_rows(&fc).is_empty());
        assert!(!fire_chance_breakdown_lines(&fc).iter().any(|line| line.contains("SetFire")));
    }

    /// The key match is exhaustive, so a stale arm is a compile error, but a key
    /// naming a string the toml does not carry is not. `rust-i18n` returns the
    /// key itself when it cannot resolve one, which is what this catches.
    #[test]
    fn every_breakdown_key_resolves_to_a_string() {
        const EXCLUSIONS: [ExclusionReason; 12] = [
            ExclusionReason::SectionAlreadyBurning,
            ExclusionReason::MergedSectionVictimBuildUnknown,
            ExclusionReason::DamageControlActive,
            ExclusionReason::DamageControlUnknown,
            ExclusionReason::ObservationGap,
            ExclusionReason::ConsumableModelUnreliable,
            ExclusionReason::VictimFateUnknown,
            ExclusionReason::HitTypeDoesNotRoll,
            ExclusionReason::NoSectionGeometry,
            ExclusionReason::ImpactUnplaceableOnVictim,
            ExclusionReason::VictimPoseUnknown,
            ExclusionReason::AmbiguousWithAnotherHit,
        ];
        const UNATTRIBUTED: [UnattributedFireReason; 7] = [
            UnattributedFireReason::BurnStateNotObserved,
            UnattributedFireReason::AlreadyCreditedToAnEarlierFire,
            UnattributedFireReason::ContestedByOurSecondary,
            UnattributedFireReason::ContestedByAnotherHitOfOurs,
            UnattributedFireReason::EveryNearbyHitExcluded,
            UnattributedFireReason::NoNearbyHitCouldStartAFire,
            UnattributedFireReason::NoHitInWindow,
        ];
        const LABELS: [&str; 13] = [
            "ui.replay.sections.fire_chance_breakdown",
            "ui.replay.sections.fire_chance_shells_fired",
            "ui.replay.sections.fire_chance_shells_fired_one",
            "ui.replay.sections.fire_chance_he_hits",
            "ui.replay.sections.fire_chance_he_hits_one",
            "ui.replay.sections.fire_chance_no_target_ship",
            "ui.replay.sections.fire_chance_no_target_ship_one",
            "ui.replay.sections.fire_chance_eligible",
            "ui.replay.sections.fire_chance_not_applicable",
            "ui.replay.sections.fire_chance_ribbons",
            "ui.replay.sections.fire_chance_ribbons_one",
            "ui.replay.sections.fire_chance_ribbons_credited",
            "ui.replay.sections.fire_chance_ribbons_uncredited",
        ];

        let mut keys: Vec<&'static str> = EXCLUSIONS.iter().map(|r| keys::exclusion_reason_key(*r)).collect();
        keys.extend(UNATTRIBUTED.iter().map(|r| keys::unattributed_fire_reason_key(*r)));
        keys.extend(LABELS);
        for key in keys {
            assert_ne!(t!(key), key, "{key} resolves to nothing");
        }
    }

    /// A row over no hits states no rate, including in the expected column: a
    /// sum over no hits implies no rate.
    #[test]
    fn per_ship_line_over_zero_eligible_hits_has_no_percentage() {
        let s = ship("Fletcher", 0, 0, Some(0.0));
        let line = fire_chance_per_ship_line(&s, &|s: &PerShipFireChance| s.victim_ship_name.clone());
        assert!(!line.contains('%'), "expected no percentage in {line:?}");
        assert_eq!(line, "Fletcher   no hits that could have started a fire");
    }

    #[test]
    fn sorted_per_ship_orders_by_eligible_hits_descending() {
        let mut fc = fixture(23, 3, None);
        fc.per_ship = vec![ship("Iowa", 11, 1, None), ship("Zao", 12, 2, None)];
        let names: Vec<&str> = sorted_per_ship(&fc).into_iter().map(|s| s.victim_ship_name.as_str()).collect();
        assert_eq!(names, vec!["Zao", "Iowa"]);
    }

    /// No shell ever resolved to compute a formula from, so there is nothing
    /// to show at all: not even a base line.
    #[test]
    fn formula_lines_are_empty_without_a_base() {
        assert!(fire_chance_formula_lines(&formula_fixture(None, Vec::new()), &no_localization).is_empty());
    }

    /// A resolved shell whose modifiers are all identities still shows the
    /// base and a total, even though there are no steps under it.
    #[test]
    fn formula_lines_show_the_base_even_with_no_steps() {
        let lines = fire_chance_formula_lines(&formula_fixture(Some(0.12), Vec::new()), &no_localization);
        assert_eq!(
            lines,
            vec![
                "Attacker fire chance formula".to_owned(),
                "    base burnProb 12.0%".to_owned(),
                "  = 12.0%".to_owned()
            ]
        );
    }

    /// Each step's source is localized, the value column reflects the op
    /// (a bare multiplier vs. a "+X.Ypp" bonus), and the total is the last
    /// step's running result.
    #[test]
    fn formula_lines_show_each_localized_step_and_the_total() {
        let formula = vec![
            formula_step("burnChanceFactorHighLevel", Some("ifhe_id"), FormulaOp::Multiply, 0.5, 0.06),
            formula_step("artilleryBurnChanceBonus", Some("de_id"), FormulaOp::Add, 0.01, 0.07),
        ];
        let localize = |source: &str| match source {
            "ifhe_id" => "IFHE".to_owned(),
            "de_id" => "DE".to_owned(),
            other => other.to_owned(),
        };
        let lines = fire_chance_formula_lines(&formula_fixture(Some(0.12), formula), &localize);
        assert_eq!(
            lines,
            vec![
                "Attacker fire chance formula".to_owned(),
                "    base burnProb                    12.0%".to_owned(),
                "  x burnChanceFactorHighLevel (IFHE) 0.50".to_owned(),
                "  + artilleryBurnChanceBonus (DE)    +1.0pp".to_owned(),
                "  = 7.0%".to_owned(),
            ]
        );
    }

    /// A step with no source renders its bare modifier name, unparenthesized.
    #[test]
    fn formula_lines_render_an_unsourced_step_without_parens() {
        let formula = vec![formula_step("burnProbModifier", None, FormulaOp::Multiply, 1.5, 0.18)];
        let lines = fire_chance_formula_lines(&formula_fixture(Some(0.12), formula), &no_localization);
        assert_eq!(lines[1], "    base burnProb    12.0%");
        assert_eq!(lines[2], "  x burnProbModifier 1.50");
        assert_eq!(lines[3], "  = 18.0%");
    }

    /// The eligibility model and `expected_fires` both read the clamped
    /// chance, so when the raw formula total runs past 100% the hover must
    /// show both values rather than only the disagreeing raw one.
    #[test]
    fn formula_lines_show_both_raw_and_clamped_when_they_disagree() {
        let formula = vec![formula_step("someBonus", None, FormulaOp::Add, 0.3, 1.2)];
        let lines = fire_chance_formula_lines(&formula_fixture(Some(0.9), formula), &no_localization);
        let total = lines.last().expect("a total line");
        assert!(total.contains("120.0%"), "got {total:?}");
        assert!(total.contains("100.0%"), "got {total:?}");
    }

    /// The raw and clamped values agree in the ordinary case, so only one
    /// number is shown.
    #[test]
    fn formula_lines_show_one_value_when_raw_and_clamped_agree() {
        let formula = vec![formula_step("someBonus", None, FormulaOp::Add, 0.01, 0.13)];
        let lines = fire_chance_formula_lines(&formula_fixture(Some(0.12), formula), &no_localization);
        assert_eq!(lines.last(), Some(&"  = 13.0%".to_owned()));
    }

    /// Column width is measured in characters, not bytes: a multi-byte
    /// localized name must not be padded as if it were wider than it displays.
    #[test]
    fn formula_lines_pad_by_character_count_not_byte_length() {
        // Ten U+00E9 ("e" with acute accent): 10 characters, but 20 bytes in
        // UTF-8, so it is longer than "base burnProb" (13 characters) by
        // byte count but shorter by character count. A width computed from
        // `.len()` would inflate the column to 20; the correct, char-counted
        // width is 13, driven by "base burnProb" instead.
        let name = "\u{e9}".repeat(10);
        let formula = vec![formula_step(&name, None, FormulaOp::Multiply, 1.0, 0.12)];
        let lines = fire_chance_formula_lines(&formula_fixture(Some(0.12), formula), &no_localization);
        assert_eq!(lines[1], "    base burnProb 12.0%");
        assert_eq!(lines[2], format!("  x {name}    1.00"));
    }
}

#[cfg(test)]
mod tests {
    use super::counts_text;
    use super::eligible_hits_text;
    use super::fires_text;

    /// A count of one reads in the singular, because the catalog has no
    /// plural machinery to do it.
    #[test]
    fn a_count_of_one_reads_in_the_singular() {
        assert_eq!(fires_text(1), "1 fire");
        assert_eq!(fires_text(2), "2 fires");
        assert_eq!(eligible_hits_text(1), "1 hit that could have started one");
        assert_eq!(eligible_hits_text(9), "9 hits that could have started one");
    }

    /// Both figures carry their unit: a bare pair says nothing about which is
    /// which.
    #[test]
    fn the_counts_line_names_both_of_its_figures() {
        let text = counts_text(3, 40);
        assert!(text.contains("3 fires"), "got {text:?}");
        assert!(text.contains("40 hits"), "got {text:?}");
    }
}
