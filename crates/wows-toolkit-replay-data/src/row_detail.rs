//! What an expanded player row shows under each column, as data: section
//! contents, labels, tooltips, ordering, and the widths the table grows a
//! column to so the detail is not clipped. The front ends lay it out.

use rust_i18n::t;
use wows_replay_insights::battle_report::AchievementResult;
use wows_replay_insights::battle_report::ConsumableResult;
use wows_replay_insights::battle_report::DamageInteraction;
use wows_replay_insights::battle_report::RibbonResult;
use wows_replay_insights::battle_report::TranslatedModule;
use wows_replay_insights::fire_chance::analysis::PerShipFireChance;
use wows_toolkit_viewmodel::breakdown::BreakdownRow;
use wows_toolkit_viewmodel::fire_chance;
use wowsunpack::game_params::skill_grid_data::SkillGridRow;
use wowsunpack::game_params::skill_grid_data::SkillGridSkill;
use wowsunpack::game_types::ChargeCount;

use crate::cells::NDA;
use crate::columns::ReplayColumn;
use crate::model::PlayerRow;
use crate::model::separate_number;
use crate::player_table::TextMeasure;
use crate::player_table::TextWeight;

/// Achievement and subribbon icons, fitted in this square.
pub const LARGE_ICON_SIZE: f32 = 32.0;
/// Ribbon icons, fitted in this square (wide ribbons stay short).
pub const RIBBON_ICON_SIZE: f32 = 64.0;
/// Modernization, signal and captain-skill icons.
pub const MODULE_ICON_SIZE: f32 = 28.0;
/// The icon beside a consumable's name.
pub const CONSUMABLE_ICON_SIZE: f32 = 20.0;
/// Between an icon and its label.
pub const ICON_LABEL_GAP: f32 = 8.0;
/// Between a breakdown grid's two columns.
pub const BREAKDOWN_GAP: f32 = 8.0;
/// The fire glyph, the copy button and the gaps around them, which is what
/// the fire-chance headline holds besides its two figures.
pub const HEADLINE_CHROME: f32 = 56.0;
/// Room for the widest count a fire-chance tally row states.
pub const TALLY_COUNT_WIDTH: f32 = 44.0;
/// How far a tally label is indented per depth.
pub const TALLY_INDENT: f32 = 12.0;
/// Room for a ship name beside its fire-chance counts.
pub const FIRE_CHANCE_SHIP_WIDTH: f32 = 160.0;
/// The captain-skill grid's point-cost label.
pub const SKILL_COST_WIDTH: f32 = 14.0;
/// A skill the captain did not learn.
pub const UNLEARNED_SKILL_OPACITY: f32 = 0.35;
/// The consumables table's name column and its two count columns.
pub const CONSUMABLE_NAME_WIDTH: f32 = 170.0;
pub const CONSUMABLE_COUNT_WIDTH: f32 = 64.0;

/// The total of a consumable with no charge limit.
pub const UNLIMITED: &str = "Unlimited";

/// Whether a damage-interaction line is damage this row dealt to, or received
/// from, the other player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageDirection {
    Dealt,
    Received,
}

/// The remaining and total charge text of one consumable, and its tooltip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsumableCounts {
    pub remaining: String,
    pub total: String,
    pub hover: String,
}

/// A build module or signal as its icon cell reads: the name shown in place
/// of a missing icon, and the tooltip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleText {
    pub name: String,
    pub tooltip: String,
}

/// The Name column has detail: achievements, ribbons, damage events or the
/// fire chance.
pub fn has_name_section(row: &PlayerRow) -> bool {
    !row.achievements.is_empty() || !row.ribbons.is_empty() || has_damage_events(row) || row.fire_chance.is_some()
}

pub fn has_damage_events(row: &PlayerRow) -> bool {
    row.fires.is_some() || row.floods.is_some() || row.citadels.is_some() || row.crits.is_some()
}

/// The figures of the row are hidden behind the NDA placeholder.
pub fn nda_hidden(row: &PlayerRow, debug: bool) -> bool {
    row.should_hide_stats() && !debug
}

/// The damage events a row records, one line per event kind the battle
/// produced.
pub fn damage_event_rows(row: &PlayerRow) -> Vec<BreakdownRow> {
    let events = [
        (t!("ui.replay.column.fires"), row.fires),
        (t!("ui.replay.column.floods"), row.floods),
        (t!("ui.replay.column.citadels"), row.citadels),
        (t!("ui.replay.column.crits"), row.crits),
    ];
    events
        .into_iter()
        .filter_map(|(label, count)| {
            count.map(|count| BreakdownRow { label: label.into_owned(), value: separate_number(count) })
        })
        .collect()
}

