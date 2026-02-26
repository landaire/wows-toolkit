use rootcause::Report;
use thiserror::Error;
use winnow::Parser;
use winnow::binary::{le_i64, le_u8, le_u16, le_u32};
use winnow::combinator::repeat;
use winnow::token::take;

use crate::data::parser_utils::{WResult, parse_packed_string, resolve_relptr};

const ENCD_MAGIC: u32 = 0x44434E45;

/// Errors that can occur during `.geometry` file parsing.
#[derive(Debug, Error)]
pub enum GeometryError {
    #[error("invalid vertex stride: {0}")]
    InvalidStride(usize),
    #[error("unsupported index size: {0}")]
    UnsupportedIndexSize(u16),
    #[error("data extends beyond file at 0x{offset:X}")]
    OutOfBounds { offset: usize },
    #[error("meshopt decode error: {0}")]
    DecodeError(String),
    #[error("parse error: {0}")]
    ParseError(String),
}

#[derive(Debug)]
pub struct MergedGeometry<'a> {
    pub vertices_mapping: Vec<MappingEntry>,
    pub indices_mapping: Vec<MappingEntry>,
    pub merged_vertices: Vec<VerticesPrototype<'a>>,
    pub merged_indices: Vec<IndicesPrototype<'a>>,
    pub collision_models: Vec<ModelPrototype<'a>>,
    pub armor_models: Vec<ArmorModel>,
}

/// A single triangle in an armor model.
#[derive(Debug, Clone, Copy)]
pub struct ArmorTriangle {
    pub vertices: [[f32; 3]; 3],
    pub normals: [[f32; 3]; 3],
    /// Collision material ID from the BVH node header (byte 0 of the u32 first_dword).
    /// Maps to a collision material name (e.g. "Cit_Belt", "Bow_Bottom").
    pub material_id: u8,
    /// 1-based layer index from the BVH node header (byte 2 of the u32 first_dword).
    /// The u32 encodes `(layer_index << 16) | material_id`.
    /// Multi-layer armor materials have separate BVH node groups per layer,
    /// each covering different spatial regions. This maps directly to the
    /// `model_index` in GameParams armor keys: `(model_index << 16) | material_id`.
    pub layer_index: u8,
}

/// A parsed armor model containing triangle geometry for hit detection.
#[derive(Debug)]
pub struct ArmorModel {
    pub name: String,
    pub triangles: Vec<ArmorTriangle>,
}

/// A named axis-aligned bounding box from a `.splash` file.
/// Used to classify armor triangles into hit-location zones.
#[derive(Debug, Clone)]
pub struct SplashBox {
    pub name: String,
    pub min: [f32; 3],
    pub max: [f32; 3],
}

#[derive(Debug, Clone)]
pub struct MappingEntry {
    pub mapping_id: u32,
    pub merged_buffer_index: u16,
    pub packed_texel_density: u16,
    pub items_offset: u32,
    pub items_count: u32,
}

#[derive(Debug)]
pub struct VerticesPrototype<'a> {
    pub data: VertexData<'a>,
    pub format_name: String,
    pub size_in_bytes: u32,
    pub stride_in_bytes: u16,
    pub is_skinned: bool,
    pub is_bumped: bool,
}

#[derive(Debug)]
pub struct IndicesPrototype<'a> {
    pub data: IndexData<'a>,
    pub size_in_bytes: u32,
    pub index_size: u16,
}

#[derive(Debug)]
pub struct ModelPrototype<'a> {
    pub data: &'a [u8],
    pub name: String,
    pub size_in_bytes: u32,
}

