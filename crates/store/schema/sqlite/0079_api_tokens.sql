-- Personal API tokens, as schema/postgres/0079_api_tokens.sql says.

CREATE TABLE api_tokens (
    id TEXT NOT NULL PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
    token_prefix TEXT NOT NULL,
    token_hash TEXT NOT NULL,
    scope TEXT NOT NULL CHECK (scope IN ('read', 'write')),
    allow_personal_space BOOLEAN NOT NULL DEFAULT FALSE,
    notify_self BOOLEAN NOT NULL DEFAULT TRUE,
    origin_provider_id TEXT,
    origin_provider_revision BIGINT,
    last_used_at BIGINT,
    last_used_address TEXT,
    created_at BIGINT NOT NULL,
    expires_at BIGINT NOT NULL CHECK (expires_at > created_at),
    revoked_at BIGINT,
    CHECK ((origin_provider_id IS NULL) = (origin_provider_revision IS NULL))
);
CREATE UNIQUE INDEX api_tokens_token_hash_key ON api_tokens (token_hash);
CREATE INDEX api_tokens_user_id_idx ON api_tokens (user_id);
CREATE INDEX api_tokens_origin_provider_idx
    ON api_tokens (origin_provider_id) WHERE origin_provider_id IS NOT NULL;

CREATE TABLE api_token_workspaces (
    org_id TEXT NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    token_id TEXT NOT NULL REFERENCES api_tokens (id) ON DELETE CASCADE,
    workspace_id TEXT NOT NULL,
    PRIMARY KEY (token_id, workspace_id),
    FOREIGN KEY (workspace_id, org_id) REFERENCES workspaces (id, org_id) ON DELETE CASCADE
);
CREATE INDEX api_token_workspaces_workspace_idx ON api_token_workspaces (org_id, workspace_id);

ALTER TABLE organizations ADD COLUMN allow_api_tokens BOOLEAN NOT NULL DEFAULT FALSE;

-- `audit_log` takes the actions of tokens (`api_tokens_enabled`, `api_tokens_disabled`,
-- `api_token_revoked`): the table is made again with its checks, as in 0078.
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
        'container_copied_out', 'copy_out_disabled', 'copy_out_enabled',
        'api_tokens_enabled', 'api_tokens_disabled', 'api_token_revoked',
        'oidc_provider_configured', 'oidc_provider_removed', 'sso_required', 'sso_released',
        'oidc_provider_pending_saved', 'oidc_provider_pending_applied',
        'oidc_provider_pending_discarded', 'password_reset_link_made',
        'password_set_by_operator',
        'oidc_former_provider_kept', 'oidc_former_provider_ended',
        'oidc_former_provider_lapsed', 'oidc_identity_bound',
        'oidc_identity_carried_over', 'email_verified_by_operator')),
    target_id TEXT NOT NULL,
    subject_id TEXT,
    created_at BIGINT NOT NULL,
    detail TEXT,
    identity TEXT,
    CHECK ((actor_kind = 'person' AND actor_id IS NOT NULL)
        OR (actor_kind IN ('server_operator', 'server') AND actor_id IS NULL)),
    CHECK (workspace_id IS NOT NULL OR action IN (
        'org_admin_granted', 'org_admin_removed', 'org_member_removed',
        'workspace_created', 'workspace_renamed', 'workspace_deleted', 'workspace_restored',
        'public_links_disabled', 'public_links_enabled',
        'container_copied_out', 'copy_out_disabled', 'copy_out_enabled',
        'api_tokens_enabled', 'api_tokens_disabled', 'api_token_revoked',
        'oidc_provider_configured', 'oidc_provider_removed', 'sso_required', 'sso_released',
        'oidc_provider_pending_saved', 'oidc_provider_pending_applied',
        'oidc_provider_pending_discarded', 'password_reset_link_made',
        'password_set_by_operator',
        'oidc_former_provider_kept', 'oidc_former_provider_ended',
        'oidc_former_provider_lapsed', 'oidc_identity_bound',
        'oidc_identity_carried_over', 'email_verified_by_operator'))
);
INSERT INTO audit_log_new (id, org_id, workspace_id, actor_id, actor_kind, action, target_id,
    subject_id, created_at, detail, identity)
SELECT id, org_id, workspace_id, actor_id, actor_kind, action, target_id,
    subject_id, created_at, detail, identity
FROM audit_log;
DROP TABLE audit_log;
ALTER TABLE audit_log_new RENAME TO audit_log;
CREATE INDEX audit_log_workspace_id_idx ON audit_log (workspace_id, created_at);
CREATE INDEX audit_log_org_idx ON audit_log (org_id, created_at) WHERE workspace_id IS NULL;
CREATE INDEX audit_log_actor_identity_idx
    ON audit_log (actor_id) WHERE identity IS NOT NULL;
CREATE INDEX audit_log_subject_identity_idx
    ON audit_log (subject_id) WHERE identity IS NOT NULL;

CREATE TRIGGER audit_log_unchanged BEFORE UPDATE ON audit_log
WHEN NOT (
    NEW.identity IS NULL
    AND NEW.id IS OLD.id AND NEW.org_id IS OLD.org_id
    AND NEW.workspace_id IS OLD.workspace_id AND NEW.actor_id IS OLD.actor_id
    AND NEW.actor_kind IS OLD.actor_kind AND NEW.action IS OLD.action
    AND NEW.target_id IS OLD.target_id AND NEW.subject_id IS OLD.subject_id
    AND NEW.created_at IS OLD.created_at AND NEW.detail IS OLD.detail
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

ALTER TABLE body_updates ADD COLUMN actor_token_id TEXT;

ALTER TABLE body_docs ADD COLUMN agent_token_id TEXT;
ALTER TABLE body_docs ADD COLUMN agent_token_name TEXT;

ALTER TABLE notification_outbox ADD COLUMN actor_token_name TEXT;
ALTER TABLE notifications ADD COLUMN actor_token_name TEXT;

-- `cause` takes 'agent' too: the column's check is the table's, made again.
CREATE TABLE body_versions_new (
    block_id TEXT NOT NULL,
    version BIGINT NOT NULL,
    body TEXT,
    body_digest BLOB NOT NULL,
    files TEXT NOT NULL,
    kept_at BIGINT NOT NULL,
    body_bytes INTEGER NOT NULL DEFAULT 0 CHECK (body_bytes >= 0),
    cause TEXT NOT NULL DEFAULT 'auto'
        CHECK (cause IN ('auto', 'manual', 'restored', 'agent')),
    actor_token_id TEXT,
    actor_token_name TEXT,
    PRIMARY KEY (block_id, version)
);
INSERT INTO body_versions_new (block_id, version, body, body_digest, files, kept_at,
    body_bytes, cause)
SELECT block_id, version, body, body_digest, files, kept_at, body_bytes, cause
FROM body_versions;
DROP TABLE body_versions;
ALTER TABLE body_versions_new RENAME TO body_versions;
