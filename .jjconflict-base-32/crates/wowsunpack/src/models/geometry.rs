use rootcause::Report;
use thiserror::Error;
use winnow::Parser;
use winnow::binary::le_i64;
use winnow::binary::le_u8;
use winnow::binary::le_u16;
use winnow::binary::le_u32;
use winnow::binary::le_u64;
use winnow::combinator::repeat;
use winnow::token::take;

use crate::data::parser_utils::WResult;
use crate::data::parser_utils::parse_packed_string;
use crate::data::parser_utils::resolve_relptr;

const ENCD_MAGIC: u32 = 0x44434E45;

/// The collision model a building's shells test against. Its faces carry the
/// GameParams `hull.armor` keys in place of an armor model.
pub const HIT_LOCATIONS_COLLISION_MODEL: &str = "hit_locations";

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
    #[error("{len} bytes are too few for a geometry header")]
    TruncatedHeader { len: usize },
    #[error("collision model {model} starts with {found:#010X}, not the convex hull magic")]
    CollisionMagic { model: String, found: u32 },
    #[error("collision model {model} ends at byte {len}, before the field at byte {offset}")]
    CollisionTruncated { model: String, offset: usize, len: usize },
    #[error("collision model {model} has {trailing} bytes after its last hull")]
    CollisionTrailingBytes { model: String, trailing: usize },
    #[error("a polygon of collision model {model} has {vertex_count} vertices, fewer than a triangle")]
    CollisionPolygonTooSmall { model: String, vertex_count: usize },
    #[error("collision model {model} names vertex {index} of a hull with {hull_vertex_count} vertices")]
    CollisionVertexOutOfRange { model: String, index: u32, hull_vertex_count: usize },
    #[error("collision model {model} names edge {index} of a hull with {hull_edge_count} edges")]
    CollisionEdgeOutOfRange { model: String, index: u32, hull_edge_count: usize },
}

#[derive(Debug)]
pub struct MergedGeometry<'a> {
    pub vertices_mapping: Vec<MappingEntry>,
    pub indices_mapping: Vec<MappingEntry>,
    pub merged_vertices: Vec<VerticesPrototype<'a>>,
    pub merged_indices: Vec<IndicesPrototype<'a>>,
    pub collision_models: Vec<CollisionModel<'a>>,
    /// The armor models. A geometry without any, as a building's, takes the
    /// armor of its `hit_locations` collision model instead.
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

/// A collision model: named convex hulls, decoded on demand by [`Self::hulls`].
#[derive(Debug)]
pub struct CollisionModel<'a> {
    pub name: String,
    pub data: &'a [u8],
}

impl CollisionModel<'_> {
    /// Decodes the hulls, checking every index against the hull it names.
    ///
    /// Layout: `u32 magic`, `u32 hull_count`; per hull `u32 vertex_count`,
    /// `f32[3]` per vertex, `u32 edge_count`, `u32[2]` per edge,
    /// `u32 polygon_count`; per polygon `u32 n`, `n` vertex indices, `n` edge
    /// indices and a `u32` [`CollisionMaterialKey`].
    pub fn hulls(&self) -> Result<Vec<CollisionHull>, Report<GeometryError>> {
        const MAGIC: u32 = 0x2402_2012;

        let mut reader = CollisionReader { model: &self.name, data: self.data, offset: 0 };
        let magic = reader.u32()?;
        if magic != MAGIC {
            return Err(Report::new(GeometryError::CollisionMagic { model: self.name.clone(), found: magic }));
        }

        // A hull holds at least its vertex, edge and polygon counts.
        let hull_count = reader.count(12)?;
        let mut hulls = Vec::with_capacity(hull_count);
        for _ in 0..hull_count {
            hulls.push(reader.hull()?);
        }

        let trailing = self.data.len() - reader.offset;
        if trailing != 0 {
            return Err(Report::new(GeometryError::CollisionTrailingBytes { model: self.name.clone(), trailing }));
        }

        Ok(hulls)
    }

    /// The hulls as armor: each polygon fan-triangulated, its normal pointing
    /// out of the hull, and wound as armor model triangles are.
    pub fn armor_model(&self) -> Result<ArmorModel, Report<GeometryError>> {
        let mut triangles = Vec::new();
        for hull in self.hulls()? {
            let centroid = mean(hull.vertices.iter().copied());
            for polygon in &hull.polygons {
                // `hulls` checked every index and that each polygon is at
                // least a triangle.
                let corners: Vec<[f32; 3]> = polygon.vertices.iter().map(|index| hull.vertices[index.get()]).collect();

                let normal = outward_normal(&corners, centroid);

                // Armor model triangles wind opposite their normal, whichever
                // way the polygon lists its corners.
                for pair in corners[1..].windows(2) {
                    let winds_with_normal =
                        dot(cross(sub(pair[0], corners[0]), sub(pair[1], corners[0])), normal) > 0.0;
                    let vertices =
                        if winds_with_normal { [corners[0], pair[1], pair[0]] } else { [corners[0], pair[0], pair[1]] };
                    triangles.push(ArmorTriangle {
                        vertices,
                        normals: [normal; 3],
                        material_id: polygon.material.material_id(),
                        layer_index: polygon.material.layer_index(),
                    });
                }
            }
        }

        Ok(ArmorModel { name: self.name.clone(), triangles })
    }
}

