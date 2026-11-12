//! A player row's expanded detail: achievements, ribbons, damage events and the effective fire
//! chance under Name, the build under Skills, and the damage, potential, hits and spotting
//! breakdowns under their own columns.
//!
//! Game icons are drawn as their names: zgui's `image` element reads a path, and the icons live in
//! the game's packed files. That is the same fallback the GPUI front end draws for an icon it has
//! not resolved.

use std::rc::Rc;
use std::sync::Arc;

use rust_i18n::t;
use wows_replay_insights::battle_report::AchievementResult;
use wows_replay_insights::battle_report::ConsumableResult;
use wows_replay_insights::battle_report::DamageInteraction;
use wows_replay_insights::battle_report::RibbonResult;
use wows_replay_insights::battle_report::TranslatedModule;
use wows_replay_insights::fire_chance::analysis::EffectiveFireChance;
use wows_replay_insights::fire_chance::analysis::PerShipFireChance;
use wows_toolkit_replay_data::columns::NDA;
use wows_toolkit_replay_data::columns::ReplayColumn;
use wows_toolkit_replay_data::damage_mix::DamageLine;
use wows_toolkit_replay_data::model::PlayerRow;
use wows_toolkit_replay_data::model::separate_number;
use wows_toolkit_viewmodel::breakdown::BreakdownRow;
use wows_toolkit_viewmodel::fire_chance;
use wowsunpack::game_params::skill_grid_data::SkillGridRow;
use wowsunpack::game_params::skill_grid_data::SkillGridSkill;
use wowsunpack::game_types::ChargeCount;
use zgui::component;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::Signal;
use zgui::reactive::prelude::Get;
use zgui::reactive::use_local_context;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::SeparatorProps;
use zgui_ui::TooltipContentProps;
use zgui_ui::TooltipProps;
use zgui_ui::TooltipTriggerProps;

use crate::icons::AppIconProps;
use crate::shell::Shell;

const SHEET: &str = "wt-replay-expanded";

const CSS: &str = css!(
    ".wt-xr {
        display: flex;
        flex-direction: row;
        align-items: flex-start;
        gap: 16px;
        padding: 4px 8px 8px 8px;
    }
    .wt-xr-col {
        display: flex;
        flex-direction: column;
        gap: 4px;
        min-width: 0;
        font-size: 12px;
    }
    .wt-xr-col--name { gap: 3px; padding-left: 8px; }
    .wt-xr-heading {
        font-size: 12px;
        font-weight: 600;
        color: var(--wt-color-text-dim);
    }
    .wt-xr-dim { color: var(--wt-color-text-dim); }
    .wt-xr-faint { color: var(--wt-color-text-faint); }
    .wt-xr-grid {
        display: flex;
        flex-direction: row;
        align-items: flex-start;
        gap: 8px;
        font-size: 12px;
    }
    .wt-xr-grid__col { display: flex; flex-direction: column; gap: 1px; }
    .wt-xr-grid__col--end { align-items: flex-end; }
    .wt-xr-key { display: flex; flex-direction: row; align-items: center; gap: 4px; }
    .wt-xr-swatch { flex: none; width: 8px; height: 8px; }
    .wt-xr-tiles {
        display: flex;
        flex-direction: row;
        flex-wrap: wrap;
        align-items: flex-start;
        gap: 6px;
    }
    .wt-xr-tiles--end { align-items: flex-end; }
    .wt-xr-ribbons { display: flex; flex-direction: column; gap: 12px; }
    .wt-xr-tile {
        display: flex;
        flex-direction: column;
        align-items: center;
        flex: none;
        gap: 2px;
        width: 64px;
        text-align: center;
        font-size: 12px;
    }
    .wt-xr-cells {
        display: flex;
        flex-direction: row;
        flex-wrap: wrap;
        gap: 4px;
    }
    .wt-xr-empty-slot {
        display: flex;
        align-items: center;
        justify-content: center;
        width: 32px;
        height: 32px;
        color: var(--wt-color-text-faint);
    }
    .wt-xr-skills { display: flex; flex-direction: column; gap: 4px; }
    .wt-xr-skills__tier { display: flex; flex-direction: row; align-items: center; gap: 4px; }
    .wt-xr-skills__cost { flex: none; width: 14px; color: var(--wt-color-text-dim); }
    .wt-xr-skill--unlearned { color: var(--wt-color-text-faint); }
    .wt-xr-table { display: flex; flex-direction: column; gap: 4px; }
    .wt-xr-table__row { display: flex; flex-direction: row; align-items: center; gap: 8px; }
    .wt-xr-table__name { flex: none; width: 170px; }
    .wt-xr-table__count { flex: none; width: 64px; }
    .wt-xr-table__head { font-weight: 600; }
    .wt-xr-fire__headline { display: flex; flex-direction: row; align-items: center; gap: 8px; }
    .wt-xr-fire__counts {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 4px;
        font-weight: 600;
    }
    .wt-xr-mono { font-family: monospace; display: flex; flex-direction: column; }
    .wt-xr-tally { display: flex; flex-direction: column; gap: 1px; }
    .wt-xr-tally__row { display: flex; flex-direction: row; align-items: flex-start; gap: 8px; }
    .wt-xr-tally__count { flex: none; width: 44px; text-align: right; }
    .wt-xr-ship { display: flex; flex-direction: row; align-items: center; gap: 8px; }
    .wt-xr-ship__name { flex: none; width: 160px; }
    .wt-xr-tip { display: flex; flex-direction: column; align-items: center; gap: 2px; max-width: 280px; }
    .wt-xr-tip__name { font-weight: 600; }
    .wt-xr-tip__text { text-align: center; white-space: pre-wrap; }"
);

