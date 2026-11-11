-- The workspaces under their own name: the table `teams` becomes `workspaces`, and a
-- column that names a workspace is `workspace_id`. What is stored stays as it is; the
-- names of what was named after a team (indexes) and the member of a board in the trash
-- that names its workspace change with it.
--
-- SQLite renames a table and a column in place, and rewrites the keys of the other
-- tables that refer to them; the key of a table's own index is renamed with the table.
-- It renames no other index: those are made again under their new names.

ALTER TABLE teams RENAME TO workspaces;
ALTER TABLE boards RENAME COLUMN team_id TO workspace_id;
ALTER TABLE deleted_boards RENAME COLUMN team_id TO workspace_id;
ALTER TABLE categories RENAME COLUMN team_id TO workspace_id;

DROP INDEX teams_org_id_idx;
CREATE INDEX workspaces_org_id_idx ON workspaces (org_id);
DROP INDEX boards_team_id_is_template_idx;
CREATE INDEX boards_workspace_id_is_template_idx ON boards (workspace_id, is_template);
DROP INDEX categories_user_id_team_id_idx;
CREATE INDEX categories_user_id_workspace_id_idx ON categories (user_id, workspace_id);
DROP INDEX deleted_boards_team_id_idx;
CREATE INDEX deleted_boards_workspace_id_idx ON deleted_boards (workspace_id, deleted_at);

-- A board in the trash is kept as its JSON form, whose member that names the workspace
-- is `workspaceId` from now on.
UPDATE deleted_boards
SET board = json_set(json_remove(board, '$.teamId'), '$.workspaceId', json_extract(board, '$.teamId'))
WHERE json_type(board, '$.teamId') IS NOT NULL;
