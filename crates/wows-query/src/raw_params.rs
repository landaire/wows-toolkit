//! Raw GameParams entries, stored per build as deflated JSON records with an
//! in-memory index.
//!
//! A store lives in `<cache_root>/raw-params-v1/<build>/`: `records.bin`
//! holds one deflate stream per param, `index.json` the index. It is built in
//! a sibling staging directory, marked with `.complete` and renamed into
//! place; a directory without the marker is treated as absent. Every open and
//! query updates the modification time of `.last-used`, which cache eviction
//! reads. The index stays in memory; records are read from disk per query
//! and not cached.

use std::collections::HashMap;
use std::fs::File;
use std::io::BufWriter;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::SystemTime;

use flate2::Compression;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use serde::Deserialize;
use serde::Serialize;
use wows_core::game_types::GameParamId;
use wowsunpack::game_params::convert::base_params_json;
use wowsunpack::game_params::convert::game_params_to_pickle;

use crate::error::RawParamsError;

/// Cache kind directory of raw GameParams stores under a cache root. The
/// suffix is the store format, so programs built with different formats
/// sharing a cache root do not replace each other's stores.
pub const RAW_PARAMS_KIND: &str = "raw-params-v1";

const RECORDS_FILE: &str = "records.bin";
const INDEX_FILE: &str = "index.json";
pub(crate) const COMPLETE_MARKER: &str = ".complete";
pub(crate) const LAST_USED_MARKER: &str = ".last-used";

/// Index entry of one GameParams entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawParamEntry {
    /// Param name, e.g. `PASC020_Des_Moines_1948`.
    pub name: String,
    /// Short index, e.g. `PASC020`.
    pub index: Option<String>,
    pub id: Option<GameParamId>,

    /// `typeinfo.type`, e.g. `Ship` or `Projectile`.
    pub kind: Option<String>,
    /// `typeinfo.species`, e.g. `Cruiser` or `Artillery`.
    pub species: Option<String>,
    /// `typeinfo.nation`, e.g. `USA`.
    pub nation: Option<String>,
    /// The `level` field: tier for ships and modules.
    pub level: Option<u32>,

    location: RecordLocation,
}

/// Where a record sits in `records.bin`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct RecordLocation {
    offset: u64,
    compressed_len: u32,
    json_len: u32,
}

#[derive(Serialize, Deserialize)]
struct IndexFile {
    build: u32,
    records_len: u64,
    entries: Vec<RawParamEntry>,
}

/// Raw GameParams of one build: the index in memory, records on disk.
pub struct RawParams {
    dir: PathBuf,
    /// Held open, so the index always reads the file it was written with.
    records: File,
    entries: Vec<RawParamEntry>,
    by_name: HashMap<String, usize>,
    /// Upper-case short index to entry.
    by_index: HashMap<String, usize>,
    by_id: HashMap<GameParamId, usize>,
}

impl RawParams {
    /// Opens the store for `build` under `cache_root`, building it from
    /// `game_params_data` when absent or damaged.
    pub fn open_or_build(
        cache_root: &Path,
        build: u32,
        game_params_data: impl FnOnce() -> Result<Vec<u8>, RawParamsError>,
    ) -> Result<Self, RawParamsError> {
        let kind_dir = cache_root.join(RAW_PARAMS_KIND);
        let dir = kind_dir.join(build.to_string());
        match Self::open(&dir, build) {
            Ok(Some(store)) => return Ok(store),
            // Absent: build it below.
            Ok(None) => {}
            Err(error) => tracing::warn!(dir = %dir.display(), %error, "rebuilding damaged raw GameParams store"),
        }

        Self::build(&kind_dir, build, game_params_data()?)
    }

