//! Client scripts, the `bin/<build>/res/scripts.zip` of each game build.
//!
//! A dump base keeps them in `scripts/`, laid out as the `scripts` release of
//! wows-replay-data: `<build>.zip` per build and `index.toml`, the copy of the
//! published index last fetched or written. The client re-obfuscates its
//! bytecode and module names every build, so no file is shared between builds
//! and the zips are stored whole rather than in the content store.

#[cfg(feature = "download")]
pub mod fetch;
pub mod publish;

use std::collections::BTreeMap;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::SystemTime;

use serde::Deserialize;
use sha2::Digest;
use sha2::Sha256;

use crate::error::ScriptsError;

/// Directory of the scripts store within a dump base.
pub const SCRIPTS_DIR: &str = "scripts";

/// Name of the index, locally and as a release asset.
pub const INDEX_FILE: &str = "index.toml";

/// Steam filelist regex matching every build's scripts.zip in the client
/// depot. Depot paths use either separator.
pub const DEPOT_FILE_REGEX: &str = r"bin[/\\]\d+[/\\]res[/\\]scripts\.zip$";

/// Extension of a partial write in the store.
const TEMPORARY_EXTENSION: &str = "tmp";

/// Lowercase hexadecimal SHA-256 digest of a scripts.zip.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(transparent)]
pub struct Sha256Digest(String);

impl Sha256Digest {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn from_hasher(hasher: Sha256) -> Self {
        Self(format!("{:x}", hasher.finalize()))
    }
}

impl std::fmt::Display for Sha256Digest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// One build's scripts.zip as the index lists it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ScriptsEntry {
    pub version: String,
    pub sha256: Sha256Digest,
    pub size: u64,
}

/// Published scripts.zip files by build number.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScriptsIndex {
    pub builds: BTreeMap<u32, ScriptsEntry>,
}

impl ScriptsIndex {
    /// Parse index text. `location` names where it came from in errors.
    pub fn parse(location: &str, text: &str) -> Result<Self, ScriptsError> {
        #[derive(Deserialize)]
        struct IndexFile {
            #[serde(default)]
            builds: BTreeMap<String, ScriptsEntry>,
        }

        let file: IndexFile =
            toml::from_str(text).map_err(|source| ScriptsError::Index { location: location.to_owned(), source })?;

        let mut builds = BTreeMap::new();
        for (key, entry) in file.builds {
            let Ok(build) = key.parse::<u32>() else {
                return Err(ScriptsError::IndexKey { location: location.to_owned(), key });
            };
            builds.insert(build, entry);
        }

        Ok(Self { builds })
    }

    /// Index text, builds in ascending order.
    pub fn render(&self) -> String {
        let mut text = String::from("# Client scripts.zip per game build, as release asset <build>.zip.\n");
        for (build, entry) in &self.builds {
            let version = toml::Value::String(entry.version.clone());
            text.push_str(&format!(
                "\n[builds.{build}]\nversion = {version}\nsha256 = \"{}\"\nsize = {}\n",
                entry.sha256, entry.size
            ));
        }

        text
    }

    /// The local copy in `output_base`, or `None` when there is none.
    pub fn load(output_base: &Path) -> Result<Option<Self>, ScriptsError> {
        let path = index_path(output_base);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(ScriptsError::Io { path, source }),
        };

        Self::parse(&path.display().to_string(), &text).map(Some)
    }

    /// Replace the local copy in `output_base`.
    pub fn save(&self, output_base: &Path) -> Result<(), ScriptsError> {
        write_atomic(&index_path(output_base), self.render().as_bytes())
    }

    pub fn get(&self, build: u32) -> Option<&ScriptsEntry> {
        self.builds.get(&build)
    }
}

/// The scripts store within `output_base`.
pub fn scripts_dir(output_base: &Path) -> PathBuf {
    output_base.join(SCRIPTS_DIR)
}

/// Where the store keeps `build`'s scripts.zip.
pub fn zip_path(output_base: &Path, build: u32) -> PathBuf {
    scripts_dir(output_base).join(format!("{build}.zip"))
}

/// The scripts.zip a game install holds for `build`.
pub fn install_zip_path(game_dir: &Path, build: u32) -> PathBuf {
    game_dir.join("bin").join(build.to_string()).join("res").join("scripts.zip")
}

fn index_path(output_base: &Path) -> PathBuf {
    scripts_dir(output_base).join(INDEX_FILE)
}

