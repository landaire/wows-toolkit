# You are encouraged to suggest edits to this file on GitHub by either filing
# an issue, editing the file directly and creating a pull request, or just
# mentioning it in the Discord.
## meta
meta-language_name = English

# -- Language selection dialog --

## dialog
dialog-continue_in_english = Continue in English
dialog-continue_in_language = Continue in English
dialog-select_language = Select Language
dialog-machine_translation_warning = Most translations were machine-generated and may be inaccurate.

# -- Tab titles --

## ui.tabs
ui-tabs-unpacker = Resource Unpacker
ui-tabs-settings = Settings
ui-tabs-replay_parser = Replay Inspector
ui-tabs-player_tracker = Player Tracker
ui-tabs-armor_viewer = Armor Viewer
ui-tabs-model_viewer = Model Viewer
ui-tabs-stats = Stats
ui-tabs-directory_stats = { $dir } Stats
ui-tabs-close_directory_stats = Close directory stats
ui-tabs-search = Search
ui-tabs-search_these_replays_unavailable = These replays have not been indexed yet
ui-tabs-zoom_in_panel = Zoom In
ui-tabs-zoom_out_panel = Zoom Out
ui-tabs-close_panel = Close
ui-tabs-close_tab = Close
ui-tabs-close_other_tabs = Close Others
ui-tabs-close_tabs_to_the_right = Close to the Right

# -- Common buttons --

## ui.buttons
ui-buttons-yes = Yes
ui-buttons-no = No
ui-buttons-send_replays = Send Replays
ui-buttons-send_build_data = Send Build Data Only
ui-buttons-send_nothing = Don't Send Anything
ui-buttons-switch_to_replays = Switch to Replays
ui-buttons-keep_current = Keep Current Setting
ui-buttons-stop = Stop
ui-buttons-copy = Copy
ui-buttons-cancel = Cancel
ui-buttons-continue_ = Continue
ui-buttons-install_update = Install Update
ui-buttons-view_release = View Release
ui-buttons-clear_settings = Clear Settings
ui-buttons-more_options = More Options
ui-buttons-create_issue = Create Issue
ui-buttons-discord = Discord
ui-buttons-github = GitHub
ui-buttons-view_github = View Project on GitHub
ui-buttons-choose = Choose...
ui-buttons-reset = Reset
ui-buttons-clear = Clear
ui-buttons-select_all = Select All
ui-buttons-download = Download
ui-buttons-retry = Retry
ui-buttons-refresh_persisted_data_confirm = Refresh

# -- Menu items --

## ui.menu
ui-menu-file = File
ui-menu-check_updates = Check for Updates
ui-menu-about = About
ui-menu-quit = Quit

# -- Window titles --

## ui.windows
ui-windows-build_consent = Build Collection Consent
ui-windows-replay_migration = Switch to Replay Sharing
ui-windows-crash_detected = Application Crash Detected
ui-windows-update_available = Update Available
ui-windows-network_warning = Network Warning
ui-windows-download_game_data = Download Game Data
ui-windows-error = Error
ui-windows-software_encode = No GPU Video Encoder
ui-windows-about = About
ui-windows-refresh_persisted_data = Refresh Persisted Replay Data
ui-windows-tactics_board = Tactics Board
ui-windows-minimize = Minimize
ui-windows-maximize = Maximize
ui-windows-restore = Restore
ui-windows-close = Close

# -- Dialog / message text --

## ui.dialogs
ui-dialogs-build_consent_message = Choose what battle data to share from ranked and random battles. Sending replays enables much richer matchmaking analysis on the server; sending build data shares only your ship setup. Battles where you play a test ship are never shared as replays. You can change this any time in settings.
ui-dialogs-replay_migration_message = Toolkit can now share full replays, which enables much richer matchmaking analysis than build data alone. Battles where you play a test ship are not shared. Would you like to switch to sharing raw replays?
ui-dialogs-crash_message = It looks like WoWs Toolkit crashed the last time it ran. If you would like to report this issue, please either post an issue on GitHub or join the Discord server and provide the below information.
ui-dialogs-crash_clear_settings = If for some reason data that the application persists may be responsible, you can try clearing settings by pressing the button below. This will clear all settings. Tracked players are kept. Your replays and any WoWs data will be safe.
ui-dialogs-update_message = Version { $tag } of WoWs Toolkit is available
ui-dialogs-update_notes = Release notes
ui-dialogs-update_asset_pending = The Windows download is not available yet. View the release for build status.
ui-dialogs-update_windows_only = Update available, but only Windows is supported at this time.
ui-dialogs-p2p_warning = Collaborative sessions use peer-to-peer networking. Other users in the session may be able to see your IP address.
ui-dialogs-software_encode_message = No supported GPU video encoder was found, so this export will encode in software. That is much slower, and a long battle can take several minutes.
ui-dialogs-download_game_data_intro = Some replays need game data that is not installed. Pick the builds to download.
ui-dialogs-download_build_row = Version { $version } (build { $build })
ui-dialogs-download_replays_needing = needed by { $count ->
    [one] { $count } replay
   *[other] { $count } replays
}
ui-dialogs-download_availability_exact = published
ui-dialogs-download_availability_nearest = no exact match, nearest is { $version } (build { $build })
ui-dialogs-download_availability_unpublished = never published
ui-dialogs-download_availability_unreachable = published, but its file list could not be read
ui-dialogs-download_plan_pending = Working out what needs fetching...
ui-dialogs-download_objects_to_fetch = Objects to fetch for the ticked builds: { $count }
ui-dialogs-download_plan_failed = Could not work out what needs fetching
ui-dialogs-download_plan_no_cache_dir = No game data cache directory is set. Choose one in Settings, then try again.
ui-dialogs-refresh_persisted_data_message = Re-parse every replay in your replays directory and rewrite its stored data. Entries recorded with a replay data mapping that does not match their game build are left as they are, except that Personal Rating and game mode are recomputed from today's expected values for every replay. This can take a while.
ui-dialogs-file_dialog_failed = The file dialog could not be opened: { $reason }

# -- Labels --

## ui.labels
ui-labels-error_occurred = An error occurred:
ui-labels-made_by = Made by landaire.
ui-labels-credits = Thanks to Trackpad, TTaro, lkolbly for their contributions.
ui-labels-pr_credits = Personal rating (PR) calculation data and formula provided by WoWs Numbers.
ui-labels-suppress_warning = Don't show this again
ui-labels-more_info = More info
ui-labels-app_version = Version { $version }
ui-labels-project_license = WoWs Toolkit is distributed under the { $license } license.
ui-labels-third_party_notices = Third-party notices
ui-labels-notices_search_hint = Search notices
ui-labels-no_matching_notices = No notices match the search.

# -- Toast messages --

## ui.messages
ui-messages-bug_report_info_copied = Bug report information copied.
ui-messages-update_downloading = Downloading the update: { $read } of { $total }
ui-messages-update_downloading_unknown = Downloading the update: { $read } so far
ui-messages-app_up_to_date = Application up-to-date
ui-messages-checking_app_updates = Checking for app updates...
ui-messages-update_starting = Downloading the update...
ui-messages-update_check_failed = Failed to check for app updates
ui-messages-game_data_loaded = Successfully loaded game data
ui-messages-no_replays_detected = No replays detected. Is your WoWs directory properly configured?
ui-messages-game_data_builds_downloaded = Downloaded game data for { $count ->
    [one] { $count } build
   *[other] { $count } builds
}
ui-messages-game_data_download_failed = Could not download game data
ui-messages-game_data_up_to_date = Game data is up to date
ui-messages-game_data_updates_available = { $count ->
    [one] { $count } cached build has newer data available
   *[other] { $count } cached builds have newer data available
}
ui-messages-game_data_cache_valid = Game data cache is valid and matches the remote repository
ui-messages-game_data_cache_invalid = { $count ->
    [one] { $count } cached build is out of date or corrupt
   *[other] { $count } cached builds are out of date or corrupt
}
ui-messages-replay_load_failed = Failed to load replay
ui-messages-twitch_token_invalid = Twitch token is invalid or expired. Please update it in Settings.
ui-messages-wows_dir_invalid = World of Warships directory is not valid. Check the path in Settings.
ui-messages-wows_dir_not_set = World of Warships directory is not set
ui-messages-constants_version_mismatch =
    Replay data mapping file version does not match game version.
    Post-battle results may not be accurate. Please be patient while project maintainers update the mapping on the server.
ui-messages-session_started = Session started
ui-messages-user_joined = { $name } joined
ui-messages-user_timeout = { $name } timed out
ui-messages-user_left = { $name } left
ui-messages-session_ended = Session ended
ui-messages-session_error = Session error: { $msg }
ui-messages-connected_to_session = Connected to session
ui-messages-host_opened_replay = Host opened replay: { $name }
ui-messages-host_closed_replay = Host closed a replay
ui-messages-session_rejected = Session rejected: { $reason }
ui-messages-drop_not_a_replay = That is not a replay file
ui-messages-drop_not_replays = Those are not replay files
ui-messages-drop_skipped = Skipped { $count ->
    [one] { $count } dropped file that is not a replay or folder
   *[other] { $count } dropped files that are not replays or folders
}
ui-messages-log_copied = Copied { $bytes } KB from { $file } to the clipboard
ui-messages-log_copy_failed = Could not read a log file: { $error }
ui-messages-log_not_found = No log file was found in { $dir }

# -- Stat names --

## stat
stat-damage = Damage
stat-spotting_damage = Spotting Damage
stat-frags = Frags
stat-raw_xp = Raw XP
stat-base_xp = Base XP
stat-win_rate = Win Rate
stat-personal_rating = Personal Rating
stat-match_contribution = Match Contribution
stat-capture_points = Capture Points
stat-planes_shot_down = Planes Shot Down
stat-anti_air_damage = Anti-Air Damage
stat-survival_rate = Survival Rate
stat-torpedoes_spotted = Torpedoes Spotted
stat-avg_pr = Avg PR
stat-avg_prefix = Avg { $name }

## chart
chart-combined = Combined
chart-combined_suffix = (Combined)
chart-rolling_average_suffix = (Rolling Average)

# -- Confirmation dialogs --

## confirm
confirm-open_in_game = This will launch World of Warships. Continue?
confirm-clear_all_session_stats = This will clear all session stats. Continue?
confirm-set_as_session_stats = This will replace your current session stats. Continue?

# -- Error messages --

## error
error-file_watcher_creation = Failed to create file watcher: { $error }
error-replay_dir_watch = Failed to watch replay directory: { $error }

# -- Settings --

## ui.settings.app
ui-settings-app-check_for_updates = Check for Updates on Startup
ui-settings-app-enable_logging = Enable Logging (takes effect on restart)
ui-settings-app-enable_logging_tooltip = Write debug logs to a file in the application data directory. Useful for troubleshooting.
ui-settings-app-data_sharing_mode = Data sharing (ranked and random battles)
ui-settings-app-theme = Theme
ui-settings-app-theme_system = System
ui-settings-app-theme_dark = Dark
ui-settings-app-theme_light = Light
ui-settings-app-code_integrity = Block Unsigned Code (takes effect on restart)
ui-settings-app-code_integrity_tooltip = Refuses to load code that Microsoft did not sign, which keeps overlays, capture tools and audio utilities out of this application. Those are a common cause of crashes and freezes while rendering. Graphics drivers are signed through Windows and keep working. This cannot be turned off once a session has started, so a change applies at the next launch. Automatic skips it when you have a third-party input method installed, which it would otherwise block. Note that other protections stay on regardless of this setting; run with --no-hardening to turn off all of them.
ui-settings-app-code_integrity_automatic = Automatic
ui-settings-app-code_integrity_always = Always
ui-settings-app-code_integrity_never = Never
ui-settings-app-language = Language
ui-settings-app-open_data_dir_tooltip = Open the application data folder (settings, logs, cached data) in your file manager.
ui-settings-app-proxy_url = HTTP Proxy
ui-settings-app-proxy_url_hint_live = host:port, or leave empty to use your system proxy settings

## ui.settings.wows
ui-settings-wows-description = Path to your World of Warships installation
ui-settings-wows-directory_hint = World of Warships Directory
ui-settings-wows-browse = Browse...

## ui.settings.wows.cache
ui-settings-wows-cache-heading = Game Data Cache
ui-settings-wows-cache-description = Dumped game data, so replays from older builds still open
ui-settings-wows-cache-auto_dump = Cache game data for old replay compatibility
ui-settings-wows-cache-browse = Browse
ui-settings-wows-cache-open_folder = Open Folder
ui-settings-wows-cache-delete_old = Delete Old Versions
ui-settings-wows-cache-stats = Game data cache: { $size } across { $count ->
    [one] { $count } version
   *[other] { $count } versions
}
ui-settings-wows-cache-check_updates = Check for Updates
ui-settings-wows-cache-actions = Cache Actions
ui-settings-wows-cache-actions_tooltip = Check for updates or validate cached builds
ui-settings-wows-cache-update_all = Update All
ui-settings-wows-cache-updates_available = { $count ->
    [one] { $count } cached build has newer data available
   *[other] { $count } cached builds have newer data available
}
ui-settings-wows-cache-validate = Validate Cache
ui-settings-wows-cache-repair = Repair
ui-settings-wows-cache-repair_needed = { $count ->
    [one] { $count } cached build is out of date or corrupt
   *[other] { $count } cached builds are out of date or corrupt
}

## ui.settings.replay
ui-settings-replay-show_raw_xp = Show Raw XP
ui-settings-replay-show_entity_id = Show Entity ID
ui-settings-replay-show_observed_damage = Show Observed Damage
ui-settings-replay-show_damage_dealt_bar = Show Damage Dealt Bar
ui-settings-replay-show_damage_received_bar = Show Damage Received Bar
ui-settings-replay-show_nation_flags = Show Nation Flags
ui-settings-replay-enable_previews = Show Minimap Preview On Hover
ui-settings-replay-enable_previews_tooltip = Hovering a replay reads and replays it in the background to build a looping minimap preview. Turn this off on slow storage.
ui-settings-replay-auto_export_data = Auto-Export Data
ui-settings-replay-export_path_hint = Data auto-export directory
ui-settings-replay-export_path_missing = The directory does not exist. No data can be exported to it.
ui-settings-replay-index_all_replays = Index All Replays

ui-settings-replay-show_heals = Show Heals
ui-settings-replay-current_path = Current Replay Path
## ui.settings.index
ui-settings-index-heading = Replay Index
ui-settings-index-description = The searchable record of every replay in your directory
ui-settings-index-build = Build Index
ui-settings-index-rebuild = Re-index Everything
ui-settings-index-rebuild_hint = Read every replay again, for rows an older parse got wrong. Build Index reads only what is missing.
ui-settings-index-progress = { $done } of { $total } read, { $failed } could not be
ui-settings-index-progress_skipped = { $done } of { $total } read, { $skipped } already in, { $failed } could not be
ui-settings-index-built = Indexed { $indexed ->
    [one] { $indexed } replay
   *[other] { $indexed } replays
}, { $failed } could not be read
ui-settings-index-built_with_skipped = Indexed { $indexed ->
    [one] { $indexed } replay
   *[other] { $indexed } replays
}, { $skipped ->
    [one] { $skipped } was already in
   *[other] { $skipped } were already in
}, { $failed } could not be read

## ui.settings.session
ui-settings-session-display_name = Display Name
ui-settings-session-suppress_ip_warning = Don't show P2P IP address warning
ui-settings-session-ip_warning_tooltip = Collaborative sessions use peer-to-peer networking. Other users in the session may be able to see your IP address.
ui-settings-session-disable_auto_open = Disable auto-opening session windows
ui-settings-session-auto_open_tooltip = When enabled, session windows (replays, tactics boards) won't open automatically. You can still open them from the session popover.

## ui.settings
ui-settings-loading = Loading settings...
ui-settings-load_failed = Failed to load settings
ui-settings-not_set = (not set)