    /// Opens a complete store, or `None` when `dir` holds none.
    pub fn open(dir: &Path, build: u32) -> Result<Option<Self>, RawParamsError> {
        if !dir.join(COMPLETE_MARKER).is_file() {
            return Ok(None);
        }

        let index_path = dir.join(INDEX_FILE);
        let file = File::open(&index_path).map_err(|source| RawParamsError::Io { path: index_path.clone(), source })?;
        let index: IndexFile = serde_json::from_reader(std::io::BufReader::new(file))
            .map_err(|source| RawParamsError::Index { path: index_path.clone(), source })?;
        if index.build != build {
            return Err(RawParamsError::WrongBuild { path: index_path, expected: build, found: index.build });
        }

        let records_path = dir.join(RECORDS_FILE);
        let records =
            File::open(&records_path).map_err(|source| RawParamsError::Io { path: records_path.clone(), source })?;
        let records_len =
            records.metadata().map_err(|source| RawParamsError::Io { path: records_path.clone(), source })?.len();
        if records_len != index.records_len {
            return Err(RawParamsError::Truncated {
                path: records_path,
                expected: index.records_len,
                found: records_len,
            });
        }

        let store = Self::from_entries(dir.to_path_buf(), records, index.entries);
        store.touch();

        Ok(Some(store))
    }

    /// Converts every base param of a `GameParams.data` blob and writes the
    /// store for `build` into `kind_dir`. A complete store another process
    /// published meanwhile is kept and opened instead.
    pub fn build(kind_dir: &Path, build: u32, game_params_data: Vec<u8>) -> Result<Self, RawParamsError> {
        let started = std::time::Instant::now();
        std::fs::create_dir_all(kind_dir)
            .map_err(|source| RawParamsError::Io { path: kind_dir.to_path_buf(), source })?;
        remove_stale_staging(kind_dir, build);

        let staging = staging_dir(kind_dir, build);
        std::fs::create_dir(&staging).map_err(|source| RawParamsError::Io { path: staging.clone(), source })?;
        let published = write_store(&staging, build, game_params_data)
            .and_then(|()| publish(&staging, &kind_dir.join(build.to_string()), build));
        if published.is_err() {
            // The staging directory is private to this build; nothing else reads it.
            let _ = std::fs::remove_dir_all(&staging);
        }
        let store = published?;

        tracing::info!(
            dir = %store.dir.display(),
            params = store.entries.len(),
            elapsed_ms = started.elapsed().as_millis(),
            "built raw GameParams store"
        );
        store.touch();

        Ok(store)
    }

    fn from_entries(dir: PathBuf, records: File, entries: Vec<RawParamEntry>) -> Self {
        let mut by_name = HashMap::with_capacity(entries.len());
        let mut by_index = HashMap::with_capacity(entries.len());
        let mut by_id = HashMap::with_capacity(entries.len());
        for (position, entry) in entries.iter().enumerate() {
            by_name.insert(entry.name.clone(), position);
            if let Some(index) = &entry.index {
                by_index.insert(index.to_ascii_uppercase(), position);
            }
            if let Some(id) = entry.id {
                by_id.insert(id, position);
            }
        }

        Self { dir, records, entries, by_name, by_index, by_id }
    }

    /// Every entry, in GameParams dictionary order.
    pub fn entries(&self) -> &[RawParamEntry] {
        &self.entries
    }

    /// The directory holding this store.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The entry with this name, short index or numeric id. Names and
    /// indexes match without regard to ASCII case.
    pub fn find(&self, key: &str) -> Option<&RawParamEntry> {
        let key = key.trim();
        let position = self
            .by_name
            .get(key)
            .or_else(|| self.by_index.get(&key.to_ascii_uppercase()))
            .or_else(|| key.parse::<u64>().ok().and_then(|id| self.by_id.get(&GameParamId::from(id))))
            .copied()
            .or_else(|| self.entries.iter().position(|entry| entry.name.eq_ignore_ascii_case(key)))?;

        self.entries.get(position)
    }

