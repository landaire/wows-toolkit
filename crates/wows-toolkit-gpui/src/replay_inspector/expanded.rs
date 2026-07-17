//! Expanded row content: achievements, ribbons, and damage events (Name
//! column), captain build (modernizations, signals, loadout, captain skills,
//! consumables; Skills column), and per-victim damage interaction breakdowns
//! (ActualDamage/ReceivedDamage columns). Ported from the egui app's
//! `ui/replay_parser/mod.rs` `if 0.0 < expandedness { match column { ... } }`
//! block (`mod.rs:1834-2088`) plus its `render_skill_grid`/
//! `render_modernization_slots`/`render_signals`/`render_consumable_inventory`/
//! `dealt_damage_details`/`received_damage_details` helpers.
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

use gpui::*;
use gpui_component::ActiveTheme;
use gpui_component::h_flex;
use gpui_component::separator::Separator;
use gpui_component::tooltip::Tooltip;
use gpui_component::v_flex;
use wows_replay_insights::battle_report::AchievementResult;
use wows_replay_insights::battle_report::ConsumableResult;
use wows_replay_insights::battle_report::DamageInteraction;
use wows_replay_insights::battle_report::RibbonResult;
use wows_replay_insights::battle_report::TranslatedModule;
use wowsunpack::game_params::skill_grid_data::SkillGridRow;
use wowsunpack::game_params::skill_grid_data::SkillGridSkill;
use wowsunpack::game_types::ChargeCount;

use super::columns::NDA;
use super::columns::ReplayColumn;
use super::icons::IconCache;
use super::model::PlayerRow;
use super::model::separate_number;

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
/// each column draws its own detail under its own cell -- Name's achievements/
/// ribbons/damage-events under Name, Skills' build under Skills, the damage
/// breakdowns under ActualDamage/ReceivedDamage, and the Potential/Spotting/
/// Hits hover text under those columns. `all_rows` resolves a damage
/// interaction's victim/attacker `db_id` into a ship name.
pub fn render_column_detail(
    ix: usize,
    col: ReplayColumn,
    row: &PlayerRow,
    all_rows: &[PlayerRow],
    icons: &IconCache,
    debug: bool,
    cx: &App,
) -> Option<AnyElement> {
    match col {
        ReplayColumn::Name => render_name_section(ix, row, debug, icons),
        ReplayColumn::Skills => render_build_section(ix, row, debug, icons),
        ReplayColumn::ActualDamage => render_damage_section(ix, row, all_rows, debug, DamageDirection::Dealt, cx),
        ReplayColumn::ReceivedDamage => render_damage_section(ix, row, all_rows, debug, DamageDirection::Received, cx),
        ReplayColumn::PotentialDamage => render_nda_gated_hover(row.potential_damage_hover_text.as_deref(), row, debug),
        ReplayColumn::Hits => render_nda_gated_hover(row.hits_hover_text.as_deref(), row, debug),
        ReplayColumn::SpottingDamage => row.spotting_damage_hover_text.as_deref().map(multiline_body),
        _ => None,
    }
}

/// Potential-damage / Hits expanded content: the column's hover text shown
/// inline, NDA-gated exactly like the egui arms (`mod.rs:1983-1989, 2077-2082`)
/// -- an NDA placeholder when the row's stats are hidden and debug is off,
/// otherwise the hover text (or nothing when there is none).
fn render_nda_gated_hover(text: Option<&str>, row: &PlayerRow, debug: bool) -> Option<AnyElement> {
    if row.should_hide_stats() && !debug {
        return Some(nda_text());
    }
    text.map(multiline_body)
}

/// Plain multi-line body text (one `div` per `\n`-separated line), matching
/// the egui app's `ui.label(hover_text)` for the Potential/Spotting/Hits
/// expanded arms, which render newlines as separate lines.
fn multiline_body(text: &str) -> AnyElement {
    v_flex().gap_0().text_xs().children(text.split('\n').map(|line| div().child(line.to_string()))).into_any_element()
}

fn section_heading(text: &'static str) -> AnyElement {
    div().text_xs().font_weight(FontWeight::BOLD).child(text).into_any_element()
}

fn body_text(text: impl Into<SharedString>) -> AnyElement {
    div().text_xs().child(text.into()).into_any_element()
}

fn nda_text() -> AnyElement {
    div().text_xs().child(NDA).into_any_element()
}

