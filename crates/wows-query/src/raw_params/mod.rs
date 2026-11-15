//! Raw GameParams entries of one build: one tree in which every distinct
//! array and object is stored once (see [`format`]), read in place from a
//! memory map, so processes and threads that open one build share its
//! pages, and an index of entry names for [`RawParams::search`].
//!
//! A store lives in `<cache_root>/raw-params-v<N>/<build>-<language>/params.bin`,
//! where `<language>` is that of the display names it indexes. It is built
//! in a sibling staging directory, marked with `.complete` and renamed into
//! place; a directory without the marker is treated as absent. Every open
//! and query updates the modification time of `.last-used`, which cache
//! eviction reads.

mod builder;
mod format;
mod json;
mod search;

use std::cmp::Ordering;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use std::time::SystemTime;

use memmap2::Mmap;
use wows_core::game_types::GameParamId;
use wowsunpack::data::ResourceLoader;
use wowsunpack::game_params::convert::base_params_json;
use wowsunpack::game_params::convert::game_params_to_pickle;

use crate::error::RawParamsError;
use crate::error::StoreDamage;
use builder::Builder;
use format::ABSENT;
use format::ENTRY_WORDS;
use format::HeaderError;
use format::Layout;
use format::Section;
use format::TERM_WORDS;

pub use format::EntryPosition;
pub use format::FORMAT_VERSION;
pub(crate) use format::Node;
pub(crate) use format::StringId;
pub(crate) use format::Tables;
pub(crate) use format::Value;
pub(crate) use format::ValueRef;
pub use search::MatchMethod;
pub use search::MatchScore;
pub use search::SearchHit;
pub use search::TermField;
pub(crate) use search::kind_rank;

const STORE_FILE: &str = "params.bin";
pub(crate) const COMPLETE_MARKER: &str = ".complete";
pub(crate) const LAST_USED_MARKER: &str = ".last-used";

/// Cache kind directory of raw GameParams stores under a cache root. The
/// suffix is [`FORMAT_VERSION`], so programs built with different formats
/// sharing a cache root do not replace each other's stores.
pub fn raw_params_kind() -> String {
    format!("raw-params-v{FORMAT_VERSION}")
}

/// Display names of GameParams entries in one language, which a store
/// indexes for search. A store is identified by build and language alone, so
/// `names` must be the catalog the client ships with that build.
#[derive(Clone, Copy)]
pub struct Localization<'a> {
    /// Language code of `names`, e.g. `en`: ASCII letters, digits, `-` and
    /// `_`. It names the store's cache directory.
    pub language: &'a str,
    /// Translates `IDS_*` keys; a catalog for `language`.
    pub names: &'a dyn ResourceLoader,
}

impl Localization<'_> {
    fn check(&self) -> Result<(), RawParamsError> {
        let valid = !self.language.is_empty()
            && self.language.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
        if !valid {
            return Err(RawParamsError::Language { language: self.language.to_owned() });
        }

        Ok(())
    }
}

/// Raw GameParams of one build. Immutable once opened; share it as
/// `Arc<RawParams>`.
pub struct RawParams {
    path: PathBuf,
    map: Mmap,
    layout: Layout,
}

/// One GameParams entry of a store.
#[derive(Clone, Copy)]
pub struct RawParamEntry<'a> {
    position: EntryPosition,
    record: [u32; ENTRY_WORDS],
    tables: Tables<'a>,
}

impl RawParams {
    /// Opens the store for `build` with display names in `localization`
    /// under `cache_root`, building it from `game_params_data`, the bytes of
    /// `content/GameParams.data`, when absent or damaged.
    pub fn open_or_build(
        cache_root: &Path,
        build: u32,
        localization: Localization<'_>,
        game_params_data: impl FnOnce() -> Result<Vec<u8>, RawParamsError>,
    ) -> Result<Arc<Self>, RawParamsError> {
        Self::open_or_build_from(cache_root, build, localization, || {
            let root = game_params_to_pickle(game_params_data()?).map_err(RawParamsError::Decode)?;

            base_params_json(&root).map_err(RawParamsError::Decode)
        })
    }

