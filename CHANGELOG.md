# Changelog

All notable changes to this project will be documented in this file.

## [0.1.64] - 2026-03-20

### 🚀 Features

- *(minimap-renderer)* Added option to dump all frames
- Allow test ships to be seen in armor viewer

### 🐛 Bug Fixes

- App defaults for updates/log files were incorrect
- I think I FINALLY fixed settings not saving for some people
- File > Check for Updates now does not respect the main app setting for checking for updates on startup

## [0.1.63] - 2026-03-17

### 🐛 Bug Fixes

- Some settings weren't being properly persisted (#30)

### ⚙️ Miscellaneous Tasks

- Get wt-translations ready for publish
- Add 15.2 to `game_versions.toml`

## [0.1.62] - 2026-03-15

### 🐛 Bug Fixes

- UI scaling was not being persisted

### Minimap-renderer-cli

- Use strict version matching and warn on mismatch for extracted data

### Replays

- Filter out removed captain skills

### Replayshark

- Make extracted dir behavior match minimap-renderer-cli

## [0.1.61] - 2026-03-14

### 🐛 Bug Fixes

- Constants fallback sometimes was unreliable, which might break constants data updates
- Main app window position was not being restored on startup
- Completely remove logic involving bundled constants data, except for using it as a fallback

## [0.1.60] - 2026-03-13

### 🐛 Bug Fixes

- Settings were not properly persisted on change

### App

- Update to main egui branch to fix crash plaguing people

### Replays

- Fix incorrect consumable packet decoding, showing wrong consumable icon

## [0.1.59] - 2026-03-12

### ⚙️ Miscellaneous Tasks

- Distribute flatpak instead of appimage

### App

- Refactor to use SQLite database for app storage

### Renderer

- Add player/ship name
- Add support for setting ship annotations as a particular ship
- Fix duplicate players + players being added to wrong team in games with ships that spawn in/respawn (like operations)

### Replays

- Add spotting/potential damage breakdowns in inspector/renderer
- Fix broken consumable packets

### Web

- Fix annotations not working and default name

## [0.1.58] - 2026-03-08

### 🚀 Features

- Multi-language support

### App

- Possibly fix background thread crashing, failing to ipck up new replays
- Reduce some inefficient checks in the hot path

### Renderer

- Fix operation building icons + improper ship icon states + crash for unsupported plane icons
- Add stat panel and translate common messages
- Fix zoom clipping canvas under stats panel
- Fix some lingering issues with the stat panel in the renderer

### Replay_inspector

