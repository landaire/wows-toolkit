"""Understand the full structure of a standalone .mfm file.

Theory: the file contains:
- One MaterialPrototype record (0x78 bytes) at offset 0
- Out-of-line data referenced by the record's pointers
- Possibly additional sub-structures (technique passes, shader variants)

Let's map what the pointers in record 0 actually reference."""

import struct


def read_u16(data, off):
    return struct.unpack_from("<H", data, off)[0]


def read_u32(data, off):
    return struct.unpack_from("<I", data, off)[0]


def read_u64(data, off):
    return struct.unpack_from("<Q", data, off)[0]


def read_f32(data, off):
    return struct.unpack_from("<f", data, off)[0]


MFM_PATH = r"C:/Users/lander/AppData/Local/Temp/mfm_study/content/location/nature/tile/textures/LNT016_BTH_01.mfm"
with open(MFM_PATH, "rb") as f:
    data = f.read()

print(f"File size: {len(data)} bytes (0x{len(data):x})")
print(f"File size: {len(data) / 1024 / 1024:.2f} MB")

# Record 0 header
count = read_u16(data, 0)
w1 = read_u16(data, 2)
w2 = read_u16(data, 4)
w3 = read_u16(data, 6)
print(f"\nRecord 0: count={count}, words=({w1},{w2},{w3})")
print(f"  +0x08: 0x{read_u64(data, 0x08):016x}")

# All pointers in record 0
ptrs = {}
for field_off in range(0x10, 0x68, 8):
    val = read_u64(data, field_off)
    if val:
        ptrs[field_off] = val

print(f"\nNon-zero pointers in record 0:")
field_names = {
    0x10: "name_ids",
    0x18: "type_idx",
    0x20: "bool_arr",
    0x28: "int32_arr",
    0x30: "float_A_arr",
    0x38: "float_B_arr",
    0x40: "texture_arr",
    0x48: "vec2_arr",
    0x50: "vec3_arr",
    0x58: "vec4_arr",
    0x60: "mat4x4_arr",
}
for off, val in sorted(ptrs.items()):
    name = field_names.get(off, f"field_0x{off:02x}")
    print(f"  +0x{off:02x} ({name:15s}): 0x{val:x}")

# Map the data ranges referenced
print(f"\nData ranges referenced:")
# name_ids: u32[17] = 68 bytes
# type_idx: u16[17] = 34 bytes
# bool: 1 byte per entry
# float_B: 4 bytes per entry
# texture: 8 bytes per entry
# vec4: 16 bytes per entry

# Count each type
type_counts = {0: 0, 1: 0, 2: 0, 3: 0, 4: 0, 5: 0, 6: 0, 7: 0, 8: 0}
tidx_ptr = ptrs[0x18]
for i in range(count):
    ti = read_u16(data, tidx_ptr + i * 2)
    typ = ti & 0xF
    idx = ti >> 4
    type_counts[typ] = max(type_counts[typ], idx + 1)

print(f"  Type counts: {type_counts}")
type_sizes = {0: 1, 1: 4, 2: 4, 3: 4, 4: 8, 5: 8, 6: 12, 7: 16, 8: 64}
total_data = 0
for off, val in sorted(ptrs.items()):
    name = field_names.get(off, f"field_0x{off:02x}")
    if off == 0x10:
        size = count * 4
    elif off == 0x18:
        size = count * 2
    else:
        typ = (off - 0x20) // 8
        size = type_counts.get(typ, 0) * type_sizes.get(typ, 0)
    end = val + size
    total_data += size
    print(f"  {name:15s}: 0x{val:06x} - 0x{end:06x} ({size} bytes)")

print(f"\nTotal data referenced by record 0: {total_data} bytes")
print(f"Record 0 itself: {0x78} bytes")
print(f"Total accounted: {0x78 + total_data} bytes")
print(f"File size: {len(data)} bytes")
print(
    f"Unaccounted: {len(data) - 0x78 - total_data} bytes ({(len(data) - 0x78 - total_data) / len(data) * 100:.1f}%)"
)

# So the vast majority of the file is NOT referenced by record 0.
# What's in the rest? Let me look at the structure after the record.
# Maybe the file has MULTIPLE records (not just one), and they share
# the out-of-line data region.

