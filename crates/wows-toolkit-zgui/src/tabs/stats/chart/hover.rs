//! What the pointer rests on in a plot: the nearest point of a line, or the bar under it, the
//! text its tooltip shows, and the marks drawn over it.

use rust_i18n::t;
use wows_toolkit_viewmodel::formatting::separate_number;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingCategory;
use wows_toolkit_viewmodel::stats::chart::ChartableStat;
use wows_toolkit_viewmodel::stats::chart::Rgb;
use wows_toolkit_viewmodel::stats::plot::Plot;
use wows_toolkit_viewmodel::stats::plot::PlotCanvas;
use wows_toolkit_viewmodel::stats::plot::PlotColor;
use wows_toolkit_viewmodel::stats::plot::PlotLayout;
use wows_toolkit_viewmodel::stats::plot::PlotPoint;
use wows_toolkit_viewmodel::stats::plot::PlotRect;
use wows_toolkit_viewmodel::stats::plot::ValueTone;

use crate::tabs::stats::chart::plot::PlotData;
use crate::tabs::stats::chart::plot::ThemeColors;

/// How far from a line's point, in logical pixels, the pointer still picks it.
pub(super) const PICK_RADIUS: f32 = 12.0;

/// The point or bar under the pointer, by where it sits in the plot's data. Held across paints,
/// so the mark over it follows the plot when it is drawn at another size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PlotHit {
    Point { series: usize, point: usize },
    Bar { index: usize },
}

/// A hit and where its mark sits, in the plot's logical pixels: the point itself, or the middle
/// of a bar's top.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Hovered {
    pub hit: PlotHit,
    pub at: PlotPoint,
}

/// What the tooltip for a hit reads.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct HoverText {
    /// The ship, or "Combined" for the combined line.
    pub name: String,
    pub color: Rgb,
    /// The game and when it was played. Absent for a bar, which is a ship over many games.
    pub game: Option<String>,
    pub value: String,
    /// The account win rate band the value falls in, for a win rate.
    pub band: Option<PersonalRatingCategory>,
}

/// The point within `radius` of `cursor` nearest it, as the egui plot picks one. Points outside
/// `area` are not drawn, so are not picked. `None` when no point is that close.
pub(super) fn nearest_point(
    lines: impl IntoIterator<Item = impl IntoIterator<Item = PlotPoint>>,
    cursor: PlotPoint,
    radius: f32,
    area: PlotRect,
) -> Option<Hovered> {
    let mut best: Option<(f32, Hovered)> = None;
    for (series, points) in lines.into_iter().enumerate() {
        for (point, at) in points.into_iter().enumerate() {
            let distance = (at.x - cursor.x).hypot(at.y - cursor.y);
            if distance > radius || !contains(area, at) {
                continue;
            }
            if best.is_some_and(|(nearest, _)| nearest <= distance) {
                continue;
            }

            best = Some((distance, Hovered { hit: PlotHit::Point { series, point }, at }));
        }
    }

    best.map(|(_, hovered)| hovered)
}

/// The bar whose column `cursor` is in, from the bar's top down to its base. A short bar is also
/// picked a little above its top, so it can be found at all.
pub(super) fn bar_under(rects: impl IntoIterator<Item = PlotRect>, cursor: PlotPoint, radius: f32) -> Option<Hovered> {
    rects
        .into_iter()
        .enumerate()
        .find(|(_, rect)| {
            cursor.x >= rect.x && cursor.x <= rect.right() && cursor.y >= rect.y - radius && cursor.y <= rect.bottom()
        })
        .map(|(index, rect)| Hovered {
            hit: PlotHit::Bar { index },
            at: PlotPoint::new(rect.x + rect.width * 0.5, rect.y),
        })
}

/// What `cursor`, in the plot's logical pixels, rests on in `data` drawn at `layout`.
pub(super) fn hover_at(data: &PlotData, layout: &PlotLayout, cursor: PlotPoint) -> Option<Hovered> {
    if !contains(layout.area, cursor) {
        return None;
    }

    let plot = data.plot();
    if plot.bars.is_empty() {
        nearest_point(layout.line_points(&plot), cursor, PICK_RADIUS, layout.area)
    } else {
        bar_under(layout.bar_rects(&plot), cursor, PICK_RADIUS)
    }
}

/// The tooltip for `hit` in `data`. `None` when `data` no longer holds it.
pub(super) fn hover_text(data: &PlotData, hit: PlotHit, stat: ChartableStat, locale: &str) -> Option<HoverText> {
    match hit {
        PlotHit::Point { series, point } => {
            let line = data.series.get(series)?;
            let at = line.points.get(point)?;
            Some(HoverText {
                name: line.name.clone(),
                color: line.color,
                game: Some(t!("ui.stats.chart_hover_game", number = point + 1, time = at.label).into_owned()),
                value: value_text(stat, at.value, locale),
                band: value_band(stat, at.value),
            })
        }
        PlotHit::Bar { index } => {
            let bar = data.bars.get(index)?;
            Some(HoverText {
                name: bar.label.clone(),
                color: bar.color,
                game: None,
                value: value_text(stat, bar.value, locale),
                band: value_band(stat, bar.value),
            })
        }
    }
}

