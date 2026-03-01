//! Parser for MaterialPrototype records (`.mfm` files / assets.bin blob index 0).
//!
//! # Record Layout (0x78 bytes)
//!
//! ```text
//! +0x00: u16  property_count
//! +0x02: u16  flags (e.g., layer count: 1=normal, 2=blend)
//! +0x04: u32  shader_id
//! +0x08: u64  reserved
//! +0x10: u64  names_ptr       → u32[count]   MurmurHash3_32 property name hashes
//! +0x18: u64  type_idx_ptr    → u16[count]   (low 4 bits = type, upper 12 bits = index)
//! +0x20: u64  bool_ptr        → u8[]         (type 0)
//! +0x28: u64  int32_ptr       → i32[]        (type 1)
//! +0x30: u64  float_a_ptr     → f32[]        (type 2)
//! +0x38: u64  float_b_ptr     → f32[]        (type 3)
//! +0x40: u64  texture_ptr     → u64[]        (type 4, texture path hashes)
//! +0x48: u64  vec2_ptr        → [f32; 2][]   (type 5)
//! +0x50: u64  vec3_ptr        → [f32; 3][]   (type 6)
//! +0x58: u64  vec4_ptr        → [f32; 4][]   (type 7)
//! +0x60: u64  mat4_ptr        → [f32; 16][]  (type 8)
//! +0x68: u64  material_hash   (identifies the material shader/template)
//! +0x70: u64  padding (zero)
//! ```
//!
//! **Pointer convention**: All pointer fields are unsigned u64 offsets relative to
//! the start of the record itself (`record_data[0]`). This differs from the signed
//! i64 relptrs used by VisualPrototype/ModelPrototype, but the effect is the same
//! since `get_prototype_data()` already returns a slice starting at the record.

use std::collections::HashMap;

use rootcause::Report;
use thiserror::Error;
use winnow::Parser;
use winnow::binary::le_u16;
use winnow::binary::le_u32;
use winnow::binary::le_u64;
use winnow::error::ContextError;
use winnow::error::ErrMode;

use crate::data::parser_utils::WResult;

/// Item size for MaterialPrototype records in the database blob.
pub const MATERIAL_ITEM_SIZE: usize = 0x78;

/// Blob index for MaterialPrototype in the assets.bin database array.
pub const MATERIAL_BLOB_INDEX: usize = 0;

// ─── Errors ─────────────────────────────────────────────────────────────────

/// Errors that can occur during MaterialPrototype parsing.
#[derive(Debug, Error)]
pub enum MaterialError {
    #[error("data too short: need {need} bytes at offset 0x{offset:X}, have {have}")]
    DataTooShort { offset: usize, need: usize, have: usize },
    #[error("parse error: {0}")]
    ParseError(String),
    #[error("invalid property type {typ} at property index {index}")]
    InvalidPropertyType { index: usize, typ: u8 },
}

// ─── Property types ─────────────────────────────────────────────────────────

/// The 9 property value types supported by MaterialPrototype.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[repr(u8)]
pub enum PropertyType {
    Bool = 0,
    Int32 = 1,
    FloatA = 2,
    FloatB = 3,
    Texture = 4,
    Vec2 = 5,
    Vec3 = 6,
    Vec4 = 7,
    Matrix4x4 = 8,
}

impl PropertyType {
    fn from_raw(raw: u8) -> Option<Self> {
        match raw {
            0 => Some(Self::Bool),
            1 => Some(Self::Int32),
            2 => Some(Self::FloatA),
            3 => Some(Self::FloatB),
            4 => Some(Self::Texture),
            5 => Some(Self::Vec2),
            6 => Some(Self::Vec3),
            7 => Some(Self::Vec4),
            8 => Some(Self::Matrix4x4),
            _ => None,
        }
    }

