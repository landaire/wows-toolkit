//! Name resolution shared by ships, captain skills, upgrades and consumables:
//! exact names, internal identifiers, curated and host aliases, initialisms,
//! partial names and misspellings. Every resolution that is not an exact name
//! is reported as an [`Interpretation`] so a reader can check what was meant.

use std::collections::BTreeSet;
use std::ops::Range;

use serde::Deserialize;
use serde::Serialize;

use crate::data::GameData;
use crate::fuzzy;
use crate::fuzzy::Candidate;
use crate::fuzzy::Fuzzy;

/// Most candidates an ambiguous name lists.
const CANDIDATE_LIMIT: usize = 15;

/// Words initialisms may skip: "Basics of Survivability" is "BS" as well as "BoS".
const MINOR_WORDS: &[&str] = &["a", "and", "de", "der", "for", "of", "the"];

/// What kind of thing a name was resolved to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NameKind {
    Ship,
    Skill,
    Upgrade,
    Consumable,
}

/// How a name that is not an exact English name was resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResolutionMethod {
    /// A curated or host-registered alias, e.g. "SE" or "Grosser Kurfurst".
    ExactAlias,

    /// The first letters of a multi-word name, e.g. "MBM3".
    Initialism,

    /// A former name of the same item.
    Historical,

    /// The start or a part of one name, e.g. "grober" for "Grosser Kurfurst".
    Partial,

    /// The nearest name to a misspelling.
    Fuzzy,

    /// "Super <ship>": the tier 11 ship researched from a tier 10 ship, e.g.
    /// "Super Colombo".
    Supership,
}

/// One name read as something it does not spell exactly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Interpretation {
    /// The name as given.
    pub input: String,

    /// The English name it was read as.
    pub resolved: String,

    pub kind: NameKind,
    pub method: ResolutionMethod,
}

/// The interpretations made while answering one query, in the order made.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Interpretations(Vec<Interpretation>);

impl Interpretations {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Interpretation> {
        self.0.iter()
    }

    pub(crate) fn push(&mut self, interpretation: Interpretation) {
        if !self.0.contains(&interpretation) {
            self.0.push(interpretation);
        }
    }

    pub(crate) fn append(&mut self, other: Interpretations) {
        for interpretation in other.0 {
            self.push(interpretation);
        }
    }

    pub(crate) fn note(&mut self, input: &str, resolved: &Resolved<'_>, kind: NameKind) {
        if let Some(method) = resolved.method {
            self.push(Interpretation { input: input.to_owned(), resolved: resolved.name.to_owned(), kind, method });
        }
    }
}

/// What a host-registered alias names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "name")]
pub enum AliasTarget {
    /// GameParams name or index, or English name of a ship.
    Ship(String),

    /// Internal name (e.g. "DetectionVisibilityRange") or English name of a captain skill.
    Skill(String),

    /// GameParams name or index, or English name of an upgrade.
    Upgrade(String),

    /// Consumable type (e.g. "rls"), GameParams name or English name of a consumable.
    Consumable(String),
}

impl AliasTarget {
    pub fn kind(&self) -> NameKind {
        match self {
            Self::Ship(_) => NameKind::Ship,
            Self::Skill(_) => NameKind::Skill,
            Self::Upgrade(_) => NameKind::Upgrade,
            Self::Consumable(_) => NameKind::Consumable,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Ship(name) | Self::Skill(name) | Self::Upgrade(name) | Self::Consumable(name) => name,
        }
    }
}

/// An extra name a host teaches the resolver, such as a skill's former name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Alias {
    pub name: String,
    pub target: AliasTarget,

    /// Reported in interpretations when the alias is used.
    pub method: ResolutionMethod,
}

/// Aliases [`crate::GameData::add_aliases`] could not register. The others
/// were registered.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "{} aliases name nothing in this game data: {}",
    unknown.len(),
    unknown.iter().map(|alias| format!("{:?} -> {:?}", alias.name, alias.target.name())).collect::<Vec<_>>().join(", ")
)]
pub struct AliasError {
    pub unknown: Vec<Alias>,
}

