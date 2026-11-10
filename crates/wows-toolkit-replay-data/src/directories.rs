//! The replay directories a front end has open beside the game's own, and
//! which of them are pinned.
//!
//! Pinned directories are saved under one settings row, so a front end that
//! starts again reopens them. Unpinned ones last one run.

use std::path::Path;
use std::path::PathBuf;

use sqlx::SqlitePool;

/// The settings row the pinned directories are saved under.
pub const PINNED_DIRECTORIES_KEY: &str = "gpui.pinned_replay_directories";

/// One open replay directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayDirectory {
    pub root: PathBuf,
    pub pinned: bool,
}

/// What [`DirectoryRegistry::open`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opened {
    /// The directory was not open; it now is, at this position.
    Added(usize),
    /// The directory was already open, at this position.
    Existing(usize),
}

/// Why [`DirectoryRegistry::close`] left a directory open.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CloseRefusal {
    #[error("{} is pinned; unpin it before closing it", root.display())]
    Pinned { root: PathBuf },
    #[error("{} is not open", root.display())]
    NotOpen { root: PathBuf },
}

/// The open replay directories, in the order they were opened.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DirectoryRegistry {
    directories: Vec<ReplayDirectory>,
}

impl DirectoryRegistry {
    pub fn new() -> Self {
        Self { directories: Vec::new() }
    }

    /// The registry a run starts with: every saved pinned directory, open.
    pub fn from_pinned(pinned: Vec<PathBuf>) -> Self {
        let mut registry = Self::new();
        for root in pinned {
            registry.open(root, true);
        }

        registry
    }

    pub fn directories(&self) -> &[ReplayDirectory] {
        &self.directories
    }

    /// Opens `root`, or finds it already open. Opening an open directory as
    /// pinned pins it; opening a pinned one unpinned leaves it pinned.
    pub fn open(&mut self, root: PathBuf, pinned: bool) -> Opened {
        let root = canonical_root(root);
        if let Some(position) = self.position(&root) {
            self.directories[position].pinned |= pinned;
            return Opened::Existing(position);
        }

        self.directories.push(ReplayDirectory { root, pinned });

        Opened::Added(self.directories.len() - 1)
    }

    /// Closes `root`. A pinned directory stays open: unpinning it is a
    /// separate, deliberate act.
    pub fn close(&mut self, root: &Path) -> Result<(), CloseRefusal> {
        let root = canonical_root(root.to_path_buf());
        let Some(position) = self.position(&root) else {
            return Err(CloseRefusal::NotOpen { root });
        };
        if self.directories[position].pinned {
            return Err(CloseRefusal::Pinned { root });
        }

        self.directories.remove(position);

        Ok(())
    }

    /// Pins or unpins `root`. `false` when it is not open.
    pub fn set_pinned(&mut self, root: &Path, pinned: bool) -> bool {
        let root = canonical_root(root.to_path_buf());
        let Some(position) = self.position(&root) else { return false };
        self.directories[position].pinned = pinned;

        true
    }

    /// The pinned directories, in order, as they are saved.
    pub fn pinned_roots(&self) -> Vec<PathBuf> {
        self.directories.iter().filter(|directory| directory.pinned).map(|directory| directory.root.clone()).collect()
    }

    fn position(&self, root: &Path) -> Option<usize> {
        self.directories.iter().position(|directory| directory.root == root)
    }
}

/// The path a directory is known by.
///
/// Canonical, so aliases of one directory are one entry. A path that cannot be
/// resolved is kept as given, so a pinned removable volume is not discarded on
/// a start without it.
pub fn canonical_root(root: PathBuf) -> PathBuf {
    std::fs::canonicalize(&root).unwrap_or(root)
}

