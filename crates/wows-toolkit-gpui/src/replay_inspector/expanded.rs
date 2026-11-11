//! Expanded row content: achievements, ribbons, and damage events (Name
//! column), captain build (modernizations, signals, loadout, captain skills,
//! consumables; Skills column), and per-victim damage interaction breakdowns
//! (ActualDamage/ReceivedDamage/SpottingDamage columns). Ported from the egui
//! app's `ui/replay_parser/mod.rs` `if 0.0 < expandedness { match column { ... } }`
//! block (`mod.rs:1834-2088`) plus its `render_skill_grid`/
//! `render_modernization_slots`/`render_signals`/`render_consumable_inventory`/
//! `dealt_damage_details`/`received_damage_details`/`spotting_damage_details`
//! helpers.
//!
//! Like the egui original, this port nests each column's expanded content
//! inside that column's own cell: `render_column_detail` returns just one
//! column's detail, and `table.rs`'s per-column cell stacks it (in a
//! `v_flex`) under that column's collapsed content, so the Name column's
//! achievements sit under Name, the Skills build under Skills, and so on.
//! The row grows to the tallest column cell (flex), matching egui.
//!
//! Icons come from `IconCache::get_keyed` (see that module's key-convention
//! doc). Nothing calls `set_keyed` until a later milestone's replay-loading
//! pipeline resolves real `GameAsset` bytes, so every icon lookup here
//! currently returns `None` and falls back to a text label, matching the
//! egui app's own icon-texture-missing fallback branches.

use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::h_flex;
use gpui_kit::component::separator::Separator;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rust_i18n::t;
use wows_replay_insights::battle_report::AchievementResult;
use wows_replay_insights::battle_report::ConsumableResult;
use wows_replay_insights::battle_report::DamageInteraction;
use wows_replay_insights::battle_report::RibbonResult;
use wows_replay_insights::battle_report::TranslatedModule;
use wows_replay_insights::fire_chance::analysis::EffectiveFireChance;
use wows_toolkit_viewmodel::breakdown::BreakdownRow;
use wows_toolkit_viewmodel::fire_chance;
use wowsunpack::game_params::skill_grid_data::SkillGridRow;
use wowsunpack::game_params::skill_grid_data::SkillGridSkill;
use wowsunpack::game_types::ChargeCount;

use super::columns::NDA;
use super::columns::ReplayColumn;
use super::damage_mix;
use super::damage_mix::DamageLine;
use super::damage_mix::DamageMix;
use super::icons::IconCache;
use super::model::PlayerRow;
use super::model::separate_number;
use super::table::text_width;
use crate::status_bar::ModeHint;
use crate::status_bar::ModifierKey;

/// Multiplier for a detail-list item's `ElementId`
/// (`row_ix * DETAIL_ID_STRIDE + item_ix`), spacing rows apart enough that no
/// two `(row, item)` pairs collide across any of the lists this module
/// renders (achievements, ribbons, modernizations, signals, loadout, captain
/// skills, consumables, damage interactions). Each list also gets its own
/// `ElementId` string tag, so only same-tag, same-row collisions matter; a
/// single row is not expected to carry anywhere near this many entries in one
/// list.
const DETAIL_ID_STRIDE: usize = 256;

/// Icon size, in pixels, for the two icon kinds the egui app renders larger
/// than the rest: achievements and non-subribbon ribbons.
const LARGE_ICON_SIZE: f32 = 32.0;
const RIBBON_ICON_SIZE: f32 = 64.0;
const MODULE_ICON_SIZE: f32 = 28.0;
const CONSUMABLE_ICON_SIZE: f32 = 20.0;

/// One ribbon tile's width: a main ribbon fitted within `RIBBON_ICON_SIZE`,
/// with its count centred below.
const RIBBON_TILE_WIDTH: f32 = RIBBON_ICON_SIZE;
const RIBBON_TILE_GAP: f32 = 6.0;
/// The ribbon grid never measures narrower than this many tiles per line, so
/// a narrow Name column still lays the ribbons out as a grid.
const RIBBON_GRID_MIN_TILES: usize = 4;

/// Captain skill, modernization and consumable icons are white glyphs drawn
/// for the game's dark interface, so they sit on this dark tile in every
/// theme: legible on light themes and identical across themes.
const GAME_ICON_TILE_BG: u32 = 0x2a2d33;
const GAME_ICON_TILE_PADDING: f32 = 2.0;
const MODULE_TILE_SIZE: f32 = MODULE_ICON_SIZE + 2.0 * GAME_ICON_TILE_PADDING;

/// Whether a damage-interaction line describes damage this row dealt to, or
/// received from, the other player.
#[derive(Clone, Copy)]
enum DamageDirection {
    Dealt,
    Received,
}

/// One column's expanded detail for `row` at list index `ix`, or `None` when
/// that column has nothing to show (so `table.rs` leaves the column's cell at
/// its collapsed height). Mirrors the egui app's per-column
/// `if 0.0 < expandedness { match column { ... } }` arms (`mod.rs:1834-2088`):
/// each column draws its own detail under its own cell -- Name's achievements,
/// ribbons, damage events and effective fire chance under Name, Skills' build
/// under Skills, the damage breakdowns under ActualDamage/ReceivedDamage/
/// SpottingDamage, and the Potential/Hits breakdowns under those columns.
/// `all_rows` resolves a damage interaction's victim/attacker `db_id` into a
/// ship name, and `alt_held` turns the breakdown's percentages around.
#[allow(clippy::too_many_arguments)]
pub fn render_column_detail(
    ix: usize,
    col: ReplayColumn,
    row: &PlayerRow,
    all_rows: &[PlayerRow],
    icons: &IconCache,
    debug: bool,
    alt_held: bool,
) -> Option<AnyElement> {
    match col {
        ReplayColumn::Name => render_name_section(ix, row, debug, icons),
        ReplayColumn::Skills => render_build_section(ix, row, debug, icons),
        ReplayColumn::ActualDamage => render_damage_section(row, all_rows, debug, DamageDirection::Dealt, alt_held),
        ReplayColumn::ReceivedDamage => {
            render_damage_section(row, all_rows, debug, DamageDirection::Received, alt_held)
        }
        ReplayColumn::PotentialDamage => {
            render_nda_gated_breakdown(row.potential_damage_breakdown.as_deref(), row, debug)
        }
        ReplayColumn::Hits => render_nda_gated_breakdown(row.hits_breakdown.as_deref(), row, debug),
        ReplayColumn::SpottingDamage => render_spotting_section(row, all_rows),
        _ => None,
    }
}

/// One column's expanded-detail content width for `row`, measured (not
/// rendered) so `table.rs`'s `measure_column_widths` can grow that column to
/// fit an expanded row instead of clipping it at the collapsed width. Kept
/// structurally aligned with `render_column_detail` above -- same match arms,
/// same per-section gating -- so the two stay in sync as either one changes;
/// each arm below carries a terse note on which `render_*` it mirrors.
/// For ActualDamage/ReceivedDamage this only measures the ammo-type breakdown
/// paragraph, not the per-victim interaction lines (the breakdown paragraph
/// is already the wider of the two in practice). SpottingDamage measures its
/// per-victim lines through `all_rows`, since most rows have no per-type
/// split to size the column by. Modernizations and signals are excluded everywhere:
/// `modernization_slots_view`/`signals_view` both use `h_flex().flex_wrap()`,
/// which wraps onto new lines instead of pushing the column wider, so they
/// must never drive a column's measured width.
pub(super) fn expanded_detail_width(
    col: ReplayColumn,
    row: &PlayerRow,
    all_rows: &[PlayerRow],
    debug: bool,
    window: &mut Window,
) -> f32 {
    match col {
        ReplayColumn::Name => name_section_width(row, debug, window),
        ReplayColumn::Skills => build_section_width(row, debug, window),
        ReplayColumn::ActualDamage => damage_section_width(
            row.actual_damage_by_type.as_deref(),
            row,
            all_rows,
            debug,
            DamageDirection::Dealt,
            window,
        ),
        ReplayColumn::ReceivedDamage => damage_section_width(
            row.received_damage_by_type.as_deref(),
            row,
            all_rows,
            debug,
            DamageDirection::Received,
            window,
        ),
        ReplayColumn::PotentialDamage => {
            nda_gated_breakdown_width(row.potential_damage_breakdown.as_deref(), row, debug, window)
        }
        ReplayColumn::Hits => nda_gated_breakdown_width(row.hits_breakdown.as_deref(), row, debug, window),
        ReplayColumn::SpottingDamage => {
            let breakdown =
                row.spotting_damage_breakdown.as_deref().map(|rows| breakdown_grid_width(rows, window)).unwrap_or(0.0);
            breakdown.max(breakdown_grid_width(&spotting_target_rows(row, all_rows), window))
        }
        _ => 0.0,
    }
}

/// Widest line of a newline-separated block.
fn multiline_width(text: &str, window: &mut Window) -> f32 {
    text.split('\n').map(|line| text_width(line.into(), window, false)).fold(0.0_f32, f32::max)
}

/// Mirrors `render_nda_gated_breakdown` (Potential/Hits) and the NDA gate at
/// the top of `render_damage_section` (ActualDamage/ReceivedDamage): the NDA
/// placeholder's width when stats are hidden, otherwise the grid's.
fn nda_gated_breakdown_width(rows: Option<&[BreakdownRow]>, row: &PlayerRow, debug: bool, window: &mut Window) -> f32 {
    if row.should_hide_stats() && !debug {
        return text_width(NDA.into(), window, false);
    }
    rows.map(|rows| breakdown_grid_width(rows, window)).unwrap_or(0.0)
}

/// Mirrors `breakdown_grid`: the widest label, the 8px gap between the two
/// columns, and the widest value.
fn breakdown_grid_width(rows: &[BreakdownRow], window: &mut Window) -> f32 {
    if rows.is_empty() {
        return 0.0;
    }
    let mut labels = 0.0_f32;
    let mut values = 0.0_f32;
    for row in rows {
        labels = labels.max(text_width(row.label.clone().into(), window, false));
        values = values.max(text_width(row.value.clone().into(), window, false));
    }
    labels + BREAKDOWN_GAP + values
}

