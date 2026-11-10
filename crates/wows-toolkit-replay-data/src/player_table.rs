//! The player table's labels, copy text and column widths, shared by the
//! front ends that draw it. Widths are measured through [`TextMeasure`], which
//! each front end implements over its own text engine.

use std::collections::HashSet;

use rust_i18n::t;
use wows_replay_insights::battle_report::ConnectionNote;
use wows_replays::types::AccountId;
use wows_toolkit_viewmodel::twitch::SniperCandidate;

use crate::cells::cell_value;
use crate::columns::ReplayColumn;
use crate::model::PlayerRow;
use crate::row_detail::expanded_detail_width;

/// Column width bounds, matching the egui app's
/// `Column::new(100.0).range(10.0..=500.0)`.
pub const COLUMN_MIN_WIDTH: f32 = 10.0;
pub const COLUMN_MAX_WIDTH: f32 = 500.0;

/// The narrowest a dragged column may be made. Below this a column's own
/// content is unreadable and its header is gone.
pub const COLUMN_DRAG_MIN: f32 = 28.0;

/// The ship-class icon in the Name cell, in pixels.
pub const SHIP_ICON_WIDTH: f32 = 16.0;

/// Room for the row actions button at the end of the Name cell.
pub const ROW_ACTIONS_WIDTH: f32 = 24.0;

/// Columns pinned to the left edge while the rest scroll horizontally: Name
/// and ShipName, which `default_columns` always puts first.
pub const STICKY_COLUMN_COUNT: usize = 2;

/// The most of the table's width the frozen columns may take, so the
/// scrolling columns stay reachable on a narrow panel.
pub const STICKY_MAX_FRACTION: f32 = 0.6;

/// Whether text is measured in the header's bold weight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextWeight {
    Regular,
    Bold,
}

/// A front end's text engine, as the width pass needs it.
pub trait TextMeasure {
    /// The rendered width of `text` on one line.
    fn width(&mut self, text: &str, weight: TextWeight) -> f32;

    /// The font size the spacing scale is relative to.
    fn rem(&self) -> f32;

    /// How much larger than its nominal size text is drawn (the user's text
    /// size setting), which fixed text columns grow by.
    fn text_scale(&self) -> f32;
}

/// Header label key for a column, matching the egui app's `ui.replay.column.*`
/// strings.
pub const fn column_label_key(col: ReplayColumn) -> &'static str {
    match col {
        ReplayColumn::Name => "ui.replay.column.player_name",
        ReplayColumn::ShipName => "ui.replay.column.ship_name",
        ReplayColumn::Skills => "ui.replay.column.skills",
        ReplayColumn::PersonalRating => "ui.replay.column.personal_rating",
        ReplayColumn::BaseXp => "stat.base_xp",
        ReplayColumn::RawXp => "stat.raw_xp",
        ReplayColumn::Kills => "ui.replay.column.kills",
        ReplayColumn::ObservedDamage => "ui.replay.column.observed_damage",
        ReplayColumn::ActualDamage => "ui.replay.column.actual_damage",
        ReplayColumn::ReceivedDamage => "ui.replay.column.received_damage",
        ReplayColumn::SpottingDamage => "stat.spotting_damage",
        ReplayColumn::PotentialDamage => "ui.replay.column.potential_damage",
        ReplayColumn::Hits => "ui.replay.column.hits",
        ReplayColumn::Heals => "ui.replay.column.heals",
        ReplayColumn::DistanceTraveled => "ui.replay.column.distance_traveled",
        ReplayColumn::TimeLived => "ui.replay.column.time_lived",
    }
}

/// The column's heading in the reader's own language.
pub fn column_label(col: ReplayColumn) -> String {
    t!(column_label_key(col)).into_owned()
}

