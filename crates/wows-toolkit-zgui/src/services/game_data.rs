//! The installed build's game data: the cache every replay is read through, and the startup load
//! of the newest build.

use std::path::PathBuf;
use std::sync::Arc;

use wows_toolkit_replay_data::heap_profile;
use wows_toolkit_replay_data::load::GameDataCache;
use wows_toolkit_replay_data::load::LoadedGameData;
use wows_toolkit_replay_data::load::load_current_build;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::StoredValue;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::UpdateValue;
use zgui::task::blocking;
use zgui::task::spawn_detached;

use crate::i18n::LanguageCode;

/// What the game data is doing.
#[derive(Clone)]
pub enum GameDataState {
    /// No game directory is set, so there is nothing to load.
    NoInstall,
    /// The newest build is loading. `version` is what the install says it is, so a view can name
    /// what it is waiting for; `None` when `preferences.xml` could not be read.
    Loading {
        version: Option<String>,
    },
    Ready(Arc<LoadedGameData>),
    Failed(String),
}

/// Where the game data comes from and how it is cached, as the settings say.
#[derive(Clone, Debug, PartialEq)]
pub struct GameDataRequest {
    /// `None` when no game directory is set.
    pub wows_dir: Option<PathBuf>,
    /// The cache directory setting, as stored: empty is the default location.
    pub cache_dir: String,
    /// `None` reads names in English.
    pub locale: Option<LanguageCode>,
    pub auto_dump: bool,
}

/// Counts requests, so a load that finishes after a newer request was made is dropped.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Generation(u64);

/// The cache and the state of its startup load.
#[derive(Clone, Copy)]
pub struct GameData {
    state: RwSignal<GameDataState, LocalStorage>,
    /// `None` until the settings have said where the install is.
    cache: RwSignal<Option<GameDataCache>, LocalStorage>,
    generation: StoredValue<Generation>,
}

impl GameData {
    pub(super) fn new() -> Self {
        Self {
            state: RwSignal::new_local(GameDataState::NoInstall),
            cache: RwSignal::new_local(None),
            generation: StoredValue::new(Generation::default()),
        }
    }

    pub fn state(self) -> GameDataState {
        self.state.get()
    }

    pub fn state_untracked(self) -> GameDataState {
        self.state.get_untracked()
    }

    /// The cache replays are read through. `None` before the settings are read.
    pub fn cache(self) -> Option<GameDataCache> {
        self.cache.get()
    }

    pub fn cache_untracked(self) -> Option<GameDataCache> {
        self.cache.get_untracked()
    }

    /// Replaces the cache with one over `request`'s install and loads its newest build.
    pub fn adopt(self, request: GameDataRequest) {
        self.generation.update_value(|generation| generation.0 += 1);
        let generation = self.generation.get_value();
        let locale = request.locale.as_ref().map_or("en", LanguageCode::as_str);

        let Some(wows_dir) = request.wows_dir else {
            let cache = GameDataCache::without_install().with_cache_dir(&request.cache_dir).with_locale(locale);
            self.cache.set(Some(cache));
            self.state.set(GameDataState::NoInstall);
            return;
        };

        let cache = GameDataCache::new(wows_dir.clone())
            .with_cache_dir(&request.cache_dir)
            .with_locale(locale)
            .with_auto_dump(request.auto_dump);
        self.cache.set(Some(cache.clone()));

        self.state.set(GameDataState::Loading { version: None });

        // A file read rather than a build load, so it can be named while the load runs. Off the
        // UI thread, since the install may be on a share that is slow to answer.
        let version_dir = wows_dir.clone();
        spawn_detached(async move {
            let version = blocking(move || {
                wows_toolkit_replay_data::install::installed_version(&version_dir)
                    .map(|version| format!("{}.{}.{}", version.major, version.minor, version.patch))
            })
            .await;

            // Read with `try_`, as the services may be gone by the time the read answers.
            let loading = matches!(self.state.try_get_untracked(), Some(GameDataState::Loading { .. }));
            if self.generation.try_get_value() == Some(generation) && loading {
                self.state.try_set(GameDataState::Loading { version });
            }
        });

        // The per-build caches for builds nothing can open any more, which is gigabytes after a
        // year of game updates.
        let pruned_dir = wows_dir.clone();
        let dump_base = wows_toolkit_config::game_data_dump_base_with_override(&request.cache_dir);
        spawn_detached(async move {
            blocking(move || {
                wows_toolkit_replay_data::load::prune_stale_caches(&pruned_dir, dump_base.as_deref());
            })
            .await;
        });

        spawn_detached(async move {
            let loaded = blocking(move || {
                heap_profile::mark("startup_preload.begin");
                let loaded = load_current_build(&wows_dir, &cache);
                heap_profile::mark("startup_preload.done");
                if let Err(reason) = &loaded {
                    tracing::warn!(wows_dir = %wows_dir.display(), %reason, "startup game-data preload failed");
                }
                loaded
            })
            .await;

            // Read with `try_`, as the services may be gone by the time the load answers.
            if self.generation.try_get_value() != Some(generation) {
                return;
            }
            let state = match loaded {
                Ok(loaded) => GameDataState::Ready(loaded),
                Err(reason) => GameDataState::Failed(reason),
            };
            self.state.try_set(state);
        });
    }

    /// Rewrites the names of every loaded build in `code`, off the UI thread: each build's
    /// translations are read from disk.
    pub fn set_locale(self, code: LanguageCode) {
        let Some(cache) = self.cache.get_untracked() else { return };
        spawn_detached(async move {
            blocking(move || cache.set_locale(code.as_str())).await;
        });
    }
}
