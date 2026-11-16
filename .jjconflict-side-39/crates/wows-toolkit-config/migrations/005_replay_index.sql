CREATE TABLE index_source (
  source_id   INTEGER PRIMARY KEY,
  name        TEXT NOT NULL,
  kind        TEXT NOT NULL,
  root_path   TEXT,
  added_at    INTEGER NOT NULL
);

CREATE TABLE indexed_match (
  arena_id       INTEGER PRIMARY KEY,
  timestamp      INTEGER NOT NULL,
  map            TEXT NOT NULL,
  game_mode      TEXT NOT NULL,
  game_type      TEXT NOT NULL,
  match_group    TEXT NOT NULL,
  version_build  INTEGER
);

CREATE TABLE replay_record (
  record_id          INTEGER PRIMARY KEY,
  arena_id           INTEGER NOT NULL REFERENCES indexed_match(arena_id) ON DELETE CASCADE,
  source_id          INTEGER NOT NULL REFERENCES index_source(source_id) ON DELETE CASCADE,
  replay_path        TEXT NOT NULL,
  file_mtime         INTEGER,
  outcome            TEXT NOT NULL,
  self_account_id    INTEGER,
  self_ship_id       INTEGER,
  self_survived      INTEGER,
  self_damage        INTEGER,
  self_kills         INTEGER,
  self_pr            REAL,
  results_available  INTEGER NOT NULL,
  indexed_at         INTEGER NOT NULL,
  UNIQUE (source_id, replay_path)
);

CREATE TABLE indexed_vehicle (
  arena_id       INTEGER NOT NULL REFERENCES indexed_match(arena_id) ON DELETE CASCADE,
  account_id     INTEGER NOT NULL,
  player_name    TEXT NOT NULL,
  clan           TEXT NOT NULL,
  realm          TEXT,
  ship_id        INTEGER NOT NULL,
  ship_index     TEXT NOT NULL,
  ship_name      TEXT NOT NULL,
  nation         TEXT NOT NULL,
  species        TEXT NOT NULL,
  tier           INTEGER NOT NULL,
  relation       TEXT NOT NULL,
  division_id    INTEGER,
  survived       INTEGER,
  damage         INTEGER,
  kills          INTEGER,
  spotting       INTEGER,
  potential      INTEGER,
  received       INTEGER,
  pr             REAL,
  is_test_ship   INTEGER NOT NULL,
  PRIMARY KEY (arena_id, account_id, ship_id)
);

CREATE INDEX idx_record_arena     ON replay_record(arena_id);
CREATE INDEX idx_record_source    ON replay_record(source_id);
CREATE INDEX idx_record_self_ship ON replay_record(self_ship_id);
CREATE INDEX idx_vehicle_account  ON indexed_vehicle(account_id);
CREATE INDEX idx_vehicle_ship     ON indexed_vehicle(ship_id, relation);
CREATE INDEX idx_match_time       ON indexed_match(timestamp);
