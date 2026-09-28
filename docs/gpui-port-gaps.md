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

1. ~~**No replay from an uninstalled build can be opened.**~~ Done 2026-09-28:
   `GameDataCache` falls back to the dump cache through `BuildCas`, keyed by
   build and then by the replay's own version (`dump_for_build`,
   `LoadedGameData::load_dump`), preferring the dump's rkyv params and this app's
   own params cache over a re-parse, and reading the dump's translations. Not yet
   ported from the egui path: the forward-only constants bridge between loaded
   builds, and the LRU eviction of non-main builds (the port keeps every build it
   has loaded for the session).
2. **Download offer for a missing build.** Done in part 2026-09-28: a scan
   reports the builds nothing can read (`missing_builds`), and the app offers to
   fetch them in one confirmation naming each build, its version and how many
   replays wait on it, then downloads through the same job and progress line the
   Settings section uses. Not ported: the per-build remote availability report
   (published / nearest / never published / unreachable), the object-count plan
   before the reader commits, per-build ticks, and the follow-up that reopens the
   replay or re-walks the directory once the data lands (the reader re-opens it
   themselves).
3. ~~**`auto_dump_game_data` is a checkbox over nothing.**~~ Done 2026-09-28:
   loading the installed build writes it to the cache on a thread of its own,
   under the version the install's `preferences.xml` names, skipping a dump
   already there, and copies this app's versioned constants in beside it.
4. **The constants pipeline is read-only.** The egui app fetches latest and
   per-build constants (`task/networking.rs:184-200`), detects a version
   mismatch and recovers (`app.rs:3496-3585`), and can import a `constants.json`
   by hand (`app.rs:3591-3644`). The port only reads
   `constants_{build}.json` (`load.rs:387-412`). A port-only user decodes post
   battle results through whatever mapping was last written by the egui app, with
   no warning that it is stale.

## Trust: a control that says one thing and does another

5. **The data-sharing radio.** Done 2026-09-28: a battle that lands is
   contributed as the setting says -- nothing, the per-player builds to
   `/api/ship_builds`, or the file itself to `/api/replays` -- and recorded in the
   `sent_replays` table both apps write, so it is sent once however it was read.
   The rules (eligible battle types, the test-ship refusal, and the grace window a
   raw upload waits for results through) moved to
   `wows_toolkit_viewmodel::upload` with their tests.

   Two things remain. The egui path still carries its own copy of those rules,
   because this crate's `DataSharingMode` is its own type rather than the
   viewmodel's; unifying the two lets `task/replay_upload.rs` call the shared
   module instead. And "Send all replays to ShipBuilds" (the palette's bulk pass
   over a whole directory, `app.rs:4712`) and the first-run consent dialogs are
   still absent, so the setting is only acted on for battles that land while the
   app is open.
6. **Batch render.** Done in part 2026-09-28: the menu item now runs one
   background batch (`replay_renderer::batch_export`) into a folder the reader
   picks, one file per replay, and reports how many were written and how many
   failed. Not ported: per-replay progress (the egui app has a task bar entry for
   it, which this port has no bar for -- item 19), batch-to-clipboard
   (`video_export.rs:488`), and the renderer's saved display defaults, so a batch
   renders with the default options rather than the reader's.
7. **`check_for_updates` and `enable_logging` govern nothing.** No updater
   (item 11) and no log file (item 12) exist in the port.
8. **Auto-export.** Done 2026-09-28: a replay the watcher reports is parsed and
   written out whether or not it is opened (`auto_export_landed`), skipped when it
   carries no battle results, which is the egui gate. It is still named by the
   replay's stem rather than egui's `better_file_name` (map, mode and ship), and
   both of the port's export paths agree on that name.
9. ~~**"Set as Session Stats" destroys the session on one click.**~~ Done
   2026-09-27: it opens the same confirmation the port already uses for Open in
   Game, on the `confirm.set_as_session_stats` wording.
10. ~~**The session game-count limit means something different.**~~ Done
    2026-09-27: `filter_games_per_ship` counts the limit per ship, and the
    per-ship table and the charts read it while the summary keeps the
    session-wide count, which is the split the egui tab makes.

## Recovery: what a stuck user has no way out of

11. **The updater.** Done 2026-09-28: a startup check (when the setting says
    so) and a manual one from the menu, the release offered with its notes, the
    install that renames the running executable to `.old`, moves the new one in
    and restarts it, and `finalize-update --replaced` -- accepted both as this
    build spawns it and as the bare path every released version spawns it -- which
    deletes only this app's own `.old` beside this executable. The version
    comparison, the release shape and that delete rule are
    `wows_toolkit_viewmodel::update`, shared with the egui app.

    Not ported: per-byte download progress (the download is one request and the
    reader is told it started), and About is there but has no window of its own --
    it is a dialog.