/// A ship, captain skill, upgrade or consumable named in a text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mention {
    /// The words as written.
    pub text: String,

    pub kind: NameKind,

    /// The English names the words may mean, more than one when they are
    /// ambiguous.
    pub names: Vec<String>,

    /// `None` for an exact English name or identifier.
    pub method: Option<ResolutionMethod>,
}

impl GameData {
    /// The ships, captain skills, upgrades and consumables `text` names by
    /// exact name, identifier, alias, initialism or former name, in the order
    /// written. Partial names and misspellings are not read, since ordinary
    /// words would match them. Abbreviations count only when written with
    /// capitals: "DM" names Des Moines, "dm" does not.
    pub fn mentions(&self, text: &str) -> Vec<Mention> {
        let longest = [
            self.ship_names.longest_key_words(),
            self.skill_names.longest_key_words(),
            self.upgrade_names.longest_key_words(),
            self.consumable_names.longest_key_words(),
        ]
        .into_iter()
        .fold(0, usize::max);
        let words = word_spans(text);

        // The longest name starting at each word wins over the shorter names
        // inside it.
        let mut mentions = Vec::new();
        let mut start = 0;
        while start < words.len() {
            let mut consumed = 1;
            for count in (1..=longest.min(words.len() - start)).rev() {
                let span = &text[words[start].start..words[start + count - 1].end];
                let Some(phrase) = Phrase::new(span) else {
                    continue;
                };

                let recognized = [
                    (NameKind::Ship, self.ship_names.recognize(&phrase)),
                    (NameKind::Skill, self.skill_names.recognize(&phrase)),
                    (NameKind::Upgrade, self.upgrade_names.recognize(&phrase)),
                    (NameKind::Consumable, self.consumable_names.recognize(&phrase)),
                ];
                let before = mentions.len();

                for (kind, recognized) in recognized {
                    let Some(recognized) = recognized else {
                        continue;
                    };

                    mentions.push(Mention {
                        text: span.to_owned(),
                        kind,
                        names: recognized.names.into_iter().map(str::to_owned).collect(),
                        method: recognized.method,
                    });
                }

                if mentions.len() > before {
                    consumed = count;
                    break;
                }
            }

            start += consumed;
        }

        mentions
    }
}

/// One way to name a target.
#[derive(Debug, Clone)]
struct Entry<T> {
    /// English name of the target, as resolutions report it.
    name: String,
    target: T,
    /// Search keys, the spelling of `name` first.
    keys: Vec<String>,
    initialisms: Vec<String>,
    source: Source,

    /// An alias written in capitals, such as "DM" or "BoS", which ordinary
    /// words such as "dm" do not spell.
    abbreviation: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    /// The target's English name. Secondary names lose fuzzy ties.
    Name {
        secondary: bool,
    },

    /// A GameParams name, index or internal name, matched only whole.
    Id,

    Alias(ResolutionMethod),
}

/// The outcome of looking a name up.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Lookup<'a, T> {
    Found(Resolved<'a>, Vec<T>),

    /// Several names fit equally; ask again with one of these.
    Ambiguous(Vec<String>),

    /// Nothing fits; these are near.
    NotFound(Vec<String>),
}

/// The English name a query resolved to, and how.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Resolved<'a> {
    pub(crate) name: &'a str,

    /// `None` for an exact English or internal name.
    pub(crate) method: Option<ResolutionMethod>,
}

/// The English names a whole span of text spells, and how.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Recognized<'a> {
    /// More than one when the span is ambiguous.
    pub(crate) names: Vec<&'a str>,

    /// `None` for an exact English or internal name.
    pub(crate) method: Option<ResolutionMethod>,
}