    /// Display name for this property type.
    pub fn name(self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::Int32 => "int32",
            Self::FloatA => "floatA",
            Self::FloatB => "floatB",
            Self::Texture => "texture",
            Self::Vec2 => "vec2",
            Self::Vec3 => "vec3",
            Self::Vec4 => "vec4",
            Self::Matrix4x4 => "mat4x4",
        }
    }
}

/// A decoded property value.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum PropertyValue {
    Bool(bool),
    Int32(i32),
    Float(f32),
    Texture(u64),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    Vec4([f32; 4]),
    Matrix4x4([f32; 16]),
}

/// A single material property with its name hash, type, and decoded value.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct MaterialProperty {
    /// Resolved property name (if known from the built-in dictionary).
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub name: Option<&'static str>,
    /// MurmurHash3_32(seed=0) of the property name.
    pub name_hash: u32,
    /// The property type.
    pub property_type: PropertyType,
    /// Index into the typed array.
    pub array_index: u16,
    /// Decoded value (None if the typed pointer was null).
    pub value: Option<PropertyValue>,
}

// ─── Parsed MaterialPrototype ───────────────────────────────────────────────

/// A parsed MaterialPrototype record.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct MaterialPrototype {
    /// Number of properties.
    pub property_count: u16,
    /// Flags field (often 0 or 1; possibly layer count).
    pub flags: u16,
    /// Shader identifier.
    pub shader_id: u32,
    /// Material template hash (identifies which shader template this material uses).
    pub material_hash: u64,
    /// Decoded properties.
    pub properties: Vec<MaterialProperty>,
}

// ─── Header parsing ─────────────────────────────────────────────────────────

struct MaterialHeader {
    property_count: u16,
    flags: u16,
    shader_id: u32,
    _reserved: u64,
    names_ptr: u64,
    type_idx_ptr: u64,
    type_ptrs: [u64; 9],
    material_hash: u64,
}

fn parse_material_header(input: &mut &[u8]) -> WResult<MaterialHeader> {
    let property_count = le_u16.parse_next(input)?;
    let flags = le_u16.parse_next(input)?;
    let shader_id = le_u32.parse_next(input)?;
    let reserved = le_u64.parse_next(input)?;
    let names_ptr = le_u64.parse_next(input)?;
    let type_idx_ptr = le_u64.parse_next(input)?;

    let mut type_ptrs = [0u64; 9];
    for slot in &mut type_ptrs {
        *slot = le_u64.parse_next(input)?;
    }

    let material_hash = le_u64.parse_next(input)?;
    let _padding = le_u64.parse_next(input)?;

    Ok(MaterialHeader {
        property_count,
        flags,
        shader_id,
        _reserved: reserved,
        names_ptr,
        type_idx_ptr,
        type_ptrs,
        material_hash,
    })
}

// ─── Value reading helpers ──────────────────────────────────────────────────

fn read_bool(data: &[u8], offset: usize) -> Option<bool> {
    data.get(offset).map(|&b| b != 0)
}

fn read_i32_at(data: &[u8], offset: usize) -> Option<i32> {
    let bytes = data.get(offset..offset + 4)?;
    Some(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_f32_at(data: &[u8], offset: usize) -> Option<f32> {
    let bytes = data.get(offset..offset + 4)?;
    Some(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_u64_at(data: &[u8], offset: usize) -> Option<u64> {
    let bytes = data.get(offset..offset + 8)?;
    Some(u64::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7]]))
}

fn read_f32_array<const N: usize>(data: &[u8], offset: usize) -> Option<[f32; N]> {
    let end = offset + N * 4;
    if end > data.len() {
        return None;
    }
    let mut arr = [0f32; N];
    for (i, val) in arr.iter_mut().enumerate() {
        let o = offset + i * 4;
        *val = f32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]);
    }
    Some(arr)
}

/// Size in bytes of one element for each property type.
const TYPE_ELEMENT_SIZES: [usize; 9] = [1, 4, 4, 4, 8, 8, 12, 16, 64];

