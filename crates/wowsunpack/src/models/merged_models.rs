//! Parser for space-level `models.bin` (MergedModels) files.
//!
//! These files contain all model instances for a space/map. Each record packs a
//! ModelPrototype, VisualPrototype, and SkeletonProto into a flat 0xA8-byte
//! record with struct-base-relative relptrs.
//!
//! See MODELS.md § "MergedModels (`models.bin`) Format" for full field layout.

use rootcause::Report;
use thiserror::Error;
use winnow::Parser;
use winnow::binary::le_i64;
use winnow::binary::le_u8;
use winnow::binary::le_u16;
use winnow::binary::le_u32;
use winnow::binary::le_u64;
use winnow::error::ContextError;
use winnow::error::ErrMode;
use winnow::token::take;

use crate::data::parser_utils::BoundingBox;
use crate::data::parser_utils::Matrix4x4;
use crate::data::parser_utils::WResult;
use crate::data::parser_utils::parse_lod_fields;
use crate::data::parser_utils::parse_matrix_array;
use crate::data::parser_utils::parse_render_set_fields;
use crate::data::parser_utils::parse_u16_array;
use crate::data::parser_utils::parse_u32_array;
use crate::data::parser_utils::resolve_relptr;
use crate::data::parser_utils::resolve_relptr_at;
use crate::data::parser_utils::{
    self,
};
use crate::models::model::ModelPrototype;
use crate::models::model::parse_model;
use crate::models::visual::Lod;
use crate::models::visual::RenderSet;
use crate::models::visual::VisualNodes;
use crate::models::visual::VisualPrototype;

/// Errors during `models.bin` parsing.
#[derive(Debug, Error)]
pub enum MergedModelsError {
    #[error("data too short: need {need} bytes at offset 0x{offset:X}, have {have}")]
    DataTooShort { offset: usize, need: usize, have: usize },
    #[error("parse error: {0}")]
    ParseError(String),
}

/// Parsed `models.bin` file.
#[derive(Debug)]
pub struct MergedModels {
    pub models: Vec<MergedModelRecord>,
    pub skeletons: Vec<SkeletonProto>,
    pub model_bone_count: u16,
}

/// A single model record from the merged array.
#[derive(Debug)]
pub struct MergedModelRecord {
    /// selfId identifying this model in pathsStorage.
    pub path_id: u64,
    /// Inlined ModelPrototype fields.
    pub model_proto: ModelPrototype,
    /// Inlined VisualPrototype (includes inline SkeletonProto).
    pub visual_proto: VisualPrototype,
    /// Index into the shared skeletons array.
    pub skeleton_proto_index: u32,
    /// First geometry mapping index for this model's render sets.
    pub render_set_geometry_start_idx: u16,
    /// Number of geometry mappings for this model.
    pub render_set_geometry_count: u16,
}

/// Shared skeleton prototype (stride 0x30).
#[derive(Debug)]
pub struct SkeletonProto {
    pub nodes: VisualNodes,
}

/// A single model instance from `space.bin`, combining a world transform
/// with a reference to the model prototype via `path_id`.
#[derive(Debug)]
pub struct SpaceInstance {
    /// 4×4 world transform matrix (column-major, row 3 = translation + w=1).
    pub transform: Matrix4x4,
    /// selfId matching a `MergedModelRecord::path_id` in the sibling `models.bin`.
    pub path_id: u64,
}

/// Parsed `space.bin` instance placements.
#[derive(Debug)]
pub struct SpaceInstances {
    pub instances: Vec<SpaceInstance>,
}

// ── Winnow sub-parsers (merged-specific) ────────────────────────────────────

// models.bin header (0x18 bytes)

struct MergedHeader {
    models_count: u32,
    skeletons_count: u16,
    model_bone_count: u16,
    models_relptr: i64,
    skeletons_relptr: i64,
}

fn parse_merged_header(input: &mut &[u8]) -> WResult<MergedHeader> {
    let models_count = le_u32.parse_next(input)?;
    let skeletons_count = le_u16.parse_next(input)?;
    let model_bone_count = le_u16.parse_next(input)?;
    let models_relptr = le_i64.parse_next(input)?;
    let skeletons_relptr = le_i64.parse_next(input)?;
    Ok(MergedHeader { models_count, skeletons_count, model_bone_count, models_relptr, skeletons_relptr })
}

// VisualProto inline fields (0x70 bytes at rec+0x30)