/// `" (Nx)"` once an achievement was earned more than once.
pub fn achievement_label(achievement: &AchievementResult) -> String {
    if achievement.count > 1 {
        format!("{} ({}x)", achievement.display_name, achievement.count)
    } else {
        achievement.display_name.clone()
    }
}

pub fn ribbon_label(ribbon: &RibbonResult) -> String {
    format!("{} ({}x)", ribbon.display_name, ribbon.count)
}

/// The square a ribbon's icon is fitted in.
pub fn ribbon_icon_box(ribbon: &RibbonResult) -> f32 {
    if ribbon.is_subribbon { LARGE_ICON_SIZE } else { RIBBON_ICON_SIZE }
}

/// The row's ribbons in display order: by name, with the torpedo-protection
/// ribbon right after the main-battery one.
pub fn sorted_ribbons(row: &PlayerRow) -> Vec<&RibbonResult> {
    let mut ribbons: Vec<&RibbonResult> = row.ribbons.iter().collect();
    ribbons.sort_by(|a, b| a.name.cmp(&b.name));
    reorder_bulge_after_main_caliber(&mut ribbons);

    ribbons
}

/// One-off ported verbatim from the egui app: RIBBON_BULGE (torpedo
/// protection) goes immediately after RIBBON_MAIN_CALIBER, if both are
/// present.
pub fn reorder_bulge_after_main_caliber(ribbons: &mut Vec<&RibbonResult>) {
    let Some(main_caliber_idx) = ribbons.iter().position(|r| r.name == "RIBBON_MAIN_CALIBER") else {
        return;
    };
    let Some(bulge_idx) = ribbons.iter().position(|r| r.name == "RIBBON_BULGE") else {
        return;
    };

    let bulge = ribbons.remove(bulge_idx);
    let insert_idx = if bulge_idx < main_caliber_idx { main_caliber_idx } else { main_caliber_idx + 1 };
    ribbons.insert(insert_idx, bulge);
}

/// An icon's width and height, in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IconSize {
    pub width: f32,
    pub height: f32,
}

impl IconSize {
    pub const fn square(edge: f32) -> Self {
        Self { width: edge, height: edge }
    }
}

/// Scales a `native` size to fit within a `max` by `max` box keeping the
/// aspect ratio, so a wide ribbon renders short. Non-positive sizes fall back
/// to a `max` square.
pub fn fit_dims(native: IconSize, max: f32) -> IconSize {
    let IconSize { width, height } = native;
    if width <= 0.0 || height <= 0.0 {
        return IconSize::square(max);
    }

    let scale = (max / width).min(max / height);

    IconSize { width: width * scale, height: height * scale }
}

/// A modernization or signal: its name (the game-params name when untranslated)
/// and `"name\n\ndescription"` as its tooltip.
pub fn module_text(module: &TranslatedModule) -> ModuleText {
    let name = module.name.clone().unwrap_or_else(|| module.game_params_name.clone());
    let tooltip = match module.description.as_deref() {
        Some(description) if !description.is_empty() => format!("{name}\n\n{description}"),
        _ => name.clone(),
    };

    ModuleText { name, tooltip }
}

pub fn skill_display_name(skill: &SkillGridSkill) -> String {
    skill.name.clone().unwrap_or_else(|| skill.internal_name.as_str().to_owned())
}

/// `"name (N pt)\n\ndescription"`.
pub fn skill_tooltip(skill: &SkillGridSkill) -> String {
    let name = skill_display_name(skill);
    let cost = skill.point_cost.map(|cost| format!(" ({} pt)", cost.get())).unwrap_or_default();
    match skill.description.as_deref() {
        Some(description) if !description.is_empty() => format!("{name}{cost}\n\n{description}"),
        _ => format!("{name}{cost}"),
    }
}

/// `"(cost) name"`, shown in place of a missing skill icon.
pub fn skill_fallback_label(skill: &SkillGridSkill) -> String {
    let name = skill_display_name(skill);
    match skill.point_cost {
        Some(cost) => format!("({}) {name}", cost.get()),
        None => name,
    }
}

/// A skill tier's point-cost label. A tier of unknown cost has none.
pub fn skill_tier_label(tier: &SkillGridRow) -> String {
    tier.point_cost.map(|cost| cost.get().to_string()).unwrap_or_default()
}

