-- Moving an organization from one provider to another.
--
-- `oidc_former_providers`: the provider an organization signed in through before its
-- issuer changed, kept for a while (`expires_at`) so that its people can show who
-- they were there and carry their accounts over to the provider that took its place.
-- Nobody signs in through it: a sign-in through it ends only in an identity of the
-- new provider tied to the account it vouches for. One for an organization, the one
-- last moved away from; `client_secret` is sealed as the provider's was, for its own
-- issuer and client. Past `expires_at` it is as if it were not there, and is removed.
-- The line that follows is there because the check of later migrations sees the words
-- of the policies (crates/store/src/schema.rs).
SELECT set_config('fvoci.system', 'on', true);

CREATE TABLE oidc_former_providers (
    org_id UUID NOT NULL PRIMARY KEY
        DEFAULT (NULLIF(current_setting('fvoci.org', true), ''))::uuid
        REFERENCES organizations (id) ON DELETE CASCADE,
    issuer TEXT NOT NULL,
    client_id TEXT NOT NULL,
    client_secret TEXT NOT NULL,
    create_at BIGINT NOT NULL,
    expires_at BIGINT NOT NULL
);
CREATE INDEX oidc_former_providers_expires_at_idx ON oidc_former_providers (expires_at);
ALTER TABLE oidc_former_providers ENABLE ROW LEVEL SECURITY;
ALTER TABLE oidc_former_providers FORCE ROW LEVEL SECURITY;
CREATE POLICY org_wall ON oidc_former_providers USING (
    org_id = (SELECT NULLIF(current_setting('fvoci.org', true), '')::uuid)
    OR (SELECT current_setting('fvoci.system', true)) = 'on');

-- The records of a move: the former provider kept, ended early by an administrator,
-- or lapsed by itself; an identity of the new provider tied to an administrator's
-- account by the trial that applied it, or carried over by a member. `detail` is the
-- issuer such a record names; NULL for the others. What lapses is ended by the
-- server itself, on its timer: an actor of its own kind, with no account, as the
-- operator has none.
ALTER TABLE audit_log ADD COLUMN detail TEXT;
ALTER TABLE audit_log DROP CONSTRAINT audit_log_actor_kind_check;
ALTER TABLE audit_log ADD CONSTRAINT audit_log_actor_kind_check
    CHECK (actor_kind IN ('person', 'server_operator', 'server'));
ALTER TABLE audit_log DROP CONSTRAINT audit_log_actor_check;
ALTER TABLE audit_log ADD CONSTRAINT audit_log_actor_check CHECK (
    (actor_kind = 'person' AND actor_id IS NOT NULL)
    OR (actor_kind IN ('server_operator', 'server') AND actor_id IS NULL));
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
    'password_set_by_operator',
    'oidc_former_provider_kept', 'oidc_former_provider_ended',
    'oidc_former_provider_lapsed', 'oidc_identity_bound', 'oidc_identity_carried_over'));
ALTER TABLE audit_log DROP CONSTRAINT audit_log_place_check;
ALTER TABLE audit_log ADD CONSTRAINT audit_log_place_check CHECK (
    workspace_id IS NOT NULL OR action IN (
        'org_admin_granted', 'org_admin_removed', 'workspace_created', 'workspace_renamed',
        'workspace_deleted', 'workspace_restored', 'public_links_disabled',
        'public_links_enabled', 'oidc_provider_configured', 'oidc_provider_removed',
        'sso_required', 'sso_released', 'oidc_provider_pending_saved',
        'oidc_provider_pending_applied', 'oidc_provider_pending_discarded',
        'password_reset_link_made', 'password_set_by_operator',
        'oidc_former_provider_kept', 'oidc_former_provider_ended',
        'oidc_former_provider_lapsed', 'oidc_identity_bound',
        'oidc_identity_carried_over'));