struct VisualProtoInlineFields {
    nodes_count: u32,
    name_map_name_ids_relptr: i64,
    name_map_node_ids_relptr: i64,
    name_ids_relptr: i64,
    matrices_relptr: i64,
    parent_ids_relptr: i64,
    merged_geometry_path_id: u64,
    underwater_model: bool,
    abovewater_model: bool,
    render_sets_count: u16,
    lods_count: u16,
    bounding_box: BoundingBox,
    render_sets_relptr: i64,
    lods_relptr: i64,
}

fn parse_visual_proto_inline_fields(input: &mut &[u8]) -> WResult<VisualProtoInlineFields> {
    // Skeleton sub-struct: +0x00..+0x30
    let nodes_count = le_u32.parse_next(input)?;
    let _pad = le_u32.parse_next(input)?;
    let name_map_name_ids_relptr = le_i64.parse_next(input)?;
    let name_map_node_ids_relptr = le_i64.parse_next(input)?;
    let name_ids_relptr = le_i64.parse_next(input)?;
    let matrices_relptr = le_i64.parse_next(input)?;
    let parent_ids_relptr = le_i64.parse_next(input)?;
    // VisualProto fields: +0x30..+0x70
    let merged_geometry_path_id = le_u64.parse_next(input)?;
    let underwater_model = le_u8.parse_next(input)? != 0;
    let abovewater_model = le_u8.parse_next(input)? != 0;
    let render_sets_count = le_u16.parse_next(input)?;
    let lods_count = le_u16.parse_next(input)?;
    let _ = take(2usize).parse_next(input)?; // padding to +0x40
    let bounding_box = parser_utils::parse_bounding_box(input)?;
    let render_sets_relptr = le_i64.parse_next(input)?;
    let lods_relptr = le_i64.parse_next(input)?;
    Ok(VisualProtoInlineFields {
        nodes_count,
        name_map_name_ids_relptr,
        name_map_node_ids_relptr,
        name_ids_relptr,
        matrices_relptr,
        parent_ids_relptr,
        merged_geometry_path_id,
        underwater_model,
        abovewater_model,
        render_sets_count,
        lods_count,
        bounding_box,
        render_sets_relptr,
        lods_relptr,
    })
}

// space.bin instance entry

fn parse_space_instance_entry(input: &mut &[u8]) -> WResult<SpaceInstance> {
    let transform = parser_utils::parse_matrix4x4(input)?;
    let _ = take(16usize).parse_next(input)?; // padding +0x40..+0x50
    let path_id = le_u64.parse_next(input)?;
    let _ = take(24usize).parse_next(input)?; // remaining to 0x70 stride
    Ok(SpaceInstance { transform, path_id })
}

// ── Helper: parse array at offset, wrapping winnow errors ───────────────────

fn parse_array_at<T>(
    data: &[u8],
    offset: usize,
    count: usize,
    parser: fn(&mut &[u8], usize) -> WResult<Vec<T>>,
) -> Result<Vec<T>, Report<MergedModelsError>> {
    if count == 0 {
        return Ok(Vec::new());
    }
    let input = &mut &data[offset..];
    parser(input, count).map_err(|e: ErrMode<ContextError>| {
        Report::new(MergedModelsError::ParseError(format!("array at 0x{offset:X}: {e}")))
    })
}

// ── Header ───────────────────────────────────────────────────────────────────

const HEADER_SIZE: usize = 0x18;
const MODEL_RECORD_SIZE: usize = 0xA8;
const SKELETON_SIZE: usize = 0x30;
const RENDER_SET_SIZE: usize = 0x28;
const LOD_SIZE: usize = 0x10;

