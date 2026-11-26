-- Personal API tokens: a user's credential for an agent, which opens the content of the
-- workspaces it names, read or written, and nothing that is the account's or an
-- administrator's.
--
-- `api_tokens` is no organization's (as `sessions` is not): a request is read for its
-- token before it is known which organization it is for. The digest of the token is
-- stored, never the token itself; `token_prefix` is what a list shows of it. Which
-- workspaces a token opens is in `api_token_workspaces`, behind the wall of the
-- workspace's organization: it is asked once the workspace is entered, and leaving the
-- workspace, being made its guest, or its going to the trash takes the row away.
SELECT set_config('fvoci.system', 'on', true);

CREATE TABLE api_tokens (
    id UUID NOT NULL PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (char_length(name) BETWEEN 1 AND 80),
    token_prefix TEXT NOT NULL,
    -- The SHA-256 digest of the token, in hexadecimal.
    token_hash TEXT NOT NULL,
    scope TEXT NOT NULL CHECK (scope IN ('read', 'write')),
    allow_personal_space BOOLEAN NOT NULL DEFAULT FALSE,
    notify_self BOOLEAN NOT NULL DEFAULT TRUE,
    -- The provider the session that made the token was made through, and its revision
    -- then: a change of the provider ends the token as it ends that session.
    origin_provider_id UUID,
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
    org_id UUID NOT NULL DEFAULT (NULLIF(current_setting('fvoci.org', true), ''))::uuid
        REFERENCES organizations (id) ON DELETE CASCADE,
    token_id UUID NOT NULL REFERENCES api_tokens (id) ON DELETE CASCADE,
    workspace_id UUID NOT NULL,
    PRIMARY KEY (token_id, workspace_id),
    FOREIGN KEY (workspace_id, org_id) REFERENCES workspaces (id, org_id) ON DELETE CASCADE
);
CREATE INDEX api_token_workspaces_workspace_idx ON api_token_workspaces (org_id, workspace_id);

ALTER TABLE api_token_workspaces ENABLE ROW LEVEL SECURITY;
ALTER TABLE api_token_workspaces FORCE ROW LEVEL SECURITY;
CREATE POLICY org_wall ON api_token_workspaces USING (
    org_id = (SELECT NULLIF(current_setting('fvoci.org', true), '')::uuid)
    OR (SELECT current_setting('fvoci.system', true)) = 'on');

-- Whether an organization that takes only sessions made through its provider lets its
-- members give its workspaces to a token. An organization that takes any session lets
-- them whatever this says.
ALTER TABLE organizations ADD COLUMN allow_api_tokens BOOLEAN NOT NULL DEFAULT FALSE;

-- The token a record of the audit log was done through, if any.
ALTER TABLE audit_log ADD COLUMN actor_token_id UUID;

CREATE OR REPLACE FUNCTION audit_log_is_kept() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' AND (
           NOT EXISTS (SELECT 1 FROM organizations WHERE id = OLD.org_id)
           OR (OLD.workspace_id IS NOT NULL
               AND NOT EXISTS (SELECT 1 FROM workspaces WHERE id = OLD.workspace_id))) THEN
        RETURN OLD;
    END IF;
    IF TG_OP = 'UPDATE'
       AND NEW.identity IS NULL
       AND (NEW.id, NEW.org_id, NEW.workspace_id, NEW.actor_id, NEW.actor_kind, NEW.action,
            NEW.target_id, NEW.subject_id, NEW.created_at, NEW.detail, NEW.actor_token_id)
           IS NOT DISTINCT FROM
           (OLD.id, OLD.org_id, OLD.workspace_id, OLD.actor_id, OLD.actor_kind, OLD.action,
            OLD.target_id, OLD.subject_id, OLD.created_at, OLD.detail, OLD.actor_token_id)
       AND EXISTS (SELECT 1 FROM users
                   WHERE id IN (OLD.actor_id, OLD.subject_id) AND erase_started_at > 0) THEN
        RETURN NEW;
    END IF;
    RAISE EXCEPTION 'a record of the audit log is not changed or removed'
        USING ERRCODE = 'restrict_violation';
END $$;

-- The token an update of a document was sent through, if any: what it typed is the
-- agent's, told and versioned as such when the body is written later.
ALTER TABLE body_updates ADD COLUMN actor_token_id UUID;

-- A version kept just before an agent first wrote a body ('agent'), and the token, by
-- its name then, of a version an agent's writing made.
ALTER TABLE body_versions ADD COLUMN actor_token_id UUID;
ALTER TABLE body_versions ADD COLUMN actor_token_name TEXT;
ALTER TABLE body_versions DROP CONSTRAINT body_versions_cause_check;
ALTER TABLE body_versions ADD CONSTRAINT body_versions_cause_check
    CHECK (cause IN ('auto', 'manual', 'restored', 'agent'));