/// Whether a damage line is damage this row dealt, or damage it received.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DamageDirection {
    Dealt,
    Received,
}

/// The rows a table shows, shared by every row's detail.
pub type Rows = Arc<[PlayerRow]>;

/// Whether `column` has detail to draw for `row`, so a table knows which cells grow.
pub fn has_column_detail(column: ReplayColumn, row: &PlayerRow, rows: &[PlayerRow], debug: bool) -> bool {
    let hidden = row.should_hide_stats() && !debug;
    match column {
        ReplayColumn::Name => {
            !row.achievements.is_empty()
                || !row.ribbons.is_empty()
                || has_damage_events(row)
                || row.fire_chance.is_some()
        }
        ReplayColumn::Skills => build_has_content(row, debug),
        ReplayColumn::ActualDamage => {
            hidden || row.actual_damage_by_type.is_some() || row.damage_interactions.is_some()
        }
        ReplayColumn::ReceivedDamage => {
            hidden || row.received_damage_by_type.is_some() || row.damage_interactions.is_some()
        }
        ReplayColumn::PotentialDamage => hidden || row.potential_damage_breakdown.is_some(),
        ReplayColumn::Hits => hidden || row.hits_breakdown.is_some(),
        ReplayColumn::SpottingDamage => {
            row.spotting_damage_breakdown.is_some() || !spotting_target_rows(row, rows).is_empty()
        }
        _ => false,
    }
}

/// Every column's detail for `row`, side by side in `columns` order, for a table whose detail
/// spans the whole row. A column with nothing to show takes no room.
#[component]
pub fn ExpandedRow(
    #[prop(into)] columns: Signal<Rc<[ReplayColumn]>, LocalStorage>,
    row: Arc<PlayerRow>,
    rows: Rows,
    #[prop(into)] debug: Signal<bool, LocalStorage>,
    #[prop(into)] alt_held: Signal<bool, LocalStorage>,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);

    view! {
        row(class = "wt-xr", attr:data-testid = "replay-expanded-row") {
            {move || {
                let debug = debug.get();
                columns
                    .get()
                    .iter()
                    .filter(|column| has_column_detail(**column, &row, &rows, debug))
                    .map(|column| {
                        AnyView::new(view! {
                            ColumnDetail(
                                column = *column,
                                row = Arc::clone(&row),
                                rows = Arc::clone(&rows),
                                debug = debug,
                                alt_held = alt_held
                            )
                        })
                    })
                    .collect::<Vec<_>>()
            }}
        }
    }
}

/// One column's detail for `row`; empty when the column has none.
#[component]
pub fn ColumnDetail(
    column: ReplayColumn,
    row: Arc<PlayerRow>,
    rows: Rows,
    #[prop(into)] debug: Signal<bool, LocalStorage>,
    #[prop(into)] alt_held: Signal<bool, LocalStorage>,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let shell = use_local_context::<Shell>();

    move || {
        let debug = debug.get();
        let hidden = row.should_hide_stats() && !debug;
        match column {
            ReplayColumn::Name => name_section(&row, debug, shell),
            ReplayColumn::Skills => build_section(&row, debug),
            ReplayColumn::ActualDamage | ReplayColumn::ReceivedDamage if hidden => Some(nda_text()),
            ReplayColumn::ActualDamage => damage_section(&row, &rows, DamageDirection::Dealt, alt_held),
            ReplayColumn::ReceivedDamage => damage_section(&row, &rows, DamageDirection::Received, alt_held),
            ReplayColumn::PotentialDamage if hidden => Some(nda_text()),
            ReplayColumn::PotentialDamage => row.potential_damage_breakdown.as_deref().map(breakdown_grid),
            ReplayColumn::Hits if hidden => Some(nda_text()),
            ReplayColumn::Hits => row.hits_breakdown.as_deref().map(breakdown_grid),
            ReplayColumn::SpottingDamage => spotting_section(&row, &rows),
            _ => None,
        }
        .map(|content| {
            AnyView::new(view! {
                column(class = "wt-xr-col", attr:data-column = format!("{column:?}")) {{content}}
            })
        })
    }
}

