/** An organization's single sign-on settings. Secrets are only ever sent. */
export interface OidcProvider {
  issuer: string;
  clientId: string;
  hasClientSecret: boolean;
  autoJoin: boolean;
  enabled: boolean;
  ssoRequired: boolean;
  /** The current session was made through this enabled provider, as it is saved now. */
  canRequireSso: boolean;
  redirectUri: string;
  createAt: number;
  updateAt: number;
  /** Settings saved aside while single sign-on is required, until a sign-in through them. */
  pending: OidcPendingProvider | null;
}

/** Pending sign-in settings: nobody signs in through them until a trial succeeds. */
export interface OidcPendingProvider {
  issuer: string;
  clientId: string;
  hasClientSecret: boolean;
  createAt: number;
}

export interface OidcSettings {
  issuer: string;
  clientId: string;
  /** Omitted to keep the stored secret; empty to remove it. */
  clientSecret?: string;
  autoJoin: boolean;
  enabled: boolean;
  ssoRequired: boolean;
}
