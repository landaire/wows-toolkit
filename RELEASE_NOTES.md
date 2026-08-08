# Release Notes

Player-facing highlights for each release. New entries are generated from `Release-Note:`
commit trailers via `cliff-release.toml`; see that file for the convention. The full,
per-commit history lives in `CHANGELOG.md`.

## 1.0.0

We've reached 1.0! I'm calling it now: this will cause confusion for people who say "omg I have 1.70 I don't have this feature???" but no you are on _0.1.70_.

Anyways, the project has many good QoL features. It's worthwhile to just consider it 1.0 now.

High-level features/changes in this release:

- New **search tab** for searching historical replays (only going forward from this day as prior history was not recorded, sorry!). Queries are persisted across restarts.
- New **command palette** (Ctrl+K or Ctrl+P) for some quick actions/stuff that doesn't fit in the UI.
- You can **open directories of replays** from the command palette which will parse in a new tab. Right-click the tab to search that set of replays
- **Hover** a replay in the replay inspector to quickly preview its render
- **Player Tracker** has been revamped with separate tabs for **Clans**, the pre-existing Player Tracker tab has been slightly reworked, and a new **Current Match** tab has been added
- **Current Match** tab allows you to view team win rate / PR composition similar to tools like Potato Alert / Matchmaking Monitor which have already done this very well for a long time. This is something I've been hesitant to add for some time now as I feel these tools do this well and I don't like the impact it has on a player's mental state of the game. People want it though so here we are.
- Improvements to the **Armor Viewer**: some fixes relating to penetration mechanics, loading a ship's textures (this still has bugs but mostly works), and misc items on the boat such as lifeboats, propellers, and other 3D items that were previously not shown.
- Updated **look and feel**. Same UI framework with some custom styling tweaks.
- You can now opt-in to sending my ShipBuilds.com server **raw replays** instead of builds. There's nuance: if you're in a test ship, it will fall back to the old method of sending builds rather than the raw replay. Client-side filtering to Ranked/Random battles replays is still performed. This will allow me to do richer server-side analysis for things like death heatmaps and correlate lag detection in matches with other toolkit users. 
- Blocked **shitware** from injecting **shit** into WoWs Toolkit which would generally cause odd behaviors (crashing, Discord overlay, Steam overlay, etc.). This might block some form of text input, and can be disabled in settings for users who need it (look for code integrity setting).

The rest is an AI-generated summary of a bunch of changes:

**Search and the command palette**

- New Search tab: build a query by clicking filters, or type it directly, with boolean AND/OR/NOT, grouping, "any player" / "all players" quantifiers, relative dates, and suggestions as you type.
- Results are sortable, preview the map on hover, copy a replay's path, and open a replay in its own tab.
- Your query is remembered across restarts, and edits can be undone and redone.
- A command palette on a hotkey jumps straight to players, ships, replays, and actions from anywhere in the app.

**Replay browsing**

- The replay listing is backed by a durable index, so it opens without parsing every file, and each row shows the result, ship, and stats on two lines.
- Any replay directory can be opened as its own closeable tab and searched from there.
- Hovering a replay row plays a looping minimap preview of that match.
- New match timeline window in the replay inspector.
- When a replay needs game data you do not have, the toolkit says so and offers to download exactly the builds it needs.

**Player tracker**

- Current Match reads the live roster from the battle in progress, split by team, with ships, win rate, PR, battles, average damage, and per-ship PR, plus links out to wows-numbers and ShipBuilds.
- New Clans sub-tab breaks your encounters down by clan over a selectable time range.
- Division mates are marked per encounter rather than per account, historical rows collapse with an expandable notes block, and disconnects and stream-snipers are flagged.

**Armor viewer**

- Camouflages now render on ship models, with a camo picker per pane.
- Ships draw their misc parts: propellers, boats, and deck fittings.
- Multi-pane comparison with mirrored cameras and synced settings, hull LOD / upgrade / module reload, and GLB export with optional armor and selectable texture detail.
- Penetration is more accurate: the fuse arms against effective armor, and fuse travel converts at the ship-model scale.

**Fire chance analysis**

- Your effective fire chance for the match, broken down by the HE hits behind it, with an explanation for every fire ribbon that did not get credited.

**Look and feel**

- New Graphite & Bone theme and a light mode, with every colour contrast-checked for legibility.
- Cleaner dock tabs, tidier tables, and a large number of layout and spacing fixes throughout.

**Sharing**

- Explicit data-sharing modes with consent dialogs: share nothing, share build data, or share raw replays. Opting down never silently escalates.
- Batch-upload your open replays to ShipBuilds with progress. Test ships are never uploaded, and raw replays only upload once a battle has actually finished.

