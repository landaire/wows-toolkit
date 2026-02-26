//! Parser for SpeedTree `.stsdk` files (vegetation geometry).
//!
//! Two formats exist:
//!
//! ## Format 1: CTable (`SpeedTreeSDK___` magic)
//!
//! Used by `content/location/speedtree/` files. CTable-based offset chain
//! to locate VB/IB. Vertices are f16 half-floats at stride 40.
//!
//! ## Format 2: STDK (`STDK` magic = 0x4B445453)
//!
//! Used by `content/gameplay/common/vegetation/` files. Simple sequential
//! layout with f32 vertices at stride 32.

use rootcause::Report;
use thiserror::Error;
use winnow::Parser;
use winnow::binary::{le_f32, le_u16, le_u32};
use winnow::combinator::repeat;
use winnow::error::{ContextError, ErrMode};
use winnow::token::take;

use crate::data::parser_utils::WResult;

const CTABLE_MAGIC: &[u8; 15] = b"SpeedTreeSDK___";
const STDK_MAGIC: u32 = 0x4B445453;
const CTABLE_VERTEX_STRIDE: usize = 40;
/// SpeedTree vertices are authored in real-world units; BigWorld uses 1 BW unit = 30 real units.
const BW_SCALE: f32 = 1.0 / 30.0;
/// Offset within the CTable data to the draw calls sub-table pointer.
const DRAW_CALLS_OFFSET: usize = 0xA4;

#[derive(Debug, Error)]
pub enum SpeedTreeError {
    #[error("invalid magic")]
    InvalidMagic,
    #[error("file too short at offset {offset:#x}: need {need}, have {have}")]
    TooShort { offset: usize, need: usize, have: usize },
    #[error("no draw calls in file")]
    NoDrawCalls,
    #[error("no vertices found")]
    NoVertices,
    #[error("no LODs in file")]
    NoLods,
    #[error("unsupported vertex stride {0} (expected {CTABLE_VERTEX_STRIDE})")]
    UnsupportedStride(u32),
    #[error("parse error: {0}")]
    ParseError(String),
}

/// Extracted LOD 0 mesh from a SpeedTree `.stsdk` file.
#[derive(Debug)]
pub struct SpeedTreeMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

// ---------------------------------------------------------------------------
// Format 1: CTable (`SpeedTreeSDK___` magic)
// ---------------------------------------------------------------------------

/// Read a LE u32 at `base + offset`, returning `base + value` (relative pointer).
fn read_relptr(data: &[u8], base: usize, offset: usize) -> Result<usize, Report<SpeedTreeError>> {
    let pos = base + offset;
    if pos + 4 > data.len() {
        return Err(Report::new(SpeedTreeError::TooShort {
            offset: pos,
            need: 4,
            have: data.len().saturating_sub(pos),
        }));
    }
    let val = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap()) as usize;
    Ok(base + val)
}

/// Read a plain LE u32 at `offset`.
fn read_u32(data: &[u8], offset: usize) -> Result<u32, Report<SpeedTreeError>> {
    if offset + 4 > data.len() {
        return Err(Report::new(SpeedTreeError::TooShort { offset, need: 4, have: data.len().saturating_sub(offset) }));
    }
    Ok(u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()))
}

/// Parse a single LE u16 half-float and convert to f32.
fn parse_le_f16(input: &mut &[u8]) -> WResult<f32> {
    let bits = le_u16.parse_next(input)?;
    Ok(half::f16::from_bits(bits).to_f32())
}

/// Parse one CTable vertex (40 bytes) → (position, normal, uv).
///
/// Layout: `half4(pos.xyz, u) + half4(norm.xyz, v) + 24 bytes ignored`.
fn parse_ctable_vertex(input: &mut &[u8]) -> WResult<([f32; 3], [f32; 3], [f32; 2])> {
    let px = parse_le_f16(input)?;
    let py = parse_le_f16(input)?;
    let pz = parse_le_f16(input)?;
    let u = parse_le_f16(input)?;

    let nx = parse_le_f16(input)?;
    let ny = parse_le_f16(input)?;
    let nz = parse_le_f16(input)?;
    let v = parse_le_f16(input)?;

    // Bytes 16-39: tangent, branch data, colors — skip
    let _rest: &[u8] = take(24usize).parse_next(input)?;

    // Negate Z for right-handed coordinate system
    Ok(([px, py, -pz], [nx, ny, -nz], [u, v]))
}

