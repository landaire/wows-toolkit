//! A localization catalog key (an `IDS_*` gettext message id), distinct from a
//! GameParams param name or an already-resolved display string.

use std::fmt;

/// A localization catalog key (an `IDS_*` gettext message id), as distinct from a
/// GameParams param name or an already-resolved display string. Resolved to a
/// translated string via [`crate::data::ResourceLoader::localized_name_from_id`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TranslationKey(String);

impl TranslationKey {
    /// Wrap a catalog key string (typically `IDS_*`). The value is taken verbatim;
    /// no case-folding or prefixing is applied (callers build the exact key).
    pub fn new(key: impl Into<String>) -> Self {
        TranslationKey(key.into())
    }

    /// The underlying key string, for catalog lookup.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TranslationKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for TranslationKey {
    fn from(value: String) -> Self {
        TranslationKey(value)
    }
}

impl From<&str> for TranslationKey {
    fn from(value: &str) -> Self {
        TranslationKey(value.to_string())
    }
}

impl AsRef<str> for TranslationKey {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::TranslationKey;

    #[test]
    fn new_and_as_str_round_trip() {
        let key = TranslationKey::new("IDS_TITLE_PXIH001");
        assert_eq!(key.as_str(), "IDS_TITLE_PXIH001");
    }

    #[test]
    fn new_accepts_owned_and_borrowed() {
        let from_owned = TranslationKey::new(String::from("IDS_A"));
        let from_borrowed = TranslationKey::new("IDS_A");
        assert_eq!(from_owned, from_borrowed);
    }

    #[test]
    fn display_writes_raw_key() {
        let key = TranslationKey::new("IDS_DOCK_CONSUME_TITLE_X");
        assert_eq!(format!("{key}"), "IDS_DOCK_CONSUME_TITLE_X");
    }

    #[test]
    fn from_string_and_str_match() {
        let a: TranslationKey = String::from("IDS_B").into();
        let b: TranslationKey = "IDS_B".into();
        assert_eq!(a, b);
    }

    #[test]
    fn as_ref_matches_as_str() {
        let key = TranslationKey::new("IDS_C");
        let as_ref: &str = key.as_ref();
        assert_eq!(as_ref, key.as_str());
    }
}
