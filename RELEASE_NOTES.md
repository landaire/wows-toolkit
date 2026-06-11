# Release Notes

Player-facing highlights for each release. New entries are generated from `Release-Note:`
commit trailers via `cliff-release.toml`; see that file for the convention. The full,
per-commit history lives in `CHANGELOG.md`.

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
