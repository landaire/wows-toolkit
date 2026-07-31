//! Pure assembly of the two-line replay-listing row: which stats win when both
//! the index and an in-memory parse have an opinion, and how the two lines read
//! for each grouping mode. No `Ui` access, so it is unit-testable.
//!
//! The functions here are consumed by the listing's label construction in the
//! task that replaces it; until then they have no non-test caller.
#![allow(dead_code)]

use rust_i18n::t;
use wows_toolkit_config::ReplayGrouping;

use crate::db::index::rows::MatchOutcome;
use crate::db::index::rows::RowSummary;
use crate::util::separate_number;

/// Identity fields lifted off a `Replay` before any layout work.
pub(crate) struct RowIdentity {
    pub ship: String,
    pub map: String,
    pub scenario: String,
    pub mode: String,
    /// Raw `dd.mm.yyyy HH:MM:SS` from the replay meta.
    pub date_time: String,
}

impl RowIdentity {
    /// The `HH:MM:SS` half of `date_time`, or the whole string when it does not
    /// split (a meta field we do not control).
    fn time_part(&self) -> &str {
        self.date_time.split(' ').nth(1).unwrap_or(&self.date_time)
    }
}

/// Stats read off a fully parsed replay held in memory.
pub(crate) struct ParsedStats {
    pub outcome: MatchOutcome,
    pub damage: Option<u64>,
    pub kills: Option<i64>,
    pub in_division: bool,
}

/// What the row draws once precedence has been applied.
pub(crate) struct RowStats {
    pub outcome: MatchOutcome,
    pub damage: Option<u64>,
    pub kills: Option<i64>,
    pub survived: Option<bool>,
    pub in_division: bool,
    /// False only when neither an index summary nor a parsed report exists.
    pub known: bool,
}

/// A parsed report in memory is fresher than the index for everything it
/// carries, so it wins. Survival is the exception: `PlayerReport` records
/// `time_lived_secs` and no survival flag, so that field always comes from the
/// index regardless of what is parsed.
pub(crate) fn resolve_row_stats(parsed: Option<ParsedStats>, summary: Option<&RowSummary>) -> RowStats {
    let survived = summary.and_then(|s| s.self_survived);
    match parsed {
        Some(parsed) => RowStats {
            outcome: parsed.outcome,
            damage: parsed.damage,
            kills: parsed.kills,
            survived,
            in_division: parsed.in_division,
            known: true,
        },
        None => match summary {
            Some(s) => RowStats {
                outcome: s.outcome,
                damage: s.self_damage,
                kills: s.self_kills,
                survived,
                in_division: s.division_id.is_some(),
                known: true,
            },
            None => RowStats {
                outcome: MatchOutcome::Unknown,
                damage: None,
                kills: None,
                survived: None,
                in_division: false,
                known: false,
            },
        },
    }
}

/// Line 1: what match this is. Each grouping omits the field its group header
/// already states.
pub(crate) fn identity_line(identity: &RowIdentity, grouping: ReplayGrouping) -> String {
    match grouping {
        ReplayGrouping::Ship => identity.map.clone(),
        ReplayGrouping::Date | ReplayGrouping::None => format!("{} - {}", identity.ship, identity.map),
    }
}

/// Line 2: the stats, then the timestamp. Absent stats are omitted rather than
/// rendered as zero.
pub(crate) fn stats_line(
    identity: &RowIdentity,
    stats: &RowStats,
    grouping: ReplayGrouping,
    locale: Option<&str>,
) -> String {
    let when = match grouping {
        ReplayGrouping::Date => identity.time_part().to_string(),
        ReplayGrouping::Ship | ReplayGrouping::None => identity.date_time.clone(),
    };

    if !stats.known {
        return format!("{}  {}", t!("ui.replay.row_not_indexed"), when);
    }

    let mut parts: Vec<String> = Vec::new();
    if let Some(damage) = stats.damage {
        parts.push(separate_number(damage, locale));
    }
    if let Some(kills) = stats.kills {
        parts.push(t!("ui.replay.row_kills", count = kills).to_string());
    }
    match stats.survived {
        Some(true) => parts.push(t!("ui.replay.row_survived").to_string()),
        Some(false) => parts.push(t!("ui.replay.row_sunk").to_string()),
        None => {}
    }
    parts.push(when);
    parts.join("  ")
}

/// Hover text for a row. The two drawn lines omit scenario and game mode to
/// keep the panel narrow, so the tooltip is where that detail survives.
pub(crate) fn hover_text(identity: &RowIdentity, stats_text: &str) -> String {
    format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        identity.ship, identity.map, identity.scenario, identity.mode, identity.date_time, stats_text
    )
}

