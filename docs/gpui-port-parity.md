# GPUI port: parity and polish backlog

What the GPUI port does not yet do that the egui app does, audited pane by
pane on 2026-09-23 against `crates/wows-toolkit/src/ui` and
`crates/wows-toolkit/src/armor_viewer`. Each entry names the egui behaviour,
what the port does instead, and what closing it takes.

`[done]` entries were closed in the same pass that produced this list and are
kept so the next audit does not re-report them.

## Shell and visual design

- A hover preview costs ~740 MB of steady-state memory after sweeping a few
  dozen rows: the renderer's whole asset set is loaded from the VFS twice per
  hover and never cached, and one baked track is kept for a return visit.
  Caching `PreviewRenderer` per build and map is the fix. (Latency is no
  longer the reason to do it: measured, the asset load is 50 ms and the build
  load that dominated is now warmed at startup.)
- Toasts: `Root` holds the queue but the window's own view has to draw the
  layer, which `App::render` now does; before that every toast and dialog was
  silent. Wired: the game-data load and its empty-replays warning, an invalid
  game directory, a failed replay parse, the Twitch credential, a copied
  login, path and chat, a saved chat, and the armor export. The rest of the
  egui app's 88 belong to features not ported yet (collab sessions, the
  replay renderer, the game-data cache, constants, the updater).
