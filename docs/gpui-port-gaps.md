# GPUI port: what the egui app has and the port does not

Audited 2026-09-27 against `crates/wows-toolkit` (the shipping egui app) by
enumerating that app's own vocabulary -- its tab enum, background task kinds,
palette actions, settings struct, keyboard handling, and every key in
`crates/wt-translations/translations/en.toml` -- and reading both sides for each
one. `docs/gpui-port-parity.md` is the pane-by-pane record of the port's UI work
and states its own scope: it was written against `crates/wows-toolkit/src/ui`
and `src/armor_viewer`. That scope is why most of what follows is absent from it:
nothing had looked at `main.rs`, `cli.rs`, `gpu/`, `hardening/`, `app.rs`'s
window and menu layer, or `replay/minimap_view/`.

A feature is "absent" here only where a negative search for the egui symbol and
its translation key was confirmed by reading the port's code path that would
have used it.

## Blocking: the port cannot do the thing it is for

1. **No replay from an uninstalled build can be opened.** The egui app falls
   back to the `wows-data-mgr` dump cache when a replay's build is not under
   `bin/<build>` (`data/wows_data.rs:567-600`, `data/build_data.rs:378`
   `from_dump`, cross-region version fallback, LRU eviction of non-main builds).
   The port hard-gates on `list_available_builds` and returns
   `UnsupportedVersion` (`replay_inspector/load.rs:91-94`, `:316`). No
   `BuildCas`, `cas_vfs` or `BuildsIndex` reference exists in the port at all.
   Everything below about old replays is downstream of this, and it makes the
   port's own game-data cache section decorative.
2. **No download offer for a missing build.** The egui app plans and offers the
   fetch, per build, with replay counts and remote availability
   (`app.rs:583`, `:590-665` `draw_download_prompt`, `:2363-2404`,
   `task/game_data_download.rs`), then reopens the replay or re-walks the
   directory once data lands. Absent: the port calls `download_repo` only for
   builds already in the cache (`game_data_cache.rs:323`).
3. **`auto_dump_game_data` is a checkbox over nothing.** It saves and persists
   (`app.rs:1363`) but nothing calls `wows_data_mgr::dump::dump_renderer_data`,
   which is what the egui app runs per build load (`task/replays.rs:308-338`).
   A user who only ever runs the port never accumulates the cache that item 1
   would read.
4. **The constants pipeline is read-only.** The egui app fetches latest and
   per-build constants (`task/networking.rs:184-200`), detects a version
   mismatch and recovers (`app.rs:3496-3585`), and can import a `constants.json`
   by hand (`app.rs:3591-3644`). The port only reads
   `constants_{build}.json` (`load.rs:387-412`). A port-only user decodes post
   battle results through whatever mapping was last written by the egui app, with
   no warning that it is stale.

## Trust: a control that says one thing and does another

5. **The data-sharing radio is inert.** Off / Build data / Replays persists and
   is shown (`app.rs:1819-1834`) and nothing reads it: there is no ShipBuilds
   client, no `/api/ship_builds`, no `/api/replays`, no sent-replay ledger
   (egui: `data/shipbuilds.rs`, `task/replay_upload.rs`, dispatched
   `app.rs:4712`). A user who selects "send replays" is told they are
   contributing and nothing is sent; a user who selects "off" gets the same
   behaviour. This is the most serious divergence on the list after item 1.
6. **"Render N Replays to Video" does not render.** The listing offers
   `ui.replay.context.render_to_video_many` and then emits one `RenderReplay`
   per path (`replay_inspector/browser_view.rs:1099-1111`), so marking 50
   replays opens 50 viewport tabs and bakes 50 previews rather than writing 50
   files. The egui app runs one background batch into a chosen folder
   (`replay/renderer/video_export.rs:458`, progress
   `ui.task.batch_render_progress`); it also has batch-to-clipboard (`:488`).
7. **`check_for_updates` and `enable_logging` govern nothing.** No updater
   (item 11) and no log file (item 12) exist in the port.
8. **Auto-export does not fire on a finished match.** The egui app writes from
   the background parser for every replay the watcher reports, gated on results
   being present (`task/replays.rs:521`, `:558-597`). The port writes only when
   the reader opens that replay in a tab (`replay_inspector/panel.rs:396`), skips
   the results gate, and names the file by its stem rather than
   `better_file_name`.
