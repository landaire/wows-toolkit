//! The battle in progress: a watch over the replays directory, and the
//! roster it reads, joined to the tracker and to the Twitch chat seen around
//! it.
//!
//! The watch runs on a thread of its own and bumps [`revision`] whenever what
//! the Current Match section shows has changed, which the view polls for.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::time::Duration;
use std::time::Instant;

use jiff::Timestamp;
use wows_replays::types::AccountId;
use wows_toolkit_viewmodel::formatting::separate_number;
use wows_toolkit_viewmodel::player_tracker::encounter_severity;
use wows_toolkit_viewmodel::player_tracker::history;
use wows_toolkit_viewmodel::player_tracker::live::LiveMatch;
use wows_toolkit_viewmodel::player_tracker::live::LiveRosterRow;
use wows_toolkit_viewmodel::player_tracker::live::TrackedIndex;
use wows_toolkit_viewmodel::player_tracker::live::resolve_roster;
use wows_toolkit_viewmodel::player_tracker::live::visible_stat_modes;
use wows_toolkit_viewmodel::player_tracker::live_source::LiveSource;
use wows_toolkit_viewmodel::player_tracker::live_source::LiveWatcher;
use wows_toolkit_viewmodel::player_tracker::live_source::POLL_INTERVAL;
use wows_toolkit_viewmodel::player_tracker::shipbuilds_player_url;
use wows_toolkit_viewmodel::player_tracker::wows_numbers_player_url;
use wows_toolkit_viewmodel::twitch;

use crate::text::text;
use crate::text::text_with;

use super::database;
use super::state::Tracker;
use super::view::LiveView;
use super::view::RosterRowView;
use super::view::ScopeHeadings;
use super::view::TeamView;

/// How often the chat observations are re-read while a battle is under way.
///
/// The chat poll writes them every two minutes, so reading faster than that
/// only repeats the same rows.
const CHAT_POLL_INTERVAL: Duration = Duration::from_secs(60);

/// Why the roster's ships and figures are missing.
#[derive(Debug, thiserror::Error)]
pub enum LiveDataError {
    #[error("this battle's game data did not load: this front end has no game data cache")]
    GameDataUnavailable,
}

/// The battle in progress, as the watch last saw it.
#[derive(Default)]
pub struct LiveState {
    /// The directory being watched; `None` while no WoWs directory is set.
    replay_dir: Option<PathBuf>,
    /// Whether the watch has looked at the directory yet.
    checked: bool,
    live_match: Option<LiveMatch>,
    /// Which of the battle's players the index has met, keyed by lower-cased
    /// name. All time, so it does not follow the period selector.
    met_before: HashMap<String, AccountId>,
    /// Twitch logins seen in chat around the battle, and when.
    chat: Vec<(String, Timestamp)>,
    missing_data: Option<LiveDataError>,
    revision: u32,
    /// Bumped per watch, so a thread whose directory was replaced stops.
    watch: u64,
}

static LIVE: LazyLock<Mutex<LiveState>> = LazyLock::new(|| Mutex::new(LiveState::default()));

fn live() -> MutexGuard<'static, LiveState> {
    // A panicking watch thread leaves the state as it was last written, which
    // is still a consistent snapshot to draw.
    LIVE.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Bumped whenever what [`view`] returns has changed.
pub fn revision() -> u32 {
    live().revision
}

/// Watches the replays directory under `wows_dir`, replacing the watch
/// already running when the directory differs. An empty `wows_dir` stops
/// watching.
pub fn watch(wows_dir: &str) {
    let replay_dir = (!wows_dir.trim().is_empty()).then(|| Path::new(wows_dir).join("replays"));

    let generation = {
        let mut state = live();
        if state.replay_dir == replay_dir && (replay_dir.is_none() || state.watch != 0) {
            return;
        }
        let watch = state.watch.wrapping_add(1);
        let revision = state.revision.wrapping_add(1);
        *state = LiveState { replay_dir: replay_dir.clone(), watch, revision, ..LiveState::default() };
        watch
    };

    let Some(replay_dir) = replay_dir else { return };
    let spawned = std::thread::Builder::new()
        .name("player-tracker-live".to_owned())
        .spawn(move || run_watch(generation, &replay_dir));
    if let Err(err) = spawned {
        log::error!("player tracker: the battle watch could not start: {err}");
    }
}

