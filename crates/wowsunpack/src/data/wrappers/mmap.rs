//! Memory-mapped PKG file source for the VFS.
//!
//! Lazily loads and caches memory-mapped PKG files, providing raw byte access
//! to volume data via the [`Prime`] trait.

use std::collections::HashMap;
use std::fs::File;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use memmap2::MmapOptions;
use vfs::VfsError;
use vfs::error::VfsErrorKind;

use crate::data::idx_vfs::Prime;

/// A data source backed by memory-mapped `.pkg` files.
///
/// Lazily maps PKG files on first access and caches them for subsequent reads.
#[derive(Debug)]
pub struct MmapPkgSource {
    pkgs_dir: PathBuf,
    pkgs: RwLock<HashMap<String, Arc<memmap2::Mmap>>>,
}

impl MmapPkgSource {
    /// Create a new source pointing at the given directory of `.pkg` files.
    pub fn new<P: AsRef<Path>>(pkgs_dir: P) -> Self {
        Self { pkgs_dir: pkgs_dir.as_ref().to_owned(), pkgs: Default::default() }
    }

    /// Ensure the named PKG file is loaded and memory-mapped.
    fn ensure_loaded(&self, volume: &str) -> Result<(), VfsError> {
        {
            let pkgs = self.pkgs.read().unwrap();
            if pkgs.contains_key(volume) {
                return Ok(());
            }
        }

        let pkg_path = self.pkgs_dir.join(volume);
        if !pkg_path.exists() {
            return Err(VfsError::from(VfsErrorKind::FileNotFound));
        }

        let file = File::open(&pkg_path).map_err(|e| VfsError::from(VfsErrorKind::IoError(e)))?;
        let mmap = unsafe { MmapOptions::new().map(&file) }.map_err(|e| VfsError::from(VfsErrorKind::IoError(e)))?;

        self.pkgs.write().unwrap().insert(volume.to_string(), Arc::new(mmap));

        Ok(())
    }

    /// Get an Arc to the mmap for the named volume.
    fn get_mmap(&self, volume: &str) -> Result<Arc<memmap2::Mmap>, VfsError> {
        self.ensure_loaded(volume)?;
        let pkgs = self.pkgs.read().unwrap();
        Ok(Arc::clone(pkgs.get(volume).unwrap()))
    }
}

/// A cloneable slice of an Arc'd mmap, used to return data from behind the RwLock.
#[derive(Clone, Debug)]
pub struct MmapSlice {
    mmap: Arc<memmap2::Mmap>,
    range: Range<usize>,
}

impl AsRef<[u8]> for MmapSlice {
    fn as_ref(&self) -> &[u8] {
        &self.mmap[self.range.clone()]
    }
}

impl Prime for MmapPkgSource {
    fn prime_volume(&self, volume: &str, range: Range<usize>) -> Result<impl AsRef<[u8]>, VfsError> {
        let mmap = self.get_mmap(volume)?;
        Ok(MmapSlice { mmap, range })
    }
}

#[cfg(feature = "async_vfs")]
#[async_trait::async_trait]
impl crate::data::idx_vfs::AsyncPrime for MmapPkgSource {
    async fn prime_volume(&self, volume: &str, range: Range<usize>) -> Result<impl AsRef<[u8]>, VfsError> {
        // For mmap, async is the same as sync — the data is already in memory
        let mmap = self.get_mmap(volume)?;
        Ok(MmapSlice { mmap, range })
    }
}
