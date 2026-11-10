//! The Player Tracker tab's services: everyone met in indexed battles over a
//! chosen window of time, the clans they played for, the notes kept against
//! them, and the battle in progress.
//!
//! The rules are the viewmodel's, shared with the GPUI front end; this holds
//! what the tab has read and words it for the TSX view.

mod database;
mod error;
mod live;
mod state;
mod view;

use std::sync::LazyLock;
use std::sync::Mutex;
use std::sync::MutexGuard;

use jiff::Timestamp;
use nsh_core::prelude::*;
use wows_replays::types::AccountId;
use wows_toolkit_config::index::query_text;
use wows_toolkit_viewmodel::query_bar::seed;

use self::error::TrackerError;
use self::state::LoadState;
use self::state::Tracker;
use self::view::ClanColumn;
use self::view::ClansTable;
use self::view::LiveView;
use self::view::Period;
use self::view::PlayerColumn;
use self::view::PlayersTable;
use self::view::Scope;
use self::view::TrackerControls;
use self::view::TrackerLabels;
use self::view::ViewMode;

/// What the tab opens on.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct TrackerOpened {
    pub labels: TrackerLabels,
    pub controls: TrackerControls,
}

static TRACKER: LazyLock<Mutex<Tracker>> = LazyLock::new(|| Mutex::new(Tracker::new()));

/// Held while a settings row is written, so two quick edits land in order:
/// each write takes the view as it stands once it holds this.
static SETTINGS_WRITE: Mutex<()> = Mutex::new(());

fn tracker() -> MutexGuard<'static, Tracker> {
    // A request that panicked leaves the tracker as it last wrote it, which is
    // still a consistent view to draw.
    TRACKER.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Player tracker services exposed to the TSX interface.
#[module]
pub struct PlayerTracker;

#[module]
impl PlayerTracker {
    /// Reads the saved view, the Current Match modes and the tracked players,
    /// in the reader's `locale`.
    pub async fn open(locale: String) -> Result<TrackerOpened, String> {
        run(move || open(&locale)).await
    }

    /// Reads the index for the current period, and the tracked players again.
    pub async fn refresh() -> Result<TrackerControls, String> {
        run(refresh).await
    }

    /// The players table, filtered and sorted.
    pub async fn players() -> Result<PlayersTable, String> {
        run(|| Ok(tracker().players_table(Timestamp::now()))).await
    }

    /// The clans table, filtered and sorted.
    pub async fn clans() -> Result<ClansTable, String> {
        run(|| Ok(tracker().clans_table(Timestamp::now()))).await
    }

    /// Reads the clans table's own index aggregates, unless they are read
    /// already for this refresh. They are whole-index scans, so they wait
    /// until the table is shown.
    pub async fn load_clans() -> Result<(), String> {
        run(load_clans).await
    }

    /// Looks back over `period`, which is another index read.
    pub async fn set_period(period: Period) -> Result<TrackerControls, String> {
        run(move || set_period(period)).await
    }

    pub async fn set_filter(filter: String) -> Result<TrackerControls, String> {
        run(move || {
            change_settings(|tracker| tracker.settings.filter = filter)?;
            Ok(tracker().controls())
        })
        .await
    }

    /// Orders the players table by `column`, in the shared three-state cycle.
    pub async fn sort_players(column: PlayerColumn) -> Result<TrackerControls, String> {
        run(move || {
            change_settings(|tracker| tracker.settings.sort = tracker.settings.sort.toggled(column.into()))?;
            Ok(tracker().controls())
        })
        .await
    }

    pub async fn sort_clans(column: ClanColumn) -> Result<TrackerControls, String> {
        run(move || {
            change_settings(|tracker| {
                tracker.settings.clan_sort = tracker.settings.clan_sort.toggled(column.into());
            })?;
            Ok(tracker().controls())
        })
        .await
    }

    /// Whether battles the reader arranged count. Stored where the other
    /// front ends keep it.
    pub async fn set_show_division_mates(show: bool) -> Result<TrackerControls, String> {
        run(move || {
            change_view_modes(|tracker| tracker.modes.show_division_mates = show)?;
            Ok(tracker().controls())
        })
        .await
    }

    pub async fn set_view_mode(mode: ViewMode) -> Result<TrackerControls, String> {
        run(move || {
            change_view_modes(|tracker| tracker.modes.current_match_view_mode = mode.into())?;
            Ok(tracker().controls())
        })
        .await
    }

    pub async fn set_scope(scope: Scope) -> Result<TrackerControls, String> {
        run(move || {
            change_view_modes(|tracker| tracker.modes.win_rate_mode = scope.into())?;
            Ok(tracker().controls())
        })
        .await
    }

    /// Keeps `note` against the account, in the rows the other front ends
    /// read. A failed write is reported in the controls' `noteError`.
    pub async fn save_note(account_id: String, note: String) -> Result<TrackerControls, String> {
        run(move || save_note(&account_id, note)).await
    }

    /// Forgets every tracked player. The rows go once the delete has landed.
    pub async fn clear() -> Result<(), String> {
        run(clear).await
    }

    /// The Search query for every match `account_id` was in.
    pub fn player_matches_query(account_id: String) -> Result<String, String> {
        let account = parse_account(&account_id).map_err(|err| err.report())?;
        Ok(query_text::print_query(&seed::matches_with_player(account)))
    }

    /// The Search query for every match mentioning `clan`. The index has no
    /// clan-only field, so a tag inside a name matches too.
    pub fn clan_matches_query(clan: String) -> String {
        query_text::print_query(&seed::matches_mentioning_clan(&clan))
    }

