"""Decode ALL MaterialPrototype records from a standalone .mfm file.

Each record is 0x78 bytes with absolute pointers into a shared data section.
Layout:
  +0x00: u16 property_count
  +0x02: u16 material_layer_count (or some other metadata)
  +0x04: u32 shader_info
  +0x08: u64 ???
  +0x10: u64 names_ptr -> u32[count] property name hashes
  +0x18: u64 type_idx_ptr -> u16[count] type_and_index
  +0x20-0x60: u64[9] type_ptrs (bool, int, floatA, floatB, tex, vec2, vec3, vec4, mat4)
  +0x68: u64 material_hash
  +0x70: u64 padding/zero
"""

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
    "diffuseMap",
    "normalMap",
    "specularMap",
    "heightMap",
]:
    KNOWN[murmurhash3_32(name)] = name

MFM_PATH = r"C:/Users/lander/AppData/Local/Temp/mfm_study/content/location/nature/tile/textures/LNT016_BTH_01.mfm"
with open(MFM_PATH, "rb") as f:
    data = f.read()

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

# Find the data section boundary: record 0's names_ptr is a good anchor.
# All valid pointers should be in the same general area (high offsets in the file).
rec0_names_ptr = read_u64(data, 0x10)
# Use record 0's names_ptr as the approximate start of the data section.
# The record area goes from 0 to somewhere before this.
# Round down to stride boundary.
data_section_start = rec0_names_ptr
# But multiple records' pointers might go further back. Let's find the true min
# by scanning only pointer fields (+0x10 to +0x60) for values > 0x10000 (reasonable threshold)
min_ptr = data_section_start
for i in range(data_section_start // STRIDE):
    off = i * STRIDE
    for f_off in range(0x10, 0x68, 8):
        if off + f_off + 8 > len(data):
            break
        ptr = read_u64(data, off + f_off)
        if 0x10000 < ptr < min_ptr:
            min_ptr = ptr

print(f"File: {len(data)} bytes")
print(f"Record 0 names_ptr: 0x{rec0_names_ptr:x}")
print(f"Min valid pointer found: 0x{min_ptr:x}")
num_records = min_ptr // STRIDE
print(f"Approx number of records: {num_records}")
print(f"Data section size: {len(data) - min_ptr} bytes")

# OK so the record area = [0, min_ptr), data area = [min_ptr, EOF)
# Let's decode each record properly

valid_count = 0
all_prop_hashes = set()
records_by_hash = {}

for i in range(num_records):
    off = i * STRIDE
    count = read_u16(data, off)

    if count == 0:
        continue
    if count > 100:
        continue

    names_ptr = read_u64(data, off + 0x10)
    tidx_ptr = read_u64(data, off + 0x18)

    if names_ptr == 0 or tidx_ptr == 0:
        continue
    if names_ptr >= len(data) or tidx_ptr >= len(data):
        continue

    # Validate: names_ptr + count*4 should roughly equal tidx_ptr
    expected_tidx = names_ptr + count * 4
    if abs(expected_tidx - tidx_ptr) > 4:  # allow small alignment gap
        continue

    # This is a valid record!
    valid_count += 1
    w1 = read_u16(data, off + 2)
    w2 = read_u16(data, off + 4)
    w3 = read_u16(data, off + 6)
    rec_hash = read_u64(data, off + 0x68)

    if rec_hash not in records_by_hash:
        records_by_hash[rec_hash] = []
    records_by_hash[rec_hash].append(i)

    # Read property names
    for j in range(count):
        h = read_u32(data, names_ptr + j * 4)
        all_prop_hashes.add(h)

print(f"\nValid records (names_ptr + count*4 == type_ptr): {valid_count}")
print(f"Unique material hashes: {len(records_by_hash)}")
print(f"Unique property name hashes: {len(all_prop_hashes)}")

# Show each unique material hash and how many records use it
print(f"\nMaterial hash distribution:")
for h, indices in sorted(records_by_hash.items(), key=lambda x: x[1][0]):
    print(f"  0x{h:016x}: {len(indices)} records (first at index {indices[0]})")

# Decode the first record of each unique material hash
print(f"\n{'=' * 70}")
print(f"DETAILED DECODE OF FIRST RECORD PER MATERIAL HASH")
print(f"{'=' * 70}")

for rec_hash, indices in sorted(records_by_hash.items(), key=lambda x: x[1][0]):
    i = indices[0]
    off = i * STRIDE
    count = read_u16(data, off)
    w1 = read_u16(data, off + 2)
    w2 = read_u16(data, off + 4)
    w3 = read_u16(data, off + 6)
    names_ptr = read_u64(data, off + 0x10)
    tidx_ptr = read_u64(data, off + 0x18)

    print(
        f"\nRecord {i} @0x{off:06x}: count={count}, meta=({w1},{w2},{w3}), hash=0x{rec_hash:016x}"
    )
    print(f"  Appears {len(indices)} times in file")

    # Read type pointers
    type_ptrs = {}
    for t in range(9):
        ptr = read_u64(data, off + 0x20 + t * 8)
        if ptr:
            type_ptrs[t] = ptr

    # Decode properties
    for j in range(count):
        h = read_u32(data, names_ptr + j * 4)
        ti = read_u16(data, tidx_ptr + j * 2)
        typ = ti & 0xF
        idx = ti >> 4
        tname = TYPE_NAMES[typ] if typ < len(TYPE_NAMES) else f"type{typ}"
        pname = KNOWN.get(h, f"0x{h:08x}")

        val_str = ""
        ptr = type_ptrs.get(typ, 0)
        if ptr and ptr < len(data):
            if typ == 0 and ptr + idx < len(data):
                val_str = f"= {data[ptr + idx]}"
            elif typ in (1, 2, 3):
                voff = ptr + idx * 4
                if voff + 4 <= len(data):
                    fval = read_f32(data, voff)
                    val_str = f"= {fval:.4f}"
            elif typ == 4:
                voff = ptr + idx * 8
                if voff + 8 <= len(data):
                    val_str = f"= 0x{read_u64(data, voff):016x}"
            elif typ == 7:
                voff = ptr + idx * 16
                if voff + 16 <= len(data):
                    v = [read_f32(data, voff + k * 4) for k in range(4)]
                    val_str = f"= ({v[0]:.2f}, {v[1]:.2f}, {v[2]:.2f}, {v[3]:.2f})"

        print(f"  [{j:2d}] {tname:8s}[{idx:2d}] {pname:30s} {val_str}")

# Show uncracked property hashes
cracked = set(KNOWN.keys())
uncracked = all_prop_hashes - cracked
print(f"\n{'=' * 70}")
print(
    f"Uncracked property hashes: {len(uncracked)} (out of {len(all_prop_hashes)} unique)"
)
if uncracked:
    for h in sorted(uncracked)[:30]:
        print(f"  0x{h:08x}")
    if len(uncracked) > 30:
        print(f"  ... and {len(uncracked) - 30} more")