/// `build`'s scripts.zip when the store holds it.
pub fn local_zip(output_base: &Path, build: u32) -> Option<PathBuf> {
    let path = zip_path(output_base, build);
    path.is_file().then_some(path)
}

/// Builds whose scripts.zip the store holds, ascending.
pub fn local_builds(output_base: &Path) -> Result<Vec<u32>, ScriptsError> {
    let dir = scripts_dir(output_base);
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => return Err(ScriptsError::Io { path: dir, source }),
    };

    let mut builds = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| ScriptsError::Io { path: dir.clone(), source })?;
        let name = entry.file_name();
        let Some(build) = name.to_str().and_then(|name| name.strip_suffix(".zip")).and_then(|stem| stem.parse().ok())
        else {
            continue;
        };
        builds.push(build);
    }
    builds.sort_unstable();

    Ok(builds)
}

/// Copy `source` into the store as `build`'s scripts.zip, replacing any copy
/// already there.
pub fn store_zip(output_base: &Path, build: u32, source: &Path) -> Result<PathBuf, ScriptsError> {
    if !is_zip(source)? {
        return Err(ScriptsError::NotZip { path: source.to_owned() });
    }

    let path = zip_path(output_base, build);
    let temporary = temporary_path(&path)?;
    let copied = std::fs::copy(source, &temporary)
        .map(|_| ())
        .map_err(|error| ScriptsError::Io { path: source.to_owned(), source: error });
    finish_temporary(copied, &temporary, &path)?;

    Ok(path)
}

/// Store the scripts.zip `game_dir` holds for `build`. `None` when the install
/// has none.
pub fn store_from_install(game_dir: &Path, build: u32, output_base: &Path) -> Result<Option<PathBuf>, ScriptsError> {
    let source = install_zip_path(game_dir, build);
    if !source.is_file() {
        return Ok(None);
    }

    store_zip(output_base, build, &source).map(Some)
}

/// Delete `build`'s scripts.zip from the store. Returns whether there was one.
pub fn remove(output_base: &Path, build: u32) -> Result<bool, ScriptsError> {
    let path = zip_path(output_base, build);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(ScriptsError::Io { path, source }),
    }
}

/// Delete partial writes in the store last modified more than `older_than`
/// ago, which a writer killed mid-download leaves behind. Returns how many
/// went.
pub fn remove_stale_temporaries(output_base: &Path, older_than: Duration) -> Result<usize, ScriptsError> {
    let dir = scripts_dir(output_base);
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(source) => return Err(ScriptsError::Io { path: dir, source }),
    };

    let now = SystemTime::now();
    let mut removed = 0;
    for entry in entries {
        let entry = entry.map_err(|source| ScriptsError::Io { path: dir.clone(), source })?;
        let path = entry.path();
        if path.extension().is_none_or(|extension| extension != TEMPORARY_EXTENSION) {
            continue;
        }
        let modified = match entry.metadata().and_then(|metadata| metadata.modified()) {
            Ok(modified) => modified,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(source) => return Err(ScriptsError::Io { path, source }),
        };
        // A time ahead of the clock is a write in progress as far as anyone can tell.
        if now.duration_since(modified).is_ok_and(|age| age > older_than) {
            match std::fs::remove_file(&path) {
                Ok(()) => removed += 1,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(source) => return Err(ScriptsError::Io { path, source }),
            }
        }
    }

    Ok(removed)
}

/// SHA-256 and size of a file, streamed.
pub fn digest_file(path: &Path) -> Result<(Sha256Digest, u64), ScriptsError> {
    let io = |source| ScriptsError::Io { path: path.to_owned(), source };

    let mut file = std::fs::File::open(path).map_err(io)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    let mut size = 0u64;
    loop {
        let read = file.read(&mut buffer).map_err(io)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        size += read as u64;
    }

    Ok((Sha256Digest::from_hasher(hasher), size))
}

/// Whether `path` ends in a zip end-of-central-directory record.
fn is_zip(path: &Path) -> Result<bool, ScriptsError> {
    // The record is 22 bytes plus a comment of at most 65535.
    const EOCD_SIGNATURE: [u8; 4] = [b'P', b'K', 5, 6];
    const MAX_EOCD_LEN: u64 = 22 + 65535;

    let io = |source| ScriptsError::Io { path: path.to_owned(), source };

    let mut file = std::fs::File::open(path).map_err(io)?;
    let len = file.metadata().map_err(io)?.len();
    let tail_len = len.min(MAX_EOCD_LEN);
    file.seek(SeekFrom::Start(len - tail_len)).map_err(io)?;
    let mut tail = Vec::with_capacity(tail_len as usize);
    file.read_to_end(&mut tail).map_err(io)?;

    Ok(tail.len() >= 22 && tail.windows(EOCD_SIGNATURE.len()).rev().any(|window| window == EOCD_SIGNATURE))
}