/// A plotted value as the reader counts it: whole figures grouped by `locale`, a rate as a
/// percentage, a distance in kilometers, a time as minutes and seconds, and a count with its
/// fraction when an average has one.
pub(super) fn value_text(stat: ChartableStat, value: f64, locale: &str) -> String {
    match stat {
        ChartableStat::WinRate | ChartableStat::SurvivalRate => format!("{value:.1}%"),
        ChartableStat::DistanceTraveled => format!("{value:.1} km"),
        ChartableStat::TimeLived => {
            let seconds = value.round() as i64;
            format!("{}:{:02}", seconds / 60, seconds % 60)
        }
        ChartableStat::Frags
        | ChartableStat::Hits
        | ChartableStat::Fires
        | ChartableStat::Floods
        | ChartableStat::Citadels
        | ChartableStat::Crits
        | ChartableStat::PlanesKilled
        | ChartableStat::TorpedoesSpotted
        | ChartableStat::Heals
            if value.fract() != 0.0 =>
        {
            format!("{value:.2}")
        }
        ChartableStat::Damage
        | ChartableStat::SpottingDamage
        | ChartableStat::Frags
        | ChartableStat::RawXp
        | ChartableStat::BaseXp
        | ChartableStat::PersonalRating
        | ChartableStat::MatchContribution
        | ChartableStat::PotentialDamage
        | ChartableStat::ReceivedDamage
        | ChartableStat::Hits
        | ChartableStat::Fires
        | ChartableStat::Floods
        | ChartableStat::Citadels
        | ChartableStat::Crits
        | ChartableStat::CapturePoints
        | ChartableStat::PlanesKilled
        | ChartableStat::AntiAirDamage
        | ChartableStat::TorpedoesSpotted
        | ChartableStat::Heals => separate_number(value.round() as i64, Some(locale)),
    }
}

/// What colour the plot writes `stat`'s value labels in: a win rate in its account band's, as the
/// overview colours one.
pub(super) fn value_tone(stat: ChartableStat) -> ValueTone {
    if stat == ChartableStat::WinRate { ValueTone::WinRateBand } else { ValueTone::Series }
}

/// The account win rate band a win rate falls in. `None` for every other statistic.
pub(super) fn value_band(stat: ChartableStat, value: f64) -> Option<PersonalRatingCategory> {
    (value_tone(stat) == ValueTone::WinRateBand).then(|| PersonalRatingCategory::from_win_rate(value))
}

/// Marks `hit` over a plot drawn at `layout`, as the egui plot does: rulers through it to both
/// axes and a larger dot over a point, a ruler to the value axis and an outline round a bar.
pub(super) fn paint_hit(
    plot: &Plot<'_>,
    layout: &PlotLayout,
    hit: PlotHit,
    colors: ThemeColors,
    canvas: &mut dyn PlotCanvas,
) {
    const RULER_WIDTH: f32 = 1.0;
    const DOT_RADIUS: f32 = 5.0;
    const RING_WIDTH: f32 = 2.0;
    const OUTLINE_WIDTH: f32 = 2.0;

    let area = layout.area;
    let ruler = colors.plot.text.opacity(0.5);

    match hit {
        PlotHit::Point { series, point } => {
            let Some(color) = plot.series.get(series).map(|line| PlotColor::from(line.color)) else { return };
            let Some(at) = layout.line_points(plot).nth(series).and_then(|mut points| points.nth(point)) else {
                return;
            };

            canvas.polyline(&[PlotPoint::new(at.x, area.y), PlotPoint::new(at.x, area.bottom())], RULER_WIDTH, ruler);
            canvas.polyline(&[PlotPoint::new(area.x, at.y), PlotPoint::new(area.right(), at.y)], RULER_WIDTH, ruler);

            let ring = DOT_RADIUS + RING_WIDTH;
            canvas.fill(square(at, ring), colors.background, ring);
            canvas.fill(square(at, DOT_RADIUS), color, DOT_RADIUS);
        }
        PlotHit::Bar { index } => {
            let Some(rect) = layout.bar_rects(plot).nth(index) else { return };

            canvas.polyline(
                &[PlotPoint::new(area.x, rect.y), PlotPoint::new(area.right(), rect.y)],
                RULER_WIDTH,
                ruler,
            );
            canvas.polyline(
                &[
                    PlotPoint::new(rect.x, rect.bottom()),
                    PlotPoint::new(rect.x, rect.y),
                    PlotPoint::new(rect.right(), rect.y),
                    PlotPoint::new(rect.right(), rect.bottom()),
                ],
                OUTLINE_WIDTH,
                colors.plot.text,
            );
        }
    }
}

fn contains(area: PlotRect, point: PlotPoint) -> bool {
    point.x >= area.x && point.x <= area.right() && point.y >= area.y && point.y <= area.bottom()
}

