//! Parser for space-level `forest.bin` (SpeedTree vegetation placement) files.
//!
//! These files store per-instance placement data for SpeedTree vegetation
//! (trees, bushes, algae, etc.) across a map space.
//!
//! ## File Layout
//!
//! ```text
//! Header (32 bytes):
//!   u64  num_species
//!   u64  string_table_offset
//!   u64  (reserved)
//!   u64  lod0_total_instances
//!
//! LOD Blocks (variable, between header and string table):
//!   Each LOD block contains:
//!   - Species group ranges (u32 pairs, terminated by first >= num_species)
//!   - (u32 num_species, u32 0)         // end marker
//!   - (u32 num_species, u32 lod_flag)  // LOD header
//!   - num_species × (u32 start, u32 count)  // per-species instance table
//!
//! String Table (at string_table_offset):
//!   num_species × (u64 len, i64 relptr)  // species name entries
//!   ...followed by null-terminated string data...
//!
//! Instance Data (from end of string pool to EOF):
//!   Dense array of 16-byte records (f32 x, f32 y, f32 z, f32 w).
//!   LOD levels are stored sequentially. Each LOD's species table uses
//!   indices relative to that LOD's base within this array.
//! ```
//!
//! We parse only the highest-detail **LOD 0** instances.

use rootcause::Report;
use thiserror::Error;
use winnow::Parser;
use winnow::binary::le_f32;
use winnow::binary::le_i64;
use winnow::binary::le_u64;
use winnow::combinator::repeat;

use winnow::error::ContextError;
use winnow::error::ErrMode;

use crate::data::parser_utils::WResult;
use crate::data::parser_utils::resolve_relptr;

const INSTANCE_SIZE: usize = 16;

#[derive(Debug, Error)]
pub enum ForestError {
    #[error("data too short: need {need} bytes at offset 0x{offset:X}, have {have}")]
    DataTooShort { offset: usize, need: usize, have: usize },
    #[error("parse error: {0}")]
    ParseError(String),
    #[error("LOD0 species table not found in header")]
    NoLod0Table,
}