- Fix players missing from received damage (#27)

### Replays

- Fix self observed damage stats

### Translations

- Fix slightly improper Polish translation for spotting damage

## [0.1.57] - 2026-03-05

### 🚀 Features

- Add replay sessions for group replay reviews
- Add tactics board
- Add tactics web page

### 🐛 Bug Fixes

- Ensure native windows do not depend on each other for repainting

### Armor_viewer

- Fix armor checkboxes not working when in a partial state + fix splash zone labels

### Collab

- Change token format
- Fix annotation syncing
- Add cursor click effect + refactor channels
- Ensure that annotation movements, clearing, etc. are properly synchronized
- Improvements to session popover/joining
- Add unit tests
- Show IP address warning when a user starts a session
- Add heartbeats to the protocol
- Use name-derived color instead of fixed color palette
- Ensure that pre-session annotations are synced properly
- Improvements to freehand line smoothing / arrow heads
- Fix pre-existing cap points not syncing
- Rework forced window sharing/multiple tactics boards
- Fix state synchronization issues
- Refactor into separate crate

### Minimap

- Add arrow + measurement tools, add keyboard shortcuts and multiselect
- Ensure arrow head scales with zoom

### Networking

- Fix rustls conflict

### Player_tracker

- Fix twitch timestamps showing 0 minutes

### Renderer

- Show centerpoint of shapes when moving them

### Replay_inspector

- Fix file listing not being resizable to smaller than default
- Add context menu action to add replay to session stats
- Fix depth charge damage not properly surfacing

### Site

- Add networking info
- Update color palette

### Stats

- Fix dialog showing up on replay inspector tab

### Web

- De-dupe names

### Wows_minimap_renderer

- Add option to use extracted game assets

## [0.1.56] - 2026-03-02

### Stats

- Fix issue with popover immediately closing when trying to change chart stat

## [0.1.55] - 2026-03-02

### 🚀 Features

- Add utility function for parsing game version from its data + update tests

### ⚙️ Miscellaneous Tasks

- Use master instead of nightly

### Armor_viewer

- Add option to simulate past ricochets

### Live_armor_viewer

- Fix shell origin / ship rotation

### Nix

- Fix building on darwin?
- Update targets

### Player_tracker

- Don't limit tracking to only ranked/randoms

### Replays

- File listing should auto-size + move game chat to separate window
- Allow multiple replays to be loaded in different tabs
- Fix issue when parsing some operation replays

### Repo

- Add a bunch of tests

### Resource_unpacker

- Massively improve UX/UI
- Support viewing binary files from assets.bin as JSON
- Perf improvements

### Site

- Change UI a bit
- Include asterisk for linux/macos
- Add session stats

### Stats

- Stats are no longer in a floating window and are instead a new tab
- Allow multiple charts tabs
- Add support for combined stat charts

### Toolkit

- Fix crash when no replays directory exists in game dir

### Wows_replays

- Fix packet 0x26 parsing

## [0.1.54] - 2026-02-26

### 🐛 Bug Fixes

- Refactoring from wows-replays
- Ribbon icons were not properly loading

### Models

- Fix hover target when ship has roll applied

### Updater

- Ensure PDB is updated with app updates

## [0.1.53] - 2026-02-25

### 🚀 Features

- Rudimentary realtime armor viewer
- Add hull upgrades to armor viewer

### 🐛 Bug Fixes

- Refactor child window repainting for perf / accuracy
- Come very close to real turret positions -- TODO rework

### App

- Make sure errors about replays failing to parse and invalid WoWs dirs are more visible

### Armor_viewer

- Implement shader, MSAA, and double-sided panel edge for better quality
- Basic implementation of penetration checks
- Begin to implement ballistics checking/arcs/etc
- Show simulated shells and where they explode
- Ensure trajectory viewer uses correct camera path when clicking model
- QoL updates to trajectory UI
- Improve trajectory analysis for multiple simultaneous ships
- Fix label for where shell exploded; ensure that we use multiple arcs for different ships
- Implement splash box mechanics and refactor armor analysis window
- Make checkboxes highlight bits of ship model they belong to + fix lighting
- Refactor lighting / hull viewing and add intersection lines to all plates
- Improvements to hull/splash box rendering + show salvo timeline in live renderer
- Support extracting/viewing with turret/hull upgrades
- Increase brightness of mesh
- Lighting improvements + fix perf when trajectories are enabled
- Splash boxes will carry override to new windows

### Data_export

- Exported data now has damage relationships, player relationships, ribbons, etc.

### Live_armor_viewer

- Add replay server ballistics comparison + fix waterline
- Refactor to pre-populate salvos and show a timeline

### Renderer

- Use strongly typed clock types + parse ahead to collect salvos
- Add option to prefer CPU decoder
- Fix capture point icons for standard battle

### Session_stats

- Add division filters, game count limit appy per-ship (instead of globally), and persist sessions
- Revamp per-ship display and add options to copy/delete ship info

## [0.1.52] - 2026-02-19

### Armor_viewer

- Fix hidden panel display state

## [0.1.51] - 2026-02-19

### Armor_renderer

- Show proper plate thickness according to the game + better describe multiple plates
- Big improvements to armor viewer UX / accuracy
- Turret rotation + opacity slider + reworked toggles
- Classify hidden plates

### Armor_viewer

- Add 3d model export

## [0.1.50] - 2026-02-18

### 🚀 Features

- Sign the toolkit binary

### 🐛 Bug Fixes

- Make it so searching doesn't make all nodes expanded when not searching

### Armor_renderer

- Fix right-clicking to disable a zone/part name
- Add waterline
- Slow camera movement down

### Armor_viewer

- Fix searching for ship names with special characters

### Replays

- Adjust score timer position in live renderer

## [0.1.49] - 2026-02-18

### 🐛 Bug Fixes

- Armor viewer nation list was not scrollable + broke armor viewer

## [0.1.48] - 2026-02-18

### 🚀 Features

- Armor viewer
- *(armor)* Fix turret transform + sync options across panes
- *(armor)* Allow clicking to select armor regions, right-click to disable
- *(armor)* Show stacked plates

### 🐛 Bug Fixes

- Refactor to use Vfs
- Operations now load
- Game params are not reloaded when loading ShipAssets

### Armor

- Remove test / NDA ships
- Load nation flags
- Use egui_ltreeview
- Refactor to use egui_dock

## [0.1.47] - 2026-02-16

### 🚀 Features

- Add buttons for opening replays in game
- Holding ALT on with expanded replay details will show inverse damage dealt details (fixes #24)
- Show confirmation dialogs for destructive or annoying actions
- *(renderer)* Use fonts from game and fix scaling + show torpedo ranegs

### 🐛 Bug Fixes

- Improve error message visibility in UI
- Refactor updating WoWs game dir to ensure state is properly cleaned up
- Ensure that unsupported game versions surface errors
- *(renderer)* Make overlapping ship config labels rotate around their circle to avoid collisions

### Resource_unpacker

- Ensure builds are listed consistently and can load on-demand

## [0.1.47-beta2] - 2026-02-16

### 🚀 Features

- Add progress callback for video export + ensure we can set prefer_cpu
- *(renderer)* Refactor how ship range filters applied

### 🐛 Bug Fixes

- Refactor networking logic to occur in background thread and re-enable logging

## [0.1.47-beta1] - 2026-02-14

### 🚀 Features

- *(renderer)* Add disable ship ranges button
- *(renderer)* Add chat overlay + enhance window title
- Show warning when GPU renderer cannot be used and fallback to CPU renderer

### 🐛 Bug Fixes

- *(renderer)* Fix smooth scrolling
- *(renderer)* Fix ghost trails
- Attempt to handle constants data updates better + surface errors better
- Update bundled constants data
- Mitigate against GPU mem leak when app is minimized

### Renderer

- Add advantage text, score timer, and improve event timeline

## [0.1.47-alpha6] - 2026-02-14

### 🚀 Features

- *(renderer)* Add speed trails, improved score bar, and kill feed
- *(renderer)* Add keyboard shortcuts and make the UI a bit prettier
- *(renderer)* Add options for disabling end-of-battle text + buff counters

## [0.1.47-alpha5] - 2026-02-13

### 🐛 Bug Fixes

- Performance improvements + hopefully fix deadlock causing app to crash

## [0.1.47-alpha4] - 2026-02-13

### 🚀 Features

- Check for constants file updates when loading replays
- *(renderer)* Add event timeline
- Add cap capture events
- Support multiple versions of the game, so long as they are installed

### 🐛 Bug Fixes

- Players that were never spotted now show on the replay results

### 🚜 Refactor

- Use egui_notify instead of my own timed message system
- Clean up UiReport mapping code

## [0.1.47-alpha2] - 2026-02-11

### 🚀 Features

- *(renderer)* Add right-click option + options for showing/hiding dead ship info

### 🐛 Bug Fixes

- If the app state cannot be deserialized, reset it

## [0.1.47-alpha1] - 2026-02-11

### 🚀 Features

- Add achievement icons
- Add option to limit session stats

### Renderer

- Support zooming/panning
- Support overlay controls
- Add basic annotation support
- Show ship aim direction

## [0.1.46] - 2026-02-06

### 🚀 Features

- Show player ribbons when row is expanded

### 🐛 Bug Fixes

- Cumulative -> average
- Maybe fix icon not working in CI builds?

## [0.1.45] - 2026-02-05

### 🚀 Features

- Add charts to session stats
- Allow taking screenshots of charts

## [0.1.44] - 2026-01-30

### 🚀 Features

- Switch graphics renderer from glow to wgpu
- Add manual secondaries

### 🐛 Bug Fixes

- Restore the stream sniper tech
- Remove plain bomb from damage to prevent double-counting
- Refactoring to fix clippy lints
- Auto-updater should write the new exe in same directory as old exe
- Incomplete match results warning was broken
- Some session stats weren't been shown. hopefully finally fixed?

## [0.1.43] - 2026-01-28

### 🚀 Features

- Add ability to update session stats from multiple selected items in replay list
- Show players who disconnect early from battle
- Show warning when replay has incomplete results
- Add error text when the toolkit fails to check for updates

### 🐛 Bug Fixes

- Tomato.gg no longer supports WoWs
- Newly parsed replays did not correctly sort by PR
- Bump wows_replays version to hopefully fix never-spotted players not being listed in score
- Re-parse replay on modification
- PR colors match WoWs-Numbers (thanks janatan)

### 📚 Documentation

- Update screenshot of the replay tab

## [0.1.42] - 2026-01-21

### 🚀 Features

- Add ship name to interaction player name hover text
- Add PR calculation (thanks WoWs Numbers)
- Reverse the damage interaction details to show ship name normally and player name on hover

### 🐛 Bug Fixes

- Auto-updater had a bug in how it renamed files
- Replay details no longer cause crash in matches with bots
- Only show achievements header when player has achievements

## [0.1.41] - 2026-01-21

### 🚀 Features

- Rudimentary session stats

### 🐛 Bug Fixes

- Restore replay view's context menus for grouped items
- Remove air support bomb to prevent damage double-counting

## [0.1.40] - 2026-01-20

### 🚀 Features

- Add replay grouping by ship/date + show win/loss

### 🐛 Bug Fixes

- Rework error propagation for better error info and app resiliance during updates

## [0.1.39] - 2026-01-20

### 🚀 Features

- Add damage breakdowns by player
- Add damage dealt/received breakdowns to tooltip and expanded info
- Add damage interactions to exported data

### 🐛 Bug Fixes

- Update embedded contants file

### ⚙️ Miscellaneous Tasks

- Update egui to v0.33.3

## [0.1.38] - 2025-10-05

### Core

- Fix GameParams unpacking/conversion to JSON

## [0.1.37] - 2025-10-02

### ⚙️ Miscellaneous Tasks

- Update changelog

### Core

- Bump wowsunpack + wowsreplay versions to fix replays in 14.9.0

## [0.1.36] - 2025-10-02

### Resource_unpacker

- Fix slow conversion of GameParams
- Fix base params unpacking

## [0.1.35] - 2025-10-01

### Resource_unpacker

- Add button for dumping base GameParams

## [0.1.34] - 2025-04-03

### Play_tracker

- Fix crash when using filter larger than hours

### Replays

- Fix regression with detecting replays stored in versioned folders

## [0.1.33] - 2025-04-03

### App

- Increase zoom factor to 1.1 by default and fix UI issues when changing zoom factor
- Only set zoom factor if no settings have been saved
- Add panic handler and detect crashes
- Ensure that the new update filename matches the old one

### Replays

- Format replay file listing with newlines for easier reading
- Expose hit information (CV data is not currently exported)

## [0.1.32] - 2025-03-27

### 🐛 Bug Fixes

- Application icon is now embedded in binary/shows when pinned to taskbar

### App

- Add Discord server link

### Replays

- Update builtin constants.json file for latest game version

## [0.1.31] - 2025-03-18

### 🚀 Features

- *(replays)* Allow exporting as CSV

## [0.1.30] - 2025-03-17

### 🚀 Features

- *(replays)* Test ship players can see their own stats

### 🐛 Bug Fixes

- *(replays)* Replay export filename replaces all characters which may bug filename

## [0.1.30-alpha2] - 2025-03-17

### 🚀 Features

- *(replays)* Add data export
- *(replays)* Add data auto-export in settings tab
- *(replays)* Data export provides module and skill names
- *(replays)* Show build info when player details are expanded

### 🐛 Bug Fixes

- *(replays)* Only make one attempt to parse historical replays
- *(replays)* Fix inconsistencies between auto data export and manual export

### Replays

- Remove dead code
- Only write results if server-provided data is available

## [0.1.29] - 2025-03-05

### 🐛 Bug Fixes

- *(replays)* Constants data was not being loaded from disk

## [0.1.28] - 2025-03-05

### 🚀 Features

- *(replays)* Add fires/floods/cits/crits
- *(replays)* Add icons for IFA/Dazzle builds
- *(replays)* Add damage received and distance traveled
- *(replays)* Move column filters to replay tab
- *(replays)* Support file drag and drop
- Refactor tables
- *(replays)* Allow double clicking a table row to expand it
- *(replays)* Improvements to the player listing table
- *(replays)* Add skill info hover text to expanded row
- Show data collection consent window

### 🐛 Bug Fixes

- *(replays)* Decode HTML entities in chat messages
- *(replays)* Fix broken potential damage breakdown
- *(replays)* Refactor background replay parsing logic to prevent possible panics
- *(replays)* Fix hover labels for received damage
- *(replays)* Fixed long damage hover text
- *(replays)* Get rid of hardcoded stats indices

### ⚙️ Miscellaneous Tasks

- Update dependencies

### Ui

- Refactor UI code into its own module

### Wip

- *(replays)* Download constants file on app launch
- Mod manager

## [0.1.27] - 2024-11-24

### 🚀 Features

- Update prompt window renders markdown
- *(replays)* Implement sortable columns in replay viewer
- Expose player on GameMessage
- *(replays)* Player clan is now shown with chat message

### 🐛 Bug Fixes

- Map.bin was being written to disk by the replay parser lib by accident
- Adjustments to stream sniper detection
- *(player_tracker)* Fix filtering by player name

### 🚜 Refactor

- *(replays)* Large refactoring of the replay viewer to clean up code + make future features easier to implement

## [0.1.26] - 2024-11-20

### 🚀 Features

- Expose clan color and make your own div gold

### 🐛 Bug Fixes

- *(replays)* Fix stream sniper detection in replay parser
- Default settings were not properly applied

### Internal

- Use release tagged as latest for updates

## [0.1.25] - 2024-11-17

### 🚀 Features

- *(player_tracker)* Only consider ranked / random battles
- Add twitch integration to detect stream snipers
- *(player_tracker)* Ignore players in division
- *(player_tracker)* Add more time ranges for time filter
- *(player_tracker)* Add players from current match with stream sniper detection
- *(settings)* Allow customizing which twitch channel to watch for player tracker

### 🐛 Bug Fixes

- Bug with loading game data when no locale is set

## [0.1.24] - 2024-11-15

### 🚀 Features

- *(player_tracker)* Add editable player notes

### 🐛 Bug Fixes

- *(player_tracker)* Fix bug with sorting encounters in time range
- *(player_tracker)* Colors stopped for high numbers
- Dark mode did not work for system-wide light mode users

### Player_tracker

- Change default sort to be times encountered within the tim range

## [0.1.23] - 2024-11-15

### 🚀 Features

- *(replays)* Add checkbox to auto-load most recent replay
- *(replays)* Colorize base XP and damage
- Add new player tracker tab
- *(replays)* Add hover text to break down damage by damage type

### 🐛 Bug Fixes

- *(replays)* Fix operation replays failing to load

### ⚙️ Miscellaneous Tasks

- Update gui

### Replays

- Adjust some table column sizes
- Enable auto loading of latest replay by default

## [0.1.22] - 2024-11-13

### 🚀 Features

- *(replays)* Add base xp

### 🐛 Bug Fixes

- *(replays)* Fixed total damage numbers reflecting incorrect teams

### ⚙️ Miscellaneous Tasks

- Update changelog

## [0.1.21] - 2024-11-12

### 🚀 Features

- *(replays)* Show which division a player was in (div letters probably don't match in-game)
- Default wows dir was previously broken, now should work

### 🐛 Bug Fixes

- Resolved application hang when first using the application

## [0.1.20] - 2024-11-11

### 🚀 Features

- *(replays)* Add total damage dealt in a match between the teams
- *(replays)* Selected replay will be highlighted in sidebar
- *(replays)* Add indicator for if a player disconnected from match
- *(replays)* Add action button to see raw player metadata

### 🐛 Bug Fixes

- Log file rotates hourly to reduce total log file size
- *(replays)* Airstrike and plane potential damage are the same

### ⚙️ Miscellaneous Tasks

- Update replay screenshot
- Use better screenshot
- Add github discord workflow
- Bump version to v0.1.20

## [0.1.19] - 2024-11-10

### 🚀 Features

- Show actual damage numbers
- Add button for showing raw battle results
- Add potential and spotting damage + fixed some labels

### ⚙️ Miscellaneous Tasks

- Add upgrade path for re-generating game params in v0.1.19
- Bump version to v0.1.19

## [0.1.18] - 2024-09-14

### 🚀 Features

- *(replays)* Add more statuses to indicate some action was done

### 🐛 Bug Fixes

- *(replays)* Fix bug where app would crash if it was focused at the end of a match
- *(settings)* Setting WoWs directory didn't work so well
- *(replays)* Chat is visually more appealing, easier to read (fixes #3)
- *(app)* Only show update window if there's a build to download

## [0.1.17] - 2024-09-05

### 🐛 Bug Fixes

- *(replays)* Watch replays directory only

## [0.1.16] - 2024-09-05

### 🚀 Features

- *(file_unpacker)* Add support for serializing as JSON/CBOR, including for WoWs Toolkit's internal representation
- Game version updates are auto-detected and new files will be auto-loaded
- *(replays)* Add support for ranked and sending ranked builds back to ShipBuild
- *(replays)* Consolidate the manual replay loading into a single button

## [0.1.15] - 2024-09-03

### 🚀 Features

- *(replays)* Add button for exporting game chat
- *(replays)* Add support for sending replays that were created when app was closed

### 🐛 Bug Fixes

- *(settings)* Sending replay data was not enabled by default
- Log files were not cleared
- *(replays)* Fix ci compilation error

## [0.1.13] - 2024-08-30

### 🐛 Bug Fixes

- *(replays)* Replays would not show any data when parsing

## [0.1.12] - 2024-08-30

### 🚀 Features

- *(resource_unpacker)* Add button for dumping GameParams.json
- Automatically send builds to ShipBuilds.com

### 🚜 Refactor

- Use crates.io versions of wowsunpack and wows_replays

### ⚙️ Miscellaneous Tasks

- Cargo fix
- Cargo fmt
- Update changelog

## [0.1.11] - 2024-06-12

### ⚙️ Miscellaneous Tasks

- Update changelog

## [0.1.10] - 2024-04-02

### 🐛 Bug Fixes

- *(replays)* Fix incompatability with 13.2.0

### ⚙️ Miscellaneous Tasks

- Oops updated changelog before tagging
- Bump version

## [0.1.9] - 2024-03-11

### 🐛 Bug Fixes

- *(replays)* Replays in build-specific dirs should now work

### ⚙️ Miscellaneous Tasks

- Add changelog
- Bump version
- Update changelog

## [0.1.8] - 2024-03-10

### 🚀 Features

- Add support for tomato.gg

### 🐛 Bug Fixes

- *(replays)* Double processing of replays
- Ensure replays dir is correctly reset if wows dir changes
- Improve perf for file listing filter + regression from egui update
- Ensure the found replays dir is used for loading replay files

### 🚜 Refactor

- Tab_name -> title

### ⚙️ Miscellaneous Tasks

- Update egui deps
- Cargo fix
- Bump version

## [0.1.0] - 2024-01-03

<!-- generated by git-cliff -->