## ui.settings.panel
ui-settings-panel-mcp = MCP Server
ui-settings-panel-open = Open Settings
ui-settings-panel-search_placeholder = Search settings
ui-settings-panel-count = { $count ->
    [one] { $count } setting
   *[other] { $count } settings
}
ui-settings-panel-no_matches = No settings match the search
ui-settings-panel-close = Close settings
ui-settings-panel-in_error = { $name }: a setting needs attention
ui-settings-panel-general = General
ui-settings-panel-appearance = Appearance
ui-settings-panel-game = World of Warships
ui-settings-panel-replays = Replays
ui-settings-panel-collaboration = Collaboration
ui-settings-panel-twitch = Twitch
ui-settings-panel-language_description = Language of the interface
ui-settings-panel-data_sharing_description = What is sent to ShipBuilds.com after ranked and random battles
ui-settings-panel-code_integrity_description = Whether code that Microsoft did not sign may load into this application
ui-settings-panel-check_for_updates_description = Look for a newer release each time the application starts
ui-settings-panel-data_folder = App Data Folder
ui-settings-panel-theme_description = Colors of the interface. Point at a choice to preview it.
ui-settings-panel-zoom_description = Size of all text and controls
ui-settings-panel-zoom = Zoom
ui-settings-panel-zoom_shortcuts = { $zoom_in } zooms in, { $zoom_out } zooms out, { $reset } returns to 1x
ui-settings-panel-zoom_changed = Zoom: { $factor }x
ui-settings-panel-cache_directory = Cache Directory
ui-settings-panel-cache_directory_description = Where the cached builds are stored
ui-settings-panel-cached_builds = Cached Builds
ui-settings-panel-current_path_description = The replay file opened most recently
ui-settings-panel-show_raw_xp_description = Add a raw XP column to the results table
ui-settings-panel-show_observed_damage_description = Add an observed damage column to the results table
ui-settings-panel-show_entity_id_description = Add an entity ID column to the results table
ui-settings-panel-show_heals_description = Add a heals column to the results table
ui-settings-panel-show_damage_dealt_bar_description = Draw the damage type bar under damage dealt in the replay player table
ui-settings-panel-show_damage_received_bar_description = Draw the damage type bar under damage received in the replay player table
ui-settings-panel-show_nation_flags_description = Draw each ship's nation flag in the replay player table
ui-settings-panel-auto_export_description = Write the data of each battle to a file in the chosen format and directory
ui-settings-panel-display_name_description = The name others in a session see
ui-settings-panel-get_token_description = Open a sign-in page that issues a chat token
ui-settings-panel-token = Login Token
ui-settings-panel-monitored_channel_description = Channel to watch. Leave empty to watch your own.
ui-settings-panel-armor_defaults = Saved Defaults
ui-settings-panel-armor_saved_description = Saved from the Model Viewer options

## ui.settings.licenses
ui-settings-licenses-title = About and licenses
ui-settings-licenses-back = Back to settings
ui-settings-licenses-third_party_intro = This application contains the third-party components listed below. Select an entry to show the crates that use it and its full license text.

## ui.settings.armor
ui-settings-armor-no_defaults = (no saved defaults)

## ui.app
ui-app-status_cache_checking = Checking the game data cache
ui-app-status_cache_validating = Verifying the game data cache
ui-app-status_cache_downloading = Downloading game data
ui-app-status_cache_planning = Asking what game data is published
ui-app-drop_to_load = Drop to load { $file }
ui-app-drop_to_list = Drop to list the replays in { $folder }
ui-app-drop_to_list_replays = Drop to list { $count ->
    [one] { $count } replay
   *[other] { $count } replays
}
ui-app-drop_to_list_folders = Drop to list the replays in { $count ->
    [one] { $count } folder
   *[other] { $count } folders
}
ui-app-drop_to_open_and_list = Drop to open { $file } and list the replays in { $folders }
ui-app-drop_to_list_replays_and_folders = Drop to list { $count ->
    [one] { $count } replay
   *[other] { $count } replays
} and the replays in { $folders }
ui-app-drop_folder_count = { $count ->
    [one] { $count } folder
   *[other] { $count } folders
}
ui-app-reading_replay = Reading { $replay }
ui-app-reading_replays = Reading { $count ->
    [one] { $count } replay
   *[other] { $count } replays
}
ui-app-status_constants_generating = Updating game constants for { $version }
ui-app-worker_constants = The game constants worker
ui-app-worker_live_match = The battle watch
ui-app-worker_replay_loader = The replay loader
ui-app-worker_restarting = { $worker } stopped and is restarting
ui-app-worker_stalled = { $worker } is not responding
ui-app-worker_stopped = { $worker } stopped
ui-app-status_indexing = Indexing replays
ui-app-status_of = { $done } of { $total }
ui-app-status_tasks = { $count ->
    [one] { $count } task running
   *[other] { $count } tasks running
}
ui-app-status_show_panel = Show panel: { $panel }
ui-app-status_hide_panel = Hide panel: { $panel }
ui-app-panel_left = Left
ui-app-panel_right = Right
ui-app-panel_bottom = Bottom
ui-app-debug_build = Debug build

# -- Command palette --

## ui.palette
ui-palette-title = Commands
ui-palette-placeholder = Type a command or search...
ui-palette-go_to = Go to: { $tab }
ui-palette-advanced_search = Advanced search...
ui-palette-set_theme = Theme: { $theme }
ui-palette-games_i_died_in = Games I died in
ui-palette-games_i_won = Games I won
ui-palette-send_all_replays = Send All Replays to ShipBuilds
ui-palette-send_all_replays_again = Send All Replays to ShipBuilds (ignore the ledger)
ui-palette-search_players = Search players...
ui-palette-my_matches_in_ship = My matches in ship...
ui-palette-open_ship_in_armor_viewer = Open ship in Model Viewer...
ui-palette-open_ship_in_shiptool = Open ship in ShipTool...
ui-palette-players_title = Search players
ui-palette-my_ships_title = My matches in ship
ui-palette-armor_ships_title = Open ship in Model Viewer
ui-palette-shiptool_ships_title = Open ship in ShipTool
ui-palette-players_placeholder = Search players by name...
ui-palette-my_ships_placeholder = Search ships you have played...
ui-palette-armor_ships_placeholder = Search every ship by name...
ui-palette-armor_ship_row = { $name } -- Tier { $tier } { $class }, { $nation }
ui-palette-back_hint = Esc: back
ui-palette-ships_loading = Loading the ship list...
ui-palette-ships_unavailable = No ship list: { $reason }
ui-palette-matches_found = { $count ->
    [one] { $count } match
   *[other] { $count } matches
}
ui-palette-sample_stats = Debug: show sample data in Stats
ui-palette-restore_stats = Debug: show recorded data in Stats

## ui.settings.twitch
ui-settings-twitch-get_token = Get Login Token
ui-settings-twitch-get_token_tooltip = We use Chatterino's login page as it provides a token with the necessary permissions (basically a moderator token with chat permissions), and it removes the need for the WoWs Toolkit developer to host their own login page website which would have the same result.
ui-settings-twitch-paste_token_no_token = Paste Token (No Token Set)
ui-settings-twitch-paste_token_tooltip = Reads the credential from the clipboard
ui-settings-twitch-paste_token_valid = Paste Token (Token Valid)
ui-settings-twitch-paste_token_unvalidated = Checking token
ui-settings-twitch-paste_token_invalid = Paste Token (Token Invalid / Expired)
ui-settings-twitch-monitored_channel = Monitored Channel (Default to Self)
ui-settings-twitch-signed_in_as = Signed in as { $who }

# -- Replay Inspector --

## ui.replay.hover
ui-replay-hover-ship = Ship
ui-replay-hover-map = Map
ui-replay-hover-mode = Mode
ui-replay-hover-blank_mode = Blank
ui-replay-hover-played = Played
ui-replay-hover-damage = Damage
ui-replay-hover-kills = Kills
ui-replay-hover-result = Result
ui-replay-hover-division = Division
ui-replay-hover-twitch = Twitch Chat

## ui.replay
ui-replay-copy_bug_report_info = Copy Bug Report Info
ui-replay-match_age = { $age } ago
ui-replay-listing_caption = Replays
ui-replay-loading_game_data_version = Loading game data for { $version }...
ui-replay-spectator = Spectator
ui-replay-loading = Loading...
ui-replay-preview_loading = Building preview...
ui-replay-preview_no_game_data = No game data for client version { $value }
ui-replay-preview_unreadable_because = This replay could not be read: { $value }
ui-replay-preview_unwalkable = This replay's battle could not be walked: { $value }
ui-replay-preview_unknown_version = This replay reports an unreadable client version: { $value }
ui-replay-preview_no_map = No map data for { $value }
ui-replay-preview_render_failed = The preview could not be drawn: { $value }
ui-replay-incomplete_results = Incomplete Match Results
ui-replay-incomplete_results_tooltip = The replay does not yet have end-of-match results. Data will be automatically re-loaded when the match ends and end-of-match results are added to the replay.
ui-replay-no_chat = No chat messages were sent in this replay
ui-replay-chat_copy_all = Copy all
ui-replay-chat_save_to_file = Save to file
ui-replay-chat_save_title = Save game chat
ui-replay-export_results_title = Export results
ui-replay-results_exported = Results exported
ui-replay-chat_saved = Chat saved to { $path }
ui-replay-chat_save_failed = The chat was not saved: { $error }
ui-replay-team_damage = Team Damage:{" "}
ui-replay-team_share = { $pct } of the team total ({ $team })
ui-replay-hide_my_stats = Hide My Test Ship Stats
ui-replay-open_manually = Manually Open Replay File...
ui-replay-open_directory = Open Replay Directory...
ui-replay-search_these_replays = Search these replays
ui-replay-import_constants = Import constants.json File...
ui-replay-refresh_persisted_data = Refresh Persisted Replay Data...
ui-replay-contributing = Contributing { $done } of { $total ->
    [one] { $total } replay
   *[other] { $total } replays
}
ui-replay-contributed = Contributed { $sent } of { $total ->
    [one] { $total } replay
   *[other] { $total } replays
}
ui-replay-contribute_nothing_shared = Choose what to share in Settings first
ui-replay-contribute_none_listed = No replays are listed to contribute
ui-replay-contribute_not_ready = Replays can be contributed once the game data and the settings have loaded
ui-replay-contribute_already_running = A contribution pass is already running
ui-replay-auto_export_failed = A battle could not be written out: { $error }
ui-replay-auto_exported = Exported the battle to { $file }
ui-replay-opened_new_battle = Opened the new battle: { $battle }
ui-replay-new_battle_listed = New battle: { $battle }
ui-replay-results_arrived = The battle results for { $battle } arrived
ui-replay-constants_written = { $count ->
    [one] The result mapping for { $count } build was downloaded
   *[other] Result mappings for { $count } builds were downloaded
}
ui-replay-constants_import_title = Import a constants.json
ui-replay-constants_imported = The result mapping for build { $build } was imported
ui-replay-constants_import_failed = That mapping was not imported: { $reason }
ui-replay-constants_no_build = Load a game build first
ui-replay-constants_latest_written = The newest result mapping was downloaded
ui-replay-constants_check_failed = The result mappings could not be checked: { $reason }
ui-replay-constants_build_label = build { $build }
ui-replay-constants_generating = Updating game constants for { $version }...
ui-replay-constants_generated = Game constants for { $version } were generated in { $seconds } s
ui-replay-constants_generation_failed = Game constants for { $version } could not be updated: { $reason }. { $fallback }
ui-replay-constants_fallback_kept = Keeping the constants already saved for it.
ui-replay-constants_fallback_published = Using the constants published for build { $build }.
ui-replay-constants_fallback_embedded = Using the built-in constants.
ui-replay-copy_latest_log = Copy Latest Log to Clipboard
ui-replay-load_alt_perspective = Load Other Team Perspective...
ui-replay-load_alt_perspective_failed = Failed to merge replay: { $error }
ui-replay-load_alt_perspective_limit = This battle is already being read through { $limit ->
    [one] { $limit } other recording
   *[other] { $limit } other recordings
}
ui-replay-consumable_charges_remaining = { $remaining } remaining out of { $total ->
    [one] { $total } charge
   *[other] { $total } charges
}
ui-replay-consumable_charges_unlimited = { $used } used (unlimited charges)
ui-replay-consumable_header_remaining = Remaining
ui-replay-consumable_header_total = Total
ui-replay-consumable_charges_infinite = ∞
ui-replay-autoload_latest = Autoload Latest Replay
ui-replay-autoload_latest_tooltip = Open a match as soon as the game finishes writing it
ui-replay-anonymize_names = Anonymize player names
ui-replay-anonymize_names_tooltip = Show generated names in place of player names and clan tags. Exports keep the real names.
ui-replay-group_win_rate_all_modes = Count all game modes in group win rates
ui-replay-group_win_rate_all_modes_tooltip = By default, group headers count only Random and Ranked battles. With this on, every game mode counts and the win rate is not colored, since the colors are calibrated for Random battles.
ui-replay-select_replay = Select a replay
ui-replay-drop_hint = Drop a replay here to open it, or a folder to list its replays
ui-replay-scanning = Scanning replays...
ui-replay-scanning_count = Reading replay { $done } of { $total }
ui-replay-no_replays_in = No replays in { $folder }
ui-replay-drop_folder_hint = Drop a folder on the window to list it in its own tab
ui-replay-new_tag = New
ui-replay-loading_game_data = Loading game data...
ui-replay-game_data_failed = Game data failed to load: { $error }
ui-replay-group_label = Replay grouping
ui-replay-column_filters = Column Filters
ui-replay-row_elements = Row Elements
ui-replay-row_element_personal_rating = Personal Rating
ui-replay-row_element_match_contribution = Match Contribution
ui-replay-row_element_division = Division
ui-replay-row_element_twitch = Twitch Chat
ui-replay-row_element_damage = Damage
ui-replay-row_element_kills = Kills
ui-replay-row_element_time = Battle Time
ui-replay-listing_options = Listing Options
ui-replay-open = Open
ui-replay-open_file = Open Replay File...
ui-replay-check_game_data = Check game data
ui-replay-check_game_data_tooltip = Check for game data required by replays in this directory
ui-replay-close_side_panel = Close Panel
ui-replay-nda = NDA
ui-replay-chat = Chat
ui-replay-game_chat = Game Chat
ui-replay-save_to_file = Save To File
ui-replay-chat_copied = Chat copied
ui-replay-copy_message = Copy message
ui-replay-actions = Actions
ui-replay-row_actions_hint = Player actions (or right-click the row)
ui-replay-section_match = Match
ui-replay-section_export = Export
ui-replay-section_debug = Debug
ui-replay-timeline = Match Timeline
ui-replay-row_not_indexed = not indexed
ui-replay-hover_survived = Survived
ui-replay-hover_sunk = Sunk
ui-replay-hover_not_indexed = Not indexed
ui-replay-row_kills = { $count ->
    [one] { $count } kill
   *[other] { $count } kills
}
ui-replay-row_survived = alive
ui-replay-row_sunk = sunk
ui-replay-row_division = Division: { $members }
ui-replay-export_results_json = Results: JSON
ui-replay-export_results_cbor = Results: CBOR
ui-replay-export_results_csv = Results: CSV
ui-replay-timeline_filter_all = All
ui-replay-timeline_filter_none = None
ui-replay-timeline_search_hint = Search ship or player
ui-replay-timeline_no_events = No significant events detected.
ui-replay-timeline_no_matches = No events match the current filter.
ui-replay-timeline_parsing = Parsing events...
ui-replay-timeline_failed = Failed to extract the match timeline: { $error }
ui-replay-timeline_retry = Retry
ui-replay-timeline_export_tooltip = Applies to the events currently visible after filtering.
ui-replay-timeline_copied = Timeline copied
ui-replay-timeline_event_copied = Event copied
ui-replay-timeline_saved = Timeline saved to { $path }
ui-replay-timeline_save_failed = The timeline was not saved: { $error }
ui-replay-timeline_kind_health_lost = Heavy Damage
ui-replay-timeline_kind_death = Destroyed
ui-replay-timeline_kind_cap_contested = Cap Contested
ui-replay-timeline_kind_cap_flipped = Cap Captured
ui-replay-timeline_kind_cap_being_captured = Cap Being Captured
ui-replay-timeline_kind_radar = Radar Used
ui-replay-timeline_kind_advantage = Advantage Change
ui-replay-timeline_kind_disconnected = Disconnected

