-- Core settings as key-value pairs for simple/flat values.
CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL  -- JSON-encoded values
);

-- Session stats: one row per game.
CREATE TABLE IF NOT EXISTS session_stats (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    sort_key        TEXT    NOT NULL,  -- YYYY-MM-DD HH:MM:SS for ordering
    ship_name       TEXT    NOT NULL DEFAULT '',
    ship_id         INTEGER NOT NULL DEFAULT 0,
    player_id       INTEGER NOT NULL DEFAULT 0,
    game_time       TEXT    NOT NULL DEFAULT '',
    match_group     TEXT    NOT NULL DEFAULT '',
    damage          INTEGER NOT NULL DEFAULT 0,
    spotting_damage INTEGER NOT NULL DEFAULT 0,
    frags           INTEGER NOT NULL DEFAULT 0,
    raw_xp          INTEGER NOT NULL DEFAULT 0,
    base_xp         INTEGER NOT NULL DEFAULT 0,
    is_win          INTEGER NOT NULL DEFAULT 0,
    is_loss         INTEGER NOT NULL DEFAULT 0,
    is_draw         INTEGER NOT NULL DEFAULT 0,
    is_div          INTEGER NOT NULL DEFAULT 0,
    achievements    TEXT    NOT NULL DEFAULT '[]'  -- JSON array of SerializableAchievement
);

-- Player tracker is stored as a JSON blob in the settings table
-- (key = "player_tracker_data") because the struct has private fields
-- and complex nested types that are simplest to round-trip as JSON.

-- Sent replays (dedup set).
CREATE TABLE IF NOT EXISTS sent_replays (
    replay_path TEXT PRIMARY KEY NOT NULL
);

-- Chart configurations: one row per chart tab.
CREATE TABLE IF NOT EXISTS chart_configs (
    chart_id INTEGER PRIMARY KEY NOT NULL,
    config   TEXT    NOT NULL  -- JSON-serialized SessionStatsChartConfig
);

-- Armor viewer defaults (single row).
CREATE TABLE IF NOT EXISTS armor_viewer_defaults (
    id                INTEGER PRIMARY KEY CHECK (id = 1),
    show_plate_edges  INTEGER NOT NULL DEFAULT 1,
    show_waterline    INTEGER NOT NULL DEFAULT 1,
    show_zero_mm      INTEGER NOT NULL DEFAULT 0,
    armor_opacity     REAL    NOT NULL DEFAULT 1.0,
    waterline_opacity REAL    NOT NULL DEFAULT 0.3,
    hull_opaque       INTEGER NOT NULL DEFAULT 0,
    hull_all_visible  INTEGER NOT NULL DEFAULT 0,
    armor_all_visible INTEGER NOT NULL DEFAULT 1,
    show_splash_boxes INTEGER NOT NULL DEFAULT 0
);

-- Renderer/display options (single row, JSON blob).
CREATE TABLE IF NOT EXISTS render_options (
    id   INTEGER PRIMARY KEY CHECK (id = 1),
    data TEXT    NOT NULL  -- JSON blob of SavedRenderOptions
);

-- Stats dock layout.
CREATE TABLE IF NOT EXISTS dock_layouts (
    name   TEXT PRIMARY KEY NOT NULL,
    layout TEXT NOT NULL  -- JSON-serialized DockState
);

-- Mod manager state (single row, JSON blob).
CREATE TABLE IF NOT EXISTS mod_manager (
    id   INTEGER PRIMARY KEY CHECK (id = 1),
    data TEXT NOT NULL  -- JSON blob of ModManagerInfo
);
