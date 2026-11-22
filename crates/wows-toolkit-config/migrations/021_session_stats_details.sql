-- Per-battle figures beyond the core columns, for the Stats charts. NULL where
-- the battle did not record one, and on every row written before these were kept.
ALTER TABLE session_stats ADD COLUMN potential_damage INTEGER;
ALTER TABLE session_stats ADD COLUMN received_damage INTEGER;
ALTER TABLE session_stats ADD COLUMN hits INTEGER;
ALTER TABLE session_stats ADD COLUMN fires INTEGER;
ALTER TABLE session_stats ADD COLUMN floods INTEGER;
ALTER TABLE session_stats ADD COLUMN citadels INTEGER;
ALTER TABLE session_stats ADD COLUMN crits INTEGER;
ALTER TABLE session_stats ADD COLUMN distance_km REAL;
ALTER TABLE session_stats ADD COLUMN capture_points INTEGER;
ALTER TABLE session_stats ADD COLUMN planes_killed INTEGER;
ALTER TABLE session_stats ADD COLUMN anti_air_damage INTEGER;
ALTER TABLE session_stats ADD COLUMN time_lived_secs INTEGER;
ALTER TABLE session_stats ADD COLUMN survived INTEGER;
ALTER TABLE session_stats ADD COLUMN torpedoes_spotted INTEGER;
ALTER TABLE session_stats ADD COLUMN heals INTEGER;
