//! The hover beside a PR or MC: the rating in its band's colour over one row
//! per part, largest first, each with its points, a bar on one scale for the
//! whole rating, a tick where average play would reach, and its inputs.

use std::rc::Rc;

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::h_flex;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use wows_replay_insights::match_contribution::MatchContributionResult;
use wows_toolkit_replay_data::columns::ColorRole;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingResult;
use wows_toolkit_viewmodel::rating_breakdown::RatingBreakdown;
use wows_toolkit_viewmodel::rating_breakdown::RatingLine;
use wows_toolkit_viewmodel::rating_breakdown::basis_badge;
use wows_toolkit_viewmodel::rating_breakdown::basis_explanation;

use crate::replay_inspector::table::resolve_color;

/// Length of every bar's track; a part's bar fills it in proportion to the
/// breakdown's scale.
const TRACK_WIDTH: f32 = 140.0;

const TRACK_HEIGHT: Pixels = px(8.);

const TICK_WIDTH: f32 = 2.0;

/// Every row's height, the same in each column so a part's figures line up
/// across them.
const ROW_HEIGHT: Pixels = px(18.);

/// A rating whose parts the hover lists, broken down only when the hover
/// opens.
pub enum RatedFigure {
    Personal(PersonalRatingResult),
    MatchContribution(Box<MatchContributionResult>),
}

impl RatedFigure {
    /// `None` for a PR read back from storage, which kept only its number.
    pub fn personal(result: &PersonalRatingResult) -> Option<Self> {
        result.breakdown.is_some().then(|| Self::Personal(result.clone()))
    }

    fn breakdown(&self) -> Option<RatingBreakdown> {
        let locale = wows_toolkit_viewmodel::locale();
        match self {
            Self::Personal(result) => RatingBreakdown::personal_rating(result, Some(&locale)),
            Self::MatchContribution(result) => Some(RatingBreakdown::match_contribution(result, Some(&locale))),
        }
    }
}

/// The `.tooltip()` callback listing `figure`'s parts.
pub fn tooltip(figure: Rc<RatedFigure>) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    move |window, cx| {
        let breakdown = figure.breakdown();
        Tooltip::element(move |_window, cx| div().children(breakdown.as_ref().map(|breakdown| listing(breakdown, cx))))
            .build(window, cx)
    }
}

/// The breakdown's title and one row per part: label, points, bar, the
/// difference from average play, and the inputs.
pub fn listing(breakdown: &RatingBreakdown, cx: &App) -> AnyElement {
    let band = resolve_color(ColorRole::PrTier(breakdown.category));
    let foreground = cx.theme().foreground;
    let dim = crate::theme::text_dim();
    let scale = breakdown.scale();
    let column = || v_flex().flex_none();
    let cell = || h_flex().h(ROW_HEIGHT).items_center().whitespace_nowrap();

    let mut labels = column();
    let mut points = column().items_end();
    let mut bars = column();
    let mut deltas = column().items_end();
    let mut bases = column();
    let mut details = column();
    for (index, line) in breakdown.lines.iter().enumerate() {
        labels = labels.child(cell().child(line.label.clone()));
        points = points.child(crate::ui::tabular(cell()).child(format!("{:.0}", line.points)));
        bars = bars.child(cell().child(bar(index, line, scale, band, foreground)));
        deltas = deltas.child(
            crate::ui::tabular(cell())
                .text_color(dim)
                .children(line.versus_average().map(|delta| format!("{delta:+.0}"))),
        );
        bases = bases.child(cell().text_color(dim).child(basis_badge(line.basis)));
        details = details.child(cell().text_color(dim).child(line.detail.clone()));
    }
    let rows = h_flex()
        .gap_2()
        .items_start()
        .child(labels)
        .child(points)
        .child(bars)
        .child(deltas)
        .child(bases)
        .child(details);
    let legend = breakdown.bases().into_iter().map(|basis| div().text_color(dim).child(basis_explanation(basis)));

    v_flex()
        .gap_1()
        .text_xs()
        .child(div().font_weight(FontWeight::BOLD).text_color(band).child(format!(
            "{} {:.0}",
            breakdown.kind.title(),
            breakdown.total
        )))
        .child(rows)
        .children(legend)
        .into_any_element()
}

