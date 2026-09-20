//! Opening `content/assets.bin` as a browsable filesystem.
//!
//! The package VFS arrives with the build; assets.bin is a single archive that
//! has to be read and parsed before its contents can be listed, which is why
//! the pane showing it loads separately from the one beside it.

use std::io::Read as _;
use wowsunpack::data::assets_bin_vfs::AssetsBinVfs;
use wowsunpack::vfs::VfsPath;

/// Where the archive sits inside a build's package VFS.
pub const ASSETS_BIN_PATH: &str = "content/assets.bin";

#[derive(Debug, thiserror::Error)]
pub enum AssetsBinError {
    #[error("the build has no {ASSETS_BIN_PATH}")]
    NotFound {
        #[source]
        source: wowsunpack::vfs::VfsError,
    },
    #[error("{ASSETS_BIN_PATH} could not be read")]
    Read {
        #[source]
        source: wowsunpack::vfs::VfsError,
    },
    #[error("{ASSETS_BIN_PATH} could not be parsed")]
    Parse(String),
}

/// Reads and parses the archive out of `package_vfs`, yielding a VFS over its
/// contents.
///
/// Reads the whole file: the parser indexes the archive as one buffer, and the
/// pane that asked for it is about to list all of it anyway. Callers run this
/// off the UI thread.
pub fn open(package_vfs: &VfsPath) -> Result<VfsPath, AssetsBinError> {
    let archive = package_vfs.join(ASSETS_BIN_PATH).map_err(|source| AssetsBinError::NotFound { source })?;

    let mut bytes = Vec::new();
    archive
        .open_file()
        .and_then(|mut file| {
            file.read_to_end(&mut bytes)?;
            Ok(())
        })
        .map_err(|source| AssetsBinError::Read { source })?;

    let parsed = AssetsBinVfs::new(bytes).map_err(|err| AssetsBinError::Parse(err.to_string()))?;
    Ok(VfsPath::new(parsed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use wowsunpack::vfs::MemoryFS;

    #[test]
    fn a_build_without_the_archive_reports_it_as_missing_rather_than_a_read_failure() {
        let root: VfsPath = MemoryFS::new().into();
        root.join("content").unwrap().create_dir_all().unwrap();

        let error = open(&root).expect_err("there is no archive to open");
        assert!(matches!(error, AssetsBinError::Read { .. }), "an absent file fails on the read, not the join");
    }

    #[test]
    fn a_file_that_is_not_an_archive_reports_a_parse_failure() {
        let root: VfsPath = MemoryFS::new().into();
        root.join("content").unwrap().create_dir_all().unwrap();
        root.join(ASSETS_BIN_PATH).unwrap().create_file().unwrap().write_all(b"not an archive").unwrap();

        let error = open(&root).expect_err("the bytes are not an archive");
        assert!(matches!(error, AssetsBinError::Parse(_)));
    }
}
