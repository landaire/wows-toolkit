"""Determine what fills the .mfm file beyond the single 0x78 record.

The MaterialPrototype record is 0x78 bytes. In assets.bin, multiple records are packed
together with shared out-of-line data. In a standalone .mfm, there's ONE record.

But the file is 5.3MB. The record's out-of-line data is only ~250 bytes at offset 0x348700+.
What fills the space from 0x78 to 0x348700?

Key clues:
- +0x08: 0x0500 (1280) - could be a count or offset
- +0x04: 0x0600 (1536) and +0x06: 0x0005 (5) - could be metadata
- The area at 0x500 had record-like structures

Maybe the file contains MULTIPLE records: the main material + technique/pass records.
But they'd need different base offsets for their pointers since they're at different
file positions.

WAIT - maybe ALL the records share the SAME out-of-line data section, and their
pointers are absolute from file start. Record 0's pointers ARE absolute (they work).
Record 1 at 0x78 has pointers that alias record 0's texture array - that's WRONG
for a property name array but it's a VALID pointer into the file!

What if record 1 is a DIFFERENT type of record that has a different field layout?
The +0x10 field in record 1 might not be 'name_ids' at all."""

import struct


def read_u16(data, off):
    return struct.unpack_from("<H", data, off)[0]


def read_u32(data, off):
    return struct.unpack_from("<I", data, off)[0]


def read_u64(data, off):
    return struct.unpack_from("<Q", data, off)[0]


def read_f32(data, off):
    return struct.unpack_from("<f", data, off)[0]


def hexdump(data, off, size=0x78):
    for i in range(0, min(size, len(data) - off), 16):
        hex_str = " ".join(f"{data[off + i + j]:02x}" for j in range(min(16, size - i)))
        ascii_str = "".join(
            chr(data[off + i + j]) if 32 <= data[off + i + j] < 127 else "."
            for j in range(min(16, size - i))
        )
        print(f"  {off + i:06x}: {hex_str:<48s} {ascii_str}")


MFM_PATH = r"C:/Users/lander/AppData/Local/Temp/mfm_study/content/location/nature/tile/textures/LNT016_BTH_01.mfm"
with open(MFM_PATH, "rb") as f:
    data = f.read()

# Let's look at this from the ASSETS.BIN perspective.
# The plan says MaterialPrototype item_size = 0x78, registered at sub_140026de0.
# Each record in the blob is 0x78 bytes. The out-of-line data is stored AFTER
# all records in the blob.
#
# For a standalone .mfm file, the record + its data is self-contained.
# But the file might actually be the ENTIRE assets.bin blob for MaterialPrototype!
# Let's check: blob size from plan = 6,767,222 bytes.
# File size = 5,299,966 bytes. These don't match.
#
# OR: the .mfm file might be extracted from the VFS (IDX/PKG) as a separate file
# that has its OWN format, different from the blob. The VFS .mfm might be an
# uncompiled XML or compiled binary with a different structure.

# Let me look for structure markers. First, search for non-zero regions.
print("=== Non-zero region scan ===")
BLOCK = 256
regions = []
in_region = False
region_start = 0
for i in range(0, len(data), BLOCK):
    block = data[i : i + BLOCK]
    has_data = any(b != 0 for b in block)
    if has_data and not in_region:
        region_start = i
        in_region = True
    elif not has_data and in_region:
        regions.append((region_start, i))
        in_region = False
if in_region:
    regions.append((region_start, len(data)))

print(f"Found {len(regions)} non-zero regions:")
for start, end in regions[:20]:
    print(f"  0x{start:06x} - 0x{end:06x} ({end - start:,} bytes)")
if len(regions) > 20:
    print(f"  ... and {len(regions) - 20} more")
    for start, end in regions[-5:]:
        print(f"  0x{start:06x} - 0x{end:06x} ({end - start:,} bytes)")

# Also check if the file contains other MFM paths as strings
print("\n=== Searching for embedded strings ===")
import re

# Find ASCII strings of length >= 4
strings_found = []
for m in re.finditer(rb"[\x20-\x7e]{6,}", data[:0x1000]):
    strings_found.append((m.start(), m.group().decode("ascii")))
if strings_found:
    for off, s in strings_found[:20]:
        print(f"  @0x{off:06x}: {s}")
else:
    print("  No ASCII strings found in first 4KB")

# Let me look at the file as if the first 0x78 bytes are the MaterialPrototype,
# and the ENTIRE rest is the "out-of-line blob" containing technique/pass data.
# The header word at +0x02 was 0x0001 for BTH_01. Maybe this means "1 sub-material"
# or "1 technique variant". The Dock_01 had 0x0002 (2).
# +0x04 (0x0600 = 1536) and +0x06 (0x0005 = 5) might be shader identifier/flags.
# +0x08 (0x0500 = 1280) might be number of technique pass records or an offset.

# Actually, let me think about it differently. The words at offset 0:
# u16 count=17, u16 w1=1, u16 w2=0x600, u16 w3=5
# What if these are SEPARATE from the property count?
# What if offset 0 is: {u16 prop_count=17, u8 flags=1, u8 type=0}?
# Or: {u32 field1=0x00010011, u32 field2=0x00050600}?

# Let me compare with the Dock file
DOCK_PATH = r"C:/Users/lander/AppData/Local/Temp/mfm_study/content/location/nature/tile/textures/LNT000_Dock_01.mfm"
with open(DOCK_PATH, "rb") as f:
    dock = f.read()

print(f"\n=== Comparing headers ===")
print(f"BTH_01: {' '.join(f'{data[i]:02x}' for i in range(16))}")
print(f"Dock01: {' '.join(f'{dock[i]:02x}' for i in range(16))}")

# BTH_01: 11 00 01 00 00 06 05 00 00 05 00 00 00 00 00 00
# Dock_01: 12 00 02 00 00 06 05 00 00 05 00 00 00 00 00 00

# Differences: byte 0 (0x11 vs 0x12 = prop count 17 vs 18), byte 2 (0x01 vs 0x02)
# Byte 2 = some count. Maybe number of materials/layers?
# Bytes 4-7: identical (0x00 0x06 0x05 0x00)
# Bytes 8-11: identical (0x00 0x05 0x00 0x00 = 0x500)

# So the structure looks like:
# +0x00: u16 property_count
# +0x02: u16 material_layers_count (1 for BTH_01, 2 for Dock_01)
# +0x04: u32 shader_info (0x00050600) -- possibly shader ID or flags
# +0x08: u64 offset_to_??? (0x500)

# If byte 2 is a "layers count" = 1, then maybe the entire file is:
# [MaterialPrototype record 0x78]  (the base material properties)
# [N embedded objects starting at some offset]
# [out-of-line data]

# The value 0x500 = 1280. Let me check if there's exactly 1280 bytes of
# embedded data starting at offset 0x78.
# 0x78 + 1280 = 0x78 + 0x500 = 0x578
# Or maybe 0x500 is an absolute offset to some section.

# Let me look at the boundary: what's at offset 0x78 and at offset 0x500?
print(f"\n=== Hexdump at 0x78 (after record 0) ===")
hexdump(data, 0x78, 0x40)

print(f"\n=== Hexdump at 0x500 ===")
hexdump(data, 0x500, 0x80)

# Let me also look at what PRECEDES the data section (around 0x348700)
print(f"\n=== Hexdump at 0x348680 (before data section) ===")
hexdump(data, 0x348680, 0xA0)

# And the very end of the file
print(f"\n=== Last 0x80 bytes of file ===")
hexdump(data, len(data) - 0x80, 0x80)
