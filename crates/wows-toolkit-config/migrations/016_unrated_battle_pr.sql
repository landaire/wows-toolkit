-- A personal rating applies only to Random and Ranked battles. Stored ratings
-- are never replaced by a re-index, so ratings written for other battles are
-- cleared here. The predicate selects unrated battles and mirrors
-- RatingScope::of_battle: a gameType decides, and an absent one falls back to
-- the match group. A test in wows-toolkit-config checks the two agree.
UPDATE replay_record SET self_pr = NULL
WHERE self_pr IS NOT NULL AND arena_id IN (
  SELECT arena_id FROM indexed_match
  WHERE NOT (
    game_type IN ('RandomBattle', 'RankedBattle', 'OldRankedBattle')
    OR (game_type = '' AND match_group IN ('pvp', 'ranked'))
  )
);

UPDATE indexed_vehicle SET pr = NULL
WHERE pr IS NOT NULL AND arena_id IN (
  SELECT arena_id FROM indexed_match
  WHERE NOT (
    game_type IN ('RandomBattle', 'RankedBattle', 'OldRankedBattle')
    OR (game_type = '' AND match_group IN ('pvp', 'ranked'))
  )
);