/// The chat read for the battle in progress, while its window is open.
struct ChatPoll {
    started_at: Timestamp,
    start: i64,
    end: i64,
    next: Instant,
}

fn run_watch(generation: u64, replay_dir: &Path) {
    let source = LiveSource::in_dir(replay_dir);
    let mut watcher = LiveWatcher::new();
    let mut chat: Option<ChatPoll> = None;

    while live().watch == generation {
        if let Some(found) = watcher.tick(&source) {
            chat = found.as_ref().map(|found| {
                let started = found.started_at.as_second();
                ChatPoll {
                    started_at: found.started_at,
                    start: started + (twitch::WINDOW_BEFORE_MINUTES * 60.0) as i64,
                    end: started + (twitch::WINDOW_AFTER_MINUTES * 60.0) as i64,
                    next: Instant::now(),
                }
            });
            adopt(generation, found);
        }

        if let Some(poll) = chat.as_mut()
            && Instant::now() >= poll.next
        {
            read_chat(generation, poll);
            poll.next = Instant::now() + CHAT_POLL_INTERVAL;
            // Past the window there is nothing further to find.
            if Timestamp::now().as_second() > poll.end {
                chat = None;
            }
        }

        std::thread::sleep(POLL_INTERVAL);
    }
}

/// Takes what a poll found, then asks the index which of its players were
/// met before.
fn adopt(generation: u64, found: Option<LiveMatch>) {
    let lookup = {
        let mut state = live();
        if state.watch != generation {
            return;
        }
        state.checked = true;
        state.met_before.clear();
        state.chat.clear();
        // Ship names, classes and the stats lookup all resolve against the
        // battle's own build, which only a game data cache can load.
        state.missing_data = found.as_ref().map(|_| LiveDataError::GameDataUnavailable);
        let lookup = found.as_ref().map(|found| {
            let names: Vec<String> = found.players.iter().map(|player| player.name.clone()).collect();
            (found.started_at, names)
        });
        state.live_match = found;
        state.revision = state.revision.wrapping_add(1);
        lookup
    };

    let Some((started_at, names)) = lookup else { return };
    match database::run(async |pool| database::accounts_named(pool, &names).await) {
        Ok(accounts) => {
            let mut state = live();
            if state.watch == generation && started_at_of(&state) == Some(started_at) {
                state.met_before = accounts;
                state.revision = state.revision.wrapping_add(1);
            }
        }
        // The roster still lists everyone; only the met counts stay empty.
        Err(err) => log::warn!("player tracker: the met-before lookup failed: {}", err.report()),
    }
}

fn read_chat(generation: u64, poll: &ChatPoll) {
    let (start, end) = (poll.start, poll.end);
    match database::run(async move |pool| database::chat_observations(pool, start, end).await) {
        Ok(observations) => {
            let mut state = live();
            if state.watch == generation && started_at_of(&state) == Some(poll.started_at) {
                state.chat = observations;
                state.revision = state.revision.wrapping_add(1);
            }
        }
        // The roster still lists everyone; only the chips are missing.
        Err(err) => log::warn!("player tracker: the chat lookup failed: {}", err.report()),
    }
}

fn started_at_of(state: &LiveState) -> Option<Timestamp> {
    state.live_match.as_ref().map(|live| live.started_at)
}

/// The Current Match section as it stands.
pub fn view(tracker: &Tracker, now: Timestamp) -> LiveView {
    let state = live();
    state_view(&state, tracker, now)
}

