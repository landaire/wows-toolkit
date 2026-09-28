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
   reports the builds nothing can read (`missing_builds`), and the app asks the
   repository what it publishes for each of them (`game_data_cache::plan`) before
   offering. The confirmation names each build, its version, how many replays wait
   on it and what the repository has for it (published / nearest / never published
   / unreachable), plus how many objects the whole selection would fetch; the
   download then runs through the same job and progress line the Settings section
   uses, and the listing is walked again once data lands, so the previews and the
   build warnings are redrawn without the reader reopening anything. Each build is
   put to the reader once a session. Not ported: per-build ticks, so the offer is
   all of them or none.
3. ~~**`auto_dump_game_data` is a checkbox over nothing.**~~ Done 2026-09-28:
   loading the installed build writes it to the cache on a thread of its own,
   under the version the install's `preferences.xml` names, skipping a dump
   already there, and copies this app's versioned constants in beside it.
4. **The constants pipeline.** Done in part 2026-09-28: the port fetches as well
   as reads. `crates/wows-toolkit-gpui/src/constants.rs` checks whether a newer
   mapping has been published for the loaded build (through the shared
   `wows_data_mgr::constants::fetch_latest_constants`, which the egui networking
   thread now calls too, against the `constants_file_commit` row both apps keep),
   fetches the mapping for any listed build that has none, and imports a
   `constants.json` the reader points at from the palette. Every write is followed
   by re-reading the open replays, so the figures on screen are the ones the new
   mapping gives. Not ported: the version-mismatch report and its recovery
   (`app.rs:3496-3585`), so a build whose mapping does not fit is read with what
   there is rather than saying so.

## Trust: a control that says one thing and does another

5. ~~**The data-sharing radio.**~~ Done 2026-09-28: a battle that lands is
   contributed as the setting says -- nothing, the per-player builds to
   `/api/ship_builds`, or the file itself to `/api/replays` -- and recorded in the
   `sent_replays` table both apps write, so it is sent once however it was read.
   The rules (eligible battle types, the test-ship refusal, and the grace window a
   raw upload waits for results through) moved to
   `wows_toolkit_viewmodel::upload` with their tests.

   The two apps now share one type and one copy of the rules: the egui crate's
   `DataSharingMode` is the viewmodel's (re-exported from `data/settings.rs`), and
   `task/replay_upload.rs` re-exports `decide_upload_action`,
   `raw_replay_snapshot_state` and their types rather than restating them, keeping
   only what is its own -- the packet-level scan and the sending. The first-run
   consent dialogs landed with item 21, and so did the bulk pass: the palette
   offers Send All Replays to ShipBuilds and a second entry that ignores the
   ledger, which walks what the listing holds, contributes each battle under the
   current setting, names how far it has got in one message it rewrites, and says
   how many were sent.
6. ~~**Batch render.**~~ Done 2026-09-28: the menu item now runs one
   background batch (`replay_renderer::batch_export`) into a folder the reader
   picks, one file per replay, and reports how many were written and how many
   failed. A batch encodes with the reader's saved display defaults and export
   settings (item 33), names the replay it has reached in one message it rewrites
   as it goes, and can put the rendered files on the clipboard instead of into a
   folder (`ui.replay.context.render_to_clipboard_many`). A frame count within the
   replay being rendered waits on a task bar to put it in (item 19).
7. ~~**`check_for_updates` and `enable_logging` govern nothing.**~~ Done
   2026-09-28 with items 11 and 12: the startup update check runs only when
   `check_for_updates` says so, and `enable_logging` is read straight from the
   database before anything else starts, because the log file is what a crash
   during startup would otherwise go unrecorded in.
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

14. **Opening another replay directory.** Done in part 2026-09-28: the header
    and the palette both open a directory the reader picks, the listing reads it as
    it is, and the header says which directory that is until the install's own is
    listed again. Everything that works on the install's listing -- previews,
    opening, indexing, the missing-build offer -- works on it.

    Not ported: workspaces, which is the egui app's shape for this. It opens one
    dock tab per directory, titled by its root, closeable, with "Search these
    replays" on its context menu (`tab_state.rs:797`, `app.rs:300-311`), and keeps
    them across launches; here one listing is replaced by another. The port's
    `AppTab` is a fixed 7-entry strip, so there is also no second Search tab.
