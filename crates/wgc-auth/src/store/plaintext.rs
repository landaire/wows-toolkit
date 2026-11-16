//! The unencrypted fallback: one JSON file in the app's data directory, readable only by the
//! user, used while no OS secret store exists.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;
use zeroize::Zeroize;
use zeroize::ZeroizeOnDrop;

use crate::error::AuthError;
use crate::store::Kind;

/// `<data>/launcher/sessions.json`: the secrets the keyring would hold, keyed by
/// `realm:account_id`. Created 0600 in a 0700 directory on Unix. Every access goes through
/// [`LockedPlaintext`], which holds an advisory lock on the sibling `sessions.json.lock`: a
/// session save, a game's token2 save and a migration to the keyring can run at once, in this
/// process or in another toolkit instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaintextFile {
    path: PathBuf,
}

impl PlaintextFile {
    pub fn in_dir(data_dir: &Path) -> PlaintextFile {
        PlaintextFile { path: data_dir.join("launcher").join("sessions.json") }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn lock_path(&self) -> PathBuf {
        self.path.with_extension("json.lock")
    }

    /// Blocks until no other handle, in any process, holds the lock file. The lock file stays
    /// in place: removing it would let two writers lock different files.
    pub(super) fn lock(&self) -> Result<LockedPlaintext<'_>, AuthError> {
        create_private_dir(self.dir()?).map_err(|source| self.io_error(source))?;

        let lock_path = self.lock_path();
        let file = open_lock_file(&lock_path)
            .and_then(|file| file.lock().map(|()| file))
            .map_err(|source| AuthError::PlaintextStore { path: lock_path, source })?;

        Ok(LockedPlaintext { file: self, _lock: file })
    }

    fn dir(&self) -> Result<&Path, AuthError> {
        self.path.parent().ok_or_else(|| self.io_error(std::io::Error::from(std::io::ErrorKind::InvalidInput)))
    }

    fn io_error(&self, source: std::io::Error) -> AuthError {
        AuthError::PlaintextStore { path: self.path.clone(), source }
    }
}

/// [`PlaintextFile`] while its lock is held, released on drop.
pub(super) struct LockedPlaintext<'a> {
    file: &'a PlaintextFile,
    _lock: fs::File,
}

impl LockedPlaintext<'_> {
    /// `None` when the file does not exist or holds nothing.
    pub(super) fn read(&self) -> Result<Option<PlaintextContents>, AuthError> {
        let path = &self.file.path;
        let mut bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(self.file.io_error(source)),
        };
        let parsed: Result<PlaintextContents, _> = serde_json::from_slice(&bytes);
        bytes.zeroize();
        let contents = parsed.map_err(|source| AuthError::PlaintextStoreFormat { path: path.clone(), source })?;

        Ok((!contents.is_empty()).then_some(contents))
    }

    pub(super) fn get(&self, kind: Kind, key: &str) -> Result<Option<String>, AuthError> {
        let contents = self.read()?;

        Ok(contents.and_then(|contents| contents.map(kind).get(key).map(|secret| secret.0.clone())))
    }

    pub(super) fn insert(&self, kind: Kind, key: &str, secret: &str) -> Result<(), AuthError> {
        let mut contents = self.read()?.unwrap_or_default();
        contents.map_mut(kind).insert(key.to_string(), Secret(secret.to_string()));

        self.write(&contents)
    }

    /// Deletes the file once its last entry goes.
    pub(super) fn remove(&self, kind: Kind, key: &str) -> Result<(), AuthError> {
        let Some(mut contents) = self.read()? else {
            return Ok(());
        };
        if contents.map_mut(kind).remove(key).is_none() {
            return Ok(());
        }
        if contents.is_empty() {
            return match fs::remove_file(&self.file.path) {
                Ok(()) => Ok(()),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(source) => Err(self.file.io_error(source)),
            };
        }

        self.write(&contents)
    }

    /// Writes a sibling temporary file and renames it over the file, so a crash never leaves a
    /// half-written one. The temporary file is created private; it is never wider first. The
    /// held lock keeps its fixed name to one writer, and lets the next write remove one a crash
    /// left.
    fn write(&self, contents: &PlaintextContents) -> Result<(), AuthError> {
        let file = self.file;
        let temporary = file.path.with_extension("json.tmp");
        match fs::remove_file(&temporary) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => return Err(file.io_error(source)),
        }

        let mut json = serde_json::to_vec(contents)
            .map_err(|source| AuthError::PlaintextStoreFormat { path: file.path.clone(), source })?;
        let written = create_private_file(&temporary).and_then(|mut out| {
            out.write_all(&json)?;
            out.sync_all()
        });
        json.zeroize();
        written.map_err(|source| file.io_error(source))?;

        fs::rename(&temporary, &file.path).map_err(|source| file.io_error(source))
    }
}

/// The file's contents. Every secret zeroizes on drop.
#[derive(Default, Serialize, Deserialize)]
pub(super) struct PlaintextContents {
    #[serde(default)]
    sessions: BTreeMap<String, Secret>,
    #[serde(default)]
    token2: BTreeMap<String, Secret>,
}

impl PlaintextContents {
    pub(super) fn sessions(&self) -> impl Iterator<Item = (&str, &str)> {
        self.sessions.iter().map(|(key, secret)| (key.as_str(), secret.0.as_str()))
    }

    pub(super) fn token2s(&self) -> impl Iterator<Item = (&str, &str)> {
        self.token2.iter().map(|(key, secret)| (key.as_str(), secret.0.as_str()))
    }

    pub(super) fn session(&self, key: &str) -> Option<&str> {
        self.sessions.get(key).map(|secret| secret.0.as_str())
    }

    fn is_empty(&self) -> bool {
        self.sessions.is_empty() && self.token2.is_empty()
    }

    fn map(&self, kind: Kind) -> &BTreeMap<String, Secret> {
        match kind {
            Kind::Session => &self.sessions,
            Kind::Token2 => &self.token2,
        }
    }

    fn map_mut(&mut self, kind: Kind) -> &mut BTreeMap<String, Secret> {
        match kind {
            Kind::Session => &mut self.sessions,
            Kind::Token2 => &mut self.token2,
        }
    }
}

#[derive(Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(transparent)]
struct Secret(String);

#[cfg(unix)]
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    use std::os::unix::fs::PermissionsExt;

    fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    // The directory may predate this file; it holds nothing else.
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dir)
}

#[cfg(unix)]
fn create_private_file(path: &Path) -> std::io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;

    fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)
}

#[cfg(not(unix))]
fn create_private_file(path: &Path) -> std::io::Result<fs::File> {
    fs::OpenOptions::new().write(true).create_new(true).open(path)
}

#[cfg(unix)]
fn open_lock_file(path: &Path) -> std::io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;

    fs::OpenOptions::new().write(true).create(true).truncate(false).mode(0o600).open(path)
}

#[cfg(not(unix))]
fn open_lock_file(path: &Path) -> std::io::Result<fs::File> {
    fs::OpenOptions::new().write(true).create(true).truncate(false).open(path)
}