9. **"Set as Session Stats" destroys the session on one click.** The egui app
   routes it through `ConfirmableAction::SetAsSessionStats` and the Confirm
   window (`ui/replay_parser/mod.rs:292-299`, executed `app.rs:3995`); the port
   emits `SessionStats { replace: true }` straight from the menu item
   (`browser_view.rs:1141-1146`).
10. **The session game-count limit means something different.** The egui app
    applies it per ship for the per-ship tables and every chart
    (`data/session_stats.rs:324-359`); the port applies one global last-N
    everywhere (`wows-toolkit-viewmodel/src/stats/mod.rs:357-368`). Same
    control, different numbers, nothing said.

## Recovery: what a stuck user has no way out of

11. **No updater and no About.** Startup and manual check, the release window
    with notes, install with progress, the rename to `.old`, the relaunch, and
    `finalize-update --replaced` with its path validation and legacy bare-path
    form (`app.rs:3421-3474`, `:2266-2291`, `task/networking.rs:601`,
    `cli.rs:40-155`). No released version can update into the port, and the port
    cannot tell a user a new version exists.
12. **No crash reporting, no log file, no Copy Latest Log.** The egui app keeps
    an hourly-rotated log and a panic log, shows a Crash Detected window with
    the last run's backtrace, and can copy the newest log
    (`app.rs:1569-1583`, `:3362`, `:3477`, `:3650`). The port logs to stderr
    only (`main.rs:74-76`).
