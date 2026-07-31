-- Collapse any duplicate live sources onto the lowest id before constraining.
-- A first launch could previously create two, because source creation was a
-- non-atomic check-then-insert called from two threads.
UPDATE OR IGNORE replay_record
SET source_id = (SELECT MIN(source_id) FROM index_source WHERE kind = 'live')
WHERE source_id IN (
  SELECT source_id FROM index_source
  WHERE kind = 'live' AND source_id > (SELECT MIN(source_id) FROM index_source WHERE kind = 'live')
);

-- Records that could not be repointed collided with an existing
-- (source_id, replay_path) row, so the surviving row already describes them.
DELETE FROM replay_record
WHERE source_id IN (
  SELECT source_id FROM index_source
  WHERE kind = 'live' AND source_id > (SELECT MIN(source_id) FROM index_source WHERE kind = 'live')
);

DELETE FROM index_source
WHERE kind = 'live' AND source_id > (SELECT MIN(source_id) FROM index_source WHERE kind = 'live');

-- Two sources pointing at the same directory would each index the same files.
UPDATE index_source
SET root_path = NULL
WHERE root_path IS NOT NULL
  AND source_id > (SELECT MIN(s2.source_id) FROM index_source s2 WHERE s2.root_path = index_source.root_path);

CREATE UNIQUE INDEX idx_source_single_live ON index_source(kind) WHERE kind = 'live';
CREATE UNIQUE INDEX idx_source_root_path ON index_source(root_path) WHERE root_path IS NOT NULL;
