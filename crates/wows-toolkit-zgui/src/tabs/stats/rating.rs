//! A personal rating drawn in its band's colours.

use wows_toolkit_viewmodel::personal_rating;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingCategory;
use zgui::component;
use zgui::css;
use zgui::view;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;

use crate::theme::rating_band_name;

const SHEET: &str = "wt-stats-rating";

const CSS: &str = css!(
    ".wt-rating-chip {
        flex: none;
        padding: 0 4px;
        border-radius: 3px;
        white-space: nowrap;
    }"
);

/// The colour text in `category` is drawn in.
pub(super) fn text_color(category: PersonalRatingCategory) -> String {
    format!("var(--wt-pr-{}-text)", rating_band_name(category))
}

/// The band a Random and Ranked win rate falls in, as a `data-band` name and a text colour.
pub(super) struct WinRateBand {
    pub(super) name: String,
    pub(super) color: String,
}

impl WinRateBand {
    /// The band for `rate`, a percentage from 0 to 100.
    pub(super) fn from_rate(rate: f64) -> Self {
        let category = PersonalRatingCategory::from_win_rate(rate);

        Self { name: rating_band_name(category).to_owned(), color: text_color(category) }
    }
}

/// The band's hue at the chip tint alpha.
fn tint(category: PersonalRatingCategory) -> String {
    let percent = f32::from(personal_rating::CHIP_TINT_ALPHA) / 255.0 * 100.0;
    format!("color-mix(in srgb, var(--wt-pr-{}-hue) {percent}%, transparent)", rating_band_name(category))
}

/// `text` on a tint of `category`'s hue, in its text colour.
#[component]
pub(super) fn RatingChip(#[prop(into)] text: String, category: PersonalRatingCategory) -> impl IntoView {
    install_stylesheet(SHEET, CSS);

    view! {
        text(
            class = "wt-rating-chip",
            style:color = text_color(category),
            style:background-color = tint(category)
        ) {{text}}
    }
}

/// The `--wt-pr-*` properties for one appearance, for a test window drawn without the shell's
/// theme.
#[cfg(test)]
pub(super) fn test_properties(dark: bool) -> String {
    use landaire_ui::Rgb;

    PersonalRatingCategory::ALL
        .iter()
        .map(|category| {
            let name = rating_band_name(*category);
            let text = Rgb::new(personal_rating::chip_text(*category, dark));
            let hue = Rgb::new(personal_rating::chip_hue(*category));
            format!("--wt-pr-{name}-text: {text}; --wt-pr-{name}-hue: {hue}; ")
        })
        .collect()
}
