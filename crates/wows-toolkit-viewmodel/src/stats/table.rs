//! One ship's session statistics as a table, and the text a copy produces.
//!
//! Both front ends show the same rows and copy the same text, so a figure
//! cannot read one way on screen and another in the clipboard. Labels stay as
//! enums: the egui app translates them and the port does not, and neither
//! choice can be made here.

use super::PerformanceInfo;
use super::PrStats;
use crate::formatting::separate_number;
use crate::personal_rating::PersonalRatingResult;

/// Which statistic a row reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatLabel {
    PersonalRating,
    Damage,
    SpottingDamage,
    Frags,
    RawXp,
    BaseXp,
}

impl StatLabel {
    /// The key the egui app looks this label up under.
    pub const fn translation_key(self) -> &'static str {
        match self {
            Self::PersonalRating => "stat.personal_rating",
            Self::Damage => "stat.damage",
            Self::SpottingDamage => "stat.spotting_damage",
            Self::Frags => "stat.frags",
            Self::RawXp => "stat.raw_xp",
            Self::BaseXp => "stat.base_xp",
        }
    }

    /// The untranslated text, which is what the port renders.
    pub const fn english(self) -> &'static str {
        match self {
            Self::PersonalRating => "Personal Rating",
            Self::Damage => "Damage",
            Self::SpottingDamage => "Spotting Damage",
            Self::Frags => "Frags",
            Self::RawXp => "Raw XP",
            Self::BaseXp => "Base XP",
        }
    }
}

/// Which figure a column reports.
///
/// The discriminants index [`StatRow::cells`], so this order is the order of
/// a row's cells and of [`COLUMNS`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Column {
    Min,
    Max,
    Total,
    Average,
}

impl Column {
    pub const fn translation_key(self) -> &'static str {
        match self {
            Self::Min => "ui.stats.table.min",
            Self::Max => "ui.stats.table.max",
            Self::Total => "ui.stats.table.total",
            Self::Average => "ui.stats.table.average",
        }
    }

    pub const fn english(self) -> &'static str {
        match self {
            Self::Min => "Min",
            Self::Max => "Max",
            Self::Total => "Total",
            Self::Average => "Average",
        }
    }
}

/// The columns, in the order the table lists them.
pub const COLUMNS: [Column; 4] = [Column::Min, Column::Max, Column::Total, Column::Average];

/// One figure in the table.
///
/// `text` is already formatted: the columns mix thousands-separated counts
/// and two-decimal averages, and which is which belongs with the statistic
/// rather than with each front end's renderer. Empty text is a figure that
/// has no meaning here (a rating does not sum), not a missing one.
///
/// `rating` is the figure itself when the cell reports a personal rating, so
/// a front end that colours ratings by band reads the band off the cell
/// rather than deriving it again from the text it was formatted into.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Cell {
    pub text: String,
    pub rating: Option<PersonalRatingResult>,
}

impl Cell {
    fn text(text: String) -> Self {
        Self { text, rating: None }
    }

    fn rated(rating: &PersonalRatingResult) -> Self {
        Self { text: format!("{:.0}", rating.pr), rating: Some(rating.clone()) }
    }
}

/// One statistic across a ship's games, one cell per column.
#[derive(Debug, Clone, PartialEq)]
pub struct StatRow {
    pub label: StatLabel,
    pub cells: [Cell; 4],
}

impl StatRow {
    pub fn cell(&self, column: Column) -> &Cell {
        &self.cells[column as usize]
    }
}

/// An absent extreme, which is what a ship with no games has. The empty cell
/// reads as "no figure" rather than as a zero that was never recorded.
fn optional_u64(value: Option<u64>, locale: Option<&str>) -> Cell {
    Cell::text(value.map(|value| separate_number(value, locale)).unwrap_or_default())
}

/// The same for a signed statistic.
fn optional_i64(value: Option<i64>, locale: Option<&str>) -> Cell {
    Cell::text(value.map(|value| separate_number(value, locale)).unwrap_or_default())
}

