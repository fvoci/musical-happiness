-- An operator has no account; existing records remain attributed to people.
SELECT set_config('fvoci.system', 'on', true);
ALTER TABLE audit_log ALTER COLUMN actor_id DROP NOT NULL;
ALTER TABLE audit_log ADD COLUMN actor_kind TEXT NOT NULL DEFAULT 'person'
    CHECK (actor_kind IN ('person', 'server_operator'));
ALTER TABLE audit_log ADD CONSTRAINT audit_log_actor_check CHECK (
    (actor_kind = 'person' AND actor_id IS NOT NULL)
    OR (actor_kind = 'server_operator' AND actor_id IS NULL));
ALTER TABLE audit_log DROP CONSTRAINT audit_log_action_check;
ALTER TABLE audit_log ADD CONSTRAINT audit_log_action_check CHECK (action IN (
    'self_added_to_board', 'added_to_board',
    'org_admin_joined', 'org_admin_left', 'org_admin_revoked',
    'org_admin_granted', 'org_admin_removed',
    'workspace_created', 'workspace_renamed', 'workspace_deleted', 'workspace_restored',
    'public_links_disabled', 'public_links_enabled',
    'oidc_provider_configured', 'oidc_provider_removed', 'sso_required', 'sso_released'));
ALTER TABLE audit_log DROP CONSTRAINT audit_log_place_check;
ALTER TABLE audit_log ADD CONSTRAINT audit_log_place_check CHECK (
    workspace_id IS NOT NULL OR action IN (
        'org_admin_granted', 'org_admin_removed', 'workspace_created', 'workspace_renamed',
        'workspace_deleted', 'workspace_restored', 'public_links_disabled',
        'public_links_enabled', 'oidc_provider_configured', 'oidc_provider_removed',
        'sso_required', 'sso_released'));
-- The existing immutable-record trigger and organization policy remain in force.
