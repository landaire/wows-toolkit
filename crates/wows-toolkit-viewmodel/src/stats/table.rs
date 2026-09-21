//! One ship's session statistics as a table, and the text a copy produces.
//!
//! Both front ends show the same rows and copy the same text, so a figure
//! cannot read one way on screen and another in the clipboard.

use super::PerformanceInfo;
use crate::personal_rating::PersonalRatingResult;

/// One statistic across a ship's games.
///
/// Every cell is already formatted: the columns mix integers, thousands-
/// separated counts and two-decimal averages, and which is which belongs with
/// the statistic rather than with each front end's renderer. An empty cell is
/// a figure that has no meaning here (a rating does not sum), not a missing
/// one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatRow {
    pub label: &'static str,
    pub min: String,
    pub max: String,
    pub total: String,
    pub average: String,
}

/// The column headings, in order.
pub const COLUMNS: [&str; 4] = ["Min", "Max", "Total", "Average"];

/// A whole number with thousands separators, which is how every count in this
/// table reads.
fn grouped(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

fn grouped_signed(value: i64) -> String {
    if value < 0 { format!("-{}", grouped(value.unsigned_abs())) } else { grouped(value as u64) }
}

/// An absent minimum, which is what a ship with no games has.
fn optional_grouped(value: Option<u64>) -> String {
    value.map(grouped).unwrap_or_default()
}

fn optional_grouped_signed(value: Option<i64>) -> String {
    value.map(grouped_signed).unwrap_or_default()
}

/// The rows for one ship, in the order the egui table lists them.
///
/// `rating` is this ship's personal rating when one could be computed; it
/// leads the table because it is the figure the rest are read against, and
/// carries no total because ratings do not sum.
pub fn ship_rows(info: &PerformanceInfo, rating: Option<&PersonalRatingResult>) -> Vec<StatRow> {
    let mut rows = Vec::with_capacity(6);

    if let Some(rating) = rating {
        rows.push(StatRow {
            label: "Personal rating",
            min: String::new(),
            max: String::new(),
            total: String::new(),
            average: format!("{:.0}", rating.pr),
        });
    }

    rows.push(StatRow {
        label: "Damage",
        min: optional_grouped(info.min_damage()),
        max: grouped(info.max_damage()),
        total: grouped(info.total_damage()),
        average: info.avg_damage().map(|value| grouped(value as u64)).unwrap_or_default(),
    });
    rows.push(StatRow {
        label: "Spotting damage",
        min: optional_grouped(info.min_spotting_damage()),
        max: grouped(info.max_spotting_damage()),
        total: grouped(info.total_spotting_damage()),
        average: info.avg_spotting_damage().map(|value| grouped(value as u64)).unwrap_or_default(),
    });
    rows.push(StatRow {
        label: "Frags",
        min: optional_grouped_signed(info.min_frags()),
        max: grouped_signed(info.max_frags()),
        total: grouped_signed(info.total_frags()),
        // Two decimals: a frag average under one is the common case, and
        // rounding it to a whole number would read as zero.
        average: info.avg_frags().map(|value| format!("{value:.2}")).unwrap_or_default(),
    });
    rows.push(StatRow {
        label: "Raw XP",
        min: optional_grouped_signed(info.min_xp()),
        max: grouped_signed(info.max_xp()),
        total: grouped_signed(info.total_xp()),
        average: info.avg_xp().map(|value| grouped_signed(value as i64)).unwrap_or_default(),
    });
    rows.push(StatRow {
        label: "Base XP",
        min: optional_grouped_signed(info.min_win_adjusted_xp()),
        max: grouped_signed(info.max_win_adjusted_xp()),
        total: grouped_signed(info.total_win_adjusted_xp()),
        average: info.avg_win_adjusted_xp().map(|value| grouped_signed(value as i64)).unwrap_or_default(),
    });

    rows
}

/// The table as a Markdown block, headed by `title`.
pub fn to_markdown(title: &str, rows: &[StatRow]) -> String {
    let mut out = format!("**{title}**\n\n| | {} |\n", COLUMNS.join(" | "));
    out.push_str("|---|---|---|---|---|\n");
    for row in rows {
        out.push_str(&format!("| {} | {} | {} | {} | {} |\n", row.label, row.min, row.max, row.total, row.average));
    }
    out
}

/// The table as CSV, its first column unlabelled like the Markdown one.
pub fn to_csv(rows: &[StatRow]) -> String {
    let mut out = format!(",{}\n", COLUMNS.join(","));
    for row in rows {
        out.push_str(&format!("{},{},{},{},{}\n", row.label, row.min, row.max, row.total, row.average));
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
        ship_rows(&ships[0].1, None)
    }

    #[test]
    fn a_thousands_separator_lands_every_three_digits() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1_000), "1,000");
        assert_eq!(grouped(1_234_567), "1,234,567");
        assert_eq!(grouped_signed(-1_234), "-1,234");
    }

    #[test]
    fn a_ships_rows_carry_its_span_and_its_average() {
        let rows = rows_for(&[game(10_000, 1, 1_000), game(30_000, 3, 3_000)]);

        let damage = rows.iter().find(|row| row.label == "Damage").expect("damage is listed");
        assert_eq!(damage.min, "10,000");
        assert_eq!(damage.max, "30,000");
        assert_eq!(damage.total, "40,000");
        assert_eq!(damage.average, "20,000");
    }

    /// A frag average under one must not round to zero.
    #[test]
    fn a_frag_average_keeps_two_decimals() {
        let rows = rows_for(&[game(1, 0, 1), game(1, 1, 1)]);
        let frags = rows.iter().find(|row| row.label == "Frags").expect("frags are listed");
        assert_eq!(frags.average, "0.50");
    }

    #[test]
    fn a_rating_leads_the_table_and_carries_no_total() {
        let games = [game(1, 0, 1)];
        let refs: Vec<&PerGameStat> = games.iter().collect();
        let ships = per_ship_performance(&refs);
        let rating = PersonalRatingResult::new(1650.0);

        let rows = ship_rows(&ships[0].1, Some(&rating));

        assert_eq!(rows[0].label, "Personal rating");
        assert_eq!(rows[0].average, "1650");
        assert!(rows[0].total.is_empty(), "a rating does not sum");
    }

    #[test]
    fn markdown_and_csv_carry_the_same_cells() {
        let rows = rows_for(&[game(10_000, 1, 1_000)]);

        let markdown = to_markdown("Yamato", &rows);
        assert!(markdown.starts_with("**Yamato**"));
        assert!(markdown.contains("| Damage | 10,000 | 10,000 | 10,000 | 10,000 |"));

        let csv = to_csv(&rows);
        assert!(csv.starts_with(",Min,Max,Total,Average"));
        assert!(csv.contains("Damage,10,000,10,000,10,000,10,000"));
    }
}