/// Parse a `models.bin` file.
pub fn parse_merged_models(file_data: &[u8]) -> Result<MergedModels, Report<MergedModelsError>> {
    if file_data.len() < HEADER_SIZE {
        return Err(Report::new(MergedModelsError::DataTooShort {
            offset: 0,
            need: HEADER_SIZE,
            have: file_data.len(),
        }));
    }

    let input = &mut &file_data[..];
    let hdr = parse_merged_header(input)
        .map_err(|e: ErrMode<ContextError>| Report::new(MergedModelsError::ParseError(format!("header: {e}"))))?;

    let models_count = hdr.models_count as usize;
    let skeletons_count = hdr.skeletons_count as usize;
    let models_offset = resolve_relptr(0, hdr.models_relptr);
    let skeletons_offset = resolve_relptr(0, hdr.skeletons_relptr);

    // Parse model records
    let need = models_count * MODEL_RECORD_SIZE;
    if models_offset + need > file_data.len() {
        return Err(Report::new(MergedModelsError::DataTooShort {
            offset: models_offset,
            need,
            have: file_data.len(),
        }));
    }
    let mut models = Vec::with_capacity(models_count);
    for i in 0..models_count {
        let rec_base = models_offset + i * MODEL_RECORD_SIZE;
        let record = parse_model_record(file_data, rec_base)
            .map_err(|e| MergedModelsError::ParseError(format!("model[{i}]: {e}")))?;
        models.push(record);
    }

    // Parse shared skeleton prototypes
    let need = skeletons_count * SKELETON_SIZE;
    if skeletons_offset + need > file_data.len() {
        return Err(Report::new(MergedModelsError::DataTooShort {
            offset: skeletons_offset,
            need,
            have: file_data.len(),
        }));
    }
    let mut skeletons = Vec::with_capacity(skeletons_count);
    for i in 0..skeletons_count {
        let skel_base = skeletons_offset + i * SKELETON_SIZE;
        let skeleton = parse_skeleton_proto(file_data, skel_base)
            .map_err(|e| MergedModelsError::ParseError(format!("skeleton[{i}]: {e}")))?;
        skeletons.push(skeleton);
    }

    Ok(MergedModels { models, skeletons, model_bone_count: hdr.model_bone_count })
}

// ── space.bin parser ─────────────────────────────────────────────────────────

const SPACE_HEADER_SIZE: usize = 0x60;
const SPACE_INSTANCE_SIZE: usize = 0x70;

/// Parse a `space.bin` file to extract instance placements (world transforms).
pub fn parse_space_instances(file_data: &[u8]) -> Result<SpaceInstances, Report<MergedModelsError>> {
    if file_data.len() < SPACE_HEADER_SIZE {
        return Err(Report::new(MergedModelsError::DataTooShort {
            offset: 0,
            need: SPACE_HEADER_SIZE,
            have: file_data.len(),
        }));
    }

    let input = &mut &file_data[..];
    let instance_count = le_u32
        .parse_next(input)
        .map_err(|e: ErrMode<ContextError>| Report::new(MergedModelsError::ParseError(format!("space header: {e}"))))?
        as usize;

    let need = instance_count * SPACE_INSTANCE_SIZE;
    if SPACE_HEADER_SIZE + need > file_data.len() {
        return Err(Report::new(MergedModelsError::DataTooShort {
            offset: SPACE_HEADER_SIZE,
            need,
            have: file_data.len(),
        }));
    }

    let input = &mut &file_data[SPACE_HEADER_SIZE..];
    let instances: Vec<SpaceInstance> =
        winnow::combinator::repeat(instance_count, parse_space_instance_entry).parse_next(input).map_err(
            |e: ErrMode<ContextError>| Report::new(MergedModelsError::ParseError(format!("space instances: {e}"))),
        )?;

    Ok(SpaceInstances { instances })
}

// ── Model Record ─────────────────────────────────────────────────────────────

fn parse_model_record(data: &[u8], rec: usize) -> Result<MergedModelRecord, Report<MergedModelsError>> {
    if rec + MODEL_RECORD_SIZE > data.len() {
        return Err(Report::new(MergedModelsError::DataTooShort {
            offset: rec,
            need: MODEL_RECORD_SIZE,
            have: data.len(),
        }));
    }

    let input = &mut &data[rec..];
    let path_id = le_u64
        .parse_next(input)
        .map_err(|e: ErrMode<ContextError>| Report::new(MergedModelsError::ParseError(format!("path_id: {e}"))))?;

    // ModelProto at rec+0x08 (0x28 bytes)
    let model_proto_base = rec + 0x08;
    let model_proto = parse_model(&data[model_proto_base..])
        .map_err(|e| MergedModelsError::ParseError(format!("ModelProto at 0x{model_proto_base:X}: {e}")))?;

    // VisualProto at rec+0x30 (0x70 bytes)
    let vp_base = rec + 0x30;
    let visual_proto = parse_visual_proto_inline(data, vp_base)?;

    // Tail fields at rec+0xA0
    let input = &mut &data[rec + 0xA0..];
    let skeleton_proto_index = le_u32.parse_next(input).map_err(|e: ErrMode<ContextError>| {
        Report::new(MergedModelsError::ParseError(format!("skeleton_proto_index: {e}")))
    })?;
    let render_set_geometry_start_idx = le_u16.parse_next(input).map_err(|e: ErrMode<ContextError>| {
        Report::new(MergedModelsError::ParseError(format!("rs_geom_start: {e}")))
    })?;
    let render_set_geometry_count = le_u16.parse_next(input).map_err(|e: ErrMode<ContextError>| {
        Report::new(MergedModelsError::ParseError(format!("rs_geom_count: {e}")))
    })?;

    Ok(MergedModelRecord {
        path_id,
        model_proto,
        visual_proto,
        skeleton_proto_index,
        render_set_geometry_start_idx,
        render_set_geometry_count,
    })
}