#[derive(Debug)]
pub enum VertexData<'a> {
    Encoded { element_count: u32, payload: &'a [u8], stride: u16 },
    Raw(&'a [u8]),
}

#[derive(Debug)]
pub enum IndexData<'a> {
    Encoded { element_count: u32, payload: &'a [u8], index_size: u16 },
    Raw(&'a [u8]),
}

/// Decode a meshoptimizer-encoded vertex buffer with a runtime-known stride.
///
/// meshopt_rs requires `size_of::<Vertex>() == stride`, but our stride is only known at
/// runtime. We dispatch to a monomorphized call for each supported stride value.
fn decode_vertex_buffer_dynamic(count: usize, stride: usize, encoded: &[u8]) -> Result<Vec<u8>, Report<GeometryError>> {
    if stride == 0 || stride > 256 || !stride.is_multiple_of(4) {
        return Err(Report::new(GeometryError::InvalidStride(stride)));
    }

    let total_bytes = count * stride;
    let mut output = vec![0u8; total_bytes];

    macro_rules! decode_with_stride {
        ($stride:literal, $count:expr, $encoded:expr, $output:expr) => {{
            #[repr(C, align(4))]
            #[derive(Copy, Clone)]
            struct Vertex([u8; $stride]);
            // Safety: output buffer has exactly count * stride bytes, and Vertex has size = stride.
            // The decode function reads/writes through the slice as raw bytes internally.
            let vertex_slice: &mut [Vertex] =
                unsafe { std::slice::from_raw_parts_mut($output.as_mut_ptr() as *mut Vertex, $count) };
            meshopt_rs::vertex::buffer::decode_vertex_buffer(vertex_slice, $encoded)
                .map_err(|e| Report::new(GeometryError::DecodeError(format!("{e:?}"))))?;
        }};
    }

    match stride {
        4 => decode_with_stride!(4, count, encoded, output),
        8 => decode_with_stride!(8, count, encoded, output),
        12 => decode_with_stride!(12, count, encoded, output),
        16 => decode_with_stride!(16, count, encoded, output),
        20 => decode_with_stride!(20, count, encoded, output),
        24 => decode_with_stride!(24, count, encoded, output),
        28 => decode_with_stride!(28, count, encoded, output),
        32 => decode_with_stride!(32, count, encoded, output),
        36 => decode_with_stride!(36, count, encoded, output),
        40 => decode_with_stride!(40, count, encoded, output),
        44 => decode_with_stride!(44, count, encoded, output),
        48 => decode_with_stride!(48, count, encoded, output),
        52 => decode_with_stride!(52, count, encoded, output),
        56 => decode_with_stride!(56, count, encoded, output),
        60 => decode_with_stride!(60, count, encoded, output),
        64 => decode_with_stride!(64, count, encoded, output),
        _ => return Err(Report::new(GeometryError::InvalidStride(stride))),
    }

    Ok(output)
}

impl VertexData<'_> {
    pub fn decode(&self) -> Result<Vec<u8>, Report<GeometryError>> {
        match self {
            VertexData::Encoded { element_count, payload, stride } => {
                decode_vertex_buffer_dynamic(*element_count as usize, *stride as usize, payload)
            }
            VertexData::Raw(data) => Ok(data.to_vec()),
        }
    }
}

impl IndexData<'_> {
    pub fn decode(&self) -> Result<Vec<u8>, Report<GeometryError>> {
        match self {
            IndexData::Encoded { element_count, payload, index_size } => {
                let count = *element_count as usize;
                let mut output = vec![0u32; count];
                meshopt_rs::index::buffer::decode_index_buffer(&mut output, payload)
                    .map_err(|e| Report::new(GeometryError::DecodeError(format!("{e:?}"))))?;

                match index_size {
                    2 => Ok(output.iter().flat_map(|i| (*i as u16).to_le_bytes()).collect()),
                    4 => Ok(output.iter().flat_map(|i| i.to_le_bytes()).collect()),
                    other => Err(Report::new(GeometryError::UnsupportedIndexSize(*other))),
                }
            }
            IndexData::Raw(data) => Ok(data.to_vec()),
        }
    }
}

fn parse_mapping_entry(input: &mut &[u8]) -> WResult<MappingEntry> {
    let mapping_id = le_u32.parse_next(input)?;
    let merged_buffer_index = le_u16.parse_next(input)?;
    let packed_texel_density = le_u16.parse_next(input)?;
    let items_offset = le_u32.parse_next(input)?;
    let items_count = le_u32.parse_next(input)?;
    Ok(MappingEntry { mapping_id, merged_buffer_index, packed_texel_density, items_offset, items_count })
}

/// Header fields parsed by winnow, before sub-structure resolution.
struct HeaderFields {
    merged_vertices_count: u32,
    merged_indices_count: u32,
    vertices_mapping_count: u32,
    indices_mapping_count: u32,
    collision_model_count: u32,
    armor_model_count: u32,
    vertices_mapping_ptr: i64,
    indices_mapping_ptr: i64,
    merged_vertices_ptr: i64,
    merged_indices_ptr: i64,
    collision_models_ptr: i64,
    armor_models_ptr: i64,
}