/// Mirrors `damage_lines_grid`: `breakdown_grid_width` with the colour key
/// and its gap before each label.
fn damage_lines_width(lines: &[DamageLine], window: &mut Window) -> f32 {
    if lines.is_empty() {
        return 0.0;
    }

    let mut labels = 0.0_f32;
    let mut values = 0.0_f32;
    for line in lines {
        labels = labels.max(text_width(line.label.into(), window, false));
        values = values.max(text_width(separate_number(line.amount).into(), window, false));
    }

    damage_mix::SWATCH_SIZE + SWATCH_GAP + labels + BREAKDOWN_GAP + values
}

/// Mirrors `render_name_section`: the section headings, each achievement
/// row's icon-box-plus-label width (`icon_label_row_width`), the ribbon grid
/// (`ribbon_grid_width`), the damage-event grid (or the NDA placeholder when
/// hidden), and the fire-chance block.
fn name_section_width(row: &PlayerRow, debug: bool, window: &mut Window) -> f32 {
    let has_achievements = !row.achievements.is_empty();
    let has_ribbons = !row.ribbons.is_empty();
    let has_damage_events =
        row.fires.is_some() || row.floods.is_some() || row.citadels.is_some() || row.crits.is_some();
    if !has_achievements && !has_ribbons && !has_damage_events && row.fire_chance.is_none() {
        return 0.0;
    }

    let mut max_w: f32 = 0.0;

    if has_achievements {
        max_w = max_w.max(text_width(t!("ui.replay.sections.achievements").into_owned().into(), window, true));
        for achievement in &row.achievements {
            let label = if achievement.count > 1 {
                format!("{} ({}x)", achievement.display_name, achievement.count)
            } else {
                achievement.display_name.clone()
            };
            let label_w = text_width(label.into(), window, false);
            // Achievements always render at LARGE_ICON_SIZE (`achievement_row`).
            max_w = max_w.max(icon_label_row_width(LARGE_ICON_SIZE, label_w));
        }
    }

    if has_ribbons {
        max_w = max_w.max(text_width(t!("ui.replay.sections.ribbons").into_owned().into(), window, true));
        max_w = max_w.max(ribbon_grid_width(row.ribbons.len()));
    }

    if has_damage_events {
        max_w = max_w.max(text_width(t!("ui.replay.sections.damage_events").into_owned().into(), window, true));
        if row.should_hide_stats() && !debug {
            max_w = max_w.max(text_width(NDA.into(), window, false));
        } else {
            max_w = max_w.max(breakdown_grid_width(&damage_event_rows(row), window));
        }
    }

    max_w.max(fire_chance_width(row, debug, window)) + NAME_SECTION_INDENT.as_f32()
}

/// How far the Name column's detail sits in from the table's edge.
const NAME_SECTION_INDENT: Pixels = px(8.);

/// Mirrors `render_fire_chance_section`: its heading, the line it stands down
/// to when there is nothing to state, the headline (the fire glyph, the counts,
/// the expected figure and the copy button), the formula lines, the tally rows
/// and the per-ship rows.
fn fire_chance_width(row: &PlayerRow, debug: bool, window: &mut Window) -> f32 {
    let Some(fire_chance) = row.fire_chance.as_ref() else { return 0.0 };

    let mut max_w = text_width(t!("ui.replay.sections.fire_chance").into_owned().into(), window, true);
    if row.should_hide_stats() && !debug {
        return max_w.max(text_width(NDA.into(), window, false));
    }
    if fire_chance.eligible_hits == 0 {
        return max_w.max(text_width(
            t!("ui.replay.sections.fire_chance_no_eligible_hits").into_owned().into(),
            window,
            false,
        ));
    }

    let counts =
        text_width(fire_chance::counts_text(fire_chance.fires, fire_chance.eligible_hits).into(), window, true);
    let expected = fire_chance::expected_fires_text(fire_chance)
        .map(|text| text_width(text.into(), window, false))
        .unwrap_or_default();
    max_w = max_w.max(HEADLINE_CHROME + counts + expected);
    max_w = max_w.max(text_width(fire_chance::ships_text(fire_chance).into_owned().into(), window, false));

    for line in fire_chance::fire_chance_formula_lines(fire_chance, &raw_source) {
        max_w = max_w.max(text_width(line.into(), window, false));
    }

    let mut tally_width = |rows: &[fire_chance::TallyRow]| {
        for row in rows {
            let label = text_width(row.label.to_string().into(), window, false);
            max_w = max_w.max(TALLY_COUNT_WIDTH.as_f32() + BREAKDOWN_GAP + row.depth as f32 * 12.0 + label);
        }
    };
    tally_width(&fire_chance::fire_chance_battle_tally_rows(fire_chance));
    tally_width(&fire_chance::fire_chance_ribbon_rows(fire_chance));

    for ship in fire_chance::sorted_per_ship(fire_chance) {
        let counts = text_width(fire_chance::counts_text(ship.fires, ship.eligible_hits).into(), window, false);
        max_w = max_w.max(FIRE_CHANCE_SHIP_WIDTH.as_f32() + BREAKDOWN_GAP + counts);
    }
    if let Some(line) = fire_chance::no_target_ship_line(fire_chance) {
        max_w = max_w.max(text_width(line.into(), window, false));
    }

    max_w
}

/// The fire glyph, the copy button and the gaps around them, which is what the
/// fire-chance headline holds besides its two figures.
const HEADLINE_CHROME: f32 = 56.0;

/// Pure width formula for `icon_label_row`: the icon box, the 8px icon-to-
/// label gap (`icon_label_row`'s `gap(px(8.))`), then the label text.
fn icon_label_row_width(icon_box: f32, label_width: f32) -> f32 {
    icon_box + 8.0 + label_width
}

/// Width `ribbons_view` asks of the column: `RIBBON_GRID_MIN_TILES` tiles per
/// line, or fewer when there are fewer ribbons. A wider column fits more
/// tiles per line, since the grid wraps to it.
fn ribbon_grid_width(ribbon_count: usize) -> f32 {
    let tiles = ribbon_count.min(RIBBON_GRID_MIN_TILES) as f32;
    if tiles == 0.0 {
        return 0.0;
    }
    tiles * RIBBON_TILE_WIDTH + (tiles - 1.0) * RIBBON_TILE_GAP
}

/// Mirrors `render_build_section`: the loadout/skill-tier hover text, the
/// sub-section headings, the loadout entries, the captain-skill grid
/// (`skill_grid_width`), and the consumables table (`consumables_width`).
/// Modernizations and signals are intentionally excluded (see
/// `expanded_detail_width`'s doc).
fn build_section_width(row: &PlayerRow, debug: bool, window: &mut Window) -> f32 {
    if row.relation.is_enemy() && !debug {
        return 0.0;
    }

    let rem = window.rem_size().as_f32();
    let gap_1 = rem * 0.25;
    let gap_2 = rem * 0.5;

    let mut max_w: f32 = 0.0;
    let mut has_content = false;

    if let Some(hover) = row.skill_hover_text.as_ref() {
        max_w = max_w.max(multiline_width(hover, window));
        has_content = true;
    }

    if let Some(build) = row.translated_build.as_ref() {
        max_w = max_w.max(text_width(t!("ui.replay.sections.modules").into_owned().into(), window, true));
        if build.modernization_slots.is_empty() {
            max_w = max_w.max(text_width(t!("ui.replay.sections.modules_none").into_owned().into(), window, false));
        }
        has_content = true;

        if !build.signals.is_empty() {
            max_w = max_w.max(text_width(t!("ui.replay.sections.signals").into_owned().into(), window, true));
        }

        max_w = max_w.max(text_width(t!("ui.replay.sections.loadout").into_owned().into(), window, true));
        if build.loadout.is_empty() {
            max_w = max_w.max(text_width(t!("ui.replay.sections.loadout_none").into_owned().into(), window, false));
        } else {
            for module in &build.loadout {
                if let Some(name) = module.name.as_ref() {
                    max_w = max_w.max(text_width(name.clone().into(), window, false));
                }
            }
        }

        match build.captain_skills.as_ref() {
            Some(skills) => {
                max_w =
                    max_w.max(text_width(t!("ui.replay.sections.captain_skills").into_owned().into(), window, true));
                if skills.is_empty() {
                    max_w = max_w.max(text_width(
                        t!("ui.replay.sections.captain_skills_none").into_owned().into(),
                        window,
                        false,
                    ));
                } else {
                    max_w = max_w.max(skill_grid_width(skills, gap_1));
                }
            }
            None => {
                max_w = max_w.max(text_width(
                    t!("ui.replay.sections.captain_skills_none").into_owned().into(),
                    window,
                    false,
                ));
            }
        }
    }

    if !row.consumables.is_empty() {
        max_w = max_w.max(text_width(t!("ui.replay.sections.consumables").into_owned().into(), window, true));
        max_w = max_w.max(consumables_width(gap_2));
        has_content = true;
    } else if let Some(build) = row.translated_build.as_ref()
        && !build.abilities.is_empty()
    {
        max_w = max_w.max(text_width(t!("ui.replay.sections.consumables").into_owned().into(), window, true));
        for ability in &build.abilities {
            if let Some(name) = ability.name.as_ref() {
                max_w = max_w.max(text_width(name.clone().into(), window, false));
            }
        }
        has_content = true;
    }

    if has_content { max_w } else { 0.0 }
}

/// Pure per-tier width for `captain_skill_grid_view`: the 14px cost-label
/// cell, `gap_1` before it and after each skill cell (matching the line's
/// `h_flex().gap_1()` across its `1 + skill_count` children), and one
/// `MODULE_TILE_SIZE` cell per skill -- a resolved icon's tile is exactly
/// that size, and an unresolved text-fallback cell is assumed to be no wider
/// (a small overshoot beats clipping, per the column's overall 500px clamp).
/// Returns the widest tier, since the tiers stack vertically.
fn skill_grid_width(rows: &[SkillGridRow], gap_1: f32) -> f32 {
    rows.iter()
        .map(|row| {
            let skill_count = row.skills.len() as f32;
            14.0 + gap_1 + skill_count * (MODULE_TILE_SIZE + gap_1)
        })
        .fold(0.0_f32, f32::max)
}

/// Pure width for `consumables_view`'s fixed 3-column layout: `NAME_COL` +
/// `gap_2` + `COUNT_COL` + `gap_2` + `COUNT_COL`, matching the header/row's
/// `h_flex().gap_2()` across its three fixed-width cells (`consumable_row`'s
/// leading icon renders inside the fixed `NAME_COL` box, so it never widens
/// the row beyond `NAME_COL`).
fn consumables_width(gap_2: f32) -> f32 {
    const NAME_COL: f32 = 170.0;
    const COUNT_COL: f32 = 64.0;
    NAME_COL + gap_2 + COUNT_COL + gap_2 + COUNT_COL
}

