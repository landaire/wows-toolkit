//! Small widgets repeated across tabs, kept in one place so the rendering
//! rule for each lives once instead of drifting per call site.

use egui::Color32;
use egui::Response;
use egui::RichText;
use egui::Ui;
use egui::Visuals;
use itertools::Itertools;
use rust_i18n::t;
use wows_replay_insights::match_contribution::MatchContributionResult;
use wows_toolkit_viewmodel::rating_breakdown::RatingBreakdown;
use wows_toolkit_viewmodel::rating_breakdown::RatingLine;
use wows_toolkit_viewmodel::rating_breakdown::basis_badge;
use wows_toolkit_viewmodel::rating_breakdown::basis_explanation;
use wows_toolkit_viewmodel::twitch::SniperCandidate;

use crate::util::personal_rating::PersonalRatingCategory;
use crate::util::personal_rating::PersonalRatingCategorySwatch;
use crate::util::personal_rating::PersonalRatingResult;

/// The personal-rating chip. Single place that knows how a rating is drawn;
/// callers never touch `RatingSwatch` directly. `strong` matches the label's
/// existing `.strong()` sites; the returned `Response` still takes
/// `.on_hover_text(..)` at the call site.
pub fn pr_chip(ui: &mut Ui, category: PersonalRatingCategory, value: &str, strong: bool) -> Response {
    let swatch = category.swatch(ui.visuals());
    let mut text = RichText::new(value).color(swatch.text).background_color(swatch.tint);
    if strong {
        text = text.strong();
    }
    ui.label(text)
}

/// What hovering a rating chip shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RatingHover {
    /// The parts the rating adds up from, with their inputs.
    Breakdown,
    /// Only the band name: the inputs include figures a stats-hidden row
    /// must not reveal.
    BandOnly,
}

/// A PR chip whose hover draws the rating's breakdown. A rating read back
/// from storage kept no terms, so its hover names the band instead.
pub fn personal_rating_chip(ui: &mut Ui, result: &PersonalRatingResult, value: &str, hover: RatingHover) -> Response {
    let response = pr_chip(ui, result.category, value, false);
    if hover == RatingHover::BandOnly || result.breakdown.is_none() {
        return response.on_hover_text(result.category.name());
    }

    // Built inside the tooltip so rows that are not hovered format nothing.
    egui::Tooltip::for_enabled(&response).show(|ui| {
        if let Some(breakdown) = RatingBreakdown::personal_rating(result, Some(&wows_toolkit_viewmodel::locale())) {
            rating_breakdown(ui, &breakdown);
        }
    });

    response
}

/// An MC chip whose hover draws the rating's breakdown.
pub fn match_contribution_chip(
    ui: &mut Ui,
    result: &MatchContributionResult,
    value: &str,
    hover: RatingHover,
) -> Response {
    let response = pr_chip(ui, result.category, value, false);
    if hover == RatingHover::BandOnly {
        return response.on_hover_text(result.category.name());
    }

    egui::Tooltip::for_enabled(&response).show(|ui| {
        rating_breakdown(ui, &RatingBreakdown::match_contribution(result, Some(&wows_toolkit_viewmodel::locale())));
    });

    response
}

/// The parts a rating adds up from, largest first: each part's points, a bar
/// on one scale shared by every part with a tick at what average play earns
/// there, the difference from that average, the part's basis and inputs, and
/// a legend for the bases shown.
pub fn rating_breakdown(ui: &mut Ui, breakdown: &RatingBreakdown) {
    const POINTS_WIDTH: f32 = 40.0;

    let swatch = breakdown.category.swatch(ui.visuals());
    let scale = breakdown.scale();

    ui.label(RichText::new(format!("{} {:.0}", breakdown.kind.title(), breakdown.total)).strong().color(swatch.text));

    let font = egui::TextStyle::Body.resolve(ui.style());
    let label_width = breakdown
        .lines
        .iter()
        .map(|line| ui.painter().layout_no_wrap(line.label.clone(), font.clone(), Color32::PLACEHOLDER).size().x)
        .fold(0.0, f32::max);
    let row_height = ui.spacing().interact_size.y;
    let item_spacing = ui.spacing().item_spacing.x;

    for line in &breakdown.lines {
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(label_width, row_height),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_min_width(label_width);
                    ui.label(&line.label);
                },
            );
            ui.allocate_ui_with_layout(
                egui::vec2(POINTS_WIDTH, row_height),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    ui.set_min_width(POINTS_WIDTH);
                    ui.label(RichText::new(format!("{:.0}", line.points)).strong());
                },
            );
            rating_bar(ui, line, scale, swatch.text);
            if let Some(delta) = line.versus_average() {
                ui.label(RichText::new(format!("{delta:+.0} {}", t!("ui.rating.versus_average"))).weak());
            }
            ui.label(RichText::new(basis_badge(line.basis)).small().weak());
        });
        ui.horizontal(|ui| {
            ui.add_space(label_width + POINTS_WIDTH + item_spacing * 2.0);
            ui.label(RichText::new(&line.detail).small().weak());
        });
    }

    for basis in breakdown.bases() {
        ui.label(RichText::new(basis_explanation(basis)).small().weak());
    }
}

