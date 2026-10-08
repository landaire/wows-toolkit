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

Every item is closed as of 2026-09-28. Item 13 is closed as far as it goes
rather than matched: gpui chooses the adapter it draws the window through and
offers no say in it, so there is no ladder to port, and the flags that still mean
something here steer the one device this port does pick.

## Blocking: the port cannot do the thing it is for

1. ~~**No replay from an uninstalled build can be opened.**~~ Done 2026-09-28:
   `GameDataCache` falls back to the dump cache through `BuildCas`, keyed by
   build and then by the replay's own version (`dump_for_build`,
   `LoadedGameData::load_dump`), preferring the dump's rkyv params and this app's
   own params cache over a re-parse, and reading the dump's translations. Not yet
   ported from the egui path: the forward-only constants bridge between loaded
   builds, and the LRU eviction of non-main builds (the port keeps every build it
   has loaded for the session).
2. ~~**Download offer for a missing build.**~~ Done 2026-09-28: a scan
   reports the builds nothing can read (`missing_builds`), and the app asks the
   repository what it publishes for each of them (`game_data_cache::plan`) before
   offering. The confirmation names each build, its version, how many replays wait
   on it and what the repository has for it (published / nearest / never published
   / unreachable), plus how many objects the whole selection would fetch; the
   download then runs through the same job and progress line the Settings section
   uses, and the listing is walked again once data lands, so the previews and the
   build warnings are redrawn without the reader reopening anything. Each build is a
   row the reader can untick, so an offer of six can be answered with two, and a
   build is only spent once the reader has said yes to it: a cancelled offer or a
   failed download leaves it worth asking about again.
3. ~~**`auto_dump_game_data` is a checkbox over nothing.**~~ Done 2026-09-28:
   loading the installed build writes it to the cache on a thread of its own,
   under the version the install's `preferences.xml` names, skipping a dump
   already there, and copies this app's versioned constants in beside it.
4. ~~**The constants pipeline.**~~ Done 2026-09-28: the port fetches as well as
   reads. `crates/wows-toolkit-gpui/src/constants.rs` checks whether a newer
   mapping has been published for the loaded build (through the shared
   `wows_data_mgr::constants::fetch_latest_constants`, which the egui networking
   thread now calls too, against the `constants_file_commit` row both apps keep),
   fetches the mapping for any listed build that has none, and imports a
   `constants.json` the reader points at from the palette. Every write is followed
   by re-reading the open replays, so the figures on screen are the ones the new
   mapping gives. A battle read through a mapping that is not its build's is
   reported and recovered from, once per build: the judgement moved to
   `wows_toolkit_viewmodel::index_rows::constants_fit`, which the egui app calls
   too, the stale file is dropped so it cannot stand in for a fetched one, and the
   right mapping is fetched. That fit is also what the index row carries, where the
   port used to store every row as though its mapping fitted, so results decoded
   through keys that had moved were kept rather than suppressed.

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
8. ~~**Auto-export.**~~ Done 2026-09-28: a replay the watcher reports is parsed
   and written out whether or not it is opened (`auto_export_landed`), skipped
   when it carries no battle results, which is the egui gate. It is named by
   ship, map, scenario, mode and time, the same name the egui app writes, through
   the shared `wows_toolkit_viewmodel::replay_export::exported_file_stem`; all
   three of the port's export paths -- the watcher's, the tab's own auto-export
   and the Export menu's save dialog -- agree on it, so a directory both apps
   write into reads as one set.
9. ~~**"Set as Session Stats" destroys the session on one click.**~~ Done
   2026-09-27: it opens the same confirmation the port already uses for Open in
   Game, on the `confirm.set_as_session_stats` wording.
10. ~~**The session game-count limit means something different.**~~ Done
    2026-09-27: `filter_games_per_ship` counts the limit per ship, and the
    per-ship table and the charts read it while the summary keeps the
    session-wide count, which is the split the egui tab makes.

## Recovery: what a stuck user has no way out of

