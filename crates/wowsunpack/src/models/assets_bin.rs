use std::collections::HashMap;

use rootcause::Report;
use thiserror::Error;
use winnow::Parser;
use winnow::binary::{le_i64, le_u16, le_u32};

use crate::data::parser_utils::{WResult, parse_packed_string_fields, resolve_relptr};

const BWDB_MAGIC: u32 = 0x42574442;
const BWDB_VERSION: u32 = 0x01010000;

/// Errors that can occur during assets.bin (PrototypeDatabase) parsing.
#[derive(Debug, Error)]
pub enum AssetsBinError {
    #[error("invalid magic: 0x{actual:08X} (expected 0x{expected:08X})")]
    InvalidMagic { actual: u32, expected: u32 },
    #[error("unsupported version: 0x{actual:08X} (expected 0x{expected:08X})")]
    UnsupportedVersion { actual: u32, expected: u32 },
    #[error("data extends beyond file at 0x{offset:X}")]
    OutOfBounds { offset: usize },
    #[error("parse error: {0}")]
    ParseError(String),
    #[error("path not found: {0}")]
    PathNotFound(String),
    #[error("prototype index {index} out of range for blob {blob} (count: {count})")]
    PrototypeOutOfRange { index: usize, blob: usize, count: u64 },
}

/// The top-level parsed assets.bin (PrototypeDatabase) file.
#[derive(Debug)]
pub struct PrototypeDatabase<'a> {
    pub header: Header,
    pub strings: StringsSection<'a>,
    pub resource_to_prototype_map: HashmapSection<'a>,
    pub paths_storage: Vec<PathEntry>,
    pub databases: Vec<DatabaseEntry<'a>>,
}

#[derive(Debug)]
pub struct Header {
    pub magic: u32,
    pub version: u32,
    pub checksum: u32,
    pub architecture: u16,
    pub endianness: u16,
}

/// The strings section: an offsetsMap hashmap + a raw string data pool.
#[derive(Debug)]
pub struct StringsSection<'a> {
    pub offsets_map: HashmapSection<'a>,
    pub string_data: &'a [u8],
}

/// A generic hashmap section with bucket and value arrays.
#[derive(Debug)]
pub struct HashmapSection<'a> {
    pub capacity: u32,
    pub buckets: &'a [u8],
    pub values: &'a [u8],
    /// Size of each bucket entry in bytes.
    pub bucket_stride: usize,
    /// Size of each value entry in bytes.
    pub value_stride: usize,
}

/// A path entry from the pathsStorage array.
#[derive(Debug)]
pub struct PathEntry {
    pub self_id: u64,
    pub parent_id: u64,
    pub name: String,
}

/// A database entry describing a prototype data blob.
#[derive(Debug)]
pub struct DatabaseEntry<'a> {
    pub prototype_magic: u32,
    pub prototype_checksum: u32,
    pub size: u32,
    /// The full blob data (including the 16-byte header).
    pub data: &'a [u8],
    /// Number of prototype records in this blob (from the blob header).
    pub record_count: u64,
}

/// Result of resolving a flat prototype index to a specific blob and record.
#[derive(Debug, Clone, Copy)]
pub struct PrototypeLocation {
    /// Index of the database blob (0-9).
    pub blob_index: usize,
    /// Index of the record within the blob.
    pub record_index: usize,
}

impl<'a> PrototypeDatabase<'a> {
    /// Build a lookup index from `selfId` to path entry index.
    pub fn build_self_id_index(&self) -> HashMap<u64, usize> {
        self.paths_storage.iter().enumerate().map(|(i, entry)| (entry.self_id, i)).collect()
    }

    /// Look up a `selfId` in the resourceToPrototypeMap hashmap.
    ///
    /// The hashmap uses open addressing with linear probing. Each bucket is 16 bytes:
    /// `(u64 key, u64 sentinel)` where sentinel=1 means occupied, sentinel=0 means empty.
    /// Values are u32 flat indices stored in a parallel array.
    pub fn lookup_r2p(&self, self_id: u64) -> Option<u32> {
        let map = &self.resource_to_prototype_map;
        let capacity = map.capacity as u64;
        if capacity == 0 {
            return None;
        }

        let start_slot = self_id % capacity;
        for probe in 0..capacity {
            let slot = ((start_slot + probe) % capacity) as usize;

            // Read the bucket: (u64 key, u64 sentinel)
            let bucket_offset = slot * map.bucket_stride;
            let key = u64::from_le_bytes(map.buckets[bucket_offset..bucket_offset + 8].try_into().ok()?);
            let sentinel = u64::from_le_bytes(map.buckets[bucket_offset + 8..bucket_offset + 16].try_into().ok()?);

            if sentinel == 0 && key == 0 {
                // Empty slot — key not found.
                return None;
            }

            if key == self_id {
                // Found. Read the u32 value from the parallel values array.
                let value_offset = slot * map.value_stride;
                let value = u32::from_le_bytes(map.values[value_offset..value_offset + 4].try_into().ok()?);
                return Some(value);
            }
        }

        None
    }