// ── VisualProto (inline at rec+0x30, size 0x70) ─────────────────────────────

fn parse_visual_proto_inline(data: &[u8], vp_base: usize) -> Result<VisualPrototype, Report<MergedModelsError>> {
    if vp_base + 0x70 > data.len() {
        return Err(Report::new(MergedModelsError::DataTooShort { offset: vp_base, need: 0x70, have: data.len() }));
    }

    let input = &mut &data[vp_base..];
    let fields = parse_visual_proto_inline_fields(input).map_err(|e: ErrMode<ContextError>| {
        Report::new(MergedModelsError::ParseError(format!("visual_proto at 0x{vp_base:X}: {e}")))
    })?;

    let nodes_count = fields.nodes_count as usize;

    let nodes = if nodes_count > 0 {
        let name_map_name_ids = parse_array_at(
            data,
            resolve_relptr(vp_base, fields.name_map_name_ids_relptr),
            nodes_count,
            parse_u32_array,
        )?;
        let name_map_node_ids = parse_array_at(
            data,
            resolve_relptr(vp_base, fields.name_map_node_ids_relptr),
            nodes_count,
            parse_u16_array,
        )?;
        let name_ids =
            parse_array_at(data, resolve_relptr(vp_base, fields.name_ids_relptr), nodes_count, parse_u32_array)?;
        let matrices =
            parse_array_at(data, resolve_relptr(vp_base, fields.matrices_relptr), nodes_count, parse_matrix_array)?;
        let parent_ids =
            parse_array_at(data, resolve_relptr(vp_base, fields.parent_ids_relptr), nodes_count, parse_u16_array)?;

        VisualNodes { name_map_name_ids, name_map_node_ids, name_ids, matrices, parent_ids }
    } else {
        VisualNodes {
            name_map_name_ids: Vec::new(),
            name_map_node_ids: Vec::new(),
            name_ids: Vec::new(),
            matrices: Vec::new(),
            parent_ids: Vec::new(),
        }
    };

    let render_sets_count = fields.render_sets_count as usize;
    let lods_count = fields.lods_count as usize;

    let render_sets = if render_sets_count > 0 {
        let rs_abs = resolve_relptr(vp_base, fields.render_sets_relptr);
        parse_render_sets_merged(data, rs_abs, render_sets_count)?
    } else {
        Vec::new()
    };

    let lods = if lods_count > 0 {
        let lod_abs = resolve_relptr(vp_base, fields.lods_relptr);
        parse_lods_merged(data, lod_abs, lods_count)?
    } else {
        Vec::new()
    };

    Ok(VisualPrototype {
        nodes,
        merged_geometry_path_id: fields.merged_geometry_path_id,
        underwater_model: fields.underwater_model,
        abovewater_model: fields.abovewater_model,
        bounding_box: fields.bounding_box,
        render_sets,
        lods,
    })
}

// ── Skeleton nodes ──────────────────────────────────────────────────────────