fn text(text: impl Into<String>) -> AnyView {
    let text = text.into();
    AnyView::new(view! { text {{text}} })
}

fn classed_text(class: &'static str, text: impl Into<String>) -> AnyView {
    let text = text.into();
    AnyView::new(view! { text(class = class) {{text}} })
}

fn heading(text: String) -> AnyView {
    classed_text("wt-xr-heading", text)
}

fn nda_text() -> AnyView {
    text(NDA)
}

fn separator() -> AnyView {
    AnyView::new(view! { Separator() })
}

/// What a game icon's tooltip says: its name in bold over its description.
fn icon_tip(name: String, description: Option<&str>) -> impl Fn() -> AnyView + 'static {
    let description = description.filter(|text| !text.trim().is_empty()).map(str::to_owned);
    move || {
        let name = name.clone();
        let description = description.clone();
        AnyView::new(view! {
            column(class = "wt-xr-tip") {
                text(class = "wt-xr-tip__name") {{name}}
                {description.map(|text| view! { text(class = "wt-xr-tip__text") {{text}} })}
            }
        })
    }
}

/// `content` with the view `tip` builds shown on hover.
fn with_tip(content: AnyView, tip: impl Fn() -> AnyView + 'static) -> AnyView {
    AnyView::new(view! {
        Tooltip {
            TooltipTrigger {{content}}
            TooltipContent {{tip()}}
        }
    })
}

/// A label-and-value listing: labels in one column and values in another, so the two line up.
fn breakdown_grid(rows: &[BreakdownRow]) -> AnyView {
    let labels: Vec<AnyView> = rows.iter().map(|row| text(row.label.clone())).collect();
    let values: Vec<AnyView> = rows.iter().map(|row| text(row.value.clone())).collect();
    AnyView::new(view! {
        row(class = "wt-xr-grid") {
            column(class = "wt-xr-grid__col") {{labels}}
            column(class = "wt-xr-grid__col wt-xr-grid__col--end") {{values}}
        }
    })
}

/// The per-type damage listing, each type with the colour key its stretch of the cell's bar uses.
fn damage_lines_grid(lines: &[DamageLine]) -> AnyView {
    let keys: Vec<AnyView> = lines
        .iter()
        .map(|line| {
            let [r, g, b] = line.kind.rgb();
            // The theme's `--wt-damage-<kind>`, which darkens the key on light themes.
            let kind = format!("{:?}", line.kind).to_lowercase();
            let color = format!("var(--wt-damage-{kind}, rgb({r}, {g}, {b}))");
            let label = line.label.to_owned();
            AnyView::new(view! {
                row(class = "wt-xr-key") {
                    box(class = "wt-xr-swatch", style:background-color = color)
                    text {{label}}
                }
            })
        })
        .collect();
    let values: Vec<AnyView> = lines.iter().map(|line| text(separate_number(line.amount))).collect();
    AnyView::new(view! {
        row(class = "wt-xr-grid", attr:data-testid = "replay-damage-lines") {
            column(class = "wt-xr-grid__col") {{keys}}
            column(class = "wt-xr-grid__col wt-xr-grid__col--end") {{values}}
        }
    })
}

fn has_damage_events(row: &PlayerRow) -> bool {
    row.fires.is_some() || row.floods.is_some() || row.citadels.is_some() || row.crits.is_some()
}

/// Name's detail: achievements, ribbons, damage events, then the effective fire chance.
fn name_section(row: &PlayerRow, debug: bool, shell: Option<Shell>) -> Option<AnyView> {
    let has_achievements = !row.achievements.is_empty();
    let has_ribbons = !row.ribbons.is_empty();
    let has_events = has_damage_events(row);
    let fire = fire_chance_section(row, debug, shell);
    if !has_achievements && !has_ribbons && !has_events && fire.is_none() {
        return None;
    }

    let mut parts: Vec<AnyView> = Vec::new();
    if has_achievements {
        parts.push(heading(t!("ui.replay.sections.achievements").into_owned()));
        parts.push(achievements_view(&row.achievements));
    }
    if has_ribbons {
        if has_achievements {
            parts.push(separator());
        }
        parts.push(heading(t!("ui.replay.sections.ribbons").into_owned()));
        let mut ribbons: Vec<&RibbonResult> = row.ribbons.iter().collect();
        ribbons.sort_by(|a, b| a.name.cmp(&b.name));
        reorder_bulge_after_main_caliber(&mut ribbons);
        parts.push(ribbons_view(ribbons));
    }
    if has_events {
        if has_achievements || has_ribbons {
            parts.push(separator());
        }
        parts.push(heading(t!("ui.replay.sections.damage_events").into_owned()));
        parts.push(if row.should_hide_stats() && !debug {
            nda_text()
        } else {
            breakdown_grid(&damage_event_rows(row))
        });
    }
    if let Some(fire) = fire {
        if has_achievements || has_ribbons || has_events {
            parts.push(separator());
        }
        parts.push(fire);
    }

    Some(AnyView::new(view! { column(class = "wt-xr-col wt-xr-col--name") {{parts}} }))
}