## ui.replay.context
ui-replay-context-open_in_new_tab = Open in New Tab
ui-replay-context-copy_cell = Copy Cell
ui-replay-context-copy_row = Copy Row
ui-replay-context-copy_table = Copy Table
ui-replay-context-pin_column = Pin Column
ui-replay-context-unpin_column = Unpin Column
ui-replay-context-copied = Copied
ui-replay-context-copy_path = Copy Path
ui-replay-context-copy_replays = { $count ->
    [one] Copy Replay
   *[other] Copy { $count } Replays
}
ui-replay-context-copy_paths = { $count ->
    [one] Copy Path
   *[other] Copy { $count } Paths
}
ui-replay-context-show_in_explorer = Show in File Explorer
ui-replay-context-show_replay_controls = Show Replay Controls
ui-replay-context-open_in_game = Open in Game
ui-replay-context-render_replay = Render Replay
ui-replay-context-set_session_stats = { $count ->
    [one] Set as Session Stats ({ $count } replay)
   *[other] Set as Session Stats ({ $count } replays)
}
ui-replay-context-add_session_stats = { $count ->
    [one] Add to Session Stats ({ $count } replay)
   *[other] Add to Session Stats ({ $count } replays)
}
ui-replay-context-render_to_video = { $count ->
    [one] Render to Video
   *[other] Render { $count } Replays to Video
}
ui-replay-context-render_to_clipboard = { $count ->
    [one] Render to Clipboard
   *[other] Render { $count } Replays to Clipboard
}

## ui.replay.column
ui-replay-column-player_name = Player Name
ui-replay-column-base_xp = Base XP
ui-replay-column-ship_name = Ship Name
ui-replay-column-hits = Hits
ui-replay-column-heals = Heals
ui-replay-column-heals_tooltip = Number of Repair Party activations observed for this player. May be inaccurate for ships not rendered on screen (i.e. enemy ships that were never spotted).
ui-replay-column-heals_no_repair_tooltip = This ship does not have a Repair Party consumable.
ui-replay-column-kills = Kills
ui-replay-column-observed_damage = Observed Damage
ui-replay-column-actual_damage = Actual Damage
ui-replay-column-potential_damage = Potential Damage
ui-replay-column-time_lived = Time Lived
ui-replay-column-fires = Fires
ui-replay-column-floods = Floods
ui-replay-column-citadels = Citadels
ui-replay-column-crits = Crits
ui-replay-column-received_damage = Received Damage
ui-replay-column-distance_traveled = Distance Traveled
ui-replay-column-skills = Skills
ui-replay-column-personal_rating = PR
ui-replay-column-match_contribution = MC

## ui.rating
ui-rating-personal_rating = Personal Rating
ui-rating-match_contribution = Match Contribution
ui-rating-versus_average = vs average
ui-rating-damage = Damage
ui-rating-kills = Kills
ui-rating-wins = Win
ui-rating-team_damage_share = Share of team damage
ui-rating-tanking = Fire drawn
ui-rating-spotting = Spotting
ui-rating-objectives = Objectives
ui-rating-anti_air = Anti-air
ui-rating-torpedo_warnings = Torpedo warnings
ui-rating-critical_bursts = Critical bursts
ui-rating-unrated_early_exit = No MC: the recording ended before the battle did
ui-rating-mc_unrated_battle_type = No MC: match contribution applies only to Random and Ranked battles

## ui.rating.detail
ui-rating-detail-versus_expected = { $actual } vs { $expected } expected
ui-rating-detail-wins = { $actual } won vs { $expected } expected
ui-rating-detail-weighted_damage = { $actual } vs { $expected } expected, destroyer and submarine damage x1.2
ui-rating-detail-team_damage_share = { $share }% of the team's damage to enemies, { $even }% is an even share
ui-rating-detail-tanking = { $potential } potential damage, { $class }x the class average, { $team }x an even share of the team's
ui-rating-detail-spotting = { $damage } spotting damage (class average { $baseline }), { $ships } ships' HP spotted (average { $ships_baseline })
ui-rating-detail-objectives = Score points: { $capture } capturing, { $block } blocking, { $reset } resetting, { $battle_end } held at the end, { $hold } holding (counted at a quarter)
ui-rating-detail-anti_air_planes = { $strike ->
    [one] { $strike } attack plane
   *[other] { $strike } attack planes
}, { $support ->
    [one] { $support } fighter or scout
   *[other] { $support } fighters and scouts
}
ui-rating-detail-anti_air_damage = { $damage } AA damage
ui-rating-detail-torpedo_warnings = { $threatening ->
    [one] { $threatening } threatening torpedo spotted
   *[other] { $threatening } threatening torpedoes spotted
}
ui-rating-detail-critical_bursts = { $count ->
    [one] { $count } critical burst
   *[other] { $count } critical bursts
}: { $bursts }. Seen only for ships near the recording player
ui-rating-detail-critical_burst = { $share }% of { $ship } in { $seconds } s
ui-rating-detail-critical_burst_at_once = { $share }% of { $ship } at once

## ui.personal_rating
ui-personal_rating-unrated_hover = Personal rating applies only to Random and Ranked battles.

## ui.rating.basis
ui-rating-basis-static = Static
ui-rating-basis-dynamic = Dynamic
ui-rating-basis-static_explained = Static: measured against a fixed constant or a class baseline
ui-rating-basis-dynamic_explained = Dynamic: measured against wows-numbers expected values or the other players in this battle

## ui.replay.group
ui-replay-group-date = Date
ui-replay-group-ship = Ship
ui-replay-group-none = None

## ui.replay.player
ui-replay-player-hidden_profile = Player has a hidden profile
ui-replay-player-never_connected = Player never connected to match

## ui.replay.build
ui-replay-build-open_shiptool = Open Build in ShipTool
ui-replay-build-open_in_ship_builder = Open Build in WoWs SB
ui-replay-build-copy_link = Copy Build Link
ui-replay-build-copy_short_link = Copy Short Build Link
ui-replay-build-link_copied = Build link copied
ui-replay-build-open_wows_numbers = Open WoWs Numbers Page
ui-replay-build-not_spotted = This ship was never spotted. Build info unavailable.

## ui.replay.sections
ui-replay-sections-achievements = Achievements
ui-replay-sections-ribbons = Ribbons
ui-replay-sections-damage_events = Damage Events
ui-replay-sections-modules = Modules:
ui-replay-sections-modules_none = No Modules
ui-replay-sections-signals = Signals:
ui-replay-sections-loadout = Loadout:
ui-replay-sections-loadout_none = No Loadout
ui-replay-sections-captain_skills = Captain Skills:
ui-replay-sections-captain_skills_none = No Captain Skills
ui-replay-sections-consumables = Consumables:
ui-replay-sections-fire_chance = Effective Fire Chance
ui-replay-sections-fire_chance_expected = expected
ui-replay-sections-fire_chance_fires = { $fires ->
    [one] { $fires } fire
   *[other] { $fires } fires
}
ui-replay-sections-fire_chance_eligible_hits = { $hits ->
    [one] { $hits } hit that could have started one
   *[other] { $hits } hits that could have started one
}
ui-replay-sections-fire_chance_ships = across { $ships ->
    [one] { $ships } target ship
   *[other] { $ships } target ships
}
ui-replay-sections-fire_chance_expected_fires = expected { $fires } fires
ui-replay-sections-fire_chance_per_ship = Per Target Ship
ui-replay-sections-fire_chance_no_eligible_hits = no hits that could have started a fire
ui-replay-sections-fire_chance_formula = Attacker fire chance formula
ui-replay-sections-fire_chance_formula_base = base burnProb
ui-replay-sections-fire_chance_formula_clamped = clamped
ui-replay-sections-fire_chance_breakdown = Breakdown
ui-replay-sections-fire_chance_shells_fired = { $count ->
    [one] HE shell fired
   *[other] HE shells fired
}
ui-replay-sections-fire_chance_he_hits = { $count ->
    [one] HE hit on a ship
   *[other] HE hits on a ship
}
ui-replay-sections-fire_chance_no_target_ship = { $count ->
    [one] HE hit not attributable to a target ship
   *[other] HE hits not attributable to a target ship
}
ui-replay-sections-fire_chance_eligible = could have started a fire
ui-replay-sections-fire_chance_ribbons = { $count ->
    [one] SetFire ribbon
   *[other] SetFire ribbons
}
ui-replay-sections-fire_chance_ribbons_credited = credited to a shell
ui-replay-sections-fire_chance_ribbons_uncredited = not credited
ui-replay-sections-fire_chance_unattributed_no_burn_state = no burn state change to match it against
ui-replay-sections-fire_chance_unattributed_already_credited = an earlier fire already used the nearby hit
ui-replay-sections-fire_chance_unattributed_secondary_contest = one of our own secondaries could equally have set it
ui-replay-sections-fire_chance_unattributed_hit_contest = every nearby hit of ours was ambiguous with another
ui-replay-sections-fire_chance_unattributed_all_excluded = every nearby hit of ours was already excluded
ui-replay-sections-fire_chance_unattributed_none_could_burn = no nearby hit of ours could have started a fire
ui-replay-sections-fire_chance_unattributed_no_hit = no hit of ours landed in the window
ui-replay-sections-fire_chance_not_applicable = victim already dead, not applicable
ui-replay-sections-fire_chance_copied = Fire chance breakdown copied
ui-replay-sections-fire_chance_click_to_copy = Click to copy this breakdown
ui-replay-sections-fire_chance_exclusion_already_burning = section already burning
ui-replay-sections-fire_chance_exclusion_victim_build_unknown = victim build unknown, fire zones may be merged
ui-replay-sections-fire_chance_exclusion_damage_control_active = Damage Control Party active
ui-replay-sections-fire_chance_exclusion_damage_control_unknown = damage control state unknown
ui-replay-sections-fire_chance_exclusion_observation_gap = observation gap
ui-replay-sections-fire_chance_exclusion_consumable_unreliable = consumable activation model unreliable
ui-replay-sections-fire_chance_exclusion_victim_fate_unknown = victim fate unknown
ui-replay-sections-fire_chance_exclusion_hit_type_does_not_roll = hit type does not roll for fire
ui-replay-sections-fire_chance_exclusion_no_geometry = no fire section geometry
ui-replay-sections-fire_chance_exclusion_impact_unplaceable = could not place the impact on the ship we matched it to
ui-replay-sections-fire_chance_exclusion_victim_pose_unknown = victim's position not known at impact
ui-replay-sections-fire_chance_exclusion_ambiguous_hit = ambiguous with another hit of ours

## ui.replay.debug
ui-replay-debug-view_raw_metadata = View Raw Player Metadata
ui-replay-debug-raw_metadata = Raw Metadata
ui-replay-debug-raw_results = Raw Results
ui-replay-debug-mapped_results = Mapped Results
ui-replay-debug-player_metadata = Player Metadata
ui-replay-debug-search = Search
ui-replay-debug-previous_match = Previous match (Shift+Enter)
ui-replay-debug-next_match = Next match (Enter)

## ui.replay.renderer
ui-replay-renderer-export_video = Render to Video
ui-replay-renderer-export_clipboard = Render to Clipboard
ui-replay-renderer-export_saved = Video saved to { $path }
ui-replay-renderer-export_copied = Video copied to the clipboard
ui-replay-renderer-export_not_written = The video could not be rendered: { $reason }
ui-replay-renderer-pop_out = Open in its own window
ui-replay-renderer-export_path_unusable = That location cannot be written to
ui-replay-renderer-batch_started = Rendering { $count ->
    [one] { $count } replay
   *[other] { $count } replays
} to { $dir }
ui-replay-renderer-batch_finished = Rendered { $written ->
    [one] { $written } replay
   *[other] { $written } replays
}; { $failed } could not be rendered
ui-replay-renderer-batch_all_written = Rendered { $written ->
    [one] { $written } replay
   *[other] { $written } replays
}
ui-replay-renderer-batch_copied = { $written ->
    [one] Rendered { $written } replay and put it on the clipboard
   *[other] Rendered { $written } replays and put them on the clipboard
}
ui-replay-renderer-batch_not_copied = Rendered into { $dir }, but the clipboard refused them: { $reason }
ui-replay-renderer-batch_progress = Rendering { $done } of { $total }: { $replay }
ui-replay-renderer-baking = Reading the replay and walking the battle...
ui-replay-renderer-failed = This replay could not be played back
ui-replay-renderer-play = Play
ui-replay-renderer-pause = Pause

## ui.replay.controls
ui-replay-controls-window_title = Replay Controls
ui-replay-controls-unavailable = Controls not available (commands.scheme.xml not found in game files).

# -- Collab / Session --

## ui.collab
ui-collab-session = Session
ui-collab-starting = Starting session...
ui-collab-session_active = Session Active
ui-collab-connected_to_session = Connected to Session
ui-collab-session_token = Session Token:
ui-collab-toggle_visibility = Toggle token visibility
ui-collab-copy_token = Copy token
ui-collab-token_copied = Token copied to clipboard
ui-collab-copy_web_link = Copy Web Link
ui-collab-web_link_copied = Web link copied to clipboard
ui-collab-copy_localhost_link = Copy Localhost Link
ui-collab-localhost_copied = Localhost link copied to clipboard
ui-collab-connected_count = { $count } connected
ui-collab-promote_cohost = Promote to co-host
ui-collab-permissions = Permissions
ui-collab-lock_annotations = Lock Annotations
ui-collab-lock_settings = Lock Settings
ui-collab-reset_overrides = Reset Client Overrides
ui-collab-reset_overrides_tooltip = Reset all client display setting changes
ui-collab-leave = Leave
ui-collab-role_host = Host
ui-collab-role_cohost = Co-host
ui-collab-you = (you)
ui-collab-display_name = Display name:
ui-collab-display_name_hint = Your name...
ui-collab-enter_display_name = Enter a display name first
ui-collab-host_session = Host a Session
ui-collab-start_session = Start Session
ui-collab-join_session = Join a Session
ui-collab-paste_and_join = Paste token & join
ui-collab-clipboard_empty = Clipboard is empty
ui-collab-invalid_token = Invalid token: { $error }
ui-collab-no_runtime = The session could not start: no runtime
ui-collab-shared_windows = Shared Windows
ui-collab-watching = Watching
ui-collab-waiting_for_frames = Waiting for the battle to arrive
ui-collab-open = Open
ui-collab-open_for_everyone = Open for everyone
ui-collab-tactics_board = Tactics Board

# -- Battle result text --

## battle
battle-victory = VICTORY
battle-defeat = DEFEAT
battle-draw = DRAW
battle-battle_starts_in = BATTLE STARTS IN

# -- Finish type descriptions --

## finish_type
finish_type-unknown = Battle ended
finish_type-extermination = All enemy ships destroyed
finish_type-base_captured = Base captured
finish_type-timeout = Time expired
finish_type-failure = Mission failed
finish_type-technical = Technical finish
finish_type-score = Score limit reached
finish_type-score_on_timeout = Score lead at time limit
finish_type-pve_success = Mission accomplished
finish_type-pve_failed = Mission failed
finish_type-score_zero = Score depleted
finish_type-score_excess = Score limit exceeded

# -- Stats Tab --

## ui.stats
ui-stats-overview = Overview
ui-stats-sample_notice = Sample data, not recorded games
ui-stats-div_all = All
ui-stats-div_solo = Solo
ui-stats-div_div = Division
ui-stats-filters = Filters
ui-stats-own_filters = Filter this chart on its own
ui-stats-own_filters_tooltip = Narrow this chart without changing what the other panes show
ui-stats-mode_label = Mode:
ui-stats-clear = Clear
ui-stats-pr = PR:
ui-stats-mc = MC:
ui-stats-best_frags = Best: { $ship } ({ $count ->
    [one] { $count } kill
   *[other] { $count } kills
})
ui-stats-max_damage = Max DMG: { $ship } ({ $damage })
ui-stats-copy_markdown = Copy as Markdown
ui-stats-copy_image = Copy as Image
ui-stats-copy_image_done = Chart copied to clipboard
ui-stats-copy_image_failed = The chart could not be copied
ui-stats-copy_csv = Copy as CSV
ui-stats-remove_games_hint = Remove all { $ship } games (Ctrl+Click to skip confirmation)
ui-stats-settings = Settings
ui-stats-options = Options
ui-stats-ships = Ships
ui-stats-filter_ships = Filter chart ships
ui-stats-all_ships = All
ui-stats-no_ships = None
ui-stats-search_ships = Search ships...
ui-stats-no_ship_matches = No ships match this search
ui-stats-search_statistics = Search statistics
ui-stats-no_statistic_matches = No statistics match this search
ui-stats-ship_selection_count = { $selected } of { $total } selected
ui-stats-all_ships_tooltip = Select all matching ships
ui-stats-no_ships_tooltip = Clear matching ship selections
ui-stats-limit_to_recent = Limit to recent games
ui-stats-limit_to_recent_tooltip = The limit applies per ship in Ships and Charts, and across all games in Overview.
ui-stats-division_label = Division:
ui-stats-add_chart = Add chart
ui-stats-clear_tooltip = Forget every recorded game
ui-stats-clear_failed = Not cleared: { $reason }
ui-stats-clear_ship_confirm = Press again to forget this ship's games
ui-stats-copy_table = Copy this table
ui-stats-copied = Copied to clipboard
ui-stats-no_games = No games recorded for the current filters
ui-stats-nothing_to_plot = Nothing to plot for the current filters
ui-stats-column_ship = Ship
ui-stats-column_battles = Battles
ui-stats-column_win_rate = Win rate
ui-stats-column_avg_damage = Avg damage
ui-stats-column_avg_frags = Avg frags
ui-stats-frags = { $count ->
    [one] { $count } frag
   *[other] { $count } frags
}
ui-stats-statistic = Statistic
ui-stats-open_directory_stats = Directory Stats
ui-stats-open_directory_stats_tooltip = Open stats for every replay in this directory and its subdirectories
ui-stats-directory_stats_unavailable = Game data is still loading
ui-stats-directory_reading_task = Reading replays for directory stats...
ui-stats-directory_walking = Finding replays...
ui-stats-directory_scanning = Checking { $done } of { $total ->
    [one] { $total } replay...
   *[other] { $total } replays...
}
ui-stats-directory_reading = Reading { $done } of { $total ->
    [one] { $total } replay...
   *[other] { $total } replays...
}
ui-stats-directory_reload = Reload
ui-stats-directory_reload_tooltip = Read this directory's replays again
ui-stats-directory_skipped = { $count ->
    [one] { $count } replay skipped
   *[other] { $count } replays skipped
}
ui-stats-directory_skipped_detail =
    Unreadable: { $unreadable }
    No game data for their build: { $missing_build }
    No battle results: { $no_results }
