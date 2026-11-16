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

-- A record of somebody put on a board or a wiki says so of the container
-- ('self_added_to_container', 'added_to_container'), the records kept so far too.
-- SQLite changes no check in place, and a record is never changed: the table is made
-- again, as in 0039, with every column, index and trigger it has.
CREATE TABLE audit_log_new (
    id TEXT NOT NULL PRIMARY KEY,
    org_id TEXT NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    workspace_id TEXT REFERENCES workspaces (id) ON DELETE CASCADE,
    actor_id TEXT,
    actor_kind TEXT NOT NULL DEFAULT 'person'
        CHECK (actor_kind IN ('person', 'server_operator', 'server')),
    action TEXT NOT NULL CHECK (action IN (
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
        'oidc_former_provider_lapsed', 'oidc_identity_bound',
        'oidc_identity_carried_over', 'email_verified_by_operator')),
    target_id TEXT NOT NULL,
    subject_id TEXT,
    create_at BIGINT NOT NULL,
    detail TEXT,
    identity TEXT,
    CHECK ((actor_kind = 'person' AND actor_id IS NOT NULL)
        OR (actor_kind IN ('server_operator', 'server') AND actor_id IS NULL)),
    -- A record of a workspace is of one, and one of the organization of none but its
    -- own.
    CHECK (workspace_id IS NOT NULL OR action IN (
        'org_admin_granted', 'org_admin_removed', 'workspace_created', 'workspace_renamed',
        'workspace_deleted', 'workspace_restored', 'public_links_disabled',
        'public_links_enabled', 'oidc_provider_configured', 'oidc_provider_removed',
        'sso_required', 'sso_released', 'oidc_provider_pending_saved',
        'oidc_provider_pending_applied', 'oidc_provider_pending_discarded',
        'password_reset_link_made', 'password_set_by_operator',
        'oidc_former_provider_kept', 'oidc_former_provider_ended',
        'oidc_former_provider_lapsed', 'oidc_identity_bound',
        'oidc_identity_carried_over', 'email_verified_by_operator'))
);
INSERT INTO audit_log_new (id, org_id, workspace_id, actor_id, actor_kind, action, target_id,
    subject_id, create_at, detail, identity)
SELECT id, org_id, workspace_id, actor_id, actor_kind,
    CASE action
        WHEN 'self_added_to_board' THEN 'self_added_to_container'
        WHEN 'added_to_board' THEN 'added_to_container'
        ELSE action
    END,
    target_id, subject_id, create_at, detail, identity
FROM audit_log;
DROP TABLE audit_log;
ALTER TABLE audit_log_new RENAME TO audit_log;
CREATE INDEX audit_log_workspace_id_idx ON audit_log (workspace_id, create_at);
CREATE INDEX audit_log_org_idx ON audit_log (org_id, create_at) WHERE workspace_id IS NULL;

-- A record is never changed, and goes only with its workspace when that is removed
-- for good, or with its organization: by then that is not there.
CREATE TRIGGER audit_log_unchanged BEFORE UPDATE ON audit_log
BEGIN
    SELECT RAISE(ABORT, 'a record of the audit log is not changed or removed');
END;
CREATE TRIGGER audit_log_kept BEFORE DELETE ON audit_log
WHEN EXISTS (SELECT 1 FROM organizations WHERE id = OLD.org_id)
    AND (OLD.workspace_id IS NULL
         OR EXISTS (SELECT 1 FROM workspaces WHERE id = OLD.workspace_id))
BEGIN
    SELECT RAISE(ABORT, 'a record of the audit log is not changed or removed');
END;

-- What the trash keeps as JSON names a block's and a member's board or wiki
-- `containerId`, and a file's `container_id`, as the model does from now on.
UPDATE deleted_blocks
SET block = json_set(json_remove(block, '$.boardId'), '$.containerId',
    json_extract(block, '$.boardId'))
WHERE json_type(block, '$.boardId') IS NOT NULL;
UPDATE deleted_containers
SET members = (
    SELECT json_group_array(CASE
        WHEN json_type(m.value, '$.boardId') IS NULL THEN json(m.value)
        ELSE json_set(json_remove(m.value, '$.boardId'), '$.containerId',
            json_extract(m.value, '$.boardId'))
    END)
    FROM json_each(deleted_containers.members) AS m
)
WHERE EXISTS (
    SELECT 1 FROM json_each(deleted_containers.members) AS m
    WHERE json_type(m.value, '$.boardId') IS NOT NULL
);
UPDATE deleted_containers
SET files = (
    SELECT json_group_array(CASE
        WHEN json_type(f.value, '$.board_id') IS NULL THEN json(f.value)
        ELSE json_set(json_remove(f.value, '$.board_id'), '$.container_id',
            json_extract(f.value, '$.board_id'))
    END)
    FROM json_each(deleted_containers.files) AS f
)
WHERE EXISTS (
    SELECT 1 FROM json_each(deleted_containers.files) AS f
    WHERE json_type(f.value, '$.board_id') IS NOT NULL
);
