-- The two duplicate live sources are populated by two separate threads
-- (background indexing and startup reconciliation), each caching its own
-- resolved source_id for the session, so a record present under both can
-- disagree: one thread can index a replay before WG post-battle results are
-- written and the other after. Drop the lower-id (surviving) source's copy
-- when the higher-id (doomed) source holds a better one for the same path,
-- so the repoint below keeps the better row: results-bearing beats
-- results-absent, then most recently indexed wins.
DELETE FROM replay_record
WHERE record_id IN (
  SELECT s.record_id
  FROM replay_record s
  JOIN replay_record d ON d.replay_path = s.replay_path
  WHERE s.source_id = (SELECT MIN(source_id) FROM index_source WHERE kind = 'live')
    AND d.source_id > s.source_id
    AND d.source_id IN (SELECT source_id FROM index_source WHERE kind = 'live')
    AND (d.results_available > s.results_available
         OR (d.results_available = s.results_available AND d.indexed_at > s.indexed_at))
);

-- Collapse any duplicate live sources onto the lowest id before constraining.
-- A first launch could previously create two, because source creation was a
-- non-atomic check-then-insert called from two threads.
UPDATE OR IGNORE replay_record
SET source_id = (SELECT MIN(source_id) FROM index_source WHERE kind = 'live')
WHERE source_id IN (
  SELECT source_id FROM index_source
  WHERE kind = 'live' AND source_id > (SELECT MIN(source_id) FROM index_source WHERE kind = 'live')
);

-- Records that could not be repointed collided with the survivor's row for
-- that path, which is now equal-or-better for every remaining collision: a
-- survivor row that was worse was already dropped above, so whatever is left
-- on a doomed source at this point is redundant.
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