/// The consumable inventory's counts. An unlimited consumable has no
/// remaining charges, so that column shows the activation count, as the egui
/// app does.
pub fn consumable_counts(consumable: &ConsumableResult) -> ConsumableCounts {
    match consumable.total_charges {
        ChargeCount::Unlimited => ConsumableCounts {
            remaining: consumable.charges_used.to_string(),
            total: UNLIMITED.to_owned(),
            hover: format!("Used: {}", consumable.charges_used),
        },
        ChargeCount::Finite(total) => {
            let remaining = total.saturating_sub(consumable.charges_used);
            ConsumableCounts {
                remaining: remaining.to_string(),
                total: total.to_string(),
                hover: format!("Remaining: {remaining} / Total: {total}"),
            }
        }
    }
}

pub fn interaction_amount(interaction: &DamageInteraction, direction: DamageDirection) -> u64 {
    match direction {
        DamageDirection::Dealt => interaction.damage_dealt,
        DamageDirection::Received => interaction.damage_received,
    }
}

/// The share this line is of a total. Normally of this row's own total; with
/// alt down, of the other player's, which answers "how much of what they took
/// was me" rather than "how much of my damage went there".
pub fn interaction_percentage(interaction: &DamageInteraction, direction: DamageDirection, alt_held: bool) -> f64 {
    match (direction, alt_held) {
        (DamageDirection::Dealt, false) => interaction.damage_dealt_percentage,
        (DamageDirection::Dealt, true) => interaction.damage_dealt_inverse_percentage,
        (DamageDirection::Received, false) => interaction.damage_received_percentage,
        (DamageDirection::Received, true) => interaction.damage_received_inverse_percentage,
    }
}

/// The row's breakdown for one damage direction.
pub fn damage_breakdown(row: &PlayerRow, direction: DamageDirection) -> Option<&[BreakdownRow]> {
    match direction {
        DamageDirection::Dealt => row.actual_damage_breakdown.as_deref(),
        DamageDirection::Received => row.received_damage_breakdown.as_deref(),
    }
}

/// One line per other player this row dealt damage to (or received it from),
/// largest first: their ship and `"amount (pct%)"`. Players `all_rows` does not
/// name and zero amounts are left out.
pub fn damage_target_rows(
    row: &PlayerRow,
    all_rows: &[PlayerRow],
    direction: DamageDirection,
    alt_held: bool,
) -> Vec<BreakdownRow> {
    let Some(interactions) = row.damage_interactions.as_ref() else {
        return Vec::new();
    };

    let mut entries: Vec<_> = interactions.iter().collect();
    entries.sort_by_key(|(_, interaction)| std::cmp::Reverse(interaction_amount(interaction, direction)));
    entries
        .into_iter()
        .filter_map(|(account_id, interaction)| {
            let amount = interaction_amount(interaction, direction);
            if amount == 0 {
                return None;
            }
            let other = all_rows.iter().find(|r| r.db_id == *account_id)?;
            let pct = interaction_percentage(interaction, direction, alt_held);
            Some(BreakdownRow {
                label: other.ship_name.clone(),
                value: format!("{} ({pct:.0}%)", separate_number(amount)),
            })
        })
        .collect()
}

/// A fire-chance modifier's raw source identifier, unresolved: resolving it
/// needs the metadata provider, which the table is not handed.
pub fn raw_source(source: &str) -> String {
    source.to_owned()
}

/// A victim's internal ship name, which the per-ship rows show for the same
/// reason.
pub fn raw_ship_name(ship: &PerShipFireChance) -> String {
    ship.victim_ship_name.clone()
}

/// One column's expanded-detail width for `row`, measured so the table can
/// grow the column to fit an expanded row. The arms follow what each front
/// end draws under the column. Modernizations and signals wrap rather than
/// widen, so they never drive the width. `all_rows` names the other players
/// of the damage columns' per-ship lines.
pub fn expanded_detail_width(
    col: ReplayColumn,
    row: &PlayerRow,
    all_rows: &[PlayerRow],
    debug: bool,
    measure: &mut impl TextMeasure,
) -> f32 {
    match col {
        ReplayColumn::Name => name_section_width(row, debug, measure),
        ReplayColumn::Skills => build_section_width(row, debug, measure),
        ReplayColumn::ActualDamage => damage_section_width(row, all_rows, debug, DamageDirection::Dealt, measure),
        ReplayColumn::ReceivedDamage => damage_section_width(row, all_rows, debug, DamageDirection::Received, measure),
        ReplayColumn::PotentialDamage => {
            nda_gated_breakdown_width(row.potential_damage_breakdown.as_deref(), row, debug, measure)
        }
        ReplayColumn::Hits => nda_gated_breakdown_width(row.hits_breakdown.as_deref(), row, debug, measure),
        ReplayColumn::SpottingDamage => {
            row.spotting_damage_breakdown.as_deref().map(|rows| breakdown_grid_width(rows, measure)).unwrap_or(0.0)
        }
        _ => 0.0,
    }
}

