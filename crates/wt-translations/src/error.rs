/// A locale code that is not in [`SUPPORTED_LANGUAGES`](crate::SUPPORTED_LANGUAGES).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unsupported locale {code:?}")]
pub struct UnknownLocale {
    pub code: String,
}