// ─── Top-level parse entry point ────────────────────────────────────────────

/// Parse a MaterialPrototype from blob data.
///
/// `record_data` is a slice starting at the record's offset within the blob,
/// extending to the end of the blob (so pointers can resolve into OOL data).
/// The first `MATERIAL_ITEM_SIZE` bytes are the fixed record fields.
pub fn parse_material(record_data: &[u8]) -> Result<MaterialPrototype, Report<MaterialError>> {
    if record_data.len() < MATERIAL_ITEM_SIZE {
        return Err(Report::new(MaterialError::DataTooShort {
            offset: 0,
            need: MATERIAL_ITEM_SIZE,
            have: record_data.len(),
        }));
    }

    let hdr = {
        let input = &mut &record_data[..];
        parse_material_header(input)
            .map_err(|e: ErrMode<ContextError>| Report::new(MaterialError::ParseError(format!("header: {e}"))))?
    };

    let count = hdr.property_count as usize;
    let mut properties = Vec::with_capacity(count);
    let name_table = build_property_name_table();

    if count > 0 {
        // Pointers are unsigned offsets from record start (= start of record_data).
        let names_off = hdr.names_ptr as usize;
        let tidx_off = hdr.type_idx_ptr as usize;

        // Bounds check the name and type_idx arrays.
        let names_end = names_off + count * 4;
        let tidx_end = tidx_off + count * 2;
        if names_end > record_data.len() || tidx_end > record_data.len() {
            return Err(Report::new(MaterialError::DataTooShort {
                offset: names_off,
                need: names_end.max(tidx_end),
                have: record_data.len(),
            }));
        }

        for i in 0..count {
            let name_hash =
                le_u32.parse_next(&mut &record_data[names_off + i * 4..]).map_err(|e: ErrMode<ContextError>| {
                    Report::new(MaterialError::ParseError(format!("name_hash[{i}]: {e}")))
                })?;

            let type_and_idx =
                le_u16.parse_next(&mut &record_data[tidx_off + i * 2..]).map_err(|e: ErrMode<ContextError>| {
                    Report::new(MaterialError::ParseError(format!("type_idx[{i}]: {e}")))
                })?;

            let raw_type = (type_and_idx & 0xF) as u8;
            let array_index = type_and_idx >> 4;

            let property_type = PropertyType::from_raw(raw_type)
                .ok_or_else(|| Report::new(MaterialError::InvalidPropertyType { index: i, typ: raw_type }))?;

            let value = {
                let ptr = hdr.type_ptrs[raw_type as usize];
                if ptr == 0 {
                    None
                } else {
                    let base = ptr as usize;
                    let elem_size = TYPE_ELEMENT_SIZES[raw_type as usize];
                    let elem_off = base + array_index as usize * elem_size;
                    read_property_value(record_data, elem_off, property_type)
                }
            };

            let name = name_table.get(&name_hash).copied();
            properties.push(MaterialProperty { name, name_hash, property_type, array_index, value });
        }
    }

    Ok(MaterialPrototype {
        property_count: hdr.property_count,
        flags: hdr.flags,
        shader_id: hdr.shader_id,
        material_hash: hdr.material_hash,
        properties,
    })
}

fn read_property_value(data: &[u8], offset: usize, property_type: PropertyType) -> Option<PropertyValue> {
    match property_type {
        PropertyType::Bool => read_bool(data, offset).map(PropertyValue::Bool),
        PropertyType::Int32 => read_i32_at(data, offset).map(PropertyValue::Int32),
        PropertyType::FloatA | PropertyType::FloatB => read_f32_at(data, offset).map(PropertyValue::Float),
        PropertyType::Texture => read_u64_at(data, offset).map(PropertyValue::Texture),
        PropertyType::Vec2 => read_f32_array::<2>(data, offset).map(PropertyValue::Vec2),
        PropertyType::Vec3 => read_f32_array::<3>(data, offset).map(PropertyValue::Vec3),
        PropertyType::Vec4 => read_f32_array::<4>(data, offset).map(PropertyValue::Vec4),
        PropertyType::Matrix4x4 => read_f32_array::<16>(data, offset).map(PropertyValue::Matrix4x4),
    }
}