/// A small icon-or-text cell shared by achievements, ribbons, modernizations,
/// and signals: a decoded icon when `icons.get_keyed(key)` has one, otherwise
/// `display_name` as plain text; a hover tooltip either way. `tag` keys the
/// `ElementId` so different lists in the same row never collide (see
/// `DETAIL_ID_STRIDE`'s doc).
#[allow(clippy::too_many_arguments)]
fn icon_or_text_cell(
    tag: &'static str,
    row_ix: usize,
    idx: usize,
    key: &str,
    display_name: String,
    tooltip_text: String,
    size: f32,
    icons: &IconCache,
) -> AnyElement {
    let tooltip: SharedString = tooltip_text.into();
    let el = match icons.get_keyed(key) {
        Some(image) => div().child(img(image).w(px(size)).h(px(size))),
        None => div().text_xs().child(display_name),
    };
    el.id((tag, row_ix * DETAIL_ID_STRIDE + idx))
        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
        .into_any_element()
}

/// An `[icon | label]` row shared by achievements and ribbons: an icon when
/// `icons.get_keyed(key)` has one (no fallback text in its place, matching the
/// egui app's `ui.horizontal` which only conditionally adds the image), then
/// `label` always rendered alongside it. A hover tooltip covers the whole row.
/// `items_center()` vertically centers the icon and label on a shared center
/// line even when the icon (up to `RIBBON_ICON_SIZE`) is much taller than the
/// text. The 8px icon-to-label gap matches egui's default
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
        row = row.child(img(image).w(px(size)).h(px(size)).flex_none());
    }
    row = row.child(div().flex_1().min_w(px(0.)).text_xs().child(label));
    row.id((tag, row_ix * DETAIL_ID_STRIDE + idx))
        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
        .into_any_element()
}

