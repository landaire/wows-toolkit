//! What an expanded row shows under each column: achievements, ribbons,
//! damage events and the fire chance under Name; the build under Skills;
//! the damage breakdowns and per-ship shares under the damage columns; the
//! potential-damage, hits and spotting breakdowns under theirs. The text is
//! selectable, for the reader taking a figure elsewhere.

use nsh_core::prelude::*;
use rust_i18n::t;
use wows_replay_insights::battle_report::ConsumableResult;
use wows_replay_insights::battle_report::TranslatedModule;
use wows_replay_insights::fire_chance::analysis::EffectiveFireChance;
use wows_replays::types::AccountId;
use wows_toolkit_replay_data::cells::NDA;
use wows_toolkit_replay_data::columns::ReplayColumn;
use wows_toolkit_replay_data::model::PlayerRow;
use wows_toolkit_replay_data::model::separate_number;
use wows_toolkit_replay_data::row_detail::BREAKDOWN_GAP;
use wows_toolkit_replay_data::row_detail::CONSUMABLE_ICON_SIZE;
use wows_toolkit_replay_data::row_detail::CONSUMABLE_NAME_WIDTH;
use wows_toolkit_replay_data::row_detail::DamageDirection;
use wows_toolkit_replay_data::row_detail::FIRE_CHANCE_SHIP_WIDTH;
use wows_toolkit_replay_data::row_detail::ICON_LABEL_GAP;
use wows_toolkit_replay_data::row_detail::LARGE_ICON_SIZE;
use wows_toolkit_replay_data::row_detail::MODULE_ICON_SIZE;
use wows_toolkit_replay_data::row_detail::SKILL_COST_WIDTH;
use wows_toolkit_replay_data::row_detail::TALLY_COUNT_WIDTH;
use wows_toolkit_replay_data::row_detail::TALLY_INDENT;
use wows_toolkit_replay_data::row_detail::UNLEARNED_SKILL_OPACITY;
use wows_toolkit_replay_data::row_detail::achievement_label;
use wows_toolkit_replay_data::row_detail::consumable_count_width;
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
use wowsunpack::game_params::skill_grid_data::SkillGridRow;
use wowsunpack::game_params::skill_grid_data::SkillGridSkill;

use crate::icons::Icon;
use crate::icons::icon;
use crate::replay_inspector::table::EngineMeasure;
use crate::replay_inspector::table::Table;
use crate::replay_inspector::table::icons::IconImage;
use crate::replay_inspector::table::icons::TableIcons;
use crate::replay_inspector::table::parts::class;
use crate::replay_inspector::table::parts::detail_text;
use crate::replay_inspector::table::parts::dim_text;
use crate::replay_inspector::table::parts::glyph_button;
use crate::replay_inspector::table::parts::section_heading;
use crate::replay_inspector::table::parts::separator;
use crate::replay_inspector::table::parts::tip;

/// `col`'s detail for `row`, or `None` when the column has none to show.
pub fn column_detail(
    table: Table,
    col: ReplayColumn,
    row: &PlayerRow,
    debug: bool,
    icons: &TableIcons,
) -> Option<AnyElement> {
    let detail = match col {
        ReplayColumn::Name => name_section(table, row, debug, icons),
        ReplayColumn::Skills => build_section(row, debug, icons),
        ReplayColumn::ActualDamage => damage_section(table, row, debug, DamageDirection::Dealt),
        ReplayColumn::ReceivedDamage => damage_section(table, row, debug, DamageDirection::Received),
        ReplayColumn::PotentialDamage => nda_gated_breakdown(row.potential_damage_breakdown.as_deref(), row, debug),
        ReplayColumn::Hits => nda_gated_breakdown(row.hits_breakdown.as_deref(), row, debug),
        ReplayColumn::SpottingDamage => row.spotting_damage_breakdown.as_deref().map(breakdown_grid),
        _ => None,
    }?;

    Some(view().selectable(true).app_class(class("flex-col")).child(detail).into_element())
}

fn nda_text() -> AnyElement {
    detail_text(NDA).into_element()
}

/// Labels in one column, values right-aligned in the next.
fn breakdown_grid(rows: &[BreakdownRow]) -> AnyElement {
    view()
        .app_class(class("flex-row items-start"))
        .gap(px(BREAKDOWN_GAP))
        .child(
            view()
                .app_class(class("flex-col gap-px"))
                .children(rows.iter().map(|row| detail_text(row.label.clone()).into_element()).collect::<Vec<_>>()),
        )
        .child(
            view()
                .app_class(class("flex-col gap-px items-end"))
                .children(rows.iter().map(|row| detail_text(row.value.clone()).into_element()).collect::<Vec<_>>()),
        )
        .into_element()
}