/// The ammo-type breakdown and the per-ship lines, with either share alt
/// shows; the NDA placeholder when the figures are hidden.
fn damage_section_width(
    row: &PlayerRow,
    all_rows: &[PlayerRow],
    debug: bool,
    direction: DamageDirection,
    measure: &mut impl TextMeasure,
) -> f32 {
    let breakdown = nda_gated_breakdown_width(damage_breakdown(row, direction), row, debug, measure);
    if nda_hidden(row, debug) {
        return breakdown;
    }

    [false, true]
        .into_iter()
        .map(|alt_held| breakdown_grid_width(&damage_target_rows(row, all_rows, direction, alt_held), measure))
        .fold(breakdown, f32::max)
}

/// The widest line of a newline-separated block.
fn multiline_width(text: &str, measure: &mut impl TextMeasure) -> f32 {
    text.split('\n').map(|line| measure.width(line, TextWeight::Regular)).fold(0.0_f32, f32::max)
}

/// The NDA placeholder's width when stats are hidden, otherwise the grid's.
fn nda_gated_breakdown_width(
    rows: Option<&[BreakdownRow]>,
    row: &PlayerRow,
    debug: bool,
    measure: &mut impl TextMeasure,
) -> f32 {
    if nda_hidden(row, debug) {
        return measure.width(NDA, TextWeight::Regular);
    }

    rows.map(|rows| breakdown_grid_width(rows, measure)).unwrap_or(0.0)
}

/// The widest label, the gap between the grid's columns, and the widest value.
pub fn breakdown_grid_width(rows: &[BreakdownRow], measure: &mut impl TextMeasure) -> f32 {
    if rows.is_empty() {
        return 0.0;
    }

    let mut labels = 0.0_f32;
    let mut values = 0.0_f32;
    for row in rows {
        labels = labels.max(measure.width(&row.label, TextWeight::Regular));
        values = values.max(measure.width(&row.value, TextWeight::Regular));
    }

    labels + BREAKDOWN_GAP + values
}

/// The section headings, each achievement and ribbon line, the damage-event
/// grid (or the NDA placeholder) and the fire-chance block.
fn name_section_width(row: &PlayerRow, debug: bool, measure: &mut impl TextMeasure) -> f32 {
    if !has_name_section(row) {
        return 0.0;
    }

    let mut max_w: f32 = 0.0;

    if !row.achievements.is_empty() {
        max_w = max_w.max(measure.width(&t!("ui.replay.sections.achievements"), TextWeight::Bold));
        for achievement in &row.achievements {
            let label_w = measure.width(&achievement_label(achievement), TextWeight::Regular);
            max_w = max_w.max(icon_label_row_width(LARGE_ICON_SIZE, label_w));
        }
    }

    if !row.ribbons.is_empty() {
        max_w = max_w.max(measure.width(&t!("ui.replay.sections.ribbons"), TextWeight::Bold));
        for ribbon in &row.ribbons {
            // A fitted wide ribbon tops out at its box size, so the box is a
            // safe bound.
            let label_w = measure.width(&ribbon_label(ribbon), TextWeight::Regular);
            max_w = max_w.max(icon_label_row_width(ribbon_icon_box(ribbon), label_w));
        }
    }

    if has_damage_events(row) {
        max_w = max_w.max(measure.width(&t!("ui.replay.sections.damage_events"), TextWeight::Bold));
        if nda_hidden(row, debug) {
            max_w = max_w.max(measure.width(NDA, TextWeight::Regular));
        } else {
            max_w = max_w.max(breakdown_grid_width(&damage_event_rows(row), measure));
        }
    }

    max_w.max(fire_chance_width(row, debug, measure))
}