13. **No renderer or adapter control, and no process hardening.** The egui app
    resolves a six-rung ladder and remembers per machine which rung worked
    (`gpu/select.rs:19-35`, `boot.rs`), pins the Vulkan ICD
    (`main.rs:302-315`), ranks Vulkan above DX12 to avoid the DXGI window-drag
    stutter (`main.rs:371-376`), applies three Windows mitigations before any
    window exists (`hardening/mod.rs:104-120`) with a `code_integrity` setting,
    and exposes all of it through the five flags in `cli.rs`. The port has none
    of it: gpui's backend with default options, and the armor viewport standing
    up an independent `wgpu` device (`viewport/device.rs:33-39`) that can land on
    a different adapter than the UI. `code_integrity` is the only egui settings
    field the port has no control for, and it is moot until the mitigations
    exist. The port now takes `--help` and `--version` and refuses the rest by
    name (`crates/wows-toolkit-gpui/src/cli.rs`) rather than ignoring it, and
    reports that message the way a release build can show it (own handle, then
    the parent's console, then a message box).

## Reach: surfaces and entry points

14. **Only one replay directory, ever.** No picker for an arbitrary directory
    (egui `app.rs:4455`), no workspaces -- the egui app opens one dock tab per
    directory, titled by its root, closeable, with "Search these replays" on its
    context menu (`tab_state.rs:797`, `app.rs:300-311`). The port's `AppTab` is a
    fixed 7-entry strip (`app.rs:89-141`, `:2235-2257`): nothing closeable,
    nothing reorderable, no second listing and no second Search tab.
15. **No drag and drop.** The egui app takes a replay dropped on the window with
    a full-window scrim (`app.rs:2979-3037`). Nothing in the port handles a drop.
16. **Tactics Board.** ~2700 lines: map, mode and preset pickers, "Populate Caps
    from Replays", editable capture points, ship placement, range circles
    (`replay/minimap_view/tactics.rs:704`). Absent, and its `WindowKind` geometry
    row is read and never used.
17. **Alt perspective.** Pick another recording of the same battle, validate
    version and arena id, merge, re-parse, and feed the renderer and video export
    (`ui/replay_parser/mod.rs:4114-4172`, `replay/renderer/playback.rs:205`).
    Absent.
18. **Realtime armor viewer as a window**, with its attacker filter, auto-scroll,
    seek-to-salvo, show-secondaries and sim-agrees controls
    (`replay/realtime_armor_viewer.rs:1198`, `ui.armor.realtime.*`). The hit
    pipeline is ported; the surface is not.
19. **No menu bar and no status bar.** File > Check for updates / About / Quit,
    Create issue, Discord (`app.rs:4751-4782`), and the bottom panel that shows
    every background task's spinner and progress (`app.rs:1604`). A background
    task can fail in the port with nothing on screen.
20. **Palette is a third of egui's.** Missing: the three cascading sub-modes
    (search a player, my matches in ship, view armor for ship,
    `ui/command_palette.rs:129-131`), Open replay directory, Import constants,
    Refresh persisted data, Copy latest log, Index all replays, Send all replays
    (x2). Also a seed bug: the port's "Games I died in" sends `survived:false`
    where the egui seed is loss AND not survived
    (`palette.rs:53` against `wows-toolkit-viewmodel/src/query_bar/seed.rs:108`),
    so it also returns wins the reader sank in.
21. **Dialogs.** Build-data consent, replay migration, language selection with
    its machine-translation warning, refresh-persisted-data, the P2P IP warning,
    the readable/copyable Error window, and the GPU-encoder-unavailable notice
    (`app.rs:3161`, `:3178`, `:3233`, `:3202`, `:4010`, `:3278`,
    `replay/renderer/mod.rs:3342`). All absent; the encoder one means an export
    silently drops to CPU encoding.

## Detail: things that are ported but thinner

22. **Collab is silent.** Thirteen session notifications -- started, joined,
    left, timed out, ended, error, rejected, connected, host opened/closed a
    replay, spam protection -- are `tracing::info!` only (`collab.rs:283-307`
    against `app.rs:4166-4402`). Also missing: the Tactics Board button, the
    shared-windows list and "Open for everyone", copy-localhost-link.
23. **The live roster lost what made it useful**: the win-rate hover comparing
    both scopes with battles and average damage, the hidden-profile eye and its
    hover (the port draws the literal English "hidden"), the PR chip and its
    hovers, the encounters hover, the notes pencil on a roster row, and the
    win-rate row tint that shows a stacked lobby at a glance
    (`ui/player_tracker/current_match.rs:471-473`, `:654`, `:702-710`,
    `:741-743`, `:760-771`, `:839-844`). The Detailed roster also has no
    scrollbar (`player_tracker/mod.rs:2199-2209`).
24. **Tracker: no Clear Stats and no Populate Data From Replays**
    (`ui/player_tracker/historical.rs:471-481`, `:518-534`). No way to wipe or
    force-backfill.
25. **Twitch: observations are never pruned** (egui prunes at
    `task/networking.rs:669`), so the table only grows; and the poll is started
    once (`app.rs:529`) with no restart on a token or channel edit, where the
    egui app re-polls immediately (`networking.rs:746-786`).
26. ~~**The replay watcher handles create and rename only.**~~ Done 2026-09-27:
    a change re-reads the row and re-parses a tab open on it
    (`ReplayPanel::reparse`), a removal drops the row with its marking and
    highlight, and both watcher failures are toasted in the egui app's words
    through a typed `WatchFailure`.
27. **Indexing.** Nothing indexes as replays are read (egui does it per replay,
    `task/replays.rs:540-552`), Build Index re-parses every replay every run (no
    skip of indexed rows), there is no re-index/repair pass for rows decoded
    through the wrong constants (`IndexWriteMode::Replace`), no unindexable
    blacklist (`data/replay_reconcile.rs:248`), and `replays_under` walks
    `temp.wowsreplay` so a live battle counts as a failure
    (`replay_index.rs:50-67`).
28. **Cache housekeeping.** No startup migration of an old cache layout, no
    `migrate_to_cas`, no `gc_unreferenced` (`app.rs:1502-1525`), and no pruning
    of `game_params_{build}.bin` for builds no longer installed
    (`util/game_params.rs:59`) although the port writes those files
    (`load.rs:104`). A clean Check or Validate also reports nothing at all
    (`app.rs:1240-1270`): only failure gets a line.
29. **Preview failures say nothing.** The egui hover preview reports why there is
    none -- no game data for that version, unreadable version, no map data
    (`replay/renderer/preview.rs:62-68`); the port logs at `debug!`
    (`preview_hover.rs:419-421`), so an old replay looks like a broken preview.
30. **Annotations are mouse-only.** `Ctrl+1`..`Ctrl+7`, `Ctrl+M`, the Ctrl-held
    cheat sheet, `Escape`, `Delete`, `[`/`]`, and annotation undo
    (`replay/minimap_view/shapes.rs:84-131`, `replay/renderer/mod.rs:2162-2252`).
    Query-bar pill stepping with the arrows and `Ctrl`-click row selection are
    also unbound. Every chord in the port is a hand-rolled `on_key_down` match
    rather than a gpui action, so none are rebindable or discoverable.
31. **Armor viewer**: no export options at all (contents, LOD, resolution, camo,
    size estimate -- `armor_viewer/export_dialog.rs:426` against the hardcoded
    `DEFAULT_LOD` at `armor_viewer/pane.rs:710`), no Analysis window with its
    ships and trajectory tabs, no camera-perspective mode with FOV, no lighting
    colour pickers, no ship-center toggle, no marker opacity, no Save Defaults.
32. **Toast mechanics.** No duration control, no sticky toast, no dismiss-all,
    and no arm-once gate, so the egui app's two deliberately permanent settings
    warnings (invalid WoWs directory, refused Twitch credential) auto-dismiss
    here. There is also no counterpart to egui's catch-all task-error funnel
    (`app.rs:2465`), so a failed background task can be entirely silent.
33. **Small persistence holes.** Every renderer display default
    (`ui.renderer.settings.save_defaults` and its five families) is not written
    back. Window geometry, the replay grouping choice, Autoload Latest Replay and
    `current_replay_path` were in this list until 2026-09-27 and are now
    written.
34. **Hardcoded English** where a key exists. Fixed 2026-09-27: the replay
    grouping labels, the tracker's six period labels, the live roster's scoped
    column headings and its Overall/Ship and Compact/Detailed labels, the live
    roster's Twitch chip hover, and the Stats dock-tab titles all read from the
    catalogue now. Still English: the tracker's sub-tab names, and the roster's
    "hidden" marker (which item 23 replaces with the eye icon anyway).
35. ~~**Window chrome.**~~ Done 2026-09-27: the window is titled
    `WoWs Toolkit v<version>`, the executable carries the icon (a build script
    under Cargo, `assets/wows_toolkit_gpui.rc` under Buck), and a release build
    is a windows-subsystem binary, with the CLI's messages routed so they are
    still visible.
36. **Proxy auto-detection** from the environment and the Windows registry
    (`util/proxy.rs`, `app.rs:1256-1282`); the port honours only the manual
    setting (`http.rs:31`).
37. **A live OS theme switch is not followed.** `apply_egui_theme` is called at
    startup, on a theme change and on a zoom drag only; there is no appearance
    observer, where egui's `ThemePreference::System` follows the desktop. Read
    from code, not verified by running.
38. **Staged ingest progress.** The egui app reports Scanning, Downloading game
    data, Loading game data for a version, and Reading (`task/replays.rs:1579`);
    the port reports scan and read counts only, so a build being fetched or
    decoded looks like a stalled scan.
39. **`clear_fire_section_failures` has no caller** in the port, so a
    fire-chance section that failed against the old cache directory stays failed
    until restart (egui clears it on a cache-dir change, `tab_state.rs:1345`).

## Corrections to `docs/gpui-port-parity.md`

- The palette is on ctrl+k and ctrl+p in both apps. Neither binds
  ctrl+shift+p. Same error in `crates/wows-toolkit-gpui/src/palette.rs:3`.
- `auto_dump_game_data` does have a checkbox (`app.rs:1363`). The real gap is
  that nothing dumps.
- "the egui tab walks the cache inline every frame it draws" is false: it
  memoises into `tab_state.game_data_cache_stats` and walks once per
  invalidation (`ui/settings_tab.rs:290-296`, `tab_state.rs:1342-1345`). Same
  wording is repeated in `game_data_cache.rs:10-12`.
- "Export options are still session-only" implies an options surface exists.
  There is none.
- The replay-watcher entry reads as parity; it handles create and rename only.
- The auto-export entry reads as parity; it fires on opening a replay, not on a
  finished match.
- `replay_inspector/view.rs:160-164` still says the port has no replay-directory
  watcher. It has one.
