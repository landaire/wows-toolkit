# GPUI port: parity and polish backlog

What the GPUI port does not yet do that the egui app does, audited pane by
pane on 2026-09-23 against `crates/wows-toolkit/src/ui` and
`crates/wows-toolkit/src/armor_viewer`. Each entry names the egui behaviour,
what the port does instead, and what closing it takes.

That scope is panes. What lives outside it -- the command line, the renderer and
adapter selection, the process mitigations, the updater, the window and menu
layer, the tactics board, and the dump cache that lets an old replay parse at all
-- is in `docs/gpui-port-gaps.md`, audited 2026-09-27.

`[done]` entries were closed in the same pass that produced this list and are
kept so the next audit does not re-report them.

## Shell and visual design

- [done] A hover preview costs ~740 MB of steady-state memory after sweeping
  a few dozen rows: the renderer's whole asset set was loaded from the VFS
  twice per hover and never kept. A renderer is now built once per build and
  map and shared by every preview that draws it, so what a sweep holds is
  bounded by the four most recently drawn maps rather than by how many rows
  were passed over. Changing the WoWs directory drops them, since they carry
  art read out of the install being replaced.
- [done] Toasts: `Root` holds the queue but the window's own view has to draw the
  layer, which `App::render` now does; before that every toast and dialog was
  silent. They are the kit's own `Notification`, not a copy of the egui app's
  bottom-right stack. Wired: the game-data load and its empty-replays warning,
  an invalid game directory, a failed replay parse, the Twitch credential, a
  copied login, path, chat, build link, WG ID, session token, web link, stats
  table, chart image, fire-chance breakdown and timeline, a saved chat, a
  timeline jump, and both export paths (the armor pane's and the viewport's own,
  which only logged). The rest of the egui app's 88 belong to features not
  ported yet (collab sessions, the game-data cache, constants, the updater) or to
  structure the port does not have: its opened_directory/open_no_directory
  notices are about workspaces, and a missing replay disables the Search row's
  open button rather than toasting when it is pressed. A popped-out renderer
  window is rooted in `window_shell::Shell`, which draws the same three layers
  `App::render` does, so a panel's messages are shown in its own window as well
  as in the dock; `toast.rs` refuses (and logs) rather than panicking on a window
  with no `Root` at all.
- [done] The command palette is `gpui_kit::component::command` over
  `palette.rs`, on ctrl+k and ctrl+p, which is what the egui app binds. The
  player and ship modes query the index as the reader types, with the same
  result limit and Escape-to-root behavior as egui. Armor ships filter against
  the loaded build's catalogue with the same 50-row cap. Advanced Search opens
  the Search tab directly. The root actions and eight added actions are
  listed in the gaps file.
- [done] The file viewers and the raw-JSON panels are read-only
  `EditorState`s with a grammar, so they highlight, select and search.
- [done] The settings tab is `component::form`: one labelled section card
  per group, Twitch in a section of its own.

- [done] Every string the port draws was an English literal. The port has its
  own `i18n!` now, a language change sets both its own locale and the shared
  one, and its chrome reads from the same catalog the egui app does. A test
  (`main.rs`'s `translation_keys`) fails on a key the catalog has no entry
  for. What is left untranslated is log text and test fixtures.

- [done] The palette was egui's stock greys rather than Graphite and Bone
  (`ui/theme/palette.rs`): one surface, one border, one text tone. The seven
  tiers and both semantic sets are now in `theme.rs`, mapped onto
  gpui-component's own tokens.
- [done] No row striping anywhere. `ui::stripe` now backs the replay listing,
  the player table, both tracker tables, the search results, the ships table
  and the unpacker listing.
- [done] No tree connector lines. `ui::indent_guides` now draws them in the
  replay listing, the ship tree and the unpacker tree.
- [done] Semantic colours were hardcoded RGB literals that ignored the theme:
  relations, captain-points bands, win/loss and every chat tone now resolve
  through `theme::semantic()`.
- [done] The tab strip carried no glyphs and sat on the page's own fill; it
  now has the egui strip's icons, the surface tier under it and a bright rule
  between it and the page.
- [done] The unpacker listing had no column headers and one glyph for every
  file; it now names Name/Size/Type and tells the four file kinds apart.
- [done] Tree rows draw at the body size and the body's own padding
  (`ui::tree_row`).
- [done] `.opacity(0.x)` stood in for a dim text tier; every text site now
  takes `theme::text_dim()` or the new `theme::text_faint()`.
- [done] The queue and dump popovers, the chart settings and the armor
  legend size to their content between a floor and a cap.
- [done] Inputs in an icon row take the space the icon leaves.
- [done] `ui::rule_v` now separates the groups in the stats filter bar and
  the replay header; the other toolbars still run their controls together. It
  separates them in the tracker's toolbar (the period, what narrows it, and
  the division-mates toggle) and the playback transport (what plays it, where
  in it, and how fast) too. The unpacker and armor toolbars already carried
  theirs.
- [done] Conditional elements still shift the layout in the settings tab,
  the unpacker's queue bar and the search tab's hint and footer. Each of them
  now holds its slot: the WoWs-directory and Twitch lines are always drawn
  and empty when there is nothing to say, the queue bar keeps the heights of
  its progress bar and its written-file line, and the search tab keeps its
  game-mode hint and its count row.
- [done] The Settings tab tints itself when the WoWs directory is invalid.
- [done] Charts draw both sets of gridlines, a legend, and value labels in
  each series' own tone.

## Replay Inspector