fn nda_gated_breakdown(rows: Option<&[BreakdownRow]>, row: &PlayerRow, debug: bool) -> Option<AnyElement> {
    if nda_hidden(row, debug) {
        return Some(nda_text());
    }

    rows.map(breakdown_grid)
}

/// An icon fitted in a `size` square keeping its aspect, then its label, the
/// tooltip over both. A missing icon leaves the label alone.
fn icon_label_row(icon: Option<IconImage>, label: String, tooltip: &str, size: f32) -> AnyElement {
    let mut line = view().app_class(class("flex-row items-center w-full")).gap(px(ICON_LABEL_GAP));
    if let Some(icon) = icon {
        let fitted = fit_dims(icon.size, size);
        line = line.child(image(icon.uri).app_class(class("flex-none")).w(px(fitted.width)).h(px(fitted.height)));
    }
    line = line.child(detail_text(label).app_class(class("flex-1 min-w-0 text-xs text-foreground")));

    if tooltip.is_empty() { line.into_element() } else { line.tooltip(tip(tooltip.to_owned())).into_element() }
}

/// Achievements, ribbons, damage events and the fire chance, each under
/// its heading with a rule between them.
fn name_section(table: Table, row: &PlayerRow, debug: bool, icons: &TableIcons) -> Option<AnyElement> {
    if !has_name_section(row) {
        return None;
    }

    let mut parts: Vec<AnyElement> = Vec::new();
    let section = |parts: &mut Vec<AnyElement>, mut content: Vec<AnyElement>| {
        if !parts.is_empty() {
            parts.push(separator());
        }
        parts.append(&mut content);
    };

    if !row.achievements.is_empty() {
        let mut content = vec![section_heading(t!("ui.replay.sections.achievements").into_owned())];
        for achievement in &row.achievements {
            let icon = icons.icon(&RowIcon::achievement(&achievement.icon_key));
            content.push(icon_label_row(icon, achievement_label(achievement), &achievement.description, LARGE_ICON_SIZE));
        }
        section(&mut parts, content);
    }

    if !row.ribbons.is_empty() {
        let mut content = vec![section_heading(t!("ui.replay.sections.ribbons").into_owned())];
        for ribbon in sorted_ribbons(row) {
            let icon = icons.icon(&RowIcon::ribbon(ribbon.is_subribbon, &ribbon.icon_key));
            content.push(icon_label_row(icon, ribbon_label(ribbon), &ribbon.description, ribbon_icon_box(ribbon)));
        }
        section(&mut parts, content);
    }

    if has_damage_events(row) {
        let grid = if nda_hidden(row, debug) { nda_text() } else { breakdown_grid(&damage_event_rows(row)) };
        section(&mut parts, vec![section_heading(t!("ui.replay.sections.damage_events").into_owned()), grid]);
    }

    if let Some(fire_chance) = row.fire_chance.as_ref() {
        section(&mut parts, fire_chance_section(table, fire_chance, row, debug));
    }

    Some(view().app_class(class("flex-col")).gap(px(3.0)).children(parts).into_element())
}

