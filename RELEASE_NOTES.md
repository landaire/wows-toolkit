# Release Notes

Player-facing highlights for each release. New entries are generated from `Release-Note:`
commit trailers via `cliff-release.toml`; see that file for the convention. The full,
per-commit history lives in `CHANGELOG.md`.

## 0.1.70

### WoWs Toolkit

**Highlights**

- AV1 should now available as a video export codec (issue caused by automated builds did not surface it before).
- Old game-version data loads should not require administrator permissions/Windows Developer Mode to create symlinks. Instead uses pre-existing metadata to load from content-addressed storage.

**Important fixes**

- Video export should no longer fails partway through with some codecs (EmptyVideoFrame error)
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
