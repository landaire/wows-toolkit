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
//! doc); a missing one falls back to a text label, as the egui app does. The
//! labels, ordering and widths are `wows_toolkit_replay_data::row_detail`.

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
use wows_replay_insights::battle_report::RibbonResult;
use wows_replay_insights::battle_report::TranslatedModule;
use wows_replay_insights::fire_chance::analysis::EffectiveFireChance;
use wowsunpack::game_params::skill_grid_data::SkillGridRow;
use wowsunpack::game_params::skill_grid_data::SkillGridSkill;

use super::icons::IconCache;
use wows_toolkit_replay_data::cells::NDA;
use wows_toolkit_replay_data::columns::ReplayColumn;
use wows_toolkit_replay_data::model::PlayerRow;
use wows_toolkit_replay_data::model::separate_number;
use wows_toolkit_replay_data::row_detail::BREAKDOWN_GAP;
use wows_toolkit_replay_data::row_detail::CONSUMABLE_COUNT_WIDTH;
use wows_toolkit_replay_data::row_detail::CONSUMABLE_ICON_SIZE;
use wows_toolkit_replay_data::row_detail::CONSUMABLE_NAME_WIDTH;
use wows_toolkit_replay_data::row_detail::DamageDirection;
use wows_toolkit_replay_data::row_detail::FIRE_CHANCE_SHIP_WIDTH;
use wows_toolkit_replay_data::row_detail::ICON_LABEL_GAP;
use wows_toolkit_replay_data::row_detail::IconSize;
use wows_toolkit_replay_data::row_detail::LARGE_ICON_SIZE;
use wows_toolkit_replay_data::row_detail::MODULE_ICON_SIZE;
use wows_toolkit_replay_data::row_detail::SKILL_COST_WIDTH;
use wows_toolkit_replay_data::row_detail::TALLY_COUNT_WIDTH;
use wows_toolkit_replay_data::row_detail::TALLY_INDENT;
use wows_toolkit_replay_data::row_detail::UNLEARNED_SKILL_OPACITY;
use wows_toolkit_replay_data::row_detail::achievement_label;
use wows_toolkit_replay_data::row_detail::consumable_counts;
use wows_toolkit_replay_data::row_detail::damage_breakdown;
use wows_toolkit_replay_data::row_detail::damage_event_rows;
use wows_toolkit_replay_data::row_detail::damage_target_rows;
use wows_toolkit_replay_data::row_detail::fit_dims;
use wows_toolkit_replay_data::row_detail::has_damage_events;
use wows_toolkit_replay_data::row_detail::has_name_section;
use wows_toolkit_replay_data::row_detail::module_text;
use wows_toolkit_replay_data::row_detail::nda_hidden;
use wows_toolkit_replay_data::row_detail::raw_ship_name;
use wows_toolkit_replay_data::row_detail::raw_source;
use wows_toolkit_replay_data::row_detail::ribbon_icon_box;
use wows_toolkit_replay_data::row_detail::ribbon_label;
use wows_toolkit_replay_data::row_detail::skill_fallback_label;
use wows_toolkit_replay_data::row_detail::skill_tier_label;
use wows_toolkit_replay_data::row_detail::skill_tooltip;
use wows_toolkit_replay_data::row_detail::sorted_ribbons;
use wows_toolkit_replay_data::row_icons::RowIcon;
use wows_toolkit_viewmodel::breakdown::BreakdownRow;
use wows_toolkit_viewmodel::fire_chance;

/// Multiplier for a detail-list item's `ElementId`
/// (`row_ix * DETAIL_ID_STRIDE + item_ix`), spacing rows apart enough that no
/// two `(row, item)` pairs collide across any of the lists this module
/// renders (achievements, ribbons, modernizations, signals, loadout, captain
/// skills, consumables, damage interactions). Each list also gets its own
/// `ElementId` string tag, so only same-tag, same-row collisions matter; a
/// single row is not expected to carry anywhere near this many entries in one
/// list.
const DETAIL_ID_STRIDE: usize = 256;

