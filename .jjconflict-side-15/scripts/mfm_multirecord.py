"""Figure out the multi-record structure of .mfm files.

Hypothesis: the file has sections. The header at +0x00 describes section 0.
+0x08 contains an offset (0x500) to the next section or to a table of sections.
Each section is a MaterialPrototype record (0x78 bytes) followed by additional
sub-records that describe shader technique passes or material layers."""

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


# Build name lookup
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

# Header analysis
print("=== File header ===")
print(
    f"+0x00: count={read_u16(data, 0)}, w1={read_u16(data, 2)}, w2={read_u16(data, 4)}, w3={read_u16(data, 6)}"
)
print(f"+0x08: 0x{read_u64(data, 8):016x} = {read_u64(data, 8)}")

# The value at +0x08 is 0x500 = 1280 for both BTH_01 and Dock_01.
# Let me check: how many 0x78-byte records fit between offset 0 and 0x500?
# 0x500 / 0x78 = 1280 / 120 = 10.67 -- not exact
# But 0x500 / 0xF0 = 1280 / 240 = 5.33 -- not exact either

# What if the file structure is:
# [record 0 at +0x00] (0x78 bytes)
# [padding to 0x78]
# [N more records at 0x78, 0xF0, ...]
# [??? at 0x500]

# Let me look at what's at 0x4B0 (= 0x500 - 0x50, just before 0x500)
print(f"\n=== Area around 0x500 ===")
for off in range(0x490, 0x530, 8):
    val = read_u64(data, off)
    nonzero = " <---" if val != 0 else ""
    print(f"  +0x{off:04x}: 0x{val:016x}{nonzero}")

# Actually, let me take a completely different approach.
# The original conversation identified the loadProperties function which processes
# a MaterialPrototype record. The record at +0x00 is the "properties" section.
# But the full MaterialPrototype in assets.bin is 0x78 bytes. So the ENTIRE
# prototype is 0x78 bytes, and all data is referenced via relative pointers
# into the out-of-line section at the end of the blob.
#
# For a standalone .mfm file, the structure might be:
# [MaterialPrototype record] (0x78 bytes)
# [some intermediate data / additional descriptors]
# [out-of-line data area]
#
# The key question is: what fills the space between 0x78 and ~0x348700?
# That's about 3.4MB of SOMETHING.

# Let me look for a pattern. Check every 0x78 offset for records that have
# valid-looking data (small count, reasonable pointers).
print(f"\n=== Valid records with in-range pointers ===")
seen_hashes = set()
for i in range(len(data) // 0x78):
    off = i * 0x78
    cnt = read_u16(data, off)
    if cnt == 0 or cnt > 50:
        continue
    names_ptr = read_u64(data, off + 0x10)
    tidx_ptr = read_u64(data, off + 0x18)
    if names_ptr == 0 or names_ptr >= len(data):
        continue
    if tidx_ptr == 0 or tidx_ptr >= len(data):
        continue
    # Check that names_ptr < tidx_ptr (expected ordering)
    if names_ptr >= tidx_ptr:
        continue
    # Check that tidx_ptr - names_ptr == cnt * 4 (name hash array size)
    expected_gap = cnt * 4
    actual_gap = tidx_ptr - names_ptr
    if actual_gap != expected_gap:
        continue

    rec_hash = read_u64(data, off + 0x68)
    seen_hashes.add(rec_hash)

    # This looks like a genuinely valid record!
    # Read the first few property names
    prop_names = []
    for j in range(min(cnt, 5)):
        h = read_u32(data, names_ptr + j * 4)
        name = KNOWN.get(h, f"0x{h:08x}")
        prop_names.append(name)

    print(
        f"  [{i:5d}] @0x{off:06x}: cnt={cnt:2d}, names@0x{names_ptr:06x}, hash=0x{rec_hash:016x}"
    )
    print(f"          props: {', '.join(prop_names[:5])}{'...' if cnt > 5 else ''}")

print(f"\nTotal valid records found: with proper name/type ptr gap")
print(f"Unique material hashes: {len(seen_hashes)}")
