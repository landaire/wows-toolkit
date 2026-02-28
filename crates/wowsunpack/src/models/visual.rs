use rootcause::Report;
use thiserror::Error;
use winnow::Parser;
use winnow::binary::{le_i64, le_u8, le_u16, le_u32, le_u64};
use winnow::error::{ContextError, ErrMode};

use crate::data::parser_utils::{
    self, WResult, parse_lod_fields, parse_matrix_array, parse_render_set_fields, parse_u16_array, parse_u32_array,
    resolve_relptr,
};
use crate::models::assets_bin::{PrototypeDatabase, StringsSection};

// Re-export shared types so existing `use crate::models::visual::Matrix4x4` etc. still work.
pub use crate::data::parser_utils::{BoundingBox, Matrix4x4};

/// Errors that can occur during VisualPrototype parsing.
#[derive(Debug, Error)]
pub enum VisualError {
    #[error("data too short: need {need} bytes at offset 0x{offset:X}, have {have}")]
    DataTooShort { offset: usize, need: usize, have: usize },
    #[error("parse error: {0}")]
    ParseError(String),
}

/// Item size for VisualPrototype records in the database blob.
pub const VISUAL_ITEM_SIZE: usize = 0x70;

#[cfg(feature = "serde")]
fn serialize_hex_u64<S: serde::Serializer>(val: &u64, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&format!("0x{val:016x}"))
}

/// A parsed VisualPrototype record.
#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct VisualPrototype {
    pub nodes: VisualNodes,
    #[cfg_attr(feature = "serde", serde(serialize_with = "serialize_hex_u64"))]
    pub merged_geometry_path_id: u64,
    pub underwater_model: bool,
    pub abovewater_model: bool,
    pub bounding_box: BoundingBox,
    pub render_sets: Vec<RenderSet>,
    pub lods: Vec<Lod>,
}

/// Scene graph node hierarchy.
#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct VisualNodes {
    pub name_map_name_ids: Vec<u32>,
    pub name_map_node_ids: Vec<u16>,
    pub name_ids: Vec<u32>,
    pub matrices: Vec<Matrix4x4>,
    pub parent_ids: Vec<u16>,
}

/// A render set binding a mesh to a material.
#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RenderSet {
    pub name_id: u32,
    pub material_name_id: u32,
    pub vertices_mapping_id: u32,
    pub indices_mapping_id: u32,
    #[cfg_attr(feature = "serde", serde(serialize_with = "serialize_hex_u64"))]
    pub material_mfm_path_id: u64,
    pub skinned: bool,
    pub node_name_ids: Vec<u32>,
}

/// A level-of-detail entry.
#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Lod {
    pub extent: f32,
    pub casts_shadow: bool,
    pub render_set_names: Vec<u32>,
}

// ---------------------------------------------------------------------------
// Winnow sub-parsers (visual-specific header only; shared parsers in parser_utils)
// ---------------------------------------------------------------------------

/// Parse the fixed header of a VisualPrototype record (0x70 bytes).
struct VisualHeader {
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
    lods_count: u8,
    bounding_box: BoundingBox,
    render_sets_relptr: i64,
    lods_relptr: i64,
}

