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