    /// [`Self::open_or_build`] with the entries `params` gives, each a name
    /// and its JSON in GameParams dictionary order.
    pub fn open_or_build_from<P>(
        cache_root: &Path,
        build: u32,
        localization: Localization<'_>,
        params: impl FnOnce() -> Result<P, RawParamsError>,
    ) -> Result<Arc<Self>, RawParamsError>
    where
        P: IntoIterator<Item = (String, serde_json::Value)>,
    {
        localization.check()?;

        let kind_dir = cache_root.join(raw_params_kind());
        let dir = kind_dir.join(store_name(build, localization.language));
        match Self::open(&dir, build, localization.language) {
            Ok(Some(store)) => return Ok(store),
            // Absent: build it below.
            Ok(None) => {}
            Err(error) => tracing::warn!(dir = %dir.display(), %error, "rebuilding damaged raw GameParams store"),
        }

        Self::build(&kind_dir, build, localization, params()?)
    }

    /// Opens a complete store, or `None` when `dir` holds none. The store
    /// file must not be modified or truncated while the store is open (see
    /// [`Self::open_file`]); removing it or its directory is safe.
    pub fn open(dir: &Path, build: u32, language: &str) -> Result<Option<Arc<Self>>, RawParamsError> {
        if !dir.join(COMPLETE_MARKER).is_file() {
            return Ok(None);
        }

        let store = Self::open_file(&dir.join(STORE_FILE))?;
        if store.layout.build != build {
            return Err(RawParamsError::WrongBuild { path: store.path, expected: build, found: store.layout.build });
        }
        if store.language() != language {
            let found = store.language().to_owned();
            return Err(RawParamsError::WrongLanguage { path: store.path, expected: language.to_owned(), found });
        }
        store.touch();

        Ok(Some(Arc::new(store)))
    }

    /// Maps and checks a store file, whatever build it holds.
    pub(crate) fn open_file(path: &Path) -> Result<Self, RawParamsError> {
        let io_error = |source| RawParamsError::Io { path: path.to_path_buf(), source };
        let file = File::open(path).map_err(io_error)?;
        // SAFETY: the mapping is sound while no one writes or truncates the
        // file, which this module guarantees for the stores it publishes: a
        // store file is written once in a staging directory and published by
        // renaming that directory, never opened for writing afterwards, and
        // replaced or evicted only by removing the directory, which leaves
        // existing mappings valid. Another program writing into the cache
        // directory breaks this; readers then may fault, as with any mapped
        // file.
        let map = unsafe { Mmap::map(&file) }.map_err(io_error)?;
        let layout = match Layout::parse(&map) {
            Ok(layout) => layout,
            Err(HeaderError::Version { found }) => {
                return Err(RawParamsError::FormatVersion { path: path.to_path_buf(), found });
            }
            Err(HeaderError::Damaged(damage)) => {
                return Err(RawParamsError::Damaged { path: path.to_path_buf(), damage });
            }
        };
        let store = Self { path: path.to_path_buf(), map, layout };
        store.validate().map_err(|damage| RawParamsError::Damaged { path: path.to_path_buf(), damage })?;

        Ok(store)
    }

    /// Writes the store of `params`, each an entry name and its JSON in
    /// GameParams dictionary order, for `build` with display names in
    /// `localization` into `kind_dir` and opens it. A complete store another
    /// process published meanwhile is kept and opened instead.
    pub fn build(
        kind_dir: &Path,
        build: u32,
        localization: Localization<'_>,
        params: impl IntoIterator<Item = (String, serde_json::Value)>,
    ) -> Result<Arc<Self>, RawParamsError> {
        localization.check()?;

        let started = std::time::Instant::now();
        let name = store_name(build, localization.language);
        std::fs::create_dir_all(kind_dir)
            .map_err(|source| RawParamsError::Io { path: kind_dir.to_path_buf(), source })?;
        remove_stale_staging(kind_dir, &name);

        let staging = staging_dir(kind_dir, &name);
        std::fs::create_dir(&staging).map_err(|source| RawParamsError::Io { path: staging.clone(), source })?;
        let published = write_store(&staging, build, &localization, params)
            .and_then(|()| publish(&staging, &kind_dir.join(&name), build, localization.language));
        if published.is_err() {
            // The staging directory is private to this build; nothing else reads it.
            let _ = std::fs::remove_dir_all(&staging);
        }
        let store = published?;

        tracing::info!(
            path = %store.path.display(),
            params = store.len(),
            bytes = store.map.len(),
            elapsed_ms = started.elapsed().as_millis(),
            "built raw GameParams store"
        );
        store.touch();

        Ok(store)
    }