/// Potential-damage / Hits expanded content: the column's own breakdown shown
/// inline, NDA-gated exactly like the egui arms (`mod.rs:1983-1989, 2077-2082`)
/// -- an NDA placeholder when the row's stats are hidden and debug is off,
/// otherwise the grid (or nothing when there is no breakdown).
fn render_nda_gated_breakdown(rows: Option<&[BreakdownRow]>, row: &PlayerRow, debug: bool) -> Option<AnyElement> {
    if row.should_hide_stats() && !debug {
        return Some(nda_text());
    }
    rows.map(|rows| breakdown_grid(rows))
}

/// A label-and-value listing: labels in one column and values in another, so
/// the two line up in the proportional font the rest of the row uses.
#[track_caller]
fn breakdown_grid(rows: &[BreakdownRow]) -> AnyElement {
    let location = std::panic::Location::caller();
    h_flex()
        .id(SharedString::from(format!("breakdown-{}-{}", location.line(), location.column())))
        .gap(px(BREAKDOWN_GAP))
        .items_start()
        .text_xs()
        .child(
            v_flex().gap_px().children(
                rows.iter()
                    .enumerate()
                    .map(|(index, row)| div().id(("label", index)).child(detail_text(row.label.clone()))),
            ),
        )
        .child(
            v_flex().gap_px().items_end().children(
                rows.iter()
                    .enumerate()
                    .map(|(index, row)| div().id(("value", index)).child(detail_text(row.value.clone()))),
            ),
        )
        .into_any_element()
}

/// Between a breakdown grid's two columns.
const BREAKDOWN_GAP: f32 = 8.0;

/// The per-type damage listing: `breakdown_grid` with each type's colour key,
/// the colour its stretch of the cell's bar is drawn in.
fn damage_lines_grid(lines: &[DamageLine]) -> AnyElement {
    let mix = DamageMix::of(lines);
    let key = |line: &DamageLine| mix.as_ref().map_or(line.kind, |mix| mix.shown_kind(line.kind));
    h_flex()
        .id("damage-lines")
        .gap(px(BREAKDOWN_GAP))
        .items_start()
        .text_xs()
        .child(v_flex().gap_px().children(lines.iter().enumerate().map(|(index, line)| {
            h_flex()
                .id(("label", index))
                .gap(px(SWATCH_GAP))
                .child(damage_mix::swatch(key(line)))
                .child(detail_text(line.label))
        })))
        .child(
            v_flex().gap_px().items_end().children(
                lines
                    .iter()
                    .enumerate()
                    .map(|(index, line)| div().id(("value", index)).child(detail_text(separate_number(line.amount)))),
            ),
        )
        .into_any_element()
}

/// Between a colour key and the type it names.
const SWATCH_GAP: f32 = 4.0;

#[track_caller]
fn detail_text(text: impl Into<SharedString>) -> impl IntoElement {
    let text = text.into();
    let location = std::panic::Location::caller();
    let id = SharedString::from(format!("detail-{}-{}-{}", location.line(), location.column(), text));
    div()
        .id(id)
        .on_click(|event, _window, cx| {
            if !event.modifiers().secondary(cx) {
                cx.stop_propagation();
            }
        })
        .child(crate::ui::selectable_text("text", text))
}

fn section_heading(text: String) -> AnyElement {
    div().text_xs().font_weight(FontWeight::BOLD).child(detail_text(text)).into_any_element()
}

fn body_text(text: impl Into<SharedString>) -> AnyElement {
    div().text_xs().child(detail_text(text)).into_any_element()
}

fn nda_text() -> AnyElement {
    div().text_xs().child(detail_text(NDA)).into_any_element()
}

/// What a game icon is drawn on.
#[derive(Clone, Copy)]
enum IconBackdrop {
    /// Icons with their own colours and shape, such as signal flags.
    Bare,
    /// White glyph icons; see `GAME_ICON_TILE_BG`.
    DarkTile,
}

/// `icon` at `icon_size` on a dark rounded tile `GAME_ICON_TILE_PADDING`
/// larger on every side.
fn game_icon_tile(icon: Img, icon_size: f32) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(icon_size + 2.0 * GAME_ICON_TILE_PADDING))
        .rounded(px(4.))
        .bg(rgb(GAME_ICON_TILE_BG))
        .child(icon.size(px(icon_size)))
}

/// A small icon-or-text cell shared by modernizations and signals: a decoded
/// icon when `icons.get_keyed(key)` has one, otherwise `name` as plain text;
/// a hover tooltip either way. `tag` keys the `ElementId` so different lists
/// in the same row never collide (see `DETAIL_ID_STRIDE`'s doc).
#[allow(clippy::too_many_arguments)]
fn icon_or_text_cell(
    tag: &'static str,
    row_ix: usize,
    idx: usize,
    key: &str,
    name: String,
    description: Option<&str>,
    size: f32,
    backdrop: IconBackdrop,
    icons: &IconCache,
) -> AnyElement {
    let image = icons.get_keyed(key);
    let preview = image.as_ref().map(|image| IconPreview::new(Arc::clone(image), backdrop, size));
    let el = match (image, backdrop) {
        (Some(image), IconBackdrop::Bare) => div().child(img(image).w(px(size)).h(px(size))),
        (Some(image), IconBackdrop::DarkTile) => game_icon_tile(img(image), size),
        (None, _) => div().text_xs().child(detail_text(name.clone())),
    };
    let tooltip = IconTooltip { preview, name: name.into(), description: description_text(description) };
    el.id((tag, row_ix * DETAIL_ID_STRIDE + idx))
        .tooltip(move |window, cx| tooltip.build().build(window, cx))
        .into_any_element()
}

/// A game icon as its tooltip draws it: larger than in the row, on the same
/// backdrop.
#[derive(Clone)]
struct IconPreview {
    image: Arc<RenderImage>,
    backdrop: IconBackdrop,
    size: f32,
}

impl IconPreview {
    /// The preview of an icon drawn at `drawn_size` in the row: twice that,
    /// but never smaller than `MIN_SIZE`.
    fn new(image: Arc<RenderImage>, backdrop: IconBackdrop, drawn_size: f32) -> Self {
        const MIN_SIZE: f32 = 64.0;

        Self { image, backdrop, size: (drawn_size * 2.0).max(MIN_SIZE) }
    }

    fn render(&self) -> AnyElement {
        match self.backdrop {
            IconBackdrop::Bare => {
                let (w, h) = fit_within_box(&self.image, self.size);
                img(Arc::clone(&self.image)).w(px(w)).h(px(h)).flex_none().into_any_element()
            }
            IconBackdrop::DarkTile => game_icon_tile(img(Arc::clone(&self.image)), self.size).into_any_element(),
        }
    }
}

/// An icon's tooltip: the icon drawn larger, with `name` in bold and
/// `description` centred under it.
#[derive(Clone)]
struct IconTooltip {
    preview: Option<IconPreview>,
    name: SharedString,
    description: Option<SharedString>,
}

impl IconTooltip {
    fn build(&self) -> Tooltip {
        /// Where a description wraps.
        const MAX_WIDTH: f32 = 300.0;

        let this = self.clone();
        Tooltip::element(move |_window, _cx| {
            v_flex()
                .max_w(px(MAX_WIDTH))
                .items_center()
                .gap_1()
                .py_0p5()
                .text_center()
                .when_some(this.preview.as_ref(), |el, preview| el.child(preview.render()))
                .child(div().font_weight(FontWeight::BOLD).child(this.name.clone()))
                .when_some(this.description.clone(), |el, description| el.child(div().child(description)))
        })
    }
}

/// A description worth showing: present and not blank.
fn description_text(description: Option<&str>) -> Option<SharedString> {
    description.filter(|text| !text.trim().is_empty()).map(|text| SharedString::from(text.to_owned()))
}

/// Fitted `(width, height)` to render `image` within a `max` by `max` box
/// while preserving its native aspect ratio. Matches the egui app's ribbon
/// rendering, which uses `Image::fit_to_exact_size((64, 64))` -- a scale that
/// keeps aspect ratio -- so a wide, short ribbon icon renders short rather
/// than being forced into a `max` by `max` square. Square icons
/// (achievements) are unaffected: they fit as `max` by `max`.
fn fit_within_box(image: &RenderImage, max: f32) -> (f32, f32) {
    let size = image.size(0);
    fit_dims(size.width.0 as f32, size.height.0 as f32, max)
}

/// Scales native `(w, h)` to fit within a `max` by `max` box preserving aspect
/// ratio. Degenerate (non-positive) dimensions fall back to a `max` square.
fn fit_dims(w: f32, h: f32, max: f32) -> (f32, f32) {
    if w <= 0.0 || h <= 0.0 {
        return (max, max);
    }
    let scale = (max / w).min(max / h);
    (w * scale, h * scale)
}

/// An `[icon | label]` achievement row: an icon when `icons.get_keyed(key)`
/// has one (no fallback text in its place, matching the egui app's
/// `ui.horizontal` which only conditionally adds the image), then `label`
/// always rendered alongside it. A hover tooltip covers the whole row.
/// The icon is fitted within a `size` by `size` box preserving its aspect
/// ratio (`fit_within_box`). `items_center()` vertically centers the icon
/// and label on a shared center line when the icon is taller than the text.
/// The 8px icon-to-label gap matches egui's default
/// `Spacing::item_spacing.x` (`egui::style::Spacing::default()`), the gap a
/// `ui.horizontal` puts between the image and the label it adds next.
/// `tag` keys the `ElementId` so different lists in the same row never
/// collide (see `DETAIL_ID_STRIDE`'s doc).
#[allow(clippy::too_many_arguments)]
fn icon_label_row(
    tag: &'static str,
    row_ix: usize,
    idx: usize,
    key: &str,
    label: String,
    tooltip_text: String,
    size: f32,
    icons: &IconCache,
) -> AnyElement {
    let tooltip: SharedString = tooltip_text.into();
    let mut row = h_flex().w_full().gap(px(8.)).items_center();
    if let Some(image) = icons.get_keyed(key) {
        let (w, h) = fit_within_box(&image, size);
        row = row.child(img(image).w(px(w)).h(px(h)).flex_none());
    }
    row = row.child(div().flex_1().min_w(px(0.)).text_xs().child(detail_text(label)));
    row.id((tag, row_ix * DETAIL_ID_STRIDE + idx))
        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
        .into_any_element()
}

