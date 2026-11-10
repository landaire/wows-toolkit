//! The tracker's reads and writes against the shared database.
//!
//! Each request opens the database on a runtime of its own, as the other
//! services of this crate do, and runs on a blocking thread.

use std::collections::HashMap;
use std::collections::HashSet;

use jiff::Timestamp;
use sqlx::SqlitePool;
use wows_replays::types::AccountId;
use wows_toolkit_config::index::query;
use wows_toolkit_config::index::rows::ClanCorrection;
use wows_toolkit_config::index::rows::MatchFilter;
use wows_toolkit_config::index::rows::PlayerFacet;
use wows_toolkit_viewmodel::player_tracker::TRACKER_SETTINGS_KEY;
use wows_toolkit_viewmodel::player_tracker::TimePeriod;
use wows_toolkit_viewmodel::player_tracker::TrackerSettings;
use wows_toolkit_viewmodel::player_tracker::store;
use wows_toolkit_viewmodel::player_tracker::tracked::Pending;
use wows_toolkit_viewmodel::player_tracker::tracked::TrackedPlayer;
use wows_toolkit_viewmodel::player_tracker::tracked::ViewModes;

use super::error::TrackerError;

/// What the tracker reads when it opens.
pub struct Opened {
    pub settings: TrackerSettings,
    pub modes: ViewModes,
    pub tracked: HashMap<AccountId, TrackedPlayer>,
}

/// What the clans table reads from the index, all time.
pub struct ClanInputs {
    pub latest: HashMap<AccountId, String>,
    pub corrections: Vec<ClanCorrection>,
}

/// Runs `work` against the shared database.
pub fn run<T>(work: impl AsyncFnOnce(&SqlitePool) -> Result<T, TrackerError>) -> Result<T, TrackerError> {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(TrackerError::Runtime)?;

    runtime.block_on(async {
        let pool = wows_toolkit_config::open_db().await.map_err(TrackerError::Open)?;
        work(&pool).await
    })
}

/// The saved view, the Current Match modes, and the tracked players.
pub async fn open(pool: &SqlitePool) -> Result<Opened, TrackerError> {
    // A database written before the tracker had tables still holds it as one
    // settings blob; whichever app opens first imports it.
    if let Err(err) = store::import_blob_once(pool).await {
        log::warn!("player tracker: the stored blob was not imported: {err}");
    }

    let settings = wows_toolkit_config::queries::try_get_setting::<TrackerSettings>(pool, TRACKER_SETTINGS_KEY)
        .await
        .map_err(|source| TrackerError::ReadSetting { key: TRACKER_SETTINGS_KEY, source })?
        // No row is the view the tracker opens on the first time.
        .unwrap_or_default();
    let modes = store::load_view_modes(pool).await;
    let tracked = store::load(pool).await.map_err(TrackerError::LoadTracked)?;

    Ok(Opened { settings, modes, tracked })
}

pub async fn tracked_players(pool: &SqlitePool) -> Result<HashMap<AccountId, TrackedPlayer>, TrackerError> {
    store::load(pool).await.map_err(TrackerError::LoadTracked)
}

/// Everyone the index met in `period`.
pub async fn index_players(
    pool: &SqlitePool,
    period: TimePeriod,
    now: Timestamp,
) -> Result<Vec<PlayerFacet>, TrackerError> {
    let filter = period.match_filter(now);
    let players = query::distinct_players(pool, &filter).await.map_err(TrackerError::Index)?;

    // The perspective player of a replay is not someone you met, and left in
    // they lead the table, since they were in every battle their replay
    // recorded.
    let own: HashSet<AccountId> = query::self_account_ids(pool, &filter).await.map_err(TrackerError::Index)?;

    Ok(players.into_iter().filter(|facet| !own.contains(&facet.account_id)).collect())
}

/// The latest clan per account and the per-encounter corrections, scoped to
/// the live source so a directory opened once does not cost every later read.
pub async fn clan_inputs(pool: &SqlitePool) -> Result<ClanInputs, TrackerError> {
    let source = query::live_source_id(pool).await.map_err(TrackerError::Index)?;
    let filter = MatchFilter { source_ids: source.map(|source| vec![source]), ..Default::default() };
    let latest = query::distinct_players(pool, &filter).await.map_err(TrackerError::Index)?;
    let corrections = query::clan_history_corrections(pool, &filter).await.map_err(TrackerError::Index)?;

    Ok(ClanInputs { latest: latest.into_iter().map(|facet| (facet.account_id, facet.clan)).collect(), corrections })
}

pub async fn save_settings(pool: &SqlitePool, settings: &TrackerSettings) -> Result<(), TrackerError> {
    wows_toolkit_config::queries::set_setting(pool, TRACKER_SETTINGS_KEY, settings)
        .await
        .map_err(|source| TrackerError::WriteSetting { key: TRACKER_SETTINGS_KEY, source })
}

pub async fn save_view_modes(pool: &SqlitePool, modes: ViewModes) -> Result<(), TrackerError> {
    store::store_view_modes(pool, modes).await.map_err(TrackerError::WriteViewModes)
}

/// Writes one account's note, which is all a note edit changes.
pub async fn save_note(pool: &SqlitePool, account: AccountId, player: TrackedPlayer) -> Result<(), TrackerError> {
    let mut pending = Pending::default();
    pending.note_changed(account);
    let players: HashMap<AccountId, TrackedPlayer> = [(account, player)].into_iter().collect();

    store::save(pool, &pending, &players).await.map_err(TrackerError::SaveTracked)
}

pub async fn clear_tracked(pool: &SqlitePool) -> Result<(), TrackerError> {
    wows_toolkit_config::tracker::clear_tracker(pool).await.map_err(TrackerError::Clear)
}

/// Which of `names` the index has met, keyed by lower-cased name.
pub async fn accounts_named(pool: &SqlitePool, names: &[String]) -> Result<HashMap<String, AccountId>, TrackerError> {
    query::accounts_named(pool, names).await.map_err(TrackerError::Index)
}

/// Twitch chat logins seen in `[start, end]`, to the whole second.
pub async fn chat_observations(
    pool: &SqlitePool,
    start: Timestamp,
    end: Timestamp,
) -> Result<Vec<(String, Timestamp)>, TrackerError> {
    let rows =
        query::observations_in_window(pool, start.as_second(), end.as_second()).await.map_err(TrackerError::Index)?;

    // Every row lies between two valid timestamps, so none is dropped here.
    Ok(rows
        .into_iter()
        .filter_map(|(login, seen_at)| Timestamp::from_second(seen_at).ok().map(|at| (login, at)))
        .collect())
}
