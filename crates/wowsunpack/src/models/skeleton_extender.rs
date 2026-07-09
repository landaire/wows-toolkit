//! Parser for SkeletonExtenderPrototype records (blob index 2, item size 0x20).
//!
//! A skeleton extender adds named nodes to a model's skeleton at runtime
//! (`Model.addSkeletonExtender`). Ship hulls ship one extender per section
//! (`<ship>_<Section>.skel_ext` and `..._ep.skel_ext`); the misc-part nodes
//! (propellers, boats, deck fittings) are the `MP_`-prefixed entries.
//!
//! Record layout (verified against live build 12668706, item size 0x20):
//! ```text
//! +0x00  u16   flag                (1 when the extender carries a skeleton, else 0)
//! +0x02  u16   node_count
//! +0x04  u32   padding
//! +0x08  i64   name_ids relptr        -> u32[node_count]  (node name string hashes)
//! +0x10  i64   parent_name_ids relptr -> u32[node_count]  (parent node name hashes)
//! +0x18  i64   matrices relptr        -> Matrix4x4[node_count] (node-local transforms)
//! ```
//! Unlike [`crate::models::visual::VisualNodes`], parents are referenced by name
//! hash (into the base model skeleton, e.g. "Scene Root"), not by array index.

use rootcause::Report;
use thiserror::Error;
use winnow::Parser;
use winnow::binary::le_i64;
use winnow::binary::le_u16;
use winnow::binary::le_u32;
use winnow::error::ContextError;
use winnow::error::ErrMode;

use crate::data::parser_utils::Matrix4x4;
use crate::data::parser_utils::WResult;
use crate::data::parser_utils::parse_matrix_array;
use crate::data::parser_utils::parse_u32_array;
use crate::data::parser_utils::resolve_relptr;

/// Item size for SkeletonExtenderPrototype records in the database blob.
pub const SKELETON_EXTENDER_ITEM_SIZE: usize = 0x20;

/// Errors that can occur during SkeletonExtenderPrototype parsing.
#[derive(Debug, Error)]
pub enum SkeletonExtenderError {
    #[error("data too short: need {need} bytes at offset 0x{offset:X}, have {have}")]
    DataTooShort { offset: usize, need: usize, have: usize },
    #[error("array offset 0x{offset:X} is out of bounds (data length 0x{len:X})")]
    OffsetOutOfBounds { offset: usize, len: usize },
    #[error("parse error: {0}")]
    ParseError(String),
}

/// A parsed SkeletonExtenderPrototype record: one node hierarchy.
///
/// The three vectors are index-aligned (`name_ids[i]`, `parent_name_ids[i]`,
/// `matrices[i]` all describe node `i`).
#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SkeletonExtender {
    /// Node name string hashes (resolve via [`crate::models::assets_bin::StringsSection`]).
    pub name_ids: Vec<u32>,
    /// Parent node name string hashes. References a node in the base model skeleton
    /// by name (e.g. the model root "Scene Root"), not an index into this record.
    pub parent_name_ids: Vec<u32>,
    /// Node-local transforms (column-major 4x4).
    pub matrices: Vec<Matrix4x4>,
}

struct SkeletonExtenderHeader {
    node_count: u16,
    name_ids_relptr: i64,
    parent_name_ids_relptr: i64,
    matrices_relptr: i64,
}

fn parse_header(input: &mut &[u8]) -> WResult<SkeletonExtenderHeader> {
    let _flag = le_u16.parse_next(input)?;
    let node_count = le_u16.parse_next(input)?;
    let _pad = le_u32.parse_next(input)?;
    let name_ids_relptr = le_i64.parse_next(input)?;
    let parent_name_ids_relptr = le_i64.parse_next(input)?;
    let matrices_relptr = le_i64.parse_next(input)?;
    Ok(SkeletonExtenderHeader { node_count, name_ids_relptr, parent_name_ids_relptr, matrices_relptr })
}

