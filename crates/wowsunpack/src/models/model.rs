//! Parser for ModelPrototype records (blob index 3, item size 0x28).
//!
//! ModelPrototype wraps a VisualPrototype with additional skeleton extension,
//! animation, and dye/tint data. The key field is `visual_resource_id` which
//! is a selfId (path hash) pointing to the corresponding `.visual` entry in
//! pathsStorage.

use rootcause::Report;
use thiserror::Error;
use winnow::Parser;
use winnow::binary::le_i32;
use winnow::binary::le_i64;
use winnow::binary::le_u8;
use winnow::binary::le_u32;
use winnow::binary::le_u64;
use winnow::combinator::repeat;
use winnow::token::take;

use winnow::error::ContextError;
use winnow::error::ErrMode;

use crate::data::parser_utils::WResult;
use crate::data::parser_utils::resolve_relptr;

/// Errors that can occur during ModelPrototype parsing.
#[derive(Debug, Error)]
pub enum ModelError {
    #[error("data too short: need {need} bytes at offset 0x{offset:X}, have {have}")]
    DataTooShort { offset: usize, need: usize, have: usize },
}

/// Item size for ModelPrototype records in the database blob.
pub const MODEL_ITEM_SIZE: usize = 0x28;

#[cfg(feature = "serde")]
fn serialize_hex_u64<S: serde::Serializer>(val: &u64, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&format!("0x{val:016x}"))
}

#[cfg(feature = "serde")]
fn serialize_hex_u64_vec<S: serde::Serializer>(val: &[u64], s: S) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeSeq;
    let mut seq = s.serialize_seq(Some(val.len()))?;
    for v in val {
        seq.serialize_element(&format!("0x{v:016x}"))?;
    }
    seq.end()
}

/// A parsed ModelPrototype record.
#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ModelPrototype {
    /// selfId (path hash) of the referenced `.visual` in pathsStorage.
    #[cfg_attr(feature = "serde", serde(serialize_with = "serialize_hex_u64"))]
    pub visual_resource_id: u64,
    /// Unknown byte at offset +0x09; purpose unclear.
    pub misc_type: u8,
    /// Skeleton extension resource IDs (selfIds of skeleton extender prototypes).
    #[cfg_attr(feature = "serde", serde(serialize_with = "serialize_hex_u64_vec"))]
    pub skel_ext_res_ids: Vec<u64>,
    /// Animation entries (each has the same layout as a ModelPrototype record).
    pub animations: Vec<ModelPrototype>,
    /// Dye entries for camouflage / cosmetic material replacement.
    pub dyes: Vec<DyeEntry>,
}

/// Material dye/tint replacement entry.
#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DyeEntry {
    /// String ID of the target material name.
    pub matter_id: u32,
    /// String ID of the replacement material name.
    pub replaces_id: u32,
    /// String IDs of tint variant names.
    pub tint_name_ids: Vec<u32>,
    /// selfIds of tint variant .mfm material files.
    #[cfg_attr(feature = "serde", serde(serialize_with = "serialize_hex_u64_vec"))]
    pub tint_material_ids: Vec<u64>,
}

/// Fixed header fields of a ModelPrototype (0x28 bytes).
struct ModelHeaderFields {
    visual_resource_id: u64,
    skel_ext_count: u8,
    misc_type: u8,
    animations_count: u8,
    dyes_count: u8,
    skel_ext_relptr: i64,
    animations_relptr: i64,
    dye_relptr: i64,
}

/// Parse the fixed 0x28-byte ModelPrototype header.
fn parse_model_header(input: &mut &[u8]) -> WResult<ModelHeaderFields> {
    let visual_resource_id = le_u64.parse_next(input)?;
    let skel_ext_count = le_u8.parse_next(input)?;
    let misc_type = le_u8.parse_next(input)?;
    let animations_count = le_u8.parse_next(input)?;
    let dyes_count = le_u8.parse_next(input)?;
    // 4 bytes padding between +0x0C and +0x10
    let _ = take(4usize).parse_next(input)?;
    let skel_ext_relptr = le_i64.parse_next(input)?;
    let animations_relptr = le_i64.parse_next(input)?;
    let dye_relptr = le_i64.parse_next(input)?;
    Ok(ModelHeaderFields {
        visual_resource_id,
        skel_ext_count,
        misc_type,
        animations_count,
        dyes_count,
        skel_ext_relptr,
        animations_relptr,
        dye_relptr,
    })
}

const DYE_ENTRY_SIZE: usize = 0x20;

/// Fixed header fields of a DyeEntry (0x20 bytes).
struct DyeHeaderFields {
    matter_id: u32,
    replaces_id: u32,
    tints_count: i32,
    tint_names_relptr: i64,
    tint_materials_relptr: i64,
}

/// Parse the fixed 0x20-byte DyeEntry header.
fn parse_dye_header(input: &mut &[u8]) -> WResult<DyeHeaderFields> {
    let matter_id = le_u32.parse_next(input)?;
    let replaces_id = le_u32.parse_next(input)?;
    let tints_count = le_i32.parse_next(input)?;
    // 4 bytes padding between +0x0C and +0x10
    let _ = take(4usize).parse_next(input)?;
    let tint_names_relptr = le_i64.parse_next(input)?;
    let tint_materials_relptr = le_i64.parse_next(input)?;
    Ok(DyeHeaderFields { matter_id, replaces_id, tints_count, tint_names_relptr, tint_materials_relptr })
}