/// Every column's content-fit width, indexed by `ReplayColumn as usize`: the
/// widest of the bold header label and every row's cell text, plus the
/// column's furniture (icons, marker glyphs, cell padding), clamped to
/// `COLUMN_MIN_WIDTH..=COLUMN_MAX_WIDTH`. A column not in `columns` keeps
/// `COLUMN_MIN_WIDTH`. A row in `expanded` also folds in its expanded detail,
/// which is drawn clipped to the column.
///
/// The Name column reserves `max(ship-class icon, species text)` for the icon
/// slot whether or not the icon has loaded, so the width holds across that.
pub fn measure_column_widths(
    rows: &[PlayerRow],
    columns: &[ReplayColumn],
    debug: bool,
    expanded: &HashSet<AccountId>,
    measure: &mut impl TextMeasure,
) -> Vec<f32> {
    let rem = measure.rem();
    let gap_1 = rem * 0.25;
    let gap_0p5 = rem * 0.125;
    let cell_padding = rem * 0.5;
    let icon_default = rem;
    // Cells truncate with an ellipsis the moment content reaches the
    // available width, so the text gets room beyond its own measure.
    let content_slack = rem * 0.5;

    let mut widths = vec![COLUMN_MIN_WIDTH; ReplayColumn::ALL.len()];
    for &col in columns {
        let mut max_w = measure.width(&column_label(col), TextWeight::Bold);
        for row in rows {
            let text_w = measure.width(&cell_value(row, col, debug).text, TextWeight::Regular);
            let mut row_w = match col {
                ReplayColumn::Name => {
                    let species_w = measure.width(&row.ship_species_text, TextWeight::Regular);
                    let icon_w = species_w.max(SHIP_ICON_WIDTH);
                    let status_count = usize::from(row.is_hidden_profile)
                        + usize::from(row.connection.is_some())
                        + usize::from(!row.twitch_candidates.is_empty());
                    icon_default
                        + gap_1
                        + icon_w
                        + gap_1
                        + text_w
                        + ROW_ACTIONS_WIDTH
                        + status_count as f32 * (icon_default + gap_1)
                }
                ReplayColumn::Skills => {
                    let marker_count = usize::from(row.skill_warning.is_some())
                        + usize::from(row.has_dazzle)
                        + usize::from(row.has_ifa);
                    if marker_count > 0 {
                        let markers_w =
                            marker_count as f32 * icon_default + marker_count.saturating_sub(1) as f32 * gap_0p5;
                        markers_w + gap_1 + text_w
                    } else {
                        text_w
                    }
                }
                _ => text_w,
            };
            if expanded.contains(&row.db_id) {
                row_w = row_w.max(expanded_detail_width(col, row, rows, debug, measure));
            }
            max_w = max_w.max(row_w);
        }
        widths[col as usize] = (max_w + cell_padding + content_slack).clamp(COLUMN_MIN_WIDTH, COLUMN_MAX_WIDTH);
    }

    widths
}

/// One row's visible cells, tab separated, which is what a spreadsheet reads
/// back as columns.
pub fn row_text(row: &PlayerRow, columns: &[ReplayColumn], debug: bool) -> String {
    columns.iter().map(|col| cell_value(row, *col, debug).text).collect::<Vec<_>>().join("\t")
}

/// The column headings, then every row in the order given.
pub fn table_text(rows: &[PlayerRow], columns: &[ReplayColumn], debug: bool) -> String {
    let mut lines = vec![columns.iter().map(|col| column_label(*col)).collect::<Vec<_>>().join("\t")];
    lines.extend(rows.iter().map(|row| row_text(row, columns, debug)));
    lines.join("\n")
}

/// `rows` as a Markdown pipe table under the column headings.
pub fn markdown_table<'a>(
    rows: impl IntoIterator<Item = &'a PlayerRow>,
    columns: &[ReplayColumn],
    debug: bool,
) -> String {
    let line = |cells: Vec<String>| format!("| {} |", cells.join(" | "));
    let mut lines = vec![
        line(columns.iter().map(|col| markdown_cell(&column_label(*col))).collect()),
        line(columns.iter().map(|_| "---".to_owned()).collect()),
    ];
    lines.extend(
        rows.into_iter()
            .map(|row| line(columns.iter().map(|col| markdown_cell(&cell_value(row, *col, debug).text)).collect())),
    );
    lines.join("\n")
}

/// `text` escaped for one cell of a Markdown pipe table.
pub fn markdown_cell(text: &str) -> String {
    let mut escaped = String::new();
    for ch in text.replace("\r\n", "\n").chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '\n' | '\r' => escaped.push_str("<br>"),
            '\\' | '|' | '`' | '*' | '_' | '[' | ']' | '~' => {
                escaped.push('\\');
                escaped.push(ch);
            }
            _ => escaped.push(ch),
        }
    }

    escaped
}

/// What a connection history says, in the egui app's own wording.
pub fn connection_hover_text(note: &ConnectionNote) -> String {
    match note {
        ConnectionNote::NeverConnected => t!("ui.replay.player.never_connected").into_owned(),
        ConnectionNote::Interrupted { events } => format!("Player {}", events.join(", ")),
    }
}

