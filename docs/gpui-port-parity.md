# GPUI port: parity and polish backlog

What the GPUI port does not yet do that the egui app does, audited pane by
pane on 2026-09-23 against `crates/wows-toolkit/src/ui` and
`crates/wows-toolkit/src/armor_viewer`. Each entry names the egui behaviour,
what the port does instead, and what closing it takes.

`[done]` entries were closed in the same pass that produced this list and are
kept so the next audit does not re-report them.

## Shell and visual design

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
- Rows are laid out at `text_base` (16px) inside `ListItem`'s own `py_1 px_3`
  while the app's body text is 12.5px, so tree rows draw larger and taller
  than everything around them. Needs a `text_size` on the row plus an
  override of the component's padding.
- `.opacity(0.x)` stands in for a dim text tier at ~69 sites; `theme::
  text_dim()` now exists and should replace them.
- Popovers and inline panels are pinned to fixed widths (`QUEUE_POPOVER_WIDTH`,
  the chart settings' 300px, the legend's 140px); most want `max_w` and
  content sizing.
- Inputs inside icon rows use `w_full()`, which overflows the row; they want
  `flex_1().min_w(px(0.))`.
- No separator primitive: 37 flush `border_b_1` calls against egui's 113
  spaced `ui.separator()` calls.
- Conditional elements still shift the layout in the settings tab, the
  unpacker's queue bar and the search tab's hint and footer. The armor
  viewer's status strip and the search completions were fixed.
- The Settings tab does not flag that it needs attention (invalid WoWs
  directory, rejected Twitch token); egui tints the tab itself.
- Charts have no vertical gridlines, no legend for bars, and value labels in
  one tone rather than per series.

## Replay Inspector

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
- Alt-held inverse percentages in the damage breakdown are missing.
- The Effective Fire Chance section of an expanded row is missing.
- Chat has no Copy All and no Save To File.
- A row's hover text is only reachable through the preview popup; with no
  build loaded there is no hover at all.
- The listing panel cannot be collapsed, and its width is fixed rather than
  fitted to the widest row.
- Tree expansion and selection are wiped on every rebuild.
- The incomplete-results warning, the match-context line, the Twitch chip,
  the hidden-profile and disconnect glyphs, and the Skills tier-marker split
  are all missing.
- Columns cannot be resized; the Name column has no wider range of its own.
- "Autoload Latest Replay" is a dead control: no replays-directory watcher.

## Armor Viewer

- [done] The navigation gizmo drew and reacted with no ship loaded.
- [done] A ship's own row now offers "Compare (split view)"; the header
  button still opens a copy of the current pane.
- [done] The pane-sharing options (mirror cameras, sync settings) moved into
  a header menu that is always present instead of a row that appeared with
  the second pane.
- [done] The pane names the ship it is showing.
- Penetration checker and the whole Analysis window are absent.
- Shell trajectory mode is absent.
- Splash/blast mode and the splash-box popover are absent.
- Gap detection, "show hidden plates", the roll slider and the camera-rings
  section are absent.
- The legend cannot be reopened once closed.
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
- Filter changes are read at startup but never written back, so every filter
  resets on restart and the egui app keeps showing its own older values.
- Chart panes and the dock layout are not persisted.
- No way to clear session stats, globally or per ship.
- Charts cannot be copied as an image.
- Achievements draw a generic star, unsorted, with no description hover.
- Best-frags and max-damage lines drop the ship that set them.
- Session PR is plain text rather than a banded chip.
- The ships table ignores the locale for numbers and column headings.
- The legend does not toggle series, and the chart has no title over the plot.

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
- The encounter-severity colour ramp is absent.
- Period, sort and filter are not persisted.
- The sub-tabs cannot be split or docked.

## Search

- [done] Pills read the query as it is typed, and the caret's field offers
  its values from the index.
- [done] Completions are a dropdown, keyboard-navigable, sized to content.
- [done] Outcome and personal rating carry their colour; damage is grouped.
- The tab never runs a query until Enter; egui opens showing everything and
  re-queries live.
- Query text, sort order and operator preferences are not persisted.
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
- File rows have almost no context menu: no "View contents" for viewable
  types, no "Extract as JSON".
- Content-search hits cannot be queued, revealed or inspected by offset.
- Filter results do not reveal the file in the tree.
- Extraction progress does not name the file being written.

## Settings

- The game-data cache section is absent entirely (auto-dump, cache directory,
  disk usage, delete old versions, check for updates, validate, repair).
- Automatic replay data export is not configurable.
- No way to build or rebuild the replay index.
- No language selector, although the locale is read and used.
- No Twitch "Get Token" link, and a stored credential is invisible.
- Zoom is not saved.
- The WoWs directory field has no validation feedback and is not locked
  during a load.
- `show_entity_id` and `auto_dump_game_data` have no controls.
- The collaboration section is absent.
