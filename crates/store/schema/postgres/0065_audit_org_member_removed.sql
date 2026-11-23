-- The record of an organization administrator removing a member from the organization
-- (`org_member_removed`): of the organization, of no workspace.
ALTER TABLE audit_log DROP CONSTRAINT audit_log_action_check;
ALTER TABLE audit_log ADD CONSTRAINT audit_log_action_check CHECK (action IN (
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
    'oidc_former_provider_lapsed', 'oidc_identity_bound', 'oidc_identity_carried_over',
    'email_verified_by_operator'));
ALTER TABLE audit_log DROP CONSTRAINT audit_log_place_check;
ALTER TABLE audit_log ADD CONSTRAINT audit_log_place_check CHECK (
    workspace_id IS NOT NULL OR action IN (
        'org_admin_granted', 'org_admin_removed', 'org_member_removed',
        'workspace_created', 'workspace_renamed', 'workspace_deleted', 'workspace_restored',
        'public_links_disabled', 'public_links_enabled',
        'oidc_provider_configured', 'oidc_provider_removed', 'sso_required', 'sso_released',
        'oidc_provider_pending_saved', 'oidc_provider_pending_applied',
        'oidc_provider_pending_discarded', 'password_reset_link_made',
        'password_set_by_operator',
        'oidc_former_provider_kept', 'oidc_former_provider_ended',
        'oidc_former_provider_lapsed', 'oidc_identity_bound',
        'oidc_identity_carried_over', 'email_verified_by_operator'));
