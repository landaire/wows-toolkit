//! Content-addressed storage for deduplicated VFS file storage.
//!
//! Files are stored by truncated SHA-256 hash in a git-style fanout directory:
//! `common/ab/cdef1234567890ab1234`
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

/// Directory name of the content-addressed store within a dump base. All builds
/// in a dump base share this one store, deduplicating files across versions.
pub const CAS_DIR: &str = "common";

/// Legacy name of the content-addressed store, migrated to [`CAS_DIR`].
pub const LEGACY_CAS_DIR: &str = "vfs_common";

/// Path to the content-addressed store within a dump base.
pub fn cas_root(output_base: &Path) -> PathBuf {
    output_base.join(CAS_DIR)
}

/// Compute a truncated SHA-256 hash of the given data.
/// Returns a lowercase hex string of `HASH_LEN` characters.
pub fn hash_bytes(data: &[u8]) -> String {
    let digest = Sha256::digest(data);
    let full_hex = format!("{digest:x}");
    full_hex[..HASH_LEN].to_string()
}

/// Compute the truncated SHA-256 hash of a file's contents.
///
/// Streams the file rather than reading it whole: content objects run to
/// hundreds of megabytes, and auditing a store means doing this thousands of
/// times.
pub fn hash_file(path: &Path) -> std::io::Result<String> {
    use std::io::Read;

    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let full_hex = format!("{:x}", hasher.finalize());
    Ok(full_hex[..HASH_LEN].to_string())
}

/// Returns the path within the CAS root for a given hash.
/// Uses the first 2 hex characters as a fanout directory.
/// e.g. `cas_root/ab/cdef1234567890ab1234`
pub fn cas_path(cas_root: &Path, hash: &str) -> PathBuf {
    cas_root.join(&hash[..2]).join(&hash[2..])
}

/// Whether a content object with the given hash is already stored.
pub fn object_exists(cas_root: &Path, hash: &str) -> bool {
    cas_path(cas_root, hash).exists()
}

/// Store data into the CAS. Returns the hash.
///
/// Idempotent, and safe to call concurrently for the same content from several
/// threads or processes. Builds share most of their objects, so two downloads
/// running at once routinely reach this for the same hash; writing straight to
/// the final path would let both write it at once and leave an interleaved file
/// that [`object_exists`] then reports as present forever. Instead the bytes go
/// to a uniquely-named temporary in the same directory and are renamed onto the
/// final path, which is atomic within a directory on Windows and Unix alike: a
/// concurrent reader sees either no file or a complete one.
pub fn store(cas_root: &Path, data: &[u8]) -> Result<String, rootcause::Report> {
    let hash = hash_bytes(data);
    let path = cas_path(cas_root, &hash);
    if path.exists() {
        return Ok(hash);
    }
    let Some(parent) = path.parent() else {
        bail!("CAS object path {} has no parent directory", path.display());
    };
    std::fs::create_dir_all(parent).attach_with(|| format!("Failed to create CAS directory {}", parent.display()))?;

    let temp_path = parent.join(temp_name(&hash));
    std::fs::write(&temp_path, data).attach_with(|| format!("Failed to write CAS object {}", temp_path.display()))?;

    // Losing the rename race is success: the destination is named after the
    // content hash, so whatever got there first holds these exact bytes.
    match std::fs::rename(&temp_path, &path) {
        Ok(()) => Ok(hash),
        Err(_) if path.exists() => {
            let _ = std::fs::remove_file(&temp_path);
            Ok(hash)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&temp_path);
            Err(e).attach_with(|| format!("Failed to store CAS object {}", path.display()))?
        }
    }
}

/// Name for a partially-written object, unique per writer so two threads or
/// processes storing the same content never share a temporary.
fn temp_name(hash: &str) -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let ticket = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!(".{}.{}.{}.tmp", &hash[2..], std::process::id(), ticket)
}

/// Create a symlink from `link_path` pointing to the CAS object.
///
/// Uses a relative symlink so that the archive is relocatable and works
/// when created from WSL but consumed on Windows (or vice versa).
/// Returns an error if the symlink cannot be created.
pub fn link_file(cas_root: &Path, hash: &str, link_path: &Path) -> Result<(), rootcause::Report> {
    let target = cas_path(cas_root, hash);
    if let Some(parent) = link_path.parent() {
        std::fs::create_dir_all(parent)
            .attach_with(|| format!("Failed to create parent directory {}", parent.display()))?;
    }

    // Compute a relative path from the link's parent directory to the CAS object.
    // Both paths share the same archive root, so we walk up from the link and back
    // down to the target.
    let link_parent = link_path.parent().unwrap_or(Path::new("."));
    let rel_target = relative_path(link_parent, &target);

    // Replace any existing entry so re-extraction (e.g. completing a build's gui
    // dir) is idempotent rather than failing on the already-present link.
    if link_path.symlink_metadata().is_ok() {
        let _ = std::fs::remove_file(link_path);
    }

    try_symlink(&rel_target, link_path)
        .attach_with(|| format!("Failed to create symlink {} -> {}", link_path.display(), rel_target.display()))?;
    Ok(())
}