fn square(centre: PlotPoint, radius: f32) -> PlotRect {
    PlotRect::new(centre.x - radius, centre.y - radius, radius * 2.0, radius * 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: PlotRect = PlotRect::new(0.0, 0.0, 200.0, 200.0);

    fn lines() -> Vec<Vec<PlotPoint>> {
        vec![
            vec![PlotPoint::new(10.0, 100.0), PlotPoint::new(50.0, 80.0), PlotPoint::new(90.0, 60.0)],
            vec![PlotPoint::new(50.0, 20.0), PlotPoint::new(90.0, 64.0)],
        ]
    }

    fn nearest(cursor: PlotPoint, area: PlotRect) -> Option<PlotHit> {
        let lines = lines();
        nearest_point(lines.iter().map(|points| points.iter().copied()), cursor, PICK_RADIUS, area)
            .map(|hovered| hovered.hit)
    }

    #[test]
    fn the_nearest_point_within_the_radius_is_picked_and_marked_where_it_is() {
        let lines = lines();
        let hovered = nearest_point(
            lines.iter().map(|points| points.iter().copied()),
            PlotPoint::new(52.0, 76.0),
            PICK_RADIUS,
            AREA,
        );
        assert_eq!(
            hovered,
            Some(Hovered { hit: PlotHit::Point { series: 0, point: 1 }, at: PlotPoint::new(50.0, 80.0) })
        );
    }

    #[test]
    fn of_two_points_in_one_column_the_one_nearer_up_or_down_is_picked() {
        assert_eq!(nearest(PlotPoint::new(90.0, 63.0), AREA), Some(PlotHit::Point { series: 1, point: 1 }));
    }

    #[test]
    fn a_point_beside_the_pointer_wins_over_one_farther_away_straight_above_it() {
        let lines = [[PlotPoint::new(50.0, 39.0)], [PlotPoint::new(53.0, 50.0)]];
        let hovered = nearest_point(lines, PlotPoint::new(50.0, 50.0), PICK_RADIUS, AREA);
        assert_eq!(hovered.map(|hovered| hovered.hit), Some(PlotHit::Point { series: 1, point: 0 }));
    }

    #[test]
    fn no_point_is_picked_outside_the_radius() {
        assert_eq!(nearest(PlotPoint::new(30.0, 40.0), AREA), None);
        assert_eq!(nearest(PlotPoint::new(50.0, 80.0 + PICK_RADIUS + 1.0), AREA), None);
    }

    #[test]
    fn a_point_panned_out_of_the_area_is_not_picked() {
        let area = PlotRect::new(15.0, 0.0, 185.0, 200.0);
        assert_eq!(nearest(PlotPoint::new(16.0, 100.0), area), None);
    }

    #[test]
    fn a_bar_is_picked_down_its_column_and_just_above_its_top() {
        let rects = [PlotRect::new(10.0, 50.0, 20.0, 50.0), PlotRect::new(40.0, 90.0, 20.0, 10.0)];
        let bar = |x: f32, y: f32| bar_under(rects, PlotPoint::new(x, y), PICK_RADIUS);

        assert_eq!(bar(20.0, 70.0), Some(Hovered { hit: PlotHit::Bar { index: 0 }, at: PlotPoint::new(20.0, 50.0) }));
        assert_eq!(bar(50.0, 85.0).map(|hovered| hovered.hit), Some(PlotHit::Bar { index: 1 }));
        assert_eq!(bar(35.0, 95.0), None, "between the bars");
        assert_eq!(bar(50.0, 40.0), None, "far above the bar");
    }

    #[test]
    fn values_read_as_the_reader_counts_them() {
        assert_eq!(value_text(ChartableStat::Damage, 52_345.6, "en"), "52,346");
        assert_eq!(value_text(ChartableStat::WinRate, 54.25, "en"), "54.2%");
        assert_eq!(value_text(ChartableStat::SurvivalRate, 100.0 / 3.0, "en"), "33.3%");
        assert_eq!(value_text(ChartableStat::Frags, 2.0, "en"), "2");
        assert_eq!(value_text(ChartableStat::Frags, 1.3333, "en"), "1.33");
        assert_eq!(value_text(ChartableStat::Fires, 2.5, "en"), "2.50");
        assert_eq!(value_text(ChartableStat::Hits, 37.0, "en"), "37");
        assert_eq!(value_text(ChartableStat::DistanceTraveled, 23.456, "en"), "23.5 km");
        assert_eq!(value_text(ChartableStat::TimeLived, 754.4, "en"), "12:34");
        assert_eq!(value_text(ChartableStat::TimeLived, 59.6, "en"), "1:00");
        assert_eq!(value_text(ChartableStat::PotentialDamage, 1_234_567.0, "en"), "1,234,567");
    }

    #[test]
    fn only_a_win_rate_takes_a_band() {
        assert_eq!(value_band(ChartableStat::WinRate, 54.0), Some(PersonalRatingCategory::VeryGood));
        assert_eq!(value_band(ChartableStat::WinRate, 46.9), Some(PersonalRatingCategory::Bad));
        assert_eq!(value_band(ChartableStat::SurvivalRate, 54.0), None);
        assert_eq!(value_band(ChartableStat::Damage, 54.0), None);
    }
}
