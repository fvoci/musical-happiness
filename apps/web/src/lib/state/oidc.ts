/** An organization's single sign-on settings. Secrets are only ever sent. */
export interface OidcProvider {
  issuer: string;
  clientId: string;
  hasClientSecret: boolean;
  autoJoin: boolean;
  enabled: boolean;
  ssoRequired: boolean;
  /** The current session was made through this enabled provider. */
  canRequireSso: boolean;
  redirectUri: string;
  createAt: number;
  updateAt: number;
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
