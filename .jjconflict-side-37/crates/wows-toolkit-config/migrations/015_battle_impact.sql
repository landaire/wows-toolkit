-- `bi_version` is the formula revision `bi` was computed with, so a rating
-- from an older formula is found and recomputed. NULL on rows written before
-- BI was computed.
--
-- `bi_torpedoes_observed` is set when `bi` includes the torpedo warning part,
-- which only a replay recorded from the player's own team can observe.
ALTER TABLE indexed_vehicle ADD COLUMN bi REAL;
ALTER TABLE indexed_vehicle ADD COLUMN bi_version INTEGER;
ALTER TABLE indexed_vehicle ADD COLUMN bi_torpedoes_observed INTEGER NOT NULL DEFAULT 0;