    /// Decode an r2p value into a (blob_index, record_index) pair.
    ///
    /// The encoding is: `val = (record_index << 8) | (blob_index * 4)`.
    /// Low byte encodes the blob index (as `blob_index * 4`), upper 24 bits
    /// encode the record index within that blob.
    pub fn decode_r2p_value(&self, value: u32) -> Result<PrototypeLocation, Report<AssetsBinError>> {
        let type_tag = (value & 0xFF) as usize;
        let record_index = (value >> 8) as usize;

        if !type_tag.is_multiple_of(4) {
            return Err(Report::new(AssetsBinError::ParseError(format!(
                "r2p value 0x{value:08X}: low byte 0x{type_tag:02X} is not a multiple of 4"
            ))));
        }
        let blob_index = type_tag / 4;

        if blob_index >= self.databases.len() {
            return Err(Report::new(AssetsBinError::ParseError(format!(
                "r2p value 0x{value:08X}: blob_index {blob_index} >= database count {}",
                self.databases.len()
            ))));
        }

        let db = &self.databases[blob_index];
        if record_index as u64 >= db.record_count {
            return Err(Report::new(AssetsBinError::PrototypeOutOfRange {
                index: record_index,
                blob: blob_index,
                count: db.record_count,
            }));
        }

        Ok(PrototypeLocation { blob_index, record_index })
    }

    /// Get the raw bytes of a prototype record within a blob.
    ///
    /// The blob data starts with a 16-byte header (`u64 count`, `u64 header_size`),
    /// followed by `count * item_size` bytes of fixed-size records, then out-of-line data.
    ///
    /// Returns the full blob data slice starting at the record's offset. The caller
    /// should parse `item_size` bytes for the fixed record and resolve relptrs into
    /// the remainder.
    pub fn get_prototype_data(
        &self,
        location: PrototypeLocation,
        item_size: usize,
    ) -> Result<&'a [u8], Report<AssetsBinError>> {
        let db = &self.databases[location.blob_index];
        if location.record_index as u64 >= db.record_count {
            return Err(Report::new(AssetsBinError::PrototypeOutOfRange {
                index: location.record_index,
                blob: location.blob_index,
                count: db.record_count,
            }));
        }

        let header_size = 16usize;
        let record_offset = header_size + location.record_index * item_size;
        if record_offset + item_size > db.data.len() {
            return Err(Report::new(AssetsBinError::OutOfBounds { offset: record_offset }));
        }

        // Return from record start to end of blob, so relptrs can resolve into OOL data.
        Ok(&db.data[record_offset..])
    }

    /// Reconstruct the full path for a path entry by walking parent links.
    pub fn reconstruct_path(&self, entry_index: usize, self_id_index: &HashMap<u64, usize>) -> String {
        let mut parts = Vec::new();
        let mut current = entry_index;
        for _ in 0..100 {
            let entry = &self.paths_storage[current];
            if !entry.name.is_empty() {
                parts.push(entry.name.as_str());
            }
            if entry.parent_id == 0 {
                break;
            }
            match self_id_index.get(&entry.parent_id) {
                Some(&parent_idx) => current = parent_idx,
                None => break,
            }
        }
        parts.reverse();
        parts.join("/")
    }

    /// Find a path entry whose reconstructed path ends with the given suffix.
    ///
    /// This is a linear scan — use for interactive/CLI lookups, not hot paths.
    pub fn find_path_by_suffix(&self, suffix: &str, self_id_index: &HashMap<u64, usize>) -> Option<(usize, String)> {
        for (i, entry) in self.paths_storage.iter().enumerate() {
            // Quick filter: the leaf name must end with the suffix's last component.
            let suffix_leaf = suffix.rsplit('/').next().unwrap_or(suffix);
            if !entry.name.ends_with(suffix_leaf) {
                continue;
            }
            let full_path = self.reconstruct_path(i, self_id_index);
            if full_path.ends_with(suffix) {
                return Some((i, full_path));
            }
        }
        None
    }

    /// Resolve a path suffix to a prototype location.
    ///
    /// Combines path lookup, r2p hashmap lookup, and flat index resolution.
    pub fn resolve_path(
        &self,
        path_suffix: &str,
        self_id_index: &HashMap<u64, usize>,
    ) -> Result<(PrototypeLocation, String), Report<AssetsBinError>> {
        let (entry_index, full_path) = self
            .find_path_by_suffix(path_suffix, self_id_index)
            .ok_or_else(|| Report::new(AssetsBinError::PathNotFound(path_suffix.to_string())))?;

        let self_id = self.paths_storage[entry_index].self_id;
        let r2p_value = self.lookup_r2p(self_id).ok_or_else(|| {
            Report::new(AssetsBinError::PathNotFound(format!("{path_suffix} (selfId=0x{self_id:016X} not in r2p map)")))
        })?;

        let location = self.decode_r2p_value(r2p_value)?;
        Ok((location, full_path))
    }
}