fn rating_bar(ui: &mut Ui, line: &RatingLine, scale: f64, fill: Color32) {
    const TRACK_WIDTH: f32 = 140.0;
    const TRACK_HEIGHT: f32 = 8.0;

    let (rect, _) = ui.allocate_exact_size(egui::vec2(TRACK_WIDTH, ui.spacing().interact_size.y), egui::Sense::hover());
    let track = egui::Rect::from_center_size(rect.center(), egui::vec2(TRACK_WIDTH, TRACK_HEIGHT));
    // A breakdown whose every part is zero has no scale; it draws empty tracks.
    let fraction = |value: f64| if scale > 0.0 { (value / scale).clamp(0.0, 1.0) as f32 } else { 0.0 };

    let painter = ui.painter();
    painter.rect_filled(track, 2.0, ui.visuals().extreme_bg_color);
    let filled =
        egui::Rect::from_min_size(track.min, egui::vec2(track.width() * fraction(line.points), track.height()));
    painter.rect_filled(filled, 2.0, fill);

    if let Some(average) = line.average {
        let x = track.left() + track.width() * fraction(average);
        painter.vline(x, track.expand(3.0).y_range(), egui::Stroke::new(2.0, ui.visuals().text_color()));
    }
}

/// Legible colour for a small per-entity identity marker (collab peer dot,
/// ship/trajectory colour dot) drawn over the current panel background. The
/// repair (`readable_on`) lives here once; callers still choose how to draw
/// the marker (painted circle vs. inline glyph).
pub fn identity_dot_color(visuals: &Visuals, r: u8, g: u8, b: u8) -> Color32 {
    crate::ui::theme::contrast::readable_on(Color32::from_rgb(r, g, b), visuals.panel_fill)
}

/// Paints a small filled circle identity dot at the cursor and advances past
/// it, matching the collab peer-list dot layout.
pub fn identity_dot(ui: &mut Ui, color: Color32) -> Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, color);
    response
}

/// The possible-stream-sniper chip. Renders the Twitch glyph as a click-to-copy
/// control and returns the login the user picked. Shared so the replay
/// inspector and the player tracker cannot drift on hover text or copy
/// behaviour. Candidates are sorted because the caller's `HashMap` has no
/// stable iteration order.
pub fn twitch_chip(ui: &mut Ui, candidates: &[SniperCandidate]) -> Option<String> {
    let first = candidates.first()?;

    let hover = candidates
        .iter()
        .map(|candidate| {
            let minutes = candidate.minutes.iter().join(", ");
            format!(
                "{}\n{}",
                t!("ui.twitch.possible_name", name = candidate.login),
                t!("ui.twitch.seen_minutes", minutes = minutes)
            )
        })
        .join("\n\n");

    let response = ui
        .add(egui::Label::new(crate::icons::TWITCH_LOGO).sense(egui::Sense::click()))
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(format!("{hover}\n\n{}", t!("ui.twitch.click_to_copy")));

    if candidates.len() == 1 {
        return response.clicked().then(|| first.login.clone());
    }

    let mut picked = None;
    egui::Popup::from_toggle_button_response(&response).close_behavior(egui::PopupCloseBehavior::CloseOnClick).show(
        |ui| {
            for candidate in candidates {
                if ui.button(&candidate.login).clicked() {
                    picked = Some(candidate.login.clone());
                }
            }
        },
    );

    picked
}

#[cfg(test)]
mod tests {
    use wows_replay_insights::match_contribution::FactorBasis;
    use wows_toolkit_viewmodel::rating_breakdown::RatingKind;

    use super::*;

    /// Bars and average ticks share one scale, so a part's bar reaching its
    /// tick means it earned what average play earns there.
    #[test]
    fn bars_and_average_ticks_share_one_scale() {
        const TRACK_WIDTH: f32 = 140.0;

        let breakdown = RatingBreakdown {
            kind: RatingKind::MatchContribution,
            total: 500.0,
            category: PersonalRatingCategory::Average,
            lines: vec![
                RatingLine {
                    label: "Damage".into(),
                    detail: String::new(),
                    points: 300.0,
                    average: Some(550.0),
                    basis: FactorBasis::Dynamic,
                },
                RatingLine {
                    label: "Objectives".into(),
                    detail: String::new(),
                    points: 200.0,
                    average: None,
                    basis: FactorBasis::Static,
                },
            ],
        };

        let ctx = egui::Context::default();
        ctx.set_theme(egui::Theme::Dark);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))),
            ..Default::default()
        };
        let mut track_fill = Color32::PLACEHOLDER;
        let mut bar_fill = Color32::PLACEHOLDER;
        let mut tick_color = Color32::PLACEHOLDER;
        let output = ctx.run_ui(input, |ui| {
            track_fill = ui.visuals().extreme_bg_color;
            bar_fill = breakdown.category.swatch(ui.visuals()).text;
            tick_color = ui.visuals().text_color();
            rating_breakdown(ui, &breakdown);
        });

        let mut tracks = Vec::new();
        let mut bars = Vec::new();
        let mut ticks = Vec::new();
        for clipped in &output.shapes {
            match &clipped.shape {
                egui::Shape::Rect(rect) if rect.fill == track_fill => tracks.push(rect.rect),
                egui::Shape::Rect(rect) if rect.fill == bar_fill => bars.push(rect.rect),
                egui::Shape::LineSegment { points, stroke } if stroke.color == tick_color => ticks.push(points[0].x),
                _ => {}
            }
        }

        assert_eq!(tracks.len(), 2, "one track per part");
        assert_eq!(bars.len(), 2, "one bar per part");
        for (track, (bar, points)) in tracks.iter().zip(bars.iter().zip([300.0, 200.0])) {
            assert!((track.width() - TRACK_WIDTH).abs() < 0.01);
            assert!((bar.left() - track.left()).abs() < 0.01, "a bar starts at its track's left edge");
            assert!((bar.width() - TRACK_WIDTH * points / 550.0).abs() < 0.01, "{bar:?} for {points} points");
        }
        assert_eq!(ticks.len(), 1, "only the part with an average carries a tick");
        assert!((ticks[0] - tracks[0].right()).abs() < 0.01, "the largest figure sets the scale");
    }
}
