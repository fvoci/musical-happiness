/**
 * The records of an audit log as the server answers them (`AuditRecord` of the
 * contract): who did what, to what and whom, and when.
 */
export const AUDIT_ACTIONS = [
  'self_added_to_board',
  'added_to_board',
  'org_admin_joined',
  'org_admin_left',
  'org_admin_revoked',
  'org_admin_granted',
  'org_admin_removed',
  'workspace_created',
  'workspace_renamed',
  'workspace_deleted',
  'workspace_restored',
  'public_links_disabled',
  'public_links_enabled',
  'oidc_provider_configured',
  'oidc_provider_removed',
  'sso_required',
  'sso_released',
] as const;

export type AuditAction = (typeof AUDIT_ACTIONS)[number];

export interface AuditPerson {
  id: string;
  /** Empty when the account is gone. */
  username: string;
  displayName: string;
}

/** One record: who did what, to what and whom, and when. */
export interface AuditRecord {
  id: string;
  action: AuditAction;
  actor: AuditPerson | null;
  actorKind: 'person' | 'server_operator';
  subject: AuditPerson | null;
  targetId: string;
  /** The board's title where the reader sees the board, the workspace's name; null otherwise. */
  targetName: string | null;
  createAt: number;
}
