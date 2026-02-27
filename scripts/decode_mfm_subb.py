"""Decode MFM sub-record B (the second 0x78 block per material entry)"""

import struct


def read_u8(data, off):
    return data[off]


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

print(f"File size: {len(data)} bytes")

# We know the file has a 16-byte header: u64 count + u64 header_size(=16)
# and each record is 0x78 bytes (MaterialPrototype item size from the plan)
# But the plan says item_size = 0x78, NOT 0xF0.
# So maybe there's no "sub-record B" — maybe each 0x78 record is independent.

# Let's first check the file header
header_count = read_u64(data, 0)
header_size = read_u64(data, 8)
print(f"Header: count={header_count}, header_size={header_size}")

# If header_size=16 and item_size=0x78, the first record starts at offset 16
# But from the previous analysis, the first record seemed to start at offset 0...
# Let me check both interpretations

# Interpretation 1: No header, records start at 0, stride 0xF0
print("\n=== Interpretation 1: No header, stride 0xF0 ===")
rec0_count = read_u16(data, 0)
print(f"Record 0 at 0x00: count={rec0_count}")
rec1_count = read_u16(data, 0xF0)
print(f"Record 1 at 0xF0: count={read_u16(data, 0xF0)}")

# Interpretation 2: 16-byte header, records at offset 16, stride 0x78
print("\n=== Interpretation 2: 16-byte header, stride 0x78 ===")
base = 16
for i in range(4):
    off = base + i * 0x78
    if off + 0x78 <= len(data):
        cnt = read_u16(data, off)
        flags = read_u16(data, off + 2)
        val4 = read_u32(data, off + 4)
        ptr10 = read_u64(data, off + 0x10)
        hash68 = read_u64(data, off + 0x68)
        print(
            f"Record {i} at 0x{off:x}: count={cnt}, flags=0x{flags:04x}, val4=0x{val4:08x}, ptr@+0x10=0x{ptr10:x}, hash@+0x68=0x{hash68:016x}"
        )

# Actually, the plan says item_size=0x78 with a 16-byte blob header.
# But record 0 was clearly at offset 0 with count=17. Unless...
# the header IS different. Let me look at offset 0 more carefully.

print("\n=== First 32 bytes of file ===")
for off in range(0, 32, 8):
    val = read_u64(data, off)
    print(f"  +0x{off:02x}: 0x{val:016x}")

# The value at +0x00 was: 0x0005060000010011
# As u16s: 0x0011=17, 0x0001=1, 0x0600=1536, 0x0005=5
# That's: count=17, ???=1, ???=1536, ???=5
# This looks like a property record, not a blob header.

# So either: the MFM blob doesn't have a standard 16-byte header,
# OR the "header" is metadata that happens to look like a record.

# Let me check: if item_size=0x78 and the blob has the standard header:
# Blob 0 header: 0x0005060000010011 as u64 = some number
# As count: that'd be a huge number. Not sensible.
# So the MFM data probably DOES NOT have the standard blob header,
# or it was already stripped when extracted.

# Let me just figure out the stride empirically.
# Record 0 at offset 0 has names_ptr=0x348703.
# The file is 5,299,966 bytes (0x50D77E).
# So the data section (arrays of values) starts somewhere around 0x348703.
# If there are N records of stride S, then N*S ~ 0x348703.
# With S=0x78=120: N = 0x348703/120 = 3,441,411/120 = 28,678 records
# With S=0xF0=240: N = 0x348703/240 = 14,339 records

# Let's check: how many MaterialPrototype records are in the database?
# From the plan: Blob 0 (MaterialPrototype) size=6,767,222B, item_size=0x78
# With 16-byte header: record_count = (6767222-16)/0x78 ... no wait, the blob
# header has u64 count. Let me compute from file size.
# But the extracted file might be the raw blob data.
# File size = 5,299,966. Blob size from plan = 6,767,222.
# These don't match, so the extracted file might be different.

# Let me just scan for the pattern. Each valid sub-record A should have:
# - Reasonable count (0-50)
# - Non-zero pointer at +0x10 (name_ids)
# - Values that look like a hash at +0x68

# Scan at stride 0x78
print("\n=== Scanning at stride 0x78 (first 10 records) ===")
for i in range(10):
    off = i * 0x78
    if off + 0x78 > len(data):
        break
    cnt = read_u16(data, off)
    w1 = read_u16(data, off + 2)
    w2 = read_u16(data, off + 4)
    w3 = read_u16(data, off + 6)
    ptr10 = read_u64(data, off + 0x10)
    hash68 = read_u64(data, off + 0x68)
    # Check if name_ids pointer seems valid (points to data area)
    valid = "OK" if 0x100000 < ptr10 < len(data) else "BAD" if ptr10 > 0 else "NULL"
    print(
        f"  [{i:3d}] @0x{off:06x}: cnt={cnt:3d}, words=({w1},{w2},{w3}), name_ptr=0x{ptr10:06x} [{valid}], hash=0x{hash68:016x}"
    )

# Scan at stride 0xF0
print("\n=== Scanning at stride 0xF0 (first 10 entries, sub-A + sub-B) ===")
for i in range(10):
    offA = i * 0xF0
    offB = offA + 0x78
    if offB + 0x78 > len(data):
        break
    cntA = read_u16(data, offA)
    ptrA = read_u64(data, offA + 0x10)
    hashA = read_u64(data, offA + 0x68)
    validA = "OK" if 0x100000 < ptrA < len(data) else "BAD" if ptrA > 0 else "NULL"

    cntB = read_u16(data, offB)
    ptrB = read_u64(data, offB + 0x10)
    hashB = read_u64(data, offB + 0x68)
    validB = "OK" if 0x100000 < ptrB < len(data) else "BAD" if ptrB > 0 else "NULL"

    print(f"  [{i:3d}] SubA @0x{offA:06x}: cnt={cntA:3d}, ptr=0x{ptrA:06x} [{validA}]")
    print(f"        SubB @0x{offB:06x}: cnt={cntB:3d}, ptr=0x{ptrB:06x} [{validB}]")
