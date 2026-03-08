"""Decode MaterialPrototype records from standalone .mfm files or assets.bin blob.

MFM Record Layout (0x78 bytes):
  +0x00: u16 property_count
  +0x02: u16 unknown (layer count? 1=normal, 2=blend)
  +0x04: u32 shader_info (e.g., 0x00050600, 0x00010000, 0x00020000)
  +0x08: u64 unknown (often 0x500 = 1280 in standalone, 0 in assets.bin)
  +0x10: u64 names_ptr     -> u32[count] MurmurHash3_32 property name hashes
  +0x18: u64 type_idx_ptr  -> u16[count] (low 4 bits = type, upper 12 bits = index)
  +0x20: u64 bool_ptr      -> u8[]   (type 0)
  +0x28: u64 int32_ptr     -> i32[]  (type 1)
  +0x30: u64 floatA_ptr    -> f32[]  (type 2)
  +0x38: u64 floatB_ptr    -> f32[]  (type 3)
  +0x40: u64 texture_ptr   -> u64[]  (type 4, texture path hashes)
  +0x48: u64 vec2_ptr      -> f32[2] (type 5)
  +0x50: u64 vec3_ptr      -> f32[3] (type 6)
  +0x58: u64 vec4_ptr      -> f32[4] (type 7)
  +0x60: u64 mat4x4_ptr    -> f32[16](type 8, 64 bytes)
  +0x68: u64 material_hash (identifies the material shader/template)
  +0x70: u64 padding (zero)

KEY INSIGHT: All pointers are RELATIVE TO THE RECORD'S OWN ADDRESS.
  absolute_address = record_file_offset + pointer_value
"""

import os
import struct
import sys


# ─── MurmurHash3_32 ─────────────────────────────────────────────────────────
def murmurhash3_32(key, seed=0):
    if isinstance(key, str):
        key = key.encode()
    length = len(key)
    nblocks = length // 4
    h1 = seed & 0xFFFFFFFF
    c1, c2 = 0xCC9E2D51, 0x1B873593
    for i in range(nblocks):
        k1 = struct.unpack_from("<I", key, i * 4)[0]
        k1 = (k1 * c1) & 0xFFFFFFFF
        k1 = ((k1 << 15) | (k1 >> 17)) & 0xFFFFFFFF
        k1 = (k1 * c2) & 0xFFFFFFFF
        h1 ^= k1
        h1 = ((h1 << 13) | (h1 >> 19)) & 0xFFFFFFFF
        h1 = (h1 * 5 + 0xE6546B64) & 0xFFFFFFFF
    tail = key[nblocks * 4 :]
    k1 = 0
    for bi, b in enumerate(tail):
        k1 ^= b << (bi * 8)
    if tail:
        k1 = (k1 * c1) & 0xFFFFFFFF
        k1 = ((k1 << 15) | (k1 >> 17)) & 0xFFFFFFFF
        k1 = (k1 * c2) & 0xFFFFFFFF
        h1 ^= k1
    h1 ^= length
    h1 ^= h1 >> 16
    h1 = (h1 * 0x85EBCA6B) & 0xFFFFFFFF
    h1 ^= h1 >> 13
    h1 = (h1 * 0xC2B2AE35) & 0xFFFFFFFF
    h1 ^= h1 >> 16
    return h1


# ─── Property Name Dictionary (174 names, all cracked) ──────────────────────
PROP_NAMES = {}
for name in [
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
]:
    PROP_NAMES[murmurhash3_32(name)] = name

TYPE_NAMES = [
    "bool",
    "int32",
    "floatA",
    "floatB",
    "texture",
    "vec2",
    "vec3",
    "vec4",
    "mat4x4",
]
TYPE_SIZES = {0: 1, 1: 4, 2: 4, 3: 4, 4: 8, 5: 8, 6: 12, 7: 16, 8: 64}
STRIDE = 0x78