    /// Entries, in GameParams dictionary order.
    pub fn len(&self) -> usize {
        self.tables().count(Section::Entries)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Every entry, in GameParams dictionary order.
    pub fn entries(&self) -> impl ExactSizeIterator<Item = RawParamEntry<'_>> {
        (0..self.len() as u32).map(|position| self.entry_at(EntryPosition(position)))
    }

    pub fn entry(&self, position: EntryPosition) -> Option<RawParamEntry<'_>> {
        (position.index() < self.len()).then(|| self.entry_at(position))
    }

    /// The entry with this name, short index or numeric id. Names and
    /// indexes match without regard to ASCII case.
    pub fn find(&self, key: &str) -> Option<RawParamEntry<'_>> {
        let key = key.trim();
        let upper = |text: &str| text.bytes().map(|byte| byte.to_ascii_uppercase()).collect::<Vec<u8>>();

        self.search_order(Section::ByName, |entry| entry.name().cmp(key))
            .or_else(|| {
                let key = upper(key);
                self.search_order(Section::ByIndex, |entry| {
                    let index = entry.index().expect("ByIndex lists entries with an index");
                    index.bytes().map(|byte| byte.to_ascii_uppercase()).cmp(key.iter().copied())
                })
            })
            .or_else(|| {
                let id = key.parse::<u64>().ok()?;
                self.search_order(Section::ById, |entry| {
                    entry.id().map(GameParamId::raw).expect("ById lists entries with an id").cmp(&id)
                })
            })
            .or_else(|| self.entries().find(|entry| entry.name().eq_ignore_ascii_case(key)))
    }

    /// The store file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The game build this store holds.
    pub fn game_build(&self) -> u32 {
        self.layout.build
    }

    /// Language of the display names this store indexes.
    pub fn language(&self) -> &str {
        self.tables().string(self.layout.language).expect("the language string is checked when the store opens")
    }

    /// Size of the store file, the most it adds to a process's memory.
    pub fn file_len(&self) -> usize {
        self.map.len()
    }

    /// Whether the store is still marked complete on disk. Cache eviction
    /// may have removed it, or a damaged entry unmarked it.
    pub fn is_present(&self) -> bool {
        self.marker(COMPLETE_MARKER).is_some_and(|marker| marker.is_file())
    }

    /// Unmarks the store so the next open rebuilds it.
    pub fn invalidate(&self) {
        let Some(marker) = self.marker(COMPLETE_MARKER) else {
            return;
        };
        if let Err(error) = std::fs::remove_file(&marker) {
            tracing::warn!(path = %marker.display(), %error, "failed to unmark a damaged raw GameParams store");
        }
    }

    /// Marks the store as used now for cache eviction.
    pub fn touch(&self) {
        if let Some(marker) = self.marker(LAST_USED_MARKER) {
            touch(&marker);
        }
    }

    /// The JSON text of `entry`.
    pub fn entry_json(&self, entry: &RawParamEntry<'_>) -> Result<Vec<u8>, RawParamsError> {
        let mut json = Vec::new();
        json::write_json(self.tables(), entry.root(), &mut json)
            .map_err(|damage| RawParamsError::DamagedEntry { name: entry.name().to_owned(), damage })?;

        Ok(json)
    }

    pub(crate) fn tables(&self) -> Tables<'_> {
        Tables::new(&self.map, &self.layout)
    }

    fn marker(&self, name: &str) -> Option<PathBuf> {
        self.path.parent().map(|dir| dir.join(name))
    }

    fn entry_at(&self, position: EntryPosition) -> RawParamEntry<'_> {
        let tables = self.tables();
        let base = position.index() * ENTRY_WORDS;
        let record =
            std::array::from_fn(|word| tables.word(Section::Entries, base + word).expect("position below len"));

        RawParamEntry { position, record, tables }
    }

    /// Binary search of an order section by `compare`, which orders an
    /// entry against the key.
    fn search_order(
        &self,
        section: Section,
        compare: impl Fn(RawParamEntry<'_>) -> Ordering,
    ) -> Option<RawParamEntry<'_>> {
        let tables = self.tables();
        let (mut low, mut high) = (0, tables.count(section));
        while low < high {
            let middle = low + (high - low) / 2;
            let position = tables.word(section, middle).expect("middle below count");
            let entry = self.entry_at(EntryPosition(position));
            match compare(entry) {
                Ordering::Less => low = middle + 1,
                Ordering::Greater => high = middle,
                Ordering::Equal => return Some(entry),
            }
        }

        None
    }

    /// Checks what readers rely on without further checks: string bounds,
    /// the language, entry records, order sections and the name index. Node
    /// slots are checked as they are read.
    fn validate(&self) -> Result<(), StoreDamage> {
        let tables = self.tables();

        let string_bytes = tables.count(Section::StringBytes);
        let mut previous = 0;
        for index in 0..tables.count(Section::StringEnds) {
            let end = tables.word(Section::StringEnds, index).ok_or(StoreDamage::String)? as usize;
            if end < previous || end > string_bytes {
                return Err(StoreDamage::String);
            }
            previous = end;
        }
        tables.string(self.layout.language).map_err(|_| StoreDamage::Header)?;

        let entries = tables.count(Section::Entries);
        for position in 0..entries {
            let base = position * ENTRY_WORDS;
            let word = |offset| tables.word(Section::Entries, base + offset).ok_or(StoreDamage::Entry);
            tables.string(StringId(word(0)?))?;
            tables.value(ValueRef(word(1)?))?;
            for offset in 2..6 {
                tables.optional_string(word(offset)?)?;
            }
        }

        // Lookups binary search the order sections: each must list exactly
        // the entries it covers, once each, sorted by key and then position.
        for section in [Section::ByName, Section::ByIndex, Section::ById] {
            let covers = |entry: &RawParamEntry<'_>| match section {
                Section::ByIndex => entry.index().is_some(),
                Section::ById => entry.id().is_some(),
                _ => true,
            };
            let order = |left: &RawParamEntry<'_>, right: &RawParamEntry<'_>| match section {
                Section::ByIndex => {
                    let upper = |entry: &RawParamEntry<'_>| entry.index().map(|index| index.to_ascii_uppercase());
                    upper(left).cmp(&upper(right))
                }
                Section::ById => left.id().map(GameParamId::raw).cmp(&right.id().map(GameParamId::raw)),
                _ => left.name().cmp(right.name()),
            };

            if tables.count(section) != self.entries().filter(covers).count() {
                return Err(StoreDamage::Order);
            }
            let mut previous: Option<RawParamEntry<'_>> = None;
            for index in 0..tables.count(section) {
                let position = tables.word(section, index).ok_or(StoreDamage::Order)? as usize;
                if position >= entries {
                    return Err(StoreDamage::Order);
                }
                let entry = self.entry_at(EntryPosition(position as u32));
                let ascending = |previous: RawParamEntry<'_>| {
                    order(&previous, &entry).then(previous.position().cmp(&entry.position())) == Ordering::Less
                };
                if !covers(&entry) || !previous.is_none_or(ascending) {
                    return Err(StoreDamage::Order);
                }
                previous = Some(entry);
            }
        }

        for index in 0..tables.count(Section::Terms) {
            let word = |offset| tables.word(Section::Terms, index * TERM_WORDS + offset).ok_or(StoreDamage::Term);
            if word(0)? as usize >= entries || TermField::from_word(word(1)?).is_none() {
                return Err(StoreDamage::Term);
            }
            tables.string(StringId(word(2)?))?;
            tables.string(StringId(word(3)?))?;
        }

        Ok(())
    }
}