- [done] The replay renderer is partly there. "Render Replay" in the row menu opens
  a playback viewport in a dock tab of its own: the battle is walked once in
  the background, and the transport plays, pauses, seeks and runs at the egui
  renderer's own ladder of 1x to 60x, opening at 20x as it does.
  - Frames are kept as draw commands and rasterised one at a time through
    `PreviewRenderer`, the path the hover preview uses; the port has no
    painter of its own, so it cannot convert commands to shapes the way the
    egui renderer does. The track is bounded (`TrackSink`'s budget), so a
    long battle is sampled more coarsely rather than costing more memory.
  - Render to Video is on the transport: the baked track is rasterised
    through the same renderer the viewport draws with and encoded to an MP4,
    so nothing is parsed a second time. The encoder gained `submit_frame` and
    `finish_submitted` for a caller that holds its own frames rather than a
    battle to draw them from; `advance_clock` is unchanged, so the egui
    export still drives it as it did. The transport is refused while an
    export runs, because both want the one renderer, and the clock reads the
    frame count instead.
  - Render to Clipboard is beside it: the same encode to a temporary file,
    handed to the clipboard as a file so it pastes into a chat window or an
    upload dialog. The file is left behind deliberately, since the clipboard
    holds a path.
  - The tab-header Render button is there too, acting on the replay showing
    in the dock, or the one last opened when none is.
  - A gear on the transport carries the display toggles: every command class
    the baked track holds can be switched off, filtered as a frame is
    rasterised rather than at bake time, so a toggle costs one frame instead
    of another walk of the battle. `should_draw_command` moved from
    `wt-collab-egui` to `minimap-renderer`, beside the options and commands it
    reads, so both apps hide the same things.
  - A viewport can be popped out to a window of its own. The same entity is
    re-hosted, so the baked track and the renderer come with it; it leaves the
    dock first, because one entity drawn twice would fight over the one
    renderer.
  - The scrubber addresses frames rather than a fraction. A slider quantises
    to its step, so one built over a range of 1 rounded every position to an
    end and only the ends of a drag did anything; it is rebuilt over the
    track once the bake lands.
  - Jump-to-start, back and forward ten seconds and jump-to-end are on the
    transport, and Space, Up, Down, Left and Right drive them from the
    keyboard as they do in the egui renderer. A skip moves by game time
    rather than by frames, since the sampling interval decides how many
    frames a second is.
  - The clock reads the battle's own time rather than the recording's: a
    replay records from the loading screen, so the raw clock of the first
    frame is already most of a minute in. The seek bar marks where the battle
    began and ended, which is what says which stretch of the bar is the
    battle. `bake_track` carries both clocks out of the walk it already
    makes.
  - The map zooms and pans. The wheel zooms about whatever is under the
    pointer, a drag moves the map under it, a double-click puts the whole map
    back, and a zoom control and Reset sit on the transport where the egui
    renderer keeps them. A drag asks for frames faster than one can be
    rasterised, so a request that arrives while a frame is still being drawn
    is remembered and made once it lands; dropping it left the map showing a
    zoom the reader had already moved past.
  - Previous and next event are on the transport, with Shift+Left and
    Shift+Right beside them, and each jump says what it landed on. Stepping
    back looks half a second behind the clock, as the egui renderer does, so
    landing on an event and pressing back again finds the one before it rather
    than itself. The events come from a second walk of the replay started
    after the bake, so the viewport still opens as soon as the track is ready
    and the controls stay refused until it lands.
    `replay::timeline` moved out of the egui crate to
    `wows_replay_insights::timeline` for this, so both front ends read the same
    events; only the egui list and its filter bar stayed behind. That needed
    `advantage.rs` moved down to `wows-core` first, since `minimap-renderer`
    already depends on `wows-replay-insights` and the timeline reads team
    advantage -- it is game logic over game types, so `wows-core` is where it
    belongs anyway.
  - The event timeline is a popover on the transport: the battle's events,
    filtered by kind and by a search over what each row says, each row clickable
    to seek to it and coloured by whose it was. Copy takes whatever the filter
    is showing. While the second walk runs it says so rather than showing an
    empty list, and it distinguishes "nothing happened" from "nothing matches".
    The filter, the row wording and the friendly/enemy decision are all in
    `wows_replay_insights::timeline`, so the egui list and this one cannot
    drift; only the widgets and the colours differ.
  - Export carries the egui renderer's own settings: prefer-CPU,
    include-pre-battle, and the codec with an Auto that names what it would
    pick. Only codecs this machine can actually encode with are offered, the
    probe runs once for the process, and a codec the GPU cannot encode falls
    back to software rather than failing the export. Leaving the pre-battle
    phase out trims the frames before the battle's own start rather than
    re-baking.
  - A playback viewport bakes every ship's range circles, so Ship Ranges is a
    display toggle rather than another walk of the battle, and the reader can
    change whose ranges they are looking at afterwards. Trails are still left
    out of the bake on purpose: a trail command carries every point so far, so
    one per frame would cost the square of the track's length. They are
    derived instead, by `trails_through` reading the ship positions already in
    the frames behind the one being drawn, which costs one walk of them and is
    what Heat Trail switches on.
  - A pointer position and a map position are different spaces once the map
    is zoomed, and `map_point` now returns map space with `drawn_point` kept
    for the wheel, which anchors on a drawn point. Picking compared the two
    directly before that, so it agreed only at zoom 1 and picked the wrong
    ship, or none, as soon as the reader zoomed in.
  - Right-clicking a ship opens its own menu: its trail, its six ranges, and
    the "only this one" shortcuts for both. Picking reads the frame on screen
    and takes the nearest ship within an icon's width, so it picks what the
    reader can see. The global switches decide whether a layer is drawn at
    all and follow the per-ship choices, so asking for one ship's ranges turns
    the layer on and clearing the last one puts it away again. Show Realtime
    Armor is not on it yet: that opens the armor viewer on a ship, which is a
    tab away rather than a filter.
  - The team-advantage breakdown reads on hover over the strip the score bar
    is drawn in. The rows themselves moved to `wows_core::advantage`
    (`breakdown_rows`), which the egui tooltip now reads too, so the two
    cannot word a factor differently. The port hovers the whole strip rather
    than the label's own rectangle: the label is drawn into the frame by the
    renderer, so there is no element of its own to rest a pointer on.
  - [done] Both of the features that were absent -- Show Realtime Armor and
    the annotation toolbar -- are in. What each needed:
    - **Show Realtime Armor** from the per-ship menu. The egui app opens an
      armor viewer bound to a `RealtimeArmorBridge` that streams shot hits at
      it as playback advances. The port's armor viewer has no shot-hit
      pipeline at all -- no `ResolvedShotHit`, no bridge, no per-ship shot
      timeline -- so this is that pipeline, not a button.
      `extract_timeline_and_shots` already hands back the per-ship timelines,
      and `ShipShotTimeline` now answers the questions a viewport asks of one
      -- the hits taken by a clock, the hits gained since the last one, and
      the health at a clock -- so the data half is shared and ready. What is
      missing is the armor viewer's side. Its bookkeeping is in:
      `armor_viewer::realtime::RealtimeArmorFeed` holds a ship's timeline and
      answers what it had taken by the moment playback is showing, reporting a
      step forward as the hits it gained and a step backwards as "draw the
      hull again", which is the part the egui app keeps tangled inside
      `RealtimeArmorBridge`. Where on the hull a hit goes is settled and
      shared: `wows_replay_insights::hull_impact` undoes the ship's own
      rotation and changes axes, which is the part that must not be
      re-derived -- a sign error puts the hit on the other side of the ship,
      where it still lands on real plating and still reads as a measurement.
      A hit whose victim was not being watched is refused rather than placed
      from a guess, for the same reason. The egui viewer delegates its own
      two rotations to it, so there is one copy of the conventions.
      The rest is in. The walk that reads the battle's events returns the
      per-ship shot timelines from the same pass, so keeping them costs
      nothing. The per-ship menu offers Show Realtime Armor, but only for a
      ship the battle recorded hits on, since where those landed is the whole
      point of the viewer; which ship it was comes off the roster the frame
      already carries, because a ship command says who is in it but not which
      ship it is. Choosing it hands the hits up to the tab the armor viewer
      lives in, since the viewport owns neither. Each hit is drawn as a small
      cross of three bars rather than a sphere: it reads at any angle, it is
      not mistaken for part of the ship, and its size is a setting. Every hit
      is the one colour, because what kind of hit it was is the penetration
      checker's answer and colouring them here would claim something the
      markers do not know. A hit the viewport cannot place is left unmarked.
      An open viewer follows playback, and is disturbed only when the set of
      hits actually changes: showing them again rebuilds the hull's meshes,
      which is far more than a frame of playback is worth, and
      `RealtimeArmorFeed` is what answers whether anything changed. A step
      backwards always counts, because a hull cannot be un-hit one shell at a
      time. Following is a separate event from opening, so playback never
      pulls the reader back to the armor tab. Beside the hull it reads the
      ship's health at that moment and a shot log of what landed, newest
      first and bounded, worded by `hull_impact::log_line` so a log in one
      viewer cannot call a shatter something else than a log in the other.
    - [done] **Team rosters, and reading a consumable by resting on it.** The
      gear carries a Team Rosters switch, as the egui display settings do,
      and the two clear each other because the rosters and the stats panel
      want the same gutters. Turning it on is a wider canvas rather than
      another layer, so the viewport moves itself onto the renderer for that
      layout. `ImageTarget::drawn_regions` records a `DrawnRegion` per
      consumable icon as it draws it, handed back with the frame it belongs
      to, so a hover reads the row and icon off the roster command the frame
      already holds rather than repeating the layout. What it reads is
      `draw_command::consumable_lines`, which the egui tooltip reads too, so
      the two cannot word a charge count differently.
      Still absent is the player-build popover, which needs a build pipeline
      the port does not have; a hover that could only ever say "build data
      not available" would read as broken rather than as parity. The stats
      panel has no switch either: its silhouettes are one of the loads a bake
      skips to stay cheap, so it would draw an empty gutter.
    - **The annotation toolbar.** Peers' cursors and their pings are drawn
      now: `wt-collab-client` already tracked both, so the work was a
      `CollabLink` the viewport holds (inert without a session, and handed on
      when one starts or stops while a viewport is open) and the projection
      from map space back to the element. A middle-click drops a ping. A
      running session drives its own redraw, so a peer's pointer moves and a
      ping finishes its ripple while this app sits still; the egui renderer
      reaches the same place by asking for a repaint while a ping is alive.
      What is still absent is the toolbar itself, which is a drawing editor
      -- tools, selection, transforms -- and its own piece of work. Its first
      piece is in: the maths an annotation needs (how near a point is to each
      of the eight shapes, which way an arrow's head faces, and the
      simplify-then-smooth a freehand stroke is tidied by) is
      `wt_collab_protocol::geometry`, over the wire types rather than egui's,
      and the egui app now reaches its own through it. Neither front end
      carries a second copy of a formula. The drawing is in too:
      `DrawCommand::Annotation` carries one of three shapes (an open run of
      points, a closed one, or a circle), because a rectangle and a triangle
      are both closed runs and an arrow is a run with a filled head, so the
      tools turn into those rather than every target learning each tool.
      `geometry::annotation_commands` does that turning, and the viewport
      puts what the session holds on the frame after the battle's own layers,
      so a drawn line sits over a ship and under the HUD, and zooms, clips
      and exports with everything else. A drawn width is a map length, so it
      thickens with the map as the egui renderer's does, and no display
      option hides an annotation: a reader put it there deliberately. The
      egui converter grew the same arm, so a web client draws them too.
      The tools are in as well. What a tool does with a pointer is
      `wt_collab_protocol::drawing`: a line needs two points far enough apart
      to mean anything, a freehand stroke is tidied before it is kept, an
      arrow drawn with shift held keeps only where it began and where the
      pointer is. A tool in hand takes the drag, so a reader drawing is not
      panning the map under their own line, and the part-drawn shape is put
      on the frame through the same conversion the finished one uses, so what
      they see while dragging is what they get. The toolbar is a popover
      beside the gear, refused without a session because an annotation is
      something everyone in one sees. The icons are the egui toolbar's own
      Phosphor codepoints rather than approximations.
      Selection is in: with no tool, a click picks out the shape under the
      pointer (ctrl-click builds a selection up, a click on open water lets
      go) and dragging one moves it, which the session hears about once, at
      the end, under the id it already knows rather than as a new shape
      beside the old one. `drawing::{move_annotation, rotate_annotation,
      can_rotate, Selection}` are shared, so both front ends move a shape the
      same way, and a selection is trimmed against the list each click
      because another peer can rub something out from under it.
      A shape that faces somewhere carries a rotation handle above it, drawn
      as an element rather than into the frame: it is a control, so it keeps
      its size at every zoom and stays out of an exported video. Dragging it
      turns the shape about the middle of its own bounds.
      `drawing::{annotation_bounds, bearing}` are shared, and the bearing
      keeps the convention a stored rotation already carries (zero up the
      map, growing anticlockwise) rather than a tidier one.
      Undo is on the toolbar as it is on the egui one. `drawing::undo_plan`
      works out what to send by id rather than by position: the list is
      shared, so a peer adding or rubbing something out shifts every index
      after it. The snapshot is restored exactly, which takes off anything
      added since -- a peer's drawing included -- because that is what the
      egui undo does when it sends its whole list back as a full sync. A
      turn is snapshotted as the handle is taken hold of rather than when it
      is let go, since a turn reaches the session as the pointer moves.
      A ship can be placed too, from the same five classes the egui toolbar
      offers, for either side. It draws as `DrawCommand::Ship`, the
      renderer's own ship command, so a placed ship is drawn from the same
      icons as a real one and carries its name above it; it answers to no
      entity and no player, which is what keeps it out of anything keyed by
      either. The viewport picks ships out of the baked frame rather than
      out of what is drawn over it, so a placed one is never mistaken for a
      ship in the battle.
      A placed ship shows its ranges too. The split is that the annotation
      decides which circles the reader asked for and what coefficients their
      build applies, while the ranges themselves are read out of the ship's
      own game data, which only a front end has: so
      `geometry::ship_range_commands` takes ranges already resolved, and the
      viewport resolves them through `Vehicle::resolve_ranges` as the frame
      is drawn. A build's coefficient moves the ring and the figure beside
      it together. `ShipConfigCircleKind::style` now holds the colour,
      opacity and dashing that the renderer had inline at six places, so a
      circle drawn for a ship in the battle and one drawn for a placed ship
      cannot come out different colours.
      The chooser that assigns a ship to a placed one is on the toolbar,
      shown while one is picked out: the same search over the same catalogue
      the armor viewer lists ships from, matched on the folded name each
      entry carries. Choosing one sets the class and the identity and leaves
      the build stock, which is where the egui chooser leaves it. The
      matches are worked out as the box is typed in rather than while the
      toolbar is built, because the toolbar is built over and over and
      cannot reach the catalogue mutably.
    The stats panel and team rosters remain outside `bake_options`.
  - Zoom and pan was the one gap that was not a matter of wiring. The egui
    renderer converts draw commands to shapes itself and applies a
    `MapTransform` as it goes, so map elements zoom while the HUD does not;
    the port rasterises a whole frame through `ImageTarget`, and transforming
    the finished image would have zoomed the score bar, the timer and the
    kill feed with the map. `ImageTarget` now has a map viewport of its own
    (`minimap-renderer` `viewport.rs`), which both apps share: map elements
    draw on a layer the size of the map, so the map's own rectangle clips
    them, and the layer is composited onto the canvas before anything that is
    not a map element, so the HUD still lands on top. Positions, radii, icons,
    bars and the labels beside them grow with the map; strokes and the grid's
    own labels do not, which is what the egui renderer's `scale_distance` and
    `scale_stroke` decide between.
- [done] "Copy Replay" puts the replay files on the clipboard through
  `arboard`, so they paste into a file manager; "Copy Path" still copies the
  text. A group offers both.
- [done] Open in Game (behind a confirmation) and Show Replay Controls are in
  the row menu. Both are listed rather than swapped on alt, and the controls
  parse moved to `wows_toolkit_viewmodel::controls` so the two apps read the
  same scheme.
- [done] Set as / Add to Session Stats for a single replay, and for a
  marked set. The battle is read off the parsed report by
  `wows_toolkit_viewmodel::stats::PerGameStat::from_report` and written
  to the `session_stats` table both apps read, keyed on the battle's own
  time and account so re-adding a replay refreshes its row rather than
  counting the battle twice.
- [done] Date grouping folds by date rather than by consecutive run, so an
  out-of-order timestamp no longer heads a second group with the same date.
- [done] The debug "Mapped Results" viewer: the battle results with their
  positional arrays resolved to named fields, beside the raw payload.
- [done] The scan reports itself with a spinner, but not with counts or a
  bar. The walk that finds the candidate files is separated from the meta
  reads, so the listing says which replay of how many it is reading and draws
  a bar across them. The scan reports itself over a channel rather than being
  polled on a timer, so nothing ticks once it is over. The port still scans in one pass where egui runs a
  staged ingest pipeline (scanning, reading, downloading, loading data), so
  the downloading and data-loading stages have no counts of their own.
- [done] Enter on the highlighted row opens it. The kit tree's own Confirm
  only expands a folder, so the listing catches the key itself.
- [done] The collab session popover and the Tactics Board button. The
  popover is on the Replay Inspector header, where the egui app puts it:
  a display name, hosting, joining by token, the token itself (masked until
  revealed, with copy and a web link), the roster with roles and
  promote-to-co-host, the annotation and settings locks, reset client
  overrides, and leave. It drives `wt-collab-client`, extracted from the egui
  app for this, so a session hosted from either reads the same to a peer; the
  session state carries a `SessionWaker` the front end supplies rather than an
  egui context. The event inbox is drained on every header draw, because it is
  unbounded. The Tactics Board button opens a separate board window with map,
  mode, cap, annotation, ship, range and preset controls (see gap 16). Not
  ported: the shared-windows list and the replay viewports a host opens for
  peers; and the web asset bundle, so a browser joining a session this app hosts
  sees no map art.

- [done] Re-opening an open replay did nothing; it now brings that tab
  forward.
- [done] The listing's context menu had one item. It now offers Open, Copy
  path and Show in file explorer, and a group offers Copy N paths.
- [done] Rows carried no striping and the table had no row selection;
  ctrl+click now selects a row and the stripe is under it.
- [done] Double-click adds a tab where egui replaces the focused one; "Open
  in New Tab" is the egui way to get a second tab. A plain open now takes the
  place of the replay tab on screen and the row menu carries "Open in New
  Tab". Which tab is replaced is read off the dock's own layout (the active
  tab of its group); with more than one group showing a replay, the one this
  view last opened or brought forward wins.