fn parse_header(input: &mut &[u8]) -> WResult<HeaderFields> {
    let merged_vertices_count = le_u32.parse_next(input)?;
    let merged_indices_count = le_u32.parse_next(input)?;
    let vertices_mapping_count = le_u32.parse_next(input)?;
    let indices_mapping_count = le_u32.parse_next(input)?;
    let collision_model_count = le_u32.parse_next(input)?;
    let armor_model_count = le_u32.parse_next(input)?;
    let vertices_mapping_ptr = le_i64.parse_next(input)?;
    let indices_mapping_ptr = le_i64.parse_next(input)?;
    let merged_vertices_ptr = le_i64.parse_next(input)?;
    let merged_indices_ptr = le_i64.parse_next(input)?;
    let collision_models_ptr = le_i64.parse_next(input)?;
    let armor_models_ptr = le_i64.parse_next(input)?;
    Ok(HeaderFields {
        merged_vertices_count,
        merged_indices_count,
        vertices_mapping_count,
        indices_mapping_count,
        collision_model_count,
        armor_model_count,
        vertices_mapping_ptr,
        indices_mapping_ptr,
        merged_vertices_ptr,
        merged_indices_ptr,
        collision_models_ptr,
        armor_models_ptr,
    })
}

/// Parse the header and resolve all sub-structures from the full file data.
pub fn parse_geometry(file_data: &[u8]) -> Result<MergedGeometry<'_>, Report<GeometryError>> {
    let input = &mut &file_data[..];

    let hdr = parse_header(input).map_err(|e| Report::new(GeometryError::ParseError(format!("header: {e}"))))?;

    let header_base = 0usize;

    let vm_offset = resolve_relptr(header_base, hdr.vertices_mapping_ptr);
    let vertices_mapping = parse_mapping_array(file_data, vm_offset, hdr.vertices_mapping_count as usize)?;

    let im_offset = resolve_relptr(header_base, hdr.indices_mapping_ptr);
    let indices_mapping = parse_mapping_array(file_data, im_offset, hdr.indices_mapping_count as usize)?;

    let mv_offset = resolve_relptr(header_base, hdr.merged_vertices_ptr);
    let merged_vertices = parse_vertices_array(file_data, mv_offset, hdr.merged_vertices_count as usize)?;

    let mi_offset = resolve_relptr(header_base, hdr.merged_indices_ptr);
    let merged_indices = parse_indices_array(file_data, mi_offset, hdr.merged_indices_count as usize)?;

    let collision_models = if hdr.collision_model_count > 0 {
        let cm_offset = resolve_relptr(header_base, hdr.collision_models_ptr);
        parse_model_array(file_data, cm_offset, hdr.collision_model_count as usize)?
    } else {
        Vec::new()
    };

    let armor_models = if hdr.armor_model_count > 0 {
        let am_offset = resolve_relptr(header_base, hdr.armor_models_ptr);
        parse_armor_model_array(file_data, am_offset, hdr.armor_model_count as usize)?
    } else {
        Vec::new()
    };

    Ok(MergedGeometry {
        vertices_mapping,
        indices_mapping,
        merged_vertices,
        merged_indices,
        collision_models,
        armor_models,
    })
}

fn parse_mapping_array(
    file_data: &[u8],
    offset: usize,
    count: usize,
) -> Result<Vec<MappingEntry>, Report<GeometryError>> {
    let input = &mut &file_data[offset..];
    let entries: Vec<MappingEntry> = repeat(count, parse_mapping_entry)
        .parse_next(input)
        .map_err(|e| Report::new(GeometryError::ParseError(format!("mapping entries at 0x{offset:X}: {e}"))))?;
    Ok(entries)
}

fn parse_vertex_data<'a>(
    file_data: &'a [u8],
    data_offset: usize,
    size_in_bytes: u32,
    stride_in_bytes: u16,
) -> Result<VertexData<'a>, Report<GeometryError>> {
    if data_offset + size_in_bytes as usize > file_data.len() {
        return Err(Report::new(GeometryError::OutOfBounds { offset: data_offset }));
    }
    let blob = &file_data[data_offset..data_offset + size_in_bytes as usize];

    if blob.len() >= 8 {
        let magic = u32::from_le_bytes(blob[0..4].try_into().unwrap());
        if magic == ENCD_MAGIC {
            let element_count = u32::from_le_bytes(blob[4..8].try_into().unwrap());
            return Ok(VertexData::Encoded { element_count, payload: &blob[8..], stride: stride_in_bytes });
        }
    }

    Ok(VertexData::Raw(blob))
}