impl<'a> RawParamEntry<'a> {
    pub fn position(&self) -> EntryPosition {
        self.position
    }

    /// Param name, e.g. `PASC020_Des_Moines_1948`.
    pub fn name(&self) -> &'a str {
        self.tables.string(StringId(self.record[0])).expect("entry strings are checked when the store opens")
    }

    /// Short index, e.g. `PASC020`.
    pub fn index(&self) -> Option<&'a str> {
        self.optional(2)
    }

    pub fn id(&self) -> Option<GameParamId> {
        let (low, high) = (self.record[7], self.record[8]);
        if low == ABSENT && high == ABSENT {
            return None;
        }

        Some(GameParamId::from(u64::from(low) | (u64::from(high) << 32)))
    }

    /// `typeinfo.type`, e.g. `Ship` or `Projectile`.
    pub fn kind(&self) -> Option<&'a str> {
        self.optional(3)
    }

    /// `typeinfo.species`, e.g. `Cruiser` or `Artillery`.
    pub fn species(&self) -> Option<&'a str> {
        self.optional(4)
    }

    /// `typeinfo.nation`, e.g. `USA`.
    pub fn nation(&self) -> Option<&'a str> {
        self.optional(5)
    }

    /// The `level` field: tier for ships and modules.
    pub fn level(&self) -> Option<u32> {
        Some(self.record[6]).filter(|level| *level != ABSENT)
    }

    /// The entry's value.
    pub(crate) fn root(&self) -> ValueRef {
        ValueRef(self.record[1])
    }

    fn optional(&self, word: usize) -> Option<&'a str> {
        self.tables.optional_string(self.record[word]).expect("entry strings are checked when the store opens")
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

