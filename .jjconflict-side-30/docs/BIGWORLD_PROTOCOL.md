# BigWorld protocol notes

What a `.wowsreplay` packet stream reveals about the server, and what it does not.

Sources: 89 replays spanning 0.5.9 (2016) through 15.6.0 (2026); entity and
component defs from unpacked builds; `WorldOfWarships64.exe` build 12830008
disassembled in Binary Ninja.

## Cells: not observable

The client is never told about CellApp topology, and no replay packet leaks it.

WoWs disables the engine's automatic area-of-interest management. Every entity
type the client can see declares `<IsManualAoI>true</IsManualAoI>`:
`Vehicle`, `Building`, `SmokeScreen`, `BattleEntity`, `BattleLogic`,
`InteractiveZone`. AoI membership is therefore chosen by cell script, which in
WoWs means the spotting rules. Measured distances from the player at each AoI
transition confirm it: enters span 30 to 1331 world units and leaves 49 to 1245,
the two distributions overlap completely, and a single entity re-enters and
leaves at unrelated ranges (entity 433446 left at 329, 345, 351, 356, 457).
Engine-managed AoI would cluster leaves at one radius plus a hysteresis band.

One space per battle. Each replay carries exactly one `Map` packet, exactly one
`CellPlayerCreate` at clock 0, and one SpaceID shared by every `EntityCreate`.
That holds for rejoin replays too.

The messages that would expose topology never appear. `ClientInterface` in the
binary declares `resetEntities`, `restoreClient`, `switchBaseApp`,
`cellAppSuspended`, `cellAppResumed`, `spaceProperty`, `enableEntities`,
`enterAoIOnVehicle`, `createEntityDetailed` and `changeVolatilePackerType`.
A raw packet-ID histogram over all 89 decrypted streams contains only IDs the
parser already maps, with no unrecognised ID anywhere.

The engine's own `ReplayController` (BigWorld server-side replays, a tick-based
compressed format unrelated to `.wowsreplay`) does record AoI membership, via
`handleEntityAoIChange` and `handleEntityPlayerStateChange`. That is where the
information lives if a server-side replay is ever available.

## What the server does leak

- SpaceID is a per-shard counter. Four consecutive battles on 2026-07-27 used
  1987, 2024, 2035 and 2055 across 56 minutes.
- Two EntityID pools. Arena setup entities (`BattleLogic`, `BattleEntity`,
  `Vehicle`, `Building`) come from a high block that advances monotonically
  across consecutive battles (333276, 335248, 341483, 346484). Runtime spawns
  (`SmokeScreen`, `InteractiveZone`) come from a separate low block that does
  not.
- IDs are handed out in avatar/vehicle pairs, the ship being the avatar's id
  plus one, so ships within one arena share a parity.
- `enterAoI` (0x03) appears only in the 2016 and 2017 replays. Later builds
  dropped entity-id aliasing and send a full `EntityCreate` on every re-entry,
  the same change as `IsManualAoI`.

## Entity components

`scripts/components.xml` lists 31 components; each `scripts/component_defs/*.def`
names its host entities in an `<ofEntity>` section. This layer is separate from
`scripts/entity_defs` and is not read by `wowsunpack::rpc::entitydefs`.

Avatar has exactly two: `HotFixComponent` (also on Account and Login) and
`DivisionsManagerComponentAvatar`. Neither declares `<Properties>`.

`PyEntity::initComponentsFromStream` (`sub_14017b9a0`) reads a single byte
holding the component count, compares it against the entity type's component
descriptor vector, and on mismatch logs "Wrong number of components in stream.
Expected %d, got %d". It then calls
`ClientEntityComponent::updateAttributesFromStream` (`sub_140196e70`) per
component. An empty stream is accepted as "no components".

So `component_data` on `CellPlayerCreate` and `BasePlayerCreate` is that count
byte and nothing else, because both Avatar components are method-only. It tracks
the component list over time:

| Version range | Trailing byte | Avatar components |
| --- | --- | --- |
| 0.5.9 to 0.8.5 | absent | predates the component system |
| 0.9.10 to 12.3.1 | 1 | HotFix only |
| 12.8.0 onward | 2 | HotFix plus DivisionsManagerAvatar |

There is no gameplay data here. Both components are account plumbing.

## Bitmask fields

Three replicated fields are packed bitmasks. All three are modelled in
`wows-core::game_types` (`AccountAttrs`, `VisibilityFlags`, `BurningFlags`), each
keeping a `raw()` accessor and an `unknown_bits()` so a build that adds a flag
surfaces it instead of dropping it.

The defining constants live in modules that stock decompilers fail on. Rather
than decompiling, read the class bodies straight out of the Python 2.7 bytecode:
walk `LOAD_CONST` followed by `STORE_NAME` in each code object. Class names are
obfuscated but attribute names are intact, so grep the dump for a known flag.

### `Avatar.attrs` (client `ACCOUNT_ATTR`, module `ma779114d`)

Entitlements of the recording account, and the only per-account signal in the
packet stream.