/// Parse a ModelPrototype from blob data.
///
/// `record_data` is a slice starting at the record's offset within the blob,
/// extending to the end of the blob (so relptrs can resolve into OOL data).
/// The first `MODEL_ITEM_SIZE` bytes are the fixed record fields.
pub fn parse_model(record_data: &[u8]) -> Result<ModelPrototype, Report<ModelError>> {
    parse_model_at(record_data, 0)
}

/// Parse a ModelPrototype at the given base offset within `blob_data`.
fn parse_model_at(blob_data: &[u8], base: usize) -> Result<ModelPrototype, Report<ModelError>> {
    if base + MODEL_ITEM_SIZE > blob_data.len() {
        return Err(Report::new(ModelError::DataTooShort {
            offset: base,
            need: MODEL_ITEM_SIZE,
            have: blob_data.len(),
        }));
    }

    let input = &mut &blob_data[base..];
    let hdr = parse_model_header(input).map_err(|_| ModelError::DataTooShort {
        offset: base,
        need: MODEL_ITEM_SIZE,
        have: blob_data.len(),
    })?;

    let skel_ext_count = hdr.skel_ext_count as usize;
    let animations_count = hdr.animations_count as usize;
    let dyes_count = hdr.dyes_count as usize;

    // skelExtResIds: array of u64, relptr at +0x10
    let skel_ext_res_ids = if skel_ext_count > 0 {
        let abs = resolve_relptr(base, hdr.skel_ext_relptr);
        let need = skel_ext_count * 8;
        if abs + need > blob_data.len() {
            return Err(Report::new(ModelError::DataTooShort { offset: abs, need, have: blob_data.len() }));
        }
        let input = &mut &blob_data[abs..];
        let ids: Vec<u64> = repeat(skel_ext_count, le_u64).parse_next(input).map_err(|_: ErrMode<ContextError>| {
            ModelError::DataTooShort { offset: abs, need, have: blob_data.len() }
        })?;
        ids
    } else {
        Vec::new()
    };

    // animations: array of ModelPrototype (0x28 each), relptr at +0x18
    let animations = if animations_count > 0 {
        let abs = resolve_relptr(base, hdr.animations_relptr);
        let need = animations_count * MODEL_ITEM_SIZE;
        if abs + need > blob_data.len() {
            return Err(Report::new(ModelError::DataTooShort { offset: abs, need, have: blob_data.len() }));
        }
        let mut anims = Vec::with_capacity(animations_count);
        for i in 0..animations_count {
            anims.push(parse_model_at(blob_data, abs + i * MODEL_ITEM_SIZE)?);
        }
        anims
    } else {
        Vec::new()
    };

    // dyes: array of DyeEntry (0x20 each), relptr at +0x20
    let dyes = if dyes_count > 0 {
        let abs = resolve_relptr(base, hdr.dye_relptr);
        parse_dye_entries(blob_data, abs, dyes_count)?
    } else {
        Vec::new()
    };

    Ok(ModelPrototype {
        visual_resource_id: hdr.visual_resource_id,
        misc_type: hdr.misc_type,
        skel_ext_res_ids,
        animations,
        dyes,
    })
}

fn parse_dye_entries(blob_data: &[u8], offset: usize, count: usize) -> Result<Vec<DyeEntry>, Report<ModelError>> {
    let need = count * DYE_ENTRY_SIZE;
    if offset + need > blob_data.len() {
        return Err(Report::new(ModelError::DataTooShort { offset, need, have: blob_data.len() }));
    }

    let mut result = Vec::with_capacity(count);
    for i in 0..count {
        let dye_base = offset + i * DYE_ENTRY_SIZE;

        let input = &mut &blob_data[dye_base..];
        let hdr = parse_dye_header(input).map_err(|_| ModelError::DataTooShort {
            offset: dye_base,
            need: DYE_ENTRY_SIZE,
            have: blob_data.len(),
        })?;

        let tints_count = hdr.tints_count.max(0) as usize;

        // tintNameIds: array of u32, relptr at +0x10
        let tint_name_ids = if tints_count > 0 {
            let abs = resolve_relptr(dye_base, hdr.tint_names_relptr);
            let need = tints_count * 4;
            if abs + need > blob_data.len() {
                return Err(Report::new(ModelError::DataTooShort { offset: abs, need, have: blob_data.len() }));
            }
            let input = &mut &blob_data[abs..];
            let ids: Vec<u32> = repeat(tints_count, le_u32).parse_next(input).map_err(|_: ErrMode<ContextError>| {
                ModelError::DataTooShort { offset: abs, need, have: blob_data.len() }
            })?;
            ids
        } else {
            Vec::new()
        };

        // tintMaterialIds: array of u64, relptr at +0x18
        let tint_material_ids = if tints_count > 0 {
            let abs = resolve_relptr(dye_base, hdr.tint_materials_relptr);
            let need = tints_count * 8;
            if abs + need > blob_data.len() {
                return Err(Report::new(ModelError::DataTooShort { offset: abs, need, have: blob_data.len() }));
            }
            let input = &mut &blob_data[abs..];
            let ids: Vec<u64> = repeat(tints_count, le_u64).parse_next(input).map_err(|_: ErrMode<ContextError>| {
                ModelError::DataTooShort { offset: abs, need, have: blob_data.len() }
            })?;
            ids
        } else {
            Vec::new()
        };

        result.push(DyeEntry {
            matter_id: hdr.matter_id,
            replaces_id: hdr.replaces_id,
            tint_name_ids,
            tint_material_ids,
        });
    }

    Ok(result)
}