ui-stats-chart = Chart
ui-stats-combine_ships = Combine ships
ui-stats-running_average = Running average
ui-stats-value_labels = Value labels
ui-stats-reset_view = Reset view
ui-stats-keep_current_layout = Keep Current Layout
ui-stats-session_title = Session stats
ui-stats-session_manual = Manual
ui-stats-session_since_opened = Since the app opened
ui-stats-session_manual_tooltip = The session holds the battles you play and the replays you set or add from the replay listing. It is kept between runs.
ui-stats-session_since_opened_tooltip = The session holds every battle that started at or after { $time }, when the app opened. Your manual session is kept and comes back when you switch to Manual.
ui-stats-session_battles = { $count ->
    [one] { $count } battle
   *[other] { $count } battles
}
ui-stats-session_chip_manual = Session: { $battles }
ui-stats-session_chip_since = Session since { $time }: { $battles }
ui-stats-session_empty_title = No battles in this session yet
ui-stats-session_empty_manual = Battles you play join the session as they finish. You can also set or add replays from the replay listing's menu.
ui-stats-session_empty_since = Battles that start after { $time } join the session as they finish.
ui-stats-session_set_add_unavailable = The session holds battles since { $time }. Switch it to Manual to set or add replays.
ui-stats-clear_unavailable_since = A session since the app opened is not saved. Switch to Manual to clear the saved session.
ui-stats-charts_save_failed = Chart settings were not saved: { $reason }
ui-stats-layout_save_failed = Chart layout was not saved: { $reason }
ui-stats-axis_game = Game
ui-stats-chart_hover_game = Game { $number }, { $time }
ui-stats-series_average = { $stat } (average)
ui-stats-series_running = { $stat } (running)
ui-stats-series_more = +{ $count } more
ui-stats-mode_random = Random
ui-stats-mode_ranked = Ranked
ui-stats-mode_other = Other
ui-stats-win_rate_rated_only = Win rate counts only Random and Ranked battles
ui-stats-win_rate_breakdown_line = { $mode }: { $rate } (battles: { $battles })
ui-stats-matches = Matches
ui-stats-column_time = Time
ui-stats-column_mode = Mode
ui-stats-column_result = Result
ui-stats-keep_game = Count this game in the stats
ui-stats-leave_out_game = Leave this game out of the stats
ui-stats-leave_out_ship = Leave out every { $ship } game
ui-stats-leave_out_ship_hint = Leave { $ship } out of the stats
ui-stats-keep_all_games = Count all games
ui-stats-keep_all_games_tooltip = Count every game you left out by hand again
ui-stats-left_out_only = Left out only
ui-stats-match_count = { $shown } of { $total ->
    [one] { $total } game
   *[other] { $total } games
}, { $left_out } left out
ui-stats-no_matches = No games match the current filters
ui-stats-search_matches = Search by ship...
ui-stats-ship_filter_tooltip = Choose which ships count in the stats
ui-stats-keep_ships_tooltip = Count every matching ship
ui-stats-leave_out_ships_tooltip = Leave every matching ship out of the stats
ui-stats-dates = Dates
ui-stats-all_dates = All dates
ui-stats-clear_dates = Show all dates
ui-stats-games_left_out = { $count ->
    [one] { $count } game left out
   *[other] { $count } games left out
}
ui-stats-reset_filters = Reset filters
ui-stats-reset_filters_tooltip = Show every game: no limit, date span, division, mode, or left-out ships and games

## ui.stats.table
ui-stats-table-min = Min
ui-stats-table-max = Max
ui-stats-table-total = Total
ui-stats-table-average = Average

# -- Player Tracker --

## ui.player_tracker
ui-player_tracker-clear_stats = Clear Stats
ui-player_tracker-populate_from_replays = Populate Data From Replays
ui-player_tracker-find_matches = Find matches
ui-player_tracker-open_wows_numbers = Open on wows-numbers
ui-player_tracker-open_shipbuilds = Open builds on shipbuilds
ui-player_tracker-subtab_players = Players
ui-player_tracker-filter_hint = Filter by player or clan...
ui-player_tracker-time_period = Time period
ui-player_tracker-allies = Allies
ui-player_tracker-enemies = Enemies
ui-player_tracker-live_no_directory = Set the World of Warships directory to watch for battles
ui-player_tracker-live_checking = Checking for a battle in progress...
ui-player_tracker-live_none = No battle in progress
ui-player_tracker-live_loading_build = Loading this build's ship data; names and classes fill in when it lands.
ui-player_tracker-waiting_for_index = Waiting for the replay index
ui-player_tracker-loading_clans = Loading clans...
ui-player_tracker-loading_players = Loading players...
ui-player_tracker-index_failed = Could not read the index: { $reason }
ui-player_tracker-no_players = No players indexed for this period
ui-player_tracker-note_not_saved = Not saved: { $reason }
ui-player_tracker-subtab_current_match = Current Match
ui-player_tracker-encounters_hover = Seen { $total ->
    [one] { $total } time
   *[other] { $total } times
} total, { $range } in the selected time range. Last seen { $last }.
ui-player_tracker-never_encountered = Not seen before
ui-player_tracker-copy_wg_id = Click to copy the WG ID
ui-player_tracker-copied_wg_id = Copied WG ID { $id }
ui-player_tracker-aliases_hover = Also known as: { $names }
ui-player_tracker-notes_hint = Notes on this player
ui-player_tracker-arena_count = { $count ->
    [one] { $count } match recorded
   *[other] { $count } matches recorded
}
ui-player_tracker-last_encountered_exact = Last encountered { $timestamp }
ui-player_tracker-subtab_clans = Clans
ui-player_tracker-clan_no_data = No clan encounters recorded yet.
ui-player_tracker-find_clan_matches = Find matches mentioning this tag. The index has no clan-only field, so player names containing the tag match too.
ui-player_tracker-clan_sightings_hover = { $range } of these fall in the selected time range.
ui-player_tracker-clan_member_matches = seen in { $count ->
    [one] { $count } match
   *[other] { $count } matches
}
ui-player_tracker-show_division_mates = Show division mates
ui-player_tracker-show_division_mates_hover = Count the matches you played in a division. Off by default, since you met those players there because you invited them. Other matches against the same players are always counted.
ui-player_tracker-seen_in_twitch_chat = Seen in Twitch chat
ui-player_tracker-seen_in_twitch_chat_hover = Show only the players whose name matched a Twitch chat login during a match with you.
ui-player_tracker-twitch_seen_hover = Seen in Twitch chat as { $logins } in { $count ->
    [one] { $count } match
   *[other] { $count } matches
} with you
ui-player_tracker-win_rate_overall = Overall
ui-player_tracker-win_rate_ship = Ship
ui-player_tracker-view_compact = Compact
ui-player_tracker-view_detailed = Detailed
ui-player_tracker-ship_stats = Ship Stats
ui-player_tracker-stats_resolving = Reading the match roster...
ui-player_tracker-stats_fetching = Fetching player stats...
ui-player_tracker-stats_unavailable = Stats unavailable: { $reason }
ui-player_tracker-stats_hidden = This player's profile is private
ui-player_tracker-stats_no_data = No stats for this player
ui-player_tracker-pr_overall_hover = Account PR across every ship
ui-player_tracker-pr_ship_hover = PR in this ship
ui-player_tracker-win_rate_hover_line = { $label }: { $rate } over { $count ->
    [one] { $battles } battle
   *[other] { $battles } battles
}
ui-player_tracker-win_rate_hover_line_damage = { $label }: { $rate } over { $count ->
    [one] { $battles } battle
   *[other] { $battles } battles
}, { $damage } average damage
ui-player_tracker-team_average_win_rate = Avg WR: { $rate }
ui-player_tracker-team_players = { $count ->
    [one] { $count } player
   *[other] { $count } players
}
ui-player_tracker-debug_replay_picker = Debug: treat a replay as the current match
ui-player_tracker-debug_replay_picker_hover = Loads a finished replay's roster into this tab so the stats path can be exercised outside a battle.
ui-player_tracker-debug_replay_none = Pick a replay

## ui.player_tracker.column
ui-player_tracker-column-win_rate = WR
ui-player_tracker-column-personal_rating = PR
ui-player_tracker-column-battles = Battles
ui-player_tracker-column-avg_damage = Dmg
ui-player_tracker-column-encounters = Seen
ui-player_tracker-column-clan = Clan
ui-player_tracker-column-player_name = Player Name
ui-player_tracker-column-total_encounters = Total Encounters
ui-player_tracker-column-encounters_in_range = Encounters in Time Range
ui-player_tracker-column-last_encountered = Last Encountered
ui-player_tracker-column-members = Members
ui-player_tracker-column-matches = Matches
ui-player_tracker-column-sightings = Sightings

## ui.player_tracker.period
ui-player_tracker-period-past_hour = Past Hour
ui-player_tracker-period-past_six_hours = Past 6 Hours
ui-player_tracker-period-past_day = Past 24 Hours
ui-player_tracker-period-past_week = Past Week
ui-player_tracker-period-past_month = Past Month
ui-player_tracker-period-all_time = All Time

## ui.twitch
ui-twitch-possible_name = Possible stream name: { $name }
ui-twitch-seen_minutes = Seen: { $minutes } minutes after match start
ui-twitch-click_to_copy = Click to copy
ui-twitch-copied = Copied { $name }

# -- Search --

## ui.search
ui-search-edit_value = Edit value
ui-search-add_filter = Add filter
ui-search-pill_select = Select
ui-search-pill_deselect = Deselect
ui-search-pill_group_all = Group with and
ui-search-pill_group_any = Group with or
ui-search-pill_ungroup = Ungroup
ui-search-pill_negate = Negate
ui-search-pill_flip_connector = Flip and/or
ui-search-pill_delete = Delete

ui-search-match_count_truncated = { $count }+ matches
ui-search-outcome_win = Win
ui-search-outcome_loss = Loss
ui-search-outcome_draw = Draw
ui-search-outcome_unknown = Unknown
ui-search-bool_true = true
ui-search-bool_false = false
ui-search-match_count = { $count ->
    [one] { $count } match
   *[other] { $count } matches
}
ui-search-row_actions_hint = Replay actions (or right-click the row)
ui-search-looking_up_values = Looking up values...
ui-search-type_a_query = Type a query and press Enter
ui-search-parse_failed = That query did not parse: { $reason }
ui-search-searching = Searching...
ui-search-failed = The search failed: { $reason }
ui-search-no_matches = No matches
ui-search-game_mode_gap_hint = { $count ->
    [one] { $count } indexed match has no game mode recorded. Re-index All Replays in Settings to fill it in.
   *[other] { $count } indexed matches have no game mode recorded. Re-index All Replays in Settings to fill them in.
}
ui-search-open = Open
ui-search-open_missing = Replay file no longer exists
ui-search-free_text = contains "{ $text }"
ui-search-quant_any = any
ui-search-quant_none = no
ui-search-quant_ge = { $n } or more
ui-search-quant_eq = exactly { $n }
ui-search-quant_le = at most { $n }
ui-search-quant_gt = more than { $n }
ui-search-quant_lt = fewer than { $n }
ui-search-quant_ne = not { $n }
ui-search-scope_self = Self
ui-search-scope_ally = Ally
ui-search-scope_enemy = Enemy
ui-search-scope_div = Div

## ui.search.suggest
ui-search-suggest-preset_divmate_test_ship = Someone in my division played a test ship
ui-search-suggest-preset_no_enemy_cv = No enemy carrier
ui-search-suggest-preset_all_enemies_died = Every enemy died
ui-search-suggest-preset_high_damage_enemies = 3 or more enemies over 100k
ui-search-suggest-preset_stream_sniper = Contains a stream sniper
ui-search-suggest-preset_i_survived = I survived
ui-search-suggest-preset_i_disconnected = I disconnected
ui-search-suggest-my_damage = My damage
ui-search-suggest-enemy_ship = Enemy ship
ui-search-suggest-allied_ship = Allied ship
ui-search-suggest-player_in_match = Player was in the match
ui-search-suggest-someone_in_division = Someone in my division
ui-search-suggest-scope_self = My
ui-search-suggest-scope_ally = Ally
ui-search-suggest-scope_enemy = Enemy
ui-search-suggest-scope_division = Division
ui-search-suggest-scope_anyone = Any player

## ui.search.field
ui-search-field-outcome = Result
ui-search-field-map = Map
ui-search-field-class = Class
ui-search-field-tier = Tier
ui-search-field-date = Date
ui-search-field-player_present = Player
ui-search-field-group = Replay group
ui-search-field-stat_damage = Damage
ui-search-field-stat_kills = Kills
ui-search-field-stat_spotting = Spotting
ui-search-field-stat_potential = Potential damage
ui-search-field-stat_received = Damage received
ui-search-field-stat_pr = PR
ui-search-field-rating = Rating
ui-search-field-stat_survived = Survived
ui-search-field-stat_disconnected = Disconnected
ui-search-field-game_type = Game type
ui-search-field-game_mode = Game mode
ui-search-field-match_group = Match group
ui-search-field-build = Build
ui-search-field-results_available = Results available
ui-search-field-relation = Relation
ui-search-field-division = Division
ui-search-field-name = Name
ui-search-field-clan = Clan
ui-search-field-realm = Realm
ui-search-field-ship = Ship
ui-search-field-ship_index = Ship index
ui-search-field-nation = Nation
ui-search-field-test_ship = Test ship
ui-search-field-stream_sniper = Stream sniper
ui-search-field-sniper_login = Sniper Twitch login

## ui.search.op
ui-search-op-contains = contains
ui-search-op-equals = equals
ui-search-op-not_equals = does not equal
ui-search-op-eq = =
ui-search-op-ne = !=
ui-search-op-gt = >
ui-search-op-ge = >=
ui-search-op-lt = <
ui-search-op-le = <=
ui-search-op-is = is
ui-search-op-is_not = is not
ui-search-op-is_set = is set
ui-search-op-is_not_set = is not set

