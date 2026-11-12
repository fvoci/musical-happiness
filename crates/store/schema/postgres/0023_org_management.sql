-- What an organization's administrators manage: whether its boards may be shared by a
-- public link, the names of its workspaces, the trash of its workspaces, and the
-- audit log that their stepping into a workspace, and what is changed of the
-- organization, is recorded in.
SELECT set_config('fvoci.system', 'on', true);

-- Whether a board of the organization may be read by a public link. Off, every link of
-- it is closed and no new one is made; on again, the links made before open again.
ALTER TABLE organizations ADD COLUMN public_links BOOLEAN NOT NULL DEFAULT true;

-- A name is one workspace's among those of the organization in use: one in the trash
-- leaves its name to be taken. An empty name is no name. Names stored twice before
-- this rule keep the oldest workspace's; the others get their identifier after it.
UPDATE workspaces AS t SET name = t.name || ' (' || t.id::text || ')'
WHERE t.delete_at = 0 AND t.name <> '' AND EXISTS (
    SELECT 1 FROM workspaces AS older
    WHERE older.org_id = t.org_id AND older.name = t.name AND older.delete_at = 0
      AND (older.create_at, older.id) < (t.create_at, t.id));
CREATE UNIQUE INDEX workspaces_name_in_use_key ON workspaces (org_id, name)
    WHERE delete_at = 0 AND name <> '';
-- The workspaces in the trash, which the housekeeping looks through.
CREATE INDEX workspaces_deleted_idx ON workspaces (delete_at) WHERE delete_at > 0;

-- The audit log records, besides what is done in a workspace, what is done to the
-- organization (no workspace), and whom a thing was done to besides what it was
-- done to (subject_id: a user, kept when the account goes).
ALTER TABLE audit_log ALTER COLUMN workspace_id DROP NOT NULL;
ALTER TABLE audit_log ADD COLUMN subject_id UUID;
ALTER TABLE audit_log DROP CONSTRAINT audit_log_action_check;
ALTER TABLE audit_log ADD CONSTRAINT audit_log_action_check CHECK (action IN (
    'self_added_to_board', 'added_to_board',
    'org_admin_joined', 'org_admin_left', 'org_admin_revoked',
    'org_admin_granted', 'org_admin_removed',
    'workspace_created', 'workspace_renamed', 'workspace_deleted', 'workspace_restored',
    'public_links_disabled', 'public_links_enabled'));
-- A record of a workspace is of one, and one of the organization of none but its own.
ALTER TABLE audit_log ADD CONSTRAINT audit_log_place_check CHECK (
    workspace_id IS NOT NULL OR action IN (
        'org_admin_granted', 'org_admin_removed', 'workspace_created', 'workspace_renamed',
        'workspace_deleted', 'workspace_restored', 'public_links_disabled',
        'public_links_enabled'));
CREATE INDEX audit_log_org_idx ON audit_log (org_id, create_at) WHERE workspace_id IS NULL;

-- A record is never changed, and goes only with its workspace when that is removed
-- for good: by then the workspace is not there. A record of the organization stays.
CREATE FUNCTION audit_log_is_kept() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' AND OLD.workspace_id IS NOT NULL
       AND NOT EXISTS (SELECT 1 FROM workspaces WHERE id = OLD.workspace_id) THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'a record of the audit log is not changed or removed'
        USING ERRCODE = 'restrict_violation';
END $$;
CREATE TRIGGER audit_log_kept BEFORE UPDATE OR DELETE ON audit_log
    FOR EACH ROW EXECUTE FUNCTION audit_log_is_kept();
