# Changelog

All notable changes to this project will be documented in this file.

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

- *(replays)* Add base xp
- *(replays)* Add checkbox to auto-load most recent replay
- *(replays)* Colorize base XP and damage
- Add new player tracker tab
- *(replays)* Add hover text to break down damage by damage type

### 🐛 Bug Fixes

- *(replays)* Fixed total damage numbers reflecting incorrect teams
- *(replays)* Fix operation replays failing to load

### ⚙️ Miscellaneous Tasks

- Update changelog
- Update gui

### Replays

- Adjust some table column sizes
- Enable auto loading of latest replay by default

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
