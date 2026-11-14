-- Moving an organization from one provider to another: the same as
-- schema/postgres/0031_provider_move.sql, which says what it is, without the wall.
CREATE TABLE oidc_former_providers (
    org_id TEXT NOT NULL PRIMARY KEY REFERENCES organizations (id) ON DELETE CASCADE,
    issuer TEXT NOT NULL,
    client_id TEXT NOT NULL,
    client_secret TEXT NOT NULL,
    create_at BIGINT NOT NULL,
    expires_at BIGINT NOT NULL
);
CREATE INDEX oidc_former_providers_expires_at_idx ON oidc_former_providers (expires_at);

-- The records of a move, their `detail`, and the server as an actor: SQLite changes no
-- check in place, so the table is made again, as in 0029, with its triggers.
CREATE TABLE audit_log_new (
    id TEXT NOT NULL PRIMARY KEY,
    org_id TEXT NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    workspace_id TEXT REFERENCES workspaces (id) ON DELETE CASCADE,
    actor_id TEXT,
    actor_kind TEXT NOT NULL DEFAULT 'person'
        CHECK (actor_kind IN ('person', 'server_operator', 'server')),
    action TEXT NOT NULL CHECK (action IN (
        'self_added_to_board', 'added_to_board',
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
        'oidc_identity_carried_over')),
    target_id TEXT NOT NULL,
    subject_id TEXT,
    create_at BIGINT NOT NULL,
    detail TEXT,
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
        'oidc_identity_carried_over'))
);
INSERT INTO audit_log_new (id, org_id, workspace_id, actor_id, actor_kind, action, target_id,
    subject_id, create_at)
SELECT id, org_id, workspace_id, actor_id, actor_kind, action, target_id, subject_id, create_at
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