/// A single vegetation instance with its species assignment.
#[derive(Debug, Clone, Copy)]
pub struct ForestInstance {
    pub species_index: usize,
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// Parsed `forest.bin` file (LOD 0 only).
#[derive(Debug)]
pub struct Forest {
    /// SpeedTree species asset paths (`.stsdk` files).
    pub species: Vec<String>,
    /// LOD 0 vegetation instances with species assignment.
    pub instances: Vec<ForestInstance>,
}

/// Parse a single string table entry: `(u64 len, i64 relptr)`.
fn parse_string_table_entry(input: &mut &[u8]) -> WResult<(u64, i64)> {
    let len = le_u64.parse_next(input)?;
    let relptr = le_i64.parse_next(input)?;
    Ok((len, relptr))
}

/// Raw instance record from the file.
struct RawInstance {
    x: f32,
    y: f32,
    z: f32,
}

fn parse_raw_instance(input: &mut &[u8]) -> WResult<RawInstance> {
    let x = le_f32.parse_next(input)?;
    let y = le_f32.parse_next(input)?;
    let z = le_f32.parse_next(input)?;
    let _w = le_f32.parse_next(input)?;
    Ok(RawInstance { x, y, z })
}

/// Find the LOD 0 per-species (start, count) table in the header.
///
/// Searches for the marker pattern `(num_species_u32, 0, num_species_u32, 1)`
/// which immediately precedes the species table. The first occurrence is LOD 0.
fn find_lod0_species_table(data: &[u8], num_species: usize, search_end: usize) -> Option<(usize, Vec<(usize, usize)>)> {
    let ns = num_species as u32;
    let marker = [ns.to_le_bytes(), 0u32.to_le_bytes(), ns.to_le_bytes(), 1u32.to_le_bytes()].concat();

    let end = search_end.min(data.len());
    let marker_pos = data[..end].windows(marker.len()).position(|w| w == marker)?;

    let table_start = marker_pos + marker.len();
    let table_end = table_start + num_species * 8;
    if table_end > end {
        return None;
    }

    let mut entries = Vec::with_capacity(num_species);
    for i in 0..num_species {
        let off = table_start + i * 8;
        let start = u32::from_le_bytes(data[off..off + 4].try_into().unwrap()) as usize;
        let count = u32::from_le_bytes(data[off + 4..off + 8].try_into().unwrap()) as usize;
        entries.push((start, count));
    }

    let total: usize = entries.iter().map(|(_, c)| *c).sum();
    Some((total, entries))
}

/// Parse a `forest.bin` file.
///
/// Returns only LOD 0 (highest detail) instances, with species indices assigned
/// from the per-species instance table in the file header.
pub fn parse_forest(file_data: &[u8]) -> Result<Forest, Report<ForestError>> {
    if file_data.len() < 32 {
        return Err(Report::new(ForestError::DataTooShort { offset: 0, need: 32, have: file_data.len() }));
    }

    // Parse header.
    let header_input = &mut &file_data[0x00..];
    let num_species = le_u64
        .parse_next(header_input)
        .map_err(|e: ErrMode<ContextError>| Report::new(ForestError::ParseError(format!("{e}"))))?
        as usize;
    let string_table_offset = le_u64
        .parse_next(header_input)
        .map_err(|e: ErrMode<ContextError>| Report::new(ForestError::ParseError(format!("{e}"))))?
        as usize;

    if num_species == 0 {
        return Ok(Forest { species: Vec::new(), instances: Vec::new() });
    }
    if num_species > 1000 {
        return Err(Report::new(ForestError::ParseError(format!("unreasonable species count: {num_species}"))));
    }

    // Validate string table bounds.
    let string_table_end = string_table_offset + num_species * 16;
    if string_table_end > file_data.len() {
        return Err(Report::new(ForestError::DataTooShort {
            offset: string_table_offset,
            need: num_species * 16,
            have: file_data.len().saturating_sub(string_table_offset),
        }));
    }

    // Parse species string table.
    let mut species = Vec::with_capacity(num_species);
    let mut data_start = string_table_end;

    for i in 0..num_species {
        let entry_off = string_table_offset + i * 16;
        let input = &mut &file_data[entry_off..];
        let (str_len, str_relptr) = parse_string_table_entry(input)
            .map_err(|e: ErrMode<ContextError>| Report::new(ForestError::ParseError(format!("{e}"))))?;

        let str_len = str_len as usize;
        let str_abs = resolve_relptr(entry_off, str_relptr);

        if str_len == 0 || str_abs + str_len > file_data.len() {
            species.push(format!("species_{i}"));
            continue;
        }

        // Exclude null terminator.
        let name_bytes = &file_data[str_abs..str_abs + str_len - 1];
        let name = String::from_utf8_lossy(name_bytes).into_owned();
        species.push(name);

        let str_end = str_abs + str_len;
        if str_end > data_start {
            data_start = str_end;
        }
    }

    // Find the LOD 0 per-species instance table in the header area
    // (between the fixed header at 0x20 and the string table).
    let (lod0_total, species_table) = find_lod0_species_table(file_data, num_species, string_table_offset)
        .ok_or_else(|| Report::new(ForestError::NoLod0Table))?;

    // Parse raw instance data.
    let remaining = file_data.len() - data_start;
    let max_instances = remaining / INSTANCE_SIZE;

    let input = &mut &file_data[data_start..data_start + max_instances * INSTANCE_SIZE];
    let raw_instances: Vec<RawInstance> = repeat(max_instances, parse_raw_instance)
        .parse_next(input)
        .map_err(|e: ErrMode<ContextError>| Report::new(ForestError::ParseError(format!("{e}"))))?;

    // Build output instances using the per-species (start, count) table.
    let mut instances = Vec::with_capacity(lod0_total);
    for (sp_idx, &(start, count)) in species_table.iter().enumerate() {
        if count == 0 {
            continue;
        }
        let end = start + count;
        if end > raw_instances.len() {
            eprintln!(
                "Warning: forest species {sp_idx} range {start}..{end} exceeds instance count {}",
                raw_instances.len()
            );
            continue;
        }
        for raw in &raw_instances[start..end] {
            instances.push(ForestInstance { species_index: sp_idx, x: raw.x, y: raw.y, z: raw.z });
        }
    }

    Ok(Forest { species, instances })
}