## ui.search.value
ui-search-value-relation_self = Self
ui-search-value-relation_ally = Ally
ui-search-value-relation_enemy = Enemy
ui-search-value-division_mine = Mine
ui-search-value-division_any = Any
ui-search-value-division_none = None
ui-search-value-class_air_carrier = Aircraft Carrier
ui-search-value-class_battleship = Battleship
ui-search-value-class_cruiser = Cruiser
ui-search-value-class_destroyer = Destroyer
ui-search-value-class_submarine = Submarine
ui-search-value-class_auxiliary = Auxiliary
# The personal-rating bands, named as wows-numbers.com names them; the
# boundaries they stand for live in wows-replay-insights.
ui-search-value-rating_bad = Bad
ui-search-value-rating_below_average = Below Average
ui-search-value-rating_average = Average
ui-search-value-rating_good = Good
ui-search-value-rating_very_good = Very Good
ui-search-value-rating_great = Great
ui-search-value-rating_unicum = Unicum
ui-search-value-rating_super_unicum = Super Unicum
# Most of these are the mode's own name from the GAME_MODE table, typeset for
# reading (underscores to spaces, words capitalised) rather than an invented
# descriptive phrase: there is no IDS_ source anywhere in the deobfuscated
# scripts to confirm a player-facing name against, and an invented phrase
# would assert a mechanic nobody verified. Typesetting the table's own name
# does not invent anything -- MEGABASE and Megabase are the same identifier.
# Standard, Domination, Epicenter, Arms Race, Convoy, Tutorial, and Test are
# the exceptions -- names confirmed unambiguous and genuinely WoWs-facing.
# PVE stays an acronym and Anti-Standard keeps its hyphen; both read better
# that way than a mechanical space would. Each value is independent, so a
# confirmed name can replace one without touching any other line or any code.
ui-search-value-game_mode_test = Test
ui-search-value-game_mode_standard = Standard
ui-search-value-game_mode_singlebase = Singlebase
ui-search-value-game_mode_domination = Domination
ui-search-value-game_mode_tutorial = Tutorial
ui-search-value-game_mode_megabase = Megabase
ui-search-value-game_mode_forts = Forts
ui-search-value-game_mode_standard_domination = Standard Domination
ui-search-value-game_mode_epicenter = Epicenter
ui-search-value-game_mode_assault_defense = Assault Defense
ui-search-value-game_mode_pve = PVE
ui-search-value-game_mode_arms_race = Arms Race
ui-search-value-game_mode_epicenter_ring = Epicenter Ring
ui-search-value-game_mode_anti_standard = Anti-Standard
ui-search-value-game_mode_attack_defense = Attack Defense
ui-search-value-game_mode_torpedo_beat = Torpedo Beat
ui-search-value-game_mode_team_battle_royale = Team Battle Royale
ui-search-value-game_mode_escape_to_portal = Escape To Portal
ui-search-value-game_mode_domination_asymm = Domination Asymm
ui-search-value-game_mode_key_battle = Key Battle
ui-search-value-game_mode_portal_2021 = Portal 2021
ui-search-value-game_mode_team_battle_royale_2021 = Team Battle Royale 2021
ui-search-value-game_mode_convoy_event = Convoy Event
ui-search-value-game_mode_convoy_airship = Convoy Airship
ui-search-value-game_mode_two_teams_battle_royale = Two Teams Battle Royale
ui-search-value-game_mode_pinata_event = Pinata Event
ui-search-value-game_mode_respawns = Respawns
ui-search-value-game_mode_respawns_sectors = Respawns Sectors

## ui.search.column
ui-search-column-date = Date
ui-search-column-map = Map
ui-search-column-mode = Mode
ui-search-column-ship = Your ship
ui-search-column-result = Result
ui-search-column-damage = Damage
ui-search-column-kills = Kills
ui-search-column-pr = PR
ui-search-column-mc = MC

# -- Mod Manager --

# -- Replay Renderer --

## ui.renderer
ui-renderer-preparing_export = Preparing export...
ui-renderer-encoding = Encoding
ui-renderer-muxing = Muxing

## ui.renderer.advantage
ui-renderer-advantage-breakdown = Advantage Breakdown
ui-renderer-advantage-team_eliminated = A team has been eliminated
ui-renderer-advantage-score_projection = Score Projection
ui-renderer-advantage-fleet_power = Fleet Power
ui-renderer-advantage-strategic_threat = Strategic Threat
ui-renderer-advantage-total = Total
ui-renderer-advantage-hp_incomplete = HP/ship data incomplete. Only score factors are shown

## ui.renderer.context
ui-renderer-context-show_trail = Show Trail
ui-renderer-context-disable_other_trails = Disable All Other Trails
ui-renderer-context-ranges = Ranges
ui-renderer-context-detection = Detection
ui-renderer-context-main_battery = Main Battery
ui-renderer-context-secondary = Secondary
ui-renderer-context-torpedo = Torpedo
ui-renderer-context-radar = Radar
ui-renderer-context-hydro = Hydro
ui-renderer-context-enable_all = Enable All
ui-renderer-context-disable_all = Disable All
ui-renderer-context-disable_other_ranges = Disable All Other Ranges
ui-renderer-context-enable_all_ranges = Enable All Ships' Ranges
ui-renderer-context-realtime_armor = Show Realtime Armor

## ui.renderer.controls
ui-renderer-controls-jump_to_start = Jump to start
ui-renderer-controls-previous_event = Previous event (Shift+Left)
ui-renderer-controls-back_10s = Back 10s (Left)
ui-renderer-controls-forward_10s = Forward 10s (Right)
ui-renderer-controls-next_event = Next event (Shift+Right)
ui-renderer-controls-jump_to_end = Jump to end
ui-renderer-controls-speed = Playback speed

## ui.renderer.annotations
ui-renderer-annotations-title = Drawing Tools
ui-renderer-annotations-undo = Undo
ui-renderer-annotations-redo = Redo
ui-renderer-annotations-ship_hint = Find a ship
ui-renderer-annotations-no_ship = No ship chosen
ui-renderer-annotations-nib = Stroke width
ui-renderer-annotations-thinner = Thinner stroke
ui-renderer-annotations-thicker = Thicker stroke
ui-renderer-annotations-shortcuts_title = Keyboard Shortcuts
ui-renderer-annotations-shortcuts_tools = Tools
ui-renderer-annotations-shortcuts_actions = Actions
ui-renderer-annotations-tool_select = Select
ui-renderer-annotations-tool_arrow = Arrow
ui-renderer-annotations-tool_freehand = Freehand
ui-renderer-annotations-tool_line = Line
ui-renderer-annotations-tool_circle = Circle
ui-renderer-annotations-tool_rectangle = Rectangle
ui-renderer-annotations-tool_triangle = Triangle
ui-renderer-annotations-tool_measurement = Measurement
ui-renderer-annotations-tool_eraser = Eraser
ui-renderer-annotations-action_undo = Undo
ui-renderer-annotations-action_multi_select = Multi-select
ui-renderer-annotations-action_stroke_width = Stroke width
ui-renderer-annotations-action_delete = Delete
ui-renderer-annotations-action_cancel = Cancel / Deselect
ui-renderer-annotations-toggle_fill = Click again to toggle fill
ui-renderer-annotations-clear_all = Clear All
ui-renderer-annotations-edit = Edit drawing
ui-renderer-annotations-size = Size
ui-renderer-annotations-color = Color
ui-renderer-annotations-custom_ink = Custom ink
ui-renderer-annotations-filled = Filled
ui-renderer-annotations-locked = Annotations locked by host

## ui.renderer.settings
ui-renderer-settings-title = Display Settings
ui-renderer-settings-hp_bars = HP Bars
ui-renderer-settings-armament = Armament
ui-renderer-settings-dead_ships = Dead Ships
ui-renderer-settings-dead_ship_names = Dead Ship Names
ui-renderer-settings-player_names = Player Names
ui-renderer-settings-ship_ranges = Ship Ranges
ui-renderer-settings-ship_names = Ship Names
ui-renderer-settings-camera_direction = Camera Direction
ui-renderer-settings-heat_trail = Heat Trail
ui-renderer-settings-speed_trails = Speed Trails
ui-renderer-settings-dead_ship_trails = Dead Ship Trails
ui-renderer-settings-buildings = Buildings
ui-renderer-settings-capture_points = Capture Points
ui-renderer-settings-consumables = Consumables
ui-renderer-settings-planes = Planes
ui-renderer-settings-smoke = Smoke
ui-renderer-settings-torpedoes = Torpedoes
ui-renderer-settings-tracers = Tracers
ui-renderer-settings-battle_result = Battle Result
ui-renderer-settings-buff_counters = Buff Counters
ui-renderer-settings-chat_label = Chat
ui-renderer-settings-kill_feed = Kill Feed
ui-renderer-settings-score_label = Score
ui-renderer-settings-score_timers = Score Timers
ui-renderer-settings-team_advantage = Team Advantage
ui-renderer-settings-team_rosters = Team Rosters
ui-renderer-settings-stats_panel = Stats Panel
ui-renderer-settings-timer = Timer
ui-renderer-settings-export_settings = Export Settings
ui-renderer-settings-prefer_cpu = Prefer CPU Encoder
ui-renderer-settings-prefer_cpu_tooltip = Use software (CPU) encoder instead of GPU for video export
ui-renderer-settings-include_pre_battle = Include Pre-Battle Phase
ui-renderer-settings-include_pre_battle_tooltip = Include the spawn and countdown before battle start in exported video
ui-renderer-settings-export_timing = Video timing
ui-renderer-settings-export_timing_length = Fit to length
ui-renderer-settings-export_timing_speed = Speed
ui-renderer-settings-export_timing_result_speed = Plays at about { $speed }x
ui-renderer-settings-export_timing_result_length = Video runs { $length }
ui-renderer-settings-codec = Codec
ui-renderer-settings-codec_auto = Auto
ui-renderer-settings-save_defaults = Save Defaults
ui-renderer-settings-save_defaults_done = Saved as what a renderer opens with
ui-renderer-settings-save_defaults_failed = The defaults could not be saved
ui-renderer-settings-event_timeline = Event Timeline

# -- File Unpacker --

## ui.unpacker
ui-unpacker-assets_bin = Assets.bin
ui-unpacker-loading_assets = Loading assets.bin...
ui-unpacker-load_failed = Failed to load: { $error }
ui-unpacker-no_game_data = No game data loaded. Set your World of Warships directory in Settings.
ui-unpacker-filter_files = Filter files...
ui-unpacker-search_in_files = Search in Files
ui-unpacker-search_hint = Search in files...
ui-unpacker-path_filter_hint = Path filter (e.g. *.xml)
ui-unpacker-queue_all_files = Queue All Files
ui-unpacker-view_as_json = View as JSON
ui-unpacker-extract_as_json = Extract as JSON
ui-unpacker-view_contents = View Contents
ui-unpacker-version = Version:
ui-unpacker-decode_prototypes = Decode prototypes as JSON
ui-unpacker-decode_prototypes_tooltip = Decode supported Assets.bin prototypes (.visual, .model, .mfm) to JSON on extraction
ui-unpacker-extraction_queue = Extraction Queue
ui-unpacker-collapse_folders = Collapse Folder Tree
ui-unpacker-expand_folders = Expand Folder Tree
ui-unpacker-clear_all = Clear All
ui-unpacker-browse = Browse
ui-unpacker-output_dir_hint = Output directory...
ui-unpacker-remove_from_queue = Remove from queue
ui-unpacker-queue_folder = Queue entire folder
ui-unpacker-queue_file = Queue this file
ui-unpacker-extraction_output = Extract to
ui-unpacker-queue_empty = Tick a file or folder in the listing to queue it
ui-unpacker-packages = Packages
ui-unpacker-item_count = { $count ->
    [one] { $count } item
   *[other] { $count } items
}
ui-unpacker-search_title = Search: { $query }
ui-unpacker-search_in = in { $source }
ui-unpacker-image_decode_failed = Could not decode this image: { $reason }
ui-unpacker-find = Find
ui-unpacker-find_previous = Previous match
ui-unpacker-find_next = Next match
ui-unpacker-find_close = Close find
ui-unpacker-find_count = { $current } of { $total }
ui-unpacker-find_none = No matches
ui-unpacker-find_match_case = Match case
ui-unpacker-queue = Queue
ui-unpacker-reveal_in_tree = Show in Package Tree
ui-unpacker-hit_offset = at { $offset }
ui-unpacker-queued_count = { $count } queued
ui-unpacker-counting_files = Counting files...
ui-unpacker-extracting = Extracting { $done } of { $total }
ui-unpacker-extract_failed = Extraction failed: { $reason }
ui-unpacker-extracted = Extracted { $count ->
    [one] { $count } file
   *[other] { $count } files
}
ui-unpacker-cancelled = Cancelled after { $count ->
    [one] { $count } file
   *[other] { $count } files
}
ui-unpacker-extract = { $count ->
    [0] Extract
    [one] Extract { $count } Item
   *[other] Extract { $count } Items
}
ui-unpacker-choose_directory = Choose a directory to extract to first
ui-unpacker-dump_parameters = Export GameParams
ui-unpacker-dump_parameters_tooltip = Export this build's GameParams.data
ui-unpacker-load_a_build = Load a build first
ui-unpacker-parameters_written = Parameters written
ui-unpacker-save_parameters_title = Save game parameters
ui-unpacker-writing = Writing...
ui-unpacker-prototype_written = Prototype written
ui-unpacker-whole_tree = Whole tree
ui-unpacker-base_only = Base parameters only

## ui.unpacker.column
ui-unpacker-column-name = Name
ui-unpacker-column-size = Size
ui-unpacker-column-type_col = Type

# -- Armor Viewer --