fn state_view(state: &LiveState, tracker: &Tracker, now: Timestamp) -> LiveView {
    let modes = visible_stat_modes(tracker.modes.current_match_view_mode, tracker.modes.win_rate_mode);
    let scope_headings = modes
        .iter()
        .map(|mode| {
            let scope = text(mode.label_key());
            let heading = |key: &str| format!("{scope} {}", text(key));
            ScopeHeadings {
                win_rate: heading("ui.player_tracker.column.win_rate"),
                personal_rating: heading("ui.player_tracker.column.personal_rating"),
                avg_damage: heading("ui.player_tracker.column.avg_damage"),
                battles: heading("ui.player_tracker.column.battles"),
            }
        })
        .collect();

    let status = match (&state.replay_dir, state.checked, &state.live_match) {
        (None, _, _) => Some(text("ui.player_tracker.live_no_directory")),
        (Some(_), false, _) => Some(text("ui.player_tracker.live_checking")),
        (Some(_), true, None) => Some(text("ui.player_tracker.live_none")),
        (Some(_), true, Some(_)) => None,
    };

    let team = |title_key: &str, rows: &[LiveRosterRow], started_at: Timestamp| TeamView {
        title: text(title_key),
        players: text_with("ui.player_tracker.team_players", &[("count", &rows.len().to_string())]),
        rows: rows.iter().map(|row| roster_row(row, state, tracker, started_at, now)).collect(),
    };

    let Some(live) = state.live_match.as_ref() else {
        return LiveView {
            status,
            note: None,
            scope_headings,
            friendly: team("ui.player_tracker.allies", &[], now),
            enemy: team("ui.player_tracker.enemies", &[], now),
        };
    };

    let index = TrackedIndex { by_name: state.met_before.clone(), players: state.met_before.len() };
    let roster = resolve_roster(live, &index, None, None);
    let note = match &state.missing_data {
        Some(err) => {
            Some(text_with("ui.player_tracker.stats_unavailable", &[("reason", &err.to_string())]))
        }
        None if !roster.ships_resolved => Some(text("ui.player_tracker.live_loading_build")),
        None => None,
    };

    LiveView {
        status,
        note,
        scope_headings,
        friendly: team("ui.player_tracker.allies", &roster.friendly, roster.started_at),
        enemy: team("ui.player_tracker.enemies", &roster.enemy, roster.started_at),
    }
}