/// Whether a listed replay's index row still describes the file on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RowFreshness {
    Fresh,
    /// No index row for this path at all.
    Missing,
    /// The file changed since it was indexed, or one of the two mtimes is
    /// unavailable. Treated as stale rather than fresh so an unknown never
    /// masquerades as up to date.
    Stale,
}

pub(crate) fn row_freshness(summary: Option<&RowSummary>, on_disk_mtime: Option<i64>) -> RowFreshness {
    let Some(summary) = summary else {
        return RowFreshness::Missing;
    };
    match (summary.file_mtime, on_disk_mtime) {
        (Some(indexed), Some(on_disk)) if indexed == on_disk => RowFreshness::Fresh,
        _ => RowFreshness::Stale,
    }
}

/// Unix-seconds modification time, matching how the index mapper records it.
pub(crate) fn file_mtime_secs(path: &std::path::Path) -> Option<i64> {
    std::fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> RowIdentity {
        RowIdentity {
            ship: "Yamato".into(),
            map: "Ocean".into(),
            scenario: "Domination".into(),
            mode: "Randoms".into(),
            date_time: "28.07.2026 14:23:05".into(),
        }
    }

    fn summary() -> RowSummary {
        RowSummary {
            outcome: MatchOutcome::Win,
            self_damage: Some(114_230),
            self_kills: Some(3),
            self_survived: Some(true),
            self_pr: Some(1500.0),
            division_id: Some(4),
            results_available: true,
            file_mtime: Some(42),
        }
    }

    #[test]
    fn summary_alone_supplies_every_stat() {
        let stats = resolve_row_stats(None, Some(&summary()));
        assert!(stats.known);
        assert_eq!(stats.outcome, MatchOutcome::Win);
        assert_eq!(stats.damage, Some(114_230));
        assert_eq!(stats.kills, Some(3));
        assert_eq!(stats.survived, Some(true));
        assert!(stats.in_division);
    }

    #[test]
    fn parsed_report_overrides_the_summary_except_for_survival() {
        // A parsed `PlayerReport` has no survival boolean at all, only
        // `time_lived_secs`, so survival must keep coming from the index.
        let parsed =
            ParsedStats { outcome: MatchOutcome::Loss, damage: Some(200_000), kills: Some(5), in_division: false };
        let stats = resolve_row_stats(Some(parsed), Some(&summary()));
        assert_eq!(stats.outcome, MatchOutcome::Loss);
        assert_eq!(stats.damage, Some(200_000));
        assert_eq!(stats.kills, Some(5));
        assert!(!stats.in_division);
        assert_eq!(stats.survived, Some(true), "survival still comes from the index");
    }

    #[test]
    fn parsed_report_alone_leaves_survival_unknown() {
        let parsed = ParsedStats { outcome: MatchOutcome::Win, damage: Some(1), kills: Some(0), in_division: true };
        let stats = resolve_row_stats(Some(parsed), None);
        assert!(stats.known);
        assert_eq!(stats.survived, None);
        assert!(stats.in_division);
    }

    #[test]
    fn neither_source_means_unknown() {
        let stats = resolve_row_stats(None, None);
        assert!(!stats.known);
        assert_eq!(stats.outcome, MatchOutcome::Unknown);
        assert_eq!(stats.damage, None);
        assert_eq!(stats.kills, None);
        assert_eq!(stats.survived, None);
        assert!(!stats.in_division);
    }

    #[test]
    fn identity_line_drops_the_field_its_group_already_states() {
        let id = identity();
        assert_eq!(identity_line(&id, ReplayGrouping::None), "Yamato - Ocean");
        // Date groups are headed by the date, so the row leads with the ship.
        assert_eq!(identity_line(&id, ReplayGrouping::Date), "Yamato - Ocean");
        // Ship groups are headed by the ship, so it would be redundant here.
        assert_eq!(identity_line(&id, ReplayGrouping::Ship), "Ocean");
    }

    #[test]
    fn stats_line_shows_numbers_with_thousands_separators() {
        let stats = resolve_row_stats(None, Some(&summary()));
        let line = stats_line(&identity(), &stats, ReplayGrouping::Date, Some("en-US"));
        assert!(line.contains("114,230"), "expected separated damage in {line:?}");
        assert!(line.contains(t!("ui.replay.row_kills", count = 3).as_ref()), "expected the kill count in {line:?}");
        // Date grouping heads the group with the date, so the row shows the time only.
        assert!(line.contains("14:23:05"), "expected the time in {line:?}");
        assert!(!line.contains("28.07.2026"), "date is already in the group header: {line:?}");
    }

    #[test]
    fn hover_text_keeps_the_detail_the_drawn_rows_drop() {
        let stats = resolve_row_stats(None, Some(&summary()));
        let stats_text = stats_line(&identity(), &stats, ReplayGrouping::Date, Some("en-US"));
        let hover = hover_text(&identity(), &stats_text);
        assert!(hover.contains("Domination"), "scenario must survive in the tooltip: {hover:?}");
        assert!(hover.contains("Randoms"), "game mode must survive in the tooltip: {hover:?}");
        assert!(hover.contains("28.07.2026 14:23:05"));
    }

    #[test]
    fn stats_line_shows_the_full_timestamp_when_ungrouped() {
        let stats = resolve_row_stats(None, Some(&summary()));
        let line = stats_line(&identity(), &stats, ReplayGrouping::None, Some("en-US"));
        assert!(line.contains("28.07.2026 14:23:05"), "expected the full timestamp in {line:?}");
    }

    #[test]
    fn stats_line_falls_back_to_the_not_indexed_string() {
        let stats = resolve_row_stats(None, None);
        let line = stats_line(&identity(), &stats, ReplayGrouping::Date, Some("en-US"));
        assert!(line.contains(t!("ui.replay.row_not_indexed").as_ref()));
        assert!(!line.contains(','), "no stat numbers should be rendered: {line:?}");
    }

    #[test]
    fn stats_line_omits_absent_stats_rather_than_defaulting_them() {
        let partial = RowSummary { self_damage: None, self_kills: None, ..summary() };
        let stats = resolve_row_stats(None, Some(&partial));
        let line = stats_line(&identity(), &stats, ReplayGrouping::Date, Some("en-US"));
        // Exact equality, not a substring check: the timestamp itself contains
        // digits, so "does not render a zero" cannot be tested by searching.
        assert_eq!(line, format!("{}  14:23:05", t!("ui.replay.row_survived")));
    }

    #[test]
    fn absent_results_render_as_an_untinted_unknown_outcome() {
        // A player who left before results were written: the index already
        // stores Unknown, so nothing extra is needed to keep the row untinted.
        let left_early =
            RowSummary { outcome: MatchOutcome::Unknown, results_available: false, self_survived: None, ..summary() };
        let stats = resolve_row_stats(None, Some(&left_early));
        assert!(stats.known);
        assert_eq!(stats.outcome, MatchOutcome::Unknown);
        let line = stats_line(&identity(), &stats, ReplayGrouping::Date, Some("en-US"));
        assert!(line.contains("114,230"), "stats that do exist still render: {line:?}");
    }

    #[test]
    fn no_summary_means_the_file_was_never_indexed() {
        assert!(matches!(row_freshness(None, Some(42)), RowFreshness::Missing));
        assert!(matches!(row_freshness(None, None), RowFreshness::Missing));
    }

    #[test]
    fn an_equal_mtime_is_fresh() {
        assert!(matches!(row_freshness(Some(&summary()), Some(42)), RowFreshness::Fresh));
    }

    #[test]
    fn a_changed_mtime_is_stale() {
        // The game appends battle results after a match, which is exactly this.
        assert!(matches!(row_freshness(Some(&summary()), Some(99)), RowFreshness::Stale));
    }

    #[test]
    fn an_unreadable_or_unrecorded_mtime_is_stale_not_fresh() {
        assert!(matches!(row_freshness(Some(&summary()), None), RowFreshness::Stale));
        let no_mtime = RowSummary { file_mtime: None, ..summary() };
        assert!(matches!(row_freshness(Some(&no_mtime), Some(42)), RowFreshness::Stale));
        let neither_mtime = RowSummary { file_mtime: None, ..summary() };
        assert!(
            matches!(row_freshness(Some(&neither_mtime), None), RowFreshness::Stale),
            "None == None is true, so equality-based implementations would wrongly report Fresh here"
        );
    }

    #[test]
    fn file_mtime_secs_reads_a_real_file_and_returns_none_for_a_missing_one() {
        let dir = std::env::temp_dir().join("wt_listing_row_mtime_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.wowsreplay");
        std::fs::write(&path, b"x").unwrap();
        assert!(file_mtime_secs(&path).is_some());
        assert_eq!(file_mtime_secs(&dir.join("absent.wowsreplay")), None);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }
}