| Bit | Flag | Bit | Flag | Bit | Flag |
| --- | --- | --- | --- | --- | --- |
| 0 | `COOPERATIVE_BATTLES_ONLY` | 10 | `ROAMING` | 29 | `ALPHA` |
| 1 | `TOURNAMENT_BATTLES_ONLY` | 11 | `DAILY_MULTIPLIED_XP` | 30 | `CBETA` |
| 2 | `CLAN` | 12 | `PAYMENTS` | 31 | `OBETA` |
| 3 | `MERCENARY` | 13 | `OUT_OF_SESSION_WALLET` | 32 | `PREMIUM` |
| 4 | `RATING` | 14 | `EXCLUDED_FROM_FAIRPLAY` | 33 | `AOGAS` |
| 5 | `USER_INFO` | 15 | `COMPLAINTS_IMMUNE` | 35 | `IGR_BASE` |
| 6 | `STATISTICS` | 21 | `DAILY_BONUS_1` | 36 | `IGR_PREMIUM` |
| 7 | `ARENA_CHANGE` | 22 | `DAILY_BONUS_2` | 37 | `FULL_ACCESS_BOTS_AI` |
| 8 | `CHAT_ADMIN` | | | 38 | `SERVER_REPLAYS_ACCESS` |
| 9 | `ADMIN` | | | | |

Bits 16-20, 23-28 and 34 are unassigned. Observed values across the corpus are
`0x100001050` (`RATING|STATISTICS|PAYMENTS|PREMIUM`) and `0x1050`, the same
account without premium.

### `Vehicle.visibilityFlags` (client `ConstantsShip.VisionFlags`, module `me658a8e4`)

Why a ship is currently spotted, never by whom. Generated by
`ConstantsUtils.bitFlagGenerator(hasZeroValue=True)`, so `INVISIBLE` is 0 and the
named flags start at bit 0.

| Bit | Flag | Bit | Flag |
| --- | --- | --- | --- |
| 0 | `BY_SHIP` | 6 | `IN_SMOKE` |
| 1 | `BY_MAIN_PLANE` | 7 | `BY_PINGER` |
| 2 | `BY_COMMON_XRAY` | 8 | `BY_MISC_PLANE` |
| 3 | `BY_RLS_PERSONAL` | 9 | `BY_SUBMARINE_LOCATOR` |
| 4 | `BY_RLS` | 10 | `BY_RECON` |
| 5 | `BY_SONAR` | 11 | `BY_ANTI_MISSILE` |

The client's composites: `BY_XRAY` is sonar, both radars and the submarine
locator (deliberately excluding `BY_COMMON_XRAY`); `BY_ANY_PLANE` is the three
plane flags; `VISIBLE_FOR_TEAM` is everything except `BY_RLS_PERSONAL` and
`BY_PINGER`, which stay private to the spotter.

### `Vehicle.burningFlags` (module `ma779114d`)

| Bits | Mask | Meaning |
| --- | --- | --- |
| 0-3 | `BURN_MASK` 0x000F | the four fire sections (`MAX_BURN_NODES_COUNT` = 4) |
| 4-7 | `FLOOD_MASK` 0x00F0 | the four flooding sections |
| 8 | `ACID_MASK` 0x0100 | acid |
| 9 | `WILD_FIRE_MASK` 0x0200 | wild fire |

Only the fire nibble was modelled previously, so flooding, acid and wild fire
were being masked off and discarded.

## Packet layout corrections

### BasePlayerCreate (0x00) carries a length prefix

```
entity_id: u32 | entity_type: u16 | data_len: u32 | base properties | components
```

`data_len` covers the properties and the component section together: 9 on modern
builds (8-byte `attrs` plus the count byte) and 8 before components existed.
Verified against all 89 replays, where `data_len` always equalled the remaining
payload exactly.

The parser previously skipped this field and decoded `attrs` four bytes early,
so `attrs` was wrong in every replay and its real tail bytes surfaced as
`component_data`. Avatar `attrs` is `0x100001050`, or `0x1050` on some accounts.

`ServerConnection::createBasePlayer` (`sub_1401f25a0`) confirms the shape. Note
that the live wire uses BigWorld's compact length (one byte, escaping to three
when it is 0xFF) while the `.wowsreplay` container uses a flat `u32`; the
client-side recorder re-serialises, which is also why the container has a
12-byte size/type/clock header that is not on the wire.

Packet 0x26 is the same layout with `data_len` always 0.

### EntityCreate (0x05) field order

The wire order is SpaceID then VehicleID, matching `CellPlayerCreate` and
`EntityEnter`. The parser had the two bindings transposed, so every
`EntityCreate` reported `space_id: 0` and `vehicle_id: <arena SpaceID>`.

`vehicle_id` is BigWorld's vehicle/passenger link, not a GameParams id. It is
always 0 in WoWs, so it is modelled as `Option<EntityId>` on `EntityCreate`,
`CellPlayerCreate` and `EntityEnter`.

## Reproducing

```sh
# Per-replay packet dump (CAS dumps work via -e)
replayshark -e G:/wows_builds dump --no-meta <replay>

# Raw packet-ID histogram: decrypt, then walk size/type/clock headers
replayshark decrypt -m meta.json -p packets.bin <replay>
```

Binary anchors, build 12830008: `0x141f0b730` (component count mismatch string),
`0x141f0e8f8` (`updateAttributesFromStream`), `0x141f13c90` and `0x141f13e78`
(the createBasePlayer and createCellPlayer log strings). Cross-reference each to
reach the handler.

## Open items

- Packet 0x26 in pre-12.6 layouts is not `BasePlayerCreateStub`. Those replays
  carry 4-byte 0x26 packets (41 of them in one 12.3.1 replay) which cannot match
  the stub layout and currently decode as Invalid.
- `Map` packet `unknown1` and `unknown2` are unidentified. They are not map name
  hashes: two different maps share `unknown2 = 1846975498`.
- `arena_id` decodes as `(salt << 32) | 1`, with the low word always 1. The
  field may be misaligned in `parse_map_packet`.
- The snapshot tests resolve game data with `game_data::load_game_resources`,
  which cannot read content-addressed dumps, while the `has_build_*` cfg gates
  that enable them accept a dump. On a machine with only dumps the tests are
  enabled and then fail in setup.