/// The recording player's effective fire chance: observed beside expected
/// with a copy button, the formula, what became of the shells and of the
/// fire ribbons, then each target ship.
fn fire_chance_section(
    table: Table,
    fire_chance: &EffectiveFireChance,
    row: &PlayerRow,
    debug: bool,
) -> Vec<AnyElement> {
    let mut parts = vec![section_heading(t!("ui.replay.sections.fire_chance").into_owned())];
    if nda_hidden(row, debug) {
        parts.push(detail_text(t!("ui.replay.nda").into_owned()).into_element());
        return parts;
    }
    if fire_chance.eligible_hits == 0 {
        // A total with nothing behind it would read as a real zero.
        parts.push(dim_text(t!("ui.replay.sections.fire_chance_no_eligible_hits").into_owned()).into_element());
        return parts;
    }

    let copy = fire_chance::copy_text(fire_chance, &raw_source, &raw_ship_name);
    let copied = t!("ui.replay.sections.fire_chance_copied").into_owned();
    let copy_hint = t!("ui.replay.sections.fire_chance_click_to_copy").into_owned();
    let mut headline = view().app_class(class("flex-row items-center gap-2")).child(
        view()
            .app_class(class("flex-row items-center gap-1"))
            .child(icon(Icon::Flame, px(12.0)).app_class(class("flex-none text-foreground")))
            .child(
                text(fire_chance::counts_text(fire_chance.fires, fire_chance.eligible_hits))
                    .app_class(class("text-xs font-bold text-foreground")),
            ),
    );
    if let Some(expected) = fire_chance::expected_fires_text(fire_chance) {
        headline = headline.child(dim_text(expected));
    }
    headline = headline.child(glyph_button(Icon::Copy, copy_hint.clone()).tooltip(tip(copy_hint)).on_press(
        move |_: &PressEvent| {
            set_clipboard(copy.clone());
            table.notice(copied.clone());
        },
    ));
    parts.push(headline.into_element());
    parts.push(dim_text(fire_chance::ships_text(fire_chance).into_owned()).into_element());

    let formula = fire_chance::fire_chance_formula_lines(fire_chance, &raw_source);
    if !formula.is_empty() {
        parts.push(separator());
        parts.push(
            view()
                .app_class(class("flex-col"))
                .children(
                    formula
                        .into_iter()
                        .map(|line| text(line).app_class(class("font-mono text-xs text-foreground")).into_element())
                        .collect::<Vec<_>>(),
                )
                .into_element(),
        );
    }

    parts.push(separator());
    parts.push(tally_block(&fire_chance::fire_chance_battle_tally_rows(fire_chance)));
    let ribbons = fire_chance::fire_chance_ribbon_rows(fire_chance);
    if !ribbons.is_empty() {
        parts.push(tally_block(&ribbons));
    }

    // A rate only means something inside one victim, whose fire resistance
    // is fixed: one line per target ship, most sampled first.
    let ships = fire_chance::sorted_per_ship(fire_chance);
    if !ships.is_empty() {
        parts.push(separator());
        parts.push(section_heading(t!("ui.replay.sections.fire_chance_per_ship").into_owned()));
        for ship in ships {
            parts.push(
                view()
                    .app_class(class("flex-row items-center gap-2"))
                    .child(
                        detail_text(ship.victim_ship_name.clone())
                            .app_class(class("flex-none text-xs text-foreground truncate"))
                            .w(px(FIRE_CHANCE_SHIP_WIDTH * font_scale())),
                    )
                    .child(dim_text(fire_chance::counts_text(ship.fires, ship.eligible_hits)))
                    .into_element(),
            );
        }
        if let Some(line) = fire_chance::no_target_ship_line(fire_chance) {
            parts.push(text(line).app_class(class("text-xs text-text-faint")).into_element());
        }
    }

    parts
}

/// Counts in a column of their own, labels indented by their depth.
fn tally_block(rows: &[fire_chance::TallyRow]) -> AnyElement {
    view()
        .app_class(class("flex-col gap-px"))
        .children(
            rows.iter()
                .map(|row| {
                    let label = if row.depth > 0 {
                        dim_text(row.label.to_string())
                    } else {
                        detail_text(row.label.to_string())
                    };
                    view()
                        .app_class(class("flex-row items-start gap-2"))
                        .child(
                            detail_text(separate_number(row.count))
                                .app_class(class("flex-none text-xs text-foreground text-right truncate"))
                                .w(px(TALLY_COUNT_WIDTH * font_scale())),
                        )
                        .child(label.pl(px(row.depth as f32 * TALLY_INDENT)))
                        .into_element()
                })
                .collect::<Vec<_>>(),
        )
        .into_element()
}