## ui.armor
ui-armor-load_settings = Load game data in Settings to use the Armor Viewer.
ui-armor-load_failed = Failed to load ship data: { $error }
ui-armor-empty = Empty
ui-armor-common_settings = Common Settings
ui-armor-tier_filter = Tiers { $lowest } to { $highest }
ui-armor-fortifications = Fortifications
ui-armor-show_hidden_ships = Show hidden ships
ui-armor-show_hidden_ships_tooltip = Also list the ships the list leaves out, such as clan rentals
ui-armor-mirror_cameras = Mirror cameras
ui-armor-sync_options = Sync options
ui-armor-show_armor_by_default = Show armor by default
ui-armor-sync_tooltip = Keep armor/hull visibility and display overlays (ship center, waterline, camera rings) in sync across all panes
ui-armor-compare_split = Compare in new split
ui-armor-open_in_new_tab = Open in new tab
ui-armor-export_model = Export Ship Model
ui-armor-export_disclaimer = These 3D models and textures are IP of Wargaming and any usage of these models should be in compliance with your local laws.
ui-armor-export_button = Export
ui-armor-hull_toggle = Hull
ui-armor-splash_tooltip = Toggle splash box visibility
ui-armor-show_hidden = Show Hidden
ui-armor-show_hidden_tooltip = Toggle (possibly) hidden panel visibility
ui-armor-gaps = Gaps
ui-armor-gaps_tooltip = Highlight boundary edges where armor panels don't connect (potential gaps)
ui-armor-display = Display
ui-armor-marker_opacity = Marker Opacity
ui-armor-export_tooltip = Export ship model to a GLB file
ui-armor-pen_check = Pen Check
ui-armor-trajectory = Trajectory
ui-armor-trajectory_range = Range
ui-armor-trajectory_clear = Clear All
ui-armor-arc_heading = Arc { $number }
ui-armor-arc_summary = { $hits ->
    [one] { $hits } hit
   *[other] { $hits } hits
}, { $armor }mm at { $range } km
ui-armor-arc_delete = Delete this arc
ui-armor-arc_isolate_plates = Isolate Plates
ui-armor-arc_isolate_plates_tooltip = Show only the armor plates this arc crossed
ui-armor-arc_isolate_zones = Isolate Zones
ui-armor-arc_isolate_zones_tooltip = Show every part of the armor zones this arc entered
ui-armor-arc_simulation_caveat = This simulation is based on reverse engineered data and may not match how the game computes ballistics.
ui-armor-arc_no_shell = No shell cast; the plates crossed are shown without one
ui-armor-arc_detonated_in = Detonates inside { $zone }
ui-armor-arc_detonated_past_armor = Detonates past all armor
ui-armor-arc_ricochet = Ricochet at { $plate }
ui-armor-arc_shatter = Shatter at { $plate }
ui-armor-arc_stopped = Stopped at { $plate }
ui-armor-arc_overpen = Overpenetration
ui-armor-arc_overpen_unarmed = Overpenetration (fuse never armed)
ui-armor-arc_plate_unknown = an unrecorded plate
ui-armor-angle_good = under 30 deg
ui-armor-angle_mid = 30 to 45 deg
ui-armor-angle_bad = over 45 deg
ui-armor-trajectory_ship = Ship:
ui-armor-trajectory_select_ship = Select ship
ui-armor-trajectory_ship_tooltip = Choose the ship supplying the trajectory shell
ui-armor-trajectory_add_ship = Add firing ship
ui-armor-trajectory_add_ship_tooltip = Open Pen Check to search for and add a ship
ui-armor-trajectory_shell = Shell:
ui-armor-trajectory_select_shell = Select shell
ui-armor-trajectory_shell_tooltip = Choose the shell used for trajectory casts
ui-armor-continue_ricochet = Continue past ricochet
ui-armor-splash_mode = Splash
ui-armor-splash_no_data = No splash data for this ship
ui-armor-splash_no_shells = Add a ship with HE/SAP shells to the comparison list
ui-armor-splash_tooltip_mode = Click armor to visualize HE splash damage volume
ui-armor-select_ship = Select a ship from the list
ui-armor-all_btn = All
ui-armor-none_btn = None
ui-armor-opaque = Opaque
ui-armor-upgrade = Upgrade:
ui-armor-camo_none = Stock (none)
ui-armor-camo_group_universal = Universal camos
ui-armor-camo_group_expendable = Expendable camos
ui-armor-camo_group_other = Other camos
ui-armor-show_armor_thickness = Show Armor Thickness Legend
ui-armor-plate_edges = Plate Edges
ui-armor-waterline = Waterline
ui-armor-camera_rings = Camera rings
ui-armor-realtime_readout = { $health } HP  -  { $hits ->
    [one] { $hits } hit
   *[other] { $hits } hits
}
ui-armor-show_camera_rings = Show camera rings
ui-armor-camera_fov = Camera FOV
ui-armor-camera_height = Camera height
ui-armor-show_camera_zoom_path = Show zoom path
ui-armor-zoom_path_regular_fov = Regular FOV
ui-armor-zoom_path_max_fov = Max FOV
ui-armor-camera_perspective = Camera perspective
ui-armor-perspective_fov = View FOV
ui-armor-perspective_projection = Projection
ui-armor-perspective_proj_game = Game
ui-armor-perspective_proj_center = Through center
ui-armor-ship_center = Ship center
ui-armor-grid = Grid
ui-armor-length_marks = Length marks
ui-armor-length_readout = Length { $length }, beam { $beam }
ui-armor-length_metres = { $value } m
ui-armor-length_centimetres = { $value } cm
ui-armor-opacity = Opacity
ui-armor-opaque_hull = Opaque Hull
ui-armor-zero_mm_plates = 0 mm Plates
ui-armor-armor_opacity = Armor Opacity
ui-armor-lighting = Hull Lighting
ui-armor-lighting_enabled = Enable lighting
ui-armor-lighting_preset_ingame = In-Game
ui-armor-lighting_preset_flat = Flat
ui-armor-lighting_preset_studio = Studio
ui-armor-lighting_flat = Flat (ambient)
ui-armor-lighting_intensity = Light intensity
ui-armor-lighting_azimuth = Light azimuth
ui-armor-lighting_elevation = Light elevation
ui-armor-lighting_rim = Rim
ui-armor-lighting_specular = Specular
ui-armor-lighting_shininess = Shininess
ui-armor-lighting_key_color = Key color
ui-armor-lighting_ambient_color = Ambient color
ui-armor-ballistic_disclaimer = Ballistic results are based on reverse engineering/estimates of how simulation works
ui-armor-roll = Roll
ui-armor-reset_plates = Reset plates
ui-armor-armor_options = Armor
ui-armor-no_ship_loaded = No ship loaded
ui-armor-lod = LOD:
ui-armor-lod_highest = 0 (highest)
ui-armor-camouflage = Camouflage
ui-armor-ships = Ships
ui-armor-compare = Compare
ui-armor-compare_tooltip = Open a new comparison pane, or right-click a ship to compare that one
ui-armor-mirror_cameras_tooltip = Move every pane's camera together
ui-armor-show_armor_by_default_tooltip = Open models with the armor in view rather than the hull
ui-armor-xray_highlight = X-ray highlight when hovering in the view
ui-armor-xray_always = Always
ui-armor-xray_while_held = While holding { $key }
ui-armor-xray_off = Off
ui-armor-stack_panes = Stack panes
ui-armor-stack_panes_tooltip = Lay the comparison panes out one above the other rather than side by side
ui-armor-viewports = Viewports
ui-armor-options_panel = Options panel
ui-armor-options_pane = Pane { $number } of { $count }
ui-armor-splash_boxes = Splash Boxes
ui-armor-cast_here_hint = Hold { $key } and click to cast a shell here
ui-armor-trajectory_next_cast = Next cast
ui-armor-arc_pick_hint = Click a marker to edit its arc.
ui-armor-arc_marker_tooltip = Arc { $number }. Click to edit it. Right-click to remove it.
ui-armor-zone_toggle_tooltip = Shows or hides every plate in this zone. Hold { $key } to show this zone alone.
ui-armor-mode_solo_zone = solo zone
ui-armor-mode_cast_trajectory = cast trajectory
ui-armor-loading_catalog = Loading ship catalog...
ui-armor-legend_collapse = Collapse
ui-armor-legend_hide = Hide legend
ui-armor-armor_thickness_title = Armor Thickness
ui-armor-initializing = Initializing 3D viewport...
ui-armor-init_failed = Armor viewport failed to initialize: { $reason }
ui-armor-loading_named_ship = Loading { $ship }...
ui-armor-options = Options
ui-armor-filter_options = Filter options...
ui-armor-filter_zones = Filter zones...
ui-armor-filter_parts = Filter parts...
ui-armor-tree_matches = Matches: { $count }
ui-armor-tree_no_matches = No matches
ui-armor-no_matching_options = No options match the filter

# -- Armor Viewer: casting --

## ui.armor.cast
ui-armor-cast-prompt = { $key }+click the hull to add an impact point
ui-armor-cast-empty = No impact points yet
ui-armor-cast-miss_nothing_under_pointer = No armor plate under the pointer
ui-armor-cast-miss_thickness_unknown = This model's plates have no known thickness to cast against
ui-armor-cast-miss_no_plate_on_path = The shell's path from this angle crosses no armor plate

# -- Armor Viewer: penetration checker --

## ui.armor.pen
ui-armor-pen-search_hint = Search ships to compare...
ui-armor-pen-add_a_ship = Search and add ships above to compare penetration
ui-armor-pen-hover_a_plate = Hover a plate on the ship to check it
ui-armor-pen-awaiting_plate = hover a plate
ui-armor-pen-clear_all = Clear all
ui-armor-pen-remove = Remove from comparison
ui-armor-pen-firing_ship_tooltip = Use this ship's shells for trajectory casts
ui-armor-pen-use_for_trajectory = Use for trajectory
ui-armor-pen-no_shells = { $ship } has no main battery shells
ui-armor-pen-ifhe = IFHE
ui-armor-pen-ifhe_tooltip = Inertia Fuse for HE Shells raises HE penetration by a quarter
ui-armor-pen-penetrates = penetrates
ui-armor-pen-shatters = shatters
ui-armor-pen-angle_dependent = depends on the angle
ui-armor-pen-no_data = no penetration data

## ui.armor.splash
ui-armor-splash-hint = Click armor in splash mode to see which zones a burst reaches
ui-armor-splash-half_extent = Burst half-extent: { $value } model units
ui-armor-splash-triangles = Armor triangles in burst: { $total }, penetrated: { $penetrated }
ui-armor-splash-no_zones = No splash boxes overlap at this point
ui-armor-splash-zones = Zones in the burst
ui-armor-splash-direct_hit = Direct hit
ui-armor-splash-hp = { $value } HP
ui-armor-splash-section = Splash Zones

## ui.armor.export
ui-armor-export-hull = Hull
ui-armor-export-contents = Contents
ui-armor-export-contents_mesh = Mesh
ui-armor-export-contents_armor = Armor
ui-armor-export-contents_both = Mesh and armor
ui-armor-export-lod = Mesh detail
ui-armor-export-lod_tooltip = 0 is the highest detail. Armor has no detail levels and is unaffected.
ui-armor-export-res = Texture resolution
ui-armor-export-res_full = Full
ui-armor-export-res_2048 = 2048 px
ui-armor-export-res_1024 = 1024 px
ui-armor-export-res_512 = 512 px
ui-armor-export-camos = Camouflages
ui-armor-export-camo_select_none = Select none
ui-armor-export-camo_reset = Reset to default
ui-armor-export-loading_details = Loading...
ui-armor-export-size_estimate = Estimated file size: { $size }
ui-armor-export-size_breakdown = Geometry { $geometry } (exact), textures { $textures } (estimated)
ui-armor-export-size_incomplete = Could not price { $count ->
    [one] { $count } camo
   *[other] { $count } camos
}; size may be incomplete
ui-armor-export-exporting = Exporting { $ship }...
ui-armor-export-already_running = An export is already running
ui-armor-export-exported = Exported { $ship }
ui-armor-export-exported_with_size = Exported { $ship } ({ $size })
ui-armor-export-vehicle_not_found = Vehicle not found
ui-armor-export-export_failed = Export failed: { $error }

## ui.armor.compass
ui-armor-compass-north = N
ui-armor-compass-east = E
ui-armor-compass-south = S
ui-armor-compass-west = W

## ui.armor.realtime
ui-armor-realtime-incoming_fire = Incoming Fire
ui-armor-realtime-attacker_filter = Attacker Filter
ui-armor-realtime-all_enemies = All Enemies
ui-armor-realtime-show_secondaries = Show secondary armament
ui-armor-realtime-seek_to_salvo = Seek to salvo
ui-armor-realtime-no_data = Shell data no longer available
ui-armor-realtime-sim_agrees = Sim agrees
ui-armor-realtime-sim_unchecked = { $count ->
    [one] { $count } shell could not be simulated
   *[other] { $count } shells could not be simulated
}
ui-armor-realtime-sim_rng_zone = RNG zone ({ $angle } deg in { $from }-{ $to })
ui-armor-realtime-sim_disagrees = Sim: { $sim } / Server: { $server }
ui-armor-realtime-no_armor_hit = No armor plates hit
ui-armor-realtime-tracked = { $salvos ->
    [one] { $salvos } salvo
   *[other] { $salvos } salvos
}, { $shells ->
    [one] { $shells } shell tracked
   *[other] { $shells } shells tracked
}
ui-armor-realtime-nothing_landed = Nothing has landed on this ship yet
ui-armor-realtime-shells = { $count }x
ui-armor-realtime-salvo_shell = { $caliber } mm { $shell }
ui-armor-realtime-salvo_range = { $km } km
ui-armor-realtime-unknown_attacker = Ship { $id }
ui-armor-realtime-server_outcome = Server: { $outcome }
ui-armor-realtime-not_cast = This shell cannot be traced through the hull: its flight or the ship's position was not recorded
ui-armor-realtime-pick_hint = Click a salvo or shell to trace it through the hull

## ui.armor.plates
ui-armor-plates-heading = Plates struck ({ $count })
ui-armor-plates-arrival = { $velocity } m/s at impact, { $time } s flight, falling at { $fall } deg
ui-armor-plates-overmatch = Overmatch: { $penetration } mm penetration at { $velocity } m/s
ui-armor-plates-penetrated = Penetrates: { $penetration } mm against { $effective } mm effective at { $velocity } m/s
ui-armor-plates-ricochet = Ricochet at { $angle } deg
ui-armor-plates-shatter = Shatters: { $penetration } mm against { $effective } mm effective
ui-armor-plates-fuse_armed = Fuse armed
ui-armor-plates-detonation = Detonates inside { $zone }, { $distance } m after plate #{ $number }
ui-armor-plates-detonation_past_armor = Detonates past all armor, { $distance } m after plate #{ $number }
ui-armor-plates-flat_penetrates = { $ammo } penetrates: { $penetration } mm against { $thickness } mm
ui-armor-plates-flat_shatters = { $ammo } shatters: { $penetration } mm against { $thickness } mm
ui-armor-plates-server_angle = server { $angle } deg

## ui.tactics
ui-tactics-a_peers_board = A board shared by someone else in this session
ui-tactics-map = Map
ui-tactics-map_hint = Find a map
ui-tactics-mode = Mode
ui-tactics-pick_a_map = Pick a map to draw on
ui-tactics-drawing = Drawing the map...
ui-tactics-reset_view = Reset view
ui-tactics-add_cap = Add cap
ui-tactics-add_cap_tooltip = Place a capture point with the next map click
ui-tactics-selected_cap = Cap { $letter }
ui-tactics-cap_radius = { $km } km
ui-tactics-cap_team = Team
ui-tactics-cap_team_tooltip = Cycle the selected capture point's team
ui-tactics-team_neutral = Neutral
ui-tactics-team_one = Team 1 (Green)
ui-tactics-team_two = Team 2 (Red)
ui-tactics-delete_cap = Delete
ui-tactics-delete_cap_tooltip = Remove the selected capture point
ui-tactics-cap_replay_delete_tooltip = A capture point loaded from a replay cannot be deleted
ui-tactics-cap_narrow_tooltip = Decrease the selected capture point radius
ui-tactics-cap_widen_tooltip = Increase the selected capture point radius
ui-tactics-undo_tooltip = Undo the last board change ({ $shortcut })
ui-tactics-redo_tooltip = Redo the last undone board change ({ $shortcut } or { $alternate })
ui-tactics-clear_caps = Clear added caps
ui-tactics-clear_added_caps_tooltip = Remove user-added capture points
ui-tactics-preset_name = Board name
ui-tactics-preset_save = Save
ui-tactics-preset_delete = Delete
ui-tactics-reset_board = Reset board
ui-tactics-reset_board_tooltip = Restore caps from the selected mode and clear drawings
ui-tactics-preset_needs_a_name = Give the board a name first
ui-tactics-preset_needs_a_map = Set the board on a map first
ui-tactics-preset_saved = Saved { $name }
ui-tactics-preset_save_failed = The board could not be saved: { $error }
ui-tactics-preset_load_failed = The board could not be opened: { $error }
ui-tactics-preset_delete_failed = The board could not be dropped: { $error }
ui-tactics-presets = Saved boards
ui-tactics-preset_open = Open
ui-tactics-clear_drawing = Clear drawing
ui-tactics-scan_replays = Populate caps from replays
ui-tactics-scan_replays_tooltip = Read the replays for the capture layouts of the modes they were played in
ui-tactics-scan_needs_game_data = Load a game build first
ui-tactics-scan_running = Reading replays...
ui-tactics-scan_progress = Reading replays... { $read } / { $total }
ui-tactics-scan_done = Read { $total ->
    [one] { $total } replay
   *[other] { $total } replays
} and found { $added ->
    [one] { $added } new layout
   *[other] { $added } new layouts
}
ui-tactics-session_boards_full = The session already has { $limit ->
    [one] { $limit } board
   *[other] { $limit } boards
} open, so this board stays on its map
ui-tactics-placing = Placing { $ship }
ui-tactics-side_friendly = Friendly
ui-tactics-side_enemy = Enemy
ui-tactics-ink_white = White ink
ui-tactics-ink_gray = Gray ink
ui-tactics-ink_red = Red ink
ui-tactics-ink_orange = Orange ink
ui-tactics-ink_yellow = Yellow ink
ui-tactics-ink_green = Green ink
ui-tactics-ink_blue = Blue ink
ui-tactics-ink_purple = Purple ink
ui-tactics-ink_pink = Pink ink
ui-tactics-no_map_art = This build ships no art for that map
ui-tactics-ship_config = Ship Config
ui-tactics-hull = Hull
ui-tactics-hull_default = Default
ui-tactics-modifiers = Modifiers
ui-tactics-concealment_expert = Concealment Expert
ui-tactics-gun_range_mod = Gun Range Mod
ui-tactics-secondary_range_mod = Secondary Range Mod
ui-tactics-ship_ranges = Range Circles
ui-tactics-shape = Shape
ui-tactics-shape_size = Size
ui-tactics-shape_size_km = Size (km)
ui-tactics-shape_filled = Filled
ui-tactics-shape_smaller = Make the shape smaller
ui-tactics-shape_larger = Make the shape larger

## ui.armor.outcome
ui-armor-outcome-penetration = Penetration
ui-armor-outcome-citadel = Citadel
ui-armor-outcome-ricochet = Ricochet
ui-armor-outcome-shatter = Shatter
ui-armor-outcome-overpenetration = Overpenetration
ui-armor-outcome-underwater = Underwater
ui-armor-outcome-unknown = Unrecognized ({ $raw })
ui-armor-outcome-stopped = Stopped in the armor

# -- What this app's own simulation made of a shell --

## ui.armor.sim
ui-armor-sim-overmatch_rules_out_ricochet = Overmatch, so no ricochet
ui-armor-sim-always_ricochet_band = Ricochet, in the always-ricochet band
ui-armor-sim-ricochet_angle_too_low = No ricochet possible, angle too low

# -- Advantage level labels --

## advantage
advantage-absolute = Absolute
advantage-strong = Strong
advantage-moderate = Moderate
advantage-weak = Weak

# -- Launcher tab --

