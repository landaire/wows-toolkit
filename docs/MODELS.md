# WoWs .geometry File Format

Reverse-engineered from `WorldOfWarships64.exe` using Binary Ninja.

The `.geometry` file is a **BigWorld/Wargaming Moo engine** binary format that stores
merged 3D mesh data: vertex buffers, index buffers, and associated metadata. The file
is designed to be memory-mapped; internal pointers are stored as **relative offsets**
that are resolved at load time.

## Pointer Convention

All pointer fields are stored as **`i64` relative offsets**. Resolution depends on
context:

- **Header-level** fields: resolved as `struct_base + value`. Since the header is at
  file offset 0, these are effectively absolute file offsets.
- **Sub-struct** fields: resolved as `sub_struct_base + value`.
- **PackedString** text pointers: resolved as `packed_string_base + value`.

A value of `0` represents a null pointer.

## Top-Level Structure: `MergedGeometryPrototype`

```
Offset  Size  Type   Field
------  ----  ----   -----
0x00    4     u32    mergedVerticesCount    # number of VerticesPrototype entries
0x04    4     u32    mergedIndicesCount     # number of IndicesPrototype entries
0x08    4     u32    verticesMappingCount   # number of MappingEntry entries (vertex)
0x0C    4     u32    indicesMappingCount    # number of MappingEntry entries (index)
0x10    4     u32    collisionModelCount    # number of CollisionModelPrototype entries
0x14    4     u32    armorModelCount        # number of ArmorModelPrototype entries
0x18    8     i64    verticesMappingPtr     # -> MappingEntry[] (relative to file start)
0x20    8     i64    indicesMappingPtr      # -> MappingEntry[] (relative to file start)
0x28    8     i64    mergedVerticesPtr      # -> VerticesPrototype[] (relative to file start)
0x30    8     i64    mergedIndicesPtr       # -> IndicesPrototype[] (relative to file start)
0x38    8     i64    collisionModelsPtr     # -> CollisionModelPrototype[] (relative to file start)
0x40    8     i64    armorModelsPtr         # -> ArmorModelPrototype[] (relative to file start)
```

Total header size: **0x48 (72) bytes**.

## MappingEntry (0x10 bytes each)

Maps a named resource (identified by hash) to a slice of a merged vertex/index buffer.

```
Offset  Size  Type   Field
------  ----  ----   -----
0x00    4     u32    mappingId            # hash identifier for this render group
0x04    2     u16    mergedBufferIndex    # which merged buffer this maps to
0x06    2     u16    packedTexelDensity   # encoded texel density value
0x08    4     u32    itemsOffset          # start offset (in items) within the merged buffer
0x0C    4     u32    itemsCount           # number of items (vertices or indices)
```

## PackedString

A variable-length string stored as a counted reference.

```
Offset  Size  Type   Field
------  ----  ----   -----
0x00    4     u32    charCount      # number of characters (including null terminator)
0x04    4     ---    (padding)
0x08    8     i64    textPtr        # relative to this struct's base -> char[]
```

Total struct size: **0x10 (16) bytes**. The text data is stored out-of-line, typically
after the associated data blob.

## VerticesPrototype (0x20 bytes each)

Describes a merged vertex buffer.

```
Offset  Size  Type         Field
------  ----  ----         -----
0x00    8     i64          verticesDataPtr    # relative to this struct -> raw data blob
0x08    16    PackedString formatName         # e.g. "set3/xyznuvtbpc"
0x18    4     u32          sizeInBytes        # total byte size of the data blob
0x1C    2     u16          strideInBytes      # per-vertex stride (e.g. 28, 32)
0x1E    1     u8           isSkinned          # 1 if skinned mesh
0x1F    1     u8           isBumped           # 1 if bump-mapped
```

### Vertex Data Blob

The data pointed to by `verticesDataPtr` may be either:

1. **ENCD-encoded** (compressed): starts with magic `0x44434E45` (`"ENCD"` in ASCII).
   Uses [meshoptimizer](https://github.com/zeux/meshoptimizer) vertex buffer encoding.
2. **Raw**: uncompressed vertex data, `sizeInBytes` total.

#### ENCD Header (8 bytes)

```
Offset  Size  Type   Field
------  ----  ----   -----
0x00    4     u32    magic          # 0x44434E45 = "ENCD"
0x04    4     u32    elementCount   # number of vertices/indices
```

Followed by the meshoptimizer-encoded payload. Decode with:
```
meshopt_decodeVertexBuffer(output, elementCount, strideInBytes,
                           encoded_data + 8, sizeInBytes - 8)
```

### Vertex Format Names

The `formatName` string encodes the vertex attribute layout. Known format:
`"set3/xyznuvtbpc"` where each group of characters after the `/` describes
vertex components. Format strings use the BigWorld vertex declaration naming
convention.

## IndicesPrototype (0x10 bytes each)

Describes a merged index buffer.

```
Offset  Size  Type   Field
------  ----  ----   -----
0x00    8     i64    indicesDataPtr   # relative to this struct -> raw data blob
0x08    4     u32    sizeInBytes      # total byte size of the index data blob
0x0C    2     u16    (reserved)
0x0E    2     u16    indexSize        # bytes per index: 2 = u16, 4 = u32
```

The index data blob follows the same ENCD encoding scheme as vertex data.
Decode with:
```
meshopt_decodeIndexBuffer(output, elementCount, indexSize,
                          encoded_data + 8, sizeInBytes - 8)
```

Where `elementCount = EncodedBufferHeader.elementCount` from the ENCD header.

## CollisionModelPrototype (0x20 bytes each)

```
Offset  Size  Type         Field
------  ----  ----         -----
0x00    8     i64          cmDataPtr            # relative to this struct -> raw data blob
0x08    16    PackedString collisionModelName   # e.g. "CM_something"
0x18    4     u32          sizeInBytes          # total byte size
0x1C    4     ---          (padding)
```

## ArmorModelPrototype (0x20 bytes each)

Same layout as CollisionModelPrototype:

```
Offset  Size  Type         Field
------  ----  ----         -----
0x00    8     i64          armorDataPtr       # relative to this struct -> raw data blob
0x08    16    PackedString armorModelName     # e.g. "CM_PA_united.armor"
0x18    4     u32          sizeInBytes        # total byte size
0x1C    4     ---          (padding)
```

## File Layout Example

For a typical ship model (`BSA013_Colossus_1945.geometry`, 192,311 bytes):

```
0x00000-0x00047  Header (72 bytes)
0x00048-0x00067  verticesMapping[2] (32 bytes)
0x00068-0x00087  indicesMapping[2] (32 bytes)
0x00088-0x000A7  VerticesPrototype[1] (32 bytes)
0x000A8-0x0119E  vertexData[0] blob (4343 bytes, ENCD-encoded)
0x0119F-0x011AE  formatName[0] text (16 bytes: "set3/xyznuvtbpc\0")
0x011AF-0x011BE  IndicesPrototype[1] (16 bytes)
0x011BF-0x012C3  indexData[0] blob (261 bytes, ENCD-encoded)
0x012C4-0x012E3  ArmorModelPrototype[1] (32 bytes)
0x012E4-0x2EF03  (unmapped region - 187,424 bytes)
0x2EF04-0x2EF23  armorData[0] blob (32 bytes)
0x2EF24-0x2EF36  armorModelName[0] text (19 bytes: "CM_PA_united.armor\0")
```

The large unmapped region likely contains additional mesh data (primitive groups,
bounding boxes, etc.) referenced by the `.visual` file system rather than the
`.geometry` header.

## Binary Ninja Annotations

The following functions and types have been annotated in the Binary Ninja database:

### Functions
| Address        | Name                                    | Purpose                              |
|----------------|-----------------------------------------|--------------------------------------|
| `0x140483660`  | `MergedGeometryPrototype_deserialize`   | Deserializes the top-level structure |
| `0x1404841e0`  | `VerticesPrototype_deserialize`         | Deserializes vertex buffer metadata  |
| `0x140484590`  | `IndicesPrototype_deserialize`          | Deserializes index buffer metadata   |
| `0x1404847c0`  | `CollisionModelPrototype_deserialize`   | Deserializes collision model metadata|
| `0x140484a00`  | `ArmorModelPrototype_deserialize`       | Deserializes armor model metadata    |
| `0x140483dc0`  | `MappingArray_deserialize`              | Deserializes mapping arrays          |
| `0x140483f40`  | `MappingEntry_deserialize`              | Deserializes individual mapping entry|
| `0x140484c40`  | `PackedString_deserialize`              | Deserializes packed string structs   |
| `0x140456c50`  | `Moo_Vertices_loadFromPrototype`        | Loads vertex buffer from prototype   |
| `0x140457390`  | `Moo_Primitive_loadFromPrototype`       | Loads index buffer from prototype    |
| `0x140459240`  | `Moo_GeometryManager_createManagedObjects` | Creates GPU resources from prototypes |
| `0x14047f590`  | `Moo_GeometryData_fetchVertices`        | Fetches vertices by mapping ID       |
| `0x140a5a940`  | `MeshDataOptimizer_decodeVertexData`    | Decodes ENCD vertex data             |
| `0x140a5ab20`  | `MeshDataOptimizer_decodeIndexData_u16` | Decodes ENCD index data (u16)        |
| `0x140a5ad00`  | `MeshDataOptimizer_decodeIndexData_u32` | Decodes ENCD index data (u32)        |
| `0x140a5a880`  | `EncodedBufferHeader_checkHeaderData`   | Validates ENCD magic/count           |
| `0x1413fa420`  | `meshopt_decodeVertexBuffer`            | meshoptimizer vertex decode          |
| `0x1413fa610`  | `meshopt_decodeIndexBuffer`             | meshoptimizer index decode           |

### Source Paths (from debug strings)
- `D:\Source\Build\SOURCE\WOWS_GIT_SPARSE\client\source\lib\moo\vertices.cpp`
- `D:\Source\Build\SOURCE\WOWS_GIT_SPARSE\client\source\lib\moo\primitive.cpp`
- `D:\Source\Build\SOURCE\WOWS_GIT_SPARSE\client\source\lib\moo\geometry_data.cpp`
- `D:\Source\Build\SOURCE\WOWS_GIT_SPARSE\client\source\lib\moo\geometry_manager.cpp`
- `D:\Source\Build\SOURCE\WOWS_GIT_SPARSE\client\source\lib\mesh_data_optimizer\coder.cpp`

---

# WoWs `assets.bin` File Format (PrototypeDatabase)

Reverse-engineered from `WorldOfWarships64.exe` using Binary Ninja.

The `assets.bin` file (located at `res/content/assets.bin`) is a **BigWorld engine
PrototypeDatabase** binary format. It serves as the master asset index, mapping
resource identifiers to prototype data blobs. The file is designed for memory-mapping
with relative pointers resolved at load time.

Source file: `D:\Source\Build\SOURCE\WOWS_GIT_SPARSE\client\source\lib\resmgr\resmgr_prototype_database.cpp`

## Pointer Convention

All pointer fields are stored as **`i64` relative offsets**. Each offset is resolved
relative to the **start of its containing structure** (`arg1[2]` in the deserialization
code). Specifically:

- **Body-level** fields (strings, databases count/relptr): resolved as `body_base + value`
  where `body_base` is the first byte after the 16-byte header.
- **Sub-section** fields (resourceToPrototypeMap, pathsStorage): resolved as
  `section_base + value` where `section_base = body_base + section_offset`.
- **Entry-level** fields (database data relptr, path name relptr): resolved as
  `entry_base + value`.

A value of `0` represents a null pointer.

## Header (16 bytes)

```
Offset  Size  Type   Field
------  ----  ----   -----
0x00    4     u32    magic           # 0x42574442 = "BWDB" (BigWorld DataBase)
0x04    4     u32    version         # 0x01010000
0x08    4     u32    checksum        # CRC32 of the body (everything after header)
0x0C    2     u16    architecture    # 0x0040 = 64-bit
0x0E    2     u16    endianness      # 0x0000 = little-endian
```

## Body Header (0x60 = 96 bytes, starting at file offset 0x10)

The body contains five logical sections packed into a 96-byte header:

```
Offset  Size  Type   Field                              Section
------  ----  ----   -----                              -------
+0x00   4     u32    offsetsMap.capacity                 strings
+0x04   4     ---    (padding)
+0x08   8     i64    offsetsMap.buckets_relptr           strings (rel. to body_base)
+0x10   8     i64    offsetsMap.values_relptr            strings (rel. to body_base)
+0x18   4     u32    stringData.size                     strings
+0x1C   4     ---    (padding)
+0x20   8     i64    stringData.relptr                   strings (rel. to body_base)
+0x28   4     u32    resourceToPrototypeMap.capacity     r2p
+0x2C   4     ---    (padding)
+0x30   8     i64    resourceToPrototypeMap.buckets_relptr  r2p (rel. to body_base+0x28)
+0x38   8     i64    resourceToPrototypeMap.values_relptr   r2p (rel. to body_base+0x28)
+0x40   4     u32    pathsStorage.count                  paths
+0x44   4     ---    (padding)
+0x48   8     i64    pathsStorage.data_relptr            paths (rel. to body_base+0x40)
+0x50   4     u32    databasesCount                      databases
+0x54   4     ---    (padding)
+0x58   8     i64    databases.relptr                    databases (rel. to body_base)
```

## Strings Section (offsetsMap + string data)

A hashmap-based string deduplication table. The `offsetsMap` maps string content
hashes to offsets within the `stringData` byte array.

### OffsetsMap Hashmap

Uses open addressing with linear probing. Slot = `name_id % capacity`.

- **capacity**: Number of hash buckets
- **buckets**: Array of `capacity` entries, each 8 bytes: `(u32 key, u32 sentinel)`.
  - `key`: The 32-bit string name hash (MurmurHash3). 0 when slot is empty.
  - `sentinel`: Has bit 31 set (0x80000000+) when occupied. 0 when empty.
- **values**: Array of `capacity` entries, each 4 bytes (`u32`).
  Contains offsets into the string data array.

Prototype records use `u32` name IDs (e.g. `nameId`, `materialNameId` in RenderSet)
that are looked up through this hashmap to get the string data offset.

### String Data

A contiguous pool of null-terminated UTF-8 strings. Strings are referenced by
offset into this pool. Typical content includes vertex format names, material
names, and other text identifiers.

## ResourceToPrototypeMap

A hashmap mapping resource IDs (64-bit hashes) to prototype locations.
Uses open addressing with linear probing. Slot = `selfId % capacity`.

- **capacity**: Number of hash buckets
- **buckets**: Array of `capacity` entries, each **16 bytes**.
  - Bytes 0-7 (`u64`): The key (`selfId` from pathsStorage)
  - Bytes 8-15 (`u64`): Occupancy sentinel (1 = occupied, 0 = empty)
- **values**: Array of `capacity` entries, each 4 bytes (`u32`).
  Encoded prototype location:
  ```
  value = (record_index << 8) | (blob_index * 4)
  ```
  - Low byte (`value & 0xFF`): `blob_index * 4` (type tag)
  - Upper 24 bits (`value >> 8`): record index within that database blob

## PathsStorage

An array of path metadata entries. Each entry associates a unique resource ID with
a parent ID and a display name.

### PathEntry (32 bytes each)

```
Offset  Size  Type   Field
------  ----  ----   -----
0x00    8     u64    selfId          # unique resource identifier (hash)
0x08    8     u64    parentId        # parent resource identifier (hash or index)
0x10    4     u32    name.size       # length of name string (including null terminator)
0x14    4     ---    (padding)
0x18    8     i64    name.data_relptr  # relative to entry_base + 0x10 -> char[]
```

The name strings are stored in a separate contiguous pool located between the
pathsStorage entries and the database entries. Typical names include:
`"OGB202_Dunkirk_dead.model"`, `"JSB023_Izumo_1945.visual"`, etc.

## Database Entries

An array of `databasesCount` database descriptors, each 0x18 (24) bytes:

```
Offset  Size  Type   Field
------  ----  ----   -----
0x00    4     u32    prototypeMagic      # prototype type hash (validated at load time)
0x04    4     u32    prototypeChecksum   # prototype checksum (validated at load time)
0x08    4     u32    size                # size of the data blob in bytes
0x0C    4     ---    (padding)
0x10    8     i64    data_relptr         # relative to entry_base -> u8[] data blob
```

The data blobs are contiguous and collectively consume the remainder of the file.
Each database represents a different prototype type (e.g., visual, model, geometry,
skeleton, material). The `prototypeMagic` and `prototypeChecksum` values are
validated against a static table during loading.

## Prototype Types

The PrototypeDatabase contains 10 registered prototype types. Each type has a
magic value (MurmurHash3_x86_32 of the type name string), a fixed item size,
and a corresponding database blob.

| Idx | Type Name                  | Magic      | Item Size   | Registration Fn  |
|-----|----------------------------|------------|-------------|------------------|
| 0   | MaterialPrototype          | 0x5069C471 | 0x78 (120B) | sub_140026de0    |
| 1   | VisualPrototype            | 0x480DC57B | 0x70 (112B) | sub_140026f40    |
| 2   | SkeletonExtenderPrototype  | 0x1AE023FF | 0x20 (32B)  | sub_140035cb0    |
| 3   | ModelPrototype             | 0xA9576F28 | 0x28 (40B)  | sub_140035b20    |
| 4   | PointLightPrototype        | 0x0D3665A4 | 0x70 (112B) | sub_1400658e0    |
| 5   | EffectPrototype            | 0xEB23E0AF | 0x10 (16B)  | sub_140033cc0    |
| 6   | VelocityFieldPrototype     | 0xAFD4A63F | 0x18 (24B)  | sub_140034190    |
| 7   | EffectPresetPrototype      | 0x42E15336 | 0x10 (16B)  | sub_140033e50    |
| 8   | EffectMetadataPrototype    | 0xDFC8F8E0 | 0x10 (16B)  | sub_140033b30    |
| 9   | AtlasContourProto          | 0xF64359AA | 0x10 (16B)  | sub_140033fb0    |

### Database Blob Structure

Each blob has a 16-byte header followed by fixed-size records and out-of-line data:

```
Offset  Size          Content
------  ----          -------
+0x00   8             count (u64 — number of records)
+0x08   8             header_size (u64 — always 16)
+0x10   count*item    Fixed-size records (item_size bytes each)
+...    remainder     Out-of-line (OOL) data: variable-length arrays, strings
```

Relative pointers (i64) in records point into the OOL region. The base for
resolving relptrs is always the start of the containing struct:
- Top-level record fields: base = record start
- Sub-struct fields (e.g. RenderSet, LOD): base = sub-struct start

## File Layout Example

For a typical `assets.bin` (170,699,420 bytes):

```
0x00000000 - 0x00000010          16 bytes  Header (BWDB magic, version, checksum, arch)
0x00000010 - 0x00000070          96 bytes  Body Header (section descriptors)
0x00000070 - 0x00600078   6,291,464 bytes  offsetsMap.buckets (786,433 x 8)
0x00600078 - 0x0090007C   3,145,732 bytes  offsetsMap.values (786,433 x 4)
0x0090007C - 0x010507A4   7,669,544 bytes  strings.data (null-terminated string pool)
0x010507A4 - 0x01650934   6,291,856 bytes  r2p.buckets (393,241 x 16)
0x01650934 - 0x017D0998   1,572,964 bytes  r2p.values (393,241 x 4)
0x017D0998 - 0x01F52FB8   7,874,080 bytes  pathsStorage entries (246,065 x 32)
0x01F52FB8 - 0x0260379E   7,014,374 bytes  path name strings pool
0x0260379E - 0x0260388E         240 bytes  database entries (10 x 24)
0x0260388E - 0x0A2CAA9C 131,408,654 bytes  database data blobs (10 databases)
```

No gaps or overlaps; every byte is accounted for.

## Binary Ninja Annotations

### Functions
| Address        | Name                                              | Purpose                                |
|----------------|---------------------------------------------------|----------------------------------------|
| `0x140a15980`  | `PrototypeDatabase_load`                          | Loads and validates BWDB file          |
| `0x140a16210`  | `PrototypeDatabase_initStaticDatabase`            | Initializes static type registry       |
| `0x140a178c0`  | `PrototypeDatabase_deserialize`                   | Top-level body deserialization         |
| `0x140a17c50`  | `PrototypeDatabase_deserialize_strings`            | Deserializes strings section           |
| `0x140a17ec0`  | `PrototypeDatabase_deserialize_resourceToPrototypeMap` | Deserializes r2p hashmap         |
| `0x140a18180`  | `PrototypeDatabase_deserialize_pathsStorage`       | Deserializes path entries array        |
| `0x140a18380`  | `PrototypeDatabase_deserialize_database`           | Deserializes a single database entry   |
| `0x140a18660`  | `PrototypeDatabase_deserialize_offsetsMap`         | Deserializes offsetsMap hashmap        |
| `0x140a18930`  | `PrototypeDatabase_deserialize_pathEntry`          | Deserializes a single path entry       |
| `0x140a18ae0`  | `PrototypeDatabase_deserialize_packedString`       | Deserializes packed string struct      |

### Source Paths (from debug strings)
- `D:\Source\Build\SOURCE\WOWS_GIT_SPARSE\client\source\lib\resmgr\resmgr_prototype_database.cpp`

---

## Dependencies

- **meshoptimizer** (`meshopt-rs` crate): Required for decoding ENCD-compressed vertex
  and index buffers.
- **winnow**: Used for binary parsing in the Rust implementation.

---

# ModelPrototype Format (blob index 3)

Reverse-engineered from `WorldOfWarships64.exe` using Binary Ninja.

ModelPrototype wraps a VisualPrototype, adding skeleton extensions, animations,
and dye/tint data. The `.model` file in BigWorld traditionally references a
`.visual` -- this is how that reference is stored in `assets.bin`.

- **Magic:** `0xA9576F28` (MurmurHash3_x86_32 of `"ModelPrototype"`)
- **Item size:** `0x28` (40 bytes)
- **Blob index:** 3
- **Registration function:** `sub_140035b20`

## ModelPrototype Record (0x28 bytes)

```
Offset  Size  Type   Field
------  ----  ----   -----
0x00    8     u64    visualResourceId     # selfId (path hash) of the .visual in pathsStorage
0x08    1     u8     skelExtResIdCount    # number of skeleton extension resource IDs
0x09    1     u8     miscType             # purpose unknown (seen as 0 or small values)
0x0A    1     u8     animationsCount      # number of animation entries
0x0B    1     u8     dyesCount            # number of dye entries
0x0C    4     ---    (padding/reserved)
0x10    8     i64    skelExtResIdsPtr     # relptr -> u64[] (skeleton extension resource IDs)
0x18    8     i64    animationsPtr        # relptr -> AnimationEntry[] (0x28 bytes each)
0x20    8     i64    dyesPtr              # relptr -> DyeEntry[] (0x20 bytes each)
```

### Resolving the Visual

`visualResourceId` is a `selfId` (64-bit path hash) that can be looked up in the
`pathsStorage` section of `assets.bin` to find the corresponding `.visual` file path.
This is more reliable than string-replacing `.model` with `.visual` in the file path.

### AnimationEntry (0x28 bytes each)

Same layout as a ModelPrototype record -- contains its own `visualResourceId`,
skeleton extensions, animations (recursive), and dyes. Used for animation
overlays.

### DyeEntry (0x20 bytes)

Material dye/tint replacement data for ship camouflage and cosmetic systems.

```
Offset  Size  Type   Field
------  ----  ----   -----
0x00    4     u32    matterId             # string ID of the target material name
0x04    4     u32    replacesId           # string ID of the replacement material name
0x08    4     i32    tintsCount           # number of tint entries
0x0C    4     ---    (padding)
0x10    8     i64    tintNameIdsPtr       # relptr -> u32[] (string IDs of tint names)
0x18    8     i64    tintMaterialIdsPtr   # relptr -> u64[] (selfIds of tint .mfm materials)
```

### Binary Ninja Functions

| Address         | Name / Purpose                                       |
|-----------------|------------------------------------------------------|
| `sub_140035b20` | Registration (sets blob_index=3, size=0x28)          |
| `sub_1407f32c0` | Per-item deserialization (reads all fields)           |
| `sub_1407f0080` | `createStaticArray` -- deserializes record array    |
| `sub_1407ef780` | `dumpDynamicArray` -- serializes record array       |
| `sub_1407f0400` | Destructor -- frees skelExtResIds, animations, dyes  |
| `sub_1407f4660` | DyeEntry deserialization                             |
| `sub_1407f3860` | skelExtResIds array deserialization                   |


---

# Armor System Reverse Engineering

## Collision Material Name Table

**Location in game binary**: `py_collisionMaterialName` function at `sub_140363ba0`.
The table is a contiguous array of `char*` pointers at `0x142a569a0`, 8 bytes per entry
(pointer + `0x01000000` tag). 255 entries total (IDs 0–254).

**How it works** (from decompiled code):
- Material ID is a `u8` extracted from armor BVH node headers (byte 0 of the first
  16-byte entry of each BVH node group).
- The function checks `id < 0xFF`, then indexes `table[id]` to get the string pointer.
- If the pointer is null or id >= 255, it falls back to `sprintf("%d", id)`.
- The function is a Python-exposed builtin (`py_collisionMaterialName`) returning a
  Python string object.

**Source path**: `D:\Source\Build\SOURCE\WOWS_GIT_SPARSE\wows\source\lib\lesta\script_junk.cpp`

**Table structure**: The table is NOT sorted alphabetically — entries are grouped
roughly by function:
- 0–1: generic (`common`, `zero`)
- 2–31: `Dual_` zone boundary materials + `Bottom` (ID 12), `Cas_Inclin` (19), `SSC_Inclin` (20)
- 32–45: turret/artillery components (`TurretSide/Top/Front/Aft`, `Art*`, `AuTurret*`)
- 46–51: `Bow_*`
- 52–54: `Bridge*`
- 55–58: `Cas_*`
- 59–67: `Cit_*`
- 68–70: `Dual_Cit_Cas_*`
- 71–79: misc hull (`Bow_Fdck`, `St_Fdck`, `Kdp*`, `OCit_*`)
- 80–83: `Rudder*`
- 84–90: `SSC_*`, `SS_*`
- 91–96: `St_*`
- 97–100: generic turret (`TurretBarbette`, `TurretDown`, `TurretFwd`)
- 101–106: generic hull (`Bulge`, `Trans`, `Deck`, `Belt`, `Inclin`) + `Dual_Cit_SSC_Bulge`
- 107–110: `SS_Bridge*`, `Cas_Bottom`
- 111–133: zone sub-face materials (`SideCit`, `DeckCit`, ..., `TransSS`)
- 134–153: `Tur1GkBar` through `Tur20GkBar` (turret barbettes)
- 154–173: more `Dual_` transitions + `Dual_Cit_Bow/St_Bottom`
- 174–193: `Tur1GkDown` through `Tur20GkDown` (turret undersides)
- 194–213: `Dual_` same-zone pairs + cross-zone `ArtDeck`/`Side` combos
- 214–233: `Tur1GkTop` through `Tur20GkTop` (turret tops)
- 234–241: hangar/forecastle (`Cas_Hang`, `Cas_Fdck`, `SSC_Fdck`, `SSC_Hang`,
  `SS_SGBarbette`, `SS_SGDown`, `SGBarbetteSS`, `SGDownSS`)
- 242–254: `Dual_Cit_Cas/SSC/Bow` deck/inclin/trans

**IMPORTANT**: This table was completely restructured compared to an earlier game
version. The entire table must be treated as version-dependent.

## Armor Geometry (BVH Format)

Armor data in each `ArmorModelPrototype` is a BVH tree with interleaved triangle soup.
Data starts right after the struct (`struct_base + 0x20`) and extends to
`resolve_relptr(struct_base, data_relptr) + size_in_bytes`.

### Entry Format

All entries are 16 bytes. The data consists of:

1. **2 global header entries** (bounding box + BVH node count)
2. **N BVH node groups**, each consisting of:
   - 2 header entries (node header + bbox/vertex_count)
   - `vertex_count` vertex entries (groups of 3 = triangles)

### Vertex Entry (16 bytes)

```
Offset  Size  Type   Field
------  ----  ----   -----
0x00    4     f32    x
0x04    4     f32    y
0x08    4     f32    z
0x0C    1     u8     packed_normal_x    # byte / 127.5 - 1.0
0x0D    1     u8     packed_normal_y
0x0E    1     u8     packed_normal_z
0x0F    1     u8     zero
```

### BVH Node Header Encoding

Each BVH node group starts with two 16-byte header entries. The first entry's
first u32 (`first_dword`) encodes both the collision material ID and the layer
index for multi-layer armor:

```
first_dword = (layer_index << 16) | material_id
```

- **byte 0** (bits [7:0]): collision material ID (0–254)
- **byte 1** (bits [15:8]): unused / zero in all observed data
- **byte 2** (bits [23:16]): 1-based layer index (matches GameParams `model_index`)
- **byte 3** (bits [31:24]): unused / zero in all observed data

The second entry has `vertex_count` at bytes 12..16 (`u32` at offset +12).

### Examples

Yamato hull: 6978 triangles (70 BVH nodes).
Iowa hull: 5762 triangles.

Patrie turret (`FGM051_430_50_Mle_1940`):
- `0x00010020` → layer=1, mat=32 (TurretSide) → 350mm
- `0x00020020` → layer=2, mat=32 (TurretSide) → 330mm
- `0x00010021` → layer=1, mat=33 (TurretTop)  → 255mm
- `0x00020021` → layer=2, mat=33 (TurretTop)  → 240mm

## Zone Classification

Material names map to armor zones via `zone_from_material_name()` in
`src/export/gltf_export.rs`. The logic:

1. **Dual-zone** (`Dual_X_Y_Surface`): primary zone = first identifier after `Dual_`.
   - `Cit`, `OCit` → Citadel
   - `Cas` → Casemate
   - `SSC` → Superstructure
   - `Bow` → Bow
   - `St_` → Stern
   - `SS_` → Superstructure

2. **Sub-face suffix** (`SideCit`, `DeckBow`, etc.): zone = suffix.
   - `*Cit` → Citadel, `*Cas` → Casemate, `*SSC` → Superstructure
   - `*Bow` → Bow, `*Stern` → Stern, `*SS` → Superstructure
   - Exception: `SG*SS` (e.g. `SGBarbetteSS`) → SteeringGear

3. **Prefix-based**: `Bow*` → Bow, `St_*` → Stern, `Cit*` → Citadel,
   `Cas*` → Casemate, `SSC*`/`SS_*` → Superstructure, `Tur*`/`AuTurret*`/`Art*` → Turret,
   `Rudder*`/`SG*` → SteeringGear, `Bulge*` → TorpedoProtection,
   `Bridge*`/`Funnel*` → Superstructure, `Kdp*` → Hull

4. **Exact match fallback**: `Deck`/`Belt`/`Trans`/`Inclin`/`Bottom`/etc. → Hull

## Multi-Layer Armor

Multi-layer armor plates are NOT stacked at the same position. Each layer covers
a **different spatial region** of the hull or turret. The game simply raycasts all
geometry and the nearest triangle hit determines the result. There is no explicit
"layer selection" logic — it's an emergent property of the geometry.

### GameParams Armor Dict

**Type**: `HashMap<u32, BTreeMap<u32, f32>>` — outer key = collision material ID
(0–254), inner key = model_index, value = thickness in mm.

Parsed from GameParams raw keys `(model_index << 16) | material_id` by
`parse_armor_dict()` in `provider.rs`.

**Two separate armor maps per ship**:
1. `A_Hull.armor` — hull-wide map covering hull + structural plates.
2. `A_Artillery.HP_XXX.armor` — per-mount turret shell armor.
   ATBA secondaries also have per-mount armor (`A_ATBA.HP_XXX.armor`).

**Sparse**: Only 71 entries for Yamato (36 nonzero). Most triangles inherit
default zone thickness via splash boxes. Keys in pickle data are **integers**,
not strings — parser must handle both.

### Spatial Layout Examples

**Patrie mat 248 (Dual_Cit_Bow_Trans)** — forward citadel athwartship, Z=5.84:
| Layer | Thickness | Y Range         | Description              |
|-------|-----------|-----------------|--------------------------|
| 1     | 370mm     | -0.21 .. 0.02   | Upper (near waterline)   |
| 2     | 235mm     | -0.77 .. -0.21  | Lower (below waterline)  |

**Patrie mat 51 (Bow_Trans)** — full bow transverse, Z=5.84:
| Layer | Thickness | Y Range         | Description              |
|-------|-----------|-----------------|--------------------------|
| 1     | 250mm     | -0.21 .. 0.23   | Above waterline          |
| 2     | 370mm     | -0.21 .. 0.02   | Upper citadel            |
| 3     | 235mm     | -0.73 .. -0.21  | Lower citadel            |

**Slava mat 61 (Cit_Belt)** — citadel side belt:
| Layer | Thickness | Y Range        | Z Range         | Description           |
|-------|-----------|----------------|-----------------|-----------------------|
| 1     | 370mm     | -0.01 .. 0.15  | -5.85 .. 5.08   | Full citadel length   |
| 2     | 350mm     | -0.01 .. 0.15  | -3.98 .. 1.87   | Shorter fore-aft span |

Note: Patrie's athwartships stack **vertically** (different Y ranges, same Z plane),
while Slava's belt layers separate **longitudinally** (same Y range, different Z extents).

### Turret Armor Layers

Turret armor uses the same layer_index mechanism as hull armor. The per-mount
armor dict has keys with `model_index` = 1, 2, 3 (never 0). Each model_index
corresponds to a separate set of BVH nodes covering different spatial regions.

**Patrie turret example** (`HP_FGM_1.armor`):
| Material      | Layer 1  | Layer 2  | Layer 3  |
|---------------|----------|----------|----------|
| TurretSide    | 350mm    | 330mm    | —        |
| TurretTop     | 255mm    | 240mm    | —        |
| TurretAft     | 330mm    | —        | —        |
| TurretDown    | 165mm    | 0mm      | 165mm    |
| TurretFwd     | 590mm    | —        | —        |

## Splash Boxes (`.splash` Files)

Named AABBs for spatial zone classification. Loaded alongside `.geometry` files.

### File Format

```
u32 count
Per box:
  u32 name_len
  char[name_len] name     # e.g. "CM_SB_bow_1", "CM_SB_cit_1"
  f32 min_x, min_y, min_z
  f32 max_x, max_y, max_z
```

**Classification**: Test triangle centroid against AABBs, smallest-volume wins.
Triangles not in any box → "Hull" fallback zone.

**GameParams hit locations**: Zones (Bow, Cit, SS, etc.) are top-level entries
in `A_Hull` with `hlType` field. Each zone has `splashBoxes` array mapping
zone → splash box names.

## Armor Raycast Pipeline

### C++ Functions (from game binary RE)

| Address | Name | Purpose |
|---------|------|---------|
| `0x140367d90` | `getArmorPickedMaterial` | Python-exposed raycast: `(origin, direction) → materialKey` |
| `0x140969370` | BVH raycast | Internal: iterates registered models, finds nearest triangle |
| `0x140369030` | `regArmorVisualModel` | Python: `(model, model_index)` → registers armor model |
| `0x14094b430` | ArmorSystem::registerModel | Allocates ArmorVisualModel entry |
| `0x140967990` | `ArmorVisualModel_loadModel` | Loads `.geometry`+`.armor`, stores model_index |
| `0x1403695d0` | `unregArmorVisualModel` | Python: unregisters armor model |
| `0x1403a1b10` | `getSplashEffectiveArmor` | Receives per-face thickness sequence, computes weighted avg |

### Raycast Flow

1. `getArmorPickedMaterial(ray_origin, ray_direction)` iterates ALL registered
   armor models (linked list at global `data_142ba78d8 + 0x20`)
2. For each model, the BVH raycast reads the instance list from `ArmorVisualModel`
   at offset +0xB0 (begin) to +0xB8 (end)
3. Each instance entry is 16 bytes:
   - offset +8: u32 transform index
   - offset +12: u32 material key `(layer_index << 16) | material_id`
4. For each instance, applies transform, tests ray-triangle intersection via BVH
5. Returns `(model_index << 16) | material_id` of the **nearest** hit, or -1

### Thickness Determination

1. C++ `getArmorPickedMaterial` raycasts geometry, returns `materialID`
2. Python looks up `armorDict[materialID]` (GameParams `.armor` dict)
3. For triangles WITH explicit overrides: thickness = `armor[materialID]` in mm
4. For triangles WITHOUT overrides (vast majority): default zone thickness
   is determined entirely in Python game scripts (encrypted)
5. `getSplashEffectiveArmor` receives per-face thickness as a Python sequence
   of up to 6 floats, computes weighted avg by distance

**GameParams `armourCas/Cit/Deck/Extremities` fields are ALL [-1,-1] for every
ship — never used as overrides.** Zone `armorCoeff` is mostly 0.0; only SG
(Steering Gear) uses non-zero values (0.2-1.2) as a damage reduction coefficient.

## Armor Viewer Rendering API (Python → C++)

The game's armor viewer is controlled entirely from Python scripts (encrypted).
The C++ engine exposes rendering primitives only.

### Material Visibility Control

Methods on `PyHitLocation` (`Physics::pyPhysics`, source: `py_hit_location.h`):

| Function | Args | Description |
|----------|------|-------------|
| `deactivateMaterial(materialId)` | `u32` | Hide a single material ID from rendering |
| `activateMaterial(materialId)` | `u32` | Show a previously hidden material |
| `deactivateMultipleMaterials(seq)` | sequence of ints | Hide multiple material IDs at once |
| `activateMultipleMaterials(seq)` | sequence of ints | Show multiple material IDs |
| `isMaterialActive(materialId)` | `u32` → `bool` | Check if a material is currently visible |

**C++ implementation** (`sub_14039d630` / `deactivateMultipleMaterials`):
- Iterates each Python int in the sequence
- For each material ID, calls `sub_1403d6700` which searches registered armor
  model entries (stride 0xD0) for a BVH node group matching `*(entry + 0x30) == materialId`
- Swaps matching entry to the end of the active range (partition-based hide)

### Armor Color & Highlight

Functions on `Lesta` module (source: `lesta/script_junk.cpp`):

| Function | Args | Description |
|----------|------|-------------|
| `setArmorMaterialColor(hitLocation, materialId, color)` | HL, u32, u32 | Set color for a material plate |
| `setArmorMaterialHighlight(hitLocation, materialId)` | HL, u32 | Highlight a material plate |
| `clearArmorMaterialColors()` | none | Reset all material colors |
| `drawHitLocation(hitLocation, materialId)` | HL, u32 | Draw a hit location |
| `drawHitLocationMaterial(hitLocation, materialId, color)` | HL, u32, u32 | Draw with specific color |
| `setArmorRenderState(...)` | ... | Set render state for armor display |
| `clearArmorSystem()` | none | Full reset: clears models, colors, and render state |

### Armor Renderer Internals

`Armor::Renderer` class (source: `gameplay_render/armor/armor_renderer.cpp`):

- **Shaders**: `shaders/armor/armor.fx`, `shaders/armor/outline_detect.fx`,
  `shaders/armor/resolve.fx`
- **Render targets**: `armor_renderer_outline_rt`, `armor_renderer_depth_rt`,
  `armor_renderer_geometry_rt`, `armor_renderer_depth_buffer`
- **Technique names**: `"Armor"` (main), `"Outline"` (outline detect), `"Resolve"` (composite)
- **Uniforms**: `armorCameraFarPlane`, `armorFadeEnabled`, `armorPaletteTexture`,
  `armorQuality`, `armorDepthTexture`, `armorGeometryTexture`
- **Quality setting**: `ARMOR_SYSTEM_QUALITY` config variable

The palette texture (`Renderer::updatePaletteTexture`) writes per-material colors
into a texture for the shader to sample.

### Armor Viewer Zone Filtering

The in-game armor viewer does NOT display all armor geometry. It iterates the
ship's **child hit locations** (Bow, Cas, Cit, SS, SSC, St, SG) and calls
`drawHitLocation` for each. The **Hull zone** (`hlType=hull_hitlocation`) is the
root/parent zone and is **never drawn** by the viewer. Its `splashBoxes` array
is always empty — it serves as the catch-all for triangles not claimed by any
child zone's splash boxes.

**Consequence**: Collision materials without a zone prefix (`Trans`, `Deck`,
`Belt`, `Bulge`, `ConstrSide`, `Inclin`, etc.) classify to the Hull fallback
zone and are **invisible in the armor viewer** despite being fully functional
in the game's combat model (raycasts DO hit them).

**Slava example** — hidden armor plates:
| Material ID | Name       | Thickness | Notes                          |
|-------------|------------|-----------|--------------------------------|
| 102         | Trans      | 420mm     | Full-height transverse bulkhead|
| 103         | Deck       | 195mm     | 3 layers [100, 75, 20]         |
| 104         | Belt       | 720mm     | 2 layers [370, 350]            |
| 69          | ConstrSide | 50mm      | Longitudinal construction side |

These span the entire hull length or height, crossing multiple splash box zones.
They provide real shell protection but players cannot see them in the armor viewer.
Our GLB export correctly includes them in the "Hull" zone node.

### Global Armor System Object

All armor state lives in a global singleton at `data_142ba78d8` (`Armor::System`).
- Created during `BW::WorldAppModule::init` via `sub_14094c1b0`
- Destroyed via `sub_14094adda`
- `clearArmorSystem` resets: `model_count = 0`, clears BVH cache, clears material
  colors (both at +0x58 and +0x98 offset vectors)

### Key Conclusion: Filtering Is Python-Only

**There is NO C++ logic that decides which plates to show/hide.** All decisions
about which materials to show, what colors to assign, and how to handle
multi-layer visibility are made in encrypted Python scripts
(`scripts/ArmorConstants.pyc`, `scripts/ModelArmor.pyc`, etc.).


---

# MergedModels (`models.bin`) Format

Reverse-engineered from `WorldOfWarships64.exe` using Binary Ninja.

Space/map directories contain a `models.bin` file that packs **all model prototypes**
for the space into a flat array. Each record inlines a ModelPrototype, VisualPrototype,
and reference to a shared SkeletonProto. These reference the sibling `models.geometry`
file for actual mesh data.

**Parser**: `src/models/merged_models.rs` → `parse_merged_models()`

## Header (0x18 = 24 bytes)

```
Offset  Size  Type   Field
------  ----  ----   -----
0x00    4     u32    modelsCount           # number of model records
0x04    2     u16    skeletonsCount        # number of shared skeleton prototypes
0x06    2     u16    modelBoneCount        # total bone count across all skeletons
0x08    8     i64    modelsRelptr          # -> MergedModelRecord[] (relative to file start)
0x10    8     i64    skeletonsRelptr       # -> SkeletonProto[] (relative to file start)
```

## MergedModelRecord (0xA8 = 168 bytes each)

```
Offset  Size  Type              Field
------  ----  ----              -----
0x00    8     u64               pathId              # selfId in pathsStorage
0x08    40    ModelPrototype    modelProto           # inlined (0x28 bytes, same as assets.bin blob 3)
0x30    112   VisualProto       visualProto          # inlined (0x70 bytes, see below)
0xA0    4     u32               skeletonProtoIndex   # index into shared skeletons array
0xA4    2     u16               rsGeometryStartIdx   # first geometry mapping index
0xA6    2     u16               rsGeometryCount      # number of geometry mappings
```

### Inlined VisualProto (0x70 bytes at record+0x30)

The first 0x30 bytes are an inlined SkeletonProto (VisualNodes), followed by
visual-specific fields. All relptrs are relative to `vp_base` (= record + 0x30).

```
Offset  Size  Type         Field
------  ----  ----         -----
+0x00   48    SkeletonProto  inlineNodes        # VisualNodes (0x30 bytes)
+0x30   8     u64            mergedGeomPathId   # selfId of the models.geometry file
+0x38   1     u8             underwaterModel    # 1 if underwater variant
+0x39   1     u8             abovewaterModel    # 1 if above-water variant
+0x3A   2     u16            renderSetsCount
+0x3C   2     u16            lodsCount
+0x3E   2     ---            (padding)
+0x40   12    3×f32          boundingBoxMin
+0x4C   4     ---            (padding)
+0x50   12    3×f32          boundingBoxMax
+0x5C   4     ---            (padding)
+0x60   8     i64            renderSetsRelptr   # -> RenderSet[] (relative to vp_base)
+0x68   8     i64            lodsRelptr         # -> Lod[] (relative to vp_base)
```

### RenderSet (0x28 = 40 bytes each)

```
Offset  Size  Type   Field
------  ----  ----   -----
0x00    4     u32    nameId                 # string hash of RS name
0x04    4     u32    materialNameId         # string hash of material name
0x08    4     u32    verticesMappingIndex   # mapping_id in geometry vertices_mapping
0x0C    4     u32    indicesMappingIndex    # mapping_id in geometry indices_mapping
0x10    8     u64    materialMfmPathId      # selfId of .mfm material in pathsStorage
0x18    1     u8     skinned
0x19    1     u8     nodesCount             # number of node name IDs
0x1A    6     ---    (padding)
0x20    8     i64    nodeNameIdsRelptr      # -> u32[] (relative to rs_base)
```

**Note**: `verticesMappingIndex` and `indicesMappingIndex` are `mapping_id` values
(NOT array indices). They must be looked up by scanning `geometry.vertices_mapping`
for a matching `mapping_id` field.

### LOD (0x10 = 16 bytes each)

```
Offset  Size  Type   Field
------  ----  ----   -----
0x00    4     u32    rsNamesCount           # number of render set name IDs in this LOD
0x04    4     ---    (padding)
0x08    8     i64    rsNamesRelptr          # -> u32[] (relative to lod_base)
```

## Shared Skeleton Array

The skeletons array contains `skeletonsCount` entries, each 0x30 bytes. Layout is
identical to the inline SkeletonProto in VisualProto (a VisualNodes structure).

Each model record's `skeletonProtoIndex` is an index into this array.

## Data Layout

Sub-arrays (render sets, LODs, node IDs) are stored contiguously between the
model records and the skeleton array. Relptrs in each record/sub-struct point
into this region.

### Example (`16_OC_bees_to_honey/models.bin`, 1,210,508 bytes)

```
0x000000 - 0x000017    Header (24 bytes)
0x000018 - 0x012CA7    Model records (458 × 0xA8 = 76,944 bytes)
0x012CA8 - 0x0B3BBF    Sub-arrays: RenderSets, LODs, node IDs (659,224 bytes)
0x0B3BC0 - 0x0B8A1F    Shared skeletons (418 × 0x30 = 20,064 bytes)
0x0B8A20 - end         Skeleton sub-arrays: transforms, names, parents (454,252 bytes)
```

---

# Space Instances (`space.bin`) Format

Reverse-engineered by analysis of `space.bin` files in space directories.

The `space.bin` file in each space directory contains **instance placement data** —
the world transform for every model instance in the space. Each instance references
a model prototype in the sibling `models.bin` via `pathId`.

**Parser**: `src/models/merged_models.rs` → `parse_space_instances()`

## Header (0x60 = 96 bytes)

```
Offset  Size  Type   Field
------  ----  ----   -----
0x00    4     u32    instanceCount          # number of model instance entries
0x04    4     u32    (unknown)              # secondary count (other section)
0x08    4     u32    (unknown)
0x0C    84    ---    (other section offsets / metadata)
```

Only `instanceCount` at offset 0x00 is needed for instance extraction.

## Instance Entry (0x70 = 112 bytes each, starting at file offset 0x60)

```
Offset  Size  Type       Field
------  ----  ----       -----
0x00    64    16×f32     transform        # 4×4 world transform matrix (row-major)
0x40    16    ---        (padding, all zeros)
0x50    8     u64        pathId           # matches MergedModelRecord.pathId in models.bin
0x58    8     u64        flags            # observed as 0x0000000004000001
0x60    16    ---        (padding, all zeros)
```

### Transform Matrix

The 4×4 f32 matrix is stored row-major with the following layout:

```
[ R00  R01  R02  0 ]    row 0: rotation/scale
[ R10  R11  R12  0 ]    row 1: rotation/scale
[ R20  R21  R22  0 ]    row 2: rotation/scale
[ Tx   Ty   Tz   1 ]    row 3: translation + w=1
```

Column 3 is always `[0, 0, 0, 1]` (affine transform). The rotation component
may include non-uniform scaling (e.g. `1.389, -1.896` in the rotation block
indicates a scaled + rotated placement).

### Instance-to-Prototype Mapping

Each `pathId` matches exactly one `MergedModelRecord.pathId` in the sibling
`models.bin`. Multiple instances can share the same `pathId` (same model placed
at different locations with different transforms).

### Example (`16_OC_bees_to_honey`)

- 3174 instances referencing 458 unique prototypes
- Most-instanced model: 763 copies (vegetation/rocks)
- Instance entries start at file offset 0x60
- After instances: ASCII hash strings and other section data

### GLB Export

When `space.bin` is available, the exporter creates one glTF mesh per unique
prototype and one node per instance with the world transform applied via the
glTF `matrix` property. This enables efficient mesh reuse — 3174 nodes sharing
only 458 unique meshes.
