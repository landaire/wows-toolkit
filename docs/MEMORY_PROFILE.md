# Memory Profile

What the egui app holds, measured 2026-09-26 with `profile_memory` (`crates/wows-toolkit/src/profiling/memory.rs`).

```
cargo run --profile profiling --features profile-bins,dhat-heap --bin profile_memory -- <scenario> [count]
  scenarios: builds reload tabs parses tracker listing encounters unpacker maps armor
  WOWS_DIR / WOWS_BUILDS_DIR / WOWS_REPLAY_DIR point it at data
```

Each scenario drives the cache or store the tab itself uses, samples the process
after every step, then drops what it built. `live heap` is exact (dhat's global
allocator); the process counters beside it carry the Windows heap's own
per-block overhead, which is why private bytes run 1.4-1.5x the live heap on the
allocation-heavy steps. Numbers below are live heap unless stated.

Data: live install build 13187581, dump archive of 132 builds, 89 replays
spanning 2020-2026.

## What each feature holds

| Feature | Live heap | Blocks | Lifetime |
| --- | --- | --- | --- |
| One build's game data (VFS index, GameParams, icons, constants) | 215 MiB | 2.01M | main build pinned; 2 more in an LRU |
| Loading one build (transient) | peak 287 MiB | | released |
| One open replay tab, new build | +150 MiB | +1.15M | until the tab closes |
| One open replay tab, build already resident | +20 MiB | +60K | until the tab closes |
| Parsing one replay (transient) | peak +330 MiB | | released |
| Resource browser, pkg pane file list | +83 MiB | +1.17M | until the build changes or exit |
| Resource browser, assets.bin pane | +270 MiB | +1.13M | until the build changes or exit |
| Armor viewer ship assets | +180 MiB | +85K | until exit |
| One cached minimap, per (version, map) | +2.25 MiB | | until exit |
| Player tracker, per player met | ~0.3 KB | | persisted, until exit |

Three builds resident is ~645 MiB before anything else. The GUI's own runtime
(egui atlases, wgpu, fonts, translations) is not headless-measurable and is not
in these figures.

## No leak

Every scenario returns to its baseline when what it built is dropped, and
`reload` (the same build loaded five times, each load's cache dropped before the
next) ends every round at **0 live bytes in 0 blocks**. Process private bytes
settle at ~140 MiB across those rounds and stay flat, so that figure is the
allocator holding freed pages, not retention.

What looks like a leak in a long session is retention by design:

1. **A hydrated replay pins its build.** A tab holds
   `Arc<GameMetadataProvider>`, so a build the LRU has evicted stays resident
   for as long as any tab, renderer window or tactics board opened from it does.
   Five tabs from five builds measured 765 MiB where the cache itself admitted
   holding two builds. The tab's own parse is only ~20 MiB of that; the other
   ~130 MiB is the build it will not let go of.
2. **Panes never give anything back.** `BrowserPane::files`,
   `ArmorViewerState::ship_assets` and the assets.bin VFS are built when a tab
   is first shown and live in `TabState` until the build changes or the app
   exits. Closing the tab does not free them.
3. **The minimap cache has no bound.** `RendererAssetCache::maps` is keyed by
   (version, map name) and only grows; the GPU-side `RendererTextureCache::maps`
   beside it is capped at 6.

## What scales with replay count

Nothing above grows with how many replays a directory holds. These do.

`listing 20000` and `encounters 20000` measure them; `WOWS_TRACKER_POOL` sets how
many distinct accounts the synthetic roster is drawn from, which is what the
tracker's cost turns on.

| Structure | Per unit | At 20,000 replays |
| --- | --- | --- |
| Workspace listing (files map, index summaries, sorted rows, group trees) | ~1.5 KB per replay | +29 MiB |
| Player tracker, mostly first meetings (273K distinct accounts) | ~0.75 KB per account | 202 MiB |
| Player tracker, heavy repeats (20K accounts, 23 meetings each) | ~35 B per encounter | 35 MiB |
| Tracker as JSON | | 16-62 MiB |

The listing is fine: a directory of 20,000 replays costs 29 MiB to list, and the
row widgets drawn from it are LRU-bound to three viewports.

The player tracker is not. Its cost is driven by distinct accounts met, at
~0.75 KB apiece, and a random-battle player meets 23 new-ish names per battle:

- 20,000 battles measured **202 MiB** live across 1.09M blocks, growing linearly
  (2,000 battles = 28 MiB).
- `save_tracked_players` serialises the whole tracker to one JSON string on
  every save, and the save task runs on a **5-second timer** whether or not
  anything changed. At that size the string is **62 MiB**, so each save allocates
  and frees 62 MiB, and `LAST_TRACKER_JSON` keeps another 62 MiB resident for the
  unchanged-comparison. The tracker's real steady-state cost is therefore live
  tree plus one JSON copy, with a second copy churning every five seconds.
- When it has changed, that 62 MiB goes into SQLite as a single setting value,
  and startup reads and parses it back.

`sent_replays` and `session_stats` share the shape of the save problem without
the size: both snapshot every row and `DELETE` + re-`INSERT` the whole table on
every save. At 10,000 uploaded replays or a "add every replay to session stats"
run, that is a full table rewrite every five seconds.

## What is already bounded

- `BuildDataCache`: the live install's build is pinned, every other build sits
  in a 2-entry LRU (`NON_MAIN_BUILD_CAPACITY`). Resolving eight builds in a row
  plateaus instead of climbing.
- Directory ingest, index reconcile, "populate player inspector", session-stats
  batches and the live-roster scan all read one replay, use it and drop it. The
  `tracker` run over 40 replays plateaued, and dropping the tracker afterwards
  freed nothing measurable: 684 players tracked cost 0.2 MiB serialized.
- `PreviewCache` (hover previews) and `RendererTextureCache` are capacity-bound.

## Peaks

A first open of a large mixed-era directory is the worst case: the ingest loads
each build in turn (peak 287 MiB apiece), parses each replay under it (peak
+330 MiB apiece), and keeps three builds resident while it does. That is ~1 GiB
of data-path memory before the GUI's own.
