//! Pure assembly of the two-line replay-listing row: which stats win when both
//! the index and an in-memory parse have an opinion, and how the two lines read
//! for each grouping mode. No `Ui` access, so it is unit-testable.

use rust_i18n::t;
use wows_toolkit_config::ReplayGrouping;
use wowsunpack::game_params::provider::GameMetadataProvider;

use crate::db::index::rows::DivisionMate;
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

/// Drops a trailing `:SS` from a `HH:MM:SS`-shaped string, leaving anything
/// else untouched. `Replay::game_time()` is the raw `dateTime` field from
/// replay metadata, whose exact shape is not guaranteed across game versions,
/// so this only acts once two-digit groups are confirmed on both sides of the
/// last two colons (a plain `HH:MM` has only one colon, so its `MM` is never
/// mistaken for seconds). No fixed-width slicing, so a shorter or
/// differently-shaped string is returned unchanged rather than panicking.
fn strip_seconds(time: &str) -> &str {
    let two_digits = |s: &str| s.len() == 2 && s.bytes().all(|b| b.is_ascii_digit());

    let Some(last_colon) = time.rfind(':') else { return time };
    let seconds = &time[last_colon + 1..];
    if !two_digits(seconds) {
        return time;
    }

    let rest = &time[..last_colon];
    let Some(prev_colon) = rest.rfind(':') else { return time };
    let minutes = &rest[prev_colon + 1..];
    if !two_digits(minutes) {
        return time;
    }

    rest
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
    /// The other players who shared the division, for the hover tooltip. Always
    /// empty on the parsed branch: a parsed report carries no mate roster.
    pub division_mates: Vec<DivisionMate>,
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
            division_mates: Vec::new(),
            known: true,
        },
        None => match summary {
            // A division id with nobody else recorded in it is not a division:
            // deriving the flag from the mate list keeps the glyph and the
            // tooltip consistent, rather than the glyph firing on a division_id
            // whose mate line would then render empty.
            Some(s) => RowStats {
                outcome: s.outcome,
                damage: s.self_damage,
                kills: s.self_kills,
                survived,
                in_division: !s.division_mates.is_empty(),
                division_mates: s.division_mates.clone(),
                known: true,
            },
            None => RowStats {
                outcome: MatchOutcome::Unknown,
                damage: None,
                kills: None,
                survived: None,
                in_division: false,
                division_mates: Vec::new(),
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

/// The timestamp half of line 2: just the time when the group header already
/// states the date, the full `dd.mm.yyyy HH:MM` otherwise. Seconds are
/// dropped in both cases so the value cannot be misread as a match duration.
fn timestamp_for(identity: &RowIdentity, grouping: ReplayGrouping) -> String {
    match grouping {
        ReplayGrouping::Date => strip_seconds(identity.time_part()).to_string(),
        ReplayGrouping::Ship | ReplayGrouping::None => strip_seconds(&identity.date_time).to_string(),
    }
}

/// Line 2 as drawn: icons instead of words, so adjacent stats never read as
/// one phrase (a word-based "0 kills sunk" was misread as "2 sunk"). Survival
/// is not shown here at all; it costs no row width in the hover tooltip
/// instead. The timestamp carries a clock glyph and drops its seconds so it
/// cannot be misread as a match duration.
pub(crate) fn stats_line(
    identity: &RowIdentity,
    stats: &RowStats,
    grouping: ReplayGrouping,
    locale: Option<&str>,
) -> String {
    let when = format!("{} {}", crate::icons::CLOCK, timestamp_for(identity, grouping));

    if !stats.known {
        return format!("{}  {}", t!("ui.replay.row_not_indexed"), when);
    }

    let mut parts: Vec<String> = Vec::new();
    if let Some(damage) = stats.damage {
        parts.push(format!("{} {}", crate::icons::CROSSHAIR_SIMPLE, separate_number(damage, locale)));
    }
    if let Some(kills) = stats.kills {
        parts.push(format!("{} {}", crate::icons::SWORD, kills));
    }
    parts.push(when);
    parts.join("  ")
}

/// The word-based equivalent of [`stats_line`], used only for the hover
/// tooltip so the icon convention stays discoverable and the translation keys
/// stay in use.
fn stats_words(stats: &RowStats, when: &str, locale: Option<&str>) -> String {
    if !stats.known {
        return format!("{}  {when}", t!("ui.replay.row_not_indexed"));
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
    parts.push(when.to_string());
    parts.join("  ")
}

/// The division member list line, formatted `[CLAN] Name` per member when the
/// member has a clan and bare `Name` otherwise. `None` when there are no
/// mates, so the tooltip never shows an empty division line.
fn division_line(stats: &RowStats) -> Option<String> {
    if stats.division_mates.is_empty() {
        return None;
    }
    let members = stats
        .division_mates
        .iter()
        .map(|m| if m.clan.is_empty() { m.player_name.clone() } else { format!("[{}] {}", m.clan, m.player_name) })
        .collect::<Vec<_>>()
        .join(", ");
    Some(t!("ui.replay.row_division", members = members).to_string())
}

/// Hover text for a row. The two drawn lines omit scenario and game mode to
/// keep the panel narrow, and line 2 now draws icons rather than words, so
/// the tooltip is where both kinds of detail survive. The division member
/// line is only present when the row has at least one division mate.
pub(crate) fn hover_text(identity: &RowIdentity, stats: &RowStats, locale: Option<&str>) -> String {
    let stats_text = stats_words(stats, &identity.date_time, locale);
    let mut lines = vec![
        identity.ship.clone(),
        identity.map.clone(),
        identity.scenario.clone(),
        identity.mode.clone(),
        identity.date_time.clone(),
    ];
    if let Some(division) = division_line(stats) {
        lines.push(division);
    }
    lines.push(stats_text);
    lines.join("\n")
}

/// The row as drawn: identity on line 1 tinted by outcome, stats on line 2 in
/// de-emphasised text, with the division glyph closing line 1. The tint
/// already encodes the outcome, so there is no separate outcome glyph.
pub(crate) fn row_layout_job(
    identity_text: &str,
    stats_text: &str,
    stats: &RowStats,
    is_selected: bool,
    visuals: &egui::Visuals,
    font_id: egui::FontId,
) -> egui::text::LayoutJob {
    use egui::TextFormat;
    use egui::text::LayoutJob;

    let sem = crate::ui::theme::semantic::semantic(visuals);
    let identity_color = if is_selected {
        sem.text_strong
    } else {
        match stats.outcome {
            MatchOutcome::Win => sem.win,
            MatchOutcome::Loss => sem.loss,
            MatchOutcome::Draw => sem.draw,
            MatchOutcome::Unknown => visuals.text_color(),
        }
    };

    let mut job = LayoutJob::default();
    job.append(
        identity_text,
        0.0,
        TextFormat { color: identity_color, font_id: font_id.clone(), ..Default::default() },
    );
    if stats.in_division {
        job.append(
            &format!(" {}", crate::icons::USERS_THREE),
            0.0,
            TextFormat { color: sem.division, font_id: font_id.clone(), ..Default::default() },
        );
    }
    job.append("\n", 0.0, TextFormat { font_id: font_id.clone(), ..Default::default() });
    job.append(stats_text, 0.0, TextFormat { color: sem.text_dim, font_id, ..Default::default() });
    job
}

/// Lift the identity fields off a replay. Requires the metadata provider for
/// every field except the timestamp.
pub(crate) fn replay_row_identity(replay: &super::Replay, metadata_provider: &GameMetadataProvider) -> RowIdentity {
    RowIdentity {
        ship: replay.vehicle_name(metadata_provider),
        map: replay.map_name(metadata_provider),
        scenario: replay.scenario(metadata_provider),
        mode: replay.game_mode(metadata_provider),
        date_time: replay.game_time().to_string(),
    }
}

/// Stats from an in-memory parse, when one exists. `None` for a replay that has
/// only been read for its metadata.
pub(crate) fn replay_parsed_stats(replay: &super::Replay) -> Option<ParsedStats> {
    let ui_report = replay.ui_report.as_ref()?;
    let self_report = ui_report.player_reports().iter().find(|report| report.relation().is_self())?;
    Some(ParsedStats {
        outcome: crate::data::replay_index::outcome_from(replay.battle_result().as_ref()),
        damage: self_report.actual_damage(),
        kills: self_report.kills(),
        in_division: self_report.division_label.is_some(),
    })
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

/// Whether a summary reload should start. False while one is in flight, and
/// false when the cached map already reflects the current index generation.
pub(crate) fn should_reload_summaries(loading: bool, cached: Option<u64>, current: u64) -> bool {
    !loading && cached != Some(current)
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
            division_mates: vec![DivisionMate { player_name: "Mate".into(), clan: "MATE".into() }],
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
    fn strip_seconds_drops_only_a_genuine_hh_mm_ss_tail() {
        assert_eq!(strip_seconds("25.07.2026 16:35:06"), "25.07.2026 16:35");
        // Already seconds-less: the lone colon pair must not be mistaken for
        // minutes:seconds and truncated further.
        assert_eq!(strip_seconds("16:35"), "16:35");
        assert_eq!(strip_seconds(""), "");
        // No space and no colons at all: nothing for the helper to find.
        assert_eq!(strip_seconds("not_a_timestamp"), "not_a_timestamp");
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
        assert!(line.contains(crate::icons::CROSSHAIR_SIMPLE), "expected the damage glyph in {line:?}");
        assert!(line.contains(&format!("{} 3", crate::icons::SWORD)), "expected the kill glyph and count in {line:?}");
        // Date grouping heads the group with the date, so the row shows the time
        // only, clock-prefixed and with the seconds dropped.
        assert!(
            line.contains(&format!("{} 14:23", crate::icons::CLOCK)),
            "expected the clock-prefixed time in {line:?}"
        );
        assert!(!line.contains("14:23:05"), "seconds must be dropped from the drawn row: {line:?}");
        assert!(!line.contains("28.07.2026"), "date is already in the group header: {line:?}");
        assert!(!line.contains(crate::icons::SKULL), "the skull glyph was removed from the drawn row: {line:?}");
    }

    #[test]
    fn hover_text_keeps_the_detail_the_drawn_rows_drop() {
        let stats = resolve_row_stats(None, Some(&summary()));
        let hover = hover_text(&identity(), &stats, Some("en-US"));
        assert!(hover.contains("Domination"), "scenario must survive in the tooltip: {hover:?}");
        assert!(hover.contains("Randoms"), "game mode must survive in the tooltip: {hover:?}");
        assert!(hover.contains("28.07.2026 14:23:05"));
        assert!(
            hover.contains(t!("ui.replay.row_kills", count = 3).as_ref()),
            "kills must read as words in the tooltip: {hover:?}"
        );
        assert!(
            hover.contains(t!("ui.replay.row_survived").as_ref()),
            "survival must read as a word in the tooltip: {hover:?}"
        );
    }

    #[test]
    fn hover_text_shows_sunk_in_words_when_the_player_died() {
        let died = RowSummary { self_survived: Some(false), ..summary() };
        let stats = resolve_row_stats(None, Some(&died));
        let hover = hover_text(&identity(), &stats, Some("en-US"));
        assert!(hover.contains("Domination"), "scenario must survive in the tooltip: {hover:?}");
        assert!(hover.contains("Randoms"), "game mode must survive in the tooltip: {hover:?}");
        assert!(
            hover.contains(t!("ui.replay.row_sunk").as_ref()),
            "death must read as a word in the tooltip: {hover:?}"
        );
        assert!(
            !hover.contains(t!("ui.replay.row_survived").as_ref()),
            "the survived and sunk words must not both appear: {hover:?}"
        );
    }

    #[test]
    fn hover_text_names_division_mates_with_clan_tags_when_present() {
        let with_mates = RowSummary {
            division_mates: vec![
                DivisionMate { player_name: "Clanned".into(), clan: "ABC".into() },
                DivisionMate { player_name: "Clanless".into(), clan: "".into() },
            ],
            ..summary()
        };
        let stats = resolve_row_stats(None, Some(&with_mates));
        let hover = hover_text(&identity(), &stats, Some("en-US"));
        assert!(hover.contains("[ABC] Clanned"), "a clanned mate must render as [CLAN] Name: {hover:?}");
        assert!(hover.contains("Clanless"), "a clanless mate must still be named: {hover:?}");
        assert!(!hover.contains("[] Clanless"), "an empty clan must not render bracketed: {hover:?}");
    }

    #[test]
    fn hover_text_has_no_division_line_when_there_are_no_mates() {
        let with_mates = summary();
        let solo = RowSummary { division_mates: Vec::new(), ..summary() };

        let with_mates_hover = hover_text(&identity(), &resolve_row_stats(None, Some(&with_mates)), Some("en-US"));
        let solo_hover = hover_text(&identity(), &resolve_row_stats(None, Some(&solo)), Some("en-US"));

        assert_eq!(
            with_mates_hover.lines().count(),
            solo_hover.lines().count() + 1,
            "a division line must be present exactly when there are mates: with_mates={with_mates_hover:?} solo={solo_hover:?}"
        );
        assert!(with_mates_hover.contains("Mate"), "the mates case must actually name the mate: {with_mates_hover:?}");
        assert!(!solo_hover.contains("Mate"), "the solo case must not carry over the mate name: {solo_hover:?}");
    }

    #[test]
    fn resolve_row_stats_treats_a_division_id_with_no_mates_as_not_in_a_division() {
        // division_id is Some, but the mate list is empty: this is the case that
        // proves in_division is derived from the mates, not from division_id.
        let orphaned = RowSummary { division_mates: Vec::new(), ..summary() };
        assert!(orphaned.division_id.is_some(), "the fixture must still carry a division id for this to be meaningful");
        let stats = resolve_row_stats(None, Some(&orphaned));
        assert!(!stats.in_division, "a division id with no recorded mates must not show the division glyph");
    }

    #[test]
    fn stats_line_shows_the_full_timestamp_without_seconds_when_ungrouped() {
        let stats = resolve_row_stats(None, Some(&summary()));
        let line = stats_line(&identity(), &stats, ReplayGrouping::None, Some("en-US"));
        assert!(
            line.contains(&format!("{} 28.07.2026 14:23", crate::icons::CLOCK)),
            "expected the clock-prefixed full timestamp with no seconds in {line:?}"
        );
        assert!(!line.contains("14:23:05"), "seconds must be dropped from the drawn row: {line:?}");
    }

    #[test]
    fn stats_line_falls_back_to_the_not_indexed_string() {
        let stats = resolve_row_stats(None, None);
        let line = stats_line(&identity(), &stats, ReplayGrouping::Date, Some("en-US"));
        assert!(line.contains(t!("ui.replay.row_not_indexed").as_ref()));
        assert!(!line.contains(','), "no stat numbers should be rendered: {line:?}");
    }

    #[test]
    fn stats_line_never_shows_the_skull_regardless_of_survival() {
        // The skull was removed from the drawn row entirely; survival now only
        // shows up in the hover tooltip. Exact equality, not a substring check,
        // since the row is nothing but the clock-prefixed time once damage and
        // kills are absent.
        for survived in [Some(false), Some(true), None] {
            let partial = RowSummary { self_damage: None, self_kills: None, self_survived: survived, ..summary() };
            let stats = resolve_row_stats(None, Some(&partial));
            let line = stats_line(&identity(), &stats, ReplayGrouping::Date, Some("en-US"));
            assert_eq!(line, format!("{} 14:23", crate::icons::CLOCK), "survived={survived:?}");
            assert!(!line.contains(crate::icons::SKULL), "survived={survived:?}: {line:?}");
        }
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

        let visuals = egui::Visuals::dark();
        let job = row_layout_job("Yamato - Ocean", &line, &stats, false, &visuals, test_font());
        assert_eq!(identity_color(&job), visuals.text_color(), "an Unknown outcome must draw in the plain text colour");
        assert!(!job.text.contains(crate::icons::TROPHY), "no result means no outcome glyph: {:?}", job.text);
    }

    fn test_font() -> egui::FontId {
        // Deliberately not a style default, so a section that drops the passed
        // `font_id` is visible in the assertions rather than coincidentally equal.
        egui::FontId::proportional(17.5)
    }

    fn row_stats(outcome: MatchOutcome, in_division: bool) -> RowStats {
        RowStats {
            outcome,
            damage: Some(1000),
            kills: Some(1),
            survived: Some(true),
            in_division,
            division_mates: Vec::new(),
            known: true,
        }
    }

    /// Line 1's tint. `LayoutJob::append` coalesces adjacent runs that share a
    /// `TextFormat`, so section indices past the first are not stable; only
    /// section 0 is guaranteed to be the head of the identity text.
    fn identity_color(job: &egui::text::LayoutJob) -> egui::Color32 {
        job.sections[0].format.color
    }

    fn has_section_colored(job: &egui::text::LayoutJob, color: egui::Color32) -> bool {
        job.sections.iter().any(|s| s.format.color == color)
    }

    #[test]
    fn a_win_tints_line_one_with_the_win_colour() {
        let visuals = egui::Visuals::dark();
        let sem = crate::ui::theme::semantic::semantic(&visuals);
        let job = row_layout_job(
            "Yamato - Ocean",
            "1,000  14:23:05",
            &row_stats(MatchOutcome::Win, false),
            false,
            &visuals,
            test_font(),
        );
        assert_eq!(identity_color(&job), sem.win);
        assert!(job.text.starts_with("Yamato - Ocean"));
    }

    #[test]
    fn a_loss_tints_line_one_with_the_loss_colour() {
        let visuals = egui::Visuals::dark();
        let sem = crate::ui::theme::semantic::semantic(&visuals);
        let job = row_layout_job(
            "Yamato - Ocean",
            "1,000  14:23:05",
            &row_stats(MatchOutcome::Loss, false),
            false,
            &visuals,
            test_font(),
        );
        assert_eq!(identity_color(&job), sem.loss);
        assert_ne!(sem.loss, sem.win, "the two outcomes must not resolve to the same colour");
    }

    #[test]
    fn a_draw_tints_line_one_with_the_draw_colour() {
        let visuals = egui::Visuals::dark();
        let sem = crate::ui::theme::semantic::semantic(&visuals);
        let job = row_layout_job(
            "Yamato - Ocean",
            "1,000  14:23:05",
            &row_stats(MatchOutcome::Draw, false),
            false,
            &visuals,
            test_font(),
        );
        assert_eq!(identity_color(&job), sem.draw);
        assert_ne!(sem.draw, sem.win, "swapping the draw colour for the win colour must not pass this test");
    }

    #[test]
    fn selection_overrides_the_outcome_tint() {
        let visuals = egui::Visuals::dark();
        let sem = crate::ui::theme::semantic::semantic(&visuals);
        let stats = row_stats(MatchOutcome::Loss, false);
        let job = row_layout_job("Yamato - Ocean", "1,000  14:23:05", &stats, true, &visuals, test_font());
        assert_eq!(identity_color(&job), sem.text_strong, "a selected row reads against the selection fill");
        assert!(!has_section_colored(&job, sem.loss), "the outcome tint must be gone everywhere it applied");
    }

    #[test]
    fn no_outcome_glyph_is_ever_emitted() {
        // The trophy used to fire for Win, Loss and Draw alike, so it only
        // encoded "outcome is known" - which the tint already says - and read
        // wrong next to a loss. Line 1 carries no outcome glyph at all now.
        let visuals = egui::Visuals::dark();
        for outcome in [MatchOutcome::Win, MatchOutcome::Loss, MatchOutcome::Draw, MatchOutcome::Unknown] {
            let job = row_layout_job("id", "stats", &row_stats(outcome, false), false, &visuals, test_font());
            assert!(!job.text.contains(crate::icons::TROPHY), "{outcome:?} must not carry an outcome glyph");
        }
    }

    #[test]
    fn no_skull_glyph_is_ever_emitted_in_the_drawn_row() {
        // The skull used to render when the player died; it is gone from the
        // drawn row entirely, for every survival state, both from the raw
        // stats line and from the assembled layout job's text.
        let visuals = egui::Visuals::dark();
        for survived in [Some(false), Some(true), None] {
            let stats = RowStats { survived, ..row_stats(MatchOutcome::Win, false) };
            let line = stats_line(&identity(), &stats, ReplayGrouping::Date, Some("en-US"));
            assert!(!line.contains(crate::icons::SKULL), "survived={survived:?}: {line:?}");

            let job = row_layout_job("Yamato - Ocean", &line, &stats, false, &visuals, test_font());
            assert!(!job.text.contains(crate::icons::SKULL), "survived={survived:?}: {:?}", job.text);
        }
    }

    #[test]
    fn the_division_glyph_appears_only_when_in_a_division() {
        let visuals = egui::Visuals::dark();
        let sem = crate::ui::theme::semantic::semantic(&visuals);
        let solo = row_layout_job("id", "stats", &row_stats(MatchOutcome::Win, false), false, &visuals, test_font());
        assert!(!solo.text.contains(crate::icons::USERS_THREE));
        assert!(!has_section_colored(&solo, sem.division));

        let div = row_layout_job("id", "stats", &row_stats(MatchOutcome::Win, true), false, &visuals, test_font());
        assert!(div.text.contains(crate::icons::USERS_THREE));
        assert!(has_section_colored(&div, sem.division), "the division glyph keeps its own colour, not the outcome's");
        assert_ne!(sem.division, sem.win, "otherwise the colour assertion above proves nothing");
    }

    #[test]
    fn the_stats_line_is_de_emphasised() {
        let visuals = egui::Visuals::dark();
        let sem = crate::ui::theme::semantic::semantic(&visuals);
        let job = row_layout_job("id", "stats", &row_stats(MatchOutcome::Win, true), false, &visuals, test_font());
        let last = job.sections.last().unwrap();
        assert_eq!(last.format.color, sem.text_dim);
        assert!(job.text.ends_with("\nstats"), "line 2 is the last section: {:?}", job.text);
        assert_ne!(sem.text_dim, sem.win, "line 2 must not inherit line 1's tint");
    }

    #[test]
    fn every_section_carries_the_passed_font_id() {
        let visuals = egui::Visuals::dark();
        let font = test_font();
        // Both glyphs present and selection on, so every branch that appends a
        // section is exercised in one job.
        let job = row_layout_job("id", "stats", &row_stats(MatchOutcome::Win, true), true, &visuals, font.clone());
        assert!(job.sections.len() >= 4, "expected at least identity, division, newline and stats runs");
        for (i, section) in job.sections.iter().enumerate() {
            assert_eq!(
                section.format.font_id, font,
                "section {i} dropped the caller's font and fell back to a default"
            );
        }
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
    fn should_reload_summaries_starts_on_first_load() {
        assert!(should_reload_summaries(false, None, 5));
    }

    #[test]
    fn should_reload_summaries_skips_when_cached_matches_current() {
        assert!(!should_reload_summaries(false, Some(5), 5));
    }

    #[test]
    fn should_reload_summaries_reloads_when_cached_is_behind() {
        assert!(should_reload_summaries(false, Some(4), 5));
    }

    #[test]
    fn should_reload_summaries_blocks_while_loading() {
        assert!(!should_reload_summaries(true, None, 5), "in-flight load must block the never-loaded case too");
        assert!(!should_reload_summaries(true, Some(4), 5));
        assert!(!should_reload_summaries(true, Some(5), 5));
    }

    #[test]
    fn file_mtime_secs_returns_whole_unix_seconds() {
        // The exact value matters, not just that one is produced. `replay_index`
        // records the mtime as whole seconds since the Unix epoch, and
        // `row_freshness` compares the two for equality: a milliseconds or
        // otherwise-shifted implementation here would mark every indexed replay
        // Stale forever and re-parse the whole library on every launch.
        let dir = std::env::temp_dir().join(format!("wt_listing_row_mtime_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.wowsreplay");

        let file = std::fs::File::create(&path).unwrap();
        // Half a second past the mark, so a rounding implementation is caught too.
        let known = std::time::UNIX_EPOCH + std::time::Duration::from_millis(1_700_000_000_500);
        file.set_modified(known).unwrap();
        drop(file);

        assert_eq!(file_mtime_secs(&path), Some(1_700_000_000));
        assert_eq!(file_mtime_secs(&dir.join("absent.wowsreplay")), None);

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }
}