/// One convex hull of a collision model.
#[derive(Debug, Clone, PartialEq)]
pub struct CollisionHull {
    pub vertices: Vec<[f32; 3]>,
    pub edges: Vec<[HullVertexIndex; 2]>,
    pub polygons: Vec<CollisionPolygon>,
}

/// A convex face of a [`CollisionHull`], at least a triangle.
#[derive(Debug, Clone, PartialEq)]
pub struct CollisionPolygon {
    /// Corners in face order, counter-clockwise seen from outside.
    pub vertices: Vec<HullVertexIndex>,
    /// One edge per side.
    pub edges: Vec<HullEdgeIndex>,
    pub material: CollisionMaterialKey,
}

/// An index into a [`CollisionHull`]'s vertices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HullVertexIndex(u32);

impl HullVertexIndex {
    pub fn new(index: u32) -> Self {
        Self(index)
    }

    pub fn get(self) -> usize {
        self.0 as usize
    }
}

/// An index into a [`CollisionHull`]'s edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HullEdgeIndex(u32);

impl HullEdgeIndex {
    pub fn new(index: u32) -> Self {
        Self(index)
    }

    pub fn get(self) -> usize {
        self.0 as usize
    }
}

/// `(layer_index << 16) | material_id`: the encoding of an armor BVH node
/// header and of a GameParams armor map key. Zero in plain collision models.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CollisionMaterialKey(u32);

impl CollisionMaterialKey {
    pub fn new(raw: u32) -> Self {
        Self(raw)
    }

    pub fn raw(self) -> u32 {
        self.0
    }

    /// Byte 0, an index into the collision material name table.
    pub fn material_id(self) -> u8 {
        self.0.to_le_bytes()[0]
    }

    /// Byte 2, the armor layer.
    pub fn layer_index(self) -> u8 {
        self.0.to_le_bytes()[2]
    }
}

/// Reads the little-endian fields of a collision model blob.
struct CollisionReader<'a> {
    model: &'a str,
    data: &'a [u8],
    offset: usize,
}