fn parse_index_data<'a>(
    file_data: &'a [u8],
    data_offset: usize,
    size_in_bytes: u32,
    index_size: u16,
) -> Result<IndexData<'a>, Report<GeometryError>> {
    if data_offset + size_in_bytes as usize > file_data.len() {
        return Err(Report::new(GeometryError::OutOfBounds { offset: data_offset }));
    }
    let blob = &file_data[data_offset..data_offset + size_in_bytes as usize];

    if blob.len() >= 8 {
        let magic = u32::from_le_bytes(blob[0..4].try_into().unwrap());
        if magic == ENCD_MAGIC {
            let element_count = u32::from_le_bytes(blob[4..8].try_into().unwrap());
            return Ok(IndexData::Encoded { element_count, payload: &blob[8..], index_size });
        }
    }

    Ok(IndexData::Raw(blob))
}

/// Parse the struct fields of a VerticesPrototype (0x20 bytes).
/// Returns (data_relptr, size_in_bytes, stride_in_bytes, is_skinned, is_bumped).
fn parse_vertices_fields(input: &mut &[u8]) -> WResult<(i64, u32, u16, bool, bool)> {
    let data_relptr = le_i64.parse_next(input)?;
    let _packed_string: &[u8] = take(16usize).parse_next(input)?;
    let size_in_bytes = le_u32.parse_next(input)?;
    let stride_in_bytes = le_u16.parse_next(input)?;
    let is_skinned = le_u8.parse_next(input)? != 0;
    let is_bumped = le_u8.parse_next(input)? != 0;
    Ok((data_relptr, size_in_bytes, stride_in_bytes, is_skinned, is_bumped))
}

fn parse_vertices_array<'a>(
    file_data: &'a [u8],
    offset: usize,
    count: usize,
) -> Result<Vec<VerticesPrototype<'a>>, Report<GeometryError>> {
    let mut result = Vec::with_capacity(count);

    for i in 0..count {
        let struct_base = offset + i * 0x20;
        let input = &mut &file_data[struct_base..];

        let (data_relptr, size_in_bytes, stride_in_bytes, is_skinned, is_bumped) = parse_vertices_fields(input)
            .map_err(|e| Report::new(GeometryError::ParseError(format!("vertices[{i}]: {e}"))))?;

        let packed_string_base = struct_base + 0x08;
        let format_name = parse_packed_string(file_data, packed_string_base)
            .map_err(|e| Report::new(GeometryError::ParseError(format!("vertices[{i}] packed string: {e}"))))?;
        let data_offset = resolve_relptr(struct_base, data_relptr);
        let data = parse_vertex_data(file_data, data_offset, size_in_bytes, stride_in_bytes)?;

        result.push(VerticesPrototype { data, format_name, size_in_bytes, stride_in_bytes, is_skinned, is_bumped });
    }

    Ok(result)
}

/// Parse a `.splash` file containing named axis-aligned bounding boxes.
///
/// Format: `u32 count`, then per box: `u32 name_len`, `name` bytes, `6× f32 (min_xyz, max_xyz)`.
pub fn parse_splash_file(data: &[u8]) -> Result<Vec<SplashBox>, Report<GeometryError>> {
    let err = || GeometryError::ParseError("splash: unexpected end of data".into());
    let mut pos = 0usize;

    let read_u32 = |pos: &mut usize| -> Result<u32, Report<GeometryError>> {
        let end = *pos + 4;
        let bytes = data.get(*pos..end).ok_or_else(err)?;
        *pos = end;
        Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
    };

    let read_f32 = |pos: &mut usize| -> Result<f32, Report<GeometryError>> {
        let end = *pos + 4;
        let bytes = data.get(*pos..end).ok_or_else(err)?;
        *pos = end;
        Ok(f32::from_le_bytes(bytes.try_into().unwrap()))
    };

    let count = read_u32(&mut pos)? as usize;
    let mut boxes = Vec::with_capacity(count);

    for _ in 0..count {
        let name_len = read_u32(&mut pos)? as usize;
        let name_end = pos + name_len;
        let name_bytes = data.get(pos..name_end).ok_or_else(err)?;
        let name = std::str::from_utf8(name_bytes)
            .map_err(|_| GeometryError::ParseError("splash: invalid UTF-8 name".into()))?
            .to_string();
        pos = name_end;

        let min_x = read_f32(&mut pos)?;
        let min_y = read_f32(&mut pos)?;
        let min_z = read_f32(&mut pos)?;
        let max_x = read_f32(&mut pos)?;
        let max_y = read_f32(&mut pos)?;
        let max_z = read_f32(&mut pos)?;

        boxes.push(SplashBox { name, min: [min_x, min_y, min_z], max: [max_x, max_y, max_z] });
    }

    Ok(boxes)
}

