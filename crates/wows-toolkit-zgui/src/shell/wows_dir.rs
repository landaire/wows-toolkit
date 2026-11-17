//! Adopting a game directory, from the settings drawer or the launcher.

use std::path::Path;

use landaire_ui::ToastKey;
use wows_toolkit_config::index::query;
use wows_toolkit_viewmodel::settings::keys;
use zgui::reactive::prelude::Set;
use zgui::task::spawn_detached;

use crate::shell::Shell;
use crate::shell::startup;

impl Shell {
    /// Adopts `path` as the game directory: saved, and the game data reloaded from it.
    ///
    /// A directory that holds no install is refused with the message that stays up until it is
    /// fixed, as the egui field refuses it: adopted, it would empty every tab that reads it.
    ///
    /// Saved under the spelling the replay index's Live source has for the same directory, so a
    /// directory picked through a symbolic link keeps matching the replays indexed under it.
    pub fn apply_wows_dir(self, path: String) {
        // Supersedes a look at the startup directory still out.
        let check = self.settings_drawer.begin_wows_dir_check();
        let invalid = startup::is_wows_dir_invalid(&path);
        self.settings_drawer.wows_dir_invalid.set(invalid);
        if invalid {
            startup::warn_wows_dir_invalid(self);
            return;
        }

        self.toasts().withdraw(&ToastKey::new(startup::STUCK_WOWS_DIR));

        let services = self.services();
        let Some(pool) = services.settings.pool().filter(|_| !path.is_empty()) else {
            self.adopt_wows_dir(path);
            return;
        };
        let given = path.clone();
        let respelled = services.on_tokio(async move { query::live_install_spelling(&pool, Path::new(&path)).await });
        // Detached: the drawer or launcher row that asked may close first. The check generation
        // drops the answer once the shell is gone or another directory has been applied since.
        spawn_detached(async move {
            let path = match respelled.await {
                Ok(Ok(respelled)) => respelled.to_string_lossy().into_owned(),
                Ok(Err(err)) => {
                    tracing::warn!(%err, "the replay index could not be read for the game directory's spelling");
                    given
                }
                Err(err) => {
                    tracing::warn!(%err, "the game directory's spelling was not looked up");
                    given
                }
            };
            if self.settings_drawer.is_latest_wows_dir_check(check) != Some(true) {
                return;
            }

            self.adopt_wows_dir(path);
        });
    }

    /// Saves `path` as the game directory and reloads the game data from it, unless it is the
    /// directory already set.
    fn adopt_wows_dir(self, path: String) {
        let settings = self.services().settings;
        let unchanged = settings.read_untracked(|settings| settings.wows_dir == path);
        if unchanged != Some(false) {
            return;
        }

        settings.edit(keys::WOWS_DIR, |settings| {
            settings.wows_dir = path.clone();
            path
        });
        if let Some(request) = settings.read_untracked(startup::game_data_request) {
            self.services().game_data.adopt(request);
        }
    }
}