impl CollisionReader<'_> {
    fn bytes<const N: usize>(&mut self) -> Result<[u8; N], Report<GeometryError>> {
        let truncated = || {
            Report::new(GeometryError::CollisionTruncated {
                model: self.model.to_string(),
                offset: self.offset,
                len: self.data.len(),
            })
        };
        let end = self.offset.checked_add(N).ok_or_else(truncated)?;
        let bytes: [u8; N] =
            self.data.get(self.offset..end).ok_or_else(truncated)?.try_into().map_err(|_| truncated())?;
        self.offset = end;

        Ok(bytes)
    }

    fn u32(&mut self) -> Result<u32, Report<GeometryError>> {
        self.bytes::<4>().map(u32::from_le_bytes)
    }

    fn f32(&mut self) -> Result<f32, Report<GeometryError>> {
        self.bytes::<4>().map(f32::from_le_bytes)
    }

    /// A count of items at least `item_len` bytes each, checked against the
    /// bytes left so a corrupt count cannot reserve unbounded memory.
    fn count(&mut self, item_len: usize) -> Result<usize, Report<GeometryError>> {
        let count_offset = self.offset;
        let count = self.u32()? as usize;
        let remaining = self.data.len() - self.offset;
        if count.saturating_mul(item_len) > remaining {
            return Err(Report::new(GeometryError::CollisionTruncated {
                model: self.model.to_string(),
                offset: count_offset,
                len: self.data.len(),
            }));
        }

        Ok(count)
    }

    fn hull(&mut self) -> Result<CollisionHull, Report<GeometryError>> {
        let vertex_count = self.count(12)?;
        let mut vertices = Vec::with_capacity(vertex_count);
        for _ in 0..vertex_count {
            vertices.push([self.f32()?, self.f32()?, self.f32()?]);
        }

        let edge_count = self.count(8)?;
        let mut edges = Vec::with_capacity(edge_count);
        for _ in 0..edge_count {
            edges.push([self.vertex_index(vertex_count)?, self.vertex_index(vertex_count)?]);
        }

        // A polygon holds at least its side count, three vertex and edge
        // indices and its material.
        let polygon_count = self.count(32)?;
        let mut polygons = Vec::with_capacity(polygon_count);
        for _ in 0..polygon_count {
            let side_count = self.count(8)?;
            if side_count < 3 {
                return Err(Report::new(GeometryError::CollisionPolygonTooSmall {
                    model: self.model.to_string(),
                    vertex_count: side_count,
                }));
            }
            let mut polygon_vertices = Vec::with_capacity(side_count);
            for _ in 0..side_count {
                polygon_vertices.push(self.vertex_index(vertex_count)?);
            }
            let mut polygon_edges = Vec::with_capacity(side_count);
            for _ in 0..side_count {
                polygon_edges.push(self.edge_index(edge_count)?);
            }
            let material = CollisionMaterialKey::new(self.u32()?);
            polygons.push(CollisionPolygon { vertices: polygon_vertices, edges: polygon_edges, material });
        }

        Ok(CollisionHull { vertices, edges, polygons })
    }

    fn vertex_index(&mut self, hull_vertex_count: usize) -> Result<HullVertexIndex, Report<GeometryError>> {
        let index = self.u32()?;
        if index as usize >= hull_vertex_count {
            return Err(Report::new(GeometryError::CollisionVertexOutOfRange {
                model: self.model.to_string(),
                index,
                hull_vertex_count,
            }));
        }

        Ok(HullVertexIndex::new(index))
    }

    fn edge_index(&mut self, hull_edge_count: usize) -> Result<HullEdgeIndex, Report<GeometryError>> {
        let index = self.u32()?;
        if index as usize >= hull_edge_count {
            return Err(Report::new(GeometryError::CollisionEdgeOutOfRange {
                model: self.model.to_string(),
                index,
                hull_edge_count,
            }));
        }

        Ok(HullEdgeIndex::new(index))
    }
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

/// The number of armor models [`parse_geometry`] yields for a `.geometry`
/// file, without decoding meshes or armor BVHs.
pub fn armor_model_count(file_data: &[u8]) -> Result<u64, Report<GeometryError>> {
    let hdr = parse_header(&mut &file_data[..])
        .map_err(|_| Report::new(GeometryError::TruncatedHeader { len: file_data.len() }))?;
    if hdr.armor_model_count > 0 {
        return Ok(u64::from(hdr.armor_model_count));
    }

    let collision_models = parse_collision_models(file_data, &hdr)?;

    Ok(u64::from(hit_location_armor(&collision_models)?.is_some()))
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

    let collision_models = parse_collision_models(file_data, &hdr)?;

    let mut armor_models = if hdr.armor_model_count > 0 {
        let am_offset = resolve_relptr(header_base, hdr.armor_models_ptr);
        parse_armor_model_array(file_data, am_offset, hdr.armor_model_count as usize)?
    } else {
        Vec::new()
    };
    // The client tests hits against a model's armor; it builds only
    // buildings, which have none, from their collision model.
    if armor_models.is_empty() {
        armor_models.extend(hit_location_armor(&collision_models)?);
    }

    Ok(MergedGeometry {
        vertices_mapping,
        indices_mapping,
        merged_vertices,
        merged_indices,
        collision_models,
        armor_models,
    })
}

fn parse_collision_models<'a>(
    file_data: &'a [u8],
    hdr: &HeaderFields,
) -> Result<Vec<CollisionModel<'a>>, Report<GeometryError>> {
    if hdr.collision_model_count == 0 {
        return Ok(Vec::new());
    }

    let offset = resolve_relptr(0, hdr.collision_models_ptr);

    parse_collision_model_array(file_data, offset, hdr.collision_model_count as usize)
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