# ─── Record Decoder ──────────────────────────────────────────────────────────
def decode_record(data, rec_offset):
    """Decode one MaterialPrototype record at the given file offset.
    All pointers in the record are relative to rec_offset."""
    base = rec_offset
    count = struct.unpack_from("<H", data, rec_offset)[0]
    w1 = struct.unpack_from("<H", data, rec_offset + 2)[0]
    shader = struct.unpack_from("<I", data, rec_offset + 4)[0]
    val08 = struct.unpack_from("<Q", data, rec_offset + 8)[0]
    mat_hash = struct.unpack_from("<Q", data, rec_offset + 0x68)[0]

    names_ptr = struct.unpack_from("<Q", data, rec_offset + 0x10)[0]
    tidx_ptr = struct.unpack_from("<Q", data, rec_offset + 0x18)[0]

    type_ptrs = []
    for t in range(9):
        ptr = struct.unpack_from("<Q", data, rec_offset + 0x20 + t * 8)[0]
        type_ptrs.append(ptr)

    props = []
    abs_names = base + names_ptr
    abs_tidx = base + tidx_ptr

    for j in range(count):
        if abs_names + (j + 1) * 4 > len(data):
            break
        if abs_tidx + (j + 1) * 2 > len(data):
            break
        h = struct.unpack_from("<I", data, abs_names + j * 4)[0]
        ti = struct.unpack_from("<H", data, abs_tidx + j * 2)[0]
        typ = ti & 0xF
        idx = ti >> 4

        name = PROP_NAMES.get(h, f"0x{h:08x}")
        tname = TYPE_NAMES[typ] if typ < 9 else f"type{typ}"
        val = None

        if typ < 9:
            ptr = type_ptrs[typ]
            if ptr:
                abs_ptr = base + ptr
                try:
                    if typ == 0:  # bool
                        val = data[abs_ptr + idx]
                    elif typ == 1:  # int32
                        val = struct.unpack_from("<i", data, abs_ptr + idx * 4)[0]
                    elif typ in (2, 3):  # floatA, floatB
                        val = struct.unpack_from("<f", data, abs_ptr + idx * 4)[0]
                    elif typ == 4:  # texture (u64 path hash)
                        val = struct.unpack_from("<Q", data, abs_ptr + idx * 8)[0]
                    elif typ == 5:  # vec2
                        off = abs_ptr + idx * 8
                        val = tuple(
                            struct.unpack_from("<f", data, off + k * 4)[0]
                            for k in range(2)
                        )
                    elif typ == 6:  # vec3
                        off = abs_ptr + idx * 12
                        val = tuple(
                            struct.unpack_from("<f", data, off + k * 4)[0]
                            for k in range(3)
                        )
                    elif typ == 7:  # vec4
                        off = abs_ptr + idx * 16
                        val = tuple(
                            struct.unpack_from("<f", data, off + k * 4)[0]
                            for k in range(4)
                        )
                    elif typ == 8:  # mat4x4
                        off = abs_ptr + idx * 64
                        val = tuple(
                            struct.unpack_from("<f", data, off + k * 4)[0]
                            for k in range(16)
                        )
                except (struct.error, IndexError):
                    val = "<read error>"

        props.append(
            {
                "hash": h,
                "name": name,
                "type": typ,
                "type_name": tname,
                "index": idx,
                "value": val,
            }
        )

    return {
        "count": count,
        "w1": w1,
        "shader": shader,
        "val08": val08,
        "material_hash": mat_hash,
        "properties": props,
    }


def format_value(prop):
    """Format a property value for display."""
    val = prop["value"]
    typ = prop["type"]
    if val is None:
        return ""
    if typ == 0:  # bool
        return f"= {bool(val)}"
    elif typ == 1:  # int32
        return f"= {val}"
    elif typ in (2, 3):  # float
        return f"= {val:.6f}"
    elif typ == 4:  # texture
        return f"= 0x{val:016x}"
    elif typ in (5, 6, 7):  # vec2/3/4
        return "= (" + ", ".join(f"{v:.4f}" for v in val) + ")"
    elif typ == 8:  # mat4x4
        return "= [4x4 matrix]"
    return f"= {val}"


# ─── Main ────────────────────────────────────────────────────────────────────
def decode_mfm_file(path, max_records=None):
    """Decode all records from a standalone .mfm file."""
    with open(path, "rb") as f:
        data = f.read()

    print(f"File: {path}")
    print(f"Size: {len(data)} bytes ({len(data) / 1024:.1f} KB)")

    # Estimate number of records from first record's names_ptr
    # Since pointers are record-relative, names_ptr for record 0 =
    # offset from record 0 to its names array = total record area size
    rec0_names = struct.unpack_from("<Q", data, 0x10)[0]
    est_records = rec0_names // STRIDE
    print(f"Estimated records: {est_records} (names_ptr=0x{rec0_names:x})")
    print()

    if max_records is not None:
        est_records = min(est_records, max_records)

    records = []
    for i in range(est_records):
        off = i * STRIDE
        if off + STRIDE > len(data):
            break
        count = struct.unpack_from("<H", data, off)[0]
        if count == 0 or count > 100:
            continue
        rec = decode_record(data, off)
        records.append((i, rec))

        # Print
        cracked = sum(1 for p in rec["properties"] if p["hash"] in PROP_NAMES)
        print(
            f"--- Record {i} (hash=0x{rec['material_hash']:016x}, shader=0x{rec['shader']:08x}, "
            f"props={rec['count']}, cracked={cracked}/{rec['count']}) ---"
        )
        for p in rec["properties"]:
            print(
                f"  {p['type_name']:8s}[{p['index']:2d}] {p['name']:40s} {format_value(p)}"
            )
        print()

    print(f"Total decoded: {len(records)} records")
    return records


if __name__ == "__main__":
    if len(sys.argv) < 2:
        # Default: decode BTH_01.mfm
        path = r"C:/Users/lander/AppData/Local/Temp/mfm_study/content/location/nature/tile/textures/LNT016_BTH_01.mfm"
        if not os.path.exists(path):
            print(f"Usage: python {sys.argv[0]} <file.mfm> [max_records]")
            sys.exit(1)
    else:
        path = sys.argv[1]

    max_records = int(sys.argv[2]) if len(sys.argv) > 2 else 20
    decode_mfm_file(path, max_records=max_records)