/// The build: its summary, then modernizations, signals, loadout and the
/// captain's skills, then the consumables. Enemies' builds show only in
/// debug mode.
fn build_section(row: &PlayerRow, debug: bool, icons: &TableIcons) -> Option<AnyElement> {
    if row.relation.is_enemy() && !debug {
        return None;
    }

    let mut parts: Vec<AnyElement> = Vec::new();

    if let Some(hover) = row.skill_hover_text.as_ref() {
        parts.push(detail_text(hover.clone()).into_element());
    }

    if let Some(build) = row.translated_build.as_ref() {
        if !parts.is_empty() {
            parts.push(separator());
        }
        if build.modernization_slots.is_empty() {
            parts.push(detail_text(t!("ui.replay.sections.modules_none").into_owned()).into_element());
        } else {
            parts.push(section_heading(t!("ui.replay.sections.modules").into_owned()));
            parts.push(wrapping(
                build
                    .modernization_slots
                    .iter()
                    .map(|slot| match slot {
                        Some(module) => {
                            module_cell(icons, RowIcon::Modernization(module.game_params_name.clone()), module)
                        }
                        None => empty_slot(),
                    })
                    .collect(),
            ));
        }

        if !build.signals.is_empty() {
            parts.push(separator());
            parts.push(section_heading(t!("ui.replay.sections.signals").into_owned()));
            parts.push(wrapping(
                build
                    .signals
                    .iter()
                    .map(|signal| module_cell(icons, RowIcon::Signal(signal.game_params_name.clone()), signal))
                    .collect(),
            ));
        }

        parts.push(separator());
        if build.loadout.is_empty() {
            parts.push(detail_text(t!("ui.replay.sections.loadout_none").into_owned()).into_element());
        } else {
            parts.push(section_heading(t!("ui.replay.sections.loadout").into_owned()));
            for module in &build.loadout {
                let Some(name) = module.name.clone() else {
                    continue;
                };
                parts.push(text_line(name, module.description.as_deref()));
            }
        }

        parts.push(separator());
        match build.captain_skills.as_deref() {
            Some(skills) => {
                parts.push(section_heading(t!("ui.replay.sections.captain_skills").into_owned()));
                if skills.is_empty() {
                    parts.push(detail_text(t!("ui.replay.sections.captain_skills_none").into_owned()).into_element());
                } else {
                    parts.push(skill_grid(skills, icons));
                }
            }
            None => parts.push(detail_text(t!("ui.replay.sections.captain_skills_none").into_owned()).into_element()),
        }
    }

    if !row.consumables.is_empty() {
        if !parts.is_empty() {
            parts.push(separator());
        }
        parts.push(section_heading(t!("ui.replay.sections.consumables").into_owned()));
        parts.push(consumables_table(&row.consumables, icons));
    } else if let Some(build) = row.translated_build.as_ref()
        && !build.abilities.is_empty()
    {
        if !parts.is_empty() {
            parts.push(separator());
        }
        parts.push(section_heading(t!("ui.replay.sections.consumables").into_owned()));
        for name in build.abilities.iter().filter_map(|ability| ability.name.clone()) {
            parts.push(text_line(name, None));
        }
    }

    if parts.is_empty() {
        return None;
    }

    Some(view().app_class(class("flex-col gap-1")).children(parts).into_element())
}

fn wrapping(cells: Vec<AnyElement>) -> AnyElement {
    view().app_class(class("flex-row flex-wrap gap-1")).children(cells).into_element()
}

/// A modernization or signal: its icon, else its name; its description on
/// hover.
fn module_cell(icons: &TableIcons, icon: RowIcon, module: &TranslatedModule) -> AnyElement {
    let text = module_text(module);
    let content = match icons.icon(&icon) {
        Some(image_icon) => image(image_icon.uri)
            .label(text.name)
            .w(px(MODULE_ICON_SIZE))
            .h(px(MODULE_ICON_SIZE))
            .into_element(),
        None => detail_text(text.name).into_element(),
    };

    view().child(content).tooltip(tip(text.tooltip)).into_element()
}

/// An unfilled modernization slot.
fn empty_slot() -> AnyElement {
    view()
        .app_class(class("items-center justify-center"))
        .w(px(MODULE_ICON_SIZE))
        .h(px(MODULE_ICON_SIZE))
        .child(text("-").app_class(class("text-xs text-text-faint")))
        .into_element()
}

/// A line of text, with its description on hover when it has one.
fn text_line(content: String, tooltip: Option<&str>) -> AnyElement {
    let line = view().child(detail_text(content));
    match tooltip.filter(|tooltip| !tooltip.is_empty()) {
        Some(tooltip) => line.tooltip(tip(tooltip.to_owned())).into_element(),
        None => line.into_element(),
    }
}