- [done] The command palette is `gpui_kit::component::command` over
  `palette.rs`, on ctrl+k and ctrl+shift+p. The egui palette's cascading
  sub-modes (search a player, a ship, a ship's armor) are not ported.
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
- `ui::rule_v` now separates the groups in the stats filter bar and the
  replay header; the other toolbars still run their controls together.
- Conditional elements still shift the layout in the settings tab, the
  unpacker's queue bar and the search tab's hint and footer. The armor
  viewer's status strip and the search completions were fixed.
- [done] The Settings tab tints itself when the WoWs directory is invalid.
- [done] Charts draw both sets of gridlines, a legend, and value labels in
  each series' own tone.

## Replay Inspector

- The replay renderer is partly there. "Render Replay" in the row menu opens
  a playback viewport in a dock tab of its own: the battle is walked once in
  the background, and the transport plays, pauses, seeks and runs at 0.5x to
  8x with the game clock beside it.
  - Frames are kept as draw commands and rasterised one at a time through
    `PreviewRenderer`, the path the hover preview uses; the port has no
    painter of its own, so it cannot convert commands to shapes the way the
    egui renderer does. The track is bounded (`TrackSink`'s budget), so a
    long battle is sampled more coarsely rather than costing more memory.
  - Still absent: the tab-header Render button, Render to Video, Render to
    Clipboard, the annotation toolbar, and video export. The panel commands
    (stats, rosters) and position trails are outside `bake_options` and so
    are not drawn.
- [done] "Copy Replay" puts the replay files on the clipboard through
  `arboard`, so they paste into a file manager; "Copy Path" still copies the
  text. A group offers both.
- [done] Open in Game (behind a confirmation) and Show Replay Controls are in
  the row menu. Both are listed rather than swapped on alt, and the controls
  parse moved to `wows_toolkit_viewmodel::controls` so the two apps read the
  same scheme.
- Set as / Add to Session Stats for a single replay.
- [done] Date grouping folds by date rather than by consecutive run, so an
  out-of-order timestamp no longer heads a second group with the same date.
- [done] The debug "Mapped Results" viewer: the battle results with their
  positional arrays resolved to named fields, beside the raw payload.
- The scan reports itself with a spinner, but not with counts or a bar:
  the port scans in one pass where egui runs a staged ingest pipeline
  (scanning, reading, downloading, loading data), which is what its counts
  come from.
- [done] Enter on the highlighted row opens it. The kit tree's own Confirm
  only expands a folder, so the listing catches the key itself.
- The collab session popover and the Tactics Board button.

- [done] Re-opening an open replay did nothing; it now brings that tab
  forward.
- [done] The listing's context menu had one item. It now offers Open, Copy
  path and Show in file explorer, and a group offers Copy N paths.
- [done] Rows carried no striping and the table had no row selection;
  ctrl+click now selects a row and the stripe is under it.
- Double-click adds a tab where egui replaces the focused one; "Open in New
  Tab" is the egui way to get a second tab.
- [done] Ctrl-clicking marks replays, and a marked row reads as selected. A
  right-click on one offers the whole set: copy the files, copy the paths, or
  play each back. Right-clicking a row outside the set acts on that row alone,
  since the reader has moved on from the set.
- Set as / Add to Session Stats is still not offered, for one replay or for a
  set: the port has no session-stats writer.
- The header's Actions menu carries "Hide My Test Ship Stats", and is shown
  only for a test ship, which is the case it means anything in. Open in Game
  and Show Replay Controls are in the listing's row menu instead. The match
  timeline and the other-team perspective are not ported at all.
- [done] Alt turns the damage breakdown's percentages around.
- [done] The Effective Fire Chance block is under the recording player's row:
  the counts, the expected figure beside them, the ships they cover, and a row
  per target ship. The geometry resolution and the analysis call moved to
  `wows_replay_insights::fire_chance::sections`, and the wording to
  `wows_toolkit_viewmodel::fire_chance`, so the two apps share both. The cache
  directory is the one the egui app already writes, so a build resolved by
  either is not re-parsed by the other.
- The block's deeper expanders are not ported: the formula listing, the
  battle/ribbon tallies, and copy-to-clipboard. Their text builders are still
  in the egui crate.
- [done] Chat now copies its whole transcript and saves it to a file.
- [done] A row's words are the preview popup's own caption, which goes up as
  soon as the row is hovered rather than waiting for a map.
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
- Shell trajectory mode is absent.
- Splash/blast mode and the splash-box popover are absent.
- Gap detection, "show hidden plates", the roll slider and the camera-rings
  section are absent.
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
- The row's `hull_all_visible`, `armor_all_visible` and `show_splash_boxes`
  are written back exactly as read: the port has no control for them, and the
  row is shared with the egui app.
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
- [done] A chart can be taken off the tab's filter bar and narrow the session
  itself (division and mode), which is the per-pane override the tab lacked.
  The Overview and Ships panels still follow the bar only.
- [done] Chart statistic names read from the catalogue rather than being
  English literals.
- [done] Filter changes are written back, so both front ends read the same
  values.
- [done] The open charts are kept in a settings row: each one's statistic,
  mode, rolling/combined/value toggles, its own filter override and its ship
  selection. Closing one drops it from the row. The dock's pane geometry is
  still not kept; only which charts exist and how each is set up.
- [done] The filter bar clears the whole session and a ship's own row clears
  that ship, both behind a two-press confirm (ctrl+click skips it).
- Charts cannot be copied as an image.
- [done] Achievements draw their own art from the installed build, ordered by
  how often they were earned, and name themselves on hover.
- [done] The records name the ship that set them.
- [done] The session rating is a banded chip.
- [done] The ships table translates its headings and groups its digits the
  way the reader's language does.
- [done] The chart names itself over the plot. The legend still does not
  toggle series.

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
- The sub-tabs cannot be split or docked.

## Search

- [done] Pills read the query as it is typed, and the caret's field offers
  its values from the index.
- [done] Completions are a dropdown, keyboard-navigable, sized to content.
- [done] Outcome and personal rating carry their colour; damage is grouped.
- [done] The tab opens showing everything and the results follow the query
  as it is typed, after a short pause rather than per keystroke.
- [done] The tab opens on the query it was left with, and saves it back.
  Sort order and operator preferences are still not persisted.
- No structural editing: no selection, grouping, negate, delete, ungroup or
  connector flip, and no right-click menu on a pill.
- [done] Up and Down walk the queries that were run, and walking back out
  restores the text the walk started from. The history is the `history` field
  the egui settings row already declares (and never filled).
- No undo/redo in the bar. The port's bar is a text input rather than the
  egui pill editor, so this is the input's own undo, not an AST stack.
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
- A parse error replaces the results instead of marking the offending span.
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

- The game-data cache section is absent entirely (auto-dump, cache directory,
  disk usage, delete old versions, check for updates, validate, repair).
- Automatic replay data export is not configurable.
- No way to build or rebuild the replay index.
- [done] The language is chosen from a combo.
- [done] The Twitch section has the "Get Token" link, and says what Twitch
  made of the stored credential.
- [done] Zoom is saved.
- [done] The Settings tab flags an invalid WoWs directory on the tab strip.
- [done] The WoWs directory field tints and says so when the path is not an
  install. It is still not locked while a load is running.
- [done] `show_entity_id` has a checkbox. `auto_dump_game_data` does not.
- The collaboration section is absent.
