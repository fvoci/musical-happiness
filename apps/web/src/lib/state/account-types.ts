/** Signing in and out and making an account, where a backend has accounts (part of `backend.ts`). */

/** What an account is made with. */
export interface NewAccount {
  /** The name to sign in with. */
  username: string;
  email: string;
  password: string;
  /** The name to be shown by; empty or left out for none. */
  displayName?: string;
}

/** An account that starts an organization: whoever signs up without an invitation. */
export interface Registration extends NewAccount {
  /** The name of the organization the account starts, and of its first workspace. */
  organizationName: string;
}

/** What the link of an invitation shows of where it leads, to whoever holds it. */
export interface InvitationPreview {
  /** The name of the workspace the link leads into; empty if it was given none. */
  workspaceName: string;
  /** Who made the link: the name they are shown by, or the one they sign in with. */
  inviterName: string;
  /** For a guest: the title of the board they are invited to. */
  boardTitle?: string;
}

/** Where an accepted invitation let somebody in. */
export interface AcceptedInvitation {
  workspaceId: string;
  /** For a guest: the board they were invited to. */
  boardId?: string;
}

/** What the login page may know of the server before anybody signs in. */
export interface LoginOptions {
  /** The organization the page is for, if it is for one. */
  organization?: LoginOrganization;
  /**
   * Whether somebody without an invitation may make an account and start an
   * organization. Where they may not, the link of an invitation is the way in.
   */
  registrationOpen: boolean;
  /** Whether the server can deliver recovery and invitation mail. */
  mailEnabled?: boolean;
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
  /** This session was made through a provider of this issuer: disconnecting it signs this session out. */
  madeThisSession: boolean;
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
  /** Makes an account and the organization it starts. It does not sign in. */
  register(registration: Registration): Promise<void>;
  /**
   * What the link of an invitation shows. A link that lets nobody in (there is
   * none, it ended, it was used up or revoked) is refused with 404 and the
   * reason `invitation_unavailable`, whatever is wrong with it.
   */
  previewInvitation(token: string): Promise<InvitationPreview>;
  /**
   * Lets somebody in by the link of an invitation: whoever is signed in, or
   * the `account` it makes for somebody who is not. It does not sign in.
   */
  acceptInvitation(token: string, account?: NewAccount): Promise<AcceptedInvitation>;
  signOut(): Promise<void>;
  /**
   * Changes the reader's password; `oldPassword` proves they know it. Their
   * other sessions end, this one goes on. A wrong old password is 400 with
   * the reason `invalid_credentials`, too many tries 429.
   */
  changePassword(oldPassword: string, newPassword: string): Promise<void>;
  /**
   * What the login page shows: the organization it is for (the one `orgId`
   * names, or the only one of a server that holds one), and whether an
   * account is made without an invitation.
   */
  loginOptions(orgId: string | undefined): Promise<LoginOptions>;
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