/// The effective-fire-chance block under the recording player's row.
///
/// Only that row carries a result, so every other expands to nothing here.
/// The wording is `wows_toolkit_viewmodel::fire_chance`, shared with the egui
/// block this mirrors (`ui/replay_parser/mod.rs`'s `render_fire_chance`).
fn render_fire_chance_section(row_ix: usize, row: &PlayerRow, debug: bool) -> Option<AnyElement> {
    let fire_chance = row.fire_chance.as_ref()?;

    let mut col = v_flex().gap(px(3.)).child(section_heading(t!("ui.replay.sections.fire_chance").into_owned()));
    if row.should_hide_stats() && !debug {
        return Some(
            col.child(div().text_xs().child(detail_text(t!("ui.replay.nda").into_owned()))).into_any_element(),
        );
    }

    if fire_chance.eligible_hits == 0 {
        // A total with nothing behind it would read as a real zero.
        return Some(
            col.child(
                div()
                    .text_xs()
                    .text_color(crate::theme::text_dim())
                    .child(detail_text(t!("ui.replay.sections.fire_chance_no_eligible_hits").into_owned())),
            )
            .into_any_element(),
        );
    }

    // Observed and expected sit together: comparing them is the point of the
    // statistic, so they must not be split apart.
    let mut headline = h_flex().gap_2().items_center().child(
        h_flex()
            .gap_1()
            .items_center()
            .font_weight(FontWeight::BOLD)
            .text_xs()
            .child(crate::icons::icon(crate::icons::Glyph::Flame))
            .child(detail_text(fire_chance::counts_text(fire_chance.fires, fire_chance.eligible_hits))),
    );
    if let Some(expected) = fire_chance::expected_fires_text(fire_chance) {
        headline = headline.child(div().text_xs().text_color(crate::theme::text_dim()).child(detail_text(expected)));
    }
    headline = headline.child(copy_breakdown_button(row_ix, fire_chance));
    col = col.child(headline).child(
        div()
            .text_xs()
            .text_color(crate::theme::text_dim())
            .child(detail_text(fire_chance::ships_text(fire_chance).into_owned())),
    );

    // The attacker-side formula: the shell's own chance and every modifier
    // that moved it. Monospace, because the columns are padded to line up.
    let formula = fire_chance::fire_chance_formula_lines(fire_chance, &raw_source);
    if !formula.is_empty() {
        col = col.child(Separator::horizontal()).child(
            v_flex()
                .font_family("monospace")
                .children(formula.into_iter().map(|line| div().text_xs().child(detail_text(line)))),
        );
    }

    // What became of the shells, and then of the fire ribbons themselves. The
    // egui block keeps these behind a second collapse; here the whole block
    // is already behind the row's own expansion.
    col =
        col.child(Separator::horizontal()).child(tally_block(&fire_chance::fire_chance_battle_tally_rows(fire_chance)));
    let ribbons = fire_chance::fire_chance_ribbon_rows(fire_chance);
    if !ribbons.is_empty() {
        col = col.child(tally_block(&ribbons));
    }

    // One row per target ship, most-sampled first: a rate only means
    // something inside a single victim, whose fire resistance is fixed.
    let ships = fire_chance::sorted_per_ship(fire_chance);
    if !ships.is_empty() {
        col = col
            .child(Separator::horizontal())
            .child(section_heading(t!("ui.replay.sections.fire_chance_per_ship").into_owned()));
        for (ship_ix, ship) in ships.iter().enumerate() {
            col = col.child(
                h_flex()
                    .id(("replay-fire-chance-ship", row_ix * FIRE_CHANCE_SHIP_STRIDE + ship_ix))
                    .gap_2()
                    .items_center()
                    .text_xs()
                    .child(
                        div().flex_none().w(FIRE_CHANCE_SHIP_WIDTH).child(detail_text(ship.victim_ship_name.clone())),
                    )
                    .child(
                        div()
                            .text_color(crate::theme::text_dim())
                            .child(detail_text(fire_chance::counts_text(ship.fires, ship.eligible_hits))),
                    ),
            );
        }
        if let Some(line) = fire_chance::no_target_ship_line(fire_chance) {
            col = col.child(div().text_xs().text_color(crate::theme::text_faint()).child(detail_text(line)));
        }
    }

    Some(col.into_any_element())
}

/// A count-and-label listing: counts in a column of their own, labels
/// indented by the depth they sit at.
#[track_caller]
fn tally_block(rows: &[fire_chance::TallyRow]) -> AnyElement {
    let location = std::panic::Location::caller();
    v_flex()
        .id(SharedString::from(format!("tally-{}", location.line())))
        .gap_px()
        .children(rows.iter().enumerate().map(|(index, row)| {
            h_flex()
                .id(index)
                .gap_2()
                .items_start()
                .text_xs()
                .child(
                    div().flex_none().w(TALLY_COUNT_WIDTH).text_right().child(detail_text(separate_number(row.count))),
                )
                .child(
                    div()
                        .pl(px(row.depth as f32 * 12.))
                        .when(row.depth > 0, |cell| cell.text_color(crate::theme::text_dim()))
                        .child(detail_text(row.label.to_string())),
                )
        }))
        .into_any_element()
}

/// The whole breakdown on the clipboard, which is what the egui headline does
/// when it is clicked.
fn copy_breakdown_button(row_ix: usize, fire_chance: &EffectiveFireChance) -> AnyElement {
    let text = fire_chance::copy_text(fire_chance, &raw_source, &raw_ship_name);
    Button::new(("replay-fire-chance-copy", row_ix))
        .child(crate::icons::icon(crate::icons::Glyph::Copy))
        .compact()
        .ghost()
        .xsmall()
        .tooltip(t!("ui.replay.sections.fire_chance_click_to_copy").into_owned())
        .on_click(move |_event, window, cx: &mut App| {
            cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(text.clone()));
            crate::toast::ok(t!("ui.replay.sections.fire_chance_copied").into_owned(), window, cx);
        })
        .into_any_element()
}

/// A modifier's raw source identifier, unresolved.
///
/// The egui block resolves it against the build's own upgrade, signal and
/// skill names; that lookup needs the metadata provider, which the expanded
/// renderer is not handed.
fn raw_source(source: &str) -> String {
    source.to_owned()
}

/// A victim's internal ship name, which is what the per-ship rows above
/// already show for the same reason.
fn raw_ship_name(ship: &wows_replay_insights::fire_chance::analysis::PerShipFireChance) -> String {
    ship.victim_ship_name.clone()
}

/// Room for the widest count a tally row states.
const TALLY_COUNT_WIDTH: Pixels = px(44.);

/// Room for a ship name beside its counts, so the two columns line up.
const FIRE_CHANCE_SHIP_WIDTH: Pixels = px(160.);

/// Keeps one row's per-ship element ids clear of the next row's.
const FIRE_CHANCE_SHIP_STRIDE: usize = 64;

/// Name-column expanded content: achievements ("name (Nx)" once a count
/// exceeds 1), ribbons (sorted by name, with the RIBBON_BULGE-after-
/// RIBBON_MAIN_CALIBER one-off reorder), the fires/floods/citadels/crits
/// damage-event counts, NDA-gated, and then the effective fire chance.
/// Mirrors `mod.rs`'s `ReplayColumn::Name` expanded arm.
fn render_name_section(row_ix: usize, row: &PlayerRow, debug: bool, icons: &IconCache) -> Option<AnyElement> {
    let has_achievements = !row.achievements.is_empty();
    let has_ribbons = !row.ribbons.is_empty();
    let has_damage_events =
        row.fires.is_some() || row.floods.is_some() || row.citadels.is_some() || row.crits.is_some();
    if !has_achievements && !has_ribbons && !has_damage_events && row.fire_chance.is_none() {
        return None;
    }

    // 3px matches egui's default `Spacing::item_spacing.y`: the section
    // heading, each achievement row, the ribbon grid, the separators, and the
    // damage event lines are all top-level children of one `ui.vertical` in
    // the egui original, so they all share that one gap -- not a larger gap
    // at this level and a tighter one nested inside `achievements_view`.
    // Set in from the table's edge, which the Name column starts at and the
    // ribbons would otherwise sit against.
    let mut col = v_flex().gap(px(3.)).pl(NAME_SECTION_INDENT);

    if has_achievements {
        col = col.child(section_heading(t!("ui.replay.sections.achievements").into_owned()));
        col = col.child(achievements_view(row_ix, &row.achievements, icons));
    }

    if has_ribbons {
        if has_achievements {
            col = col.child(Separator::horizontal());
        }
        col = col.child(section_heading(t!("ui.replay.sections.ribbons").into_owned()));
        let mut ribbons: Vec<&RibbonResult> = row.ribbons.iter().collect();
        ribbons.sort_by(|a, b| a.name.cmp(&b.name));
        reorder_bulge_after_main_caliber(&mut ribbons);
        col = col.child(ribbons_view(row_ix, ribbons, icons));
    }

    if has_damage_events {
        if has_achievements || has_ribbons {
            col = col.child(Separator::horizontal());
        }
        col = col.child(section_heading(t!("ui.replay.sections.damage_events").into_owned()));
        if row.should_hide_stats() && !debug {
            col = col.child(nda_text());
        } else {
            col = col.child(breakdown_grid(&damage_event_rows(row)));
        }
    }

    // The effective fire chance closes the column, which is where the egui
    // block sits (`mod.rs:2206-2215`).
    if let Some(fire_chance) = render_fire_chance_section(row_ix, row, debug) {
        if has_achievements || has_ribbons || has_damage_events {
            col = col.child(Separator::horizontal());
        }
        col = col.child(fire_chance);
    }

    Some(col.into_any_element())
}

