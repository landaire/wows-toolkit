"""Test relative pointer interpretation for MFM records.

In BigWorld's compiled prototype format, pointers are stored as relative offsets
from the pointer's own position in the file. So:
    absolute_offset = pointer_position + stored_value

For record 0 at file offset 0, the pointer at +0x10 stores value V, and
absolute = 0x10 + V. We already know record 0's names_ptr at +0x10 is 0x348703.
That was treated as an absolute offset and it worked.

So either:
A) The values ARE absolute (and record 0 just happens to work because it's first)
B) The values are relative from the field position

Let's test interpretation B for record 0:
  names_ptr stored value = 0x348703 at position 0x10
  If relative: absolute = 0x10 + 0x348703 = 0x348713 -- different from 0x348703!

But 0x348703 worked as absolute... so let's verify by checking if 0x348713
also makes sense, or if the pointer IS actually absolute."""

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

# Record 0: stored names_ptr = 0x348703 at field offset 0x10
# Record 0: type_idx stored = 0x348747 at field offset 0x18

# If absolute: names are at 0x348703 -- this worked!
# If relative from field: names are at 0x10 + 0x348703 = 0x348713
# If relative from record start: names are at 0 + 0x348703 = 0x348703 -- same as absolute for rec 0

# The only way to distinguish is to check record 1
rec1_off = 0x78
stored_ptr = read_u64(data, rec1_off + 0x10)
print(f"Record 1 at 0x{rec1_off:x}:")
print(f"  Stored ptr at +0x10: 0x{stored_ptr:x}")
print(f"  As absolute: 0x{stored_ptr:x}")
print(f"  As rel from field: 0x{rec1_off + 0x10 + stored_ptr:x}")
print(f"  As rel from record: 0x{rec1_off + stored_ptr:x}")

# Record 1 has count=2, stored_ptr was 0x348782
# Let me check what's at each interpretation
for label, addr in [
    ("absolute", stored_ptr),
    ("rel from field", rec1_off + 0x10 + stored_ptr),
    ("rel from record", rec1_off + stored_ptr),
]:
    if addr + 8 <= len(data):
        h0 = read_u32(data, addr)
        h1 = read_u32(data, addr + 4)
        print(f"  {label:20s} @ 0x{addr:x}: 0x{h0:08x} 0x{h1:08x}")

# For record 0 at offset 0, rec0 names_ptr stored = 0x348703
# Record 0 has 17 properties. The names array is u32[17] = 68 bytes.
# After names: type_idx array at 0x348747. 0x348747 - 0x348703 = 0x44 = 68. Correct!
# So for record 0, the stored value IS the absolute offset.

# For record 1, count=2, stored names_ptr = 0x348782
# As absolute: let's read 2 u32 hashes
print(f"\nRecord 1 names as absolute (0x348782):")
for i in range(2):
    h = read_u32(data, 0x348782 + i * 4)
    print(f"  [{i}] 0x{h:08x}")

# Record 1 type_idx_ptr at +0x18
rec1_tidx = read_u64(data, rec1_off + 0x18)
print(f"\nRecord 1 type_idx_ptr stored: 0x{rec1_tidx:x}")
for i in range(2):
    ti = read_u16(data, rec1_tidx + i * 2)
    typ = ti & 0xF
    idx = ti >> 4
    print(f"  [{i}] type={typ}, idx={idx}")

# HMMMM. Record 1 has count=2. If names_ptr=0x348782 (absolute), what hashes do we get?
# From previous output: 0xcd1f228a, 0xec343c5a
# But 0xcd1f228a and 0xec343c5a look like PARTS of record 0's texture values!
# Record 0 tex[0] = 0xec343c5acd1f228a -- that's these two u32s in reverse!
# So 0x348782 points to the texture value array of record 0!

# This means record 1's names_ptr is WRONG when interpreted as absolute.
# Let's check: record 0's texture pointer was at +0x40 (type 4 = texture).
rec0_tex_ptr = read_u64(data, 0x40)
print(f"\nRecord 0 texture array pointer: 0x{rec0_tex_ptr:x}")
print(f"Record 1 names pointer:         0x{stored_ptr:x}")
print(f"Same? {rec0_tex_ptr == stored_ptr}")

# YES! Record 1's names_ptr (0x348782) = Record 0's texture pointer!
# This means the pointers ARE relative, but from what base?

# Record 0 texture pointer at field offset 0x40, stored value 0x348782
# Record 0 texture data verified at absolute 0x348782
# So for record 0: absolute = stored_value (which means base=0)

# Record 1 names_ptr at field offset 0x78+0x10=0x88, stored value 0x348782
# If absolute: 0x348782 (same as rec0's texture array -- WRONG)
# If relative from field: 0x88 + 0x348782 = 0x34880A
# If relative from record: 0x78 + 0x348782 = 0x3487FA

# Let me check 0x34880A and 0x3487FA
print(f"\nChecking rel-from-field 0x34880A:")
for i in range(2):
    h = read_u32(data, 0x34880A + i * 4)
    print(f"  [{i}] 0x{h:08x}")

print(f"\nChecking rel-from-record 0x3487FA:")
for i in range(2):
    h = read_u32(data, 0x3487FA + i * 4)
    print(f"  [{i}] 0x{h:08x}")