/// A span of text as [`NameIndex::recognize`] compares it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Phrase {
    key: String,

    /// The span lowercased whole, compared with identifiers.
    id: String,

    /// Two or more capitals, as in "DM" or "BoS". Initialisms and abbreviated
    /// aliases need them, so the word "dm" is not "DM".
    capitals: bool,

    /// Capitals, a digit or an underscore, as in "PASC020". Identifiers need
    /// them, so the word "fighter" is not the consumable type "fighter".
    identifier: bool,
}

impl Phrase {
    /// `None` when `span` has no letters or digits.
    pub(crate) fn new(span: &str) -> Option<Self> {
        let key = search_key(span);
        if key.is_empty() {
            return None;
        }

        let capitals = capital_count(span) >= 2;
        let identifier = capitals || span.contains(|character: char| character.is_ascii_digit() || character == '_');

        Some(Self { key, id: span.trim().to_lowercase(), capitals, identifier })
    }
}

/// Names of one kind of thing, each pointing at a target such as an index
/// into the indexed ships.
#[derive(Debug, Clone)]
pub(crate) struct NameIndex<T> {
    entries: Vec<Entry<T>>,
}

impl<T> Default for NameIndex<T> {
    fn default() -> Self {
        Self { entries: Vec::new() }
    }
}

impl<T: Clone + PartialEq> NameIndex<T> {
    pub(crate) fn add_name(&mut self, name: &str, target: T, secondary: bool) {
        let keys = key_variants(name);
        let initialisms = keys.iter().flat_map(|key| initialisms(key)).collect();
        self.entries.push(Entry {
            name: name.to_owned(),
            target,
            keys,
            initialisms,
            source: Source::Name { secondary },
            abbreviation: false,
        });
    }

    /// An identifier matched only whole and case-insensitively, e.g. "PASC020".
    pub(crate) fn add_id(&mut self, id: &str, name: &str, target: T) {
        self.entries.push(Entry {
            name: name.to_owned(),
            target,
            keys: vec![id.trim().to_lowercase()],
            initialisms: Vec::new(),
            source: Source::Id,
            abbreviation: false,
        });
    }

    /// Registers `alias` for every target named `target`, an English name,
    /// identifier or search key. False when nothing has that name.
    pub(crate) fn add_alias(&mut self, alias: &str, target: &str, method: ResolutionMethod) -> bool {
        let id = target.trim().to_lowercase();
        let key = search_key(target);
        let mut named: Vec<(String, T, bool)> = Vec::new();
        for entry in &self.entries {
            let matches = match entry.source {
                Source::Id => entry.keys[0] == id,
                Source::Name { .. } => entry.keys.contains(&key),
                Source::Alias(_) => false,
            };
            if matches && !named.iter().any(|(_, target, _)| *target == entry.target) {
                let secondary = matches!(entry.source, Source::Name { secondary: true });
                named.push((entry.name.clone(), entry.target.clone(), secondary));
            }
        }
        // Aliases name the regular ship, not its event and test copies.
        if named.iter().any(|(_, _, secondary)| !secondary) {
            named.retain(|(_, _, secondary)| !secondary);
        }
        if named.is_empty() {
            return false;
        }

        let keys = key_variants(alias);
        let initialisms: Vec<String> = keys.iter().flat_map(|key| initialisms(key)).collect();
        let abbreviation = !alias.contains(' ') && capital_count(alias) >= 2;
        for (name, target, _) in named {
            self.entries.push(Entry {
                name,
                target,
                keys: keys.clone(),
                initialisms: initialisms.clone(),
                source: Source::Alias(method),
                abbreviation,
            });
        }
        true
    }

    /// Every target whose English name is `name`.
    pub(crate) fn targets(&self, name: &str) -> Vec<T> {
        let mut targets: Vec<T> = Vec::new();
        for entry in &self.entries {
            if matches!(entry.source, Source::Name { .. }) && entry.name == name && !targets.contains(&entry.target) {
                targets.push(entry.target.clone());
            }
        }
        targets
    }

    /// The names `phrase` spells whole: an exact name or identifier, an alias
    /// or an initialism. Partial names and misspellings are not read, since
    /// ordinary words would match them.
    pub(crate) fn recognize(&self, phrase: &Phrase) -> Option<Recognized<'_>> {
        let key = &phrase.key;