/// The damage events a row records, as a breakdown: one line per event kind
/// the battle produced.
fn damage_event_rows(row: &PlayerRow) -> Vec<BreakdownRow> {
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

/// One-off fix ported verbatim from the egui app: insert RIBBON_BULGE (torp
/// protection) immediately after RIBBON_MAIN_CALIBER, if both are present.
fn reorder_bulge_after_main_caliber(ribbons: &mut Vec<&RibbonResult>) {
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

/// A tight vertical list of achievement rows (icon + label side by side),
/// matching the egui app's `ui.vertical` of per-achievement `ui.horizontal`
/// rows -- one achievement per line, not a wrapping icon flow. The 3px gap
/// matches egui's default `Spacing::item_spacing.y` and keeps the row-to-row
/// rhythm identical to the outer section's heading-to-first-row gap
/// (`render_name_section`'s `v_flex().gap(px(3.))`), instead of a different
/// nested gap that would read uneven against it.
fn achievements_view(row_ix: usize, achievements: &[AchievementResult], icons: &IconCache) -> AnyElement {
    let mut col = v_flex().gap(px(3.));
    for (idx, achievement) in achievements.iter().enumerate() {
        col = col.child(achievement_row(row_ix, idx, achievement, icons));
    }
    col.into_any_element()
}

/// Ribbon tiles in the already-sorted/reordered display order, wrapping to
/// the column width. Ribbons that stand alone come first; each ribbon with
/// sub-ribbons then starts a line of its own with its sub-ribbons after it,
/// so a family reads as one block rather than breaking into the plain run.
/// `items_end` lines the counts of one line up despite unequal icon heights.
fn ribbons_view(row_ix: usize, ribbons: Vec<&RibbonResult>, icons: &IconCache) -> AnyElement {
    let RibbonGroups { standalone, families } = group_ribbons(ribbons);
    let mut next_idx = 0;
    let mut grid =
        |tiles: Vec<&RibbonResult>| {
            let first = next_idx;
            next_idx += tiles.len();
            h_flex().w_full().flex_wrap().items_end().gap(px(RIBBON_TILE_GAP)).children(
                tiles.into_iter().enumerate().map(|(at, ribbon)| ribbon_tile(row_ix, first + at, ribbon, icons)),
            )
        };
    let mut col = v_flex().gap(px(RIBBON_TILE_GAP * 2.0));
    if !standalone.is_empty() {
        col = col.child(grid(standalone));
    }
    for family in families {
        col = col.child(grid(family));
    }
    col.into_any_element()
}

/// Ribbons split for display: those with no sub-ribbons, and each ribbon
/// that has some followed by them.
struct RibbonGroups<'a> {
    standalone: Vec<&'a RibbonResult>,
    families: Vec<Vec<&'a RibbonResult>>,
}

/// Splits `ordered` into [`RibbonGroups`]. The name order already places
/// each sub-ribbon after its parent, so a sub-ribbon belongs to the nearest
/// ribbon before it that is not one.
fn group_ribbons(ordered: Vec<&RibbonResult>) -> RibbonGroups<'_> {
    let mut runs: Vec<Vec<&RibbonResult>> = Vec::new();
    for ribbon in ordered {
        match runs.last_mut() {
            Some(run) if ribbon.is_subribbon => run.push(ribbon),
            _ => runs.push(vec![ribbon]),
        }
    }
    let mut standalone = Vec::new();
    let mut families = Vec::new();
    for run in runs {
        match run.as_slice() {
            [only] if !only.is_subribbon => standalone.push(*only),
            _ => families.push(run),
        }
    }
    RibbonGroups { standalone, families }
}

fn achievement_row(row_ix: usize, idx: usize, achievement: &AchievementResult, icons: &IconCache) -> AnyElement {
    let key = format!("achievement:{}", achievement.icon_key);
    let label = if achievement.count > 1 {
        format!("{} ({}x)", achievement.display_name, achievement.count)
    } else {
        achievement.display_name.clone()
    };
    icon_label_row(
        "replay-achievement",
        row_ix,
        idx,
        &key,
        label,
        achievement.description.clone(),
        LARGE_ICON_SIZE,
        icons,
    )
}

/// One ribbon: its icon fitted within its box (or, absent an icon, its name)
/// over its count. The count reads "x20" rather than a bare "20" so it is
/// read as a multiplier on the ribbon, not as a score. Hovering shows the
/// icon larger with the name and description (`IconTooltip`).
fn ribbon_tile(row_ix: usize, idx: usize, ribbon: &RibbonResult, icons: &IconCache) -> AnyElement {
    let key = if ribbon.is_subribbon {
        format!("subribbon:{}", ribbon.icon_key)
    } else {
        format!("ribbon:{}", ribbon.icon_key)
    };
    let size = if ribbon.is_subribbon { LARGE_ICON_SIZE } else { RIBBON_ICON_SIZE };
    let image = icons.get_keyed(&key);
    let preview = image.as_ref().map(|image| IconPreview::new(Arc::clone(image), IconBackdrop::Bare, size));
    let icon = match image {
        Some(image) => {
            let (w, h) = fit_within_box(&image, size);
            img(image).w(px(w)).h(px(h)).flex_none().into_any_element()
        }
        None => {
            div().w_full().text_xs().text_center().child(detail_text(ribbon.display_name.clone())).into_any_element()
        }
    };
    let tooltip = IconTooltip {
        preview,
        name: ribbon.display_name.clone().into(),
        description: description_text(ribbon.description.as_deref()),
    };
    v_flex()
        .id(("replay-ribbon", row_ix * DETAIL_ID_STRIDE + idx))
        .w(px(RIBBON_TILE_WIDTH))
        .flex_none()
        .items_center()
        .gap_0p5()
        .child(icon)
        .child(div().text_xs().child(detail_text(format!("x{}", ribbon.count))))
        .tooltip(move |window, cx| tooltip.build().build(window, cx))
        .into_any_element()
}

/// Skills-column expanded content: the skill-tier hover text, then (when this
/// replay observed the vehicle's build) modernizations, signals, loadout, and
/// the captain-skill grid, then the consumable inventory (or, absent any
/// recorded activations, the configured ability names). Mirrors `mod.rs`'s
/// `ReplayColumn::Skills` expanded arm, gated the same way: not an enemy row,
/// unless `debug`.
fn render_build_section(row_ix: usize, row: &PlayerRow, debug: bool, icons: &IconCache) -> Option<AnyElement> {
    if row.relation.is_enemy() && !debug {
        return None;
    }

    let mut col = v_flex().gap_1();
    let mut has_content = false;

    if let Some(hover) = row.skill_hover_text.as_ref() {
        col = col.child(body_text(hover.clone()));
        has_content = true;
    }

    if let Some(build) = row.translated_build.as_ref() {
        if has_content {
            col = col.child(Separator::horizontal());
        }
        if build.modernization_slots.is_empty() {
            col = col.child(body_text(t!("ui.replay.sections.modules_none").into_owned()));
        } else {
            col = col.child(section_heading(t!("ui.replay.sections.modules").into_owned()));
            col = col.child(modernization_slots_view(row_ix, &build.modernization_slots, icons));
        }
        has_content = true;

        if !build.signals.is_empty() {
            col = col.child(Separator::horizontal());
            col = col.child(section_heading(t!("ui.replay.sections.signals").into_owned()));
            col = col.child(signals_view(row_ix, &build.signals, icons));
        }

        col = col.child(Separator::horizontal());
        if build.loadout.is_empty() {
            col = col.child(body_text(t!("ui.replay.sections.loadout_none").into_owned()));
        } else {
            col = col.child(section_heading(t!("ui.replay.sections.loadout").into_owned()));
            for (idx, module) in build.loadout.iter().enumerate() {
                if let Some(name) = module.name.as_ref() {
                    col = col.child(text_row("replay-loadout", row_ix, idx, name.clone(), module.description.clone()));
                }
            }
        }

        col = col.child(Separator::horizontal());
        match build.captain_skills.as_ref() {
            Some(skills) => {
                col = col.child(section_heading(t!("ui.replay.sections.captain_skills").into_owned()));
                if skills.is_empty() {
                    col = col.child(body_text(t!("ui.replay.sections.captain_skills_none").into_owned()));
                } else {
                    col = col.child(captain_skill_grid_view(row_ix, skills, icons));
                }
            }
            None => {
                col = col.child(body_text(t!("ui.replay.sections.captain_skills_none").into_owned()));
            }
        }
    }

    if !row.consumables.is_empty() {
        if has_content {
            col = col.child(Separator::horizontal());
        }
        col = col.child(section_heading(t!("ui.replay.sections.consumables").into_owned()));
        col = col.child(consumables_view(row_ix, &row.consumables, icons));
        has_content = true;
    } else if let Some(build) = row.translated_build.as_ref()
        && !build.abilities.is_empty()
    {
        if has_content {
            col = col.child(Separator::horizontal());
        }
        col = col.child(section_heading(t!("ui.replay.sections.consumables").into_owned()));
        for (idx, ability) in build.abilities.iter().enumerate() {
            if let Some(name) = ability.name.as_ref() {
                col = col.child(text_row("replay-ability", row_ix, idx, name.clone(), None));
            }
        }
        has_content = true;
    }

    if has_content { Some(col.into_any_element()) } else { None }
}

fn modernization_slots_view(row_ix: usize, slots: &[Option<TranslatedModule>], icons: &IconCache) -> AnyElement {
    let mut row = h_flex().flex_wrap().gap_1();
    for (idx, slot) in slots.iter().enumerate() {
        row = row.child(match slot {
            Some(module) => {
                let display_name = module.name.clone().unwrap_or_else(|| module.game_params_name.clone());
                icon_or_text_cell(
                    "replay-modernization",
                    row_ix,
                    idx,
                    &format!("modernization:{}", module.game_params_name),
                    display_name,
                    module.description.as_deref(),
                    MODULE_ICON_SIZE,
                    IconBackdrop::DarkTile,
                    icons,
                )
            }
            None => empty_slot_box(),
        });
    }
    row.into_any_element()
}

/// A placeholder for an unfilled modernization slot, matching the egui app's
/// faint filled rect (`render_modernization_slots`'s `None` arm), sized like
/// a filled slot's tile.
fn empty_slot_box() -> AnyElement {
    div()
        .w(px(MODULE_TILE_SIZE))
        .h(px(MODULE_TILE_SIZE))
        .flex()
        .items_center()
        .justify_center()
        .child(div().text_xs().text_color(crate::theme::text_faint()).child("-"))
        .into_any_element()
}

fn signals_view(row_ix: usize, signals: &[TranslatedModule], icons: &IconCache) -> AnyElement {
    let mut row = h_flex().flex_wrap().gap_1();
    for (idx, signal) in signals.iter().enumerate() {
        let display_name = signal.name.clone().unwrap_or_else(|| signal.game_params_name.clone());
        row = row.child(icon_or_text_cell(
            "replay-signal",
            row_ix,
            idx,
            &format!("signal:{}", signal.game_params_name),
            display_name,
            signal.description.as_deref(),
            MODULE_ICON_SIZE,
            IconBackdrop::Bare,
            icons,
        ));
    }
    row.into_any_element()
}

