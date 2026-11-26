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

ALTER TABLE audit_log ADD COLUMN actor_token_id TEXT;

DROP TRIGGER audit_log_unchanged;
CREATE TRIGGER audit_log_unchanged BEFORE UPDATE ON audit_log
WHEN NOT (
    NEW.identity IS NULL
    AND NEW.id IS OLD.id AND NEW.org_id IS OLD.org_id
    AND NEW.workspace_id IS OLD.workspace_id AND NEW.actor_id IS OLD.actor_id
    AND NEW.actor_kind IS OLD.actor_kind AND NEW.action IS OLD.action
    AND NEW.target_id IS OLD.target_id AND NEW.subject_id IS OLD.subject_id
    AND NEW.created_at IS OLD.created_at AND NEW.detail IS OLD.detail
    AND NEW.actor_token_id IS OLD.actor_token_id
    AND EXISTS (SELECT 1 FROM users
                WHERE id IN (OLD.actor_id, OLD.subject_id) AND erase_started_at > 0))
BEGIN
    SELECT RAISE(ABORT, 'a record of the audit log is not changed or removed');
END;

ALTER TABLE body_updates ADD COLUMN actor_token_id TEXT;

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