/// The damage events a row records: one line per event kind the battle produced.
fn damage_event_rows(row: &PlayerRow) -> Vec<BreakdownRow> {
    [
        (t!("ui.replay.column.fires"), row.fires),
        (t!("ui.replay.column.floods"), row.floods),
        (t!("ui.replay.column.citadels"), row.citadels),
        (t!("ui.replay.column.crits"), row.crits),
    ]
    .into_iter()
    .filter_map(|(label, count)| {
        count.map(|count| BreakdownRow { label: label.into_owned(), value: separate_number(count) })
    })
    .collect()
}

/// Ported from the egui app: RIBBON_BULGE goes immediately after RIBBON_MAIN_CALIBER when both
/// are present.
fn reorder_bulge_after_main_caliber(ribbons: &mut Vec<&RibbonResult>) {
    let Some(main_caliber) = ribbons.iter().position(|r| r.name == "RIBBON_MAIN_CALIBER") else {
        return;
    };
    let Some(bulge_at) = ribbons.iter().position(|r| r.name == "RIBBON_BULGE") else {
        return;
    };
    let bulge = ribbons.remove(bulge_at);
    let insert_at = if bulge_at < main_caliber { main_caliber } else { main_caliber + 1 };
    ribbons.insert(insert_at, bulge);
}

/// Ribbons split for display: those with no sub-ribbons, and each ribbon that has some followed
/// by them.
struct RibbonGroups<'a> {
    standalone: Vec<&'a RibbonResult>,
    families: Vec<Vec<&'a RibbonResult>>,
}

/// The name order already places each sub-ribbon after its parent, so a sub-ribbon belongs to the
/// nearest ribbon before it that is not one.
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

fn achievements_view(achievements: &[AchievementResult]) -> AnyView {
    let tiles: Vec<AnyView> = achievements.iter().map(achievement_tile).collect();
    AnyView::new(view! { row(class = "wt-xr-tiles") {{tiles}} })
}

/// Standalone ribbons first; each ribbon with sub-ribbons then starts a line of its own, so a
/// family reads as one block.
fn ribbons_view(ribbons: Vec<&RibbonResult>) -> AnyView {
    let RibbonGroups { standalone, families } = group_ribbons(ribbons);
    let grid = |tiles: Vec<&RibbonResult>| {
        let tiles: Vec<AnyView> = tiles.into_iter().map(ribbon_tile).collect();
        AnyView::new(view! { row(class = "wt-xr-tiles wt-xr-tiles--end") {{tiles}} })
    };
    let mut lines = Vec::new();
    if !standalone.is_empty() {
        lines.push(grid(standalone));
    }
    lines.extend(families.into_iter().map(grid));
    AnyView::new(view! { column(class = "wt-xr-ribbons") {{lines}} })
}

fn achievement_tile(achievement: &AchievementResult) -> AnyView {
    let name = achievement.display_name.clone();
    let count = (achievement.count > 1).then(|| format!("x{}", achievement.count));
    let tile = AnyView::new(view! {
        column(class = "wt-xr-tile", attr:data-testid = "replay-achievement") {
            text {{name.clone()}}
            {count.map(|count| view! { text(attr:data-testid = "replay-achievement-count", attr:data-count-of = name.clone()) {{count}} })}
        }
    });
    with_tip(tile, icon_tip(achievement.display_name.clone(), Some(&achievement.description)))
}

/// The count reads "x20" rather than "20" so it is read as a multiplier on the ribbon.
fn ribbon_tile(ribbon: &RibbonResult) -> AnyView {
    let name = ribbon.display_name.clone();
    let count = format!("x{}", ribbon.count);
    let tile = AnyView::new(view! {
        column(class = "wt-xr-tile", attr:data-testid = "replay-ribbon") {
            text {{name}}
            text {{count}}
        }
    });
    with_tip(tile, icon_tip(ribbon.display_name.clone(), ribbon.description.as_deref()))
}

/// Whether Skills has anything to draw: never for an enemy, unless `debug`.
fn build_has_content(row: &PlayerRow, debug: bool) -> bool {
    if row.relation.is_enemy() && !debug {
        return false;
    }
    row.skill_hover_text.is_some() || row.translated_build.is_some() || !row.consumables.is_empty()
}