11. ~~**The updater.**~~ Done 2026-09-28: a startup check (when the setting says
    so) and a manual one from the menu, the release offered with its notes, the
    install that renames the running executable to `.old`, moves the new one in
    and restarts it, and `finalize-update --replaced` -- accepted both as this
    build spawns it and as the bare path every released version spawns it -- which
    deletes only this app's own `.old` beside this executable. The version
    comparison, the release shape and that delete rule are
    `wows_toolkit_viewmodel::update`, shared with the egui app.

    The download reports itself as it arrives, written to disk chunk by chunk
    with a message saying how much of the release has landed, or how much so far
    where the server did not say how much there is.

    One difference stands, deliberately: About is a dialog here rather than a
    window of its own. It carries the same lines and the same link; a window
    would be a second thing to close for a paragraph nobody keeps open.
12. ~~**No crash reporting, no log file, no Copy Latest Log.**~~ Done
    2026-09-28: `logging.rs` writes the same hourly-rotated file beside the
    executable and the same panic log in the shared storage directory the egui app
    uses, the next launch reports what a crash left and offers to copy it, and the
    palette copies the newest log. `enable_logging` governs the file and now
    defaults on, as it does in the egui app.
13. ~~**No renderer or adapter control.**~~ Done 2026-09-28.
    The process hardening is done
    2026-09-28: it moved to `crates/wows-toolkit-hardening`, which both front ends
    now apply -- extension points disabled, image loads restricted and code
    integrity guard applied before any window exists, with the text-service probe
    that decides whether the last of those would block an input method the reader
    types with. The `code_integrity` setting has its control in the port's Settings
    tab, writing the row the egui app reads, and both apps report what each policy
    did to the log.

    What has no counterpart here is the renderer ladder. The egui app resolves six
    rungs, remembers which one worked, pins the Vulkan ICD and ranks Vulkan above
    DX12 to dodge the DXGI window-drag stutter (`gpu/select.rs`, `main.rs:302-376`);
    gpui draws through Direct3D 11 on Windows and walks the adapters itself
    (`gpui-pre-windows`'s `directx_devices.rs`), with nothing an application can
    choose, so there is no ladder to port and no ICD to pin.

    The armor viewport stands up a `wgpu` device of its own, though, and that one
    is the port's to pick: `--gpu-adapter NAME` puts it on the adapter whose name
    contains NAME, `--cpu-renderer` puts it on WARP, `--list-gpus` says what this
    machine offers and exits, and the device says in the log which adapter it got,
    since nothing can make it and the window agree. `--no-hardening` skips the
    process mitigations, which this port applies and had no way to turn off.
    `--gpu-safe-mode` names a rung of the ladder that is not here, so it is still
    refused by name rather than parsed and ignored.

## Reach: surfaces and entry points

14. ~~**Opening another replay directory.**~~ Done 2026-09-28: the header
    and the palette both open a directory the reader picks, the listing reads it as
    it is, and the header says which directory that is until the install's own is
    listed again. Everything that works on the install's listing -- previews,
    opening, indexing, the missing-build offer -- works on it.

    Workspaces landed the same day: a directory the reader opens is listed in a
    tab of its own in the inspector's dock, titled by its root and closeable,
    beside the install's own listing rather than in place of it. The tab carries
    "Search these replays", which runs the query naming the index source that
    directory was read under (`query_bar::seed::source_scoped`) and says so when
    nothing has indexed it yet. Its listing raises the same events the sidebar's
    does and is handled through the same arm, so a replay opens from an archive
    exactly as it does from the install. Neither app keeps the open directories
    across launches; the egui one has the same gap
    (`tab_state.rs`'s `workspaces` is session state).

    One difference stands: the egui Search tab is a closeable dock tab that can
    be pushed again, where the port's is a fixed entry in the tab strip and so is
    always there. Nothing is missing, and there is no second Search tab in either
    (`Tab::Search` is one variant).
15. ~~**Drag and drop.**~~ Done 2026-09-28: a replay dropped on the window opens
    in the inspector, a drop that is not one replay says so, and while the drag
    hovers a scrim over the window names the file it would open, or says one at a
    time when there are several. What the scrim follows is gpui's own drag: an
    entering file drag arrives as a drag of `ExternalPaths`, and a drag that leaves
    is reported as a file-drop event, which a paint-time listener takes.
16. ~~**Tactics Board.**~~ Done 2026-09-28: the app menu opens one in a
    window of its own, which now uses the `WindowKind::TacticsBoard` geometry row
    that was read and never used. It is set on a map and one of that map's game
    modes, drawn through the same renderer the battle is, and its capture points
    are the reader's: placed, moved, widened, handed to a side and taken off
    again. What is on it can be drawn over with the same tools and inks the
    replay viewport draws with, ships can be placed with the range circles their
    own params state, and a visible Blank mode clears the selected layout without
    requiring a second click on the active mode. The whole board saves and
    reopens through the preset format the egui board reads and writes
    (`wows_toolkit_viewmodel::tactics::preset`). "Populate caps from replays"
    reads the replay directory for the layouts of the modes they were played in,
    reporting as it goes, and writes them to the cache both apps share
    (`wows_replay_insights::cap_layout`, moved out of the egui crate).

    The board zooms about the pointer, pans with a drag past every zone, and
    goes back to the whole map on a double click, through the same window the
    replay viewport uses (`MapViewport`).

    A change to what is on the board can be taken back and put back again, by
    the buttons or by Ctrl+Z and Ctrl+Y, and a zone's width reads and steps in
    kilometres beside the cap it belongs to. Escape puts the tool down and
    Delete erases the cap picked out, which are the egui board's own chords.

    A drawn shape can be picked back up: a press on one picks it out and draws a
    halo under it, ctrl adds another, a drag moves what is picked, and Delete
    erases it. One picked shape that has a bearing carries the handle it is
    turned by, the same handle the replay viewport draws, at the same size and
    reach at every zoom. The two directions between the map and the element are
    one reading (`Letterbox`), so the handle is pressed where it was drawn.

    The board is a session's as much as a reader's: what it is set on, its
    capture points and what is drawn on it all travel, and it draws from art a
    peer sent where this build ships none. See item 22.

    Not there: the map pings and the grid, which belong to a session rather than
    to a board.
17. ~~**Alt perspective.**~~ Done 2026-09-28: the open replay's Actions menu
    takes another recording of the same battle, refuses one that is not (a different
    version, a different battle, or one whose battle cannot be read, each with its
    own reason), and reads the battle again through both by way of
    `wows_battle_world::merged::MergedReplays`, the same merge the egui app uses.
    The count rides on the menu item as it does there. A playback opened from that
    tab bakes through the same merge, so the map shows what the primary's team never
    saw, and an export of that playback writes what the map shows.
18. ~~**Realtime armor viewer.**~~ Done 2026-09-28: the Incoming Fire log is
    there, in the panel this port already reads its armor questions in rather
    than a window of its own. It lists what was fired at the ship salvo by salvo
    -- who fired, when its first shell landed, and what the server said each one
    did -- narrows to one attacker, counts or ignores secondary armament, and
    moves the playback behind it to the moment a salvo landed. The grouping and
    the outcome names are shared (`wows_toolkit_viewmodel::armor::incoming`),
    including `ServerOutcome`, which moved out of the egui crate.

    The health strip is there too, above the log: the ship's health across the
    battle with a tick where each shell landed and a mark where playback has
    reached, and a press along it moves playback to that moment. Its shape is
    read by `wows_toolkit_viewmodel::armor::health_strip`, which both apps use.

    Each armour-piercing shell in the log also carries what this app's own
    simulation made of it: agrees, an RNG-zone angle where either call is right,
    or the two verdicts where they differ. The shell is cast back along the way
    it arrived -- how far it flew and which way it was going come from
    `wows_replay_insights::hull_impact::shell_arrival`, off the salvo rather
    than off the post-impact terminal ballistics -- through the hull on screen,
    and compared by `wows_toolkit_viewmodel::armor::arc::compare_with_server`,
    which moved out of the egui crate along with `sim_outcome` and the verdict
    types. Run once as the ship loads, since the log reads the whole battle
    rather than where playback stands, and only against the hull the shells
    actually struck. A shell the check cannot answer for is counted and said
    rather than left as a blank line, and an outcome the simulation does not
    model no longer reads as agreement.

    Placing an impact on a hull was wrong in both apps and is fixed with it: the
    exported meshes have their Z negated, so the bow sits at -Z in a mesh while
    `into_model_axes` leaves it at +Z, and markers were drawn on the other end of
    the ship. `hull_impact::into_mesh_space` applies that negation for points and
    directions alike, the egui viewer's `world_to_model` and compass rose read
    the same frame, and an ignored test pins the sign against Iowa's main
    battery mounts.

    Auto-scroll has nothing to scroll: the log is the panel's own list rather
    than a pane that follows playback.
19. ~~**The menu bar and the status bar.**~~ Done 2026-09-28: the strip's trailing
    end carries Check for Updates, About, Create Issue, Discord and Quit, and a
    status strip along the bottom names the jobs the app owns -- a game-data cache
    job, a download plan and an index build -- with their progress. Where the egui
    panel lists every background task in one place, this port reports each where
    the work is: a directory walk and a game-data load on the listing, a replay's
    own read in its tab, a batch render or a bulk contribution in a message it
    rewrites as it goes, and an outcome nobody was watching for through
    `JobReport` (item 32). Nothing runs unreported.
20. ~~**Palette is a third of egui's.**~~ Done 2026-09-28: the three cascading
    modes are there (search a player, my matches in ship, view armor for ship),
    filled from the index and the loaded build's ship catalogue. Player and ship
    modes run a bounded index query after each settled query change; the palette
    discards stale results and returns to the root mode on Escape. Armor ships use
    the same 50-row cap against the loaded catalogue, and Advanced Search opens the
    Search tab directly. A player or ship row leads to the
    search the shared seeds express (`query_bar::seed`, printed through
    `query_text::print_query`, which a test parses back); an armor row opens that
    ship in the Armor Viewer. A mode with nothing in it says so rather than opening
    an empty list.
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

22. ~~**Collab.**~~ Done 2026-09-28. The session
    notifications are done (2026-09-27): `poll` returns
    typed `SessionNotice`s the header says after its draw -- started or connected
    by role, joined, left, timed out, ended, error, rejected, and the host
    opening or closing a replay. Done 2026-09-28: the popover lists what the
    session is on (`shared_windows`), and a debug build offers the localhost link
    beside the web one.

    Done 2026-09-28: this app can be the end a session watches. A playback
    announces itself with its map art (`SessionCommand::ReplayOpened`, then
    `BecomeFrameSource`), broadcasts each frame as it draws it, and withdraws when
    its window closes. The popover carries "Open for everyone" per shared window.
    A frame is a `Vec<DrawCommand>` rather than an image, so each peer draws the
    battle with its own art at its own size, and an egui peer or the web client
    watching this app sees the battle it is playing.

    The other direction is done with it: `watched_playback.rs` is a dock panel
    that registers a sink for one of the session's windows
    (`ViewportSink`'s `frame_tx`), draws the newest frame it has been sent with
    this build's own art or the art the owning end sent, and says where that end
    has reached. It steers nothing, because the clock belongs to the end that owns
    the replay. The popover's Open button opens one per shared window, and
    "Open for everyone" is honoured on the peers as it arrives. Earlier notes here
    called this an architecture this port does not have, on the mistaken belief
    that a frame was an image; it is a `Vec<DrawCommand>`.

    Not ported: the spam-protection notice that drops a peer out of a session
    whose host opens replays faster than five in ten seconds. It guards against a
    host churning through windows; this port opens a viewport per window on the
    reader's own press rather than on the host's, so there is nothing yet for it
    to protect.

    The tactics board sync did not, and is done 2026-09-28. The popover carries
    the Tactics Board button, which opens the board the session is on. A board
    announces the map it is set on with that map's art, so a peer draws it from
    what it was sent rather than needing the build it came from
    (`PreviewRenderer::with_art`). Its capture points travel by id, so a zone one
    reader moves is the same zone on every board, and what is drawn on it goes
    through the same annotation sync the replay viewport uses, keyed by the
    board. A peer that has no board of its own opens one on a board a peer has,
    and says whose it is; closing the window takes it out of the session. One
    board at a time, as the rest of this app's windows are: a reader with their
    own board open keeps it.

    Drawing also works with nobody connected, which it did not: a shape drawn
    outside a session was sent to a session that was not there and never drawn at
    all. `CollabLink` holds what this end has drawn while there is no session to
    hold it, and carries it across a session starting or ending, so the shapes
    stay on the map either way.

    Two limits are the shared drawing layer's rather than either front end's, and
    both apply to the replay viewport as much as to a board. A selection is a set
    of places in a list (`wt_collab_protocol::drawing::Selection`), while the
    session keys shapes by id, so a peer erasing a shape shifts the indices a
    drag or a delete is about, and the check that guards them can only see a list
    that has grown shorter. And `undo_plan` is order-blind, so a shape put back
    lands at the end of the list rather than where it was. Closing either means
    keying a selection by id, in the crate both apps draw through.
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
30. ~~**Annotation keys.**~~ Done 2026-09-28: ctrl and a digit take up each
    tool in the egui board's own order, ctrl+m the measurement, escape puts the tool
    down, delete or backspace erases what is picked out, and `[`/`]` narrow and
    widen the nib between 1 and 8 as the egui board holds it. The chords are
    discoverable now: holding ctrl over a viewport puts up the same cheat sheet the
    egui board does, each tool button says what it is and which chord takes it up,
    and the nib has a stepper beside the inks so the bracket keys have something
    visible to move. Ctrl-click row selection in the listing was already there
    (`handle_leaf_click`). The query bar's pills step too, from 2026-09-28: Left
    from the start of the text takes the pill before the caret and walks back
    along them, Right walks forward and puts the caret back in the text past the
    last one, Shift extends from wherever the run was anchored, and Delete erases
    what is selected. It steps over the shared selection model
    (`query_bar::select`'s `step`, `range` and `selectable_paths`), which is what
    the egui bar walks, and a clicked pill anchors the next step. Every chord in
    the port is still a hand-rolled `on_key_down` match rather than a gpui
    action, so none are rebindable.
31. ~~**Armor viewer** gaps.~~ Done 2026-09-28. The export asks what it should
    contain before it asks where to write: contents, hull, level of detail,
    texture cap, camo set and the size it is expected to weigh, with the choices
    themselves shared (`wows_toolkit_viewmodel::armor::export`) so an export set
    up in either app is what the other offers next time. What a cast shell did is
    read out in words beside the penetration checker -- one block per arc with
    its hits, the plating crossed, the range it was fired from, a stepper to fire
    it from another, and the two isolations -- through a shared reader of the
    simulation (`wows_toolkit_viewmodel::armor::arc`), which is the egui Analysis
    window's Trajectory tab in the panel this port already puts its Ships tab in.
    The camera locks onto the ship's own orbit and looks out from it, with its
    own field of view and the two projections, on the shared camera math
    (`wows_toolkit_viewmodel::armor::camera_perspective`); a drag turns the eye
    on its orbit and the wheel moves between the inner and outer one. The two
    lighting colours have pickers, the ship's own origin has a cross, and the
    impact markers have an opacity. Defaults are written as they are changed
    rather than behind a button, into the same `armor_viewer_defaults` row the
    egui app reads. Still thinner: a cast fires the first comparison ship's
    shell rather than one arc per compared ship, and the Splash tab's readout is
    not built (the boxes themselves are drawn).
32. ~~**Toast mechanics.**~~ Done 2026-09-28: sticky and arm-once landed
    2026-09-27 (`toast::stuck(key, ...)` keeps one message per state,
    `toast::resolved(key, ...)` takes it down), and what stood in for the egui
    app's catch-all task-error funnel is now `JobReport`: whatever a background job
    has to say is kept until a draw can say it, so an index build started from the
    palette reports where the reader is rather than only on the Settings tab, and
    an auto-export that fails keeps a message up rather than logging alone. A
    failure too long for a toast opens the error window instead (item 21).
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