/// One line per skill-point tier: the cost, then each skill, dimmed when
/// the captain did not learn it.
fn skill_grid(tiers: &[SkillGridRow], icons: &TableIcons) -> AnyElement {
    view()
        .app_class(class("flex-col gap-1"))
        .children(
            tiers
                .iter()
                .map(|tier| {
                    view()
                        .app_class(class("flex-row items-center gap-1"))
                        .child(
                            dim_text(skill_tier_label(tier)).app_class(class("flex-none text-xs text-muted-foreground")).w(px(SKILL_COST_WIDTH)),
                        )
                        .children(tier.skills.iter().map(|skill| skill_cell(skill, icons)).collect::<Vec<_>>())
                        .into_element()
                })
                .collect::<Vec<_>>(),
        )
        .into_element()
}

fn skill_cell(skill: &SkillGridSkill, icons: &TableIcons) -> AnyElement {
    let content = match icons.icon(&RowIcon::Skill(skill.internal_name.clone())) {
        Some(icon) => image(icon.uri)
            .opacity(if skill.learned { 1.0 } else { UNLEARNED_SKILL_OPACITY })
            .w(px(MODULE_ICON_SIZE))
            .h(px(MODULE_ICON_SIZE))
            .into_element(),
        None if skill.learned => detail_text(skill_fallback_label(skill)).into_element(),
        None => text(skill_fallback_label(skill)).app_class(class("text-xs text-text-faint")).into_element(),
    };

    view().child(content).tooltip(tip(skill_tooltip(skill))).into_element()
}

/// Consumable, remaining and total charges, one line per consumable. The
/// columns hold text, so they grow with the user's text scale.
fn consumables_table(consumables: &[ConsumableResult], icons: &TableIcons) -> AnyElement {
    let scale = font_scale();
    let name_width = CONSUMABLE_NAME_WIDTH * scale;
    let count_width = consumable_count_width(&mut EngineMeasure { text_scale: scale });
    let heading = |key: &str, width: f32| {
        text(t!(key).into_owned()).app_class(class("flex-none text-xs font-bold text-foreground truncate")).w(px(width))
    };
    let count = |content: String| {
        detail_text(content).app_class(class("flex-none text-xs text-foreground truncate")).w(px(count_width))
    };
    let header = view()
        .app_class(class("flex-row gap-2"))
        .child(heading("ui.replay.consumable_header_consumable", name_width))
        .child(heading("ui.replay.consumable_header_remaining", count_width))
        .child(heading("ui.replay.consumable_header_total", count_width));

    let lines = consumables.iter().map(|consumable| {
        let counts = consumable_counts(consumable);
        let mut name = view().app_class(class("flex-row flex-none items-center gap-1 overflow-hidden")).w(px(name_width));
        if let Some(icon) = icons.icon(&RowIcon::Consumable(consumable.icon_key.clone())) {
            name = name.child(
                image(icon.uri).app_class(class("flex-none")).w(px(CONSUMABLE_ICON_SIZE)).h(px(CONSUMABLE_ICON_SIZE)),
            );
        }
        name = name.child(
            detail_text(consumable.display_name.clone()).app_class(class("flex-1 min-w-0 text-xs text-foreground truncate")),
        );

        view()
            .app_class(class("flex-row items-center gap-2"))
            .tooltip(tip(counts.hover))
            .child(name)
            .child(count(counts.remaining))
            .child(count(counts.total))
            .into_element()
    });

    view()
        .app_class(class("flex-col gap-1"))
        .child(header)
        .children(lines.collect::<Vec<_>>())
        .into_element()
}

/// The ammo-type breakdown, then one line per other player with the share,
/// which holding alt turns around.
fn damage_section(
    table: Table,
    row: &PlayerRow,
    debug: bool,
    direction: DamageDirection,
) -> Option<AnyElement> {
    if nda_hidden(row, debug) {
        return Some(nda_text());
    }

    let breakdown = damage_breakdown(row, direction);
    if breakdown.is_none() && row.damage_interactions.is_none() {
        return None;
    }

    let mut parts = Vec::new();
    if let Some(rows) = breakdown {
        parts.push(breakdown_grid(rows));
        if row.damage_interactions.is_some() {
            parts.push(separator());
        }
    }
    if row.damage_interactions.is_some() {
        let player: AccountId = row.db_id;
        parts.push(dynamic(move || {
            let alt_held = table.alt_held.get();
            table.rows.with_untracked(|rows| {
                let rows = &rows.0;
                let Some(row) = rows.iter().find(|row| row.db_id == player) else {
                    return view().into_element();
                };
                breakdown_grid(&damage_target_rows(row, rows, direction, alt_held))
            })
        }));
    }

    Some(view().app_class(class("flex-col gap-1")).children(parts).into_element())
}
