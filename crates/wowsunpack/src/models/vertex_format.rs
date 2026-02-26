/// Vertex format string parser and attribute unpacking for BigWorld geometry.
///
/// Format strings look like `set3/xyznuvtbpc` where the suffix encodes which
/// attributes are present and in what order. Vertices are always tightly packed
/// with a fixed stride per format.
///
/// Semantic meaning of a vertex attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttributeSemantic {
    Position,
    Normal,
    TexCoord0,
    TexCoord1,
    Tangent,
    Binormal,
    BoneIndices,
    BoneWeights,
    Extra,
}

/// How an attribute is stored in the vertex buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttributeFormat {
    /// 3 x f32 = 12 bytes (position)
    Float32x3,
    /// 4 bytes packed normal/tangent/binormal
    PackedNormal,
    /// 4 bytes = 2 x float16 (UV coordinates)
    PackedUV,
    /// 4 bytes (unknown / extra data)
    Raw4,
}

/// A single vertex attribute descriptor.
#[derive(Debug, Clone)]
pub struct VertexAttribute {
    pub semantic: AttributeSemantic,
    pub format: AttributeFormat,
    pub offset: usize,
}

/// Parsed vertex format with all attributes and computed stride.
#[derive(Debug, Clone)]
pub struct VertexFormat {
    pub attributes: Vec<VertexAttribute>,
    pub stride: usize,
}

/// Parse a format string like `set3/xyznuvtbpc` or just `xyznuvtb` into a
/// vertex format descriptor.
///
/// Recognized attribute codes (must appear in this order):
/// - `xyz`    → POSITION (f32 x 3, 12 bytes)
/// - `n`      → NORMAL (packed 4 bytes)
/// - `uv`     → TEXCOORD_0 (packed 4 bytes, 2x float16)
/// - `uv2`    → if a second `uv` appears after the first, it's TEXCOORD_1
/// - `tb`     → TANGENT + BINORMAL (2 x packed 4 bytes)
/// - `iiiww`  → BONE_INDICES (3) + BONE_WEIGHTS (2) — 8 bytes, skipped for now
/// - `r`      → extra data (4 bytes)
/// - `pc`     → per-vertex color flag (0 bytes in practice based on stride matching)
/// - `i`, `oi`→ instance data variants (0 bytes)
pub fn parse_vertex_format(format_name: &str) -> VertexFormat {
    // Strip the `set3/` or `setN/` prefix if present.
    let code = format_name.rsplit('/').next().unwrap_or(format_name);

    let mut attrs = Vec::new();
    let mut offset = 0usize;
    let mut uv_count = 0u32;

    let mut chars = code.chars().peekable();

    while let Some(&ch) = chars.peek() {
        match ch {
            'x' => {
                // xyz → POSITION
                chars.next(); // x
                if chars.peek() == Some(&'y') {
                    chars.next();
                } // y
                if chars.peek() == Some(&'z') {
                    chars.next();
                } // z
                attrs.push(VertexAttribute {
                    semantic: AttributeSemantic::Position,
                    format: AttributeFormat::Float32x3,
                    offset,
                });
                offset += 12;
            }
            'n' => {
                chars.next();
                attrs.push(VertexAttribute {
                    semantic: AttributeSemantic::Normal,
                    format: AttributeFormat::PackedNormal,
                    offset,
                });
                offset += 4;
            }
            'u' => {
                // uv or uv2
                chars.next(); // u
                if chars.peek() == Some(&'v') {
                    chars.next();
                } // v
                // Check for '2' suffix (second UV set)
                if chars.peek() == Some(&'2') {
                    chars.next();
                    // This is a second UV channel, but it comes *before* the first
                    // uv in the code string for formats like xyznuv2tb. Actually
                    // "uv2" means there are TWO uv channels.
                    // Format: first uv at current offset, second uv right after.
                    attrs.push(VertexAttribute {
                        semantic: AttributeSemantic::TexCoord0,
                        format: AttributeFormat::PackedUV,
                        offset,
                    });
                    offset += 4;
                    attrs.push(VertexAttribute {
                        semantic: AttributeSemantic::TexCoord1,
                        format: AttributeFormat::PackedUV,
                        offset,
                    });
                    offset += 4;
                    uv_count = 2;
                } else {
                    let semantic =
                        if uv_count == 0 { AttributeSemantic::TexCoord0 } else { AttributeSemantic::TexCoord1 };
                    attrs.push(VertexAttribute { semantic, format: AttributeFormat::PackedUV, offset });
                    offset += 4;
                    uv_count += 1;
                }
            }
            't' => {
                // tb → tangent + binormal
                chars.next(); // t
                if chars.peek() == Some(&'b') {
                    chars.next(); // b
                    attrs.push(VertexAttribute {
                        semantic: AttributeSemantic::Tangent,
                        format: AttributeFormat::PackedNormal,
                        offset,
                    });
                    offset += 4;
                    attrs.push(VertexAttribute {
                        semantic: AttributeSemantic::Binormal,
                        format: AttributeFormat::PackedNormal,
                        offset,
                    });
                    offset += 4;
                } else {
                    // lone 't' — skip
                    chars.next();
                }
            }
            'i' => {
                // iiiww → bone indices + weights (8 bytes)
                // lone i → single index (4 bytes)
                chars.next();
                if chars.peek() == Some(&'i') {
                    chars.next(); // second i
                    if chars.peek() == Some(&'i') {
                        chars.next(); // third i
                    }
                    if chars.peek() == Some(&'w') {
                        chars.next(); // first w
                        if chars.peek() == Some(&'w') {
                            chars.next(); // second w
                        }
                    }
                    // Skip skinning data for now (8 bytes: 3 indices + 2 weights)
                    attrs.push(VertexAttribute {
                        semantic: AttributeSemantic::BoneIndices,
                        format: AttributeFormat::Raw4,
                        offset,
                    });
                    offset += 4;
                    attrs.push(VertexAttribute {
                        semantic: AttributeSemantic::BoneWeights,
                        format: AttributeFormat::Raw4,
                        offset,
                    });
                    offset += 4;
                } else {
                    // lone 'i' → single index (4 bytes)
                    attrs.push(VertexAttribute {
                        semantic: AttributeSemantic::BoneIndices,
                        format: AttributeFormat::Raw4,
                        offset,
                    });
                    offset += 4;
                }
            }
            'r' => {
                chars.next();
                attrs.push(VertexAttribute {
                    semantic: AttributeSemantic::Extra,
                    format: AttributeFormat::Raw4,
                    offset,
                });
                offset += 4;
            }
            'p' => {
                // pc — flag only, no extra bytes
                chars.next();
                if chars.peek() == Some(&'c') {
                    chars.next();
                }
            }
            'o' => {
                // oi — instance flag, no extra bytes
                chars.next();
                if chars.peek() == Some(&'i') {
                    chars.next();
                }
            }
            'w' => {
                // stray w (shouldn't happen outside iiiww), skip
                chars.next();
            }
            _ => {
                // Unknown character, skip
                chars.next();
            }
        }
    }

    VertexFormat { attributes: attrs, stride: offset }
}

