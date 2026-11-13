//! The texts a Model Viewer sidebar row is found by.

use wows_toolkit_viewmodel::text_filter::FilterKeyTable;

use crate::text_filter::FilterQuery;

/// A row's texts, each folded once so that a query typed only scores them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FilterFields {
    /// The texts as shown, in the order added.
    texts: Vec<String>,
    /// Row `i` is `texts[i]` folded.
    keys: FilterKeyTable,
}

impl FilterFields {
    pub fn new() -> Self {
        Self { texts: Vec::new(), keys: FilterKeyTable::new() }
    }

    /// Adds `text` as a field, unless it is empty or already one.
    pub fn push(&mut self, text: impl Into<String>) {
        let text = text.into();
        if text.is_empty() || self.texts.contains(&text) {
            return;
        }

        self.keys.push(&text);
        self.texts.push(text);
    }

    /// Adds every field of `other` that this one lacks.
    pub fn extend(&mut self, other: &FilterFields) {
        for text in &other.texts {
            self.push(text.as_str());
        }
    }

    /// Whether `query` matches a field. An inactive query matches every row,
    /// even one without fields.
    pub fn matches(&self, query: &FilterQuery) -> bool {
        !query.is_active() || (0..self.keys.len()).any(|index| query.score_at(&self.keys, index).is_some())
    }

    /// The field `query` matches best, as shown; the earliest wins a tie.
    pub fn best(&self, query: &FilterQuery) -> Option<&str> {
        let mut best = None;
        for (index, text) in self.texts.iter().enumerate() {
            let Some(score) = query.score_at(&self.keys, index) else {
                continue;
            };
            if best.is_none_or(|(_, kept)| score > kept) {
                best = Some((text.as_str(), score));
            }
        }

        best.map(|(text, _)| text)
    }

    /// The fields as shown, in the order added.
    pub fn fields(&self) -> impl Iterator<Item = &str> {
        self.texts.iter().map(String::as_str)
    }
}

impl FromIterator<String> for FilterFields {
    fn from_iter<I: IntoIterator<Item = String>>(iter: I) -> Self {
        let mut fields = Self::new();
        for text in iter {
            fields.push(text);
        }

        fields
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(texts: &[&str]) -> FilterFields {
        texts.iter().map(|text| text.to_string()).collect()
    }

    #[test]
    fn a_query_matches_any_field_folded_and_names_the_best() {
        let fields = fields(&["N\u{fc}rnberg", "PGSC106", "PGSC106_Nurnberg"]);

        assert!(fields.matches(&FilterQuery::new("nurn")));
        assert_eq!(fields.best(&FilterQuery::new("nurn")), Some("N\u{fc}rnberg"));
        assert_eq!(fields.best(&FilterQuery::new("pgsc106")), Some("PGSC106"), "the whole text beats a prefix");
        assert_eq!(fields.best(&FilterQuery::new("106 n")), Some("PGSC106_Nurnberg"));
        assert!(!fields.matches(&FilterQuery::new("bergpgsc")), "no match spans two fields");
        assert!(!fields.matches(&FilterQuery::new("yamato")));
    }

    #[test]
    fn empty_and_repeated_fields_are_not_added() {
        let fields = fields(&["Iowa", "", "Iowa", "PASB018"]);

        assert_eq!(fields.fields().collect::<Vec<_>>(), ["Iowa", "PASB018"]);
    }

    #[test]
    fn extending_adds_the_missing_fields_in_order() {
        let mut model = fields(&["AGM034_16in50_Mk7"]);
        let mount = fields(&["406 mm/50 Mk.7 in a turret", "PAGM034", "AGM034_16in50_Mk7"]);

        model.extend(&mount);

        assert_eq!(model.fields().collect::<Vec<_>>(), ["AGM034_16in50_Mk7", "406 mm/50 Mk.7 in a turret", "PAGM034"]);
        assert_eq!(model.best(&FilterQuery::new("406 mm")), Some("406 mm/50 Mk.7 in a turret"));
        assert_eq!(model.best(&FilterQuery::new("pagm")), Some("PAGM034"));
        assert_eq!(model.best(&FilterQuery::new("agm034")), Some("AGM034_16in50_Mk7"), "a run at the start wins");
    }
}
