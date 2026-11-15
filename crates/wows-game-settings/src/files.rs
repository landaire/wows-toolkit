use std::fmt;
use std::fs;
use std::io;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use jiff::Timestamp;
use tempfile::NamedTempFile;

/// A game build, the `<build>` in `bin/<build>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BuildNumber(pub u32);

/// Resolves install-relative paths to the files to read and write.
///
/// A read-only install under an overlay reads the upper layer first and
/// writes only to it. Valid only while the game is not running.
pub trait SettingsFiles {
    /// `None` when the file does not exist in the effective view.
    fn read_path(&self, relative: &Path) -> io::Result<Option<PathBuf>>;

    /// Where a new version of the file goes. Parent directories exist on
    /// return.
    fn write_path(&self, relative: &Path) -> io::Result<PathBuf>;
}

/// An install that is read and written in place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectFiles {
    root: PathBuf,
}

/// The files this crate reads, relative to the install root.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SettingsFile {
    Preferences,
    UserPreferences,
    /// `bin/<build>/res_mods/engine_config.xml`, read from
    /// `bin/<build>/res/engine_config.xml` while no override exists.
    EngineConfig {
        build: BuildNumber,
    },
    GameInfo,
}

impl fmt::Display for BuildNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl DirectFiles {
    pub fn new(root: PathBuf) -> DirectFiles {
        DirectFiles { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl SettingsFiles for DirectFiles {
    fn read_path(&self, relative: &Path) -> io::Result<Option<PathBuf>> {
        let path = self.root.join(relative);
        match fs::metadata(&path) {
            Ok(_) => Ok(Some(path)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn write_path(&self, relative: &Path) -> io::Result<PathBuf> {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        Ok(path)
    }
}

impl SettingsFile {
    /// The path the game reads.
    pub fn relative_path(self) -> PathBuf {
        match self {
            SettingsFile::Preferences => PathBuf::from("preferences.xml"),
            SettingsFile::UserPreferences => PathBuf::from("user_preferences.xml"),
            SettingsFile::EngineConfig { build } => {
                ["bin", &build.to_string(), "res_mods", "engine_config.xml"].iter().collect()
            }
            SettingsFile::GameInfo => PathBuf::from("game_info.xml"),
        }
    }

    /// The file the game falls back to when [`Self::relative_path`] is absent.
    pub fn fallback_path(self) -> Option<PathBuf> {
        match self {
            SettingsFile::EngineConfig { build } => {
                Some(["bin", &build.to_string(), "res", "engine_config.xml"].iter().collect())
            }
            SettingsFile::Preferences | SettingsFile::UserPreferences | SettingsFile::GameInfo => None,
        }
    }
}

impl fmt::Display for SettingsFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.relative_path().display())
    }
}

/// Copies `previous`, the current content of `target`, to a timestamped
/// backup beside it.
pub(crate) fn create_backup(target: &Path, previous: &str) -> io::Result<PathBuf> {
    let (directory, file_name) = backup_location(target)?;
    let backups = list_backups(directory, file_name)?;

    // Numbering continues past the newest backup of this second, so a name
    // freed by pruning is never reused for a newer backup.
    let stamp = Timestamp::now().strftime("%Y%m%dT%H%M%SZ").to_string();
    let mut attempt =
        backups.iter().filter(|(order, _)| order.stamp == stamp).map(|(order, _)| order.attempt + 1).max().unwrap_or(0);

    // A failed write leaves no file under a backup name: the content goes
    // to a temporary file that only becomes the backup once complete.
    let mut temporary = stage_replacement(target, previous)?;

    loop {
        let name = match attempt {
            0 => format!("{file_name}.{stamp}.bak"),
            attempt => format!("{file_name}.{stamp}-{attempt}.bak"),
        };
        let path = directory.join(name);
        match temporary.persist_noclobber(&path) {
            Ok(_) => return Ok(path),
            Err(error) if error.error.kind() == io::ErrorKind::AlreadyExists => {
                temporary = error.file;
                attempt += 1;
            }
            Err(error) => return Err(error.error),
        }
    }
}

/// Deletes all but the newest few backups of `target`.
pub(crate) fn prune_backups(target: &Path) -> io::Result<()> {
    const KEPT_BACKUPS: usize = 5;

    let (directory, file_name) = backup_location(target)?;
    let mut backups = list_backups(directory, file_name)?;

    backups.sort();
    let excess = backups.len().saturating_sub(KEPT_BACKUPS);
    for (_, path) in backups.into_iter().take(excess) {
        fs::remove_file(path)?;
    }

    Ok(())
}

/// Replaces `target` through a temporary file in the same directory, so a
/// reader sees the old file or the new one and never a partial write.
pub(crate) fn write_atomic(target: &Path, contents: &str) -> io::Result<()> {
    commit_replacement(stage_replacement(target, contents)?, target)
}

/// Writes and syncs `contents` to a temporary file beside `target`. The file
/// is removed if it is dropped before [`commit_replacement`].
pub(crate) fn stage_replacement(target: &Path, contents: &str) -> io::Result<NamedTempFile> {
    let Some(directory) = target.parent() else {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "settings path has no parent"));
    };

    let mut temporary = tempfile::Builder::new().prefix(".wows-toolkit-").suffix(".tmp").tempfile_in(directory)?;
    temporary.write_all(contents.as_bytes())?;
    temporary.as_file().sync_all()?;

    Ok(temporary)
}

pub(crate) fn commit_replacement(temporary: NamedTempFile, target: &Path) -> io::Result<()> {
    temporary.persist(target).map_err(|error| error.error)?;

    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct BackupOrder {
    stamp: String,
    attempt: u32,
}

/// Orders `<file>.<YYYYMMDDTHHMMSSZ>[-N].bak` names oldest first; other names
/// are not backups of `file_name`.
fn backup_order(file_name: &str, name: &str) -> Option<BackupOrder> {
    const STAMP_LEN: usize = "YYYYMMDDTHHMMSSZ".len();

    let rest = name.strip_prefix(file_name)?.strip_prefix('.')?.strip_suffix(".bak")?;
    let (stamp, attempt) = match rest.split_once('-') {
        Some((stamp, attempt)) => (stamp, attempt.parse().ok()?),
        None => (rest, 0),
    };

    let well_formed = stamp.len() == STAMP_LEN
        && stamp.bytes().enumerate().all(|(idx, byte)| match idx {
            8 => byte == b'T',
            15 => byte == b'Z',
            _ => byte.is_ascii_digit(),
        });
    if !well_formed {
        return None;
    }

    Some(BackupOrder { stamp: stamp.to_owned(), attempt })
}

fn backup_location(target: &Path) -> io::Result<(&Path, &str)> {
    match (target.parent(), target.file_name().and_then(|name| name.to_str())) {
        (Some(directory), Some(file_name)) => Ok((directory, file_name)),
        _ => Err(io::Error::new(io::ErrorKind::InvalidInput, "settings path has no file name")),
    }
}

fn list_backups(directory: &Path, file_name: &str) -> io::Result<Vec<(BackupOrder, PathBuf)>> {
    let mut backups = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(order) = name.to_str().and_then(|name| backup_order(file_name, name)) else {
            continue;
        };
        backups.push((order, entry.path()));
    }

    Ok(backups)
}