        let named: Vec<&Entry<T>> = self
            .entries
            .iter()
            .filter(|entry| matches!(entry.source, Source::Name { .. }) && entry.keys.contains(key))
            .collect();
        if !named.is_empty() {
            let method = (!named.iter().any(|entry| entry.keys[0] == *key)).then_some(ResolutionMethod::ExactAlias);
            return Some(Recognized { names: primary_names(&named), method });
        }

        if phrase.identifier
            && let Some(entry) =
                self.entries.iter().find(|entry| entry.source == Source::Id && entry.keys[0] == phrase.id)
        {
            return Some(Recognized { names: vec![entry.name.as_str()], method: None });
        }

        let aliased: Vec<(&Entry<T>, ResolutionMethod)> = self
            .entries
            .iter()
            .filter_map(|entry| match entry.source {
                Source::Alias(method) if entry.keys.contains(key) && (phrase.capitals || !entry.abbreviation) => {
                    Some((entry, method))
                }
                _ => None,
            })
            .collect();
        if let Some(&(_, method)) = aliased.first() {
            let entries: Vec<&Entry<T>> = aliased.iter().map(|(entry, _)| *entry).collect();
            return Some(Recognized { names: distinct_names(&entries), method: Some(method) });
        }

        if !phrase.capitals || !is_initialism_query(key) {
            return None;
        }

        let initialism: Vec<&Entry<T>> =
            self.entries.iter().filter(|entry| entry.source != Source::Id && entry.initialisms.contains(key)).collect();
        let first = initialism.first()?;
        let method = match first.source {
            Source::Alias(ResolutionMethod::Historical) => ResolutionMethod::Historical,
            _ => ResolutionMethod::Initialism,
        };

