-- A board or a wiki is a container: the tables and the columns that hold one or name
-- one are named after it. What is stored stays as it is, and so do the kind of a
-- container (`kind`, 'board' or 'wiki') and the type of the legacy board block; the
-- names of what was named after a board (constraints, indexes, a trigger and its
-- function) change with it, and so do the bodies of the functions that name the
-- tables, which are read when their triggers run.
--
-- The rows changed below are of organizations: the system's reach is needed for the
-- updates to find them.
SELECT set_config('fvoci.system', 'on', true);

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

-- The constraints named after a board: by what the catalog has, as a database made by
-- PostgreSQL 18 has a named constraint for every NOT NULL column and an older one has
-- none. A constraint that has an index (a key) renames its index too.
DO $$
DECLARE
    found RECORD;
BEGIN
    FOR found IN
        SELECT c.conrelid::regclass AS tbl, c.conname AS name
        FROM pg_constraint AS c
        WHERE c.connamespace = current_schema()::regnamespace AND c.conname LIKE '%board%'
    LOOP
        EXECUTE format('ALTER TABLE %s RENAME CONSTRAINT %I TO %I', found.tbl, found.name,
                       replace(found.name, 'board', 'container'));
    END LOOP;
END $$;

-- An index keeps the name a column had when the index was made: one on a renamed
-- column is made again. The index of a key keeps it, as no statement reads it there
-- (the definition of an index reads the table's columns), and is renamed with its key.
DROP INDEX blocks_board_id_parent_id_idx;
CREATE INDEX blocks_container_id_parent_id_idx ON blocks (container_id, parent_id);
DROP INDEX deleted_blocks_board_id_idx;
CREATE INDEX deleted_blocks_container_id_idx ON deleted_blocks (container_id);
DROP INDEX category_boards_board_id_idx;
CREATE INDEX category_containers_container_id_idx ON category_containers (container_id);
DROP INDEX file_refs_board_id_name_idx;
CREATE INDEX file_refs_container_id_name_idx ON file_refs (container_id, name);

-- The other indexes named after a board.
DO $$
DECLARE
    found RECORD;
BEGIN
    FOR found IN
        SELECT c.relname AS name
        FROM pg_class AS c
        WHERE c.relnamespace = current_schema()::regnamespace AND c.relkind = 'i'
            AND c.relname LIKE '%board%'
    LOOP
        EXECUTE format('ALTER INDEX %I RENAME TO %I', found.name,
                       replace(found.name, 'board', 'container'));
    END LOOP;
END $$;

-- The checks of a row's container name the tables and the columns in their bodies,
-- which are read when the triggers run: the bodies are written again with the new
-- names, the second under the new name of its trigger.
CREATE OR REPLACE FUNCTION uploads_target_of_its_organization() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    -- A row of no organization is refused by its column, which takes no NULL.
    IF NEW.org_id IS NULL THEN
        RETURN NEW;
    END IF;
    IF NEW.purpose = 'import' THEN
        IF NOT EXISTS (
            SELECT 1 FROM workspaces WHERE id = NEW.container_id AND org_id = NEW.org_id
        ) THEN
            RAISE foreign_key_violation USING
                MESSAGE = 'the workspace of an import is not one of its organization';
        END IF;
    ELSIF NOT EXISTS (
        SELECT 1 FROM containers WHERE id = NEW.container_id AND org_id = NEW.org_id
    ) THEN
        RAISE foreign_key_violation USING
            MESSAGE = 'the board of an upload is not one of its organization';
    END IF;
    RETURN NEW;
END $$;

ALTER TRIGGER category_boards_board_of_its_organization ON category_containers
    RENAME TO category_containers_container_of_its_organization;
ALTER FUNCTION category_boards_board_of_its_organization()
    RENAME TO category_containers_container_of_its_organization;
CREATE OR REPLACE FUNCTION category_containers_container_of_its_organization()
RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    -- A row of no organization is refused by its column, which takes no NULL.
    IF NEW.org_id IS NULL THEN
        RETURN NEW;
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM containers WHERE id = NEW.container_id
        AND org_id IN (NEW.org_id, '00000000-0000-7000-8000-00000000a000')
    ) THEN
        RAISE foreign_key_violation USING
            MESSAGE = 'the board of a category is neither of its organization nor a template';
    END IF;
    RETURN NEW;
END $$;

-- A row of what the search server is still to be told that is about every document of
-- a container says 'container'.
ALTER TABLE search_outbox DROP CONSTRAINT search_outbox_scope_check;
UPDATE search_outbox SET scope = 'container' WHERE scope = 'board';
ALTER TABLE search_outbox ADD CONSTRAINT search_outbox_scope_check
    CHECK (scope IN ('block', 'container'));

-- A record of somebody put on a board or a wiki says so of the container
-- ('self_added_to_container', 'added_to_container'), the records kept so far too. A
-- record is never changed but here: its guard stands aside for this one statement.
ALTER TABLE audit_log DROP CONSTRAINT audit_log_action_check;
ALTER TABLE audit_log DISABLE TRIGGER audit_log_kept;
UPDATE audit_log
SET action = CASE action
    WHEN 'self_added_to_board' THEN 'self_added_to_container'
    ELSE 'added_to_container'
END
WHERE action IN ('self_added_to_board', 'added_to_board');
ALTER TABLE audit_log ENABLE TRIGGER audit_log_kept;
ALTER TABLE audit_log ADD CONSTRAINT audit_log_action_check CHECK (action IN (
    'self_added_to_container', 'added_to_container',
    'org_admin_joined', 'org_admin_left', 'org_admin_revoked',
    'org_admin_granted', 'org_admin_removed',
    'workspace_created', 'workspace_renamed', 'workspace_deleted', 'workspace_restored',
    'public_links_disabled', 'public_links_enabled',
    'oidc_provider_configured', 'oidc_provider_removed', 'sso_required', 'sso_released',
    'oidc_provider_pending_saved', 'oidc_provider_pending_applied',
    'oidc_provider_pending_discarded', 'password_reset_link_made',
    'password_set_by_operator',
    'oidc_former_provider_kept', 'oidc_former_provider_ended',
    'oidc_former_provider_lapsed', 'oidc_identity_bound', 'oidc_identity_carried_over',
    'email_verified_by_operator'));

-- What the trash keeps as JSON names a block's and a member's board or wiki
-- `containerId`, and a file's `container_id`, as the model does from now on. The JSON
-- is rewritten as text, never parsed: PostgreSQL's `jsonb` and `json_each` refuse a
-- `\u0000` that a `json` value keeps. A member and a file are flat objects, so every
-- member so named is theirs. A block's own comes after its `fields` and `body` as the
-- server writes a block and as `jsonb` orders its members, so it is the last one; a
-- block that a data step wrote again has its members in the order of their names, so
-- its own is the first.
UPDATE deleted_blocks
SET block = (CASE
    WHEN block::text ~ '^\s*\{\s*"boardId"\s*:'
        THEN regexp_replace(block::text, '^(\s*\{\s*)"boardId"(\s*):', '\1"containerId"\2:')
    ELSE regexp_replace(block::text, '^(.*)"boardId"(\s*):', '\1"containerId"\2:')
END)::json
WHERE block::text ~ '"boardId"\s*:';
UPDATE deleted_containers
SET members = regexp_replace(members::text, '"boardId"(\s*):', '"containerId"\1:', 'g')::json
WHERE members::text ~ '"boardId"\s*:';
UPDATE deleted_containers
SET files = regexp_replace(files::text, '"board_id"(\s*):', '"container_id"\1:', 'g')::json
WHERE files::text ~ '"board_id"\s*:';