/// A temporary path beside `path`, unique per writer, with its parent created.
fn temporary_path(path: &Path) -> Result<PathBuf, ScriptsError> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let parent = path.parent().expect("a store path has a parent");
    std::fs::create_dir_all(parent).map_err(|source| ScriptsError::Io { path: parent.to_owned(), source })?;
    let name = path.file_name().expect("a store path has a file name").to_string_lossy();
    let unique = COUNTER.fetch_add(1, Ordering::Relaxed);

    Ok(parent.join(format!("{name}.{}.{unique}.{TEMPORARY_EXTENSION}", std::process::id())))
}

/// Move `temporary` onto `path` once `written` succeeded; remove it otherwise.
fn finish_temporary(written: Result<(), ScriptsError>, temporary: &Path, path: &Path) -> Result<(), ScriptsError> {
    let result = written.and_then(|()| {
        std::fs::rename(temporary, path).map_err(|source| ScriptsError::Io { path: path.to_owned(), source })
    });
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }

    result
}

fn write_atomic(path: &Path, data: &[u8]) -> Result<(), ScriptsError> {
    let temporary = temporary_path(path)?;
    let written =
        std::fs::write(&temporary, data).map_err(|source| ScriptsError::Io { path: temporary.clone(), source });

    finish_temporary(written, &temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PUBLISHED_INDEX: &str = "# Client scripts.zip per game build, from Steam depot 552993.\n\
        # Release asset <build>.zip of release `scripts`.\n\n\
        [builds.13533378]\nversion = \"15.10.0\"\nsha256 = \"4ecf40821474f4b8648006647a377913095d3bce8b56e46fa3e3415ff3f83ffe\"\nsize = 54694389\n\n\
        [builds.296659]\nversion = \"0.6.13\"\nsha256 = \"95a6662beb63d11fdda5e8f0360ebb4e425779ab13c7edced41340fefb0c33ee\"\nsize = 38303964\n";

    /// A zip archive holding one empty stored member, `a.pyc`.
    fn tiny_zip() -> Vec<u8> {
        let name = b"a.pyc";
        let mut zip = Vec::new();
        zip.extend_from_slice(b"PK\x03\x04");
        zip.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        zip.extend_from_slice(&[0; 12]);
        zip.extend_from_slice(&(name.len() as u16).to_le_bytes());
        zip.extend_from_slice(&[0, 0]);
        zip.extend_from_slice(name);
        let central = zip.len() as u32;
        zip.extend_from_slice(b"PK\x01\x02");
        zip.extend_from_slice(&[20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        zip.extend_from_slice(&[0; 12]);
        zip.extend_from_slice(&(name.len() as u16).to_le_bytes());
        zip.extend_from_slice(&[0; 12]);
        zip.extend_from_slice(&0u32.to_le_bytes());
        zip.extend_from_slice(name);
        let central_len = zip.len() as u32 - central;
        zip.extend_from_slice(b"PK\x05\x06");
        zip.extend_from_slice(&[0, 0, 0, 0, 1, 0, 1, 0]);
        zip.extend_from_slice(&central_len.to_le_bytes());
        zip.extend_from_slice(&central.to_le_bytes());
        zip.extend_from_slice(&[0, 0]);
        zip
    }

    #[test]
    fn parses_the_published_index() {
        let index = ScriptsIndex::parse("index.toml", PUBLISHED_INDEX).unwrap();

        assert_eq!(index.builds.keys().copied().collect::<Vec<_>>(), [296_659, 13_533_378]);
        let entry = index.get(13_533_378).unwrap();
        assert_eq!(entry.version, "15.10.0");
        assert_eq!(entry.size, 54_694_389);
        assert_eq!(entry.sha256.as_str(), "4ecf40821474f4b8648006647a377913095d3bce8b56e46fa3e3415ff3f83ffe");
    }

    #[test]
    fn rendered_index_lists_builds_numerically_and_parses_back() {
        let index = ScriptsIndex::parse("index.toml", PUBLISHED_INDEX).unwrap();

        let text = index.render();

        assert_eq!(
            text,
            "# Client scripts.zip per game build, as release asset <build>.zip.\n\n\
             [builds.296659]\nversion = \"0.6.13\"\nsha256 = \"95a6662beb63d11fdda5e8f0360ebb4e425779ab13c7edced41340fefb0c33ee\"\nsize = 38303964\n\n\
             [builds.13533378]\nversion = \"15.10.0\"\nsha256 = \"4ecf40821474f4b8648006647a377913095d3bce8b56e46fa3e3415ff3f83ffe\"\nsize = 54694389\n"
        );
        assert_eq!(ScriptsIndex::parse("rendered", &text).unwrap(), index);
    }

    #[test]
    fn a_non_numeric_build_key_is_rejected() {
        let text = "[builds.latest]\nversion = \"15.10.0\"\nsha256 = \"aa\"\nsize = 1\n";

        let error = ScriptsIndex::parse("index.toml", text).unwrap_err();

        assert!(matches!(error, ScriptsError::IndexKey { key, .. } if key == "latest"));
    }

    #[test]
    fn stores_an_install_zip_and_lists_it() {
        let install = tempfile::tempdir().unwrap();
        let base = tempfile::tempdir().unwrap();
        let source = install_zip_path(install.path(), 13_533_378);
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        std::fs::write(&source, tiny_zip()).unwrap();

        let stored = store_from_install(install.path(), 13_533_378, base.path()).unwrap().unwrap();

        assert_eq!(stored, base.path().join("scripts/13533378.zip"));
        assert_eq!(std::fs::read(&stored).unwrap(), tiny_zip());
        assert_eq!(local_builds(base.path()).unwrap(), [13_533_378]);
        assert_eq!(local_zip(base.path(), 13_533_378), Some(stored));
    }

    #[test]
    fn an_install_without_scripts_stores_nothing() {
        let install = tempfile::tempdir().unwrap();
        let base = tempfile::tempdir().unwrap();

        assert_eq!(store_from_install(install.path(), 13_533_378, base.path()).unwrap(), None);
        assert_eq!(local_builds(base.path()).unwrap(), Vec::<u32>::new());
    }

    #[test]
    fn a_file_that_is_not_a_zip_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("scripts.zip");
        std::fs::write(&source, b"<html>not found</html>").unwrap();

        let error = store_zip(dir.path(), 1, &source).unwrap_err();

        assert!(matches!(error, ScriptsError::NotZip { .. }));
        assert_eq!(local_zip(dir.path(), 1), None);
    }

    #[test]
    fn removing_reports_whether_a_zip_was_stored() {
        let base = tempfile::tempdir().unwrap();
        let source = base.path().join("source.zip");
        std::fs::write(&source, tiny_zip()).unwrap();
        store_zip(base.path(), 7, &source).unwrap();

        assert!(remove(base.path(), 7).unwrap());
        assert!(!remove(base.path(), 7).unwrap());
    }

    #[test]
    fn the_index_round_trips_through_the_store() {
        let base = tempfile::tempdir().unwrap();
        let index = ScriptsIndex::parse("index.toml", PUBLISHED_INDEX).unwrap();

        assert_eq!(ScriptsIndex::load(base.path()).unwrap(), None);
        index.save(base.path()).unwrap();

        assert_eq!(ScriptsIndex::load(base.path()).unwrap(), Some(index));
    }

    #[test]
    fn only_partial_writes_older_than_the_limit_are_removed() {
        let base = tempfile::tempdir().unwrap();
        let dir = scripts_dir(base.path());
        std::fs::create_dir_all(&dir).unwrap();
        let hour = Duration::from_secs(3600);
        let stale = dir.join("100.zip.1.0.tmp");
        let fresh = dir.join("200.zip.1.1.tmp");
        let zip = dir.join("300.zip");
        for path in [&stale, &fresh, &zip] {
            std::fs::write(path, b"x").unwrap();
        }
        for path in [&stale, &zip] {
            std::fs::File::options()
                .write(true)
                .open(path)
                .unwrap()
                .set_modified(SystemTime::now() - 2 * hour)
                .unwrap();
        }

        assert_eq!(remove_stale_temporaries(base.path(), hour).unwrap(), 1);

        assert!(!stale.exists());
        assert!(fresh.exists() && zip.exists());
    }

    #[test]
    fn digest_matches_sha256_of_the_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x");
        std::fs::write(&path, b"abc").unwrap();

        let (digest, size) = digest_file(&path).unwrap();

        assert_eq!(digest.as_str(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(size, 3);
    }
}