        Some(Recognized { names: primary_names(&initialism), method: Some(method) })
    }

    /// Words in the longest search key.
    pub(crate) fn longest_key_words(&self) -> usize {
        self.entries.iter().flat_map(|entry| entry.keys.iter()).map(|key| key.split(' ').count()).max().unwrap_or(0)
    }

    /// Resolves `query`: exact English name, identifier, alias, initialism,
    /// start or part of a name, then the nearest misspelling. An initialism
    /// shared by several names gives way only to a partial match among them.
    pub(crate) fn lookup(&self, query: &str) -> Lookup<'_, T> {
        let key = search_key(query);
        if key.is_empty() {
            return Lookup::NotFound(Vec::new());
        }

        // Names that differ only in punctuation, such as "[Conqueror]" and
        // "Conqueror", share a search key; the exact name tells them apart.
        let trimmed = query.trim();
        if let Some(entry) = self
            .entries
            .iter()
            .find(|entry| matches!(entry.source, Source::Name { .. }) && entry.name.eq_ignore_ascii_case(trimmed))
        {
            return self.found(entry, None);
        }

        let exact: Vec<&Entry<T>> = self
            .entries
            .iter()
            .filter(|entry| matches!(entry.source, Source::Name { .. }) && entry.keys.contains(&key))
            .collect();
        if let Some(found) = self.single(&exact, |entry| (entry.keys[0] != key).then_some(ResolutionMethod::ExactAlias))
        {
            return found;
        }

        let id = trimmed.to_lowercase();
        if let Some(entry) = self.entries.iter().find(|entry| entry.source == Source::Id && entry.keys[0] == id) {
            return Lookup::Found(Resolved { name: &entry.name, method: None }, vec![entry.target.clone()]);
        }

        let aliased: Vec<&Entry<T>> = self
            .entries
            .iter()
            .filter(|entry| matches!(entry.source, Source::Alias(_)) && entry.keys.contains(&key))
            .collect();
        if let Some(found) = self.single(&aliased, |entry| match entry.source {
            Source::Alias(method) => Some(method),
            _ => None,
        }) {
            return found;
        }

        let named = || self.entries.iter().filter(|entry| entry.source != Source::Id);
        let historical_or = |method: ResolutionMethod| {
            move |entry: &Entry<T>| match entry.source {
                Source::Alias(ResolutionMethod::Historical) => Some(ResolutionMethod::Historical),
                _ => Some(method),
            }
        };

        let initialism: Vec<&Entry<T>> = if is_initialism_query(&key) {
            named().filter(|entry| entry.initialisms.contains(&key)).collect()
        } else {
            Vec::new()
        };
        let shared_initialism = match self.single(&initialism, historical_or(ResolutionMethod::Initialism)) {
            Some(found @ Lookup::Found(..)) => return found,
            other => other,
        };

        // "SE" read as an initialism names Steady Eye or Sharp Edge; "Base
        // Expert" merely contains it.
        let fits_initialism =
            |entry: &&Entry<T>| shared_initialism.is_none() || initialism.iter().any(|fits| fits.target == entry.target);
        let prefix: Vec<&Entry<T>> = named()
            .filter(|entry| entry.keys.iter().any(|name| name.starts_with(key.as_str())))
            .filter(fits_initialism)
            .collect();
        let partial: Vec<&Entry<T>> = if prefix.is_empty() {
            named()
                .filter(|entry| entry.keys.iter().any(|name| name.contains(key.as_str())))
                .filter(fits_initialism)
                .collect()
        } else {
            prefix
        };
        if let Some(found) = self.single(&partial, historical_or(ResolutionMethod::Partial)) {
            return found;
        }
        if let Some(ambiguous) = shared_initialism {
            return ambiguous;
        }

        let candidates = |with_historical: bool| {
            named()
                .filter(move |entry| with_historical || entry.source != Source::Alias(ResolutionMethod::Historical))
                .flat_map(|entry| {
                    let secondary = matches!(entry.source, Source::Name { secondary: true });
                    entry.keys.iter().map(move |key| Candidate { key, name: &entry.name, secondary })
                })
        };
        match fuzzy::closest(&key, candidates(true)) {
            Fuzzy::Unique(name) => {
                let Some(entry) = named().find(|entry| entry.name == name) else {
                    return Lookup::NotFound(Vec::new());
                };

                // A name near only through a former name's spelling was meant
                // by that former name.
                let method = match fuzzy::closest(&key, candidates(false)) {
                    Fuzzy::Unique(current) if current == name => ResolutionMethod::Fuzzy,
                    _ => ResolutionMethod::Historical,
                };
                self.found(entry, Some(method))
            }
            Fuzzy::Suggestions(names) => Lookup::NotFound(names.into_iter().map(str::to_owned).collect()),
        }
    }

    /// The match when `matches` name one target name or one target, else
    /// ambiguity, or `None` when empty.
    fn single<'a>(
        &'a self,
        matches: &[&'a Entry<T>],
        method: impl Fn(&Entry<T>) -> Option<ResolutionMethod>,
    ) -> Option<Lookup<'a, T>> {
        let first = matches.first()?;
        let one_target = matches.iter().all(|entry| entry.target == first.target);
        let primary: Vec<&Entry<T>> =
            matches.iter().copied().filter(|entry| !matches!(entry.source, Source::Name { secondary: true })).collect();
        // Event and test copies share a spelling with the ship they copy:
        // "[Conqueror]" is "conqueror" like "Conqueror".
        let copies_only = primary.first().is_some_and(|head| {
            distinct_names(&primary).len() == 1 && matches.iter().all(|entry| entry.keys[0] == head.keys[0])
        });
        match distinct_names(matches).as_slice() {
            [_] => Some(self.found(first, method(first))),
            _ if one_target => Some(self.found(first, method(first))),
            _ if copies_only => Some(self.found(primary[0], method(primary[0]))),
            names => {
                Some(Lookup::Ambiguous(names.iter().take(CANDIDATE_LIMIT).map(|name| (*name).to_owned()).collect()))
            }
        }
    }

    fn found<'a>(&'a self, entry: &'a Entry<T>, method: Option<ResolutionMethod>) -> Lookup<'a, T> {
        Lookup::Found(Resolved { name: &entry.name, method }, self.targets(&entry.name))
    }
}

