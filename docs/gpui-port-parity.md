# GPUI port: parity and polish backlog

What the GPUI port does not yet do that the egui app does, audited pane by
pane on 2026-09-23 against `crates/wows-toolkit/src/ui` and
`crates/wows-toolkit/src/armor_viewer`. Each entry names the egui behaviour,
what the port does instead, and what closing it takes.

`[done]` entries were closed in the same pass that produced this list and are
kept so the next audit does not re-report them.

## Shell and visual design

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

- The replay renderer is absent wholesale: the tab-header Render button, the
  row menu's Render Replay / Render to Video / Render to Clipboard, and the
  playback viewport itself (transport, seek, speed, annotation toolbar, video
  export). Six entry points in the egui app, none in the port.
- "Copy Replay" puts the replay *files* on the clipboard in egui
  (`arboard set().file_list`), so they paste into Explorer; the port copies
  path text only.
- Open in Game / Show Replay Controls (egui swaps the label on alt) are
  missing from the row menu, and there is no confirmation-action pipeline.
- Set as / Add to Session Stats for a single replay.
- Date grouping buckets consecutive runs rather than folding by name, so an
  out-of-order timestamp gives the same date twice; egui folds through a map.
- The debug "Results (Mapped JSON)" viewer.
- Ingest progress has no counts and no bar.
- The listing has no keyboard: egui activates a row on Enter.
- The collab session popover and the Tactics Board button.

- [done] Re-opening an open replay did nothing; it now brings that tab
  forward.
- [done] The listing's context menu had one item. It now offers Open, Copy
  path and Show in file explorer, and a group offers Copy N paths.
- [done] Rows carried no striping and the table had no row selection;
  ctrl+click now selects a row and the stripe is under it.
- Double-click adds a tab where egui replaces the focused one; "Open in New
  Tab" is the egui way to get a second tab.
- No multi-selection, so no batch actions (render N, copy N, set as session
  stats).
- The Actions menu is missing entirely: match timeline, open in game, replay
  controls, load other team perspective, hide my test-ship stats.
- [done] Alt turns the damage breakdown's percentages around.
- The Effective Fire Chance section of an expanded row is missing.
- [done] Chat now copies its whole transcript and saves it to a file.
- [done] A row's words are the preview popup's own caption, which goes up as
  soon as the row is hovered rather than waiting for a map.
- The listing panel cannot be collapsed, and its width is fixed rather than
  fitted to the widest row.
- [done] Tree expansion and selection survive a rebuild.
- [done] A dock holding several replays drew no tab bar, so only the last one
  opened was reachable; every dock is skinned now.
- The incomplete-results warning, the match-context line, the Twitch chip,
  the hidden-profile and disconnect glyphs, and the Skills tier-marker split
  are all missing.
- Columns cannot be resized; the Name column has no wider range of its own.
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
- Camo selection decodes on the UI thread.
- Display settings, legend placement and export options are never persisted.
- Part, material and nation names are shown untranslated.
- The gizmo-snap animation does not mirror to the other panes.
- Panes split horizontally only, and the last pane cannot be closed.
- Ctrl+S and Ctrl+T accelerators are missing.

## Stats

- [done] Charts drew one combined series with no axes, legend or panning.
- [done] Ship colours collided; they are spread now.
- [done] Bar charts no longer pan or zoom, and the ship picker is sorted by
  name.
- [done] Filter changes are written back, so both front ends read the same
  values.
- Chart panes and the dock layout are not persisted.
- [done] The filter bar clears the whole session and a ship's own row clears
  that ship, both behind a two-press confirm (ctrl+click skips it).
- Charts cannot be copied as an image.
- Achievements draw a generic star, unsorted, with no description hover.
- [done] The records name the ship that set them.
- [done] The session rating is a banded chip.
- The ships table ignores the locale for numbers and column headings.
- [done] The chart names itself over the plot. The legend still does not
  toggle series.

## Player Tracker

- [done] Your own account was listed as a tracked player.
- [done] The clan aggregates blocked the players table; they load with the
  Clans tab now.
- [done] The period is a combo and the three sections are tabs.
- [done] Roster figures are grouped.
- The historical table is missing the in-range count and last-encountered
  columns, and the division-mates checkbox does not affect it.
- The clans table is missing four columns, the member list and the clan
  search.
- No "find matches for this player/clan" action anywhere.
- Roster rows have no action menu (wows-numbers, shipbuilds).
- Team headings drop the player count, both team averages and the win/loss
  colour.
- The roster's Encounters column is a static "met before" string.
- Historical rows do not expand; notes are a separate bottom panel, and
  aliases and account ids are invisible.
- [done] Encounter counts carry the severity ramp.
- Period, sort and filter are not persisted.
- The sub-tabs cannot be split or docked.

## Search

- [done] Pills read the query as it is typed, and the caret's field offers
  its values from the index.
- [done] Completions are a dropdown, keyboard-navigable, sized to content.
- [done] Outcome and personal rating carry their colour; damage is grouped.
- The tab never runs a query until Enter; egui opens showing everything and
  re-queries live.
- [done] The tab opens on the query it was left with, and saves it back.
  Sort order and operator preferences are still not persisted.
- No structural editing: no selection, grouping, negate, delete, ungroup or
  connector flip, and no right-click menu on a pill.
- No undo/redo in the bar, and no history recall on Up.
- No date picker for timestamp values.
- Rows cannot open the replay renderer, and a copied path reports nothing.
- Results are fixed-width and unresizable.
- A parse error replaces the results instead of marking the offending span.
- The footer reports truncation one row too eagerly.

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
- Content-search hits cannot be queued, revealed or inspected by offset.
- Filter results do not reveal the file in the tree.
- Extraction progress does not name the file being written.

## Settings

- The game-data cache section is absent entirely (auto-dump, cache directory,
  disk usage, delete old versions, check for updates, validate, repair).
- Automatic replay data export is not configurable.
- No way to build or rebuild the replay index.
- [done] The language is chosen from a combo.
- No Twitch "Get Token" link, and a stored credential is invisible.
- [done] Zoom is saved.
- [done] The Settings tab flags an invalid WoWs directory on the tab strip.
- The WoWs directory field has no validation feedback and is not locked
  during a load.
- [done] `show_entity_id` has a checkbox. `auto_dump_game_data` does not.
- The collaboration section is absent.