## ui.launcher
ui-launcher-rescan = Rescan
ui-launcher-add_install = Add install...
ui-launcher-add_install_title = World of Warships install directory
ui-launcher-no_installs = No installs found. Add one, or install a game below.
ui-launcher-managed_by_steam = managed by Steam
ui-launcher-checking_updates = checking updates
ui-launcher-up_to_date = Up to date
ui-launcher-update_available = Update available: { $parts }
ui-launcher-not_installed = not installed: { $parts }
ui-launcher-check_failed = update check failed: { $error }
ui-launcher-recheck = Recheck
ui-launcher-download = Download / Update...
ui-launcher-download_title = Where to install the build
ui-launcher-download_hint = A fresh install is tens of GB; delta updates are not supported yet, so a changed part re-downloads in full.
ui-launcher-pause = Pause
ui-launcher-cancel = Cancel
ui-launcher-resume = Resume
ui-launcher-paused = Download paused; staged files are kept.
ui-launcher-installed = Installed
ui-launcher-installed_skipped = Installed ({ $count ->
    [one] { $count } delta part
   *[other] { $count } delta parts
} skipped: not yet supported, downloaded but not applied)
ui-launcher-download_failed = Download failed: { $error }
ui-launcher-update_in_place = An update goes into the install's own folder. Choose { $path } to update it.
ui-launcher-cancelled = Download cancelled.
ui-launcher-launch_failed = Launch failed: { $error }
ui-launcher-use_for_toolkit = Use for toolkit
ui-launcher-install_heading = Install a game
ui-launcher-install_hint = A fresh install is tens of GB.
ui-launcher-install_title = Where to install the game
ui-launcher-install = Install...
ui-launcher-checking_version = checking available version
ui-launcher-version = version { $version }
ui-launcher-version_unreported = client version not reported
ui-launcher-unavailable = unavailable: { $error }
ui-launcher-retry = Retry

## ui.launcher.account
ui-launcher-account-add_title = Add account
ui-launcher-account-again_title = Sign in again
ui-launcher-account-memory_only = Signed in for this session only. It is not saved, so you will sign in again after a restart.
ui-launcher-account-password_note = Your password goes to Wargaming only and is never stored.
ui-launcher-account-realm_fixed = Realm: { $realm }
ui-launcher-account-sign_in = Sign in
ui-launcher-account-email = Email
ui-launcher-account-password = Password
ui-launcher-account-code = Two-factor code
ui-launcher-account-code_required = Two-factor code required.
ui-launcher-account-use_backup_code = Use backup code
ui-launcher-account-submit_code = Submit code
ui-launcher-account-captcha = This login needs a CAPTCHA, which this launcher can't show yet. Sign in once via the official Wargaming Game Center or wargaming.net in a browser, then try again.

## ui.launcher.sign_in_error
ui-launcher-sign_in_error-wrong_credentials = Wrong email or password.
ui-launcher-sign_in_error-two_factor_required = This account needs two-factor approval. Approve the sign-in in Wargaming Auth or with the emailed code, then sign in again.
ui-launcher-sign_in_error-two_factor_invalid = The two-factor code is wrong or expired. Enter a new code.
ui-launcher-sign_in_error-captcha_required = Wargaming asks for a CAPTCHA. Sign in once in the official Game Center or at wargaming.net, then try again.
ui-launcher-sign_in_error-rate_limited = Too many sign-in attempts. Wait a few minutes, then try again.
ui-launcher-sign_in_error-account_banned = This account is banned.
ui-launcher-sign_in_error-network_unreachable = Could not reach Wargaming. Check your internet connection.
ui-launcher-sign_in_error-service_unavailable = Wargaming's sign-in service is unavailable. Try again later.
ui-launcher-sign_in_error-session_expired = The saved sign-in has expired. Sign in again.
ui-launcher-sign_in_error-keyring_locked = Your system keyring is locked. Unlock it, then sign in again.
ui-launcher-sign_in_error-not_saved = Signed in, but the sign-in could not be saved. You will sign in again after a restart.
ui-launcher-sign_in_error-unexpected = Sign-in failed. The log has the details.

## ui.launcher.realm
ui-launcher-realm-eu = Europe
ui-launcher-realm-na = North America
ui-launcher-realm-asia = Asia

## ui.launcher.runner
ui-launcher-runner-looking = Looking for Proton.
ui-launcher-runner-steam_proton = Steam's { $name }
ui-launcher-runner-umu = umu-run with { $name }
ui-launcher-runner-umu_default = umu-run with UMU-Proton
ui-launcher-runner-native = The game runs natively.
ui-launcher-runner-first_run_prefix = The first launch creates a Wine prefix, which takes a few seconds.
ui-launcher-runner-first_run_download = The first launch downloads UMU-Proton and a Steam runtime, about 700 MB.
ui-launcher-runner-choose = Choose a Proton folder
ui-launcher-runner-automatic = Find automatically
ui-launcher-runner-detect = Look again
ui-launcher-runner-needed = Needs Proton to launch on Linux. Install Steam with Proton, or umu-launcher.

## ui.launcher.opt_in
ui-launcher-opt_in-setting = Enable game launcher
ui-launcher-opt_in-description = Play World of Warships from the toolkit on Linux, signed in with your Wargaming account.
ui-launcher-opt_in-title = Enable the game launcher?
ui-launcher-opt_in-notice_account = By using the launcher feature you acknowledge that this feature is not a supported scenario by the game developer. You are entering your password directly into this application which, if you do not trust, could be used to steal your account (although the application developer does not wish to do that).
ui-launcher-opt_in-notice_reports = Further, you acknowledge that you will not report graphical/reliability bugs to the game developer if you use this feature unless you truly believe it is a cross-platform issue that does not ONLY impact Linux.
ui-launcher-opt_in-continue = Continue
ui-launcher-opt_in-cancel = Cancel

## ui.launcher.keyring
ui-launcher-keyring-unencrypted = Your sign-in is saved unencrypted in the app's data folder because no Secret Service provider (such as GNOME Keyring, KeePassXC or KDE Wallet) is available.
ui-launcher-keyring-how = How to keep it encrypted
ui-launcher-keyring-how_detail = Install and start a Secret Service provider such as GNOME Keyring, KeePassXC or KDE Wallet, then restart the toolkit. Saved sign-ins move into it.
ui-launcher-keyring-unlock_hint = Your system keyring may ask to unlock so the toolkit can read your saved sign-in.

## ui.launcher.play
ui-launcher-play-play = Play
ui-launcher-play-play_through_steam = Play through Steam
ui-launcher-play-set_up_game = Set up game
ui-launcher-play-set_up_runner = Set up Proton
ui-launcher-play-sign_in_again = Sign in again
ui-launcher-play-update = Update { $size }
ui-launcher-play-updating = Updating { $percent }%
ui-launcher-play-updating_unknown = Updating
ui-launcher-play-paused = Paused
ui-launcher-play-starting = Starting...
ui-launcher-play-running = Running
ui-launcher-play-in_battle = In battle
ui-launcher-play-menu = Accounts and game options

## ui.launcher.popover
ui-launcher-popover-game = World of Warships
ui-launcher-popover-no_install = No game install is set.
ui-launcher-popover-update_available = Update available
ui-launcher-popover-check_failed = Could not check for updates
ui-launcher-popover-accounts = Accounts
ui-launcher-popover-no_accounts = No accounts yet. Add one to play signed in.
ui-launcher-popover-close_to_switch = Close the game to switch accounts.
ui-launcher-popover-default = Default
ui-launcher-popover-ready = Ready
ui-launcher-popover-restoring = Checking...
ui-launcher-popover-not_saved = Not saved
ui-launcher-popover-unencrypted = Saved unencrypted
ui-launcher-popover-keyring_locked = Keyring locked: unlock it to restore saved sign-ins
ui-launcher-popover-forget = Remove
ui-launcher-popover-sign_out = Sign out
ui-launcher-popover-expires_in = Expires in { $duration }
ui-launcher-popover-expired = Session expired
ui-launcher-popover-expiry_unknown = Expiry unknown
ui-launcher-popover-make_default = Make default
ui-launcher-popover-account_actions = Account actions
ui-launcher-popover-play_anyway = Play anyway
ui-launcher-popover-play_safe_mode = Play in safe mode
ui-launcher-popover-add_account = Add account...
ui-launcher-popover-game_settings = Game settings...
ui-launcher-popover-installs = Installs and setup...
ui-launcher-popover-open_replay = Open replay in game...

## ui.launcher.palette
ui-launcher-palette-play = Play
ui-launcher-palette-play_as = Play as { $nickname } ({ $realm })
ui-launcher-palette-game_settings = Game settings
ui-launcher-palette-add_account = Add account
ui-launcher-palette-update = Update World of Warships
ui-launcher-palette-open_replay = Open replay in game

## ui.launcher.installs
ui-launcher-installs-toolkit = Toolkit's game directory
ui-launcher-installs-played = Default account plays this
ui-launcher-installs-play_from = Play from here
ui-launcher-installs-update = Update

## ui.launcher.wgc_login
ui-launcher-wgc_login-label = Use Wargaming login
ui-launcher-wgc_login-about = Runs this Steam install through the toolkit's sign-in instead of Steam. Settings and replays are kept apart from Steam runs. Steam wallet, achievements and playtime don't work in this mode.
ui-launcher-wgc_login-steam_accounts = Accounts created through Steam need an email and password. Add them in Wargaming Account Management first.
ui-launcher-wgc_login-steam_accounts_link = How to add an email and password
ui-launcher-wgc_login-ready = Game Center files ready
ui-launcher-wgc_login-fetchable = Turning this on downloads WG's Game Center files for this build ({ $size }).
ui-launcher-wgc_login-downloading = Downloading the Game Center files: { $percent }%
ui-launcher-wgc_login-downloading_unknown = Downloading the Game Center files
ui-launcher-wgc_login-checking = Looking for the Game Center files of this build...
ui-launcher-wgc_login-needs_sign_in = Add a Wargaming account to the toolkit first.
ui-launcher-wgc_login-unavailable = Not available for this build: { $error }

## ui.launcher.replay
ui-launcher-replay-no_install = No install has game build { $build }. Keep or install a copy of that build to watch this replay.

## ui.launcher.setup
ui-launcher-setup-title = Set up World of Warships
ui-launcher-setup-game = Game
ui-launcher-setup-proton = Proton
ui-launcher-setup-accounts = Accounts
ui-launcher-setup-replays = Replays
ui-launcher-setup-game_question = Which copy of the game should the toolkit use?
ui-launcher-setup-none_found = No install was found on this computer.
ui-launcher-setup-choose_folder = Choose a folder...
ui-launcher-setup-install_new = Install a new copy...
ui-launcher-setup-proton_explain = On Linux the game runs through Proton: Steam's when Steam is installed, otherwise umu-run.
ui-launcher-setup-proton_ready = Proton is found.
ui-launcher-setup-accounts_explain = Sign in to the Wargaming accounts you play. Play signs the game in as the default account.
ui-launcher-setup-replays_explain = The game deletes its oldest replays once it holds this many.
ui-launcher-setup-skip = Skip setup
ui-launcher-setup-back = Back
ui-launcher-setup-next = Next
ui-launcher-setup-done = Done

## ui.launcher.game_settings
ui-launcher-game_settings-title = Game settings
ui-launcher-game_settings-build = build { $build }
ui-launcher-game_settings-display = Display
ui-launcher-game_settings-graphics = Graphics
ui-launcher-game_settings-sound = Sound
ui-launcher-game_settings-replays = Replays
ui-launcher-game_settings-locked = The game is running. It rewrites these files when it exits, so they are read-only until then.
ui-launcher-game_settings-footer = Written to the game's preference files. A backup is kept.
ui-launcher-game_settings-cancel = Cancel
ui-launcher-game_settings-save = Save
ui-launcher-game_settings-saved = Game settings saved.
ui-launcher-game_settings-no_install = No game install is set.
ui-launcher-game_settings-window_mode = Window mode
ui-launcher-game_settings-windowed = Windowed
ui-launcher-game_settings-borderless = Borderless
ui-launcher-game_settings-fullscreen = Fullscreen
ui-launcher-game_settings-monitor = Monitor
ui-launcher-game_settings-monitor_name = Monitor name
ui-launcher-game_settings-resolution = Fullscreen resolution
ui-launcher-game_settings-width = Width
ui-launcher-game_settings-height = Height
ui-launcher-game_settings-refresh = Refresh rate
ui-launcher-game_settings-display_rate = Display rate
ui-launcher-game_settings-vsync = Vertical sync
ui-launcher-game_settings-ui_scale = Interface scale
ui-launcher-game_settings-battle_fps = Frame rate limit in battle
ui-launcher-game_settings-port_fps = Frame rate limit in port
ui-launcher-game_settings-unlimited = Unlimited
ui-launcher-game_settings-skip_intro = Skip intro video
ui-launcher-game_settings-master = Master volume
ui-launcher-game_settings-music_port = Music in port
ui-launcher-game_settings-music_battle = Music in battle
ui-launcher-game_settings-preset = Preset
ui-launcher-game_settings-antialiasing = Anti-aliasing
ui-launcher-game_settings-textures = Texture quality
ui-launcher-game_settings-filtering = Texture filtering
ui-launcher-game_settings-shadows = Shadow quality
ui-launcher-game_settings-graphics_note = Change graphics in the game. It checks them against your hardware.
ui-launcher-game_settings-replays_kept = Replays to keep
ui-launcher-game_settings-replays_kept_zero = Keep at least one replay.
ui-launcher-game_settings-versioned = Sort replays by game version
ui-launcher-game_settings-replays_note = Written to this build's res_mods. Safe mode does not use it.
ui-launcher-game_settings-absent = Not set in the game's files
ui-launcher-game_settings-unrecognized = Unknown value: { $value }
ui-launcher-game_settings-not_a_number = { $field }: enter a number.

## ui.settings.launcher
ui-settings-launcher-installs = Game installs
ui-settings-launcher-installs_description = The installs the launcher plays and updates, for the game.
ui-settings-launcher-runner = Proton (Linux)
ui-settings-launcher-runner_description = How the game runs on Linux. Steam's Proton Experimental or newest Proton is found automatically, else umu-run.
ui-settings-launcher-launch_args = Launch arguments
ui-settings-launcher-launch_args_description = Extra command-line arguments for the game, one per line.
ui-settings-launcher-environment = Environment
ui-settings-launcher-environment_description = Environment variables for the game, KEY=VALUE, one per line.
ui-settings-launcher-safe_mode_args = Safe mode arguments
ui-settings-launcher-safe_mode_args_description = Arguments added when you play the game in safe mode, one per line.

## ui.replay.damage_share
ui-replay-damage_share-mode = Damage as a share of the other ship's total
ui-replay-damage_share-dealt = { $pct } of the damage { $ship } dealt went to { $other }.
ui-replay-damage_share-dealt_inverse = { $pct } of the damage { $other } received came from { $ship }.
ui-replay-damage_share-received = { $pct } of the damage { $ship } received came from { $other }.
ui-replay-damage_share-received_inverse = { $pct } of the damage { $other } dealt went to { $ship }.
ui-replay-damage_share-hold = Hold { $key } to invert.
ui-replay-damage_share-release = Release { $key } to invert back.

