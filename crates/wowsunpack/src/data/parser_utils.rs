//! Shared winnow-based parsing utilities and types used across all binary parsers.

use rootcause::Report;
use thiserror::Error;
use winnow::Parser;
use winnow::binary::{le_f32, le_i64, le_u8, le_u16, le_u32, le_u64};
use winnow::combinator::repeat;
use winnow::error::ContextError;
use winnow::token::take;

/// Common result type for winnow parsers.
pub type WResult<T> = Result<T, winnow::error::ErrMode<ContextError>>;

/// Errors that can occur during shared parsing operations.
#[derive(Debug, Error)]
pub enum ParseError {
    #[error("packed string at 0x{offset:X} extends beyond file (need 0x{needed:X}, have 0x{available:X})")]
    PackedStringOutOfBounds { offset: usize, needed: usize, available: usize },
    #[error("winnow parse error at 0x{offset:X}: {detail}")]
    WinnowError { offset: usize, detail: String },
}

/// Resolve a relative pointer: base_offset + rel_value = absolute file offset.
pub fn resolve_relptr(base_offset: usize, rel_value: i64) -> usize {
    (base_offset as i64 + rel_value) as usize
}

/// Parse packed string fields: (char_count, padding, text_relptr).
pub fn parse_packed_string_fields(input: &mut &[u8]) -> WResult<(u32, u32, i64)> {
    let char_count = le_u32.parse_next(input)?;
    let padding = le_u32.parse_next(input)?;
    let text_relptr = le_i64.parse_next(input)?;
    Ok((char_count, padding, text_relptr))
}

/// Resolve a packed string from file data given the struct base offset.
///
/// Packed strings are stored as: char_count (u32), padding (u32), text_relptr (i64).
/// The actual string data is at `struct_base + text_relptr`.
pub fn parse_packed_string(file_data: &[u8], struct_base: usize) -> Result<String, Report<ParseError>> {
    let input = &mut &file_data[struct_base..];
    let (char_count, _padding, text_relptr) = parse_packed_string_fields(input)
        .map_err(|e| Report::new(ParseError::WinnowError { offset: struct_base, detail: format!("{e}") }))?;

    if char_count == 0 {
        return Ok(String::new());
    }

    let text_offset = resolve_relptr(struct_base, text_relptr);
    let text_end = text_offset + char_count as usize;
    if text_end > file_data.len() {
        return Err(Report::new(ParseError::PackedStringOutOfBounds {
            offset: text_offset,
            needed: text_end,
            available: file_data.len(),
        }));
    }

    let text_bytes = &file_data[text_offset..text_end];
    let text_bytes = text_bytes.strip_suffix(&[0]).unwrap_or(text_bytes);
    Ok(String::from_utf8_lossy(text_bytes).into_owned())
}

/// Read a null-terminated string from `file_data` starting at `offset`.
pub fn read_null_terminated_string(file_data: &[u8], offset: usize) -> &str {
    let remaining = &file_data[offset..];
    let end = remaining.iter().position(|&b| b == 0).unwrap_or(remaining.len());
    std::str::from_utf8(&remaining[..end]).expect("invalid UTF-8 in null-terminated string")
}

// ── Shared BigWorld types ───────────────────────────────────────────────────

/// 4×4 transformation matrix (column-major, 64 bytes).
#[derive(Debug, Clone)]
pub struct Matrix4x4(pub [f32; 16]);