- [done] Ctrl-clicking marks replays, and a marked row reads as selected. A
  right-click on one offers the whole set: copy the files, copy the paths, or
  play each back. Right-clicking a row outside the set acts on that row alone,
  since the reader has moved on from the set.
- [done] Set as / Add to Session Stats is still not offered, for one replay
  or for a set: the port has no session-stats writer. It has one now. The
  egui app rewrites the whole table when it saves, so a battle recorded here
  while it is running is lost on its next save; started afterwards, it reads
  what the port wrote.
- [changed] A player row's actions are on the row rather than in a column. The
  egui table gives them an Actions column of `...` buttons; here the same items
  are on the row's right-click menu, with dots revealed at the end of the
  player's name, between it and the ship, while the pointer is on that row (the
  chat pane reveals its copy button the same way). The table gets that column's
  width back, and the actions stay where the row is. Both ways in are headed by
  the player they belong to, since a right-click menu lands wherever the pointer
  was and the rows it could have come from are one line apart: one line reading
  clan tag, player, class icon, ship, with the clan tag and the player in the
  colours the row gives them. The heading is a disabled item, which the kit skips
  when the arrows walk the menu; pressing Enter while it is the selection (the
  kit selects item 0 on the first Down, disabled or not) closes the menu without
  doing anything, which needs a `PopupMenu::confirm` guard in the kit to fix.
