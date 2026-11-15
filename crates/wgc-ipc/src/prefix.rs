//! The files `wgc_api.dll` checks to decide that WGC is installed, written
//! into a toolkit-owned Wine prefix. The relay writes the matching registry
//! value itself on start, inside the prefix's wineserver.

use std::fmt;
use std::io::ErrorKind;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use wgc_relay_protocol::GAME_CENTER_DIR;

use crate::buck_resources::BuckResources;
use crate::error::PrefixError;

const RELAY_FILE_NAME: &str = "wgc-relay.exe";

/// Overrides the directory the relay binaries are taken from.
pub const RELAY_DIR_ENV: &str = "WOWS_TOOLKIT_WGC_RELAY_DIR";

/// A Windows executable shipped beside the Linux toolkit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayBinary {
    Relay,
    Placeholder,
}

impl RelayBinary {
    pub fn file_name(self) -> &'static str {
        match self {
            RelayBinary::Relay => RELAY_FILE_NAME,
            RelayBinary::Placeholder => "wgc-placeholder.exe",
        }
    }
}

impl fmt::Display for RelayBinary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.file_name())
    }
}

/// The two Windows executables shipped with the Linux toolkit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayBinaries {
    pub relay: PathBuf,
    /// Installed as `wgc.exe`. `wgc_api.dll` rejects a WGC whose `wgc.exe`
    /// carries no version resource as outdated.
    pub placeholder: PathBuf,
}

impl RelayBinaries {
    pub fn new(relay: PathBuf, placeholder: PathBuf) -> RelayBinaries {
        RelayBinaries { relay, placeholder }
    }

    /// Looks in [`RELAY_DIR_ENV`], else as [`RelayBinaries::beside`] the
    /// running executable.
    pub fn locate() -> Result<RelayBinaries, PrefixError> {
        if let Some(dir) = std::env::var_os(RELAY_DIR_ENV) {
            return RelayBinaries::find(Path::new(&dir), None);
        }

        let exe = std::env::current_exe().map_err(PrefixError::CurrentExe)?;
        RelayBinaries::beside(&exe)
    }

    /// Looks in `exe`'s directory, else in the Buck resource database of an
    /// `exe` in a build tree.
    pub fn beside(exe: &Path) -> Result<RelayBinaries, PrefixError> {
        let dir = exe.parent().ok_or_else(|| PrefixError::CurrentExe(ErrorKind::NotFound.into()))?;
        let resources = BuckResources::load(exe, dir)?;

        RelayBinaries::find(dir, resources.as_ref())
    }

    fn find(dir: &Path, resources: Option<&BuckResources>) -> Result<RelayBinaries, PrefixError> {
        let path = |binary: RelayBinary| {
            let shipped = dir.join(binary.file_name());
            if shipped.is_file() {
                return Ok(shipped);
            }

            resources
                .and_then(|resources| resources.path(binary.file_name()))
                .filter(|path| path.is_file())
                .ok_or_else(|| PrefixError::BinaryMissing { binary, dir: dir.to_path_buf() })
        };

        Ok(RelayBinaries::new(path(RelayBinary::Relay)?, path(RelayBinary::Placeholder)?))
    }
}

/// The result of [`prepare_prefix`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedPrefix {
    /// The relay's copy inside the prefix. Launch this one: runners that
    /// replace `/usr` (pressure-vessel) cannot see the shipped copy.
    pub relay_exe: PathBuf,
}

/// Writes the fake WGC install into `drive_c`. Rewrites only files whose
/// content differs, so repeated launches touch nothing.
pub fn prepare_prefix(drive_c: &Path, binaries: &RelayBinaries) -> Result<PreparedPrefix, PrefixError> {
    let game_center = drive_c.join("ProgramData").join("Wargaming.net").join("GameCenter");

    // No trailing newline: WGC writes the bare path.
    write_if_changed(&game_center.join("data").join("wgc_path.dat"), GAME_CENTER_DIR.as_bytes())?;
    write_if_changed(&game_center.join("wgc.exe"), &read(&binaries.placeholder)?)?;

    let relay_exe = game_center.join(RELAY_FILE_NAME);
    write_if_changed(&relay_exe, &read(&binaries.relay)?)?;

    Ok(PreparedPrefix { relay_exe })
}

fn read(path: &Path) -> Result<Vec<u8>, PrefixError> {
    std::fs::read(path).map_err(|source| PrefixError::Read { path: path.to_path_buf(), source })
}

fn write_if_changed(path: &Path, contents: &[u8]) -> Result<(), PrefixError> {
    let write_error = |source| PrefixError::Write { path: path.to_path_buf(), source };

    match std::fs::read(path) {
        Ok(existing) if existing == contents => return Ok(()),
        Ok(_) => {}
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Err(source) => return Err(PrefixError::Read { path: path.to_path_buf(), source }),
    }

    let dir = path.parent().ok_or_else(|| write_error(ErrorKind::InvalidInput.into()))?;
    std::fs::create_dir_all(dir).map_err(write_error)?;

    // A half-written wgc.exe would fail the version check on the next launch,
    // and concurrent launches must not share a staging file.
    let mut staging = tempfile::NamedTempFile::new_in(dir).map_err(write_error)?;
    staging.write_all(contents).map_err(write_error)?;
    staging.persist(path).map_err(|err| write_error(err.error))?;

    Ok(())
}