fn distinct_names<'a, T>(entries: &[&'a Entry<T>]) -> Vec<&'a str> {
    entries.iter().map(|entry| entry.name.as_str()).collect::<BTreeSet<_>>().into_iter().collect()
}

/// Distinct names, leaving out event and test copies when the regular item
/// is among them.
fn primary_names<'a, T>(entries: &[&'a Entry<T>]) -> Vec<&'a str> {
    let primary: Vec<&Entry<T>> =
        entries.iter().copied().filter(|entry| !matches!(entry.source, Source::Name { secondary: true })).collect();
    match primary.is_empty() {
        true => distinct_names(entries),
        false => distinct_names(&primary),
    }
}

fn capital_count(text: &str) -> usize {
    text.chars().filter(char::is_ascii_uppercase).count()
}

/// Byte ranges of the runs of letters and digits in `text`.
fn word_spans(text: &str) -> Vec<Range<usize>> {
    let mut spans = Vec::new();
    let mut start = None;
    for (offset, character) in text.char_indices() {
        match (character.is_alphanumeric(), start) {
            (true, None) => start = Some(offset),
            (false, Some(begin)) => {
                spans.push(begin..offset);
                start = None;
            }
            // Inside a word or between words.
            _ => {}
        }
    }
    if let Some(begin) = start {
        spans.push(begin..text.len());
    }

    spans
}

/// Initialisms are two to five letters or digits without spaces.
fn is_initialism_query(key: &str) -> bool {
    (2..=5).contains(&key.len()) && !key.contains(' ')
}

