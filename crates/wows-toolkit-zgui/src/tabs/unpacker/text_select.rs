//! Character selection and find over the viewer's lines. Columns count characters, not bytes.

use std::collections::VecDeque;
use std::ops::Range;

use crate::tabs::unpacker::viewer::CodeLine;
use crate::tabs::unpacker::viewer::TokenClass;

/// A place between two characters of the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TextPos {
    pub line: usize,
    pub column: usize,
}

/// A selection from where the press was to where the pointer is now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub anchor: TextPos,
    pub head: TextPos,
}

impl Selection {
    /// Every character of `lines`.
    pub fn all(lines: &[CodeLine]) -> Option<Self> {
        let last = lines.len().checked_sub(1)?;
        Some(Self {
            anchor: TextPos { line: 0, column: 0 },
            head: TextPos { line: last, column: line_len(&lines[last]) },
        })
    }

    /// The start and the end, in reading order.
    pub fn ordered(self) -> (TextPos, TextPos) {
        if self.anchor <= self.head { (self.anchor, self.head) } else { (self.head, self.anchor) }
    }

    pub fn is_empty(self) -> bool {
        self.anchor == self.head
    }

    /// The columns of `line` it covers, and whether it runs on past the line's end.
    fn on_line(self, line: usize, len: usize) -> Option<(Range<usize>, bool)> {
        let (start, end) = self.ordered();
        if self.is_empty() || line < start.line || line > end.line {
            return None;
        }
        let from = if line == start.line { start.column.min(len) } else { 0 };
        let to = if line == end.line { end.column.min(len) } else { len };
        Some((from..to, line < end.line))
    }
}

pub fn line_len(line: &CodeLine) -> usize {
    line.iter().map(|token| token.text.chars().count()).sum()
}

fn line_chars(line: &CodeLine) -> impl Iterator<Item = char> + '_ {
    line.iter().flat_map(|token| token.text.chars())
}

/// The selected characters, with a newline between lines.
pub fn selected_text(lines: &[CodeLine], selection: Selection) -> String {
    let (start, end) = selection.ordered();
    let mut out = String::new();
    let taken = end.line.saturating_sub(start.line) + 1;
    for (at, line) in lines.iter().enumerate().skip(start.line).take(taken) {
        let from = if at == start.line { start.column } else { 0 };
        let to = if at == end.line { end.column } else { usize::MAX };
        out.extend(line_chars(line).skip(from).take(to.saturating_sub(from)));
        if at < end.line {
            out.push('\n');
        }
    }

    out
}

/// One occurrence of the find query.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    pub line: usize,
    pub columns: Range<usize>,
}

/// Whether find tells upper case from lower.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchCase {
    Ignore,
    Exact,
}

/// Where one case-folded character came from.
#[derive(Clone, Copy)]
struct Origin {
    /// The column of the character it was folded from.
    column: usize,
    /// Whether it is the first character that character folded to.
    first: bool,
}

/// Appends `ch` as it is compared: unchanged when case counts, otherwise its full Unicode case
/// folding, which may be more than one character.
fn fold_into(ch: char, case: MatchCase, out: &mut Vec<char>) {
    if case == MatchCase::Exact {
        out.push(ch);
        return;
    }
    if ch.is_ascii() {
        out.push(ch.to_ascii_lowercase());
        return;
    }

    let mut buffer = [0; 4];
    out.extend(unicase::UniCase::new(&*ch.encode_utf8(&mut buffer)).to_folded_case().chars());
}

/// For each prefix of `needle`, the length of its longest proper prefix that is also its suffix.
fn fallback_table(needle: &[char]) -> Vec<usize> {
    let mut table = vec![0; needle.len()];
    let mut matched = 0;
    for at in 1..needle.len() {
        while matched > 0 && needle[at] != needle[matched] {
            matched = table[matched - 1];
        }
        if needle[at] == needle[matched] {
            matched += 1;
        }
        table[at] = matched;
    }

    table
}

