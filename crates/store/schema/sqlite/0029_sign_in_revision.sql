-- Which state of a provider's sign-in a session proves. `oidc_providers.revision` is
-- raised by every change of what signing in through the provider takes (its issuer,
-- client, secret, and whether it is on), and kept by the others; a session made
-- through the provider keeps the revision it was made at, and shows the provider lets
-- people in at that one only. NULL for a session made with a password, and for one
-- made before this migration, which proves no revision.
ALTER TABLE oidc_providers ADD COLUMN revision BIGINT NOT NULL DEFAULT 1;
ALTER TABLE sessions ADD COLUMN login_revision BIGINT;

-- Who a session made through a provider was made for: the subject the provider gave,
-- beside the issuer of 0028. Unlinking an identity ends the sessions made for that
-- identity, issuer and subject, and no other. NULL for a session made with a
-- password, and for one made before this migration, which knows its issuer only:
-- unlinking an identity of that issuer ends it.
ALTER TABLE sessions ADD COLUMN login_subject TEXT;

-- Only a session made through a provider names a subject or a revision, as
-- PostgreSQL's checks say; SQLite adds no check to a table in place.
CREATE TRIGGER sessions_login_subject_insert BEFORE INSERT ON sessions
WHEN (NEW.login_subject IS NOT NULL OR NEW.login_revision IS NOT NULL)
    AND NEW.login_issuer IS NULL
BEGIN
    SELECT RAISE(ABORT, 'only a session made through a provider names a subject or a revision');
END;
CREATE TRIGGER sessions_login_subject_update
BEFORE UPDATE OF login_subject, login_revision, login_issuer ON sessions
WHEN (NEW.login_subject IS NOT NULL OR NEW.login_revision IS NOT NULL)
    AND NEW.login_issuer IS NULL
BEGIN
    SELECT RAISE(ABORT, 'only a session made through a provider names a subject or a revision');
END;

-- The pending settings of a provider, and the sign-ins that try them: the same as
-- schema/postgres/0029_sign_in_revision.sql, which says what they are, without the wall.
CREATE TABLE oidc_pending_providers (
    org_id TEXT NOT NULL PRIMARY KEY REFERENCES organizations (id) ON DELETE CASCADE,
    provider_id TEXT NOT NULL REFERENCES oidc_providers (id) ON DELETE CASCADE,
    issuer TEXT NOT NULL,
    client_id TEXT NOT NULL,
    client_secret TEXT NOT NULL,
    create_at BIGINT NOT NULL
);

ALTER TABLE oidc_login_flows ADD COLUMN pending_at BIGINT;
ALTER TABLE oidc_login_flows ADD COLUMN pending_by TEXT;

-- The records of the pending settings, and of the server operator setting a member's
-- password or making a link to set it, as schema/postgres/0029_sign_in_revision.sql
-- says: SQLite changes no check in place, so the table is made again, as in 0027,
-- with its triggers.
CREATE TABLE audit_log_new (
    id TEXT NOT NULL PRIMARY KEY,
    org_id TEXT NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    workspace_id TEXT REFERENCES workspaces (id) ON DELETE CASCADE,
    actor_id TEXT,
    actor_kind TEXT NOT NULL DEFAULT 'person' CHECK (actor_kind IN ('person', 'server_operator')),
    action TEXT NOT NULL CHECK (action IN (
        'self_added_to_board', 'added_to_board',
        'org_admin_joined', 'org_admin_left', 'org_admin_revoked',
        'org_admin_granted', 'org_admin_removed',
        'workspace_created', 'workspace_renamed', 'workspace_deleted', 'workspace_restored',
        'public_links_disabled', 'public_links_enabled',
        'oidc_provider_configured', 'oidc_provider_removed', 'sso_required', 'sso_released',
        'oidc_provider_pending_saved', 'oidc_provider_pending_applied',
        'oidc_provider_pending_discarded', 'password_reset_link_made',
        'password_set_by_operator')),
    target_id TEXT NOT NULL,
    subject_id TEXT,
    create_at BIGINT NOT NULL,
    CHECK ((actor_kind = 'person' AND actor_id IS NOT NULL)
        OR (actor_kind = 'server_operator' AND actor_id IS NULL)),
    -- A record of a workspace is of one, and one of the organization of none but its
    -- own.
    CHECK (workspace_id IS NOT NULL OR action IN (
        'org_admin_granted', 'org_admin_removed', 'workspace_created', 'workspace_renamed',
        'workspace_deleted', 'workspace_restored', 'public_links_disabled',
        'public_links_enabled', 'oidc_provider_configured', 'oidc_provider_removed',
        'sso_required', 'sso_released', 'oidc_provider_pending_saved',
        'oidc_provider_pending_applied', 'oidc_provider_pending_discarded',
        'password_reset_link_made', 'password_set_by_operator'))
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
