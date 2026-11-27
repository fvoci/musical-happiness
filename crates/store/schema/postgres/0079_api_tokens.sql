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
    -- The identity that session was made for, as it names it (its subject is NULL for
    -- a session made before subjects were kept): unlinking the identity ends the token
    -- as it ends the identity's sessions.
    origin_issuer TEXT,
    origin_subject TEXT,
    last_used_at BIGINT,
    last_used_address TEXT,
    created_at BIGINT NOT NULL,
    expires_at BIGINT NOT NULL CHECK (expires_at > created_at),
    revoked_at BIGINT,
    CHECK ((origin_provider_id IS NULL) = (origin_provider_revision IS NULL)),
    CHECK ((origin_provider_id IS NULL) = (origin_issuer IS NULL))
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

-- The audit log records the switch above, and an administrator taking a member's token
-- out of the organization.
ALTER TABLE audit_log DROP CONSTRAINT audit_log_action_check;
ALTER TABLE audit_log ADD CONSTRAINT audit_log_action_check CHECK (action IN (
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
    'oidc_former_provider_lapsed', 'oidc_identity_bound', 'oidc_identity_carried_over',
    'email_verified_by_operator'));

ALTER TABLE audit_log DROP CONSTRAINT audit_log_place_check;
ALTER TABLE audit_log ADD CONSTRAINT audit_log_place_check CHECK (
    workspace_id IS NOT NULL OR action IN (
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
        'oidc_identity_carried_over', 'email_verified_by_operator'));

-- The token an update of a document was sent through, if any: what it typed is the
-- agent's, told and versioned as such when the body is written later.
ALTER TABLE body_updates ADD COLUMN actor_token_id UUID;

-- The agent that wrote in a body last since its newest version was kept, by its token
-- and the token's name then: the next version kept of what its editors typed is that
-- agent's, as the one kept as soon as it wrote would have been.
ALTER TABLE body_docs ADD COLUMN agent_token_id UUID;
ALTER TABLE body_docs ADD COLUMN agent_token_name TEXT;

-- The name of the token through which an agent did what a row tells, where one did:
-- it is told as the agent's doing, and to the token's owner as well where they asked.
ALTER TABLE notification_outbox ADD COLUMN actor_token_name TEXT;
ALTER TABLE notifications ADD COLUMN actor_token_name TEXT;

-- A version kept just before an agent first wrote a body ('agent'), and the token, by
-- its name then, of a version an agent's writing made.
ALTER TABLE body_versions ADD COLUMN actor_token_id UUID;
ALTER TABLE body_versions ADD COLUMN actor_token_name TEXT;
ALTER TABLE body_versions DROP CONSTRAINT body_versions_cause_check;
ALTER TABLE body_versions ADD CONSTRAINT body_versions_cause_check
    CHECK (cause IN ('auto', 'manual', 'restored', 'agent'));