/// Every occurrence of `query` in `lines`, without overlaps, in reading order.
///
/// Unless case counts, both sides are compared case-folded. A match covers whole characters of
/// the line.
pub fn find_all(lines: &[CodeLine], query: &str, case: MatchCase) -> Vec<Match> {
    let mut needle = Vec::new();
    for ch in query.chars() {
        fold_into(ch, case, &mut needle);
    }
    if needle.is_empty() {
        return Vec::new();
    }
    let fallback = fallback_table(&needle);

    let mut found = Vec::new();
    let mut folded = Vec::new();
    // Only the origins of the last `needle.len()` folded characters are kept, which is all a match
    // ending at the newest one reads, so a long line costs no memory in proportion to its length.
    let mut recent: VecDeque<Origin> = VecDeque::with_capacity(needle.len());
    for (line, tokens) in lines.iter().enumerate() {
        recent.clear();
        let mut matched = 0;
        for (column, ch) in line_chars(tokens).enumerate() {
            folded.clear();
            fold_into(ch, case, &mut folded);
            for (at, folded_ch) in folded.iter().enumerate() {
                if recent.len() == needle.len() {
                    recent.pop_front();
                }
                recent.push_back(Origin { column, first: at == 0 });

                while matched > 0 && needle[matched] != *folded_ch {
                    matched = fallback[matched - 1];
                }
                if needle[matched] == *folded_ch {
                    matched += 1;
                }
                if matched < needle.len() {
                    continue;
                }

                let start = recent[0];
                let whole = start.first && at + 1 == folded.len();
                if whole {
                    found.push(Match { line, columns: start.column..column + 1 });
                    matched = 0;
                } else {
                    matched = fallback[matched - 1];
                }
            }
        }
    }

    found
}

/// How a piece of a line is drawn beyond its token colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Marks {
    pub selected: bool,
    pub matched: bool,
    pub current: bool,
}

/// A run of one token that is marked the same throughout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub class: Option<TokenClass>,
    pub text: String,
    pub marks: Marks,
}

/// What is marked on one line: the selection, every match in reading order, and the index of the
/// current one.
pub struct LineMarks<'a> {
    pub line: usize,
    pub selection: Option<Selection>,
    pub matches: &'a [Match],
    pub current: Option<usize>,
}

/// The most matches marked on one line. A minified file can hold millions on one line, and each
/// mark is a separate element, so only those nearest the current match are marked.
pub const LINE_MATCH_LIMIT: usize = 256;

/// The first `chars` characters of `text`: their length in bytes and how many there were.
fn advance(text: &str, chars: usize) -> (usize, usize) {
    let mut taken = 0;
    for (at, _) in text.char_indices() {
        if taken == chars {
            return (at, taken);
        }
        taken += 1;
    }

    (text.len(), taken)
}

