//! The hover over a PR or BI figure: the rating's parts, largest first, each with a bar of its
//! points and a mark where average play would have put it.

use landaire_ui::TooltipContentProps;
use landaire_ui::TooltipProps;
use landaire_ui::TooltipTriggerProps;
use wows_replay_insights::battle_impact::BattleImpactResult;
use wows_replay_insights::personal_rating::PersonalRatingResult;
use wows_toolkit_viewmodel::rating_breakdown::RatingBreakdown;
use wows_toolkit_viewmodel::rating_breakdown::RatingKind;
use zgui::css;
use zgui::prelude::AnyView;
use zgui::reactive::StoredValue;
use zgui::reactive::prelude::WithValue;
use zgui::view;
use zgui::view::install_stylesheet;

use crate::theme::rating_band_name;

const SHEET: &str = "wt-rating-hover";

const CSS: &str = css!(
    ".wt-rh {
        display: flex;
        flex-direction: column;
        gap: 4px;
        font-size: 0.75rem;
    }
    .wt-rh__title { font-weight: 700; font-variant-numeric: tabular-nums; }
    .wt-rh__line { display: flex; flex-direction: column; gap: 1px; }
    .wt-rh__figures {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 6px;
        font-variant-numeric: tabular-nums;
    }
    .wt-rh__label { width: 104px; flex: none; }
    .wt-rh__points { width: 40px; flex: none; text-align: right; }
    .wt-rh__track {
        position: relative;
        width: 140px;
        height: 8px;
        flex: none;
        background-color: color-mix(in srgb, var(--lui-color-text) 10%, transparent);
    }
    .wt-rh__bar { position: absolute; left: 0; top: 0; bottom: 0; }
    .wt-rh__tick {
        position: absolute;
        top: -2px;
        bottom: -2px;
        width: 2px;
        margin-left: -1px;
        background-color: var(--lui-color-text);
    }
    .wt-rh__versus { width: 44px; flex: none; text-align: right; color: var(--lui-color-text-dim); }
    .wt-rh__detail { color: var(--lui-color-text-dim); max-width: 360px; white-space: normal; }"
);

/// The rating a hover explains, kept whole so its breakdown is built only once the hover opens.
#[derive(Clone, Debug)]
pub enum RatedFigure {
    Personal(PersonalRatingResult),
    BattleImpact(Box<BattleImpactResult>),
}

impl RatedFigure {
    /// `None` for a PR read back from storage, which kept no parts.
    fn breakdown(&self) -> Option<RatingBreakdown> {
        let locale = wows_toolkit_viewmodel::locale();
        match self {
            Self::Personal(rating) => RatingBreakdown::personal_rating(rating, Some(&locale)),
            Self::BattleImpact(rating) => Some(RatingBreakdown::battle_impact(rating, Some(&locale))),
        }
    }
}

/// `content` with the rating's parts shown over it, the trigger drawn with `trigger_class`.
pub fn with_rating_hover(content: AnyView, figure: RatedFigure, trigger_class: &'static str) -> AnyView {
    let figure = StoredValue::new_local(figure);
    let explained = move || figure.with_value(RatedFigure::breakdown).map(|breakdown| listing(&breakdown));

    AnyView::new(view! {
        Tooltip {
            TooltipTrigger(class = trigger_class) {{content}}
            TooltipContent {{explained()}}
        }
    })
}

/// The rating's total and one line per part, bars on one scale in the total's band colour.
pub fn listing(breakdown: &RatingBreakdown) -> AnyView {
    install_stylesheet(SHEET, CSS);

    let band = rating_band_name(breakdown.category);
    let ink = format!("var(--wt-pr-{band}-text)");
    let fill = format!("var(--wt-pr-{band}-hue)");
    let title = format!("{} {:.0}", breakdown.kind.title(), breakdown.total);
    let scale = breakdown.scale();
    // Where `points` falls along the track. A rating of nothing has no length to scale by.
    let along = move |points: f64| {
        let fraction = if scale > 0.0 { (points / scale).clamp(0.0, 1.0) } else { 0.0 };
        format!("{}%", fraction * 100.0)
    };
    let kind = match breakdown.kind {
        RatingKind::Personal => "pr",
        RatingKind::BattleImpact => "bi",
    };

    let lines: Vec<AnyView> = breakdown
        .lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let tick = line.average.map(|average| {
                let left = along(average);
                AnyView::new(view! { box(class = "wt-rh__tick", style:left = left) {} })
            });
            let versus = line.versus_average().map(|delta| format!("{delta:+.0}")).unwrap_or_default();
            let width = along(line.points);
            let fill = fill.clone();
            AnyView::new(view! {
                box(class = "wt-rh__line", attr:data-testid = format!("rating-{kind}-line-{index}")) {
                    box(class = "wt-rh__figures") {
                        text(class = "wt-rh__label") {{line.label.clone()}}
                        text(class = "wt-rh__points") {{format!("{:.0}", line.points)}}
                        box(class = "wt-rh__track") {
                            box(class = "wt-rh__bar", style:width = width, style:background-color = fill) {}
                            {tick}
                        }
                        text(class = "wt-rh__versus") {{versus}}
                    }
                    text(class = "wt-rh__detail") {{line.detail.clone()}}
                }
            })
        })
        .collect();

    AnyView::new(view! {
        box(class = "wt-rh", attr:data-testid = format!("rating-{kind}-hover")) {
            text(class = "wt-rh__title", style:color = ink) {{title}}
            {lines}
        }
    })
}