/// Plain text with an optional hover tooltip, shared by the loadout list and
/// the no-activations-recorded ability-name fallback list.
fn text_row(tag: &'static str, row_ix: usize, idx: usize, text: String, tooltip_text: Option<String>) -> AnyElement {
    let base = div().id((tag, row_ix * DETAIL_ID_STRIDE + idx)).text_xs().child(detail_text(text));
    match tooltip_text.filter(|t| !t.is_empty()) {
        Some(hover) => {
            let hover: SharedString = hover.into();
            base.id((tag, row_ix * DETAIL_ID_STRIDE + idx))
                .tooltip(move |window, cx| Tooltip::new(hover.clone()).build(window, cx))
                .into_any_element()
        }
        None => base.into_any_element(),
    }
}

/// The captain-skill grid: one horizontal line per skill-point tier, a
/// point-cost label, then a cell per skill in that tier -- a decoded icon on
/// a dark tile (`GAME_ICON_TILE_BG`; the glyph dimmed when not learned) or,
/// absent one, `"(cost) name"` text (dimmed the same way). Mirrors
/// `render_skill_grid`.
fn captain_skill_grid_view(row_ix: usize, rows: &[SkillGridRow], icons: &IconCache) -> AnyElement {
    let mut col = v_flex().gap_1();
    let mut skill_idx = 0usize;
    for row in rows {
        let mut line = h_flex().gap_1().items_center();
        let cost_label = row.point_cost.map(|c| c.get().to_string()).unwrap_or_default();
        line = line.child(
            div().w(px(14.)).flex_none().text_xs().text_color(crate::theme::text_dim()).child(detail_text(cost_label)),
        );
        for skill in &row.skills {
            line = line.child(skill_cell(row_ix, skill_idx, skill, icons));
            skill_idx += 1;
        }
        col = col.child(line);
    }
    col.into_any_element()
}

fn skill_cell(row_ix: usize, idx: usize, skill: &SkillGridSkill, icons: &IconCache) -> AnyElement {
    let display_name = skill.name.clone().unwrap_or_else(|| skill.internal_name.as_str().to_string());
    let cost = skill.point_cost.map(|c| format!(" ({} pt)", c.get())).unwrap_or_default();
    let key = format!("skill:{}", skill.internal_name.as_str());
    let image = icons.get_keyed(&key);
    let tooltip = IconTooltip {
        preview: image
            .as_ref()
            .map(|image| IconPreview::new(Arc::clone(image), IconBackdrop::DarkTile, MODULE_ICON_SIZE)),
        name: format!("{display_name}{cost}").into(),
        description: description_text(skill.description.as_deref()),
    };

    let el = match image {
        Some(image) => {
            // Dim only the glyph: the tile stays put so the grid keeps its
            // shape, and white against dim gray separates learned skills.
            let opacity = if skill.learned { 1.0 } else { 0.35 };
            game_icon_tile(img(image).opacity(opacity), MODULE_ICON_SIZE)
        }
        None => {
            let label = match skill.point_cost {
                Some(c) => format!("({}) {display_name}", c.get()),
                None => display_name,
            };
            div()
                .text_xs()
                .when(!skill.learned, |this| this.text_color(crate::theme::text_faint()))
                .child(detail_text(label))
        }
    };
    el.id(("replay-skill", row_ix * DETAIL_ID_STRIDE + idx))
        .tooltip(move |window, cx| tooltip.build().build(window, cx))
        .into_any_element()
}

/// The 3-column consumable inventory (Consumable/Remaining/Total). Mirrors
/// `render_consumable_inventory`, including its `ChargeCount::Unlimited`
/// quirk: the "remaining" column shows the activation count (there is no
/// remaining-charges concept for an unlimited consumable) and "total" holds
/// the infinity mark instead of a number.
fn consumables_view(row_ix: usize, consumables: &[ConsumableResult], icons: &IconCache) -> AnyElement {
    const NAME_COL: f32 = 170.0;
    const COUNT_COL: f32 = 64.0;

    // The name column needs no heading: the section's own heading says what
    // the rows are.
    let header = h_flex()
        .gap_2()
        .child(div().w(px(NAME_COL)).flex_none())
        .child(
            div()
                .w(px(COUNT_COL))
                .flex_none()
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .child(detail_text(t!("ui.replay.consumable_header_remaining").into_owned())),
        )
        .child(
            div()
                .w(px(COUNT_COL))
                .flex_none()
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .child(detail_text(t!("ui.replay.consumable_header_total").into_owned())),
        );

    let mut col = v_flex().gap_1().child(header);
    for (idx, consumable) in consumables.iter().enumerate() {
        col = col.child(consumable_row(row_ix, idx, consumable, icons));
    }
    col.into_any_element()
}

fn consumable_row(row_ix: usize, idx: usize, consumable: &ConsumableResult, icons: &IconCache) -> AnyElement {
    const NAME_COL: f32 = 170.0;
    const COUNT_COL: f32 = 64.0;

    let (remaining_text, total_text, hover_text) = match consumable.total_charges {
        ChargeCount::Unlimited => (
            consumable.charges_used.to_string(),
            t!("ui.replay.consumable_charges_infinite").into_owned(),
            t!("ui.replay.consumable_charges_unlimited", used = consumable.charges_used).into_owned(),
        ),
        ChargeCount::Finite(total) => {
            let remaining = total.saturating_sub(consumable.charges_used);
            (
                remaining.to_string(),
                total.to_string(),
                t!("ui.replay.consumable_charges_remaining", remaining = remaining, total = total).into_owned(),
            )
        }
    };

    let key = format!("consumable:{}", consumable.icon_key);
    let image = icons.get_keyed(&key);
    let tooltip = IconTooltip {
        preview: image
            .as_ref()
            .map(|image| IconPreview::new(Arc::clone(image), IconBackdrop::DarkTile, CONSUMABLE_ICON_SIZE)),
        name: consumable.display_name.clone().into(),
        description: Some(hover_text.into()),
    };

    let mut name_cell = h_flex().w(px(NAME_COL)).flex_none().gap_1().items_center();
    if let Some(image) = image {
        name_cell = name_cell.child(game_icon_tile(img(image), CONSUMABLE_ICON_SIZE));
    }
    name_cell = name_cell.child(div().text_xs().child(detail_text(consumable.display_name.clone())));

    h_flex()
        .id(("replay-consumable", row_ix * DETAIL_ID_STRIDE + idx))
        .gap_2()
        .items_center()
        .child(name_cell)
        .child(div().w(px(COUNT_COL)).flex_none().text_xs().child(detail_text(remaining_text)))
        .child(div().w(px(COUNT_COL)).flex_none().text_xs().child(detail_text(total_text)))
        .tooltip(move |window, cx| tooltip.build().build(window, cx))
        .into_any_element()
}

/// ActualDamage/ReceivedDamage expanded content: the per-type breakdown
/// (`row.actual_damage_by_type`/`received_damage_by_type`), then a
/// per-victim (or per-attacker) grid of `amount (pct%)` against ship name,
/// sorted by amount descending, skipping zero entries. NDA-gated like the collapsed cell (`should_hide_stats() &&
/// !debug`), mirroring the egui app's `dealt_damage_details`/
/// `received_damage_details`. No heading: each side sits under its own column
/// (ActualDamage/ReceivedDamage), so the column header already names it, just
/// as in egui.
fn render_damage_section(
    row: &PlayerRow,
    all_rows: &[PlayerRow],
    debug: bool,
    direction: DamageDirection,
    alt_held: bool,
) -> Option<AnyElement> {
    if row.should_hide_stats() && !debug {
        return Some(nda_text());
    }

    let breakdown = match direction {
        DamageDirection::Dealt => row.actual_damage_by_type.as_deref(),
        DamageDirection::Received => row.received_damage_by_type.as_deref(),
    };
    let interactions = row.damage_interactions.as_ref();
    if breakdown.is_none() && interactions.is_none() {
        return None;
    }

    let mut col = v_flex().gap_1();

    if let Some(lines) = breakdown {
        col = col.child(damage_lines_grid(lines));
        if interactions.is_some() {
            col = col.child(Separator::horizontal());
        }
    }

    if interactions.is_some() {
        let section = InteractionSection {
            direction,
            this: Party { player: row.display_name.clone(), ship: row.ship_name.clone() },
            lines: damage_interactions(row, all_rows, direction),
        };
        col = col.child(interaction_grid(Rc::new(section), alt_held));
    }

    Some(col.into_any_element())
}

/// The mode alt puts the damage breakdowns in, as the status bar names it.
pub(super) const SHARE_MODE: ModeHint = ModeHint { key: ModifierKey::Alt, name_key: "ui.replay.damage_share.mode" };

/// Whether alt changes anything on screen: some row among `open`, the rows
/// showing their detail, lists damage shares under one of `columns`.
pub(super) fn shows_damage_shares<'a>(
    open: impl IntoIterator<Item = &'a PlayerRow>,
    columns: impl IntoIterator<Item = ReplayColumn>,
    debug: bool,
) -> bool {
    let damage_shown =
        columns.into_iter().any(|col| matches!(col, ReplayColumn::ActualDamage | ReplayColumn::ReceivedDamage));

    damage_shown
        && open.into_iter().any(|row| {
            let shown = !row.should_hide_stats() || debug;
            shown && row.damage_interactions.as_ref().is_some_and(|interactions| !interactions.is_empty())
        })
}

/// A player as a share tooltip names them: by player and ship, as two rows
/// can be in the same ship.
struct Party {
    player: String,
    ship: String,
}

impl Party {
    fn named(&self) -> String {
        format!("{} ({})", self.player, self.ship)
    }
}

/// One ship this row dealt damage to or took it from.
struct InteractionLine {
    other: Party,
    amount: u64,
    /// Percent of this row's total.
    share: f64,
    /// Percent of the other player's total.
    inverse_share: f64,
}

impl InteractionLine {
    fn shown_share(&self, inverted: bool) -> f64 {
        if inverted { self.inverse_share } else { self.share }
    }

    fn value(&self, inverted: bool) -> String {
        format!("{} ({:.0}%)", separate_number(self.amount), self.shown_share(inverted))
    }

    fn breakdown(&self, inverted: bool) -> BreakdownRow {
        BreakdownRow { label: self.other.ship.clone(), value: self.value(inverted) }
    }
}