/// Unpack a 4-byte packed normal into `[f32; 3]`.
///
/// The packed format is 4 signed bytes: `[x, y, z, w]` where each component
/// is mapped from `[-127, 127]` to `[-1.0, 1.0]`.
pub fn unpack_normal(packed: u32) -> [f32; 3] {
    let bytes = packed.to_le_bytes();
    [(bytes[0] as i8) as f32 / 127.0, (bytes[1] as i8) as f32 / 127.0, (bytes[2] as i8) as f32 / 127.0]
}

/// Unpack a 4-byte packed UV into `[f32; 2]`.
///
/// The on-disk format is 2 x IEEE 754 float16 (half-precision) with a -0.5
/// bias, stored as `[u_half, v_half]` in little-endian order. The engine
/// stores `actual_uv - 0.5` to center values around zero where float16 has
/// the most precision, then adds 0.5 back when loading to the GPU.
pub fn unpack_uv(packed: u32) -> [f32; 2] {
    let bytes = packed.to_le_bytes();
    let u_bits = u16::from_le_bytes([bytes[0], bytes[1]]);
    let v_bits = u16::from_le_bytes([bytes[2], bytes[3]]);
    [half::f16::from_bits(u_bits).to_f32() + 0.5, half::f16::from_bits(v_bits).to_f32() + 0.5]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xyznuv() {
        let fmt = parse_vertex_format("set3/xyznuv");
        assert_eq!(fmt.stride, 20);
        assert_eq!(fmt.attributes.len(), 3);
        assert_eq!(fmt.attributes[0].semantic, AttributeSemantic::Position);
        assert_eq!(fmt.attributes[0].offset, 0);
        assert_eq!(fmt.attributes[1].semantic, AttributeSemantic::Normal);
        assert_eq!(fmt.attributes[1].offset, 12);
        assert_eq!(fmt.attributes[2].semantic, AttributeSemantic::TexCoord0);
        assert_eq!(fmt.attributes[2].offset, 16);
    }

    #[test]
    fn test_xyznuvtb() {
        let fmt = parse_vertex_format("set3/xyznuvtb");
        assert_eq!(fmt.stride, 28);
        assert_eq!(fmt.attributes.len(), 5);
        assert_eq!(fmt.attributes[3].semantic, AttributeSemantic::Tangent);
        assert_eq!(fmt.attributes[3].offset, 20);
        assert_eq!(fmt.attributes[4].semantic, AttributeSemantic::Binormal);
        assert_eq!(fmt.attributes[4].offset, 24);
    }

    #[test]
    fn test_xyznuvr() {
        let fmt = parse_vertex_format("set3/xyznuvr");
        assert_eq!(fmt.stride, 24);
    }

    #[test]
    fn test_xyznuvtbpc() {
        // pc adds no bytes
        let fmt = parse_vertex_format("set3/xyznuvtbpc");
        assert_eq!(fmt.stride, 28);
    }

    #[test]
    fn test_xyznuv2tb() {
        let fmt = parse_vertex_format("set3/xyznuv2tb");
        assert_eq!(fmt.stride, 32);
        // Should have 2 UV channels
        let uv_attrs: Vec<_> = fmt
            .attributes
            .iter()
            .filter(|a| matches!(a.semantic, AttributeSemantic::TexCoord0 | AttributeSemantic::TexCoord1))
            .collect();
        assert_eq!(uv_attrs.len(), 2);
    }

    #[test]
    fn test_xyznuvtbipc() {
        // xyz(12) + n(4) + uv(4) + tb(8) + i(4) + pc(0) = 32
        let fmt = parse_vertex_format("set3/xyznuvtbipc");
        assert_eq!(fmt.stride, 32);
    }

    #[test]
    fn test_xyznuviiiwwtb() {
        let fmt = parse_vertex_format("set3/xyznuviiiwwtb");
        // xyz(12) + n(4) + uv(4) + iiiww(8) + tb(8) = 36
        assert_eq!(fmt.stride, 36);
    }

    #[test]
    fn test_unpack_normal() {
        // All-positive unit vector approximation
        let packed = u32::from_le_bytes([127, 0, 0, 0]);
        let n = unpack_normal(packed);
        assert!((n[0] - 1.0).abs() < 0.01);
        assert!(n[1].abs() < 0.01);
        assert!(n[2].abs() < 0.01);
    }
}
