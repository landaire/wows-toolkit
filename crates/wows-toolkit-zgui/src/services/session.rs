//! The session's battles, which the replay listing, the session stats and the Stats tab all read.

use std::sync::Arc;

use tokio::runtime::Runtime;
use wows_replays::types::GameParamId;
use wows_toolkit_config::queries;
use wows_toolkit_viewmodel::stats::PerGameStat;
use wows_toolkit_viewmodel::stats::session::AppOpened;
use wows_toolkit_viewmodel::stats::session::SessionBattles;
use wows_toolkit_viewmodel::stats::session::SessionScope;
use wows_toolkit_viewmodel::stats::setting_keys;
use zgui::reactive::Memo;
use zgui::reactive::RwSignal;
use zgui::reactive::StoredValue;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::UpdateValue;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::task::spawn_local;

use crate::services::Settings;
use crate::services::error::SessionWriteError;
use crate::services::on_tokio;

/// Which battles the session holds under the scope the reader chose, and where a change to the
/// saved session is written.
#[derive(Clone, Copy)]
pub struct Session {
    battles: RwSignal<SessionBattles>,
    games: Memo<Arc<[PerGameStat]>>,
    /// Whether the saved session has been read.
    read: RwSignal<bool>,
    /// How many changes to the saved session have gone through, so a read that started before one
    /// is not taken over it.
    changes: StoredValue<u64>,
    settings: Settings,
    runtime: StoredValue<Option<Arc<Runtime>>>,
}

impl Session {
    /// A session for the run that opened at `opened`.
    pub(super) fn new(settings: Settings, runtime: StoredValue<Option<Arc<Runtime>>>, opened: AppOpened) -> Self {
        let battles = RwSignal::new(SessionBattles::new(opened));
        let games = Memo::new(move |_| {
            let scope = scope_of(settings);
            battles.with(|battles| Arc::from(battles.games(scope)))
        });

        Self { battles, games, read: RwSignal::new(false), changes: StoredValue::new(0), settings, runtime }
    }

    /// The chosen scope, subscribing to changes.
    pub fn scope(self) -> SessionScope {
        scope_of(self.settings)
    }

    pub fn scope_untracked(self) -> SessionScope {
        self.settings.read_untracked(|settings| settings.session_scope).unwrap_or_default()
    }

    /// Takes `scope` and saves it.
    pub fn choose_scope(self, scope: SessionScope) {
        self.settings.edit(setting_keys::SESSION_SCOPE, |settings| {
            settings.session_scope = scope;
            scope
        });
    }

    pub fn opened(self) -> AppOpened {
        self.battles.with_untracked(SessionBattles::opened)
    }

    /// The session's games under the chosen scope, oldest first.
    pub fn games(self) -> Memo<Arc<[PerGameStat]>> {
        self.games
    }

    /// Reads the saved session's games, whatever the scope.
    pub fn with_saved<R>(self, read: impl FnOnce(&[PerGameStat]) -> R) -> R {
        self.battles.with_untracked(|battles| read(battles.games(SessionScope::Manual)))
    }

    /// Whether the saved session has been read, before which an empty session says nothing.
    pub fn is_read(self) -> bool {
        self.read.get()
    }

    /// Reads the saved session. Called once the settings database is open.
    pub fn read_saved(self) {
        let Some(pool) = self.settings.pool() else { return };
        let runtime = self.runtime.get_value();
        spawn_local(async move {
            // A change that lands while the read is out would be undone by what it read: read again.
            loop {
                let changes = self.changes.get_value();
                let reading = pool.clone();
                let read =
                    on_tokio(
                        runtime.clone(),
                        async move { wows_toolkit_services::stats::load_saved_games(&reading).await },
                    )
                    .await;
                if self.changes.get_value() != changes {
                    continue;
                }
                match read {
                    Ok(Ok(games)) => self.battles.update(|battles| battles.save(games)),
                    Ok(Err(err)) => tracing::warn!("session stats: the saved session was not read: {err}"),
                    Err(err) => tracing::warn!("session stats: the read of the saved session did not complete: {err}"),
                }
                break;
            }
            self.read.set(true);
        });
    }

    /// Adds `game` to the saved session once its row is written.
    pub async fn save(self, game: PerGameStat) -> Result<(), SessionWriteError> {
        let row = game.to_row();
        self.write(move |pool| async move { queries::add_session_stat(&pool, &row).await }).await?;
        self.changes.update_value(|changes| *changes += 1);
        self.battles.update(|battles| battles.save([game]));

        Ok(())
    }

    /// Empties the saved session once the delete has gone through, so a failure does not leave an
    /// empty session that comes back at the next start.
    pub async fn clear_saved(self) -> Result<(), SessionWriteError> {
        self.write(|pool| async move { queries::clear_session_stats(&pool).await.map(drop) }).await?;
        self.changes.update_value(|changes| *changes += 1);
        self.battles.update(SessionBattles::clear_saved);

        Ok(())
    }

    /// Forgets the saved session's games in `ship`.
    pub async fn clear_saved_ship(self, ship: GameParamId) -> Result<(), SessionWriteError> {
        let ship_id = ship.raw() as i64;
        self.write(move |pool| async move { queries::clear_session_stats_for_ship(&pool, ship_id).await.map(drop) })
            .await?;
        self.changes.update_value(|changes| *changes += 1);
        self.battles.update(|battles| battles.clear_saved_ship(ship));

        Ok(())
    }

    /// A battle the game wrote to its replays directory this run, landing under `scope`. It joins
    /// the session since the app opened when it started after the app did, and under Manual it is
    /// written to the saved session, which shows it once the write has gone through.
    pub fn played(self, game: PerGameStat, scope: SessionScope) {
        self.listed(game.clone());
        if scope != SessionScope::Manual {
            return;
        }

        spawn_local(async move {
            if let Err(err) = self.save(game).await {
                tracing::warn!("session stats: the battle played was not recorded: {err}");
            }
        });
    }

    /// A replay in the game's replays directory that a session since the app opened may hold.
    pub fn listed(self, game: PerGameStat) {
        self.battles.update(|battles| battles.listed(game));
    }

    async fn write<W, F>(self, write: W) -> Result<(), SessionWriteError>
    where
        W: FnOnce(sqlx::SqlitePool) -> F,
        F: Future<Output = Result<(), sqlx::Error>> + Send + 'static,
    {
        let pool = self.settings.pool().ok_or(SessionWriteError::NoDatabase)?;
        on_tokio(self.runtime.get_value(), write(pool)).await??;

        Ok(())
    }
}

/// Manual until the settings are read, which is the setting's own default.
fn scope_of(settings: Settings) -> SessionScope {
    settings.read(|settings| settings.session_scope).unwrap_or_default()
}

#[cfg(test)]
impl Session {
    /// Starts the session over as a run that opened at `opened` with `saved` as its saved session,
    /// without a database.
    pub fn restart_for_test(self, opened: AppOpened, saved: Vec<PerGameStat>) {
        let mut battles = SessionBattles::new(opened);
        battles.save(saved);
        self.battles.set(battles);
        self.read.set(true);
    }

    /// When each game under `scope` was played, oldest first.
    pub fn game_times(self, scope: SessionScope) -> Vec<String> {
        self.battles.with_untracked(|battles| battles.games(scope).iter().map(|game| game.game_time.clone()).collect())
    }
}