    /// Watches the replays directory of the WoWs directory the settings
    /// hold, restarting the watch when that has changed.
    pub async fn sync_live() -> Result<(), String> {
        run(|| {
            let wows_dir = database::run(async |pool| database::wows_dir(pool).await)?;
            live::watch(&wows_dir);
            Ok(())
        })
        .await
    }

    /// Bumped whenever the Current Match section has something new to show.
    pub fn live_revision() -> u32 {
        live::revision()
    }

    /// The Current Match section.
    pub async fn live_view() -> Result<LiveView, String> {
        run(|| Ok(live::view(&tracker(), Timestamp::now()))).await
    }
}

/// Runs `work` on a blocking thread, reporting its error as the view shows it.
async fn run<T>(work: impl FnOnce() -> Result<T, TrackerError> + Send + 'static) -> Result<T, String>
where
    T: Send + 'static,
{
    match blocking(work).await {
        Ok(result) => result.map_err(|err| err.report()),
        Err(err) => Err(TrackerError::Task(err).report()),
    }
}

fn open(locale: &str) -> Result<TrackerOpened, TrackerError> {
    // The chip hover is worded by the viewmodel, in its own locale.
    wows_toolkit_viewmodel::set_locale(locale);
    let opened = database::run(async |pool| database::open(pool).await)?;

    let mut tracker = tracker();
    *tracker = Tracker::new();
    tracker.settings = opened.settings;
    tracker.modes = opened.modes;
    tracker.tracked = opened.tracked;

    Ok(TrackerOpened { labels: tracker.labels(), controls: tracker.controls() })
}

fn refresh() -> Result<TrackerControls, TrackerError> {
    let (generation, period) = {
        let mut tracker = tracker();
        tracker.generation = tracker.generation.wrapping_add(1);
        tracker.index = LoadState::Loading;
        tracker.clans = LoadState::Idle;
        (tracker.generation, tracker.settings.period)
    };

    let read = database::run(async |pool| {
        let tracked = database::tracked_players(pool).await;
        let players = database::index_players(pool, period, Timestamp::now()).await;
        Ok((tracked, players))
    });

    let mut tracker = tracker();
    if tracker.generation != generation {
        return Ok(tracker.controls());
    }
    let (tracked, players) = match read {
        Ok(read) => read,
        Err(err) => {
            tracker.index = LoadState::Failed(err.report());
            return Err(err);
        }
    };
    match tracked {
        Ok(tracked) => tracker.tracked = tracked,
        // Left as it was rather than emptied: a note written from here would
        // then be the only row an account has.
        Err(err) => tracker.note_error = Some(err.report()),
    }
    match players {
        Ok(players) => {
            tracker.players = players;
            tracker.index = LoadState::Loaded;
        }
        Err(err) => tracker.index = LoadState::Failed(err.report()),
    }

    Ok(tracker.controls())
}

fn load_clans() -> Result<(), TrackerError> {
    let generation = {
        let mut tracker = tracker();
        if tracker.clans != LoadState::Idle {
            return Ok(());
        }
        tracker.clans = LoadState::Loading;
        tracker.generation
    };

    let read = database::run(async |pool| database::clan_inputs(pool).await);

    let mut tracker = tracker();
    if tracker.generation != generation {
        return Ok(());
    }
    match read {
        Ok(inputs) => {
            tracker.clan_latest = inputs.latest;
            tracker.clan_corrections = inputs.corrections;
            tracker.clans = LoadState::Loaded;
            Ok(())
        }
        Err(err) => {
            tracker.clans = LoadState::Failed(err.report());
            Err(err)
        }
    }
}

fn set_period(period: Period) -> Result<TrackerControls, TrackerError> {
    let period = period.into();
    if tracker().settings.period == period {
        return Ok(tracker().controls());
    }
    change_settings(|tracker| tracker.settings.period = period)?;

    // A different window of time is a different query, not a filter over
    // what is already loaded.
    refresh()
}

/// Applies `change` and writes the saved view back.
fn change_settings(change: impl FnOnce(&mut Tracker)) -> Result<(), TrackerError> {
    change(&mut tracker());

    let _writing = SETTINGS_WRITE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let settings = tracker().settings.clone();
    database::run(async |pool| database::save_settings(pool, &settings).await)
}

/// Applies `change` and writes the Current Match modes back.
fn change_view_modes(change: impl FnOnce(&mut Tracker)) -> Result<(), TrackerError> {
    change(&mut tracker());

    let _writing = SETTINGS_WRITE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let modes = tracker().modes;
    database::run(async |pool| database::save_view_modes(pool, modes).await)
}

fn save_note(account_id: &str, note: String) -> Result<TrackerControls, TrackerError> {
    let account = parse_account(account_id)?;
    let Some(player) = tracker().set_note(account, note) else {
        return Ok(tracker().controls());
    };

    let written = database::run(async |pool| database::save_note(pool, account, player).await);

    let mut tracker = tracker();
    tracker.note_error = written.err().map(|err| {
        log::warn!("player tracker: the note was not saved: {}", err.report());
        err.report()
    });

    Ok(tracker.controls())
}

fn clear() -> Result<(), TrackerError> {
    database::run(async |pool| database::clear_tracked(pool).await)?;

    tracker().tracked.clear();
    Ok(())
}

fn parse_account(value: &str) -> Result<AccountId, TrackerError> {
    value.parse::<i64>().map(AccountId).map_err(|_| TrackerError::AccountId { value: value.to_owned() })
}