fn parse_skeleton_nodes(data: &[u8], skel_base: usize) -> Result<VisualNodes, Report<MergedModelsError>> {
    if skel_base + 0x30 > data.len() {
        return Err(Report::new(MergedModelsError::DataTooShort { offset: skel_base, need: 0x30, have: data.len() }));
    }

    let input = &mut &data[skel_base..];
    let nodes_count = le_u32
        .parse_next(input)
        .map_err(|e: ErrMode<ContextError>| Report::new(MergedModelsError::ParseError(format!("nodes_count: {e}"))))?
        as usize;

    if nodes_count == 0 {
        return Ok(VisualNodes {
            name_map_name_ids: Vec::new(),
            name_map_node_ids: Vec::new(),
            name_ids: Vec::new(),
            matrices: Vec::new(),
            parent_ids: Vec::new(),
        });
    }

    let name_map_name_ids = {
        let abs = resolve_relptr_at(data, skel_base, 0x08).map_err(|e: ErrMode<ContextError>| {
            Report::new(MergedModelsError::ParseError(format!("skel relptr: {e}")))
        })?;
        parse_array_at(data, abs, nodes_count, parse_u32_array)?
    };
    let name_map_node_ids = {
        let abs = resolve_relptr_at(data, skel_base, 0x10).map_err(|e: ErrMode<ContextError>| {
            Report::new(MergedModelsError::ParseError(format!("skel relptr: {e}")))
        })?;
        parse_array_at(data, abs, nodes_count, parse_u16_array)?
    };
    let name_ids = {
        let abs = resolve_relptr_at(data, skel_base, 0x18).map_err(|e: ErrMode<ContextError>| {
            Report::new(MergedModelsError::ParseError(format!("skel relptr: {e}")))
        })?;
        parse_array_at(data, abs, nodes_count, parse_u32_array)?
    };
    let matrices = {
        let abs = resolve_relptr_at(data, skel_base, 0x20).map_err(|e: ErrMode<ContextError>| {
            Report::new(MergedModelsError::ParseError(format!("skel relptr: {e}")))
        })?;
        parse_array_at(data, abs, nodes_count, parse_matrix_array)?
    };
    let parent_ids = {
        let abs = resolve_relptr_at(data, skel_base, 0x28).map_err(|e: ErrMode<ContextError>| {
            Report::new(MergedModelsError::ParseError(format!("skel relptr: {e}")))
        })?;
        parse_array_at(data, abs, nodes_count, parse_u16_array)?
    };

    Ok(VisualNodes { name_map_name_ids, name_map_node_ids, name_ids, matrices, parent_ids })
}

fn parse_skeleton_proto(data: &[u8], skel_base: usize) -> Result<SkeletonProto, Report<MergedModelsError>> {
    let nodes = parse_skeleton_nodes(data, skel_base)?;
    Ok(SkeletonProto { nodes })
}

// ── RenderSet (stride 0x28) ─────────────────────────────────────────────────

fn parse_render_sets_merged(
    data: &[u8],
    offset: usize,
    count: usize,
) -> Result<Vec<RenderSet>, Report<MergedModelsError>> {
    let need = count * RENDER_SET_SIZE;
    if offset + need > data.len() {
        return Err(Report::new(MergedModelsError::DataTooShort { offset, need, have: data.len() }));
    }

    let mut result = Vec::with_capacity(count);
    for i in 0..count {
        let rs_base = offset + i * RENDER_SET_SIZE;
        let input = &mut &data[rs_base..];

        let fields = parse_render_set_fields(input).map_err(|e: ErrMode<ContextError>| {
            Report::new(MergedModelsError::ParseError(format!("render_set[{i}]: {e}")))
        })?;

        let node_name_ids = if fields.nodes_count > 0 {
            let abs = resolve_relptr(rs_base, fields.node_name_ids_relptr);
            parse_array_at(data, abs, fields.nodes_count as usize, parse_u32_array)?
        } else {
            Vec::new()
        };

        result.push(RenderSet {
            name_id: fields.name_id,
            material_name_id: fields.material_name_id,
            vertices_mapping_id: fields.vertices_mapping_id,
            indices_mapping_id: fields.indices_mapping_id,
            material_mfm_path_id: fields.material_mfm_path_id,
            skinned: fields.skinned,
            node_name_ids,
        });
    }

    Ok(result)
}

// ── LOD (stride 0x10) ───────────────────────────────────────────────────────

fn parse_lods_merged(data: &[u8], offset: usize, count: usize) -> Result<Vec<Lod>, Report<MergedModelsError>> {
    let need = count * LOD_SIZE;
    if offset + need > data.len() {
        return Err(Report::new(MergedModelsError::DataTooShort { offset, need, have: data.len() }));
    }

    let mut result = Vec::with_capacity(count);
    for i in 0..count {
        let lod_base = offset + i * LOD_SIZE;
        let input = &mut &data[lod_base..];

        let fields = parse_lod_fields(input)
            .map_err(|e: ErrMode<ContextError>| Report::new(MergedModelsError::ParseError(format!("lod[{i}]: {e}"))))?;

        let render_set_names = if fields.render_set_names_count > 0 {
            let abs = resolve_relptr(lod_base, fields.render_set_names_relptr);
            parse_array_at(data, abs, fields.render_set_names_count as usize, parse_u32_array)?
        } else {
            Vec::new()
        };

        result.push(Lod { extent: fields.extent, casts_shadow: fields.casts_shadow, render_set_names });
    }

    Ok(result)
}