/// Parse the struct fields of an IndicesPrototype (0x10 bytes).
/// Returns (data_relptr, size_in_bytes, index_size).
fn parse_indices_fields(input: &mut &[u8]) -> WResult<(i64, u32, u16)> {
    let data_relptr = le_i64.parse_next(input)?;
    let size_in_bytes = le_u32.parse_next(input)?;
    let _reserved = le_u16.parse_next(input)?;
    let index_size = le_u16.parse_next(input)?;
    Ok((data_relptr, size_in_bytes, index_size))
}

fn parse_indices_array<'a>(
    file_data: &'a [u8],
    offset: usize,
    count: usize,
) -> Result<Vec<IndicesPrototype<'a>>, Report<GeometryError>> {
    let mut result = Vec::with_capacity(count);

    for i in 0..count {
        let struct_base = offset + i * 0x10;
        let input = &mut &file_data[struct_base..];

        let (data_relptr, size_in_bytes, index_size) = parse_indices_fields(input)
            .map_err(|e| Report::new(GeometryError::ParseError(format!("indices[{i}]: {e}"))))?;

        let data_offset = resolve_relptr(struct_base, data_relptr);
        let data = parse_index_data(file_data, data_offset, size_in_bytes, index_size)?;

        result.push(IndicesPrototype { data, size_in_bytes, index_size });
    }

    Ok(result)
}

/// Parse the struct fields of a ModelPrototype (0x20 bytes: armor or collision).
/// Returns (data_relptr, size_in_bytes).
fn parse_model_fields(input: &mut &[u8]) -> WResult<(i64, u32)> {
    let data_relptr = le_i64.parse_next(input)?;
    let _packed_string: &[u8] = take(16usize).parse_next(input)?;
    let size_in_bytes = le_u32.parse_next(input)?;
    let _padding = le_u32.parse_next(input)?;
    Ok((data_relptr, size_in_bytes))
}

/// Parse the BVH + triangle data from an armor model's raw 16-byte entry stream.
///
/// Format (all entries are 16 bytes):
///   - 2 global header entries (bounding box + BVH node count)
///   - N BVH node groups, each:
///       - 2 entries: node header (flags, bbox min) + bbox max with vertex_count
///       - vertex_count triangle vertices (groups of 3 = triangles)
///   - Each vertex: f32 x, f32 y, f32 z, u8[3] packed_normal, u8 zero
fn parse_armor_data(data: &[u8]) -> Result<Vec<ArmorTriangle>, Report<GeometryError>> {
    const ENTRY_SIZE: usize = 16;

    if data.len() < ENTRY_SIZE * 2 {
        return Ok(Vec::new());
    }

    let entry_count = data.len() / ENTRY_SIZE;
    if entry_count < 2 {
        return Ok(Vec::new());
    }

    let read_f32 = |off: usize| -> f32 { f32::from_le_bytes(data[off..off + 4].try_into().unwrap()) };
    let read_u32 = |off: usize| -> u32 { u32::from_le_bytes(data[off..off + 4].try_into().unwrap()) };

    // Skip 2-entry global header, then walk BVH node groups
    let mut pos = 2; // entry index
    let mut triangles = Vec::new();

    while pos < entry_count {
        // Each BVH node group: 2 header entries + vertex_count vertex entries
        if pos + 1 >= entry_count {
            break;
        }

        // First entry of node header: u32 encoding (layer_index << 16) | material_id
        // byte 0 = collision material ID, byte 2 = 1-based layer index
        // This matches the GameParams key encoding: (model_index << 16) | material_id
        let node_entry0_off = pos * ENTRY_SIZE;
        let material_id = data[node_entry0_off];
        let layer_index = data[node_entry0_off + 2];

        // Second entry of the node header has vertex_count at bytes 12..16
        let node_entry1_off = (pos + 1) * ENTRY_SIZE;
        let vertex_count = read_u32(node_entry1_off + 12) as usize;
        pos += 2; // skip 2 node header entries

        if vertex_count == 0 {
            continue;
        }
        if pos + vertex_count > entry_count {
            return Err(Report::new(GeometryError::ParseError(format!(
                "armor BVH node claims {} vertices but only {} entries remain",
                vertex_count,
                entry_count - pos
            ))));
        }

        // vertex_count should be divisible by 3 (triangle soup)
        let tri_count = vertex_count / 3;
        for t in 0..tri_count {
            let mut tri = ArmorTriangle { vertices: [[0.0; 3]; 3], normals: [[0.0; 3]; 3], material_id, layer_index };
            for v in 0..3 {
                let entry_off = (pos + t * 3 + v) * ENTRY_SIZE;
                tri.vertices[v] = [read_f32(entry_off), read_f32(entry_off + 4), read_f32(entry_off + 8)];
                // Packed normal: 3 bytes at offset 12, each maps [-1, 1]
                let nx = data[entry_off + 12] as f32 / 127.5 - 1.0;
                let ny = data[entry_off + 13] as f32 / 127.5 - 1.0;
                let nz = data[entry_off + 14] as f32 / 127.5 - 1.0;
                tri.normals[v] = [nx, ny, nz];
            }
            triangles.push(tri);
        }

        pos += vertex_count;
    }

    Ok(triangles)
}

