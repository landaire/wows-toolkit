//! Typed-text filtering shared by every filtered list in the app.
//!
//! Text and query are both folded the same way: each character is
//! transliterated to ASCII (`unidecode`), lowered, and everything that is
//! not a letter or a digit is dropped. "Smaland" finds "Smaland" with a ring
//! on its a, and "bow patch" finds "Bow_patch".
//!
//! A text matches when the folded query is, in order of preference:
//! 1. a run in it (a run starting a word scores higher than one inside a
//!    word, and one starting the text higher still),
//! 2. letters each starting a word or following the one before ("arm op"
//!    finds "Armor Opacity"),
//! 3. letters in order, each word they fall in entered at its start but
//!    letters inside a word skipped freely ("shrnhst" finds "Scharnhorst"),
//!    scored down for every letter between them. Entering a word anywhere
//!    but its start is left out: in a long text nearly any short query is in
//!    order somewhere, and a list that keeps its order cannot rank that away.
//!
//! The matched letters map back to whole characters of the original text,
//! which is what a row draws marked. Ranking applies to flat lists; a tree
//! keeps its order and only drops what does not match.

use std::ops::Range;

/// A query as typed, folded once for scoring many texts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FilterQuery {
    folded: Box<[u8]>,
}

/// A text folded once, to be scored against each query typed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FilterKey {
    letters: Box<[Letter]>,
}

/// The keys of a long list folded into one buffer, so that a list of
/// hundreds of thousands of rows costs no allocation a row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilterKeyTable {
    letters: Vec<Letter>,
    /// Where each row's letters end in `letters`.
    ends: Vec<usize>,
}

/// How well a text matched a query; higher is better.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MatchScore(u32);

/// A folded letter: a lowered ASCII letter or digit, with the top bit set
/// when it starts a word. Folded letters are ASCII, so the bit is free, and
/// a key costs one byte a letter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Letter(u8);

/// Which folded letters of a text a query took.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Alignment {
    Run { start: usize, len: usize },
    Letters(Vec<usize>),
}

/// A text folded with where each folded letter came from.
struct Folded {
    letters: Vec<Letter>,
    origins: Vec<Range<usize>>,
}

impl FilterQuery {
    pub fn new(query: &str) -> Self {
        let mut folded = Vec::with_capacity(query.len());
        fold(query, |byte, _word_start, _origin| folded.push(byte));
        Self { folded: folded.into_boxed_slice() }
    }

    /// Whether the query holds a letter or a digit. Without one it keeps
    /// every text and marks nothing.
    pub fn is_active(&self) -> bool {
        !self.folded.is_empty()
    }

    /// The score of `key`, or `None` when it does not match. An inactive
    /// query matches every key with the same score.
    pub fn score(&self, key: &FilterKey) -> Option<MatchScore> {
        align(&self.folded, &key.letters).map(|(score, _)| score)
    }

    /// The score of row `index` of `table`, as [`Self::score`] scores a key.
    /// Panics past the table's last row.
    pub fn score_at(&self, table: &FilterKeyTable, index: usize) -> Option<MatchScore> {
        align(&self.folded, table.letters(index)).map(|(score, _)| score)
    }

