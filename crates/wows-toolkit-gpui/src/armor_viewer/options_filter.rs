//! The options rail's filter: which controls a typed query keeps, and which
//! letters of each label it matched.
//!
//! A label is kept when it contains the query, or failing that when the
//! query's letters appear in it in order, each one starting a word or
//! following the letter before it. Case and spaces in the query are ignored.

use std::ops::Range;

use gpui_kit::FontWeight;
use gpui_kit::HighlightStyle;
use gpui_kit::Hsla;
use gpui_kit::SharedString;
use gpui_kit::StyledText;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct OptionsFilter {
    /// The query with surrounding space removed, lower-cased a character at
    /// a time as labels are.
    query: Vec<char>,
    /// The query's letters without its spaces, for the scattered match.
    wanted: Vec<char>,
}

impl OptionsFilter {
    pub(crate) fn new(query: &str) -> Self {
        let query: Vec<char> = query.trim().chars().flat_map(char::to_lowercase).collect();
        let wanted = query.iter().copied().filter(|c| !c.is_whitespace()).collect();
        Self { query, wanted }
    }

    /// Whether a query is typed. With none every control is kept and the
    /// sections keep their own open state.
    pub(crate) fn is_active(&self) -> bool {
        !self.query.is_empty()
    }

    /// Whether `label` is kept, for a label that decides what else is kept
    /// but is drawn by another control.
    pub(crate) fn keeps(&self, label: &str) -> bool {
        self.matched(label).is_some()
    }

    /// `label` with the matched letters picked out on `mark`; `None` when the
    /// filter drops it.
    pub(crate) fn label(&self, label: impl Into<SharedString>, mark: Hsla) -> Option<StyledText> {
        let label: SharedString = label.into();
        let ranges = self.matched(&label)?;
        let style =
            HighlightStyle { font_weight: Some(FontWeight::BOLD), background_color: Some(mark), ..Default::default() };
        Some(StyledText::new(label).with_highlights(ranges.into_iter().map(|range| (range, style))))
    }

    /// The byte ranges of `label` the query matched, merged where they touch.
    /// Empty for no query; `None` when the label does not match.
    fn matched(&self, label: &str) -> Option<Vec<Range<usize>>> {
        if self.query.is_empty() {
            return Some(Vec::new());
        }

        // Lowered a character at a time, each lowered character remembering
        // the original it came from, so a match maps back to whole original
        // characters whatever lowering does to their count or length.
        let chars: Vec<(usize, char)> = label.char_indices().collect();
        let mut lowered = Vec::with_capacity(chars.len());
        let mut origin = Vec::with_capacity(chars.len());
        let mut starts = Vec::with_capacity(chars.len());
        for (ix, &(_, c)) in chars.iter().enumerate() {
            let word_start = match ix.checked_sub(1).map(|previous| chars[previous].1) {
                None => true,
                Some(previous) => {
                    !previous.is_alphanumeric() && c.is_alphanumeric()
                        || previous.is_lowercase() && c.is_uppercase()
                        || previous.is_alphabetic() && c.is_numeric()
                }
            };
            for (piece, lower) in c.to_lowercase().enumerate() {
                lowered.push(lower);
                origin.push(ix);
                starts.push(word_start && piece == 0);
            }
        }
        let span = |ix: usize| {
            let (start, c) = chars[ix];
            start..start + c.len_utf8()
        };

        if let Some(found) = lowered.windows(self.query.len()).position(|window| window == self.query.as_slice()) {
            let first = origin[found];
            let last = origin[found + self.query.len() - 1];
            return Some(std::iter::once(span(first).start..span(last).end).collect());
        }

        // Scattered letters only count where each one either carries on the
        // run before it or starts a word: "arm op" finds "Armor Opacity", and
        // "opac" does not find "Bow_patch_MidFront".
        let picked = anchored(&self.wanted, &lowered, &starts)?;

        let mut ranges: Vec<Range<usize>> = Vec::new();
        for position in picked {
            let matched = span(origin[position]);
            match ranges.last_mut() {
                // Touching, or a second lowered piece of the same character.
                Some(previous) if previous.end >= matched.start => previous.end = previous.end.max(matched.end),
                _ => ranges.push(matched),
            }
        }
        Some(ranges)
    }
}

