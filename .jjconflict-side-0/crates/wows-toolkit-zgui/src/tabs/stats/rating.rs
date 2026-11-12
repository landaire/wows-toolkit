//! A personal rating drawn in its band's colours.

use wows_toolkit_viewmodel::personal_rating;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingCategory;
use zgui::component;
use zgui::css;
use zgui::view;
use zgui::view::Attrs;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;

const SHEET: &str = "wt-stats-rating";

const CSS: &str = css!(
    ".wt-rating-chip {
        flex: none;
        padding: 0 4px;
        border-radius: 3px;
        white-space: nowrap;
    }"
);

/// The suffix of a rating band's `--wt-pr-*` properties.
const fn band_name(category: PersonalRatingCategory) -> &'static str {
    match category {
        PersonalRatingCategory::Bad => "bad",
        PersonalRatingCategory::BelowAverage => "below-average",
        PersonalRatingCategory::Average => "average",
        PersonalRatingCategory::Good => "good",
        PersonalRatingCategory::VeryGood => "very-good",
        PersonalRatingCategory::Great => "great",
        PersonalRatingCategory::Unicum => "unicum",
        PersonalRatingCategory::SuperUnicum => "super-unicum",
    }
}

/// The colour text in `category` is drawn in.
pub(super) fn text_color(category: PersonalRatingCategory) -> String {
    format!("var(--wt-pr-{}-text)", band_name(category))
}

/// The band's hue, tinted as the GPUI chip tints it.
fn tint(category: PersonalRatingCategory) -> String {
    let percent = f32::from(personal_rating::CHIP_TINT_ALPHA) / 255.0 * 100.0;
    format!("color-mix(in srgb, var(--wt-pr-{}-hue) {percent}%, transparent)", band_name(category))
}

/// `text` on a tint of `category`'s hue, in its text colour.
#[component]
pub(super) fn RatingChip(
    #[prop(into)] text: String,
    category: PersonalRatingCategory,
    #[prop(attrs)] attrs: Attrs,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);

    view! {
        text(
            class = "wt-rating-chip",
            style:color = text_color(category),
            style:background-color = tint(category),
            {..attrs}
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
            let name = band_name(*category);
            let text = Rgb::new(personal_rating::chip_text(*category, dark));
            let hue = Rgb::new(personal_rating::chip_hue(*category));
            format!("--wt-pr-{name}-text: {text}; --wt-pr-{name}-hue: {hue}; ")
        })
        .collect()
}
