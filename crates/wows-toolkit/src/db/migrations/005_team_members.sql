-- Add team_members column to session_stats for storing allied player stats per game.
ALTER TABLE session_stats ADD COLUMN team_members TEXT NOT NULL DEFAULT '[]';
