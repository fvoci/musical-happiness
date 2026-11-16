-- A board or a wiki is a container: the tables and the columns that hold one or name
-- one are named after it. What is stored stays as it is, and so do the kind of a
-- container (`kind`, 'board' or 'wiki') and the type of the legacy board block.
--
-- SQLite renames a table and a column in place, and rewrites the keys, the checks and
-- the indexes that refer to them; the key of a table's own index is renamed with the
-- table. It renames no other index: those are made again under their new names.

ALTER TABLE boards RENAME TO containers;
ALTER TABLE board_members RENAME TO container_members;
ALTER TABLE board_teams RENAME TO container_teams;
ALTER TABLE category_boards RENAME TO category_containers;
ALTER TABLE deleted_boards RENAME TO deleted_containers;

ALTER TABLE blocks RENAME COLUMN board_id TO container_id;
ALTER TABLE body_links RENAME COLUMN board_id TO container_id;
ALTER TABLE category_containers RENAME COLUMN board_id TO container_id;
ALTER TABLE container_members RENAME COLUMN board_id TO container_id;
ALTER TABLE container_teams RENAME COLUMN board_id TO container_id;
ALTER TABLE deleted_blocks RENAME COLUMN board_id TO container_id;
ALTER TABLE file_info RENAME COLUMN board_id TO container_id;
ALTER TABLE file_refs RENAME COLUMN board_id TO container_id;
ALTER TABLE invitations RENAME COLUMN board_id TO container_id;
ALTER TABLE invitations RENAME COLUMN board_role TO container_role;
ALTER TABLE sharing RENAME COLUMN board_id TO container_id;
ALTER TABLE uploads RENAME COLUMN board_id TO container_id;
ALTER TABLE deleted_containers RENAME COLUMN board TO container;
ALTER TABLE workspace_members RENAME COLUMN prior_board_roles TO prior_container_roles;

DROP INDEX blocks_board_id_parent_id_idx;
CREATE INDEX blocks_container_id_parent_id_idx ON blocks (container_id, parent_id);
DROP INDEX board_members_user_id_idx;
CREATE INDEX container_members_user_id_idx ON container_members (user_id);
DROP INDEX board_teams_team_idx;
CREATE INDEX container_teams_team_idx ON container_teams (team_id, org_id);
DROP INDEX boards_workspace_id_is_template_idx;
CREATE INDEX containers_workspace_id_is_template_idx ON containers (workspace_id, is_template);
DROP INDEX boards_workspace_scope_key;
CREATE UNIQUE INDEX containers_workspace_scope_key ON containers (id, workspace_id);
DROP INDEX category_boards_board_id_idx;
CREATE INDEX category_containers_container_id_idx ON category_containers (container_id);
DROP INDEX category_boards_category_id_idx;
CREATE INDEX category_containers_category_id_idx ON category_containers (category_id);
DROP INDEX deleted_blocks_board_id_idx;
CREATE INDEX deleted_blocks_container_id_idx ON deleted_blocks (container_id);
DROP INDEX deleted_boards_deleted_at_idx;
CREATE INDEX deleted_containers_deleted_at_idx ON deleted_containers (deleted_at);
DROP INDEX deleted_boards_workspace_id_idx;
CREATE INDEX deleted_containers_workspace_id_idx ON deleted_containers (workspace_id, deleted_at);
DROP INDEX file_refs_board_id_name_idx;
CREATE INDEX file_refs_container_id_name_idx ON file_refs (container_id, name);

-- A row of what the search server is still to be told that is about every document of
-- a container says 'container'. SQLite changes no check of a table: the table is made
-- again, as in 0038, with its index.
CREATE TABLE search_outbox_new (
    -- 'block': the document of the block `id` is to be put, or removed if the block
    -- has none now. 'container': every document of the container `id` is, those of
    -- blocks that are gone too.
    scope TEXT NOT NULL CHECK (scope IN ('block', 'container')),
    id TEXT NOT NULL,
    -- The organization of the block or container when the row was written: what is
    -- gone from its table by the time the row is read is still removed as that
    -- organization's. Every statement that writes a row names it.
    org_id TEXT NOT NULL,
    -- Moved on by every change of what the row is about. Who sent what was stored
    -- takes the row away only if it still has the `seq` they read.
    seq BIGINT NOT NULL,
    -- When the row is next looked at: at once, or later after a failure.
    due_at BIGINT NOT NULL,
    -- How many times in a row it could not be sent.
    attempts BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (org_id, scope, id)
);
INSERT INTO search_outbox_new (scope, id, org_id, seq, due_at, attempts)
SELECT CASE scope WHEN 'board' THEN 'container' ELSE scope END, id, org_id, seq, due_at,
    attempts
FROM search_outbox;
DROP TABLE search_outbox;
ALTER TABLE search_outbox_new RENAME TO search_outbox;
CREATE INDEX search_outbox_due_at_idx ON search_outbox (due_at);
