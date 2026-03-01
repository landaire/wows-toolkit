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

struct Header {
    endianness: u32,
    _murmur_hash: u32,
    version: u32,
}

struct ResourceMetadata {
    resources_count: u32,
    file_infos_count: u32,
    volumes_count: u32,
    _unused: u32,
    resources_table_pointer: u64,
    file_infos_table_pointer: u64,
    volumes_table_pointer: u64,
}

// --- Winnow parsers ---

fn parse_header(input: &mut &[u8]) -> WResult<Header> {
    let magic = le_u32.parse_next(input)?;
    if magic != IDX_MAGIC {
        return Err(winnow::error::ErrMode::Cut(winnow::error::ContextError::new()));
    }
    let endianness = le_u32.parse_next(input)?;
    let murmur_hash = le_u32.parse_next(input)?;
    let version = le_u32.parse_next(input)?;
    Ok(Header { endianness, _murmur_hash: murmur_hash, version })
}

fn parse_resource_metadata(input: &mut &[u8]) -> WResult<ResourceMetadata> {
    let resources_count = le_u32.parse_next(input)?;
    let file_infos_count = le_u32.parse_next(input)?;
    let volumes_count = le_u32.parse_next(input)?;
    let unused = le_u32.parse_next(input)?;
    let resources_table_pointer = le_u64.parse_next(input)?;
    let file_infos_table_pointer = le_u64.parse_next(input)?;
    let volumes_table_pointer = le_u64.parse_next(input)?;
    Ok(ResourceMetadata {
        resources_count,
        file_infos_count,
        volumes_count,
        _unused: unused,
        resources_table_pointer,
        file_infos_table_pointer,
        volumes_table_pointer,
    })
}

fn parse_file_info(input: &mut &[u8]) -> WResult<FileInfo> {
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

/// Parse a single PackedFileMetadata entry.
///
/// Each entry is 32 bytes of fixed fields, but the filename is stored at a relative
/// offset (`filename_ptr`) from the start of the entry. We need the full `file_data`
/// to resolve it.
fn parse_packed_file_metadata(file_data: &[u8], entry_offset: usize) -> Result<PackedFileMetadata, IdxError> {
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

/// Parse a single Volume entry.
///
/// Each entry has fixed fields, and the volume name is at a relative offset
/// (`name_ptr`) from the start of the entry.
fn parse_volume(file_data: &[u8], entry_offset: usize) -> Result<Volume, IdxError> {
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
    let filename = read_null_terminated_string(file_data, name_offset).to_owned();

    Ok(Volume { volume_id, filename })
}

/// Parse an `.idx` file from raw bytes.
pub fn parse(file_data: &[u8]) -> Result<IdxFile, IdxError> {
    let input = &mut &file_data[..];

    let header = parse_header(input).map_err(|e| IdxError::ParseError(format!("header: {e}")))?;

    if header.endianness != 0x02000000 && header.version != 0x40 {
        return Err(IdxError::IncorrectEndian);
    }

    // The resource metadata starts right after the 16-byte header
    let resources_meta_offset = 16usize;
    let meta_input = &mut &file_data[resources_meta_offset..];
    let meta =
        parse_resource_metadata(meta_input).map_err(|e| IdxError::ParseError(format!("resource metadata: {e}")))?;

    // Parse resources table
    let resources_table_offset = resources_meta_offset + meta.resources_table_pointer as usize;
    // Each PackedFileMetadata entry is 32 bytes of fixed fields
    const RESOURCE_ENTRY_SIZE: usize = 32;
    let mut resources = Vec::with_capacity(meta.resources_count as usize);
    for i in 0..meta.resources_count as usize {
        let entry_offset = resources_table_offset + i * RESOURCE_ENTRY_SIZE;
        resources.push(parse_packed_file_metadata(file_data, entry_offset)?);
    }

    // Parse file infos table
    let file_infos_table_offset = resources_meta_offset + meta.file_infos_table_pointer as usize;
    // Each FileInfo entry is 48 bytes
    const FILE_INFO_ENTRY_SIZE: usize = 48;
    let mut file_infos = Vec::with_capacity(meta.file_infos_count as usize);
    for i in 0..meta.file_infos_count as usize {
        let entry_offset = file_infos_table_offset + i * FILE_INFO_ENTRY_SIZE;
        let fi_input = &mut &file_data[entry_offset..];
        let fi = parse_file_info(fi_input).map_err(|e| IdxError::ParseError(format!("file_info[{i}]: {e}")))?;
        file_infos.push(fi);
    }

    // Parse volumes table
    let volumes_table_offset = resources_meta_offset + meta.volumes_table_pointer as usize;
    // Each Volume entry is 24 bytes of fixed fields
    const VOLUME_ENTRY_SIZE: usize = 24;
    let mut volumes = Vec::with_capacity(meta.volumes_count as usize);
    for i in 0..meta.volumes_count as usize {
        let entry_offset = volumes_table_offset + i * VOLUME_ENTRY_SIZE;
        volumes.push(parse_volume(file_data, entry_offset)?);
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