- [done] The header's Actions menu carries "Hide My Test Ship Stats", and is shown
  only for a test ship, which is the case it means anything in. Open in Game
  and Show Replay Controls are in the listing's row menu instead. The match
  timeline and the other-team perspective are not ported at all.
- [done] Alt turns the damage breakdown's percentages around.
- [done] An expanded row's breakdowns are grids: labels in one column and
  figures in another, rather than the padded `label   : value` block the egui
  app draws them as. The lines themselves come from
  `wows_toolkit_viewmodel::breakdown`, which the egui app's tooltips pad back
  into one string, so both read the same lines in the same order. The damage
  events, the ammo-type breakdowns, the potential and hits breakdowns, and the
  per-victim damage lines all read this way.
- [done] The Effective Fire Chance block is under the recording player's row,
  in the Name column after its damage events, which is where the egui block
  sits: the counts, the expected figure beside them, the ships they cover, and
  a row per target ship. The geometry resolution and the analysis call moved to
  `wows_replay_insights::fire_chance::sections`, and the wording to
  `wows_toolkit_viewmodel::fire_chance`, so the two apps share both. The cache
  directory is the one the egui app already writes, so a build resolved by
  either is not re-parsed by the other.
- [done] The block's deeper expanders are not ported: the formula listing,
  the battle/ribbon tallies, and copy-to-clipboard. All three are here, and
  their text builders moved to `wows_toolkit_viewmodel::fire_chance` with the
  tests that cover them, so the two apps read the same rows. They stand open
  rather than behind a second collapse, since the whole block is already
  behind the row's own expansion. The port's formula still names a modifier's
  source and a victim's ship by its raw identifier where the egui block
  resolves both against the build: that lookup needs the metadata provider,
  which the expanded renderer is not handed.
