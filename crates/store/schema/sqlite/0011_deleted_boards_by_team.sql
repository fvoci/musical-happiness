-- The deleted boards of a team are listed, the last deleted first.
CREATE INDEX deleted_boards_team_id_idx ON deleted_boards (team_id, deleted_at);