impl StringsSection<'_> {
    /// Look up a string by its raw offset into the string data pool.
    pub fn get_string(&self, offset: u32) -> Option<&str> {
        let start = offset as usize;
        if start >= self.string_data.len() {
            return None;
        }
        let remaining = &self.string_data[start..];
        let end = remaining.iter().position(|&b| b == 0).unwrap_or(remaining.len());
        std::str::from_utf8(&remaining[..end]).ok()
    }

    /// Look up a string by its hashed name ID, using the offsets_map hashmap.
    ///
    /// The offsets_map uses open addressing with linear probing. Each bucket is
    /// 8 bytes: `(u32 key, u32 sentinel)`. Sentinel has bit 31 set when occupied,
    /// and both key and sentinel are 0 when the slot is empty.
    /// Values are u32 string offsets in a parallel array.
    pub fn get_string_by_id(&self, name_id: u32) -> Option<&str> {
        let map = &self.offsets_map;
        let capacity = map.capacity as u64;
        if capacity == 0 {
            return None;
        }

        let start_slot = (name_id as u64) % capacity;
        for probe in 0..capacity {
            let slot = ((start_slot + probe) % capacity) as usize;
            let bucket_offset = slot * map.bucket_stride;

            let bucket_key = u32::from_le_bytes(map.buckets[bucket_offset..bucket_offset + 4].try_into().ok()?);
            let sentinel = u32::from_le_bytes(map.buckets[bucket_offset + 4..bucket_offset + 8].try_into().ok()?);

            if bucket_key == 0 && sentinel == 0 {
                return None; // empty slot
            }

            if bucket_key == name_id {
                let value_offset = slot * map.value_stride;
                let string_offset = u32::from_le_bytes(map.values[value_offset..value_offset + 4].try_into().ok()?);
                return self.get_string(string_offset);
            }
        }

        None
    }
}

fn parse_header_fields(input: &mut &[u8]) -> WResult<Header> {
    let magic = le_u32.parse_next(input)?;
    let version = le_u32.parse_next(input)?;
    let checksum = le_u32.parse_next(input)?;
    let architecture = le_u16.parse_next(input)?;
    let endianness = le_u16.parse_next(input)?;
    Ok(Header { magic, version, checksum, architecture, endianness })
}

/// Raw body header fields before resolution.
struct BodyHeaderFields {
    // strings/offsetsMap
    offsets_map_capacity: u32,
    offsets_map_buckets_relptr: i64,
    offsets_map_values_relptr: i64,
    string_data_size: u32,
    string_data_relptr: i64,
    // resourceToPrototypeMap
    r2p_capacity: u32,
    r2p_buckets_relptr: i64,
    r2p_values_relptr: i64,
    // pathsStorage
    paths_count: u32,
    paths_data_relptr: i64,
    // databases
    databases_count: u32,
    databases_relptr: i64,
}