    /// The JSON text of `entry`.
    pub fn read(&self, entry: &RawParamEntry) -> Result<Vec<u8>, RawParamsError> {
        let location = entry.location;
        let io_error = |source| RawParamsError::Io { path: self.dir.join(RECORDS_FILE), source };

        let mut compressed = vec![0; location.compressed_len as usize];
        read_exact_at(&self.records, &mut compressed, location.offset).map_err(io_error)?;
        let mut json = Vec::with_capacity(location.json_len as usize);
        // A stream that does not inflate is damage, not an I/O failure.
        DeflateDecoder::new(compressed.as_slice())
            .read_to_end(&mut json)
            .map_err(|_| RawParamsError::DamagedRecord { name: entry.name.clone() })?;
        if json.len() != location.json_len as usize {
            return Err(RawParamsError::DamagedRecord { name: entry.name.clone() });
        }

        Ok(json)
    }

    /// Whether the store is still marked complete on disk. Cache eviction
    /// may have removed it, or a damaged record unmarked it.
    pub fn is_present(&self) -> bool {
        self.dir.join(COMPLETE_MARKER).is_file()
    }

    /// Unmarks the store so the next open rebuilds it.
    pub fn invalidate(&self) {
        let marker = self.dir.join(COMPLETE_MARKER);
        if let Err(error) = std::fs::remove_file(&marker) {
            tracing::warn!(path = %marker.display(), %error, "failed to unmark a damaged raw GameParams store");
        }
    }

    /// Marks the store as used now for cache eviction.
    pub fn touch(&self) {
        touch(&self.dir.join(LAST_USED_MARKER));
    }
}

/// Sets the modification time of `path` to now, creating it. A failure only
/// makes the store look older to eviction.
pub(crate) fn touch(path: &Path) {
    let touched = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
        .and_then(|file| file.set_modified(SystemTime::now()));
    if let Err(error) = touched {
        tracing::debug!(path = %path.display(), %error, "failed to update .last-used");
    }
}

#[cfg(unix)]
fn read_exact_at(file: &File, buffer: &mut [u8], offset: u64) -> std::io::Result<()> {
    std::os::unix::fs::FileExt::read_exact_at(file, buffer, offset)
}