/// Name-column expanded content: achievements ("name (Nx)" once a count
/// exceeds 1), ribbons (sorted by name, with the RIBBON_BULGE-after-
/// RIBBON_MAIN_CALIBER one-off reorder), and the fires/floods/citadels/crits
/// damage-event counts, NDA-gated. Mirrors `mod.rs`'s `ReplayColumn::Name`
/// expanded arm.
fn render_name_section(row_ix: usize, row: &PlayerRow, debug: bool, icons: &IconCache) -> Option<AnyElement> {
    let has_achievements = !row.achievements.is_empty();
    let has_ribbons = !row.ribbons.is_empty();
    let has_damage_events =
        row.fires.is_some() || row.floods.is_some() || row.citadels.is_some() || row.crits.is_some();
    if !has_achievements && !has_ribbons && !has_damage_events {
        return None;
    }

    // 3px matches egui's default `Spacing::item_spacing.y`: the section
    // heading, each achievement/ribbon row, the separators, and the damage
    // event lines are all top-level children of one `ui.vertical` in the
    // egui original, so they all share that one gap -- not a larger gap at
    // this level and a tighter one nested inside `achievements_view`/
    // `ribbons_view`.
    let mut col = v_flex().gap(px(3.));

    if has_achievements {
        col = col.child(section_heading("Achievements"));
        col = col.child(achievements_view(row_ix, &row.achievements, icons));
    }

    if has_ribbons {
        if has_achievements {
            col = col.child(Separator::horizontal());
        }
        col = col.child(section_heading("Ribbons"));
        let mut ribbons: Vec<&RibbonResult> = row.ribbons.iter().collect();
        ribbons.sort_by(|a, b| a.name.cmp(&b.name));
        reorder_bulge_after_main_caliber(&mut ribbons);
        col = col.child(ribbons_view(row_ix, ribbons, icons));
    }

    if has_damage_events {
        if has_achievements || has_ribbons {
            col = col.child(Separator::horizontal());
        }
        col = col.child(section_heading("Damage Events"));
        if row.should_hide_stats() && !debug {
            col = col.child(nda_text());
        } else {
            if let Some(fires) = row.fires {
                col = col.child(body_text(format!("Fires: {fires}")));
            }
            if let Some(floods) = row.floods {
                col = col.child(body_text(format!("Floods: {floods}")));
            }
            if let Some(citadels) = row.citadels {
                col = col.child(body_text(format!("Citadels: {citadels}")));
            }
            if let Some(crits) = row.crits {
                col = col.child(body_text(format!("Crits: {crits}")));
            }
        }
    }

    Some(col.into_any_element())
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

/// A tight vertical list of ribbon rows (icon + label side by side), in the
/// already-sorted/reordered display order. Matches the egui app's `ui.vertical`
/// of per-ribbon `ui.horizontal` rows; see `achievements_view`'s doc for why
/// the row gap matches the outer section gap.
fn ribbons_view(row_ix: usize, ribbons: Vec<&RibbonResult>, icons: &IconCache) -> AnyElement {
    let mut col = v_flex().gap(px(3.));
    for (idx, ribbon) in ribbons.into_iter().enumerate() {
        col = col.child(ribbon_row(row_ix, idx, ribbon, icons));
    }
    col.into_any_element()
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

fn ribbon_row(row_ix: usize, idx: usize, ribbon: &RibbonResult, icons: &IconCache) -> AnyElement {
    let key = if ribbon.is_subribbon {
        format!("subribbon:{}", ribbon.icon_key)
    } else {
        format!("ribbon:{}", ribbon.icon_key)
    };
    let size = if ribbon.is_subribbon { LARGE_ICON_SIZE } else { RIBBON_ICON_SIZE };
    let label = format!("{} ({}x)", ribbon.display_name, ribbon.count);
    icon_label_row("replay-ribbon", row_ix, idx, &key, label, ribbon.description.clone(), size, icons)
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
            col = col.child(body_text("No Modules"));
        } else {
            col = col.child(section_heading("Modules:"));
            col = col.child(modernization_slots_view(row_ix, &build.modernization_slots, icons));
        }
        has_content = true;

        if !build.signals.is_empty() {
            col = col.child(Separator::horizontal());
            col = col.child(section_heading("Signals:"));
            col = col.child(signals_view(row_ix, &build.signals, icons));
        }

        col = col.child(Separator::horizontal());
        if build.loadout.is_empty() {
            col = col.child(body_text("No Loadout"));
        } else {
            col = col.child(section_heading("Loadout:"));
            for (idx, module) in build.loadout.iter().enumerate() {
                if let Some(name) = module.name.as_ref() {
                    col = col.child(text_row("replay-loadout", row_ix, idx, name.clone(), module.description.clone()));
                }
            }
        }

        col = col.child(Separator::horizontal());
        match build.captain_skills.as_ref() {
            Some(skills) => {
                col = col.child(section_heading("Captain Skills:"));
                if skills.is_empty() {
                    col = col.child(body_text("No Captain Skills"));
                } else {
                    col = col.child(captain_skill_grid_view(row_ix, skills, icons));
                }
            }
            None => {
                col = col.child(body_text("No Captain Skills"));
            }
        }
    }

    if !row.consumables.is_empty() {
        if has_content {
            col = col.child(Separator::horizontal());
        }
        col = col.child(section_heading("Consumables:"));
        col = col.child(consumables_view(row_ix, &row.consumables, icons));
        has_content = true;
    } else if let Some(build) = row.translated_build.as_ref()
        && !build.abilities.is_empty()
    {
        if has_content {
            col = col.child(Separator::horizontal());
        }
        col = col.child(section_heading("Consumables:"));
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
                let tooltip = match module.description.as_deref() {
                    Some(d) if !d.is_empty() => format!("{display_name}\n\n{d}"),
                    _ => display_name.clone(),
                };
                icon_or_text_cell(
                    "replay-modernization",
                    row_ix,
                    idx,
                    &format!("modernization:{}", module.game_params_name),
                    display_name,
                    tooltip,
                    MODULE_ICON_SIZE,
                    icons,
                )
            }
            None => empty_slot_box(),
        });
    }
    row.into_any_element()
}

/// A gray placeholder for an unfilled modernization slot, matching the egui
/// app's faint filled rect (`render_modernization_slots`'s `None` arm).
fn empty_slot_box() -> AnyElement {
    div()
        .w(px(MODULE_ICON_SIZE))
        .h(px(MODULE_ICON_SIZE))
        .flex()
        .items_center()
        .justify_center()
        .opacity(0.3)
        .child(div().text_xs().child("-"))
        .into_any_element()
}

fn signals_view(row_ix: usize, signals: &[TranslatedModule], icons: &IconCache) -> AnyElement {
    let mut row = h_flex().flex_wrap().gap_1();
    for (idx, signal) in signals.iter().enumerate() {
        let display_name = signal.name.clone().unwrap_or_else(|| signal.game_params_name.clone());
        let tooltip = match signal.description.as_deref() {
            Some(d) if !d.is_empty() => format!("{display_name}\n\n{d}"),
            _ => display_name.clone(),
        };
        row = row.child(icon_or_text_cell(
            "replay-signal",
            row_ix,
            idx,
            &format!("signal:{}", signal.game_params_name),
            display_name,
            tooltip,
            MODULE_ICON_SIZE,
            icons,
        ));
    }
    row.into_any_element()
}