/// Parse the fields of an armor or collision model prototype (0x20 bytes).
/// Returns (size_in_bytes, data_relptr); the name sits between them.
fn parse_blob_prototype_fields(input: &mut &[u8]) -> WResult<(u64, i64)> {
    let size_in_bytes = le_u64.parse_next(input)?;
    let _packed_string: &[u8] = take(16usize).parse_next(input)?;
    let data_relptr = le_i64.parse_next(input)?;
    Ok((size_in_bytes, data_relptr))
}

/// The name and data of the armor or collision model prototype at
/// `struct_base`, the `index`th of its array.
fn parse_blob_prototype(
    file_data: &[u8],
    struct_base: usize,
    index: usize,
) -> Result<(String, &[u8]), Report<GeometryError>> {
    let input = &mut file_data.get(struct_base..).ok_or(GeometryError::OutOfBounds { offset: struct_base })?;
    let (size_in_bytes, data_relptr) = parse_blob_prototype_fields(input)
        .map_err(|e| Report::new(GeometryError::ParseError(format!("model prototype[{index}]: {e}"))))?;

    let name = parse_packed_string(file_data, struct_base + 0x08)
        .map_err(|e| Report::new(GeometryError::ParseError(format!("model prototype[{index}] name: {e}"))))?;

    let data_offset = resolve_relptr(struct_base, data_relptr);
    let data = usize::try_from(size_in_bytes)
        .ok()
        .and_then(|size| data_offset.checked_add(size))
        .and_then(|data_end| file_data.get(data_offset..data_end))
        .ok_or(GeometryError::OutOfBounds { offset: data_offset })?;

    Ok((name, data))
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

fn parse_armor_model_array(
    file_data: &[u8],
    offset: usize,
    count: usize,
) -> Result<Vec<ArmorModel>, Report<GeometryError>> {
    prototype_array_fits(file_data, offset, count)?;
    let mut result = Vec::with_capacity(count);

    for i in 0..count {
        let (name, armor_data) = parse_blob_prototype(file_data, offset + i * 0x20, i)?;
        let triangles = parse_armor_data(armor_data)?;

        result.push(ArmorModel { name, triangles });
    }

    Ok(result)
}

fn parse_collision_model_array(
    file_data: &[u8],
    offset: usize,
    count: usize,
) -> Result<Vec<CollisionModel<'_>>, Report<GeometryError>> {
    prototype_array_fits(file_data, offset, count)?;
    let mut result = Vec::with_capacity(count);

    for i in 0..count {
        let (name, data) = parse_blob_prototype(file_data, offset + i * 0x20, i)?;

        result.push(CollisionModel { name, data });
    }

    Ok(result)
}

/// Checks that `count` prototypes of 0x20 bytes from `offset` lie in the file,
/// so a corrupt count cannot reserve unbounded memory.
fn prototype_array_fits(file_data: &[u8], offset: usize, count: usize) -> Result<(), Report<GeometryError>> {
    let end = count.checked_mul(0x20).and_then(|len| offset.checked_add(len));
    if end.is_none_or(|end| end > file_data.len()) {
        return Err(Report::new(GeometryError::OutOfBounds { offset }));
    }

    Ok(())
}

