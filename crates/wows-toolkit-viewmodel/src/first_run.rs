//! What a first run takes from the machine when nothing is saved for it yet.

use sqlx::SqlitePool;
use wows_toolkit_config::queries;

use crate::settings::keys;

#[derive(Debug, thiserror::Error)]
pub enum FirstRunError {
    #[error("the setting `{key}` could not be read")]
    Read {
        key: &'static str,
        #[source]
        source: queries::SettingError,
    },
    #[error("the setting `{key}` could not be written")]
    Write {
        key: &'static str,
        #[source]
        source: sqlx::Error,
    },
}

/// Saves the language the machine reads in as the reader's, so the language question offers it.
///
/// Nothing is saved once a language is saved or the question was answered, or when no catalogue
/// matches `system_locale`. Returns the language saved.
pub async fn adopt_system_locale(
    pool: &SqlitePool,
    system_locale: Option<&str>,
) -> Result<Option<&'static str>, FirstRunError> {
    let read = |key| move |source| FirstRunError::Read { key, source };
    let answered = queries::try_get_setting::<bool>(pool, keys::LANGUAGE_SELECTION_SHOWN)
        .await
        .map_err(read(keys::LANGUAGE_SELECTION_SHOWN))?;
    let saved =
        queries::try_get_setting::<Option<String>>(pool, keys::LOCALE).await.map_err(read(keys::LOCALE))?.flatten();
    if answered == Some(true) || saved.is_some() {
        return Ok(None);
    }

    let Some(code) = system_locale.and_then(wt_translations::system_locale_to_wows) else { return Ok(None) };
    queries::set_setting(pool, keys::LOCALE, &code)
        .await
        .map_err(|source| FirstRunError::Write { key: keys::LOCALE, source })?;

    Ok(Some(code))
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn saved_locale(pool: &SqlitePool) -> Option<String> {
        queries::try_get_setting::<Option<String>>(pool, keys::LOCALE).await.unwrap().flatten()
    }

    #[tokio::test]
    async fn a_first_run_saves_the_machines_language() {
        let pool = wows_toolkit_config::test_pool().await;

        assert_eq!(adopt_system_locale(&pool, Some("de-DE")).await.unwrap(), Some("de"));
        assert_eq!(saved_locale(&pool).await.as_deref(), Some("de"));
    }

    #[tokio::test]
    async fn a_saved_language_is_kept() {
        let pool = wows_toolkit_config::test_pool().await;
        queries::set_setting(&pool, keys::LOCALE, &"ja").await.unwrap();

        assert_eq!(adopt_system_locale(&pool, Some("de-DE")).await.unwrap(), None);
        assert_eq!(saved_locale(&pool).await.as_deref(), Some("ja"));
    }

    #[tokio::test]
    async fn an_answered_question_is_not_reopened() {
        let pool = wows_toolkit_config::test_pool().await;
        queries::set_setting(&pool, keys::LANGUAGE_SELECTION_SHOWN, &true).await.unwrap();

        assert_eq!(adopt_system_locale(&pool, Some("de-DE")).await.unwrap(), None);
        assert_eq!(saved_locale(&pool).await, None);
    }

    #[tokio::test]
    async fn a_language_without_a_catalogue_saves_nothing() {
        let pool = wows_toolkit_config::test_pool().await;

        assert_eq!(adopt_system_locale(&pool, Some("xx-YY")).await.unwrap(), None);
        assert_eq!(adopt_system_locale(&pool, None).await.unwrap(), None);
        assert_eq!(saved_locale(&pool).await, None);
    }
}
