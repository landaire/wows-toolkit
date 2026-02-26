//! Parser for space-level `terrain.bin` heightmap files.
//!
//! Format: 16-byte header (magic `trb\0`, width, height, tile info) followed
//! by an RLE-compressed stream of `f32` height values. The RLE encoding uses a
//! double-write escape: when a value repeats, it is stored as `[val, val, count]`
//! where `count` is a `u32 < 0x100000` giving the total number of occurrences.
//!
//! After decoding, the stream contains a preamble of min/max height pairs
//! (one per tile row or LOD region) followed by a flat row-major `width × height`
//! f32 heightmap.

use rootcause::Report;
use thiserror::Error;
use winnow::Parser;
use winnow::binary::le_u32;
use winnow::combinator::repeat;

use crate::data::parser_utils::WResult;

/// Magic bytes: `trb\0` = 0x00627274 little-endian.
pub const TERRAIN_MAGIC: u32 = 0x00627274;
/// RLE marker threshold: any u32 value below this after a double-write is a repeat count.
const RLE_THRESHOLD: u32 = 0x100000;

#[derive(Debug, Error)]
pub enum TerrainError {
    #[error("data too short: need {need} bytes, have {have}")]
    DataTooShort { need: usize, have: usize },
    #[error("bad magic: expected 0x{:08X}, got 0x{got:08X}", TERRAIN_MAGIC)]
    BadMagic { got: u32 },
    #[error("RLE decode produced {decoded} values, expected at least {expected} (width*height)")]
    SizeMismatch { decoded: usize, expected: usize },
    #[error("parse error: {0}")]
    ParseError(String),
}

/// Parsed terrain heightmap.
#[derive(Debug)]
pub struct Terrain {
    /// Grid width in cells.
    pub width: u32,
    /// Grid height in cells.
    pub height: u32,
    /// Cells per tile edge.
    pub tile_size: u16,
    /// Number of tiles per axis.
    pub tiles_per_axis: u16,
    /// Flat row-major heightmap (`width * height` entries), heights in metres.
    pub heightmap: Vec<f32>,
}

/// Winnow sub-parser for the 16-byte header.
/// Returns `(magic, width, height, tile_info)`.
fn parse_header(input: &mut &[u8]) -> WResult<(u32, u32, u32, u32)> {
    let magic = le_u32.parse_next(input)?;
    let width = le_u32.parse_next(input)?;
    let height = le_u32.parse_next(input)?;
    let tile_info = le_u32.parse_next(input)?;
    Ok((magic, width, height, tile_info))
}

/// Parse a `terrain.bin` file.
pub fn parse_terrain(file_data: &[u8]) -> Result<Terrain, Report<TerrainError>> {
    let input = &mut &file_data[..];

    // Parse header via winnow.
    let (magic, width, height, tile_info) = parse_header(input).map_err(|e| {
        if file_data.len() < 16 {
            Report::new(TerrainError::DataTooShort { need: 16, have: file_data.len() })
        } else {
            Report::new(TerrainError::ParseError(format!("{e}")))
        }
    })?;

    if magic != TERRAIN_MAGIC {
        return Err(Report::new(TerrainError::BadMagic { got: magic }));
    }

    let tile_size = (tile_info & 0xFFFF) as u16;
    let tiles_per_axis = ((tile_info >> 16) & 0xFFFF) as u16;

    // RLE decode the body.
    // `input` now points past the 16-byte header thanks to winnow advancing it.
    let body = *input;
    let n_u32 = body.len() / 4;
    let expected = (width as usize) * (height as usize);

    // Pre-read all u32s via winnow repeat combinator.
    let body_input = &mut &body[..n_u32 * 4]; // trim to u32-aligned length
    let raw: Vec<u32> = repeat(n_u32, le_u32).parse_next(body_input).map_err(
        |e: winnow::error::ErrMode<winnow::error::ContextError>| Report::new(TerrainError::ParseError(format!("{e}"))),
    )?;

    let mut values = Vec::with_capacity(expected + 1024);
    let mut idx = 0;
    while idx < raw.len() {
        let val = raw[idx];
        if idx + 2 < raw.len() && raw[idx + 1] == val && raw[idx + 2] > 0 && raw[idx + 2] < RLE_THRESHOLD {
            let count = raw[idx + 2] as usize;
            let f = f32::from_bits(val);
            values.resize(values.len() + count, f);
            idx += 3;
        } else {
            values.push(f32::from_bits(val));
            idx += 1;
        }
    }

    if values.len() < expected {
        return Err(Report::new(TerrainError::SizeMismatch { decoded: values.len(), expected }));
    }

    // Strip preamble (everything before the width*height heightmap).
    let preamble = values.len() - expected;
    let heightmap = values.split_off(preamble);

    Ok(Terrain { width, height, tile_size, tiles_per_axis, heightmap })
}