/// One damage section's per-ship lines, shared with every line's tooltip.
struct InteractionSection {
    direction: DamageDirection,
    this: Party,
    lines: Vec<InteractionLine>,
}

/// One line per ship this row dealt damage to or took it from, largest
/// first.
fn damage_interactions(row: &PlayerRow, all_rows: &[PlayerRow], direction: DamageDirection) -> Vec<InteractionLine> {
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
            Some(InteractionLine {
                other: Party { player: other.display_name.clone(), ship: other.ship_name.clone() },
                amount,
                share: interaction_percentage(interaction, direction, false),
                inverse_share: interaction_percentage(interaction, direction, true),
            })
        })
        .collect()
}

/// `breakdown_grid` for the per-ship lines, each line explaining its share
/// on hover. The tooltip reads alt itself when drawn, so one left open while
/// alt changes follows the line it explains.
fn interaction_grid(section: Rc<InteractionSection>, alt_held: bool) -> AnyElement {
    let explained = |index: usize, id: &'static str, text: String| {
        let section = Rc::clone(&section);
        div().id((id, index)).child(detail_text(text)).tooltip(move |window, cx| {
            let section = Rc::clone(&section);
            Tooltip::element(move |window, cx| {
                let inverted = crate::status_bar::is_held(ModifierKey::Alt, window, cx);
                let line = &section.lines[index];
                let text = ShareText {
                    direction: section.direction,
                    inverted,
                    share: line.shown_share(inverted),
                    this: &section.this.named(),
                    other: &line.other.named(),
                    key: ModifierKey::Alt.label(cx),
                };
                div().child(text.said())
            })
            .build(window, cx)
        })
    };
    h_flex()
        .id("damage-interactions")
        .gap(px(BREAKDOWN_GAP))
        .items_start()
        .text_xs()
        .child(
            v_flex().gap_px().children(
                section
                    .lines
                    .iter()
                    .enumerate()
                    .map(|(index, line)| explained(index, "damage-interaction-ship", line.other.ship.clone())),
            ),
        )
        .child(
            v_flex().gap_px().items_end().children(
                section
                    .lines
                    .iter()
                    .enumerate()
                    .map(|(index, line)| explained(index, "damage-interaction-share", line.value(alt_held))),
            ),
        )
        .into_any_element()
}

/// What a damage line's share means in the mode it is shown in, then how to
/// see the other one.
struct ShareText<'a> {
    direction: DamageDirection,
    inverted: bool,
    share: f64,
    /// The row the line is under.
    this: &'a str,
    /// The player the line names.
    other: &'a str,
    /// The key that inverts the share.
    key: &'a str,
}

impl ShareText<'_> {
    /// While inverted the key is already down, so the hint says to let it go
    /// rather than to hold it.
    fn said(&self) -> String {
        let (pct, ship, other) = (format!("{:.0}%", self.share), self.this, self.other);
        let said = match (self.direction, self.inverted) {
            (DamageDirection::Dealt, false) => {
                t!("ui.replay.damage_share.dealt", pct = pct, ship = ship, other = other)
            }
            (DamageDirection::Dealt, true) => {
                t!("ui.replay.damage_share.dealt_inverse", pct = pct, ship = ship, other = other)
            }
            (DamageDirection::Received, false) => {
                t!("ui.replay.damage_share.received", pct = pct, ship = ship, other = other)
            }
            (DamageDirection::Received, true) => {
                t!("ui.replay.damage_share.received_inverse", pct = pct, ship = ship, other = other)
            }
        };
        let hint = if self.inverted {
            t!("ui.replay.damage_share.release", key = self.key)
        } else {
            t!("ui.replay.damage_share.hold", key = self.key)
        };

        format!("{said}\n{hint}")
    }
}

/// Mirrors `render_damage_section`: the breakdown above, and the per-ship
/// lines below it at whichever share alt shows, so the column fits either.
fn damage_section_width(
    breakdown: Option<&[DamageLine]>,
    row: &PlayerRow,
    all_rows: &[PlayerRow],
    debug: bool,
    direction: DamageDirection,
    window: &mut Window,
) -> f32 {
    if row.should_hide_stats() && !debug {
        return text_width(NDA.into(), window, false);
    }

    let mut widest = breakdown.map(|lines| damage_lines_width(lines, window)).unwrap_or(0.0);
    let lines = damage_interactions(row, all_rows, direction);
    for alt_held in [false, true] {
        let rows: Vec<BreakdownRow> = lines.iter().map(|line| line.breakdown(alt_held)).collect();
        widest = widest.max(breakdown_grid_width(&rows, window));
    }
    widest
}

fn interaction_amount(interaction: &DamageInteraction, direction: DamageDirection) -> u64 {
    match direction {
        DamageDirection::Dealt => interaction.damage_dealt,
        DamageDirection::Received => interaction.damage_received,
    }
}

/// The share this line is of a total. Normally of this row's own total; with
/// alt down, of the other player's, which answers "how much of what they took
/// was me" rather than "how much of my damage went there".
fn interaction_percentage(interaction: &DamageInteraction, direction: DamageDirection, alt_held: bool) -> f64 {
    match (direction, alt_held) {
        (DamageDirection::Dealt, false) => interaction.damage_dealt_percentage,
        (DamageDirection::Dealt, true) => interaction.damage_dealt_inverse_percentage,
        (DamageDirection::Received, false) => interaction.damage_received_percentage,
        (DamageDirection::Received, true) => interaction.damage_received_inverse_percentage,
    }
}

/// SpottingDamage expanded content: the recording player's per-type split
/// (`row.spotting_damage_breakdown`), then a per-victim grid of
/// `amount (pct%)` against ship name, mirroring the egui app's
/// `spotting_damage_details`. Not NDA-gated, like the collapsed cell: the
/// figure is damage allies dealt, which says nothing about this row's ship.
fn render_spotting_section(row: &PlayerRow, all_rows: &[PlayerRow]) -> Option<AnyElement> {
    let breakdown = row.spotting_damage_breakdown.as_deref();
    let per_ship = spotting_target_rows(row, all_rows);
    if breakdown.is_none() && per_ship.is_empty() {
        return None;
    }

    let mut col = v_flex().gap_1();

    if let Some(rows) = breakdown {
        col = col.child(breakdown_grid(rows));
        if !per_ship.is_empty() {
            col = col.child(Separator::horizontal());
        }
    }

    if !per_ship.is_empty() {
        col = col.child(breakdown_grid(&per_ship));
    }

    Some(col.into_any_element())
}

/// One line per spotted victim, largest share first, skipping victims with
/// no spotting damage and those `all_rows` does not name. The share is left
/// off when the results carry no spotting total to divide by.
fn spotting_target_rows(row: &PlayerRow, all_rows: &[PlayerRow]) -> Vec<BreakdownRow> {
    let Some(interactions) = row.damage_interactions.as_ref() else {
        return Vec::new();
    };

    let mut entries: Vec<_> = interactions.iter().filter(|(_, interaction)| interaction.spotting_damage > 0).collect();
    entries.sort_by_key(|(_, interaction)| std::cmp::Reverse(interaction.spotting_damage));

    entries
        .into_iter()
        .filter_map(|(account_id, interaction)| {
            let other = all_rows.iter().find(|r| r.db_id == *account_id)?;
            let amount = separate_number(interaction.spotting_damage);
            let value = match interaction.spotting_damage_percentage {
                Some(pct) => format!("{amount} ({pct:.0}%)"),
                None => amount,
            };
            Some(BreakdownRow { label: other.ship_name.clone(), value })
        })
        .collect()
}

// `use super::*` here would also pull in `gpui_kit::*` (glob-reexported at the
// top of this file), and combined with this file's heavy element-builder
// chains that blows the compiler's macro/name-resolution recursion limit
// while expanding `#[test]` (reproduced: a single trivial `#[test]` fails to
// compile under `use super::*`, but compiles fine with explicit imports).
// Import only the specific pure-logic items these tests exercise instead.
#[cfg(test)]
mod tests {
    use wows_replay_insights::battle_report::DamageInteraction;
    use wows_replay_insights::battle_report::RibbonResult;
    use wows_replays::types::AccountId;
    use wows_toolkit_viewmodel::breakdown::BreakdownRow;
    use wowsunpack::game_params::skill_grid_data::SkillGridRow;
    use wowsunpack::game_params::skill_grid_data::SkillGridSkill;
    use wowsunpack::game_params::types::CrewSkillName;
    use wowsunpack::game_params::types::CrewSkillType;
    use wowsunpack::game_params::types::SkillPointCost;

    use super::DamageDirection;
    use super::IconCache;
    use super::MODULE_ICON_SIZE;
    use super::RIBBON_GRID_MIN_TILES;
    use super::RIBBON_TILE_GAP;
    use super::RIBBON_TILE_WIDTH;
    use super::ReplayColumn;
    use super::ShareText;
    use super::consumables_width;
    use super::damage_event_rows;
    use super::fit_dims;
    use super::group_ribbons;
    use super::icon_label_row_width;
    use super::interaction_amount;
    use super::interaction_percentage;
    use super::render_column_detail;
    use super::reorder_bulge_after_main_caliber;
    use super::ribbon_grid_width;
    use super::shows_damage_shares;
    use super::skill_grid_width;
    use super::spotting_target_rows;
    use crate::replay_inspector::test_support::base_row;
    use crate::replay_inspector::test_support::fire_chance_result;
    use wows_replays::types::Relation;

    /// The block expands under Name, after the damage events, which is where
    /// the egui app puts it; Kills has nothing of its own to show.
    #[test]
    fn the_fire_chance_block_expands_under_the_name_column() {
        let mut row = base_row(1, Relation::new(0), true);
        row.fire_chance = Some(fire_chance_result());
        let icons = IconCache::new();

        assert!(
            render_column_detail(0, ReplayColumn::Name, &row, &[], &icons, false, false).is_some(),
            "the Name column carries it"
        );
        assert!(
            render_column_detail(0, ReplayColumn::Kills, &row, &[], &icons, false, false).is_none(),
            "and the Kills column does not"
        );
    }

