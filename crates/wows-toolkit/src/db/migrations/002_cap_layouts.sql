-- Cap point layouts: one row per (map, scenario) combination.
-- Each row stores a single CapLayout serialized via rkyv.
CREATE TABLE IF NOT EXISTS cap_layouts (
    map_id             INTEGER NOT NULL,
    scenario_config_id INTEGER NOT NULL,
    layout_data        BLOB    NOT NULL,  -- rkyv-serialized CapLayout
    PRIMARY KEY (map_id, scenario_config_id)
);
