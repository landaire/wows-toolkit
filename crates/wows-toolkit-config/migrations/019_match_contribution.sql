ALTER TABLE indexed_vehicle RENAME COLUMN bi TO mc;
ALTER TABLE indexed_vehicle RENAME COLUMN bi_version TO mc_version;
ALTER TABLE indexed_vehicle RENAME COLUMN bi_torpedoes_observed TO mc_torpedoes_observed;
ALTER TABLE session_stats RENAME COLUMN battle_impact TO match_contribution;
ALTER TABLE session_stats RENAME COLUMN battle_impact_version TO match_contribution_version;