// ─── Property name dictionary ───────────────────────────────────────────────

/// Build a MurmurHash3_32(seed=0) hash for a property name string.
pub fn property_name_hash(name: &str) -> u32 {
    murmur3_32(name.as_bytes(), 0)
}

fn murmur3_32(key: &[u8], seed: u32) -> u32 {
    let len = key.len();
    let nblocks = len / 4;
    let mut h1 = seed;
    let c1: u32 = 0xCC9E_2D51;
    let c2: u32 = 0x1B87_3593;

    for i in 0..nblocks {
        let k1 = u32::from_le_bytes([key[i * 4], key[i * 4 + 1], key[i * 4 + 2], key[i * 4 + 3]]);
        let k1 = k1.wrapping_mul(c1).rotate_left(15).wrapping_mul(c2);
        h1 ^= k1;
        h1 = h1.rotate_left(13).wrapping_mul(5).wrapping_add(0xE654_6B64);
    }

    let tail = &key[nblocks * 4..];
    let mut k1: u32 = 0;
    if tail.len() >= 3 {
        k1 ^= (tail[2] as u32) << 16;
    }
    if tail.len() >= 2 {
        k1 ^= (tail[1] as u32) << 8;
    }
    if !tail.is_empty() {
        k1 ^= tail[0] as u32;
        k1 = k1.wrapping_mul(c1).rotate_left(15).wrapping_mul(c2);
        h1 ^= k1;
    }

    h1 ^= len as u32;
    h1 ^= h1 >> 16;
    h1 = h1.wrapping_mul(0x85EB_CA6B);
    h1 ^= h1 >> 13;
    h1 = h1.wrapping_mul(0xC2B2_AE35);
    h1 ^= h1 >> 16;
    h1
}