15. ~~**Drag and drop.**~~ Done 2026-09-28: a replay dropped on the window opens
    in the inspector, a drop that is not one replay says so, and while the drag
    hovers a scrim over the window names the file it would open, or says one at a
    time when there are several. What the scrim follows is gpui's own drag: an
    entering file drag arrives as a drag of `ExternalPaths`, and a drag that leaves
    is reported as a file-drop event, which a paint-time listener takes.
16. **Tactics Board.** ~2700 lines: map, mode and preset pickers, "Populate Caps
    from Replays", editable capture points, ship placement, range circles
    (`replay/minimap_view/tactics.rs:704`). Absent, and its `WindowKind` geometry
    row is read and never used.
17. ~~**Alt perspective.**~~ Done 2026-09-28: the open replay's Actions menu
    takes another recording of the same battle, refuses one that is not (a different
    version, a different battle, or one whose battle cannot be read, each with its
    own reason), and reads the battle again through both by way of
    `wows_battle_world::merged::MergedReplays`, the same merge the egui app uses.
    The count rides on the menu item as it does there. A playback opened from that
    tab bakes through the same merge, so the map shows what the primary's team never
    saw, and an export of that playback writes what the map shows.
18. **Realtime armor viewer as a window**, with its attacker filter, auto-scroll,
    seek-to-salvo, show-secondaries and sim-agrees controls
    (`replay/realtime_armor_viewer.rs:1198`, `ui.armor.realtime.*`). The hit
    pipeline is ported; the surface is not.
19. **The menu bar and the status bar.** Done 2026-09-28: the strip's trailing
    end carries Check for Updates, About, Create Issue, Discord and Quit, and a
    status strip along the bottom names the two jobs the app owns -- a game-data
    cache job and an index build -- with their progress. A tab's own work (a
    directory scan, a replay parse) is still reported by that tab rather than
    there, where the egui panel names every background task.
20. **Palette is a third of egui's.** Still missing: the three cascading sub-modes
    (search a player, my matches in ship, view armor for ship,
    `ui/command_palette.rs:129-131`).
    Landed 2026-09-28: Copy latest log, Open replay directory, Index All Replays,
    Refresh Persisted Replay Data, Import constants, and both Send All Replays
    entries. "Games I died
    in" now seeds `outcome:loss self.survived:false`, which is the egui seed itself
    (`query_bar::seed::games_i_died_in`) rather than a `survived:false` that also
    returned the wins the reader sank in; a test parses every seeded search and
    compares it against that module.
21. ~~**Dialogs.**~~ Done 2026-09-28: all seven are there. `first_run` asks the
    three questions a fresh run has -- what battle data may be shared, whether to
    share whole replays, and which language to read in, with its
    machine-translation warning -- in the egui app's order and records the answers
    in the same rows, so a reader who answered in either app is not asked again. It
    also holds the refresh-persisted-data confirmation, which the palette now
    offers along with Index All Replays. `notices` holds the two suppressible
    warnings: hosting or joining a session says it reveals this machine's address
    before it does, and an export with no GPU encoder behind it says it will encode
    in software, both writing the suppression rows the egui app reads. A failure
    too long for a toast -- a chain of causes rather than a sentence -- opens the
    error window instead, where it can be read and copied.

## Detail: things that are ported but thinner

22. **Collab.** The session notifications are done (2026-09-27): `poll` returns
    typed `SessionNotice`s the header says after its draw -- started or connected
    by role, joined, left, timed out, ended, error, rejected, and the host
    opening or closing a replay. Still missing: the Tactics Board button, the
    shared-windows list and "Open for everyone", copy-localhost-link, and the
    spam-protection notice (which belongs to the shared viewports).
