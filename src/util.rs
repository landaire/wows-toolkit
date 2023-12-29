use egui::Color32;
use language_tags::LanguageTag;
use thousands::Separable;

pub fn separate_number<T: Separable>(num: T, locale: Option<&str>) -> String {
    let language: LanguageTag = locale
        .and_then(|locale| locale.parse().ok())
        .unwrap_or_else(|| LanguageTag::parse("en-US").unwrap());

    match language.primary_language() {
        "fr" => num.separate_with_spaces(),
        _ => num.separate_with_commas(),
    }
}

pub fn player_color_for_team_relation(relation: u32, is_dark_mode: bool) -> Color32 {
    match relation {
        0 => Color32::GOLD,
        1 => {
            if is_dark_mode {
                Color32::LIGHT_GREEN
            } else {
                Color32::DARK_GREEN
            }
        }
        _ => {
            if is_dark_mode {
                Color32::LIGHT_RED
            } else {
                Color32::DARK_RED
            }
        }
    }
}