    /// The best score among `keys`, with the position of the key that has it.
    /// The first key wins a tie.
    pub fn best<'a>(&self, keys: impl IntoIterator<Item = &'a FilterKey>) -> Option<(usize, MatchScore)> {
        let mut best: Option<(usize, MatchScore)> = None;
        for (position, key) in keys.into_iter().enumerate() {
            let Some(score) = self.score(key) else { continue };

            if best.is_none_or(|(_, kept)| score > kept) {
                best = Some((position, score));
            }
        }

        best
    }

    /// Whether `text` matches, folding it now. For texts scored once; a list
    /// scored on every keystroke keeps a [`FilterKey`] per row.
    pub fn keeps(&self, text: &str) -> bool {
        self.ranges(text).is_some()
    }

    /// The byte ranges of `text` the query matched, merged where they touch.
    /// Letters matched one after the other are marked as one range with the
    /// spaces or punctuation between them. Empty for an inactive query;
    /// `None` when `text` does not match.
    pub fn ranges(&self, text: &str) -> Option<Vec<Range<usize>>> {
        if !self.is_active() {
            return Some(Vec::new());
        }

        let folded = Folded::new(text);
        let (_, alignment) = align(&self.folded, &folded.letters)?;
        let positions: Vec<usize> = match alignment {
            Alignment::Run { start, len } => (start..start + len).collect(),
            Alignment::Letters(letters) => letters,
        };

        let mut ranges: Vec<Range<usize>> = Vec::new();
        let mut previous_position: Option<usize> = None;
        for position in positions {
            let matched = folded.origins[position].clone();
            let follows = previous_position.is_some_and(|previous| previous + 1 == position);
            match ranges.last_mut() {
                Some(previous) if follows || previous.end >= matched.start => {
                    previous.end = previous.end.max(matched.end);
                }
                _ => ranges.push(matched),
            }
            previous_position = Some(position);
        }

        Some(ranges)
    }

    /// The items whose key matches. An inactive query keeps every item in
    /// its order; an active one orders them best first, ties in their order.
    pub fn rank<T>(&self, items: impl IntoIterator<Item = T>, key: impl Fn(&T) -> &FilterKey) -> Vec<T> {
        if !self.is_active() {
            return items.into_iter().collect();
        }

        let mut scored: Vec<(MatchScore, T)> =
            items.into_iter().filter_map(|item| Some((self.score(key(&item))?, item))).collect();
        scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));

        scored.into_iter().map(|(_, item)| item).collect()
    }
}

impl FilterKey {
    pub fn new(text: &str) -> Self {
        let mut letters = Vec::with_capacity(text.len());
        fold(text, |byte, word_start, _origin| letters.push(Letter::new(byte, word_start)));

        Self { letters: letters.into_boxed_slice() }
    }
}

impl FilterKeyTable {
    pub fn new() -> Self {
        Self { letters: Vec::new(), ends: Vec::new() }
    }

    /// Folds `text` as the next row's key.
    pub fn push(&mut self, text: &str) {
        fold(text, |byte, word_start, _origin| self.letters.push(Letter::new(byte, word_start)));
        self.ends.push(self.letters.len());
    }

    pub fn len(&self) -> usize {
        self.ends.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ends.is_empty()
    }

    /// The folded letters of row `index`. Panics past the last row, as
    /// slice indexing does.
    fn letters(&self, index: usize) -> &[Letter] {
        let start = index.checked_sub(1).map_or(0, |previous| self.ends[previous]);
        &self.letters[start..self.ends[index]]
    }
}

impl Default for FilterKeyTable {
    fn default() -> Self {
        Self::new()
    }
}

impl Letter {
    const WORD_START: u8 = 0x80;

    fn new(byte: u8, word_start: bool) -> Self {
        Self(if word_start { byte | Self::WORD_START } else { byte })
    }

    fn byte(self) -> u8 {
        self.0 & !Self::WORD_START
    }

    fn starts_word(self) -> bool {
        self.0 & Self::WORD_START != 0
    }
}

impl Folded {
    fn new(text: &str) -> Self {
        let mut folded = Self { letters: Vec::new(), origins: Vec::new() };
        fold(text, |byte, word_start, origin| {
            folded.letters.push(Letter::new(byte, word_start));
            folded.origins.push(origin);
        });

        folded
    }
}

