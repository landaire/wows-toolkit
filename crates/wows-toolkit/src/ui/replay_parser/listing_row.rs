//! The egui half of a listing row: the drawn two-line layout, and the stats
//! an in-memory parse contributes.
//!
//! Everything else -- which stats win, and how the two lines read -- is
//! shared with the GPUI port in `wows_toolkit_viewmodel::listing_row`.

pub use wows_toolkit_viewmodel::listing_row::ListedReplay;
pub(crate) use wows_toolkit_viewmodel::listing_row::ParsedStats;
pub(crate) use wows_toolkit_viewmodel::listing_row::RowFreshness;
pub(crate) use wows_toolkit_viewmodel::listing_row::RowStats;
pub(crate) use wows_toolkit_viewmodel::listing_row::file_mtime_secs;
pub(crate) use wows_toolkit_viewmodel::listing_row::hover_text;
pub(crate) use wows_toolkit_viewmodel::listing_row::identity_line;
pub(crate) use wows_toolkit_viewmodel::listing_row::listed_row_identity;
pub(crate) use wows_toolkit_viewmodel::listing_row::listed_ship_name;
pub(crate) use wows_toolkit_viewmodel::listing_row::resolve_row_stats;
pub(crate) use wows_toolkit_viewmodel::listing_row::row_freshness;
pub(crate) use wows_toolkit_viewmodel::listing_row::should_reload_summaries;
pub(crate) use wows_toolkit_viewmodel::listing_row::stats_line;

use wows_toolkit_config::index::rows::MatchOutcome;

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
            &format!(" {}", wows_toolkit_viewmodel::glyphs::USERS_THREE),
            0.0,
            TextFormat { color: sem.division, font_id: font_id.clone(), ..Default::default() },
        );
    }
    job.append("\n", 0.0, TextFormat { font_id: font_id.clone(), ..Default::default() });
    job.append(stats_text, 0.0, TextFormat { color: sem.text_dim, font_id, ..Default::default() });
    job
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

#[cfg(test)]
mod tests {
    use super::*;
    use wows_toolkit_config::ReplayGrouping;
    use wows_toolkit_config::index::rows::DivisionMate;
    use wows_toolkit_config::index::rows::RowSummary;
    use wows_toolkit_viewmodel::listing_row::RowIdentity;
    use wows_toolkit_viewmodel::listing_row::resolve_row_stats;
    use wows_toolkit_viewmodel::listing_row::stats_line;

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
        assert!(!solo.text.contains(wows_toolkit_viewmodel::glyphs::USERS_THREE));
        assert!(!has_section_colored(&solo, sem.division));

        let div = row_layout_job("id", "stats", &row_stats(MatchOutcome::Win, true), false, &visuals, test_font());
        assert!(div.text.contains(wows_toolkit_viewmodel::glyphs::USERS_THREE));
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

        // MatchOutcome::Unknown is the one branch where line 1 renders at
        // body-text weight (`visuals.text_color()`); every other outcome
        // tints line 1 with a semantic role instead. This is checked against
        // the app's own installed style, not stock egui's `Visuals::dark()`:
        // stock `text_color()` is `Color32::from_gray(140)`, a colour this
        // app never paints, so comparing against it cannot tell body weight
        // from de-emphasised weight. The app's real dark body text comes from
        // `dark_style()`'s `noninteractive.fg_stroke`, which is
        // `palette::dark::TEXT_DIM` - the actual colour `text_dim` must stay
        // clear of.
        let app_visuals = crate::ui::theme::style::dark_style().visuals;
        let unknown_job =
            row_layout_job("id", "stats", &row_stats(MatchOutcome::Unknown, false), false, &app_visuals, test_font());
        assert_eq!(identity_color(&unknown_job), app_visuals.text_color());
        assert_ne!(sem.text_dim, identity_color(&unknown_job), "line 2 must not render at body-text weight");
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
}