/// One column's expanded detail for `row` at list index `ix`, or `None` when
/// that column has nothing to show (so `table.rs` leaves the column's cell at
/// its collapsed height). Mirrors the egui app's per-column
/// `if 0.0 < expandedness { match column { ... } }` arms (`mod.rs:1834-2088`):
/// each column draws its own detail under its own cell -- Name's achievements,
/// ribbons, damage events and effective fire chance under Name, Skills' build
/// under Skills, the damage breakdowns under ActualDamage/ReceivedDamage, and
/// the Potential/Spotting/Hits breakdowns under those columns. `all_rows`
/// resolves a damage interaction's victim/attacker `db_id` into a ship name,
/// and `alt_held` turns the breakdown's percentages around.
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
        ReplayColumn::SpottingDamage => row.spotting_damage_breakdown.as_deref().map(breakdown_grid),
        _ => None,
    }
}

/// Potential-damage / Hits expanded content: the column's own breakdown shown
/// inline, NDA-gated exactly like the egui arms (`mod.rs:1983-1989, 2077-2082`)
/// -- an NDA placeholder when the row's stats are hidden and debug is off,
/// otherwise the grid (or nothing when there is no breakdown).
fn render_nda_gated_breakdown(rows: Option<&[BreakdownRow]>, row: &PlayerRow, debug: bool) -> Option<AnyElement> {
    if nda_hidden(row, debug) {
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
        None => div().text_xs().child(detail_text(display_name)),
    };
    el.id((tag, row_ix * DETAIL_ID_STRIDE + idx))
        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
        .into_any_element()
}

/// The size to render `image` at within a `max` by `max` box
/// while preserving its native aspect ratio. Matches the egui app's ribbon
/// rendering, which uses `Image::fit_to_exact_size((64, 64))` -- a scale that
/// keeps aspect ratio -- so a wide, short ribbon icon renders short (its row
/// only as tall as the fitted icon) rather than being forced into a `max` by
/// `max` square and inflating the row height. Square icons (achievements) are
/// unaffected: they fit as `max` by `max`.
fn fit_within_box(image: &RenderImage, max: f32) -> IconSize {
    let size = image.size(0);
    fit_dims(IconSize { width: size.width.0 as f32, height: size.height.0 as f32 }, max)
}