/// Feeds `emit` each folded letter of `text`: the lowered ASCII letter or
/// digit, whether it starts a word, and the byte range of the character it
/// came from. Other characters split words and are dropped.
fn fold(text: &str, mut emit: impl FnMut(u8, bool, Range<usize>)) {
    let mut previous_alphanumeric = false;
    let mut previous_alphabetic = false;
    let mut previous_lowercase = false;
    for (start, c) in text.char_indices() {
        let origin = start..start + c.len_utf8();
        let ascii = if c.is_ascii() { &text[origin.clone()] } else { unidecode::unidecode_char(c) };
        // "MidFront" starts a word at the capital F.
        let camel = previous_lowercase && c.is_uppercase();

        let mut first = true;
        for byte in ascii.bytes() {
            if !byte.is_ascii_alphanumeric() {
                previous_alphanumeric = false;
                previous_alphabetic = false;
                continue;
            }

            let word_start =
                !previous_alphanumeric || (first && camel) || (byte.is_ascii_digit() && previous_alphabetic);
            emit(byte.to_ascii_lowercase(), word_start, origin.clone());
            previous_alphanumeric = true;
            previous_alphabetic = byte.is_ascii_alphabetic();
            first = false;
        }

        // A character that folds to nothing, such as an emoji, splits words
        // as punctuation does.
        if first {
            previous_alphanumeric = false;
            previous_alphabetic = false;
        }

        previous_lowercase = c.is_lowercase();
    }
}

/// The best placement of `query` in `text`, both folded, and its score.
fn align(query: &[u8], text: &[Letter]) -> Option<(MatchScore, Alignment)> {
    const RUN: u32 = 3_000_000;
    const ANCHORED: u32 = 2_000_000;
    const SCATTERED: u32 = 1_000_000;

    if query.is_empty() {
        return Some((MatchScore(0), Alignment::Letters(Vec::new())));
    }

    if query.len() > text.len() {
        return None;
    }

    let runs = text
        .windows(query.len())
        .enumerate()
        .filter(|(_, window)| window.iter().zip(query).all(|(letter, &byte)| letter.byte() == byte))
        .map(|(start, _)| start);
    let mut first_run = None;
    let mut word_run = None;
    for start in runs {
        first_run.get_or_insert(start);
        if text[start].starts_word() {
            word_run = Some(start);
            break;
        }
    }
    if let Some(start) = word_run.or(first_run) {
        let mut bonus = 0;
        if start == 0 && query.len() == text.len() {
            bonus += 400_000;
        }
        if start == 0 {
            bonus += 200_000;
        } else if text[start].starts_word() {
            bonus += 100_000;
        }
        // No term for the text's length: equal matches keep the list's own
        // order, which a curated list (completions, commands) chose.
        let score = RUN + bonus - capped(start, 255) * 256;
        return Some((MatchScore(score), Alignment::Run { start, len: query.len() }));
    }

    if !in_order(query, text) {
        return None;
    }

    let cells = query.len() * text.len();
    if let Some(letters) = with_fits(cells, |fits| anchored(query, text, fits)) {
        let runs = run_count(&letters);
        let score = ANCHORED + 500_000 - capped(runs, 99) * 2_000 - capped(letters[0], 255) * 4;
        return Some((MatchScore(score), Alignment::Letters(letters)));
    }

    let letters = with_fits(cells, |fits| word_local(query, text, fits))?;
    let gaps = letters[letters.len() - 1] + 1 - letters[0] - query.len();
    let word_starts = letters.iter().filter(|&&position| text[position].starts_word()).count();
    let score = SCATTERED + 500_000 + capped(word_starts, 50) * 5_000
        - capped(gaps, 200) * 1_000
        - capped(run_count(&letters), 99) * 2_000;

    Some((MatchScore(score), Alignment::Letters(letters)))
}

fn capped(value: usize, cap: u32) -> u32 {
    u32::try_from(value).map_or(cap, |value| value.min(cap))
}

/// How many separate runs `letters`, ascending positions, fall in.
fn run_count(letters: &[usize]) -> usize {
    1 + letters.windows(2).filter(|pair| pair[1] != pair[0] + 1).count()
}