/// A short label for `root`: every directory above the last as its initial,
/// and the last cut to its ends when it is long.
pub fn shortened_path(root: &Path) -> String {
    const LONG_NAME: usize = 24;
    const KEPT_END: usize = 12;

    let parts: Vec<_> = root.components().collect();
    let mut result = PathBuf::new();
    for (index, component) in parts.iter().enumerate() {
        match component {
            std::path::Component::Normal(name) if index + 1 < parts.len() => {
                if let Some(initial) = name.to_string_lossy().chars().next() {
                    result.push(initial.to_string());
                }
            }
            std::path::Component::Normal(name) => {
                let name = name.to_string_lossy();
                let chars: Vec<_> = name.chars().collect();
                if chars.len() > LONG_NAME {
                    let head: String = chars[..KEPT_END].iter().collect();
                    let tail: String = chars[chars.len() - KEPT_END..].iter().collect();
                    result.push(format!("{head}..{tail}"));
                } else {
                    result.push(name.as_ref());
                }
            }
            _ => result.push(component.as_os_str()),
        }
    }

    let path = result.display().to_string();
    path.strip_prefix(r"\\?\").unwrap_or(&path).to_string()
}

/// The saved pinned directories. `None` when none were ever saved.
pub async fn load_pinned(
    pool: &SqlitePool,
) -> Result<Option<Vec<PathBuf>>, wows_toolkit_config::queries::SettingError> {
    wows_toolkit_config::queries::try_get_setting::<Vec<PathBuf>>(pool, PINNED_DIRECTORIES_KEY).await
}

/// Saves `pinned` as the pinned directories.
pub async fn save_pinned(pool: &SqlitePool, pinned: &[PathBuf]) -> Result<(), sqlx::Error> {
    wows_toolkit_config::queries::set_setting(pool, PINNED_DIRECTORIES_KEY, &pinned).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_an_open_directory_finds_it_and_pinning_sticks() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let mut registry = DirectoryRegistry::new();

        assert_eq!(registry.open(dir.path().to_path_buf(), false), Opened::Added(0));
        assert_eq!(registry.open(dir.path().join("."), true), Opened::Existing(0), "an alias is the same directory");
        assert!(registry.directories()[0].pinned);

        assert_eq!(registry.open(dir.path().to_path_buf(), false), Opened::Existing(0));
        assert!(registry.directories()[0].pinned, "opening unpinned does not unpin");
    }

    #[test]
    fn a_pinned_directory_is_not_closed() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let mut registry = DirectoryRegistry::from_pinned(vec![dir.path().to_path_buf()]);
        let root = canonical_root(dir.path().to_path_buf());

        assert_eq!(registry.close(dir.path()), Err(CloseRefusal::Pinned { root: root.clone() }));
        assert!(registry.set_pinned(dir.path(), false));
        assert_eq!(registry.close(dir.path()), Ok(()));
        assert!(registry.directories().is_empty());
        assert_eq!(registry.close(dir.path()), Err(CloseRefusal::NotOpen { root }));
    }

    #[test]
    fn only_pinned_directories_are_saved_in_order() {
        let first = tempfile::tempdir().expect("a temporary directory");
        let second = tempfile::tempdir().expect("a temporary directory");
        let third = tempfile::tempdir().expect("a temporary directory");
        let mut registry = DirectoryRegistry::new();
        registry.open(first.path().to_path_buf(), true);
        registry.open(second.path().to_path_buf(), false);
        registry.open(third.path().to_path_buf(), true);

        assert_eq!(
            registry.pinned_roots(),
            vec![canonical_root(first.path().to_path_buf()), canonical_root(third.path().to_path_buf())]
        );
    }

    /// A missing directory keeps its path, so a pinned removable volume
    /// survives a start without it.
    #[test]
    fn a_directory_that_is_not_there_keeps_its_path() {
        let missing = PathBuf::from("/definitely/not/a/mounted/volume/replays");
        assert_eq!(canonical_root(missing.clone()), missing);
    }

    #[test]
    fn a_long_path_is_shortened_to_initials_and_the_ends_of_its_last_name() {
        assert_eq!(shortened_path(Path::new("/home/user/replays")), "/h/u/replays");
        assert_eq!(shortened_path(Path::new("/games/abcdefghijklmnopqrstuvwxyz0123")), "/g/abcdefghijkl..stuvwxyz0123");
    }
}
