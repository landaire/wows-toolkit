//! Parser for World of Warships `.idx` index files.
//!
//! IDX files describe the contents of `.pkg` data archives. Each IDX file contains:
//! - A header with magic number and format version
//! - A resource metadata section with counts and table pointers
//! - A resources table (files/directories with parent-child relationships)
//! - A file info table (compression, offset, size metadata)
//! - A volumes table (which `.pkg` file contains the data)

use std::collections::HashMap;
use std::io;

use thiserror::Error;
use tracing::warn;
use winnow::Parser;
use winnow::binary::le_u32;
use winnow::binary::le_u64;

use crate::data::parser_utils::WResult;
use crate::data::parser_utils::read_null_terminated_string;

#[derive(Debug, Error)]
pub enum IdxError {
    #[error("File has incorrect endian markers")]
    IncorrectEndian,
    #[error("Unsupported idx version: 0x{0:08x}")]
    UnsupportedVersion(u32),
    #[error("File not found: {0}")]
    FileNotFound(String),
    #[error("I/O error")]
    IoError(#[from] io::Error),
    #[error("Parse error: {0}")]
    ParseError(String),
}

/// The IDX file magic number: "ISPF" as little-endian u32.
/// "ISFP" as little-endian u32 (bytes: 49 53 46 50).
const IDX_MAGIC: u32 = 0x50465349;

/// The root node sentinel parent ID.
pub const ROOT_PARENT_ID: u64 = 0xdbb1a1d1b108b927;

/// Main struct describing a parsed `.idx` file.
#[derive(Debug)]
pub struct IdxFile {
    pub resources: Vec<PackedFileMetadata>,
    pub file_infos: Vec<FileInfo>,
    pub volumes: Vec<Volume>,
}

/// A file or directory entry in the resource table.
#[derive(Debug, Clone)]
pub struct PackedFileMetadata {
    /// Unknown field (possibly a resource pointer).
    pub resource_ptr: u64,
    /// This resource's unique ID.
    pub id: u64,
    /// This resource's parent ID. Root entries have `parent_id == ROOT_PARENT_ID`.
    pub parent_id: u64,
    /// This resource's filename (just the name, not the full path).
    pub filename: String,
}

/// Metadata about a file's location and compression within a volume.
#[derive(Debug, Clone)]
pub struct FileInfo {
    /// The resource ID this file info belongs to.
    pub resource_id: u64,
    /// The volume ID where this resource resides.
    pub volume_id: u64,
    /// Byte offset within the volume.
    pub offset: u64,
    /// How the file is compressed (0 = uncompressed).
    pub compression_info: u64,
    /// Compressed data size in bytes.
    pub size: u32,
    /// CRC32 of the uncompressed data.
    pub crc32: u32,
    /// Uncompressed file size in bytes.
    pub unpacked_size: u32,
    /// Padding.
    pub padding: u32,
}

/// Metadata about a `.pkg` volume file.
#[derive(Debug, Clone)]
pub struct Volume {
    /// The volume's unique ID.
    pub volume_id: u64,
    /// The volume's `.pkg` filename.
    pub filename: String,
}

// --- Internal parsing structures ---

/// Known idx format versions (the u32 at header offset 4).
const VERSION_V20: u32 = 0x01010004;
const VERSION_V40: u32 = 0x02000000;

struct Header {
    /// Format version identifier.
    version: u32,
    /// Murmur hash checksum of the data after the header.
    _murmur_hash: u32,
    /// Architecture bitness (u16 at offset 12) + endianness (u16 at offset 14).
    _arch_endian: u32,
}

/// Metadata for v0x40 format: 4 u32 counts + 3 u64 pointers.
struct ResourceMetadataV40 {
    resources_count: u32,
    file_infos_count: u32,
    volumes_count: u32,
    _unused: u32,
    resources_table_pointer: u64,
    file_infos_table_pointer: u64,
    volumes_table_pointer: u64,
}

/// Metadata for v0x20 format: 3 x (u32 count, u32 pointer).
/// Pointers are relative to the metadata start (offset 16).
struct ResourceMetadataV20 {
    resources_count: u32,
    resources_table_pointer: u32,
    file_infos_count: u32,
    file_infos_table_pointer: u32,
    volumes_count: u32,
    volumes_table_pointer: u32,
}

// --- Winnow parsers ---

fn parse_header(input: &mut &[u8]) -> WResult<Header> {
    let magic = le_u32.parse_next(input)?;
    if magic != IDX_MAGIC {
        return Err(winnow::error::ErrMode::Cut(winnow::error::ContextError::new()));
    }
    let version = le_u32.parse_next(input)?;
    let murmur_hash = le_u32.parse_next(input)?;
    let arch_endian = le_u32.parse_next(input)?;
    Ok(Header { version, _murmur_hash: murmur_hash, _arch_endian: arch_endian })
}

fn parse_resource_metadata_v40(input: &mut &[u8]) -> WResult<ResourceMetadataV40> {
    let resources_count = le_u32.parse_next(input)?;
    let file_infos_count = le_u32.parse_next(input)?;
    let volumes_count = le_u32.parse_next(input)?;
    let unused = le_u32.parse_next(input)?;
    let resources_table_pointer = le_u64.parse_next(input)?;
    let file_infos_table_pointer = le_u64.parse_next(input)?;
    let volumes_table_pointer = le_u64.parse_next(input)?;
    Ok(ResourceMetadataV40 {
        resources_count,
        file_infos_count,
        volumes_count,
        _unused: unused,
        resources_table_pointer,
        file_infos_table_pointer,
        volumes_table_pointer,
    })
}

fn parse_resource_metadata_v20(input: &mut &[u8]) -> WResult<ResourceMetadataV20> {
    let resources_count = le_u32.parse_next(input)?;
    let resources_table_pointer = le_u32.parse_next(input)?;
    let file_infos_count = le_u32.parse_next(input)?;
    let file_infos_table_pointer = le_u32.parse_next(input)?;
    let volumes_count = le_u32.parse_next(input)?;
    let volumes_table_pointer = le_u32.parse_next(input)?;
    Ok(ResourceMetadataV20 {
        resources_count,
        resources_table_pointer,
        file_infos_count,
        file_infos_table_pointer,
        volumes_count,
        volumes_table_pointer,
    })
}

fn parse_file_info_v40(input: &mut &[u8]) -> WResult<FileInfo> {
    let resource_id = le_u64.parse_next(input)?;
    let volume_id = le_u64.parse_next(input)?;
    let offset = le_u64.parse_next(input)?;
    let compression_info = le_u64.parse_next(input)?;
    let size = le_u32.parse_next(input)?;
    let crc32 = le_u32.parse_next(input)?;
    let unpacked_size = le_u32.parse_next(input)?;
    let padding = le_u32.parse_next(input)?;
    Ok(FileInfo { resource_id, volume_id, offset, compression_info, size, crc32, unpacked_size, padding })
}

fn parse_file_info_v20(input: &mut &[u8]) -> WResult<FileInfo> {
    let offset = le_u64.parse_next(input)?;
    let _padding = le_u32.parse_next(input)?;
    let size = le_u32.parse_next(input)?;
    let crc32 = le_u32.parse_next(input)?;
    let unpacked_size = le_u32.parse_next(input)?;
    let compression_info = le_u64.parse_next(input)?;
    let resource_id = le_u64.parse_next(input)?;
    let volume_id = le_u64.parse_next(input)?;
    Ok(FileInfo { resource_id, volume_id, offset, compression_info, size, crc32, unpacked_size, padding: 0 })
}

/// Parse a v0x40 PackedFileMetadata entry (32 bytes).
///
/// Filename is stored at a relative offset from the entry start.
fn parse_packed_file_metadata_v40(file_data: &[u8], entry_offset: usize) -> Result<PackedFileMetadata, IdxError> {
    let input = &mut &file_data[entry_offset..];
    let resource_ptr: u64 =
        le_u64.parse_next(input).map_err(|e: winnow::error::ErrMode<winnow::error::ContextError>| {
            IdxError::ParseError(format!("resource_ptr: {e}"))
        })?;
    let filename_ptr: u64 =
        le_u64.parse_next(input).map_err(|e: winnow::error::ErrMode<winnow::error::ContextError>| {
            IdxError::ParseError(format!("filename_ptr: {e}"))
        })?;
    let id: u64 = le_u64
        .parse_next(input)
        .map_err(|e: winnow::error::ErrMode<winnow::error::ContextError>| IdxError::ParseError(format!("id: {e}")))?;
    let parent_id: u64 =
        le_u64.parse_next(input).map_err(|e: winnow::error::ErrMode<winnow::error::ContextError>| {
            IdxError::ParseError(format!("parent_id: {e}"))
        })?;

    let filename_offset = entry_offset + filename_ptr as usize;
    let filename = read_null_terminated_string(file_data, filename_offset).to_owned();

    Ok(PackedFileMetadata { resource_ptr, id, parent_id, filename })
}

/// Parse a v0x20 PackedFileMetadata entry (24 bytes).
///
/// Layout: `[u64 name_hash][u64 parent_id][u32 name_len][u32 name_ptr]`
/// `name_ptr` is relative to the name field's own file offset (entry + 16).
fn parse_packed_file_metadata_v20(file_data: &[u8], entry_offset: usize) -> Result<PackedFileMetadata, IdxError> {
    let input = &mut &file_data[entry_offset..];
    let id: u64 = le_u64
        .parse_next(input)
        .map_err(|e: winnow::error::ErrMode<winnow::error::ContextError>| IdxError::ParseError(format!("id: {e}")))?;
    let parent_id: u64 =
        le_u64.parse_next(input).map_err(|e: winnow::error::ErrMode<winnow::error::ContextError>| {
            IdxError::ParseError(format!("parent_id: {e}"))
        })?;
    let name_len: u32 =
        le_u32.parse_next(input).map_err(|e: winnow::error::ErrMode<winnow::error::ContextError>| {
            IdxError::ParseError(format!("name_len: {e}"))
        })?;
    let name_ptr: u32 =
        le_u32.parse_next(input).map_err(|e: winnow::error::ErrMode<winnow::error::ContextError>| {
            IdxError::ParseError(format!("name_ptr: {e}"))
        })?;

    // name_ptr is relative to the name field's own offset in the file
    let name_field_offset = entry_offset + 16;
    let name_abs = name_field_offset + name_ptr as usize;
    let name_end = (name_abs + name_len as usize).min(file_data.len());
    let raw = &file_data[name_abs..name_end];
    // Trim trailing nulls
    let trimmed = match raw.iter().position(|&b| b == 0) {
        Some(pos) => &raw[..pos],
        None => raw,
    };
    let filename = String::from_utf8_lossy(trimmed).into_owned();

    Ok(PackedFileMetadata { resource_ptr: 0, id, parent_id, filename })
}

/// Parse a v0x40 Volume entry (24 bytes).
fn parse_volume_v40(file_data: &[u8], entry_offset: usize) -> Result<Volume, IdxError> {
    let input = &mut &file_data[entry_offset..];
    let _len: u64 = le_u64.parse_next(input).map_err(|e: winnow::error::ErrMode<winnow::error::ContextError>| {
        IdxError::ParseError(format!("volume len: {e}"))
    })?;
    let name_ptr: u64 =
        le_u64.parse_next(input).map_err(|e: winnow::error::ErrMode<winnow::error::ContextError>| {
            IdxError::ParseError(format!("volume name_ptr: {e}"))
        })?;
    let volume_id: u64 =
        le_u64.parse_next(input).map_err(|e: winnow::error::ErrMode<winnow::error::ContextError>| {
            IdxError::ParseError(format!("volume_id: {e}"))
        })?;

    let name_offset = entry_offset + name_ptr as usize;
    let mut filename = read_null_terminated_string(file_data, name_offset).to_owned();

    // Early v0x40 files still use BigWorld path convention: "//.//name.pkg"
    // Strip the prefix so the name matches the actual file on disk.
    if let Some(stripped) = filename.strip_prefix("//.//") {
        filename = stripped.to_string();
    }

    Ok(Volume { volume_id, filename })
}

/// Parse a v0x20 Volume entry (16 bytes).
///
/// Layout: `[u64 volume_id][u32 name_len][u32 name_ptr]`
/// `name_ptr` is relative to the name field's own file offset (entry + 8).
fn parse_volume_v20(file_data: &[u8], entry_offset: usize) -> Result<Volume, IdxError> {
    let input = &mut &file_data[entry_offset..];
    let volume_id: u64 =
        le_u64.parse_next(input).map_err(|e: winnow::error::ErrMode<winnow::error::ContextError>| {
            IdxError::ParseError(format!("volume_id: {e}"))
        })?;
    let name_len: u32 =
        le_u32.parse_next(input).map_err(|e: winnow::error::ErrMode<winnow::error::ContextError>| {
            IdxError::ParseError(format!("volume name_len: {e}"))
        })?;
    let name_ptr: u32 =
        le_u32.parse_next(input).map_err(|e: winnow::error::ErrMode<winnow::error::ContextError>| {
            IdxError::ParseError(format!("volume name_ptr: {e}"))
        })?;

    let name_field_offset = entry_offset + 8;
    let name_abs = name_field_offset + name_ptr as usize;
    let name_end = (name_abs + name_len as usize).min(file_data.len());
    let raw = &file_data[name_abs..name_end];
    let trimmed = match raw.iter().position(|&b| b == 0) {
        Some(pos) => &raw[..pos],
        None => raw,
    };
    let mut filename = String::from_utf8_lossy(trimmed).into_owned();

    // v0x20 volume names use BigWorld path convention: "//.//name.pkg"
    // Strip the prefix so the name matches the actual file on disk.
    if let Some(stripped) = filename.strip_prefix("//.//") {
        filename = stripped.to_string();
    }

    Ok(Volume { volume_id, filename })
}

/// Parse an `.idx` file from raw bytes.
pub fn parse(file_data: &[u8]) -> Result<IdxFile, IdxError> {
    let input = &mut &file_data[..];

    let header = parse_header(input).map_err(|e| IdxError::ParseError(format!("header: {e}")))?;

    match header.version {
        VERSION_V40 => parse_v40(file_data),
        VERSION_V20 => parse_v20(file_data),
        other => Err(IdxError::UnsupportedVersion(other)),
    }
}

/// Parse a v0x40 (modern) idx file.
fn parse_v40(file_data: &[u8]) -> Result<IdxFile, IdxError> {
    let meta_offset = 16usize;
    let meta_input = &mut &file_data[meta_offset..];
    let meta =
        parse_resource_metadata_v40(meta_input).map_err(|e| IdxError::ParseError(format!("resource metadata: {e}")))?;

    // Parse resources table (32-byte entries)
    let resources_table_offset = meta_offset + meta.resources_table_pointer as usize;
    const RESOURCE_ENTRY_SIZE: usize = 32;
    let mut resources = Vec::with_capacity(meta.resources_count as usize);
    for i in 0..meta.resources_count as usize {
        let entry_offset = resources_table_offset + i * RESOURCE_ENTRY_SIZE;
        resources.push(parse_packed_file_metadata_v40(file_data, entry_offset)?);
    }

    // Parse file infos table (48-byte entries)
    let file_infos_table_offset = meta_offset + meta.file_infos_table_pointer as usize;
    const FILE_INFO_ENTRY_SIZE: usize = 48;
    let mut file_infos = Vec::with_capacity(meta.file_infos_count as usize);
    for i in 0..meta.file_infos_count as usize {
        let entry_offset = file_infos_table_offset + i * FILE_INFO_ENTRY_SIZE;
        let fi_input = &mut &file_data[entry_offset..];
        let fi = parse_file_info_v40(fi_input).map_err(|e| IdxError::ParseError(format!("file_info[{i}]: {e}")))?;
        file_infos.push(fi);
    }

    // Parse volumes table (24-byte entries)
    let volumes_table_offset = meta_offset + meta.volumes_table_pointer as usize;
    const VOLUME_ENTRY_SIZE: usize = 24;
    let mut volumes = Vec::with_capacity(meta.volumes_count as usize);
    for i in 0..meta.volumes_count as usize {
        let entry_offset = volumes_table_offset + i * VOLUME_ENTRY_SIZE;
        volumes.push(parse_volume_v40(file_data, entry_offset)?);
    }

    Ok(IdxFile { resources, file_infos, volumes })
}

/// Parse a v0x20 (legacy BigWorld) idx file.
fn parse_v20(file_data: &[u8]) -> Result<IdxFile, IdxError> {
    let meta_offset = 16usize;
    let meta_input = &mut &file_data[meta_offset..];
    let meta =
        parse_resource_metadata_v20(meta_input).map_err(|e| IdxError::ParseError(format!("resource metadata: {e}")))?;

    // Parse resources table (24-byte entries, pointers relative to meta_offset)
    let resources_table_offset = meta_offset + meta.resources_table_pointer as usize;
    const RESOURCE_ENTRY_SIZE: usize = 24;
    let mut resources = Vec::with_capacity(meta.resources_count as usize);
    for i in 0..meta.resources_count as usize {
        let entry_offset = resources_table_offset + i * RESOURCE_ENTRY_SIZE;
        resources.push(parse_packed_file_metadata_v20(file_data, entry_offset)?);
    }

    // Parse file infos table (48-byte entries)
    let file_infos_table_offset = meta_offset + meta.file_infos_table_pointer as usize;
    const FILE_INFO_ENTRY_SIZE: usize = 48;
    let mut file_infos = Vec::with_capacity(meta.file_infos_count as usize);
    for i in 0..meta.file_infos_count as usize {
        let entry_offset = file_infos_table_offset + i * FILE_INFO_ENTRY_SIZE;
        let fi_input = &mut &file_data[entry_offset..];
        let fi = parse_file_info_v20(fi_input).map_err(|e| IdxError::ParseError(format!("file_info[{i}]: {e}")))?;
        file_infos.push(fi);
    }

    // Parse volumes table (16-byte entries)
    let volumes_table_offset = meta_offset + meta.volumes_table_pointer as usize;
    const VOLUME_ENTRY_SIZE: usize = 16;
    let mut volumes = Vec::with_capacity(meta.volumes_count as usize);
    for i in 0..meta.volumes_count as usize {
        let entry_offset = volumes_table_offset + i * VOLUME_ENTRY_SIZE;
        volumes.push(parse_volume_v20(file_data, entry_offset)?);
    }

    Ok(IdxFile { resources, file_infos, volumes })
}

/// An entry in the VFS built from IDX files.
#[derive(Debug, Clone)]
pub enum VfsEntry {
    File { file_info: FileInfo, volume: Volume },
    Directory,
}

/// Build a flat path → entry map from parsed IDX files.
///
/// Returns a `HashMap` mapping full paths (using `/` separators, no leading slash)
/// to their VFS entries. Directory entries are inferred from the parent-child
/// relationships and do not have file info.
pub fn build_file_tree(idx_files: &[IdxFile]) -> HashMap<String, VfsEntry> {
    let count = idx_files.iter().fold(0, |acc, file| acc + file.resources.len());
    // Create lookup tables across all IDX files
    let mut packed_resources = HashMap::with_capacity(count);
    let mut file_infos = HashMap::with_capacity(count);
    let mut volumes = HashMap::with_capacity(count);

    for idx_file in idx_files {
        for resource in &idx_file.resources {
            packed_resources.insert(resource.id, resource.clone());
        }
        for file_info in &idx_file.file_infos {
            file_infos.insert(file_info.resource_id, file_info.clone());
        }
        for volume in &idx_file.volumes {
            if volumes.insert(volume.volume_id, volume.clone()).is_some() {
                warn!("duplicate volume ID?");
            }
        }
    }

    let mut entries = HashMap::<String, VfsEntry>::with_capacity(count);
    // Cache: resource_id → full path
    let mut path_cache = HashMap::<u64, String>::with_capacity(count);

    // Resolve the full path for a resource by walking the parent chain
    fn resolve_path(
        id: u64,
        packed_resources: &HashMap<u64, PackedFileMetadata>,
        path_cache: &mut HashMap<u64, String>,
    ) -> String {
        if let Some(cached) = path_cache.get(&id) {
            return cached.clone();
        }

        let resource = packed_resources.get(&id).expect("failed to find packed resource");

        let path = if resource.parent_id == ROOT_PARENT_ID {
            format!("/{}", &resource.filename)
        } else {
            let mut parent_path = resolve_path(resource.parent_id, packed_resources, path_cache);
            parent_path.reserve(1 + resource.filename.len());
            if resource.parent_id != ROOT_PARENT_ID {
                parent_path.push('/');
            }
            parent_path.push_str(resource.filename.as_str());

            parent_path
        };

        path_cache.insert(id, path.clone());
        path
    }

    for id in packed_resources.keys() {
        let path = resolve_path(*id, &packed_resources, &mut path_cache);
        let file_info = file_infos.get(id).cloned();
        let volume = file_info.as_ref().and_then(|fi| volumes.get(&fi.volume_id).cloned());

        let entry = match (file_info, volume) {
            (Some(file_info), Some(volume)) => VfsEntry::File { file_info, volume },
            _ => VfsEntry::Directory,
        };

        entries.insert(path, entry);
    }

    // Ensure parent directories exist in the map
    let paths: Vec<String> = entries.keys().cloned().collect();
    let mut current = String::new();
    for path in &paths {
        current.clear();
        let parts: Vec<&str> = path.split('/').collect();

        // All parts except the last are directories
        for part in &parts[..parts.len().saturating_sub(1)] {
            if current != "/" {
                current.push('/');
            }

            current.push_str(part);
            entries.entry(current.clone()).or_insert(VfsEntry::Directory);
        }
    }

    entries
}