23. ~~**The live roster's detail.**~~ Done 2026-09-28: the hidden-profile marker is
    the eye with its own hover (and the two other absences are marked too, from the
    catalogue rather than as bare English), and the encounters cell carries the
    hover that says the total, how many fall in the period, and when they were last
    seen. The rest followed the same day: the win-rate cell's hover names both
    scopes with the battles and damage behind each (the wording moved to
    `wows_toolkit_viewmodel::player_tracker::live::win_rate_hover`, which the egui
    roster now calls too), the rating reads as the chip it is everywhere else with
    the scope in its hover, a player the reader has written a note about carries the
    pencil with the note as its hover, each row is tinted by the band of the scope
    it leads with, and the roster scrolls.
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
28. ~~**Cache housekeeping.**~~ Done 2026-09-28: every finished cache job says
    what it found, and the per-build caches (`game_params_<build>.bin`,
    `constants_<build>.json`) are pruned for builds neither the install nor the dump
    cache can open -- the egui app prunes on installed builds alone, which would
    drop a dumped build's cache this port can still use. The three startup passes
    are there too (`game_data_cache::maintain`): the content store's old directory
    name is moved, whole build directories written before that store existed are
    deduplicated into it, and objects nothing references are dropped, all off the UI
    thread and reported to the log, as the egui app runs them.
29. ~~**Preview failures say nothing.**~~ Done 2026-09-27: `PreviewError::said`
    carries the egui popup's four messages (plus one for a render failure the
    egui path cannot have), `PreviewHover` keeps the reason for the watched row,
    and both surfaces draw it where the map would have been.
30. **Annotation keys.** Done in part 2026-09-28: ctrl and a digit take up each
    tool in the egui board's own order, ctrl+m the measurement, escape puts the tool
    down, delete or backspace erases what is picked out, and `[`/`]` narrow and
    widen the nib between 1 and 8 as the egui board holds it. The chords are
    discoverable now: holding ctrl over a viewport puts up the same cheat sheet the
    egui board does, each tool button says what it is and which chord takes it up,
    and the nib has a stepper beside the inks so the bracket keys have something
    visible to move. Ctrl-click row selection in the listing was already there
    (`handle_leaf_click`). Still missing: query-bar pill stepping with the arrows,
    which needs the caret and selection model the egui bar has and this one does
    not (the port's pills are a reading of the query, not handles on it). Every
    chord in the port is a hand-rolled `on_key_down` match rather than a gpui
    action, so none are rebindable.
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
33. ~~**Small persistence holes.**~~ Done 2026-09-28: `SavedRenderOptions` moved
    into the renderer crate beside the options it mirrors, with `apply_to` and
    `read_from` for the two directions, so both apps store the same row through
    the same mapping. The gear's Save Defaults writes what a viewport is showing,
    a viewport opens showing it once its bake lands, and a batch render encodes
    with it rather than with the built-in set. Two fields the port has no control
    over are carried through a save rather than written from it: the self-range
    flags, keyed per entity in the egui renderer and per player name here, and the
    stats panel, whose silhouettes a bake skips. Both apps read the row once and
    the egui app rewrites the whole of it from that copy on its periodic save, so
    a default saved in either is what the other opens with next time it starts,
    and with both running the last save wins. Window geometry, the replay grouping
    choice, Autoload Latest Replay and `current_replay_path` were in this list
    until 2026-09-27 and are now written.
34. ~~**Hardcoded English** where a key exists.~~ Done 2026-09-28: the last of
    them read from the catalogue -- the heals and never-spotted column hovers, the
    consumable table's three headings, the Modules/Loadout/Captain Skills empty
    markers, the replay panel's loading and failed titles, the results-export
    dialog and its confirmation, the unpacker's no-build and written statuses and
    its Extract as JSON dialog, the armor pane's ship-load failure, the Twitch
    paste's empty clipboard, and the unset-directory status (new keys:
    `ui.messages.wows_dir_not_set`, `ui.replay.export_results_title`,
    `ui.replay.results_exported`, `ui.unpacker.parameters_written`,
    `ui.unpacker.prototype_written`, `ui.armor.ship_load_failed`). What English
    remains is English in the egui app too, at the same wording: the skill-hover
    sentences (`util/formatting.rs`), the ship-species names and the splash module
    names, and the armor menu's "Show all hidden plates".
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
38. ~~**Staged ingest progress.**~~ Done 2026-09-28: the listing names every
    stage it goes through. The walk and the header reads were already apart
    ("Scanning replays" with no denominator, then "Reading replay N of M"), and the
    wait for the game data now names the version it is waiting for, read from the
    install's own `preferences.xml` rather than from the load it is waiting on
    (`GameDataStatus::Loading { version }`). A download is reported on the status
    strip rather than in the listing, because in this port it is a job the app owns
    and not a stage of the walk.
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