/// Build the default property name lookup table (174 known names).
pub fn build_property_name_table() -> HashMap<u32, &'static str> {
    let names: &[&str] = &[
        "AHArray",
        "ODMap",
        "RNAOArray",
        "RNBMap",
        "addSheenTintColor",
        "alphaMul",
        "alphaPow",
        "alphaReference",
        "alphaTestEnable",
        "ambientOcclusionMap",
        "animEmissionPower",
        "animMap",
        "animScale",
        "blendMap",
        "blazeNoiseMap",
        "blurAmount",
        "borderColor",
        "colorIceParallaxRampMax",
        "colorIceParallaxRampMin",
        "colorIceRampMax",
        "colorIceRampMin",
        "detailAlbedoInfluence",
        "detailFadeDistance",
        "detailMap",
        "detailNormalInfluence",
        "detailScale",
        "diffuseMap",
        "directLightShadowMap",
        "distortMap",
        "doubleSided",
        "emissionColor",
        "emissivePower",
        "enableForegroundFoil",
        "enableHolographic",
        "enableRadialOpacity",
        "foamColor",
        "foilHSpeed",
        "foilScale",
        "foilSpeed",
        "g_albedoMap",
        "g_autoScaleTiles",
        "g_bakedDirLightSettings",
        "g_bakedIndirLightSettings",
        "g_detailAlbedoInfluence",
        "g_detailFadeDistance",
        "g_detailGlossInfluence",
        "g_detailNormalInfluence",
        "g_detailScale",
        "g_detailScaleU",
        "g_detailScaleV",
        "g_floatingAmplitude",
        "g_floatingPeriod",
        "g_legacyAlbedoMul",
        "g_legacyAlbedoToSpecular",
        "g_legacyGlossRemap",
        "g_legacySpecularMul",
        "g_legacySpecularPow",
        "g_metallic",
        "g_overlayDepth",
        "g_overlayDetail",
        "g_overlayOpacity",
        "g_pendulumAmplitude",
        "g_pendulumPeriod",
        "g_pendulumRotation",
        "g_texanimBoxOrigin",
        "g_texanimBoxSize",
        "g_texanimFrameNum",
        "g_texanimFramesPerSecond",
        "g_texanimFramesPerSecondSpread",
        "g_texanimOriginalMeshBoxSize",
        "g_texanimPivotNum",
        "g_texanimTexture_pos",
        "g_texanimTexture_posn",
        "g_texanimTexture_rot",
        "g_texanimTexture_tb",
        "g_texanimVertexNum",
        "g_texanimWidth",
        "g_tilesIndex",
        "g_tilesScale",
        "g_translucency",
        "g_translucencyDiffuseFactor",
        "g_translucencyDirectMin",
        "g_translucencyFaceSelection",
        "g_translucencyHighlightFactor",
        "g_translucencyHighlightPower",
        "g_translucencyIndirectFactor",
        "glassAbsorptionCoef",
        "glassColor",
        "glassGlossiness",
        "glassSpecular",
        "glassSubmaterialGlossiness",
        "glassSubmaterialSpecular",
        "glassSubmaterialThreshold",
        "glassTint",
        "glitchLineOffset",
        "glitchLinePeriod",
        "glitchLineWidth",
        "glintsChannelMask",
        "glintsChannelSource",
        "glintsDirectIntensity",
        "glintsHeightMaskMin",
        "glintsInirectIntensity",
        "glowColor",
        "glowStrength",
        "iceChannelMask",
        "iceGlobalInfluence",
        "iceIntensityDirect",
        "iceIntensityIndirect",
        "iceIntensitySunIndirect",
        "iceMaxDepth",
        "iceSunIndirectPower",
        "iceTransmissionDepthMult",
        "iceTransmissionDepthPower",
        "imageTexture",
        "imgFoilColor",
        "incandescenceMap",
        "indirectLightAOMap",
        "indirectLightMul",
        "legacyAlbedoMul",
        "legacyAlbedoToSpecular",
        "legacySpecularMul",
        "legacySpecularPow",
        "magmaFlowTexture",
        "magmaFrequency",
        "magmaLuminance",
        "magmaStep",
        "magmaTexture",
        "magmaVelocity",
        "markColor",
        "maskColor1",
        "maskColor2",
        "maskSmooth",
        "maskSpeed",
        "maskTexture",
        "metallicGlossMap",
        "normalMap",
        "normalsHardness",
        "pulsePeriod",
        "refractionColor",
        "refractionMult",
        "refractionParallaxPercent",
        "sandChannelMask",
        "scanlineFreq",
        "scanlineStrength",
        "shakeFactor",
        "sheen",
        "sheenChannelMask",
        "sheenRoughness",
        "sheenTint",
        "sideFalloffPow",
        "slidePeriod",
        "snowChannelMask",
        "speed1",
        "speed2",
        "sssAttenuation",
        "sssScatterColor",
        "sssShadowAttenuation",
        "sssSunInfluence",
        "sunDiffuseMult",
        "sunSpecMult",
        "sunSpecularMult",
        "texAddressMode",
        "textureOffset",
        "textureScale",
        "topCutting",
        "topScale",
        "topScaleFalloffPow",
        "transitionDuration",
        "waterfallColor",
        "waveScaleX",
        "waveSpeed",
        "waveSpeedX",
        "waveSpeedY",
        "YScale",
    ];

    let mut map = HashMap::with_capacity(names.len());
    for &name in names {
        map.insert(property_name_hash(name), name);
    }
    map
}

// ─── Display helpers ────────────────────────────────────────────────────────

