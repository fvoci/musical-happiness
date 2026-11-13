-- Its policies and checks name the rows of organizations' tables: reached as the
-- migrator's transaction names no organization.
SELECT set_config('fvoci.system', 'on', true);

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
ALTER TABLE sessions ADD CONSTRAINT sessions_login_subject_check
    CHECK (login_subject IS NULL OR login_issuer IS NOT NULL);
ALTER TABLE sessions ADD CONSTRAINT sessions_login_revision_check
    CHECK (login_revision IS NULL OR login_issuer IS NOT NULL);

-- What an administrator saved of the provider's sign-in while the organization takes
-- no password: kept aside, and not signed in through, until a sign-in through it ends
-- well (a trial, `oidc_login_flows.pending_at`), which makes it the provider's; or it is
-- thrown away. One for an organization, of its provider, gone with it. `client_secret`
-- is sealed as the provider's is, for its own issuer and client; `create_at` names the
-- state saved, and is later than that of the one before.
CREATE TABLE oidc_pending_providers (
    org_id UUID NOT NULL PRIMARY KEY
        DEFAULT (NULLIF(current_setting('fvoci.org', true), ''))::uuid
        REFERENCES organizations (id) ON DELETE CASCADE,
    provider_id UUID NOT NULL,
    issuer TEXT NOT NULL,
    client_id TEXT NOT NULL,
    client_secret TEXT NOT NULL,
    create_at BIGINT NOT NULL,
    FOREIGN KEY (provider_id, org_id) REFERENCES oidc_providers (id, org_id) ON DELETE CASCADE
);
ALTER TABLE oidc_pending_providers ENABLE ROW LEVEL SECURITY;
ALTER TABLE oidc_pending_providers FORCE ROW LEVEL SECURITY;
CREATE POLICY org_wall ON oidc_pending_providers USING (
    org_id = (SELECT NULLIF(current_setting('fvoci.org', true), '')::uuid)
    OR (SELECT current_setting('fvoci.system', true)) = 'on');

-- A sign-in under way that tries the pending settings saved at this time, in place of
-- the provider's, and the administrator who started it, whose account alone it may end
-- in; NULL for a sign-in through the provider as it is.
ALTER TABLE oidc_login_flows ADD COLUMN pending_at BIGINT;
ALTER TABLE oidc_login_flows ADD COLUMN pending_by UUID;

-- The records of the pending settings, and of the server operator setting a member's
-- password or making a link to set it: each of the organization, of no workspace.
SELECT set_config('fvoci.system', 'on', true);
ALTER TABLE audit_log DROP CONSTRAINT audit_log_action_check;
ALTER TABLE audit_log ADD CONSTRAINT audit_log_action_check CHECK (action IN (
    'self_added_to_board', 'added_to_board',
    'org_admin_joined', 'org_admin_left', 'org_admin_revoked',
    'org_admin_granted', 'org_admin_removed',
    'workspace_created', 'workspace_renamed', 'workspace_deleted', 'workspace_restored',
    'public_links_disabled', 'public_links_enabled',
    'oidc_provider_configured', 'oidc_provider_removed', 'sso_required', 'sso_released',
    'oidc_provider_pending_saved', 'oidc_provider_pending_applied',
    'oidc_provider_pending_discarded', 'password_reset_link_made',
    'password_set_by_operator'));
ALTER TABLE audit_log DROP CONSTRAINT audit_log_place_check;
ALTER TABLE audit_log ADD CONSTRAINT audit_log_place_check CHECK (
    workspace_id IS NOT NULL OR action IN (
        'org_admin_granted', 'org_admin_removed', 'workspace_created', 'workspace_renamed',
        'workspace_deleted', 'workspace_restored', 'public_links_disabled',
        'public_links_enabled', 'oidc_provider_configured', 'oidc_provider_removed',
        'sso_required', 'sso_released', 'oidc_provider_pending_saved',
        'oidc_provider_pending_applied', 'oidc_provider_pending_discarded',
        'password_reset_link_made', 'password_set_by_operator'));