**Performance**

- Much faster replay loading, and directory scans now read headers only, spread across every core.
- Noticeably lower memory use when loading game data.
- Camouflage loading is dramatically faster: composited on the GPU and decoded on background workers instead of stalling the UI.

**Important fixes**

- The renderer pins a working GPU driver and falls back if a launch hangs, and reports a failed startup instead of exiting with no window.
- Optional blocking of third-party code injecting itself into the toolkit's process.
- The auto-updater no longer grabs the command-line tools zip by mistake, and update finalization handles both old and new argument forms.
- Freshly written replays are parsed off the UI thread with retries, so the app no longer stutters or dies when a battle ends.
- Crew skills are no longer discarded on some game builds, and ship config dumps match the in-game description, including old versions.
- Exported videos are named with the battle timestamp first.

### Command-line tools and libraries

- Replay parsing is substantially faster: zero-copy metadata APIs, streaming interleaved Blowfish decrypt, and per-packet hashing, dispatch, and allocation costs cut across the board. `replayshark bench` profiles the load path.
- wowsunpack discovers universal camouflages via `isTileflage` on Exterior params (skipping death skins), extracts hull `burnNodes` and length, and builds the VFS index without copying it three times.
- wows-battle-world gained fire-analysis logging: burn-bit transitions, timestamped self ribbons, AOI presence windows, and hit history. `BattleReport::players` now has a stable order, and a loss carries the real winning team id.
- wows-replay-insights adds an egui-free `NormalizedBattleReport`, per-victim burn-state and damage-control tracks, and a headless effective-fire-chance resolver with corpus measurement.
- A new SQLite replay index provides match and roster predicates, facet queries, live ingestion, panic-isolated backfill and reconcile, boundary-checked source relocation, and per-source row summaries.
- minimap-renderer bakes bounded preview tracks in a single forward pass behind a dwell gate.
- replayshark adds `query roster`, a battle-results dump command, and a constants resolver with approximate-match classification.
- wows-data-mgr plans deduplicated multi-build downloads, shares one HTTP client, downloads sequentially into the shared CAS, and verifies object hashes, naming the build a corrupt object breaks.

## 0.1.70

### WoWs Toolkit

**Highlights**

- AV1 should now be available as a video export codec (an issue with the automated builds kept it from surfacing before).
- Old game-version data loads should not require administrator permissions/Windows Developer Mode to create symlinks. Instead uses pre-existing metadata to load from content-addressed storage.

**Important fixes**

- Video export should no longer fail partway through with some codecs (EmptyVideoFrame error).
- Single-replay renders default to the stats panel again; team rosters remain the default only for merged replays. This was an unintentional change previously.
- Fix issue with not being able to see ship-specific context menu items in live replay renderer when player names were disabled.

### Command-line tools and libraries

- Far more detailed ship stats extracted from game data: artillery and dispersion, per-caliber secondaries, torpedoes, concealment, durability, mobility, and consumables resolved with upgrades, skills, and signals applied, plus a per-stat breakdown of what each input contributes.
- Faster GameParams parsing and lower memory use.
- wowsunpack, replayshark, wows-data-mgr, and minimap_renderer now ship as platform-named zips (wows_toolkit_tools_<version>_<platform>.zip).

## 0.1.69 - 2026-06-11

**Highlights**

- Compare replays side by side: merge multiple per-player replays into a single rendered view, with team roster panels and merged camera perspectives.
- Video export now supports multiple codecs (H.264, H.265, and AV1) with a configurable bitrate / target file size.
- Armor viewer hull lighting: realistic shading with In-Game / Flat / Studio presets, full controls (direction, intensity, rim, specular, colors), a light-source marker, and a detachable settings window.
- Replay view shows each player's consumable inventory and equipped module loadout.
- Game data for older client versions downloads on demand, and the local cache is validated against the remote and kept up to date automatically.
- Broad support for old replays (back to ~0.7.x): correct assets, ranges, captain skills, consumable IDs, and chat/voiceline names.
- Web (WASM) build support.

**Important fixes**

- Smoother rendering on Windows and fixes to render-backend selection (resolves window-drag stutter).
- Linux/macOS: fixed "permission denied" when extracting game files (#35).
- Smoother ship motion on the minimap (full-precision positions, per-variant interpolation instead of snapping).
- Shell tracers are paced by server time-to-impact for more accurate timing.
- More reliable networking: honors the OS trust store for HTTPS, with timeouts, retries, and validation of downloaded data.
- Correct healing / regeneration display while a ship is repairing.
