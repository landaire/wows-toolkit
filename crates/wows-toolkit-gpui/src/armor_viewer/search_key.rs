//! The text a Model Viewer sidebar row is found by.

/// A row's searchable fields, each ASCII-folded and lowercased once so a
/// search runs only substring tests.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchKey {
    /// Every folded field joined by a newline. No query holds one, so a
    /// single substring test covers every field and no match spans two.
    folded: String,
    fields: Vec<SearchField>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SearchField {
    /// The field as shown.
    text: String,
    /// Where its folded text ends in [`SearchKey::folded`].
    end: usize,
}

impl SearchKey {
    pub fn new() -> Self {
        Self { folded: String::new(), fields: Vec::new() }
    }

    /// Adds `text` as a field, unless it is empty or already one.
    pub fn push(&mut self, text: impl Into<String>) {
        let text = text.into();
        if text.is_empty() || self.has_field(&text) {
            return;
        }

        let folded = fold(&text);
        self.push_folded(text, &folded);
    }

    /// Adds every field of `other` that this key lacks.
    pub fn extend(&mut self, other: &SearchKey) {
        let mut start = 0;
        for field in &other.fields {
            let folded = &other.folded[start..field.end];
            start = field.end + 1;
            if !self.has_field(&field.text) {
                self.push_folded(field.text.clone(), folded);
            }
        }
    }

    /// Whether a field holds `query`, which is folded already.
    pub fn matches(&self, query: &str) -> bool {
        self.folded.contains(query)
    }

    /// The first field holding `query`, as shown.
    pub fn matched_field(&self, query: &str) -> Option<&str> {
        let start = self.folded.find(query)?;

        self.fields.iter().find(|field| start < field.end).map(|field| field.text.as_str())
    }

    /// The fields as shown, in the order added.
    pub fn fields(&self) -> impl Iterator<Item = &str> {
        self.fields.iter().map(|field| field.text.as_str())
    }

    fn has_field(&self, text: &str) -> bool {
        self.fields.iter().any(|field| field.text == text)
    }

    fn push_folded(&mut self, text: String, folded: &str) {
        if !self.fields.is_empty() {
            self.folded.push('\n');
        }
        self.folded.push_str(folded);
        self.fields.push(SearchField { text, end: self.folded.len() });
    }
}

impl FromIterator<String> for SearchKey {
    fn from_iter<I: IntoIterator<Item = String>>(iter: I) -> Self {
        let mut key = Self::new();
        for text in iter {
            key.push(text);
        }

        key
    }
}

/// `text` as a search compares it: ASCII-folded, lowercased and without
/// line breaks.
pub fn fold(text: &str) -> String {
    unidecode::unidecode(text).to_lowercase().replace('\n', " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(fields: &[&str]) -> SearchKey {
        fields.iter().map(|field| field.to_string()).collect()
    }

    #[test]
    fn a_query_matches_any_field_folded_and_names_the_field() {
        let key = key(&["N\u{fc}rnberg", "PGSC106", "PGSC106_Nurnberg"]);

        assert!(key.matches("nurn"));
        assert_eq!(key.matched_field("nurn"), Some("N\u{fc}rnberg"));
        assert_eq!(key.matched_field("sc106"), Some("PGSC106"));
        assert_eq!(key.matched_field("106_n"), Some("PGSC106_Nurnberg"));
        assert!(!key.matches("bergpgsc"), "no match spans two fields");
        assert!(!key.matches("yamato"));
    }

    #[test]
    fn empty_and_repeated_fields_are_not_added() {
        let key = key(&["Iowa", "", "Iowa", "PASB018"]);

        assert_eq!(key.fields().collect::<Vec<_>>(), ["Iowa", "PASB018"]);
    }

    #[test]
    fn extending_a_key_keeps_each_fields_folded_text() {
        let mut model = key(&["AGM034_16in50_Mk7"]);
        let mount = key(&["406 mm/50 Mk.7 in a turret", "PAGM034", "AGM034_16in50_Mk7"]);

        model.extend(&mount);

        assert_eq!(model.fields().collect::<Vec<_>>(), ["AGM034_16in50_Mk7", "406 mm/50 Mk.7 in a turret", "PAGM034"]);
        assert_eq!(model.matched_field("406 mm"), Some("406 mm/50 Mk.7 in a turret"));
        assert_eq!(model.matched_field("pagm"), Some("PAGM034"));
        assert_eq!(model.matched_field("mk7"), Some("AGM034_16in50_Mk7"));
    }
}