fn parse_visual_header(input: &mut &[u8]) -> WResult<VisualHeader> {
    let nodes_count = le_u32.parse_next(input)?;
    let _pad = le_u32.parse_next(input)?;
    let name_map_name_ids_relptr = le_i64.parse_next(input)?;
    let name_map_node_ids_relptr = le_i64.parse_next(input)?;
    let name_ids_relptr = le_i64.parse_next(input)?;
    let matrices_relptr = le_i64.parse_next(input)?;
    let parent_ids_relptr = le_i64.parse_next(input)?;
    let merged_geometry_path_id = le_u64.parse_next(input)?;
    let underwater_model = le_u8.parse_next(input)? != 0;
    let abovewater_model = le_u8.parse_next(input)? != 0;
    let render_sets_count = le_u16.parse_next(input)?;
    let lods_count = le_u8.parse_next(input)?;
    let _pad1 = le_u8.parse_next(input)?;
    let _pad2 = le_u8.parse_next(input)?;
    let _pad3 = le_u8.parse_next(input)?;
    let bounding_box = parser_utils::parse_bounding_box(input)?;
    let render_sets_relptr = le_i64.parse_next(input)?;
    let lods_relptr = le_i64.parse_next(input)?;

    Ok(VisualHeader {
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

// ---------------------------------------------------------------------------
// Top-level parse entry point
// ---------------------------------------------------------------------------

/// Parse a VisualPrototype from blob data.
///
/// `record_data` is a slice starting at the record's offset within the blob,
/// extending to the end of the blob (so relptrs can resolve into OOL data).
/// The first `VISUAL_ITEM_SIZE` bytes are the fixed record fields.
pub fn parse_visual(record_data: &[u8]) -> Result<VisualPrototype, Report<VisualError>> {
    if record_data.len() < VISUAL_ITEM_SIZE {
        return Err(Report::new(VisualError::DataTooShort {
            offset: 0,
            need: VISUAL_ITEM_SIZE,
            have: record_data.len(),
        }));
    }

    let hdr = {
        let input = &mut &record_data[..];
        parse_visual_header(input)
            .map_err(|e: ErrMode<ContextError>| Report::new(VisualError::ParseError(format!("header: {e}"))))?
    };

    let base = 0usize;
    let nodes_count = hdr.nodes_count as usize;

    let nodes = if nodes_count > 0 {
        let name_map_name_ids = parse_array_at(
            record_data,
            resolve_relptr(base, hdr.name_map_name_ids_relptr),
            nodes_count,
            parse_u32_array,
        )?;
        let name_map_node_ids = parse_array_at(
            record_data,
            resolve_relptr(base, hdr.name_map_node_ids_relptr),
            nodes_count,
            parse_u16_array,
        )?;
        let name_ids =
            parse_array_at(record_data, resolve_relptr(base, hdr.name_ids_relptr), nodes_count, parse_u32_array)?;
        let matrices =
            parse_array_at(record_data, resolve_relptr(base, hdr.matrices_relptr), nodes_count, parse_matrix_array)?;
        let parent_ids =
            parse_array_at(record_data, resolve_relptr(base, hdr.parent_ids_relptr), nodes_count, parse_u16_array)?;

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

    let render_sets_count = hdr.render_sets_count as usize;
    let lods_count = hdr.lods_count as usize;

    let render_sets = if render_sets_count > 0 {
        let rs_abs = resolve_relptr(base, hdr.render_sets_relptr);
        parse_render_sets(record_data, rs_abs, render_sets_count)?
    } else {
        Vec::new()
    };

    let lods = if lods_count > 0 {
        let lod_abs = resolve_relptr(base, hdr.lods_relptr);
        parse_lods(record_data, lod_abs, lods_count)?
    } else {
        Vec::new()
    };

    Ok(VisualPrototype {
        nodes,
        merged_geometry_path_id: hdr.merged_geometry_path_id,
        underwater_model: hdr.underwater_model,
        abovewater_model: hdr.abovewater_model,
        bounding_box: hdr.bounding_box,
        render_sets,
        lods,
    })
}

// ---------------------------------------------------------------------------
// Helper: parse an array at a given offset, wrapping winnow errors
// ---------------------------------------------------------------------------

fn parse_array_at<T>(
    data: &[u8],
    offset: usize,
    count: usize,
    parser: fn(&mut &[u8], usize) -> WResult<Vec<T>>,
) -> Result<Vec<T>, Report<VisualError>> {
    let input = &mut &data[offset..];
    parser(input, count)
        .map_err(|e: ErrMode<ContextError>| Report::new(VisualError::ParseError(format!("array at 0x{offset:X}: {e}"))))
}

// ---------------------------------------------------------------------------
// Sub-structure parsers
// ---------------------------------------------------------------------------

const RENDER_SET_SIZE: usize = 0x28;

fn parse_render_sets(blob_data: &[u8], offset: usize, count: usize) -> Result<Vec<RenderSet>, Report<VisualError>> {
    let need = count * RENDER_SET_SIZE;
    if offset + need > blob_data.len() {
        return Err(Report::new(VisualError::DataTooShort { offset, need, have: blob_data.len() }));
    }

    let mut result = Vec::with_capacity(count);
    for i in 0..count {
        let rs_base = offset + i * RENDER_SET_SIZE;
        let input = &mut &blob_data[rs_base..];

        let fields = parse_render_set_fields(input).map_err(|e: ErrMode<ContextError>| {
            Report::new(VisualError::ParseError(format!("render_set[{i}]: {e}")))
        })?;

        let node_name_ids = if fields.nodes_count > 0 {
            let abs = resolve_relptr(rs_base, fields.node_name_ids_relptr);
            parse_array_at(blob_data, abs, fields.nodes_count as usize, parse_u32_array)?
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

const LOD_SIZE: usize = 0x10;

fn parse_lods(blob_data: &[u8], offset: usize, count: usize) -> Result<Vec<Lod>, Report<VisualError>> {
    let need = count * LOD_SIZE;
    if offset + need > blob_data.len() {
        return Err(Report::new(VisualError::DataTooShort { offset, need, have: blob_data.len() }));
    }

    let mut result = Vec::with_capacity(count);
    for i in 0..count {
        let lod_base = offset + i * LOD_SIZE;
        let input = &mut &blob_data[lod_base..];

        let fields = parse_lod_fields(input)
            .map_err(|e: ErrMode<ContextError>| Report::new(VisualError::ParseError(format!("lod[{i}]: {e}"))))?;

        let render_set_names = if fields.render_set_names_count > 0 {
            let abs = resolve_relptr(lod_base, fields.render_set_names_relptr);
            parse_array_at(blob_data, abs, fields.render_set_names_count as usize, parse_u32_array)?
        } else {
            Vec::new()
        };

        result.push(Lod { extent: fields.extent, casts_shadow: fields.casts_shadow, render_set_names });
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// VisualPrototype methods (unchanged)
// ---------------------------------------------------------------------------

impl VisualPrototype {
    /// Resolve string IDs and path IDs using the database.
    pub fn print_summary(&self, db: &PrototypeDatabase<'_>) {
        let strings = &db.strings;
        let self_id_index = db.build_self_id_index();

        let resolve_path_leaf = |self_id: u64| -> String {
            if self_id == 0 {
                return "(none)".to_string();
            }
            match self_id_index.get(&self_id) {
                Some(&idx) => db.paths_storage[idx].name.clone(),
                None => format!("0x{self_id:016X}"),
            }
        };

        println!("  Nodes: {}", self.nodes.name_ids.len());
        for (i, &name_id) in self.nodes.name_ids.iter().enumerate() {
            let name = strings.get_string_by_id(name_id).unwrap_or("<unknown>");
            let parent = self.nodes.parent_ids[i];
            let parent_str = if parent == 0xFFFF { "root".to_string() } else { format!("{parent}") };
            println!("    [{i}] name=\"{name}\" parent={parent_str}");
        }

        let geom_name = resolve_path_leaf(self.merged_geometry_path_id);
        println!("  MergedGeometry: {geom_name}");
        println!("  UnderwaterModel: {}", self.underwater_model);
        println!("  AbovewaterModel: {}", self.abovewater_model);
        println!(
            "  BoundingBox: min=({:.3}, {:.3}, {:.3}) max=({:.3}, {:.3}, {:.3})",
            self.bounding_box.min[0],
            self.bounding_box.min[1],
            self.bounding_box.min[2],
            self.bounding_box.max[0],
            self.bounding_box.max[1],
            self.bounding_box.max[2],
        );

        println!("  RenderSets: {}", self.render_sets.len());
        for (i, rs) in self.render_sets.iter().enumerate() {
            let name = strings.get_string_by_id(rs.name_id).unwrap_or("<unknown>");
            let mat_name = strings.get_string_by_id(rs.material_name_id).unwrap_or("<unknown>");
            let mfm_name = resolve_path_leaf(rs.material_mfm_path_id);
            println!(
                "    [{i}] name=\"{name}\" material=\"{mat_name}\" mfm=\"{mfm_name}\" skinned={} nodes={}\n        vertices_mapping=0x{:08X} indices_mapping=0x{:08X}",
                rs.skinned,
                rs.node_name_ids.len(),
                rs.vertices_mapping_id,
                rs.indices_mapping_id,
            );
        }

        println!("  LODs: {}", self.lods.len());
        for (i, lod) in self.lods.iter().enumerate() {
            let rs_names: Vec<String> = lod
                .render_set_names
                .iter()
                .map(|&id| strings.get_string_by_id(id).unwrap_or("<unknown>").to_string())
                .collect();
            println!(
                "    [{i}] extent={:.1} shadow={} renderSets=[{}]",
                lod.extent,
                lod.casts_shadow,
                rs_names.join(", ")
            );
        }
    }

    /// Find the world-space transform for a named hardpoint node.
    pub fn find_hardpoint_transform(&self, hp_name: &str, strings: &StringsSection<'_>) -> Option<[f32; 16]> {
        let node_idx = self.find_node_index_by_name(hp_name, strings)?;

        let mut result = self.nodes.matrices[node_idx as usize].0;
        let mut current = node_idx;
        loop {
            let parent = self.nodes.parent_ids[current as usize];
            if parent == 0xFFFF || parent as usize >= self.nodes.matrices.len() {
                break;
            }
            result = mat4_mul(&self.nodes.matrices[parent as usize].0, &result);
            current = parent;
        }

        Some(result)
    }

    /// Get the local (non-composed) matrix of a named node.
    pub fn find_node_local_matrix(&self, name: &str, strings: &StringsSection<'_>) -> Option<[f32; 16]> {
        let node_idx = self.find_node_index_by_name(name, strings)?;
        Some(self.nodes.matrices[node_idx as usize].0)
    }

    /// Check whether `node_idx` is a descendant of `ancestor_idx` in the
    /// skeleton hierarchy.
    pub fn is_descendant_of(&self, mut node_idx: u16, ancestor_idx: u16) -> bool {
        loop {
            let parent = self.nodes.parent_ids[node_idx as usize];
            if parent == 0xFFFF || parent as usize >= self.nodes.parent_ids.len() {
                return false;
            }
            if parent == ancestor_idx {
                return true;
            }
            node_idx = parent;
        }
    }

    /// Find the node index for a given node name string.
    pub fn find_node_index_by_name(&self, name: &str, strings: &StringsSection<'_>) -> Option<u16> {
        for (i, &name_id) in self.nodes.name_map_name_ids.iter().enumerate() {
            if let Some(resolved) = strings.get_string_by_id(name_id)
                && resolved == name
            {
                return Some(self.nodes.name_map_node_ids[i]);
            }
        }
        None
    }
}

/// Multiply two 4x4 matrices (column-major order, as stored in BigWorld).
fn mat4_mul(a: &[f32; 16], b: &[f32; 16]) -> [f32; 16] {
    let mut out = [0.0f32; 16];
    for col in 0..4 {
        for row in 0..4 {
            let mut sum = 0.0;
            for k in 0..4 {
                sum += a[k * 4 + row] * b[col * 4 + k];
            }
            out[col * 4 + row] = sum;
        }
    }
    out
}