fn grouped_u64(value: u64, locale: Option<&str>) -> Cell {
    Cell::text(separate_number(value, locale))
}

fn grouped_i64(value: i64, locale: Option<&str>) -> Cell {
    Cell::text(separate_number(value, locale))
}

/// An average, empty when there were no games to average.
fn average_u64(value: Option<f64>, locale: Option<&str>) -> Cell {
    Cell::text(value.map(|value| separate_number(value as u64, locale)).unwrap_or_default())
}

fn average_i64(value: Option<f64>, locale: Option<&str>) -> Cell {
    Cell::text(value.map(|value| separate_number(value as i64, locale)).unwrap_or_default())
}

/// The rows for one ship, in the order the egui table lists them.
///
/// `rating` is this ship's rating across the games in view when one could be
/// computed; it leads the table because it is the figure the rest are read
/// against, and carries no total because ratings do not sum.
pub fn ship_rows(info: &PerformanceInfo, rating: Option<&PrStats>, locale: Option<&str>) -> Vec<StatRow> {
    let mut rows = Vec::with_capacity(6);

    if let Some(rating) = rating {
        rows.push(StatRow {
            label: StatLabel::PersonalRating,
            cells: [Cell::rated(&rating.min), Cell::rated(&rating.max), Cell::default(), Cell::rated(&rating.average)],
        });
    }

    rows.push(StatRow {
        label: StatLabel::Damage,
        cells: [
            optional_u64(info.min_damage(), locale),
            optional_u64(info.max_damage(), locale),
            grouped_u64(info.total_damage(), locale),
            average_u64(info.avg_damage(), locale),
        ],
    });
    rows.push(StatRow {
        label: StatLabel::SpottingDamage,
        cells: [
            optional_u64(info.min_spotting_damage(), locale),
            optional_u64(info.max_spotting_damage(), locale),
            grouped_u64(info.total_spotting_damage(), locale),
            average_u64(info.avg_spotting_damage(), locale),
        ],
    });
    rows.push(StatRow {
        label: StatLabel::Frags,
        cells: [
            optional_i64(info.min_frags(), locale),
            optional_i64(info.max_frags(), locale),
            grouped_i64(info.total_frags(), locale),
            // Two decimals: a frag average under one is the common case, and
            // rounding it to a whole number would read as zero.
            Cell::text(info.avg_frags().map(|value| format!("{value:.2}")).unwrap_or_default()),
        ],
    });
    rows.push(StatRow {
        label: StatLabel::RawXp,
        cells: [
            optional_i64(info.min_xp(), locale),
            optional_i64(info.max_xp(), locale),
            grouped_i64(info.total_xp(), locale),
            average_i64(info.avg_xp(), locale),
        ],
    });
    rows.push(StatRow {
        label: StatLabel::BaseXp,
        cells: [
            optional_i64(info.min_win_adjusted_xp(), locale),
            optional_i64(info.max_win_adjusted_xp(), locale),
            grouped_i64(info.total_win_adjusted_xp(), locale),
            average_i64(info.avg_win_adjusted_xp(), locale),
        ],
    });

    rows
}

/// The table as a Markdown block headed by `header`, with the average column
/// emphasised as it is on screen.
pub fn to_markdown(
    header: &str,
    rows: &[StatRow],
    column: impl Fn(Column) -> String,
    label: impl Fn(StatLabel) -> String,
) -> String {
    let headings: Vec<String> = COLUMNS.iter().map(|c| column(*c)).collect();
    let mut out = format!("**{header}**\n\n| | {} |\n", headings.join(" | "));
    out.push_str("|---|---|---|---|---|\n");
    for row in rows {
        out.push_str(&format!(
            "| {} | {} | {} | {} | **{}** |\n",
            label(row.label),
            row.cell(Column::Min).text,
            row.cell(Column::Max).text,
            row.cell(Column::Total).text,
            row.cell(Column::Average).text
        ));
    }
    out
}

