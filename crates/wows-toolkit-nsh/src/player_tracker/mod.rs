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

use std::collections::HashMap;

use jiff::Timestamp;
use nsh_core::prelude::*;
use wows_replays::types::AccountId;
use wows_toolkit_config::index::query_text;
use wows_toolkit_config::index::rows::PlayerFacet;
use wows_toolkit_viewmodel::player_tracker::tracked::TrackedPlayer;
use wows_toolkit_viewmodel::query_bar::seed;

use self::error::TrackerError;
use self::error::TrackerReport;
use self::live::LiveRevision;
use self::state::LoadState;
use self::state::Tracker;
use self::state::TrackedRevision;
use self::view::AccountIdText;
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

/// Held across each edit of the tracked rows, in memory and in the database
/// together, and across each read of them: a note or a clear lands whole, in
/// the order it was made, and a read sees the rows as one edit left them.
static TRACKED_ROWS: Mutex<()> = Mutex::new(());

/// What a refresh read: the stored rows with the revision they were read
/// at, and the index's players for the period.
struct IndexRead {
    read_at: TrackedRevision,
    tracked: Result<HashMap<AccountId, TrackedPlayer>, TrackerError>,
    players: Result<Vec<PlayerFacet>, TrackerError>,
}

/// Takes one of the ordering locks. They guard no data, so a poisoned one is
/// still sound to hold.
fn hold(lock: &'static Mutex<()>) -> MutexGuard<'static, ()> {
    lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn tracked_rows() -> MutexGuard<'static, ()> {
    hold(&TRACKED_ROWS)
}

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
    pub async fn open(locale: String) -> Result<TrackerOpened, TrackerReport> {
        run(move || open(&locale)).await
    }

    /// The labels and controls again, in `locale`, after a language change.
    pub fn relabel(locale: String) -> TrackerOpened {
        wows_toolkit_viewmodel::set_locale(&locale);
        let tracker = tracker();
        TrackerOpened { labels: tracker.labels(), controls: tracker.controls() }
    }

    /// Reads the index for the current period, and the tracked players again.
    pub async fn refresh() -> Result<TrackerControls, TrackerReport> {
        run(refresh).await
    }

    /// The players table, filtered and sorted.
    pub async fn players() -> Result<PlayersTable, TrackerReport> {
        run(|| Ok(tracker().players_table(Timestamp::now()))).await
    }

    /// The clans table, filtered and sorted.
    pub async fn clans() -> Result<ClansTable, TrackerReport> {
        run(|| Ok(tracker().clans_table(Timestamp::now()))).await
    }

    /// Reads the clans table's own index aggregates, unless they are read
    /// already for this refresh. They are whole-index scans, so they wait
    /// until the table is shown.
    pub async fn load_clans() -> Result<(), TrackerReport> {
        run(load_clans).await
    }

    /// Looks back over `period`, which is another index read.
    pub async fn set_period(period: Period) -> Result<TrackerControls, TrackerReport> {
        run(move || set_period(period)).await
    }

    pub async fn set_filter(filter: String) -> Result<TrackerControls, TrackerReport> {
        run(move || {
            change_settings(|tracker| tracker.settings.filter = filter)?;
            Ok(tracker().controls())
        })
        .await
    }

    /// Orders the players table by `column`, in the shared three-state cycle.
    pub async fn sort_players(column: PlayerColumn) -> Result<TrackerControls, TrackerReport> {
        run(move || {
            change_settings(|tracker| tracker.settings.sort = tracker.settings.sort.toggled(column.into()))?;
            Ok(tracker().controls())
        })
        .await
    }

    pub async fn sort_clans(column: ClanColumn) -> Result<TrackerControls, TrackerReport> {
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
    pub async fn set_show_division_mates(show: bool) -> Result<TrackerControls, TrackerReport> {
        run(move || {
            change_view_modes(|tracker| tracker.modes.show_division_mates = show)?;
            Ok(tracker().controls())
        })
        .await
    }

    pub async fn set_view_mode(mode: ViewMode) -> Result<TrackerControls, TrackerReport> {
        run(move || {
            change_view_modes(|tracker| tracker.modes.current_match_view_mode = mode.into())?;
            Ok(tracker().controls())
        })
        .await
    }

    pub async fn set_scope(scope: Scope) -> Result<TrackerControls, TrackerReport> {
        run(move || {
            change_view_modes(|tracker| tracker.modes.win_rate_mode = scope.into())?;
            Ok(tracker().controls())
        })
        .await
    }

    /// Keeps `note` against the account, in the rows the other front ends
    /// read. A failed write is reported in the controls' `noteError`.
    pub async fn save_note(account_id: AccountIdText, note: String) -> Result<TrackerControls, TrackerReport> {
        let account = account_id.to_account()?;

        run(move || save_note(account, note)).await
    }

    /// Forgets every tracked player. The rows go once the delete has landed.
    pub async fn clear() -> Result<(), TrackerReport> {
        run(clear).await
    }

    /// The Search query for every match `account_id` was in.
    pub fn player_matches_query(account_id: AccountIdText) -> Result<String, TrackerReport> {
        let account = account_id.to_account()?;

        Ok(query_text::print_query(&seed::matches_with_player(account)))
    }

    /// The Search query for every match mentioning `clan`. The index has no
    /// clan-only field, so a tag inside a name matches too.
    pub fn clan_matches_query(clan: String) -> String {
        query_text::print_query(&seed::matches_mentioning_clan(&clan))
    }

    /// Watches the replays directory of the WoWs directory the settings
    /// hold, restarting the watch when that has changed. The game data the
    /// battle resolves against is rebound to the same settings first.
    pub async fn sync_live() -> Result<(), TrackerReport> {
        run(|| {
            crate::game_data::reload_settings().map_err(TrackerError::GameData)?;
            live::watch(crate::game_data::wows_dir().as_deref());
            Ok(())
        })
        .await
    }

    /// Whether the Current Match section is on screen. The battle's stats
    /// lookup is a request to an outside service, so it waits until it is.
    pub fn set_current_match_shown(shown: bool) {
        live::set_shown(shown);
    }

    /// Bumped whenever the Current Match section has something new to show.
    pub fn live_revision() -> LiveRevision {
        live::revision()
    }

    /// The Current Match section.
    pub async fn live_view() -> Result<LiveView, TrackerReport> {
        run(|| Ok(live::view(&tracker(), Timestamp::now()))).await
    }
}

/// Runs `work` on a blocking thread, reporting its error as the view shows it.
async fn run<T>(work: impl FnOnce() -> Result<T, TrackerError> + Send + 'static) -> Result<T, TrackerReport>
where
    T: Send + 'static,
{
    let result = blocking(work).await.map_err(TrackerError::Task)?;

    Ok(result?)
}

fn open(locale: &str) -> Result<TrackerOpened, TrackerError> {
    // The chip hover is worded by the viewmodel, in its own locale.
    wows_toolkit_viewmodel::set_locale(locale);
    let _rows = tracked_rows();
    let opened = database::run(async |pool| database::open(pool).await)?;

    let mut tracker = tracker();
    // Moved on rather than reset, so a read begun before this one cannot
    // land over the rows it reads.
    let tracked_revision = tracker.tracked_revision.next();
    *tracker = Tracker::new();
    tracker.tracked_revision = tracked_revision;
    tracker.settings = opened.settings;
    tracker.modes = opened.modes;
    tracker.tracked = opened.tracked;

    Ok(TrackerOpened { labels: tracker.labels(), controls: tracker.controls() })
}

fn refresh() -> Result<TrackerControls, TrackerError> {
    let (generation, period) = {
        let mut tracker = tracker();
        tracker.generation = tracker.generation.next();
        tracker.index = LoadState::Loading;
        tracker.clans = LoadState::Idle;
        (tracker.generation, tracker.settings.period)
    };

    // The revision is taken with the rows, so an edit made after this read
    // keeps the read's rows from replacing it.
    let tracked = {
        let _rows = tracked_rows();
        let read_at = tracker().tracked_revision;
        database::run(async |pool| Ok(database::tracked_players(pool).await)).map(|tracked| (read_at, tracked))
    };
    let read = tracked.and_then(|(read_at, tracked)| {
        database::run(async |pool| {
            let players = database::index_players(pool, period, Timestamp::now()).await;
            Ok(IndexRead { read_at, tracked, players })
        })
    });

    let mut tracker = tracker();
    if tracker.generation != generation {
        return Ok(tracker.controls());
    }
    let IndexRead { read_at, tracked, players } = match read {
        Ok(read) => read,
        Err(err) => {
            tracker.index = LoadState::Failed(err.chain().to_string());
            return Err(err);
        }
    };
    match tracked {
        Ok(tracked) => {
            tracker.install_tracked(read_at, tracked);
        }
        // Left as it was rather than emptied: a note written from here would
        // then be the only row an account has.
        Err(err) => tracker.note_error = Some(err.chain().to_string()),
    }
    match players {
        Ok(players) => {
            tracker.players = players;
            tracker.index = LoadState::Loaded;
        }
        Err(err) => tracker.index = LoadState::Failed(err.chain().to_string()),
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
            tracker.clans = LoadState::Failed(err.chain().to_string());
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

    let _writing = hold(&SETTINGS_WRITE);
    let settings = tracker().settings.clone();
    database::run(async |pool| database::save_settings(pool, &settings).await)
}

/// Applies `change` and writes the Current Match modes back.
fn change_view_modes(change: impl FnOnce(&mut Tracker)) -> Result<(), TrackerError> {
    change(&mut tracker());

    let _writing = hold(&SETTINGS_WRITE);
    let modes = tracker().modes;
    database::run(async |pool| database::save_view_modes(pool, modes).await)
}

fn save_note(account: AccountId, note: String) -> Result<TrackerControls, TrackerError> {
    let _rows = tracked_rows();
    let Some(player) = tracker().set_note(account, note) else {
        return Ok(tracker().controls());
    };
    let written = database::run(async |pool| database::save_note(pool, account, player).await);

    let mut tracker = tracker();
    tracker.note_error = written.err().map(|err| {
        log::warn!("player tracker: the note was not saved: {}", err.chain());
        err.chain().to_string()
    });

    Ok(tracker.controls())
}

fn clear() -> Result<(), TrackerError> {
    let _rows = tracked_rows();
    database::run(async |pool| database::clear_tracked(pool).await)?;

    tracker().clear_tracked();
    Ok(())
}
