-- What an organization's administrators manage: whether its boards may be shared by a
-- public link, the names of its workspaces, the trash of its workspaces, and the
-- audit log that their stepping into a workspace, and what is changed of the
-- organization, is recorded in.

-- Whether a board of the organization may be read by a public link. Off, every link of
-- it is closed and no new one is made; on again, the links made before open again.
ALTER TABLE organizations ADD COLUMN public_links BOOLEAN NOT NULL DEFAULT true;

-- A name is one workspace's among those of the organization in use: one in the trash
-- leaves its name to be taken. An empty name is no name. Names stored twice before
-- this rule keep the oldest workspace's; the others get their identifier after it.
UPDATE workspaces SET name = name || ' (' || id || ')'
WHERE delete_at = 0 AND name <> '' AND EXISTS (
    SELECT 1 FROM workspaces AS older
    WHERE older.org_id = workspaces.org_id AND older.name = workspaces.name
      AND older.delete_at = 0
      AND (older.create_at < workspaces.create_at
           OR (older.create_at = workspaces.create_at AND older.id < workspaces.id)));
CREATE UNIQUE INDEX workspaces_name_in_use_key ON workspaces (org_id, name)
    WHERE delete_at = 0 AND name <> '';
-- The workspaces in the trash, which the housekeeping looks through.
CREATE INDEX workspaces_deleted_idx ON workspaces (delete_at) WHERE delete_at > 0;

-- The roles on the workspace's boards an administrator of the organization held when
-- they joined it as its administrator (workspace_members.via_org_admin), as JSON: they
-- have those again, and no other, when the joining ends. NULL while not joined so.
ALTER TABLE workspace_members ADD COLUMN prior_board_roles TEXT;

-- The audit log records, besides what is done in a workspace, what is done to the
-- organization (no workspace), and whom a thing was done to besides what it was
-- done to (subject_id: a user, kept when the account goes). SQLite changes no
-- constraint of a table in place: the table is made again.
CREATE TABLE audit_log_new (
    id TEXT NOT NULL PRIMARY KEY,
    org_id TEXT NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    workspace_id TEXT REFERENCES workspaces (id) ON DELETE CASCADE,
    actor_id TEXT NOT NULL,
    action TEXT NOT NULL CHECK (action IN (
        'self_added_to_board', 'added_to_board',
        'org_admin_joined', 'org_admin_left', 'org_admin_revoked',
        'org_admin_granted', 'org_admin_removed',
        'workspace_created', 'workspace_renamed', 'workspace_deleted', 'workspace_restored',
        'public_links_disabled', 'public_links_enabled')),
    target_id TEXT NOT NULL,
    subject_id TEXT,
    create_at BIGINT NOT NULL,
    -- A record of a workspace is of one, and one of the organization of none but its
    -- own.
    CHECK (workspace_id IS NOT NULL OR action IN (
        'org_admin_granted', 'org_admin_removed', 'workspace_created', 'workspace_renamed',
        'workspace_deleted', 'workspace_restored', 'public_links_disabled',
        'public_links_enabled'))
);
INSERT INTO audit_log_new (id, org_id, workspace_id, actor_id, action, target_id, create_at)
SELECT id, org_id, workspace_id, actor_id, action, target_id, create_at FROM audit_log;
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
