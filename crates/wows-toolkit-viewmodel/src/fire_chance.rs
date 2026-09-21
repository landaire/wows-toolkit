//! Reading an effective-fire-chance result out loud.
//!
//! The analysis itself is `wows_replay_insights::fire_chance`; this is the
//! wording both front ends put around it, so the same result reads the same
//! way in either app.
//!
//! The translation layer carries no plural machinery, so every count that can
//! be one takes a separate key, the way the session-stats labels do.

use std::borrow::Cow;

use rust_i18n::t;
use wows_replay_insights::fire_chance::analysis::EffectiveFireChance;
use wows_replay_insights::fire_chance::analysis::PerShipFireChance;

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