/// Axis-aligned bounding box (32 bytes on disk: 3×f32 min, pad, 3×f32 max, pad).
#[derive(Debug, Clone)]
pub struct BoundingBox {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

/// Parsed fields of a RenderSet record (0x28 bytes).
pub struct RenderSetFields {
    pub name_id: u32,
    pub material_name_id: u32,
    pub vertices_mapping_id: u32,
    pub indices_mapping_id: u32,
    pub material_mfm_path_id: u64,
    pub skinned: bool,
    pub nodes_count: u8,
    pub node_name_ids_relptr: i64,
}

/// Parsed fields of a LOD record (0x10 bytes).
pub struct LodFields {
    pub extent: f32,
    pub casts_shadow: bool,
    pub render_set_names_count: u16,
    pub render_set_names_relptr: i64,
}

// ── Shared winnow sub-parsers ───────────────────────────────────────────────

/// Parse a Matrix4x4 (16 × f32, 64 bytes).
pub fn parse_matrix4x4(input: &mut &[u8]) -> WResult<Matrix4x4> {
    let vals: Vec<f32> = repeat(16, le_f32).parse_next(input)?;
    let mut m = [0f32; 16];
    m.copy_from_slice(&vals);
    Ok(Matrix4x4(m))
}

/// Parse a BoundingBox: 3×f32 min, 4-byte pad, 3×f32 max, 4-byte pad (32 bytes).
pub fn parse_bounding_box(input: &mut &[u8]) -> WResult<BoundingBox> {
    let min_x = le_f32.parse_next(input)?;
    let min_y = le_f32.parse_next(input)?;
    let min_z = le_f32.parse_next(input)?;
    let _pad = le_f32.parse_next(input)?;
    let max_x = le_f32.parse_next(input)?;
    let max_y = le_f32.parse_next(input)?;
    let max_z = le_f32.parse_next(input)?;
    let _pad2 = le_f32.parse_next(input)?;
    Ok(BoundingBox { min: [min_x, min_y, min_z], max: [max_x, max_y, max_z] })
}

/// Parse a RenderSet record (0x28 bytes).
pub fn parse_render_set_fields(input: &mut &[u8]) -> WResult<RenderSetFields> {
    let name_id = le_u32.parse_next(input)?;
    let material_name_id = le_u32.parse_next(input)?;
    let vertices_mapping_id = le_u32.parse_next(input)?;
    let indices_mapping_id = le_u32.parse_next(input)?;
    let material_mfm_path_id = le_u64.parse_next(input)?;
    let skinned = le_u8.parse_next(input)? != 0;
    let nodes_count = le_u8.parse_next(input)?;
    let _ = take(6usize).parse_next(input)?; // padding to +0x20
    let node_name_ids_relptr = le_i64.parse_next(input)?;
    Ok(RenderSetFields {
        name_id,
        material_name_id,
        vertices_mapping_id,
        indices_mapping_id,
        material_mfm_path_id,
        skinned,
        nodes_count,
        node_name_ids_relptr,
    })
}

/// Parse a LOD record (0x10 bytes).
pub fn parse_lod_fields(input: &mut &[u8]) -> WResult<LodFields> {
    let extent = le_f32.parse_next(input)?;
    let casts_shadow = le_u8.parse_next(input)? != 0;
    let _pad = le_u8.parse_next(input)?;
    let render_set_names_count = le_u16.parse_next(input)?;
    let render_set_names_relptr = le_i64.parse_next(input)?;
    Ok(LodFields { extent, casts_shadow, render_set_names_count, render_set_names_relptr })
}

// ── Array helpers ───────────────────────────────────────────────────────────

/// Parse `count` little-endian u32 values.
pub fn parse_u32_array(input: &mut &[u8], count: usize) -> WResult<Vec<u32>> {
    repeat(count, le_u32).parse_next(input)
}

/// Parse `count` little-endian u16 values.
pub fn parse_u16_array(input: &mut &[u8], count: usize) -> WResult<Vec<u16>> {
    repeat(count, le_u16).parse_next(input)
}

/// Parse `count` Matrix4x4 values.
pub fn parse_matrix_array(input: &mut &[u8], count: usize) -> WResult<Vec<Matrix4x4>> {
    repeat(count, parse_matrix4x4).parse_next(input)
}

/// Read an i64 relptr at `base + relptr_offset` and resolve it to an absolute offset.
pub fn resolve_relptr_at(data: &[u8], base: usize, relptr_offset: usize) -> WResult<usize> {
    let input = &mut &data[base + relptr_offset..];
    let relptr = le_i64.parse_next(input)?;
    Ok(resolve_relptr(base, relptr))
}