fn roster_row(
    row: &LiveRosterRow,
    state: &LiveState,
    tracker: &Tracker,
    started_at: Timestamp,
    now: Timestamp,
) -> RosterRowView {
    let show_division_mates = tracker.modes.show_division_mates;

    // How many battles this player has been met in, counted the way the
    // tables beside it count.
    let tracked = row.tracked.and_then(|account| tracker.tracked.get(&account));
    let met = tracked.map_or(0, |tracked| tracked.visible_arena_ids(show_division_mates).count());
    let met_hover = match tracked {
        Some(tracked) if met > 0 => {
            let total = tracked.arena_ids.len().to_string();
            let last = tracked
                .last_visible_timestamp(show_division_mates)
                .map(|at| history::last_seen_text(Some(at), now))
                .unwrap_or_else(|| text("ui.player_tracker.never_encountered"));
            text_with(
                "ui.player_tracker.encounters_hover",
                &[("total", &total), ("range", &met.to_string()), ("last", &last)],
            )
        }
        _ => text("ui.player_tracker.never_encountered"),
    };

    let observations = state.chat.iter().map(|(login, seen_at)| (login.as_str(), *seen_at));
    let candidates = twitch::sniper_candidates(observations, &row.name, started_at);
    let twitch_hover = if candidates.is_empty() { String::new() } else { twitch::sniper_hover_text(&candidates) };

    let links = row.account_id.zip(row.region);

    RosterRowView {
        name: match row.clan.as_deref() {
            Some(clan) => format!("[{clan}] {}", row.name),
            None => row.name.clone(),
        },
        tint: row.tint.into(),
        ship: row.ship_name.clone().unwrap_or_else(|| "-".to_string()),
        met: if met == 0 { String::new() } else { separate_number(met as i64, None) },
        met_severity: encounter_severity(met).into(),
        met_hover,
        note: tracked.map(|tracked| tracked.notes.clone()).filter(|notes| !notes.trim().is_empty()),
        twitch_logins: candidates.into_iter().map(|candidate| candidate.login).collect(),
        twitch_hover,
        wows_numbers_url: links.map(|(account, region)| wows_numbers_player_url(region, account, &row.name)),
        shipbuilds_url: links.map(|(account, region)| shipbuilds_player_url(region, account, &row.name)),
        account_id: links.map(|(account, _)| account.0.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use jiff::ToSpan as _;
    use wows_replays::types::ArenaId;
    use wows_replays::types::GameParamId;
    use wows_replays::types::Relation;
    use wows_toolkit_viewmodel::player_tracker::live::LiveMatchPlayer;
    use wows_toolkit_viewmodel::player_tracker::tracked::TrackedPlayer;

    use super::*;

    fn battle(started_at: Timestamp) -> LiveMatch {
        let player = |name: &str, relation: u32| LiveMatchPlayer {
            name: name.to_string(),
            ship_id: GameParamId::from(100u64),
            relation: Relation::new(relation),
        };
        LiveMatch {
            started_at,
            build: Some(1),
            players: vec![player("Harvey635", 0), player("Stranger99", 2), player("Friendly42", 1)],
        }
    }

    fn watching(started_at: Timestamp) -> LiveState {
        LiveState {
            replay_dir: Some(PathBuf::from("/replays")),
            checked: true,
            live_match: Some(battle(started_at)),
            missing_data: Some(LiveDataError::GameDataUnavailable),
            ..LiveState::default()
        }
    }

    #[test]
    fn no_directory_no_look_and_no_battle_each_say_so() {
        let tracker = Tracker::new();
        let now = Timestamp::now();

        let unset = LiveState::default();
        assert_eq!(state_view(&unset, &tracker, now).status, Some(text("ui.player_tracker.live_no_directory")));

        let checking = LiveState { replay_dir: Some(PathBuf::from("/replays")), ..LiveState::default() };
        assert_eq!(state_view(&checking, &tracker, now).status, Some(text("ui.player_tracker.live_checking")));

        let idle = LiveState { checked: true, ..checking };
        assert_eq!(state_view(&idle, &tracker, now).status, Some("No battle in progress".to_string()));
    }

    #[test]
    fn the_roster_splits_by_team_and_counts_who_was_met_before() {
        let now = Timestamp::now();
        let mut tracker = Tracker::new();
        let mut met = TrackedPlayer { last_name: "Stranger99".to_string(), notes: "rushes B".to_string(), ..TrackedPlayer::default() };
        for arena in 1..=4 {
            met.arena_ids.insert(ArenaId::from(arena));
            met.timestamps.insert(now - arena.hours());
        }
        tracker.tracked.insert(AccountId(5), met);

        let mut state = watching(now);
        state.met_before.insert("stranger99".to_string(), AccountId(5));

        let view = state_view(&state, &tracker, now);
        assert!(view.status.is_none());
        assert_eq!(view.friendly.rows.len(), 2);
        assert_eq!(view.enemy.rows.len(), 1);
        assert_eq!(view.enemy.players, "1 players");

        let foe = &view.enemy.rows[0];
        assert_eq!(foe.met, "4");
        assert_eq!(foe.met_severity, super::super::view::Severity::Warned);
        assert_eq!(foe.note.as_deref(), Some("rushes B"));
        assert!(foe.met_hover.starts_with("Seen 4 times total"), "got {:?}", foe.met_hover);
        assert_eq!(foe.ship, "-", "no game data resolves the ship");
        assert!(view.note.as_deref().is_some_and(|note| note.starts_with("Stats unavailable")));
    }

    #[test]
    fn a_chat_login_that_names_a_player_puts_a_chip_on_their_row() {
        let now = Timestamp::now();
        let tracker = Tracker::new();
        let mut state = watching(now);
        state.chat = vec![("harvey635".to_string(), now + 1.minute())];

        let view = state_view(&state, &tracker, now);
        let me = view.friendly.rows.iter().find(|row| row.name == "Harvey635").expect("the reader is listed");
        assert_eq!(me.twitch_logins, vec!["harvey635".to_string()]);
        assert!(me.twitch_hover.contains("harvey635"));
        let other = view.enemy.rows.first().expect("the enemy is listed");
        assert!(other.twitch_logins.is_empty());
    }

    #[test]
    fn the_detailed_roster_heads_both_scopes() {
        let tracker = Tracker::new();
        let view = state_view(&LiveState::default(), &tracker, Timestamp::now());
        assert_eq!(view.scope_headings.len(), 2);
        assert_eq!(view.scope_headings[0].win_rate, "Overall WR");
        assert_eq!(view.scope_headings[1].battles, "Ship Battles");
    }
}