/// `tokens` cut where a mark starts or ends. A selection that runs past the line's end adds a
/// selected space, so a selected empty line still shows.
pub fn spans(tokens: &CodeLine, marks: &LineMarks<'_>) -> Vec<Span> {
    let len = line_len(tokens);
    let selected = marks.selection.and_then(|selection| selection.on_line(marks.line, len));

    let first = marks.matches.partition_point(|found| found.line < marks.line);
    let end = first + marks.matches[first..].partition_point(|found| found.line == marks.line);
    let current_here = marks.current.filter(|at| (first..end).contains(at));
    let shown_start = match current_here {
        Some(at) => at.saturating_sub(LINE_MATCH_LIMIT / 2).min(end.saturating_sub(LINE_MATCH_LIMIT)).max(first),
        None => first,
    };
    let shown = &marks.matches[shown_start..end.min(shown_start + LINE_MATCH_LIMIT)];
    let current = current_here.map(|at| marks.matches[at].columns.clone());

    let mut cuts: Vec<usize> = shown.iter().flat_map(|found| [found.columns.start, found.columns.end]).collect();
    if let Some((range, _)) = &selected {
        cuts.extend([range.start, range.end]);
    }
    cuts.sort_unstable();
    cuts.dedup();

    let marks_at = |column: usize| Marks {
        selected: selected.as_ref().is_some_and(|(range, _)| range.contains(&column)),
        matched: shown
            .get(shown.partition_point(|found| found.columns.end <= column))
            .is_some_and(|found| found.columns.contains(&column)),
        current: current.as_ref().is_some_and(|range| range.contains(&column)),
    };

    let mut out = Vec::new();
    let mut column = 0;
    let mut next_cut = 0;
    for token in tokens {
        let mut rest = token.text.as_str();
        while !rest.is_empty() {
            while cuts.get(next_cut).is_some_and(|cut| *cut <= column) {
                next_cut += 1;
            }
            let wanted = cuts.get(next_cut).map_or(usize::MAX, |cut| cut - column);
            let (bytes, taken) = advance(rest, wanted);
            out.push(Span { class: token.class, text: rest[..bytes].to_owned(), marks: marks_at(column) });
            rest = &rest[bytes..];
            column += taken;
        }
    }
    if selected.is_some_and(|(_, past_end)| past_end) {
        out.push(Span { class: None, text: " ".to_owned(), marks: Marks { selected: true, ..Marks::default() } });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tabs::unpacker::viewer::Token;
    use crate::tabs::unpacker::viewer::plain_lines;

    fn pos(line: usize, column: usize) -> TextPos {
        TextPos { line, column }
    }

    fn texts(spans: &[Span]) -> Vec<(&str, Marks)> {
        spans.iter().map(|span| (span.text.as_str(), span.marks)).collect()
    }

    const SELECTED: Marks = Marks { selected: true, matched: false, current: false };
    const PLAIN: Marks = Marks { selected: false, matched: false, current: false };

    #[test]
    fn a_selection_backwards_copies_the_same_text_as_forwards() {
        let lines = plain_lines("alpha\nbeta\ngamma");
        let forward = Selection { anchor: pos(0, 2), head: pos(2, 3) };
        let backward = Selection { anchor: pos(2, 3), head: pos(0, 2) };
        assert_eq!(selected_text(&lines, forward), "pha\nbeta\ngam");
        assert_eq!(selected_text(&lines, backward), "pha\nbeta\ngam");
    }

    #[test]
    fn select_all_covers_every_character() {
        let lines = plain_lines("one\n\nthree");
        let all = Selection::all(&lines).expect("the text has lines");
        assert_eq!(selected_text(&lines, all), "one\n\nthree");
        assert_eq!(Selection::all(&[]), None);
    }

    #[test]
    fn columns_count_characters_not_bytes() {
        let lines = plain_lines("Zuih\u{14d} (old)");
        let selection = Selection { anchor: pos(0, 4), head: pos(0, 5) };
        assert_eq!(selected_text(&lines, selection), "\u{14d}");
    }

    #[test]
    fn find_ignores_case_and_does_not_overlap() {
        let lines = plain_lines("Ocean ocean\naaaa");
        let found = find_all(&lines, "OCEAN", MatchCase::Ignore);
        assert_eq!(found, vec![Match { line: 0, columns: 0..5 }, Match { line: 0, columns: 6..11 }]);
        assert_eq!(find_all(&lines, "aa", MatchCase::Ignore).len(), 2, "aaaa holds two matches, not three");
        assert!(find_all(&lines, "", MatchCase::Ignore).is_empty());
    }

    #[test]
    fn find_matching_case_tells_upper_from_lower() {
        let lines = plain_lines("Ocean ocean\nStra\u{df}e");
        assert_eq!(find_all(&lines, "ocean", MatchCase::Exact), vec![Match { line: 0, columns: 6..11 }]);
        assert!(find_all(&lines, "OCEAN", MatchCase::Exact).is_empty());
        assert!(find_all(&lines, "strasse", MatchCase::Exact).is_empty(), "no case folding either");
    }

    #[test]
    fn spans_cut_tokens_at_the_selection_and_keep_their_class() {
        let line = vec![
            Token { class: Some(TokenClass::Tag), text: "<ship".to_owned() },
            Token { class: None, text: ">".to_owned() },
        ];
        let selection = Some(Selection { anchor: pos(0, 1), head: pos(0, 6) });
        let cut = spans(&line, &LineMarks { line: 0, selection, matches: &[], current: None });
        assert_eq!(texts(&cut), vec![("<", PLAIN), ("ship", SELECTED), (">", SELECTED)]);
        assert_eq!(cut[1].class, Some(TokenClass::Tag));
    }

    #[test]
    fn a_selection_running_past_the_line_marks_its_end() {
        let lines = plain_lines("ab\n\ncd");
        let selection = Some(Selection { anchor: pos(0, 1), head: pos(2, 1) });
        let first = spans(&lines[0], &LineMarks { line: 0, selection, matches: &[], current: None });
        assert_eq!(texts(&first), vec![("a", PLAIN), ("b", SELECTED), (" ", SELECTED)]);
        let empty = spans(&lines[1], &LineMarks { line: 1, selection, matches: &[], current: None });
        assert_eq!(texts(&empty), vec![(" ", SELECTED)], "an empty line in the selection still shows");
        let last = spans(&lines[2], &LineMarks { line: 2, selection, matches: &[], current: None });
        assert_eq!(texts(&last), vec![("c", SELECTED), ("d", PLAIN)]);
    }

    #[test]
    fn the_current_match_is_told_apart_from_the_others() {
        let lines = plain_lines("sea and sea");
        let found = find_all(&lines, "sea", MatchCase::Ignore);
        let marks = LineMarks { line: 0, selection: None, matches: &found, current: Some(1) };
        let cut = spans(&lines[0], &marks);
        let other = Marks { matched: true, ..PLAIN };
        let current = Marks { matched: true, current: true, ..PLAIN };
        assert_eq!(texts(&cut), vec![("sea", other), (" and ", PLAIN), ("sea", current)]);
    }

    #[test]
    fn find_folds_case_fully_and_matches_whole_characters() {
        let lines = plain_lines("Stra\u{df}e\n\u{df}");
        assert_eq!(find_all(&lines, "STRASSE", MatchCase::Ignore), vec![Match { line: 0, columns: 0..6 }]);
        assert_eq!(
            find_all(&lines, "ss", MatchCase::Ignore),
            vec![Match { line: 0, columns: 4..5 }, Match { line: 1, columns: 0..1 }]
        );
        assert_eq!(
            find_all(&lines, "s", MatchCase::Ignore),
            vec![Match { line: 0, columns: 0..1 }],
            "half of a folded character is not a match"
        );
    }

    #[test]
    fn find_restarts_after_a_partial_match() {
        let lines = plain_lines("aabaabaaab");
        assert_eq!(find_all(&lines, "aaab", MatchCase::Ignore), vec![Match { line: 0, columns: 6..10 }]);
    }

    #[test]
    fn a_line_marks_at_most_the_limit_around_the_current_match() {
        let lines = plain_lines(&"x".repeat(LINE_MATCH_LIMIT * 4));
        let found = find_all(&lines, "x", MatchCase::Ignore);
        let current = LINE_MATCH_LIMIT * 3;
        let cut = spans(&lines[0], &LineMarks { line: 0, selection: None, matches: &found, current: Some(current) });
        let marked: usize = cut.iter().filter(|span| span.marks.matched).map(|span| span.text.len()).sum();
        assert_eq!(marked, LINE_MATCH_LIMIT);
        let current_at = cut.iter().position(|span| span.marks.current).expect("the current match is marked");
        let before: usize = cut[..current_at].iter().map(|span| span.text.len()).sum();
        assert_eq!(before, current, "the current match is drawn at its column");
    }
}
