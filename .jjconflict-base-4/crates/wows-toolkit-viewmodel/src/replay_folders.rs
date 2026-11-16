//! The replay directories a listing keeps across sessions: those pinned as tabs and those
//! opened recently.

use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

/// The settings row the pinned directories are kept in, in tab order.
pub const PINNED_FOLDERS: &str = "gpui.pinned_replay_directories";

/// The settings row the recently opened directories are kept in, most recent first.
pub const FOLDER_HISTORY: &str = "gpui.replay_folder_history";

/// The pinned directories in tab order, as kept in the settings.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PinnedFolders(pub Vec<PathBuf>);

/// The directories most recently opened, most recent first, as kept in the settings.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FolderHistory(Vec<PathBuf>);

impl FolderHistory {
    const LIMIT: usize = 10;

    pub fn folders(&self) -> &[PathBuf] {
        &self.0
    }

    /// Puts `root` first, once.
    pub fn record(&mut self, root: PathBuf) {
        self.0.retain(|held| *held != root);
        self.0.insert(0, root);
        self.0.truncate(Self::LIMIT);
    }

    /// Drops `root`, saying whether it was there.
    pub fn forget(&mut self, root: &Path) -> bool {
        let before = self.0.len();
        self.0.retain(|held| held != root);

        self.0.len() != before
    }

    /// Puts `new` where `old` was, once. Says whether `old` was there.
    pub fn replace(&mut self, old: &Path, new: &Path) -> bool {
        let Some(at) = self.0.iter().position(|held| held == old) else { return false };

        self.0[at] = new.to_path_buf();
        let mut kept = false;
        self.0.retain(|held| held != new || !std::mem::replace(&mut kept, true));

        true
    }

    /// Puts `older` after what was recorded here, as a stored list read after some directories
    /// were already opened this session.
    pub fn merge_older(&mut self, older: FolderHistory) {
        for root in older.0 {
            if !self.0.contains(&root) {
                self.0.push(root);
            }
        }
        self.0.truncate(Self::LIMIT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_moves_a_folder_first_and_keeps_ten() {
        let mut history = FolderHistory::default();
        for at in 0..12 {
            history.record(PathBuf::from(format!("/r/{at}")));
        }
        history.record(PathBuf::from("/r/5"));

        assert_eq!(history.folders().len(), 10);
        assert_eq!(history.folders()[0], PathBuf::from("/r/5"));
        assert_eq!(history.folders().iter().filter(|held| held.as_path() == Path::new("/r/5")).count(), 1);
    }

    #[test]
    fn replacing_keeps_the_place_and_drops_a_second_copy() {
        let mut history = FolderHistory(vec![PathBuf::from("/a"), PathBuf::from("/b"), PathBuf::from("/c")]);

        assert!(history.replace(Path::new("/a"), Path::new("/c")));
        assert_eq!(history.folders(), [PathBuf::from("/c"), PathBuf::from("/b")]);
        assert!(!history.replace(Path::new("/x"), Path::new("/y")));
    }

    #[test]
    fn an_older_list_goes_after_this_session() {
        let mut history = FolderHistory(vec![PathBuf::from("/new")]);
        history.merge_older(FolderHistory(vec![PathBuf::from("/old"), PathBuf::from("/new")]));

        assert_eq!(history.folders(), [PathBuf::from("/new"), PathBuf::from("/old")]);
    }
}