/// The fire-chance heading, the line it stands down to when there is nothing
/// to state, the headline, the formula lines, the tally rows and the per-ship
/// rows.
fn fire_chance_width(row: &PlayerRow, debug: bool, measure: &mut impl TextMeasure) -> f32 {
    let Some(fire_chance) = row.fire_chance.as_ref() else {
        return 0.0;
    };

    let mut max_w = measure.width(&t!("ui.replay.sections.fire_chance"), TextWeight::Bold);
    if nda_hidden(row, debug) {
        return max_w.max(measure.width(NDA, TextWeight::Regular));
    }
    if fire_chance.eligible_hits == 0 {
        return max_w.max(measure.width(&t!("ui.replay.sections.fire_chance_no_eligible_hits"), TextWeight::Regular));
    }

    let counts =
        measure.width(&fire_chance::counts_text(fire_chance.fires, fire_chance.eligible_hits), TextWeight::Bold);
    let expected = fire_chance::expected_fires_text(fire_chance)
        .map(|text| measure.width(&text, TextWeight::Regular))
        .unwrap_or_default();
    max_w = max_w.max(HEADLINE_CHROME + counts + expected);
    max_w = max_w.max(measure.width(&fire_chance::ships_text(fire_chance), TextWeight::Regular));

    for line in fire_chance::fire_chance_formula_lines(fire_chance, &raw_source) {
        max_w = max_w.max(measure.width(&line, TextWeight::Regular));
    }

    let tallies =
        [fire_chance::fire_chance_battle_tally_rows(fire_chance), fire_chance::fire_chance_ribbon_rows(fire_chance)];
    for tally in tallies.iter().flatten() {
        let label = measure.width(&tally.label, TextWeight::Regular);
        let count = TALLY_COUNT_WIDTH * measure.text_scale();
        max_w = max_w.max(count + BREAKDOWN_GAP + tally.depth as f32 * TALLY_INDENT + label);
    }

    for ship in fire_chance::sorted_per_ship(fire_chance) {
        let counts = measure.width(&fire_chance::counts_text(ship.fires, ship.eligible_hits), TextWeight::Regular);
        max_w = max_w.max(FIRE_CHANCE_SHIP_WIDTH * measure.text_scale() + BREAKDOWN_GAP + counts);
    }
    if let Some(line) = fire_chance::no_target_ship_line(fire_chance) {
        max_w = max_w.max(measure.width(&line, TextWeight::Regular));
    }

    max_w
}

/// The icon box, the gap between icon and label, then the label.
pub fn icon_label_row_width(icon_box: f32, label_width: f32) -> f32 {
    icon_box + ICON_LABEL_GAP + label_width
}

/// The build hover text, the sub-section headings, the loadout entries, the
/// captain-skill grid and the consumables table.
fn build_section_width(row: &PlayerRow, debug: bool, measure: &mut impl TextMeasure) -> f32 {
    if row.relation.is_enemy() && !debug {
        return 0.0;
    }

    let rem = measure.rem();
    let gap_1 = rem * 0.25;
    let gap_2 = rem * 0.5;

    let mut max_w: f32 = 0.0;
    let mut has_content = false;

    if let Some(hover) = row.skill_hover_text.as_ref() {
        max_w = max_w.max(multiline_width(hover, measure));
        has_content = true;
    }

    if let Some(build) = row.translated_build.as_ref() {
        max_w = max_w.max(measure.width(&t!("ui.replay.sections.modules"), TextWeight::Bold));
        if build.modernization_slots.is_empty() {
            max_w = max_w.max(measure.width(&t!("ui.replay.sections.modules_none"), TextWeight::Regular));
        }
        has_content = true;

        if !build.signals.is_empty() {
            max_w = max_w.max(measure.width(&t!("ui.replay.sections.signals"), TextWeight::Bold));
        }

        max_w = max_w.max(measure.width(&t!("ui.replay.sections.loadout"), TextWeight::Bold));
        if build.loadout.is_empty() {
            max_w = max_w.max(measure.width(&t!("ui.replay.sections.loadout_none"), TextWeight::Regular));
        }
        for name in build.loadout.iter().filter_map(|module| module.name.as_ref()) {
            max_w = max_w.max(measure.width(name, TextWeight::Regular));
        }

        match build.captain_skills.as_deref() {
            Some(skills) if !skills.is_empty() => {
                max_w = max_w.max(measure.width(&t!("ui.replay.sections.captain_skills"), TextWeight::Bold));
                max_w = max_w.max(skill_grid_width(skills, gap_1));
            }
            Some(_) => {
                max_w = max_w.max(measure.width(&t!("ui.replay.sections.captain_skills"), TextWeight::Bold));
                max_w = max_w.max(measure.width(&t!("ui.replay.sections.captain_skills_none"), TextWeight::Regular));
            }
            None => {
                max_w = max_w.max(measure.width(&t!("ui.replay.sections.captain_skills_none"), TextWeight::Regular));
            }
        }
    }

    if !row.consumables.is_empty() {
        max_w = max_w.max(measure.width(&t!("ui.replay.sections.consumables"), TextWeight::Bold));
        let count_width = consumable_count_width(measure);
        max_w = max_w.max(consumables_width(gap_2, measure.text_scale(), count_width));
        has_content = true;
    } else if let Some(build) = row.translated_build.as_ref()
        && !build.abilities.is_empty()
    {
        max_w = max_w.max(measure.width(&t!("ui.replay.sections.consumables"), TextWeight::Bold));
        for name in build.abilities.iter().filter_map(|ability| ability.name.as_ref()) {
            max_w = max_w.max(measure.width(name, TextWeight::Regular));
        }
        has_content = true;
    }

    if has_content { max_w } else { 0.0 }
}