/// An `[icon | label]` row shared by achievements and ribbons: an icon when
/// `icons.get_keyed(key)` has one (no fallback text in its place, matching the
/// egui app's `ui.horizontal` which only conditionally adds the image), then
/// `label` always rendered alongside it. A hover tooltip covers the whole row.
/// The icon is fitted within a `size` by `size` box preserving its aspect
/// ratio (`fit_within_box`), so wide ribbon icons stay short and their row
/// with them. `items_center()` vertically centers the icon and label on a
/// shared center line when the icon is taller than the text (e.g. a square
/// achievement icon). The 8px icon-to-label gap matches egui's default
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
    let mut row = h_flex().w_full().gap(px(ICON_LABEL_GAP)).items_center();
    if let Some(image) = icons.get_keyed(key) {
        let fitted = fit_within_box(&image, size);
        row = row.child(img(image).w(px(fitted.width)).h(px(fitted.height)).flex_none());
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
    if nda_hidden(row, debug) {
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
            .child(crate::icons::icon(crate::icons::FIRE))
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
                        div()
                            .flex_none()
                            .w(px(FIRE_CHANCE_SHIP_WIDTH))
                            .child(detail_text(ship.victim_ship_name.clone())),
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
                    div()
                        .flex_none()
                        .w(px(TALLY_COUNT_WIDTH))
                        .text_right()
                        .child(detail_text(separate_number(row.count))),
                )
                .child(
                    div()
                        .pl(px(row.depth as f32 * TALLY_INDENT))
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
        .child(crate::icons::icon(crate::icons::COPY))
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

/// Keeps one row's per-ship element ids clear of the next row's.
const FIRE_CHANCE_SHIP_STRIDE: usize = 64;

/// Name-column expanded content: achievements ("name (Nx)" once a count
/// exceeds 1), ribbons (sorted by name, with the RIBBON_BULGE-after-
/// RIBBON_MAIN_CALIBER one-off reorder), the fires/floods/citadels/crits
/// damage-event counts, NDA-gated, and then the effective fire chance.
/// Mirrors `mod.rs`'s `ReplayColumn::Name` expanded arm.
fn render_name_section(row_ix: usize, row: &PlayerRow, debug: bool, icons: &IconCache) -> Option<AnyElement> {
    if !has_name_section(row) {
        return None;
    }

    let has_achievements = !row.achievements.is_empty();
    let has_ribbons = !row.ribbons.is_empty();
    let has_damage_events = has_damage_events(row);

    // 3px matches egui's default `Spacing::item_spacing.y`: the section
    // heading, each achievement/ribbon row, the separators, and the damage
    // event lines are all top-level children of one `ui.vertical` in the
    // egui original, so they all share that one gap -- not a larger gap at
    // this level and a tighter one nested inside `achievements_view`/
    // `ribbons_view`.
    let mut col = v_flex().gap(px(3.));

    if has_achievements {
        col = col.child(section_heading(t!("ui.replay.sections.achievements").into_owned()));
        col = col.child(achievements_view(row_ix, &row.achievements, icons));
    }

    if has_ribbons {
        if has_achievements {
            col = col.child(Separator::horizontal());
        }
        col = col.child(section_heading(t!("ui.replay.sections.ribbons").into_owned()));
        col = col.child(ribbons_view(row_ix, sorted_ribbons(row), icons));
    }

    if has_damage_events {
        if has_achievements || has_ribbons {
            col = col.child(Separator::horizontal());
        }
        col = col.child(section_heading(t!("ui.replay.sections.damage_events").into_owned()));
        if nda_hidden(row, debug) {
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
    let key = RowIcon::achievement(&achievement.icon_key).cache_key();
    icon_label_row(
        "replay-achievement",
        row_ix,
        idx,
        &key,
        achievement_label(achievement),
        achievement.description.clone(),
        LARGE_ICON_SIZE,
        icons,
    )
}

fn ribbon_row(row_ix: usize, idx: usize, ribbon: &RibbonResult, icons: &IconCache) -> AnyElement {
    let key = RowIcon::ribbon(ribbon.is_subribbon, &ribbon.icon_key).cache_key();
    icon_label_row(
        "replay-ribbon",
        row_ix,
        idx,
        &key,
        ribbon_label(ribbon),
        ribbon.description.clone(),
        ribbon_icon_box(ribbon),
        icons,
    )
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
                let text = module_text(module);
                icon_or_text_cell(
                    "replay-modernization",
                    row_ix,
                    idx,
                    &RowIcon::Modernization(module.game_params_name.clone()).cache_key(),
                    text.name,
                    text.tooltip,
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
        .child(div().text_xs().text_color(crate::theme::text_faint()).child("-"))
        .into_any_element()
}

fn signals_view(row_ix: usize, signals: &[TranslatedModule], icons: &IconCache) -> AnyElement {
    let mut row = h_flex().flex_wrap().gap_1();
    for (idx, signal) in signals.iter().enumerate() {
        let text = module_text(signal);
        row = row.child(icon_or_text_cell(
            "replay-signal",
            row_ix,
            idx,
            &RowIcon::Signal(signal.game_params_name.clone()).cache_key(),
            text.name,
            text.tooltip,
            MODULE_ICON_SIZE,
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
/// point-cost label, then a cell per skill in that tier -- a decoded icon
/// (dimmed when not learned) or, absent one, `"(cost) name"` text (dimmed the
/// same way). Mirrors `render_skill_grid`.
fn captain_skill_grid_view(row_ix: usize, rows: &[SkillGridRow], icons: &IconCache) -> AnyElement {
    let mut col = v_flex().gap_1();
    let mut skill_idx = 0usize;
    for row in rows {
        let mut line = h_flex().gap_1().items_center();
        line = line.child(
            div()
                .w(px(SKILL_COST_WIDTH))
                .flex_none()
                .text_xs()
                .text_color(crate::theme::text_dim())
                .child(detail_text(skill_tier_label(row))),
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
    let tooltip_text: SharedString = skill_tooltip(skill).into();

    let key = RowIcon::Skill(skill.internal_name.clone()).cache_key();
    let el = match icons.get_keyed(&key) {
        Some(image) => {
            let opacity = if skill.learned { 1.0 } else { UNLEARNED_SKILL_OPACITY };
            div().opacity(opacity).child(img(image).w(px(MODULE_ICON_SIZE)).h(px(MODULE_ICON_SIZE)))
        }
        None => div()
            .text_xs()
            .when(!skill.learned, |this| this.text_color(crate::theme::text_faint()))
            .child(detail_text(skill_fallback_label(skill))),
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
    let header = h_flex()
        .gap_2()
        .child(
            div()
                .w(px(CONSUMABLE_NAME_WIDTH))
                .flex_none()
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .child(detail_text(t!("ui.replay.consumable_header_consumable").into_owned())),
        )
        .child(
            div()
                .w(px(CONSUMABLE_COUNT_WIDTH))
                .flex_none()
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .child(detail_text(t!("ui.replay.consumable_header_remaining").into_owned())),
        )
        .child(
            div()
                .w(px(CONSUMABLE_COUNT_WIDTH))
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
    let counts = consumable_counts(consumable);
    let hover: SharedString = counts.hover.into();

    let key = RowIcon::Consumable(consumable.icon_key.clone()).cache_key();
    let mut name_cell = h_flex().w(px(CONSUMABLE_NAME_WIDTH)).flex_none().gap_1().items_center();
    if let Some(image) = icons.get_keyed(&key) {
        name_cell = name_cell.child(img(image).w(px(CONSUMABLE_ICON_SIZE)).h(px(CONSUMABLE_ICON_SIZE)));
    }
    name_cell = name_cell.child(div().text_xs().child(detail_text(consumable.display_name.clone())));

    h_flex()
        .id(("replay-consumable", row_ix * DETAIL_ID_STRIDE + idx))
        .gap_2()
        .items_center()
        .child(name_cell)
        .child(div().w(px(CONSUMABLE_COUNT_WIDTH)).flex_none().text_xs().child(detail_text(counts.remaining)))
        .child(div().w(px(CONSUMABLE_COUNT_WIDTH)).flex_none().text_xs().child(detail_text(counts.total)))
        .tooltip(move |window, cx| Tooltip::new(hover.clone()).build(window, cx))
        .into_any_element()
}

/// ActualDamage/ReceivedDamage expanded content: the per-ammo-type breakdown
/// (`row.actual_damage_breakdown`/`received_damage_breakdown`), then a
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
    if nda_hidden(row, debug) {
        return Some(nda_text());
    }

    let breakdown = damage_breakdown(row, direction);
    let interactions = row.damage_interactions.as_ref();
    if breakdown.is_none() && interactions.is_none() {
        return None;
    }

    let mut col = v_flex().gap_1();

    if let Some(rows) = breakdown {
        col = col.child(breakdown_grid(rows));
        if interactions.is_some() {
            col = col.child(Separator::horizontal());
        }
    }

    if interactions.is_some() {
        col = col.child(breakdown_grid(&damage_target_rows(row, all_rows, direction, alt_held)));
    }

    Some(col.into_any_element())
}

// `use super::*` here would also pull in `gpui_kit::*` (glob-reexported at the
// top of this file), and combined with this file's heavy element-builder
// chains that blows the compiler's macro/name-resolution recursion limit
// while expanding `#[test]` (reproduced: a single trivial `#[test]` fails to
// compile under `use super::*`, but compiles fine with explicit imports).
// Import only the specific pure-logic items these tests exercise instead.
#[cfg(test)]
mod tests {
    use super::IconCache;
    use super::ReplayColumn;
    use super::render_column_detail;
    use wows_replays::types::Relation;
    use wows_toolkit_replay_data::test_support::base_row;
    use wows_toolkit_replay_data::test_support::fire_chance_result;

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
}