/// Directory name of the store of `build` with display names in `language`.
fn store_name(build: u32, language: &str) -> String {
    format!("{build}-{language}")
}

/// Prefix of the staging directories of the store `name`. Store names hold
/// no `.`, so no store's prefix starts another's.
fn staging_prefix(name: &str) -> String {
    format!(".staging-{name}.")
}

fn staging_dir(kind_dir: &Path, name: &str) -> PathBuf {
    let nanos = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_nanos());

    kind_dir.join(format!("{}{}-{nanos}", staging_prefix(name), std::process::id()))
}

/// Removes staging directories of the store `name` left by interrupted
/// builds. A build finishes well within the age limit, so a younger
/// directory may belong to a build still running in another process.
fn remove_stale_staging(kind_dir: &Path, name: &str) {
    const STALE_AFTER: Duration = Duration::from_secs(60 * 60);

    let Ok(listing) = std::fs::read_dir(kind_dir) else {
        return;
    };
    let prefix = staging_prefix(name);
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

/// Writes `params.bin` and `.last-used` into `dir`, synced to disk so the
/// complete marker never precedes the data.
fn write_store(
    dir: &Path,
    build: u32,
    localization: &Localization<'_>,
    params: impl IntoIterator<Item = (String, serde_json::Value)>,
) -> Result<(), RawParamsError> {
    let mut builder = Builder::new(localization.language)?;
    for (name, value) in params {
        builder.add_entry(&name, &value, localization)?;
    }

    let path = dir.join(STORE_FILE);
    let io_error = |source| RawParamsError::Io { path: path.clone(), source };
    let file = File::create(&path).map_err(io_error)?;
    let mut writer = BufWriter::new(file);
    builder.write(build, &mut writer).map_err(io_error)?;
    let file = writer.into_inner().map_err(|error| io_error(error.into_error()))?;
    file.sync_all().map_err(io_error)?;

    touch(&dir.join(LAST_USED_MARKER));

    Ok(())
}

/// Marks a finished staging directory complete, renames it to `final_dir`
/// and opens it. A complete store already at `final_dir`, published by
/// another process, is kept and opened instead; anything else there is
/// replaced.
fn publish(staging: &Path, final_dir: &Path, build: u32, language: &str) -> Result<Arc<RawParams>, RawParamsError> {
    let marker = staging.join(COMPLETE_MARKER);
    File::create(&marker).map_err(|source| RawParamsError::Io { path: marker, source })?;

    if let Ok(Some(existing)) = RawParams::open(final_dir, build, language) {
        let _ = std::fs::remove_dir_all(staging);
        return Ok(existing);
    }
    if final_dir.exists() {
        std::fs::remove_dir_all(final_dir)
            .map_err(|source| RawParamsError::Io { path: final_dir.to_path_buf(), source })?;
    }

    if let Err(source) = std::fs::rename(staging, final_dir) {
        // Another process published between the removal and the rename.
        if let Ok(Some(existing)) = RawParams::open(final_dir, build, language) {
            let _ = std::fs::remove_dir_all(staging);
            return Ok(existing);
        }
        return Err(RawParamsError::Io { path: final_dir.to_path_buf(), source });
    }

    RawParams::open(final_dir, build, language)?
        .ok_or_else(|| RawParamsError::Vanished { dir: final_dir.to_path_buf() })
}

#[cfg(test)]
pub(crate) mod tests;
