# NSH port: parity with the GPUI app

The Native Shell front end (`crates/wows-toolkit-nsh`, binary `crates/wows-toolkit-nsh-app`) reproduces the GPUI app's features and layout on [nsh](https://nsh.dev) 0.4.0. Layout follows GPUI; visual style is nsh's own (shadcn-style tokens, Lucide icons). It runs on Linux only, because nsh's GTK backend is the only one it is built for.

## Structure

- **TSX interface** (`ui/`): one file per GPUI view. `ui/main.tsx` is the bundle entry; it draws `ui/App.tsx` in the main window and `ui/shell/window_root.tsx` in a Tactics Board or popped-out renderer window.
- **Rust services** (`src/`): `#[module]` services and `#[component]` native views the TSX imports from `wows-toolkit/native`. Heavy work (replay parsing, indexing, rendering, the armor GPU viewport) stays in Rust and in the shared crates `wows-toolkit-replay-data`, `wows-toolkit-armor`, `wows-toolkit-viewmodel`, `wt-collab-client`, which GPUI uses too.
- **Frames** (`src/frames`): a `wows.frame` GTK native component shows RGBA frames a Rust producer presents to a named surface, and forwards pointer, wheel, pinch, key (down and up) and focus input to it. The replay renderer, minimap preview, watched playback, tactics board, stats charts and armor viewport draw through it.
- **Windows** (`src/desktop.rs`): the app owns its GTK runner. Each extra window is a second `AppView` running the same bundle; Rust services are process-wide, so a popped-out renderer keeps its live session. Window sizes are kept in the `window_sizes` row GPUI uses.
- **Icons** (`ui/icons.tsx`, `src/icons.rs`): vendored Lucide 1.43.0 (ISC) plus the Twitch mark from Simple Icons (CC0), in `assets/icons/`.
- **Settings and data**: the same SQLite database, keys and rows as the egui and GPUI apps, so all three front ends share state.

## Views

| GPUI view | NSH |
|---|---|
| Tab strip, application menu, status strip, toasts, dialogs, about, crash report, first run, update check | `ui/App.tsx`, `ui/shell/*` |
| Replay Inspector: listing, dock of replays, player table, expanded rows, chat and raw JSON panels, exports, alternate perspective | `ui/replay_inspector.tsx`, `src/replay_inspector/**` |
| Replay directories and Directory Stats as top-level tabs | `ui/shell/tabs.tsx` |
| Replay renderer, docked in the inspector or popped out to its own window; watched playback; hover minimap preview; batch render | `ui/renderer.tsx`, `src/renderer/**` |
| Tactics Board in its own window | `ui/tactics.tsx`, `src/tactics/**` |
| Launcher | `ui/launcher.tsx`, `src/launcher.rs` |
| Stats with chart docks and Directory Stats | `ui/stats.tsx`, `src/stats/**` |
| Player Tracker | `ui/player_tracker.tsx`, `src/player_tracker/**` |
| Search with the query bar | `ui/search.tsx`, `src/search/**` |
| Command palette | `ui/palette.tsx` |
| Armor Viewer | `ui/armor.tsx`, `ui/armor/*`, `src/armor/**` |
| Resource Unpacker | `ui/unpacker.tsx`, `src/unpacker/**` |
| Settings, game-data cache, replay index | `ui/settings.tsx`, `ui/game_data.tsx`, `src/game_data.rs` |
| Collaboration sessions | `ui/collab.tsx`, `src/collab.rs` |

## Differences from GPUI

These follow from nsh 0.4.0 or from Linux; everything else matches.

- **Drop overlay**: nsh's file-drag-enter event carries no file list (`nsh-core-0.4.0/src/ops.rs:236`), so the overlay cannot name the dragged file before the drop. The drop itself still opens a replay and warns about other files.
- **Window position**: GTK on Wayland can neither read nor set a window's position. Size, maximized and fullscreen are remembered.
- **Updates**: GPUI installs updates only on Windows. The NSH app shows release notes and the release page, as GPUI does on Linux.
- **Copy a replay file**: nsh's clipboard writes text and images only (`nsh-core-0.4.0/src/runtime/clipboard.rs`); file lists go through `arboard`.
- **Tooltips in tests**: `nsh-testing` 0.4.0 does not show tooltips headless, so tooltip text is checked by reading the code, not by tests.

## Tests

Headless interaction tests under `crates/wows-toolkit-nsh/tests/` bundle `ui/App.tsx` and drive it with `nsh_testing`. Producers are tested by sending `SurfaceInput` to their surface and reading the frames back. Tests point the settings database at a temporary directory; a `test-support` build of `wows-toolkit-config` panics if a test reaches the real one.