/// Whether `query` is in `text` in order: the cheap test that turns away
/// most texts before either placement is tried.
fn in_order(query: &[u8], text: &[Letter]) -> bool {
    let mut wanted = query.iter().peekable();
    for letter in text {
        if wanted.next_if_eq(&&letter.byte()).is_some() && wanted.peek().is_none() {
            return true;
        }
    }

    false
}

/// Runs `body` on a placement table of `cells` cells, kept between calls so
/// that scoring a long list allocates it once per thread rather than once
/// per row. The cells hold whatever the last call left; `anchored` and
/// `word_local` write every cell before reading it.
fn with_fits<R>(cells: usize, body: impl FnOnce(&mut [bool]) -> R) -> R {
    thread_local! {
        static FITS: std::cell::RefCell<Vec<bool>> = const { std::cell::RefCell::new(Vec::new()) };
    }

    FITS.with(|fits| {
        let mut fits = fits.borrow_mut();
        if fits.len() < cells {
            fits.resize(cells, false);
        }
        body(&mut fits[..cells])
    })
}

/// The leftmost positions in `text` that take `query` in order, each either
/// on a word start or in the same word as the one before it. Decided
/// backward as [`anchored`] is.
fn word_local(query: &[u8], text: &[Letter], fits: &mut [bool]) -> Option<Vec<usize>> {
    let width = text.len();
    let at = |letter: usize, position: usize| letter * width + position;

    for (letter, &byte) in query.iter().enumerate().rev() {
        let next = letter + 1;
        // Whether a place past `position` fits the next letter, on a word
        // start, or anywhere up to the end of `position`'s word.
        let mut start_ahead = false;
        let mut word_ahead = false;
        for position in (0..width).rev() {
            let rest_fits = next == query.len() || start_ahead || word_ahead;
            fits[at(letter, position)] = text[position].byte() == byte && rest_fits;
            if next < query.len() {
                let next_fits = fits[at(next, position)];
                start_ahead |= text[position].starts_word() && next_fits;
                // Places left of a word start are in an earlier word.
                word_ahead = !text[position].starts_word() && (word_ahead || next_fits);
            }
        }
    }

    let mut picked = Vec::with_capacity(query.len());
    let mut previous: Option<usize> = None;
    for letter in 0..query.len() {
        let from = previous.map_or(0, |previous| previous + 1);
        let mut left_word = false;
        let position = (from..width).find(|&position| {
            left_word |= text[position].starts_word();
            let reachable = text[position].starts_word() || (previous.is_some() && !left_word);
            reachable && fits[at(letter, position)]
        })?;
        picked.push(position);
        previous = Some(position);
    }

    Some(picked)
}