/// The Twitch chip's tooltip: each candidate's login and the minutes it was
/// seen, then how to copy. `None` without candidates, which is a row with no
/// chip.
pub fn twitch_hover_text(candidates: &[SniperCandidate]) -> Option<String> {
    if candidates.is_empty() {
        return None;
    }

    let names = candidates
        .iter()
        .map(|candidate| {
            let minutes = candidate.minutes.iter().map(i64::to_string).collect::<Vec<_>>().join(", ");
            format!(
                "{}\n{}",
                t!("ui.twitch.possible_name", name = candidate.login),
                t!("ui.twitch.seen_minutes", minutes = minutes)
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    Some(format!("{names}\n\n{}", t!("ui.twitch.click_to_copy")))
}

/// A row's actions menu heading as one run of text, for a screen reader:
/// `"[clan ]name[, ship]"`.
pub fn menu_heading_text(row: &PlayerRow) -> String {
    let mut spoken = String::new();
    if let Some(clan) = row.clan.as_ref() {
        spoken.push_str(&clan.tag);
        spoken.push(' ');
    }
    spoken.push_str(&row.display_name);
    if !row.ship_name.is_empty() {
        spoken.push_str(", ");
        spoken.push_str(&row.ship_name);
    }

    spoken
}

/// Whether the row's build links belong in its menu: the build is not shown
/// for enemies outside debug mode, and only exists once the replay saw the
/// vehicle.
pub fn shows_build_links(row: &PlayerRow, debug: bool) -> bool {
    (!row.relation.is_enemy() || debug) && row.has_vehicle_entity
}

#[cfg(test)]
mod tests {
    use wows_replays::types::Relation;

    use super::*;
    use crate::test_support::base_row;

    #[test]
    fn markdown_cell_escapes_markup_and_breaks_lines() {
        assert_eq!(markdown_cell("a|b_c\r\nd<e>&"), "a\\|b\\_c<br>d&lt;e&gt;&amp;");
    }

    #[test]
    fn a_row_and_the_table_copy_as_tab_separated_text() {
        let rows = vec![base_row(1, Relation::new(0), true), base_row(2, Relation::new(2), false)];
        let columns = [ReplayColumn::ShipName, ReplayColumn::Kills];

        let row = row_text(&rows[0], &columns, false);
        assert_eq!(row.split('\t').count(), 2, "{row:?}");

        let table = table_text(&rows, &columns, false);
        let lines: Vec<&str> = table.lines().collect();
        assert_eq!(lines.len(), 3, "a heading line and one line per row: {table:?}");
        assert_eq!(
            lines[0],
            format!("{}\t{}", column_label(ReplayColumn::ShipName), column_label(ReplayColumn::Kills))
        );
        assert_eq!(lines[1], row);
    }

    #[test]
    fn a_markdown_table_has_a_heading_a_rule_and_a_line_per_row() {
        let rows = vec![base_row(1, Relation::new(0), true)];
        let markdown = markdown_table(&rows, &[ReplayColumn::Kills], false);
        let lines: Vec<&str> = markdown.lines().collect();

        assert_eq!(lines.len(), 3, "{markdown:?}");
        assert_eq!(lines[1], "| --- |");
    }

    #[test]
    fn the_twitch_tooltip_lists_every_candidate_and_ends_with_how_to_copy() {
        assert_eq!(twitch_hover_text(&[]), None);

        let candidates = [
            SniperCandidate { login: "one".to_owned(), minutes: vec![2, 5] },
            SniperCandidate { login: "two".to_owned(), minutes: vec![7] },
        ];
        let text = twitch_hover_text(&candidates).expect("candidates give a tooltip");

        assert!(text.contains("one") && text.contains("two") && text.contains("2, 5"), "{text:?}");
        assert!(text.ends_with(t!("ui.twitch.click_to_copy").as_ref()), "{text:?}");
    }

    struct FixedWidth;

    impl TextMeasure for FixedWidth {
        fn width(&mut self, text: &str, _weight: TextWeight) -> f32 {
            text.chars().count() as f32 * 7.0
        }

        fn rem(&self) -> f32 {
            16.0
        }

        fn text_scale(&self) -> f32 {
            1.0
        }
    }

    #[test]
    fn a_column_not_shown_keeps_the_minimum_width_and_shown_ones_stay_in_bounds() {
        let rows = vec![base_row(1, Relation::new(0), true)];
        let widths = measure_column_widths(&rows, &[ReplayColumn::Name], false, &HashSet::new(), &mut FixedWidth);

        assert_eq!(widths.len(), ReplayColumn::ALL.len());
        assert_eq!(widths[ReplayColumn::Kills as usize], COLUMN_MIN_WIDTH);
        let name = widths[ReplayColumn::Name as usize];
        assert!(name > ROW_ACTIONS_WIDTH && name <= COLUMN_MAX_WIDTH, "{name}");
    }
}
