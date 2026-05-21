//! In-memory PKG data source for the VFS.
//!
//! Holds each PKG volume's bytes in memory. Pure Rust with no filesystem or
//! mmap dependency, so the VFS works in any environment (wasm, embedded).
//! Callers supply the volume bytes up front (uploaded, fetched, or bundled).

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use vfs::VfsError;
use vfs::error::VfsErrorKind;

use crate::data::idx_vfs::Prime;

/// A PKG data source backed by in-memory buffers, one per volume file.
#[derive(Debug, Default, Clone)]
pub struct BytesPkgSource {
    volumes: HashMap<String, Arc<[u8]>>,
}

impl BytesPkgSource {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a volume's bytes under its file name (e.g. `content_0001.pkg`).
    pub fn insert(&mut self, volume: impl Into<String>, data: impl Into<Arc<[u8]>>) {
        self.volumes.insert(volume.into(), data.into());
    }

    /// Build a source from `(volume name, bytes)` pairs.
    pub fn from_volumes<N, D>(volumes: impl IntoIterator<Item = (N, D)>) -> Self
    where
        N: Into<String>,
        D: Into<Arc<[u8]>>,
    {
        let mut src = Self::new();
        for (name, data) in volumes {
            src.insert(name, data);
        }
        src
    }
}

/// A cloneable view into a registered volume's bytes.
#[derive(Clone, Debug)]
pub struct BytesSlice {
    data: Arc<[u8]>,
    range: Range<usize>,
}

impl AsRef<[u8]> for BytesSlice {
    fn as_ref(&self) -> &[u8] {
        &self.data[self.range.clone()]
    }
}

impl Prime for BytesPkgSource {
    fn prime_volume(&self, volume: &str, range: Range<usize>) -> Result<impl AsRef<[u8]>, VfsError> {
        let data = self
            .volumes
            .get(volume)
            .ok_or_else(|| VfsError::from(VfsErrorKind::FileNotFound))?;
        if range.end > data.len() {
            return Err(VfsError::from(VfsErrorKind::Other(format!(
                "range {}..{} exceeds volume {volume} length {}",
                range.start,
                range.end,
                data.len()
            ))));
        }
        Ok(BytesSlice { data: Arc::clone(data), range })
    }
}

#[cfg(feature = "async_vfs")]
#[async_trait::async_trait]
impl crate::data::idx_vfs::AsyncPrime for BytesPkgSource {
    async fn prime_volume(&self, volume: &str, range: Range<usize>) -> Result<impl AsRef<[u8]>, VfsError> {
        Prime::prime_volume(self, volume, range)
    }
}
