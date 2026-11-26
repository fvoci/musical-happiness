/**
 * The reader's personal API tokens, which they give an agent to work in
 * their workspaces as them: read-only, or reading and writing content. The
 * token itself is told once, when it is made; the list shows its first
 * characters.
 */

/** What a token may do. */
export type ApiTokenScope = 'read' | 'write';

/** A workspace a token reaches. */
export interface ApiTokenWorkspace {
  workspaceId: string;
  orgId: string;
  name: string;
}

/** A token as its list shows it. */
export interface ApiToken {
  id: string;
  userId: string;
  name: string;
  /** `fvoci_pat_` and six more characters: which token a client holds. */
  tokenPrefix: string;
  scope: ApiTokenScope;
  workspaces: ApiTokenWorkspace[];
  allowPersonalSpace: boolean;
  createdAt: number;
  expiresAt: number;
  /** When it was last used, to within five minutes; `undefined` if never. */
  lastUsedAt?: number | undefined;
}

/** What a new token is asked to be. */
export interface NewApiToken {
  name: string;
  scope: ApiTokenScope;
  workspaceIds: string[];
  expiresInDays: number;
  /** Whether it reaches the reader's own space too, which `password` gives. */
  allowPersonalSpace: boolean;
  password?: string | undefined;
}

/** A token as it is made: the only time `token` is told. */
export interface IssuedApiToken {
  token: string;
  apiToken: ApiToken;
}

export interface ApiTokens {
  /** The reader's tokens that still open something, the newest first. */
  list(): Promise<ApiToken[]>;
  /**
   * Makes one. 400 at the member a request gets wrong; 403 `sign_in_again`
   * or 400 `invalid_credentials` for the own space without the password.
   */
  issue(token: NewApiToken): Promise<IssuedApiToken>;
  /** Ends one at once; 404 for one that is gone. */
  revoke(tokenId: string): Promise<void>;
}

/** The token an administrator of an organization sees, of its members. */
export interface OrgApiTokens {
  list(orgId: string): Promise<ApiToken[]>;
  revoke(orgId: string, tokenId: string): Promise<void>;
}