/// The table as CSV, its first column unlabelled like the Markdown one.
///
/// Cells are written unquoted, so a locale that groups thousands with a comma
/// splits a figure across two fields. That is what the egui app has always
/// written and what anything already consuming this expects, so it is not
/// changed here.
pub fn to_csv(rows: &[StatRow], column: impl Fn(Column) -> String, label: impl Fn(StatLabel) -> String) -> String {
    let headings: Vec<String> = COLUMNS.iter().map(|c| column(*c)).collect();
    let mut out = format!(",{}\n", headings.join(","));
    for row in rows {
        out.push_str(&format!(
            "{},{},{},{},{}\n",
            label(row.label),
            row.cell(Column::Min).text,
            row.cell(Column::Max).text,
            row.cell(Column::Total).text,
            row.cell(Column::Average).text
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::PerGameStat;
    use crate::stats::per_ship_performance;

    fn game(damage: u64, frags: i64, xp: i64) -> PerGameStat {
        PerGameStat {
            ship_name: "Yamato".to_string(),
            ship_id: 1u64.into(),
            game_time: "t".to_string(),
            sort_key: "t".to_string(),
            player_id: 1,
            damage,
            spotting_damage: damage / 10,
            frags,
            raw_xp: xp,
            base_xp: xp / 2,
            is_win: true,
            is_loss: false,
            is_draw: false,
            is_div: false,
            match_group: "pvp".to_string(),
            achievements: Vec::new(),
        }
    }

    fn rows_for(games: &[PerGameStat]) -> Vec<StatRow> {
        let refs: Vec<&PerGameStat> = games.iter().collect();
        let ships = per_ship_performance(&refs);
        ship_rows(&ships[0].1, None, None)
    }

    fn text(rows: &[StatRow], label: StatLabel, column: Column) -> &str {
        &rows.iter().find(|row| row.label == label).expect("the statistic is listed").cell(column).text
    }

    fn english(label: StatLabel) -> String {
        label.english().to_string()
    }

    fn english_column(column: Column) -> String {
        column.english().to_string()
    }

    #[test]
    fn a_ships_rows_carry_its_span_and_its_average() {
        let rows = rows_for(&[game(10_000, 1, 1_000), game(30_000, 3, 3_000)]);

        assert_eq!(text(&rows, StatLabel::Damage, Column::Min), "10,000");
        assert_eq!(text(&rows, StatLabel::Damage, Column::Max), "30,000");
        assert_eq!(text(&rows, StatLabel::Damage, Column::Total), "40,000");
        assert_eq!(text(&rows, StatLabel::Damage, Column::Average), "20,000");
    }

    /// Every statistic reports its own span, not just the one the damage row
    /// exercises.
    #[test]
    fn each_statistic_spans_its_own_games() {
        let rows = rows_for(&[game(10_000, 1, 1_000), game(30_000, 3, 3_000)]);

        assert_eq!(text(&rows, StatLabel::SpottingDamage, Column::Min), "1,000");
        assert_eq!(text(&rows, StatLabel::SpottingDamage, Column::Max), "3,000");
        assert_eq!(text(&rows, StatLabel::Frags, Column::Min), "1");
        assert_eq!(text(&rows, StatLabel::Frags, Column::Max), "3");
        assert_eq!(text(&rows, StatLabel::Frags, Column::Total), "4");
        assert_eq!(text(&rows, StatLabel::RawXp, Column::Min), "1,000");
        assert_eq!(text(&rows, StatLabel::RawXp, Column::Max), "3,000");
        assert_eq!(text(&rows, StatLabel::BaseXp, Column::Min), "500");
        assert_eq!(text(&rows, StatLabel::BaseXp, Column::Max), "1,500");
    }

    /// A frag average under one must not round to zero.
    #[test]
    fn a_frag_average_keeps_two_decimals() {
        let rows = rows_for(&[game(1, 0, 1), game(1, 1, 1)]);
        assert_eq!(text(&rows, StatLabel::Frags, Column::Average), "0.50");
    }

    #[test]
    fn the_locale_reaches_every_cell() {
        let games = [game(1_234_567, 1, 1_000)];
        let refs: Vec<&PerGameStat> = games.iter().collect();
        let ships = per_ship_performance(&refs);

        let rows = ship_rows(&ships[0].1, None, Some("fr"));

        assert_eq!(text(&rows, StatLabel::Damage, Column::Min), "1 234 567");
        assert_eq!(text(&rows, StatLabel::Damage, Column::Total), "1 234 567");
    }

    /// A ship with no games reports no extreme rather than a zero it never
    /// recorded.
    #[test]
    fn an_empty_aggregate_reports_no_extremes() {
        let rows = ship_rows(&PerformanceInfo::default(), None, None);

        for label in [StatLabel::Damage, StatLabel::Frags, StatLabel::RawXp, StatLabel::BaseXp] {
            assert!(text(&rows, label, Column::Min).is_empty(), "{label:?} has no minimum");
            assert!(text(&rows, label, Column::Max).is_empty(), "{label:?} has no maximum");
            assert!(text(&rows, label, Column::Average).is_empty(), "{label:?} has no average");
        }
    }

    #[test]
    fn a_rating_leads_the_table_and_carries_no_total() {
        let games = [game(1, 0, 1)];
        let refs: Vec<&PerGameStat> = games.iter().collect();
        let ships = per_ship_performance(&refs);
        let rating = PrStats {
            min: PersonalRatingResult::new(900.0),
            max: PersonalRatingResult::new(2100.0),
            average: PersonalRatingResult::new(1650.0),
        };

        let rows = ship_rows(&ships[0].1, Some(&rating), None);

        assert_eq!(rows[0].label, StatLabel::PersonalRating);
        assert_eq!(rows[0].cell(Column::Min).text, "900");
        assert_eq!(rows[0].cell(Column::Max).text, "2100");
        assert_eq!(rows[0].cell(Column::Average).text, "1650");
        assert!(rows[0].cell(Column::Total).text.is_empty(), "a rating does not sum");
    }

    /// The band a renderer colours by comes off the cell, so it cannot drift
    /// from the figure printed in it.
    #[test]
    fn every_rating_cell_carries_its_own_band() {
        let games = [game(1, 0, 1)];
        let refs: Vec<&PerGameStat> = games.iter().collect();
        let ships = per_ship_performance(&refs);
        let rating = PrStats {
            min: PersonalRatingResult::new(400.0),
            max: PersonalRatingResult::new(2100.0),
            average: PersonalRatingResult::new(1650.0),
        };

        let rows = ship_rows(&ships[0].1, Some(&rating), None);
        let rating_row = &rows[0];

        assert_eq!(rating_row.cell(Column::Min).rating.as_ref().map(|r| r.category), Some(rating.min.category));
        assert_eq!(rating_row.cell(Column::Max).rating.as_ref().map(|r| r.category), Some(rating.max.category));
        assert_ne!(rating.min.category, rating.max.category, "the fixture spans two bands");
        assert!(rating_row.cell(Column::Total).rating.is_none(), "there is no total to band");

        let damage = rows.iter().find(|row| row.label == StatLabel::Damage).expect("damage is listed");
        assert!(damage.cells.iter().all(|cell| cell.rating.is_none()), "only ratings carry a band");
    }

    #[test]
    fn markdown_and_csv_carry_the_same_cells() {
        let rows = rows_for(&[game(10_000, 1, 1_000)]);

        let markdown = to_markdown("Yamato", &rows, english_column, english);
        assert!(markdown.starts_with("**Yamato**"));
        assert!(markdown.contains("| | Min | Max | Total | Average |"));
        assert!(markdown.contains("| Damage | 10,000 | 10,000 | 10,000 | **10,000** |"));

        // Unquoted, so the grouped figures split across fields. That is what
        // the egui app writes; see `to_csv`.
        let csv = to_csv(&rows, english_column, english);
        assert!(csv.starts_with(",Min,Max,Total,Average"));
        assert!(csv.contains("Damage,10,000,10,000,10,000,10,000"));
    }
}
