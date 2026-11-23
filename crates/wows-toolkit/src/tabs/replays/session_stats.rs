//! Putting listed battles in the session: the replays the reader sets or adds, the battles the game
//! writes to its replays directory, the replays the reader opens from it, and, for a session since
//! the app opened, the replays that directory already lists from this run.

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

use wows_toolkit_replay_data::directory_stats::GameRead;
use wows_toolkit_replay_data::directory_stats::read_game;
use wows_toolkit_replay_data::load::GameDataCache;
use wows_toolkit_viewmodel::stats::PerGameStat;
use wows_toolkit_viewmodel::stats::session::SessionScope;
use zgui::reactive::LocalStorage;
use zgui::reactive::RenderEffect;
use zgui::reactive::StoredValue;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::UpdateValue;
use zgui::reactive::prelude::WithValue;
use zgui::task::spawn_local;

use crate::long_job::long_job;
use crate::services::GameDataState;
use crate::services::Session;
use crate::shell::palette::TabRequest;
use crate::tabs::replays::browser::BrowserControl;
use crate::tabs::replays::workspace::ReplayWorkspace;

/// Records `paths` in the saved session, each read as opening it would: a session row's figures
/// come from the battle results, which are only in the file. A replay that will not read is
/// skipped. `replace` forgets what is recorded first, and brings the Stats tab forward on its
/// Overview.
pub fn record(workspace: ReplayWorkspace, paths: Vec<PathBuf>, replace: bool) {
    let services = workspace.services;
    let Some(game_data) = services.game_data.cache_untracked() else {
        tracing::warn!("session stats: asked for before the game directory was known");
        return;
    };
    let session = services.session;
    if replace {
        let shell = workspace.shell;
        shell.palette.send(shell, TabRequest::ShowSessionOverview);
    }

    workspace.under_tab(|| {
        spawn_local(async move {
            if replace && let Err(err) = session.clear_saved().await {
                tracing::warn!("session stats: the session was not cleared: {err}");
                return;
            }

            for path in paths {
                let (reading, cache) = (path.clone(), game_data.clone());
                let stat = match long_job(move || read_played_game(&reading, &cache)).await {
                    GameRead::Game(stat) => *stat,
                    GameRead::NoResults => {
                        tracing::warn!(path = %path.display(), "session stats: the replay holds no results to record");
                        continue;
                    }
                    GameRead::MissingBuild | GameRead::Unreadable => {
                        tracing::warn!(path = %path.display(), "session stats: the replay could not be read");
                        continue;
                    }
                };
                if let Err(err) = session.save(stat).await {
                    tracing::warn!("session stats: the battle could not be recorded: {err}");
                }
            }
        });
    });
}

/// Why a replay of the game's replays directory is read for the session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReadCause {
    /// The game wrote it, or wrote it again, while the session had `scope`.
    Played { scope: SessionScope },
    /// It was listed when a session since the app opened looked for its battles.
    Listed,
}

/// The reads of the game's replays directory that feed the session.
#[derive(Clone, Copy)]
pub struct SessionReads {
    workspace: ReplayWorkspace,
    control: BrowserControl,
    /// The replays read, or being read, this run, which a session since the app opened need not
    /// read again. A replay that would not read leaves it, to be tried again.
    seen: StoredValue<HashSet<PathBuf>, LocalStorage>,
    /// Battles the game wrote before there was game data to read them with, and the scope each
    /// landed under.
    waiting: StoredValue<Vec<(PathBuf, SessionScope)>, LocalStorage>,
    /// The latest read asked for of each replay: reads run side by side, and only the latest one
    /// counts, so an older write of the file finishing last does not replace a newer one.
    latest: StoredValue<HashMap<PathBuf, u64>, LocalStorage>,
}

impl SessionReads {
    pub fn new(workspace: ReplayWorkspace, control: BrowserControl) -> Self {
        let reads = Self {
            workspace,
            control,
            seen: StoredValue::new_local(HashSet::new()),
            waiting: StoredValue::new_local(Vec::new()),
            latest: StoredValue::new_local(HashMap::new()),
        };
        reads.follow_game_data();

        reads
    }

    /// A watched directory gained or rewrote `path`. Only the install's replays directory holds
    /// the reader's own battles, so a replay elsewhere is left alone. The battle counts under the
    /// scope in force as it landed, not the one in force once it is read.
    ///
    /// What the landing's read of the battle hands to [`Self::landed_read`]. `None` when the
    /// session takes nothing from that read: the replay is elsewhere, or it landed before there
    /// was game data to read it with and is read here once there is.
    pub fn landed(self, path: PathBuf) -> Option<SessionLanding> {
        if !self.control.install_holds(&path) {
            return None;
        }
        let scope = self.workspace.services.session.scope_untracked();
        if self.workspace.services.game_data.cache_untracked().is_none() {
            self.waiting.update_value(|waiting| waiting.push((path, scope)));
            return None;
        }

        Some(SessionLanding { scope, generation: self.begin(&path) })
    }

