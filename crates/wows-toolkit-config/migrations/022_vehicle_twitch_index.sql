-- The roster rows matched to a Twitch chat login, for the Player Tracker's
-- per-player counts. Partial, since nearly every row is unmatched, and
-- covering, so the read never touches the table.
CREATE INDEX IF NOT EXISTS idx_vehicle_twitch ON indexed_vehicle (account_id, arena_id, sniper_twitch_login)
    WHERE is_stream_sniper = 1 AND sniper_twitch_login IS NOT NULL;
