//! The app-wide services a renderer draws on: the game data replays are
//! read against, and the collab session viewports share cursors and frames
//! through.

use std::path::PathBuf;
use std::sync::LazyLock;
use std::sync::Mutex;

use tokio::sync::watch;
use wows_toolkit_replay_data::load::GameDataCache;
use wows_toolkit_viewmodel::settings::keys;
use wt_collab_client::controller::SessionController;
use wt_collab_client::controller::SharedWindow;
use wt_collab_client::link::CollabLink;

/// The process's game data, bound to the saved install and cache directory.
pub fn game_data() -> GameDataCache {
    static CACHE: LazyLock<GameDataCache> = LazyLock::new(bind_game_data);

    CACHE.clone()
}

fn bind_game_data() -> GameDataCache {
    let setting = |key: &str| match wows_toolkit_config::load_startup_setting::<String>(key) {
        Ok(value) => value.filter(|value| !value.is_empty()),
        Err(err) => {
            log::warn!("renderer: the {key} setting could not be read: {err}");
            None
        }
    };

    // With no install the cache still reads the builds already dumped.
    let cache = match setting(keys::WOWS_DIR) {
        Some(dir) => GameDataCache::new(PathBuf::from(dir)),
        None => GameDataCache::without_install(),
    };
    let cache = match setting(keys::GAME_DATA_CACHE_DIR) {
        Some(dir) => cache.with_cache_dir(&dir),
        None => cache,
    };
    let cache = match setting(keys::LOCALE) {
        Some(locale) => cache.with_locale(&locale),
        None => cache,
    };

    cache
}

/// The app's collab session.
fn session() -> std::sync::MutexGuard<'static, SessionController> {
    static SESSION: LazyLock<Mutex<SessionController>> = LazyLock::new(|| Mutex::new(SessionController::new()));

    SESSION.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The session's link, inert while there is none.
pub fn collab_link() -> CollabLink {
    session().link()
}

/// The session's link, replaced each time a session starts or ends. A
/// viewport carries what it drew onto the new link.
pub fn collab_links() -> watch::Receiver<CollabLink> {
    static LINKS: LazyLock<watch::Sender<CollabLink>> = LazyLock::new(|| watch::channel(collab_link()).0);

    LINKS.subscribe()
}

/// The battles the session is playing, as its host has told everyone.
pub fn shared_windows() -> Vec<SharedWindow> {
    session().shared_windows()
}