/// A part's points as a filled length of the track, with a tick at what
/// average play earns there.
fn bar(index: usize, line: &RatingLine, scale: f64, band: Hsla, foreground: Hsla) -> impl IntoElement {
    let fraction = |value: f64| if scale > 0.0 { (value / scale).clamp(0.0, 1.0) as f32 } else { 0.0 };

    div()
        .relative()
        .flex_none()
        .w(px(TRACK_WIDTH))
        .h(TRACK_HEIGHT)
        .bg(foreground.opacity(0.08))
        .child(
            div()
                .id(("rating-bar", index))
                .test_support()
                .absolute()
                .left_0()
                .top_0()
                .bottom_0()
                .w(px(TRACK_WIDTH * fraction(line.points)))
                .bg(band),
        )
        .when_some(line.average, |track, average| {
            track.child(
                div()
                    .id(("rating-average", index))
                    .test_support()
                    .absolute()
                    .top(px(-2.))
                    .bottom(px(-2.))
                    .left(px(TRACK_WIDTH * fraction(average) - TICK_WIDTH / 2.0))
                    .w(px(TICK_WIDTH))
                    .bg(foreground),
            )
        })
}

#[cfg(test)]
mod tests {
    use wows_replay_insights::match_contribution::FactorBasis;
    use wows_replay_insights::personal_rating::PersonalRatingCategory;
    use wows_toolkit_viewmodel::rating_breakdown::RatingBreakdown;
    use wows_toolkit_viewmodel::rating_breakdown::RatingKind;
    use wows_toolkit_viewmodel::rating_breakdown::RatingLine;

    fn line(points: f64, average: Option<f64>) -> RatingLine {
        RatingLine { label: "part".into(), detail: String::new(), points, average, basis: FactorBasis::Static }
    }

    /// Every bar and tick is drawn on the one scale of the largest figure, so
    /// a part's length against its tick says how it compares to average play.
    #[gpui_kit::test]
    fn bars_and_ticks_share_the_breakdowns_scale(cx: &mut gpui_kit::TestAppContext) {
        use gpui_kit::AppContext as _;
        use gpui_kit::test::TestWindowExt as _;

        struct Hover(RatingBreakdown);

        impl gpui_kit::Render for Hover {
            fn render(
                &mut self,
                _window: &mut gpui_kit::Window,
                cx: &mut gpui_kit::Context<Self>,
            ) -> impl gpui_kit::IntoElement {
                super::listing(&self.0, cx)
            }
        }

        let breakdown = RatingBreakdown {
            kind: RatingKind::MatchContribution,
            total: 1_200.0,
            category: PersonalRatingCategory::Good,
            lines: vec![line(800.0, Some(400.0)), line(200.0, Some(400.0)), line(200.0, None)],
        };
        cx.update(gpui_kit::init);
        let window =
            cx.open_window(gpui_kit::size(gpui_kit::px(800.), gpui_kit::px(200.)), |_window, _cx| Hover(breakdown));

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            let width = |id: (&'static str, usize)| window.find(id).bounds().size.width.as_f32();
            let left = |id: (&'static str, usize)| window.find(id).bounds().origin.x.as_f32();
            let bars = [0usize, 1, 2].map(|index| width(("rating-bar", index)));
            for (bar, expected) in bars.iter().zip([140.0, 35.0, 35.0]) {
                assert!((bar - expected).abs() < 0.5, "{bars:?}");
            }

            let track_start = left(("rating-bar", 0));
            let tick_centre = left(("rating-average", 0)) + super::TICK_WIDTH / 2.0 - track_start;
            assert!((tick_centre - 70.0).abs() < 0.5, "average 400 of 800 sits mid-track, got {tick_centre}");
            assert!(window.try_find(("rating-average", 2usize)).is_none(), "a part with no average has no tick");
        })
        .expect("the window is open");
    }
}