impl MaterialPrototype {
    /// Look up a property by name (using MurmurHash3 matching).
    pub fn get_property(&self, name: &str) -> Option<&MaterialProperty> {
        let target = property_name_hash(name);
        self.properties.iter().find(|p| p.name_hash == target)
    }

    /// Get the texture path hash for a named texture property.
    pub fn get_texture_hash(&self, name: &str) -> Option<u64> {
        self.get_property(name).and_then(|p| match &p.value {
            Some(PropertyValue::Texture(h)) => Some(*h),
            _ => None,
        })
    }

    /// Get a float property value (works for both FloatA and FloatB types).
    pub fn get_float(&self, name: &str) -> Option<f32> {
        self.get_property(name).and_then(|p| match &p.value {
            Some(PropertyValue::Float(v)) => Some(*v),
            _ => None,
        })
    }

    /// Get a vec4 property value.
    pub fn get_vec4(&self, name: &str) -> Option<[f32; 4]> {
        self.get_property(name).and_then(|p| match &p.value {
            Some(PropertyValue::Vec4(v)) => Some(*v),
            _ => None,
        })
    }

    /// Get a bool property value.
    pub fn get_bool(&self, name: &str) -> Option<bool> {
        self.get_property(name).and_then(|p| match &p.value {
            Some(PropertyValue::Bool(v)) => Some(*v),
            _ => None,
        })
    }

    /// Print a human-readable summary of the material.
    pub fn print_summary(&self, prop_names: &HashMap<u32, &str>) {
        println!("  material_hash: 0x{:016X}", self.material_hash);
        println!("  shader_id:     0x{:08X}", self.shader_id);
        println!("  flags:         {}", self.flags);
        println!("  properties:    {}", self.property_count);

        for prop in &self.properties {
            let name = prop_names.get(&prop.name_hash).copied().unwrap_or("???");
            let type_name = prop.property_type.name();
            let val_str = match &prop.value {
                None => String::from("(null)"),
                Some(PropertyValue::Bool(v)) => format!("{v}"),
                Some(PropertyValue::Int32(v)) => format!("{v}"),
                Some(PropertyValue::Float(v)) => format!("{v:.6}"),
                Some(PropertyValue::Texture(v)) => format!("0x{v:016X}"),
                Some(PropertyValue::Vec2(v)) => format!("({:.4}, {:.4})", v[0], v[1]),
                Some(PropertyValue::Vec3(v)) => format!("({:.4}, {:.4}, {:.4})", v[0], v[1], v[2]),
                Some(PropertyValue::Vec4(v)) => {
                    format!("({:.4}, {:.4}, {:.4}, {:.4})", v[0], v[1], v[2], v[3])
                }
                Some(PropertyValue::Matrix4x4(_)) => String::from("[4x4 matrix]"),
            };

            if name == "???" {
                println!("    {type_name:8} 0x{:08X} = {val_str}", prop.name_hash);
            } else {
                println!("    {type_name:8} {name:40} = {val_str}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_murmur3_known_hashes() {
        assert_eq!(property_name_hash("diffuseMap"), 0x820f0280);
        assert_eq!(property_name_hash("normalMap"), 0x4858745d);
        assert_eq!(property_name_hash("doubleSided"), 0xa23cbf8b);
        assert_eq!(property_name_hash("metallicGlossMap"), 0x89babfe7);
        assert_eq!(property_name_hash("AHArray"), 0x0f6cd1d5);
        assert_eq!(property_name_hash("g_tilesScale"), 0x4c72e480);
        assert_eq!(property_name_hash("sheen"), 0x985c860c);
    }

    #[test]
    fn test_property_name_table_completeness() {
        let table = build_property_name_table();
        // All 174 names should be present with unique hashes
        assert_eq!(table.len(), 174);
        // Spot check a few
        assert_eq!(table[&0x820f0280], "diffuseMap");
        assert_eq!(table[&0x4858745d], "normalMap");
        assert_eq!(table[&0x0f6cd1d5], "AHArray");
    }
}