# Actually, wait. Let me reconsider. Maybe the pointers in the records
# are NOT from the record start or field position. In BigWorld compiled data,
# relative pointers are typically:
#   absolute_offset_in_blob = stored_value
# Where the blob starts after the 16-byte header.
# But our file doesn't seem to have a header (record 0 starts at byte 0).

# OR: maybe the file DOES have a header that we're misinterpreting.
# The "header" might be: record_count as u32/u64, then records start.
# But record 0's first 8 bytes (0x0005060000010011) decoded perfectly as
# count=17 (u16) + padding.

# Let me think differently. The subagent said the string table was found
# inside assets.bin. The MFM file we have was extracted from assets.bin.
# The blob has a 16-byte header (u64 count + u64 header_size=16).
# Our extracted file might be missing that header!

# If the REAL blob starts 16 bytes before our file, then all pointers
# would need +16 adjustment. But record 0 worked without adjustment...
# Unless the header IS part of our file and we need to account for it.

# Actually: the blob format from the plan says:
# "Data is at: database_blob[blob_index].data + 16 + record_index * item_size"
# So records start at offset 16 in the blob. If our extracted file IS the blob:
# - Bytes 0-7: u64 record count
# - Bytes 8-15: u64 header_size (=16)
# - Byte 16+: first record

# But we verified that record 0 at offset 0 has count=17 which decoded perfectly...
# Unless bytes 0-15 happen to look like a valid record by coincidence.

# Let me check: what if the REAL record 0 starts at offset 16?
print(f"\n=== Testing records starting at offset 16 ===")
for rec in range(5):
    off = 16 + rec * 0x78
    cnt = read_u16(data, off)
    ptr = read_u64(data, off + 0x10)
    tidx = read_u64(data, off + 0x18)
    hash68 = read_u64(data, off + 0x68)
    print(
        f"  Rec {rec} @0x{off:x}: cnt={cnt}, names_ptr=0x{ptr:x}, type_ptr=0x{tidx:x}, hash=0x{hash68:016x}"
    )

    # Try reading hashes at names_ptr (absolute)
    if 0 < ptr < len(data) and cnt > 0 and cnt < 100:
        hashes = [read_u32(data, ptr + i * 4) for i in range(min(cnt, 5))]
        print(f"    Name hashes (abs): {['0x%08x' % h for h in hashes]}")

# ALTERNATIVELY: maybe pointers are relative to the start of the entire blob
# (offset 0 of our file), which means they're absolute file offsets.
# For record 0 this works. For record 1 it gives garbage because
# record 1's pointers happen to alias record 0's value arrays.
# This suggests the file format stores SHARED data at those offsets,
# and record 1 genuinely points to the same name array as some other structure.

# Wait -- what if records DON'T start at offset 0? What if the first record
# IS at some other offset and the data at offset 0 is something else?
# Let me check what the resource-to-prototype map says about where specific
# MFM entries are located.
print(f"\n=== Raw bytes 0-15 ===")
for i in range(16):
    print(f"  [{i:2d}] 0x{data[i]:02x}", end="")
print()
print(f"  As u64[0]: {read_u64(data, 0)} = 0x{read_u64(data, 0):016x}")
print(f"  As u64[1]: {read_u64(data, 8)} = 0x{read_u64(data, 8):016x}")

# u64[0] = 0x0005060000010011 = 1413971953385489
# This is NOT a reasonable record count (too large for 5MB file with 0x78 stride)
# u64[1] = 0x0000000000000500 = 1280
# 1280 is not 16 (expected header_size)

# So this file does NOT have the standard 16-byte blob header.
# It might have been extracted without the header.
# Or the format is different for MaterialPrototype.

# Let me just check: total file size / 0x78 stride
print(f"\nFile size / 0x78 = {len(data) / 0x78:.2f}")
print(f"File size / 0x78 = {len(data) // 0x78} records + {len(data) % 0x78} remainder")

# Hmm. 5299966 / 120 = 44166.38 -- not exact.
# (5299966 - 16) / 120 = 44166.25 -- not exact either.
# Let me check if the file has a different header size.

# Actually, I just realized: the MFM file was extracted from the GAME VFS,
# not from the assets.bin blob! The conversation says "mfm_study" directory.
# This file (LNT016_BTH_01.mfm) is a SEPARATE file, not a blob record.
# The blob is in assets.bin with all MaterialPrototypes together.
# Individual .mfm files ARE in the VFS (IDX/PKG archives).
# So this is a standalone MFM file, not a blob extract!

# That means the file format is different from the blob format.
# It has its own internal structure with absolute pointers.
# Record 0 at offset 0 decoded correctly. But there's only ONE material
# in a single .mfm file! The "records" after 0x78 might be something else
# entirely (sub-structures, technique passes, etc.)

# Let me check how many non-zero 0x78-byte blocks there are
print(f"\n=== Scanning all 0x78-byte blocks for valid-looking records ===")
valid_count = 0
for i in range(len(data) // 0x78):
    off = i * 0x78
    cnt = read_u16(data, off)
    if cnt == 0 or cnt > 100:
        continue
    ptr = read_u64(data, off + 0x10)
    if ptr == 0 or ptr >= len(data):
        continue
    # Check if the pointer target has reasonable u32 values (not all zeros, not all FFs)
    test = read_u32(data, ptr)
    if test == 0 or test == 0xFFFFFFFF:
        continue
    valid_count += 1

print(f"Valid-looking records at stride 0x78: {valid_count}")
print(f"Total 0x78 blocks: {len(data) // 0x78}")