/// The armor of the `hit_locations` collision model, when there is one with
/// a face.
fn hit_location_armor(collision_models: &[CollisionModel<'_>]) -> Result<Option<ArmorModel>, Report<GeometryError>> {
    let Some(hit_locations) = collision_models.iter().find(|model| model.name == HIT_LOCATIONS_COLLISION_MODEL) else {
        return Ok(None);
    };

    let armor = hit_locations.armor_model()?;

    Ok((!armor.triangles.is_empty()).then_some(armor))
}

/// The mean of `points`, the origin when there are none.
fn mean(points: impl Iterator<Item = [f32; 3]>) -> [f32; 3] {
    let mut sum = [0.0f32; 3];
    let mut count = 0usize;
    for point in points {
        for axis in 0..3 {
            sum[axis] += point[axis];
        }
        count += 1;
    }

    if count == 0 {
        return sum;
    }

    sum.map(|total| total / count as f32)
}

/// The unit normal of the planar polygon `corners` (Newell's method), turned
/// away from `hull_centroid`. Zero for a degenerate polygon, which has none.
fn outward_normal(corners: &[[f32; 3]], hull_centroid: [f32; 3]) -> [f32; 3] {
    let mut normal = [0.0f32; 3];
    for (index, current) in corners.iter().enumerate() {
        let next = corners[(index + 1) % corners.len()];
        normal[0] += (current[1] - next[1]) * (current[2] + next[2]);
        normal[1] += (current[2] - next[2]) * (current[0] + next[0]);
        normal[2] += (current[0] - next[0]) * (current[1] + next[1]);
    }

    let length = normal.iter().map(|component| component * component).sum::<f32>().sqrt();
    if length == 0.0 {
        return normal;
    }
    let normal = normal.map(|component| component / length);

    let center = mean(corners.iter().copied());
    let outward = dot(normal, sub(center, hull_centroid));

    if outward < 0.0 { normal.map(|component| -component) } else { normal }
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

#[cfg(test)]
mod tests {
    use super::*;

    const BELT_1: u32 = (1 << 16) | 77;
    const BELT_2: u32 = (2 << 16) | 77;
    const DECK_1: u32 = (1 << 16) | 78;

    fn push_u32(out: &mut Vec<u8>, value: u32) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    /// A square pyramid over the unit square at y = 0 with its apex at
    /// y = 1: a quad base on the deck key, four triangle sides on the belt.
    /// The game lists corners counter-clockwise seen from outside;
    /// `reversed` lists them the other way.
    fn pyramid_blob(reversed: bool) -> Vec<u8> {
        let vertices = [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 1.0], [0.0, 0.0, 1.0], [0.5, 1.0, 0.5]];
        let edges = [[0u32, 1], [1, 2], [2, 3], [3, 0], [0, 4], [1, 4], [2, 4], [3, 4]];
        let polygons: [(&[u32], &[u32], u32); 5] = [
            (&[0, 1, 2, 3], &[0, 1, 2, 3], DECK_1),
            (&[0, 4, 1], &[4, 5, 0], BELT_1),
            (&[1, 4, 2], &[5, 6, 1], BELT_1),
            (&[2, 4, 3], &[6, 7, 2], BELT_2),
            (&[3, 4, 0], &[7, 4, 3], BELT_1),
        ];

        let mut blob = Vec::new();
        push_u32(&mut blob, 0x2402_2012);
        push_u32(&mut blob, 1);
        push_u32(&mut blob, vertices.len() as u32);
        for vertex in vertices {
            for component in vertex {
                blob.extend_from_slice(&component.to_le_bytes());
            }
        }
        push_u32(&mut blob, edges.len() as u32);
        for edge in edges {
            push_u32(&mut blob, edge[0]);
            push_u32(&mut blob, edge[1]);
        }
        push_u32(&mut blob, polygons.len() as u32);
        for (polygon_vertices, polygon_edges, key) in polygons {
            let mut polygon_vertices = polygon_vertices.to_vec();
            let mut polygon_edges = polygon_edges.to_vec();
            if reversed {
                polygon_vertices.reverse();
                polygon_edges.reverse();
            }
            push_u32(&mut blob, polygon_vertices.len() as u32);
            for index in polygon_vertices.into_iter().chain(polygon_edges) {
                push_u32(&mut blob, index);
            }
            push_u32(&mut blob, key);
        }

        blob
    }

    /// A geometry file holding only collision and armor models, laid out as
    /// the game writes them: header, prototypes, then each blob followed by
    /// its name.
    fn geometry_with(collision_models: &[(&str, &[u8])], armor_models: &[(&str, &[u8])]) -> Vec<u8> {
        const HEADER_LEN: usize = 0x48;
        const PROTOTYPE_LEN: usize = 0x20;

        let armor_array = HEADER_LEN + collision_models.len() * PROTOTYPE_LEN;
        let mut file = vec![0u8; armor_array + armor_models.len() * PROTOTYPE_LEN];
        file[0x10..0x14].copy_from_slice(&(collision_models.len() as u32).to_le_bytes());
        file[0x14..0x18].copy_from_slice(&(armor_models.len() as u32).to_le_bytes());
        file[0x38..0x40].copy_from_slice(&(HEADER_LEN as i64).to_le_bytes());
        file[0x40..0x48].copy_from_slice(&(armor_array as i64).to_le_bytes());

        let prototypes = collision_models.iter().chain(armor_models);
        for (index, (name, blob)) in prototypes.enumerate() {
            let struct_base = HEADER_LEN + index * PROTOTYPE_LEN;
            let data_offset = file.len();
            file.extend_from_slice(blob);
            let name_offset = file.len();
            file.extend_from_slice(name.as_bytes());
            file.push(0);

            let prototype = &mut file[struct_base..struct_base + PROTOTYPE_LEN];
            prototype[0x00..0x08].copy_from_slice(&(blob.len() as u64).to_le_bytes());
            prototype[0x08..0x0C].copy_from_slice(&(name.len() as u32 + 1).to_le_bytes());
            prototype[0x10..0x18].copy_from_slice(&((name_offset - (struct_base + 0x08)) as i64).to_le_bytes());
            prototype[0x18..0x20].copy_from_slice(&((data_offset - struct_base) as i64).to_le_bytes());
        }

        file
    }

    fn collision<'a>(name: &str, data: &'a [u8]) -> CollisionModel<'a> {
        CollisionModel { name: name.to_string(), data }
    }

    #[test]
    fn collision_hulls_decode_vertices_edges_and_keyed_polygons() {
        let blob = pyramid_blob(false);
        let hulls = collision("hit_locations", &blob).hulls().unwrap();

        assert_eq!(hulls.len(), 1);
        let hull = &hulls[0];
        assert_eq!(hull.vertices[4], [0.5, 1.0, 0.5]);
        assert_eq!(hull.edges.len(), 8);
        assert_eq!(hull.polygons[0].vertices, [0, 1, 2, 3].map(HullVertexIndex::new));
        assert_eq!(hull.polygons[0].edges, [0, 1, 2, 3].map(HullEdgeIndex::new));
        assert_eq!(hull.edges[4], [HullVertexIndex::new(0), HullVertexIndex::new(4)]);
        let keys: Vec<(u8, u8)> = hull
            .polygons
            .iter()
            .map(|polygon| (polygon.material.material_id(), polygon.material.layer_index()))
            .collect();
        assert_eq!(keys, [(78, 1), (77, 1), (77, 1), (77, 2), (77, 1)]);
    }

    #[test]
    fn collision_armor_fans_polygons_with_outward_normals_and_armor_winding() {
        for reversed in [false, true] {
            let blob = pyramid_blob(reversed);
            let armor = collision("hit_locations", &blob).armor_model().unwrap();

            assert_eq!(armor.triangles.len(), 6, "a quad base fans into two triangles");
            let base: Vec<&ArmorTriangle> =
                armor.triangles.iter().filter(|triangle| triangle.material_id == 78).collect();
            assert_eq!(base.len(), 2);
            assert!(base.iter().all(|triangle| triangle.normals == [[0.0, -1.0, 0.0]; 3]), "the base faces down");
            assert_eq!(armor.triangles.iter().filter(|triangle| triangle.layer_index == 2).count(), 1);

            let centroid = [0.5, 0.2, 0.5];
            for triangle in &armor.triangles {
                let [a, b, c] = triangle.vertices;
                let normal = triangle.normals[0];
                let center = mean(triangle.vertices.into_iter());
                assert!(dot(normal, sub(center, centroid)) > 0.0, "{triangle:?} faces out");
                assert!(dot(cross(sub(b, a), sub(c, a)), normal) < 0.0, "{triangle:?} winds against its normal");
            }
        }
    }

    #[test]
    fn collision_models_follow_the_size_name_data_layout() {
        let blob = pyramid_blob(false);
        let plain = [0x12, 0x20, 0x02, 0x24, 0, 0, 0, 0];
        let file = geometry_with(&[("default", &plain), ("hit_locations", &blob)], &[]);

        let geometry = parse_geometry(&file).unwrap();

        let models: Vec<(&str, &[u8])> =
            geometry.collision_models.iter().map(|model| (model.name.as_str(), model.data)).collect();
        assert_eq!(models, [("default", &plain[..]), ("hit_locations", &blob[..])]);
        assert_eq!(geometry.armor_models.len(), 1, "the hit_locations model is the armor");
        assert_eq!(geometry.armor_models[0].name, HIT_LOCATIONS_COLLISION_MODEL);
        assert_eq!(geometry.armor_models[0].triangles.len(), 6);
        assert_eq!(armor_model_count(&file).unwrap(), 1);
    }

    #[test]
    fn plain_collision_models_add_no_armor() {
        let file = geometry_with(&[("default", &pyramid_blob(false))], &[]);

        assert!(parse_geometry(&file).unwrap().armor_models.is_empty());
        assert_eq!(armor_model_count(&file).unwrap(), 0);
    }

    #[test]
    fn a_model_with_its_own_armor_keeps_it() {
        let blob = pyramid_blob(false);
        let armor = [0u8; 32];
        let file = geometry_with(&[("hit_locations", &blob)], &[("CM_PA_united.armor", &armor)]);

        let geometry = parse_geometry(&file).unwrap();

        let names: Vec<&str> = geometry.armor_models.iter().map(|model| model.name.as_str()).collect();
        assert_eq!(names, ["CM_PA_united.armor"]);
        assert_eq!(armor_model_count(&file).unwrap(), 1);
    }

    #[test]
    fn a_prototype_count_past_the_end_of_the_file_is_rejected() {
        let mut file = geometry_with(&[("default", &pyramid_blob(false))], &[]);
        file[0x10..0x14].copy_from_slice(&u32::MAX.to_le_bytes());

        let error = parse_geometry(&file).unwrap_err();
        assert!(matches!(error.current_context(), GeometryError::OutOfBounds { offset: 0x48 }));
        assert!(armor_model_count(&file).is_err());
    }

    #[test]
    fn a_hull_count_past_the_minimum_hull_size_is_rejected() {
        // Four hulls fit in 16 bytes at 4 bytes each, not at the 12 bytes
        // each hull needs.
        let mut blob = Vec::new();
        blob.extend_from_slice(&0x2402_2012u32.to_le_bytes());
        blob.extend_from_slice(&4u32.to_le_bytes());
        blob.extend_from_slice(&[0; 16]);

        let error = collision("hit_locations", &blob).hulls().unwrap_err();
        assert!(matches!(error.current_context(), GeometryError::CollisionTruncated { offset: 4, .. }));
    }

    #[test]
    fn malformed_collision_blobs_report_where_they_fail() {
        let blob = pyramid_blob(false);

        let mut wrong_magic = blob.clone();
        wrong_magic[0] = 0;
        let error = collision("hit_locations", &wrong_magic).hulls().unwrap_err();
        assert!(matches!(error.current_context(), GeometryError::CollisionMagic { found: 0x2402_2000, .. }));

        let error = collision("hit_locations", &blob[..blob.len() - 2]).hulls().unwrap_err();
        assert!(matches!(error.current_context(), GeometryError::CollisionTruncated { .. }));

        let mut trailing = blob.clone();
        trailing.push(0);
        let error = collision("hit_locations", &trailing).hulls().unwrap_err();
        assert!(matches!(error.current_context(), GeometryError::CollisionTrailingBytes { trailing: 1, .. }));

        // The blob's first edge sits after the magic, the counts and the five
        // vertices; the first polygon after the edges and the polygon count.
        let first_edge = 4 * 3 + 5 * 12 + 4;
        let first_polygon = first_edge + 8 * 8 + 4;
        let patched = |offset: usize, value: u32| {
            let mut patched = blob.clone();
            patched[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            collision("hit_locations", &patched).hulls().unwrap_err()
        };

        let error = patched(first_edge, 7);
        assert!(matches!(
            error.current_context(),
            GeometryError::CollisionVertexOutOfRange { index: 7, hull_vertex_count: 5, .. }
        ));

        let error = patched(first_polygon, 2);
        assert!(matches!(error.current_context(), GeometryError::CollisionPolygonTooSmall { vertex_count: 2, .. }));

        let error = patched(first_polygon + 4, 9);
        assert!(matches!(
            error.current_context(),
            GeometryError::CollisionVertexOutOfRange { index: 9, hull_vertex_count: 5, .. }
        ));

        let error = patched(first_polygon + 4 + 4 * 4, 8);
        assert!(matches!(
            error.current_context(),
            GeometryError::CollisionEdgeOutOfRange { index: 8, hull_edge_count: 8, .. }
        ));
    }
}