12. ~~**No crash reporting, no log file, no Copy Latest Log.**~~ Done
    2026-09-28: `logging.rs` writes the same hourly-rotated file beside the
    executable and the same panic log in the shared storage directory the egui app
    uses, the next launch reports what a crash left and offers to copy it, and the
    palette copies the newest log. `enable_logging` governs the file and now
    defaults on, as it does in the egui app.
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
15. **Drag and drop.** Done in part 2026-09-28: a replay dropped on the window
    opens in the inspector, and a drop that is not one replay says so. The egui
    app's full-window scrim while the file hovers is not drawn yet.
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
    Refresh persisted data, Index all replays, Send all replays (x2). Copy latest
    log landed 2026-09-28. Also a seed bug: the port's "Games I died in" sends `survived:false`
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

22. **Collab.** The session notifications are done (2026-09-27): `poll` returns
    typed `SessionNotice`s the header says after its draw -- started or connected
    by role, joined, left, timed out, ended, error, rejected, and the host
    opening or closing a replay. Still missing: the Tactics Board button, the
    shared-windows list and "Open for everyone", copy-localhost-link, and the
    spam-protection notice (which belongs to the shared viewports).
23. **The live roster's detail.** Done 2026-09-28: the hidden-profile marker is
    the eye with its own hover (and the two other absences are marked too, from the
    catalogue rather than as bare English), and the encounters cell carries the
    hover that says the total, how many fall in the period, and when they were last
    seen. Still missing: the win-rate hover comparing both scopes, the PR chip and
    its hovers, the notes pencil on a roster row, the win-rate row tint, and a
    scrollbar on the Detailed roster
    (`ui/player_tracker/current_match.rs:471`, `:706`, `:741`, `:760`).
24. ~~**Tracker: no Clear Stats and no Populate Data From Replays.**~~ Done
    2026-09-28: the toolbar carries both. Clear asks first and then drops every
    player with their aliases, encounters and notes (`tracker::clear_tracker`, one
    transaction, children first); the other re-reads the index, which is where the
    port's rows come from in the first place.
25. ~~**Twitch: observations are never pruned, and no re-poll on an edit.**~~
    Done 2026-09-28: a poll prunes sightings older than the same thirty days the
    egui app keeps, and pasting a credential or editing the channel restarts the
    poll. The monitored channel is also written back now; the field was seeded and
    never saved.
26. ~~**The replay watcher handles create and rename only.**~~ Done 2026-09-27:
    a change re-reads the row and re-parses a tab open on it
    (`ReplayPanel::reparse`), a removal drops the row with its marking and
    highlight, and both watcher failures are toasted in the egui app's words
    through a typed `WatchFailure`.
27. ~~**Indexing.**~~ Done 2026-09-28: a battle that lands is indexed as it
    lands, Build Index skips what is already in (and says how many it skipped),
    Re-index Everything reads the lot again for rows an older parse got wrong, the
    unreadable-file ledger moved to `wows_toolkit_config::index::unindexable` so
    both apps share it, and the walk leaves `temp.wowsreplay` alone.
28. **Cache housekeeping.** Reporting is done 2026-09-28: every finished cache
    job says what it found, clean or not, in the egui app's words. Still missing:
    the startup migration of an old cache layout, `migrate_to_cas`,
    `gc_unreferenced` (`app.rs:1502-1525`), and the pruning of
    `game_params_{build}.bin` for builds no longer installed
    (`util/game_params.rs:59`) although the port writes those files
    (`load.rs:104`).
29. ~~**Preview failures say nothing.**~~ Done 2026-09-27: `PreviewError::said`
    carries the egui popup's four messages (plus one for a render failure the
    egui path cannot have), `PreviewHover` keeps the reason for the watched row,
    and both surfaces draw it where the map would have been.
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
32. **Toast mechanics.** Sticky and arm-once are done (2026-09-27):
    `toast::stuck(key, ...)` stays up and keeps one message per state, and
    `toast::resolved(key, ...)` takes it down when the state is fixed, which the
    invalid directory and the refused Twitch credential both now use. Still
    missing: a counterpart to egui's catch-all task-error funnel
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
36. ~~**Proxy auto-detection.**~~ Done 2026-09-28: the resolution moved to
    `wows_toolkit_viewmodel::proxy`, so both apps read the reader's setting, then
    the standard environment variables, then the Windows configuration, and the
    port applies the bypass list too. The egui app's `util::proxy` is a re-export
    of it.
37. ~~**A live OS theme switch is not followed.**~~ Done 2026-09-28: the window's
    appearance observer re-applies the palette while the theme is "System".
38. **Staged ingest progress.** The egui app reports Scanning, Downloading game
    data, Loading game data for a version, and Reading (`task/replays.rs:1579`);
    the port reports scan and read counts only, so a build being fetched or
    decoded looks like a stalled scan.
39. ~~**`clear_fire_section_failures` has no caller.**~~ Done 2026-09-28: it is
    cleared with the rest of what the old cache directory told us.

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
