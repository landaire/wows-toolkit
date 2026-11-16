-- The display string stays for the surfaces that already read it. The numeric
-- id is what the query layer filters on, because the string is not stable
-- across locales or game versions.
ALTER TABLE indexed_match ADD COLUMN game_mode_id INTEGER;

CREATE INDEX idx_indexed_match_game_mode_id ON indexed_match(game_mode_id);