## ui.reindex
ui-reindex-context = { $count ->
    [one] Force Re-index
   *[other] Force Re-index { $count } Replays
}
ui-reindex-palette_by_version = Force Re-index Replays: By Version
ui-reindex-palette_all = Force Re-index Replays: All
ui-reindex-versions_title = Force Re-index Replays: By Version
ui-reindex-versions_placeholder = Filter game versions...
ui-reindex-version_row = { $version } -- { $count ->
    [one] { $count } replay
   *[other] { $count } replays
}
ui-reindex-no_versions = No listed replay names a game version
ui-reindex-confirm_title = Force Re-index Replays
ui-reindex-confirm_ok = Re-index
ui-reindex-confirm_count = { $count ->
    [one] { $count } replay will be read again and its index entry rewritten.
   *[other] { $count } replays will be read again and their index entries rewritten.
}
ui-reindex-confirm_range = Played from { $oldest } to { $newest }.
ui-reindex-confirm_undated = { $count ->
    [one] { $count } replay to be read has no readable battle date and is not in that range.
   *[other] { $count } replays to be read have no readable battle date and are not in that range.
}
ui-reindex-confirm_downloads = Game data for these versions is downloaded first:
ui-reindex-confirm_unobtainable = No game data can be obtained for these versions, so their { $count ->
    [one] { $count } replay is skipped:
   *[other] { $count } replays are skipped:
}
ui-reindex-confirm_no_cache_dir = No game data cache directory is set, so game data for these versions cannot be downloaded and their { $count ->
    [one] { $count } replay is skipped:
   *[other] { $count } replays are skipped:
}
ui-reindex-confirm_no_build = { $count ->
    [one] { $count } replay names no game build and is skipped.
   *[other] { $count } replays name no game build and are skipped.
}
ui-reindex-confirm_no_version = { $count ->
    [one] { $count } replay names no game version that can be read and is skipped.
   *[other] { $count } replays name no game version that can be read and are skipped.
}
ui-reindex-build_row = { $version } (build { $build }): { $count ->
    [one] { $count } replay
   *[other] { $count } replays
}
ui-reindex-nothing_listed = The listing holds no replays to re-index.
ui-reindex-nothing_readable = None of these replays can be re-indexed: no game data is available for them ({ $count } skipped).
ui-reindex-no_game_data = Replays cannot be read until the game directory is set.
ui-reindex-busy_index = An index pass is already running.
ui-reindex-busy_cache = A game data job is already running. Try again when it finishes.
ui-reindex-planning = Checking which game data is available...
ui-reindex-no_index = The replay index is not open, so nothing was re-indexed.
ui-reindex-done = Re-indexed { $indexed ->
    [one] { $indexed } replay
   *[other] { $indexed } replays
}; { $skipped } skipped, { $failed } could not be read
ui-reindex-stopped = Re-indexing stopped: { $indexed } re-indexed, { $skipped } skipped, { $failed } could not be read
ui-reindex-stopped_before_reading = Re-indexing stopped before any replay was read.

## ui.index_folder
ui-index_folder-tooltip = Index every replay in this list that the replay index does not hold yet
ui-index_folder-confirm_title = Index Replays
ui-index_folder-confirm_ok = Index
ui-index_folder-confirm_count = { $count ->
    [one] { $count } replay in this list is not in the replay index yet and will be read and added to it.
   *[other] { $count } replays in this list are not in the replay index yet and will be read and added to it.
}
ui-index_folder-note_live = Indexed replays are added to the replay index that Search and the Player Tracker read.
ui-index_folder-note_folder = Indexed replays are added to the replay index under this directory's own source ({ $dir }). Search and the Player Tracker's player list include them. The Player Tracker's clan list reads only the game's own replays.
ui-index_folder-nothing_listed = This list holds no replays to index.
ui-index_folder-nothing_missing = Every replay in this list is already indexed.
ui-index_folder-unreadable_index = The replay index could not be read.
ui-index_folder-done = Indexed { $indexed ->
    [one] { $indexed } replay
   *[other] { $indexed } replays
}; { $skipped } skipped, { $failed } could not be read
ui-index_folder-stopped = Indexing stopped: { $indexed } indexed, { $skipped } skipped, { $failed } could not be read
ui-index_folder-stopped_before_reading = Indexing stopped before any replay was read.

## ui.replay_folders
ui-replay_folders-install = Game
ui-replay_folders-watched = Watched: new and changed replays appear here
ui-replay_folders-unwatched = Not watched: use Refresh to read it again
ui-replay_folders-unseen = { $count ->
    [one] { $count } new or changed replay since you last viewed it
   *[other] { $count } new or changed replays since you last viewed it
}
ui-replay_folders-missing = This directory no longer exists: { $dir }
ui-replay_folders-missing_hint = It opens here at every startup until you unpin it.
ui-replay_folders-pin = Pin
ui-replay_folders-unpin = Unpin
ui-replay_folders-refresh = Refresh
ui-replay_folders-close = Close
ui-replay_folders-pins_unreadable = Pinned replay directories could not be read.
ui-replay_folders-game_data_available = Game data is available for the listed replays.
ui-replay_folders-browse = Browse...
ui-replay_folders-recent_title = Open Replay Directory
ui-replay_folders-recent_placeholder = Filter recent directories...
ui-replay_folders-recent_missing = This directory no longer exists and was removed from the recent list: { $dir }
ui-replay_folders-checking = Looking for { $dir }...
ui-replay_folders-indexing = Indexing { $dir }
ui-replay_folders-indexing_in_background = This directory is still being indexed
ui-replay_folders-search_while_indexing = This directory is still being indexed, so the results are not complete yet. Search again once it finishes.
ui-replay_folders-source_failed = This directory's replays cannot be indexed: { $error }
ui-replay_folders-restart_watch = Restart Watching
ui-replay_folders-watch_stopped = Watching { $folder } stopped: { $cause }.
ui-replay_folders-watch_restarting = It restarts in { $seconds } s (attempt { $attempt } of { $attempts }).
ui-replay_folders-watch_gave_up = It failed to restart { $attempts ->
    [one] { $attempts } time
   *[other] { $attempts } times
} and is not tried again.
ui-replay_folders-watch_restart_hint = Click the red dot or choose Restart Watching from the tab menu to restart it now.
ui-replay_folders-watch_resumed = Watching { $folder } resumed.
ui-replay_folders-watch_cause_backend = the file system reported an error: { $error }
ui-replay_folders-watch_cause_gone = the directory was moved or removed
ui-replay_folders-watch_cause_panicked = internal error: { $error }
ui-replay_folders-watch_cause_exited = the watcher ended unexpectedly
ui-replay_folders-dropped = Dropped
ui-replay_folders-dropped_numbered = Dropped { $number }
ui-replay_folders-dropped_count = { $count ->
    [one] { $count } dropped replay
   *[other] { $count } dropped replays
}
ui-replay_folders-dropped_no_source = Dropped replays are indexed with their own directories. Open a directory to search or index it.
ui-replay_folders-unreadable_replay = Could not be read: { $error }

## ui.models
ui-models-heading = Models
ui-models-loading = Loading models...
ui-models-unavailable = Models unavailable
ui-models-search = Search ships and models...
ui-models-open = Open
ui-models-export = Export Model
ui-models-copy_path = Copy model path
ui-models-path_copied = Copied { $path }
ui-models-skin_replaces = Replaces { $model } when { $exterior } is equipped
ui-models-geometry_absent = Not in your game files
ui-models-geometry_none = This model has no geometry
ui-models-hardpoints_tooltip = Show attachment points
ui-models-common = Common models

## ui.models.group
ui-models-group-gameplay = Gameplay
ui-models-group-building = Buildings
ui-models-group-character = Characters
ui-models-group-location = Locations
ui-models-group-port = Ports
ui-models-group-style = Styles
ui-models-group-other = Other models

## ui.models.building
ui-models-building-weapon = Building weapons
ui-models-building-unassigned = Unassigned buildings
ui-models-building-unspecified = Buildings without a type

## ui.models.misc
ui-models-misc-crane = Cranes
ui-models-misc-anchor = Anchors
ui-models-misc-deck_fitting = Deck fittings
ui-models-misc-instrument = Instruments
ui-models-misc-propeller = Propellers
ui-models-misc-searchlight = Searchlights
ui-models-misc-boat = Boats
ui-models-misc-flag = Flags
ui-models-misc-insignia = Insignia
ui-models-misc-railing = Railings
ui-models-misc-hatch = Hatches
ui-models-misc-ventilation = Ventilation
ui-models-misc-antenna = Antennas
ui-models-misc-weapon = Weapons
ui-models-misc-stowage = Stowage
ui-models-misc-other = Other

## ui.models.variant
ui-models-variant-dead = Wreck
ui-models-variant-skin = Skin

## ui.models.kind
ui-models-kind-ship = Ships
ui-models-kind-gun = Guns
ui-models-kind-director = Directors
ui-models-kind-finder = Rangefinders
ui-models-kind-radar = Radars
ui-models-kind-catapult = Catapults
ui-models-kind-aircraft = Aircraft
ui-models-kind-projectile = Projectiles
ui-models-kind-misc = Miscellaneous
ui-models-kind-cap = Capture points
ui-models-kind-other = Other

## ui.models.sub_kind
ui-models-sub_kind-battleship = Battleships
ui-models-sub_kind-cruiser = Cruisers
ui-models-sub_kind-destroyer = Destroyers
ui-models-sub_kind-aircraft_carrier = Aircraft carriers
ui-models-sub_kind-submarine = Submarines
ui-models-sub_kind-auxiliary = Auxiliary ships
ui-models-sub_kind-main_battery = Main battery
ui-models-sub_kind-secondary_battery = Secondary battery
ui-models-sub_kind-anti_aircraft = Anti-aircraft
ui-models-sub_kind-torpedo_tubes = Torpedo tubes
ui-models-sub_kind-depth_charge_launcher = Depth charge launchers
ui-models-sub_kind-missile_launcher = Missile launchers
ui-models-sub_kind-search_radar = Search radars
ui-models-sub_kind-fire_control_radar = Fire control radars
ui-models-sub_kind-fighter = Fighters
ui-models-sub_kind-bomber = Bombers
ui-models-sub_kind-dive_bomber = Dive bombers
ui-models-sub_kind-scout = Scouts
ui-models-sub_kind-torpedo = Torpedoes
ui-models-sub_kind-bomb = Bombs
ui-models-sub_kind-rocket = Rockets
ui-models-sub_kind-depth_charge = Depth charges
ui-models-sub_kind-mine = Mines
ui-models-sub_kind-missile = Missiles
ui-models-sub_kind-jato = Takeoff boosters
ui-models-sub_kind-shell = Shells

## ui.models.part
ui-models-part-hull = Hull
ui-models-part-main_battery = Main battery
ui-models-part-secondaries = Secondaries
ui-models-part-anti_aircraft = AA
ui-models-part-torpedoes = Torpedoes
ui-models-part-directors = Directors
ui-models-part-finders = Rangefinders
ui-models-part-radars = Radars
ui-models-part-depth_charges = Depth charges
ui-models-part-aircraft = Aircraft
ui-models-part-other = Other
ui-models-part-misc = Misc

## ui.model_view
ui-model_view-thickness_from_building = Thickness from { $building }
ui-model_view-thickness_from = Thickness from { $ship } ({ $hp })
ui-model_view-thickness_unknown = Thickness unknown: no ship mounts this model
ui-model_view-thickness_other_ships = Other ships
ui-model_view-export_title = Export Model
ui-model_view-export_tooltip = Export the model to a GLB file
ui-model_view-part_model = Model: { $model }
ui-model_view-part_hardpoint = Attachment point: { $hardpoint }

## ui.model_view.part_group
ui-model_view-part_group-hull = Hull
ui-model_view-part_group-main_battery = Main battery
ui-model_view-part_group-secondary_battery = Secondary battery
ui-model_view-part_group-aa_guns = AA guns
ui-model_view-part_group-torpedoes = Torpedoes
ui-model_view-part_group-depth_charges = Depth charges
ui-model_view-part_group-fire_control = Fire control
ui-model_view-part_group-radar = Radar
ui-model_view-part_group-missiles = Missiles
ui-model_view-part_group-decorations = Decorations
ui-model_view-part_group-misc = Misc
ui-model_view-part_group-armor = Armor
ui-model_view-part_group-other = Other

## ui.scene_problem
ui-scene_problem-cannot_show = { $name } cannot be shown
ui-scene_problem-geometry_absent = This model's geometry is not in your game files.
ui-scene_problem-model_unreadable = This model's data could not be read. The log has the details.
ui-scene_problem-ship_unreadable = This ship's data could not be read. The log has the details.
ui-scene_problem-no_meshes = This model has no meshes.
ui-scene_problem-no_meshes_at_lod = This model has no meshes at the chosen detail level.
ui-scene_problem-no_visible_surface = This model has no visible surface to draw.
ui-scene_problem-section_stand_in = This hull section has no geometry of its own. The ship draws this part of its hull in another section.

## ui.armor.option_tabs
ui-armor-option_tabs-label = Option tabs
ui-armor-option_tabs-ship = Ship
ui-armor-option_tabs-layers = Layers
ui-armor-option_tabs-analysis = Analysis
ui-armor-option_tabs-configuration = Configuration
ui-armor-option_tabs-hull_parts = Hull Parts
ui-armor-option_tabs-tab_matches = { $tab }, { $count } matching
ui-armor-option_tabs-empty = Nothing in this tab applies to the shown model.
ui-armor-option_tabs-no_choices = This model has no hull, module, camo or detail level to choose.

## ui.armor.hidden_ship
ui-armor-hidden_ship-clan_rental = Clan rental
ui-armor-hidden_ship-other_class = Not a ship class

## ui.armor.textures
ui-armor-textures-title = Textures
ui-armor-textures-hint = The game reads a file under the build's res_mods folder in place of the packaged file at the same path. Right-click a texture to copy that path.
ui-armor-textures-under_pointer = Under the pointer: { $material }
ui-armor-textures-reading = Reading textures...
ui-armor-textures-summary = { $materials ->
    [one] { $materials } material
   *[other] { $materials } materials
}, { $textures ->
    [one] { $textures } texture
   *[other] { $textures } textures
}
ui-armor-textures-no_material = No material
ui-armor-textures-unresolved = The path is not in assets.bin
ui-armor-textures-absent = Not in the installed packages
ui-armor-textures-unreadable = The DDS header could not be read
ui-armor-textures-unknown_format = Unrecognized format
ui-armor-textures-details = { $format }, { $size }, { $count ->
    [one] { $count } file
   *[other] { $count } files
}, { $bytes }
ui-armor-textures-role_albedo = Albedo
ui-armor-textures-role_normal = Normal
ui-armor-textures-role_metal_gloss = Metal-gloss
ui-armor-textures-role_ambient_occlusion = Ambient occlusion
ui-armor-textures-role_detail = Detail
ui-armor-textures-role_name_matched = Albedo, found by file name
ui-armor-textures-copy_path = Copy Path
ui-armor-textures-copy_overrides = { $count ->
    [one] Copy res_mods Path
   *[other] Copy { $count } res_mods Paths
}
ui-armor-textures-paths_copied = Copied { $count ->
    [one] { $count } path
   *[other] { $count } paths
}
ui-armor-textures-no_build = The game data has not loaded, so the build's mod folder is not known.
ui-armor-textures-show_in_unpacker = Show in Resource Unpacker
ui-armor-textures-show_textures = Show Textures
ui-armor-textures-extract = Extract Files...
ui-armor-textures-extract_title = Extract texture files to
ui-armor-textures-extracting = Extracting texture files...
ui-armor-textures-extracted = Extracted { $count ->
    [one] { $count } file
   *[other] { $count } files
} to { $folder }
ui-armor-textures-extract_failed = The texture files could not be extracted: { $reason }
ui-armor-textures-replace_title = Replace Existing Files?
ui-armor-textures-replace_description = { $count ->
    [one] { $count } of these files is already in { $folder }. Extracting replaces it.
   *[other] { $count } of these files are already in { $folder }. Extracting replaces them.
}
ui-armor-textures-replace = Replace

## ui.settings.mcp
ui-settings-mcp-enabled = Enable MCP server
ui-settings-mcp-enabled_description = Serve replays and game data to AI assistants over MCP. Shows a status bar control.
ui-settings-mcp-start_on_launch = Start with the program
ui-settings-mcp-start_on_launch_description = Start the server when WoWs Toolkit opens.
ui-settings-mcp-bind_address = Bind address
ui-settings-mcp-bind_address_description = Loopback IP address to listen on, such as 127.0.0.1 or ::1. Only this computer can reach the server.
ui-settings-mcp-port = Port
ui-settings-mcp-port_description = TCP port to listen on. Chosen at random on first enable and kept.
ui-settings-mcp-port_unset = Chosen on first enable
ui-settings-mcp-address_not_loopback = { $input } is not a loopback address. The server listens only on this computer, such as on 127.0.0.1.
ui-settings-mcp-address_invalid = { $input } is not an IP address
ui-settings-mcp-port_invalid = { $input } is not a port from 1 to 65535
ui-settings-mcp-port_in_use = Another program is using this port. Choose another port.

## ui.mcp
ui-mcp-started = MCP server running at { $url }
ui-mcp-copy_url = Copy URL
ui-mcp-start_failed = Cannot start the MCP server: { $reason }
ui-mcp-port_in_use = port { $port } on { $address } is in use by another program
ui-mcp-no_watchdog = the background worker supervisor is not running
ui-mcp-status_running = MCP server running at { $url }. Click to stop.
ui-mcp-status_starting = MCP server starting on { $address }
ui-mcp-status_failed = MCP server failed to start. Click to retry.
ui-mcp-status_stopped = MCP server stopped. Click to start.