/// Parse a SkeletonExtenderPrototype from blob data.
///
/// `record_data` starts at the record's offset within the blob and extends to the
/// end of the blob so relptrs resolve into out-of-line data. The first
/// [`SKELETON_EXTENDER_ITEM_SIZE`] bytes are the fixed record fields.
pub fn parse_skeleton_extender(record_data: &[u8]) -> Result<SkeletonExtender, Report<SkeletonExtenderError>> {
    if record_data.len() < SKELETON_EXTENDER_ITEM_SIZE {
        return Err(Report::new(SkeletonExtenderError::DataTooShort {
            offset: 0,
            need: SKELETON_EXTENDER_ITEM_SIZE,
            have: record_data.len(),
        }));
    }

    let hdr = {
        let input = &mut &record_data[..];
        parse_header(input).map_err(|e: ErrMode<ContextError>| {
            Report::new(SkeletonExtenderError::ParseError(format!("header: {e}")))
        })?
    };

    let count = hdr.node_count as usize;
    if count == 0 {
        return Ok(SkeletonExtender { name_ids: Vec::new(), parent_name_ids: Vec::new(), matrices: Vec::new() });
    }

    let base = 0usize;
    let name_ids = parse_array_at(record_data, resolve_relptr(base, hdr.name_ids_relptr), count, parse_u32_array)?;
    let parent_name_ids =
        parse_array_at(record_data, resolve_relptr(base, hdr.parent_name_ids_relptr), count, parse_u32_array)?;
    let matrices = parse_array_at(record_data, resolve_relptr(base, hdr.matrices_relptr), count, parse_matrix_array)?;

    Ok(SkeletonExtender { name_ids, parent_name_ids, matrices })
}

fn parse_array_at<T>(
    data: &[u8],
    offset: usize,
    count: usize,
    parser: fn(&mut &[u8], usize) -> WResult<Vec<T>>,
) -> Result<Vec<T>, Report<SkeletonExtenderError>> {
    if offset > data.len() {
        return Err(Report::new(SkeletonExtenderError::OffsetOutOfBounds { offset, len: data.len() }));
    }
    let input = &mut &data[offset..];
    parser(input, count).map_err(|e: ErrMode<ContextError>| {
        Report::new(SkeletonExtenderError::ParseError(format!("array at 0x{offset:X}: {e}")))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a synthetic record: 0x20 header then contiguous OOL arrays
    /// (name_ids, parent_name_ids, matrices), relptrs relative to record start.
    fn build_record(names: &[u32], parents: &[u32], mats: &[[f32; 16]]) -> Vec<u8> {
        let count = names.len();
        assert_eq!(count, parents.len());
        assert_eq!(count, mats.len());

        let name_off = SKELETON_EXTENDER_ITEM_SIZE;
        let parent_off = name_off + count * 4;
        let mat_off = parent_off + count * 4;

        let mut buf = Vec::new();
        buf.extend_from_slice(&1u16.to_le_bytes()); // flag
        buf.extend_from_slice(&(count as u16).to_le_bytes()); // count
        buf.extend_from_slice(&0u32.to_le_bytes()); // pad
        buf.extend_from_slice(&(name_off as i64).to_le_bytes());
        buf.extend_from_slice(&(parent_off as i64).to_le_bytes());
        buf.extend_from_slice(&(mat_off as i64).to_le_bytes());
        assert_eq!(buf.len(), SKELETON_EXTENDER_ITEM_SIZE);

        for &n in names {
            buf.extend_from_slice(&n.to_le_bytes());
        }
        for &p in parents {
            buf.extend_from_slice(&p.to_le_bytes());
        }
        for m in mats {
            for &f in m {
                buf.extend_from_slice(&f.to_le_bytes());
            }
        }
        buf
    }

    #[test]
    fn parses_nodes_names_parents_and_matrices() {
        let mut m0 = [0.0f32; 16];
        m0[0] = 1.0;
        m0[5] = 1.0;
        m0[10] = 1.0;
        m0[15] = 1.0;
        m0[12] = -0.478;
        m0[13] = 0.473;
        m0[14] = 5.253;
        let mut m1 = m0;
        m1[12] = 0.478;

        let names = [0x9350F960u32, 0x124712A8];
        let parents = [0x10C30510u32, 0x10C30510];
        let rec = build_record(&names, &parents, &[m0, m1]);

        let parsed = parse_skeleton_extender(&rec).expect("parse");
        assert_eq!(parsed.name_ids, names);
        assert_eq!(parsed.parent_name_ids, parents);
        assert_eq!(parsed.matrices.len(), 2);
        assert_eq!(parsed.matrices[0].0[12], -0.478);
        assert_eq!(parsed.matrices[1].0[12], 0.478);
        assert_eq!(parsed.matrices[0].0[14], 5.253);
    }

    #[test]
    fn empty_extender_yields_no_nodes() {
        let mut buf = vec![0u8; SKELETON_EXTENDER_ITEM_SIZE];
        // flag=0, count=0, relptrs=0
        buf[0] = 0;
        let parsed = parse_skeleton_extender(&buf).expect("parse");
        assert!(parsed.name_ids.is_empty());
        assert!(parsed.parent_name_ids.is_empty());
        assert!(parsed.matrices.is_empty());
    }

    #[test]
    fn too_short_record_errors() {
        let buf = vec![0u8; SKELETON_EXTENDER_ITEM_SIZE - 1];
        assert!(parse_skeleton_extender(&buf).is_err());
    }
}