#[cfg(windows)]
fn read_exact_at(file: &File, mut buffer: &mut [u8], mut offset: u64) -> std::io::Result<()> {
    while !buffer.is_empty() {
        match std::os::windows::fs::FileExt::seek_read(file, buffer, offset) {
            Ok(0) => return Err(std::io::ErrorKind::UnexpectedEof.into()),
            Ok(read) => {
                buffer = &mut buffer[read..];
                offset += read as u64;
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }

    Ok(())
}

fn staging_prefix(build: u32) -> String {
    format!(".staging-{build}-")
}

fn staging_dir(kind_dir: &Path, build: u32) -> PathBuf {
    let nanos = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_nanos());

    kind_dir.join(format!("{}{}-{nanos}", staging_prefix(build), std::process::id()))
}

/// Removes staging directories of `build` left by interrupted builds. A
/// build finishes well within the age limit, so a younger directory may
/// belong to a build still running in another process.
fn remove_stale_staging(kind_dir: &Path, build: u32) {
    const STALE_AFTER: Duration = Duration::from_secs(60 * 60);

    let Ok(listing) = std::fs::read_dir(kind_dir) else {
        return;
    };
    let prefix = staging_prefix(build);
    for entry in listing.flatten() {
        let is_staging = entry.file_name().to_str().is_some_and(|name| name.starts_with(&prefix));
        let is_stale = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age > STALE_AFTER);
        if is_staging && is_stale {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// Writes `records.bin`, `index.json` and `.last-used` into `dir`, synced to
/// disk so the complete marker never precedes the data.
fn write_store(dir: &Path, build: u32, game_params_data: Vec<u8>) -> Result<(), RawParamsError> {
    let root = game_params_to_pickle(game_params_data).map_err(RawParamsError::Decode)?;
    let params = base_params_json(&root).map_err(RawParamsError::Decode)?;

    let records_path = dir.join(RECORDS_FILE);
    let io_error = |path: &Path| {
        let path = path.to_path_buf();
        move |source| RawParamsError::Io { path, source }
    };
    let file = File::create(&records_path).map_err(io_error(&records_path))?;
    let mut records = BufWriter::new(file);

    let mut entries = Vec::new();
    let mut offset = 0u64;
    let mut json = Vec::new();
    let mut compressed = Vec::new();
    for (name, value) in params {
        json.clear();
        serde_json::to_writer(&mut json, &value)
            .map_err(|source| RawParamsError::Encode { name: name.clone(), source })?;

        compressed.clear();
        let mut encoder = DeflateEncoder::new(&mut compressed, Compression::default());
        encoder.write_all(&json).and_then(|()| encoder.finish().map(drop)).map_err(io_error(&records_path))?;
        records.write_all(&compressed).map_err(io_error(&records_path))?;

        let location = RecordLocation {
            offset,
            compressed_len: record_len(&name, compressed.len())?,
            json_len: record_len(&name, json.len())?,
        };
        offset += compressed.len() as u64;
        entries.push(entry_from_json(name, &value, location));
    }
    let records = records.into_inner().map_err(|error| io_error(&records_path)(error.into_error()))?;
    records.sync_all().map_err(io_error(&records_path))?;
    drop(root);

    let index = IndexFile { build, records_len: offset, entries };
    let index_path = dir.join(INDEX_FILE);
    let file = File::create(&index_path).map_err(io_error(&index_path))?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer(&mut writer, &index)
        .map_err(|source| RawParamsError::Index { path: index_path.clone(), source })?;
    let file = writer.into_inner().map_err(|error| io_error(&index_path)(error.into_error()))?;
    file.sync_all().map_err(io_error(&index_path))?;

    touch(&dir.join(LAST_USED_MARKER));

    Ok(())
}

fn record_len(name: &str, len: usize) -> Result<u32, RawParamsError> {
    u32::try_from(len).map_err(|_| RawParamsError::RecordTooLarge { name: name.to_owned(), len })
}

fn entry_from_json(name: String, value: &serde_json::Value, location: RecordLocation) -> RawParamEntry {
    let text = |value: Option<&serde_json::Value>| value.and_then(serde_json::Value::as_str).map(str::to_owned);
    let typeinfo = value.get("typeinfo");
    let field = |key: &str| typeinfo.and_then(|typeinfo| typeinfo.get(key));

    RawParamEntry {
        index: text(value.get("index")),
        id: value.get("id").and_then(serde_json::Value::as_u64).map(GameParamId::from),
        kind: text(field("type")),
        species: text(field("species")),
        nation: text(field("nation")),
        level: value.get("level").and_then(serde_json::Value::as_u64).and_then(|level| u32::try_from(level).ok()),
        name,
        location,
    }
}

/// Marks a finished staging directory complete, renames it to `final_dir`
/// and opens it. A complete store already at `final_dir`, published by
/// another process, is kept and opened instead; anything else there is
/// replaced.
fn publish(staging: &Path, final_dir: &Path, build: u32) -> Result<RawParams, RawParamsError> {
    let marker = staging.join(COMPLETE_MARKER);
    File::create(&marker).map_err(|source| RawParamsError::Io { path: marker, source })?;

    if let Ok(Some(existing)) = RawParams::open(final_dir, build) {
        let _ = std::fs::remove_dir_all(staging);
        return Ok(existing);
    }
    if final_dir.exists() {
        std::fs::remove_dir_all(final_dir)
            .map_err(|source| RawParamsError::Io { path: final_dir.to_path_buf(), source })?;
    }

    if let Err(source) = std::fs::rename(staging, final_dir) {
        // Another process published between the removal and the rename.
        if let Ok(Some(existing)) = RawParams::open(final_dir, build) {
            let _ = std::fs::remove_dir_all(staging);
            return Ok(existing);
        }
        return Err(RawParamsError::Io { path: final_dir.to_path_buf(), source });
    }

    RawParams::open(final_dir, build)?.ok_or_else(|| RawParamsError::Vanished { dir: final_dir.to_path_buf() })
}