/// The leftmost positions in `text` that take `query` in order, each on a
/// word start or right after the one before it.
///
/// Decided backward over every (letter, position) pair once, rather than by
/// trying placements forward and backing out of them, which costs time
/// exponential in the query's length on texts of many short words.
fn anchored(query: &[u8], text: &[Letter], fits: &mut [bool]) -> Option<Vec<usize>> {
    let width = text.len();
    let at = |letter: usize, position: usize| letter * width + position;

    // `fits[at(letter, position)]`: `query[letter..]` can be placed with
    // `query[letter]` at `position`.
    // Every cell is written before it is read.
    for (letter, &byte) in query.iter().enumerate().rev() {
        let next = letter + 1;
        // Whether a word start past `position` fits the next letter.
        let mut start_ahead = false;
        for position in (0..width).rev() {
            let rest_fits =
                next == query.len() || start_ahead || (position + 1 < width && fits[at(next, position + 1)]);
            fits[at(letter, position)] = text[position].byte() == byte && rest_fits;
            if next < query.len() {
                start_ahead |= text[position].starts_word() && fits[at(next, position)];
            }
        }
    }

    let mut picked = Vec::with_capacity(query.len());
    let mut previous: Option<usize> = None;
    for letter in 0..query.len() {
        let from = previous.map_or(0, |previous| previous + 1);
        let position = (from..width).find(|&position| {
            let reachable = text[position].starts_word() || previous.is_some_and(|previous| previous + 1 == position);
            reachable && fits[at(letter, position)]
        })?;
        picked.push(position);
        previous = Some(position);
    }

    Some(picked)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn score(query: &str, text: &str) -> Option<MatchScore> {
        FilterQuery::new(query).score(&FilterKey::new(text))
    }

    fn ranges(query: &str, text: &str) -> Option<Vec<Range<usize>>> {
        FilterQuery::new(query).ranges(text)
    }

    #[test]
    fn accents_and_ligatures_fold_to_ascii() {
        assert!(score("Smaland", "Sm\u{e5}land").is_some());
        assert!(score("Sodermanland", "S\u{f6}dermanland").is_some());
        assert!(score("aegir", "\u{c6}gir").is_some());
        assert!(score("strasse", "Stra\u{df}e").is_some());
        assert!(score("sm\u{e5}", "Smaland").is_some());
    }

    #[test]
    fn spaces_and_punctuation_are_ignored() {
        assert!(score("bow patch", "Bow_patch_MidFrontShape").is_some());
        assert!(score("Hai-Tun", "hai tun").is_some());
        assert!(score("St. Louis", "St Louis").is_some());
    }

    #[test]
    fn a_query_without_letters_keeps_everything() {
        let query = FilterQuery::new("  -_ ");
        assert!(!query.is_active());
        assert_eq!(query.ranges("Roll"), Some(Vec::new()));
        assert!(query.score(&FilterKey::new("Roll")).is_some());
    }

    #[test]
    fn a_run_always_matches() {
        assert!(score("tch", "Bow_patch").is_some());
        assert!(score("idfro", "Bow_patch_MidFrontShape").is_some());
    }

    #[test]
    fn letters_in_order_match_and_others_do_not() {
        assert!(score("shrnhst", "Scharnhorst").is_some());
        assert_eq!(score("yamatox", "Yamato"), None);
        assert_eq!(score("otamay", "Yamato"), None);
    }

    #[test]
    fn scattered_letters_enter_each_word_at_its_start() {
        assert!(score("sodrmnlnd", "S\u{f6}dermanland").is_some());
        assert!(score("hwrk", "Hull Works").is_some());
        assert_eq!(score("opac", "Bow_patch_MidFrontShape"), None);
        assert_eq!(score("rnhrst", "Scharnhorst"), None);
        assert_eq!(ranges("shrnhst", "Scharnhorst"), Some(vec![0..1, 2..3, 4..7, 9..11]));
    }

    #[test]
    fn a_word_start_scores_above_the_middle_of_a_word() {
        assert!(score("rom", "Roma Exterior") > score("rom", "Chromed"));
        assert!(score("kid", "Kidd") > score("kid", "Skidder"));
        assert!(score("front", "MidFront") > score("front", "Affronted"));
    }

    #[test]
    fn a_run_scores_above_scattered_letters() {
        assert!(score("yama", "Yamato") > score("yama", "Yahagi Arm Mast A"));
        assert!(score("arm", "Armor") > score("arm", "Anti Rust Mesh"));
        assert!(score("arm op", "Armor Opacity") > score("arm op", "a ram oops"));
    }

    #[test]
    fn the_whole_text_scores_above_its_start() {
        assert!(score("Yamato", "Yamato") > score("Yamato", "Yamato Kai"));
        assert!(score("Yam", "Yamato") > score("Yam", "Old Yamato"));
    }

    #[test]
    fn rank_orders_best_first_and_keeps_ties_in_order() {
        let texts = ["Chromed", "Roma", "Old Roman", "Rome"];
        let keys: Vec<(&str, FilterKey)> = texts.iter().map(|text| (*text, FilterKey::new(text))).collect();
        let ranked: Vec<&str> =
            FilterQuery::new("rom").rank(&keys, |(_, key)| key).iter().map(|(text, _)| *text).collect();
        assert_eq!(ranked, ["Roma", "Rome", "Old Roman", "Chromed"]);

        let unfiltered: Vec<&str> =
            FilterQuery::new("").rank(&keys, |(_, key)| key).iter().map(|(text, _)| *text).collect();
        assert_eq!(unfiltered, texts);
    }

    #[test]
    fn a_key_table_row_scores_as_its_own_key_does() {
        let texts = ["content/a.xml", "Sm\u{e5}land/b.dds", "", "gui/c.png"];
        let mut table = FilterKeyTable::new();
        for text in texts {
            table.push(text);
        }
        assert_eq!(table.len(), 4);

        for query in ["xml", "smaland", "gc", "zzz", ""] {
            let query = FilterQuery::new(query);
            for (index, text) in texts.iter().enumerate() {
                assert_eq!(query.score_at(&table, index), query.score(&FilterKey::new(text)), "{query:?} on {text}");
            }
        }
    }

    #[test]
    fn a_character_that_folds_to_nothing_splits_words() {
        // An emoji is past unidecode's table and folds to nothing.
        assert!(score("bar", "A\u{1f600}bar") > score("bar", "Abar"));
    }

    #[test]
    fn best_names_the_key_that_matched_best() {
        let keys = [FilterKey::new("PJSB018"), FilterKey::new("Yamato"), FilterKey::new("Old Yamato")];
        assert_eq!(FilterQuery::new("yamato").best(&keys).map(|(position, _)| position), Some(1));
        assert_eq!(FilterQuery::new("zzz").best(&keys), None);
    }

    #[test]
    fn a_run_marks_one_span() {
        assert_eq!(ranges("OPAC", "Armor Opacity"), Some(std::iter::once(6..10).collect()));
    }

    #[test]
    fn word_start_letters_mark_each_run() {
        assert_eq!(ranges("arm op", "Armor Opacity"), Some(vec![0..3, 6..8]));
        assert_eq!(ranges("mfs", "Bow_patch_MidFrontShape"), Some(vec![10..11, 13..14, 18..19]));
    }

    #[test]
    fn spans_cover_whole_characters_outside_ascii() {
        // "Smaland" with a ring a: two bytes at 2..4.
        assert_eq!(ranges("smal", "Sm\u{e5}land"), Some(std::iter::once(0..5).collect()));
        assert_eq!(ranges("aland", "Sm\u{e5}land"), Some(std::iter::once(2..8).collect()));
        // An AE ligature folds to two letters; either marks all of it.
        assert_eq!(ranges("e", "\u{c6}gir"), Some(std::iter::once(0..2).collect()));
        assert_eq!(ranges("aeg", "\u{c6}gir"), Some(std::iter::once(0..3).collect()));
        // A sharp s folds to "ss".
        assert_eq!(ranges("asse", "Stra\u{df}e"), Some(std::iter::once(3..7).collect()));
    }

    #[test]
    fn a_run_marks_the_punctuation_inside_it() {
        assert_eq!(ranges("bowpa", "Bow_patch"), Some(std::iter::once(0..6).collect()));
        assert_eq!(ranges("raw xp", "Show Raw XP"), Some(std::iter::once(5..11).collect()));
    }

    #[test]
    fn spans_fall_on_character_boundaries_in_cyrillic() {
        // "Yamato" in Cyrillic, two bytes a letter, folds to "iamato".
        let text = "\u{42f}\u{43c}\u{430}\u{442}\u{43e}";
        assert_eq!(ranges("mato", text), Some(std::iter::once(2..10).collect()));
    }

    /// Many one-letter words and a query that almost fits them: placing
    /// letters forward and backing out takes on the order of 10^17 tries here.
    #[test]
    fn a_query_that_almost_fits_many_short_words_is_refused_quickly() {
        let text = vec!["s"; 60].join(" ");
        let query = format!("{}x", "s".repeat(30));
        assert_eq!(ranges(&query, &text), None);
    }
}