/// The widest skill tier: the cost label, then one icon cell per skill, with
/// `gap_1` between each. A text fallback cell is assumed no wider than an
/// icon; a small overshoot beats clipping.
pub fn skill_grid_width(rows: &[SkillGridRow], gap_1: f32) -> f32 {
    rows.iter()
        .map(|row| {
            let skill_count = row.skills.len() as f32;
            SKILL_COST_WIDTH + gap_1 + skill_count * (MODULE_ICON_SIZE + gap_1)
        })
        .fold(0.0_f32, f32::max)
}

/// The consumables table's Remaining and Total columns: their nominal
/// width at the text scale, or wider where a heading or "Unlimited" needs it.
pub fn consumable_count_width(measure: &mut impl TextMeasure) -> f32 {
    let headings = [t!("ui.replay.consumable_header_remaining"), t!("ui.replay.consumable_header_total")];
    let unlimited = measure.width(UNLIMITED, TextWeight::Regular);
    let widest = headings.iter().map(|heading| measure.width(heading, TextWeight::Bold)).fold(unlimited, f32::max);

    (CONSUMABLE_COUNT_WIDTH * measure.text_scale()).max(widest)
}

/// The consumables table: the name column, which grows with `text_scale`,
/// and two count columns `count_width` wide, with `gap_2` between them.
pub fn consumables_width(gap_2: f32, text_scale: f32, count_width: f32) -> f32 {
    CONSUMABLE_NAME_WIDTH * text_scale + count_width * 2.0 + gap_2 * 2.0
}

#[cfg(test)]
mod tests {
    use wows_replays::types::Relation;
    use wowsunpack::game_params::types::CrewSkillName;
    use wowsunpack::game_params::types::CrewSkillType;
    use wowsunpack::game_params::types::SkillPointCost;

    use super::*;
    use crate::test_support::base_row;

    /// A breakdown lists what a battle produced and leaves out what it did
    /// not, so the grid has a line per kind that has something behind it.
    #[test]
    fn the_damage_event_grid_lists_only_the_events_a_row_recorded() {
        let mut row = base_row(1, Relation::new(1), false);
        row.fires = Some(4);
        row.citadels = Some(2);

        let rows = damage_event_rows(&row);

        assert_eq!(rows.len(), 2, "got {rows:?}");
        assert_eq!(rows[1].value, "2", "the counts read as numbers: {rows:?}");
    }

    #[test]
    fn fit_dims_keeps_a_wide_ribbon_short() {
        let IconSize { width: w, height: h } = fit_dims(IconSize { width: 220.0, height: 60.0 }, 64.0);
        assert!((w - 64.0).abs() < 1e-4, "width should hit the box: {w}");
        assert!((h - 64.0 * 60.0 / 220.0).abs() < 1e-4, "height should scale with aspect: {h}");
        assert!(h < 20.0, "a wide ribbon must render short, got {h}");
    }

    #[test]
    fn fit_dims_leaves_a_square_icon_square() {
        assert_eq!(fit_dims(IconSize::square(32.0), 32.0), IconSize::square(32.0));
        let IconSize { width: w, height: h } = fit_dims(IconSize::square(128.0), 32.0);
        assert!((w - 32.0).abs() < 1e-4 && (h - 32.0).abs() < 1e-4);
    }

    #[test]
    fn fit_dims_falls_back_to_a_square_for_degenerate_sizes() {
        assert_eq!(fit_dims(IconSize { width: 0.0, height: 10.0 }, 64.0), IconSize::square(64.0));
        assert_eq!(fit_dims(IconSize { width: 10.0, height: 0.0 }, 64.0), IconSize::square(64.0));
    }

