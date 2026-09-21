//! Number formatting, shared so a figure reads the same in both front ends.

use language_tags::LanguageTag;
use thousands::Separable;

/// A whole number carrying the group separator its locale uses.
///
/// French groups with spaces, every other locale with commas. A locale that
/// is absent or does not parse falls back to `en-US`, which is the locale the
/// app itself opens in.
pub fn separate_number<T: Separable>(num: T, locale: Option<&str>) -> String {
    let language: LanguageTag = locale
        .and_then(|locale| locale.replace('_', "-").parse().ok())
        .unwrap_or_else(|| LanguageTag::parse("en-US").expect("en-US is a well-formed language tag"));

    match language.primary_language() {
        "fr" => num.separate_with_spaces(),
        _ => num.separate_with_commas(),
    }
}

/// A byte count in the largest binary unit that keeps it under four digits.
///
/// Matches what the egui app prints through `humansize`'s `BINARY` format,
/// so the two apps report the same size for the same file.
pub fn byte_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit + 1 < UNITS.len() {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 { format!("{bytes} B") } else { format!("{size:.1} {}", UNITS[unit]) }
}

#[cfg(test)]
mod tests {
    use super::separate_number;

    #[test]
    fn a_separator_lands_every_three_digits() {
        assert_eq!(separate_number(0u64, None), "0");
        assert_eq!(separate_number(999u64, None), "999");
        assert_eq!(separate_number(1_234_567u64, None), "1,234,567");
        assert_eq!(separate_number(-1_234i64, None), "-1,234");
    }

    #[test]
    fn french_groups_with_spaces() {
        assert_eq!(separate_number(1_234_567u64, Some("fr")), "1 234 567");
        assert_eq!(separate_number(1_234_567u64, Some("fr_FR")), "1 234 567");
    }

    /// A locale nothing can parse still has to produce a number.
    #[test]
    fn an_unreadable_locale_falls_back_to_commas() {
        assert_eq!(separate_number(1_234_567u64, Some("not a tag")), "1,234,567");
    }

    #[test]
    fn sizes_are_shown_in_the_largest_unit_that_keeps_them_under_four_digits() {
        assert_eq!(super::byte_size(0), "0 B");
        assert_eq!(super::byte_size(512), "512 B");
        assert_eq!(super::byte_size(1024), "1.0 KiB");
        assert_eq!(super::byte_size(1024 * 1024), "1.0 MiB");
        assert_eq!(super::byte_size(1536 * 1024), "1.5 MiB");
    }
}
