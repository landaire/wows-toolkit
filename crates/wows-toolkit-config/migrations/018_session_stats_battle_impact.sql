-- The recording player's battle impact for each recorded game, with the
-- formula revision that produced it.
ALTER TABLE session_stats ADD COLUMN battle_impact REAL;
ALTER TABLE session_stats ADD COLUMN battle_impact_version INTEGER;