/// Parse the CTable format (SpeedTreeSDK___ magic).
fn parse_ctable_format(data: &[u8]) -> Result<SpeedTreeMesh, Report<SpeedTreeError>> {
    // data_ptr = file[15] (byte after the 15-char magic)
    let data_ptr = 15;

    // Follow CTable chain to draw calls sub-table
    let draw_table = read_relptr(data, data_ptr, DRAW_CALLS_OFFSET)?;
    let draw_count = read_u32(data, draw_table)? as usize;
    if draw_count == 0 {
        return Err(Report::new(SpeedTreeError::NoDrawCalls));
    }

    let dc0 = read_relptr(data, draw_table, 4)?;

    // Follow vertex chain
    let vert_sub = read_relptr(data, dc0, 4)?;
    let vert_data = read_relptr(data, vert_sub, 4)?;
    let vert_header = read_relptr(data, vert_data, 8)?;

    let vertex_count = read_u32(data, vert_header)? as usize;
    let vertex_stride = read_u32(data, vert_header + 4)?;
    let vb_start = vert_header + 8;

    if vertex_stride != CTABLE_VERTEX_STRIDE as u32 {
        return Err(Report::new(SpeedTreeError::UnsupportedStride(vertex_stride)));
    }
    if vertex_count == 0 {
        return Err(Report::new(SpeedTreeError::NoVertices));
    }

    let vb_end = vb_start + vertex_count * CTABLE_VERTEX_STRIDE;
    if vb_end > data.len() {
        return Err(Report::new(SpeedTreeError::TooShort {
            offset: vb_start,
            need: vertex_count * CTABLE_VERTEX_STRIDE,
            have: data.len().saturating_sub(vb_start),
        }));
    }

    // Follow index chain
    let idx_sub = read_relptr(data, dc0, 8)?;
    let index_count = read_u32(data, idx_sub)? as usize;
    let ib_start = idx_sub + 8;

    let ib_end = ib_start + index_count * 2;
    if ib_end > data.len() {
        return Err(Report::new(SpeedTreeError::TooShort {
            offset: ib_start,
            need: index_count * 2,
            have: data.len().saturating_sub(ib_start),
        }));
    }

    // Parse vertices (f16)
    let vb_input = &mut &data[vb_start..vb_end];
    let vertices: Vec<([f32; 3], [f32; 3], [f32; 2])> = repeat(vertex_count, parse_ctable_vertex)
        .parse_next(vb_input)
        .map_err(|e: ErrMode<ContextError>| Report::new(SpeedTreeError::ParseError(format!("vertex parse: {e}"))))?;

    // Parse indices (LE u16)
    let ib_input = &mut &data[ib_start..ib_end];
    let raw_indices: Vec<u16> = repeat(index_count, le_u16)
        .parse_next(ib_input)
        .map_err(|e: ErrMode<ContextError>| Report::new(SpeedTreeError::ParseError(format!("index parse: {e}"))))?;

    let mut positions = Vec::with_capacity(vertex_count);
    let mut normals = Vec::with_capacity(vertex_count);
    let mut uvs = Vec::with_capacity(vertex_count);

    for (pos, norm, uv) in &vertices {
        positions.push(*pos);
        normals.push(*norm);
        uvs.push(*uv);
    }

    let indices: Vec<u32> = raw_indices.iter().map(|&i| i as u32).collect();

    Ok(SpeedTreeMesh { positions, normals, uvs, indices })
}

// ---------------------------------------------------------------------------
// Format 2: STDK sequential format
// ---------------------------------------------------------------------------