/// The content hash a link into the store points at, or `None` when the path is
/// not a symlink aimed at a [`CAS_DIR`] object.
///
/// A link's target names the object the extraction stored for that path, which
/// is the only record left when metadata never got the entry.
pub fn link_target_hash(link_path: &Path) -> Option<String> {
    let target = std::fs::read_link(link_path).ok()?;
    let mut components = target.components().rev();
    let name = components.next()?.as_os_str().to_str()?.to_string();
    let fanout = components.next()?.as_os_str().to_str()?;
    let store = components.next()?.as_os_str().to_str()?;
    if store != CAS_DIR && store != LEGACY_CAS_DIR {
        return None;
    }
    let hash = format!("{fanout}{name}");
    (hash.len() == HASH_LEN && hash.chars().all(|c| c.is_ascii_hexdigit())).then_some(hash)
}

/// Compute a relative path from `from_dir` to `to_path`.
fn relative_path(from_dir: &Path, to_path: &Path) -> PathBuf {
    // Canonicalize-lite: just use the paths as-is since they share a common root.
    let from_components: Vec<_> = from_dir.components().collect();
    let to_components: Vec<_> = to_path.components().collect();

    // Find the common prefix length
    let common = from_components.iter().zip(to_components.iter()).take_while(|(a, b)| a == b).count();

    // Walk up from `from_dir` for each remaining component
    let ups = from_components.len() - common;
    let mut rel = PathBuf::new();
    for _ in 0..ups {
        rel.push("..");
    }
    // Walk down to `to_path`
    for comp in &to_components[common..] {
        rel.push(comp);
    }
    rel
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

    /// Streaming a file in chunks must agree with hashing its bytes in one go,
    /// including across a buffer boundary.
    #[test]
    fn hashing_a_file_matches_hashing_its_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let cas_root = dir.path().join("common");

        let data: Vec<u8> = (0..200_000).map(|i| (i % 251) as u8).collect();
        let hash = store(&cas_root, &data).unwrap();

        assert_eq!(hash_file(&cas_path(&cas_root, &hash)).unwrap(), hash_bytes(&data));
    }

    #[test]
    fn hashing_a_missing_file_reports_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let err = hash_file(&dir.path().join("nothing")).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }

    #[test]
    fn store_and_retrieve() {
        let dir = tempfile::tempdir().unwrap();
        let cas_root = dir.path().join("common");

        let data = b"test file contents";
        let hash = store(&cas_root, data).unwrap();

        let stored_path = cas_path(&cas_root, &hash);
        assert!(stored_path.exists());
        assert_eq!(std::fs::read(&stored_path).unwrap(), data);

        // Idempotent
        let hash2 = store(&cas_root, data).unwrap();
        assert_eq!(hash, hash2);
    }

    /// The hazard is not two writers producing wrong bytes -- they write
    /// identical content, so interleaving them is invisible -- it is that a
    /// direct write publishes the object at its final path progressively.
    /// `object_exists` then reports it present from the first byte, a
    /// concurrent `store` returns success on its fast path, and every reader
    /// downstream (the CAS-backed VFS, `validate_cache`, the next build's
    /// dedup check) can read a file that is not all there.
    ///
    /// So a reader is what asserts here, because a reader is what breaks, and
    /// it samples the published length rather than reading the whole object:
    /// an 8 MiB read is far too slow to land inside the window it is looking
    /// for. Sampling the length catches it. Verified to fail against a direct
    /// write, which tears on the order of ten times per run.
    #[test]
    fn an_object_is_never_visible_before_it_is_complete() {
        use std::sync::atomic::AtomicBool;
        use std::sync::atomic::AtomicUsize;
        use std::sync::atomic::Ordering;

        let dir = tempfile::tempdir().unwrap();
        let cas_root = dir.path().join("common");

        // Large enough that publishing it is not instantaneous, so a reader
        // spinning alongside genuinely lands inside the write.
        let data: Vec<u8> = (0..8 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
        let expected = hash_bytes(&data);
        let expected_len = data.len() as u64;
        let path = cas_path(&cas_root, &expected);

        let torn = AtomicUsize::new(0);
        let complete = AtomicUsize::new(0);
        let done = AtomicBool::new(false);

        std::thread::scope(|scope| {
            for _ in 0..3 {
                let path = path.clone();
                let (torn, complete, done) = (&torn, &complete, &done);
                scope.spawn(move || {
                    loop {
                        // Read after sampling the flag, so the last pass always
                        // sees the finished object and `complete` cannot be 0
                        // just because the writer won the race.
                        let finished = done.load(Ordering::Acquire);
                        if let Ok(meta) = std::fs::metadata(&path) {
                            if meta.len() == expected_len {
                                complete.fetch_add(1, Ordering::Relaxed);
                            } else {
                                torn.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                        if finished {
                            break;
                        }
                    }
                });
            }

            // Removed between rounds so the `path.exists()` fast path does not
            // turn every round after the first into a no-op.
            for _ in 0..8 {
                let _ = std::fs::remove_file(&path);
                store(&cas_root, &data).unwrap();
            }
            done.store(true, Ordering::Release);
        });

        assert!(complete.load(Ordering::Relaxed) > 0, "readers never saw the object at all, so nothing was checked");
        assert_eq!(torn.load(Ordering::Relaxed), 0, "a reader saw the published object before all of it was there");

        let stored = std::fs::read(&path).unwrap();
        assert_eq!(hash_bytes(&stored), expected, "the stored object does not hash to its own name");

        // No half-written temporaries left behind, and exactly one object.
        let fanout = cas_root.join(&expected[..2]);
        let entries: Vec<_> = std::fs::read_dir(&fanout).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(entries.len(), 1, "expected one object in the fanout directory, found {entries:?}");
    }

    #[test]
    fn link_creates_readable_file() {
        let dir = tempfile::tempdir().unwrap();
        let cas_root = dir.path().join("common");

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
        let cas_root = dir.path().join("common");

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