/// Skills' detail: the skill-tier text, the build the replay observed, then the consumables.
fn build_section(row: &PlayerRow, debug: bool) -> Option<AnyView> {
    if !build_has_content(row, debug) {
        return None;
    }

    let mut parts: Vec<AnyView> = Vec::new();
    if let Some(hover) = row.skill_hover_text.as_ref() {
        parts.push(text(hover.clone()));
    }

    if let Some(build) = row.translated_build.as_ref() {
        if !parts.is_empty() {
            parts.push(separator());
        }
        if build.modernization_slots.is_empty() {
            parts.push(text(t!("ui.replay.sections.modules_none").into_owned()));
        } else {
            parts.push(heading(t!("ui.replay.sections.modules").into_owned()));
            parts.push(modernization_slots_view(&build.modernization_slots));
        }

        if !build.signals.is_empty() {
            parts.push(separator());
            parts.push(heading(t!("ui.replay.sections.signals").into_owned()));
            parts.push(signals_view(&build.signals));
        }

        parts.push(separator());
        if build.loadout.is_empty() {
            parts.push(text(t!("ui.replay.sections.loadout_none").into_owned()));
        } else {
            parts.push(heading(t!("ui.replay.sections.loadout").into_owned()));
            for module in &build.loadout {
                if let Some(name) = module.name.as_ref() {
                    parts.push(text_row(name.clone(), module.description.clone()));
                }
            }
        }

        parts.push(separator());
        match build.captain_skills.as_ref() {
            Some(skills) if !skills.is_empty() => {
                parts.push(heading(t!("ui.replay.sections.captain_skills").into_owned()));
                parts.push(captain_skill_grid_view(skills));
            }
            Some(_) => {
                parts.push(heading(t!("ui.replay.sections.captain_skills").into_owned()));
                parts.push(text(t!("ui.replay.sections.captain_skills_none").into_owned()));
            }
            None => parts.push(text(t!("ui.replay.sections.captain_skills_none").into_owned())),
        }
    }

    if !row.consumables.is_empty() {
        if !parts.is_empty() {
            parts.push(separator());
        }
        parts.push(heading(t!("ui.replay.sections.consumables").into_owned()));
        parts.push(consumables_view(&row.consumables));
    } else if let Some(build) = row.translated_build.as_ref()
        && !build.abilities.is_empty()
    {
        parts.push(separator());
        parts.push(heading(t!("ui.replay.sections.consumables").into_owned()));
        for ability in &build.abilities {
            if let Some(name) = ability.name.as_ref() {
                parts.push(text_row(name.clone(), None));
            }
        }
    }

    Some(AnyView::new(view! { column(class = "wt-xr-col", attr:data-testid = "replay-build") {{parts}} }))
}

fn module_cell(module: &TranslatedModule, testid: &'static str) -> AnyView {
    let name = module.name.clone().unwrap_or_else(|| module.game_params_name.clone());
    let cell = AnyView::new(view! { text(attr:data-testid = testid) {{name.clone()}} });
    with_tip(cell, icon_tip(name, module.description.as_deref()))
}

fn modernization_slots_view(slots: &[Option<TranslatedModule>]) -> AnyView {
    let cells: Vec<AnyView> = slots
        .iter()
        .map(|slot| match slot {
            Some(module) => module_cell(module, "replay-modernization"),
            None => AnyView::new(view! { box(class = "wt-xr-empty-slot") {"-"} }),
        })
        .collect();
    AnyView::new(view! { row(class = "wt-xr-cells") {{cells}} })
}

fn signals_view(signals: &[TranslatedModule]) -> AnyView {
    let cells: Vec<AnyView> = signals.iter().map(|signal| module_cell(signal, "replay-signal")).collect();
    AnyView::new(view! { row(class = "wt-xr-cells") {{cells}} })
}

/// Plain text, with `tip` on hover when there is one.
fn text_row(label: String, tip: Option<String>) -> AnyView {
    match tip.filter(|tip| !tip.is_empty()) {
        Some(tip) => with_tip(text(label), move || text(tip.clone())),
        None => text(label),
    }
}

/// One line per skill-point tier: the point cost, then each skill in that tier, dimmed when not
/// learned.
fn captain_skill_grid_view(tiers: &[SkillGridRow]) -> AnyView {
    let lines: Vec<AnyView> = tiers
        .iter()
        .map(|tier| {
            let cost = tier.point_cost.map(|c| c.get().to_string()).unwrap_or_default();
            let cells: Vec<AnyView> = tier.skills.iter().map(skill_cell).collect();
            AnyView::new(view! {
                row(class = "wt-xr-skills__tier") {
                    text(class = "wt-xr-skills__cost") {{cost}}
                    {cells}
                }
            })
        })
        .collect();
    AnyView::new(view! { column(class = "wt-xr-skills") {{lines}} })
}

fn skill_cell(skill: &SkillGridSkill) -> AnyView {
    let name = skill.name.clone().unwrap_or_else(|| skill.internal_name.as_str().to_string());
    let cost = skill.point_cost.map(|c| format!(" ({} pt)", c.get())).unwrap_or_default();
    let label = match skill.point_cost {
        Some(c) => format!("({}) {name}", c.get()),
        None => name.clone(),
    };
    let class = if skill.learned { "" } else { "wt-xr-skill--unlearned" };
    let learned = skill.learned.then(String::new);
    let cell = AnyView::new(view! {
        text(class = class, attr:data-testid = "replay-skill", attr:data-learned = learned) {{label}}
    });
    with_tip(cell, icon_tip(format!("{name}{cost}"), skill.description.as_deref()))
}

