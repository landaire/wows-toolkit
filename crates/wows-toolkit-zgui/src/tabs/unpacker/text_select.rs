//! Character selection and find over the viewer's lines. Columns count characters, not bytes.

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

fn line_text(line: &CodeLine) -> String {
    line.iter().map(|token| token.text.as_str()).collect()
}

/// The selected characters, with a newline between lines.
pub fn selected_text(lines: &[CodeLine], selection: Selection) -> String {
    let (start, end) = selection.ordered();
    let mut out = String::new();
    let taken = end.line.saturating_sub(start.line) + 1;
    for (at, line) in lines.iter().enumerate().skip(start.line).take(taken) {
        let text = line_text(line);
        let from = if at == start.line { start.column } else { 0 };
        let to = if at == end.line { end.column } else { usize::MAX };
        out.extend(text.chars().skip(from).take(to.saturating_sub(from)));
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

/// Every occurrence of `query` in `lines`, ignoring case, without overlaps, in reading order.
pub fn find_all(lines: &[CodeLine], query: &str) -> Vec<Match> {
    let needle: Vec<char> = query.chars().collect();
    if needle.is_empty() {
        return Vec::new();
    }
    let same = |a: char, b: char| a == b || a.to_lowercase().eq(b.to_lowercase());
    let mut found = Vec::new();
    for (line, tokens) in lines.iter().enumerate() {
        let hay: Vec<char> = tokens.iter().flat_map(|token| token.text.chars()).collect();
        let mut at = 0;
        while at + needle.len() <= hay.len() {
            if hay[at..at + needle.len()].iter().zip(&needle).all(|(a, b)| same(*a, *b)) {
                found.push(Match { line, columns: at..at + needle.len() });
                at += needle.len();
            } else {
                at += 1;
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

/// What is marked on one line: the selection, the matches on it and the current match.
pub struct LineMarks<'a> {
    pub line: usize,
    pub selection: Option<Selection>,
    pub matches: &'a [Match],
    pub current: Option<&'a Match>,
}

/// `tokens` cut where a mark starts or ends. A selection that runs past the line's end adds a
/// selected space, so a selected empty line still shows.
pub fn spans(tokens: &CodeLine, marks: &LineMarks<'_>) -> Vec<Span> {
    let len = line_len(tokens);
    let selected = marks.selection.and_then(|selection| selection.on_line(marks.line, len));
    let on_line = |found: &&Match| found.line == marks.line;
    let current = marks.current.filter(on_line).map(|found| found.columns.clone());
    let matched: Vec<Range<usize>> = marks.matches.iter().filter(on_line).map(|found| found.columns.clone()).collect();

    let mut cuts: Vec<usize> = Vec::new();
    if let Some((range, _)) = &selected {
        cuts.extend([range.start, range.end]);
    }
    for range in matched.iter().chain(current.iter()) {
        cuts.extend([range.start, range.end]);
    }
    cuts.sort_unstable();
    cuts.dedup();

    let marks_at = |column: usize| Marks {
        selected: selected.as_ref().is_some_and(|(range, _)| range.contains(&column)),
        matched: matched.iter().any(|range| range.contains(&column)),
        current: current.as_ref().is_some_and(|range| range.contains(&column)),
    };

    let mut out = Vec::new();
    let mut column = 0;
    for token in tokens {
        let mut rest = token.text.as_str();
        while !rest.is_empty() {
            let count = rest.chars().count();
            let next_cut = cuts.iter().copied().find(|cut| *cut > column).unwrap_or(usize::MAX);
            let take = (next_cut - column).min(count);
            let bytes = rest.char_indices().nth(take).map_or(rest.len(), |(at, _)| at);
            out.push(Span { class: token.class, text: rest[..bytes].to_owned(), marks: marks_at(column) });
            rest = &rest[bytes..];
            column += take;
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
        let found = find_all(&lines, "OCEAN");
        assert_eq!(found, vec![Match { line: 0, columns: 0..5 }, Match { line: 0, columns: 6..11 }]);
        assert_eq!(find_all(&lines, "aa").len(), 2, "aaaa holds two matches, not three");
        assert!(find_all(&lines, "").is_empty());
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
        let found = find_all(&lines, "sea");
        let marks = LineMarks { line: 0, selection: None, matches: &found, current: found.get(1) };
        let cut = spans(&lines[0], &marks);
        let other = Marks { matched: true, ..PLAIN };
        let current = Marks { matched: true, current: true, ..PLAIN };
        assert_eq!(texts(&cut), vec![("sea", other), (" and ", PLAIN), ("sea", current)]);
    }
}