/// Lowercase ASCII with punctuation folded to single spaces, so names with
/// diacritics match their plain spelling.
pub(crate) fn search_key(name: &str) -> String {
    let ascii = deunicode::deunicode(name).to_lowercase();

    ascii
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Search keys of `name`. A German sharp s is typed "ss", or as "b" by
/// readers who take it for one, or left out: "Grosser", "Grober", "Groer".
fn key_variants(name: &str) -> Vec<String> {
    let mut keys = vec![search_key(name)];
    if name.contains(['\u{df}', '\u{1e9e}']) {
        for replacement in ["b", ""] {
            let key = search_key(&name.replace(['\u{df}', '\u{1e9e}'], replacement));
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
    }
    keys
}

/// First letters of each word of a multi-word key, with and without minor
/// words. Words of digits are kept whole: "main battery modification 3" is
/// "mbm3".
fn initialisms(key: &str) -> Vec<String> {
    let words: Vec<&str> = key.split(' ').collect();
    if words.len() < 2 {
        return Vec::new();
    }

    let initial = |word: &&str| -> String {
        if word.bytes().all(|byte| byte.is_ascii_digit()) { (*word).to_owned() } else { word[..1].to_owned() }
    };
    let all: String = words.iter().map(initial).collect();
    let major: String = words.iter().filter(|word| !MINOR_WORDS.contains(word)).map(initial).collect();

    let mut out = vec![all];
    if major.len() >= 2 && !out.contains(&major) {
        out.push(major);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(names: &[&'static str]) -> NameIndex<&'static str> {
        let mut index = NameIndex::default();
        for name in names {
            index.add_name(name, *name, false);
        }
        index
    }

    fn found<'a>(lookup: Lookup<'a, &'static str>) -> (&'a str, Option<ResolutionMethod>) {
        match lookup {
            Lookup::Found(resolved, _) => (resolved.name, resolved.method),
            other => panic!("expected a match, got {other:?}"),
        }
    }

    #[test]
    fn search_key_folds_diacritics_and_punctuation() {
        assert_eq!(search_key("Gro\u{df}er Kurf\u{fc}rst"), "grosser kurfurst");
        assert_eq!(search_key("ARP My\u{14d}k\u{14d}"), "arp myoko");
        assert_eq!(search_key("Des-Moines "), "des moines");
    }

    #[test]
    fn sharp_s_is_typed_three_ways() {
        assert_eq!(
            key_variants("Gro\u{df}er Kurf\u{fc}rst"),
            ["grosser kurfurst", "grober kurfurst", "groer kurfurst"]
        );
        assert_eq!(key_variants("Des Moines"), ["des moines"]);
    }

    #[test]
    fn initialisms_keep_digit_words_and_drop_minor_words() {
        assert_eq!(initialisms("main battery modification 3"), ["mbm3"]);
        assert_eq!(initialisms("basics of survivability"), ["bos", "bs"]);
        assert_eq!(initialisms("yamato"), Vec::<String>::new());
    }

    #[test]
    fn exact_names_need_no_interpretation() {
        let names = index(&["Des Moines", "Salem"]);

        assert_eq!(found(names.lookup("des moines")), ("Des Moines", None));
    }

    #[test]
    fn unique_initialism_resolves_and_shared_one_lists_candidates() {
        let names = index(&["Survivability Expert", "Superintendent", "Concealment Expert", "Consumable Enhancements"]);

        assert_eq!(found(names.lookup("SE")), ("Survivability Expert", Some(ResolutionMethod::Initialism)));
        assert_eq!(
            names.lookup("CE"),
            Lookup::Ambiguous(vec!["Concealment Expert".to_owned(), "Consumable Enhancements".to_owned()])
        );
    }

    #[test]
    fn aliases_win_over_initialisms() {
        let mut names = index(&["Concealment Expert", "Consumable Enhancements"]);
        assert!(names.add_alias("CE", "Concealment Expert", ResolutionMethod::ExactAlias));
        assert!(!names.add_alias("XX", "Nothing", ResolutionMethod::ExactAlias));

        assert_eq!(found(names.lookup("ce")), ("Concealment Expert", Some(ResolutionMethod::ExactAlias)));
    }

    #[test]
    fn sharp_s_spellings_resolve_through_aliases() {
        let mut names = index(&["G. Kurf\u{fc}rst", "Grosseto"]);
        assert!(names.add_alias("Gro\u{df}er Kurf\u{fc}rst", "G. Kurf\u{fc}rst", ResolutionMethod::ExactAlias));

        assert_eq!(found(names.lookup("grober")), ("G. Kurf\u{fc}rst", Some(ResolutionMethod::Partial)));
        assert_eq!(found(names.lookup("Grober Kurfurst")), ("G. Kurf\u{fc}rst", Some(ResolutionMethod::ExactAlias)));
        assert_eq!(found(names.lookup("gk")), ("G. Kurf\u{fc}rst", Some(ResolutionMethod::Initialism)));
    }

    #[test]
    fn identifiers_match_only_whole() {
        let mut names = index(&["Des Moines"]);
        names.add_id("PASC020", "Des Moines", "Des Moines");

        assert_eq!(found(names.lookup("pasc020")), ("Des Moines", None));
        assert_eq!(names.lookup("pasc"), Lookup::NotFound(Vec::new()));
    }

    #[test]
    fn shared_initialism_ignores_names_that_merely_contain_it() {
        let names = index(&["Steady Eye", "Sharp Edge", "Base Expert"]);

        assert_eq!(names.lookup("SE"), Lookup::Ambiguous(vec!["Sharp Edge".to_owned(), "Steady Eye".to_owned()]));
    }

    #[test]
    fn shared_initialism_ignores_unrelated_names_starting_with_it() {
        let names = index(&["Concealment Expert", "Close Escort", "Centurion"]);
        let without = index(&["Concealment Expert", "Close Escort"]);

        assert_eq!(names.lookup("CE"), without.lookup("CE"));
        assert_eq!(found(names.lookup("CE")), ("Concealment Expert", Some(ResolutionMethod::Partial)));
    }

    #[test]
    fn exact_name_selects_a_copy_sharing_its_search_key() {
        let mut names = NameIndex::default();
        names.add_name("Conqueror", "released", false);
        names.add_name("[Conqueror]", "test", true);

        assert_eq!(
            names.lookup("[Conqueror]"),
            Lookup::Found(Resolved { name: "[Conqueror]", method: None }, vec!["test"])
        );
        assert_eq!(
            names.lookup("conqueror"),
            Lookup::Found(Resolved { name: "Conqueror", method: None }, vec!["released"])
        );
    }

    #[test]
    fn former_names_matched_in_part_or_misspelled_are_historical() {
        let mut names = index(&["Gun Feeder", "Basics of Survivability"]);
        assert!(names.add_alias("Basic Firing Training", "Gun Feeder", ResolutionMethod::Historical));

        assert_eq!(found(names.lookup("Basic Firing")), ("Gun Feeder", Some(ResolutionMethod::Historical)));
        assert_eq!(found(names.lookup("Basic Firing Trainng")), ("Gun Feeder", Some(ResolutionMethod::Historical)));
        assert_eq!(found(names.lookup("Gun Feder")), ("Gun Feeder", Some(ResolutionMethod::Fuzzy)));
    }

    fn recognized(names: &NameIndex<&'static str>, span: &str) -> Option<(Vec<String>, Option<ResolutionMethod>)> {
        let phrase = Phrase::new(span).expect("span has words");

        names
            .recognize(&phrase)
            .map(|recognized| (recognized.names.into_iter().map(str::to_owned).collect(), recognized.method))
    }

    #[test]
    fn recognize_reads_whole_names_only() {
        let mut names = index(&["Des Moines", "Survivability Expert", "Gun Feeder", "Catapult Fighter"]);
        names.add_id("PASC020", "Des Moines", "Des Moines");
        names.add_id("fighter", "Catapult Fighter", "Catapult Fighter");
        assert!(names.add_alias("DM", "Des Moines", ResolutionMethod::ExactAlias));
        assert!(names.add_alias("Basic Firing Training", "Gun Feeder", ResolutionMethod::Historical));
        let one = |name: &str, method| Some((vec![name.to_owned()], method));

        assert_eq!(recognized(&names, "des moines"), one("Des Moines", None));
        assert_eq!(recognized(&names, "pasc020"), one("Des Moines", None));
        assert_eq!(recognized(&names, "DM"), one("Des Moines", Some(ResolutionMethod::ExactAlias)));
        assert_eq!(recognized(&names, "SE"), one("Survivability Expert", Some(ResolutionMethod::Initialism)));
        assert_eq!(recognized(&names, "basic firing training"), one("Gun Feeder", Some(ResolutionMethod::Historical)));

        // Ordinary words that spell an abbreviation, an identifier, a partial
        // name and a misspelling.
        assert_eq!(recognized(&names, "dm"), None);
        assert_eq!(recognized(&names, "se"), None);
        assert_eq!(recognized(&names, "fighter"), None);
        assert_eq!(recognized(&names, "moines"), None);
        assert_eq!(recognized(&names, "Des Moins"), None);
    }

    #[test]
    fn recognize_prefers_the_regular_ship_to_its_copies() {
        let mut names = NameIndex::default();
        names.add_name("Conqueror", "released", false);
        names.add_name("[Conqueror]", "test", true);
        names.add_name("[Neptunus]", "test only", true);

        let names_of =
            |span| names.recognize(&Phrase::new(span).expect("span has words")).map(|recognized| recognized.names);

        assert_eq!(names_of("conqueror"), Some(vec!["Conqueror"]));
        assert_eq!(names_of("Neptunus"), Some(vec!["[Neptunus]"]));
    }

    #[test]
    fn word_spans_split_on_punctuation() {
        let text = "G. Kurf\u{fc}rst's AP?";
        let words: Vec<&str> = word_spans(text).into_iter().map(|span| &text[span]).collect();

        assert_eq!(words, ["G", "Kurf\u{fc}rst", "s", "AP"]);
    }
}