/// Consumable / Remaining / Total. An unlimited consumable shows its activations under Remaining
/// and the infinity mark under Total, as the egui inventory does.
fn consumables_view(consumables: &[ConsumableResult]) -> AnyView {
    let rows: Vec<AnyView> = consumables.iter().map(consumable_row).collect();
    let remaining = t!("ui.replay.consumable_header_remaining").into_owned();
    let total = t!("ui.replay.consumable_header_total").into_owned();
    AnyView::new(view! {
        column(class = "wt-xr-table") {
            row(class = "wt-xr-table__row wt-xr-table__head") {
                box(class = "wt-xr-table__name")
                text(class = "wt-xr-table__count") {{remaining}}
                text(class = "wt-xr-table__count") {{total}}
            }
            {rows}
        }
    })
}

/// What a consumable's counts read as: remaining, total, and the sentence its tooltip says.
fn consumable_counts(consumable: &ConsumableResult) -> (String, String, String) {
    match consumable.total_charges {
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
    }
}

fn consumable_row(consumable: &ConsumableResult) -> AnyView {
    let (remaining, total, said) = consumable_counts(consumable);
    let name = consumable.display_name.clone();
    let line = AnyView::new(view! {
        row(class = "wt-xr-table__row", attr:data-testid = "replay-consumable") {
            text(class = "wt-xr-table__name") {{name}}
            text(class = "wt-xr-table__count") {{remaining}}
            text(class = "wt-xr-table__count") {{total}}
        }
    });
    with_tip(line, icon_tip(consumable.display_name.clone(), Some(&said)))
}

/// The effective-fire-chance block. Only the recording player's row carries one.
fn fire_chance_section(row: &PlayerRow, debug: bool, shell: Option<Shell>) -> Option<AnyView> {
    let fire_chance = row.fire_chance.as_ref()?;

    let mut parts = vec![heading(t!("ui.replay.sections.fire_chance").into_owned())];
    if row.should_hide_stats() && !debug {
        parts.push(text(t!("ui.replay.nda").into_owned()));
        return Some(fire_column(parts));
    }
    if fire_chance.eligible_hits == 0 {
        // A total with nothing behind it would read as a real zero.
        parts.push(classed_text("wt-xr-dim", t!("ui.replay.sections.fire_chance_no_eligible_hits").into_owned()));
        return Some(fire_column(parts));
    }

    // Observed and expected sit together: comparing them is the point of the statistic.
    let counts = fire_chance::counts_text(fire_chance.fires, fire_chance.eligible_hits);
    let expected = fire_chance::expected_fires_text(fire_chance);
    let copy = copy_breakdown_button(fire_chance, shell);
    parts.push(AnyView::new(view! {
        row(class = "wt-xr-fire__headline", attr:data-testid = "replay-fire-chance") {
            row(class = "wt-xr-fire__counts") {
                AppIcon(glyph = wows_toolkit_icons::Glyph::Flame)
                text {{counts}}
            }
            {expected.map(|expected| view! { text(class = "wt-xr-dim") {{expected}} })}
            {copy}
        }
    }));
    parts.push(classed_text("wt-xr-dim", fire_chance::ships_text(fire_chance).into_owned()));

    // Monospace, because the formula's columns are padded to line up.
    let formula = fire_chance::fire_chance_formula_lines(fire_chance, &raw_source);
    if !formula.is_empty() {
        let lines: Vec<AnyView> = formula.into_iter().map(text).collect();
        parts.push(separator());
        parts.push(AnyView::new(view! { column(class = "wt-xr-mono") {{lines}} }));
    }

    parts.push(separator());
    parts.push(tally_block(&fire_chance::fire_chance_battle_tally_rows(fire_chance)));
    let ribbons = fire_chance::fire_chance_ribbon_rows(fire_chance);
    if !ribbons.is_empty() {
        parts.push(tally_block(&ribbons));
    }

    // A rate only means something inside a single victim, whose fire resistance is fixed.
    let ships = fire_chance::sorted_per_ship(fire_chance);
    if !ships.is_empty() {
        parts.push(separator());
        parts.push(heading(t!("ui.replay.sections.fire_chance_per_ship").into_owned()));
        for ship in ships {
            let name = ship.victim_ship_name.clone();
            let counts = fire_chance::counts_text(ship.fires, ship.eligible_hits);
            parts.push(AnyView::new(view! {
                row(class = "wt-xr-ship", attr:data-testid = "replay-fire-chance-ship") {
                    text(class = "wt-xr-ship__name") {{name}}
                    text(class = "wt-xr-dim") {{counts}}
                }
            }));
        }
        if let Some(line) = fire_chance::no_target_ship_line(fire_chance) {
            parts.push(classed_text("wt-xr-faint", line));
        }
    }

    Some(fire_column(parts))
}

