/** Signing in and out and making an account, where a backend has accounts (part of `backend.ts`). */

/** What an account is made with. */
export interface Registration {
  /** The name to sign in with. */
  username: string;
  email: string;
  password: string;
  /** The name to be shown by; empty or left out for none. */
  displayName?: string;
  /** The workspace's invitation; the first account of a server needs none. */
  token?: string;
}

/** What the login page may know of an organization before anybody signs in. */
export interface LoginOrganization {
  id: string;
  /** Empty if it was given none. */
  name: string;
  /** Whether people sign in through its provider. */
  oidc: boolean;
}

/** Who the reader is at a single sign-on they connected to the account, or first signed in through. */
export interface LinkedIdentity {
  /** The provider, as it names itself: an address. */
  issuer: string;
  /** Who the reader is there. */
  subject: string;
  /** The email address the provider gave when it was connected; empty for none. */
  email: string;
  createAt: number;
}

/** An organization of the reader's that signs in through a provider, as the settings name it. */
export interface SignInOrganization {
  orgId: string;
  name: string;
}

/**
 * Signing in and out, where a backend has accounts: the server has, the one
 * in memory has not. Until somebody is signed in, every read of a backend
 * with accounts is refused with 401.
 */
export interface Account {
  /** `login` is the name to sign in with, or the email address. */
  signIn(login: string, password: string): Promise<void>;
  /** Makes an account. It does not sign in. */
  register(registration: Registration): Promise<void>;
  signOut(): Promise<void>;
  /**
   * Changes the reader's password; `oldPassword` proves they know it. Their
   * other sessions end, this one goes on. A wrong old password is 400 with
   * the reason `invalid_credentials`, too many tries 429.
   */
  changePassword(oldPassword: string, newPassword: string): Promise<void>;
  /**
   * The organization the login page is for: the one `orgId` names, or the
   * only one of a server that holds one; undefined for none.
   */
  loginOrganization(orgId: string | undefined): Promise<LoginOrganization | undefined>;
  /**
   * Starts a sign-in through the organization's provider for this browser:
   * the provider's address the browser is then sent to. With `linkPassword`,
   * the reader's password, it connects instead: the sign-in comes back to
   * tie who the provider says they are to the account they are signed in to.
   */
  startOidc(orgId: string, linkPassword?: string): Promise<string>;
  /** The sign-ins connected to the reader's account, oldest first. */
  identities(): Promise<LinkedIdentity[]>;
  /**
   * Disconnects one; `password` proves the reader knows the account's. An
   * account without a password keeps its last (409, `last_sign_in`).
   */
  unlinkIdentity(identity: LinkedIdentity, password: string): Promise<void>;
}