    fn ribbon(name: &str) -> RibbonResult {
        RibbonResult {
            name: name.to_owned(),
            display_name: name.to_owned(),
            description: String::new(),
            icon_key: name.to_owned(),
            is_subribbon: false,
            count: 1,
        }
    }

    fn names(ribbons: &[&RibbonResult]) -> Vec<String> {
        ribbons.iter().map(|r| r.name.clone()).collect()
    }

    #[test]
    fn reorder_bulge_after_main_caliber_moves_bulge_when_it_precedes_main_caliber() {
        let bulge = ribbon("RIBBON_BULGE");
        let main_caliber = ribbon("RIBBON_MAIN_CALIBER");
        let other = ribbon("RIBBON_CITADEL");
        let mut ribbons = vec![&bulge, &main_caliber, &other];

        reorder_bulge_after_main_caliber(&mut ribbons);

        assert_eq!(names(&ribbons), ["RIBBON_MAIN_CALIBER", "RIBBON_BULGE", "RIBBON_CITADEL"]);
    }

    #[test]
    fn reorder_bulge_after_main_caliber_moves_bulge_when_it_follows_main_caliber() {
        let main_caliber = ribbon("RIBBON_MAIN_CALIBER");
        let other = ribbon("RIBBON_CITADEL");
        let bulge = ribbon("RIBBON_BULGE");
        let mut ribbons = vec![&main_caliber, &other, &bulge];

        reorder_bulge_after_main_caliber(&mut ribbons);

        assert_eq!(names(&ribbons), ["RIBBON_MAIN_CALIBER", "RIBBON_BULGE", "RIBBON_CITADEL"]);
    }

    #[test]
    fn reorder_bulge_after_main_caliber_is_a_no_op_when_either_ribbon_is_absent() {
        let main_caliber = ribbon("RIBBON_MAIN_CALIBER");
        let other = ribbon("RIBBON_CITADEL");
        let mut ribbons = vec![&other, &main_caliber];

        reorder_bulge_after_main_caliber(&mut ribbons);

        assert_eq!(names(&ribbons), ["RIBBON_CITADEL", "RIBBON_MAIN_CALIBER"]);
    }

    #[test]
    fn sorted_ribbons_orders_by_name_then_puts_the_bulge_after_the_main_battery() {
        let mut row = base_row(1, Relation::new(0), true);
        row.ribbons = vec![ribbon("RIBBON_MAIN_CALIBER"), ribbon("RIBBON_BULGE"), ribbon("RIBBON_CITADEL")];

        assert_eq!(names(&sorted_ribbons(&row)), ["RIBBON_CITADEL", "RIBBON_MAIN_CALIBER", "RIBBON_BULGE"]);
    }

    fn interaction(
        damage_dealt: u64,
        damage_dealt_pct: f64,
        damage_received: u64,
        damage_received_pct: f64,
    ) -> DamageInteraction {
        DamageInteraction {
            damage_dealt,
            damage_dealt_percentage: damage_dealt_pct,
            damage_received,
            damage_received_percentage: damage_received_pct,
            ..Default::default()
        }
    }

    #[test]
    fn interaction_amount_and_percentage_read_the_direction_specific_fields() {
        let i = interaction(42_000, 56.4, 12_000, 38.1);

        assert_eq!(interaction_amount(&i, DamageDirection::Dealt), 42_000);
        assert_eq!(interaction_amount(&i, DamageDirection::Received), 12_000);
        assert!((interaction_percentage(&i, DamageDirection::Dealt, false) - 56.4).abs() < 1e-9);
        assert!((interaction_percentage(&i, DamageDirection::Received, false) - 38.1).abs() < 1e-9);
    }

    #[test]
    fn alt_swaps_a_percentage_for_the_share_of_the_other_player_total() {
        let mut i = interaction(42_000, 56.4, 12_000, 38.1);
        i.damage_dealt_inverse_percentage = 71.2;
        i.damage_received_inverse_percentage = 9.4;

        assert!((interaction_percentage(&i, DamageDirection::Dealt, true) - 71.2).abs() < 1e-9);
        assert!((interaction_percentage(&i, DamageDirection::Received, true) - 9.4).abs() < 1e-9);
    }