fn fire_column(parts: Vec<AnyView>) -> AnyView {
    AnyView::new(view! { column(class = "wt-xr-col", style:gap = "3px") {{parts}} })
}

/// Counts in a column of their own, labels indented by their depth.
fn tally_block(rows: &[fire_chance::TallyRow]) -> AnyView {
    let lines: Vec<AnyView> = rows
        .iter()
        .map(|row| {
            let count = separate_number(row.count);
            let label = row.label.to_string();
            let indent = format!("{}px", row.depth * 12);
            let class = if row.depth > 0 { "wt-xr-dim" } else { "" };
            AnyView::new(view! {
                row(class = "wt-xr-tally__row") {
                    text(class = "wt-xr-tally__count") {{count}}
                    text(class = class, style:padding-left = indent) {{label}}
                }
            })
        })
        .collect();
    AnyView::new(view! { column(class = "wt-xr-tally") {{lines}} })
}

/// Puts the whole breakdown on the clipboard, which is what the egui headline does when clicked.
fn copy_breakdown_button(fire_chance: &EffectiveFireChance, shell: Option<Shell>) -> AnyView {
    let copied = fire_chance::copy_text(fire_chance, &raw_source, &raw_ship_name);
    let tip = t!("ui.replay.sections.fire_chance_click_to_copy").into_owned();
    let button = AnyView::new(view! {
        Button(
            variant = ButtonVariant::Ghost,
            size = ButtonSize::IconSm,
            attr:data-testid = "replay-fire-chance-copy",
            a11y:label = tip.clone(),
            on:click = move |_| {
                if let Some(shell) = shell {
                    shell.copy_text(copied.clone());
                    shell.toasts().ok(t!("ui.replay.sections.fire_chance_copied").into_owned());
                }
            }
        ) {
            AppIcon(glyph = wows_toolkit_icons::Glyph::Copy)
        }
    });
    with_tip(button, move || text(tip.clone()))
}

/// A modifier's raw source identifier. Resolving it needs the metadata provider, which the
/// detail is not handed; the GPUI front end shows it raw for the same reason.
fn raw_source(source: &str) -> String {
    source.to_owned()
}

fn raw_ship_name(ship: &PerShipFireChance) -> String {
    ship.victim_ship_name.clone()
}

/// A player as a share tooltip names them: by player and ship, as two rows can be in the same
/// ship.
#[derive(Clone, Debug, PartialEq)]
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
#[derive(Clone, Debug, PartialEq)]
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
}

fn interaction_amount(interaction: &DamageInteraction, direction: DamageDirection) -> u64 {
    match direction {
        DamageDirection::Dealt => interaction.damage_dealt,
        DamageDirection::Received => interaction.damage_received,
    }
}

/// Normally the share of this row's own total; with Alt down, of the other player's, which
/// answers "how much of what they took was me".
fn interaction_percentage(interaction: &DamageInteraction, direction: DamageDirection, inverted: bool) -> f64 {
    match (direction, inverted) {
        (DamageDirection::Dealt, false) => interaction.damage_dealt_percentage,
        (DamageDirection::Dealt, true) => interaction.damage_dealt_inverse_percentage,
        (DamageDirection::Received, false) => interaction.damage_received_percentage,
        (DamageDirection::Received, true) => interaction.damage_received_inverse_percentage,
    }
}

/// One line per ship this row dealt damage to or took it from, largest first, skipping zero
/// amounts and players `rows` does not name.
fn damage_interactions(row: &PlayerRow, rows: &[PlayerRow], direction: DamageDirection) -> Vec<InteractionLine> {
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
            let other = rows.iter().find(|r| r.db_id == *account_id)?;
            Some(InteractionLine {
                other: Party { player: other.display_name.clone(), ship: other.ship_name.clone() },
                amount,
                share: interaction_percentage(interaction, direction, false),
                inverse_share: interaction_percentage(interaction, direction, true),
            })
        })
        .collect()
}

/// What a damage line's share means in the mode it is shown in, then how to see the other one.
/// While inverted the key is already down, so the hint says to let it go.
fn share_text(direction: DamageDirection, inverted: bool, share: f64, this: &str, other: &str) -> String {
    let key = "Alt";
    let pct = format!("{share:.0}%");
    let said = match (direction, inverted) {
        (DamageDirection::Dealt, false) => t!("ui.replay.damage_share.dealt", pct = pct, ship = this, other = other),
        (DamageDirection::Dealt, true) => {
            t!("ui.replay.damage_share.dealt_inverse", pct = pct, ship = this, other = other)
        }
        (DamageDirection::Received, false) => {
            t!("ui.replay.damage_share.received", pct = pct, ship = this, other = other)
        }
        (DamageDirection::Received, true) => {
            t!("ui.replay.damage_share.received_inverse", pct = pct, ship = this, other = other)
        }
    };
    let hint = if inverted {
        t!("ui.replay.damage_share.release", key = key)
    } else {
        t!("ui.replay.damage_share.hold", key = key)
    };
    format!("{said}\n{hint}")
}