- [done] Chat now copies its whole transcript and saves it to a file.
- [done] A row's words are the preview popup's own caption, which goes up as
  soon as the row is hovered rather than waiting for a map.
- [changed] A preview that has nothing to show yet holds Ocean's water colour at
  the size the map will be, in the listing popup and in the Search tab's strip.
  The egui popup draws the row's map straight away where it can and its own dark
  background where it cannot; here the placeholder means the preview becomes the
  real map in place rather than appearing out of a hole in the panel, and the
  Search strip no longer resizes the results when the first frame lands. The
  space is held from the dwell, not from the bake: a bake draws its first map
  within milliseconds of starting, so the wait a reader sees is the one before
  it.
- [done] The listing collapses and expands from a caret rail beside it, and
  the state is kept in the `listing_collapsed` field the shared replay
  settings row already declares. Its width was already draggable; it is still
  not fitted to the widest row.
- [done] A column toggle made in the header is written back to that row too.
  It used to change only the tab's own copy, so it was lost on restart.
- [done] Tree expansion and selection survive a rebuild.
- [done] A dock holding several replays drew no tab bar, so only the last one
  opened was reachable; every dock is skinned now.
- [done] The incomplete-results warning and the match-context line (who was
  recording, the battle type, the build, the mode, the map, and each side's
  damage) are under the header.
- [done] The Skills cell tells the two captain mistakes apart: a turret for
  every point in tier 1, a warning for nothing above tier 2, with what the
  captain has (Dazzle, IFA) drawn ahead of them as the egui label orders it.
- [done] The Twitch chip, the hidden-profile eye and the disconnect glyph
  are drawn beside a name. The chip's candidates come from the same shared
  observation table the egui app writes.
- [done] A column is resized by dragging the grip on its header's trailing
  edge, and double-clicking that grip puts it back on its content. The egui
  table gives the Name column a wider range than the rest because its columns
  carry explicit ranges; here a column is content-fitted until it is dragged
  and a drag has only a lower bound, so no column needs one.
- [done] A dragged width is written back and read on the next table's first
  frame. Two tables open at once keep their own widths until one is reopened;
  the row is the port's own, since the egui table keeps its widths in egui's
  memory.
- [done] The replays directory is watched; a finished match joins the
  listing and, with the checkbox on, opens.

## Armor Viewer

- [done] The navigation gizmo drew and reacted with no ship loaded.
- [done] A ship's own row now offers "Compare (split view)"; the header
  button still opens a copy of the current pane.
- [done] The pane-sharing options (mirror cameras, sync settings) moved into
  a header menu that is always present instead of a row that appeared with
  the second pane.
- [done] The pane names the ship it is showing.
- [done] The penetration checker is a popover on the pane's own strip, over
  the plate the pointer was last on. The rest of the Analysis window (the
  comparison list, the server-vs-simulation report) is still absent.