fn parse_body_header(input: &mut &[u8]) -> WResult<BodyHeaderFields> {
    // strings section (0x28 bytes)
    let offsets_map_capacity = le_u32.parse_next(input)?;
    let _pad = le_u32.parse_next(input)?;
    let offsets_map_buckets_relptr = le_i64.parse_next(input)?;
    let offsets_map_values_relptr = le_i64.parse_next(input)?;
    let string_data_size = le_u32.parse_next(input)?;
    let _pad = le_u32.parse_next(input)?;
    let string_data_relptr = le_i64.parse_next(input)?;

    // resourceToPrototypeMap section (0x18 bytes)
    let r2p_capacity = le_u32.parse_next(input)?;
    let _pad = le_u32.parse_next(input)?;
    let r2p_buckets_relptr = le_i64.parse_next(input)?;
    let r2p_values_relptr = le_i64.parse_next(input)?;

    // pathsStorage section (0x10 bytes)
    let paths_count = le_u32.parse_next(input)?;
    let _pad = le_u32.parse_next(input)?;
    let paths_data_relptr = le_i64.parse_next(input)?;

    // databases section (0x10 bytes)
    let databases_count = le_u32.parse_next(input)?;
    let _pad = le_u32.parse_next(input)?;
    let databases_relptr = le_i64.parse_next(input)?;

    Ok(BodyHeaderFields {
        offsets_map_capacity,
        offsets_map_buckets_relptr,
        offsets_map_values_relptr,
        string_data_size,
        string_data_relptr,
        r2p_capacity,
        r2p_buckets_relptr,
        r2p_values_relptr,
        paths_count,
        paths_data_relptr,
        databases_count,
        databases_relptr,
    })
}

fn resolve_hashmap<'a>(
    file_data: &'a [u8],
    base: usize,
    capacity: u32,
    buckets_relptr: i64,
    values_relptr: i64,
    bucket_stride: usize,
    value_stride: usize,
) -> Result<HashmapSection<'a>, Report<AssetsBinError>> {
    let cap = capacity as usize;

    let buckets_offset = resolve_relptr(base, buckets_relptr);
    let buckets_end = buckets_offset + cap * bucket_stride;
    if buckets_end > file_data.len() {
        return Err(Report::new(AssetsBinError::OutOfBounds { offset: buckets_offset }));
    }
    let buckets = &file_data[buckets_offset..buckets_end];

    let values_offset = resolve_relptr(base, values_relptr);
    let values_end = values_offset + cap * value_stride;
    if values_end > file_data.len() {
        return Err(Report::new(AssetsBinError::OutOfBounds { offset: values_offset }));
    }
    let values = &file_data[values_offset..values_end];

    Ok(HashmapSection { capacity, buckets, values, bucket_stride, value_stride })
}

/// Parse path entry fields: (selfId, parentId)
fn parse_path_entry_ids(input: &mut &[u8]) -> WResult<(u64, u64)> {
    let self_id = winnow::binary::le_u64.parse_next(input)?;
    let parent_id = winnow::binary::le_u64.parse_next(input)?;
    Ok((self_id, parent_id))
}

fn parse_path_entries(
    file_data: &[u8],
    data_offset: usize,
    count: usize,
) -> Result<Vec<PathEntry>, Report<AssetsBinError>> {
    let mut result = Vec::with_capacity(count);

    for i in 0..count {
        let entry_base = data_offset + i * 32;
        let input = &mut &file_data[entry_base..];

        let (self_id, parent_id) = parse_path_entry_ids(input)
            .map_err(|e| Report::new(AssetsBinError::ParseError(format!("pathEntry[{i}]: {e}"))))?;

        // name is a packed string at entry_base + 0x10
        let name_base = entry_base + 0x10;
        let name_input = &mut &file_data[name_base..];
        let (name_size, _pad, name_relptr) = parse_packed_string_fields(name_input)
            .map_err(|e| Report::new(AssetsBinError::ParseError(format!("pathEntry[{i}] name: {e}"))))?;

        let name = if name_size > 0 {
            let name_data_offset = resolve_relptr(name_base, name_relptr);
            let name_end = name_data_offset + name_size as usize;
            if name_end > file_data.len() {
                return Err(Report::new(AssetsBinError::OutOfBounds { offset: name_data_offset }));
            }
            let name_bytes = &file_data[name_data_offset..name_end];
            let name_bytes = name_bytes.strip_suffix(&[0]).unwrap_or(name_bytes);
            String::from_utf8_lossy(name_bytes).into_owned()
        } else {
            String::new()
        };

        result.push(PathEntry { self_id, parent_id, name });
    }

    Ok(result)
}

