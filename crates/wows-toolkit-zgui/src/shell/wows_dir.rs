//! Adopting a game directory, from the settings drawer or the launcher.

use landaire_ui::ToastKey;
use wows_toolkit_viewmodel::settings::keys;
use zgui::reactive::prelude::Set;

use crate::shell::Shell;
use crate::shell::startup;

impl Shell {
    /// Adopts `path` as the game directory: saved, and the game data reloaded from it.
    ///
    /// A directory that holds no install is refused with the message that stays up until it is
    /// fixed, as the egui field refuses it: adopted, it would empty every tab that reads it.
    pub fn apply_wows_dir(self, path: String) {
        let invalid = startup::is_wows_dir_invalid(&path);
        self.settings_drawer.wows_dir_invalid.set(invalid);
        if invalid {
            startup::warn_wows_dir_invalid(self);
            return;
        }

        self.toasts().withdraw(&ToastKey::new(startup::STUCK_WOWS_DIR));

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
