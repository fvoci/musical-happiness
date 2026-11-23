-- The record of an organization administrator removing a member from the organization
-- (`org_member_removed`): of the organization, of no workspace. SQLite changes no check
-- in place, so the table is made again with every column, index, and trigger it has,
-- keeping the identity-erasure guard of 0044_audit_identity_erasure.sql.
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
        'org_admin_granted', 'org_admin_removed', 'org_member_removed',
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
        'org_admin_granted', 'org_admin_removed', 'org_member_removed',
        'workspace_created', 'workspace_renamed', 'workspace_deleted', 'workspace_restored',
        'public_links_disabled', 'public_links_enabled',
        'oidc_provider_configured', 'oidc_provider_removed', 'sso_required', 'sso_released',
        'oidc_provider_pending_saved', 'oidc_provider_pending_applied',
        'oidc_provider_pending_discarded', 'password_reset_link_made',
        'password_set_by_operator',
        'oidc_former_provider_kept', 'oidc_former_provider_ended',
        'oidc_former_provider_lapsed', 'oidc_identity_bound',
        'oidc_identity_carried_over', 'email_verified_by_operator'))
);
INSERT INTO audit_log_new (id, org_id, workspace_id, actor_id, actor_kind, action, target_id,
    subject_id, create_at, detail, identity)
SELECT id, org_id, workspace_id, actor_id, actor_kind, action, target_id,
    subject_id, create_at, detail, identity
FROM audit_log;
DROP TABLE audit_log;
ALTER TABLE audit_log_new RENAME TO audit_log;
CREATE INDEX audit_log_workspace_id_idx ON audit_log (workspace_id, create_at);
CREATE INDEX audit_log_org_idx ON audit_log (org_id, create_at) WHERE workspace_id IS NULL;

-- Who an identity tied by a record is, is taken out of the records about a person
-- whose account is being erased, and nothing else of a record is ever changed: the
-- same as schema/sqlite/0044_audit_identity_erasure.sql. `IS` compares a NULL as a value.
CREATE TRIGGER audit_log_unchanged BEFORE UPDATE ON audit_log
WHEN NOT (
    NEW.identity IS NULL
    AND NEW.id IS OLD.id AND NEW.org_id IS OLD.org_id
    AND NEW.workspace_id IS OLD.workspace_id AND NEW.actor_id IS OLD.actor_id
    AND NEW.actor_kind IS OLD.actor_kind AND NEW.action IS OLD.action
    AND NEW.target_id IS OLD.target_id AND NEW.subject_id IS OLD.subject_id
    AND NEW.create_at IS OLD.create_at AND NEW.detail IS OLD.detail
    AND EXISTS (SELECT 1 FROM users
                WHERE id IN (OLD.actor_id, OLD.subject_id) AND erase_started_at > 0))
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
