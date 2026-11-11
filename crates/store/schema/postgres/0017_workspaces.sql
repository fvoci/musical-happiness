-- The workspaces under their own name: the table `teams` becomes `workspaces`, and a
-- column that names a workspace is `workspace_id`. What is stored stays as it is; the
-- names of what was named after a team (constraints, indexes, a function's body) and
-- the member of a board in the trash that names its workspace change with it.
--
-- The board in the trash is a row of an organization: the system's reach is needed for
-- the update below to find it.
SELECT set_config('fvoci.system', 'on', true);

ALTER TABLE teams RENAME TO workspaces;
ALTER TABLE boards RENAME COLUMN team_id TO workspace_id;
ALTER TABLE deleted_boards RENAME COLUMN team_id TO workspace_id;
ALTER TABLE categories RENAME COLUMN team_id TO workspace_id;

-- The constraints named after a team: by what the catalog has, as a database made by
-- PostgreSQL 18 has a named constraint for every NOT NULL column and an older one has
-- none. A constraint that has an index (a key) renames its index too.
DO $$
DECLARE
    found RECORD;
BEGIN
    FOR found IN
        SELECT c.conrelid::regclass AS tbl, c.conname AS name
        FROM pg_constraint AS c
        WHERE c.connamespace = current_schema()::regnamespace AND c.conname LIKE '%team%'
    LOOP
        EXECUTE format('ALTER TABLE %s RENAME CONSTRAINT %I TO %I', found.tbl, found.name,
                       replace(replace(found.name, 'teams_', 'workspaces_'), 'team_', 'workspace_'));
    END LOOP;
END $$;

-- The other indexes named after a team. An index keeps the name a column had when the
-- index was made, so one on a renamed column is made again.
ALTER INDEX teams_org_id_idx RENAME TO workspaces_org_id_idx;
DROP INDEX boards_team_id_is_template_idx;
CREATE INDEX boards_workspace_id_is_template_idx ON boards (workspace_id, is_template);
DROP INDEX categories_user_id_team_id_idx;
CREATE INDEX categories_user_id_workspace_id_idx ON categories (user_id, workspace_id);
DROP INDEX deleted_boards_team_id_idx;
CREATE INDEX deleted_boards_workspace_id_idx ON deleted_boards (workspace_id, deleted_at);

-- The check of an upload's target names the table in its body, which is read when the
-- trigger runs: the body is written again with the new name.
CREATE OR REPLACE FUNCTION uploads_target_of_its_organization() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    -- A row of no organization is refused by its column, which takes no NULL.
    IF NEW.org_id IS NULL THEN
        RETURN NEW;
    END IF;
    IF NEW.purpose = 'import' THEN
        IF NOT EXISTS (
            SELECT 1 FROM workspaces WHERE id = NEW.board_id AND org_id = NEW.org_id
        ) THEN
            RAISE foreign_key_violation USING
                MESSAGE = 'the workspace of an import is not one of its organization';
        END IF;
    ELSIF NOT EXISTS (SELECT 1 FROM boards WHERE id = NEW.board_id AND org_id = NEW.org_id) THEN
        RAISE foreign_key_violation USING
            MESSAGE = 'the board of an upload is not one of its organization';
    END IF;
    RETURN NEW;
END $$;

-- A board in the trash is kept as its JSON form, whose member that names the workspace
-- is `workspaceId` from now on.
UPDATE deleted_boards
SET board = ((board::jsonb - 'teamId')
             || jsonb_build_object('workspaceId', board::jsonb -> 'teamId'))::json
WHERE board::jsonb ? 'teamId';
