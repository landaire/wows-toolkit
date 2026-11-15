//! The resource database Buck writes beside a binary that declares
//! `resources`: `<exe>.resources.json`, mapping each resource name to its path
//! relative to the binary's directory.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::io::ErrorKind;
use std::path::Path;
use std::path::PathBuf;

use crate::error::PrefixError;

pub(crate) struct BuckResources {
    dir: PathBuf,
    entries: HashMap<String, PathBuf>,
}

impl BuckResources {
    /// `None` outside a build tree, where no database exists.
    pub(crate) fn load(exe: &Path, dir: &Path) -> Result<Option<BuckResources>, PrefixError> {
        let mut path = exe.as_os_str().to_owned();
        path.push(".resources.json");
        let path = PathBuf::from(path);

        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(PrefixError::Read { path, source }),
        };
        let entries =
            serde_json::from_slice(&bytes).map_err(|source| PrefixError::ResourceDatabase { path, source })?;

        Ok(Some(BuckResources { dir: dir.to_path_buf(), entries }))
    }

    /// Names carry the declaring package (`crates/wows-toolkit-zgui/wgc-relay.exe`),
    /// so only the final component is matched.
    pub(crate) fn path(&self, file_name: &str) -> Option<PathBuf> {
        self.entries
            .iter()
            .find(|(name, _)| Path::new(name).file_name() == Some(OsStr::new(file_name)))
            .map(|(_, relative)| self.dir.join(relative))
    }
}