/// ActualDamage / ReceivedDamage: the per-type breakdown, then one line per ship at the share Alt
/// selects. The caller has already applied the NDA gate.
fn damage_section(
    row: &PlayerRow,
    rows: &[PlayerRow],
    direction: DamageDirection,
    alt_held: Signal<bool, LocalStorage>,
) -> Option<AnyView> {
    let breakdown = match direction {
        DamageDirection::Dealt => row.actual_damage_by_type.as_deref(),
        DamageDirection::Received => row.received_damage_by_type.as_deref(),
    };
    let has_interactions = row.damage_interactions.is_some();
    if breakdown.is_none() && !has_interactions {
        return None;
    }

    let mut parts = Vec::new();
    if let Some(lines) = breakdown {
        parts.push(damage_lines_grid(lines));
        if has_interactions {
            parts.push(separator());
        }
    }
    if has_interactions {
        let this = Party { player: row.display_name.clone(), ship: row.ship_name.clone() };
        parts.push(interaction_grid(damage_interactions(row, rows, direction), direction, this, alt_held));
    }
    Some(AnyView::new(view! { column(class = "wt-xr-col") {{parts}} }))
}

/// The per-ship lines; each explains its share on hover, and Alt flips every share.
fn interaction_grid(
    lines: Vec<InteractionLine>,
    direction: DamageDirection,
    this: Party,
    alt_held: Signal<bool, LocalStorage>,
) -> AnyView {
    let lines = Rc::new(lines);
    let this = Rc::new(this);
    let labels: Vec<AnyView> = lines.iter().map(|line| text(line.other.ship.clone())).collect();
    let values: Vec<AnyView> = (0..lines.len())
        .map(|at| {
            let value_lines = Rc::clone(&lines);
            let tip_lines = Rc::clone(&lines);
            let this = Rc::clone(&this);
            let value = AnyView::new(view! {
                text(attr:data-testid = "replay-damage-share") {{move || value_lines[at].value(alt_held.get())}}
            });
            let tip = move || {
                let tip_lines = Rc::clone(&tip_lines);
                let this = Rc::clone(&this);
                AnyView::new(view! {
                    text(class = "wt-xr-tip__text") {{move || {
                        let line = &tip_lines[at];
                        let inverted = alt_held.get();
                        share_text(direction, inverted, line.shown_share(inverted), &this.named(), &line.other.named())
                    }}}
                })
            };
            with_tip(value, tip)
        })
        .collect();
    AnyView::new(view! {
        row(class = "wt-xr-grid", attr:data-testid = "replay-damage-interactions") {
            column(class = "wt-xr-grid__col") {{labels}}
            column(class = "wt-xr-grid__col wt-xr-grid__col--end") {{values}}
        }
    })
}

/// SpottingDamage: the per-type split, then one line per spotted victim. Not NDA-gated: the
/// figure is damage allies dealt, which says nothing about this row's ship.
fn spotting_section(row: &PlayerRow, rows: &[PlayerRow]) -> Option<AnyView> {
    let breakdown = row.spotting_damage_breakdown.as_deref();
    let per_ship = spotting_target_rows(row, rows);
    if breakdown.is_none() && per_ship.is_empty() {
        return None;
    }

    let mut parts = Vec::new();
    if let Some(breakdown) = breakdown {
        parts.push(breakdown_grid(breakdown));
        if !per_ship.is_empty() {
            parts.push(separator());
        }
    }
    if !per_ship.is_empty() {
        parts.push(breakdown_grid(&per_ship));
    }
    Some(AnyView::new(view! { column(class = "wt-xr-col") {{parts}} }))
}

/// One line per spotted victim, largest first. The share is left off when the results carry no
/// spotting total to divide by.
fn spotting_target_rows(row: &PlayerRow, rows: &[PlayerRow]) -> Vec<BreakdownRow> {
    let Some(interactions) = row.damage_interactions.as_ref() else {
        return Vec::new();
    };
    let mut entries: Vec<_> = interactions.iter().filter(|(_, interaction)| interaction.spotting_damage > 0).collect();
    entries.sort_by_key(|(_, interaction)| std::cmp::Reverse(interaction.spotting_damage));
    entries
        .into_iter()
        .filter_map(|(account_id, interaction)| {
            let other = rows.iter().find(|r| r.db_id == *account_id)?;
            let amount = separate_number(interaction.spotting_damage);
            let value = match interaction.spotting_damage_percentage {
                Some(pct) => format!("{amount} ({pct:.0}%)"),
                None => amount,
            };
            Some(BreakdownRow { label: other.ship_name.clone(), value })
        })
        .collect()
}

#[cfg(test)]
mod tests;