/// Parse armor model array. Unlike collision models where data_relptr points to
/// the start of the data, for armor models the actual data extends from right
/// after the struct (struct_base + 0x20) to data_offset + size_in_bytes.
fn parse_armor_model_array(
    file_data: &[u8],
    offset: usize,
    count: usize,
) -> Result<Vec<ArmorModel>, Report<GeometryError>> {
    let mut result = Vec::with_capacity(count);

    for i in 0..count {
        let struct_base = offset + i * 0x20;
        let input = &mut &file_data[struct_base..];

        let (data_relptr, size_in_bytes) = parse_model_fields(input)
            .map_err(|e| Report::new(GeometryError::ParseError(format!("armor[{i}]: {e}"))))?;

        let packed_string_base = struct_base + 0x08;
        let name = parse_packed_string(file_data, packed_string_base)
            .map_err(|e| Report::new(GeometryError::ParseError(format!("armor[{i}] packed string: {e}"))))?;

        // Armor data starts right after the struct and extends to where
        // data_relptr + size_in_bytes points (relptr points near the end)
        let data_start = struct_base + 0x20;
        let data_end = resolve_relptr(struct_base, data_relptr) + size_in_bytes as usize;

        if data_end > file_data.len() {
            return Err(Report::new(GeometryError::OutOfBounds { offset: data_end }));
        }

        let armor_data = &file_data[data_start..data_end];
        let triangles = parse_armor_data(armor_data)?;

        result.push(ArmorModel { name, triangles });
    }

    Ok(result)
}

fn parse_model_array<'a>(
    file_data: &'a [u8],
    offset: usize,
    count: usize,
) -> Result<Vec<ModelPrototype<'a>>, Report<GeometryError>> {
    let mut result = Vec::with_capacity(count);

    for i in 0..count {
        let struct_base = offset + i * 0x20;
        let input = &mut &file_data[struct_base..];

        let (data_relptr, size_in_bytes) = parse_model_fields(input)
            .map_err(|e| Report::new(GeometryError::ParseError(format!("model[{i}]: {e}"))))?;

        let packed_string_base = struct_base + 0x08;
        let name = parse_packed_string(file_data, packed_string_base)
            .map_err(|e| Report::new(GeometryError::ParseError(format!("model[{i}] packed string: {e}"))))?;
        let data_offset = resolve_relptr(struct_base, data_relptr);

        if data_offset + size_in_bytes as usize > file_data.len() {
            return Err(Report::new(GeometryError::OutOfBounds { offset: data_offset }));
        }
        let data = &file_data[data_offset..data_offset + size_in_bytes as usize];

        result.push(ModelPrototype { data, name, size_in_bytes });
    }

    Ok(result)
}