/// Plain text with an optional hover tooltip, shared by the loadout list and
/// the no-activations-recorded ability-name fallback list.
fn text_row(tag: &'static str, row_ix: usize, idx: usize, text: String, tooltip_text: Option<String>) -> AnyElement {
    let base = div().text_xs().child(text);
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
/// point-cost label, then a cell per skill in that tier -- a decoded icon
/// (dimmed when not learned) or, absent one, `"(cost) name"` text (dimmed the
/// same way). Mirrors `render_skill_grid`.
fn captain_skill_grid_view(row_ix: usize, rows: &[SkillGridRow], icons: &IconCache) -> AnyElement {
    let mut col = v_flex().gap_1();
    let mut skill_idx = 0usize;
    for row in rows {
        let mut line = h_flex().gap_1().items_center();
        let cost_label = row.point_cost.map(|c| c.get().to_string()).unwrap_or_default();
        line = line.child(div().w(px(14.)).flex_none().text_xs().opacity(0.6).child(cost_label));
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
    let tooltip_text: SharedString = match skill.description.as_deref() {
        Some(desc) if !desc.is_empty() => format!("{display_name}{cost}\n\n{desc}"),
        _ => format!("{display_name}{cost}"),
    }
    .into();

    let key = format!("skill:{}", skill.internal_name.as_str());
    let el = match icons.get_keyed(&key) {
        Some(image) => {
            let opacity = if skill.learned { 1.0 } else { 0.35 };
            div().opacity(opacity).child(img(image).w(px(MODULE_ICON_SIZE)).h(px(MODULE_ICON_SIZE)))
        }
        None => {
            let label = match skill.point_cost {
                Some(c) => format!("({}) {display_name}", c.get()),
                None => display_name,
            };
            let text = div().text_xs().child(label);
            if skill.learned { text } else { text.opacity(0.5) }
        }
    };
    el.id(("replay-skill", row_ix * DETAIL_ID_STRIDE + idx))
        .tooltip(move |window, cx| Tooltip::new(tooltip_text.clone()).build(window, cx))
        .into_any_element()
}

/// The 3-column consumable inventory (Consumable/Remaining/Total). Mirrors
/// `render_consumable_inventory`, including its `ChargeCount::Unlimited`
/// quirk: the "remaining" column shows the activation count (there is no
/// remaining-charges concept for an unlimited consumable) and "total" reads
/// "Unlimited" instead of a number.
fn consumables_view(row_ix: usize, consumables: &[ConsumableResult], icons: &IconCache) -> AnyElement {
    const NAME_COL: f32 = 170.0;
    const COUNT_COL: f32 = 64.0;

    let header = h_flex()
        .gap_2()
        .child(div().w(px(NAME_COL)).flex_none().text_xs().font_weight(FontWeight::BOLD).child("Consumable"))
        .child(div().w(px(COUNT_COL)).flex_none().text_xs().font_weight(FontWeight::BOLD).child("Remaining"))
        .child(div().w(px(COUNT_COL)).flex_none().text_xs().font_weight(FontWeight::BOLD).child("Total"));

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
        ChargeCount::Unlimited => {
            (consumable.charges_used.to_string(), "Unlimited".to_string(), format!("Used: {}", consumable.charges_used))
        }
        ChargeCount::Finite(total) => {
            let remaining = total.saturating_sub(consumable.charges_used);
            (remaining.to_string(), total.to_string(), format!("Remaining: {remaining} / Total: {total}"))
        }
    };
    let hover: SharedString = hover_text.into();

    let key = format!("consumable:{}", consumable.icon_key);
    let mut name_cell = h_flex().w(px(NAME_COL)).flex_none().gap_1().items_center();
    if let Some(image) = icons.get_keyed(&key) {
        name_cell = name_cell.child(img(image).w(px(CONSUMABLE_ICON_SIZE)).h(px(CONSUMABLE_ICON_SIZE)));
    }
    name_cell = name_cell.child(div().text_xs().child(consumable.display_name.clone()));

    h_flex()
        .id(("replay-consumable", row_ix * DETAIL_ID_STRIDE + idx))
        .gap_2()
        .items_center()
        .child(name_cell)
        .child(div().w(px(COUNT_COL)).flex_none().text_xs().child(remaining_text))
        .child(div().w(px(COUNT_COL)).flex_none().text_xs().child(total_text))
        .tooltip(move |window, cx| Tooltip::new(hover.clone()).build(window, cx))
        .into_any_element()
}

/// ActualDamage/ReceivedDamage expanded content: the per-ammo-type breakdown
/// paragraph (`row.actual_damage_hover_text`/`received_damage_hover_text`),
/// then per-victim (or per-attacker) interaction lines,
/// `"{ship}: {amount} ({pct}%)"`, sorted by amount descending, skipping zero
/// entries. NDA-gated like the collapsed cell (`should_hide_stats() &&
/// !debug`), mirroring the egui app's `dealt_damage_details`/
/// `received_damage_details`. No heading: each side sits under its own column
/// (ActualDamage/ReceivedDamage), so the column header already names it, just
/// as in egui.
fn render_damage_section(
    ix: usize,
    row: &PlayerRow,
    all_rows: &[PlayerRow],
    debug: bool,
    direction: DamageDirection,
    cx: &App,
) -> Option<AnyElement> {
    if row.should_hide_stats() && !debug {
        return Some(nda_text());
    }

    let hover_text = match direction {
        DamageDirection::Dealt => row.actual_damage_hover_text.as_ref(),
        DamageDirection::Received => row.received_damage_hover_text.as_ref(),
    };
    let interactions = row.damage_interactions.as_ref();
    if hover_text.is_none() && interactions.is_none() {
        return None;
    }

    let mut col = v_flex().gap_1();

    if let Some(text) = hover_text {
        col = col.child(hover_paragraph(text, cx));
        if interactions.is_some() {
            col = col.child(Separator::horizontal());
        }
    }

    if let Some(interactions) = interactions {
        let mut entries: Vec<_> = interactions.iter().collect();
        entries.sort_by_key(|(_, interaction)| std::cmp::Reverse(interaction_amount(interaction, direction)));
        for (idx, (account_id, interaction)) in entries.into_iter().enumerate() {
            let amount = interaction_amount(interaction, direction);
            if amount == 0 {
                continue;
            }
            let Some(other) = all_rows.iter().find(|r| r.db_id == *account_id) else {
                continue;
            };
            let pct = interaction_percentage(interaction, direction);
            col = col.child(div().id(("replay-interaction", ix * DETAIL_ID_STRIDE + idx)).text_xs().child(format!(
                "{}: {} ({pct:.0}%)",
                other.ship_name,
                separate_number(amount)
            )));
        }
    }

    Some(col.into_any_element())
}

/// The ammo-type damage breakdown as a monospace paragraph, one line per
/// `\n`-separated entry, matching the mono styling `table.rs`'s
/// `hover_tooltip` uses for the same text when it shows as the collapsed
/// cell's hover tooltip.
fn hover_paragraph(text: &str, cx: &App) -> AnyElement {
    let mono_font_family = cx.theme().mono_font_family.clone();
    v_flex()
        .gap_0()
        .text_xs()
        .font_family(mono_font_family)
        .children(text.split('\n').map(|line| div().child(line.to_string())))
        .into_any_element()
}

fn interaction_amount(interaction: &DamageInteraction, direction: DamageDirection) -> u64 {
    match direction {
        DamageDirection::Dealt => interaction.damage_dealt,
        DamageDirection::Received => interaction.damage_received,
    }
}

fn interaction_percentage(interaction: &DamageInteraction, direction: DamageDirection) -> f64 {
    match direction {
        DamageDirection::Dealt => interaction.damage_dealt_percentage,
        DamageDirection::Received => interaction.damage_received_percentage,
    }
}

// `use super::*` here would also pull in `gpui::*` (glob-reexported at the
// top of this file), and combined with this file's heavy element-builder
// chains that blows the compiler's macro/name-resolution recursion limit
// while expanding `#[test]` (reproduced: a single trivial `#[test]` fails to
// compile under `use super::*`, but compiles fine with explicit imports).
// Import only the specific pure-logic items these tests exercise instead.
#[cfg(test)]
mod tests {
    use wows_replay_insights::battle_report::DamageInteraction;
    use wows_replay_insights::battle_report::RibbonResult;

    use super::DamageDirection;
    use super::interaction_amount;
    use super::interaction_percentage;
    use super::reorder_bulge_after_main_caliber;

    fn ribbon(name: &str) -> RibbonResult {
        RibbonResult {
            name: name.to_string(),
            display_name: name.to_string(),
            description: String::new(),
            icon_key: name.to_string(),
            is_subribbon: false,
            count: 1,
        }
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
        assert!((interaction_percentage(&i, DamageDirection::Dealt) - 56.4).abs() < 1e-9);
        assert!((interaction_percentage(&i, DamageDirection::Received) - 38.1).abs() < 1e-9);
    }
}
