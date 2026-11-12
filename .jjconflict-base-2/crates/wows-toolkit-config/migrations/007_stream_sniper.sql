CREATE TABLE IF NOT EXISTS twitch_observation (
    login   TEXT    NOT NULL,
    seen_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_twitch_observation_seen_at ON twitch_observation (seen_at);
CREATE UNIQUE INDEX IF NOT EXISTS idx_twitch_observation_login_seen ON twitch_observation (login, seen_at);
ALTER TABLE indexed_vehicle ADD COLUMN is_stream_sniper INTEGER;
ALTER TABLE indexed_vehicle ADD COLUMN sniper_twitch_login TEXT;
