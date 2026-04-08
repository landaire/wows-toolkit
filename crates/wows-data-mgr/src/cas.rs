//! Content-addressed storage for deduplicated VFS file storage.
//!
//! Files are stored by truncated SHA-256 hash in a git-style fanout directory:
//! `vfs_common/ab/cdef1234567890ab1234`
//!
//! Build directories contain symlinks (or copies as fallback) pointing to the
//! shared CAS objects, avoiding duplication across game versions.

use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

use rootcause::prelude::*;
use sha2::Digest;
use sha2::Sha256;

/// Number of hex characters to keep from the SHA-256 hash.
/// 20 hex chars = 80 bits, plenty for content addressing.
const HASH_LEN: usize = 20;

/// Compute a truncated SHA-256 hash of the given data.
/// Returns a lowercase hex string of `HASH_LEN` characters.
pub fn hash_bytes(data: &[u8]) -> String {
    let digest = Sha256::digest(data);
    let full_hex = format!("{digest:x}");
    full_hex[..HASH_LEN].to_string()
}

/// Returns the path within the CAS root for a given hash.
/// Uses the first 2 hex characters as a fanout directory.
/// e.g. `cas_root/ab/cdef1234567890ab1234`
pub fn cas_path(cas_root: &Path, hash: &str) -> PathBuf {
    cas_root.join(&hash[..2]).join(&hash[2..])
}

/// Store data into the CAS. Returns the hash.
/// Idempotent: skips writing if the hash file already exists.
pub fn store(cas_root: &Path, data: &[u8]) -> Result<String, rootcause::Report> {
    let hash = hash_bytes(data);
    let path = cas_path(cas_root, &hash);
    if path.exists() {
        return Ok(hash);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .attach_with(|| format!("Failed to create CAS directory {}", parent.display()))?;
    }
    std::fs::write(&path, data)
        .attach_with(|| format!("Failed to write CAS object {}", path.display()))?;
    Ok(hash)
}

/// Create a symlink from `link_path` pointing to the CAS object.
///
/// Uses `symlink_file` on Windows (requires Developer Mode) and `symlink` on Unix.
/// Returns an error if the symlink cannot be created.
pub fn link_file(cas_root: &Path, hash: &str, link_path: &Path) -> Result<(), rootcause::Report> {
    let target = cas_path(cas_root, hash);
    if let Some(parent) = link_path.parent() {
        std::fs::create_dir_all(parent)
            .attach_with(|| format!("Failed to create parent directory {}", parent.display()))?;
    }

    try_symlink(&target, link_path)
        .attach_with(|| format!("Failed to create symlink {} -> {}", link_path.display(), target.display()))?;
    Ok(())
}

/// Try to create a symlink. Returns Ok on success, Err on failure.
fn try_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        std::os::windows::fs::symlink_file(target, link)
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::os::unix::fs::symlink(target, link)
    }
}

/// Collect garbage: remove CAS objects not in the `live_hashes` set.
/// Returns the number of files removed.
pub fn gc(cas_root: &Path, live_hashes: &HashSet<String>) -> Result<usize, rootcause::Report> {
    let mut removed = 0;
    if !cas_root.exists() {
        return Ok(0);
    }

    // Walk fanout directories (2-char hex prefixes)
    for fanout_entry in std::fs::read_dir(cas_root)?.flatten() {
        if !fanout_entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let prefix = fanout_entry.file_name();
        let prefix_str = prefix.to_string_lossy();

        for file_entry in std::fs::read_dir(fanout_entry.path())?.flatten() {
            if !file_entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                continue;
            }
            let suffix = file_entry.file_name();
            let hash = format!("{}{}", prefix_str, suffix.to_string_lossy());

            if !live_hashes.contains(&hash) {
                if let Err(e) = std::fs::remove_file(file_entry.path()) {
                    tracing::warn!("Failed to remove CAS object {}: {e}", file_entry.path().display());
                } else {
                    removed += 1;
                }
            }
        }

        // Clean up empty fanout directory
        if std::fs::read_dir(fanout_entry.path()).map(|mut d| d.next().is_none()).unwrap_or(false) {
            let _ = std::fs::remove_dir(fanout_entry.path());
        }
    }

    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_deterministic_and_truncated() {
        let hash = hash_bytes(b"hello world");
        assert_eq!(hash.len(), HASH_LEN);
        assert_eq!(hash, hash_bytes(b"hello world"));
        assert_ne!(hash, hash_bytes(b"hello world!"));
    }

    #[test]
    fn store_and_retrieve() {
        let dir = tempfile::tempdir().unwrap();
        let cas_root = dir.path().join("vfs_common");

        let data = b"test file contents";
        let hash = store(&cas_root, data).unwrap();

        let stored_path = cas_path(&cas_root, &hash);
        assert!(stored_path.exists());
        assert_eq!(std::fs::read(&stored_path).unwrap(), data);

        // Idempotent
        let hash2 = store(&cas_root, data).unwrap();
        assert_eq!(hash, hash2);
    }

    #[test]
    fn link_creates_readable_file() {
        let dir = tempfile::tempdir().unwrap();
        let cas_root = dir.path().join("vfs_common");

        let data = b"linked file";
        let hash = store(&cas_root, data).unwrap();

        let link_path = dir.path().join("build/vfs/some/file.txt");
        link_file(&cas_root, &hash, &link_path).unwrap();

        assert!(link_path.exists());
        assert_eq!(std::fs::read(&link_path).unwrap(), data);
    }

    #[test]
    fn gc_removes_orphans() {
        let dir = tempfile::tempdir().unwrap();
        let cas_root = dir.path().join("vfs_common");

        let hash_a = store(&cas_root, b"file a").unwrap();
        let hash_b = store(&cas_root, b"file b").unwrap();

        let mut live = HashSet::new();
        live.insert(hash_a.clone());

        let removed = gc(&cas_root, &live).unwrap();
        assert_eq!(removed, 1);

        assert!(cas_path(&cas_root, &hash_a).exists());
        assert!(!cas_path(&cas_root, &hash_b).exists());
    }
}