- [done] Shell trajectory mode is absent. It is on the pane's toolbar,
  carrying how many shells have been cast, and Ctrl+T toggles it. A click
  casts a shell at the armor: the ray is sent along the camera's bearing,
  flattened and then pitched by the solver's own fall angle, from outside the
  hull so it crosses every plate rather than only those past the point
  clicked. Shift-click adds a shell to those already cast; clicking past the
  ship drops them. The arc, the impact markers coloured by strike angle, the
  path between plates and the detonation burst are one overlay mesh, scaled
  to the camera. The flight solver and the penetration chain are
  `wowsunpack::ballistics`, which the egui viewer casts through too, so both
  draw the same shell; the attacker is the penetration checker's own, so
  choosing a ship there is what the cast fires. The range slider and the
  continue-past-ricochet toggle are in the display popover, and both re-cast
  what is already on screen. A hit on a triangle carrying no thickness is
  dropped rather than guessed at. Not ported: the Trajectory tab's per-arc
  list, its per-arc range sliders and its isolate-plates/isolate-zones
  buttons, and the per-ship arcs a comparison list of more than one draws.
- [done] Splash/blast mode and the splash-box popover are absent. Splash mode
  is on the pane's toolbar, carrying how many zones the last burst reached; a
  click places a burst, sized from the largest high-explosive or
  semi-armour-piercing shell the chosen attacker carries, and the zones it
  reaches are read off the named boxes the hull ships with. The boxes
  themselves have a toolbar toggle of their own, since they are worth seeing
  while placing a burst and after. The boxes come from
  `wowsunpack::models::geometry::parse_splash_file` and their zones from
  GameParams' hit locations, which is what the egui viewer reads, so both
  answer from the same data. A zone GameParams names no hit location for
  reports an unknown thickness rather than a zero one, and a shell with no
  published penetration figure reports no verdict rather than reading as one
  that bounces off everything. Trajectory and splash mode each turn the other
  off, since both want the click. Not ported: the per-box visibility popover
  with its group tri-state checkboxes, the box name labels drawn over the
  hull, and the per-triangle penetration shading inside the burst.
- [done] The camera-rings section is absent. It is in the display popover:
  the orbits the game's camera rides are drawn over the ship, one mode at a
  time, with the field-of-view and height sliders the egui section offers and
  the inner-to-outer zoom path at either field of view. The orbits at both
  extremes of the range are drawn faint behind the selected one. The
  resolution is `wowsunpack`'s own `CameraTrajectory::resolve`, so both apps
  draw the same orbits; a mode name belongs to the ship that named it, so it
  is corrected whenever the loaded hull changes. Resting the pointer on an
  orbit reads it: which mode it belongs to, which of the two orbits it is,
  at which field of view, and the numbers behind it. Measured to the drawn
  curve on screen rather than by raycast, as the egui app measures it, since
  an orbit is drawn a few centimetres wide and no pick would land on one;
  the orbit takes the tooltip from the plate behind it, which still picks
  for a click. Gap detection is on the pane's
  toolbar, carrying its own count: the openings in the armor a shell could
  pass through are marked in red, found by the same two rules the egui app
  uses (an edge belonging to one triangle, no longer than 5 m, with no other
  boundary edge within 12 cm). They are looked for over the triangles
  currently shown, so a plate the reader hid does not read as a hole in the
  ship. "Show Hidden" is on the pane's toolbar too: it swaps the view to the
  plates that are part of the combat model but that the game's own viewer
  never draws, and still obeys a plate the reader has switched off. The roll slider is in the display
  popover: it heels the hull over through the same range the egui slider
  offers, leaving the waterline and the other world-space overlays level.
- [done] The legend reopens from the pane's own strip.
- [done] Camo selection decodes on the UI thread. Picking a scheme now
  decodes and composites it on a background thread and applies the result
  when it lands, dropping it if the selection or the ship moved on
  meanwhile; the hull keeps the camo it has until then. A hull, LOD or
  module reload still re-applies the scheme inline, because that pass is
  re-uploading the hull anyway.
- [done] Display settings and the legend's placement, visibility and
  collapsed state are written back to the `armor_viewer_defaults` row the
  viewer already reads at startup. Export options are still session-only.
- [done] The row's `hull_all_visible`, `armor_all_visible` and
  `show_splash_boxes` are written back exactly as read. The first two are now
  derived from what the pane is showing when the defaults are saved, the way
  the egui app derives them, so hiding everything and opening another ship
  opens it the same way in either app. The derivation reads this port's own
  absence rules, which are the opposite of the egui app's on the hull side.
  `show_splash_boxes` belongs to a mode the port does not draw and is still
  written back as read.
