"""Decode multiple MFM MaterialPrototype records (item_size=0x78, no file header)"""

import re
import struct


def read_u16(data, off):
    return struct.unpack_from("<H", data, off)[0]


def read_u32(data, off):
    return struct.unpack_from("<I", data, off)[0]


def read_u64(data, off):
    return struct.unpack_from("<Q", data, off)[0]


def read_f32(data, off):
    return struct.unpack_from("<f", data, off)[0]


def murmurhash3_32(key, seed=0):
    key = key.encode("utf-8") if isinstance(key, str) else key
    length = len(key)
    nblocks = length // 4
    h1 = seed & 0xFFFFFFFF
    c1 = 0xCC9E2D51
    c2 = 0x1B873593
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
    if len(tail) >= 3:
        k1 ^= tail[2] << 16
    if len(tail) >= 2:
        k1 ^= tail[1] << 8
    if len(tail) >= 1:
        k1 ^= tail[0]
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


# Build hash -> name lookup from assets.bin string table approach
# For now, use the known names we've confirmed
KNOWN = {}
for name in [
    "AHArray",
    "ODMap",
    "RNAOArray",
    "RNBMap",
    "blendMap",
    "doubleSided",
    "g_metallic",
    "g_overlayDetail",
    "g_tilesScale",
    "sheen",
    "sheenRoughness",
    "sheenTint",
    "addSheenTintColor",
    "g_overlayDepth",
    "g_overlayOpacity",
    "g_tilesIndex",
    "sheenChannelMask",
    # Common material props
    "diffuseMap",
    "normalMap",
    "specularMap",
    "heightMap",
    "overlayMap",
    "alphaTest",
    "alphaBlend",
    "alphaReference",
    "opacity",
    "specularPower",
    "glossiness",
    "metallic",
    "roughness",
]:
    h = murmurhash3_32(name)
    KNOWN[h] = name

# Also load the names file if the subagent created one
import os

names_file = r"G:/dev/wows-toolkit/scripts/mfm_property_names.txt"
if os.path.exists(names_file):
    with open(names_file) as f:
        for line in f:
            name = line.strip()
            if name:
                KNOWN[murmurhash3_32(name)] = name

MFM_PATH = r"C:/Users/lander/AppData/Local/Temp/mfm_study/content/location/nature/tile/textures/LNT016_BTH_01.mfm"

with open(MFM_PATH, "rb") as f:
    data = f.read()

TYPE_NAMES = [
    "bool",
    "int32",
    "float_A",
    "float_B",
    "texture",
    "vec2",
    "vec3",
    "vec4",
    "mat4x4",
    "type9",
    "type10",
]
STRIDE = 0x78
num_records = len(data) // STRIDE  # approximate

print(f"File: {len(data)} bytes, ~{num_records} records at stride 0x{STRIDE:x}")

# Decode first 20 records to understand the structure
all_hashes = set()
for rec_idx in range(min(20, num_records)):
    off = rec_idx * STRIDE
    if off + STRIDE > len(data):
        break

    count = read_u16(data, off)
    w1 = read_u16(data, off + 2)
    w2 = read_u16(data, off + 4)
    w3 = read_u16(data, off + 6)

    if count == 0 or count > 200:
        print(
            f"\nRecord {rec_idx} @0x{off:06x}: count={count} (skipping, likely invalid)"
        )
        continue

    names_ptr = read_u64(data, off + 0x10)
    type_idx_ptr = read_u64(data, off + 0x18)
    rec_hash = read_u64(data, off + 0x68)

    print(f"\n{'=' * 70}")
    print(
        f"Record {rec_idx} @0x{off:06x}: count={count}, words=({w1},{w2},{w3}), hash=0x{rec_hash:016x}"
    )

    # Read type-specific pointers
    type_ptrs = {}
    for t in range(min(11, (0x68 - 0x20) // 8)):
        ptr = read_u64(data, off + 0x20 + t * 8)
        if ptr:
            tname = TYPE_NAMES[t] if t < len(TYPE_NAMES) else f"type{t}"
            type_ptrs[t] = ptr

    if names_ptr == 0 or names_ptr >= len(data):
        print(f"  Invalid names_ptr=0x{names_ptr:x}")
        continue
    if type_idx_ptr == 0 or type_idx_ptr >= len(data):
        print(f"  Invalid type_idx_ptr=0x{type_idx_ptr:x}")
        continue

    # Decode properties
    for i in range(count):
        h = read_u32(data, names_ptr + i * 4)
        ti = read_u16(data, type_idx_ptr + i * 2)
        typ = ti & 0xF
        idx = ti >> 4
        tname = TYPE_NAMES[typ] if typ < len(TYPE_NAMES) else f"type{typ}"
        prop_name = KNOWN.get(h, f"0x{h:08x}")
        all_hashes.add(h)

        # Read value
        val_str = ""
        ptr = type_ptrs.get(typ, 0)
        if ptr and ptr < len(data):
            if typ == 0:  # bool
                if ptr + idx < len(data):
                    val_str = f"= {data[ptr + idx]}"
            elif typ in (1, 2, 3):  # int/float
                voff = ptr + idx * 4
                if voff + 4 <= len(data):
                    ival = read_u32(data, voff)
                    fval = read_f32(data, voff)
                    val_str = f"= {fval:.4f}" if typ >= 2 else f"= {ival}"
            elif typ == 4:  # texture (u64)
                voff = ptr + idx * 8
                if voff + 8 <= len(data):
                    val_str = f"= 0x{read_u64(data, voff):016x}"
            elif typ == 7:  # vec4
                voff = ptr + idx * 16
                if voff + 16 <= len(data):
                    x, y, z, w = [read_f32(data, voff + j * 4) for j in range(4)]
                    val_str = f"= ({x:.4f}, {y:.4f}, {z:.4f}, {w:.4f})"

        print(f"  [{i:2d}] {tname:8s}[{idx}] {prop_name:30s} {val_str}")

print(f"\n\n{'=' * 70}")
print(f"Total unique property hashes seen: {len(all_hashes)}")
uncracked = [h for h in sorted(all_hashes) if h not in KNOWN]
if uncracked:
    print(f"Uncracked hashes ({len(uncracked)}):")
    for h in uncracked:
        print(f"  0x{h:08x}")
