//! Reading the tracker's saved view, including the one-time import of the
//! legacy keys it was kept under.

use serde::Deserialize;
use sqlx::SqlitePool;
use wows_toolkit_config::queries;

use crate::player_tracker::ClanSort;
use crate::player_tracker::ClanSortColumn;
use crate::player_tracker::Sort;
use crate::player_tracker::SortColumn;
use crate::player_tracker::SortOrder;
use crate::player_tracker::TRACKER_SETTINGS_KEY;
use crate::player_tracker::TimePeriod;
use crate::player_tracker::TrackerSettings;

/// Legacy tracker keys, one setting apiece.
pub const EGUI_SORT_ORDER_KEY: &str = "player_tracker.sort_order";
pub const EGUI_CLAN_SORT_ORDER_KEY: &str = "player_tracker.clan_sort_order";
pub const EGUI_PLAYER_FILTER_KEY: &str = "player_tracker.player_filter";
pub const EGUI_TIME_PERIOD_KEY: &str = "player_tracker.filter_time_period";

#[derive(Debug, thiserror::Error)]
pub enum LoadViewError {
    #[error("the saved tracker view could not be read")]
    Read(#[source] queries::SettingError),
    #[error("the imported tracker view could not be stored")]
    Store(#[source] sqlx::Error),
}

/// The serialized shape of the legacy sort direction.
#[derive(Debug, Clone, Copy, Deserialize)]
enum EguiOrder {
    Asc,
    Desc,
}

impl From<EguiOrder> for SortOrder {
    fn from(order: EguiOrder) -> Self {
        match order {
            EguiOrder::Asc => SortOrder::Ascending,
            EguiOrder::Desc => SortOrder::Descending,
        }
    }
}

/// The serialized shape of the legacy player table sort.
#[derive(Debug, Clone, Copy, Deserialize)]
enum EguiSort {
    Name(EguiOrder),
    Clan(EguiOrder),
    LastEncountered(EguiOrder),
    TimesEncountered(EguiOrder),
    TimesEncounteredInTimeRange(EguiOrder),
}

impl From<EguiSort> for Sort {
    fn from(sort: EguiSort) -> Self {
        let (column, order) = match sort {
            EguiSort::Name(order) => (SortColumn::Name, order),
            EguiSort::Clan(order) => (SortColumn::Clan, order),
            EguiSort::LastEncountered(order) => (SortColumn::LastEncountered, order),
            EguiSort::TimesEncountered(order) => (SortColumn::TotalEncounters, order),
            EguiSort::TimesEncounteredInTimeRange(order) => (SortColumn::Encounters, order),
        };

        Self { column, order: order.into() }
    }
}

/// The serialized shape of the legacy clans table sort.
#[derive(Debug, Clone, Copy, Deserialize)]
enum EguiClanSort {
    Clan(EguiOrder),
    Members(EguiOrder),
    Matches(EguiOrder),
    MatchesInRange(EguiOrder),
    Sightings(EguiOrder),
    LastSeen(EguiOrder),
}

impl From<EguiClanSort> for ClanSort {
    fn from(sort: EguiClanSort) -> Self {
        let (column, order) = match sort {
            EguiClanSort::Clan(order) => (ClanSortColumn::Clan, order),
            EguiClanSort::Members(order) => (ClanSortColumn::Members, order),
            EguiClanSort::Matches(order) => (ClanSortColumn::Encounters, order),
            EguiClanSort::MatchesInRange(order) => (ClanSortColumn::EncountersInRange, order),
            EguiClanSort::Sightings(order) => (ClanSortColumn::Sightings, order),
            EguiClanSort::LastSeen(order) => (ClanSortColumn::LastEncountered, order),
        };

        Self { column, order: order.into() }
    }
}

/// The tracker's saved view, or `None` when nothing was saved.
///
/// Without a saved view, the legacy keys are read and the view built from
/// them is stored, so later launches read the stored view and never the
/// legacy keys again.
pub async fn load(pool: &SqlitePool) -> Result<Option<TrackerSettings>, LoadViewError> {
    let saved =
        queries::try_get_setting::<TrackerSettings>(pool, TRACKER_SETTINGS_KEY).await.map_err(LoadViewError::Read)?;
    if saved.is_some() {
        return Ok(saved);
    }

    // An unreadable legacy key costs that field only.
    let sort = queries::get_setting::<EguiSort>(pool, EGUI_SORT_ORDER_KEY).await;
    let clan_sort = queries::get_setting::<EguiClanSort>(pool, EGUI_CLAN_SORT_ORDER_KEY).await;
    let filter = queries::get_setting::<String>(pool, EGUI_PLAYER_FILTER_KEY).await;
    let period = queries::get_setting::<TimePeriod>(pool, EGUI_TIME_PERIOD_KEY).await;
    if sort.is_none() && clan_sort.is_none() && filter.is_none() && period.is_none() {
        return Ok(None);
    }

    // An absent legacy key stands for the default view. The legacy keys have no Twitch chat
    // toggle, so the import leaves it off.
    let settings = TrackerSettings {
        period: period.unwrap_or_default(),
        sort: sort.map(Sort::from).unwrap_or_default(),
        clan_sort: clan_sort.map(ClanSort::from).unwrap_or_default(),
        filter: filter.unwrap_or_default(),
        seen_in_twitch_chat: false,
    };
    // A view saved while this ran wins over the import.
    let inserted =
        queries::insert_setting_if_absent(pool, TRACKER_SETTINGS_KEY, &settings).await.map_err(LoadViewError::Store)?;
    if inserted {
        return Ok(Some(settings));
    }

    queries::try_get_setting::<TrackerSettings>(pool, TRACKER_SETTINGS_KEY).await.map_err(LoadViewError::Read)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn egui_keys_are_imported_once() {
        let pool = wows_toolkit_config::test_pool().await;
        queries::set_setting_raw(&pool, EGUI_SORT_ORDER_KEY, r#"{"TimesEncountered":"Asc"}"#).await.unwrap();
        queries::set_setting_raw(&pool, EGUI_CLAN_SORT_ORDER_KEY, r#"{"LastSeen":"Desc"}"#).await.unwrap();
        queries::set_setting_raw(&pool, EGUI_PLAYER_FILTER_KEY, r#""RAIN""#).await.unwrap();
        queries::set_setting_raw(&pool, EGUI_TIME_PERIOD_KEY, r#""LastWeek""#).await.unwrap();

        let imported = load(&pool).await.unwrap();
        let expected = TrackerSettings {
            period: TimePeriod::LastWeek,
            sort: Sort { column: SortColumn::TotalEncounters, order: SortOrder::Ascending },
            clan_sort: ClanSort { column: ClanSortColumn::LastEncountered, order: SortOrder::Descending },
            filter: "RAIN".to_owned(),
            seen_in_twitch_chat: false,
        };
        assert_eq!(imported, Some(expected.clone()));

        queries::set_setting_raw(&pool, EGUI_PLAYER_FILTER_KEY, r#""OTHER""#).await.unwrap();
        assert_eq!(load(&pool).await.unwrap(), Some(expected), "a later egui write is not imported again");
    }

    #[tokio::test]
    async fn a_saved_view_wins_over_egui_keys() {
        let pool = wows_toolkit_config::test_pool().await;
        let saved = TrackerSettings { period: TimePeriod::AllTime, ..TrackerSettings::default() };
        queries::set_setting(&pool, TRACKER_SETTINGS_KEY, &saved).await.unwrap();
        queries::set_setting_raw(&pool, EGUI_TIME_PERIOD_KEY, r#""LastHour""#).await.unwrap();

        assert_eq!(load(&pool).await.unwrap(), Some(saved));
    }

    #[tokio::test]
    async fn a_view_saved_without_the_twitch_toggle_reads_with_it_off() {
        let pool = wows_toolkit_config::test_pool().await;
        queries::set_setting_raw(&pool, TRACKER_SETTINGS_KEY, r#"{"filter":"RAIN"}"#).await.unwrap();

        let view = load(&pool).await.unwrap().expect("the saved view reads");
        assert_eq!(view.filter, "RAIN");
        assert!(!view.seen_in_twitch_chat);
    }

    #[tokio::test]
    async fn a_database_without_any_view_has_none() {
        let pool = wows_toolkit_config::test_pool().await;

        assert_eq!(load(&pool).await.unwrap(), None);
    }
}