/// Parse one STDK draw call (f32, stride 32), merging into output vectors.
fn parse_stdk_draw_call(
    input: &mut &[u8],
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    uvs: &mut Vec<[f32; 2]>,
    indices: &mut Vec<u32>,
) -> WResult<()> {
    let vertex_count = le_u32.parse_next(input)? as usize;
    let _vertex_stride = le_u32.parse_next(input)?; // always 32
    let index_count = le_u32.parse_next(input)? as usize;
    let _material_id = le_u32.parse_next(input)?;

    let base_vertex = positions.len() as u32;

    for _ in 0..vertex_count {
        let px = le_f32.parse_next(input)?;
        let py = le_f32.parse_next(input)?;
        let pz = le_f32.parse_next(input)?;
        let nx = le_f32.parse_next(input)?;
        let ny = le_f32.parse_next(input)?;
        let nz = le_f32.parse_next(input)?;
        let u = le_f32.parse_next(input)?;
        let v = le_f32.parse_next(input)?;

        // Negate Z for right-handed coordinate system
        positions.push([px, py, -pz]);
        normals.push([nx, ny, -nz]);
        uvs.push([u, v]);
    }

    let raw_indices: Vec<u16> = repeat(index_count, le_u16).parse_next(input)?;
    for idx in raw_indices {
        indices.push(idx as u32 + base_vertex);
    }

    Ok(())
}

/// Parse the STDK sequential format.
fn parse_stdk_format(data: &[u8]) -> Result<SpeedTreeMesh, Report<SpeedTreeError>> {
    let input = &mut &data[4..]; // skip magic already validated

    let _version = le_u32
        .parse_next(input)
        .map_err(|e: ErrMode<ContextError>| Report::new(SpeedTreeError::ParseError(format!("version: {e}"))))?;
    let lod_count = le_u32
        .parse_next(input)
        .map_err(|e: ErrMode<ContextError>| Report::new(SpeedTreeError::ParseError(format!("lod_count: {e}"))))?
        as usize;
    let _has_billboard = le_u32
        .parse_next(input)
        .map_err(|e: ErrMode<ContextError>| Report::new(SpeedTreeError::ParseError(format!("billboard: {e}"))))?;

    if lod_count == 0 {
        return Err(Report::new(SpeedTreeError::NoLods));
    }

    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();

    // Parse LOD 0 only
    let dc_count = le_u32
        .parse_next(input)
        .map_err(|e: ErrMode<ContextError>| Report::new(SpeedTreeError::ParseError(format!("dc_count: {e}"))))?
        as usize;

    for _ in 0..dc_count {
        parse_stdk_draw_call(input, &mut positions, &mut normals, &mut uvs, &mut indices)
            .map_err(|e: ErrMode<ContextError>| Report::new(SpeedTreeError::ParseError(format!("draw_call: {e}"))))?;
    }

    Ok(SpeedTreeMesh { positions, normals, uvs, indices })
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Parse an `.stsdk` file and extract LOD 0 geometry.
///
/// Auto-detects the format from the file magic:
/// - `SpeedTreeSDK___` → CTable format (f16 vertices, stride 40)
/// - `STDK` (0x4B445453) → sequential format (f32 vertices, stride 32)
pub fn parse_stsdk(data: &[u8]) -> Result<SpeedTreeMesh, Report<SpeedTreeError>> {
    let mut mesh = if data.len() >= 15 && &data[..15] == CTABLE_MAGIC {
        parse_ctable_format(data)?
    } else if data.len() >= 4 && u32::from_le_bytes(data[..4].try_into().unwrap()) == STDK_MAGIC {
        parse_stdk_format(data)?
    } else {
        return Err(Report::new(SpeedTreeError::InvalidMagic));
    };

    // Scale from authoring units to BigWorld units.
    for pos in &mut mesh.positions {
        pos[0] *= BW_SCALE;
        pos[1] *= BW_SCALE;
        pos[2] *= BW_SCALE;
    }

    Ok(mesh)
}