- [done] Part, material and nation names are drawn in the reader's own
  language (`catalog::translate_part`, the same `IDS_<NAME>` lookup the egui
  app's `translate_part` does). The raw key stays the identity, so a
  `PlateKey` is unchanged.
- [done] The gizmo-snap animation does not mirror to the other panes. It
  does: the snap ticker emits `CameraChanged` on every one of its frames,
  so a mirrored pane follows the whole eased move rather than jumping to
  the settled orientation.
- [done] Panes split horizontally only, and the last pane cannot be
  closed. The common-settings menu now carries a "Stack panes" toggle
  that lays the comparison panes out one above the other, and every pane
  carries a close button; closing the only one replaces it with a fresh
  empty pane, as the egui app does when its final dock tab is closed.
  Dragging a pane to re-dock it elsewhere is still egui-only.
- [done] Ctrl+S opens and closes the display-settings popover, which
  anchors to its toolbar button rather than to the pointer. Ctrl+T waits
  on shell trajectory mode, which it toggles and which is not ported.

## Stats

- [done] Charts drew one combined series with no axes, legend or panning.
- [done] Ship colours collided; they are spread now.
- [done] Bar charts no longer pan or zoom, and the ship picker is sorted by
  name.
- [done] Add-chart is a menu of statistics, so a chart opens on the one asked
  for rather than on a default to be changed afterwards.
- [done] Chart tabs identify line and bar plots with their chart glyph and name
  average, combined and rolling plots the way the egui dock titles do.
- [done] A chart can be taken off the tab's filter bar and narrow the session
  itself (division and mode), which is the per-pane override the tab lacked.
  The Overview and Ships panels still follow the bar only.
- [done] Chart statistic names read from the catalogue rather than being
  English literals.
- [done] Overview damage figures use the selected locale's grouping rules, and
  cached ship-table values are rebuilt when the language changes.
- [done] The Overview ship table keeps its columns readable when its dock pane
  is narrow: the header and virtualized rows scroll horizontally together.
- [done] Filter changes are written back, so both front ends read the same
  values.
- [done] The open charts are kept in a settings row: each one's statistic,
  mode, rolling/combined/value toggles, its own filter override and its ship
  selection. Closing one drops it from the row. The dock layout, split sizes,
  active tabs and panel placement are kept with it.
- [done] The filter bar clears the whole session and a ship's own row clears
  that ship, both behind a two-press confirm (ctrl+click skips it).
- [done] Charts cannot be copied as an image. A Copy as Image control on the
  chart's own strip puts it on the clipboard as a picture. It is drawn rather
  than captured: `Window::render_to_image` is public but only the test
  platform implements it, so a real window returns "render_to_image not
  implemented for this platform". The plot is therefore drawn a second time
  through the same `PlotCanvas` the window draws through, so the two agree on
  where every tick, bar and label sits and only the primitives differ --
  `tiny-skia` for the geometry rather than GPUI's quads and paths.
  - Text is the part that cannot be shared, because GPUI will not lend out
    the font it shaped with (`rasterize_glyph` is private and nothing reads a
    face's bytes back). The glyphs come from the system's own faces through
    `fontdb`, chosen per character so the first face that covers it wins.
    Nothing is bundled: a chart in Japanese, Russian or Thai is drawn with
    the faces the desktop already has, where a bundled Latin font would have
    left those labels as blank boxes.
- [done] Copy as Image is hidden when filters leave no chart data or the dock
  pane is too small to draw the plot, avoiding a blank copied image.

## Player Tracker

- [done] Your own account was listed as a tracked player.
- [done] The clan aggregates blocked the players table; they load with the
  Clans tab now.
- [done] The period is a combo and the three sections are tabs.
- [done] Roster figures are grouped.
- [done] The historical table is missing the in-range count and
  last-encountered columns, and the division-mates checkbox does not affect
  it. It now draws Total Encounters, Encounters in Time Range and Last
  Encountered (an age, with the exact local time behind it), every one of
  them sortable and headed in the reader's own language; a player met inside
  the period only as a division mate leaves the table while the toggle is
  off. The counting is
  `wows_toolkit_viewmodel::player_tracker::visible_player_rows` over the
  shared `history` helpers, which the egui tracker was moved onto in the same
  change, so the two cannot disagree. A player the index names but the
  tracker never recorded keeps the index's own count for the period and shows
  a dash where the all-time figure would be.
- [done] The clans table is missing four columns, the member list and the
  clan search. It draws Encounters in Time Range, Sightings (with the
  in-range figure on hover) and Last Encountered, the tag carries the
  severity tint, and every heading reads from the same
  `ui.player_tracker.column.*` keys the egui table uses, with the three new
  sorts in the shared `ClanSortColumn`. Each row's magnifying glass runs the
  clan search in the Search tab, and a row opens on the members met from that
  clan, each naming how many battles they were in and offering to look those
  up.
- [done] No "find matches for this player/clan" action anywhere. Every
  historical row and every clans row carries a magnifying glass, and the
  roster row's menu carries the same item. The query is
  `wows_toolkit_viewmodel::query_bar::seed`'s, printed back through the
  grammar, and the tab raises it for the app to show in the Search tab.
- [done] A roster row's menu links the player to wows-numbers and to
  shipbuilds. The URL builders moved to
  `wows_toolkit_viewmodel::player_tracker`, so both apps link the same way. A
  row the identity scan never named has nothing to link to and shows no menu.
- [done] The menu has no "find matches for this player" item; that needs the
  Search tab to accept a seeded player query.
- [done] Each team is headed by its name, how many players are on it, and its
  average win rate and personal rating in the band's colour. The averaging is
  `wows_toolkit_viewmodel::player_tracker::live`, shared with the egui tab, so
  the heading and the cells under it cannot disagree.
- [done] The roster's Encounters column counts the battles, in the tone that
  number deserves, and follows the division-mates toggle. (This entry was
  already stale when it was written.)
- [done] Historical rows do not expand; notes are a separate bottom panel,
  and aliases and account ids are invisible. A row opens on its account id
  (click to copy), the other names the account has been seen under, the exact
  time it was last met, how many battles are recorded, and the note editor,
  which is no longer a panel of its own. Writing a note opens the row it
  belongs to.
- [done] Encounter counts carry the severity ramp.
- [done] Period, both table sorts and the filter are kept in a settings row
  and read back on the first frame. The egui tracker keeps none of these, so
  the row is the port's own rather than one the two apps share.
- [done] The sub-tabs cannot be split or docked. The three sections are
  dock panels now, so they can be split and dragged the way the egui
  tracker's are, and the dock draws the tab bar over them. Each keeps
  its own scroll, which is what lets two of them be useful at once; the
  tab still owns the period, the filter, the division toggle and the
  data, so the toolbar governs whichever sections are showing. The
  division toggle is no longer hidden on the roster, whose own Seen
  column follows it too.

## Search

- [done] Pills read the query as it is typed, and the caret's field offers
  its values from the index.
- [done] Completions are a dropdown, keyboard-navigable, sized to content.
- [done] Outcome and personal rating carry their colour; damage is grouped.
- [done] The tab opens showing everything and the results follow the query
  as it is typed, after a short pause rather than per keystroke.
- [done] The tab opens on the query it was left with, and saves it back.
  Sort order and last-used operator preferences are read and written in the
  shared settings row as well.
- [done] No structural editing: no selection, grouping, negate, delete,
  ungroup or connector flip, and no right-click menu on a pill. Every pill
  carries a menu offering all six, and a selected pill reads as selected so
  what a group or a delete will act on is visible. A menu opened on an
  unselected pill acts on that pill alone. The edits are
  `wows_toolkit_viewmodel::query_bar::select`'s, already shared with the egui
  bar, so the two reshape a query the same way; the result is printed back
  through the grammar and re-run, which is how every other edit in this bar
  lands.
- [done] Up and Down walk the queries that were run, and walking back out
  restores the text the walk started from. The history is the `history` field
  the egui settings row already declares (and never filled).
- [done] No undo/redo in the bar. Two controls beside the bar, and Ctrl+Z /
  Ctrl+Shift+Z (Ctrl+Y too), step back and forward through structural edits:
  group, ungroup, negate, delete and connector flips, which are the changes
  the pill menu makes to the query tree. Typing is not recorded, because the
  text field has its own undo and pushing every keystroke would bury the
  edits this stack is for. The stack is bounded at 64, and a new edit clears
  what was undone, since redoing past one would restore a query that no
  longer follows from what is in the bar.
- [done] No date picker for timestamp values. A caret on a date field
  opens a calendar under the bar in place of the value list, and the day
  taken replaces the half-typed value. Which fields take a date is
  `wows_toolkit_viewmodel::query_bar::suggest::date_value_at_caret`, read
  off the same grammar the egui bar reads.
- [done] A copied path reports itself.
- [done] A result row plays its replay back, in the viewport the Replay
  Inspector opens.
- [done] Results are fixed-width and unresizable. A header's trailing
  edge drags its column wider or narrower, double-clicking it puts the
  column back on its default, and the widths are kept in a settings row
  of the port's own, since the egui table has none to share.
- [done] A parse error replaces the results instead of marking the
  offending span. The results the last query returned stay on screen,
  and the objection is said in a strip under the bar with the run of the
  query the parser named underlined beneath it.
- [done] The footer reports truncation on the right row; the query fetches
  one past the limit itself, so the extra `+ 1` the port added is gone.

## Resource Unpacker

- [done] The listing has column headers, striped rows, file-kind glyphs, a
  chevron per directory and a folder glyph that follows the open state.
- [done] File rows offer View contents and Extract as JSON.
- [done] The queue is a panel beside the listing carrying every control that
  acts on it (destination, decode-as-JSON, Extract, Cancel, Clear), so the
  tab's chrome is one row rather than three, and a directory is ticked like a
  file rather than pressed with a plus button.
- [done] Starting an extraction no longer empties the queue, so a cancelled
  or failed run does not cost every tick the reader made.
- [done] A content-search hit has its own menu: view the file, put it in the
  extraction queue, or show it in the package tree it came from. The row also
  carries the match's byte offset, so a hit in a large file can be found
  again.
- [done] Revealing a hit selects the directory it is in and clears a filter
  that would hide it, then brings that tree forward. A listing-filter row
  still has no reveal of its own.
- [done] Extraction progress names the file it is writing, under the bar.

## Settings

- [done] The game-data cache section is absent entirely (auto-dump, cache
  directory, disk usage, delete old versions, check for updates, validate,
  repair). It is its own section under the game directory, carrying all of
  them. The maintenance runs `wows_data_mgr::download_repo`, which the egui
  app already ran, so both act on one cache; the measurement and the pruning
  moved to `wows_data_mgr::dump` and the cache path to `wows-toolkit-config`,
  so neither app can drift on where the cache is or what is in it. Pruning
  ordered by directory name in the egui app, which sorts `0.10.0` before
  `0.9.0` and `_100` before `_99`, so Delete Old Versions could delete the
  newest cached build and keep an older one; it now orders by the build
  number the name carries, and leaves alone a directory carrying none. The
  measurement, the pruning and the jobs all run off the UI thread here, where
  the egui tab walks the cache on the UI thread and memoises the result until
  something invalidates it. The repository
  commit is written back only when a run finds nothing to do, so a run that
  found work is re-checked rather than skipped.
- [done] Automatic replay data export is not configurable. The Replay
  section carries the checkbox, the three formats and the directory,
  written to the same settings row the egui tab reads, and a finished
  parse writes itself out under the replay's own name. A directory that
  is not there turns the writing off rather than failing once per
  battle, and is marked in the settings tab.
- [done] No way to build or rebuild the replay index. The mapping is shared:
  `wows_toolkit_viewmodel::index_rows::map_rows` reads the
  `NormalizedBattleReport` both apps build rather than the egui app's own
  presentation rows, so the two index a replay identically and the damage
  fallbacks a row stores are the ones a cell shows. The egui app was moved
  onto it with its 1043 tests unchanged, and `NormalizedPlayer` gained
  `survived`, which `time_lived_secs` could not express (it is absent both
  for a player who lived and for one who never had a ship). What remains is
  the port's own half, which is now there too: the Settings tab carries a
  Build Index control that walks the replay directory newest first, parses
  each file and writes its rows through the shared `upsert_*_with_mode`,
  reporting how many were read and how many could not be, and stopping within
  one replay of being asked to. One replay's failure does not stop the pass:
  a directory of a few hundred reliably holds one the parser cannot read.
  `parse_replay` hands the normalized report back so nothing is walked twice.
  Not ported: the egui app's incremental indexing on load, its repair pass for
  rows an older parse decoded through the wrong constants, and its
  stream-sniper columns.
- [done] The language is chosen from a combo.
- [done] The Twitch section has the "Get Token" link, and says what Twitch
  made of the stored credential.
- [done] Zoom is saved.
- [done] The Settings tab flags an invalid WoWs directory on the tab strip.
- [done] The WoWs directory field tints and says so when the path is not an
  install. It is still not locked while a load is running.
- [done] `show_entity_id` and `auto_dump_game_data` both have a checkbox. The
  auto-dump one governs nothing yet: no dump runs here, so the cache this port
  maintains is one only the egui app fills. See the gaps list.
- [done] The collaboration section is absent. The Settings tab carries it:
  the session display name, the peer-to-peer address warning suppression, and
  the auto-open toggle, written to the same three rows the egui tab reads. The
  name is the one the session popover hosts and joins under, seeded from the
  stored value and written back from either place, so it does not drift
  between the two. The auto-open toggle is stored and shown but acts on
  nothing yet: the windows it governs are the shared replay viewports this
  port does not open.
