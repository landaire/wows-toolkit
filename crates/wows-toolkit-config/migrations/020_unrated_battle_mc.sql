-- Match contribution applies only to Random and Ranked battles. A replay that
-- is never parsed again keeps its stored MC, so values written for other
-- battles are cleared here. The predicate selects unrated battles and mirrors
-- RatingScope::of_battle, as 016_unrated_battle_pr.sql does. A test in
-- wows-toolkit-config checks the two agree.
UPDATE indexed_vehicle SET mc = NULL, mc_torpedoes_observed = 0
WHERE (mc IS NOT NULL OR mc_torpedoes_observed <> 0) AND arena_id IN (
  SELECT arena_id FROM indexed_match
  WHERE NOT (
    game_type IN ('RandomBattle', 'RankedBattle', 'OldRankedBattle')
    OR (game_type = '' AND match_group IN ('pvp', 'ranked'))
  )
);