# From the original conversation, the file was extracted as a complete .mfm.
# In BigWorld, .mfm files can contain multiple materials (one per technique pass).
# Let me check: how many unique hash values are at +0x68 (the material hash)?
print(f"\n=== Scanning for material hashes at +0x68 in 0x78 blocks ===")
hash_counts = {}
for i in range(len(data) // 0x78):
    off = i * 0x78
    h = read_u64(data, off + 0x68)
    if h != 0:
        hash_counts[h] = hash_counts.get(h, 0) + 1

print(f"Unique non-zero hashes at +0x68: {len(hash_counts)}")
for h, c in sorted(hash_counts.items(), key=lambda x: -x[1])[:20]:
    print(f"  0x{h:016x}: appears {c} times")

# Let me also look for a different structure. Maybe the file starts with
# a table of contents or array header.
# The first 8 bytes: 0x0005060000010011
# As separate fields:
# u16: 0x0011 = 17 (property count)
# u16: 0x0001 = 1
# u16: 0x0600 = 1536
# u16: 0x0005 = 5
#
# What if 0x0001 means "1 material", and 0x0600/0x0005 are shader info?
# And then at +0x08: 0x0000000000000500 -- what's 0x500?
# 0x500 = 1280 -- could be an offset?

# Let me check what's at offset 0x500
print(f"\n=== Data at offset 0x500 (1280) ===")
for off in range(0x500, min(0x500 + 0x80, len(data)), 8):
    val = read_u64(data, off)
    print(f"  +0x{off:04x}: 0x{val:016x}")

# Also check if 0x500 could be the start of a data section
# Record 0 pointers all point to ~0x348700 area.
# What about a DIFFERENT .mfm file that's smaller?

# Let me look at the smallest file
import os

files = []
for f in os.listdir(os.path.dirname(MFM_PATH)):
    if f.endswith(".mfm"):
        fp = os.path.join(os.path.dirname(MFM_PATH), f)
        files.append((os.path.getsize(fp), f, fp))
files.sort()

print(f"\n=== MFM files by size ===")
for size, name, path in files:
    print(f"  {size:>10,} bytes  {name}")

# Read the smallest one for comparison
smallest_path = files[0][2]
with open(smallest_path, "rb") as f:
    small = f.read()

print(f"\n=== Smallest MFM: {files[0][1]} ({len(small)} bytes) ===")
print(f"Record 0 header:")
for off in range(0, 0x78, 8):
    val = read_u64(small, off)
    print(f"  +0x{off:02x}: 0x{val:016x}")

sm_count = read_u16(small, 0)
sm_names_ptr = read_u64(small, 0x10)
print(f"\ncount={sm_count}, names_ptr=0x{sm_names_ptr:x}")
print(f"File size / record data ratio: {len(small)} / 0x78 = {len(small) / 0x78:.1f}")
print(f"names_ptr / file_size: {sm_names_ptr / len(small) * 100:.1f}%")

# For BTH_01: names_ptr=0x348703 in 5,299,966 byte file
# 0x348703 / 5299966 = 0x348703 / 0x50D77E = 64.9%
# So the data area starts at ~65% of the file.
# The first 65% is... records? Or some other structure?

# Let me count how many valid property records there are
print(f"\n=== Counting valid property records in BTH_01 ===")
valid_records = []
for i in range(len(data) // 0x78):
    off = i * 0x78
    cnt = read_u16(data, off)
    if cnt == 0 or cnt > 100:
        continue
    ptr = read_u64(data, off + 0x10)
    if ptr == 0 or ptr >= len(data):
        continue
    tidx = read_u64(data, off + 0x18)
    if tidx == 0 or tidx >= len(data):
        continue
    # Both pointers valid - this looks like a real record
    valid_records.append((i, off, cnt))

print(f"Valid records (both name+type ptrs valid): {len(valid_records)}")
if valid_records:
    print(f"First: index {valid_records[0][0]} at offset 0x{valid_records[0][1]:x}")
    print(f"Last:  index {valid_records[-1][0]} at offset 0x{valid_records[-1][1]:x}")
    end_of_records = valid_records[-1][1] + 0x78
    print(f"End of last record: 0x{end_of_records:x}")
    print(f"Data area starts around: 0x{ptrs[0x10]:x}")
    print(
        f"Gap between last record end and data start: {ptrs[0x10] - end_of_records} bytes"
    )