/// Parse database entry fields: (magic, checksum, size, pad, data_relptr)
fn parse_database_entry_fields(input: &mut &[u8]) -> WResult<(u32, u32, u32, u32, i64)> {
    let magic = le_u32.parse_next(input)?;
    let checksum = le_u32.parse_next(input)?;
    let size = le_u32.parse_next(input)?;
    let pad = le_u32.parse_next(input)?;
    let relptr = le_i64.parse_next(input)?;
    Ok((magic, checksum, size, pad, relptr))
}

fn parse_database_entries<'a>(
    file_data: &'a [u8],
    entries_offset: usize,
    count: usize,
) -> Result<Vec<DatabaseEntry<'a>>, Report<AssetsBinError>> {
    let mut result = Vec::with_capacity(count);

    for i in 0..count {
        let entry_base = entries_offset + i * 0x18;
        let input = &mut &file_data[entry_base..];

        let (prototype_magic, prototype_checksum, size, _pad, data_relptr) = parse_database_entry_fields(input)
            .map_err(|e| Report::new(AssetsBinError::ParseError(format!("database[{i}]: {e}"))))?;

        let (data, record_count) = if size > 0 {
            let data_offset = resolve_relptr(entry_base, data_relptr);
            let data_end = data_offset + size as usize;
            if data_end > file_data.len() {
                return Err(Report::new(AssetsBinError::OutOfBounds { offset: data_offset }));
            }
            let data = &file_data[data_offset..data_end];

            // Parse blob header: u64 count + u64 header_size (always 16)
            let record_count = u64::from_le_bytes(data[..8].try_into().map_err(|_| {
                Report::new(AssetsBinError::ParseError(format!("database[{i}] blob header too short")))
            })?);

            (data, record_count)
        } else {
            (&[] as &[u8], 0)
        };

        result.push(DatabaseEntry { prototype_magic, prototype_checksum, size, data, record_count });
    }

    Ok(result)
}

/// Parse an assets.bin file into a PrototypeDatabase.
pub fn parse_assets_bin(file_data: &[u8]) -> Result<PrototypeDatabase<'_>, Report<AssetsBinError>> {
    let input = &mut &file_data[..];

    let header =
        parse_header_fields(input).map_err(|e| Report::new(AssetsBinError::ParseError(format!("header: {e}"))))?;

    if header.magic != BWDB_MAGIC {
        return Err(Report::new(AssetsBinError::InvalidMagic { actual: header.magic, expected: BWDB_MAGIC }));
    }
    if header.version != BWDB_VERSION {
        return Err(Report::new(AssetsBinError::UnsupportedVersion { actual: header.version, expected: BWDB_VERSION }));
    }

    let body_base = 0x10; // header is 16 bytes

    let body = parse_body_header(&mut &file_data[body_base..])
        .map_err(|e| Report::new(AssetsBinError::ParseError(format!("body header: {e}"))))?;

    // strings section: base = body_base (0x10)
    let strings_base = body_base;
    let offsets_map = resolve_hashmap(
        file_data,
        strings_base,
        body.offsets_map_capacity,
        body.offsets_map_buckets_relptr,
        body.offsets_map_values_relptr,
        8, // u64 buckets
        4, // u32 values
    )?;

    let string_data_offset = resolve_relptr(strings_base, body.string_data_relptr);
    let string_data_end = string_data_offset + body.string_data_size as usize;
    if string_data_end > file_data.len() {
        return Err(Report::new(AssetsBinError::OutOfBounds { offset: string_data_offset }));
    }
    let string_data = &file_data[string_data_offset..string_data_end];

    let strings = StringsSection { offsets_map, string_data };

    // resourceToPrototypeMap: base = body_base + 0x28
    let r2p_base = body_base + 0x28;
    let resource_to_prototype_map = resolve_hashmap(
        file_data,
        r2p_base,
        body.r2p_capacity,
        body.r2p_buckets_relptr,
        body.r2p_values_relptr,
        16, // u128 buckets (key_hash + metadata)
        4,  // u32 values
    )?;

    // pathsStorage: base = body_base + 0x40
    let paths_base = body_base + 0x40;
    let paths_data_offset = resolve_relptr(paths_base, body.paths_data_relptr);
    let paths_storage = parse_path_entries(file_data, paths_data_offset, body.paths_count as usize)?;

    // databases: relptr relative to body_base
    let db_entries_offset = resolve_relptr(body_base, body.databases_relptr);
    let databases = parse_database_entries(file_data, db_entries_offset, body.databases_count as usize)?;

    Ok(PrototypeDatabase { header, strings, resource_to_prototype_map, paths_storage, databases })
}