    /// Takes what the landing's read of `path` found.
    pub fn landed_read(self, path: &Path, landing: SessionLanding, read: GameRead) {
        #[cfg(test)]
        let read = simulated::game(path).map_or(read, |game| GameRead::Game(Box::new(game)));

        self.take(path, landing.generation, ReadCause::Played { scope: landing.scope }, read);
    }

    /// Once game data is ready, reads the battles that landed before it was. While the session is
    /// the one since the app opened, also reads the battles the install lists that started since
    /// then and were not read yet.
    fn follow_game_data(self) {
        let services = self.workspace.services;
        let following = RenderEffect::new(move |_| {
            if !matches!(services.game_data.state(), GameDataState::Ready(_)) {
                return;
            }
            for (path, scope) in self.waiting.try_update_value(std::mem::take).unwrap_or_default() {
                self.read(path, ReadCause::Played { scope });
            }
            if services.session.scope() != SessionScope::SinceAppOpened {
                return;
            }
            let unread: Vec<PathBuf> = self
                .control
                .install_replays_since(services.session.opened())
                .into_iter()
                .filter(|path| self.seen.with_value(|seen| !seen.contains(path)))
                .collect();
            for path in unread {
                self.read(path, ReadCause::Listed);
            }
        });
        on_cleanup_local(move || drop(following));
    }

    fn read(self, path: PathBuf, cause: ReadCause) {
        let services = self.workspace.services;
        let Some(game_data) = services.game_data.cache_untracked() else { return };
        let generation = self.begin(&path);

        self.workspace.under_tab(|| {
            spawn_local(async move {
                let named = path.clone();
                let read = long_job(move || read_played_game(&path, &game_data)).await;
                self.take(&named, generation, cause, read);
            });
        });
    }

    /// Marks `path` as read this run, returning which read of it this is.
    fn begin(self, path: &Path) -> u64 {
        self.seen.update_value(|seen| {
            seen.insert(path.to_path_buf());
        });

        self.latest
            .try_update_value(|latest| {
                let generation = latest.entry(path.to_path_buf()).or_default();
                *generation += 1;
                *generation
            })
            .unwrap_or_default()
    }

    /// Puts what read `generation` of `path` found in the session, unless a later read of it
    /// started since.
    fn take(self, path: &Path, generation: u64, cause: ReadCause, read: GameRead) {
        if self.latest.with_value(|latest| latest.get(path) != Some(&generation)) {
            return;
        }
        let services = self.workspace.services;
        match read {
            GameRead::Game(game) => match cause {
                ReadCause::Played { scope } => services.session.played(*game, scope),
                ReadCause::Listed => services.session.listed(*game),
            },
            // Read again when the game writes the results into the file.
            GameRead::NoResults => {
                tracing::debug!(path = %path.display(), "session stats: the battle has no results yet");
            }
            // Tried again once the listing is looked through again, as after a download.
            GameRead::MissingBuild | GameRead::Unreadable => {
                tracing::warn!(path = %path.display(), "session stats: the battle could not be read");
                self.seen.update_value(|seen| {
                    seen.remove(path);
                });
            }
        }
    }
}

/// A battle the game wrote, which the session takes once the landing's read of it ends.
#[derive(Clone, Copy, Debug)]
pub struct SessionLanding {
    scope: SessionScope,
    /// Which read of the replay the landing's is, so an older one finishing last is dropped.
    generation: u64,
}

/// Counts a replay of the install's replays directory the reader opened from its listing, `game`
/// being what its tab read. It counts as a battle the game wrote does, under the scope in force as
/// the read landed.
pub fn count_opened(session: Session, path: &Path, game: Option<PerGameStat>) {
    // A test stands in a game for the replay: the test fixtures name builds no test has game data
    // for.
    #[cfg(test)]
    let game = simulated::game(path).or(game);
    let Some(game) = game else {
        tracing::debug!(path = %path.display(), "session stats: the opened replay names no game to count");
        return;
    };

    session.played(game, session.scope_untracked());
}

/// Reads `path`'s game, or the one a test stands in for it: the test fixtures name builds no test
/// has game data for.
fn read_played_game(path: &Path, game_data: &GameDataCache) -> GameRead {
    #[cfg(test)]
    if let Some(game) = simulated::game(path) {
        return GameRead::Game(Box::new(game));
    }

    read_game(path, game_data)
}

#[cfg(test)]
pub mod simulated {
    use std::collections::HashMap;
    use std::path::Path;
    use std::path::PathBuf;
    use std::sync::Mutex;

    use wows_toolkit_viewmodel::stats::PerGameStat;

    /// Games read in place of their replays, by path. Shared across threads: the read runs on the
    /// long-job pool.
    static GAMES: Mutex<Option<HashMap<PathBuf, PerGameStat>>> = Mutex::new(None);

    /// Has a read of `path` produce `game`.
    pub fn stand_in(path: PathBuf, game: PerGameStat) {
        GAMES.lock().expect("the stand-ins are not poisoned").get_or_insert_default().insert(path, game);
    }

    pub(super) fn game(path: &Path) -> Option<PerGameStat> {
        GAMES.lock().expect("the stand-ins are not poisoned").as_ref()?.get(path).cloned()
    }
}