/// The leftmost positions in `lowered` that take `wanted` in order, each on a
/// word start or right after the one before it.
///
/// Decided backward over every (letter, position) pair once, rather than by
/// trying placements forward and backing out of them, which costs time
/// exponential in the query's length on labels of many short words.
fn anchored(wanted: &[char], lowered: &[char], starts: &[bool]) -> Option<Vec<usize>> {
    let width = lowered.len();
    let at = |letter: usize, position: usize| letter * width + position;

    // `fits[at(letter, position)]`: `wanted[letter..]` can be placed with
    // `wanted[letter]` at `position`.
    let mut fits = vec![false; wanted.len() * width];
    for (letter, &c) in wanted.iter().enumerate().rev() {
        let next = letter + 1;
        // Whether a word start past `position` fits the next letter.
        let mut start_ahead = false;
        for position in (0..width).rev() {
            let rest_fits =
                next == wanted.len() || start_ahead || (position + 1 < width && fits[at(next, position + 1)]);
            fits[at(letter, position)] = lowered[position] == c && rest_fits;
            if next < wanted.len() {
                start_ahead |= starts[position] && fits[at(next, position)];
            }
        }
    }

    let mut picked = Vec::with_capacity(wanted.len());
    let mut previous: Option<usize> = None;
    for letter in 0..wanted.len() {
        let from = previous.map_or(0, |previous| previous + 1);
        let position = (from..width).find(|&position| {
            let reachable = starts[position] || previous.is_some_and(|previous| previous + 1 == position);
            reachable && fits[at(letter, position)]
        })?;
        picked.push(position);
        previous = Some(position);
    }
    Some(picked)
}

#[cfg(test)]
mod tests {
    use super::OptionsFilter;

    #[test]
    fn a_run_of_letters_is_matched_as_one_span_before_a_scattered_one() {
        let filter = OptionsFilter::new("OPAC");
        assert_eq!(filter.matched("Armor Opacity"), Some(std::iter::once(6..10).collect()));
    }

    #[test]
    fn scattered_letters_match_only_at_word_starts() {
        let filter = OptionsFilter::new("arm op");
        assert_eq!(filter.matched("Armor Opacity"), Some(vec![0..3, 6..8]));
        assert_eq!(OptionsFilter::new("opac").matched("Bow_patch_MidFrontShape"), None);
        assert_eq!(OptionsFilter::new("mfs").matched("Bow_patch_MidFrontShape"), Some(vec![10..11, 13..14, 18..19]));
    }

    #[test]
    fn no_query_keeps_everything_and_marks_nothing() {
        let filter = OptionsFilter::new("  ");
        assert!(!filter.is_active());
        assert_eq!(filter.matched("Roll"), Some(Vec::new()));
    }

    #[test]
    fn a_span_covers_whole_characters_outside_ascii() {
        let filter = OptionsFilter::new("d\u{e9}");
        assert_eq!(filter.matched("D\u{e9}g\u{e2}ts"), Some(std::iter::once(0..3).collect()));
    }

    /// "Aeussere Panzerung" with an umlaut and a sharp s, each two bytes.
    #[test]
    fn scattered_spans_fall_on_character_boundaries_outside_ascii() {
        let label = "\u{c4}u\u{df}ere Panzerung";
        assert_eq!(OptionsFilter::new("pan").matched(label), Some(std::iter::once(9..12).collect()));
        assert_eq!(OptionsFilter::new("\u{e4}p").matched(label), Some(vec![0..2, 9..10]));
    }

    /// A capital dotted I lowers to two characters; matching either piece
    /// marks the whole of it.
    #[test]
    fn a_character_that_lowers_to_two_is_marked_whole() {
        let label = "\u{130}zmir Plate";
        assert_eq!(OptionsFilter::new("\u{130}p").matched(label), Some(vec![0..2, 7..8]));
        assert_eq!(OptionsFilter::new("i").matched(label), Some(std::iter::once(0..2).collect()));
    }

    /// Many one-letter words and a query that almost fits them: placing
    /// letters forward and backing out takes on the order of 10^17 tries here.
    #[test]
    fn a_query_that_almost_fits_many_short_words_is_refused_quickly() {
        let label = vec!["s"; 60].join(" ");
        let query = format!("{}x", "s".repeat(30));
        assert_eq!(OptionsFilter::new(&query).matched(&label), None);
    }
}