    /// A breakdown lists what a battle produced and leaves out what it did
    /// not, so the grid draws a line per kind that has something behind it.
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
        // A 220x60 ribbon fitted into a 64 box: 64 wide, proportionally short.
        let (w, h) = fit_dims(220.0, 60.0, 64.0);
        assert!((w - 64.0).abs() < 1e-4, "width should hit the box: {w}");
        assert!((h - 64.0 * 60.0 / 220.0).abs() < 1e-4, "height should scale with aspect: {h}");
        assert!(h < 20.0, "a wide ribbon must render short, got {h}");
    }

    #[test]
    fn fit_dims_leaves_a_square_icon_square() {
        assert_eq!(fit_dims(32.0, 32.0, 32.0), (32.0, 32.0));
        let (w, h) = fit_dims(128.0, 128.0, 32.0);
        assert!((w - 32.0).abs() < 1e-4 && (h - 32.0).abs() < 1e-4);
    }

    #[test]
    fn fit_dims_falls_back_to_a_square_for_degenerate_sizes() {
        assert_eq!(fit_dims(0.0, 10.0, 64.0), (64.0, 64.0));
        assert_eq!(fit_dims(10.0, 0.0, 64.0), (64.0, 64.0));
    }

    fn ribbon(name: &str) -> RibbonResult {
        RibbonResult {
            name: name.to_string(),
            display_name: name.to_string(),
            description: None,
            icon_key: name.to_string(),
            is_subribbon: false,
            count: 1,
        }
    }

    /// Ribbons that stand alone come first; a ribbon with sub-ribbons is
    /// kept with them and moved after every standalone one.
    #[test]
    fn ribbons_with_sub_ribbons_follow_the_standalone_ones() {
        let sub = |name: &str| RibbonResult { is_subribbon: true, ..ribbon(name) };
        let (fire, main, pen, ricochet, spotted, bulge) = (
            ribbon("RIBBON_BURN"),
            ribbon("RIBBON_MAIN_CALIBER"),
            sub("RIBBON_MAIN_CALIBER_PENETRATION"),
            sub("RIBBON_MAIN_CALIBER_RICOCHET"),
            ribbon("RIBBON_SPOTTED"),
            sub("RIBBON_BULGE"),
        );
        let ordered = vec![&fire, &main, &pen, &bulge, &ricochet, &spotted];
        let groups = group_ribbons(ordered);
        let names = |ribbons: &[&RibbonResult]| ribbons.iter().map(|r| r.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&groups.standalone), ["RIBBON_BURN", "RIBBON_SPOTTED"]);
        assert_eq!(groups.families.len(), 1);
        assert_eq!(
            names(&groups.families[0]),
            ["RIBBON_MAIN_CALIBER", "RIBBON_MAIN_CALIBER_PENETRATION", "RIBBON_BULGE", "RIBBON_MAIN_CALIBER_RICOCHET"]
        );
    }

    #[test]
    fn reorder_bulge_after_main_caliber_moves_bulge_when_it_precedes_main_caliber() {
        let bulge = ribbon("RIBBON_BULGE");
        let main_caliber = ribbon("RIBBON_MAIN_CALIBER");
        let other = ribbon("RIBBON_CITADEL");
        let mut ribbons = vec![&bulge, &main_caliber, &other];

        reorder_bulge_after_main_caliber(&mut ribbons);

        let names: Vec<&str> = ribbons.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["RIBBON_MAIN_CALIBER", "RIBBON_BULGE", "RIBBON_CITADEL"]);
    }

    #[test]
    fn reorder_bulge_after_main_caliber_moves_bulge_when_it_follows_main_caliber() {
        let main_caliber = ribbon("RIBBON_MAIN_CALIBER");
        let other = ribbon("RIBBON_CITADEL");
        let bulge = ribbon("RIBBON_BULGE");
        let mut ribbons = vec![&main_caliber, &other, &bulge];

        reorder_bulge_after_main_caliber(&mut ribbons);

        let names: Vec<&str> = ribbons.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["RIBBON_MAIN_CALIBER", "RIBBON_BULGE", "RIBBON_CITADEL"]);
    }

    #[test]
    fn reorder_bulge_after_main_caliber_is_a_no_op_when_either_ribbon_is_absent() {
        let main_caliber = ribbon("RIBBON_MAIN_CALIBER");
        let other = ribbon("RIBBON_CITADEL");
        let mut ribbons = vec![&other, &main_caliber];

        reorder_bulge_after_main_caliber(&mut ribbons);

        let names: Vec<&str> = ribbons.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["RIBBON_CITADEL", "RIBBON_MAIN_CALIBER"]);
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

    /// Each mode says whose total the share is of, and the hint says how to
    /// reach the other mode: hold the key from the plain one, let it go from
    /// the inverted one.
    #[test]
    fn a_damage_share_explains_itself_in_each_mode() {
        rust_i18n::set_locale("en");
        let said = |direction, inverted| {
            ShareText { direction, inverted, share: 34.4, this: "Yamato", other: "Iowa", key: "Alt" }.said()
        };

        assert_eq!(
            said(DamageDirection::Dealt, false),
            "34% of the damage Yamato dealt went to Iowa.\nHold Alt to invert."
        );
        assert_eq!(
            said(DamageDirection::Dealt, true),
            "34% of the damage Iowa received came from Yamato.\nRelease Alt to invert back."
        );
        assert_eq!(
            said(DamageDirection::Received, false),
            "34% of the damage Yamato received came from Iowa.\nHold Alt to invert."
        );
        assert_eq!(
            said(DamageDirection::Received, true),
            "34% of the damage Iowa dealt went to Yamato.\nRelease Alt to invert back."
        );
    }

    /// Alt is worth naming on the status bar only while it changes something
    /// on screen: an open row with damage shares under a shown damage column.
    #[test]
    fn alt_names_a_mode_only_over_damage_shares_on_screen() {
        let mut open = base_row(1, Relation::new(0), true);
        open.damage_interactions = Some([(AccountId(2), interaction(1_000, 10.0, 0, 0.0))].into_iter().collect());
        let bare = base_row(3, Relation::new(1), false);
        let damage = [ReplayColumn::ActualDamage];

        assert!(shows_damage_shares([&open], damage, false));
        assert!(!shows_damage_shares([&bare], damage, false), "the open row has no shares");
        assert!(!shows_damage_shares([], damage, false), "no row is open");
        assert!(!shows_damage_shares([&open], [ReplayColumn::Kills], false), "no damage column is shown");
    }

    /// Largest share first; a victim with no spotting damage, or one no row
    /// names, gets no line.
    #[test]
    fn spotting_lines_list_named_victims_by_amount() {
        let spotted = |spotting_damage: u64, spotting_damage_percentage: Option<f64>| DamageInteraction {
            spotting_damage,
            spotting_damage_percentage,
            ..Default::default()
        };
        let mut row = base_row(1, Relation::new(0), true);
        row.damage_interactions = Some(
            [
                (AccountId(2), spotted(1_000, Some(20.0))),
                (AccountId(3), spotted(4_000, Some(80.0))),
                (AccountId(4), spotted(0, None)),
                (AccountId(9), spotted(500, Some(10.0))),
            ]
            .into_iter()
            .collect(),
        );
        let mut low = base_row(2, Relation::new(2), false);
        low.ship_name = "Low".to_string();
        let mut high = base_row(3, Relation::new(2), false);
        high.ship_name = "High".to_string();
        let none = base_row(4, Relation::new(2), false);

        assert_eq!(
            spotting_target_rows(&row, &[low, high, none]),
            vec![
                BreakdownRow { label: "High".to_string(), value: "4,000 (80%)".to_string() },
                BreakdownRow { label: "Low".to_string(), value: "1,000 (20%)".to_string() },
            ]
        );

        // Without a spotting total there is no share to show.
        let mut row = base_row(1, Relation::new(0), true);
        row.damage_interactions = Some([(AccountId(3), spotted(4_000, None))].into_iter().collect());
        let mut high = base_row(3, Relation::new(2), false);
        high.ship_name = "High".to_string();
        assert_eq!(
            spotting_target_rows(&row, &[high]),
            vec![BreakdownRow { label: "High".to_string(), value: "4,000".to_string() }]
        );
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
        let rows = [skill_tier(7)];
        let width = skill_grid_width(&rows, 4.0);
        // 14 (cost label) + gap_1 + 7 skills * (icon tile + gap_1); the tile
        // pads the 28px icon by 2px on each side.
        let expected = 14.0 + 4.0 + 7.0 * (MODULE_ICON_SIZE + 4.0 + 4.0);
        assert!((width - expected).abs() < 1e-4, "width {width} != expected {expected}");
    }

    /// The grid asks the column for at most `RIBBON_GRID_MIN_TILES` tiles per
    /// line and wraps the rest, rather than one line of every ribbon.
    #[test]
    fn ribbon_grid_width_caps_at_the_minimum_tiles_per_line() {
        assert_eq!(ribbon_grid_width(0), 0.0);
        assert_eq!(ribbon_grid_width(1), RIBBON_TILE_WIDTH);
        assert_eq!(ribbon_grid_width(2), 2.0 * RIBBON_TILE_WIDTH + RIBBON_TILE_GAP);
        assert_eq!(ribbon_grid_width(15), ribbon_grid_width(RIBBON_GRID_MIN_TILES));
    }

    #[test]
    fn skill_grid_width_takes_the_widest_tier() {
        let rows = [skill_tier(1), skill_tier(7), skill_tier(3)];
        let width = skill_grid_width(&rows, 4.0);
        assert!((width - skill_grid_width(&[skill_tier(7)], 4.0)).abs() < 1e-4);
    }

    #[test]
    fn skill_grid_width_of_a_full_tier_far_exceeds_a_typical_collapsed_skills_width() {
        // The collapsed Skills cell is a short label ("Fine 12pt" and change);
        // a 7-skill tier at MODULE_ICON_SIZE per icon must dwarf it, which is
        // the whole point of re-measuring on expand.
        let collapsed_skills_column_width = 90.0;
        let width = skill_grid_width(&[skill_tier(7)], 4.0);
        assert!(
            width > collapsed_skills_column_width,
            "grid width {width} should exceed {collapsed_skills_column_width}"
        );
    }

    #[test]
    fn consumables_width_matches_the_fixed_three_column_layout() {
        // NAME_COL(170) + gap_2 + COUNT_COL(64) + gap_2 + COUNT_COL(64), with
        // gap_2 = 8 at the default 16px rem.
        assert!((consumables_width(8.0) - 314.0).abs() < 1e-4);
    }

    #[test]
    fn icon_label_row_width_adds_the_8px_gap_between_icon_and_label() {
        assert!((icon_label_row_width(64.0, 40.0) - 112.0).abs() < 1e-4);
        assert!((icon_label_row_width(0.0, 40.0) - 48.0).abs() < 1e-4);
    }
}