    /// Largest first; a zero amount, or a player no row names, gets no line.
    #[test]
    fn damage_target_rows_list_named_players_by_amount() {
        let mut row = base_row(1, Relation::new(0), true);
        row.damage_interactions = Some(
            [
                (wows_replays::types::AccountId(2), interaction(1_000, 20.0, 0, 0.0)),
                (wows_replays::types::AccountId(3), interaction(4_000, 80.0, 0, 0.0)),
                (wows_replays::types::AccountId(9), interaction(500, 10.0, 0, 0.0)),
            ]
            .into_iter()
            .collect(),
        );
        let mut low = base_row(2, Relation::new(2), false);
        low.ship_name = "Low".to_owned();
        let mut high = base_row(3, Relation::new(2), false);
        high.ship_name = "High".to_owned();

        assert_eq!(
            damage_target_rows(&row, &[low, high], DamageDirection::Dealt, false),
            vec![
                BreakdownRow { label: "High".to_owned(), value: "4,000 (80%)".to_owned() },
                BreakdownRow { label: "Low".to_owned(), value: "1,000 (20%)".to_owned() },
            ]
        );
        assert!(damage_target_rows(&row, &[], DamageDirection::Received, false).is_empty());
    }

    fn skill_tier(skill_count: usize) -> SkillGridRow {
        SkillGridRow {
            point_cost: Some(SkillPointCost::new(1)),
            skills: (0..skill_count)
                .map(|i| SkillGridSkill {
                    internal_name: CrewSkillName::from(format!("Skill{i}").as_str()),
                    skill_type: CrewSkillType::new(0),
                    name: None,
                    description: None,
                    point_cost: Some(SkillPointCost::new(1)),
                    learned: i % 2 == 0,
                })
                .collect(),
        }
    }

    #[test]
    fn skill_grid_width_matches_the_cost_label_plus_gap_plus_icon_formula() {
        let width = skill_grid_width(&[skill_tier(7)], 4.0);
        let expected = 14.0 + 4.0 + 7.0 * (MODULE_ICON_SIZE + 4.0);
        assert!((width - expected).abs() < 1e-4, "width {width} != expected {expected}");
    }

    #[test]
    fn skill_grid_width_takes_the_widest_tier() {
        let width = skill_grid_width(&[skill_tier(1), skill_tier(7), skill_tier(3)], 4.0);
        assert!((width - skill_grid_width(&[skill_tier(7)], 4.0)).abs() < 1e-4);
    }

    #[test]
    fn skill_grid_width_of_a_full_tier_far_exceeds_a_typical_collapsed_skills_width() {
        let collapsed_skills_column_width = 90.0;
        let width = skill_grid_width(&[skill_tier(7)], 4.0);
        assert!(
            width > collapsed_skills_column_width,
            "grid width {width} should exceed {collapsed_skills_column_width}"
        );
    }

    #[test]
    fn skill_text_names_the_cost_and_falls_back_to_the_internal_name() {
        let skill = &skill_tier(1).skills[0];

        assert_eq!(skill_tooltip(skill), "Skill0 (1 pt)");
        assert_eq!(skill_fallback_label(skill), "(1) Skill0");
    }

    #[test]
    fn consumables_width_matches_the_fixed_three_column_layout() {
        assert!((consumables_width(8.0, 1.0, CONSUMABLE_COUNT_WIDTH) - 314.0).abs() < 1e-4);
        assert!((consumables_width(8.0, 1.5, 80.0) - (170.0 * 1.5 + 160.0 + 16.0)).abs() < 1e-4);
    }

    #[test]
    fn icon_label_row_width_adds_the_8px_gap_between_icon_and_label() {
        assert!((icon_label_row_width(64.0, 40.0) - 112.0).abs() < 1e-4);
        assert!((icon_label_row_width(0.0, 40.0) - 48.0).abs() < 1e-4);
    }

    #[test]
    fn an_unlimited_consumable_counts_its_uses() {
        let consumable = |total_charges| ConsumableResult {
            display_name: "Repair Party".to_owned(),
            description: String::new(),
            icon_key: "PCY010".to_owned(),
            charges_used: 3,
            total_charges,
        };

        let unlimited = consumable_counts(&consumable(ChargeCount::Unlimited));
        assert_eq!((unlimited.remaining.as_str(), unlimited.total.as_str()), ("3", "Unlimited"));

        let finite = consumable_counts(&consumable(ChargeCount::Finite(5)));
        assert_eq!(finite.hover, "Remaining: 2 / Total: 5");
    }

    #[test]
    fn a_module_without_a_description_is_its_own_tooltip() {
        let module = TranslatedModule { name: None, description: None, game_params_name: "PCM001".to_owned() };
        assert_eq!(module_text(&module), ModuleText { name: "PCM001".to_owned(), tooltip: "PCM001".to_owned() });
    }
}
