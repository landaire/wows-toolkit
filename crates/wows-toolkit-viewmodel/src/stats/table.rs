//! One ship's session statistics as a table, and the text a copy produces.
//!
//! Both front ends show the same rows and copy the same text, so a figure
//! cannot read one way on screen and another in the clipboard. Labels stay as
//! enums: the egui app translates them and the port does not, and neither
//! choice can be made here.

use super::PerformanceInfo;
use super::PrStats;
use crate::formatting::separate_number;

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

/// One statistic across a ship's games.
///
/// Every cell is already formatted: the columns mix thousands-separated
/// counts and two-decimal averages, and which is which belongs with the
/// statistic rather than with each front end's renderer. An empty cell is a
/// figure that has no meaning here (a rating does not sum), not a missing one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatRow {
    pub label: StatLabel,
    pub min: String,
    pub max: String,
    pub total: String,
    pub average: String,
}

impl StatRow {
    pub fn cell(&self, column: Column) -> &str {
        match column {
            Column::Min => &self.min,
            Column::Max => &self.max,
            Column::Total => &self.total,
            Column::Average => &self.average,
        }
    }
}

/// An absent minimum, which is what a ship with no games has.
fn optional_u64(value: Option<u64>, locale: Option<&str>) -> String {
    value.map(|value| separate_number(value, locale)).unwrap_or_default()
}

fn optional_i64(value: Option<i64>, locale: Option<&str>) -> String {
    value.map(|value| separate_number(value, locale)).unwrap_or_default()
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
            min: format!("{:.0}", rating.min),
            max: format!("{:.0}", rating.max),
            total: String::new(),
            average: format!("{:.0}", rating.average),
        });
    }

    rows.push(StatRow {
        label: StatLabel::Damage,
        min: optional_u64(info.min_damage(), locale),
        max: separate_number(info.max_damage(), locale),
        total: separate_number(info.total_damage(), locale),
        average: info.avg_damage().map(|value| separate_number(value as u64, locale)).unwrap_or_default(),
    });
    rows.push(StatRow {
        label: StatLabel::SpottingDamage,
        min: optional_u64(info.min_spotting_damage(), locale),
        max: separate_number(info.max_spotting_damage(), locale),
        total: separate_number(info.total_spotting_damage(), locale),
        average: info.avg_spotting_damage().map(|value| separate_number(value as u64, locale)).unwrap_or_default(),
    });
    rows.push(StatRow {
        label: StatLabel::Frags,
        min: optional_i64(info.min_frags(), locale),
        max: separate_number(info.max_frags(), locale),
        total: separate_number(info.total_frags(), locale),
        // Two decimals: a frag average under one is the common case, and
        // rounding it to a whole number would read as zero.
        average: info.avg_frags().map(|value| format!("{value:.2}")).unwrap_or_default(),
    });
    rows.push(StatRow {
        label: StatLabel::RawXp,
        min: optional_i64(info.min_xp(), locale),
        max: separate_number(info.max_xp(), locale),
        total: separate_number(info.total_xp(), locale),
        average: info.avg_xp().map(|value| separate_number(value as i64, locale)).unwrap_or_default(),
    });
    rows.push(StatRow {
        label: StatLabel::BaseXp,
        min: optional_i64(info.min_win_adjusted_xp(), locale),
        max: separate_number(info.max_win_adjusted_xp(), locale),
        total: separate_number(info.total_win_adjusted_xp(), locale),
        average: info.avg_win_adjusted_xp().map(|value| separate_number(value as i64, locale)).unwrap_or_default(),
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
            row.min,
            row.max,
            row.total,
            row.average
        ));
    }
    out
}

/// The table as CSV, its first column unlabelled like the Markdown one.
pub fn to_csv(rows: &[StatRow], column: impl Fn(Column) -> String, label: impl Fn(StatLabel) -> String) -> String {
    let headings: Vec<String> = COLUMNS.iter().map(|c| column(*c)).collect();
    let mut out = format!(",{}\n", headings.join(","));
    for row in rows {
        out.push_str(&format!("{},{},{},{},{}\n", label(row.label), row.min, row.max, row.total, row.average));
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

    fn english(label: StatLabel) -> String {
        label.english().to_string()
    }

    fn english_column(column: Column) -> String {
        column.english().to_string()
    }

    #[test]
    fn a_ships_rows_carry_its_span_and_its_average() {
        let rows = rows_for(&[game(10_000, 1, 1_000), game(30_000, 3, 3_000)]);

        let damage = rows.iter().find(|row| row.label == StatLabel::Damage).expect("damage is listed");
        assert_eq!(damage.min, "10,000");
        assert_eq!(damage.max, "30,000");
        assert_eq!(damage.total, "40,000");
        assert_eq!(damage.average, "20,000");
    }

    /// A frag average under one must not round to zero.
    #[test]
    fn a_frag_average_keeps_two_decimals() {
        let rows = rows_for(&[game(1, 0, 1), game(1, 1, 1)]);
        let frags = rows.iter().find(|row| row.label == StatLabel::Frags).expect("frags are listed");
        assert_eq!(frags.average, "0.50");
    }

    #[test]
    fn the_locale_reaches_every_cell() {
        let games = [game(1_234_567, 1, 1_000)];
        let refs: Vec<&PerGameStat> = games.iter().collect();
        let ships = per_ship_performance(&refs);

        let rows = ship_rows(&ships[0].1, None, Some("fr"));

        let damage = rows.iter().find(|row| row.label == StatLabel::Damage).expect("damage is listed");
        assert_eq!(damage.min, "1 234 567");
        assert_eq!(damage.total, "1 234 567");
    }

    #[test]
    fn a_rating_leads_the_table_and_carries_no_total() {
        let games = [game(1, 0, 1)];
        let refs: Vec<&PerGameStat> = games.iter().collect();
        let ships = per_ship_performance(&refs);
        let rating = PrStats { min: 900.0, max: 2100.0, average: 1650.0 };

        let rows = ship_rows(&ships[0].1, Some(&rating), None);

        assert_eq!(rows[0].label, StatLabel::PersonalRating);
        assert_eq!(rows[0].min, "900");
        assert_eq!(rows[0].max, "2100");
        assert_eq!(rows[0].average, "1650");
        assert!(rows[0].total.is_empty(), "a rating does not sum");
    }

    #[test]
    fn markdown_and_csv_carry_the_same_cells() {
        let rows = rows_for(&[game(10_000, 1, 1_000)]);

        let markdown = to_markdown("Yamato", &rows, english_column, english);
        assert!(markdown.starts_with("**Yamato**"));
        assert!(markdown.contains("| | Min | Max | Total | Average |"));
        assert!(markdown.contains("| Damage | 10,000 | 10,000 | 10,000 | **10,000** |"));

        let csv = to_csv(&rows, english_column, english);
        assert!(csv.starts_with(",Min,Max,Total,Average"));
        assert!(csv.contains("Damage,10,000,10,000,10,000,10,000"));
    }
}
