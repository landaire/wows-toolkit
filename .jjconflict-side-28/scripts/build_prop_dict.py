"""Build property name hash dictionary by scanning assets.bin for property names."""

import struct


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


ASSETS_BIN = r"C:/Users/lander/AppData/Local/Temp/ab_dump/assets.bin"
with open(ASSETS_BIN, "rb") as f:
    data = f.read()

# Step 1: Collect ALL unique property hashes from all 20547 MaterialPrototype records
RECORDS_START = 0x374F8BB
NUM_RECORDS = 20547

all_prop_hashes = set()
for ri in range(NUM_RECORDS):
    off = RECORDS_START + ri * 0x78
    rec_count = struct.unpack_from("<H", data, off)[0]
    if rec_count == 0 or rec_count > 100:
        continue
    names_ptr = struct.unpack_from("<Q", data, off + 0x10)[0]
    if names_ptr == 0:
        continue
    abs_names = off + names_ptr
    if abs_names + rec_count * 4 > len(data):
        continue
    for j in range(rec_count):
        h = struct.unpack_from("<I", data, abs_names + j * 4)[0]
        all_prop_hashes.add(h)

print(f"Total unique property hashes: {len(all_prop_hashes)}")

# Step 2: Extract all null-terminated ASCII strings from the property name region
# We know property names exist around 0x480190. Let's scan a broader region.
# The property name section is within the MaterialPrototype blob data.
# Blob 0 data starts at DATA_START and extends 2,130,677 bytes.
DATA_START = RECORDS_START + NUM_RECORDS * 0x78
DATA_END = DATA_START + 2130677

# But the strings at 0x480190 are BEFORE the blob. Let's scan the whole file
# for regions containing property-name-like strings.
# A simpler approach: extract ALL null-terminated strings of length 2-64 from the file
# that consist only of [a-zA-Z0-9_], and hash them.

hash_dict = {}
pos = 0
file_len = len(data)

# Focus on the region around where we found property names: 0x460000 to 0x490000
# This is much faster than scanning 140MB
scan_ranges = [
    (0x460000, 0x4A0000),  # Region with known property names
    (0x0, 0x100000),  # Early part of file (might have more)
]

for scan_start, scan_end in scan_ranges:
    scan_end = min(scan_end, file_len)
    pos = scan_start
    while pos < scan_end:
        # Find next null byte
        null_pos = data.find(b"\x00", pos, scan_end)
        if null_pos < 0:
            break
        s = data[pos:null_pos]
        pos = null_pos + 1
        if len(s) < 2 or len(s) > 64:
            continue
        # Check if it's a valid property name (alphanumeric + underscore)
        valid = True
        for b in s:
            if not ((65 <= b <= 90) or (97 <= b <= 122) or (48 <= b <= 57) or b == 95):
                valid = False
                break
        if not valid:
            continue
        try:
            name = s.decode("ascii")
        except:
            continue
        h = murmurhash3_32(name)
        if h in all_prop_hashes:
            hash_dict[h] = name

print(f"Matched {len(hash_dict)}/{len(all_prop_hashes)} from scan ranges")

# Also try all the names we already know from the previous session
known_names = [
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
    "alphaTestEnable",
    "alphaReference",
    "selfIllumination",
    "g_useNormalPackDXT1",
    "emissiveMap",
    "metallicGlossMap",
    # Additional common shader property names
    "ambientOcclusionMap",
    "detailMap",
    "g_detailAlbedoInfluence",
    "g_detailFadeDistance",
    "g_detailGlossInfluence",
    "g_detailNormalInfluence",
    "g_detailScaleU",
    "g_detailScaleV",
    "PBS_Misc",
    "PBS_Misc_cl1",
    "EMISSIVE_PBS",
    "EMISSIVE_PBS_cl1",
    "blazeNoiseMap",
    "g_detailScale",
    "imageTexture",
    "maskTexture",
    "glitchLineOffset",
    "glitchLinePeriod",
    "glitchLineWidth",
    "glowStrength",
    "shakeFactor",
    "waveScaleX",
    "waveSpeedX",
    "waveSpeedY",
    "emissivePower",
]
for name in known_names:
    h = murmurhash3_32(name)
    if h in all_prop_hashes and h not in hash_dict:
        hash_dict[h] = name

print(f"After adding known names: {len(hash_dict)}/{len(all_prop_hashes)}")

# Show all matches
print("\n=== Complete Property Name Dictionary ===")
for h in sorted(hash_dict.keys()):
    print(f"  0x{h:08x} = {hash_dict[h]}")

# Show unmatched
unmatched = sorted(all_prop_hashes - set(hash_dict.keys()))
print(f"\nUnmatched ({len(unmatched)}):")
for h in unmatched:
    print(f"  0x{h:08x}")
